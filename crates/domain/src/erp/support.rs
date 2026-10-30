//! The support domain — the Archxy 2 port of `packages/support/src/*`:
//! the case lifecycle (open → in_progress → waiting → resolved → closed,
//! with cancellation and a clean reopen cycle), assignment, priority and
//! target edits, and the immutable notes/transitions/audit history.
//!
//! ## What this port keeps from the baseline
//!
//! All eleven command arms (`case.create`, `case.start`, `case.wait`,
//! `case.resume`, `case.close`, `case.cancel`, `case.resolve`, `case.reopen`,
//! `case.assign`, `case.set-priority`, `case.set-target`, `note.add` — the
//! last rides the same revision chain), the strict object grammar the TS
//! `.strict()` schemas enforce, the closed vocabularies (states, priorities),
//! the receipt (`requestId`, `operation`, `id`, `revision`, `replayed`) and
//! the `unknown`/`committed` command-status union.
//!
//! ## The rules the pure layer owns
//!
//! - [`authorize`] — `cases.read` on every arm; `notes.write` for a note and
//!   `cases.write` for the rest; `case.assign` additionally needs
//!   `cases.assign` AND the tenant-wide scope (the TS `authorize`).
//! - [`state_transition`] — the closed state table; a transition the table
//!   does not name is a conflict, not an invention.
//! - [`mutable_lifecycle`] — assignment/priority/target edits refuse on a
//!   closed or cancelled case.
//! - [`reopen_resets`] — a reopen starts a clean cycle: the resolution and
//!   its stamps clear.
//!
//! ## The read projections
//!
//! `cases` is a keyset page with `evaluatedAt` (the clock the overdue flag
//! is measured against), `detail` carries the three bounded histories (100
//! rows each, `hasMore` flags), and `assignees` is the human-reader picker.
//! The adapter owns the SQL; this module owns the wire shapes and the
//! grammar.
//!
//! The service layer's HUMAN-OPERATOR rule lives in the host, exactly where
//! the TS service applies it: every mutation, the assignee picker and the
//! command status refuse a service principal; `access`/`cases`/`detail`
//! admit a service principal under the intersection rule.

use serde::Serialize;
use serde_json::Value;

use super::purchasing::ValidationError;
use super::scheduling::parse_instant;
use super::{bounded_title, trimmed, valid_uuid};

/// The permission vocabulary migration 001's CHECK allows.
pub const SUPPORT_PERMISSIONS: &[&str] = &[
    "cases.read",
    "cases.write",
    "cases.assign",
    "notes.read",
    "notes.write",
    "audit.read",
];

/// The case states (the TS `SUPPORT_STATES`).
pub const STATE_OPEN: &str = "open";
pub const STATE_IN_PROGRESS: &str = "in_progress";
pub const STATE_WAITING: &str = "waiting";
pub const STATE_RESOLVED: &str = "resolved";
pub const STATE_CLOSED: &str = "closed";
pub const STATE_CANCELLED: &str = "cancelled";

/// The priority vocabulary.
pub const PRIORITY_LOW: &str = "low";
pub const PRIORITY_NORMAL: &str = "normal";
pub const PRIORITY_HIGH: &str = "high";

/// Field bounds (the TS schemas' own numbers).
pub const TITLE_MAX: usize = 160;
pub const BODY_MAX: usize = 4000;
pub const REASON_MAX: usize = 2000;
pub const PAGE_MAX: i64 = 100;
pub const PAGE_DEFAULT: i64 = 50;
pub const HISTORY_MAX: usize = 100;

// --- Vocabularies -------------------------------------------------------------

/// The case state — the TS enum, parsed without invention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportState {
    Open,
    InProgress,
    Waiting,
    Resolved,
    Closed,
    Cancelled,
}

impl SupportState {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATE_OPEN => Some(Self::Open),
            STATE_IN_PROGRESS => Some(Self::InProgress),
            STATE_WAITING => Some(Self::Waiting),
            STATE_RESOLVED => Some(Self::Resolved),
            STATE_CLOSED => Some(Self::Closed),
            STATE_CANCELLED => Some(Self::Cancelled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => STATE_OPEN,
            Self::InProgress => STATE_IN_PROGRESS,
            Self::Waiting => STATE_WAITING,
            Self::Resolved => STATE_RESOLVED,
            Self::Closed => STATE_CLOSED,
            Self::Cancelled => STATE_CANCELLED,
        }
    }

    /// The states the worklist's overdue flag measures against.
    pub fn is_active(self) -> bool {
        matches!(self, Self::Open | Self::InProgress | Self::Waiting)
    }
}

