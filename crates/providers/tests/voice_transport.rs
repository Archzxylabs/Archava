//! Realtime Voice & Presence Transport integration tests.

use archava_domain::presence::{PresenceError, PresenceMode, VoiceTranscriptFrame};
use archava_providers::voice::{VoiceClient, VoiceProviderError};

#[tokio::test]
async fn test_voice_session_handshake_and_token() {
    let client = VoiceClient::for_loopback();

    let (session, token) = client
        .start_session(
            "tenant-test",
            "sess-100",
            "user-alice",
            PresenceMode::Voice,
            1000,
        )
        .expect("handshake must succeed");

    assert_eq!(session.current_mode, PresenceMode::Voice);
    assert!(session.room_name.starts_with("archava-tenant-test-"));
    assert_eq!(session.room_name.len(), "archava-tenant-test-".len() + 32);
    assert!(!token.is_empty());
    assert_eq!(token.split('.').count(), 3);
}

#[tokio::test]
async fn test_room_name_delimiter_collision_prevention() {
    let client = VoiceClient::for_loopback();
    let (s1, _) = client
        .start_session("acme", "west-123", "alice", PresenceMode::Voice, 1000)
        .unwrap();
    let (s2, _) = client
        .start_session("acme-west", "123", "alice", PresenceMode::Voice, 1000)
        .unwrap();
    assert_ne!(
        s1.room_name, s2.room_name,
        "delimiter collision: acme/west-123 and acme-west/123 must not alias to the same room"
    );
}

#[tokio::test]
async fn test_voice_transcript_bidirectional_exchange() {
    let client = VoiceClient::for_loopback();
    client
        .start_session("acme", "sess-turn", "user-bob", PresenceMode::Voice, 2000)
        .unwrap();

    let user_frame = VoiceTranscriptFrame::create(
        "f-user-1",
        "sess-turn",
        1,
        "user",
        "Buatkan invoice baru dong",
        true,
        90,
        0.97,
    );

    let reply = client
        .process_transcript_turn("acme", &user_frame, 2100)
        .expect("turn processing must succeed");

    assert_eq!(reply.sequence_number, 2);
    assert_eq!(reply.speaker, "ava");
    assert!(reply.text.contains("Buatkan invoice baru dong"));
    assert!(!reply.digest.is_empty());
}

#[tokio::test]
async fn test_voice_degradation_to_chat_fallback() {
    let client = VoiceClient::for_loopback();
    client
        .start_session(
            "acme",
            "sess-fallback",
            "user-charlie",
            PresenceMode::Voice,
            3000,
        )
        .unwrap();

    let new_mode = client
        .degrade_to_chat("acme", "sess-fallback", "webrtc connection dropped", 3500)
        .expect("degradation must succeed");

    assert_eq!(new_mode, PresenceMode::Chat);

    let session = client.get_session("acme", "sess-fallback", 3600).unwrap();
    assert_eq!(session.current_mode, PresenceMode::Chat);
    assert_eq!(session.fallback_history.len(), 1);
    assert_eq!(session.fallback_history[0].from_mode, PresenceMode::Voice);
    assert_eq!(session.fallback_history[0].to_mode, PresenceMode::Chat);
    assert_eq!(
        session.fallback_history[0].reason,
        "webrtc connection dropped"
    );
}

#[tokio::test]
async fn test_voice_cross_tenant_isolation_refusal() {
    let client = VoiceClient::for_loopback();
    client
        .start_session(
            "tenant-a",
            "sess-iso",
            "user-dan",
            PresenceMode::Voice,
            4000,
        )
        .unwrap();

    let err = client
        .get_session("tenant-b", "sess-iso", 4100)
        .expect_err("foreign tenant access must be refused");

    match err {
        VoiceProviderError::Presence(PresenceError::SessionNotFound(id)) => {
            assert_eq!(id, "sess-iso");
        }
        other => panic!("unexpected error: {:?}", other),
    }
}
