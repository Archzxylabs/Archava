//! The scheduling SQL adapter — the Archxy 2 port of
//! `packages/scheduling-pg/src/repository.ts` (the R05 first-cohort mutation
//! and status surface), executed strictly inside the shared RLS transaction
//! cohort.
//!
//! ## Statement order (the TS port)
//!
//! `mutate()`'s ladder is the R04 cohorts' ladder with scheduling's own
//! vocabulary: the membership row (FOR SHARE through `lock_membership`,
//! resolved by the host's context step), the human/service on-behalf
//! intersection, the write permission the operation implies, then
//! `mutate`'s read-floor grants (`resources.read` always,
//! `reservations.read` beside any reservation arm, and
//! `reservation.block` additionally carrying `resources.write`), the
//! `commands` claim, and then the arm's effect. Every bookkeeping write
//! after the effect — the audit row (one per command; `(tenant_id,
//! request_id)` is the table's PRIMARY KEY), the receipt binding (the
//! `scheduling_command_immutable` trigger refuses a second binding and any
//! other change) — happens in the same transaction before the host's commit.
//!
//! ## The resource-first discipline
//!
//! All writes serialize on the RESOURCE row: every non-create arm takes the
//! resource `FOR UPDATE` and samples `clock_timestamp()` only after that
//! lock (the migration's own comment explains why: transaction start time is
//! unsafe for expiring holds). Reads used by recovery re-run the same
//! scope-or-owner predicate the TS `ownedResource`/`ownedReservation` embed
//! — a scoped access cannot recover a receipt that no longer belongs to it.
//!
//! ## Error vocabulary
//!
//! The TS `SchedulingError('forbidden'|'not_found'|'conflict'|'unavailable')`
//! rides this crate's [`PurchaseError`]/[`ErpError`] carrier. The catch
//! whitelist maps the scheduling-specific SQL states to conflict:
//! `23P01` (the `guard_reservation` overlap verdict — the class SchedulingError
//! never names in text but reports as `conflict`), `23514`, `23503`,
//! `22003`; the crate-level mapping keeps the shared extras. Row-grammar
//! mismatches (a stored row one of the TS schema parses would refuse) mark
//! ledger drift rather than caller blame — they answer conflict, never
//! silently pass.
//!
//! ## Deliberately not here
//!
//! The TS repository's read paths (`access`, `resources`,
//! `resource`, `reservations`, `detail`, `availability`) and the occupancy
//! runtime of `occupancy.ts` ride with the cohorts that serve them: the
//! reads belong to no mounted route yet, and the occupancy kernel belongs
//! to the accommodation cohort — porting them now would ship unused surface
//! with no witness. The mutation + commandStatus surface this module ports
//! is exactly what the host routes and the suite drive.

use sqlx::PgConnection;
use uuid::Uuid;

use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::scheduling::{
    parse_instant, replayed_receipt, SchedulingAccess, SchedulingCommand, SchedulingCommandStatus,
    SchedulingEffectiveState, SchedulingMutation, SchedulingReceipt, SchedulingReservation,
    SchedulingReservationKind, SchedulingReservationState, SchedulingResource,
    SchedulingResourceKind, REVISION_MAX, SCHEDULING_PERMISSIONS, SCHEDULING_TIMEZONE_MAX,
    SCHEDULING_TITLE_MAX,
};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::PurchaseError;

/// The ledger label on database errors.
pub const SCHEDULING_LEDGER: &str = "scheduling";

/// The `archava_scheduling` namespace — checked constants only.
pub const SCHEDULING_NAMESPACE: &str = "archava_scheduling";

// --- Refusal helpers ---------------------------------------------------------

fn conflict() -> PurchaseError {
    PurchaseError::Domain(ErpError::Conflict)
}

fn forbidden() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}

fn not_found() -> PurchaseError {
    PurchaseError::Domain(ErpError::NotFound)
}