/// The priority vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupportPriority {
    Low,
    Normal,
    High,
}

impl SupportPriority {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            PRIORITY_LOW => Some(Self::Low),
            PRIORITY_NORMAL => Some(Self::Normal),
            PRIORITY_HIGH => Some(Self::High),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => PRIORITY_LOW,
            Self::Normal => PRIORITY_NORMAL,
            Self::High => PRIORITY_HIGH,
        }
    }
}

// --- The command grammar ------------------------------------------------------

/// One command arm, typed. The dotted operation spelling is the TS
/// discriminated union's tag.
#[derive(Debug, Clone, PartialEq)]
pub enum SupportCommand {
    Create {
        requester_name: String,
        subject: String,
        description: String,
        priority: String,
        due_at: Option<String>,
    },
    Start {
        id: String,
        expected_revision: i64,
        reason: String,
    },
    Wait {
        id: String,
        expected_revision: i64,
        reason: String,
    },
    Resume {
        id: String,
        expected_revision: i64,
        reason: String,
    },
    Close {
        id: String,
        expected_revision: i64,
        reason: String,
    },
    Cancel {
        id: String,
        expected_revision: i64,
        reason: String,
    },
    Resolve {
        id: String,
        expected_revision: i64,
        resolution: String,
    },
    Reopen {
        id: String,
        expected_revision: i64,
        reason: String,
        due_at: Option<String>,
    },
    Assign {
        id: String,
        expected_revision: i64,
        assignee_id: Option<String>,
        reason: String,
    },
    SetPriority {
        id: String,
        expected_revision: i64,
        priority: String,
        reason: String,
    },
    SetTarget {
        id: String,
        expected_revision: i64,
        due_at: Option<String>,
        reason: String,
    },
    NoteAdd {
        id: String,
        expected_revision: i64,
        body: String,
    },
}

impl SupportCommand {
    pub fn operation(&self) -> &'static str {
        match self {
            Self::Create { .. } => "case.create",
            Self::Start { .. } => "case.start",
            Self::Wait { .. } => "case.wait",
            Self::Resume { .. } => "case.resume",
            Self::Close { .. } => "case.close",
            Self::Cancel { .. } => "case.cancel",
            Self::Resolve { .. } => "case.resolve",
            Self::Reopen { .. } => "case.reopen",
            Self::Assign { .. } => "case.assign",
            Self::SetPriority { .. } => "case.set-priority",
            Self::SetTarget { .. } => "case.set-target",
            Self::NoteAdd { .. } => "note.add",
        }
    }

    /// The typed revision check every arm but create carries.
    pub fn target(&self) -> Option<(&str, i64)> {
        match self {
            Self::Create { .. } => None,
            Self::Start {
                id,
                expected_revision,
                ..
            }
            | Self::Wait {
                id,
                expected_revision,
                ..
            }
            | Self::Resume {
                id,
                expected_revision,
                ..
            }
            | Self::Close {
                id,
                expected_revision,
                ..
            }
            | Self::Cancel {
                id,
                expected_revision,
                ..
            }
            | Self::Resolve {
                id,
                expected_revision,
                ..
            }
            | Self::Reopen {
                id,
                expected_revision,
                ..
            }
            | Self::Assign {
                id,
                expected_revision,
                ..
            }
            | Self::SetPriority {
                id,
                expected_revision,
                ..
            }
            | Self::SetTarget {
                id,
                expected_revision,
                ..
            }
            | Self::NoteAdd {
                id,
                expected_revision,
                ..
            } => Some((id, *expected_revision)),
        }
    }

    /// The reason a transition/audit row records (`None` on create, resolve
    /// and note.add, exactly like the TS `'reason' in c` checks).
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::Start { reason, .. }
            | Self::Wait { reason, .. }
            | Self::Resume { reason, .. }
            | Self::Close { reason, .. }
            | Self::Cancel { reason, .. }
            | Self::Reopen { reason, .. }
            | Self::Assign { reason, .. }
            | Self::SetPriority { reason, .. }
            | Self::SetTarget { reason, .. } => Some(reason),
            Self::Create { .. } | Self::Resolve { .. } | Self::NoteAdd { .. } => None,
        }
    }
}

