//! The field services domain — the Archxy 2 port of `packages/field-services/src/*`:
//! bounded technician dispatch, site visits, work evidence and approval lifecycle.
//!
//! ## What this port keeps from the baseline
//!
//! - All seven command arms (`create_job`, `assign_job`, `record_visit`, `append_work_line`,
//!   `record_signoff`, `approve_job`, `cancel_job`).
//! - Closed vocabularies for job statuses (`requested`, `assigned`, `visited`, `evidenced`,
//!   `signed_off`, `approved`, `canceled`), priorities (`low`, `normal`, `high`, `urgent`),
//!   visit outcomes (`completed`, `partial`, `no_access`), and line kinds (`labour`, `part`).
//! - Strict monotonic forward transitions and rules:
//!   - `append_work_line` requires a recorded site visit (`visited` or `signed_off`).
//!   - `record_signoff` advances to `signed_off` if accepted, or demotes back to `evidenced` if rejected.
//!   - `approve_job` strictly requires an accepted customer signoff.
//!   - Terminal statuses (`approved`, `canceled`) refuse further mutations.
//! - Wire contracts: receipts, job detail, audit trail, pagination queries.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::purchasing::ValidationError;
use super::{bounded_title, trimmed, ErpScope};

pub const FIELD_PERMISSIONS: &[&str] = &[
    "field.read",
    "field.write",
    "field.dispatch",
    "field.signoff",
    "audit.read",
];

// Job statuses
pub const STATUS_REQUESTED: &str = "requested";
pub const STATUS_ASSIGNED: &str = "assigned";
pub const STATUS_VISITED: &str = "visited";
pub const STATUS_EVIDENCED: &str = "evidenced";
pub const STATUS_SIGNED_OFF: &str = "signed_off";
pub const STATUS_APPROVED: &str = "approved";
pub const STATUS_CANCELED: &str = "canceled";

// Priorities
pub const PRIORITY_LOW: &str = "low";
pub const PRIORITY_NORMAL: &str = "normal";
pub const PRIORITY_HIGH: &str = "high";
pub const PRIORITY_URGENT: &str = "urgent";

// Visit outcomes
pub const OUTCOME_COMPLETED: &str = "completed";
pub const OUTCOME_PARTIAL: &str = "partial";
pub const OUTCOME_NO_ACCESS: &str = "no_access";

// Work line kinds and units
pub const LINE_KIND_LABOUR: &str = "labour";
pub const LINE_KIND_PART: &str = "part";
pub const LINE_UNIT_HOURS: &str = "hours";
pub const LINE_UNIT_PIECES: &str = "pieces";

// Field bounds
pub const ID_MAX: usize = 100;
pub const SUMMARY_MAX: usize = 500;
pub const NOTE_MAX: usize = 2000;
pub const DESCRIPTION_MAX: usize = 500;
pub const COMMENT_MAX: usize = 2000;
pub const REASON_MAX: usize = 500;
pub const PAGE_DEFAULT_LIMIT: i64 = 50;
pub const PAGE_MAX_LIMIT: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Requested,
    Assigned,
    Visited,
    Evidenced,
    SignedOff,
    Approved,
    Canceled,
}

impl JobStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATUS_REQUESTED => Some(Self::Requested),
            STATUS_ASSIGNED => Some(Self::Assigned),
            STATUS_VISITED => Some(Self::Visited),
            STATUS_EVIDENCED => Some(Self::Evidenced),
            STATUS_SIGNED_OFF => Some(Self::SignedOff),
            STATUS_APPROVED => Some(Self::Approved),
            STATUS_CANCELED => Some(Self::Canceled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Requested => STATUS_REQUESTED,
            Self::Assigned => STATUS_ASSIGNED,
            Self::Visited => STATUS_VISITED,
            Self::Evidenced => STATUS_EVIDENCED,
            Self::SignedOff => STATUS_SIGNED_OFF,
            Self::Approved => STATUS_APPROVED,
            Self::Canceled => STATUS_CANCELED,
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Self::Approved | Self::Canceled)
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        match self {
            Self::Requested => matches!(target, Self::Assigned | Self::Canceled),
            Self::Assigned => matches!(target, Self::Visited | Self::Canceled),
            Self::Visited => matches!(target, Self::Evidenced | Self::Canceled),
            Self::Evidenced => matches!(target, Self::SignedOff),
            Self::SignedOff => matches!(target, Self::Approved | Self::Evidenced),
            Self::Approved | Self::Canceled => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    Normal,
    High,
    Urgent,
}

