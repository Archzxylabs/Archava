//! Mission and Flight Plan DAG (Directed Acyclic Graph) for Ava Cockpit.
//! Evaluates work item dependencies, resolving parallel steps and prerequisites.

use crate::flight_status::FlightStatus;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    Running,
    Passed,
    Failed,
    Skipped,
}

/// An individual work item card within the mission's flight plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlightStep {
    pub step_index: usize,
    pub step_name: String,
    pub tool_target: String,
    pub payload_input: serde_json::Value,
    pub payload_output: Option<serde_json::Value>,
    pub status: StepStatus,
    /// Indices of prerequisite steps that must pass before this step can run.
    pub depends_on_steps: Vec<usize>,
    pub subagent_role: String,
    /// Claude Code style condensed summary of results
    pub isolated_context_summary: Option<String>,
    pub error_message: Option<String>,
}

impl FlightStep {
    pub fn new(
        step_index: usize,
        step_name: impl Into<String>,
        tool_target: impl Into<String>,
        payload_input: serde_json::Value,
        depends_on_steps: Vec<usize>,
    ) -> Self {
        Self {
            step_index,
            step_name: step_name.into(),
            tool_target: tool_target.into(),
            payload_input,
            payload_output: None,
            status: StepStatus::Pending,
            depends_on_steps,
            subagent_role: "domain_worker".into(),
            isolated_context_summary: None,
            error_message: None,
        }
    }
}

/// A top-level autonomous objective containing a flight plan DAG.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mission {
    pub id: Uuid,
    pub tenant_id: String,
    pub mission_code: String,
    pub title: String,
    pub objective: String,
    pub status: FlightStatus,
    pub steps: Vec<FlightStep>,
    pub holding_reason: Option<String>,
    pub holding_signal_key: Option<String>,
}

