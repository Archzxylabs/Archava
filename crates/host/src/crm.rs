//! §1.4B's vertical-slice backend: the CRM serving state the slice UI talks to.
//!
//! ## What this module is
//!
//! The HTTP boundary and coordination layer for CRM operations.
//! Transactional persistence, RLS enforcement, idempotency claims, and SQL statements
//! are extracted to [`archava_adapters::crm`].
//!
//! This module coordinates:
//! - Route extraction and input parsing (`/contacts`, `/crm/records`, `/crm/commands`, `/crm/commands/status`).
//! - CSRF and session authentication verification.
//! - Tenant configuration / optional module enablement (`crm-pipeline`).
//! - HMAC fingerprint generation for idempotency verification.
//! - Mapping adapter persistence outcomes and domain refusals to HTTP status codes.

use crate::authority::AuthorityError;
use crate::identity::AuthenticatedSession;
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, unknown_request, AppState,
    CommandOutcome, Ledger,
};
use archava_adapters::crm::{
    CrmPersistenceError, CRM_LEDGER_NAME, CRM_LEDGER_NAMESPACE, CRM_TABLES,
};
use archava_adapters::rls::TenantTransaction;
use archava_core::tenant::TenantId;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;

// Re-export CRM domain types and adapter vocabulary so callers and test suites
// continue to resolve them from `archava_host::crm`.
pub use archava_adapters::crm::{
    member_of, transition_allowed, ActivityKind, ContactFields, ContactPatch, ContactsPageQuery,
    CrmAccess, CrmActor, CrmCommand, CrmContact, CrmFollowup, CrmOpportunity, CrmPageValue,
    CrmRecordValue, CrmScope, CrmStatusValue, CrmSubmission, Permission, PipelineStage,
    RecordQuery, StatusQuery,
};

/// Resolves the actor's CRM authority for one tenant, mapped to [`AuthorityError`].
pub async fn authorize(
    connection: &mut sqlx::PgConnection,
    tenant: &TenantId,
    actor: &CrmActor,
) -> Result<CrmAccess, AuthorityError> {
    archava_adapters::crm::authorize(connection, tenant, actor)
        .await
        .map_err(AuthorityError::from)
}

/// The CRM ledger — every tenant-scoped table migrations 001 + 002 force RLS on.
pub(crate) const CRM_LEDGER: Ledger = Ledger {
    namespace: CRM_LEDGER_NAMESPACE,
    ledger: CRM_LEDGER_NAME,
    tables: CRM_TABLES,
};

// --- Response shapes -----------------------------------------------------------

/// The union body this module's routes answer with.
pub enum CrmBody {
    Outcome(CommandOutcome),
    Page(CrmPageValue),
    Status(CrmStatusValue),
    Record(CrmRecordValue),
}

impl IntoResponse for CrmBody {
    fn into_response(self) -> Response {
        match self {
            CrmBody::Outcome(value) => Json(value).into_response(),
            CrmBody::Page(value) => Json(value).into_response(),
            CrmBody::Status(value) => Json(value).into_response(),
            CrmBody::Record(value) => Json(value).into_response(),
        }
    }
}

/// Levels a `Json<CommandOutcome>` answer up to this module's body type.
fn outcome(answer: (StatusCode, Json<CommandOutcome>)) -> (StatusCode, CrmBody) {
    (answer.0, CrmBody::Outcome(answer.1 .0))
}

/// 400 for a syntactically valid document the host refuses as a submission.
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

/// 404 for a resource the caller cannot see under the scope it holds.
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

/// 409 for a bind that moved: the caller re-reads and tries again.
fn revision_changed() -> (StatusCode, CrmBody) {
    (
        StatusCode::CONFLICT,
        CrmBody::Outcome(CommandOutcome {
            reason: Some("revision_changed".to_string()),
            message: Some(
                "A concurrent edit changed the resource; re-read and try again.".to_string(),
            ),
            ..CommandOutcome::base("refused")
        }),
    )
}

/// 409 for a pipeline move the state machine does not allow.
fn transition_refused() -> (StatusCode, CrmBody) {
    (
        StatusCode::CONFLICT,
        CrmBody::Outcome(CommandOutcome {
            reason: Some("transition_not_allowed".to_string()),
            message: Some("The pipeline does not allow that stage change.".to_string()),
            ..CommandOutcome::base("refused")
        }),
    )
}

impl From<CrmPersistenceError> for AuthorityError {
    fn from(error: CrmPersistenceError) -> Self {
        match error {
            CrmPersistenceError::TenantContext(ctx) => AuthorityError::Context(ctx),
            CrmPersistenceError::MissingMembership => AuthorityError::MissingMembership,
            CrmPersistenceError::UnusableGrant => AuthorityError::UnusableGrant,
            CrmPersistenceError::Database { ledger, source } => {
                AuthorityError::database(ledger, source)
            }
            _ => AuthorityError::UnusableGrant,
        }
    }
}

