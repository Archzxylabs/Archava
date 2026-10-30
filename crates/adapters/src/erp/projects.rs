//! The projects SQL adapter — the Archxy 2 port of
//! `packages/projects-pg/src/repository.ts` (the R04 project/task/milestone
//! delivery surface), executed strictly inside the shared RLS transaction
//! cohort.
//!
//! ## Statement order (the TS port)
//!
//! `mutate()` runs: membership ladder (one ledger), the HUMAN-ONLY kind
//! gate (`Project machine writes are not registered.`), the per-arm write
//! permission, the `commands` claim (INSERT ON CONFLICT DO NOTHING with a
//! `RETURNING request_id` the TS distinguishes), then the arm. Non-create
//! arms load the project via `ownedProject` (the assignment scope rule),
//! refuse a mismatched `projectRevision` / an over-ceiling revision, and
//! THEIR gates:
//!
//! - `project.transition` refuses out-of-table moves (a conflict), and a
//!   `delivered` target refuses crooked counts — required tasks exist, no
//!   pending one, no pending milestones.
//! - `task.create`/`milestone.create` refuse finished projects and the
//!   100-work-item ceiling; a task bound to a milestone must read it and
//!   its state must still be `open`.
//! - `task.transition` reads its task FOR UPDATE, refuses a stalled
//!   expected revision or out-of-table state, and refuses work under a
//!   REACHED milestone unless it is being done already... (the TS check:
//!   a non-`done` target under a reached milestone conflicts).
//! - `milestone.complete` reads its milestone FOR UPDATE, refuses a
//!   reached milestone and counts its required tasks: at least one
//!   required task, none pending — else conflict.
//! - Every non-create arm bumps the project revision at the end; the audit
//!   row records the operation, the receipt's ids/revisions and the reason
//!   (`reason` for transition arms, `evidence` for milestone.complete).
//!
//! ## Error vocabulary
//!
//! `ProjectError('forbidden'|'not_found'|'conflict')` rides the shared
//! `PurchaseError`/`ErpError` carrier 1:1 (no word-level flattening here).
//! Replays (`recover_from_row`) re-read the recorded project FOR SHARE
//! under the CURRENT assignment scope before handing the receipt back.

use sqlx::postgres::PgConnection;
use uuid::Uuid;

use archava_domain::erp::projects::{
    replayed_receipt, ProjectAccess, ProjectCommand, ProjectCommandStatus, ProjectMutation,
    ProjectReceipt, PROJECT_PERMISSIONS,
};
use archava_domain::erp::purchasing::PrincipalKind;
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::purchase_database_error;
use super::PurchaseError;

/// The ledger label on database errors.
pub const PROJECTS_LEDGER: &str = "projects";

/// The `archava_projects` namespace — checked constants only.
pub const PROJECTS_NAMESPACE: &str = "archava_projects";

fn refuses(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(PROJECTS_LEDGER, error)
}

fn conflict() -> PurchaseError {
    PurchaseError::Domain(ErpError::Conflict)
}

fn forbidden() -> PurchaseError {
    PurchaseError::Domain(ErpError::Forbidden)
}

fn not_found() -> PurchaseError {
    PurchaseError::Domain(ErpError::NotFound)
}

fn parse_uuid_value(value: &str) -> Result<Uuid, PurchaseError> {
    Uuid::parse_str(value).map_err(|_| conflict())
}

fn int4(value: i64) -> Result<i32, PurchaseError> {
    if value < 1 || value > i32::MAX as i64 {
        return Err(conflict());
    }
    Ok(value as i32)
}

/// The committed instant every receipt stamps (the same to_char render the
/// R04 adapters share).
async fn committed_at(connection: &mut PgConnection) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT to_char(clock_timestamp() at time zone 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"')",
    )
    .fetch_one(&mut *connection)
    .await
    .map(|(text,)| text)
    .map_err(refuses)
}

// --- The context ladder ------------------------------------------------------

