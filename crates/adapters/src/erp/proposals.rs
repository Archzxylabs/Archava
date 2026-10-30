//! The proposals SQL adapter — the Archxy 2 port of
//! `packages/proposals-pg/src/repository.ts` (the R04 proposal,
//! scope-version and change-request surface), executed strictly inside the
//! shared RLS transaction cohort.
//!
//! ## Statement order (the TS port)
//!
//! `mutate()` runs: membership ladder (one ledger), the human-only gate,
//! `requiredProposalPermission`, the `commands` claim (same actor +
//! fingerprint else conflict), then the arm's effect, its audit row and the
//! receipt binding. The arm's effects:
//!
//! - `create_proposal` seeds draft v1 AND its `scope_items`.
//! - `update_proposal` edits DRAFT titles/summaries/leads — anything else
//!   is `scope_frozen` (a conflict-class word here).
//! - `set_scope_items` rewrites v1 and recomputes the total.
//! - `submit_proposal` walks `draft → submitted`.
//! - `accept_proposal` walks `submitted → accepted`, stamps
//!   accepted_at/by, binds `linkedProjectId`, and SNAPSHOTs v1 into
//!   `scope_versions` (the agreed-scope ledger).
//! - `reject_proposal`/`withdraw_proposal` walk the state table, recording
//!   the reason in audit.
//! - `link_project` walks `accepted` only, binding an external project.
//! - The `change_requests` arms guard the accepted-parent proposal, one
//!   active CR per proposal (`active_change_request_exists` — a conflict
//!   refusal here; the partial unique index would raise it anyway), and
//!   `approve_change_request` CLONES v{current} into v{current+1} with the
//!   amendment item `CR-N: <title>` appended, re-appends the snapshot and
//!   bumps `current_scope_version` + revisions.
//!
//! ## Error vocabulary
//!
//! `ProposalError('forbidden'|'not_found'|'conflict'|'scope_frozen'|'invalid_state'|'active_change_request_exists'|'invalid_request')`
//! rides the shared `PurchaseError`/`ErpError` carrier — all of
//! scope_frozen/invalid_state/active_change_request_exists are
//! conflict-class (409) refusals, forfeiting their word-level names like the
//! baseline's own map_purchase_result flattens the R06 classes. Replays
//! (`recover_from_row`) re-check the proposal owner for scoped callers.

use sqlx::postgres::PgConnection;
use uuid::Uuid;

use archava_domain::erp::proposals::{
    replayed_receipt, ProposalAccess, ProposalCommandStatus, ProposalData, ProposalMutation,
    ProposalReceipt, PROPOSAL_PERMISSIONS,
};
use archava_domain::erp::purchasing::PrincipalKind;
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::{ErpError, ErpMembershipKind, ErpScope};

use super::commerce_kernel::{load_locked_membership, MembershipRow};
use super::purchase_database_error;
use super::PurchaseError;

/// The ledger label on database errors.
pub const PROPOSALS_LEDGER: &str = "proposals";

/// The `archava_proposals` namespace — checked constants only.
pub const PROPOSALS_NAMESPACE: &str = "archava_proposals";

fn refuses(error: sqlx::Error) -> PurchaseError {
    purchase_database_error(PROPOSALS_LEDGER, error)
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
    if value < 0 || value > i32::MAX as i64 {
        return Err(conflict());
    }
    Ok(value as i32)
}

/// The revision columns are int4 with a CHECK>0 — conversions bound the arm
/// values against `i32::MAX` (the parse already bounds them at 2^31-1).
fn int4_positive(value: i64) -> Result<i32, PurchaseError> {
    if value < 1 || value > i32::MAX as i64 {
        return Err(conflict());
    }
    Ok(value as i32)
}

/// The committed instant — the same `to_char` render the organizations
/// adapter stamps receipts and lock rows with.
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