fn map_crm_error(err: CrmPersistenceError) -> (StatusCode, CrmBody) {
    match err {
        CrmPersistenceError::TenantContext(ctx) => {
            outcome(authority_refusal(&AuthorityError::Context(ctx)))
        }
        CrmPersistenceError::MissingMembership => {
            outcome(authority_refusal(&AuthorityError::MissingMembership))
        }
        CrmPersistenceError::UnusableGrant => {
            outcome(authority_refusal(&AuthorityError::UnusableGrant))
        }
        CrmPersistenceError::RoleNotAllowed(msg) => refused("role_not_allowed", msg),
        CrmPersistenceError::UnknownResource => unknown_resource(),
        CrmPersistenceError::UnknownRequest => outcome(unknown_request()),
        CrmPersistenceError::RequestIdReused => (
            StatusCode::CONFLICT,
            CrmBody::Outcome(CommandOutcome {
                reason: Some("request_id_reused".to_string()),
                message: Some("This request id already records a different command.".to_string()),
                ..CommandOutcome::base("refused")
            }),
        ),
        CrmPersistenceError::RevisionChanged => revision_changed(),
        CrmPersistenceError::TransitionNotAllowed => transition_refused(),
        CrmPersistenceError::TimelineRejection | CrmPersistenceError::InvalidSubmission => {
            invalid_submission()
        }
        CrmPersistenceError::LedgerMismatch => {
            outcome(authority_refusal(&AuthorityError::UnusableGrant))
        }
        CrmPersistenceError::Database { ledger, source } => {
            outcome(authority_refusal(&AuthorityError::database(ledger, source)))
        }
    }
}

// --- Transaction helper --------------------------------------------------------

/// Opens the tenant transaction with CRM guards: statement timeout 10s, lock timeout 3s.
pub(crate) async fn begin(
    state: &AppState,
    tenant: TenantId,
) -> Result<TenantTransaction<'_>, AuthorityError> {
    archava_adapters::crm::begin(state.pool(), tenant)
        .await
        .map_err(AuthorityError::from)
}

/// The session's principal as a human actor.
pub(crate) fn human(presented: &AuthenticatedSession) -> CrmActor {
    CrmActor::Human(presented.principal_id.clone())
}

/// Computes the stable fingerprint for a submission.
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

// --- Handlers ------------------------------------------------------------------

/// `GET /contacts` — the session-scoped contact list.
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
    let limit: i64 = match query.limit {
        None => 20,
        Some(limit) if (1..=100).contains(&limit) => i64::from(limit),
        Some(_) => return invalid_submission(),
    };
    let mut transaction = match begin(&state, query.tenant.clone()).await {
        Ok(tx) => tx,
        Err(err) => return outcome(authority_refusal(&err)),
    };
    let access = {
        let connection = match transaction.checked_executor().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&AuthorityError::from(err)));
            }
        };
        match authorize(connection, &query.tenant, &human(&presented)).await {
            Ok(access) => access,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&err));
            }
        }
    };

    match archava_adapters::crm::list_contacts(
        &mut transaction,
        &query.tenant,
        &access,
        &query.search,
        query.after,
        limit,
    )
    .await
    {
        Ok(page) => {
            let _ = transaction.rollback().await;
            (StatusCode::OK, CrmBody::Page(page))
        }
        Err(err) => {
            let _ = transaction.rollback().await;
            map_crm_error(err)
        }
    }
}

/// `GET /crm/records` — one contact and the records bound to it.
pub(crate) async fn record(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<RecordQuery>, QueryRejection>,
) -> (StatusCode, CrmBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let mut transaction = match begin(&state, query.tenant.clone()).await {
        Ok(tx) => tx,
        Err(err) => return outcome(authority_refusal(&err)),
    };
    let access = {
        let connection = match transaction.checked_executor().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&AuthorityError::from(err)));
            }
        };
        match authorize(connection, &query.tenant, &human(&presented)).await {
            Ok(access) => access,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&err));
            }
        }
    };
    let pipeline_enabled = {
        let connection = match transaction.checked_executor().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&AuthorityError::from(err)));
            }
        };
        matches!(
            crate::own::module_enabled(connection, &query.tenant, "crm-pipeline").await,
            Ok(true)
        )
    };

    match archava_adapters::crm::read_record(
        &mut transaction,
        &query.tenant,
        &access,
        query.contact,
        pipeline_enabled,
    )
    .await
    {
        Ok(record_val) => {
            let _ = transaction.rollback().await;
            (StatusCode::OK, CrmBody::Record(record_val))
        }
        Err(err) => {
            let _ = transaction.rollback().await;
            map_crm_error(err)
        }
    }
}