/// The mutation document — the TS `supportMutationSchema`.
#[derive(Debug, Clone, PartialEq)]
pub struct SupportMutation {
    pub request_id: String,
    pub command: SupportCommand,
}

/// The typed command rendered back to the schema's document — the shape the
/// idempotency fingerprint covers (defaults included, exactly as the TS
/// parse produced them).
pub fn command_document(command: &SupportCommand) -> Value {
    let mut document = serde_json::Map::new();
    document.insert(
        "operation".to_string(),
        Value::String(command.operation().to_string()),
    );
    match command {
        SupportCommand::Create {
            requester_name,
            subject,
            description,
            priority,
            due_at,
        } => {
            document.insert(
                "requesterName".into(),
                Value::String(requester_name.clone()),
            );
            document.insert("subject".into(), Value::String(subject.clone()));
            document.insert("description".into(), Value::String(description.clone()));
            document.insert("priority".into(), Value::String(priority.clone()));
            document.insert(
                "dueAt".into(),
                due_at.clone().map(Value::String).unwrap_or(Value::Null),
            );
        }
        SupportCommand::Start {
            id,
            expected_revision,
            reason,
        }
        | SupportCommand::Wait {
            id,
            expected_revision,
            reason,
        }
        | SupportCommand::Resume {
            id,
            expected_revision,
            reason,
        }
        | SupportCommand::Close {
            id,
            expected_revision,
            reason,
        }
        | SupportCommand::Cancel {
            id,
            expected_revision,
            reason,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert("reason".into(), Value::String(reason.clone()));
        }
        SupportCommand::Resolve {
            id,
            expected_revision,
            resolution,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert("resolution".into(), Value::String(resolution.clone()));
        }
        SupportCommand::Reopen {
            id,
            expected_revision,
            reason,
            due_at,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert("reason".into(), Value::String(reason.clone()));
            document.insert(
                "dueAt".into(),
                due_at.clone().map(Value::String).unwrap_or(Value::Null),
            );
        }
        SupportCommand::Assign {
            id,
            expected_revision,
            assignee_id,
            reason,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert(
                "assigneeId".into(),
                assignee_id
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            document.insert("reason".into(), Value::String(reason.clone()));
        }
        SupportCommand::SetPriority {
            id,
            expected_revision,
            priority,
            reason,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert("priority".into(), Value::String(priority.clone()));
            document.insert("reason".into(), Value::String(reason.clone()));
        }
        SupportCommand::SetTarget {
            id,
            expected_revision,
            due_at,
            reason,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert(
                "dueAt".into(),
                due_at.clone().map(Value::String).unwrap_or(Value::Null),
            );
            document.insert("reason".into(), Value::String(reason.clone()));
        }
        SupportCommand::NoteAdd {
            id,
            expected_revision,
            body,
        } => {
            document.insert("id".into(), Value::String(id.clone()));
            document.insert(
                "expectedRevision".into(),
                serde_json::json!(expected_revision),
            );
            document.insert("body".into(), Value::String(body.clone()));
        }
    }
    Value::Object(document)
}

/// Parse one mutation. The strict walk refuses an unknown/extra member the
/// TS `.strict()` object would refuse, and the closed vocabularies refuse
/// any target the schemas do not name.
pub fn parse_mutation(raw: &Value) -> Result<SupportMutation, ValidationError> {
    let outer = raw.as_object().ok_or(ValidationError("mutation"))?;
    if outer.len() != 2 || !outer.contains_key("requestId") || !outer.contains_key("command") {
        return Err(ValidationError("mutation"));
    }
    let request_id = outer
        .get("requestId")
        .and_then(Value::as_str)
        .filter(|value| valid_uuid(value))
        .ok_or(ValidationError("requestId"))?;
    let command = outer
        .get("command")
        .and_then(Value::as_object)
        .ok_or(ValidationError("command"))?;
    let operation = command
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ValidationError("operation"))?;

    let allowed: &[&str] = match operation {
        "case.create" => &[
            "operation",
            "requesterName",
            "subject",
            "description",
            "priority",
            "dueAt",
        ],
        "case.start" | "case.wait" | "case.resume" | "case.close" | "case.cancel" => {
            &["operation", "id", "expectedRevision", "reason"]
        }
        "case.resolve" => &["operation", "id", "expectedRevision", "resolution"],
        "case.reopen" => &["operation", "id", "expectedRevision", "reason", "dueAt"],
        "case.assign" => &[
            "operation",
            "id",
            "expectedRevision",
            "assigneeId",
            "reason",
        ],
        "case.set-priority" => &["operation", "id", "expectedRevision", "priority", "reason"],
        "case.set-target" => &["operation", "id", "expectedRevision", "dueAt", "reason"],
        "note.add" => &["operation", "id", "expectedRevision", "body"],
        _ => return Err(ValidationError("operation")),
    };
    if command.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ValidationError("member"));
    }

    let target = || -> Result<(String, i64), ValidationError> {
        let id = command
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| valid_uuid(value))
            .ok_or(ValidationError("id"))?;
        let revision = command
            .get("expectedRevision")
            .and_then(Value::as_i64)
            .filter(|value| (1..=2_147_483_647).contains(value))
            .ok_or(ValidationError("expectedRevision"))?;
        Ok((id.to_string(), revision))
    };
    let text = |key: &'static str, max: usize| -> Result<String, ValidationError> {
        let value = command
            .get(key)
            .and_then(Value::as_str)
            .ok_or(ValidationError(key))?;
        bounded_title(value, max).ok_or(ValidationError(key))
    };
    let instant = |key: &'static str| -> Result<Option<String>, ValidationError> {
        match command.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(value)) => {
                let parsed = parse_instant(value).ok_or(ValidationError(key))?;
                if parsed.epoch_millis() >= 4_102_444_800_000 {
                    return Err(ValidationError(key));
                }
                Ok(Some(parsed.canonical().to_string()))
            }
            _ => Err(ValidationError(key)),
        }
    };
    let priority_of = |key: &'static str| -> Result<String, ValidationError> {
        let value = command
            .get(key)
            .and_then(Value::as_str)
            .ok_or(ValidationError(key))?;
        SupportPriority::parse(value)
            .map(|priority| priority.as_str().to_string())
            .ok_or(ValidationError(key))
    };

    let parsed = match operation {
        "case.create" => {
            let requester_name = text("requesterName", TITLE_MAX)?;
            let subject = text("subject", TITLE_MAX)?;
            let description = text("description", BODY_MAX)?;
            let priority = match command.get("priority") {
                None => PRIORITY_NORMAL.to_string(),
                Some(_) => priority_of("priority")?,
            };
            SupportCommand::Create {
                requester_name,
                subject,
                description,
                priority,
                due_at: instant("dueAt")?,
            }
        }
        "case.start" | "case.wait" | "case.resume" | "case.close" | "case.cancel" => {
            let (id, expected_revision) = target()?;
            let reason = text("reason", REASON_MAX)?;
            match operation {
                "case.start" => SupportCommand::Start {
                    id,
                    expected_revision,
                    reason,
                },
                "case.wait" => SupportCommand::Wait {
                    id,
                    expected_revision,
                    reason,
                },
                "case.resume" => SupportCommand::Resume {
                    id,
                    expected_revision,
                    reason,
                },
                "case.close" => SupportCommand::Close {
                    id,
                    expected_revision,
                    reason,
                },
                _ => SupportCommand::Cancel {
                    id,
                    expected_revision,
                    reason,
                },
            }
        }
        "case.resolve" => {
            let (id, expected_revision) = target()?;
            SupportCommand::Resolve {
                id,
                expected_revision,
                resolution: text("resolution", BODY_MAX)?,
            }
        }
        "case.reopen" => {
            let (id, expected_revision) = target()?;
            SupportCommand::Reopen {
                id,
                expected_revision,
                reason: text("reason", REASON_MAX)?,
                due_at: instant("dueAt")?,
            }
        }
        "case.assign" => {
            let (id, expected_revision) = target()?;
            let assignee_id = match command.get("assigneeId") {
                None | Some(Value::Null) => None,
                Some(Value::String(value))
                    if !value.is_empty()
                        && value.len() <= 100
                        && value
                            .chars()
                            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') =>
                {
                    Some(value.clone())
                }
                Some(Value::String(_)) => return Err(ValidationError("assigneeId")),
                _ => return Err(ValidationError("assigneeId")),
            };
            SupportCommand::Assign {
                id,
                expected_revision,
                assignee_id,
                reason: text("reason", REASON_MAX)?,
            }
        }
        "case.set-priority" => {
            let (id, expected_revision) = target()?;
            SupportCommand::SetPriority {
                id,
                expected_revision,
                priority: priority_of("priority")?,
                reason: text("reason", REASON_MAX)?,
            }
        }
        "case.set-target" => {
            let (id, expected_revision) = target()?;
            SupportCommand::SetTarget {
                id,
                expected_revision,
                due_at: instant("dueAt")?,
                reason: text("reason", REASON_MAX)?,
            }
        }
        _ => {
            let (id, expected_revision) = target()?;
            SupportCommand::NoteAdd {
                id,
                expected_revision,
                body: text("body", BODY_MAX)?,
            }
        }
    };
    Ok(SupportMutation {
        request_id: request_id.to_string(),
        command: parsed,
    })
}

