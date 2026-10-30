//! The projects domain contracts — the Archxy 2 port of
//! `packages/projects/src/contracts.ts` (R04: projects + tasks + milestones,
//! the delivery half of the R04 cohort).
//!
//! ## The wire grammar
//!
//! `projectCommandSchema` is a 6-arm discriminated union tagged by
//! `operation` with dotted arm names, wrapped by
//! `projectMutationSchema = {requestId, command}`. Existing-record arms
//! carry the project reference `{projectId, projectRevision}` beside the
//! task/milestone's own `{id, expectedRevision}`; the two transition arms
//! refine `state !== 'cancelled' || !!reason` — the parse reproduces that
//! refine as a structural rule. Optional members ride default fills
//! (`milestoneId` → null, `required` → true, `dueAt` → null,
//! `reason` → absent-on-wire).

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::purchasing::PurchasePrincipal as ProjectPrincipal;
use super::purchasing::ValidationError;
use super::purchasing::REVISION_MAX;
use super::{bounded_title, valid_uuid, ErpScope};

/// The projects ledger's permission vocabulary — the schema CHECK's
/// 7-word array (`permissions <@ ARRAY[...]`).
pub const PROJECT_PERMISSIONS: &[&str] = &[
    "projects.read",
    "projects.write",
    "tasks.read",
    "tasks.write",
    "milestones.read",
    "milestones.write",
    "audit.read",
];

/// `PROJECT_STATES` — the column CHECK's vocabulary.
pub const PROJECT_STATES: &[&str] = &[
    "draft",
    "active",
    "review",
    "delivered",
    "closed",
    "cancelled",
];

/// `TASK_STATES`.
pub const TASK_STATES: &[&str] = &["todo", "doing", "done", "cancelled"];

/// The milestone state vocabulary.
pub const MILESTONE_STATES: &[&str] = &["open", "reached"];

/// `PROJECT_TRANSITIONS` — the frozen state table.
pub fn project_transition_allowed(from: &str, to: &str) -> bool {
    match from {
        "draft" => matches!(to, "active" | "cancelled"),
        "active" => matches!(to, "review" | "cancelled"),
        "review" => matches!(to, "active" | "delivered" | "cancelled"),
        "delivered" => matches!(to, "closed"),
        _ => false,
    }
}

/// `TASK_TRANSITIONS`.
pub fn task_transition_allowed(from: &str, to: &str) -> bool {
    match from {
        "todo" => matches!(to, "doing" | "done" | "cancelled"),
        "doing" => matches!(to, "todo" | "done" | "cancelled"),
        "done" => matches!(to, "doing"),
        "cancelled" => matches!(to, "todo"),
        _ => false,
    }
}

/// The per-arm permission — the repository's triple switch (`project.*`
/// = `projects.write`, `task.*` = `tasks.write`, `milestone.*` =
/// `milestones.write`) plus the POST-forbidden service rule and the
/// `projects.read` floor every arm re-grants.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectKind {
    ProjectCreate,
    ProjectTransition,
    TaskCreate,
    TaskTransition,
    MilestoneCreate,
    MilestoneComplete,
}

impl ProjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProjectCreate => "project.create",
            Self::ProjectTransition => "project.transition",
            Self::TaskCreate => "task.create",
            Self::TaskTransition => "task.transition",
            Self::MilestoneCreate => "milestone.create",
            Self::MilestoneComplete => "milestone.complete",
        }
    }

    /// The WRITE permission the arm itself asks for.
    pub fn permission(self) -> &'static str {
        match self {
            Self::ProjectCreate | Self::ProjectTransition => "projects.write",
            Self::TaskCreate | Self::TaskTransition => "tasks.write",
            Self::MilestoneCreate | Self::MilestoneComplete => "milestones.write",
        }
    }

    /// The READ floors the mutate step re-grants BEFORE the arm's effect.
    pub fn read_floors(self) -> &'static [&'static str] {
        match self {
            Self::ProjectCreate => &["projects.read"],
            Self::ProjectTransition => &["projects.read"],
            Self::TaskCreate | Self::TaskTransition => &["projects.read", "tasks.read"],
            Self::MilestoneCreate => &["projects.read"],
            Self::MilestoneComplete => &["projects.read", "tasks.read", "milestones.read"],
        }
    }

    /// The arm whose project.transition targets `delivered` must also read
    /// tasks AND milestones (the required-work count check reads both).
    pub fn delivery_floors(self, state: &str) -> &'static [&'static str] {
        if matches!(self, Self::ProjectTransition) && state == "delivered" {
            &["tasks.read", "milestones.read"]
        } else {
            &[]
        }
    }

    /// A task.create bound to a milestone reads it before binding.
    pub fn milestone_binding_floor(self, milestone_id_truth: bool) -> &'static [&'static str] {
        if matches!(self, Self::TaskCreate) && milestone_id_truth {
            &["milestones.read"]
        } else {
            &[]
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "project.create" => Self::ProjectCreate,
            "project.transition" => Self::ProjectTransition,
            "task.create" => Self::TaskCreate,
            "task.transition" => Self::TaskTransition,
            "milestone.create" => Self::MilestoneCreate,
            "milestone.complete" => Self::MilestoneComplete,
            _ => return None,
        })
    }
}

