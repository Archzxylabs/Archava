//! The CRM SQL transactional adapter — ported from `packages/crm-pg/src/repository.ts`
//! and extracted from `crates/host/src/crm.rs` (ADR-014); moved under `erp/` per ADR-018.
//!
//! Enforces:
//! - Row-level security on `archava_crm` tables through `TenantTransaction`.
//! - Per-transaction timeout guards: statement_timeout 10s, lock_timeout 3s.
//! - Member authorization: `lock_membership` with kind verification, human vs. service principal intersection.
//! - Scope isolation: `all` vs `assigned` (owner check).
//! - 7 mutation ladder: `contact.create`, `contact.update`, `activity.add`, `followup.create`,
//!   `followup.complete`, `opportunity.create`, `opportunity.move`.
//! - Idempotency claims via `archava_crm.commands` (ON CONFLICT DO NOTHING RETURNING).
//! - Immutable audit ledger logging in `archava_crm.audit`.
//! - Optimistic concurrency control via revision validation (`expected_revision`, `contact_revision`).
//! - Pipeline stage machine enforcement (`PIPELINE_TRANSITIONS`) and `opportunity_transitions` recording.
//! - Read projections: `contacts` pagination, `record` composite, and `command_status` recovery.

use archava_core::tenant::TenantId;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use std::collections::HashMap;
use thiserror::Error;
use uuid::Uuid;

use crate::rls::{TenantContextError, TenantTransaction};

// --- Constants & Ledger definitions --------------------------------------------

pub const CRM_LEDGER_NAMESPACE: &str = "archava_crm";
pub const CRM_LEDGER_NAME: &str = "CRM";
pub const CRM_TABLES: &[&str] = &[
    "memberships",
    "contacts",
    "activities",
    "followups",
    "commands",
    "audit",
    "opportunities",
    "opportunity_transitions",
];

// Re-export CRM domain contracts from the unified ERP domain (ADR-018).
pub use archava_domain::erp::crm::*;

// --- Domain types & serialization ---------------------------------------------

/// The seven commands `packages/crm/src/contracts.ts` and
/// `pipeline-contracts.ts` define, field names carried over exactly.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "operation", deny_unknown_fields)]
pub enum CrmCommand {
    #[serde(rename = "contact.create")]
    ContactCreate { data: ContactFields },
    #[serde(rename = "contact.update")]
    ContactUpdate {
        id: Uuid,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        data: ContactPatch,
    },
    #[serde(rename = "activity.add")]
    ActivityAdd {
        #[serde(rename = "contactId")]
        contact_id: Uuid,
        #[serde(rename = "contactRevision")]
        contact_revision: i64,
        kind: ActivityKind,
        summary: String,
    },
    #[serde(rename = "followup.create")]
    FollowupCreate {
        #[serde(rename = "contactId")]
        contact_id: Uuid,
        #[serde(rename = "contactRevision")]
        contact_revision: i64,
        summary: String,
        #[serde(rename = "dueAt")]
        due_at: String,
    },
    #[serde(rename = "followup.complete")]
    FollowupComplete {
        id: Uuid,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "opportunity.create")]
    OpportunityCreate {
        #[serde(rename = "contactId")]
        contact_id: Uuid,
        #[serde(rename = "contactRevision")]
        contact_revision: i64,
        title: String,
    },
    #[serde(rename = "opportunity.move")]
    OpportunityMove {
        id: Uuid,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        stage: PipelineStage,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
}

