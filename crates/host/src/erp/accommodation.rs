//! The accommodation routes — the Archxy 2 port of the R05 second-cohort
//! mutation surface against `archava_accommodation`, in the shape
//! [`crate::erp::purchasing`] pinned for this host: extraction → session →
//! ONE tenant transaction whose member ladder, authorization, idempotency
//! claim, effect and audit all run on the adapters' step functions.
//!
//! ## The answers the two commands give
//!
//! - `POST /erp/accommodation/commands` — the TS api answers
//!   `json(await service.mutate(...))`: the BARE receipt document
//!   (`{kind, id, propertyId, revision}` — the accommodation receipt has
//!   no `replayed` member, so a replay is wire-identical to the original),
//!   or one of the repository's refusal codes as HTTP code + fixed text:
//!   forbidden 403 (`denied`), missing record 404 (`denied`/
//!   `unknown_resource`), conflict 409 (`refused` — stale revisions, hold
//!   overlapped through the kernel's 23P01 verdict, terminal/expired
//!   stays, unresolvable DST instants, idempotency reuse), malformed
//!   submission 400 (`rejected`).
//! - `GET /erp/accommodation/commands/status?tenant=&requestId=` — the TS
//!   `commandStatus` document verbatim: `committed` with its receipt, or
//!   `not_committed`. The commit only exists once the effect, audit and
//!   receipt rows share one transaction; a lost/corrupt receipt is a
//!   conflict, never a fabricated empty answer.
//!
//! Both routes are session-bound (`authenticate`), CSRF-bound on the
//! write, and the tenant comes only from the caller's own document — the
//! database, not the session, has the final word on whether the principal
//! belongs to it.

use archava_domain::erp::accommodation::{parse_mutation, AccommodationMutation};
use archava_domain::erp::purchasing::PurchasePrincipal;
use archava_domain::erp::ErpError;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::erp::{begin_accommodation, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};
use archava_adapters::erp::accommodation::{
    accommodation_command_status, record_accommodation_command, resolve_accommodation_context,
};
use archava_adapters::erp::PurchaseError;
use archava_core::tenant::TenantId;

// --- Response envelope ------------------------------------------------------

/// The union body this module's routes answer with — same shape as the
/// purchasing module's, module-local by the codebase's per-module rule.
pub enum ErpBody {
    Doc(Value),
    Outcome(CommandOutcome),
}

impl IntoResponse for ErpBody {
    fn into_response(self) -> Response {
        match self {
            ErpBody::Doc(value) => Json(value).into_response(),
            ErpBody::Outcome(value) => Json(value).into_response(),
        }
    }
}

/// Levels a host `Json<CommandOutcome>` answer up to this module's type.
fn outcome(answer: (StatusCode, Json<CommandOutcome>)) -> (StatusCode, ErpBody) {
    (answer.0, ErpBody::Outcome(answer.1 .0))
}

/// 400 for a document the boundary parse refuses.
fn invalid_submission() -> (StatusCode, ErpBody) {
    (
        StatusCode::BAD_REQUEST,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("invalid_submission".to_string()),
            message: Some("The submission could not be processed.".to_string()),
            ..CommandOutcome::base("rejected")
        }),
    )
}

/// 403 for a grant gate the caller can act on.
fn forbidden() -> (StatusCode, ErpBody) {
    (
        StatusCode::FORBIDDEN,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("forbidden".to_string()),
            message: Some("This session cannot perform that operation.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 404 for a record the caller cannot see or that no longer exists.
fn unknown_resource() -> (StatusCode, ErpBody) {
    (
        StatusCode::NOT_FOUND,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("unknown_resource".to_string()),
            message: Some("No resource matches this request.".to_string()),
            ..CommandOutcome::base("denied")
        }),
    )
}

/// 409 for the idempotency/transition conflicts the repository rejects —
/// stale revisions, the occupancy kernel's overlap verdict, expired
/// holds, terminal stays, unresolvable local times, claim reuse.
fn conflict() -> (StatusCode, ErpBody) {
    (
        StatusCode::CONFLICT,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("conflict".to_string()),
            message: Some(
                "A concurrent write or a state boundary refused this submission.".to_string(),
            ),
            ..CommandOutcome::base("refused")
        }),
    )
}

fn map_purchase_result(error: PurchaseError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        PurchaseError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        PurchaseError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        PurchaseError::Domain(ErpError::Conflict) => Ok(conflict()),
        PurchaseError::Domain(ErpError::Unavailable) => Ok(conflict()),
        PurchaseError::Database { ledger, source } => Err(ErpGateError::Database(ledger, source)),
    }
}

fn gate_outcome(error: ErpGateError) -> (StatusCode, ErpBody) {
    match error {
        ErpGateError::Configuration(error) => {
            let (status, body) = error.response();
            (status, ErpBody::Doc(body.0))
        }
        ErpGateError::Authority(refusal) => outcome(authority_refusal(&refusal)),
        ErpGateError::Database(ledger, source) => {
            tracing::error!(error = %source, "refusing: {ledger} database failure");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                ErpBody::Outcome(CommandOutcome {
                    reason: Some("database_failure".to_string()),
                    ..CommandOutcome::base("unavailable")
                }),
            )
        }
    }
}

// --- Actor model ------------------------------------------------------------

/// Who is acting — module-local, same shape as the other ERP modules'.
#[derive(Debug, Clone)]
pub enum ErpActor {
    Human(String),
}

impl ErpActor {
    fn principal(&self) -> PurchasePrincipal {
        match self {
            Self::Human(id) => PurchasePrincipal::human(id.clone()),
        }
    }
}

// --- Shapes ----------------------------------------------------------------

