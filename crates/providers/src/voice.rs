//! Realtime Voice & Presence Transport Provider (LiveKit & Gemini Realtime bridge).
//!
//! Provides the presence transport seam:
//! - LiveKit JWT token generation for client WebRTC rooms.
//! - Synthetic bidirectional transcript frames carrying audio events and telemetry.
//! - Enforces strict fallback: Voice -> Chat degradation on disconnect or timeout.
//! - Loopback mode for isolated, offline, and non-network integration testing.

use archava_domain::presence::{
    generate_livekit_room_token, PresenceError, PresenceGateway, PresenceMode, PresenceSession,
    VoiceTranscriptFrame, DEFAULT_SESSION_TTL_MS,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VoiceProviderError {
    #[error("Domain presence error: {0}")]
    Presence(#[from] PresenceError),

    #[error("LiveKit connection or handshake timeout")]
    HandshakeTimeout,

    #[error("Provider degraded: {0}; session fallen back to Chat mode")]
    DegradedToChat(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// Configuration for the realtime voice bridge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceBridgeConfig {
    pub livekit_url: String,
    pub api_key: String,
    #[serde(skip_serializing)]
    pub api_secret: Vec<u8>,
    pub request_timeout: Duration,
    pub loopback_mode: bool,
}

impl Default for VoiceBridgeConfig {
    fn default() -> Self {
        Self {
            livekit_url: "https://livekit.local.archava.internal".into(),
            api_key: "dev_key".into(),
            api_secret: b"dev_secret_key_32_bytes_archava_".to_vec(),
            request_timeout: Duration::from_secs(4),
            loopback_mode: true,
        }
    }
}

/// Realtime voice client managing presence tokens and transcript streaming.
#[derive(Debug, Clone)]
pub struct VoiceClient {
    config: VoiceBridgeConfig,
    gateway: Arc<Mutex<PresenceGateway>>,
}

impl VoiceClient {
    pub fn new(config: VoiceBridgeConfig) -> Self {
        Self {
            config,
            gateway: Arc::new(Mutex::new(PresenceGateway::new())),
        }
    }

    /// Creates a loopback client for mock and unit testing without network dependencies.
    pub fn for_loopback() -> Self {
        Self::new(VoiceBridgeConfig::default())
    }

    /// Creates an active presence session and mints the signed LiveKit room join token.
    pub fn start_session(
        &self,
        tenant_id: &str,
        session_id: &str,
        participant_id: &str,
        requested_mode: PresenceMode,
        now_epoch_ms: u64,
    ) -> Result<(PresenceSession, String), VoiceProviderError> {
        // C10-02: Derive room_name from length-prefixed digest to eliminate delimiter collision
        let mut hasher = Sha256::new();
        hasher.update((tenant_id.len() as u32).to_be_bytes());
        hasher.update(tenant_id.as_bytes());
        hasher.update((session_id.len() as u32).to_be_bytes());
        hasher.update(session_id.as_bytes());
        let digest: String = hasher.finalize()[..16]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let room_name = format!("archava-{tenant_id}-{digest}");

        let session = PresenceSession::new(
            session_id,
            tenant_id,
            participant_id,
            requested_mode,
            &room_name,
            now_epoch_ms,
            DEFAULT_SESSION_TTL_MS,
        );

        let now_sec = now_epoch_ms / 1000;
        let token = generate_livekit_room_token(
            &self.config.api_key,
            &self.config.api_secret,
            &room_name,
            participant_id,
            requested_mode,
            3600,
            now_sec,
        )?;

        let mut gw = self
            .gateway
            .lock()
            .map_err(|_| PresenceError::SigningError("mutex poisoned".into()))?;
        gw.register_session(session.clone(), now_epoch_ms)?;

        Ok((session, token))
    }

    /// Streams an incoming voice transcript frame from the client or synthetic speech engine.
    pub fn process_transcript_turn(
        &self,
        tenant_id: &str,
        frame: &VoiceTranscriptFrame,
        now_epoch_ms: u64,
    ) -> Result<VoiceTranscriptFrame, VoiceProviderError> {
        let mut gw = self
            .gateway
            .lock()
            .map_err(|_| PresenceError::SigningError("mutex poisoned".into()))?;

        gw.ingest_frame(tenant_id, frame, now_epoch_ms)?;

        // Produce deterministic assistant turn response
        let reply_seq = frame.sequence_number + 1;
        let reply_text = format!("Acknowledged: {}", frame.text);

        let reply_frame = VoiceTranscriptFrame::create(
            format!("reply-{}", frame.frame_id),
            &frame.session_id,
            reply_seq,
            "ava",
            reply_text,
            true,
            65,
            0.99,
        );

        gw.ingest_frame(tenant_id, &reply_frame, now_epoch_ms + 65)?;

        Ok(reply_frame)
    }

    /// Triggers an immediate degradation to Chat mode when WebRTC media fails or drops.
    pub fn degrade_to_chat(
        &self,
        tenant_id: &str,
        session_id: &str,
        reason: &str,
        now_epoch_ms: u64,
    ) -> Result<PresenceMode, VoiceProviderError> {
        let mut gw = self
            .gateway
            .lock()
            .map_err(|_| PresenceError::SigningError("mutex poisoned".into()))?;

        let new_mode = gw.trigger_fallback(tenant_id, session_id, reason, now_epoch_ms)?;
        Ok(new_mode)
    }

    /// Retrieves current session state.
    pub fn get_session(
        &self,
        tenant_id: &str,
        session_id: &str,
        now_epoch_ms: u64,
    ) -> Result<PresenceSession, VoiceProviderError> {
        let gw = self
            .gateway
            .lock()
            .map_err(|_| PresenceError::SigningError("mutex poisoned".into()))?;
        let session = gw.get_session(tenant_id, session_id, now_epoch_ms)?;
        Ok(session.clone())
    }

    /// Finds session across tenants.
    pub fn find_session(
        &self,
        session_id: &str,
        now_epoch_ms: u64,
    ) -> Result<PresenceSession, VoiceProviderError> {
        let gw = self
            .gateway
            .lock()
            .map_err(|_| PresenceError::SigningError("mutex poisoned".into()))?;
        let session = gw.find_session_by_id(session_id, now_epoch_ms)?;
        Ok(session.clone())
    }
}