// --- The policy ---------------------------------------------------------------

/// The caller's effective grant — the TS `supportAccessSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: String,
}

impl SupportAccess {
    pub fn holds(&self, permission: &str) -> bool {
        self.permissions.iter().any(|held| held == permission)
    }

    pub fn sees_all(&self) -> bool {
        self.scope == "all"
    }
}

/// The grant ladder — the TS `authorize()`.
pub fn authorize(access: &SupportAccess, command: &SupportCommand) -> Result<(), ErpError> {
    if !access.holds("cases.read") {
        return Err(ErpError::Forbidden);
    }
    let write = if matches!(command, SupportCommand::NoteAdd { .. }) {
        "notes.write"
    } else {
        "cases.write"
    };
    if !access.holds(write) {
        return Err(ErpError::Forbidden);
    }
    if matches!(command, SupportCommand::Assign { .. })
        && (!access.holds("cases.assign") || !access.sees_all())
    {
        return Err(ErpError::Forbidden);
    }
    Ok(())
}

/// The state table for the five pure transitions — `None` for a state the
/// arm cannot leave, which the adapter answers as a conflict.
pub fn state_transition(from: SupportState, operation: &str) -> Option<SupportState> {
    match (from, operation) {
        (SupportState::Open, "case.start") => Some(SupportState::InProgress),
        (SupportState::InProgress, "case.wait") => Some(SupportState::Waiting),
        (SupportState::Waiting, "case.resume") => Some(SupportState::InProgress),
        (SupportState::Resolved, "case.close") => Some(SupportState::Closed),
        (SupportState::Open | SupportState::InProgress | SupportState::Waiting, "case.cancel") => {
            Some(SupportState::Cancelled)
        }
        _ => None,
    }
}