impl Priority {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            PRIORITY_LOW => Some(Self::Low),
            PRIORITY_NORMAL => Some(Self::Normal),
            PRIORITY_HIGH => Some(Self::High),
            PRIORITY_URGENT => Some(Self::Urgent),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => PRIORITY_LOW,
            Self::Normal => PRIORITY_NORMAL,
            Self::High => PRIORITY_HIGH,
            Self::Urgent => PRIORITY_URGENT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisitOutcome {
    Completed,
    Partial,
    NoAccess,
}

impl VisitOutcome {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            OUTCOME_COMPLETED => Some(Self::Completed),
            OUTCOME_PARTIAL => Some(Self::Partial),
            OUTCOME_NO_ACCESS => Some(Self::NoAccess),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => OUTCOME_COMPLETED,
            Self::Partial => OUTCOME_PARTIAL,
            Self::NoAccess => OUTCOME_NO_ACCESS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineKind {
    Labour,
    Part,
}

impl LineKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            LINE_KIND_LABOUR => Some(Self::Labour),
            LINE_KIND_PART => Some(Self::Part),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Labour => LINE_KIND_LABOUR,
            Self::Part => LINE_KIND_PART,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkLine {
    pub line_id: String,
    pub kind: LineKind,
    pub description: String,
    pub quantity: f64,
    pub unit: String,
    pub part_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldCommand {
    CreateJob {
        command_id: String,
        job_id: String,
        customer_ref: String,
        site_ref: String,
        summary: String,
        priority: Priority,
        appointment_ref: String,
        asset_ref: Option<String>,
    },
    AssignJob {
        command_id: String,
        job_id: String,
        assigned_principal: String,
    },
    RecordVisit {
        command_id: String,
        job_id: String,
        visit_id: String,
        outcome: VisitOutcome,
        note: String,
        technician_ref: String,
    },
    AppendWorkLine {
        command_id: String,
        job_id: String,
        line: WorkLine,
    },
    RecordSignoff {
        command_id: String,
        job_id: String,
        signoff_id: String,
        signer_ref: String,
        accepted: bool,
        comment: Option<String>,
    },
    ApproveJob {
        command_id: String,
        job_id: String,
    },
    CancelJob {
        command_id: String,
        job_id: String,
        reason: String,
    },
}

impl FieldCommand {
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateJob { command_id, .. } => command_id,
            Self::AssignJob { command_id, .. } => command_id,
            Self::RecordVisit { command_id, .. } => command_id,
            Self::AppendWorkLine { command_id, .. } => command_id,
            Self::RecordSignoff { command_id, .. } => command_id,
            Self::ApproveJob { command_id, .. } => command_id,
            Self::CancelJob { command_id, .. } => command_id,
        }
    }