/// The membership ladder for the ONE projects ledger.
pub async fn resolve_projects_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<ProjectAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        PROJECTS_NAMESPACE,
        PROJECTS_LEDGER,
        tenant,
        &principal.id,
        PROJECT_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
        return Err(forbidden());
    }
    Ok(match principal.kind {
        PrincipalKind::Human => ProjectAccess {
            permissions: own.permissions.clone(),
            owner_id: principal.id.clone(),
            scope: own.scope,
        },
        PrincipalKind::Service => {
            let on_behalf_of = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
            let human: MembershipRow = load_locked_membership(
                connection,
                PROJECTS_NAMESPACE,
                PROJECTS_LEDGER,
                tenant,
                on_behalf_of,
                PROJECT_PERMISSIONS,
            )
            .await?
            .ok_or_else(forbidden)?;
            if human.kind != ErpMembershipKind::Human || !human.active {
                return Err(forbidden());
            }
            ProjectAccess {
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

// --- Loads and gates ---------------------------------------------------------

/// `ownedProject()` — the assignment-scoped read (`FOR UPDATE` when a
/// mutation follows, `FOR SHARE` for recovery).
async fn owned_project(
    connection: &mut PgConnection,
    tenant: &str,
    access: &ProjectAccess,
    id: Uuid,
    lock: bool,
) -> Result<ProjectRow, PurchaseError> {
    let lock_clause = if lock { "FOR UPDATE" } else { "FOR SHARE" };
    // Only the two known lock clauses ever reach this statement — the
    // checked-allow pattern the crate pins for interpolated SQL.
    let statement = format!(
        "SELECT owner_id,state,revision FROM archava_projects.projects \
         WHERE tenant_id=$1 AND id=$2 {lock_clause}"
    );
    let (owner_id, state, revision_i32): (String, String, i32) =
        sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
            .bind(tenant)
            .bind(id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            .ok_or_else(not_found)?;
    if !access.sees(&owner_id) {
        // `assigned` scope hides other owners' records exactly like the
        // TS's `($3::boolean OR owner_id=$4)` filter clause.
        return Err(not_found());
    }
    Ok(ProjectRow {
        id,
        owner_id,
        state,
        revision: i64::from(revision_i32),
    })
}

/// The loaded project row — the field set the gates read.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ProjectRow {
    id: Uuid,
    owner_id: String,
    state: String,
    revision: i64,
}

/// The arms' revision/ceiling gate —
/// `record.revision !== command.projectRevision || record.revision >= 2147483647`.
fn project_revision_gate(row: &ProjectRow, expected: i64) -> Result<(), PurchaseError> {
    if row.revision != expected || row.revision >= 2_147_483_647 {
        return Err(conflict());
    }
    Ok(())
}

/// The audit row — `(tenant, request, actor, operation, project_id,
/// resource_id, revision, project_revision, reason)`.
#[allow(clippy::too_many_arguments)]
async fn insert_project_audit(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    operation: &str,
    project_id: Uuid,
    resource_id: Uuid,
    revision: i64,
    project_revision: i64,
    reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_projects.audit(tenant_id,request_id,actor_id,operation,project_id,resource_id,revision,project_revision,reason) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(project_id)
    .bind(resource_id)
    .bind(int4(revision)?)
    .bind(int4(project_revision)?)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// Bumps the project revision and stamps `updated_at` (the trailing write
/// every non-create arm still runs).
async fn bump_project(
    connection: &mut PgConnection,
    tenant: &str,
    project_id: Uuid,
) -> Result<i64, PurchaseError> {
    let (revision_i32,): (i32,) = sqlx::query_as(
        "UPDATE archava_projects.projects SET revision=revision+1, updated_at=now() \
         WHERE tenant_id=$1 AND id=$2 RETURNING revision",
    )
    .bind(tenant)
    .bind(project_id)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(i64::from(revision_i32))
}

// --- The claim + record pipeline ---------------------------------------------

async fn claim_project_command(
    connection: &mut PgConnection,
    tenant: &str,
    request_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<bool, PurchaseError> {
    let claimed = sqlx::query(
        "INSERT INTO archava_projects.commands(tenant_id,request_id,actor_id,fingerprint) \
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
        "SELECT actor_id,fingerprint,receipt FROM archava_projects.commands \
         WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

/// The project mutation pipeline — the port of `mutate()`. The host hands
/// over the ALREADY STRICT-PARSED mutation (its boundary parse answers
/// 400), so the fingerprint and the effect read one document.
pub async fn record_project_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &ProjectAccess,
    fingerprint: &str,
    mutation: &ProjectMutation,
) -> Result<ProjectReceipt, PurchaseError> {
    if !access.holds(&["projects.read"]) {
        return Err(forbidden());
    }
    if !access.holds(&[mutation.command.kind().permission()]) {
        return Err(forbidden());
    }
    for floor in mutation.command.kind().read_floors() {
        if !access.holds(&[floor]) {
            return Err(forbidden());
        }
    }
    let request_id = parse_uuid_value(&mutation.request_id)?;
    let actor_id = principal.id.as_str();

    let claimed =
        claim_project_command(connection, tenant, request_id, actor_id, fingerprint).await?;
    if !claimed {
        let (row_actor, row_fingerprint, row_receipt) =
            fetch_command_row(connection, tenant, request_id)
                .await?
                .ok_or_else(conflict)?;
        if row_fingerprint != fingerprint || row_actor != actor_id {
            return Err(conflict());
        }
        let receipt_value = row_receipt.ok_or_else(conflict)?;
        let stored = ProjectReceipt::parse(&receipt_value).map_err(|_| conflict())?;
        // Recovery: the recorded project re-reads FOR SHARE under the
        // CURRENT scope (the TS receiptAccess re-run) — revoked grants take
        // the recoverability with them.
        let project_id = parse_uuid_value(&stored.project_id)?;
        owned_project(connection, tenant, access, project_id, false).await?;
        return Ok(replayed_receipt(&stored));
    }

    let now = committed_at(connection).await?;
    let kind = mutation.command.kind();
    let (receipt, reason) =
        execute_project_arm(connection, tenant, actor_id, access, mutation, &now).await?;
    let receipt_value = serde_json::to_value(&receipt).map_err(|_| conflict())?;
    let project_id = parse_uuid_value(&receipt.project_id)?;
    let resource_id = parse_uuid_value(&receipt.id)?;
    insert_project_audit(
        connection,
        tenant,
        request_id,
        actor_id,
        kind.as_str(),
        project_id,
        resource_id,
        receipt.revision,
        receipt.project_revision,
        reason.as_deref(),
    )
    .await?;
    sqlx::query(
        "UPDATE archava_projects.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant)
    .bind(request_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

/// `commandStatus()` — the discovered receipt re-reads FOR SHARE only while
/// the CURRENT scope still sees the recorded project.
pub async fn project_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &ProjectAccess,
    request_id: Uuid,
) -> Result<ProjectCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    if !access.holds(&["projects.read"]) {
        return Err(forbidden());
    }
    let Some((row_actor, _, Some(receipt_value))) =
        fetch_command_row(connection, tenant, request_id).await?
    else {
        return Ok(ProjectCommandStatus::NotCommitted {
            request_id: request_id.to_string(),
        });
    };
    if row_actor != principal.id {
        return Err(forbidden());
    }
    let stored = ProjectReceipt::parse(&receipt_value).map_err(|_| conflict())?;
    let project_id = parse_uuid_value(&stored.project_id)?;
    owned_project(connection, tenant, access, project_id, false).await?;
    Ok(ProjectCommandStatus::Committed {
        request_id: request_id.to_string(),
        receipt: stored,
    })
}

// --- execute(): the six arms --------------------------------------------------

#[allow(clippy::too_many_arguments)]
async fn execute_project_arm(
    connection: &mut PgConnection,
    tenant: &str,
    actor_id: &str,
    access: &ProjectAccess,
    mutation: &ProjectMutation,
    _now: &str,
) -> Result<(ProjectReceipt, Option<String>), PurchaseError> {
    match &mutation.command {
        ProjectCommand::ProjectCreate {
            title,
            scope_summary,
            due_at,
        } => {
            let id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_projects.projects(\
                    tenant_id,id,owner_id,title,scope_summary,due_at) \
                 VALUES($1,$2,$3,$4,$5,$6::timestamptz)",
            )
            .bind(tenant)
            .bind(id)
            .bind(&access.owner_id)
            .bind(title)
            .bind(scope_summary)
            .bind(due_at)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((
                ProjectReceipt {
                    kind: "project".to_string(),
                    id: id.to_string(),
                    project_id: id.to_string(),
                    revision: 1,
                    project_revision: 1,
                    replayed: false,
                },
                None,
            ))
        }
        ProjectCommand::ProjectTransition {
            project_id,
            project_revision,
            state,
            reason,
        } => {
            let project_id = parse_uuid_value(project_id)?;
            let row = owned_project(connection, tenant, access, project_id, true).await?;
            project_revision_gate(&row, *project_revision)?;
            // The frozen state table — an out-of-table move is a conflict.
            if !archava_domain::erp::projects::project_transition_allowed(&row.state, state) {
                return Err(conflict());
            }
            // DELIVERY gate: required tasks exist, none pending, no pending
            // milestone — the TS count triple re-read.
            if state == "delivered" {
                for floor in mutation.command.kind().delivery_floors(state) {
                    if !access.holds(&[floor]) {
                        return Err(forbidden());
                    }
                }
                let (required, pending, milestones): (i32, i32, i32) = sqlx::query_as(
                    "SELECT \
                        (SELECT count(*)::integer FROM archava_projects.tasks WHERE tenant_id=$1 \
                            AND project_id=$2 AND required), \
                        (SELECT count(*)::integer FROM archava_projects.tasks WHERE tenant_id=$1 \
                            AND project_id=$2 AND required AND state<>'done'), \
                        (SELECT count(*)::integer FROM archava_projects.milestones WHERE tenant_id=$1 \
                            AND project_id=$2 AND required AND state<>'reached')",
                )
                .bind(tenant)
                .bind(project_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(refuses)?;
                if required == 0 || pending > 0 || milestones > 0 {
                    return Err(conflict());
                }
            }
            let next_revision = row.revision + 1;
            sqlx::query(
                "UPDATE archava_projects.projects SET state=$3, \
                    scope_confirmed_at=CASE WHEN $3='active' \
                        THEN COALESCE(scope_confirmed_at, now()) ELSE scope_confirmed_at END, \
                    scope_confirmed_by=CASE WHEN $3='active' \
                        THEN COALESCE(scope_confirmed_by, $4) ELSE scope_confirmed_by END \
                    WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(project_id)
            .bind(state)
            .bind(actor_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((
                ProjectReceipt {
                    kind: "project".to_string(),
                    id: project_id.to_string(),
                    project_id: project_id.to_string(),
                    revision: next_revision,
                    project_revision: next_revision,
                    replayed: false,
                },
                reason.clone(),
            ))
        }
        ProjectCommand::TaskCreate {
            project_id,
            project_revision,
            title,
            milestone_id,
            required,
            due_at,
        } => {
            let project_id = parse_uuid_value(project_id)?;
            let row = owned_project(connection, tenant, access, project_id, true).await?;
            project_revision_gate(&row, *project_revision)?;
            writable_project_gate(&row)?;
            let count = work_item_count(connection, tenant, project_id, "tasks").await?;
            if count >= 100 {
                return Err(conflict());
            }
            let milestone_bound = matches!(milestone_id, Some(Some(_)));
            for floor in mutation
                .command
                .kind()
                .milestone_binding_floor(milestone_bound)
            {
                if !access.holds(&[floor]) {
                    return Err(forbidden());
                }
            }
            let milestone = match milestone_id {
                Some(Some(id)) => Some(parse_uuid_value(id)?),
                _ => None,
            };
            if let Some(milestone_id) = milestone {
                let (state,): (String,) = sqlx::query_as(
                    "SELECT state FROM archava_projects.milestones WHERE tenant_id=$1 \
                     AND project_id=$2 AND id=$3",
                )
                .bind(tenant)
                .bind(project_id)
                .bind(milestone_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(refuses)?
                .ok_or_else(not_found)?;
                if state != "open" {
                    return Err(conflict());
                }
            }
            let task_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_projects.tasks(\
                    tenant_id,id,project_id,milestone_id,title,required,due_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7::timestamptz)",
            )
            .bind(tenant)
            .bind(task_id)
            .bind(project_id)
            .bind(milestone)
            .bind(title)
            .bind(required)
            .bind(due_at)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let project_revision_next = bump_project(connection, tenant, project_id).await?;
            Ok((
                ProjectReceipt {
                    kind: "task".to_string(),
                    id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    revision: 1,
                    project_revision: project_revision_next,
                    replayed: false,
                },
                None,
            ))
        }
        ProjectCommand::MilestoneCreate {
            project_id,
            project_revision,
            title,
            required,
            due_at,
        } => {
            let project_id = parse_uuid_value(project_id)?;
            let row = owned_project(connection, tenant, access, project_id, true).await?;
            project_revision_gate(&row, *project_revision)?;
            writable_project_gate(&row)?;
            let count = work_item_count(connection, tenant, project_id, "milestones").await?;
            if count >= 100 {
                return Err(conflict());
            }
            let milestone_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_projects.milestones(\
                    tenant_id,id,project_id,title,required,due_at) \
                 VALUES($1,$2,$3,$4,$5,$6::timestamptz)",
            )
            .bind(tenant)
            .bind(milestone_id)
            .bind(project_id)
            .bind(title)
            .bind(required)
            .bind(due_at)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let project_revision_next = bump_project(connection, tenant, project_id).await?;
            Ok((
                ProjectReceipt {
                    kind: "milestone".to_string(),
                    id: milestone_id.to_string(),
                    project_id: project_id.to_string(),
                    revision: 1,
                    project_revision: project_revision_next,
                    replayed: false,
                },
                None,
            ))
        }
        ProjectCommand::TaskTransition {
            project_id,
            project_revision,
            id,
            expected_revision,
            state,
            reason,
        } => {
            let project_id = parse_uuid_value(project_id)?;
            let task_id = parse_uuid_value(id)?;
            let row = owned_project(connection, tenant, access, project_id, true).await?;
            project_revision_gate(&row, *project_revision)?;
            finished_project_gate(&row)?;
            let (task_state, task_revision, milestone): (String, i32, Option<Uuid>) =
                sqlx::query_as(
                    "SELECT state,revision,milestone_id FROM archava_projects.tasks \
                 WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
                )
                .bind(tenant)
                .bind(project_id)
                .bind(task_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(refuses)?
                .ok_or_else(not_found)?;
            let task_revision = i64::from(task_revision);
            if task_revision != *expected_revision
                || task_revision >= 2_147_483_647
                || !archava_domain::erp::projects::task_transition_allowed(&task_state, state)
            {
                return Err(conflict());
            }
            // Work under a REACHED milestone may not reopen (a non-done
            // target under a reached parent conflicts).
            if let Some(milestone_id) = milestone {
                if state != "done" {
                    let milestone_state = sqlx::query_as::<_, (String,)>(
                        "SELECT state FROM archava_projects.milestones WHERE tenant_id=$1 \
                         AND project_id=$2 AND id=$3",
                    )
                    .bind(tenant)
                    .bind(project_id)
                    .bind(milestone_id)
                    .fetch_optional(&mut *connection)
                    .await
                    .map_err(refuses)?;
                    if let Some((state,)) = milestone_state {
                        if state == "reached" {
                            return Err(conflict());
                        }
                    }
                }
            }
            sqlx::query(
                "UPDATE archava_projects.tasks SET state=$4, revision=revision+1, updated_at=now() \
                 WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
            )
            .bind(tenant)
            .bind(project_id)
            .bind(task_id)
            .bind(state)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let project_revision_next = bump_project(connection, tenant, project_id).await?;
            Ok((
                ProjectReceipt {
                    kind: "task".to_string(),
                    id: task_id.to_string(),
                    project_id: project_id.to_string(),
                    revision: task_revision + 1,
                    project_revision: project_revision_next,
                    replayed: false,
                },
                reason.clone(),
            ))
        }
        ProjectCommand::MilestoneComplete {
            project_id,
            project_revision,
            id,
            expected_revision,
            evidence,
        } => {
            let project_id = parse_uuid_value(project_id)?;
            let milestone_id = parse_uuid_value(id)?;
            let row = owned_project(connection, tenant, access, project_id, true).await?;
            project_revision_gate(&row, *project_revision)?;
            finished_project_gate(&row)?;
            let (milestone_state, milestone_revision): (String, i32) = sqlx::query_as(
                "SELECT state,revision FROM archava_projects.milestones \
                 WHERE tenant_id=$1 AND project_id=$2 AND id=$3 FOR UPDATE",
            )
            .bind(tenant)
            .bind(project_id)
            .bind(milestone_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            .ok_or_else(not_found)?;
            let milestone_revision = i64::from(milestone_revision);
            if milestone_revision != *expected_revision
                || milestone_revision >= 2_147_483_647
                || milestone_state != "open"
            {
                return Err(conflict());
            }
            let (total, pending): (i64, i64) = sqlx::query_as(
                "SELECT count(*), count(*) FILTER (WHERE required AND state<>'done') \
                 FROM archava_projects.tasks WHERE tenant_id=$1 AND project_id=$2 AND milestone_id=$3",
            )
            .bind(tenant)
            .bind(project_id)
            .bind(milestone_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(refuses)?;
            if total == 0 || pending > 0 {
                return Err(conflict());
            }
            sqlx::query(
                "UPDATE archava_projects.milestones SET state='reached', revision=revision+1, \
                 reached_at=now(), evidence=$4, updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
            )
            .bind(tenant)
            .bind(project_id)
            .bind(milestone_id)
            .bind(evidence)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let project_revision_next = bump_project(connection, tenant, project_id).await?;
            Ok((
                ProjectReceipt {
                    kind: "milestone".to_string(),
                    id: milestone_id.to_string(),
                    project_id: project_id.to_string(),
                    revision: milestone_revision + 1,
                    project_revision: project_revision_next,
                    replayed: false,
                },
                Some(evidence.clone()),
            ))
        }
    }
}

/// `['delivered','closed','cancelled'].includes(state)` — no work left.
fn writable_project_gate(row: &ProjectRow) -> Result<(), PurchaseError> {
    if ["delivered", "closed", "cancelled"].contains(&row.state.as_str()) {
        return Err(conflict());
    }
    Ok(())
}

/// Same list as [`writable_project_gate`], the TS's longer messaging path.
fn finished_project_gate(row: &ProjectRow) -> Result<(), PurchaseError> {
    writable_project_gate(row)
}

async fn work_item_count(
    connection: &mut PgConnection,
    tenant: &str,
    project_id: Uuid,
    table: &str,
) -> Result<i64, PurchaseError> {
    // Only two literals ever reach this — the checked-allow pattern the
    // crate pins for interpolated statements.
    if !matches!(table, "tasks" | "milestones") {
        return Err(conflict());
    }
    let statement = format!(
        "SELECT count(*) FROM archava_projects.{table} WHERE tenant_id=$1 AND project_id=$2"
    );
    let (count,): (i64,) = sqlx::query_as(sqlx::AssertSqlSafe(statement.as_str()))
        .bind(tenant)
        .bind(project_id)
        .fetch_one(&mut *connection)
        .await
        .map_err(refuses)?;
    Ok(count)
}