/// Whether assignment/priority/target edits are admissible (the TS refuses
/// them on a closed or cancelled case).
pub fn mutable_lifecycle(state: SupportState) -> bool {
    !matches!(state, SupportState::Closed | SupportState::Cancelled)
}

/// Whether the resolve arm may run (`in_progress`/`waiting`).
pub fn resolvable(state: SupportState) -> bool {
    matches!(state, SupportState::InProgress | SupportState::Waiting)
}

/// Whether the reopen arm may run (`resolved`/`closed`).
pub fn reopenable(state: SupportState) -> bool {
    matches!(state, SupportState::Resolved | SupportState::Closed)
}

/// A reopen starts a clean cycle: resolution and its stamps clear.
pub fn reopen_resets() -> (Option<String>, Option<String>, Option<String>) {
    (None, None, None)
}

// --- The receipt and status ---------------------------------------------------

/// The receipt — the TS `supportReceiptSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportReceipt {
    pub request_id: String,
    pub operation: String,
    pub id: String,
    pub revision: i64,
    pub replayed: bool,
}

impl SupportReceipt {
    /// Parse a stored receipt document (the command-status read's job).
    pub fn parse(raw: &Value, replay: bool) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("receipt"))?;
        let request_id = object
            .get("requestId")
            .and_then(Value::as_str)
            .filter(|value| valid_uuid(value))
            .ok_or(ValidationError("requestId"))?;
        let operation = object
            .get("operation")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 100)
            .ok_or(ValidationError("operation"))?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|value| valid_uuid(value))
            .ok_or(ValidationError("id"))?;
        let revision = object
            .get("revision")
            .and_then(Value::as_i64)
            .filter(|value| (1..=2_147_483_647).contains(value))
            .ok_or(ValidationError("revision"))?;
        Ok(Self {
            request_id: request_id.to_string(),
            operation: operation.to_string(),
            id: id.to_string(),
            revision,
            replayed: replay,
        })
    }
}

