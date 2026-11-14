//! The purchasing routes — the Archxy 2 port of the R06 purchasing mutation
//! surface against `archava_purchasing`, in the shape [`crate::erp::crm`] pinned
//! for this host: extraction → session → ONE tenant transaction whose member
//! ladder, authorization, idempotency claim, effect and audit all run on the
//! adapters' step functions.
//!
//! ## The answers the two commands give
//!
//! - `POST /erp/purchasing/commands` — like `runMutation`: a receipt (fresh
//!   or replayed; `receipt.replayed` tells them apart), or one of the
//!   repository's refusal codes as HTTP code + fixed text: forbidden 403
//!   (`denied`), missing record 404 (`denied`/`unknown_resource`), conflict
//!   409 (`refused`), malformed submission 400 (`rejected`).
//! - `GET /erp/purchasing/commands?tenant=&requestId=` — like
//!   `commandStatus`: `committed` with its receipt, or `unknown`. A
//!   committed receipt is only recoverable while the CURRENT access still
//!   authorizes the recorded operation — recovery refusals surface here too.
//!
//! No shadow authorization: every grant below is read from the two ledger
//! rows inside the same transaction, and the adapters reapply the domain
//! authorize ladder on top.

use archava_adapters::erp::purchasing::{
    command_status as adapter_command_status, record_purchase_command, resolve_purchasing_context,
    ResolvedPurchaseContext,
};
use archava_adapters::erp::PurchaseError;
use archava_core::tenant::TenantId;
use archava_domain::erp::purchasing::{PurchaseMutation, PurchasePrincipal, PurchaseReceipt};
use archava_domain::erp::ErpError;
use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::erp::{begin, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};

// --- Response envelope ------------------------------------------------------

/// The union body this module's routes answer with. The ERP receipts are
/// plain documents; refusals reuse the host's [`CommandOutcome`].
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
/// concurrent revisions, no-op updates, claim reuse, cross-domain receipts.
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

/// 503 for a domain-classed unavailability the TS vocabulary carries
/// (`unavailable`) — the ledger itself refused rather than a caller error.
fn unavailable() -> (StatusCode, ErpBody) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("ledger_unavailable".to_string()),
            message: Some("The purchasing ledger is temporarily unavailable.".to_string()),
            ..CommandOutcome::base("unavailable")
        }),
    )
}

fn map_purchase_result(error: PurchaseError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        PurchaseError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        PurchaseError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        PurchaseError::Domain(ErpError::Conflict) => Ok(conflict()),
        // A conflict whose database guard is catalogued: the same 409 and
        // `reason`, with the specific sentence and its code.
        PurchaseError::Refused(refusal) => Ok((
            StatusCode::CONFLICT,
            ErpBody::Doc(crate::erp::refused_document(refusal)),
        )),
        PurchaseError::Domain(ErpError::Unavailable) => Ok(unavailable()),
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

/// Who is acting. The HTTP session gives a human; the suite (and, later, a
/// signed integration runner) gives a service principal carrying the human
/// it acts in behalf of — narrowed exactly like the TS transaction does.
#[derive(Debug, Clone)]
pub enum ErpActor {
    Human(String),
    Service { id: String, on_behalf_of: String },
}

impl ErpActor {
    fn principal(&self) -> PurchasePrincipal {
        match self {
            Self::Human(id) => PurchasePrincipal::human(id.clone()),
            Self::Service { id, on_behalf_of } => {
                PurchasePrincipal::service(id.clone(), on_behalf_of.clone())
            }
        }
    }
}

// --- Shapes ----------------------------------------------------------------

/// The mutation submission: tenant from the caller's own document (the
/// database, not the session, has the final word on whether the principal
/// belongs to it), the idempotency key, and the raw command the strict
/// parse judges.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PurchaseSubmission {
    pub tenant: TenantId,
    pub request_id: Uuid,
    pub command: Value,
}

/// The status query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PurchaseStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

/// The fingerprint a submission records: HMAC-SHA256 (keyed by the identity
/// store's secret, labelled `erp-purchasing-fingerprint`) over the domain
/// label, the tenant, the caller's own principal document and the EXACT
/// typed command. Two submissions share a key only when they are, byte for
/// byte, the same command from the same principal for the same tenant —
/// which is what makes the replay branch a faithful replay and the reuse
/// branch a genuine conflict.
///
/// Recorded divergence from the TS digest (see KNOWN_DIFFERENCES ledger in
/// the ERP packet): the baseline hashes the raw canonical document with an
/// unlabeled HMAC; this port uses the identity store's labeled digest, the
/// same class of label the CRM slice uses. Ledger-internal stability is
/// exact; cross-system byte equality is not a contract.
fn fingerprint(
    state: &AppState,
    tenant: &TenantId,
    principal: &PurchasePrincipal,
    mutation: &PurchaseMutation,
) -> String {
    let command = serde_json::to_value(&mutation.command)
        .expect("a typed command serializes back to its own JSON");
    let canonical = serde_json::json!({
        "domain": "purchasing",
        "tenant": tenant.as_str(),
        "principal": principal,
        "command": command,
    });
    state
        .identity()
        .digest("erp-purchasing-fingerprint", &canonical.to_string())
}

