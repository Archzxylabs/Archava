//! HTTP endpoints for the industry preset catalogue and the proactive action
//! feed.
//!
//! Both families were mounted outside the identity chain: an unauthenticated
//! caller could read a feed that presented itself as tenant-shaped, and
//! "execute" an action card whose response carried a `receipt_id` no ledger
//! contained. Every handler here now runs [`require_session`] first, the
//! extractors are the guarded `Result<…, Rejection>` shape so no refusal
//! reflects the caller's own input, and the execution endpoint answers 501:
//! there is no dispatch worker to hand work to, so removing the fabricated
//! receipt is the honest answer.
//!
//! The feed's numbers are synthesized demo material, not a tenant roll-up. It
//! no longer accepts a caller-supplied tenant to label them with.

use archava_domain::feed::generate_action_feed;
use archava_domain::presets::IndustryProfile;
use axum::extract::rejection::{JsonRejection, PathRejection, QueryRejection};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::router::{require_session, AppState};

/// The label the synthesized demo feed is generated under.
///
/// It is deliberately not a request parameter: the previous version accepted
/// `?tenant=…` and echoed it into a fabricated feed, which read as a
/// tenant-scoped read that no tenant ever authorized.
const DEMO_WORKSPACE: &str = "demo-workspace";

#[derive(Debug, Deserialize)]
pub struct FeedQueryParams {
    pub profile: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ExecuteActionRequest {
    pub action_type: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
pub struct ExecuteActionResponse {
    pub status: String,
    pub card_id: String,
    pub action_type: String,
    pub detail: String,
    pub is_simulated: bool,
}

/// GET /api/v1/presets/profiles
/// Returns all available industry profile masks for zero-config onboarding.
pub async fn list_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    if let Err((status, reason)) = require_session(&state, &headers, false).await {
        return (status, Json(json!({ "error": { "message": reason } })));
    }
    let profiles = vec![
        IndustryProfile::FnBCafe.mask(),
        IndustryProfile::ClinicHealthcare.mask(),
        IndustryProfile::HospitalityHotel.mask(),
        IndustryProfile::RetailCommerce.mask(),
        IndustryProfile::LogisticsFleet.mask(),
        IndustryProfile::ProfessionalServices.mask(),
    ];

    (StatusCode::OK, Json(json!({ "profiles": profiles })))
}

/// GET /api/v1/presets/profiles/{slug}
/// Returns a single tailored profile mask by its slug.
pub async fn get_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err((status, reason)) = require_session(&state, &headers, false).await {
        return (status, Json(json!({ "error": { "message": reason } })));
    }
    let slug = match path {
        Ok(Path(slug)) => slug,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(json!({ "error": { "message": "malformed_profile_path" } })),
            )
        }
    };
    match IndustryProfile::parse(&slug) {
        Some(profile) => (StatusCode::OK, Json(json!(profile.mask()))),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": { "message": "Unknown industry profile." } })),
        ),
    }
}

/// GET /api/v1/feed/action-cards
/// Returns proactive action cards tailored to the industry profile.
pub async fn get_action_cards(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FeedQueryParams>, QueryRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err((status, reason)) = require_session(&state, &headers, false).await {
        return (status, Json(json!({ "error": { "message": reason } })));
    }
    let params = match query {
        Ok(Query(params)) => params,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(json!({ "error": { "message": "malformed_feed_query" } })),
            )
        }
    };
    let profile = params
        .profile
        .as_deref()
        .and_then(IndustryProfile::parse)
        .unwrap_or(IndustryProfile::FnBCafe);

    // Synthesized demo metrics. The card generator is a planning helper, not a
    // tenant read; nothing here is derived from a caller-supplied tenant.
    let feed = generate_action_feed(
        DEMO_WORKSPACE,
        profile,
        3,
        1_420_000_000, // Rp 14.200.000 overdue
        &["Biji Kopi Arabica 1kg", "Susu UHT Fresh"],
    );

    (StatusCode::OK, Json(json!({ "feed": feed })))
}

/// POST /api/v1/feed/action-cards/{id}/execute
///
/// Answers 501. The route used to return `accepted_pending_worker` with a
/// `receipt_id` built from the card id, which no receipt ledger ever contained
/// and no worker ever consumed. Returning a not-implemented refusal is the only
/// honest answer until a dispatch path exists; the session and CSRF checks stay
/// in front of it so it cannot be probed anonymously either.
pub async fn execute_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    path: Result<Path<String>, PathRejection>,
    body: Result<Json<ExecuteActionRequest>, JsonRejection>,
) -> (StatusCode, Json<Value>) {
    if let Err((status, reason)) = require_session(&state, &headers, true).await {
        return (status, Json(json!({ "error": { "message": reason } })));
    }
    let card_id = match path {
        Ok(Path(card_id)) => card_id,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(json!({ "error": { "message": "malformed_action_path" } })),
            )
        }
    };
    let request = match body {
        Ok(Json(request)) => request,
        Err(rejection) => {
            return (
                rejection.status(),
                Json(json!({ "error": { "message": "Invalid execution payload." } })),
            )
        }
    };

    let detail = match request.action_type.as_str() {
        "dispatch_ar_reminders" => "Reminder dispatch has no worker mounted in this host.",
        "generate_reorder_po" => "Purchase-order drafting is not wired through this route.",
        "print_shift_summary" => "No printer transport is mounted on this host.",
        "blast_patient_confirmations" => "No messaging transport is mounted on this host.",
        "issue_milestone_invoice" => "Invoice issuance is not wired through this route.",
        _ => "No execution path is mounted for this action.",
    };

    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": detail,
                "code": "action_execution_not_implemented",
            },
            "card_id": card_id,
            "action_type": request.action_type,
            "is_simulated": false,
        })),
    )
}