/// The command-status document — the TS `supportCommandStatusSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SupportCommandStatus {
    Unknown,
    Committed { receipt: Box<SupportReceipt> },
}

// --- The read projections -----------------------------------------------------

/// One case row — the TS `caseSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportCase {
    pub id: String,
    pub tenant_id: String,
    pub owner_id: String,
    pub assignee_id: Option<String>,
    pub requester_name: String,
    pub subject: String,
    pub description: String,
    pub priority: String,
    pub state: String,
    pub due_at: Option<String>,
    pub overdue: bool,
    pub cycle: i64,
    pub resolution: Option<String>,
    pub resolved_at: Option<String>,
    pub closed_at: Option<String>,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// One note row — the TS `supportNoteSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportNote {
    pub id: String,
    pub case_id: String,
    pub author_id: String,
    pub body: String,
    pub request_id: String,
    pub occurred_at: String,
}

/// One transition — the TS `supportTransitionSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportTransition {
    pub request_id: String,
    pub actor_id: String,
    pub operation: String,
    pub previous_assignee_id: Option<String>,
    pub assignee_id: Option<String>,
    pub previous_priority: Option<String>,
    pub priority: String,
    pub previous_due_at: Option<String>,
    pub due_at: Option<String>,
    pub from: Option<String>,
    pub to: String,
    pub cycle: i64,
    pub previous_resolution: Option<String>,
    pub resolution: Option<String>,
    pub reason: Option<String>,
    pub occurred_at: String,
}

/// One audit row — the TS `supportAuditSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportAudit {
    pub request_id: String,
    pub actor_id: String,
    pub operation: String,
    pub record_id: String,
    pub revision: i64,
    pub reason: Option<String>,
    pub occurred_at: String,
}

/// The case detail — record + the three bounded histories.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaseDetail {
    pub record: SupportCase,
    pub notes: Option<Vec<SupportNote>>,
    pub notes_has_more: bool,
    pub transitions: Option<Vec<SupportTransition>>,
    pub transitions_has_more: bool,
    pub audit: Option<Vec<SupportAudit>>,
    pub audit_has_more: bool,
}

/// The case page — `evaluatedAt` is the clock the overdue flags used.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CasePage {
    pub items: Vec<SupportCase>,
    pub next: Option<String>,
    pub evaluated_at: String,
}

/// The assignee picker row.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Assignee {
    pub id: String,
    pub display_name: String,
}

/// The assignee page.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssigneePage {
    pub items: Vec<Assignee>,
    pub next: Option<String>,
}

/// The case page query — the TS `casePageSchema`.
#[derive(Debug, Clone, PartialEq)]
pub struct CasePageQuery {
    pub limit: i64,
    pub after: Option<String>,
    pub state: Option<String>,
    pub priority: Option<String>,
    pub overdue: Option<bool>,
}

/// The assignee page query.
#[derive(Debug, Clone, PartialEq)]
pub struct AssigneePageQuery {
    pub limit: i64,
    pub after: Option<String>,
}

/// Parse the case-page query with the schema's own bounds and closed
/// vocabularies. The host maps a refusal to its invalid-submission class.
pub fn parse_case_page(raw: &Value) -> Result<CasePageQuery, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("query"))?;
    let allowed = ["limit", "after", "state", "priority", "overdue"];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ValidationError("query"));
    }
    let limit = match object.get("limit") {
        None => PAGE_DEFAULT,
        Some(Value::Number(number)) => number
            .as_i64()
            .filter(|value| (1..=PAGE_MAX).contains(value))
            .ok_or(ValidationError("limit"))?,
        _ => return Err(ValidationError("limit")),
    };
    let after = match object.get("after") {
        None => None,
        Some(Value::String(value)) if valid_uuid(value) => Some(value.clone()),
        _ => return Err(ValidationError("after")),
    };
    let state = match object.get("state") {
        None => None,
        Some(Value::String(value)) if SupportState::parse(value).is_some() => Some(value.clone()),
        _ => return Err(ValidationError("state")),
    };
    let priority = match object.get("priority") {
        None => None,
        Some(Value::String(value)) if SupportPriority::parse(value).is_some() => {
            Some(value.clone())
        }
        _ => return Err(ValidationError("priority")),
    };
    let overdue = match object.get("overdue") {
        None => None,
        Some(Value::Bool(value)) => Some(*value),
        _ => return Err(ValidationError("overdue")),
    };
    Ok(CasePageQuery {
        limit,
        after,
        state,
        priority,
        overdue,
    })
}

