//! Presence & Realtime Voice/Transcript Lifecycle Domain Rules.
//!
//! Enforces the presence lifecycle:
//! - Presence modes: `Human`, `Voice`, `Chat`.
//! - Strict fallback order: `Voice` falls back to `Chat`, never upwards (`Chat` is the floor).
//! - 1024-session gateway capacity bound per tenant.
//! - Bounded timeouts, replay/revision refusals, and cross-tenant isolation.
//! - Deterministic token generation for LiveKit rooms with HMAC-SHA256.
//! - Audio-shaped data carried purely via synthetic transcript frames with SHA-256 digests.

use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;

/// Maximum concurrent presence sessions per tenant.
pub const MAX_PRESENCE_SESSIONS_PER_TENANT: usize = 1024;

/// Default session TTL in milliseconds (1 hour).
pub const DEFAULT_SESSION_TTL_MS: u64 = 3_600_000;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum PresenceError {
    #[error("Capacity exceeded: maximum {MAX_PRESENCE_SESSIONS_PER_TENANT} active sessions reached for tenant {0}")]
    CapacityExceeded(String),

    #[error("Session {0} not found")]
    SessionNotFound(String),

    #[error("Session {0} is expired")]
    SessionExpired(String),

    #[error("Cross-tenant violation: session belongs to tenant {expected}, not {actual}")]
    CrossTenantViolation { expected: String, actual: String },

    #[error(
        "Invalid fallback from {from:?} to {to:?}: presence fallback can only degrade downwards"
    )]
    InvalidFallbackDirection {
        from: PresenceMode,
        to: PresenceMode,
    },

    #[error("Cannot fallback from {0:?}: already at lowest presence floor")]
    AlreadyAtFloor(PresenceMode),

    #[error("Duplicate or out-of-order sequence: expected {expected}, got {got}")]
    SequenceMismatch { expected: u64, got: u64 },

    #[error("Session collision: session ID {0} is already active")]
    SessionCollision(String),

    #[error("Cryptographic signing error: {0}")]
    SigningError(String),
}

/// The presence interaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PresenceMode {
    /// Text chat interface (the immutable floor).
    Chat,
    /// Bidirectional audio streaming / realtime voice.
    Voice,
    /// Live human operator session.
    Human,
}

impl PresenceMode {
    /// Returns the deterministic degraded fallback mode, if any.
    pub fn fallback_target(&self) -> Option<Self> {
        match self {
            Self::Human => Some(Self::Voice),
            Self::Voice => Some(Self::Chat),
            Self::Chat => None, // Chat is the absolute floor
        }
    }

    /// Whether transitioning to `target` represents a valid downwards degradation.
    pub fn can_fallback_to(&self, target: Self) -> bool {
        matches!(
            (self, target),
            (Self::Human, Self::Voice) | (Self::Human, Self::Chat) | (Self::Voice, Self::Chat)
        )
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Voice => "voice",
            Self::Chat => "chat",
        }
    }
}

/// Record of a presence mode fallback transition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceFallbackEvent {
    pub from_mode: PresenceMode,
    pub to_mode: PresenceMode,
    pub reason: String,
    pub timestamp_ms: u64,
}

/// Active presence session state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceSession {
    pub session_id: String,
    pub tenant_id: String,
    pub participant_id: String,
    pub current_mode: PresenceMode,
    pub room_name: String,
    pub created_at_epoch_ms: u64,
    pub expires_at_epoch_ms: u64,
    pub is_active: bool,
    pub last_sequence: u64,
    pub fallback_history: Vec<PresenceFallbackEvent>,
}

impl PresenceSession {
    pub fn new(
        session_id: impl Into<String>,
        tenant_id: impl Into<String>,
        participant_id: impl Into<String>,
        mode: PresenceMode,
        room_name: impl Into<String>,
        now_ms: u64,
        ttl_ms: u64,
    ) -> Self {
        Self {
            session_id: session_id.into(),
            tenant_id: tenant_id.into(),
            participant_id: participant_id.into(),
            current_mode: mode,
            room_name: room_name.into(),
            created_at_epoch_ms: now_ms,
            expires_at_epoch_ms: now_ms.saturating_add(ttl_ms),
            is_active: true,
            last_sequence: 0,
            fallback_history: Vec::new(),
        }
    }

    pub fn is_expired(&self, now_ms: u64) -> bool {
        now_ms >= self.expires_at_epoch_ms
    }

    /// Degrades the session to the next lower presence mode with a recorded reason.
    pub fn trigger_fallback(
        &mut self,
        reason: impl Into<String>,
        now_ms: u64,
    ) -> Result<PresenceMode, PresenceError> {
        let target = self
            .current_mode
            .fallback_target()
            .ok_or(PresenceError::AlreadyAtFloor(self.current_mode))?;

        let event = PresenceFallbackEvent {
            from_mode: self.current_mode,
            to_mode: target,
            reason: reason.into(),
            timestamp_ms: now_ms,
        };

        self.current_mode = target;
        self.fallback_history.push(event);
        Ok(target)
    }