/// `POST /crm/commands` — submit (or replay) a CRM command.
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

/// Kept for test suite direct calls: delegates to [`submit_command`].
pub async fn submit_followup(
    state: &AppState,
    actor: CrmActor,
    submission: CrmSubmission,
) -> Result<(StatusCode, CrmBody), AuthorityError> {
    submit_command(state, actor, submission).await
}

/// Executes a CRM command through verification, idempotency claim, effect, and audit receipt.
pub async fn submit_command(
    state: &AppState,
    actor: CrmActor,
    mut submission: CrmSubmission,
) -> Result<(StatusCode, CrmBody), AuthorityError> {
    submission.command.normalize();
    if !submission.command.valid() {
        return Ok(invalid_submission());
    }
    let fingerprint = fingerprint(state, &submission.tenant, &actor, &submission.command);

    let mut transaction = begin(state, submission.tenant.clone()).await?;

    // 1. Authorize actor
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
    if !access.allows(submission.command.permission()) {
        let _ = transaction.rollback().await;
        return Ok(refused(
            "role_not_allowed",
            "The command requires its write grant.",
        ));
    }
    if submission.command.needs_contacts_read() && !access.allows(Permission::CONTACTS_READ) {
        let _ = transaction.rollback().await;
        return Ok(refused("role_not_allowed", "Contact access is required."));
    }

    // 2. Module gate
    if submission.command.needs_pipeline_module() {
        let enabled = {
            let connection = transaction.checked_executor().await?;
            crate::own::module_enabled(connection, &submission.tenant, "crm-pipeline").await
        };
        match enabled {
            Ok(true) => {}
            Ok(false) => {
                let _ = transaction.rollback().await;
                return Ok(refused(
                    "module_disabled",
                    "The CRM pipeline module is disabled for this tenant.",
                ));
            }
            Err(_) => {
                let _ = transaction.rollback().await;
                return Ok(refused(
                    "owner_unavailable",
                    "The owner configuration is unavailable; nothing was changed.",
                ));
            }
        }
    }

    // 3. Delegate execution to transactional adapter
    let outcome = archava_adapters::crm::execute_command(
        &mut transaction,
        &submission.tenant,
        &actor,
        &access,
        submission.request_id,
        &submission.command,
        &fingerprint,
    )
    .await;

    match outcome {
        Ok(archava_adapters::crm::CommandExecution::Committed {
            receipt,
            is_replay: false,
            ..
        }) => {
            transaction.commit().await.map_err(AuthorityError::from)?;
            Ok((
                StatusCode::OK,
                CrmBody::Outcome(CommandOutcome {
                    tenant: Some(submission.tenant.as_str().to_string()),
                    request_id: Some(submission.request_id),
                    receipt: Some(receipt),
                    ..CommandOutcome::base("receipt")
                }),
            ))
        }
        Ok(archava_adapters::crm::CommandExecution::Committed {
            receipt,
            is_replay: true,
            fingerprint: stored_fp,
        }) => {
            let _ = transaction.rollback().await;
            Ok((
                StatusCode::OK,
                CrmBody::Outcome(CommandOutcome {
                    tenant: Some(submission.tenant.as_str().to_string()),
                    request_id: Some(submission.request_id),
                    receipt: Some(receipt),
                    fingerprint: stored_fp,
                    ..CommandOutcome::base("receipt")
                }),
            ))
        }
        Ok(archava_adapters::crm::CommandExecution::Unconfirmed { request_id }) => {
            let _ = transaction.rollback().await;
            Ok((
                StatusCode::OK,
                CrmBody::Status(CrmStatusValue {
                    status: "unconfirmed",
                    request_id,
                    receipt: None,
                }),
            ))
        }
        Err(err) => {
            let _ = transaction.rollback().await;
            Ok(map_crm_error(err))
        }
    }
}

/// `GET /crm/commands/status` — authorized command status recovery.
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
    let mut transaction = match begin(&state, query.tenant.clone()).await {
        Ok(tx) => tx,
        Err(err) => return outcome(authority_refusal(&err)),
    };
    let access = {
        let connection = match transaction.checked_executor().await {
            Ok(conn) => conn,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&AuthorityError::from(err)));
            }
        };
        match authorize(connection, &query.tenant, &human(&presented)).await {
            Ok(access) => access,
            Err(err) => {
                let _ = transaction.rollback().await;
                return outcome(authority_refusal(&err));
            }
        }
    };

    let status_val = archava_adapters::crm::read_status(
        &mut transaction,
        &query.tenant,
        &access,
        &presented.principal_id,
        None,
        query.request_id,
    )
    .await;

    let _ = transaction.rollback().await;
    match status_val {
        Ok(status) => (StatusCode::OK, CrmBody::Status(status)),
        Err(err) => map_crm_error(err),
    }
}