/// Parse the assignee-page query.
pub fn parse_assignee_page(raw: &Value) -> Result<AssigneePageQuery, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("query"))?;
    let allowed = ["limit", "after"];
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err(ValidationError("query"));
    }
    let limit = match object.get("limit") {
        None => PAGE_DEFAULT,
        Some(Value::Number(number)) => number
            .as_i64()
            .filter(|value| (1..=PAGE_MAX).contains(value))
            .ok_or(ValidationError("limit"))?,
        _ => return Err(ValidationError("limit")),
    };
    let after = match object.get("after") {
        None => None,
        Some(Value::String(value)) => {
            if value.is_empty() || value.len() > 100 {
                return Err(ValidationError("after"));
            }
            Some(value.clone())
        }
        _ => return Err(ValidationError("after")),
    };
    Ok(AssigneePageQuery { limit, after })
}

// Re-export the shared refusal so the adapter imports one name.
pub use super::ErpError;

/// A lowercase identifier with the TS `supportIdSchema` shape — principals
/// and tenants share the pattern `^[a-z0-9]+(?:-[a-z0-9]+)*$`.
pub fn valid_support_id(value: &str) -> bool {
    if value.is_empty() || value.len() > 100 {
        return false;
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return false;
    }
    let mut previous_dash = false;
    for &byte in bytes {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' => previous_dash = false,
            b'-' if !previous_dash => previous_dash = true,
            _ => return false,
        }
    }
    !previous_dash
}

