//! §1.4B's vertical-slice backend: the CRM serving state the slice UI talks to.
//!
//! ## What this module is
//!
//! One real CRM operation — `followup.create`, from `packages/crm/src/contracts.ts`
//! — served by this host against the real `archava_crm` schema, with the
//! caller-owned rules carried over from the commerce side: the tenant comes
//! from the caller's own submission, the authority comes from the row the
//! database holds (never from the body), and the effect is one transaction
//! that commits exactly once and records the receipt the caller can later
//! recover through [`command_status`].
//!
//! ## Why `followup.create`
//!
//! The TS baseline's own command vocabulary for CRM is `contact.create` /
//! `contact.update` / `activity.add` / `followup.create` / `followup.complete`
//! (plus pipeline moves), and `followup.create` is the one operation the
//! baseline runs through BOTH the human path and the
//! service-on-behalf-of path (`packages/crm-pg/src/repository.ts:592-606` —
//! service principals may submit only follow-ups). It carries every required
//! property of the slice flow at once:
//!
//! - the draft binds to a REAL contact (`contactId` + `contactRevision`), so
//!   the stale-revision refusal is a property of the database row rather than
//!   of a form field;
//! - the effect touches four records (`followups` insert, `contacts` revision
//!   bump, `commands` claim, `audit` entry) — a persisted business fact, not
//!   an input blob;
//! - the receipt `{kind:"followup", id, revision:1}` is durable and
//!   recoverable through `GET /crm/commands/status`, whose permission gate and
//!   containment check are ports of `readCommandStatus`;
//! - the service path exercises the actor×initiator grant intersection that
//!   TS records in `initiator_id`; every audit row carries both ids.
//!
//! What this module deliberately does NOT do: invent a second command
//! vocabulary aliasing the TS contracts, or place a service principal's HTTP
//! route before a signed native runner exists. Service submissions are
//! exercised by the suite through [`submit_followup`] — the same core the
//! human route calls — which is where the intersection is provable; a browser
//! route for it would be new surface, and the baseline serves the native path
//! through the operator's daemon over an epoch control file, not open HTTP.
//!
//! ## How the pieces map (`packages/crm-pg/src/repository.ts` → here)
//!
//! | baseline                           | here                                            |
//! | ---                                | ---                                             |
//! | `authorize` (locked `lock_membership`) | [`member_of`] + [`authorize`]               |
//! | `runMutation` claim INSERT + replay/other-actor branches | [`submit_followup`] step 2 plus [`replay_or_conflict`] |
//! | `ownedContact` (`FOR UPDATE` on write) | [`own_contact`]                             |
//! | `readCommandStatus` + `receiptResource` | [`read_status`]                             |
//! | `transaction()`'s `SET LOCAL` timeouts  | [`begin`]                                   |
//!
//! Refusal vocabulary: the host's own RN02-disposed shapes — `unknown_request`
//! 404 for another actor's key (existence is not disclosed), `request_id_reused`
//! 409 with no receipt for the same key carrying a different command,
//! `revision_changed` 409 for a stale revision, and `role_not_allowed` 403 for
//! a missing grant. TS's `CrmError` text differs in wording; the status codes
//! and the no-leak property match.

use crate::authority::AuthorityError;
use crate::identity::AuthenticatedSession;
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, unknown_request, AppState,
    CommandOutcome, Ledger,
};
use archava_adapters::rls::TenantTransaction;
use archava_core::tenant::TenantId;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// The CRM ledger — every tenant-scoped table migrations 001 + 002 force RLS
/// on. Migration 003 adds no table (it widens `commands`/`audit` with
/// `initiator_id`), so the count is exact: six from 001, two from 002.
///
/// The count matters the same way it does for commerce: the readiness gate
/// counts the rows `pg_class` reports back, and one table missing from this
/// list lets a forced-policy drift go unobserved.
pub(crate) const CRM_LEDGER: Ledger = Ledger {
    namespace: "archava_crm",
    ledger: "CRM",
    tables: &[
        "memberships",
        "contacts",
        "activities",
        "followups",
        "commands",
        "audit",
        "opportunities",
        "opportunity_transitions",
    ],
};

/// The permission names the schema's CHECK constrains
/// (`packages/crm/src/contracts.ts`'s permission vocabulary). Checked by exact
/// string equality; a grant outside the vocabulary makes [`member_of`]
/// refuse loudly rather than guess.
pub struct Permission;

impl Permission {
    pub(crate) const CONTACTS_READ: &'static str = "contacts.read";
    pub(crate) const CONTACTS_WRITE: &'static str = "contacts.write";
    pub(crate) const ACTIVITIES_READ: &'static str = "activities.read";
    pub(crate) const ACTIVITIES_WRITE: &'static str = "activities.write";
    pub(crate) const FOLLOWUPS_READ: &'static str = "followups.read";
    pub(crate) const FOLLOWUPS_WRITE: &'static str = "followups.write";
    pub(crate) const PIPELINE_READ: &'static str = "pipeline.read";
    pub(crate) const PIPELINE_WRITE: &'static str = "pipeline.write";
    pub(crate) const AUDIT_READ: &'static str = "audit.read";
}

/// Every permission value the schema's CHECK admits, in one place for the
/// stored-grant validation in [`member_of`].
const PERMISSION_VOCABULARY: [&str; 9] = [
    Permission::CONTACTS_READ,
    Permission::CONTACTS_WRITE,
    Permission::ACTIVITIES_READ,
    Permission::ACTIVITIES_WRITE,
    Permission::FOLLOWUPS_READ,
    Permission::FOLLOWUPS_WRITE,
    Permission::PIPELINE_READ,
    Permission::PIPELINE_WRITE,
    Permission::AUDIT_READ,
];