/// The scheduling catch whitelist (see the module header): the overlap
/// verdict the migration's guard trigger raises maps to conflict; every
/// other state goes through the crate-level classifier (which reclassifies
/// the shared conflict states and otherwise keeps the Database class).
fn refuses(error: sqlx::Error) -> PurchaseError {
    if let sqlx::Error::Database(database) = &error {
        if database.code().as_deref() == Some("23P01") {
            return conflict();
        }
    }
    super::purchase_database_error(SCHEDULING_LEDGER, error)
}

fn parse_uuid_value(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

/// The sextuple the claim + execution report: `(receipt, reason)` — the
/// receipt for the command row, the audit reason from the arm.
struct ArmOutcome {
    receipt: SchedulingReceipt,
    reason: Option<String>,
}

/// The `clock_timestamp()` sample every scheduling write takes — an RFC-3339
/// UTC render of the same instants the TS `clock()` produces (the driver's
/// JS Date through `instant()`; this SQL render IS the port's form of the
/// same column), always milliseconds and always `Z`.
async fn observed_now(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map(|(text,)| text)
    .map_err(refuses)
}

/// The point-in-time of a canonical render — every value this module
/// compares or adds to came through the domain parse or the `to_char`
/// render above, which are the same canonical family.
fn instant_millis(raw: &str) -> Result<i64, PurchaseError> {
    parse_instant(raw)
        .map(|instant| instant.epoch_millis())
        .ok_or_else(conflict)
}

/// An instant bound parameter (string bind of the canonical render).
fn instant_bind(raw: &str) -> Result<String, PurchaseError> {
    parse_instant(raw)
        .map(|instant| instant.canonical().to_string())
        .ok_or_else(conflict)
}

// --- Row grammar -------------------------------------------------------------

/// The resource row's column reader — `resource = (row) =>
/// schedulingResourceSchema.parse({...})`. Grammar failures are ledger
/// drift (see the module header) and answer conflict.
fn resource_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
) -> Result<SchedulingResource, PurchaseError> {
    use sqlx::Row;
    let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
    if tenant_id != tenant {
        // RLS makes a cross-tenant screen impossible; a mismatch is drift.
        return Err(conflict());
    }
    let title: String = row.try_get("title").map_err(refuses)?;
    let title = archava_domain::erp::trimmed(&title);
    let kind_raw: String = row.try_get("kind").map_err(refuses)?;
    let timezone: String = row.try_get("timezone").map_err(refuses)?;
    let active: bool = row.try_get("active").map_err(refuses)?;
    let capacity: i32 = row.try_get("capacity").map_err(refuses)?;
    let revision: i32 = row.try_get("revision").map_err(refuses)?;
    if title.is_empty() || title.len() > SCHEDULING_TITLE_MAX {
        return Err(conflict());
    }
    let kind = match SchedulingResourceKind::parse(&kind_raw) {
        Some(kind) => kind,
        None => return Err(conflict()),
    };
    if timezone.is_empty() || timezone.len() > SCHEDULING_TIMEZONE_MAX {
        return Err(conflict());
    }
    let revision = i64::from(revision);
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(conflict());
    }
    if i64::from(capacity) != archava_domain::erp::scheduling::SCHEDULING_CAPACITY {
        return Err(conflict());
    }
    Ok(SchedulingResource {
        id: row.try_get::<Uuid, _>("id").map_err(refuses)?.to_string(),
        tenant_id,
        owner_id: row.try_get("owner_id").map_err(refuses)?,
        title: title.to_string(),
        kind,
        timezone,
        active,
        capacity: i64::from(capacity),
        revision,
        created_at: row.try_get("created_at").map_err(refuses)?,
        updated_at: row.try_get("updated_at").map_err(refuses)?,
    })
}