/// Trim a display name for the assignee projection (the TS answer keeps
/// the stored value, empty display names fall back to the principal id in
/// SQL; this helper is only the wire-side trim).
pub fn display_name(value: &str) -> &str {
    trimmed(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn access(permissions: &[&str], scope: &str) -> SupportAccess {
        SupportAccess {
            permissions: permissions.iter().map(|p| p.to_string()).collect(),
            owner_id: "acme".to_string(),
            scope: scope.to_string(),
        }
    }

    #[test]
    fn the_state_table_is_closed() {
        assert_eq!(
            state_transition(SupportState::Open, "case.start"),
            Some(SupportState::InProgress)
        );
        assert_eq!(
            state_transition(SupportState::Resolved, "case.close"),
            Some(SupportState::Closed)
        );
        assert_eq!(state_transition(SupportState::Closed, "case.cancel"), None);
        assert_eq!(state_transition(SupportState::Open, "case.close"), None);
        assert_eq!(
            state_transition(SupportState::Cancelled, "case.start"),
            None
        );
    }

    #[test]
    fn a_reopen_is_clean() {
        assert!(reopenable(SupportState::Resolved));
        assert!(reopenable(SupportState::Closed));
        assert!(!reopenable(SupportState::Waiting));
        let (resolution, resolved_at, closed_at) = reopen_resets();
        assert!(resolution.is_none() && resolved_at.is_none() && closed_at.is_none());
    }

    #[test]
    fn the_grant_ladder_holds() {
        let write = access(&["cases.read", "cases.write"], "all");
        assert!(authorize(
            &write,
            &SupportCommand::Start {
                id: "a".into(),
                expected_revision: 1,
                reason: "r".into()
            }
        )
        .is_ok());
        let note = access(&["cases.read", "notes.write"], "all");
        assert!(authorize(
            &note,
            &SupportCommand::NoteAdd {
                id: "a".into(),
                expected_revision: 1,
                body: "b".into()
            }
        )
        .is_ok());
        assert!(authorize(
            &note,
            &SupportCommand::Start {
                id: "a".into(),
                expected_revision: 1,
                reason: "r".into()
            }
        )
        .is_err());
        let assign = access(&["cases.read", "cases.write", "cases.assign"], "assigned");
        assert!(authorize(
            &assign,
            &SupportCommand::Assign {
                id: "a".into(),
                expected_revision: 1,
                assignee_id: None,
                reason: "r".into()
            }
        )
        .is_err());
    }

    #[test]
    fn the_grammar_is_strict() {
        let ok = json!({
            "requestId": "11111111-1111-1111-1111-111111111111",
            "command": {"operation": "case.create", "requesterName": "Ada", "subject": "Printer", "description": "Broken"}
        });
        let mutation = parse_mutation(&ok).expect("a valid create");
        assert_eq!(mutation.command.operation(), "case.create");
        match mutation.command {
            SupportCommand::Create {
                priority, due_at, ..
            } => {
                assert_eq!(priority, "normal");
                assert!(due_at.is_none());
            }
            _ => panic!("create arm"),
        }

        let extra = json!({
            "requestId": "11111111-1111-1111-1111-111111111111",
            "command": {"operation": "case.create", "requesterName": "Ada", "subject": "Printer", "description": "Broken", "extra": true}
        });
        assert!(parse_mutation(&extra).is_err());

        let bad_state = json!({
            "requestId": "11111111-1111-1111-1111-111111111111",
            "command": {"operation": "case.start", "id": "22222222-2222-2222-2222-222222222222", "expectedRevision": 1, "reason": "go"}
        });
        assert!(parse_mutation(&bad_state).is_ok());
        let wrong_revision = json!({
            "requestId": "11111111-1111-1111-1111-111111111111",
            "command": {"operation": "case.start", "id": "22222222-2222-2222-2222-222222222222", "expectedRevision": 0, "reason": "go"}
        });
        assert!(parse_mutation(&wrong_revision).is_err());
    }

    #[test]
    fn the_query_bounds_hold() {
        let page = parse_case_page(&json!({"limit": 10, "state": "open", "overdue": true}))
            .expect("a valid page");
        assert_eq!(page.limit, 10);
        assert_eq!(page.overdue, Some(true));
        assert!(parse_case_page(&json!({"limit": 0})).is_err());
        assert!(parse_case_page(&json!({"limit": 101})).is_err());
        assert!(parse_case_page(&json!({"state": "unknown"})).is_err());
        assert!(parse_case_page(&json!({"extra": 1})).is_err());
        assert!(parse_assignee_page(&json!({"limit": 50, "after": "ax"})).is_ok());
    }

    #[test]
    fn the_receipt_round_trips() {
        let receipt = SupportReceipt {
            request_id: "11111111-1111-1111-1111-111111111111".into(),
            operation: "case.create".into(),
            id: "22222222-2222-2222-2222-222222222222".into(),
            revision: 1,
            replayed: false,
        };
        let value = serde_json::to_value(&receipt).expect("serialize");
        let parsed = SupportReceipt::parse(&value, true).expect("parse");
        assert!(parsed.replayed);
        assert_eq!(parsed.operation, "case.create");
        let status = SupportCommandStatus::Committed {
            receipt: Box::new(parsed),
        };
        assert_eq!(
            serde_json::to_value(&status).expect("status serialize")["status"],
            "committed"
        );
    }

    #[test]
    fn the_support_id_shape_holds() {
        assert!(valid_support_id("acme-events"));
        assert!(valid_support_id("a1"));
        assert!(!valid_support_id("Acme"));
        assert!(!valid_support_id("-acme"));
        assert!(!valid_support_id("acme-"));
        assert!(!valid_support_id("acme--events"));
        assert!(!valid_support_id(""));
    }

    #[test]
    fn the_due_instant_is_canonical() {
        let raw = json!({
            "requestId": "11111111-1111-1111-1111-111111111111",
            "command": {"operation": "case.create", "requesterName": "Ada", "subject": "Printer", "description": "Broken", "dueAt": "2026-10-20T12:00:00+07:00"}
        });
        match parse_mutation(&raw).expect("valid due").command {
            SupportCommand::Create { due_at, .. } => {
                assert_eq!(due_at.as_deref(), Some("2026-10-20T05:00:00.000Z"));
            }
            _ => panic!("create arm"),
        }
        let out_of_range = json!({
            "requestId": "11111111-1111-1111-1111-111111111111",
            "command": {"operation": "case.create", "requesterName": "Ada", "subject": "Printer", "description": "Broken", "dueAt": "2100-01-01T00:00:00Z"}
        });
        assert!(parse_mutation(&out_of_range).is_err());
    }
}