impl Mission {
    pub fn new(
        tenant_id: impl Into<String>,
        mission_code: impl Into<String>,
        title: impl Into<String>,
        objective: impl Into<String>,
        steps: Vec<FlightStep>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            tenant_id: tenant_id.into(),
            mission_code: mission_code.into(),
            title: title.into(),
            objective: objective.into(),
            status: FlightStatus::PreFlight,
            steps,
            holding_reason: None,
            holding_signal_key: None,
        }
    }

    /// Validates DAG integrity: unique step indices, valid dependencies, no self-dependency, and no cycles.
    pub fn validate_dag(&self) -> Result<(), &'static str> {
        if self.steps.is_empty() {
            return Err("Mission must have at least one flight step");
        }

        let mut indices = std::collections::HashSet::new();
        for step in &self.steps {
            if !indices.insert(step.step_index) {
                return Err("Duplicate step_index detected in flight plan");
            }
        }

        for step in &self.steps {
            for dep in &step.depends_on_steps {
                if *dep == step.step_index {
                    return Err("Step cannot depend on itself");
                }
                if !indices.contains(dep) {
                    return Err("Step depends on non-existent step_index");
                }
            }
        }

        // Cycle detection using Kahn's algorithm
        let mut in_degree: std::collections::HashMap<usize, usize> =
            std::collections::HashMap::new();
        let mut adj: std::collections::HashMap<usize, Vec<usize>> =
            std::collections::HashMap::new();

        for step in &self.steps {
            in_degree.entry(step.step_index).or_insert(0);
            for dep in &step.depends_on_steps {
                adj.entry(*dep).or_default().push(step.step_index);
                *in_degree.entry(step.step_index).or_insert(0) += 1;
            }
        }

        let mut queue: std::collections::VecDeque<usize> = in_degree
            .iter()
            .filter(|&(_, &deg)| deg == 0)
            .map(|(&idx, _)| idx)
            .collect();

        let mut visited_count = 0;
        while let Some(u) = queue.pop_front() {
            visited_count += 1;
            if let Some(neighbors) = adj.get(&u) {
                for &v in neighbors {
                    if let Some(deg) = in_degree.get_mut(&v) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push_back(v);
                        }
                    }
                }
            }
        }

        if visited_count != self.steps.len() {
            return Err("Cycle detected in mission DAG flight plan");
        }

        Ok(())
    }

    /// Evaluates the DAG and returns steps that are ready for immediate execution:
    /// Status must be `Pending` and all `depends_on_steps` must be `Passed`.
    pub fn next_executable_steps(&self) -> Vec<&FlightStep> {
        if self.status != FlightStatus::InFlight {
            return Vec::new();
        }

        self.steps
            .iter()
            .filter(|step| {
                if step.status != StepStatus::Pending {
                    return false;
                }
                // Check all prerequisites
                step.depends_on_steps.iter().all(|&prereq_idx| {
                    self.steps
                        .iter()
                        .find(|s| s.step_index == prereq_idx)
                        .map(|s| s.status == StepStatus::Passed)
                        .unwrap_or(false)
                })
            })
            .collect()
    }

    /// Returns true if every step in the flight plan has successfully passed.
    pub fn is_all_steps_passed(&self) -> bool {
        !self.steps.is_empty() && self.steps.iter().all(|s| s.status == StepStatus::Passed)
    }

    /// Transitions mission to Holding Pattern with an L4 confirmation reason and resume signal key.
    pub fn enter_holding_pattern(
        &mut self,
        reason: impl Into<String>,
        signal_key: impl Into<String>,
    ) -> Result<(), &'static str> {
        if !self.status.can_transition_to(FlightStatus::HoldingPattern) {
            return Err("Illegal transition to HoldingPattern");
        }
        self.status = FlightStatus::HoldingPattern;
        self.holding_reason = Some(reason.into());
        self.holding_signal_key = Some(signal_key.into());
        Ok(())
    }

    /// Resumes a mission from Holding Pattern back to In-Flight upon human approval.
    pub fn resume_from_holding(&mut self) -> Result<(), &'static str> {
        if !self.status.can_transition_to(FlightStatus::InFlight) {
            return Err("Illegal transition from HoldingPattern to InFlight");
        }
        self.status = FlightStatus::InFlight;
        self.holding_reason = None;
        self.holding_signal_key = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mission_dag_dependency_resolution() {
        let step1 = FlightStep::new(
            1,
            "Fetch PO",
            "purchasing.get_po",
            serde_json::json!({}),
            vec![],
        );
        let step2 = FlightStep::new(
            2,
            "Verify Warehouse Stock",
            "inventory.check",
            serde_json::json!({}),
            vec![1],
        );
        let step3 = FlightStep::new(
            3,
            "Post Accounting Journal",
            "gl.post",
            serde_json::json!({}),
            vec![2],
        );

        let mut mission = Mission::new(
            "ten_01",
            "PO-RECON",
            "Reconcile PO",
            "Reconcile inbound goods",
            vec![step1, step2, step3],
        );

        // PreFlight: Nothing executable yet
        assert!(mission.next_executable_steps().is_empty());

        // InFlight: Step 1 has no dependencies, so it is executable
        mission.status = FlightStatus::InFlight;
        let ready = mission.next_executable_steps();
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].step_index, 1);

        // Mark Step 1 as passed -> Step 2 becomes executable
        mission.steps[0].status = StepStatus::Passed;
        let ready2 = mission.next_executable_steps();
        assert_eq!(ready2.len(), 1);
        assert_eq!(ready2[0].step_index, 2);

        // Mark Step 2 as passed -> Step 3 becomes executable
        mission.steps[1].status = StepStatus::Passed;
        let ready3 = mission.next_executable_steps();
        assert_eq!(ready3.len(), 1);
        assert_eq!(ready3[0].step_index, 3);

        // Mark Step 3 as passed -> All passed!
        mission.steps[2].status = StepStatus::Passed;
        assert!(mission.is_all_steps_passed());
        assert!(mission.next_executable_steps().is_empty());
    }

    #[test]
    fn test_dag_validation_catches_cycles_and_invalid_deps() {
        let step1 = FlightStep::new(1, "S1", "tool", serde_json::json!({}), vec![2]);
        let step2 = FlightStep::new(2, "S2", "tool", serde_json::json!({}), vec![1]); // Cycle 1 <-> 2
        let cycle_mission = Mission::new("t1", "M1", "Title", "Obj", vec![step1, step2]);
        assert_eq!(
            cycle_mission.validate_dag(),
            Err("Cycle detected in mission DAG flight plan")
        );

        let s_dup1 = FlightStep::new(1, "S1", "tool", serde_json::json!({}), vec![]);
        let s_dup2 = FlightStep::new(1, "S2", "tool", serde_json::json!({}), vec![]);
        let dup_mission = Mission::new("t1", "M2", "Title", "Obj", vec![s_dup1, s_dup2]);
        assert_eq!(
            dup_mission.validate_dag(),
            Err("Duplicate step_index detected in flight plan")
        );

        let s_self = FlightStep::new(1, "S1", "tool", serde_json::json!({}), vec![1]);
        let self_mission = Mission::new("t1", "M3", "Title", "Obj", vec![s_self]);
        assert_eq!(
            self_mission.validate_dag(),
            Err("Step cannot depend on itself")
        );

        let s_missing = FlightStep::new(1, "S1", "tool", serde_json::json!({}), vec![99]);
        let missing_mission = Mission::new("t1", "M4", "Title", "Obj", vec![s_missing]);
        assert_eq!(
            missing_mission.validate_dag(),
            Err("Step depends on non-existent step_index")
        );
    }
}