/// A resolved projects-side access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl ProjectAccess {
    /// The `grant()` rule.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// The scope rule behind `ownedProject`.
    pub fn sees(&self, owner_id: &str) -> bool {
        self.scope.covers_all() || self.owner_id == owner_id
    }
}

/// The receipt — `projectReceiptSchema`. `kind` records WHICH record
/// progressed, `revision` its own revision and `projectRevision` the
/// project's bump every non-create arm applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectReceipt {
    pub kind: String,
    pub id: String,
    #[serde(rename = "projectId")]
    pub project_id: String,
    pub revision: i64,
    #[serde(rename = "projectRevision")]
    pub project_revision: i64,
    /// Replay knowledge for the HOST decision — never serialized: the TS
    /// receipt schemas are `.strict()` and carry no such key, so the stored
    /// and served receipt documents stay exactly TS-shaped. The route's
    /// `decision` answers instead.
    #[serde(default, skip_serializing)]
    pub replayed: bool,
}

impl ProjectReceipt {
    /// The stored receipt's strict parse — the recovery replay path.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("kind"))?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .filter(|kind| ["project", "task", "milestone"].contains(kind))
            .ok_or(ValidationError("kind"))?
            .to_string();
        let uuid = |key: &'static str| -> Result<String, ValidationError> {
            object
                .get(key)
                .and_then(Value::as_str)
                .filter(|raw| valid_uuid(raw))
                .ok_or(ValidationError(key))
                .map(str::to_string)
        };
        let revision = |key: &'static str| -> Result<i64, ValidationError> {
            object
                .get(key)
                .and_then(Value::as_i64)
                .filter(|number| *number >= 1 && *number <= REVISION_MAX)
                .ok_or(ValidationError(key))
        };
        Ok(Self {
            kind,
            id: uuid("id")?,
            project_id: uuid("projectId")?,
            revision: revision("revision")?,
            project_revision: revision("projectRevision")?,
            replayed: false,
        })
    }
}

/// One typed project command — the 6 arms of `projectCommandSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "operation")]
pub enum ProjectCommand {
    #[serde(rename = "project.create")]
    ProjectCreate {
        title: String,
        #[serde(rename = "scopeSummary")]
        scope_summary: String,
        #[serde(rename = "dueAt", skip_serializing_if = "Option::is_none")]
        due_at: Option<String>,
    },
    #[serde(rename = "project.transition")]
    ProjectTransition {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "projectRevision")]
        project_revision: i64,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "task.create")]
    TaskCreate {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "projectRevision")]
        project_revision: i64,
        title: String,
        #[serde(rename = "milestoneId", skip_serializing_if = "Option::is_none")]
        milestone_id: Option<Option<String>>,
        required: bool,
        #[serde(rename = "dueAt", skip_serializing_if = "Option::is_none")]
        due_at: Option<String>,
    },
    #[serde(rename = "task.transition")]
    TaskTransition {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "projectRevision")]
        project_revision: i64,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        state: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    #[serde(rename = "milestone.create")]
    MilestoneCreate {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "projectRevision")]
        project_revision: i64,
        title: String,
        required: bool,
        #[serde(rename = "dueAt", skip_serializing_if = "Option::is_none")]
        due_at: Option<String>,
    },
    #[serde(rename = "milestone.complete")]
    MilestoneComplete {
        #[serde(rename = "projectId")]
        project_id: String,
        #[serde(rename = "projectRevision")]
        project_revision: i64,
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        evidence: String,
    },
}

impl ProjectCommand {
    /// The arm discriminator.
    pub fn kind(&self) -> ProjectKind {
        match self {
            Self::ProjectCreate { .. } => ProjectKind::ProjectCreate,
            Self::ProjectTransition { .. } => ProjectKind::ProjectTransition,
            Self::TaskCreate { .. } => ProjectKind::TaskCreate,
            Self::TaskTransition { .. } => ProjectKind::TaskTransition,
            Self::MilestoneCreate { .. } => ProjectKind::MilestoneCreate,
            Self::MilestoneComplete { .. } => ProjectKind::MilestoneComplete,
        }
    }