// --- Typed submissions ---------------------------------------------------------

/// The one command the slice serves: a reviewed new follow-up bound to a
/// specific contact revision (`packages/crm/src/contracts.ts`'s
/// `followup.create`, field names carried over exactly). `deny_unknown_fields`
/// plus the `operation` tag make any other operation or a stray key a boundary
/// refusal — a body cannot claim an operation this host does not implement.
#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "kebab-case", deny_unknown_fields)]
pub enum CrmCommand {
    #[serde(rename = "followup.create")]
    FollowupCreate {
        /// The contact the follow-up is bound to; its CURRENT revision must
        /// equal `contactRevision` or the command conflicts.
        #[serde(rename = "contactId")]
        contact_id: Uuid,
        #[serde(rename = "contactRevision")]
        contact_revision: i64,
        /// 1–2000 characters — the schema's own CHECK, enforced here before
        /// any database work.
        summary: String,
        /// RFC 3339 instant. The `.timestamptz` cast is the calendar
        /// authority (SQLSTATE 22007/22008 map to a submission refusal, not
        /// an outage).
        #[serde(rename = "dueAt")]
        due_at: String,
    },
}

impl CrmCommand {
    fn summary(&self) -> &str {
        match self {
            CrmCommand::FollowupCreate { summary, .. } => summary,
        }
    }

    fn contact_revision(&self) -> i64 {
        match self {
            CrmCommand::FollowupCreate {
                contact_revision, ..
            } => *contact_revision,
        }
    }

    fn due_at(&self) -> &str {
        match self {
            CrmCommand::FollowupCreate { due_at, .. } => due_at,
        }
    }

    fn contact_id(&self) -> Uuid {
        match self {
            CrmCommand::FollowupCreate { contact_id, .. } => *contact_id,
        }
    }
}

/// The route's caller-owned envelope: which tenant, the caller's own
/// idempotency key, the typed command. `tenant` is a route field, NOT part of
/// the TS mutation contract, for the same reason the commerce route owns its
/// tenant: it must reach the handler only from the submission the caller sent.
#[derive(Debug, Deserialize)]
pub struct CrmSubmission {
    pub tenant: TenantId,
    #[serde(rename = "requestId")]
    pub request_id: Uuid,
    pub command: CrmCommand,
}

/// `GET /contacts` query — the TS `ContactPage`: `after` cursor, 1..100 limit
/// (default 20), literal search on `full_name`.
#[derive(Debug, Deserialize)]
pub(crate) struct ContactsPageQuery {
    pub(crate) tenant: TenantId,
    #[serde(default)]
    pub(crate) search: String,
    pub(crate) after: Option<Uuid>,
    pub(crate) limit: Option<u16>,
}

/// `GET /crm/commands/status` query — TS `requestId` + the tenant.
#[derive(Debug, Deserialize)]
pub(crate) struct StatusQuery {
    pub(crate) tenant: TenantId,
    #[serde(rename = "requestId")]
    pub(crate) request_id: Uuid,
}

// --- Response shapes -----------------------------------------------------------

/// One contact, the TS `Contact` shape; timestamps render as jsonb renders
/// timestamptz (`to_jsonb(x) #>> '{}'`) — the ISO-8601 instance
/// `Date.toISOString()` prints — because this host carries no date crate and
/// the database already owns a calendar.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmContact {
    pub(crate) id: String,
    pub(crate) tenant_id: String,
    pub(crate) full_name: String,
    pub(crate) email: Option<String>,
    pub(crate) organization: Option<String>,
    pub(crate) owner_id: String,
    pub(crate) revision: i64,
    pub(crate) created_at: String,
    pub(crate) updated_at: String,
}

/// One contacts page (`ContactPage`): `items` plus the after-cursor — the
/// cursor is the last emitted id, present only when a further row exists.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmPageValue {
    pub(crate) items: Vec<CrmContact>,
    pub(crate) next: Option<String>,
}

/// The recovery contract (`CrmCommandStatus`): `committed` with a receipt, or
/// `unconfirmed` — **absence is not proof of failure** (the contracts file's
/// own comment), because the original transaction may still be running.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CrmStatusValue {
    pub(crate) status: &'static str,
    pub request_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) receipt: Option<Value>,
}

/// The union body this module's routes answer with. The commerce routes speak
/// one envelope [`CommandOutcome`]; the CRM list and status responses are
/// different documents, and one enum keeps every path's -*refusal* handling in
/// the same shape while the success bodies stay honest to their contracts.
pub enum CrmBody {
    Outcome(CommandOutcome),
    Page(CrmPageValue),
    Status(CrmStatusValue),
}

impl IntoResponse for CrmBody {
    fn into_response(self) -> Response {
        match self {
            CrmBody::Outcome(value) => Json(value).into_response(),
            CrmBody::Page(value) => Json(value).into_response(),
            CrmBody::Status(value) => Json(value).into_response(),
        }
    }
}

/// Levels a `Json<CommandOutcome>` answer up to this module's body type.
fn outcome(answer: (StatusCode, Json<CommandOutcome>)) -> (StatusCode, CrmBody) {
    (answer.0, CrmBody::Outcome(answer.1 .0))
}

