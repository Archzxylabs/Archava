//! Episodic Memory for Ava Orbz (CoALA Quadrant 2).
//! Stores chronological interaction streams, customer dialogues, and tool audit trails.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A discrete episodic interaction between a user and Ava.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    pub id: Uuid,
    pub tenant_id: String,
    pub session_id: String,
    pub user_id: String,
    pub user_role: String,
    pub channel: String,
    pub user_utterance: String,
    pub agent_response: String,
    pub tools_called: Vec<String>,
    pub timestamp_millis: i64,
}

impl Episode {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tenant_id: impl Into<String>,
        session_id: impl Into<String>,
        user_id: impl Into<String>,
        user_role: impl Into<String>,
        channel: impl Into<String>,
        user_utterance: impl Into<String>,
        agent_response: impl Into<String>,
        tools_called: Vec<String>,
        timestamp_millis: i64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.into(),
            session_id: session_id.into(),
            user_id: user_id.into(),
            user_role: user_role.into(),
            channel: channel.into(),
            user_utterance: user_utterance.into(),
            agent_response: agent_response.into(),
            tools_called,
            timestamp_millis,
        }
    }
}