    pub fn action(&self) -> &'static str {
        match self {
            Self::CreateJob { .. } => "create_job",
            Self::AssignJob { .. } => "assign_job",
            Self::RecordVisit { .. } => "record_visit",
            Self::AppendWorkLine { .. } => "append_work_line",
            Self::RecordSignoff { .. } => "record_signoff",
            Self::ApproveJob { .. } => "approve_job",
            Self::CancelJob { .. } => "cancel_job",
        }
    }

    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::CreateJob { .. } | Self::AppendWorkLine { .. } => "field.write",
            Self::AssignJob { .. } | Self::RecordVisit { .. } | Self::CancelJob { .. } => {
                "field.dispatch"
            }
            Self::RecordSignoff { .. } | Self::ApproveJob { .. } => "field.signoff",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldReceipt {
    pub command_id: String,
    pub action: String,
    pub job_id: String,
    pub status: JobStatus,
    pub revision: i32,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldCommandStatus {
    pub command_id: String,
    pub action: String,
    pub job_id: String,
    pub status: String,
    pub revision: Option<i32>,
    pub recorded_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldJob {
    pub job_id: String,
    pub customer_ref: String,
    pub site_ref: String,
    pub summary: String,
    pub priority: Priority,
    pub status: JobStatus,
    pub appointment_ref: String,
    pub asset_ref: Option<String>,
    pub assigned_principal: Option<String>,
    pub labor_hours: f64,
    pub parts_pieces: i32,
    pub revision: i32,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisitRecord {
    pub visit_id: String,
    pub job_id: String,
    pub outcome: VisitOutcome,
    pub note: String,
    pub technician_ref: String,
    pub visited_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignoffRecord {
    pub signoff_id: String,
    pub job_id: String,
    pub signer_ref: String,
    pub accepted: bool,
    pub comment: Option<String>,
    pub signed_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldJobDetail {
    pub job: FieldJob,
    pub visits: Vec<VisitRecord>,
    pub work_lines: Vec<WorkLine>,
    pub signoff: Option<SignoffRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldAuditEntry {
    pub entry_id: String,
    pub job_id: String,
    pub action: String,
    pub actor_ref: String,
    pub command_id: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldJobPageQuery {
    pub status: Option<JobStatus>,
    pub limit: i64,
    pub cursor: Option<String>,
}

// --- Parsers ---

pub fn parse_command(raw: &Value) -> Result<FieldCommand, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("command"))?;
    let action = object
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or(ValidationError("action"))?;

    let command_id = object
        .get("commandId")
        .and_then(|v| v.as_str())
        .and_then(|v| bounded_title(v, ID_MAX))
        .ok_or(ValidationError("commandId"))?;

    match action {
        "create_job" => {
            let job_obj = object
                .get("job")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("job"))?;

            let job_id = job_obj
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            let customer_ref = job_obj
                .get("customerRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("customerRef"))?;

            let site_ref = job_obj
                .get("siteRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("siteRef"))?;

            let summary = job_obj
                .get("summary")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, SUMMARY_MAX))
                .ok_or(ValidationError("summary"))?;

            let priority = match job_obj.get("priority") {
                Some(Value::String(s)) => Priority::parse(s).ok_or(ValidationError("priority"))?,
                Some(Value::Null) | None => Priority::Normal,
                _ => return Err(ValidationError("priority")),
            };

            let appointment_ref = job_obj
                .get("appointmentRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("appointmentRef"))?;

            let asset_ref = match job_obj.get("assetRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > ID_MAX {
                        return Err(ValidationError("assetRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("assetRef")),
            };

            Ok(FieldCommand::CreateJob {
                command_id,
                job_id,
                customer_ref,
                site_ref,
                summary,
                priority,
                appointment_ref,
                asset_ref,
            })
        }
        "assign_job" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            let assigned_principal = object
                .get("assignedPrincipal")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("assignedPrincipal"))?;

            Ok(FieldCommand::AssignJob {
                command_id,
                job_id,
                assigned_principal,
            })
        }
        "record_visit" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            let visit_obj = object
                .get("visit")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("visit"))?;

            let visit_id = visit_obj
                .get("visitId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("visitId"))?;

            let outcome_str = visit_obj
                .get("outcome")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("outcome"))?;
            let outcome = VisitOutcome::parse(outcome_str).ok_or(ValidationError("outcome"))?;

            let note = visit_obj
                .get("note")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, NOTE_MAX))
                .ok_or(ValidationError("note"))?;

            let technician_ref = visit_obj
                .get("technicianRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("technicianRef"))?;

            Ok(FieldCommand::RecordVisit {
                command_id,
                job_id,
                visit_id,
                outcome,
                note,
                technician_ref,
            })
        }
        "append_work_line" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            let line_obj = object
                .get("line")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("line"))?;

            let line_id = line_obj
                .get("lineId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("lineId"))?;

            let kind_str = line_obj
                .get("kind")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("kind"))?;
            let kind = LineKind::parse(kind_str).ok_or(ValidationError("kind"))?;

            let description = line_obj
                .get("description")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, DESCRIPTION_MAX))
                .ok_or(ValidationError("description"))?;

            let quantity = line_obj
                .get("quantity")
                .and_then(|v| v.as_f64())
                .filter(|&q| q > 0.0)
                .ok_or(ValidationError("quantity"))?;

            let unit = line_obj
                .get("unit")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("unit"))?;

            // Super-refine checks:
            match kind {
                LineKind::Labour => {
                    if unit != LINE_UNIT_HOURS {
                        return Err(ValidationError("unit"));
                    }
                    if line_obj.get("partRef").is_some() && !line_obj["partRef"].is_null() {
                        return Err(ValidationError("partRef"));
                    }
                }
                LineKind::Part => {
                    if unit != LINE_UNIT_PIECES {
                        return Err(ValidationError("unit"));
                    }
                    if (quantity.fract()).abs() > f64::EPSILON {
                        return Err(ValidationError("quantity"));
                    }
                }
            }

            let part_ref = match line_obj.get("partRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > ID_MAX {
                        return Err(ValidationError("partRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("partRef")),
            };

            Ok(FieldCommand::AppendWorkLine {
                command_id,
                job_id,
                line: WorkLine {
                    line_id,
                    kind,
                    description,
                    quantity,
                    unit: unit.to_string(),
                    part_ref,
                },
            })
        }
        "record_signoff" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            let signoff_obj = object
                .get("signoff")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("signoff"))?;