/// The mutation submission: tenant from the caller's own document (the
/// database, not the session, has the final word on whether the principal
/// belongs to it) and the raw `{requestId, command}` document the strict
/// parse judges, exactly the TS wire's `accommodationMutationSchema`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccommodationSubmission {
    pub tenant: TenantId,
    pub mutation: Value,
}

/// The status query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccommodationStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

/// The fingerprint a submission records: HMAC-SHA256 (keyed by the identity
/// store's secret, labelled `erp-accommodation-fingerprint`) over the
/// domain label, the tenant, the caller's own principal document and the
/// EXACT typed command — the TS `canonical({tenant, principal, command})`
/// digest, with the same recorded divergence the other ERP ledgers carry
/// (labelled digest rather than the baseline's raw unlabeled HMAC;
/// ledger-internal stability is exact). Two submissions share a key only
/// when they are, byte for byte, the same command from the same principal
/// for the same tenant — which is what makes the replay branch a faithful
/// replay and the reuse branch a genuine conflict. The idempotency key
/// itself rides the command row, not the digest.
fn fingerprint(
    state: &AppState,
    tenant: &TenantId,
    principal: &PurchasePrincipal,
    mutation: &AccommodationMutation,
) -> String {
    let document = serde_json::to_value(&mutation.command)
        .expect("a typed command serializes back to its own JSON");
    let canonical = serde_json::json!({
        "domain": "accommodation",
        "tenant": tenant.as_str(),
        "principal": principal,
        "command": document,
    });
    state
        .identity()
        .digest("erp-accommodation-fingerprint", &canonical.to_string())
}

// --- Routes ----------------------------------------------------------------

/// `POST /erp/accommodation/commands` — submit (or replay) an
/// accommodation command. CSRF-bound like the other ERP write routes; the
/// tenant comes only from the body. The OK body is the BARE receipt — the
/// TS api answers `service.mutate` verbatim and the accommodation receipt
/// carries no replay marker.
pub(crate) async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<AccommodationSubmission>, JsonRejection>,
) -> (StatusCode, ErpBody) {
    let submission = match submission {
        Ok(submission) => submission,
        Err(rejected) => return outcome(rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match submit_accommodation_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/accommodation/commands/status?tenant=&requestId=` — the
/// durable status of one request id. Session-bound like the write route;
/// the human-only + recorded-actor gates live in the repository step, same
/// as the TS status read. The OK body is the commandStatus document
/// verbatim.
pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<AccommodationStatusQuery>, QueryRejection>,
) -> (StatusCode, ErpBody) {
    let query = match query {
        Ok(query) => query,
        Err(rejected) => return outcome(query_rejection(rejected)),
    };
    let presented = match authenticate(&state, &headers, false).await {
        Ok(presented) => presented,
        Err(refusal) => return outcome(refusal),
    };
    let actor = ErpActor::Human(presented.principal_id.clone());
    match accommodation_status_read(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- One transaction per request: the submit ---------------------------------

/// The submission pipeline — the port of `mutate()` driven by the host's
/// identity key. One transaction for the whole request: member ladder,
/// authorize, claim, execute, audit, receipt binding. It commits in exactly
/// one place; every refusal rolls back explicitly, so no path can commit a
/// half-applied command.
pub async fn submit_accommodation_command(
    state: &AppState,
    actor: ErpActor,
    submission: AccommodationSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mutation = match parse_mutation(&submission.mutation) {
        Ok(mutation) => mutation,
        Err(_) => return Ok(invalid_submission()),
    };
    let fingerprint = fingerprint(state, &submission.tenant, &principal, &mutation);

    let mut transaction = begin_accommodation(state, submission.tenant.clone()).await?;

    // 1. The member ladder — the ledger membership row, the active+kinds
    //    match, the permission intersection over the human's scope.
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_accommodation_context(connection, submission.tenant.as_str(), &principal)
            .await
        {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };

    // 2. The claim → execute → audit → receipt pipeline. The commit at the
    //    end is the ONE commit; the idempotency reuse branch replays from
    //    the stored receipt surface through the same step function.
    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_accommodation_command(
            connection,
            submission.tenant.as_str(),
            &principal,
            &access,
            &fingerprint,
            &mutation,
        )
        .await
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };

    transaction.commit().await.map_err(ErpGateError::from)?;
    // The TS api answers the receipt itself — no envelope, no replay
    // marker (the accommodation receipt's schema has no `replayed`).
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&receipt)
                .expect("a typed receipt serializes back to its own JSON"),
        ),
    ))
}

// --- One transaction per request: the status read -----------------------------

/// The durable status of one request id against the CURRENT access — the
/// recorded actor must match (the repository's own gate), a revoked or
/// invisible receipt refuses, and a lost/corrupt receipt is a conflict.
/// The answer is the TS commandStatus document verbatim.
pub async fn accommodation_status_read(
    state: &AppState,
    actor: ErpActor,
    query: AccommodationStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_accommodation(state, query.tenant.clone()).await?;
    let access = {
        let connection = transaction.checked_executor().await?;
        match resolve_accommodation_context(connection, query.tenant.as_str(), &principal).await {
            Ok(access) => access,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };
    let status = {
        let connection = transaction.checked_executor().await?;
        match accommodation_command_status(
            connection,
            query.tenant.as_str(),
            &principal,
            &access,
            query.request_id,
        )
        .await
        {
            Ok(status) => status,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };
    // Read-only path: rolled back, never committed.
    let _ = transaction.rollback().await;
    Ok((
        StatusCode::OK,
        ErpBody::Doc(
            serde_json::to_value(&status).expect("a typed status serializes back to its own JSON"),
        ),
    ))
}
