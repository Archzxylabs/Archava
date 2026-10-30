//! Realtime Voice & Presence Session Lifecycle HTTP endpoints.
//!
//! Provides authenticated presence session establishment, LiveKit WebRTC token
//! minting, sequential transcript turns, and degradation to Chat fallback.

use crate::{
    authority::MembershipKind,
    router::{authenticate, AppState},
};
use archava_core::tenant::TenantId;
use archava_domain::presence::{
    PresenceError, PresenceMode, PresenceSession, VoiceTranscriptFrame,
};
use archava_providers::voice::{VoiceClient, VoiceProviderError};
use axum::{
    extract::{Path as RoutePath, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[derive(Clone, Debug, Default)]
pub struct PresenceRegistry {
    pub client: Option<Arc<VoiceClient>>,
}

impl PresenceRegistry {
    pub fn new(client: VoiceClient) -> Self {
        Self {
            client: Some(Arc::new(client)),
        }
    }

    pub fn for_loopback() -> Self {
        Self {
            client: Some(Arc::new(VoiceClient::for_loopback())),
        }
    }

    pub fn from_env() -> Self {
        match (
            std::env::var("LIVEKIT_URL"),
            std::env::var("LIVEKIT_API_KEY"),
            std::env::var("LIVEKIT_API_SECRET"),
        ) {
            (Ok(url), Ok(key), Ok(secret))
                if !url.is_empty() && !key.is_empty() && !secret.is_empty() =>
            {
                Self::new(VoiceClient::new(
                    archava_providers::voice::VoiceBridgeConfig {
                        livekit_url: url,
                        api_key: key,
                        api_secret: secret.into_bytes(),
                        request_timeout: std::time::Duration::from_secs(5),
                        loopback_mode: false,
                    },
                ))
            }
            _ => Self::default(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct StartSessionRequest {
    pub tenant_id: Option<String>,
    pub session_id: Option<String>,
    #[serde(default = "default_voice_mode")]
    pub mode: PresenceMode,
}

fn default_voice_mode() -> PresenceMode {
    PresenceMode::Voice
}

#[derive(Debug, Serialize)]
pub struct StartSessionResponse {
    pub session: PresenceSession,
    pub token: String,
}

#[derive(Debug, Deserialize)]
pub struct DegradeSessionRequest {
    pub reason: String,
}

#[derive(Debug, Deserialize)]
pub struct TranscriptTurnRequest {
    pub frame_id: String,
    pub sequence_number: u64,
    pub speaker: String,
    pub text: String,
    #[serde(default = "default_true")]
    pub is_final: bool,
    #[serde(default = "default_latency")]
    pub latency_ms: u32,
    #[serde(default = "default_confidence")]
    pub confidence_score: f32,
}

fn default_true() -> bool {
    true
}
fn default_latency() -> u32 {
    50
}
fn default_confidence() -> f32 {
    0.95
}

async fn verify_active_membership(
    state: &AppState,
    tenant_str: &str,
    principal_id: &str,
) -> Result<(), Response> {
    let tenant = match TenantId::new(tenant_str) {
        Ok(t) => t,
        Err(_) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "invalid_tenant" })),
            )
                .into_response())
        }
    };
    match crate::authority::resolve_tenant_membership(
        state.pool(),
        *state.ledger(),
        &tenant,
        principal_id,
    )
    .await
    {
        Ok(Some(m)) if m.kind == MembershipKind::Human => Ok(()),
        Ok(Some(_)) => Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "human_membership_required" })),
        )
            .into_response()),
        Ok(None) => Err((
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "forbidden_tenant_access" })),
        )
            .into_response()),
        Err(e) => {
            tracing::warn!(error = %e, "authority error while checking presence membership");
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({ "error": "forbidden_tenant_access" })),
            )
                .into_response())
        }
    }
}

fn get_voice_client(state: &AppState) -> Result<&Arc<VoiceClient>, Box<Response>> {
    match state.presence.client.as_ref() {
        Some(client) => Ok(client),
        None => Err(Box::new(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "presence_unconfigured",
                    "message": "Realtime presence provider is unconfigured"
                })),
            )
                .into_response(),
        )),
    }
}

/// POST /presence/sessions — starts a presence session and returns a signed LiveKit token.
pub async fn start_presence_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StartSessionRequest>,
) -> Response {
    let session = match authenticate(&state, &headers, true).await {
        Ok(session) => session,
        Err(err_response) => return err_response.into_response(),
    };

    let tenant_id = payload.tenant_id.unwrap_or_else(|| "acme".to_string());
    if let Err(res) = verify_active_membership(&state, &tenant_id, &session.principal_id).await {
        return res;
    }

    let session_id = match payload.session_id {
        Some(sid) => {
            if sid.is_empty()
                || sid.len() > 64
                || !sid
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "invalid_session_id",
                        "message": "session_id must be 1-64 characters and contain only ASCII alphanumeric, '-', or '_'"
                    })),
                )
                    .into_response();
            }
            sid
        }
        None => uuid::Uuid::new_v4().to_string(),
    };
    let participant_id = session.principal_id;
    let current_ms = now_ms();

    let voice_client = match get_voice_client(&state) {
        Ok(c) => c,
        Err(r) => return *r,
    };
    match voice_client.start_session(
        &tenant_id,
        &session_id,
        &participant_id,
        payload.mode,
        current_ms,
    ) {
        Ok((pres_session, token)) => (
            StatusCode::CREATED,
            Json(json!({
                "session": pres_session,
                "token": token
            })),
        )
            .into_response(),
        Err(VoiceProviderError::Presence(PresenceError::SessionCollision(s))) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "session_collision",
                "message": format!("Presence session {s} already exists")
            })),
        )
            .into_response(),
        Err(VoiceProviderError::Presence(PresenceError::CapacityExceeded(t))) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "error": "capacity_exceeded",
                "message": format!("Maximum presence session capacity reached for tenant {t}")
            })),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// GET /presence/sessions/{id} — retrieves the active presence session.
