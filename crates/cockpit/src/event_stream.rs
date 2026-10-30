//! Event-sourced stream for Ava Cockpit (OpenHands Paradigm).
//! Tracks immutable sequences of Actions and Observations, enabling
//! full real-time telemetry streaming over WebSockets and state reduction.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightEventType {
    ActionDispatched,
    ObservationReturned,
    StatusChanged,
    CheckpointSaved,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightEvent {
    pub id: u64,
    pub mission_id: Uuid,
    pub tenant_id: String,
    pub event_type: FlightEventType,
    pub event_payload: serde_json::Value,
    pub timestamp_millis: i64,
}

impl FlightEvent {
    pub fn new(
        id: u64,
        mission_id: Uuid,
        tenant_id: impl Into<String>,
        event_type: FlightEventType,
        event_payload: serde_json::Value,
        timestamp_millis: i64,
    ) -> Self {
        Self {
            id,
            mission_id,
            tenant_id: tenant_id.into(),
            event_type,
            event_payload,
            timestamp_millis,
        }
    }
}