    /// The arm's own state when the command moves one — the delivery floors
    /// and the archive-refinements hang off this input.
    pub fn target_state(&self) -> Option<&str> {
        match self {
            Self::ProjectTransition { state, .. } | Self::TaskTransition { state, .. } => {
                Some(state.as_str())
            }
            _ => None,
        }
    }

    /// The cancel-reason refine: `state !== 'cancelled' || !!reason`.
    pub fn refine_holds(command: &Self) -> bool {
        match command.target_state() {
            Some("cancelled") => cancelled_carries_reason(command),
            _ => true,
        }
    }
}

fn cancelled_carries_reason(command: &ProjectCommand) -> bool {
    match command {
        ProjectCommand::ProjectTransition { reason, .. }
        | ProjectCommand::TaskTransition { reason, .. } => reason.is_some(),
        _ => false,
    }
}

/// The mutation envelope — `{requestId, command}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectMutation {
    #[serde(rename = "requestId")]
    pub request_id: String,
    pub command: ProjectCommand,
}

// --- Field parsers ----------------------------------------------------------

fn as_title(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    max: usize,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|raw| bounded_title(raw, max))
        .ok_or(ValidationError(key))
}

fn as_uuid_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|raw| valid_uuid(raw))
        .ok_or(ValidationError(key))
        .map(str::to_string)
}

fn as_revision_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<i64, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .filter(|number| *number >= 1 && *number <= REVISION_MAX)
        .ok_or(ValidationError(key))
}

/// `.nullable().default(null)` INSTANT: absent or null → None; present must
/// look like an offset-less iso instant (the column is timestamptz).
fn as_due_at(object: &serde_json::Map<String, Value>) -> Result<Option<String>, ValidationError> {
    match object.get("dueAt") {
        None | Some(Value::Null) => Ok(None),
        Some(value) => {
            let raw = value.as_str().ok_or(ValidationError("dueAt"))?;
            // Light structural shape: `YYYY-MM-DDT...` — the column is a
            // timestamptz; a full calendar validation is the DB's refusal.
            let looks_instant = raw.len() >= 11
                && raw.as_bytes()[4] == b'-'
                && raw.as_bytes()[7] == b'-'
                && raw.as_bytes()[10] == b'T'
                && raw.as_bytes()[..4].iter().all(|b| b.is_ascii_digit());
            if !looks_instant {
                return Err(ValidationError("dueAt"));
            }
            Ok(Some(raw.to_string()))
        }
    }
}

fn as_state(
    object: &serde_json::Map<String, Value>,
    vocabulary: &[&str],
) -> Result<String, ValidationError> {
    let raw = object
        .get("state")
        .and_then(Value::as_str)
        .ok_or(ValidationError("state"))?;
    if !vocabulary.contains(&raw) {
        return Err(ValidationError("state"));
    }
    Ok(raw.to_string())
}

/// The optional summary/reason cell — `summary.optional()`.
fn as_reason(object: &serde_json::Map<String, Value>) -> Result<Option<String>, ValidationError> {
    match object.get("reason") {
        None => Ok(None),
        Some(value) => {
            let raw = value.as_str().ok_or(ValidationError("reason"))?;
            bounded_title(raw, 2000)
                .map(Some)
                .ok_or(ValidationError("reason"))
        }
    }
}

