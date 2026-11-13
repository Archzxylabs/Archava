//! The foodservice routes — the Archxy 2 port of the R06 foodservice mutation
//! surface against `archava_foodservice`, in the shape
//! [`crate::erp::purchasing`] pinned: extraction → session → ONE tenant
//! transaction whose both-ledger member ladder, authorization, idempotency
//! claim, effect and audit all run on the adapters' step functions.
//!
//! ## What the two commands answer
//!
//! - `POST /erp/foodservice/commands` — the receipt of a food service
//!   command (a ticket whose queue is a COMMERCE order reservation and
//!   whose prepare is a COMMERCE consumption), or the repository's refusal
//!   codes as HTTP code + fixed text: forbidden 403 (`denied`), missing
//!   record 404 (`denied`/`unknown_resource`), conflict 409 (`refused`),
//!   malformed submission 400 (`rejected`).
//! - `GET /erp/foodservice/commands/status?tenant=&requestId=` — like
//!   `commandStatus`: `committed` with its receipt, or `unknown`. A
//!   committed receipt is only recoverable while the CURRENT access still
//!   authorizes the recorded operation.
//!
//! The mutation surface is human-only (the TS repository refuses service
//! principals on both paths), and the transaction's commerce-side floor
//! grants (`catalog.read`, `orders.read`) are enforced by the adapters'
//! context step — no shadow authorization anywhere.

use archava_adapters::erp::foodservice::{
    food_command_status as adapter_food_command_status, record_food_command, resolve_food_context,
};
use archava_adapters::erp::PurchaseError;
use archava_core::tenant::TenantId;
use archava_domain::erp::foodservice::{
    parse_command as parse_food_command, FoodCommandStatus, FoodMutation, FoodReceipt,
};
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

use crate::erp::{begin_foodservice, ErpGateError};
use crate::router::{
    authenticate, authority_refusal, query_rejection, rejection, AppState, CommandOutcome,
};

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
/// concurrent revisions, no-op updates, claim reuse, stock lapses.
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

/// 503 for a domain-classed unavailability the TS vocabulary carries.
fn unavailable() -> (StatusCode, ErpBody) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        ErpBody::Outcome(CommandOutcome {
            reason: Some("ledger_unavailable".to_string()),
            message: Some("The foodservice ledger is temporarily unavailable.".to_string()),
            ..CommandOutcome::base("unavailable")
        }),
    )
}

fn map_food_result(error: PurchaseError) -> Result<(StatusCode, ErpBody), ErpGateError> {
    match error {
        PurchaseError::Domain(ErpError::Forbidden) => Ok(forbidden()),
        PurchaseError::Domain(ErpError::NotFound) => Ok(unknown_resource()),
        PurchaseError::Domain(ErpError::Conflict) => Ok(conflict()),
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

/// Who is acting — the same enum the purchasing host uses. The HTTP session
/// gives a human; a signed integration path gives a service principal, which
/// the foodservice mutation step refuses (the TS repository is human-only).
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

/// The mutation submission: tenant from the caller's document, the
/// idempotency key, and the raw command the strict parse judges.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FoodSubmission {
    pub tenant: TenantId,
    pub request_id: Uuid,
    pub command: Value,
}

/// The status query.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FoodStatusQuery {
    pub tenant: TenantId,
    pub request_id: Uuid,
}

/// The fingerprint a submission records: HMAC-SHA256 (keyed by the identity
/// store's secret, labelled `erp-foodservice-fingerprint`) over the domain
/// label, the tenant, the caller's own principal document and the EXACT
/// typed command — the same rule the purchasing host pins, so a foodservice
/// submission and a purchasing submission can never share a key by accident.
fn fingerprint(
    state: &AppState,
    tenant: &TenantId,
    principal: &PurchasePrincipal,
    mutation: &FoodMutation,
) -> String {
    let command = serde_json::to_value(&mutation.command)
        .expect("a typed command serializes back to its own JSON");
    let canonical = serde_json::json!({
        "domain": "foodservice",
        "tenant": tenant.as_str(),
        "principal": principal,
        "command": command,
    });
    state
        .identity()
        .digest("erp-foodservice-fingerprint", &canonical.to_string())
}

// --- Routes ----------------------------------------------------------------

/// `POST /erp/foodservice/commands` — submit (or replay) a food service
/// command. CSRF-bound like the other ERP write routes; the tenant comes
/// only from the body.
pub(crate) async fn food_service_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    submission: Result<Json<FoodSubmission>, JsonRejection>,
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
    match submit_food_command(&state, actor, submission.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

/// `GET /erp/foodservice/commands/status?tenant=&requestId=` — the durable
/// status of one request id against the CURRENT access.
pub(crate) async fn food_service_status(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FoodStatusQuery>, QueryRejection>,
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
    match food_command_status(&state, actor, query.0).await {
        Ok(answer) => answer,
        Err(error) => gate_outcome(error),
    }
}

// --- One transaction per request: the submit ---------------------------------

/// The submission pipeline — the port of the foodservice `mutate()` driven
/// by the host's identity key. One transaction for the whole request;
/// it commits in exactly one place.
pub async fn submit_food_command(
    state: &AppState,
    actor: ErpActor,
    submission: FoodSubmission,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mutation = FoodMutation {
        request_id: submission.request_id.to_string(),
        command: match parse_food_command(&submission.command) {
            Ok(command) => command,
            Err(_) => return Ok(invalid_submission()),
        },
    };
    let fingerprint = fingerprint(state, &submission.tenant, &principal, &mutation);

    let mut transaction = begin_foodservice(state, submission.tenant.clone()).await?;

    // 1. The member ladder — BOTH ledgers in one transaction, the service
    //    narrow, the commerce-narrows-foodservice rule, the commerce floor.
    let context = {
        let connection = transaction.checked_executor().await?;
        match resolve_food_context(connection, submission.tenant.as_str(), &principal).await {
            Ok(context) => context,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_food_result(error);
            }
        }
    };

    // 2. The claim → execute → audit → receipt pipeline.
    let receipt = {
        let connection = transaction.checked_executor().await?;
        match record_food_command(
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
                return map_food_result(error);
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

/// The durable status of one request id against the CURRENT access —
/// replay and revoked-grant refusals surface here exactly like the TS
/// `commandStatus`.
pub async fn food_command_status(
    state: &AppState,
    actor: ErpActor,
    query: FoodStatusQuery,
) -> Result<(StatusCode, ErpBody), ErpGateError> {
    let principal = actor.principal();
    let mut transaction = begin_foodservice(state, query.tenant.clone()).await?;
    let context = {
        let connection = transaction.checked_executor().await?;
        match resolve_food_context(connection, query.tenant.as_str(), &principal).await {
            Ok(context) => context,
            Err(error) => {
                let _ = transaction.rollback().await;
                return map_food_result(error);
            }
        }
    };
    let status = {
        let connection = transaction.checked_executor().await?;
        match adapter_food_command_status(
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
                return map_food_result(error);
            }
        }
    };
    // Read-only path: rolled back, never committed.
    let _ = transaction.rollback().await;
    let (decision, receipt) = match &status {
        FoodCommandStatus::Committed { receipt } => (
            "committed",
            serde_json::to_value(receipt).expect("a typed receipt serializes back to its own JSON"),
        ),
        FoodCommandStatus::Unknown => ("unknown", Value::Null),
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

/// The foodservice receipt documents the suite re-reads through the plain
/// JSONB to verify against the SQL triggers.
pub fn receipt_json(receipt: &FoodReceipt) -> Value {
    serde_json::to_value(receipt).expect("a typed receipt serializes back to its own JSON")
}
