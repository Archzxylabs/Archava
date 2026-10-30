//! The assets SQL adapter — the Archxy 2 port of
//! `packages/assets-pg/src/repository.ts` (`mutate()` + `commandStatus()`,
//! the R05 third-cohort surface).
//!
//! ## Statement order (the TS port)
//!
//! The `mutate()` ladder is the ERP cohorts' ladder with the assets
//! vocabulary: the membership row (FOR SHARE through
//! `archava_assets.lock_membership`, resolved by the host's context step),
//! the human/service on-behalf intersection, the operation's own write
//! permission (`required waggingAssetPermission`: create/update/retire →
//! `assets.write`, handover → `assets.handover`, record maintenance →
//! `assets.maintenance`), then `mutate`'s read grant — TS has no extra
//! read-floor on the mutate path beyond the arm's own record lock — the
//! `commands` claim, then the arm's effect. Effect + audit row + receipt
//! binding all share ONE transaction the host commits exactly once.
//!
//! The arms lock their own asset row first (`SELECT * ... FOR UPDATE`);
//! there is no second ledger to coordinate — the custody/maintenance
//! history and the audit's own immutability triggers are the ledger's
//! defenses, and every SQLSTATE the TS catch whitelist names
//! (`23514`, `23503`, `23505`, `22003`, `40P01`, `55P01`) already rides
//! the crate-level classifier, so no ledger-specific arm exists here.
//!
//! ## The sampled clock
//!
//! One `clock_timestamp()` sample per command (the TS `now` SELECT),
//! canonical-rendered and bound into every stamping position of the
//! arm's statements — created/updated/recorded/occurred stamp the SAME
//! instant, exactly as the TS binds its single `now`.
//!
//! ## The refusal net
//!
//! `conflict` folds the TS 409 class (`already_checked_out`,
//! `not_in_custody`, `archived`, stale revisions, idempotency reuse);
//! the TS 400 class (`invalid_condition`, the transaction-time
//! `invalid_request` behind a business-owned `customer_return`) rides the
//! [`ErpError::Unavailable`] arm to THIS cohort's gate, which answers the
//! baseline's 400 `invalid_submission` body (recorded divergence — the
//! label travels module-internally and never into a public reason).
//! The parse-level refusals (unknown `type`, out-of-grammar members, the
//! `customer_owned` refine) are the gate's own boundary 400, like every
//! cohort.
//!
//! ## Deliberately not here
//!
//! The repository's read paths (`access`, `listAssets`, `detail`,
//! `listCustodyHistory`, `listMaintenanceHistory`) and the tools surface
//! ride their serving cohorts: no mounted read route exists yet, so
//! porting them would ship unused SQL with no witness. The mutation +
//! commandStatus surface this module ports is exactly what the host routes
//! and the suite drive, plus `assert_assets_kernel` — the port of
//! repository `create()`'s provision checks.

use sqlx::PgConnection;
use uuid::Uuid;

use archava_domain::erp::assets::{
    AssetAccess, AssetCommand, AssetCommandStatus, AssetCondition, AssetMutation, AssetOwnership,
    AssetReceipt, CustodyEventType, ASSET_PERMISSIONS, ASSET_REVISION_MAX, STATUS_AVAILABLE,
    STATUS_IN_CUSTODY, STATUS_RETIRED, STATUS_RETURNED_TO_CUSTOMER, STATUS_UNDER_MAINTENANCE,
};
use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::PurchaseError;

/// The ledger label on database errors.
pub const ASSETS_LEDGER: &str = "assets";

/// The `archava_assets` namespace — checked constants only.
pub const ASSETS_NAMESPACE: &str = "archava_assets";

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

/// The TS 400-class verdicts (`invalid_condition`, the transaction-time
/// `invalid_request`) — carried on the [`ErpError::Unavailable`] arm so
/// THIS cohort's gate answers its 400 `invalid_submission` body. See the
/// module header's refusal net.
fn invalid() -> PurchaseError {
    PurchaseError::Domain(ErpError::Unavailable)
}