/// 400 for a syntactically valid document the host refuses as a submission —
/// a summary of the wrong length, a revision below 1, a limit outside the
/// page bounds. Fixed text; the detail lives in the log only.
fn invalid_submission() -> (StatusCode, CrmBody) {
    (
        StatusCode::BAD_REQUEST,
        CrmBody::Outcome(CommandOutcome {
            reason: Some("invalid_submission".to_string()),
            message: Some("The submission could not be processed.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// 403 for a typed grant gate the caller can act on.
fn refused(reason: &str, message: &str) -> (StatusCode, CrmBody) {
    (
        StatusCode::FORBIDDEN,
        CrmBody::Outcome(CommandOutcome {
            reason: Some(reason.to_string()),
            message: Some(message.to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 404 for a resource the caller cannot see under the scope it holds — a
/// contact outside `assigned` and a contact that does not exist are one
/// answer, exactly as TS's `ownedContact` makes them.
fn unknown_resource() -> (StatusCode, CrmBody) {
    (
        StatusCode::NOT_FOUND,
        CrmBody::Outcome(CommandOutcome {
            reason: Some("unknown_resource".to_string()),
            message: Some("No resource matches this request.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

// --- Actor model ---------------------------------------------------------------

/// Who is acting. The HTTP session gives a human; the suite (and, later, a
/// signed native runner) gives a service principal on behalf of a named human.
#[derive(Debug, Clone)]
pub enum CrmActor {
    /// A human the identity store authenticated; the value is the user id.
    Human(String),
    /// A service principal and the named human it acts for. Both must hold a
    /// CRM membership in the target tenant; the effective grant is their
    /// INTERSECTION ([`authorize`]), and every audit row records both ids.
    Service { id: String, on_behalf_of: String },
}

impl CrmActor {
    /// The principal whose membership gates the request (`actor_id`).
    pub fn principal_id(&self) -> &str {
        match self {
            CrmActor::Human(id) => id,
            CrmActor::Service { id, .. } => id,
        }
    }

    /// The human on whose behalf a service acts — `None` for a human actor,
    /// which is what makes the audit column and the status filter line up.
    pub fn initiator_id(&self) -> Option<&str> {
        match self {
            CrmActor::Human(_) => None,
            CrmActor::Service { on_behalf_of, .. } => Some(on_behalf_of),
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            CrmActor::Human(_) => "human",
            CrmActor::Service { .. } => "service",
        }
    }
}

/// Scope, mirroring the schema's CHECK.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum CrmScope {
    /// Every contact in the tenant.
    All,
    /// Only contacts whose `owner_id` is the access owner (the human id).
    Assigned,
}

impl CrmScope {
    fn parse(raw: &str) -> Result<Self, AuthorityError> {
        match raw {
            "all" => Ok(CrmScope::All),
            "assigned" => Ok(CrmScope::Assigned),
            // Operator-facing data problem: the log gets the value, the caller
            // gets an unavailability — never a defaulted guess.
            other => {
                tracing::error!(
                    value = %other,
                    "refusing: stored CRM scope outside its vocabulary"
                );
                Err(AuthorityError::UnusableGrant)
            }
        }
    }
}

/// The resolved authority for one tenant — the port of TS `CrmAccess`.
#[derive(Debug, Clone)]
pub(crate) struct CrmAccess {
    pub(crate) permissions: Vec<String>,
    pub(crate) owner_id: String,
    pub(crate) scope: CrmScope,
}

impl CrmAccess {
    pub(crate) fn allows(&self, permission: &str) -> bool {
        self.permissions.iter().any(|held| held == permission)
    }
}

/// One membership row as the authorization code reads it.
struct CrmMember {
    kind: String,
    permissions: Vec<String>,
    scope: CrmScope,
}

/// Reads the actor's membership through `archava_crm.lock_membership`, the
/// SECURITY DEFINER row-lock the baseline's kernel uses (`authorize`'s
/// `member()`): authorization takes a FOR SHARE lock so a concurrent
/// revocation is visible inside this transaction, without granting the
/// runtime any authority to change memberships.
///
/// Revoked and absent are deliberately the same answer (`None`): whether a
/// membership is `active=false` or missing row-wise is indistinguishable to
/// the caller, and telling them apart would be a membership-existence oracle.
/// Stored values outside their vocabularies (kind, scope, permissions) are
/// [`AuthorityError::UnusableGrant`] — fail closed.
async fn member_of(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    principal_id: &str,
) -> Result<Option<CrmMember>, AuthorityError> {
    let row = sqlx::query_as::<_, (String, Vec<String>, String, bool)>(
        "SELECT kind, permissions, scope, active FROM archava_crm.lock_membership($1, $2)",
    )
    .bind(tenant.as_str())
    .bind(principal_id)
    .fetch_optional(connection)
    .await
    .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))?;

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
            return Err(AuthorityError::UnusableGrant);
        }
    }
    for held in &permissions {
        if !PERMISSION_VOCABULARY.contains(&held.as_str()) {
            tracing::error!(
                value = %held,
                "refusing: stored CRM permission outside its vocabulary"
            );
            return Err(AuthorityError::UnusableGrant);
        }
    }
    Ok(Some(CrmMember {
        kind,
        permissions,
        scope: CrmScope::parse(&scope)?,
    }))
}

/// Resolves the actor's CRM authority for one tenant — the port of
/// `PostgresCrmRepository::authorize` (`packages/crm-pg/src/repository.ts:192-226`).
///
/// - A human's authority is its own row: own grants, own scope, owner = self.
/// - A service principal must name a human whose membership is real, active
///   and human-shaped. The effective grant is the INTERSECTION of the two
///   grant sets, the owner is the human (whose contacts are the ones `assigned`
///   scope reaches), and the effective scope is `assigned` if EITHER side
///   carries it. A service principal can therefore never widen: it acts only
///   inside what the human could have acted inside.
///
/// Every refusal shape collapses to [`AuthorityError::MissingMembership`]:
/// absent, inactive, wrong kind, and an unusable initiator are one answer —
/// the request was not authorized — so none is an existence oracle for another.
pub(crate) async fn authorize(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    actor: &CrmActor,
) -> Result<CrmAccess, AuthorityError> {
    let own = member_of(connection, tenant, actor.principal_id()).await?;
    let Some(own) = own else {
        return Err(AuthorityError::MissingMembership);
    };
    if own.kind != actor.kind() {
        // A human session standing on a service membership (or the reverse)
        // is exactly as unauthorized as having no membership at all.
        return Err(AuthorityError::MissingMembership);
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
                return Err(AuthorityError::MissingMembership);
            };
            if human.kind != "human" {
                return Err(AuthorityError::MissingMembership);
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
                // The human owns: `contact.create` stamps them owner and
                // `assigned`-scope containment reads against their id.
                owner_id: on_behalf_of.clone(),
                scope,
            })
        }
    }
}

/// The fingerprint a submission records: HMAC-SHA256 (keyed by the identity
/// store's secret, labelled `crm-fingerprint`) over the tenant, the actor
/// (kind + id + initiator) and the EXACT typed command as received. Two
/// submissions share a key only when they are, byte for byte, the same
/// command from the same actor for the same tenant — which is what makes the
/// replay branch a faithful replay and the reuse branch a genuine conflict.
///
/// The key order in the canonical document is fixed by construction (serde's
/// `json!` preserves insertion order and `to_string` is deterministic), so the
/// digest is stable across runs for the same input.
fn fingerprint(
    state: &AppState,
    tenant: &TenantId,
    actor: &CrmActor,
    command: &CrmCommand,
) -> String {
    let canonical = serde_json::json!({
        "tenant": tenant.as_str(),
        "actorKind": actor.kind(),
        "actorId": actor.principal_id(),
        "initiatorId": actor.initiator_id(),
        "command": serde_json::to_value(command)
            .expect("a typed command must serialize back to its own JSON"),
    });
    state
        .identity()
        .digest("crm-fingerprint", &canonical.to_string())
}

// --- Transaction helpers -------------------------------------------------------

/// Opens the tenant transaction with the baseline's per-transaction guards:
/// non-owner runtime first (inside [`TenantTransaction::begin`]), then the
/// `SET LOCAL` timeouts `packages/crm-pg/src/repository.ts` sets on every
/// transaction — statement timeout 10s, lock timeout 3s.
pub(crate) async fn begin(
    state: &AppState,
    tenant: TenantId,
) -> Result<TenantTransaction<'_>, AuthorityError> {
    let mut transaction = TenantTransaction::begin(
        state.pool(),
        CRM_LEDGER.namespace,
        CRM_LEDGER.ledger,
        tenant,
    )
    .await
    .map_err(AuthorityError::from)?;
    {
        let connection = transaction.checked_executor().await?;
        for statement in [
            "SET LOCAL statement_timeout='10s'",
            "SET LOCAL lock_timeout='3s'",
        ] {
            sqlx::query(statement)
                .execute(&mut *connection)
                .await
                .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))?;
        }
    }
    Ok(transaction)
}

/// A contact visible to this access, optionally locked `FOR UPDATE` — the
/// port of `ownedContact`. The scope filter is the SAME predicate the list
/// uses, so read-containment and write-containment cannot drift apart. The
/// SQL suffix is built from two fixed strings — nothing caller-supplied ever
/// interpolates.
async fn own_contact(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    access: &CrmAccess,
    contact_id: Uuid,
    lock: bool,
) -> Result<Option<i32>, AuthorityError> {
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
        .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))?;
    Ok(row.map(|(revision,)| revision))
}

/// The idempotency claim. `ON CONFLICT DO NOTHING RETURNING` answers `None`
/// when a row for this (tenant, request) key already exists — the caller then
/// decides, from the stored row, whether this was a faithful replay, a reuse,
/// or another actor's key.
async fn claim_command(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    request_id: Uuid,
    actor_id: &str,
    initiator_id: Option<&str>,
    fingerprint: &str,
) -> Result<bool, AuthorityError> {
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
    .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))?;
    Ok(claimed.is_some())
}

