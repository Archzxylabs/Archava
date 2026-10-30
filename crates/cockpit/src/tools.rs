//! Hermes-style Autonomous Cockpit Toolset (`cockpit_*`).
//! Exposes structured operations for Ava to inspect, advance, and govern
//! flight missions autonomously through tool calls.

use crate::flight_status::FlightStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", content = "params")]
pub enum CockpitCommand {
    /// List active missions on the Cockpit radar.
    ListMissions { status_filter: Option<FlightStatus> },
    /// Dispatches and starts execution on a specific step.
    ClaimStep { mission_id: Uuid, step_index: usize },
    /// Records completion of a step with an isolated summary and output payload.
    CompleteStep {
        mission_id: Uuid,
        step_index: usize,
        summary: String,
        payload_output: serde_json::Value,
    },
    /// Enters Holding Pattern awaiting human confirmation (L4 Gate).
    EnterHoldingPattern {
        mission_id: Uuid,
        reason: String,
        signal_key: String,
    },
    /// Completes the entire mission, landing into Touchdown after all steps passed.
    Touchdown {
        mission_id: Uuid,
        final_summary: String,
    },
    /// Aborts and rolls back the mission due to unrecoverable failure or refusal.
    Abort { mission_id: Uuid, reason: String },
}
