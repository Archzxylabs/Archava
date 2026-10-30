//! Durable Execution Checkpoints for Ava Cockpit (Temporal.io Paradigm).
//! Snapshots mission state, completed step outputs, and intermediate variables.
//! Enables immediate crash recovery so server restarts never drop in-flight workflows.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightCheckpoint {
    pub mission_id: Uuid,
    pub current_step_index: usize,
    pub step_outputs: HashMap<usize, serde_json::Value>,
    pub execution_variables: HashMap<String, serde_json::Value>,
    pub saved_at_millis: i64,
}

impl FlightCheckpoint {
    pub fn new(mission_id: Uuid, current_step_index: usize, saved_at_millis: i64) -> Self {
        Self {
            mission_id,
            current_step_index,
            step_outputs: HashMap::new(),
            execution_variables: HashMap::new(),
            saved_at_millis,
        }
    }

    pub fn record_step_output(&mut self, step_index: usize, output: serde_json::Value) {
        self.step_outputs.insert(step_index, output);
    }
}