/// The ways one effect application can fail. `Timeline` carries a real
/// database failure — [`timeline_rejection`] reclassifies the caller-side
/// calendar mistakes among them. `LedgerMismatch` is the receipt update
/// landing on anything but the claim row this same transaction inserted —
/// a state nothing reachable can produce, refused instead of papered over.
pub(crate) enum ApplyFailure {
    Timeline(sqlx::Error),
    LedgerMismatch,
}

/// The follow-up effect, statement for statement the TS branch:
/// insert the followup, bump the contact's revision, append the audit row
/// (actor + initiator), record the receipt onto the claim. Runs on the
/// connection the caller opened — every statement is part of the caller's one
/// transaction, and the receipt lands before the transaction commits.
///
/// The `due_at` cast is the calendar authority: a value no database accepts
/// surfaces as SQLSTATE 22007/22008, which [`timeline_rejection`] maps to a
/// submission refusal.
/// The one new follow-up the command applies — everything gathered at the
/// call so the SQL reads as a document rather than ten positional parameters.
struct FollowupEffect<'a> {
    tenant: &'a TenantId,
    request_id: Uuid,
    actor_id: &'a str,
    initiator_id: Option<&'a str>,
    operation: &'a str,
    contact_id: Uuid,
    summary: &'a str,
    due_at: &'a str,
    followup_id: Uuid,
}