/// The reservation row's column reader — `reservation = (row, observed) =>`
/// the schema parse WITH the effective-state rule (a stale hold is
/// `expired`).
fn reservation_row(
    row: &sqlx::postgres::PgRow,
    tenant: &str,
    observed_millis: i64,
) -> Result<SchedulingReservation, PurchaseError> {
    use sqlx::Row;
    let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
    if tenant_id != tenant {
        return Err(conflict());
    }
    let id: Uuid = row.try_get("id").map_err(refuses)?;
    let resource_id: Uuid = row.try_get("resource_id").map_err(refuses)?;
    let owner_id: String = row.try_get("owner_id").map_err(refuses)?;
    let title_raw: String = row.try_get("title").map_err(refuses)?;
    let title = archava_domain::erp::trimmed(&title_raw);
    let kind_raw: String = row.try_get("kind").map_err(refuses)?;
    let state_raw: String = row.try_get("state").map_err(refuses)?;
    let starts_at: String = row.try_get("starts_at").map_err(refuses)?;
    let ends_at: String = row.try_get("ends_at").map_err(refuses)?;
    let expires_at: Option<String> = row.try_get("expires_at").map_err(refuses)?;
    let revision: i32 = row.try_get("revision").map_err(refuses)?;
    let state = match SchedulingReservationState::parse(&state_raw) {
        Some(state) => state,
        None => return Err(conflict()),
    };
    let kind = match SchedulingReservationKind::parse(&kind_raw) {
        Some(kind) => kind,
        None => return Err(conflict()),
    };
    let revision = i64::from(revision);
    if !(1..=REVISION_MAX).contains(&revision) {
        return Err(conflict());
    }
    // The effective state: a held row whose hold is now the past reads as
    // `expired` — the repository's derived word, never a stored one.
    let expires_millis = expires_at.as_deref().map(instant_millis).transpose()?;
    let effective_state = SchedulingEffectiveState::of(state, expires_millis, observed_millis);
    let item = SchedulingReservation {
        id: id.to_string(),
        tenant_id,
        resource_id: resource_id.to_string(),
        owner_id,
        title: title.to_string(),
        kind,
        state,
        effective_state,
        starts_at,
        ends_at,
        expires_at,
        revision,
        created_at: row.try_get("created_at").map_err(refuses)?,
        updated_at: row.try_get("updated_at").map_err(refuses)?,
    };
    Ok(item)
}

// --- The context ladder ------------------------------------------------------

/// The membership ladder — ONE ledger. Loads the actor's own grant, refuses
/// inactive/unknown/shape-foreign rows, intersects with the human principal
/// a service names, and hands the access envelope back. The TS
/// `member(...)` + service branch of `transaction()`, statement for
/// statement.
pub async fn resolve_scheduling_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<SchedulingAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        SCHEDULING_NAMESPACE,
        SCHEDULING_LEDGER,
        tenant,
        &principal.id,
        SCHEDULING_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    // The TS ladder compares the ROW's kind against the PRINCIPAL's kind
    // directly (`actor.kind !== principal.kind`).
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
        return Err(forbidden());
    }
    Ok(match principal.kind {
        PrincipalKind::Human => SchedulingAccess {
            permissions: own.permissions.clone(),
            owner_id: principal.id.clone(),
            scope: own.scope,
        },
        PrincipalKind::Service => {
            let on_behalf_of = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
            let human: MembershipRow = load_locked_membership(
                connection,
                SCHEDULING_NAMESPACE,
                SCHEDULING_LEDGER,
                tenant,
                on_behalf_of,
                SCHEDULING_PERMISSIONS,
            )
            .await?
            .ok_or_else(forbidden)?;
            if human.kind != ErpMembershipKind::Human || !human.active {
                return Err(forbidden());
            }
            SchedulingAccess {
                permissions: own
                    .permissions
                    .iter()
                    .filter(|held| human.permissions.contains(*held))
                    .cloned()
                    .collect(),
                owner_id: on_behalf_of.to_string(),
                scope: match (own.scope, human.scope) {
                    (ErpScope::Assigned, _) | (_, ErpScope::Assigned) => ErpScope::Assigned,
                    (ErpScope::All, ErpScope::All) => ErpScope::All,
                },
            }
        }
    })
}