// --- Routes ----------------------------------------------------------------

/// `POST /erp/purchasing/commands` — submit (or replay) a purchasing
/// command. CSRF-bound like the CRM write routes; the tenant comes only
/// from the body.
pub(crate) async fn command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<PurchaseSubmission>, JsonRejection>,
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
    match submit_purchase_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/purchasing/commands?tenant=&requestId=` — the durable status of
/// one request id. Session-bound like the write route (the human-kind gate
/// is part of the repository step, same as the TS status read).
pub(crate) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<PurchaseStatusQuery>, QueryRejection>,
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
    match purchase_command_status(&state, actor, query.0).await {
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
pub async fn submit_purchase_command(
    state: &AppState,
    actor: ErpActor,
    submission: PurchaseSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mutation = PurchaseMutation {
        request_id: submission.request_id.to_string(),
        command: match archava_domain::erp::purchasing::parse_command(&submission.command) {
            Ok(command) => command,
            Err(_) => return Ok(invalid_submission()),
        },
    };
    let fingerprint = fingerprint(state, &submission.tenant, &principal, &mutation);

    let mut transaction = begin(state, submission.tenant.clone()).await?;
    if matches!(
        mutation.command,
        archava_domain::erp::purchasing::PurchaseCommand::SupplierCreateFromContact { .. }
            | archava_domain::erp::purchasing::PurchaseCommand::SupplierRefreshContact { .. }
    ) {
        crate::own::require_erp_module(&mut transaction, "crm-contacts")
            .await
            .map_err(ErpGateError::Configuration)?;
    }

    if matches!(
        mutation.command,
        archava_domain::erp::purchasing::PurchaseCommand::OrderCreateForCompany { .. }
            | archava_domain::erp::purchasing::PurchaseCommand::SupplierCreateFromParty { .. }
            | archava_domain::erp::purchasing::PurchaseCommand::SupplierRefreshParty { .. }
    ) {
        crate::own::require_erp_module(&mut transaction, "organizations")
            .await
            .map_err(ErpGateError::Configuration)?;
    }
    // 1. The member ladder — BOTH ledgers, the service narrow, the
    //    commerce-narrows-purchasing rule, the catalog.read floor.
    let context = {
        let connection = transaction.checked_executor().await?;
        match resolve_purchasing_context(connection, submission.tenant.as_str(), &principal).await {
            Ok(context) => context,
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
        match record_purchase_command(
            connection,
            submission.tenant.as_str(),
            &principal,
            &context,
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
    let decision = if receipt.replayed {
        "replayed"
    } else {
        "receipt"
    };
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::json!({
            "decision": decision,
            "tenant": submission.tenant.as_str(),
            "requestId": submission.request_id,
            "receipt": serde_json::to_value(&receipt)
                .expect("a typed receipt serializes back to its own JSON"),
        })),
    ))
}

// --- One transaction per request: the status read -----------------------------

/// The durable status of one request id against the CURRENT access — replay
/// and revoked-grant refusals surface here exactly like TS's
/// `commandStatus` (a receipt whose grants were revoked is `denied`, a
/// lost/corrupt receipt is a conflict).
pub async fn purchase_command_status(
    state: &AppState,
    actor: ErpActor,
    query: PurchaseStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin(state, query.tenant.clone()).await?;
    let context: ResolvedPurchaseContext = {
        let connection = transaction.checked_executor().await?;
        match resolve_purchasing_context(connection, query.tenant.as_str(), &principal).await {
            Ok(context) => context,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_purchase_result(error);
            }
        }
    };
    let status = {
        let connection = transaction.checked_executor().await?;
        match adapter_command_status(
            connection,
            query.tenant.as_str(),
            &principal,
            &context,
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
    let receipt = match &status {
        archava_domain::erp::purchasing::PurchaseCommandStatus::Committed { receipt } => {
            serde_json::to_value(receipt).expect("a typed receipt serializes back to its own JSON")
        }
        archava_domain::erp::purchasing::PurchaseCommandStatus::Unknown => Value::Null,
    };
    let decision = match &status {
        archava_domain::erp::purchasing::PurchaseCommandStatus::Committed { .. } => "committed",
        archava_domain::erp::purchasing::PurchaseCommandStatus::Unknown => "unknown",
    };
    Ok((
        StatusCode::OK,
        ErpBody::Doc(serde_json::json!({
            "decision": decision,
            "tenant": query.tenant.as_str(),
            "requestId": query.request_id,
            "receipt": receipt,
        })),
    ))
}

/// The purchasing receipt documents the suite re-reads through the plain
/// JSONB to verify against the SQL triggers — one helper instead of a raw
/// document in four test bodies.
pub fn receipt_json(receipt: &PurchaseReceipt) -> Value {
    serde_json::to_value(receipt).expect("a typed receipt serializes back to its own JSON")
}
