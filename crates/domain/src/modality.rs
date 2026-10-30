//! Interaction Modality Engine for Ava (Decoupled Sensory Layer).
//! Supports independent unbundling and mix-and-match of Chat, Voice, and Avatar.

use serde::{Deserialize, Serialize};

/// The sensory presentation and interaction modality configuration for Ava.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AvaModality {
    /// 1. Chat Only: Lightweight text streaming + interactive action cards (QRIS, SOW, products).
    ChatOnly,
    /// 2. Voice Only: Low-latency bidirectional audio streaming (WebRTC / Opus / WhatsApp voice notes).
    VoiceOnly,
    /// 3. Avatar + Voice: Humanoid 3D/video concierge with lip-sync, no text chatbox.
    AvatarVoice,
    /// 4. Full Experience: Avatar visual + voice audio + interactive chat & action cards.
    #[default]
    Full,
    /// 5. Avatar + Chat Mute: Visual ambient gestures/lip-sync with silent text chat.
    AvatarChat,
}

impl AvaModality {
    /// Parses a string representation into a strongly-typed `AvaModality`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "chat_only" | "chat" => Some(Self::ChatOnly),
            "voice_only" | "voice" => Some(Self::VoiceOnly),
            "avatar_voice" | "concierge" => Some(Self::AvatarVoice),
            "full" | "all" => Some(Self::Full),
            "avatar_chat" | "silent_avatar" => Some(Self::AvatarChat),
            _ => None,
        }
    }

    /// Whether this modality includes an interactive text chatbox.
    pub fn has_chat(&self) -> bool {
        matches!(self, Self::ChatOnly | Self::Full | Self::AvatarChat)
    }

    /// Whether this modality includes real-time microphone input and audio synthesis.
    pub fn has_voice(&self) -> bool {
        matches!(self, Self::VoiceOnly | Self::AvatarVoice | Self::Full)
    }

    /// Whether this modality includes a visual 3D canvas or neural video avatar.
    pub fn has_avatar(&self) -> bool {
        matches!(self, Self::AvatarVoice | Self::Full | Self::AvatarChat)
    }

    /// Returns the canonical string identifier for the modality.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::ChatOnly => "chat_only",
            Self::VoiceOnly => "voice_only",
            Self::AvatarVoice => "avatar_voice",
            Self::Full => "full",
            Self::AvatarChat => "avatar_chat",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modality_parsing_and_flags() {
        assert_eq!(AvaModality::parse("chat_only"), Some(AvaModality::ChatOnly));
        assert_eq!(
            AvaModality::parse("voice_only"),
            Some(AvaModality::VoiceOnly)
        );
        assert_eq!(
            AvaModality::parse("avatar_voice"),
            Some(AvaModality::AvatarVoice)
        );
        assert_eq!(AvaModality::parse("full"), Some(AvaModality::Full));
        assert_eq!(
            AvaModality::parse("avatar_chat"),
            Some(AvaModality::AvatarChat)
        );
        assert_eq!(AvaModality::parse("invalid_mode"), None);

        // Chat Only: has chat, no voice, no avatar
        let chat = AvaModality::ChatOnly;
        assert!(chat.has_chat());
        assert!(!chat.has_voice());
        assert!(!chat.has_avatar());

        // Voice Only: no chat, has voice, no avatar
        let voice = AvaModality::VoiceOnly;
        assert!(!voice.has_chat());
        assert!(voice.has_voice());
        assert!(!voice.has_avatar());

        // Avatar + Voice: no chat, has voice, has avatar
        let concierge = AvaModality::AvatarVoice;
        assert!(!concierge.has_chat());
        assert!(concierge.has_voice());
        assert!(concierge.has_avatar());

        // Full: all enabled
        let full = AvaModality::Full;
        assert!(full.has_chat());
        assert!(full.has_voice());
        assert!(full.has_avatar());

        // Avatar + Chat: has chat, no voice, has avatar
        let silent = AvaModality::AvatarChat;
        assert!(silent.has_chat());
        assert!(!silent.has_voice());
        assert!(silent.has_avatar());
    }

    #[test]
    fn test_modality_serialization_roundtrip() {
        for modality in [
            AvaModality::ChatOnly,
            AvaModality::VoiceOnly,
            AvaModality::AvatarVoice,
            AvaModality::Full,
            AvaModality::AvatarChat,
        ] {
            let json = serde_json::to_string(&modality).unwrap();
            let parsed: AvaModality = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, modality);
        }
    }
}