// --- The owned-row reads -----------------------------------------------------

enum Lock {
    Share,
    Update,
}

/// `ownedResource` — the managed-resource screen (`managed_by IS NULL`
/// keeps the accommodation-managed rows out of the timed module's view)
/// plus the scope-or-owner predicate, under the named lock. Two static
/// statements (sqlx audits dynamic strings): the TS interpolates the lock
/// word into one function; this port splits them.
const RESOURCE_SELECT_SHARE: &str =
    "SELECT tenant_id,id,owner_id,title,kind,timezone,active,capacity,revision,\
 to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,\
 to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
 FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 AND managed_by IS NULL \
 AND ($3::boolean OR owner_id=$4) FOR SHARE";

const RESOURCE_SELECT_UPDATE: &str =
    "SELECT tenant_id,id,owner_id,title,kind,timezone,active,capacity,revision,\
 to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,\
 to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
 FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 AND managed_by IS NULL \
 AND ($3::boolean OR owner_id=$4) FOR UPDATE";

async fn owned_resource(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SchedulingAccess,
    id: Uuid,
    lock: Lock,
) -> Result<SchedulingResource, PurchaseError> {
    let statement = match lock {
        Lock::Share => RESOURCE_SELECT_SHARE,
        Lock::Update => RESOURCE_SELECT_UPDATE,
    };
    let row = sqlx::query(statement)
        .bind(tenant)
        .bind(id)
        .bind(access.scope == ErpScope::All)
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    resource_row(&row, tenant)
}

/// `ownedReservation` — the scope-or-owner screen, NO row lock: the
/// migration serializes writers on the resource the reservation hangs
/// from, so the read only has to find the row (a missing one is the TS
/// `not_found`).
const RESERVATION_SELECT: &str =
    "SELECT tenant_id,id,resource_id,owner_id,title,kind,state,revision,\
 to_char(starts_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS starts_at,\
 to_char(ends_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS ends_at,\
 to_char(expires_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS expires_at,\
 to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at,\
 to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at \
 FROM archava_scheduling.reservations WHERE tenant_id=$1 AND id=$2 \
 AND ($3::boolean OR owner_id=$4)";

async fn owned_reservation_row(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SchedulingAccess,
    id: Uuid,
) -> Result<sqlx::postgres::PgRow, PurchaseError> {
    sqlx::query(RESERVATION_SELECT)
        .bind(tenant)
        .bind(id)
        .bind(access.scope == ErpScope::All)
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)
}

// --- The claim + replay plumbing ---------------------------------------------

async fn claim_scheduling_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed = sqlx::query(
        "INSERT INTO archava_scheduling.commands(tenant_id,request_id,actor_id,fingerprint) \
         VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(claimed.rows_affected() == 1)
}

async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_scheduling.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The scheduling audit row — `(tenant, request, actor, operation,
/// resource_id, record_id, revision, reason?)`, one per command.
#[allow(clippy::too_many_arguments)]
async fn insert_scheduling_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    resource_id: Uuid,
    record_id: Uuid,
    revision: i64,
    reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_scheduling.audit(tenant_id,request_id,actor_id,operation,resource_id,record_id,revision,reason) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(resource_id)
    .bind(record_id)
    .bind(int4(revision)?)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

fn int4(value: i64) -> Result<i32, PurchaseError> {
    if !(i32::MIN as i64..=i32::MAX as i64).contains(&value) {
        return Err(conflict());
    }
    Ok(value as i32)
}

// --- The mutation pipeline ----------------------------------------------------