async fn apply_followup_create(
    connection: &mut sqlx::PgConnection,
    effect: FollowupEffect<'_>,
) -> Result<(), ApplyFailure> {
    let FollowupEffect {
        tenant,
        request_id,
        actor_id,
        initiator_id,
        operation,
        contact_id,
        summary,
        due_at,
        followup_id,
    } = effect;
    sqlx::query(
        "INSERT INTO archava_crm.followups(tenant_id,id,contact_id,summary,due_at) \
         VALUES($1,$2,$3,$4,$5::timestamptz)",
    )
    .bind(tenant.as_str())
    .bind(followup_id)
    .bind(contact_id)
    .bind(summary)
    .bind(due_at)
    .execute(&mut *connection)
    .await
    .map_err(ApplyFailure::Timeline)?;
    sqlx::query(
        "UPDATE archava_crm.contacts SET revision=revision+1, updated_at=now() \
         WHERE tenant_id=$1 AND id=$2",
    )
    .bind(tenant.as_str())
    .bind(contact_id)
    .execute(&mut *connection)
    .await
    .map_err(ApplyFailure::Timeline)?;
    sqlx::query(
        "INSERT INTO archava_crm.audit(tenant_id,request_id,actor_id,operation,resource_id,revision,initiator_id) \
         VALUES($1,$2,$3,$4,$5,1,$6)",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .bind(actor_id)
    .bind(operation)
    .bind(followup_id)
    .bind(initiator_id)
    .execute(&mut *connection)
    .await
    .map_err(ApplyFailure::Timeline)?;
    let receipt = serde_json::json!({
        "kind": "followup",
        "id": followup_id.to_string(),
        "revision": 1,
    });
    let recorded = sqlx::query(
        "UPDATE archava_crm.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2",
    )
    .bind(tenant.as_str())
    .bind(request_id)
    .bind(receipt)
    .execute(&mut *connection)
    .await
    .map_err(ApplyFailure::Timeline)?;
    // hit exactly it. Anything else is a ledger the caller can reason about.
    if recorded.rows_affected() != 1 {
        tracing::error!(
            updated = recorded.rows_affected(),
            "refusing: the claim row did not hold for its receipt"
        );
        return Err(ApplyFailure::LedgerMismatch);
    }
    Ok(())
}

/// True when a database error is a caller-side calendar mistake (the
/// timestamptz cast of the due time), which maps to a submission refusal —
/// an outage would misinform an operator and a 500 would invite a retry of a
/// request that will never become valid.
fn timeline_rejection(error: &sqlx::Error) -> bool {
    match error {
        sqlx::Error::Database(database) => {
            matches!(database.code().as_deref(), Some("22007") | Some("22008"))
        }
        _ => false,
    }
}

/// Validates a STORED receipt against its schema — the port of
/// `mutationReceiptSchema.parse`. Anything outside the four kinds, a
/// non-uuid id, or a revision below 1 is operator-facing data corruption
/// (every receipt in that column was written by a receipt-recording
/// transaction), so it refuses as [`AuthorityError::UnusableGrant`], never as
/// a guessed success shape. Returns the kind's own read permission.
fn stored_receipt(receipt: &Value) -> Result<(&'static str, Uuid), AuthorityError> {
    let kind = receipt
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(AuthorityError::UnusableGrant)?;
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
            return Err(AuthorityError::UnusableGrant);
        }
    };
    let id = receipt
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| Uuid::parse_str(id).ok())
        .ok_or(AuthorityError::UnusableGrant)?;
    let _revision = receipt
        .get("revision")
        .and_then(Value::as_i64)
        .filter(|revision| *revision > 0)
        .ok_or(AuthorityError::UnusableGrant)?;
    Ok((kind_permission, id))
}

/// Whether a STORED receipt of any kind resolves to a contact the CURRENT
/// access still sees — the port of `receiptResource`. Every receipt kind
/// resolves to a contact (contact receipts are the contact itself; activity
/// and followup receipts carry theirs; opportunity receipts carry theirs,
/// which migration 002's composite FK keeps owner-locked to the same
/// contact), and the final predicate is the SAME `ownedContact` filter the
/// list and the write path use — containment cannot drift from reading.
async fn receipt_visible(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    access: &CrmAccess,
    receipt: &Value,
    resource_id: Uuid,
) -> Result<bool, AuthorityError> {
    let kind = receipt
        .get("kind")
        .and_then(Value::as_str)
        .ok_or(AuthorityError::UnusableGrant)?;
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
        "followup" | "activity" => {
            let Some(contact_id) =
                resource_contact(connection, tenant, "followups", resource_id).await?
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
            return Err(AuthorityError::UnusableGrant);
        }
    };
    own_contact(connection, tenant, access, contact_id, false)
        .await
        .map(|found| found.is_some())
}

// --- Contacts read -------------------------------------------------------------

/// `GET /contacts` — the session-scoped contact list (the slice replaces the
/// pre-slice placeholder denial on this path; that refusal existed because
/// the read was not yet session-scoped). Port of `PostgresCrmRepository::list`
/// (`packages/crm-pg/src/repository.ts:386-405`).
///
/// The caller's own tenant and CRM grants decide what comes back: no
/// membership, one refusal; `assigned` scope, only the contacts they own;
/// the search string's `%`, `_` and `\` stay literals (`ESCAPE '\'`), which
/// is what the baseline's `search` does.
pub(crate) async fn contacts(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<ContactsPageQuery>, QueryRejection>,
) -> (StatusCode, CrmBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    match list_contacts(&state, &presented, &query).await {
        Ok(answer) => answer,
        Err(error) => outcome(authority_refusal(&error)),
    }
}