impl CrmCommand {
    /// The operation name the audit row carries — the contract's own string.
    pub fn operation(&self) -> &'static str {
        match self {
            CrmCommand::ContactCreate { .. } => "contact.create",
            CrmCommand::ContactUpdate { .. } => "contact.update",
            CrmCommand::ActivityAdd { .. } => "activity.add",
            CrmCommand::FollowupCreate { .. } => "followup.create",
            CrmCommand::FollowupComplete { .. } => "followup.complete",
            CrmCommand::OpportunityCreate { .. } => "opportunity.create",
            CrmCommand::OpportunityMove { .. } => "opportunity.move",
        }
    }

    /// The write grant the operation needs — the port of `runMutation`'s
    /// `permission` (`packages/crm-pg/src/repository.ts`).
    pub fn permission(&self) -> &'static str {
        match self {
            CrmCommand::ContactCreate { .. } | CrmCommand::ContactUpdate { .. } => {
                Permission::CONTACTS_WRITE
            }
            CrmCommand::ActivityAdd { .. } => Permission::ACTIVITIES_WRITE,
            CrmCommand::FollowupCreate { .. } | CrmCommand::FollowupComplete { .. } => {
                Permission::FOLLOWUPS_WRITE
            }
            CrmCommand::OpportunityCreate { .. } | CrmCommand::OpportunityMove { .. } => {
                Permission::PIPELINE_WRITE
            }
        }
    }

    /// Whether the command also needs `contacts.read` — the pipeline commands
    /// resolve their resource through the contact the caller must be able to
    /// see (TS `ownedOpportunity` / `opportunity.create`).
    pub fn needs_contacts_read(&self) -> bool {
        matches!(
            self,
            CrmCommand::OpportunityCreate { .. } | CrmCommand::OpportunityMove { .. }
        )
    }

    /// Whether the operation belongs to the optional `crm-pipeline` module.
    pub fn needs_pipeline_module(&self) -> bool {
        matches!(
            self,
            CrmCommand::OpportunityCreate { .. } | CrmCommand::OpportunityMove { .. }
        )
    }

    /// Trim every caller string the way the TS schema's `.trim()` does, in
    /// place, BEFORE the fingerprint is computed.
    pub fn normalize(&mut self) {
        match self {
            CrmCommand::ContactCreate { data } => {
                data.full_name = data.full_name.trim().to_string();
                data.email = data.email.as_ref().map(|email| email.trim().to_string());
                data.organization = data
                    .organization
                    .as_ref()
                    .map(|organization| organization.trim().to_string());
            }
            CrmCommand::ContactUpdate { data, .. } => {
                data.full_name = data.full_name.as_ref().map(|name| name.trim().to_string());
                if let Some(email) = data.email.as_mut() {
                    *email = email.as_ref().map(|value| value.trim().to_string());
                }
                if let Some(organization) = data.organization.as_mut() {
                    *organization = organization.as_ref().map(|value| value.trim().to_string());
                }
            }
            CrmCommand::ActivityAdd { summary, .. }
            | CrmCommand::FollowupCreate { summary, .. } => {
                *summary = summary.trim().to_string();
            }
            CrmCommand::FollowupComplete { .. } => {}
            CrmCommand::OpportunityCreate { title, .. } => {
                *title = title.trim().to_string();
            }
            CrmCommand::OpportunityMove { reason, .. } => {
                if let Some(reason) = reason.as_mut() {
                    *reason = reason.trim().to_string();
                }
            }
        }
    }

    /// The TS zod refinements, re-checked at the boundary: lengths,
    /// revision bounds, the email shape, a non-empty patch, a loss reason.
    pub fn valid(&self) -> bool {
        match self {
            CrmCommand::ContactCreate { data } => {
                text_len(&data.full_name, 1, 160)
                    && data.email.as_deref().is_none_or(valid_email)
                    && data
                        .organization
                        .as_deref()
                        .is_none_or(|value| value.chars().count() <= 160)
            }
            CrmCommand::ContactUpdate {
                expected_revision,
                data,
                ..
            } => {
                *expected_revision >= 1
                    && !data.is_empty()
                    && data
                        .full_name
                        .as_deref()
                        .is_none_or(|name| text_len(name, 1, 160))
                    && data
                        .email
                        .as_ref()
                        .is_none_or(|email| email.as_deref().is_none_or(valid_email))
                    && data.organization.as_ref().is_none_or(|organization| {
                        organization
                            .as_deref()
                            .is_none_or(|value| value.chars().count() <= 160)
                    })
            }
            CrmCommand::ActivityAdd {
                contact_revision,
                summary,
                ..
            } => *contact_revision >= 1 && text_len(summary, 1, 2000),
            CrmCommand::FollowupCreate {
                contact_revision,
                summary,
                ..
            } => *contact_revision >= 1 && text_len(summary, 1, 2000),
            CrmCommand::FollowupComplete {
                expected_revision, ..
            } => *expected_revision >= 1,
            CrmCommand::OpportunityCreate {
                contact_revision,
                title,
                ..
            } => *contact_revision >= 1 && text_len(title, 1, 160),
            CrmCommand::OpportunityMove {
                expected_revision,
                stage,
                reason,
                ..
            } => {
                *expected_revision >= 1
                    && match stage {
                        PipelineStage::Lost => reason
                            .as_deref()
                            .is_some_and(|reason| text_len(reason, 1, 1000)),
                        _ => reason
                            .as_deref()
                            .is_none_or(|reason| reason.chars().count() <= 1000),
                    }
            }
        }
    }

    pub fn bound_contact(&self) -> Option<Uuid> {
        match self {
            CrmCommand::ContactCreate { .. } | CrmCommand::FollowupComplete { .. } => None,
            CrmCommand::ContactUpdate { id, .. } => Some(*id),
            CrmCommand::ActivityAdd { contact_id, .. } => Some(*contact_id),
            CrmCommand::FollowupCreate { contact_id, .. } => Some(*contact_id),
            CrmCommand::OpportunityCreate { contact_id, .. } => Some(*contact_id),
            CrmCommand::OpportunityMove { .. } => None,
        }
    }

    pub fn bound_revision(&self) -> Option<i64> {
        match self {
            CrmCommand::ContactCreate { .. } | CrmCommand::FollowupComplete { .. } => None,
            CrmCommand::ContactUpdate {
                expected_revision, ..
            } => Some(*expected_revision),
            CrmCommand::ActivityAdd {
                contact_revision, ..
            }
            | CrmCommand::FollowupCreate {
                contact_revision, ..
            }
            | CrmCommand::OpportunityCreate {
                contact_revision, ..
            } => Some(*contact_revision),
            CrmCommand::OpportunityMove {
                expected_revision, ..
            } => Some(*expected_revision),
        }
    }
}

/// The route's caller-owned envelope.
#[derive(Debug, Deserialize)]
pub struct CrmSubmission {
    pub tenant: TenantId,
    #[serde(rename = "requestId")]
    pub request_id: Uuid,
    pub command: CrmCommand,
}

// --- Query parameters & Responses ----------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ContactsPageQuery {
    pub tenant: TenantId,
    #[serde(default)]
    pub search: String,
    pub after: Option<Uuid>,
    pub limit: Option<u16>,
}

#[derive(Debug, Deserialize)]
pub struct RecordQuery {
    pub tenant: TenantId,
    pub contact: Uuid,
}