            let signoff_id = signoff_obj
                .get("signoffId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("signoffId"))?;

            let signer_ref = signoff_obj
                .get("signerRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("signerRef"))?;

            let accepted = signoff_obj
                .get("accepted")
                .and_then(|v| v.as_bool())
                .ok_or(ValidationError("accepted"))?;

            let comment = match signoff_obj.get("comment") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > COMMENT_MAX {
                        return Err(ValidationError("comment"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("comment")),
            };

            Ok(FieldCommand::RecordSignoff {
                command_id,
                job_id,
                signoff_id,
                signer_ref,
                accepted,
                comment,
            })
        }
        "approve_job" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            Ok(FieldCommand::ApproveJob { command_id, job_id })
        }
        "cancel_job" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ID_MAX))
                .ok_or(ValidationError("jobId"))?;

            let reason = object
                .get("reason")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, REASON_MAX))
                .ok_or(ValidationError("reason"))?;

            Ok(FieldCommand::CancelJob {
                command_id,
                job_id,
                reason,
            })
        }
        _ => Err(ValidationError("action")),
    }
}

pub fn parse_page_query(
    status_param: Option<&str>,
    limit_param: Option<i64>,
    cursor_param: Option<&str>,
) -> Result<FieldJobPageQuery, ValidationError> {
    let status = match status_param {
        Some(s) if !s.trim().is_empty() => {
            Some(JobStatus::parse(s.trim()).ok_or(ValidationError("status"))?)
        }
        _ => None,
    };

    let limit = match limit_param {
        Some(l) if (1..=PAGE_MAX_LIMIT).contains(&l) => l,
        Some(_) => return Err(ValidationError("limit")),
        None => PAGE_DEFAULT_LIMIT,
    };

    let cursor = cursor_param
        .map(trimmed)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    Ok(FieldJobPageQuery {
        status,
        limit,
        cursor,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_job_status_lifecycle() {
        assert!(JobStatus::Requested.can_transition_to(JobStatus::Assigned));
        assert!(JobStatus::Requested.can_transition_to(JobStatus::Canceled));
        assert!(!JobStatus::Requested.can_transition_to(JobStatus::Visited));

        assert!(JobStatus::Assigned.can_transition_to(JobStatus::Visited));
        assert!(JobStatus::Assigned.can_transition_to(JobStatus::Canceled));
        assert!(!JobStatus::Assigned.can_transition_to(JobStatus::Approved));

        assert!(JobStatus::Visited.can_transition_to(JobStatus::Evidenced));
        assert!(JobStatus::Visited.can_transition_to(JobStatus::Canceled));

        assert!(JobStatus::Evidenced.can_transition_to(JobStatus::SignedOff));
        assert!(!JobStatus::Evidenced.can_transition_to(JobStatus::Approved));

        assert!(JobStatus::SignedOff.can_transition_to(JobStatus::Approved));
        assert!(JobStatus::SignedOff.can_transition_to(JobStatus::Evidenced)); // demoted when rejected

        assert!(JobStatus::Approved.is_closed());
        assert!(JobStatus::Canceled.is_closed());
        assert!(!JobStatus::Approved.can_transition_to(JobStatus::Requested));
    }

    #[test]
    fn test_parse_commands() {
        let create_cmd = json!({
            "action": "create_job",
            "commandId": "cmd-001",
            "job": {
                "jobId": "job-101",
                "customerRef": "cust-1",
                "siteRef": "site-hq",
                "summary": "Fix HVAC unit on roof",
                "priority": "high",
                "appointmentRef": "appt-90"
            }
        });
        let parsed = parse_command(&create_cmd).unwrap();
        assert_eq!(parsed.action(), "create_job");
        assert_eq!(parsed.command_id(), "cmd-001");
        assert_eq!(parsed.required_permission(), "field.write");

        let labour_line = json!({
            "action": "append_work_line",
            "commandId": "cmd-002",
            "jobId": "job-101",
            "line": {
                "lineId": "line-1",
                "kind": "labour",
                "description": "Compressor diagnostic & repair",
                "quantity": 2.5,
                "unit": "hours"
            }
        });
        let parsed_line = parse_command(&labour_line).unwrap();
        assert_eq!(parsed_line.action(), "append_work_line");

        // Labour line with partRef should fail
        let bad_labour = json!({
            "action": "append_work_line",
            "commandId": "cmd-003",
            "jobId": "job-101",
            "line": {
                "lineId": "line-2",
                "kind": "labour",
                "description": "Bad line",
                "quantity": 1.0,
                "unit": "hours",
                "partRef": "PRT-01"
            }
        });
        assert!(parse_command(&bad_labour).is_err());

        // Part line with fractional quantity should fail
        let bad_part = json!({
            "action": "append_work_line",
            "commandId": "cmd-004",
            "jobId": "job-101",
            "line": {
                "lineId": "line-3",
                "kind": "part",
                "description": "Filter replacement",
                "quantity": 2.5,
                "unit": "pieces",
                "partRef": "FLT-9"
            }
        });
        assert!(parse_command(&bad_part).is_err());
    }
}