/// The strict parse of the command union — arm members exactly, including
/// the cancellation refine both transition arms carry.
pub fn parse_command(raw: &Value) -> Result<ProjectCommand, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("operation"))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ValidationError("operation"))?;
    let arm = ProjectKind::parse(operation).ok_or(ValidationError("operation"))?;
    let allowed: &[&str] = match arm {
        ProjectKind::ProjectCreate => &["operation", "title", "scopeSummary", "dueAt"],
        ProjectKind::ProjectTransition => &[
            "operation",
            "projectId",
            "projectRevision",
            "state",
            "reason",
        ],
        ProjectKind::TaskCreate => &[
            "operation",
            "projectId",
            "projectRevision",
            "title",
            "milestoneId",
            "required",
            "dueAt",
        ],
        ProjectKind::TaskTransition => &[
            "operation",
            "projectId",
            "projectRevision",
            "id",
            "expectedRevision",
            "state",
            "reason",
        ],
        ProjectKind::MilestoneCreate => &[
            "operation",
            "projectId",
            "projectRevision",
            "title",
            "required",
            "dueAt",
        ],
        ProjectKind::MilestoneComplete => &[
            "operation",
            "projectId",
            "projectRevision",
            "id",
            "expectedRevision",
            "evidence",
        ],
    };
    if let Some(_key) = object.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Err(ValidationError("operation"));
    }
    let command = match arm {
        ProjectKind::ProjectCreate => ProjectCommand::ProjectCreate {
            title: as_title(object, "title", 160)?,
            scope_summary: as_title(object, "scopeSummary", 2000)?,
            due_at: as_due_at(object)?,
        },
        ProjectKind::ProjectTransition => {
            let state = as_state(object, PROJECT_STATES)?;
            let reason = as_reason(object)?;
            let command = ProjectCommand::ProjectTransition {
                project_id: as_uuid_member(object, "projectId")?,
                project_revision: as_revision_member(object, "projectRevision")?,
                state,
                reason,
            };
            if !ProjectCommand::refine_holds(&command) {
                return Err(ValidationError("reason"));
            }
            command
        }
        ProjectKind::TaskCreate => {
            let milestone_id = match object.get("milestoneId") {
                None | Some(Value::Null) => None,
                Some(value) => Some(Some(
                    value
                        .as_str()
                        .filter(|raw| valid_uuid(raw))
                        .ok_or(ValidationError("milestoneId"))
                        .map(str::to_string)?,
                )),
            };
            let required = match object.get("required") {
                None => true,
                Some(value) => value.as_bool().ok_or(ValidationError("required"))?,
            };
            ProjectCommand::TaskCreate {
                project_id: as_uuid_member(object, "projectId")?,
                project_revision: as_revision_member(object, "projectRevision")?,
                title: as_title(object, "title", 160)?,
                milestone_id,
                required,
                due_at: as_due_at(object)?,
            }
        }
        ProjectKind::TaskTransition => {
            let state = as_state(object, TASK_STATES)?;
            let reason = as_reason(object)?;
            let command = ProjectCommand::TaskTransition {
                project_id: as_uuid_member(object, "projectId")?,
                project_revision: as_revision_member(object, "projectRevision")?,
                id: as_uuid_member(object, "id")?,
                expected_revision: as_revision_member(object, "expectedRevision")?,
                state,
                reason,
            };
            if !ProjectCommand::refine_holds(&command) {
                return Err(ValidationError("reason"));
            }
            command
        }
        ProjectKind::MilestoneCreate => {
            let required = match object.get("required") {
                None => true,
                Some(value) => value.as_bool().ok_or(ValidationError("required"))?,
            };
            ProjectCommand::MilestoneCreate {
                project_id: as_uuid_member(object, "projectId")?,
                project_revision: as_revision_member(object, "projectRevision")?,
                title: as_title(object, "title", 160)?,
                required,
                due_at: as_due_at(object)?,
            }
        }
        ProjectKind::MilestoneComplete => ProjectCommand::MilestoneComplete {
            project_id: as_uuid_member(object, "projectId")?,
            project_revision: as_revision_member(object, "projectRevision")?,
            id: as_uuid_member(object, "id")?,
            expected_revision: as_revision_member(object, "expectedRevision")?,
            evidence: as_title(object, "evidence", 2000)?,
        },
    };
    Ok(command)
}

/// The strict parse of the envelope — `{requestId,command}`.
pub fn parse_mutation(raw: &Value) -> Result<ProjectMutation, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("requestId"))?;
    let request_id =
        as_uuid_member(object, "requestId").map_err(|_| ValidationError("requestId"))?;
    if let Some(_key) = object
        .keys()
        .find(|key| !["requestId", "command"].contains(&key.as_str()))
    {
        return Err(ValidationError("requestId"));
    }
    let command = object.get("command").ok_or(ValidationError("operation"))?;
    let command = parse_command(command)?;
    Ok(ProjectMutation {
        request_id,
        command,
    })
}

/// Builds the replayed receipt: the stored one with `replayed` flipped. A
/// recovery IS a replay by construction — the same class the purchasing and
/// foodservice ledgers serve recovery with.
pub fn replayed_receipt(receipt: &ProjectReceipt) -> ProjectReceipt {
    let mut replayed = receipt.clone();
    replayed.replayed = true;
    replayed
}