/// The membership ladder for the ONE proposals ledger — purchases' shape
/// with the proposals vocabulary.
pub async fn resolve_proposals_context(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
) -> Result<ProposalAccess, PurchaseError> {
    let own: MembershipRow = load_locked_membership(
        connection,
        PROPOSALS_NAMESPACE,
        PROPOSALS_LEDGER,
        tenant,
        &principal.id,
        PROPOSAL_PERMISSIONS,
    )
    .await?
    .ok_or_else(forbidden)?;
    if !own.active || own.kind.as_str() != principal.kind.as_str() {
        return Err(forbidden());
    }
    Ok(match principal.kind {
        PrincipalKind::Human => ProposalAccess {
            permissions: own.permissions.clone(),
            owner_id: principal.id.clone(),
            scope: own.scope,
        },
        PrincipalKind::Service => {
            let on_behalf_of = principal.on_behalf_of.as_deref().ok_or_else(forbidden)?;
            let human: MembershipRow = load_locked_membership(
                connection,
                PROPOSALS_NAMESPACE,
                PROPOSALS_LEDGER,
                tenant,
                on_behalf_of,
                PROPOSAL_PERMISSIONS,
            )
            .await?
            .ok_or_else(forbidden)?;
            if human.kind != ErpMembershipKind::Human || !human.active {
                return Err(forbidden());
            }
            ProposalAccess {
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

// --- The claim + audit + receipt pipeline ------------------------------------

async fn claim_proposal_command(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: Uuid,
    actor_id: &str,
    fingerprint: &str,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_proposals.commands(tenant_id,command_id,actor_id,fingerprint) \
         VALUES($1,$2,$3,$4)",
    )
    .bind(tenant)
    .bind(command_id)
    .bind(actor_id)
    .bind(fingerprint)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

async fn fetch_command_row(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: Uuid,
) -> Result<Option<(String, String, Option<serde_json::Value>)>, PurchaseError> {
    sqlx::query_as(
        "SELECT actor_id,fingerprint,receipt FROM archava_proposals.commands \
         WHERE tenant_id=$1 AND command_id=$2",
    )
    .bind(tenant)
    .bind(command_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)
}

#[allow(clippy::too_many_arguments)]
async fn insert_proposal_audit(
    connection: &mut PgConnection,
    tenant: &str,
    command_id: Uuid,
    actor_id: &str,
    operation: &str,
    proposal_id: Uuid,
    change_request_id: Option<Uuid>,
    revision: i64,
    reason: Option<&str>,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_proposals.audit(tenant_id,command_id,actor_id,operation,proposal_id,change_request_id,revision,reason) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant)
    .bind(command_id)
    .bind(actor_id)
    .bind(operation)
    .bind(proposal_id)
    .bind(change_request_id)
    .bind(int4_positive(revision)?)
    .bind(reason)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// One proposal row the arms load — the gates the TS `mutate` branches read.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ProposalRow {
    id: Uuid,
    owner_id: String,
    state: String,
    revision: i64,
    current_scope_version: i64,
}

async fn load_proposal(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<ProposalRow, PurchaseError> {
    let (owner_id, state, revision_i32, version_i32): (String, String, i32, i32) = sqlx::query_as(
        "SELECT owner_id,state,revision,current_scope_version FROM archava_proposals.proposals \
         WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?
    .ok_or_else(not_found)?;
    Ok(ProposalRow {
        id,
        owner_id,
        state,
        revision: i64::from(revision_i32),
        current_scope_version: i64::from(version_i32),
    })
}

/// The assigned-scope owner gate the TS branches run before their own state
/// gates.
fn assignment_gate(access: &ProposalAccess, row: &ProposalRow) -> Result<(), PurchaseError> {
    if !access.sees(&row.owner_id) {
        return Err(forbidden());
    }
    Ok(())
}

/// One change-request row the CR arms load.
#[derive(Debug, Clone)]
struct ChangeRequestRow {
    id: Uuid,
    proposal_id: Uuid,
    change_number: i64,
    title: String,
    state: String,
    revision: i64,
}

async fn load_change_request(
    connection: &mut PgConnection,
    tenant: &str,
    id: Uuid,
) -> Result<ChangeRequestRow, PurchaseError> {
    let (proposal_id, change_number_i32, title, state, revision_i32): (
        Uuid,
        i32,
        String,
        String,
        i32,
    ) = sqlx::query_as(
        "SELECT proposal_id,change_number,title,state,revision \
             FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
    )
    .bind(tenant)
    .bind(id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?
    .ok_or_else(not_found)?;
    Ok(ChangeRequestRow {
        id,
        proposal_id,
        change_number: i64::from(change_number_i32),
        title,
        state,
        revision: i64::from(revision_i32),
    })
}

/// The raw scope-item row the loader reads (the sqlx tuple shape, factored
/// out so `load_scope_items` stays under the type-complexity lint).
type ScopeItemRow = (Uuid, i32, String, String, Option<i32>, i64, String, String);

/// One scope item the snapshots and clones carry.
#[derive(Debug, Clone, serde::Serialize)]
struct ScopeItemDoc {
    id: String,
    #[serde(rename = "proposalId")]
    proposal_id: String,
    #[serde(rename = "scopeVersion")]
    scope_version: i64,
    title: String,
    description: String,
    #[serde(rename = "estimatedDays")]
    estimated_days: Option<i64>,
    #[serde(rename = "estimatedValue")]
    estimated_value: i64,
    #[serde(rename = "deliverableType")]
    deliverable_type: String,
    #[serde(rename = "createdAt")]
    created_at: String,
}

async fn load_scope_items(
    connection: &mut PgConnection,
    tenant: &str,
    proposal_id: Uuid,
    scope_version: i64,
) -> Result<Vec<ScopeItemDoc>, PurchaseError> {
    let rows: Vec<ScopeItemRow> = sqlx::query_as(
        "SELECT id,scope_version,title,description,estimated_days,estimated_value,deliverable_type, \
         to_char(created_at at time zone 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.MS\"Z\"') \
         FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 \
         AND scope_version=$3 ORDER BY created_at ASC",
    )
    .bind(tenant)
    .bind(proposal_id)
    .bind(int4(scope_version)?)
    .fetch_all(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(rows
        .into_iter()
        .map(
            |(id, version, title, description, days, value, deliverable, at)| ScopeItemDoc {
                id: id.to_string(),
                proposal_id: proposal_id.to_string(),
                scope_version: i64::from(version),
                title,
                description,
                estimated_days: days.map(i64::from),
                estimated_value: value,
                deliverable_type: deliverable,
                created_at: at,
            },
        )
        .collect())
}

/// Inserts one scope item row — the shared shape the create / rewrite /
/// clone branches all bind.
#[allow(clippy::too_many_arguments)]
async fn insert_scope_item(
    connection: &mut PgConnection,
    tenant: &str,
    proposal_id: Uuid,
    scope_version: i64,
    title: &str,
    description: &str,
    estimated_days: Option<i64>,
    estimated_value: i64,
    deliverable_type: &str,
) -> Result<(), PurchaseError> {
    sqlx::query(
        "INSERT INTO archava_proposals.scope_items(\
            tenant_id,id,proposal_id,scope_version,title,description,estimated_days,estimated_value,deliverable_type) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9)",
    )
    .bind(tenant)
    .bind(Uuid::new_v4())
    .bind(proposal_id)
    .bind(int4(scope_version)?)
    .bind(title)
    .bind(description)
    .bind(estimated_days.map(int4).transpose()?)
    .bind(estimated_value)
    .bind(deliverable_type)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(())
}

/// The proposal mutation pipeline — the port of `mutate()`. The access
/// ladder (resolvers above) runs first in the caller's transaction; the
/// human-only kind gate is both a route and TS rule; every refusal rolls
/// the whole transaction back through the host. The host hands over the
/// ALREADY STRICT-PARSED mutation (its boundary parse answers 400), so the
/// fingerprint and the effect read one document.
pub async fn record_proposal_command(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &ProposalAccess,
    fingerprint: &str,
    mutation: &ProposalMutation,
) -> Result<ProposalReceipt, PurchaseError> {
    if !access.holds(&[mutation.data.kind().permission()]) {
        return Err(forbidden());
    }
    let command_id = parse_uuid_value(mutation.data.command_id())?;
    let actor_id = principal.id.as_str();

    // The TS reads the EXISTING row FOR UPDATE before the claim: the same
    // actor + fingerprint reuses (replay or committed receipt), a different
    // one conflicts, and a receipt-less row conflicts too (a concurrent
    // uncommitted claim is not a replay).
    let existing = fetch_command_row(connection, tenant, command_id).await?;
    match &existing {
        None => {
            claim_proposal_command(connection, tenant, command_id, actor_id, fingerprint).await?
        }
        Some((row_actor, row_fingerprint, row_receipt)) => {
            if row_fingerprint != fingerprint || row_actor != actor_id {
                return Err(conflict());
            }
            match row_receipt {
                Some(receipt_value) => {
                    let stored = ProposalReceipt::parse(receipt_value).map_err(|_| conflict())?;
                    // Recovery re-authorization: scoped callers replay only
                    // receipts whose proposal is STILL theirs to see.
                    if let Some(proposal_id) = stored.proposal_id.as_deref() {
                        if let Ok(proposal_id) = parse_uuid_value(proposal_id) {
                            if let Some((owner_id,)) = sqlx::query_as::<_, (String,)>(
                                "SELECT owner_id FROM archava_proposals.proposals \
                                 WHERE tenant_id=$1 AND id=$2",
                            )
                            .bind(tenant)
                            .bind(proposal_id)
                            .fetch_optional(&mut *connection)
                            .await
                            .map_err(refuses)?
                            {
                                if !access.sees(&owner_id) {
                                    return Err(forbidden());
                                }
                            }
                        }
                    }
                    return Ok(replayed_receipt(&stored));
                }
                None => return Err(conflict()),
            }
        }
    }

    let now = committed_at(connection).await?;
    let kind = mutation.data.kind();
    let outcome = execute_proposal_arm(connection, tenant, actor_id, access, mutation).await?;
    let (proposal_id, change_request_id, revision, reason, scope_version) = outcome;
    let receipt = ProposalReceipt {
        command_id: mutation.data.command_id().to_string(),
        committed_at: now,
        proposal_id: proposal_id.as_ref().map(Uuid::to_string),
        change_request_id: change_request_id.as_ref().map(Uuid::to_string),
        scope_version,
        revision,
        status: "committed".to_string(),
        replayed: false,
    };
    let receipt_value = serde_json::to_value(&receipt).map_err(|_| conflict())?;
    insert_proposal_audit(
        connection,
        tenant,
        command_id,
        actor_id,
        kind.as_str(),
        proposal_id.unwrap_or_else(Uuid::nil),
        change_request_id,
        revision,
        reason.as_deref(),
    )
    .await?;
    sqlx::query(
        "UPDATE archava_proposals.commands SET receipt=$3 WHERE tenant_id=$1 AND command_id=$2",
    )
    .bind(tenant)
    .bind(command_id)
    .bind(&receipt_value)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok(receipt)
}

/// `commandStatus()` — the discovered receipt replays only while the
/// CURRENT access still authorizes the recorded proposal.
pub async fn proposal_command_status(
    connection: &mut PgConnection,
    tenant: &str,
    principal: &PurchasePrincipal,
    access: &ProposalAccess,
    command_id: Uuid,
) -> Result<ProposalCommandStatus, PurchaseError> {
    if principal.kind.as_str() != "human" {
        return Err(forbidden());
    }
    if !access.holds(&["proposals.read"]) {
        return Err(forbidden());
    }
    let Some((row_actor, _, Some(receipt_value))) =
        fetch_command_row(connection, tenant, command_id).await?
    else {
        return Ok(ProposalCommandStatus::Unknown);
    };
    // A foreign principal cannot tell a receipt that is not theirs from a
    // command id that was never written (`unknown` both ways, no oracle).
    if row_actor != principal.id {
        return Ok(ProposalCommandStatus::Unknown);
    }
    let stored = ProposalReceipt::parse(&receipt_value).map_err(|_| conflict())?;
    if let Some(proposal_id) = stored.proposal_id.as_deref() {
        if let Ok(proposal_id) = parse_uuid_value(proposal_id) {
            if let Some((owner_id,)) = sqlx::query_as::<_, (String,)>(
                "SELECT owner_id FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant)
            .bind(proposal_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?
            {
                if !access.sees(&owner_id) {
                    return Err(forbidden());
                }
            }
        }
    }
    Ok(ProposalCommandStatus::Committed { receipt: stored })
}

// --- execute(): the fourteen arms --------------------------------------------

/// One arm's outcome: `(proposal_id, change_request_id, receipt revision,
/// audit reason, scope version)` — everything the audit row and receipt
/// record.
type ProposalOutcome = (Option<Uuid>, Option<Uuid>, i64, Option<String>, Option<i64>);

/// The `Value`-shaped nullable-string cells (reason, links) unwrap here: an
/// explicit null collapses to the `None` the columns keep.
fn value_option(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::String(text) => Some(text.clone()),
        _ => None,
    }
}

/// The revision gate — `Number(row.revision) !== expected`.
fn revision_gate(row_revision: i64, expected: i64) -> Result<(), PurchaseError> {
    if row_revision != expected {
        return Err(conflict());
    }
    Ok(())
}

/// The `state_frozen` gate — an edit arm inside a non-draft proposal.
fn frozen_gate(state: &str) -> Result<(), PurchaseError> {
    if state != "draft" {
        return Err(conflict());
    }
    Ok(())
}

/// The `invalid_state` gate — the wrong state for the arm.
fn state_gate(state: &str, expected: &str) -> Result<(), PurchaseError> {
    if state != expected {
        return Err(conflict());
    }
    Ok(())
}

/// The state-membership gate — `!['draft','submitted'].includes(state)`.
fn state_in_gate(state: &str, expected: &[&str]) -> Result<(), PurchaseError> {
    if !expected.contains(&state) {
        return Err(conflict());
    }
    Ok(())
}

/// The assignment gate through the proposal row a CR arm re-derives.
async fn cr_assignment_gate(
    connection: &mut PgConnection,
    tenant: &str,
    access: &ProposalAccess,
    proposal_id: Uuid,
) -> Result<(), PurchaseError> {
    if access.scope.covers_all() {
        return Ok(());
    }
    let (owner_id,): (String,) = sqlx::query_as(
        "SELECT owner_id FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(proposal_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?
    .ok_or_else(not_found)?;
    if !access.sees(&owner_id) {
        return Err(forbidden());
    }
    Ok(())
}

/// The fourteen executed arms, in the TS branch order.
#[allow(clippy::too_many_arguments)]
async fn execute_proposal_arm(
    connection: &mut PgConnection,
    tenant: &str,
    actor_id: &str,
    access: &ProposalAccess,
    mutation: &ProposalMutation,
) -> Result<ProposalOutcome, PurchaseError> {
    match &mutation.data {
        ProposalData::CreateProposal {
            title,
            summary,
            organization_id,
            contact_id,
            lead_id,
            currency,
            scope_items,
            ..
        } => {
            let id = Uuid::new_v4();
            // `?? null` on every optional link cell.
            let organization_id = match organization_id {
                serde_json::Value::Null => None,
                serde_json::Value::String(raw) => Some(parse_uuid_value(raw)?),
                _ => None,
            };
            let contact_id = value_option(contact_id);
            let lead_id = value_option(lead_id);
            let total: i64 = scope_items.iter().map(|item| item.estimated_value).sum();
            sqlx::query(
                "INSERT INTO archava_proposals.proposals(\
                    tenant_id,id,title,summary,organization_id,contact_id,lead_id,currency, \
                    total_estimated_value,state,current_scope_version,owner_id,revision,created_at,updated_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'draft',1,$10,1,clock_timestamp(),clock_timestamp())",
            )
            .bind(tenant)
            .bind(id)
            .bind(title)
            .bind(summary)
            .bind(organization_id)
            .bind(contact_id)
            .bind(lead_id)
            .bind(currency)
            .bind(total)
            .bind(actor_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            for item in scope_items {
                insert_scope_item(
                    connection,
                    tenant,
                    id,
                    1,
                    &item.title,
                    &item.description,
                    item.estimated_days,
                    item.estimated_value,
                    &item.deliverable_type,
                )
                .await?;
            }
            Ok((Some(id), None, 1, None, Some(1)))
        }
        ProposalData::UpdateProposal {
            proposal_id,
            expected_revision,
            title,
            summary,
            organization_id,
            contact_id,
            lead_id,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            frozen_gate(&row.state)?;
            revision_gate(row.revision, *expected_revision)?;
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.proposals SET \
                    title=$1, summary=$2, \
                    organization_id=COALESCE($3, organization_id), \
                    contact_id=COALESCE($4, contact_id), \
                    lead_id=COALESCE($5, lead_id), \
                    revision=$6, updated_at=clock_timestamp() \
                  WHERE tenant_id=$7 AND id=$8",
            )
            .bind(title)
            .bind(summary)
            .bind(match organization_id {
                Some(serde_json::Value::String(raw)) => Some(parse_uuid_value(raw)?),
                _ => None,
            })
            .bind(match contact_id {
                Some(value) => value_option(value),
                None => None,
            })
            .bind(match lead_id {
                Some(value) => value_option(value),
                None => None,
            })
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((Some(proposal_id), None, next_revision, None, None))
        }
        ProposalData::SetScopeItems {
            proposal_id,
            expected_revision,
            scope_items,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            frozen_gate(&row.state)?;
            revision_gate(row.revision, *expected_revision)?;
            sqlx::query(
                "DELETE FROM archava_proposals.scope_items WHERE tenant_id=$1 AND proposal_id=$2 AND scope_version=1",
            )
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let total: i64 = scope_items.iter().map(|item| item.estimated_value).sum();
            for item in scope_items {
                insert_scope_item(
                    connection,
                    tenant,
                    proposal_id,
                    1,
                    &item.title,
                    &item.description,
                    item.estimated_days,
                    item.estimated_value,
                    &item.deliverable_type,
                )
                .await?;
            }
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.proposals SET total_estimated_value=$1, revision=$2, \
                 updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4",
            )
            .bind(total)
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((Some(proposal_id), None, next_revision, None, Some(1)))
        }
        ProposalData::SubmitProposal {
            proposal_id,
            expected_revision,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            state_gate(&row.state, "draft")?;
            revision_gate(row.revision, *expected_revision)?;
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.proposals SET state='submitted', revision=$1, \
                 updated_at=clock_timestamp() WHERE tenant_id=$2 AND id=$3",
            )
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((Some(proposal_id), None, next_revision, None, None))
        }
        ProposalData::AcceptProposal {
            proposal_id,
            expected_revision,
            linked_project_id,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            state_gate(&row.state, "submitted")?;
            revision_gate(row.revision, *expected_revision)?;
            // Freeze the v1 scope snapshot beside the decision.
            let items = load_scope_items(connection, tenant, proposal_id, 1).await?;
            let total_value: i64 = items.iter().map(|item| item.estimated_value).sum();
            let total_days: i64 = items
                .iter()
                .map(|item| item.estimated_days.unwrap_or(0))
                .sum();
            let summary = row_summary(connection, tenant, proposal_id).await?;
            let linked_project_id = match linked_project_id {
                serde_json::Value::Null => None,
                serde_json::Value::String(raw) => Some(parse_uuid_value(raw)?),
                _ => None,
            };
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.proposals SET state='accepted', \
                 accepted_at=clock_timestamp(), accepted_by=$1, linked_project_id=$2, revision=$3, \
                 updated_at=clock_timestamp() WHERE tenant_id=$4 AND id=$5",
            )
            .bind(actor_id)
            .bind(linked_project_id)
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            let items_json = serde_json::to_value(&items).map_err(|_| conflict())?;
            sqlx::query(
                "INSERT INTO archava_proposals.scope_versions(\
                    tenant_id,id,proposal_id,scope_version,change_request_id,summary, \
                    total_value,total_days,confirmed_at,confirmed_by,items) \
                 VALUES($1,$2,$3,1,NULL,$4,$5,$6,clock_timestamp(),$7,$8)",
            )
            .bind(tenant)
            .bind(Uuid::new_v4())
            .bind(proposal_id)
            .bind(summary)
            .bind(total_value)
            .bind(total_days)
            .bind(actor_id)
            .bind(&items_json)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((Some(proposal_id), None, next_revision, None, Some(1)))
        }
        ProposalData::RejectProposal {
            proposal_id,
            expected_revision,
            reason,
            ..
        }
        | ProposalData::WithdrawProposal {
            proposal_id,
            expected_revision,
            reason,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            match &mutation.data {
                ProposalData::RejectProposal { .. } => state_gate(&row.state, "submitted")?,
                ProposalData::WithdrawProposal { .. } => {
                    state_in_gate(&row.state, &["draft", "submitted"])?
                }
                _ => unreachable!("matched by the destructuring above"),
            }
            revision_gate(row.revision, *expected_revision)?;
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.proposals SET state=$1, revision=$2, \
                 updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4",
            )
            .bind(
                mutation
                    .data
                    .kind()
                    .as_str()
                    .split('_')
                    .next()
                    .map(|word| match word {
                        "reject" => "rejected",
                        _ => "withdrawn",
                    }),
            )
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((
                Some(proposal_id),
                None,
                next_revision,
                value_option(reason),
                None,
            ))
        }
        ProposalData::LinkProject {
            proposal_id,
            expected_revision,
            project_id,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let project_id = parse_uuid_value(project_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            state_gate(&row.state, "accepted")?;
            revision_gate(row.revision, *expected_revision)?;
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.proposals SET linked_project_id=$1, revision=$2, \
                 updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4",
            )
            .bind(project_id)
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(proposal_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((Some(proposal_id), None, next_revision, None, None))
        }
        ProposalData::CreateChangeRequest {
            proposal_id,
            title,
            reason,
            delta_summary,
            time_delta_days,
            cost_delta_value,
            ..
        } => {
            let proposal_id = parse_uuid_value(proposal_id)?;
            let row = load_proposal(connection, tenant, proposal_id).await?;
            assignment_gate(access, &row)?;
            state_gate(&row.state, "accepted")?;
            // One open change request per proposal — the TS precheck whose
            // deterministic error the unique index would surface as SQL.
            let active = sqlx::query_as::<_, (Uuid,)>(
                "SELECT id FROM archava_proposals.change_requests WHERE tenant_id=$1 \
                 AND proposal_id=$2 AND state IN ('draft','submitted')",
            )
            .bind(tenant)
            .bind(proposal_id)
            .fetch_optional(&mut *connection)
            .await
            .map_err(refuses)?;
            if active.is_some() {
                return Err(conflict());
            }
            let (max_change,): (i32,) = sqlx::query_as(
                "SELECT COALESCE(MAX(change_number), 0) FROM archava_proposals.change_requests \
                 WHERE tenant_id=$1 AND proposal_id=$2",
            )
            .bind(tenant)
            .bind(proposal_id)
            .fetch_one(&mut *connection)
            .await
            .map_err(refuses)?;
            let change_number = i64::from(max_change) + 1;
            let change_request_id = Uuid::new_v4();
            sqlx::query(
                "INSERT INTO archava_proposals.change_requests(\
                    tenant_id,id,proposal_id,change_number,title,reason,delta_summary, \
                    time_delta_days,cost_delta_value,state,owner_id,revision,created_at,updated_at) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'draft',$10,1,clock_timestamp(),clock_timestamp())",
            )
            .bind(tenant)
            .bind(change_request_id)
            .bind(proposal_id)
            .bind(int4(change_number)?)
            .bind(title)
            .bind(reason)
            .bind(delta_summary)
            .bind(time_delta_days)
            .bind(cost_delta_value)
            .bind(actor_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((Some(proposal_id), Some(change_request_id), 1, None, None))
        }
        ProposalData::UpdateChangeRequest {
            change_request_id,
            expected_revision,
            title,
            reason,
            delta_summary,
            time_delta_days,
            cost_delta_value,
            ..
        } => {
            let change_request_id = parse_uuid_value(change_request_id)?;
            let cr = load_change_request(connection, tenant, change_request_id).await?;
            cr_assignment_gate(connection, tenant, access, cr.proposal_id).await?;
            state_gate(&cr.state, "draft")?;
            revision_gate(cr.revision, *expected_revision)?;
            let next_revision = expected_revision + 1;
            sqlx::query(
                "UPDATE archava_proposals.change_requests SET title=$1, reason=$2, \
                 delta_summary=$3, time_delta_days=$4, cost_delta_value=$5, revision=$6, \
                 updated_at=clock_timestamp() WHERE tenant_id=$7 AND id=$8",
            )
            .bind(title)
            .bind(reason)
            .bind(delta_summary)
            .bind(time_delta_days)
            .bind(cost_delta_value)
            .bind(int4_positive(next_revision)?)
            .bind(tenant)
            .bind(change_request_id)
            .execute(&mut *connection)
            .await
            .map_err(refuses)?;
            Ok((
                Some(cr.proposal_id),
                Some(change_request_id),
                next_revision,
                None,
                None,
            ))
        }
        ProposalData::SubmitChangeRequest {
            change_request_id,
            expected_revision,
            ..
        }
        | ProposalData::ApproveChangeRequest {
            change_request_id,
            expected_revision,
            ..
        }
        | ProposalData::RejectChangeRequest {
            change_request_id,
            expected_revision,
            ..
        }
        | ProposalData::WithdrawChangeRequest {
            change_request_id,
            expected_revision,
            ..
        } => {
            let change_request_id = parse_uuid_value(change_request_id)?;
            let cr = load_change_request(connection, tenant, change_request_id).await?;
            cr_assignment_gate(connection, tenant, access, cr.proposal_id).await?;
            let (next_state, decided) = match &mutation.data {
                ProposalData::SubmitChangeRequest { .. } => {
                    state_gate(&cr.state, "draft")?;
                    ("submitted", false)
                }
                ProposalData::ApproveChangeRequest { .. } => {
                    return approve_change_request(
                        connection,
                        tenant,
                        actor_id,
                        cr,
                        *expected_revision,
                    )
                    .await
                }
                ProposalData::RejectChangeRequest { .. } => {
                    state_gate(&cr.state, "submitted")?;
                    ("rejected", true)
                }
                ProposalData::WithdrawChangeRequest { .. } => {
                    state_in_gate(&cr.state, &["draft", "submitted"])?;
                    ("withdrawn", false)
                }
                _ => unreachable!("matched above"),
            };
            revision_gate(cr.revision, *expected_revision)?;
            let next_revision = expected_revision + 1;
            if decided {
                sqlx::query(
                    "UPDATE archava_proposals.change_requests SET state=$1, \
                     decided_at=clock_timestamp(), decided_by=$2, revision=$3, \
                     updated_at=clock_timestamp() WHERE tenant_id=$4 AND id=$5",
                )
                .bind(next_state)
                .bind(actor_id)
                .bind(int4_positive(next_revision)?)
                .bind(tenant)
                .bind(change_request_id)
                .execute(&mut *connection)
                .await
                .map_err(refuses)?;
            } else {
                sqlx::query(
                    "UPDATE archava_proposals.change_requests SET state=$1, \
                     submitted_at=CASE WHEN $1='submitted' THEN clock_timestamp() ELSE submitted_at END, \
                     revision=$2, updated_at=clock_timestamp() WHERE tenant_id=$3 AND id=$4",
                )
                .bind(next_state)
                .bind(int4_positive(next_revision)?)
                .bind(tenant)
                .bind(change_request_id)
                .execute(&mut *connection)
                .await
                .map_err(refuses)?;
            }
            let audit_reason = match &mutation.data {
                ProposalData::RejectChangeRequest { reason, .. }
                | ProposalData::WithdrawChangeRequest { reason, .. } => value_option(reason),
                _ => None,
            };
            Ok((
                Some(cr.proposal_id),
                Some(change_request_id),
                next_revision,
                audit_reason,
                None,
            ))
        }
    }
}

/// Reads the proposal summary the accept-arm snapshot records (the TS
/// SELCONSUME uses the UPDATE's RETURNING; this port re-reads the row).
async fn row_summary(
    connection: &mut PgConnection,
    tenant: &str,
    proposal_id: Uuid,
) -> Result<String, PurchaseError> {
    sqlx::query_as::<_, (String,)>(
        "SELECT summary FROM archava_proposals.proposals WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(proposal_id)
    .fetch_optional(&mut *connection)
    .await
    .map_err(refuses)?
    .map(|(text,)| text)
    .ok_or_else(not_found)
}

/// The approve arm — cloning v{current} into v{current+1} with the
/// amendment line appended, current_scope_version advanced, the CR decided
/// and the snapshot re-recorded.
async fn approve_change_request(
    connection: &mut PgConnection,
    tenant: &str,
    actor_id: &str,
    cr: ChangeRequestRow,
    expected_revision: i64,
) -> Result<ProposalOutcome, PurchaseError> {
    if cr.state != "submitted" {
        return Err(conflict());
    }
    revision_gate(cr.revision, expected_revision)?;
    let proposal_id = cr.proposal_id;
    let row = load_proposal(connection, tenant, proposal_id).await?;
    if row.state != "accepted" {
        return Err(conflict());
    }
    let next_scope_version = row.current_scope_version + 1;
    let next_prop_revision = row.revision + 1;
    let next_cr_revision = expected_revision + 1;

    let current_items =
        load_scope_items(connection, tenant, proposal_id, row.current_scope_version).await?;
    for item in &current_items {
        insert_scope_item(
            connection,
            tenant,
            proposal_id,
            next_scope_version,
            &item.title,
            &item.description,
            item.estimated_days,
            item.estimated_value,
            &item.deliverable_type,
        )
        .await?;
    }
    // The amendment line — `CR-<number>: <title>`.
    let amendment_title = format!("CR-{}: {}", cr.change_number, cr.title);
    let amendment_description = sqlx::query_as::<_, (String,)>(
        "SELECT delta_summary FROM archava_proposals.change_requests WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(cr.id)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?
    .0;
    // `time_delta_days` is an INT4 column, `cost_delta_value` a BIGINT —
    // the tuple types follow the migration exactly.
    let (amendment_time, amendment_cost): (i32, i64) = sqlx::query_as(
        "SELECT time_delta_days, cost_delta_value FROM archava_proposals.change_requests \
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant)
    .bind(cr.id)
    .fetch_one(&mut *connection)
    .await
    .map_err(refuses)?;
    insert_scope_item(
        connection,
        tenant,
        proposal_id,
        next_scope_version,
        &amendment_title,
        &amendment_description,
        Some(i64::from(amendment_time)),
        amendment_cost,
        "service",
    )
    .await?;

    let next_items = load_scope_items(connection, tenant, proposal_id, next_scope_version).await?;
    let total_value: i64 = next_items.iter().map(|item| item.estimated_value).sum();
    let total_days: i64 = next_items
        .iter()
        .map(|item| item.estimated_days.unwrap_or(0))
        .sum();

    sqlx::query(
        "UPDATE archava_proposals.proposals SET current_scope_version=$1, total_estimated_value=$2, \
         revision=$3, updated_at=clock_timestamp() WHERE tenant_id=$4 AND id=$5",
    )
    .bind(int4(next_scope_version)?)
    .bind(total_value)
    .bind(int4_positive(next_prop_revision)?)
    .bind(tenant)
    .bind(proposal_id)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;

    sqlx::query(
        "UPDATE archava_proposals.change_requests SET state='approved', resulting_scope_version=$1, \
         decided_at=clock_timestamp(), decided_by=$2, revision=$3, updated_at=clock_timestamp() \
         WHERE tenant_id=$4 AND id=$5",
    )
    .bind(int4(next_scope_version)?)
    .bind(actor_id)
    .bind(int4_positive(next_cr_revision)?)
    .bind(tenant)
    .bind(cr.id)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;

    let next_items_json = serde_json::to_value(&next_items).map_err(|_| conflict())?;
    let snapshot_summary = format!(
        "Agreed Scope Version {next_scope_version} (Amendment via CR-{})",
        cr.change_number
    );
    sqlx::query(
        "INSERT INTO archava_proposals.scope_versions(\
            tenant_id,id,proposal_id,scope_version,change_request_id,summary, \
            total_value,total_days,confirmed_at,confirmed_by,items) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,clock_timestamp(),$9,$10)",
    )
    .bind(tenant)
    .bind(Uuid::new_v4())
    .bind(proposal_id)
    .bind(int4(next_scope_version)?)
    .bind(cr.id)
    .bind(snapshot_summary)
    .bind(total_value)
    .bind(total_days)
    .bind(actor_id)
    .bind(next_items_json)
    .execute(&mut *connection)
    .await
    .map_err(refuses)?;
    Ok((
        Some(proposal_id),
        Some(cr.id),
        next_cr_revision,
        None,
        Some(next_scope_version),
    ))
}
