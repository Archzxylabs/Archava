//! Tenant-derived proactive cards backed by reviewed, authorized domain actions.
mod actions;
use crate::router::{require_session, AppState};
use archava_core::tenant::TenantId;
use archava_domain::presets::IndustryProfile;
use axum::{
    extract::{
        rejection::{JsonRejection, PathRejection, QueryRejection},
        Path, Query, State,
    },
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedQueryParams {
    pub tenant: TenantId,
    pub profile: Option<String>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecuteActionRequest {
    pub tenant: TenantId,
    pub plan_hash: String,
    pub confirmed: bool,
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

pub async fn get_action_cards(
    State(state): State<AppState>,
    headers: HeaderMap,
    query: Result<Query<FeedQueryParams>, QueryRejection>,
) -> crate::business::Response {
    if let Err((s, b)) = crate::router::authenticate(&state, &headers, false).await {
        return (s, Json(json!(b.0)));
    }
    let input = match query {
        Ok(Query(v)) => v,
        Err(_) => return crate::business::refused(StatusCode::BAD_REQUEST, "invalid_feed_query"),
    };
    if input
        .profile
        .as_ref()
        .is_some_and(|p| IndustryProfile::parse(p).is_none())
    {
        return crate::business::refused(StatusCode::BAD_REQUEST, "invalid_profile");
    }
    match actions::cards(&state, &headers, input).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => e,
    }
}
pub async fn execute_action(
    State(state): State<AppState>,
    headers: HeaderMap,
    path: Result<Path<uuid::Uuid>, PathRejection>,
    body: Result<Json<ExecuteActionRequest>, JsonRejection>,
) -> crate::business::Response {
    if let Err((s, b)) = crate::router::authenticate(&state, &headers, true).await {
        return (s, Json(json!(b.0)));
    }
    let (Ok(Path(id)), Ok(Json(input))) = (path, body) else {
        return crate::business::refused(StatusCode::BAD_REQUEST, "invalid_action_review");
    };
    match actions::execute(&state, &headers, id, input).await {
        Ok(v) => (StatusCode::OK, Json(v)),
        Err(e) => e,
    }
}
