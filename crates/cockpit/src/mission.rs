//! Mission and Flight Plan DAG (Directed Acyclic Graph) for Ava Cockpit.
//! Evaluates work item dependencies, resolving parallel steps and prerequisites.

use crate::flight_status::FlightStatus;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ApprovalGateError {
    #[error("No approval gate configured for this step")]
    NoGateConfigured,
    #[error("Gate is already in approved status")]
    AlreadyApproved,
    #[error("Gate has been rejected: {0}")]
    AlreadyRejected(String),
    #[error("Step requires approval before transitioning to running")]
    ApprovalRequired,
    #[error("Missing approval token")]
    MissingApprovalToken,
    #[error("Invalid approval token")]
    InvalidApprovalToken,
    #[error("Insufficient role: expected '{expected}', got '{actual}'")]
    InsufficientRole { expected: String, actual: String },
    #[error("Insufficient capability tier: expected '{expected}', got '{actual}'")]
    InsufficientCapability { expected: String, actual: String },
    #[error("Self-approval is forbidden for high-impact authority level {0}")]
    SelfApprovalForbidden(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GateStatus {
    AwaitingApproval,
    Approved,
    Rejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupervisorStamp {
    pub supervisor_id: String,
    pub role: String,
    pub capability: String,
    pub approval_token: String,
    pub approved_at_millis: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalGate {
    pub required_level: String, // e.g. "L4" or "L5"
    pub required_role: String,  // e.g. "owner" or "admin"
    pub status: GateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stamp: Option<SupervisorStamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

impl ApprovalGate {
    pub fn new(required_level: impl Into<String>, required_role: impl Into<String>) -> Self {
        Self {
            required_level: required_level.into(),
            required_role: required_role.into(),
            status: GateStatus::AwaitingApproval,
            stamp: None,
            rejection_reason: None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn compute_token(
        secret_key: &[u8],
        tenant: &str,
        mission_id: Uuid,
        step_index: usize,
        supervisor_id: &str,
        role: &str,
        capability: &str,
        approved_at_millis: i64,
    ) -> String {
        let mut mac =
            HmacSha256::new_from_slice(secret_key).expect("HMAC can take key of any size");
        let payload = format!(
            "{}:{}:{}:{}:{}:{}:{}",
            tenant, mission_id, step_index, supervisor_id, role, capability, approved_at_millis
        );
        mac.update(payload.as_bytes());
        let result = mac.finalize().into_bytes();
        result.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn approve(
        &mut self,
        secret_key: &[u8],
        tenant: &str,
        mission_id: Uuid,
        step_index: usize,
        supervisor_id: &str,
        role: &str,
        capability: &str,
        approved_at_millis: i64,
        initiator_id: &str,
    ) -> Result<String, ApprovalGateError> {
        if self.status == GateStatus::Approved {
            return Err(ApprovalGateError::AlreadyApproved);
        }
        if let Some(ref reason) = self.rejection_reason {
            return Err(ApprovalGateError::AlreadyRejected(reason.clone()));
        }

        let level_upper = self.required_level.to_uppercase();
        if (level_upper == "L4" || level_upper == "L5") && supervisor_id == initiator_id {
            return Err(ApprovalGateError::SelfApprovalForbidden(
                self.required_level.clone(),
            ));
        }

        let role_lower = role.to_lowercase();
        let expected_role_lower = self.required_role.to_lowercase();
        let role_ok = match expected_role_lower.as_str() {
            "owner" => role_lower == "owner",
            "admin" => role_lower == "owner" || role_lower == "admin",
            _ => {
                role_lower == expected_role_lower || role_lower == "owner" || role_lower == "admin"
            }
        };
        if !role_ok {
            return Err(ApprovalGateError::InsufficientRole {
                expected: self.required_role.clone(),
                actual: role.to_string(),
            });
        }

        let cap_lower = capability.to_lowercase();
        if (level_upper == "L4" || level_upper == "L5") && cap_lower == "assist" {
            return Err(ApprovalGateError::InsufficientCapability {
                expected: "enterprise or execute".into(),
                actual: capability.to_string(),
            });
        }

        let token = Self::compute_token(
            secret_key,
            tenant,
            mission_id,
            step_index,
            supervisor_id,
            role,
            capability,
            approved_at_millis,
        );

        self.status = GateStatus::Approved;
        self.stamp = Some(SupervisorStamp {
            supervisor_id: supervisor_id.to_string(),
            role: role.to_string(),
            capability: capability.to_string(),
            approval_token: token.clone(),
            approved_at_millis,
        });

        Ok(token)
    }

    pub fn verify_token(
        &self,
        secret_key: &[u8],
        tenant: &str,
        mission_id: Uuid,
        step_index: usize,
        token: &str,
    ) -> Result<(), ApprovalGateError> {
        if self.status != GateStatus::Approved {
            return Err(ApprovalGateError::ApprovalRequired);
        }
        let Some(ref stamp) = self.stamp else {
            return Err(ApprovalGateError::MissingApprovalToken);
        };

        let expected_token = Self::compute_token(
            secret_key,
            tenant,
            mission_id,
            step_index,
            &stamp.supervisor_id,
            &stamp.role,
            &stamp.capability,
            stamp.approved_at_millis,
        );

        let expected_bytes = expected_token.as_bytes();
        let stored_bytes = stamp.approval_token.as_bytes();
        let presented_bytes = token.as_bytes();

        let token_matches: bool =
            (expected_bytes.ct_eq(stored_bytes) & expected_bytes.ct_eq(presented_bytes)).into();

        if !token_matches {
            return Err(ApprovalGateError::InvalidApprovalToken);
        }

        Ok(())
    }

    pub fn reject(
        &mut self,
        supervisor_id: &str,
        reason: impl Into<String>,
    ) -> Result<(), ApprovalGateError> {
        if self.status == GateStatus::Approved {
            return Err(ApprovalGateError::AlreadyApproved);
        }
        let reason_str = reason.into();
        self.status = GateStatus::Rejected;
        self.rejection_reason = Some(format!("{}: {}", supervisor_id, reason_str));
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    RequiresApproval,
    Running,
    Passed,
    Failed,
    Skipped,
}

impl StepStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::RequiresApproval => "requires_approval",
            Self::Running => "running",
            Self::Passed => "passed",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_gate: Option<ApprovalGate>,
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
            approval_gate: None,
        }
    }

    pub fn with_approval_gate(mut self, gate: ApprovalGate) -> Self {
        self.status = StepStatus::RequiresApproval;
        self.approval_gate = Some(gate);
        self
    }

    pub fn requires_approval(&self) -> bool {
        self.status == StepStatus::RequiresApproval
            || self
                .approval_gate
                .as_ref()
                .is_some_and(|g| g.status == GateStatus::AwaitingApproval)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn approve_gate(
        &mut self,
        secret_key: &[u8],
        tenant: &str,
        mission_id: Uuid,
        supervisor_id: &str,
        role: &str,
        capability: &str,
        approved_at_millis: i64,
        initiator_id: &str,
    ) -> Result<String, ApprovalGateError> {
        let gate = self
            .approval_gate
            .as_mut()
            .ok_or(ApprovalGateError::NoGateConfigured)?;
        let token = gate.approve(
            secret_key,
            tenant,
            mission_id,
            self.step_index,
            supervisor_id,
            role,
            capability,
            approved_at_millis,
            initiator_id,
        )?;
        if self.status == StepStatus::RequiresApproval {
            self.status = StepStatus::Pending;
        }
        Ok(token)
    }

    pub fn transition_to_running(
        &mut self,
        secret_key: &[u8],
        tenant: &str,
        mission_id: Uuid,
        token: Option<&str>,
    ) -> Result<(), ApprovalGateError> {
        if self.status == StepStatus::RequiresApproval {
            let Some(ref gate) = self.approval_gate else {
                return Err(ApprovalGateError::ApprovalRequired);
            };
            if gate.status != GateStatus::Approved {
                return Err(ApprovalGateError::ApprovalRequired);
            }
            let presented_token = token.ok_or(ApprovalGateError::MissingApprovalToken)?;
            gate.verify_token(
                secret_key,
                tenant,
                mission_id,
                self.step_index,
                presented_token,
            )?;
        } else if let Some(ref gate) = self.approval_gate {
            if gate.status != GateStatus::Approved {
                return Err(ApprovalGateError::ApprovalRequired);
            }
            if let Some(t) = token {
                gate.verify_token(secret_key, tenant, mission_id, self.step_index, t)?;
            }
        }
        self.status = StepStatus::Running;
        Ok(())
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

    #[test]
    fn test_approval_gate_lifecycle_and_cryptographic_verification() {
        let key = b"super-secret-key-32-bytes-long!!";
        let tenant = "acme";
        let mission_id = Uuid::new_v4();

        let gate = ApprovalGate::new("L4", "owner");
        let mut step = FlightStep::new(
            1,
            "High Impact Payout",
            "payout.exec",
            serde_json::json!({}),
            vec![],
        )
        .with_approval_gate(gate);

        assert_eq!(step.status, StepStatus::RequiresApproval);
        assert!(step.requires_approval());

        // Attempt transition to running without approval fails
        let run_attempt = step.transition_to_running(key, tenant, mission_id, None);
        assert_eq!(run_attempt, Err(ApprovalGateError::ApprovalRequired));

        // Self-approval by initiator (alice) is refused
        let self_approval = step.approve_gate(
            key,
            tenant,
            mission_id,
            "alice", // supervisor is initiator
            "owner",
            "enterprise",
            1700000000,
            "alice", // initiator
        );
        assert_eq!(
            self_approval,
            Err(ApprovalGateError::SelfApprovalForbidden("L4".into()))
        );

        // Insufficient role (viewer) is refused
        let role_fail = step.approve_gate(
            key,
            tenant,
            mission_id,
            "bob",
            "viewer",
            "enterprise",
            1700000000,
            "alice",
        );
        assert!(matches!(
            role_fail,
            Err(ApprovalGateError::InsufficientRole { .. })
        ));

        // Insufficient capability (assist) is refused
        let cap_fail = step.approve_gate(
            key, tenant, mission_id, "bob", "owner", "assist", 1700000000, "alice",
        );
        assert!(matches!(
            cap_fail,
            Err(ApprovalGateError::InsufficientCapability { .. })
        ));

        // Valid independent supervisor approval succeeds
        let token = step
            .approve_gate(
                key,
                tenant,
                mission_id,
                "bob",
                "owner",
                "enterprise",
                1700000000,
                "alice",
            )
            .expect("valid approval");

        assert_eq!(step.status, StepStatus::Pending);

        // Forged token fails
        let forged_attempt =
            step.transition_to_running(key, tenant, mission_id, Some("forged-token-value"));
        assert_eq!(forged_attempt, Err(ApprovalGateError::InvalidApprovalToken));

        // Legitimate token succeeds -> status becomes Running
        let valid_transition = step.transition_to_running(key, tenant, mission_id, Some(&token));
        assert!(valid_transition.is_ok());
        assert_eq!(step.status, StepStatus::Running);
    }
}
