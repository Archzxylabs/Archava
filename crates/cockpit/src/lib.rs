//! Ava Cockpit: Autonomous Durable Execution & Task Engine for Archava.
//! Synthesizes Temporal.io (Durable Checkpoints), LangGraph (StateGraph & Cyclic ReAct),
//! OpenHands (Event-Sourced Stream), and Claude Code (Subagent Context Isolation).

pub mod durable;
pub mod event_stream;
pub mod flight_status;
pub mod mission;
pub mod subagent;
pub mod tools;

use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub use durable::FlightCheckpoint;
pub use event_stream::{FlightEvent, FlightEventType};
pub use flight_status::FlightStatus;
pub use mission::{FlightStep, Mission, StepStatus};
pub use subagent::{SubagentWorkerContext, WorkerReport};
pub use tools::CockpitCommand;

/// Thread-safe in-memory execution engine for Ava Cockpit missions.
#[derive(Debug, Default, Clone)]
pub struct CockpitEngine {
    missions: Arc<RwLock<HashMap<Uuid, Mission>>>,
    events: Arc<RwLock<Vec<FlightEvent>>>,
    checkpoints: Arc<RwLock<HashMap<Uuid, FlightCheckpoint>>>,
}

impl CockpitEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a new mission into the Cockpit.
    pub fn register_mission(&self, mission: Mission) -> Uuid {
        let id = mission.id;
        self.missions.write().expect("lock").insert(id, mission);
        id
    }

    /// Dispatches a mission from Pre-Flight into In-Flight.
    pub fn launch_flight(&self, mission_id: Uuid) -> Result<(), &'static str> {
        let mut guard = self.missions.write().expect("lock");
        let mission = guard.get_mut(&mission_id).ok_or("Mission not found")?;

        if !mission.status.can_transition_to(FlightStatus::InFlight) {
            return Err("Illegal transition to InFlight");
        }

        mission.status = FlightStatus::InFlight;
        Ok(())
    }

    /// Records the completion of an individual flight step.
    /// Strictly verifies that the mission is InFlight and all dependency prerequisites have passed.
    pub fn record_step_result(
        &self,
        mission_id: Uuid,
        step_index: usize,
        report: WorkerReport,
        now_millis: i64,
    ) -> Result<(), &'static str> {
        let mut guard = self.missions.write().expect("lock");
        let mission = guard.get_mut(&mission_id).ok_or("Mission not found")?;

        if mission.status != FlightStatus::InFlight {
            return Err("Cannot complete step on a mission that is not InFlight");
        }

        // Verify dependencies
        let step_ref = mission
            .steps
            .iter()
            .find(|s| s.step_index == step_index)
            .ok_or("Step index not found")?;

        // Terminal state check: finalized steps are immutable to prevent tampering or overwriting Passed with Failed
        if step_ref.status == StepStatus::Passed || step_ref.status == StepStatus::Failed {
            let was_passed = step_ref.status == StepStatus::Passed;
            if was_passed == report.success {
                return Ok(()); // Idempotent execution
            }
            return Err("Cannot mutate already finalized terminal step");
        }

        for dep_idx in &step_ref.depends_on_steps {
            let dep_ok = mission
                .steps
                .iter()
                .any(|s| s.step_index == *dep_idx && s.status == StepStatus::Passed);
            if !dep_ok {
                return Err("Prerequisite step dependency has not passed");
            }
        }

        let step = mission
            .steps
            .iter_mut()
            .find(|s| s.step_index == step_index)
            .ok_or("Step index not found")?;

        if report.success {
            step.status = StepStatus::Passed;
            step.isolated_context_summary = Some(report.high_signal_summary);
            step.payload_output = Some(report.output_payload);
        } else {
            step.status = StepStatus::Failed;
            step.error_message = Some(report.high_signal_summary);
        }

        // Save durable checkpoint (Temporal.io pattern)
        let mut checkpoints_guard = self.checkpoints.write().expect("lock");
        let checkpoint = checkpoints_guard
            .entry(mission_id)
            .or_insert_with(|| FlightCheckpoint::new(mission_id, step_index, now_millis));
        checkpoint.current_step_index = step_index;
        if let Some(ref out) = step.payload_output {
            checkpoint.record_step_output(step_index, out.clone());
        }

        Ok(())
    }

    /// Lands a mission into Touchdown once all steps have passed and invariants verified.
    pub fn touchdown(&self, mission_id: Uuid) -> Result<(), &'static str> {
        let mut guard = self.missions.write().expect("lock");
        let mission = guard.get_mut(&mission_id).ok_or("Mission not found")?;

        if !mission.is_all_steps_passed() {
            return Err("Cannot touchdown: not all steps have passed");
        }

        if !mission.status.can_transition_to(FlightStatus::Touchdown) {
            return Err("Illegal transition to Touchdown");
        }

        mission.status = FlightStatus::Touchdown;
        Ok(())
    }

    /// Aborts a mission due to failure, rejection, or invariant violation.
    pub fn abort_flight(
        &self,
        mission_id: Uuid,
        reason: impl Into<String>,
    ) -> Result<(), &'static str> {
        let mut guard = self.missions.write().expect("lock");
        let mission = guard.get_mut(&mission_id).ok_or("Mission not found")?;

        if !mission.status.can_transition_to(FlightStatus::Aborted) {
            return Err("Illegal transition to Aborted");
        }

        mission.status = FlightStatus::Aborted;
        mission.holding_reason = Some(reason.into());
        Ok(())
    }

    /// Appends an immutable flight event to the stream (OpenHands pattern).
    pub fn emit_event(&self, event: FlightEvent) {
        self.events.write().expect("lock").push(event);
    }

    /// Retrieves a mission by ID.
    pub fn get_mission(&self, mission_id: Uuid) -> Option<Mission> {
        self.missions
            .read()
            .expect("lock")
            .get(&mission_id)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_full_cockpit_mission_lifecycle() {
        let engine = CockpitEngine::new();
        let step1 = FlightStep::new(
            1,
            "Parse Order",
            "order.parse",
            serde_json::json!({}),
            vec![],
        );
        let step2 = FlightStep::new(2, "Post GL", "gl.post", serde_json::json!({}), vec![1]);

        let mission = Mission::new(
            "ten_01",
            "ORD-101",
            "Process Order",
            "Order processing",
            vec![step1, step2],
        );
        let mission_id = engine.register_mission(mission);

        // 1. Launch flight
        engine.launch_flight(mission_id).expect("launch ok");
        assert_eq!(
            engine.get_mission(mission_id).unwrap().status,
            FlightStatus::InFlight
        );

        // 1b. Attempting step 2 before prerequisite step 1 must fail (C04 enforcement)
        let report_early =
            WorkerReport::success("worker", "Premature run", serde_json::json!({}), 50);
        let early_res = engine.record_step_result(mission_id, 2, report_early, 1000);
        assert_eq!(
            early_res,
            Err("Prerequisite step dependency has not passed")
        );

        // 2. Complete Step 1
        let report1 = WorkerReport::success(
            "parser",
            "Order parsed successfully with 2 lines",
            serde_json::json!({ "total": 10000 }),
            120,
        );
        engine
            .record_step_result(mission_id, 1, report1, 1000)
            .expect("step 1 ok");

        // 3. Complete Step 2
        let report2 = WorkerReport::success(
            "gl_worker",
            "Journal #401 posted. Debit = Credit = 10000",
            serde_json::json!({ "journal_id": 401 }),
            180,
        );
        engine
            .record_step_result(mission_id, 2, report2, 2000)
            .expect("step 2 ok");

        // 4. Touchdown!
        engine.touchdown(mission_id).expect("touchdown ok");
        assert_eq!(
            engine.get_mission(mission_id).unwrap().status,
            FlightStatus::Touchdown
        );
    }

    #[test]
    fn test_terminal_step_cannot_be_overwritten_from_passed_to_failed() {
        let engine = CockpitEngine::new();
        let step1 = FlightStep::new(1, "Step 1", "tool.s1", serde_json::json!({}), vec![]);
        let mission = Mission::new(
            "tenant_1",
            "MISSION-IMMUTABLE",
            "Immutability Test",
            "Test",
            vec![step1],
        );
        let mission_id = engine.register_mission(mission);
        engine.launch_flight(mission_id).expect("launch ok");

        let rep_ok = WorkerReport::success(
            "worker",
            "Passed successfully",
            serde_json::json!({ "done": true }),
            100,
        );
        engine
            .record_step_result(mission_id, 1, rep_ok, 1000)
            .expect("step 1 passed");

        // Idempotent retry with identical success outcome is accepted
        let rep_ok_retry = WorkerReport::success(
            "worker",
            "Passed again",
            serde_json::json!({ "done": true }),
            100,
        );
        assert!(engine
            .record_step_result(mission_id, 1, rep_ok_retry, 1010)
            .is_ok());

        // Conflicting failure report trying to mutate Passed to Failed must be REFUSED
        let rep_fail =
            WorkerReport::failure("worker", "Attempting to corrupt step 1 to failed", 50);
        let res = engine.record_step_result(mission_id, 1, rep_fail, 1020);
        assert_eq!(res, Err("Cannot mutate already finalized terminal step"));
    }
}
