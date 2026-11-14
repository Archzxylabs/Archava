//! The support SQL adapter — the Archxy 2 port of
//! `packages/support-pg/src/repository.ts`: the case ledger's command
//! ladder (claim → authorize → arm → history → receipt) and the four read
//! projections (access, assignees, case page, case detail).
//!
//! ## Statement order (the TS port)
//!
//! The membership ladder resolves the actor's own row and, for a service
//! principal, the human it acts for: permissions INTERSECT, scope narrows
//! to `assigned` if either side is assigned, and the effective owner is the
//! human. A mutation then runs the TS `authorize()` (read + write + the
//! assign arm's extra grant and scope), claims the command row (SELECT,
//! fingerprint mismatch → conflict, stored receipt → replay, absent → claim),
//! executes the arm under the case row lock, appends the audit/transition
//! history and binds the receipt.
//!
//! ## The arm rules the pure layer names
//!
//! `case.start/wait/resume/close/cancel` walk the closed state table
//! ([`state_transition`]); `resolve` needs `in_progress`/`waiting`;
//! `reopen` needs `resolved`/`closed` and clears the resolution stamps;
//! assign/priority/target refuse a closed or cancelled case; an assignee
//! must be an active human with `cases.read`. A stale revision is a
//! conflict before any write.
//!
//! ## The history invariants
//!
//! Migration 001's deferred constraint trigger requires every case revision
//! to have its audit row, its transition row and its receipt. The adapter
//! writes all three in the same transaction; the fixture's commit is the
//! proof that the chain is complete.

use archava_domain::erp::purchasing::{PrincipalKind, PurchasePrincipal};
use archava_domain::erp::support::{
    authorize, mutable_lifecycle, parse_case_page, reopenable, resolvable, state_transition,
    Assignee, AssigneePage, AssigneePageQuery, CaseDetail, CasePage, SupportAccess, SupportAudit,
    SupportCase, SupportCommand, SupportCommandStatus, SupportMutation, SupportNote,
    SupportReceipt, SupportState, SupportTransition, SUPPORT_PERMISSIONS,
};
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};
use serde_json::Value;
use sqlx::PgConnection;
use sqlx::Row;
use thiserror::Error;

use super::commerce_kernel::load_locked_membership;
use super::PurchaseError;

/// The ledger this adapter owns.
pub const SUPPORT_LEDGER: &str = "support";
/// The schema the statements ride.
pub const SUPPORT_NAMESPACE: &str = "archava_support";

/// The support refusal vocabulary — the TS `SupportError` codes.
#[derive(Debug, Error)]
pub enum SupportError {
    #[error("support domain refusal: {0:?}")]
    Domain(ErpError),
    /// A stored/query document the strict grammar refuses (400 class).
    #[error("support document refused by the grammar")]
    Invalid,
    #[error("support database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<SupportError> for PurchaseError {
    fn from(error: SupportError) -> Self {
        match error {
            SupportError::Domain(domain) => PurchaseError::Domain(domain),
            SupportError::Invalid => PurchaseError::Domain(ErpError::Unavailable),
            SupportError::Database { ledger, source } => PurchaseError::Database { ledger, source },
        }
    }
}

fn forbidden() -> SupportError {
    SupportError::Domain(ErpError::Forbidden)
}

fn not_found() -> SupportError {
    SupportError::Domain(ErpError::NotFound)
}

fn conflict() -> SupportError {
    SupportError::Domain(ErpError::Conflict)
}

fn database(error: sqlx::Error) -> SupportError {
    SupportError::Database {
        ledger: SUPPORT_LEDGER,
        source: error,
    }
}

fn invalid() -> SupportError {
    SupportError::Invalid
}

/// The case projection's columns, in the TS `caseSchema` order.
const CASE_COLUMNS: &str = "id::text AS id, tenant_id, owner_id, assignee_id, requester_name, \
subject, description, priority, state, \
to_char(due_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS due_at, \
cycle::bigint AS cycle, resolution, \
to_char(resolved_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS resolved_at, \
to_char(closed_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS closed_at, \
revision::bigint AS revision, \
to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS created_at, \
to_char(updated_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS updated_at";