#[derive(Debug, Deserialize)]
pub struct StatusQuery {
    pub tenant: TenantId,
    #[serde(rename = "requestId")]
    pub request_id: Uuid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmContact {
    pub id: String,
    pub tenant_id: String,
    pub full_name: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub owner_id: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmPageValue {
    pub items: Vec<CrmContact>,
    pub next: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmStatusValue {
    pub status: &'static str,
    pub request_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receipt: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmActivity {
    pub id: String,
    pub contact_id: String,
    pub kind: String,
    pub summary: String,
    pub actor_id: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmFollowup {
    pub id: String,
    pub contact_id: String,
    pub summary: String,
    pub due_at: String,
    pub status: String,
    pub revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmOpportunity {
    pub id: String,
    pub contact_id: String,
    pub title: String,
    pub stage: String,
    pub revision: i64,
    pub loss_reason: Option<String>,
    pub transitions: Vec<CrmTransition>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmTransition {
    pub from: Option<String>,
    pub to: String,
    pub actor_id: String,
    pub revision: i64,
    pub occurred_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmRecordValue {
    /// Current public presentation only; no Owner management data or grants.
    #[serde(default)]
    pub pipeline_labels: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub configuration_revision: i64,
    pub contact: CrmContact,
    pub activities: Vec<CrmActivity>,
    pub followups: Vec<CrmFollowup>,
    pub opportunities: Vec<CrmOpportunity>,
    #[serde(rename = "pipelineEnabled")]
    pub pipeline_enabled: bool,
}

// --- Actor & Authority models --------------------------------------------------

#[derive(Debug, Clone)]
pub enum CrmActor {
    Human(String),
    Service { id: String, on_behalf_of: String },
}

impl CrmActor {
    pub fn principal_id(&self) -> &str {
        match self {
            CrmActor::Human(id) => id,
            CrmActor::Service { id, .. } => id,
        }
    }

    pub fn initiator_id(&self) -> Option<&str> {
        match self {
            CrmActor::Human(_) => None,
            CrmActor::Service { on_behalf_of, .. } => Some(on_behalf_of),
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            CrmActor::Human(_) => "human",
            CrmActor::Service { .. } => "service",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrmScope {
    All,
    Assigned,
}

impl CrmScope {
    pub fn parse(raw: &str) -> Result<Self, CrmPersistenceError> {
        match raw {
            "all" => Ok(CrmScope::All),
            "assigned" => Ok(CrmScope::Assigned),
            other => {
                tracing::error!(
                    value = %other,
                    "refusing: stored CRM scope outside its vocabulary"
                );
                Err(CrmPersistenceError::UnusableGrant)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct CrmAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: CrmScope,
}

impl CrmAccess {
    pub fn allows(&self, permission: &str) -> bool {
        self.permissions.iter().any(|held| held == permission)
    }
}

pub struct CrmMember {
    pub kind: String,
    pub permissions: Vec<String>,
    pub scope: CrmScope,
}

// --- Error handling ------------------------------------------------------------

#[derive(Debug, Error)]
pub enum CrmPersistenceError {
    #[error(transparent)]
    TenantContext(#[from] TenantContextError),

    #[error("the principal holds no active membership in the requested tenant")]
    MissingMembership,

    #[error("the stored membership grant or receipt is not usable")]
    UnusableGrant,

    #[error("unauthorized role/grant: {0}")]
    RoleNotAllowed(&'static str),

    #[error("unknown resource")]
    UnknownResource,

    #[error("unknown request")]
    UnknownRequest,

    #[error("request id reused for a different command")]
    RequestIdReused,

    #[error("revision changed")]
    RevisionChanged,

    #[error("pipeline transition not allowed")]
    TransitionNotAllowed,

    #[error("timeline format or due date invalid")]
    TimelineRejection,

    #[error("the submission is invalid")]
    InvalidSubmission,

    #[error("claim row did not hold for receipt")]
    LedgerMismatch,

    #[error("CRM database failure in {ledger}: {source}")]
    Database {
        ledger: &'static str,
        #[source]
        source: sqlx::Error,
    },
}

impl From<sqlx::Error> for CrmPersistenceError {
    fn from(source: sqlx::Error) -> Self {
        CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        }
    }
}

// --- Internal row types --------------------------------------------------------

pub type LockedContactRow = (i32, String, Option<String>, Option<String>, String);
pub type ContactRecordRow = (
    Uuid,
    String,
    Option<String>,
    Option<String>,
    String,
    i32,
    String,
    String,
);

pub struct LockedContact {
    pub revision: i32,
    pub full_name: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub owner_id: String,
}

pub enum ApplyFailure {
    Timeline(sqlx::Error),
    LedgerMismatch,
}

pub fn timeline_rejection(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database) => {
            matches!(database.code().as_deref(), Some("22007") | Some("22008"))
        }
        _ => false,
    }
}

pub fn receipt(kind: &str, id: Uuid, revision: i64) -> Value {
    serde_json::json!({
        "kind": kind,
        "id": id.to_string(),
        "revision": revision,
    })
}

// --- Persistence Operations ----------------------------------------------------

/// Opens the tenant transaction with CRM guards: statement timeout 10s, lock timeout 3s.
pub async fn begin(
    pool: &PgPool,
    tenant: TenantId,
) -> Result<TenantTransaction<'_>, CrmPersistenceError> {
    let mut transaction =
        TenantTransaction::begin(pool, CRM_LEDGER_NAMESPACE, CRM_LEDGER_NAME, tenant).await?;
    {
        let connection = transaction.checked_executor().await?;
        for statement in [
            "SET LOCAL statement_timeout='10s'",
            "SET LOCAL lock_timeout='3s'",
        ] {
            sqlx::query(statement)
                .execute(&mut *connection)
                .await
                .map_err(|source| CrmPersistenceError::Database {
                    ledger: CRM_LEDGER_NAME,
                    source,
                })?;
        }
    }
    Ok(transaction)
}

/// Reads the actor's membership through `archava_crm.lock_membership`.
pub async fn member_of(
    connection: &mut PgConnection,
    tenant: &TenantId,
    principal_id: &str,
) -> Result<Option<CrmMember>, CrmPersistenceError> {
    let row = sqlx::query_as::<_, (String, Vec<String>, String, bool)>(
        "SELECT kind, permissions, scope, active FROM archava_crm.lock_membership($1, $2)",
    )
    .bind(tenant.as_str())
    .bind(principal_id)
    .fetch_optional(connection)
    .await
    .map_err(|source| CrmPersistenceError::Database {
        ledger: CRM_LEDGER_NAME,
        source,
    })?;

    let Some((kind, permissions, scope, active)) = row else {
        return Ok(None);
    };
    if !active {
        return Ok(None);
    }
    match kind.as_str() {
        "human" | "service" => {}
        other => {
            tracing::error!(
                value = %other,
                "refusing: stored CRM membership kind outside its vocabulary"
            );
            return Err(CrmPersistenceError::UnusableGrant);
        }
    }
    for held in &permissions {
        if !PERMISSION_VOCABULARY.contains(&held.as_str()) {
            tracing::error!(
                value = %held,
                "refusing: stored CRM permission outside its vocabulary"
            );
            return Err(CrmPersistenceError::UnusableGrant);
        }
    }
    Ok(Some(CrmMember {
        kind,
        permissions,
        scope: CrmScope::parse(&scope)?,
    }))
}

/// Resolves the actor's CRM authority for one tenant — the port of
/// `PostgresCrmRepository::authorize`.
pub async fn authorize(
    connection: &mut PgConnection,
    tenant: &TenantId,
    actor: &CrmActor,
) -> Result<CrmAccess, CrmPersistenceError> {
    let own = member_of(connection, tenant, actor.principal_id()).await?;
    let Some(own) = own else {
        return Err(CrmPersistenceError::MissingMembership);
    };
    if own.kind != actor.kind() {
        return Err(CrmPersistenceError::MissingMembership);
    }
    let own_align = own;
    match actor {
        CrmActor::Human(_) => Ok(CrmAccess {
            permissions: own_align.permissions,
            owner_id: actor.principal_id().to_string(),
            scope: own_align.scope,
        }),
        CrmActor::Service { on_behalf_of, .. } => {
            let human = member_of(connection, tenant, on_behalf_of).await?;
            let Some(human) = human else {
                return Err(CrmPersistenceError::MissingMembership);
            };
            if human.kind != "human" {
                return Err(CrmPersistenceError::MissingMembership);
            }
            let permissions: Vec<String> = own_align
                .permissions
                .into_iter()
                .filter(|held| human.permissions.iter().any(|other| other == held))
                .collect();
            let scope = match (own_align.scope, human.scope) {
                (CrmScope::Assigned, _) | (_, CrmScope::Assigned) => CrmScope::Assigned,
                _ => CrmScope::All,
            };
            Ok(CrmAccess {
                permissions,
                owner_id: on_behalf_of.clone(),
                scope,
            })
        }
    }
}

pub async fn own_contact(
    connection: &mut PgConnection,
    tenant: &TenantId,
    access: &CrmAccess,
    contact_id: Uuid,
    lock: bool,
) -> Result<Option<i32>, CrmPersistenceError> {
    let sql = "SELECT revision FROM archava_crm.contacts \
               WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4)";
    let locked_sql = if lock {
        format!("{sql} FOR UPDATE")
    } else {
        sql.to_string()
    };
    let row: Option<(i32,)> = sqlx::query_as(sqlx::AssertSqlSafe(locked_sql.as_str()))
        .bind(tenant.as_str())
        .bind(contact_id)
        .bind(access.scope == CrmScope::All)
        .bind(&access.owner_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?;
    Ok(row.map(|(revision,)| revision))
}

pub async fn own_contact_row(
    connection: &mut PgConnection,
    tenant: &TenantId,
    access: &CrmAccess,
    contact_id: Uuid,
    lock: bool,
) -> Result<Option<LockedContact>, CrmPersistenceError> {
    let sql = "SELECT revision, full_name, email, organization, owner_id \
               FROM archava_crm.contacts \
               WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4)";
    let locked_sql = if lock {
        format!("{sql} FOR UPDATE")
    } else {
        sql.to_string()
    };
    let row: Option<LockedContactRow> = sqlx::query_as(sqlx::AssertSqlSafe(locked_sql.as_str()))
        .bind(tenant.as_str())
        .bind(contact_id)
        .bind(access.scope == CrmScope::All)
        .bind(&access.owner_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?;
    Ok(row.map(
        |(revision, full_name, email, organization, owner_id)| LockedContact {
            revision,
            full_name,
            email,
            organization,
            owner_id,
        },
    ))
}

pub async fn claim_command(
    connection: &mut PgConnection,
    tenant: &TenantId,
    request_id: Uuid,
    actor_id: &str,
    initiator_id: Option<&str>,
    fingerprint: &str,
) -> Result<bool, CrmPersistenceError> {
    let claimed: Option<(Uuid,)> = sqlx::query_as(
        "INSERT INTO archava_crm.commands(tenant_id,request_id,actor_id,fingerprint,initiator_id) \
         VALUES($1,$2,$3,$4,$5) \
         ON CONFLICT (tenant_id,request_id) DO NOTHING RETURNING request_id",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .bind(actor_id)
    .bind(fingerprint)
    .bind(initiator_id)
    .fetch_optional(connection)
    .await
    .map_err(|source| CrmPersistenceError::Database {
        ledger: CRM_LEDGER_NAME,
        source,
    })?;
    Ok(claimed.is_some())
}

pub fn stored_receipt(receipt: &Value) -> Result<(&'static str, Uuid), CrmPersistenceError> {
    let kind = receipt
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(CrmPersistenceError::UnusableGrant)?;
    let kind_permission = match kind {
        "contact" => Permission::CONTACTS_READ,
        "activity" => Permission::ACTIVITIES_READ,
        "followup" => Permission::FOLLOWUPS_READ,
        "opportunity" => Permission::PIPELINE_READ,
        other => {
            tracing::error!(
                value = %other,
                "refusing: stored receipt kind outside its vocabulary"
            );
            return Err(CrmPersistenceError::UnusableGrant);
        }
    };
    let id = receipt
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(CrmPersistenceError::UnusableGrant)?;
    let _revision = receipt
        .get("revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision > 0)
        .ok_or(CrmPersistenceError::UnusableGrant)?;
    Ok((kind_permission, id))
}

pub async fn receipt_visible(
    connection: &mut PgConnection,
    tenant: &TenantId,
    access: &CrmAccess,
    receipt: &Value,
    resource_id: Uuid,
) -> Result<bool, CrmPersistenceError> {
    let kind = receipt
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(CrmPersistenceError::UnusableGrant)?;
    let contact_id = match kind {
        "contact" => resource_id,
        "opportunity" => {
            let Some(contact_id) =
                resource_contact(connection, tenant, "opportunities", resource_id).await?
            else {
                return Ok(false);
            };
            contact_id
        }
        "followup" => {
            let Some(contact_id) =
                resource_contact(connection, tenant, "followups", resource_id).await?
            else {
                return Ok(false);
            };
            contact_id
        }
        "activity" => {
            let Some(contact_id) =
                resource_contact(connection, tenant, "activities", resource_id).await?
            else {
                return Ok(false);
            };
            contact_id
        }
        other => {
            tracing::error!(
                value = %other,
                "refusing: stored receipt kind outside its vocabulary"
            );
            return Err(CrmPersistenceError::UnusableGrant);
        }
    };
    own_contact(connection, tenant, access, contact_id, false)
        .await
        .map(|found| found.is_some())
}

pub async fn resource_contact(
    connection: &mut PgConnection,
    tenant: &TenantId,
    table: &str,
    resource_id: Uuid,
) -> Result<Option<Uuid>, CrmPersistenceError> {
    debug_assert!(
        matches!(table, "followups" | "opportunities" | "activities"),
        "only a fixed table name may reach this query"
    );
    if !matches!(table, "followups" | "opportunities" | "activities") {
        return Err(CrmPersistenceError::UnusableGrant);
    }
    let sql = format!("SELECT contact_id FROM archava_crm.{table} WHERE tenant_id=$1 AND id=$2");
    let row: Option<(Uuid,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant.as_str())
        .bind(resource_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?;
    Ok(row.map(|(contact_id,)| contact_id))
}

pub async fn finish_effect(
    connection: &mut PgConnection,
    tenant: &TenantId,
    request_id: Uuid,
    actor: &CrmActor,
    operation: &str,
    receipt: &Value,
) -> Result<(), ApplyFailure> {
    let resource_id = receipt
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(ApplyFailure::LedgerMismatch)?;
    let revision = receipt
        .get("revision")
        .and_then(Value::as_i64)
        .and_then(|revision| i32::try_from(revision).ok())
        .ok_or(ApplyFailure::LedgerMismatch)?;
    sqlx::query(
        "INSERT INTO archava_crm.audit(tenant_id,request_id,actor_id,operation,resource_id,revision,initiator_id) \
         VALUES($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .bind(actor.principal_id())
    .bind(operation)
    .bind(resource_id)
    .bind(revision)
    .bind(actor.initiator_id())
    .execute(&mut *connection)
    .await
    .map_err(ApplyFailure::Timeline)?;
    let recorded = sqlx::query(
        "UPDATE archava_crm.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .bind(receipt)
    .execute(&mut *connection)
    .await
    .map_err(ApplyFailure::Timeline)?;
    if recorded.rows_affected() != 1 {
        tracing::error!(
            updated = recorded.rows_affected(),
            "refusing: the claim row did not hold for its receipt"
        );
        return Err(ApplyFailure::LedgerMismatch);
    }
    Ok(())
}

fn map_apply_failure(failure: ApplyFailure) -> CrmPersistenceError {
    match failure {
        ApplyFailure::Timeline(source) if timeline_rejection(&source) => {
            CrmPersistenceError::TimelineRejection
        }
        ApplyFailure::Timeline(source) => CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        },
        ApplyFailure::LedgerMismatch => CrmPersistenceError::LedgerMismatch,
    }
}

/// The command execution outcome: committed receipt (fresh or replayed) or unconfirmed in-flight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CommandExecution {
    Committed {
        receipt: Value,
        is_replay: bool,
        fingerprint: Option<String>,
    },
    Unconfirmed {
        request_id: Uuid,
    },
}

/// Executes a CRM mutation inside an existing `TenantTransaction`.
pub async fn execute_command(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    actor: &CrmActor,
    access: &CrmAccess,
    request_id: Uuid,
    command: &CrmCommand,
    fingerprint: &str,
) -> Result<CommandExecution, CrmPersistenceError> {
    // 1. Grant check
    if !access.allows(command.permission()) {
        return Err(CrmPersistenceError::RoleNotAllowed(
            "The command requires its write grant.",
        ));
    }
    if command.needs_contacts_read() && !access.allows(Permission::CONTACTS_READ) {
        return Err(CrmPersistenceError::RoleNotAllowed(
            "Contact access is required.",
        ));
    }

    // 2. Idempotency claim
    let claimed = {
        let connection = transaction.checked_executor().await?;
        claim_command(
            connection,
            tenant,
            request_id,
            actor.principal_id(),
            actor.initiator_id(),
            fingerprint,
        )
        .await?
    };

    if !claimed {
        return replay_or_conflict(
            transaction,
            tenant,
            request_id,
            actor.principal_id(),
            actor.initiator_id(),
            fingerprint,
            access,
        )
        .await;
    }

    // 3. Resource check & effect
    let receipt = apply_command(transaction, tenant, request_id, actor, access, command).await?;
    Ok(CommandExecution::Committed {
        receipt,
        is_replay: false,
        fingerprint: None,
    })
}

async fn replay_or_conflict(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    request_id: Uuid,
    actor_id: &str,
    initiator_id: Option<&str>,
    fingerprint: &str,
    access: &CrmAccess,
) -> Result<CommandExecution, CrmPersistenceError> {
    let read = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as::<_, (String, Option<Value>, String)>(
            "SELECT actor_id, receipt, fingerprint FROM archava_crm.commands \
             WHERE tenant_id=$1 AND request_id=$2 AND initiator_id IS NOT DISTINCT FROM $3",
        )
        .bind(tenant.as_str())
        .bind(request_id)
        .bind(initiator_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    };

    let Some((existing_actor, receipt, existing_fingerprint)) = read else {
        tracing::info!(
            %request_id,
            "answering unconfirmed: the key is claimed by an in-flight submission"
        );
        return Ok(CommandExecution::Unconfirmed { request_id });
    };

    if existing_actor != actor_id {
        return Err(CrmPersistenceError::UnknownRequest);
    }
    if existing_fingerprint != fingerprint {
        return Err(CrmPersistenceError::RequestIdReused);
    }
    let Some(receipt) = receipt else {
        return Ok(CommandExecution::Unconfirmed { request_id });
    };

    let (kind_permission, resource_id) = stored_receipt(&receipt)?;
    if !access.allows(kind_permission) || !access.allows(Permission::CONTACTS_READ) {
        return Err(CrmPersistenceError::RoleNotAllowed(
            "CRM receipt access is not permitted.",
        ));
    }

    {
        let connection = transaction.checked_executor().await?;
        if !receipt_visible(connection, tenant, access, &receipt, resource_id).await? {
            return Err(CrmPersistenceError::UnknownResource);
        }
    }

    Ok(CommandExecution::Committed {
        receipt,
        is_replay: true,
        fingerprint: Some(existing_fingerprint),
    })
}

async fn apply_command(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    request_id: Uuid,
    actor: &CrmActor,
    access: &CrmAccess,
    command: &CrmCommand,
) -> Result<Value, CrmPersistenceError> {
    let bound = match command.bound_contact() {
        None => None,
        Some(contact_id) => {
            let found = {
                let connection = transaction.checked_executor().await?;
                own_contact_row(connection, tenant, access, contact_id, true).await?
            };
            match found {
                None => return Err(CrmPersistenceError::UnknownResource),
                Some(row) => Some(row),
            }
        }
    };
    if let (Some(expected), Some(row)) = (command.bound_revision(), bound.as_ref()) {
        if i64::from(row.revision) != expected {
            return Err(CrmPersistenceError::RevisionChanged);
        }
    }

    match command {
        CrmCommand::ContactCreate { data } => {
            let id = Uuid::new_v4();
            let receipt_val = receipt("contact", id, 1);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "INSERT INTO archava_crm.contacts(tenant_id,id,full_name,email,organization,owner_id) \
                 VALUES($1,$2,$3,$4,$5,$6)",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(&data.full_name)
            .bind(&data.email)
            .bind(&data.organization)
            .bind(&access.owner_id)
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
        CrmCommand::ContactUpdate { id, data, .. } => {
            let row = bound.as_ref().expect("the bound contact was loaded");
            let full_name = data
                .full_name
                .clone()
                .unwrap_or_else(|| row.full_name.clone());
            let email = match &data.email {
                None => row.email.clone(),
                Some(value) => value.clone(),
            };
            let organization = match &data.organization {
                None => row.organization.clone(),
                Some(value) => value.clone(),
            };
            let receipt_val = receipt("contact", *id, i64::from(row.revision) + 1);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "UPDATE archava_crm.contacts \
                 SET full_name=$3,email=$4,organization=$5,revision=revision+1,updated_at=now() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(&full_name)
            .bind(&email)
            .bind(&organization)
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
        CrmCommand::ActivityAdd {
            contact_id,
            kind,
            summary,
            ..
        } => {
            let id = Uuid::new_v4();
            let receipt_val = receipt("activity", id, 1);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "INSERT INTO archava_crm.activities(tenant_id,id,contact_id,kind,summary,actor_id) \
                 VALUES($1,$2,$3,$4,$5,$6)",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(contact_id)
            .bind(kind.as_str())
            .bind(summary)
            .bind(actor.principal_id())
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "UPDATE archava_crm.contacts SET revision=revision+1, updated_at=now() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant.as_str())
            .bind(contact_id)
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
        CrmCommand::FollowupCreate {
            contact_id,
            summary,
            due_at,
            ..
        } => {
            let id = Uuid::new_v4();
            let receipt_val = receipt("followup", id, 1);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at) \
                 VALUES($1,$2,$3,$4,$5::timestamptz)",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(contact_id)
            .bind(summary)
            .bind(due_at)
            .execute(&mut *connection)
            .await
            .map_err(|source| map_apply_failure(ApplyFailure::Timeline(source)))?;
            sqlx::query(
                "UPDATE archava_crm.contacts SET revision=revision+1, updated_at=now() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant.as_str())
            .bind(contact_id)
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
        CrmCommand::FollowupComplete {
            id,
            expected_revision,
        } => {
            let locked = {
                let connection = transaction.checked_executor().await?;
                sqlx::query_as::<_, (Uuid, i32, String)>(
                    "SELECT contact_id, revision, status FROM archava_crm.followups \
                     WHERE tenant_id=$1 AND id=$2 FOR UPDATE",
                )
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(connection)
                .await
                .map_err(|source| CrmPersistenceError::Database {
                    ledger: CRM_LEDGER_NAME,
                    source,
                })?
            };
            let Some((contact_id, revision, status)) = locked else {
                return Err(CrmPersistenceError::UnknownResource);
            };
            let visible = {
                let connection = transaction.checked_executor().await?;
                own_contact(connection, tenant, access, contact_id, true).await?
            };
            if visible.is_none() {
                return Err(CrmPersistenceError::UnknownResource);
            }
            if i64::from(revision) != *expected_revision || status != "open" {
                return Err(CrmPersistenceError::RevisionChanged);
            }
            let receipt_val = receipt("followup", *id, *expected_revision + 1);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "UPDATE archava_crm.followups SET status='completed',revision=revision+1,updated_at=now() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant.as_str())
            .bind(id)
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
        CrmCommand::OpportunityCreate {
            contact_id, title, ..
        } => {
            let contact = bound.as_ref().expect("the bound contact was loaded");
            let id = Uuid::new_v4();
            let receipt_val = receipt("opportunity", id, 1);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "INSERT INTO archava_crm.opportunities(tenant_id,id,contact_id,owner_id,title) \
                 VALUES($1,$2,$3,$4,$5)",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(contact_id)
            .bind(&contact.owner_id)
            .bind(title)
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "INSERT INTO archava_crm.opportunity_transitions \
                 (tenant_id,request_id,opportunity_id,to_stage,actor_id,revision) \
                 VALUES($1,$2,$3,'new',$4,1)",
            )
            .bind(tenant.as_str())
            .bind(request_id)
            .bind(id)
            .bind(actor.principal_id())
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
        CrmCommand::OpportunityMove {
            id,
            expected_revision,
            stage,
            reason,
        } => {
            let locked = {
                let connection = transaction.checked_executor().await?;
                sqlx::query_as::<_, (i32, String, Uuid)>(
                    "SELECT revision, stage, contact_id FROM archava_crm.opportunities \
                     WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) FOR UPDATE",
                )
                .bind(tenant.as_str())
                .bind(id)
                .bind(access.scope == CrmScope::All)
                .bind(&access.owner_id)
                .fetch_optional(connection)
                .await
                .map_err(|source| CrmPersistenceError::Database {
                    ledger: CRM_LEDGER_NAME,
                    source,
                })?
            };
            let Some((revision, from_stage, contact_id)) = locked else {
                return Err(CrmPersistenceError::UnknownResource);
            };
            let visible = {
                let connection = transaction.checked_executor().await?;
                own_contact(connection, tenant, access, contact_id, false).await?
            };
            if visible.is_none() {
                return Err(CrmPersistenceError::UnknownResource);
            }
            if i64::from(revision) != *expected_revision {
                return Err(CrmPersistenceError::RevisionChanged);
            }
            let from =
                PipelineStage::parse(&from_stage).ok_or(CrmPersistenceError::UnusableGrant)?;
            if !transition_allowed(from, *stage) {
                return Err(CrmPersistenceError::TransitionNotAllowed);
            }
            let loss_reason = if *stage == PipelineStage::Lost {
                reason.clone()
            } else {
                None
            };
            let next_revision = i64::from(revision) + 1;
            let transition_revision =
                i32::try_from(next_revision).map_err(|_| CrmPersistenceError::RevisionChanged)?;
            let receipt_val = receipt("opportunity", *id, next_revision);
            let connection = transaction.checked_executor().await?;
            sqlx::query(
                "UPDATE archava_crm.opportunities \
                 SET stage=$3,loss_reason=$4,revision=revision+1,updated_at=now() \
                 WHERE tenant_id=$1 AND id=$2",
            )
            .bind(tenant.as_str())
            .bind(id)
            .bind(stage.as_str())
            .bind(&loss_reason)
            .execute(&mut *connection)
            .await?;
            sqlx::query(
                "INSERT INTO archava_crm.opportunity_transitions \
                 (tenant_id,request_id,opportunity_id,from_stage,to_stage,reason,actor_id,revision) \
                 VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(tenant.as_str())
            .bind(request_id)
            .bind(id)
            .bind(from.as_str())
            .bind(stage.as_str())
            .bind(&loss_reason)
            .bind(actor.principal_id())
            .bind(transition_revision)
            .execute(&mut *connection)
            .await?;
            finish_effect(
                connection,
                tenant,
                request_id,
                actor,
                command.operation(),
                &receipt_val,
            )
            .await
            .map_err(map_apply_failure)?;
            Ok(receipt_val)
        }
    }
}

// --- Read Projections ----------------------------------------------------------

/// `list_contacts` — session-scoped contact list query.
pub async fn list_contacts(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    access: &CrmAccess,
    search: &str,
    after: Option<Uuid>,
    limit: i64,
) -> Result<CrmPageValue, CrmPersistenceError> {
    if !access.allows(Permission::CONTACTS_READ) {
        return Err(CrmPersistenceError::RoleNotAllowed(
            "The contact list requires a contact read grant.",
        ));
    }

    let escaped_search = search
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");

    let connection = transaction.checked_executor().await?;
    let rows = sqlx::query_as::<
        _,
        (
            Uuid,
            String,
            Option<String>,
            Option<String>,
            String,
            i32,
            String,
            String,
        ),
    >(
        "SELECT id, full_name, email, organization, owner_id, revision, \
                to_jsonb(created_at)#>>'{}' AS created, to_jsonb(updated_at)#>>'{}' AS updated \
         FROM archava_crm.contacts \
         WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) \
           AND ($4::uuid IS NULL OR id>$4) \
           AND full_name ILIKE $5 ESCAPE '\\' \
         ORDER BY id LIMIT $6",
    )
    .bind(tenant.as_str())
    .bind(access.scope == CrmScope::All)
    .bind(&access.owner_id)
    .bind(after)
    .bind(format!("%{escaped_search}%"))
    .bind(limit + 1)
    .fetch_all(connection)
    .await
    .map_err(|source| CrmPersistenceError::Database {
        ledger: CRM_LEDGER_NAME,
        source,
    })?;

    let items: Vec<CrmContact> = rows
        .iter()
        .take(limit as usize)
        .map(
            |(id, full_name, email, organization, owner_id, revision, created, updated)| {
                CrmContact {
                    id: id.to_string(),
                    tenant_id: tenant.as_str().to_string(),
                    full_name: full_name.clone(),
                    email: email.clone(),
                    organization: organization.clone(),
                    owner_id: owner_id.clone(),
                    revision: i64::from(*revision),
                    created_at: created.clone(),
                    updated_at: updated.clone(),
                }
            },
        )
        .collect();

    let next = if rows.len() > limit as usize {
        items.last().map(|contact| contact.id.clone())
    } else {
        None
    };

    Ok(CrmPageValue { items, next })
}

/// `read_record` — contact composite record read.
pub async fn read_record(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    access: &CrmAccess,
    contact_id: Uuid,
    pipeline_enabled: bool,
) -> Result<CrmRecordValue, CrmPersistenceError> {
    if !access.allows(Permission::CONTACTS_READ) {
        return Err(CrmPersistenceError::RoleNotAllowed(
            "The record requires a contact read grant.",
        ));
    }

    let contact: Option<ContactRecordRow> = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as(
            "SELECT id, full_name, email, organization, owner_id, revision, \
                    to_jsonb(created_at) #>> '{}', to_jsonb(updated_at) #>> '{}' \
             FROM archava_crm.contacts \
             WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4)",
        )
        .bind(tenant.as_str())
        .bind(contact_id)
        .bind(access.scope == CrmScope::All)
        .bind(&access.owner_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    };

    let Some((id, full_name, email, organization, owner_id, revision, created_at, updated_at)) =
        contact
    else {
        return Err(CrmPersistenceError::UnknownResource);
    };

    let activities: Vec<(Uuid, Uuid, String, String, String, String)> = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as(
            "SELECT id, contact_id, kind, summary, actor_id, to_jsonb(created_at) #>> '{}' \
             FROM archava_crm.activities \
             WHERE tenant_id=$1 AND contact_id=$2 \
             ORDER BY created_at DESC, id DESC",
        )
        .bind(tenant.as_str())
        .bind(contact_id)
        .fetch_all(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    };

    let followups: Vec<(Uuid, Uuid, String, String, String, i32)> = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as(
            "SELECT id, contact_id, summary, to_jsonb(due_at) #>> '{}', status, revision \
             FROM archava_crm.followups \
             WHERE tenant_id=$1 AND contact_id=$2 \
             ORDER BY due_at, id",
        )
        .bind(tenant.as_str())
        .bind(contact_id)
        .fetch_all(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    };

    let opportunities: Vec<(Uuid, Uuid, String, String, i32, Option<String>)> = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as(
            "SELECT id, contact_id, title, stage, revision, loss_reason \
             FROM archava_crm.opportunities \
             WHERE tenant_id=$1 AND contact_id=$2 AND ($3::boolean OR owner_id=$4) \
             ORDER BY created_at DESC, id DESC",
        )
        .bind(tenant.as_str())
        .bind(contact_id)
        .bind(access.scope == CrmScope::All)
        .bind(&access.owner_id)
        .fetch_all(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    };

    let transitions: Vec<(Uuid, Option<String>, String, String, i32, String)> = if pipeline_enabled
    {
        let connection = transaction.checked_executor().await?;
        let ids: Vec<Uuid> = opportunities.iter().map(|row| row.0).collect();
        sqlx::query_as(
            "SELECT opportunity_id, from_stage, to_stage, actor_id, revision, occurred_at::text \
             FROM archava_crm.opportunity_transitions \
             WHERE tenant_id=$1 AND opportunity_id = ANY($2) \
             ORDER BY revision",
        )
        .bind(tenant.as_str())
        .bind(&ids)
        .fetch_all(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    } else {
        Vec::new()
    };

    let mut history: HashMap<Uuid, Vec<CrmTransition>> = HashMap::new();
    for (opportunity_id, from, to, actor_id, revision, occurred_at) in transitions {
        history
            .entry(opportunity_id)
            .or_default()
            .push(CrmTransition {
                from,
                to,
                actor_id,
                revision: i64::from(revision),
                occurred_at,
            });
    }

    Ok(CrmRecordValue {
        pipeline_labels: Default::default(),
        configuration_revision: 0,
        pipeline_enabled,
        contact: CrmContact {
            id: id.to_string(),
            tenant_id: tenant.as_str().to_string(),
            full_name,
            email,
            organization,
            owner_id,
            revision: i64::from(revision),
            created_at,
            updated_at,
        },
        activities: activities
            .into_iter()
            .map(
                |(id, contact_id, kind, summary, actor_id, created_at)| CrmActivity {
                    id: id.to_string(),
                    contact_id: contact_id.to_string(),
                    kind,
                    summary,
                    actor_id,
                    created_at,
                },
            )
            .collect(),
        followups: followups
            .into_iter()
            .map(
                |(id, contact_id, summary, due_at, status, revision)| CrmFollowup {
                    id: id.to_string(),
                    contact_id: contact_id.to_string(),
                    summary,
                    due_at,
                    status,
                    revision: i64::from(revision),
                },
            )
            .collect(),
        opportunities: opportunities
            .into_iter()
            .map(
                |(id, contact_id, title, stage, revision, loss_reason)| CrmOpportunity {
                    transitions: history.remove(&id).unwrap_or_default(),
                    id: id.to_string(),
                    contact_id: contact_id.to_string(),
                    title,
                    stage,
                    revision: i64::from(revision),
                    loss_reason,
                },
            )
            .collect(),
    })
}

/// `read_status` — command recovery read (TS `commandStatus`).
pub async fn read_status(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    access: &CrmAccess,
    principal_id: &str,
    initiator: Option<&str>,
    request_id: Uuid,
) -> Result<CrmStatusValue, CrmPersistenceError> {
    let row = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as::<_, (Option<Value>,)>(
            "SELECT receipt FROM archava_crm.commands \
             WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3 \
               AND initiator_id IS NOT DISTINCT FROM $4",
        )
        .bind(tenant.as_str())
        .bind(request_id)
        .bind(principal_id)
        .bind(initiator)
        .fetch_optional(connection)
        .await
        .map_err(|source| CrmPersistenceError::Database {
            ledger: CRM_LEDGER_NAME,
            source,
        })?
    };

    let Some((Some(receipt),)) = row else {
        return Ok(CrmStatusValue {
            status: "unconfirmed",
            request_id,
            receipt: None,
        });
    };

    let (kind_permission, resource_id) = stored_receipt(&receipt)?;
    if !access.allows(kind_permission) || !access.allows(Permission::CONTACTS_READ) {
        return Err(CrmPersistenceError::RoleNotAllowed(
            "CRM receipt access is not permitted.",
        ));
    }

    {
        let connection = transaction.checked_executor().await?;
        if !receipt_visible(connection, tenant, access, &receipt, resource_id).await? {
            return Err(CrmPersistenceError::UnknownResource);
        }
    };

    Ok(CrmStatusValue {
        status: "committed",
        request_id,
        receipt: Some(receipt),
    })
}