/// The mutation pipeline — the port of `mutate()`. The access ladder runs
/// FIRST in the caller's transaction; the human-only kind gate is the
/// route-boundary rule; every refusal rolls the WHOLE transaction back.
/// The host hands over the ALREADY STRICT-PARSED mutation (its boundary
/// parse answers 400), so the fingerprint and effect read one document.
pub async fn record_scheduling_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &SchedulingAccess,
    fingerprint: &str,
    mutation: &SchedulingMutation,
) -> Result<SchedulingReceipt, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    let command = &mutation.command;
    // The permission ladder in the TS order: the operation's own write
    // grant (the transaction() call), then the read floor, then the
    // reservation arm's read companion, then the block arm's extra write.
    if !access.holds(&[command.required_permission()]) {
        return Err(forbidden());
    }
    if !access.holds(&["resources.read"]) {
        return Err(forbidden());
    }
    let reservations_arm = command.operation().starts_with("reservation.");
    if reservations_arm && !access.holds(&["reservations.read"]) {
        return Err(forbidden());
    }
    if command.operation() == "reservation.block" && !access.holds(&["resources.write"]) {
        return Err(forbidden());
    }
    let request_id = parse_uuid_value(&mutation.request_id)?;
    let actor_id = principal.id.as_str();

    let claimed =
        claim_scheduling_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    if !claimed {
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_command_row(connection, tenant, request_id)
                .await?
                .ok_or_else(conflict)?;
        if row_fingerprint != fingerprint || row_actor != actor_id {
            return Err(conflict());
        }
        let receipt_value = row_receipt.ok_or_else(conflict)?;
        let stored = SchedulingReceipt::parse(&receipt_value).map_err(|_| conflict())?;
        // Recovery re-authorization — `receiptAccess`: resources.read (the
        // grant ladder above already proved it) + the resource MUST still
        // belong to the current access, and a reservation receipt must
        // still be readable too.
        let resource = parse_uuid_value(&stored.resource_id)?;
        owned_resource(connection, tenant, access, resource, Lock::Share).await?;
        if stored.kind == "reservation" {
            let id = parse_uuid_value(&stored.id)?;
            owned_reservation_row(connection, tenant, access, id).await?;
        }
        return Ok(replayed_receipt(&stored));
    }

    let outcome = execute_scheduling_arm(connection, tenant, access, command).await?;
    insert_scheduling_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        command.operation(),
        parse_uuid_value(&outcome.receipt.resource_id)?,
        parse_uuid_value(&outcome.receipt.id)?,
        outcome.receipt.revision,
        outcome.reason.as_deref(),
    )
    .await?;
    let receipt_value = serde_json::to_value(&outcome.receipt).map_err(|_| conflict())?;
    sqlx::query(
        "UPDATE archava_scheduling.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(outcome.receipt)
}

// --- execute(): the seven arms ------------------------------------------------