fn uuid_of(value: &str) -> uuid::Uuid {
    uuid::Uuid::parse_str(value).expect("a grammar-admitted uuid")
}

fn row_instant(row: &sqlx::postgres::PgRow, key: &str) -> Result<String, SupportError> {
    row.try_get::<Option<String>, _>(key)
        .map_err(database)?
        .ok_or_else(invalid)
}

fn opt_instant(row: &sqlx::postgres::PgRow, key: &str) -> Result<Option<String>, SupportError> {
    row.try_get(key).map_err(database)
}

// --- The context ---------------------------------------------------------------

/// The member ladder — the TS `transaction()`'s membership resolution.
pub async fn resolve_support_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<SupportAccess, SupportError> {
    let own = load_locked_membership(
        connection,
        SUPPORT_NAMESPACE,
        SUPPORT_LEDGER,
        tenant,
        &principal.id,
        SUPPORT_PERMISSIONS,
    )
    .await
    .map_err(|error| match error {
        PurchaseError::Domain(domain) => SupportError::Domain(domain),
        // This ledger's error type has no catalogued-reason variant yet.
        PurchaseError::Refused(_) => SupportError::Domain(ErpError::Conflict),
        PurchaseError::Database { ledger, source } => SupportError::Database { ledger, source },
    })?
    .ok_or_else(forbidden)?;
    if !own.active {
        return Err(forbidden());
    }
    let principal_kind = match principal.kind {
        PrincipalKind::Human => ErpMembershipKind::Human,
        PrincipalKind::Service => ErpMembershipKind::Service,
    };
    if own.kind != principal_kind {
        return Err(forbidden());
    }
    let mut access = SupportAccess {
        permissions: own.permissions.clone(),
        owner_id: principal.id.clone(),
        scope: match own.scope {
            ErpScope::All => "all".to_string(),
            ErpScope::Assigned => "assigned".to_string(),
        },
    };
    if principal.kind == PrincipalKind::Service {
        let human_id = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
        let human = load_locked_membership(
            connection,
            SUPPORT_NAMESPACE,
            SUPPORT_LEDGER,
            tenant,
            human_id,
            SUPPORT_PERMISSIONS,
        )
        .await
        .map_err(|error| match error {
            PurchaseError::Domain(domain) => SupportError::Domain(domain),
            // This ledger's error type has no catalogued-reason variant yet.
            PurchaseError::Refused(_) => SupportError::Domain(ErpError::Conflict),
            PurchaseError::Database { ledger, source } => SupportError::Database { ledger, source },
        })?
        .ok_or_else(forbidden)?;
        if !human.active || human.kind != ErpMembershipKind::Human {
            return Err(forbidden());
        }
        let permissions = own
            .permissions
            .iter()
            .filter(|permission| human.permissions.iter().any(|held| held == *permission))
            .cloned()
            .collect();
        access = SupportAccess {
            permissions,
            owner_id: human_id.to_string(),
            scope: if own.scope == ErpScope::Assigned || human.scope == ErpScope::Assigned {
                "assigned".to_string()
            } else {
                "all".to_string()
            },
        };
    }
    Ok(access)
}