async fn list_contacts(
    state: &AppState,
    presented: &AuthenticatedSession,
    query: &ContactsPageQuery,
) -> Result<(StatusCode, CrmBody), AuthorityError> {
    let limit: i64 = match query.limit {
        None => 20,
        Some(limit) if (1..=100).contains(&limit) => i64::from(limit),
        // TS `pageSchema`'s `.int().min(1).max(100)` — out of range is a
        // caller mistake, refused before any database work.
        Some(_) => return Ok(invalid_submission()),
    };
    let mut transaction = begin(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match authorize(connection, &query.tenant, &human(presented)).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        }
    };
    if !access.allows(Permission::CONTACTS_READ) {
        let _ = transaction.rollback().await;
        return Ok(refused(
            "role_not_allowed",
            "The contact list requires a contact read grant.",
        ));
    }

    // The baseline's exact escaping: the caller's `\`, `%` and `_` are
    // literals, so a search value can never widen into a wildcard scan.
    let search = query
        .search
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    let rows = {
        let connection = transaction.checked_executor().await?;
        sqlx::query_as::<
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
        .bind(query.tenant.as_str())
        .bind(access.scope == CrmScope::All)
        .bind(&access.owner_id)
        .bind(query.after)
        .bind(format!("%{search}%"))
        .bind(limit + 1)
        .fetch_all(connection)
        .await
        .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))
    };
    let rows = match rows {
        Ok(rows) => rows,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(error);
        }
    };

    // Same pagination as `list()`: cap at `limit`; `next` is the last emitted
    // id only when a further row existed after it.
    let items: Vec<CrmContact> = rows
        .iter()
        .take(limit as usize)
        .map(
            |(id, full_name, email, organization, owner_id, revision, created, updated)| {
                CrmContact {
                    id: id.to_string(),
                    tenant_id: query.tenant.as_str().to_string(),
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
    let _ = transaction.rollback().await;
    Ok((StatusCode::OK, CrmBody::Page(CrmPageValue { items, next })))
}

// --- Command submit ------------------------------------------------------------

/// `POST /crm/commands` — submit (or replay) a reviewed follow-up. CSRF-bound
/// like the commerce write routes; the tenant comes only from the body.
pub(crate) async fn crm_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<CrmSubmission>, JsonRejection>,
) -> (StatusCode, CrmBody) {
    let submission = match submission {
        Ok(submission) => submission,
        Err(rejected) => return outcome(rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = CrmActor::Human(presented.principal_id.clone());
    match submit_followup(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => outcome(authority_refusal(&error)),
    }
}

/// The follow-up creation — the slice's one real business command. Order and
/// shapes port `runMutation`'s `followup.create` branch
/// (`packages/crm-pg/src/repository.ts:636-768`).
///
/// One transaction for the whole request: authority, grant gate, idempotency
/// claim, resource containment, effect, audit, receipt. It commits in exactly
/// one place; every refusal rolls back explicitly, so no path can commit a
/// half-applied command.
pub async fn submit_followup(
    state: &AppState,
    actor: CrmActor,
    submission: CrmSubmission,
) -> Result<(StatusCode, CrmBody), AuthorityError> {
    // Boundary validation the TS zod layer does at parse time: lengths and
    // revision bounds refuse before any database work; the calendar stays the
    // database's authority (a mis-formed instant is refused as a submission
    // by the code that sees SQLSTATE, not surfaced as an outage).
    let summary_length = submission.command.summary().chars().count();
    if !(1..=2000).contains(&summary_length) {
        return Ok(invalid_submission());
    }
    if submission.command.contact_revision() < 1 {
        return Ok(invalid_submission());
    }
    let fingerprint = fingerprint(state, &submission.tenant, &actor, &submission.command);
    let operation = "followup.create";
    let contact_id = submission.command.contact_id();
    let contact_revision = submission.command.contact_revision();

    let mut transaction = begin(state, submission.tenant.clone()).await?;

    // 1. The grant — from the membership row, never the body. `runMutation`
    //    computes the same gate from the operation prefix (`followups.write`
    //    for every followup operation).
    let access = {
        let connection = transaction.checked_executor().await?;
        match authorize(connection, &submission.tenant, &actor).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        }
    };
    if !access.allows(Permission::FOLLOWUPS_WRITE) {
        let _ = transaction.rollback().await;
        return Ok(refused(
            "role_not_allowed",
            "The follow-up requires a followup write grant.",
        ));
    }

    // 2. The idempotency claim. `ON CONFLICT DO NOTHING` loses the claim to
    //    the row that already holds the key; that branch reads the stored row
    //    IN THE SAME TRANSACTION (TS does the same) and decides between
    //    faithful replay, reuse and another actor's key.
    let claimed = {
        let connection = transaction.checked_executor().await?;
        claim_command(
            connection,
            &submission.tenant,
            submission.request_id,
            actor.principal_id(),
            actor.initiator_id(),
            &fingerprint,
        )
        .await
    };
    let claimed_short = match claimed {
        Ok(claimed) => claimed,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(error);
        }
    };
    if !claimed_short {
        let answer = replay_or_conflict(
            &mut transaction,
            &submission.tenant,
            submission.request_id,
            actor.principal_id(),
            actor.initiator_id(),
            &fingerprint,
            &access,
        )
        .await;
        let _ = transaction.rollback().await;
        return Ok(answer);
    }

    // 3. The resource: read FOR UPDATE under the caller's scope. A contact
    //    outside `assigned` and a contact that does not exist are one answer
    //    (`unknown_resource`), because to an assigned caller they are
    //    indistinguishable — exactly TS's `ownedContact` property.
    let contact = {
        let connection = transaction.checked_executor().await?;
        own_contact(connection, &submission.tenant, &access, contact_id, true).await
    };
    let contact = match contact {
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(error);
        }
        Ok(None) => {
            let _ = transaction.rollback().await;
            return Ok(unknown_resource());
        }
        Ok(Some(revision)) => revision,
    };
    if i64::from(contact) != contact_revision {
        let _ = transaction.rollback().await;
        return Ok((
            StatusCode::CONFLICT,
            CrmBody::Outcome(CommandOutcome {
                reason: Some("revision_changed".to_string()),
                message: Some(
                    "A concurrent edit changed the resource; re-read and try again.".to_string(),
                ),
                ..CommandOutcome::base("refused")
            }),
        ));
    }

    // 4. The effect: followup row, contact revision bump, audit entry
    //    (actor + initiator), receipt on the claim — then the single commit.
    let followup_id = Uuid::new_v4();
    {
        let connection = transaction.checked_executor().await?;
        let applied = apply_followup_create(
            connection,
            FollowupEffect {
                tenant: &submission.tenant,
                request_id: submission.request_id,
                actor_id: actor.principal_id(),
                initiator_id: actor.initiator_id(),
                operation,
                contact_id,
                summary: submission.command.summary(),
                due_at: submission.command.due_at(),
                followup_id,
            },
        )
        .await;
        if let Err(error) = applied {
            let _ = transaction.rollback().await;
            match error {
                ApplyFailure::Timeline(timeline) => {
                    if timeline_rejection(&timeline) {
                        tracing::info!(
                            "refused: the due time is not an instant this calendar holds"
                        );
                        return Ok(invalid_submission());
                    }
                    return Err(AuthorityError::database(CRM_LEDGER.ledger, timeline));
                }
                // Nothing reachable produces this; the refusal it earns is
                // operator-facing (the log already named the row count).
                ApplyFailure::LedgerMismatch => return Err(AuthorityError::UnusableGrant),
            }
        }
    }
    transaction.commit().await.map_err(AuthorityError::from)?;

    Ok((
        StatusCode::OK,
        CrmBody::Outcome(CommandOutcome {
            tenant: Some(submission.tenant.as_str().to_string()),
            request_id: Some(submission.request_id),
            receipt: Some(serde_json::json!({
                "kind": "followup",
                "id": followup_id.to_string(),
                "revision": 1,
            })),
            ..CommandOutcome::base("receipt")
        }),
    ))
}