pub async fn get_presence_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
) -> Response {
    let session = match authenticate(&state, &headers, false).await {
        Ok(session) => session,
        Err(err_response) => return err_response.into_response(),
    };

    let current_ms = now_ms();
    let voice_client = match get_voice_client(&state) {
        Ok(c) => c,
        Err(r) => return *r,
    };

    match voice_client.find_session(&id, current_ms) {
        Ok(pres_session) => {
            if pres_session.participant_id != session.principal_id {
                return (
                    StatusCode::FORBIDDEN,
                    Json(json!({ "error": "forbidden_session_access" })),
                )
                    .into_response();
            }
            if let Err(res) =
                verify_active_membership(&state, &pres_session.tenant_id, &session.principal_id)
                    .await
            {
                return res;
            }
            (StatusCode::OK, Json(json!(pres_session))).into_response()
        }
        Err(VoiceProviderError::Presence(PresenceError::SessionNotFound(_))) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(VoiceProviderError::Presence(PresenceError::SessionExpired(_))) => {
            (StatusCode::GONE, Json(json!({"error": "session_expired"}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /presence/sessions/{id}/turn — processes sequential transcript turns.
pub async fn process_presence_turn(
    State(state): State<AppState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Json(turn): Json<TranscriptTurnRequest>,
) -> Response {
    let session = match authenticate(&state, &headers, true).await {
        Ok(session) => session,
        Err(err_response) => return err_response.into_response(),
    };

    let current_ms = now_ms();
    let voice_client = match get_voice_client(&state) {
        Ok(c) => c,
        Err(r) => return *r,
    };

    let target_session = match voice_client.find_session(&id, current_ms) {
        Ok(s) => s,
        Err(VoiceProviderError::Presence(PresenceError::SessionNotFound(_))) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(VoiceProviderError::Presence(PresenceError::SessionExpired(_))) => {
            return (StatusCode::GONE, Json(json!({"error": "session_expired"}))).into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    if target_session.participant_id != session.principal_id {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "forbidden_session_access" })),
        )
            .into_response();
    }

    if let Err(res) =
        verify_active_membership(&state, &target_session.tenant_id, &session.principal_id).await
    {
        return res;
    }

    let frame = VoiceTranscriptFrame::create(
        turn.frame_id,
        id,
        turn.sequence_number,
        turn.speaker,
        turn.text,
        turn.is_final,
        turn.latency_ms,
        turn.confidence_score,
    );

    match voice_client.process_transcript_turn(&target_session.tenant_id, &frame, current_ms) {
        Ok(reply_frame) => (StatusCode::OK, Json(json!(reply_frame))).into_response(),
        Err(VoiceProviderError::Presence(PresenceError::SequenceMismatch { expected, got })) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "sequence_mismatch",
                "expected": expected,
                "got": got
            })),
        )
            .into_response(),
        Err(VoiceProviderError::Presence(PresenceError::SessionNotFound(_))) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

/// POST /presence/sessions/{id}/degrade — triggers fallback to Chat mode.
pub async fn degrade_presence_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    RoutePath(id): RoutePath<String>,
    Json(payload): Json<DegradeSessionRequest>,
) -> Response {
    let session = match authenticate(&state, &headers, true).await {
        Ok(session) => session,
        Err(err_response) => return err_response.into_response(),
    };

    let current_ms = now_ms();
    let voice_client = match get_voice_client(&state) {
        Ok(c) => c,
        Err(r) => return *r,
    };

    let target_session = match voice_client.find_session(&id, current_ms) {
        Ok(s) => s,
        Err(VoiceProviderError::Presence(PresenceError::SessionNotFound(_))) => {
            return (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(VoiceProviderError::Presence(PresenceError::SessionExpired(_))) => {
            return (StatusCode::GONE, Json(json!({"error": "session_expired"}))).into_response()
        }
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
                .into_response()
        }
    };

    if target_session.participant_id != session.principal_id {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({ "error": "forbidden_session_access" })),
        )
            .into_response();
    }

    if let Err(res) =
        verify_active_membership(&state, &target_session.tenant_id, &session.principal_id).await
    {
        return res;
    }

    match voice_client.degrade_to_chat(&target_session.tenant_id, &id, &payload.reason, current_ms)
    {
        Ok(new_mode) => (
            StatusCode::OK,
            Json(json!({
                "session_id": id,
                "current_mode": new_mode,
                "degraded": true
            })),
        )
            .into_response(),
        Err(VoiceProviderError::Presence(PresenceError::AlreadyAtFloor(_))) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "error": "already_at_floor",
                "message": "Presence is already at the lowest Chat mode floor"
            })),
        )
            .into_response(),
        Err(VoiceProviderError::Presence(PresenceError::SessionNotFound(_))) => {
            (StatusCode::NOT_FOUND, Json(json!({"error": "not_found"}))).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