fn require(access: &SupportAccess, permission: &str) -> Result<(), SupportError> {
    if access.holds(permission) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

// --- The claim -----------------------------------------------------------------

enum Claim {
    Fresh,
    Replay(Box<SupportReceipt>),
}

async fn claim_command(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    actor_id: &str,
    fingerprint: &str,
) -> Result<Claim, SupportError> {
    let row = sqlx::query(
        "SELECT receipt, fingerprint FROM archava_support.commands \
WHERE tenant_id=$1 AND request_id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    if let Some(row) = row {
        let stored_fingerprint: String = row.try_get("fingerprint").map_err(database)?;
        if stored_fingerprint != fingerprint {
            return Err(conflict());
        }
        let receipt: Option<Value> = row.try_get("receipt").map_err(database)?;
        if let Some(receipt) = receipt {
            let parsed = SupportReceipt::parse(&receipt, true).map_err(|_| invalid())?;
            return Ok(Claim::Replay(Box::new(parsed)));
        }
        return Ok(Claim::Fresh);
    }
    sqlx::query(
        "INSERT INTO archava_support.commands(tenant_id, request_id, actor_id, fingerprint) \
VALUES($1, $2, $3, $4)",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(Claim::Fresh)
}

async fn bind_receipt(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: &str,
    receipt: &SupportReceipt,
) -> Result<(), SupportError> {
    let document = serde_json::to_value(receipt).map_err(|_| invalid())?;
    sqlx::query(
        "UPDATE archava_support.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3",
    )
    .bind(document)
    .bind(tenant)
    .bind(uuid_of(command_id))
    .execute(&mut *connection)
    .await
    .map_err(database)?;
    Ok(())
}

// --- The case row --------------------------------------------------------------

struct CaseSnapshot {
    state: String,
    cycle: i64,
    resolution: Option<String>,
    due_at: Option<String>,
    assignee_id: Option<String>,
    priority: String,
    revision: i64,
}

async fn owned_case(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SupportAccess,
    id: &str,
    lock: bool,
) -> Result<CaseSnapshot, SupportError> {
    require(access, "cases.read")?;
    let statement = format!(
        "SELECT state, cycle::bigint AS cycle, resolution, \
to_char(due_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS due_at, \
assignee_id, priority, revision::bigint AS revision \
FROM archava_support.cases WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4 OR assignee_id=$4) {}",
        if lock { "FOR UPDATE" } else { "FOR SHARE" }
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(statement))
        .bind(tenant)
        .bind(uuid_of(id))
        .bind(access.sees_all())
        .bind(&access.owner_id)
        .fetch_optional(&mut *connection)
        .await
        .map_err(database)?;
    let Some(row) = row else {
        return Err(not_found());
    };
    Ok(CaseSnapshot {
        state: row.try_get("state").map_err(database)?,
        cycle: row.try_get("cycle").map_err(database)?,
        resolution: row.try_get("resolution").map_err(database)?,
        due_at: row.try_get("due_at").map_err(database)?,
        assignee_id: row.try_get("assignee_id").map_err(database)?,
        priority: row.try_get("priority").map_err(database)?,
        revision: row.try_get("revision").map_err(database)?,
    })
}

fn map_case(row: &sqlx::postgres::PgRow, observed: &str) -> Result<SupportCase, SupportError> {
    let state: String = row.try_get("state").map_err(database)?;
    let due_at: Option<String> = row.try_get("due_at").map_err(database)?;
    let overdue = SupportState::parse(&state)
        .map(SupportState::is_active)
        .unwrap_or(false)
        && due_at.as_deref().is_some_and(|due| due <= observed);
    Ok(SupportCase {
        id: row.try_get("id").map_err(database)?,
        tenant_id: row.try_get("tenant_id").map_err(database)?,
        owner_id: row.try_get("owner_id").map_err(database)?,
        assignee_id: row.try_get("assignee_id").map_err(database)?,
        requester_name: row.try_get("requester_name").map_err(database)?,
        subject: row.try_get("subject").map_err(database)?,
        description: row.try_get("description").map_err(database)?,
        priority: row.try_get("priority").map_err(database)?,
        state,
        due_at,
        overdue,
        cycle: row.try_get("cycle").map_err(database)?,
        resolution: row.try_get("resolution").map_err(database)?,
        resolved_at: opt_instant(row, "resolved_at")?,
        closed_at: opt_instant(row, "closed_at")?,
        revision: row.try_get("revision").map_err(database)?,
        created_at: row_instant(row, "created_at")?,
        updated_at: row_instant(row, "updated_at")?,
    })
}

// --- The arm -------------------------------------------------------------------

/// The whole command ladder — claim, authorize, arm, history, receipt.
pub async fn record_support_command(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SupportAccess,
    fingerprint: &str,
    mutation: &SupportMutation,
) -> Result<SupportReceipt, SupportError> {
    authorize(access, &mutation.command).map_err(SupportError::Domain)?;
    match claim_command(
        connection,
        tenant,
        &mutation.request_id,
        &access.owner_id,
        fingerprint,
    )
    .await?
    {
        Claim::Replay(receipt) => return Ok(*receipt),
        Claim::Fresh => {}
    }

    let command = &mutation.command;
    let (
        previous,
        current_state,
        current_cycle,
        current_resolution,
        current_due,
        current_assignee,
        current_priority,
        resolved,
        closed,
        reopened,
        target_id,
        revision,
    ) = match command {
        SupportCommand::Create {
            requester_name,
            subject,
            description,
            priority,
            due_at,
        } => {
            let row = sqlx::query(sqlx::AssertSqlSafe(format!(
                "INSERT INTO archava_support.cases(\
tenant_id, id, owner_id, requester_name, subject, description, priority, due_at) \
VALUES($1, $2, $3, $4, $5, $6, $7, $8::timestamptz) RETURNING {CASE_COLUMNS}"
            )))
            .bind(tenant)
            .bind(uuid::Uuid::new_v4())
            .bind(&access.owner_id)
            .bind(requester_name)
            .bind(subject)
            .bind(description)
            .bind(priority)
            .bind(due_at.as_deref())
            .fetch_one(&mut *connection)
            .await
            .map_err(database)?;
            let id: String = row.try_get("id").map_err(database)?;
            (
                None,
                row.try_get::<String, _>("state").map_err(database)?,
                row.try_get::<i64, _>("cycle").map_err(database)?,
                row.try_get::<Option<String>, _>("resolution")
                    .map_err(database)?,
                row.try_get::<Option<String>, _>("due_at")
                    .map_err(database)?,
                row.try_get::<Option<String>, _>("assignee_id")
                    .map_err(database)?,
                row.try_get::<String, _>("priority").map_err(database)?,
                false,
                false,
                false,
                id,
                1i64,
            )
        }
        _ => {
            let (id, expected) = command.target().ok_or_else(conflict)?;
            let previous = owned_case(connection, tenant, access, id, true).await?;
            if previous.revision != expected {
                return Err(conflict());
            }
            let from = SupportState::parse(&previous.state).ok_or_else(conflict)?;
            let mut state = from;
            let mut cycle = previous.cycle;
            let mut resolution = previous.resolution.clone();
            let mut due = previous.due_at.clone();
            let mut assignee = previous.assignee_id.clone();
            let mut priority = previous.priority.clone();
            let mut resolved = false;
            let mut closed = false;
            let mut reopened = false;

            match command {
                SupportCommand::NoteAdd { body, .. } => {
                    sqlx::query(
                        "INSERT INTO archava_support.notes(\
tenant_id, id, case_id, author_id, body, request_id) VALUES($1, $2, $3, $4, $5, $6)",
                    )
                    .bind(tenant)
                    .bind(uuid::Uuid::new_v4())
                    .bind(uuid_of(id))
                    .bind(&access.owner_id)
                    .bind(body)
                    .bind(uuid_of(&mutation.request_id))
                    .execute(&mut *connection)
                    .await
                    .map_err(database)?;
                }
                SupportCommand::Assign { assignee_id, .. } => {
                    if !mutable_lifecycle(from) {
                        return Err(conflict());
                    }
                    assignee = assignee_id.clone();
                    if let Some(wanted) = assignee_id {
                        let membership = load_locked_membership(
                            connection,
                            SUPPORT_NAMESPACE,
                            SUPPORT_LEDGER,
                            tenant,
                            wanted,
                            SUPPORT_PERMISSIONS,
                        )
                        .await
                        .map_err(|error| match error {
                            PurchaseError::Domain(domain) => SupportError::Domain(domain),
                            // This ledger's error type has no catalogued-reason variant yet.
                            PurchaseError::Refused(_) => SupportError::Domain(ErpError::Conflict),
                            PurchaseError::Database { ledger, source } => {
                                SupportError::Database { ledger, source }
                            }
                        })?;
                        let Some(membership) = membership else {
                            return Err(forbidden());
                        };
                        if membership.kind != ErpMembershipKind::Human
                            || !membership.active
                            || !membership
                                .permissions
                                .iter()
                                .any(|permission| permission == "cases.read")
                        {
                            return Err(forbidden());
                        }
                    }
                }
                SupportCommand::SetPriority { priority: next, .. } => {
                    if !mutable_lifecycle(from) {
                        return Err(conflict());
                    }
                    priority = next.clone();
                }
                SupportCommand::SetTarget { due_at, .. } => {
                    if !mutable_lifecycle(from) {
                        return Err(conflict());
                    }
                    due = due_at.clone();
                }
                SupportCommand::Resolve {
                    resolution: text, ..
                } => {
                    if !resolvable(from) {
                        return Err(conflict());
                    }
                    state = SupportState::Resolved;
                    resolution = Some(text.clone());
                    resolved = true;
                }
                SupportCommand::Reopen { due_at, .. } => {
                    if !reopenable(from) {
                        return Err(conflict());
                    }
                    state = SupportState::Open;
                    cycle += 1;
                    resolution = None;
                    due = due_at.clone();
                    reopened = true;
                }
                _ => {
                    state = state_transition(from, command.operation()).ok_or_else(conflict)?;
                    closed = matches!(state, SupportState::Closed);
                }
            }

            (
                Some(previous),
                state.as_str().to_string(),
                cycle,
                resolution,
                due,
                assignee,
                priority,
                resolved,
                closed,
                reopened,
                id.to_string(),
                expected + 1,
            )
        }
    };

    // The create arm's row is already at revision 1 — the common UPDATE is
    // the non-create arm's step (the TS `if (c.operation !== 'case.create')`).
    if previous.is_some() {
        sqlx::query(
            "UPDATE archava_support.cases SET state=$3, cycle=$4, resolution=$5, due_at=$6::timestamptz, \
assignee_id=$7, priority=$8, \
resolved_at=CASE WHEN $9::boolean THEN clock_timestamp() WHEN $11::boolean THEN NULL ELSE resolved_at END, \
closed_at=CASE WHEN $10::boolean THEN clock_timestamp() WHEN $11::boolean THEN NULL ELSE closed_at END, \
revision=revision+1, updated_at=clock_timestamp() \
WHERE tenant_id=$1 AND id=$2",
        )
    .bind(tenant)
    .bind(uuid_of(&target_id))
    .bind(&current_state)
    .bind(current_cycle)
    .bind(&current_resolution)
    .bind(current_due.as_deref())
    .bind(&current_assignee)
    .bind(&current_priority)
        .bind(resolved)
        .bind(closed)
        .bind(reopened)
        .execute(&mut *connection)
        .await
        .map_err(database)?;
    }

    let reason = command.reason();
    sqlx::query(
        "INSERT INTO archava_support.audit(\
tenant_id, request_id, actor_id, operation, record_id, revision, reason) \
VALUES($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(tenant)
    .bind(uuid_of(&mutation.request_id))
    .bind(&access.owner_id)
    .bind(command.operation())
    .bind(uuid_of(&target_id))
    .bind(revision)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    let previous_state = previous.as_ref().map(|row| row.state.clone());
    let previous_resolution = previous.as_ref().and_then(|row| row.resolution.clone());
    let previous_assignee = previous.as_ref().and_then(|row| row.assignee_id.clone());
    let previous_priority = previous.as_ref().map(|row| row.priority.clone());
    let previous_due = previous.as_ref().and_then(|row| row.due_at.clone());
    sqlx::query(
        "INSERT INTO archava_support.transitions(\
tenant_id, request_id, case_id, actor_id, previous_state, next_state, cycle, case_revision, \
previous_resolution, resolution, reason, operation, previous_assignee_id, assignee_id, \
previous_priority, priority, previous_due_at, due_at) \
VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, \
$17::timestamptz, $18::timestamptz)",
    )
    .bind(tenant)
    .bind(uuid_of(&mutation.request_id))
    .bind(uuid_of(&target_id))
    .bind(&access.owner_id)
    .bind(previous_state)
    .bind(&current_state)
    .bind(current_cycle)
    .bind(revision)
    .bind(previous_resolution)
    .bind(&current_resolution)
    .bind(reason)
    .bind(command.operation())
    .bind(previous_assignee)
    .bind(&current_assignee)
    .bind(previous_priority)
    .bind(&current_priority)
    .bind(previous_due.as_deref())
    .bind(current_due.as_deref())
    .execute(&mut *connection)
    .await
    .map_err(database)?;

    let receipt = SupportReceipt {
        request_id: mutation.request_id.clone(),
        operation: command.operation().to_string(),
        id: target_id,
        revision,
        replayed: false,
    };
    bind_receipt(connection, tenant, &mutation.request_id, &receipt).await?;
    Ok(receipt)
}

// --- The reads -----------------------------------------------------------------

/// The caller's effective access envelope.
pub async fn support_access(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<SupportAccess, SupportError> {
    resolve_support_context(connection, tenant, principal).await
}

/// The assignee picker — human readers, tenant-wide scope only.
pub async fn support_assignees(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SupportAccess,
    query: &AssigneePageQuery,
) -> Result<AssigneePage, SupportError> {
    require(access, "cases.read")?;
    require(access, "cases.assign")?;
    if !access.sees_all() {
        return Err(forbidden());
    }
    let rows = sqlx::query(
        "SELECT principal_id, display_name FROM archava_support.memberships \
WHERE tenant_id=$1 AND active AND kind='human' AND 'cases.read'=ANY(permissions) \
AND ($2::text IS NULL OR principal_id>$2) ORDER BY principal_id LIMIT $3",
    )
    .bind(tenant)
    .bind(query.after.as_deref())
    .bind(query.limit + 1)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;
    let mut items = Vec::new();
    for row in rows.iter().take(query.limit as usize) {
        let id: String = row.try_get("principal_id").map_err(database)?;
        let display: String = row.try_get("display_name").map_err(database)?;
        items.push(Assignee {
            display_name: if display.is_empty() {
                id.clone()
            } else {
                display
            },
            id,
        });
    }
    let next = if rows.len() as i64 > query.limit {
        items.last().map(|item| item.id.clone())
    } else {
        None
    };
    Ok(AssigneePage { items, next })
}

/// The case worklist — keyset page, scope predicate, overdue measured
/// against ONE sampled clock.
pub async fn support_cases(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SupportAccess,
    raw: &Value,
) -> Result<CasePage, SupportError> {
    let query = parse_case_page(raw).map_err(|_| SupportError::Invalid)?;
    require(access, "cases.read")?;
    let observed = observed_clock(connection).await?;
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {CASE_COLUMNS} FROM archava_support.cases \
WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3 OR assignee_id=$3) \
AND ($4::uuid IS NULL OR id>$4) AND ($5::text IS NULL OR state=$5) \
AND ($6::text IS NULL OR priority=$6) \
AND ($7::boolean IS NULL OR (state IN ('open','in_progress','waiting') AND due_at IS NOT NULL \
AND due_at<=$8::timestamptz)=$7) AND ($10='' OR strpos(lower(requester_name||' '||subject||' '||description),lower($10))>0) ORDER BY id LIMIT $9"
    )))
    .bind(tenant)
    .bind(access.sees_all())
    .bind(&access.owner_id)
    .bind(query.after.as_deref().map(uuid_of))
    .bind(query.state.as_deref())
    .bind(query.priority.as_deref())
    .bind(query.overdue)
    .bind(&observed)
    .bind(query.limit + 1)
    .bind(query.search)
    .fetch_all(&mut *connection)
    .await
    .map_err(database)?;
    let mut items = Vec::new();
    for row in rows.iter().take(query.limit as usize) {
        items.push(map_case(row, &observed)?);
    }
    let next = if rows.len() as i64 > query.limit {
        items.last().map(|item| item.id.clone())
    } else {
        None
    };
    Ok(CasePage {
        items,
        next,
        evaluated_at: observed,
    })
}