/// The seven executed arms, in the TS branch order. The resource create is
/// unguarded (its own row IS the write); the rest all take the resource
/// FOR UPDATE and stamp `observed` AFTER the lock, then the per-row work.
async fn execute_scheduling_arm(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SchedulingAccess,
    command: &SchedulingCommand,
) -> Result<ArmOutcome, PurchaseError> {
    if command.operation() == "resource.create" {
        let (title, kind, timezone) = match command {
            SchedulingCommand::ResourceCreate {
                title,
                kind,
                timezone,
            } => (title, kind, timezone),
            _ => return Err(conflict()),
        };
        let id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO archava_scheduling.resources(tenant_id,id,owner_id,title,kind,timezone) \
             VALUES($1,$2,$3,$4,$5,$6)",
        )
        .bind(tenant)
        .bind(id)
        .bind(&access.owner_id)
        .bind(title)
        .bind(kind.as_str())
        .bind(timezone)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;
        return Ok(ArmOutcome {
            receipt: SchedulingReceipt {
                kind: "resource".to_string(),
                id: id.to_string(),
                resource_id: id.to_string(),
                revision: 1,
                replayed: false,
            },
            reason: None,
        });
    }

    // Every other arm locks the resource first and samples the clock after.
    let resource_id = parse_uuid_value(match command {
        SchedulingCommand::ResourceSetActive { resource_id, .. }
        | SchedulingCommand::ReservationHold { resource_id, .. }
        | SchedulingCommand::ReservationBlock { resource_id, .. }
        | SchedulingCommand::ReservationConfirm { resource_id, .. }
        | SchedulingCommand::ReservationMove { resource_id, .. }
        | SchedulingCommand::ReservationCancel { resource_id, .. } => resource_id,
        _ => return Err(conflict()),
    })?;
    let record = owned_resource(connection, tenant, access, resource_id, Lock::Update).await?;
    let observed_raw = observed_now(connection).await?;
    let observed = instant_millis(&observed_raw)?;

    match command {
        SchedulingCommand::ResourceSetActive {
            resource_id,
            expected_revision,
            active,
            ..
        } => {
            if record.revision != *expected_revision
                || record.active == *active
                || record.revision >= REVISION_MAX
            {
                return Err(conflict());
            }
            let resource = parse_uuid_value(resource_id)?;
            sqlx::query(
                "UPDATE archava_scheduling.resources \
                 SET active=$3,revision=revision+1,updated_at=clock_timestamp() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(resource)
            .bind(active)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok(ArmOutcome {
                receipt: SchedulingReceipt {
                    kind: "resource".to_string(),
                    id: record.id.clone(),
                    resource_id: record.id.clone(),
                    revision: record.revision + 1,
                    replayed: false,
                },
                reason: command.reason().map(str::to_string),
            })
        }
        SchedulingCommand::ReservationHold { .. } | SchedulingCommand::ReservationBlock { .. } => {
            let (held, title, starts_at, ends_at, hold_seconds) = match command {
                SchedulingCommand::ReservationHold {
                    title,
                    starts_at,
                    ends_at,
                    hold_seconds,
                    ..
                } => (true, title, starts_at, ends_at, *hold_seconds),
                SchedulingCommand::ReservationBlock {
                    title,
                    starts_at,
                    ends_at,
                    ..
                } => (false, title, starts_at, ends_at, 0i64),
                _ => return Err(conflict()),
            };
            let starts = instant_millis(starts_at)?;
            if !record.active || starts <= observed {
                return Err(conflict());
            }
            let id = Uuid::new_v4();
            let record_id = parse_uuid_value(&record.id)?;
            let expires_at: Option<String> = if held {
                Some(archava_domain::erp::scheduling::instant_render(
                    observed + hold_seconds * 1_000,
                ))
            } else {
                None
            };
            sqlx::query(
                "INSERT INTO archava_scheduling.reservations(\
                 tenant_id,id,resource_id,owner_id,title,kind,state,starts_at,ends_at,expires_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz,$9::timestamptz,$10::timestamptz)",
            )
            .bind(tenant)
            .bind(id)
            .bind(record_id)
            .bind(&access.owner_id)
            .bind(title)
            .bind(if held { "booking" } else { "block" })
            .bind(if held { "held" } else { "confirmed" })
            .bind(instant_bind(starts_at)?)
            .bind(instant_bind(ends_at)?)
            .bind(expires_at)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok(ArmOutcome {
                receipt: SchedulingReceipt {
                    kind: "reservation".to_string(),
                    id: id.to_string(),
                    resource_id: record.id.clone(),
                    revision: 1,
                    replayed: false,
                },
                reason: command.reason().map(str::to_string),
            })
        }
        SchedulingCommand::ReservationConfirm {
            id,
            expected_revision,
            ..
        }
        | SchedulingCommand::ReservationMove {
            id,
            expected_revision,
            ..
        }
        | SchedulingCommand::ReservationCancel {
            id,
            expected_revision,
            ..
        } => {
            let row =
                owned_reservation_row(connection, tenant, access, parse_uuid_value(id)?).await?;
            let item = reservation_row(&row, tenant, observed)?;
            if item.resource_id != record.id {
                return Err(not_found());
            }
            if item.revision != *expected_revision
                || item.revision >= REVISION_MAX
                || matches!(
                    item.effective_state,
                    SchedulingEffectiveState::Cancelled | SchedulingEffectiveState::Expired
                )
            {
                return Err(conflict());
            }
            // A block's confirm/cancel/move needs the resource write — the
            // TS gate runs BEFORE the operation's own refusals, so a caller
            // without it sees forbidden even where the arm would conflict.
            if item.kind == SchedulingReservationKind::Block && !access.holds(&["resources.write"])
            {
                return Err(forbidden());
            }
            match command {
                SchedulingCommand::ReservationConfirm { .. } => {
                    let item_start = instant_millis(&item.starts_at)?;
                    if item.kind != SchedulingReservationKind::Booking
                        || item.effective_state != SchedulingEffectiveState::Held
                        || item_start <= observed
                    {
                        return Err(conflict());
                    }
                    sqlx::query(
                        "UPDATE archava_scheduling.reservations \
                         SET state='confirmed',revision=revision+1,updated_at=clock_timestamp() \
                         WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(parse_uuid_value(id)?)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                }
                SchedulingCommand::ReservationCancel { .. } => {
                    sqlx::query(
                        "UPDATE archava_scheduling.reservations \
                         SET state='cancelled',revision=revision+1,updated_at=clock_timestamp() \
                         WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(parse_uuid_value(id)?)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                }
                SchedulingCommand::ReservationMove {
                    starts_at, ends_at, ..
                } => {
                    let item_start = instant_millis(&item.starts_at)?;
                    let command_start = instant_millis(starts_at)?;
                    if !record.active
                        || item_start <= observed
                        || command_start <= observed
                        || (starts_at == &item.starts_at && ends_at == &item.ends_at)
                    {
                        return Err(conflict());
                    }
                    sqlx::query(
                        "UPDATE archava_scheduling.reservations \
                         SET starts_at=$3::timestamptz,ends_at=$4::timestamptz,revision=revision+1,updated_at=clock_timestamp() \
                         WHERE tenant_id=$1 AND id=$2",
                    )
                    .bind(tenant)
                    .bind(parse_uuid_value(id)?)
                    .bind(instant_bind(starts_at)?)
                    .bind(instant_bind(ends_at)?)
                    .execute(&mut *connection)
                    .await
                    .map_err(refuses)?;
                }
                _ => return Err(conflict()),
            }
            Ok(ArmOutcome {
                receipt: SchedulingReceipt {
                    kind: "reservation".to_string(),
                    id: item.id.clone(),
                    resource_id: record.id.clone(),
                    revision: item.revision + 1,
                    replayed: false,
                },
                reason: command.reason().map(str::to_string),
            })
        }
        SchedulingCommand::ResourceCreate { .. } => Err(conflict()),
    }
}

// --- commandStatus -------------------------------------------------------------

/// `commandStatus()` — the discovered receipt replays only while the
/// CURRENT access still authorizes the recorded operation: the status read
/// refuses a caller whose actor id is not the recorded one (the TS
/// `row.actor_id !== principal.id` gate), and the receipt's resource (and
/// reservation) must still be inside the caller's scope.
pub async fn scheduling_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &SchedulingAccess,
    request_id: Uuid,
) -> Result<SchedulingCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    let Some((row_actor, _, row_receipt)) =
        fetch_command_row(connection, tenant, request_id).await?
    else {
        return Ok(SchedulingCommandStatus::NotCommitted {
            request_id: request_id.to_string(),
        });
    };
    if row_actor != principal.id {
        return Err(forbidden());
    }
    let receipt_value = row_receipt.ok_or_else(conflict)?;
    let stored = SchedulingReceipt::parse(&receipt_value).map_err(|_| conflict())?;
    let resource = parse_uuid_value(&stored.resource_id)?;
    owned_resource(connection, tenant, access, resource, Lock::Share).await?;
    if stored.kind == "reservation" {
        let id = parse_uuid_value(&stored.id)?;
        owned_reservation_row(connection, tenant, access, id).await?;
    }
    Ok(SchedulingCommandStatus::Committed {
        request_id: request_id.to_string(),
        receipt: stored,
    })
}
