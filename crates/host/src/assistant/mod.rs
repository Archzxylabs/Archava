//! The CRM conversation route — the host half of R03's assistant slice.
//!
//! `POST /api/crm/{tenant}/conversation` ports `apps/crm/src/host.ts`'s
//! conversation route: session + CSRF first, the tenant slice from the path,
//! the validated [`ConversationInput`], then the deterministic local planner
//! and the validated conversation flow. The reply is a *describing string*
//! over CRM data: the planner's output is a request resolved against real
//! grants, never a capability statement of its own.
//!
//! What this module adds beyond `archava_assistant`'s pure flow:
//!
//! - [`SqlHost`] — the [`HostCrm`] implementation over real transactions. Each
//!   tool opens its own [`begin`] transaction and re-authorizes inside it —
//!   the shape of the baseline's `repository.transaction(tenant, principal,
//!   permission, ...)`, which re-runs the full grant resolution per call —
//!   and every refusal rolls the transaction back explicitly. Reads may end
//!   by drop: a rolled-back read is indistinguishable from a committed one.
//! - The wire mapping: caller mistakes → 400, authorization → 403, lifecycle
//!   races → 409, infrastructure conditions → 503 — the TS host's status
//!   table for `CrmError`, each answered with the masked `{error:{message}}`
//!   body (`'CRM request refused.'`; 503 keeps its own recorded text).
//!   Identity refusals pass through in the house envelope, unchanged from
//!   every other route.
//!
//! Deliberately not in this slice (recorded as known composition
//! differences, never as claims):
//!
//! - The pipeline/native-actions module gate (`'Pipeline module is
//!   disabled.'`, 404) now reads the persisted C03 owner decision
//!   (`archava_owner`, module `crm-pipeline`) in the same tool transaction:
//!   a disabled module removes the pipeline tool and zeroes the overview's
//!   pipeline counts, while the grants still gate who may ask at all.
//! - **The Gemini planner lane** — a provider decision (`plannerFor`), not a
//!   CRM contract. This composition serves the local deterministic planner,
//!   exactly like the TS fallback.
//!
//! Timestamps inside assistant payloads render through
//! `to_char(...,'YYYY-MM-DD"T"HH24:MI:SS.US"Z"')` — the worklist's recorded
//! convention, which also keeps the `(dueAt,id)` keyset exact. The TS
//! baseline renders detail instants from JS `Date` (millisecond precision);
//! this port carries microseconds, a documented superset
//! (KNOWN_DIFFERENCES). The serving instant (`occurredAt`, the vote the
//! default follow-up `asOf` is anchored to) comes from the DATABASE clock —
//! the same authority rule the CRM commands ride — not from the process
//! clock.

pub mod bridge;
pub mod sql_host;

pub(crate) use sql_host::SqlHost;

use archava_assistant::{
    ask, AssistantError, ConversationContext, ConversationInput, ErrorCode, LocalCrmPlanner,
};
use archava_core::tenant::TenantId;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use uuid::Uuid;

use crate::router::{authenticate, AppState};

// --- The route ---------------------------------------------------------------