/// The case detail — record + the three bounded histories.
pub async fn support_detail(
    connection: &mut PgConnection,
    tenant: &str,
    access: &SupportAccess,
    id: &str,
) -> Result<CaseDetail, SupportError> {
    let observed = observed_clock(connection).await?;
    // The owned-case read is the scope gate; a hidden case is not found.
    let _ = owned_case(connection, tenant, access, id, false).await?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {CASE_COLUMNS} FROM archava_support.cases WHERE tenant_id=$1 AND id=$2"
    )))
    .bind(tenant)
    .bind(uuid_of(id))
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;
    let record = map_case(&row, &observed)?;

    let (notes, notes_has_more) = if access.holds("notes.read") {
        let rows = sqlx::query(
            "SELECT id::text AS id, case_id::text AS case_id, author_id, body, \
request_id::text AS request_id, \
to_char(occurred_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS occurred_at \
FROM archava_support.notes WHERE tenant_id=$1 AND case_id=$2 \
ORDER BY occurred_at DESC, id DESC LIMIT 101",
        )
        .bind(tenant)
        .bind(uuid_of(id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
        let has_more = rows.len() > 100;
        let mut items = Vec::new();
        for row in rows.iter().take(100) {
            items.push(SupportNote {
                id: row.try_get("id").map_err(database)?,
                case_id: row.try_get("case_id").map_err(database)?,
                author_id: row.try_get("author_id").map_err(database)?,
                body: row.try_get("body").map_err(database)?,
                request_id: row.try_get("request_id").map_err(database)?,
                occurred_at: row_instant(row, "occurred_at")?,
            });
        }
        (Some(items), has_more)
    } else {
        (None, false)
    };

    let (transitions, transitions_has_more) = if access.holds("audit.read") {
        let rows = sqlx::query(
            "SELECT request_id::text AS request_id, actor_id, operation, \
previous_assignee_id, assignee_id, previous_priority, priority, \
to_char(previous_due_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS previous_due_at, \
to_char(due_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS due_at, \
previous_state, next_state, cycle::bigint AS cycle, case_revision::bigint AS case_revision, \
previous_resolution, resolution, reason, \
to_char(occurred_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS occurred_at \
FROM archava_support.transitions WHERE tenant_id=$1 AND case_id=$2 \
ORDER BY case_revision DESC LIMIT 101",
        )
        .bind(tenant)
        .bind(uuid_of(id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
        let has_more = rows.len() > 100;
        let mut items = Vec::new();
        for row in rows.iter().take(100) {
            items.push(SupportTransition {
                request_id: row.try_get("request_id").map_err(database)?,
                actor_id: row.try_get("actor_id").map_err(database)?,
                operation: row.try_get("operation").map_err(database)?,
                previous_assignee_id: row.try_get("previous_assignee_id").map_err(database)?,
                assignee_id: row.try_get("assignee_id").map_err(database)?,
                previous_priority: row.try_get("previous_priority").map_err(database)?,
                priority: row.try_get("priority").map_err(database)?,
                previous_due_at: opt_instant(row, "previous_due_at")?,
                due_at: opt_instant(row, "due_at")?,
                from: row.try_get("previous_state").map_err(database)?,
                to: row.try_get("next_state").map_err(database)?,
                cycle: row.try_get("cycle").map_err(database)?,
                previous_resolution: row.try_get("previous_resolution").map_err(database)?,
                resolution: row.try_get("resolution").map_err(database)?,
                reason: row.try_get("reason").map_err(database)?,
                occurred_at: row_instant(row, "occurred_at")?,
            });
        }
        (Some(items), has_more)
    } else {
        (None, false)
    };

    let (audit, audit_has_more) = if access.holds("audit.read") {
        let rows = sqlx::query(
            "SELECT request_id::text AS request_id, actor_id, operation, record_id::text AS record_id, \
revision::bigint AS revision, reason, \
to_char(occurred_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS occurred_at \
FROM archava_support.audit WHERE tenant_id=$1 AND record_id=$2 \
ORDER BY revision DESC LIMIT 101",
        )
        .bind(tenant)
        .bind(uuid_of(id))
        .fetch_all(&mut *connection)
        .await
        .map_err(database)?;
        let has_more = rows.len() > 100;
        let mut items = Vec::new();
        for row in rows.iter().take(100) {
            items.push(SupportAudit {
                request_id: row.try_get("request_id").map_err(database)?,
                actor_id: row.try_get("actor_id").map_err(database)?,
                operation: row.try_get("operation").map_err(database)?,
                record_id: row.try_get("record_id").map_err(database)?,
                revision: row.try_get("revision").map_err(database)?,
                reason: row.try_get("reason").map_err(database)?,
                occurred_at: row_instant(row, "occurred_at")?,
            });
        }
        (Some(items), has_more)
    } else {
        (None, false)
    };

    Ok(CaseDetail {
        record,
        notes,
        notes_has_more,
        transitions,
        transitions_has_more,
        audit,
        audit_has_more,
    })
}

async fn observed_clock(connection: &mut PgConnection) -> Result<String, SupportError> {
    let row = sqlx::query(
        "SELECT to_char(clock_timestamp() at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') AS observed",
    )
    .fetch_one(&mut *connection)
    .await
    .map_err(database)?;
    row_instant(&row, "observed")
}

/// The durable status of one command id — the TS `commandStatus()`.
pub async fn support_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    command_id: &str,
) -> Result<SupportCommandStatus, SupportError> {
    let access = resolve_support_context(connection, tenant, principal).await?;
    require(&access, "cases.read")?;
    let row = sqlx::query(
        "SELECT actor_id, receipt FROM archava_support.commands \
WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(uuid_of(command_id))
    .fetch_optional(&mut *connection)
    .await
    .map_err(database)?;
    let Some(row) = row else {
        return Ok(SupportCommandStatus::Unknown);
    };
    let actor_id: String = row.try_get("actor_id").map_err(database)?;
    if actor_id != principal.id {
        return Err(conflict());
    }
    let stored: Option<Value> = row.try_get("receipt").map_err(database)?;
    let Some(stored) = stored else {
        return Err(conflict());
    };
    let receipt = SupportReceipt::parse(&stored, true).map_err(|_| invalid())?;
    let write = if receipt.operation == "note.add" {
        "notes.write"
    } else {
        "cases.write"
    };
    require(&access, write)?;
    if receipt.operation == "case.assign" {
        require(&access, "cases.assign")?;
        if !access.sees_all() {
            return Err(forbidden());
        }
    }
    let _ = owned_case(connection, tenant, &access, &receipt.id, false).await?;
    Ok(SupportCommandStatus::Committed {
        receipt: Box::new(receipt),
    })
}

// --- The kernel -----------------------------------------------------------------

/// The ledger's own readiness facts — the port of the TS repository's
/// `create()` guards: a NON-OWNER runtime role, the exact migration ledger,
/// and six FORCED tenant policies.
pub async fn assert_support_kernel(
    pool: &sqlx::PgPool,
    support_ledger: &[(String, String)],
) -> Result<(), String> {
    let role: Option<(bool,)> = sqlx::query_as(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class \
WHERE relnamespace='archava_support'::regnamespace AND relowner=pg_roles.oid)) AS unsafe \
FROM pg_roles WHERE rolname=current_user",
    )
    .fetch_optional(pool)
    .await
    .map_err(|error| format!("support runtime role check failed: {error}"))?;
    if role.map(|(unsafe_,)| unsafe_).unwrap_or(true) {
        return Err("Support requires a non-owner runtime".to_string());
    }
    let actual: Vec<(String, String)> =
        sqlx::query_as("SELECT id,checksum FROM archava_support.schema_migrations ORDER BY id")
            .fetch_all(pool)
            .await
            .map_err(|error| format!("unsupported support ledger read: {error}"))?;
    if actual.len() != support_ledger.len()
        || actual.iter().zip(support_ledger).any(|(a, b)| a != b)
    {
        return Err("Unsupported support migration checksum".to_string());
    }
    let names: &[&str] = &[
        "memberships",
        "cases",
        "commands",
        "notes",
        "transitions",
        "audit",
    ][..];
    let statement = "SELECT count(*)::integer AS count FROM pg_class WHERE \
relnamespace='archava_support'::regnamespace AND relname=ANY($1::text[]) \
AND relrowsecurity AND relforcerowsecurity";
    let (count,): (i32,) = sqlx::query_as(statement)
        .bind(names)
        .fetch_one(pool)
        .await
        .map_err(|error| format!("support policy check failed: {error}"))?;
    if count as usize != names.len() {
        return Err("Support tenant policies must be forced".to_string());
    }
    Ok(())
}