/// The claim that already belonged to a row, decided from the stored row
/// (still inside the caller's transaction, as TS does):
///
/// - another actor's key → 404 `unknown_request` (existence is not disclosed);
/// - the caller's key with a different fingerprint → 409 `request_id_reused`,
///   carrying NO receipt (the recorded command is not this one and its receipt
///   belongs to that other command);
/// - the caller's key with the same fingerprint → replay: the grant and read
///   containment are re-derived against CURRENT authority (TS runs
///   `receiptResource` on replay too), then the stored receipt returns as
///   `decision:"receipt"` with the STORED fingerprint, so a caller can prove
///   the effect it got back is its own command's;
/// - a match with no receipt → `unconfirmed` (a committed claim without a
///   receipt cannot exist — receipts land before commit — so this is the
///   fail-closed answer, never a fabricated success).
async fn replay_or_conflict(
    transaction: &mut TenantTransaction<'_>,
    tenant: &TenantId,
    request_id: Uuid,
    actor_id: &str,
    initiator_id: Option<&str>,
    fingerprint: &str,
    access: &CrmAccess,
) -> (StatusCode, CrmBody) {
    let read = {
        let connection = match transaction.checked_executor().await {
            Ok(connection) => connection,
            Err(error) => return outcome(authority_refusal(&AuthorityError::from(error))),
        };
        sqlx::query_as::<_, (String, Option<Value>, String)>(
            "SELECT actor_id, receipt, fingerprint FROM archava_crm.commands \
             WHERE tenant_id=$1 AND request_id=$2 AND initiator_id IS NOT DISTINCT FROM $3",
        )
        .bind(tenant.as_str())
        .bind(request_id)
        .bind(initiator_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))
    };
    let existing = match read {
        Err(error) => return outcome(authority_refusal(&error)),
        Ok(existing) => existing,
    };
    let Some((existing_actor, receipt, existing_fingerprint)) = existing else {
        // The claim lost against a row that is not visible to this
        // snapshot: an in-flight transaction INSERTed the same key but has
        // not committed. `unconfirmed` — absence (visibility) is not proof
        // of failure.
        tracing::info!(
            %request_id,
            "answering unconfirmed: the key is claimed by an in-flight submission"
        );
        return (
            StatusCode::OK,
            CrmBody::Status(CrmStatusValue {
                status: "unconfirmed",
                request_id,
                receipt: None,
            }),
        );
    };
    if existing_actor != actor_id {
        return outcome(unknown_request());
    }
    if existing_fingerprint != fingerprint {
        return (
            StatusCode::CONFLICT,
            CrmBody::Outcome(CommandOutcome {
                reason: Some("request_id_reused".to_string()),
                message: Some("This request id already records a different command.".to_string()),
                ..CommandOutcome::base("refused")
            }),
        );
    }
    let Some(receipt) = receipt else {
        return (
            StatusCode::OK,
            CrmBody::Status(CrmStatusValue {
                status: "unconfirmed",
                request_id,
                receipt: None,
            }),
        );
    };
    // Replay re-derives authority against CURRENT grants and containment
    // against the CURRENT scope state: a replay is a fresh authorization of
    // an already-recorded effect, not a bypass of the gate.
    let (kind_permission, resource_id) = match stored_receipt(&receipt) {
        Ok(found) => found,
        Err(error) => return outcome(authority_refusal(&error)),
    };
    if !access.allows(kind_permission) || !access.allows(Permission::CONTACTS_READ) {
        return refused("role_not_allowed", "CRM receipt access is not permitted.");
    }
    {
        let connection = match transaction.checked_executor().await {
            Ok(connection) => connection,
            Err(error) => return outcome(authority_refusal(&AuthorityError::from(error))),
        };
        match receipt_visible(connection, tenant, access, &receipt, resource_id).await {
            Err(error) => return outcome(authority_refusal(&error)),
            Ok(false) => return unknown_resource(),
            Ok(true) => (),
        }
    }
    (
        StatusCode::OK,
        CrmBody::Outcome(CommandOutcome {
            tenant: Some(tenant.as_str().to_string()),
            request_id: Some(request_id),
            receipt: Some(receipt),
            fingerprint: Some(existing_fingerprint),
            ..CommandOutcome::base("receipt")
        }),
    )
}