/// The command-status union — `{status, requestId?, receipt?}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum ProjectCommandStatus {
    #[serde(rename = "committed")]
    Committed {
        #[serde(rename = "requestId")]
        request_id: String,
        receipt: ProjectReceipt,
    },
    #[serde(rename = "not_committed")]
    NotCommitted {
        #[serde(rename = "requestId")]
        request_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn project_id() -> &'static str {
        "112e5843-c01b-4a3c-a68a-5a5f7a1d1234"
    }

    #[test]
    fn a_project_create_parses_with_its_default_fills() {
        let doc = json!({
            "operation": "project.create",
            "title": "Portal",
            "scopeSummary": "Ruang lingkup pertama"
        });
        match parse_command(&doc).expect("parses") {
            ProjectCommand::ProjectCreate { due_at, .. } => assert_eq!(due_at, None),
            other => panic!("wrong arm {other:?}"),
        }
        let with_due = json!({
            "operation": "task.create",
            "projectId": project_id(),
            "projectRevision": 2,
            "title": "Tugas awal",
            "milestoneId": project_id(),
            "required": false
        });
        match parse_command(&with_due).expect("parses") {
            ProjectCommand::TaskCreate {
                milestone_id,
                required,
                due_at,
                ..
            } => {
                assert_eq!(milestone_id, Some(Some(project_id().to_string())));
                assert!(!required);
                assert_eq!(due_at, None);
            }
            other => panic!("wrong arm {other:?}"),
        }
    }

    #[test]
    fn a_cancellation_without_reason_is_structurally_refused() {
        let critic = json!({
            "operation": "project.transition",
            "projectId": project_id(),
            "projectRevision": 1,
            "state": "cancelled"
        });
        assert!(parse_command(&critic).is_err(), "refine refuses it");
        let with_reason = json!({
            "operation": "project.transition",
            "projectId": project_id(),
            "projectRevision": 1,
            "state": "cancelled",
            "reason": "Funding selesai"
        });
        assert!(parse_command(&with_reason).is_ok());
        let bad_state = json!({
            "operation": "project.transition",
            "projectId": project_id(),
            "projectRevision": 1,
            "state": "shelved"
        });
        assert!(parse_command(&bad_state).is_err());
    }

    #[test]
    fn an_unknown_member_or_arm_is_refused() {
        let filler = json!({
            "operation": "milestone.complete",
            "projectId": project_id(),
            "projectRevision": 3,
            "id": project_id(),
            "expectedRevision": 1,
            "evidence": "Scope terekam",
            "extra": true
        });
        assert!(parse_command(&filler).is_err());
        let unknown_arm = json!({
            "operation": "milestone.delete",
            "projectId": project_id(),
            "projectRevision": 3
        });
        assert!(parse_command(&unknown_arm).is_err());
    }

    #[test]
    fn the_state_tables_reproduce_the_frozen_transitions() {
        assert!(project_transition_allowed("review", "delivered"));
        assert!(!project_transition_allowed("closed", "draft"));
        assert!(!project_transition_allowed("draft", "review"));
        assert!(task_transition_allowed("doing", "done"));
        assert!(!task_transition_allowed("done", "done"));
        assert!(task_transition_allowed("cancelled", "todo"));
    }

    #[test]
    fn the_permission_rule_is_the_baseline_switch() {
        assert_eq!(ProjectKind::ProjectCreate.permission(), "projects.write");
        assert_eq!(ProjectKind::TaskTransition.permission(), "tasks.write");
        assert_eq!(
            ProjectKind::MilestoneComplete.permission(),
            "milestones.write"
        );
        assert_eq!(
            ProjectKind::MilestoneComplete.read_floors(),
            &["projects.read", "tasks.read", "milestones.read"][..]
        );
        assert!(ProjectKind::TaskCreate
            .milestone_binding_floor(false)
            .is_empty());
        assert_eq!(
            ProjectKind::TaskCreate.milestone_binding_floor(true),
            &["milestones.read"][..]
        );
        assert_eq!(
            ProjectKind::ProjectTransition.delivery_floors("delivered"),
            &["tasks.read", "milestones.read"][..]
        );
        assert!(ProjectKind::ProjectTransition
            .delivery_floors("closed")
            .is_empty());
    }

    #[test]
    fn the_envelope_and_receipt_round_trip() {
        let doc = json!({
            "requestId": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "command": {
                "operation": "milestone.create",
                "projectId": project_id(),
                "projectRevision": 2,
                "title": "Bawah"
            }
        });
        let mutation = parse_mutation(&doc).expect("parses");
        assert_eq!(mutation.command.kind(), ProjectKind::MilestoneCreate);
        let receipt = json!({
            "kind": "milestone",
            "id": "212e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "projectId": project_id(),
            "revision": 1,
            "projectRevision": 3
        });
        let parsed = ProjectReceipt::parse(&receipt).expect("parses");
        assert_eq!(parsed.revision, 1);
        assert_eq!(serde_json::to_value(&parsed).expect("serialize"), receipt);
        assert!(ProjectReceipt::parse(&Value::Bool(true)).is_err());
    }
}