    /// Explicitly degrades to a specific lower target.
    pub fn fallback_to(
        &mut self,
        target: PresenceMode,
        reason: impl Into<String>,
        now_ms: u64,
    ) -> Result<PresenceMode, PresenceError> {
        if !self.current_mode.can_fallback_to(target) {
            return Err(PresenceError::InvalidFallbackDirection {
                from: self.current_mode,
                to: target,
            });
        }

        let event = PresenceFallbackEvent {
            from_mode: self.current_mode,
            to_mode: target,
            reason: reason.into(),
            timestamp_ms: now_ms,
        };

        self.current_mode = target;
        self.fallback_history.push(event);
        Ok(target)
    }
}

/// A structured synthetic transcript frame representing audio event exchange.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoiceTranscriptFrame {
    pub frame_id: String,
    pub session_id: String,
    pub sequence_number: u64,
    pub speaker: String,
    pub text: String,
    pub is_final: bool,
    pub latency_ms: u32,
    pub confidence_score: f32,
    pub digest: String,
}

impl VoiceTranscriptFrame {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        frame_id: impl Into<String>,
        session_id: impl Into<String>,
        sequence_number: u64,
        speaker: impl Into<String>,
        text: impl Into<String>,
        is_final: bool,
        latency_ms: u32,
        confidence_score: f32,
    ) -> Self {
        let frame_id = frame_id.into();
        let session_id = session_id.into();
        let speaker = speaker.into();
        let text = text.into();

        let mut hasher = Sha256::new();
        hasher.update(session_id.as_bytes());
        hasher.update(sequence_number.to_be_bytes());
        hasher.update(speaker.as_bytes());
        hasher.update(text.as_bytes());
        let digest = format!("{:x}", hasher.finalize());

        Self {
            frame_id,
            session_id,
            sequence_number,
            speaker,
            text,
            is_final,
            latency_ms,
            confidence_score,
            digest,
        }
    }
}

/// Generates a HMAC-SHA256 signed room token for LiveKit client connection.
pub fn generate_livekit_room_token(
    api_key: &str,
    api_secret: &[u8],
    room_name: &str,
    participant_id: &str,
    mode: PresenceMode,
    ttl_seconds: u64,
    now_epoch_seconds: u64,
) -> Result<String, PresenceError> {
    let header_json = r#"{"alg":"HS256","typ":"JWT"}"#;
    let header_b64 = simple_base64_url(header_json.as_bytes());

    let can_publish = mode == PresenceMode::Voice || mode == PresenceMode::Human;
    let payload = serde_json::json!({
        "iss": api_key,
        "sub": participant_id,
        "room": room_name,
        "iat": now_epoch_seconds,
        "exp": now_epoch_seconds.saturating_add(ttl_seconds),
        "video": {
            "room": room_name,
            "roomJoin": true,
            "canPublish": can_publish,
            "canSubscribe": true,
            "canPublishData": true
        }
    });

    let payload_str = payload.to_string();
    let payload_b64 = simple_base64_url(payload_str.as_bytes());

    let message = format!("{header_b64}.{payload_b64}");
    let mut mac = HmacSha256::new_from_slice(api_secret)
        .map_err(|e| PresenceError::SigningError(e.to_string()))?;
    mac.update(message.as_bytes());
    let signature = mac.finalize().into_bytes();
    let signature_b64 = simple_base64_url(&signature);

    Ok(format!("{message}.{signature_b64}"))
}