/// The contact a recorded non-contact receipt resolves through. The SQL string
/// interpolates only a comparison against two fixed table names, never any
/// caller value.
async fn resource_contact(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    table: &str,
    resource_id: Uuid,
) -> Result<Option<Uuid>, AuthorityError> {
    debug_assert!(
        matches!(table, "followups" | "opportunities" | "activities"),
        "only a fixed table name may reach this query"
    );
    if !matches!(table, "followups" | "opportunities" | "activities") {
        return Err(AuthorityError::UnusableGrant);
    }
    let sql = format!("SELECT contact_id FROM archava_crm.{table} WHERE tenant_id=$1 AND id=$2");
    let row: Option<(Uuid,)> = sqlx::query_as(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(tenant.as_str())
        .bind(resource_id)
        .fetch_optional(connection)
        .await
        .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source))?;
    Ok(row.map(|(contact_id,)| contact_id))
}

// --- Authorized recovery -------------------------------------------------------

/// `GET /crm/commands/status` — authorized recovery (TS `commandStatus`):
/// `committed` with the receipt, or `unconfirmed` when the key has no visible
/// committed row. GET, and therefore not CSRF-bound — a read that changes
/// nothing; the commerce status route drew the same line.
pub(crate) async fn command_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<StatusQuery>, QueryRejection>,
) -> (StatusCode, CrmBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    match read_status(&state, &presented, None, &query).await {
        Ok(answer) => answer,
        Err(error) => outcome(authority_refusal(&error)),
    }
}

/// The recovery read — port of `readCommandStatus`
/// (`packages/crm-pg/src/repository.ts:267-324`). `initiator` is the value the
/// stored row must carry: `None` for the human route (a service's key is not
/// recoverable here), the initiating human's id for a future service route.
///
/// Permission + containment gates re-derive against CURRENT authority before
/// the receipt is shown, exactly like the TS: the stored receipt grants
/// nothing; the membership row the call sees today does.
async fn read_status(
    state: &AppState,
    presented: &AuthenticatedSession,
    initiator: Option<&str>,
    query: &StatusQuery,
) -> Result<(StatusCode, CrmBody), AuthorityError> {
    let mut transaction = begin(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match authorize(connection, &query.tenant, &human(presented)).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        }
    };
    let row = {
        let connection = transaction.checked_executor().await?;
        let queried = sqlx::query_as::<_, (Option<Value>,)>(
            "SELECT receipt FROM archava_crm.commands \
             WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3 \
               AND initiator_id IS NOT DISTINCT FROM $4",
        )
        .bind(query.tenant.as_str())
        .bind(query.request_id)
        .bind(&presented.principal_id)
        .bind(initiator)
        .fetch_optional(connection)
        .await
        .map_err(|source| AuthorityError::database(CRM_LEDGER.ledger, source));
        match queried {
            Ok(row) => row,
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
        }
    };
    let Some((Some(receipt),)) = row else {
        // No row, or a claimed row without its receipt — the two answers are
        // the same: `unconfirmed`, because "may still be running" and "no
        // such key" are indistinguishable from a caller's chair, and the
        // contracts file states the rule: absence is no proof of failure.
        let _ = transaction.rollback().await;
        return Ok((
            StatusCode::OK,
            CrmBody::Status(CrmStatusValue {
                status: "unconfirmed",
                request_id: query.request_id,
                receipt: None,
            }),
        ));
    };
    let (kind_permission, resource_id) = match stored_receipt(&receipt) {
        Ok(found) => found,
        Err(error) => {
            let _ = transaction.rollback().await;
            return Err(error);
        }
    };
    if !access.allows(kind_permission) || !access.allows(Permission::CONTACTS_READ) {
        let _ = transaction.rollback().await;
        return Ok(refused(
            "role_not_allowed",
            "CRM receipt access is not permitted.",
        ));
    }
    // Containment: every receipt kind resolves to a contact the CURRENT scope
    // still sees. A revoked assignment hiding the contact hides the receipt.
    {
        let connection = transaction.checked_executor().await?;
        match receipt_visible(connection, &query.tenant, &access, &receipt, resource_id).await {
            Err(error) => {
                let _ = transaction.rollback().await;
                return Err(error);
            }
            Ok(false) => {
                let _ = transaction.rollback().await;
                return Ok(unknown_resource());
            }
            Ok(true) => (),
        }
    };
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        CrmBody::Status(CrmStatusValue {
            status: "committed",
            request_id: query.request_id,
            receipt: Some(receipt),
        }),
    ))
}

/// The session's principal as a human actor.
pub(crate) fn human(presented: &AuthenticatedSession) -> CrmActor {
    CrmActor::Human(presented.principal_id.clone())
}