/// `POST /api/crm/{tenant}/conversation`.
///
/// Order: extraction → session (+ CSRF, the dispatcher's POST rule) → input
/// validation → the flow. The tenant path segment validates against the same
/// slug rule `TenantId` already enforces for body-borne tenants, before any
/// authority question.
pub(crate) async fn conversation(
    State(state): State<AppState>,
    path: Result<Path<String>, axum::extract::rejection::PathRejection>,
    headers: HeaderMap,
    body: Result<Json<serde_json::Value>, JsonRejection>,
) -> (StatusCode, Json<serde_json::Value>) {
    let Ok(Path(tenant)) = path else {
        tracing::info!("conversation path refused at extraction");
        return masked(StatusCode::BAD_REQUEST, "Invalid request.");
    };
    let Ok(Json(raw)) = body else {
        tracing::info!("conversation body rejected at extraction");
        return masked(StatusCode::BAD_REQUEST, "Invalid request.");
    };
    let Ok(tenant) = TenantId::new(tenant) else {
        tracing::info!("conversation tenant refused at the slug gate");
        return masked(StatusCode::BAD_REQUEST, "Invalid request.");
    };
    let presented = match authenticate(&state, &headers, true).await {
        Ok(presented) => presented,
        Err((status, outcome)) => {
            // Identity refusal shapes are already masked and reasoned in the
            // house envelope; forward them unchanged, like the other
            // session-scoped routes do.
            let outcome = serde_json::to_value(outcome.0).unwrap_or_else(
                |_| serde_json::json!({ "error": { "message": "CRM request refused." } }),
            );
            return (status, Json(outcome));
        }
    };
    let input = match ConversationInput::parse(raw) {
        Ok(input) => input,
        Err(_) => {
            // Fixed masked text on purpose: which key misbehaved is caller
            // information the baseline also refuses to name.
            tracing::info!("conversation input refused outside its boundaries");
            return masked(StatusCode::BAD_REQUEST, "Invalid request.");
        }
    };
    // System 1 measures the turn before the flow answers it. Shadow: the gate
    // runs, its judgment is logged, and nothing about the reply below can move
    // — see `crate::decision` for why that is structural rather than hoped for.
    crate::decision::shadow_pass(tenant.as_str(), &input.utterance);
    let occurred_at = match serving_instant(&state).await {
        Ok(instant) => instant,
        Err(source) => {
            tracing::error!(error = %source, "conversation died reading the serving instant");
            return masked(
                StatusCode::SERVICE_UNAVAILABLE,
                "Service temporarily unavailable.",
            );
        }
    };

    let context = ConversationContext {
        tenant_id: tenant.as_str().to_string(),
        turn_id: Uuid::new_v4().to_string(),
        occurred_at,
    };
    let host = SqlHost {
        state: &state,
        tenant,
        principal_id: presented.principal_id.clone(),
    };
    match ask(&host, &LocalCrmPlanner::new(), &context, &input).await {
        Ok(reply) => (
            StatusCode::OK,
            Json(
                serde_json::to_value(reply)
                    .expect("a typed conversation reply must serialize to its own JSON"),
            ),
        ),
        Err(error) => refuse(error),
    }
}

/// The masked error body for a CRM refusal — the TS host's `{error:{message}}`
/// shape with the recorded texts. The flow's detailed `message` stays in the
/// log only, exactly like the baseline's catch masks `CrmError`'s text.
fn refusal_body(status: StatusCode) -> Json<serde_json::Value> {
    let message = if status == StatusCode::SERVICE_UNAVAILABLE {
        "Service temporarily unavailable."
    } else {
        "CRM request refused."
    };
    Json(serde_json::json!({ "error": { "message": message } }))
}

/// Map an [`AssistantError`] from the flow to the recorded status + body pair.
fn refuse(error: AssistantError) -> (StatusCode, Json<serde_json::Value>) {
    let status = match error.code {
        ErrorCode::Invalid => StatusCode::BAD_REQUEST,
        ErrorCode::Unauthenticated => StatusCode::UNAUTHORIZED,
        ErrorCode::Forbidden => StatusCode::FORBIDDEN,
        ErrorCode::NotFound => StatusCode::NOT_FOUND,
        ErrorCode::Conflict => StatusCode::CONFLICT,
        ErrorCode::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
    };
    tracing::info!(error = %error, status = %status, "conversation flow refused");
    (status, refusal_body(status))
}

/// 400/503 with a fixed masked text, before any authority question.
fn masked(status: StatusCode, message: &'static str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({ "error": { "message": message } })),
    )
}

/// The database-rendered serving instant — `occurredAt`'s authority.
async fn serving_instant(state: &AppState) -> Result<String, sqlx::Error> {
    sqlx::query_scalar::<_, String>(clock_sql())
        .fetch_one(state.pool())
        .await
}

/// The instant rendering every consumer in this module shares.
fn clock_sql() -> &'static str {
    r#"SELECT to_char(now() AT TIME ZONE 'UTC','YYYY-MM-DD"T"HH24:MI:SS.US"Z"')"#
}