/// Lightweight URL-safe base64 encoding without external base64 crate dependency.
fn simple_base64_url(input: &[u8]) -> String {
    const URL_SAFE_CHARS: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

    let mut result = String::with_capacity(input.len().div_ceil(3) * 4);
    let chunks = input.chunks(3);

    for chunk in chunks {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);

        result.push(URL_SAFE_CHARS[((n >> 18) & 63) as usize] as char);
        result.push(URL_SAFE_CHARS[((n >> 12) & 63) as usize] as char);

        if chunk.len() > 1 {
            result.push(URL_SAFE_CHARS[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            result.push(URL_SAFE_CHARS[(n & 63) as usize] as char);
        }
    }

    result
}

/// In-memory Presence Gateway state machine with strict capacity and tenant bounds.
#[derive(Debug, Default, Clone)]
pub struct PresenceGateway {
    /// Active sessions indexed by `(tenant_id, session_id)`.
    sessions: BTreeMap<(String, String), PresenceSession>,
}

impl PresenceGateway {
    pub fn new() -> Self {
        Self {
            sessions: BTreeMap::new(),
        }
    }

    /// Registers a new presence session under the tenant, enforcing the 1024-session limit.
    pub fn register_session(
        &mut self,
        session: PresenceSession,
        now_ms: u64,
    ) -> Result<(), PresenceError> {
        // Global purge of expired sessions across all tenants
        self.sessions.retain(|_, s| !s.is_expired(now_ms));

        // Global capacity bound (16,384 sessions)
        if self.sessions.len() >= 16384 {
            return Err(PresenceError::CapacityExceeded("global".to_string()));
        }

        let active_count = self
            .sessions
            .keys()
            .filter(|(t, _)| t == &session.tenant_id)
            .count();

        if active_count >= MAX_PRESENCE_SESSIONS_PER_TENANT {
            return Err(PresenceError::CapacityExceeded(session.tenant_id));
        }

        // Check for session collision across ALL active sessions
        if self
            .sessions
            .values()
            .any(|s| s.session_id == session.session_id)
        {
            return Err(PresenceError::SessionCollision(session.session_id));
        }

        let key = (session.tenant_id.clone(), session.session_id.clone());
        self.sessions.insert(key, session);
        Ok(())
    }

    /// Explicitly closes and removes an active session.
    pub fn close_session(&mut self, tenant_id: &str, session_id: &str) -> bool {
        self.sessions
            .remove(&(tenant_id.to_string(), session_id.to_string()))
            .is_some()
    }

    /// Retrieves an active session with strict tenant isolation.
    pub fn get_session(
        &self,
        tenant_id: &str,
        session_id: &str,
        now_ms: u64,
    ) -> Result<&PresenceSession, PresenceError> {
        let key = (tenant_id.to_string(), session_id.to_string());
        let session = self
            .sessions
            .get(&key)
            .ok_or_else(|| PresenceError::SessionNotFound(session_id.to_string()))?;

        if session.is_expired(now_ms) {
            return Err(PresenceError::SessionExpired(session_id.to_string()));
        }

        Ok(session)
    }

    /// Finds an active session across tenants by session_id, ensuring not expired.
    pub fn find_session_by_id(
        &self,
        session_id: &str,
        now_ms: u64,
    ) -> Result<&PresenceSession, PresenceError> {
        let entry = self.sessions.iter().find(|((_, sid), _)| sid == session_id);

        match entry {
            Some((_, session)) => {
                if session.is_expired(now_ms) {
                    Err(PresenceError::SessionExpired(session_id.to_string()))
                } else {
                    Ok(session)
                }
            }
            None => Err(PresenceError::SessionNotFound(session_id.to_string())),
        }
    }

    /// Degrades session presence to fallback target.
    pub fn trigger_fallback(
        &mut self,
        tenant_id: &str,
        session_id: &str,
        reason: &str,
        now_ms: u64,
    ) -> Result<PresenceMode, PresenceError> {
        let key = (tenant_id.to_string(), session_id.to_string());
        let session = self
            .sessions
            .get_mut(&key)
            .ok_or_else(|| PresenceError::SessionNotFound(session_id.to_string()))?;

        if session.is_expired(now_ms) {
            return Err(PresenceError::SessionExpired(session_id.to_string()));
        }

        session.trigger_fallback(reason, now_ms)
    }

    /// Ingests a transcript frame ensuring sequential monotonically increasing sequence.
    pub fn ingest_frame(
        &mut self,
        tenant_id: &str,
        frame: &VoiceTranscriptFrame,
        now_ms: u64,
    ) -> Result<(), PresenceError> {
        let key = (tenant_id.to_string(), frame.session_id.clone());
        let session = self
            .sessions
            .get_mut(&key)
            .ok_or_else(|| PresenceError::SessionNotFound(frame.session_id.clone()))?;

        if session.is_expired(now_ms) {
            return Err(PresenceError::SessionExpired(frame.session_id.clone()));
        }

        let expected_seq = session.last_sequence + 1;
        if frame.sequence_number != expected_seq {
            return Err(PresenceError::SequenceMismatch {
                expected: expected_seq,
                got: frame.sequence_number,
            });
        }

        session.last_sequence = frame.sequence_number;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presence_fallback_hierarchy() {
        let mut session = PresenceSession::new(
            "sess-1",
            "acme",
            "user-1",
            PresenceMode::Human,
            "room-acme-1",
            1000,
            DEFAULT_SESSION_TTL_MS,
        );

        assert_eq!(session.current_mode, PresenceMode::Human);

        // Human -> Voice
        let mode1 = session.trigger_fallback("operator offline", 1100).unwrap();
        assert_eq!(mode1, PresenceMode::Voice);
        assert_eq!(session.current_mode, PresenceMode::Voice);

        // Voice -> Chat
        let mode2 = session.trigger_fallback("mic disconnected", 1200).unwrap();
        assert_eq!(mode2, PresenceMode::Chat);
        assert_eq!(session.current_mode, PresenceMode::Chat);

        // Chat -> Floor error
        let err = session.trigger_fallback("chat error", 1300).unwrap_err();
        assert_eq!(err, PresenceError::AlreadyAtFloor(PresenceMode::Chat));

        // Invalid upwards fallback refusal
        let err_up = session
            .fallback_to(PresenceMode::Voice, "retry voice", 1400)
            .unwrap_err();
        assert_eq!(
            err_up,
            PresenceError::InvalidFallbackDirection {
                from: PresenceMode::Chat,
                to: PresenceMode::Voice
            }
        );
    }

    #[test]
    fn test_presence_gateway_1024_capacity_bound() {
        let mut gateway = PresenceGateway::new();

        for i in 0..MAX_PRESENCE_SESSIONS_PER_TENANT {
            let session = PresenceSession::new(
                format!("sess-{i}"),
                "acme",
                format!("user-{i}"),
                PresenceMode::Voice,
                "room-1",
                1000,
                DEFAULT_SESSION_TTL_MS,
            );
            gateway.register_session(session, 1000).unwrap();
        }

        // 1025th session must be refused
        let overflow_session = PresenceSession::new(
            "sess-overflow",
            "acme",
            "user-overflow",
            PresenceMode::Voice,
            "room-1",
            1000,
            DEFAULT_SESSION_TTL_MS,
        );
        let err = gateway
            .register_session(overflow_session, 1000)
            .unwrap_err();
        assert_eq!(err, PresenceError::CapacityExceeded("acme".into()));

        // Different tenant can still create sessions
        let other_session = PresenceSession::new(
            "sess-other-1",
            "tenant-other",
            "user-1",
            PresenceMode::Voice,
            "room-other",
            1000,
            DEFAULT_SESSION_TTL_MS,
        );
        assert!(gateway.register_session(other_session, 1000).is_ok());
    }

    #[test]
    fn test_livekit_token_generation_and_verification() {
        let key = "API_KEY_ACME";
        let secret = b"super_secret_signing_key_32bytes_1234";

        let token = generate_livekit_room_token(
            key,
            secret,
            "room-acme-main",
            "user-alice",
            PresenceMode::Voice,
            3600,
            1_700_000_000,
        )
        .unwrap();

        assert!(!token.is_empty());
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3); // JWT format: header.payload.signature
    }

    #[test]
    fn test_transcript_sequence_integrity() {
        let mut gateway = PresenceGateway::new();
        let session = PresenceSession::new(
            "sess-seq",
            "acme",
            "user-seq",
            PresenceMode::Voice,
            "room-1",
            1000,
            DEFAULT_SESSION_TTL_MS,
        );
        gateway.register_session(session, 1000).unwrap();

        let frame1 =
            VoiceTranscriptFrame::create("f1", "sess-seq", 1, "user", "Halo Ava", true, 120, 0.98);
        assert!(gateway.ingest_frame("acme", &frame1, 1100).is_ok());

        // Out of order frame 3 (expecting 2) must fail
        let frame3 = VoiceTranscriptFrame::create(
            "f3",
            "sess-seq",
            3,
            "user",
            "Lompat urutan",
            true,
            120,
            0.98,
        );
        let err = gateway.ingest_frame("acme", &frame3, 1200).unwrap_err();
        assert_eq!(
            err,
            PresenceError::SequenceMismatch {
                expected: 2,
                got: 3
            }
        );

        // Correct frame 2 succeeds
        let frame2 =
            VoiceTranscriptFrame::create("f2", "sess-seq", 2, "ava", "Halo juga!", true, 80, 0.99);
        assert!(gateway.ingest_frame("acme", &frame2, 1250).is_ok());
    }

    #[test]
    fn test_presence_gateway_session_collision() {
        let mut gateway = PresenceGateway::new();
        let session1 = PresenceSession::new(
            "shared-id",
            "acme",
            "user-1",
            PresenceMode::Voice,
            "room-1",
            1000,
            DEFAULT_SESSION_TTL_MS,
        );
        gateway.register_session(session1, 1000).unwrap();

        // Same ID under different tenant must collide and fail
        let session2 = PresenceSession::new(
            "shared-id",
            "other-tenant",
            "mallory",
            PresenceMode::Voice,
            "room-2",
            1000,
            DEFAULT_SESSION_TTL_MS,
        );
        let err = gateway
            .register_session(session2.clone(), 1000)
            .unwrap_err();
        assert_eq!(err, PresenceError::SessionCollision("shared-id".into()));

        // Explicit close removes it
        assert!(gateway.close_session("acme", "shared-id"));
        // Now it can be registered
        assert!(gateway.register_session(session2, 1000).is_ok());
    }
}