/// The crate-level whitelist already covers every SQLSTATE the TS assets
/// catch names — there is no ledger-specific arm to add.
fn refuses(error: sqlx::Error) -> PurchaseError {
    super::purchase_database_error(ASSETS_LEDGER, error)
}

fn parse_uuid_value(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

/// The `clock_timestamp()` sample every command takes once — the canonical
/// millisecond `Z` render, same family as the accommodation adapter's.
async fn observed_now(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map(|(text,)| text)
    .map_err(refuses)
}

// --- The context ladder ------------------------------------------------------

/// The membership ladder — ONE ledger. Loads the actor's own grant,
/// refuses inactive/unknown/shape-foreign rows, intersects with the human
/// principal a service names, and hands the access envelope back. The TS
/// `member(...)` + service branch of `transaction()`, statement for
/// statement.
pub async fn resolve_assets_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<AssetAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        ASSETS_NAMESPACE,
        ASSETS_LEDGER,
        tenant,
        &principal.id,
        ASSET_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    // The TS ladder compares the ROW's kind against the PRINCIPAL's kind
    // directly (`actor.kind !== principal.kind`).
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
        return Err(forbidden());
    }
    Ok(match principal.kind {
        PrincipalKind::Human => AssetAccess {
            permissions: own.permissions.clone(),
            owner_id: principal.id.clone(),
            scope: own.scope,
        },
        PrincipalKind::Service => {
            let on_behalf_of = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
            let human: MembershipRow = load_locked_membership(
                connection,
                ASSETS_NAMESPACE,
                ASSETS_LEDGER,
                tenant,
                on_behalf_of,
                ASSET_PERMISSIONS,
            )
            .await?
            .ok_or_else(forbidden)?;
            if human.kind != ErpMembershipKind::Human || !human.active {
                return Err(forbidden());
            }
            AssetAccess {
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

// --- The asset row read ------------------------------------------------------

/// `record` — the arm's lock read. `SELECT * FOR UPDATE`; the assigned
/// scope's screen (`owner_id` vs `current_custodian_id`) rides the CALLER,
/// exactly as the TS arms order it (no rows = `not_found`; a row outside
/// the assigned scope = `forbidden`).
const ASSET_SELECT_UPDATE: &str = "SELECT tenant_id,owner_id,status,condition,ownership,\
 current_custodian_id,revision FROM archava_assets.assets \
 WHERE tenant_id=$1 AND id=$2 FOR UPDATE";

/// The asset row's cells the arms judge against — `row` in TS terms, with
/// the lexical cells (`code`/`name`/…) left unmapped: no arm reads them
/// (the writes bind the command's own values, the schema carries the
/// bounds).
struct AssetRow {
    owner_id: String,
    status: String,
    condition: String,
    ownership: String,
    current_custodian_id: Option<String>,
    revision: i64,
}

impl AssetRow {
    fn try_get(row: &sqlx::postgres::PgRow, tenant: &str) -> Result<Self, PurchaseError> {
        use sqlx::Row;
        let tenant_id: String = row.try_get("tenant_id").map_err(refuses)?;
        if tenant_id != tenant {
            // RLS makes a cross-tenant screen impossible; a mismatch is drift.
            return Err(conflict());
        }
        let revision: i32 = row.try_get("revision").map_err(refuses)?;
        let revision = i64::from(revision);
        if !(1..=ASSET_REVISION_MAX).contains(&revision) {
            return Err(conflict());
        }
        Ok(Self {
            owner_id: row.try_get("owner_id").map_err(refuses)?,
            status: row.try_get("status").map_err(refuses)?,
            condition: row.try_get("condition").map_err(refuses)?,
            ownership: row.try_get("ownership").map_err(refuses)?,
            current_custodian_id: row.try_get("current_custodian_id").map_err(refuses)?,
            revision,
        })
    }

    /// The condition cell — parsed against the five-way vocabulary; a drift
    /// verdict (conflict) rather than an assumed state.
    fn condition(&self) -> Result<AssetCondition, PurchaseError> {
        AssetCondition::parse(&self.condition).ok_or_else(conflict)
    }

    /// The ownership cell — parsed against the two-way vocabulary.
    fn ownership(&self) -> Result<AssetOwnership, PurchaseError> {
        AssetOwnership::parse(&self.ownership).ok_or_else(conflict)
    }

    /// The assigned scope's screen — the TS
    /// `row.owner_id !== access.ownerId && row.current_custodian_id !==
    /// access.ownerId` gate. An `all` scope never runs it.
    fn visible(&self, access: &AssetAccess) -> bool {
        access.scope != ErpScope::Assigned
            || self.owner_id == access.owner_id
            || self.current_custodian_id.as_deref() == Some(access.owner_id.as_str())
    }

    /// The shared pre-arm family: visible, non-retired, expected revision.
    /// Every non-create arm runs exactly this triple, in this order —
    /// `not_found` wins only when the row itself was absent.
    fn guards(&self, access: &AssetAccess, expected_revision: i64) -> Result<(), PurchaseError> {
        if !self.visible(access) {
            return Err(forbidden());
        }
        if self.status == STATUS_RETIRED {
            // `AssetError('archived')` — a 409 in the TS api's code map.
            return Err(conflict());
        }
        if self.revision != expected_revision {
            return Err(conflict());
        }
        Ok(())
    }
}

async fn asset_row(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<AssetRow, PurchaseError> {
    let row = sqlx::query(ASSET_SELECT_UPDATE)
        .bind(tenant)
        .bind(id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?
        .ok_or_else(not_found)?;
    AssetRow::try_get(&row, tenant)
}

// --- The claim + replay plumbing ---------------------------------------------

async fn claim_assets_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed = sqlx::query(
        "INSERT INTO archava_assets.commands(\
tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING",
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
        "SELECT actor_id,fingerprint,receipt FROM archava_assets.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The assets audit row — `(tenant, request, actor, operation, asset,
/// revision, reason?)`, one per command.
#[allow(clippy::too_many_arguments)]
async fn insert_assets_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    asset_id: Uuid,
    revision: i64,
    reason: Option<&str>,
    now: &str,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_assets.audit(\
tenant_id,request_id,actor_id,operation,asset_id,revision,reason,occurred_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8::timestamptz)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(asset_id)
    .bind(int4(revision)?)
    .bind(reason)
    .bind(now)
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

/// The mutation pipeline — the port of `mutate()`. The human-only kind
/// gate is the route-boundary rule; every refusal rolls the WHOLE
/// transaction back. The host hands over the ALREADY STRICT-PARSED
/// mutation (its boundary parse answers 400), and the fingerprint that
/// keyed the claim.
pub async fn record_assets_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &AssetAccess,
    fingerprint: &str,
    mutation: &AssetMutation,
) -> Result<AssetReceipt, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    let command = &mutation.command;
    if !access.holds(&[command.required_permission()]) {
        return Err(forbidden());
    }
    let request_id = parse_uuid_value(&mutation.request_id)?;
    let actor_id = principal.id.as_str();

    let claimed =
        claim_assets_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    if !claimed {
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_command_row(connection, tenant, request_id)
                .await?
                .ok_or_else(conflict)?;
        if row_fingerprint != fingerprint || row_actor != actor_id {
            return Err(conflict());
        }
        let receipt_value = row_receipt.ok_or_else(conflict)?;
        let stored = AssetReceipt::parse(&receipt_value).map_err(|_| conflict())?;
        // Recovery re-authorization — the replay's assigned-scope gate:
        // the RECEIPT's asset must still sit inside the caller's scope
        // (`row &&` passes when the asset row is gone — the TS predicate
        // runs only on a found row).
        if access.scope == ErpScope::Assigned {
            let replay_asset = parse_uuid_value(&stored.asset_id)?;
            let row: Option<(String, Option<String>)> = sqlx::query_as(
                "SELECT owner_id,current_custodian_id FROM archava_assets.assets \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(replay_asset)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?;
            if let Some((owner_id, current_custodian_id)) = row {
                if owner_id != access.owner_id
                    && current_custodian_id.as_deref() != Some(access.owner_id.as_str())
                {
                    return Err(forbidden());
                }
            }
        }
        return Ok(stored);
    }

    // The sampled clock — one instant per command, bound everywhere.
    let now = observed_now(connection).await?;

    let receipt = execute_assets_arm(connection, tenant, access, request_id, command, &now).await?;
    insert_assets_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        command.mutation_type(),
        parse_uuid_value(&receipt.asset_id)?,
        receipt.revision,
        command.reason(),
        &now,
    )
    .await?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|_| conflict())?;
    sqlx::query(
        "UPDATE archava_assets.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

// --- execute(): the five arms -------------------------------------------------

/// The five executed arms, in the TS branch order. The create IS its own
/// row; every other arm locks the asset row first and runs the shared
/// guard family. All statements are static `.bind`ed forms — the TS
/// parameterized queries one-for-one.
async fn execute_assets_arm(
    connection: &mut PgConnection,
    tenant: &str,
    access: &AssetAccess,
    request_id: Uuid,
    command: &AssetCommand,
    now: &str,
) -> Result<AssetReceipt, PurchaseError> {
    if let AssetCommand::CreateAsset {
        code,
        name,
        category,
        serial_number,
        model,
        ownership,
        customer_contact_id,
        condition,
        initial_location,
    } = command
    {
        // create_asset — the unguarded create (`initialStatus` derives off
        // the ownership; a customer-owned asset starts in custody BY the
        // actor's own hand).
        let asset_id = Uuid::new_v4();
        let initial_status = match ownership {
            AssetOwnership::BusinessOwned => STATUS_AVAILABLE,
            AssetOwnership::CustomerOwned => STATUS_IN_CUSTODY,
        };
        let initial_custodian = match ownership {
            AssetOwnership::BusinessOwned => None,
            AssetOwnership::CustomerOwned => Some(principal_id(access)),
        };
        sqlx::query(
            "INSERT INTO archava_assets.assets(\
tenant_id,id,code,name,category,serial_number,model,ownership,customer_contact_id,\
status,condition,current_custodian_id,current_location,owner_id,revision,created_at,updated_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,1,$15::timestamptz,$15::timestamptz)",
        )
        .bind(tenant)
        .bind(asset_id)
        .bind(code)
        .bind(name)
        .bind(category)
        .bind(serial_number)
        .bind(model)
        .bind(ownership.as_str())
        .bind(customer_contact_id)
        .bind(initial_status)
        .bind(condition.as_str())
        .bind(initial_custodian)
        .bind(initial_location)
        .bind(principal_id(access))
        .bind(now)
        .execute(&mut *connection)
        .await
        .map_err(refuses)?;

        // Log the initial intake event if customer-owned.
        if *ownership == AssetOwnership::CustomerOwned {
            let event_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_assets.custody_events(\
tenant_id,id,asset_id,event_type,from_custodian_id,to_custodian_id,\
condition_before,condition_after,location,notes,recorded_by,recorded_at) \
VALUES($1,$2,$3,'service_intake',$4,$5,$6,$6,$7,\
'Initial service intake from customer',$5,$8::timestamptz)",
            )
            .bind(tenant)
            .bind(event_id)
            .bind(asset_id)
            .bind(customer_contact_id)
            .bind(principal_id(access))
            .bind(condition.as_str())
            .bind(initial_location)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
        }
        return Ok(AssetReceipt {
            request_id: request_id.to_string(),
            committed_at: now.to_string(),
            command_type: "create_asset",
            asset_id: asset_id.to_string(),
            revision: 1,
        });
    }

    // Every non-create arm names its asset. The typed extraction makes the
    // malformed case a hard conflict, never an assumed identifier.
    let asset_id = parse_uuid_value(command.asset_ref())?;
    let row = asset_row(connection, tenant, asset_id).await?;

    let receipt = match command {
        AssetCommand::UpdateAsset {
            name,
            category,
            serial_number,
            model,
            current_location,
            expected_revision,
            ..
        } => {
            row.guards(access, *expected_revision)?;
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_assets.assets SET \
name=COALESCE($3,name),category=COALESCE($4,category),\
serial_number=CASE WHEN $5::text IS NOT NULL THEN $5 ELSE serial_number END,\
model=CASE WHEN $6::text IS NOT NULL THEN $6 ELSE model END,\
current_location=CASE WHEN $7::text IS NOT NULL THEN $7 ELSE current_location END,\
revision=$8,updated_at=$9::timestamptz \
WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(asset_id)
            .bind(name)
            .bind(category)
            .bind(serial_number)
            .bind(model)
            .bind(current_location)
            .bind(int4(next_revision)?)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            AssetReceipt {
                request_id: request_id.to_string(),
                committed_at: now.to_string(),
                command_type: "update_asset",
                asset_id: asset_id.to_string(),
                revision: next_revision,
            }
        }
        AssetCommand::HandoverAsset {
            event_type,
            to_custodian_id,
            condition_after,
            location,
            notes,
            expected_revision,
            ..
        } => {
            row.guards(access, *expected_revision)?;
            // `archived` rides the shared guard for `retired`; the
            // custody terminal is this arm's OWN refusal
            // (`returned_to_customer` refuses any movement).
            if row.status == STATUS_RETURNED_TO_CUSTOMER {
                return Err(conflict());
            }
            let condition_before = row.condition()?;
            // Definite assignment across the exhaustive five-way match —
            // every arm derives its own next state, exactly the TS ladder.
            let next_status: String;
            let next_custodian: Option<String>;
            match event_type {
                CustodyEventType::Dispatch => {
                    if row.status == STATUS_IN_CUSTODY {
                        // `already_checked_out` — 409.
                        return Err(conflict());
                    }
                    if matches!(
                        condition_before,
                        AssetCondition::Unserviceable | AssetCondition::Damaged
                    ) {
                        // `invalid_condition` — the TS 400 class.
                        return Err(invalid());
                    }
                    next_status = STATUS_IN_CUSTODY.to_string();
                    next_custodian = to_custodian_id.clone();
                }
                CustodyEventType::Checkin => {
                    if row.status != STATUS_IN_CUSTODY {
                        // `not_in_custody` — 409.
                        return Err(conflict());
                    }
                    next_status = if matches!(
                        condition_after,
                        AssetCondition::Damaged | AssetCondition::Unserviceable
                    ) {
                        STATUS_UNDER_MAINTENANCE.to_string()
                    } else {
                        STATUS_AVAILABLE.to_string()
                    };
                    next_custodian = None;
                }
                CustodyEventType::Transfer => {
                    if row.status != STATUS_IN_CUSTODY {
                        return Err(conflict());
                    }
                    next_status = STATUS_IN_CUSTODY.to_string();
                    next_custodian = to_custodian_id.clone();
                }
                CustodyEventType::ServiceIntake => {
                    next_status = STATUS_UNDER_MAINTENANCE.to_string();
                    next_custodian = Some(to_custodian_id.clone().unwrap_or_else(|| {
                        // `principal.id` in TS — the caller's own id.
                        principal_id(access)
                    }));
                }
                CustodyEventType::CustomerReturn => {
                    if row.ownership()? != AssetOwnership::CustomerOwned {
                        // `invalid_request` — the TS 400 class.
                        return Err(invalid());
                    }
                    next_status = STATUS_RETURNED_TO_CUSTOMER.to_string();
                    next_custodian = None;
                }
            }
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_assets.assets SET \
status=$3,condition=$4,current_custodian_id=$5,\
current_location=COALESCE($6,current_location),revision=$7,updated_at=$8::timestamptz \
WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(asset_id)
            .bind(&next_status)
            .bind(condition_after.as_str())
            .bind(&next_custodian)
            .bind(location)
            .bind(int4(next_revision)?)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;

            let event_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_assets.custody_events(\
tenant_id,id,asset_id,event_type,from_custodian_id,to_custodian_id,\
condition_before,condition_after,location,notes,recorded_by,recorded_at) \
VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12::timestamptz)",
            )
            .bind(tenant)
            .bind(event_id)
            .bind(asset_id)
            .bind(event_type.as_str())
            .bind(&row.current_custodian_id)
            .bind(to_custodian_id)
            .bind(&row.condition)
            .bind(condition_after.as_str())
            .bind(location)
            .bind(notes)
            .bind(principal_id(access))
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            AssetReceipt {
                request_id: request_id.to_string(),
                committed_at: now.to_string(),
                command_type: "handover_asset",
                asset_id: asset_id.to_string(),
                revision: next_revision,
            }
        }
        AssetCommand::RecordMaintenance {
            service_type,
            description,
            parts_used,
            condition_after,
            location,
            expected_revision,
            ..
        } => {
            row.guards(access, *expected_revision)?;
            let mut next_status = row.status.clone();
            if row.status == STATUS_UNDER_MAINTENANCE && condition_after.serviceable() {
                // A customer-owned asset stays under maintenance after a
                // serviceable recording; a business-owned one releases.
                next_status = match row.ownership()? {
                    AssetOwnership::CustomerOwned => STATUS_UNDER_MAINTENANCE.to_string(),
                    AssetOwnership::BusinessOwned => STATUS_AVAILABLE.to_string(),
                };
            }
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_assets.assets SET \
status=$3,condition=$4,current_location=COALESCE($5,current_location),\
revision=$6,updated_at=$7::timestamptz WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(asset_id)
            .bind(&next_status)
            .bind(condition_after.as_str())
            .bind(location)
            .bind(int4(next_revision)?)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let maint_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_assets.maintenance_records(\
tenant_id,id,asset_id,service_type,description,parts_used,condition_after,\
performed_by,recorded_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9::timestamptz)",
            )
            .bind(tenant)
            .bind(maint_id)
            .bind(asset_id)
            .bind(service_type.as_str())
            .bind(description)
            .bind(parts_used)
            .bind(condition_after.as_str())
            .bind(principal_id(access))
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            AssetReceipt {
                request_id: request_id.to_string(),
                committed_at: now.to_string(),
                command_type: "record_maintenance",
                asset_id: asset_id.to_string(),
                revision: next_revision,
            }
        }
        AssetCommand::RetireAsset {
            expected_revision, ..
        } => {
            row.guards(access, *expected_revision)?;
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_assets.assets SET status='retired',current_custodian_id=null,\
revision=$3,updated_at=$4::timestamptz WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(asset_id)
            .bind(int4(next_revision)?)
            .bind(now)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            AssetReceipt {
                request_id: request_id.to_string(),
                committed_at: now.to_string(),
                command_type: "retire_asset",
                asset_id: asset_id.to_string(),
                revision: next_revision,
            }
        }
        AssetCommand::CreateAsset { .. } => return Err(conflict()),
    };

    Ok(receipt)
}

/// The caller's own principal id — the TS `principal.id` inside the arms'
/// custodian fallbacks and owner bindings. The access envelope pins it.
fn principal_id(access: &AssetAccess) -> String {
    access.owner_id.clone()
}

// --- commandStatus -------------------------------------------------------------

/// `commandStatus()` — the discovered receipt replays only while the
/// CURRENT access still authorizes the recorded operation: the read
/// refuses a caller whose actor id is not the recorded one (the TS
/// `row.actor_id !== principal.id` gate; assigned scope only — an `all`
/// reader answers any row in its tenant), and the receipt's asset must
/// still sit inside the assigned scope (`row &&` passes when the asset
/// row is gone).
pub async fn assets_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &AssetAccess,
    request_id: Uuid,
) -> Result<AssetCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    // The status transaction's own grant ('assets.read').
    if !access.holds(&["assets.read"]) {
        return Err(forbidden());
    }
    let Some((row_actor, _, row_receipt)) =
        fetch_command_row(connection, tenant, request_id).await?
    else {
        return Ok(AssetCommandStatus::NotCommitted);
    };
    let receipt_value = row_receipt.ok_or_else(conflict)?;
    let stored = AssetReceipt::parse(&receipt_value).map_err(|_| conflict())?;
    // The TS commandStatus's assigned-scope order: the recorded-actor gate
    // runs ONLY under an assigned scope, before the asset's own screen.
    if access.scope == ErpScope::Assigned && row_actor != principal.id.as_str() {
        return Err(forbidden());
    }
    if access.scope == ErpScope::Assigned {
        let Ok(asset_id) = parse_uuid_value(&stored.asset_id) else {
            return Err(conflict());
        };
        let row: Option<(String, Option<String>)> = sqlx::query_as(
            "SELECT owner_id,current_custodian_id FROM archava_assets.assets \
             WHERE tenant_id=$1 AND id=$2",
        )
        .bind(tenant)
        .bind(asset_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(refuses)?;
        if let Some((owner_id, current_custodian_id)) = row {
            if owner_id != access.owner_id
                && current_custodian_id.as_deref() != Some(access.owner_id.as_str())
            {
                return Err(forbidden());
            }
        }
    }
    Ok(AssetCommandStatus::Committed { receipt: stored })
}

// --- The startup support check ---------------------------------------------------

/// The port of `PostgresAssetRepository.create()`'s checks: the runtime
/// role must be a NON-owner (no super, no bypassrls, no schema ownership
/// of the assets ledger), the migration ledger must match the frozen pair
/// list, and the ledger's six tenant policies must be forced. Startup
/// support, not an authorization token — the posting ladder belongs to the
/// request path.
///
/// The TS derives the expected pair list from its OWN module's file bytes
/// (`migrate.ts` recomputes at import); this crate consumes the frozen
/// pair list its fixture recorded — the same derivation, the caller
/// supplies it (recorded divergence, ledger-internal).
///
/// Errors are `&'static str` refusals — the TS generic `Error`s. These
/// paths never answer callers; the serving state refuses to come up.
pub async fn assert_assets_kernel(
    pool: &sqlx::PgPool,
    assets_ledger: &[(String, String)],
) -> Result<(), String> {
    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
WHERE relnamespace='archava_assets'::regnamespace AND relowner=pg_roles.oid)) AS unsafe \
FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("assets runtime role check failed: {error}"))?;
    if role.map(|(unsafe_,)| unsafe_).unwrap_or(true) {
        return Err("Assets requires a non-owner runtime".to_string());
    }
    let actual: Vec<(String, String)> =
        sqlx::query_as("SELECT id,checksum FROM archava_assets.schema_migrations ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("unsupported assets ledger read: {error}"))?;
    if actual.len() != assets_ledger.len() || actual.iter().zip(assets_ledger).any(|(a, b)| a != b)
    {
        return Err("Unsupported assets migration checksum".to_string());
    }
    let names: &[&str] = &[
        "memberships",
        "assets",
        "custody_events",
        "maintenance_records",
        "commands",
        "audit",
    ][..];
    let statement = "SELECT count(*)::integer AS count FROM pg_class WHERE \
relnamespace='archava_assets'::regnamespace AND relname=ANY($1::text[]) \
AND relrowsecurity AND relforcerowsecurity";
    let (count,): (i32,) = sqlx::query_as(statement)
        .bind(names)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("assets policy check failed: {error}"))?;
    if count as usize != names.len() {
        return Err("Assets tenant policies must be forced".to_string());
    }
    Ok(())
}
