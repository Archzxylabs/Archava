//! The formal education domain — admission, enrollment, recorded attendance,
//! assessment results, review, and retention.
//!
//! Enforces:
//! - Four non-ranked access scopes: `all`, `guardian`, `self`, `assigned_class`.
//! - Monotonic enrollment lifecycle:
//!   `applied -> admitted -> enrolled -> attending -> reported -> reviewed -> (retained | graduated | withdrawn)`
//! - Assessment scores bounded by maximum: `score <= out_of`.
//! - Published boundary: results are visible only at `reviewed`, `retained`, `graduated`.
//! - Idempotent command envelope.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const SCOPE_ALL: &str = "all";
pub const SCOPE_GUARDIAN: &str = "guardian";
pub const SCOPE_SELF: &str = "self";
pub const SCOPE_ASSIGNED_CLASS: &str = "assigned_class";

pub const STATUS_APPLIED: &str = "applied";
pub const STATUS_ADMITTED: &str = "admitted";
pub const STATUS_ENROLLED: &str = "enrolled";
pub const STATUS_ATTENDING: &str = "attending";
pub const STATUS_REPORTED: &str = "reported";
pub const STATUS_REVIEWED: &str = "reviewed";
pub const STATUS_RETAINED: &str = "retained";
pub const STATUS_GRADUATED: &str = "graduated";
pub const STATUS_WITHDRAWN: &str = "withdrawn";

pub const OUTCOME_PRESENT: &str = "present";
pub const OUTCOME_ABSENT: &str = "absent";
pub const OUTCOME_LATE: &str = "late";
pub const OUTCOME_EXCUSED: &str = "excused";

pub const DECISION_APPROVED: &str = "approved";
pub const DECISION_RETURNED: &str = "returned";

pub const STRING_MAX: usize = 100;
pub const NOTE_MAX: usize = 2000;
pub const REASON_MAX: usize = 500;
pub const LETTER_REF_MAX: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError(pub &'static str);

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Formal education validation failed on {}", self.0)
    }
}

impl std::error::Error for ValidationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EducationScope {
    All,
    Guardian,
    SelfScope,
    AssignedClass,
}

impl EducationScope {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            SCOPE_ALL => Some(Self::All),
            SCOPE_GUARDIAN => Some(Self::Guardian),
            SCOPE_SELF => Some(Self::SelfScope),
            SCOPE_ASSIGNED_CLASS => Some(Self::AssignedClass),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => SCOPE_ALL,
            Self::Guardian => SCOPE_GUARDIAN,
            Self::SelfScope => SCOPE_SELF,
            Self::AssignedClass => SCOPE_ASSIGNED_CLASS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EnrollmentStatus {
    Applied,
    Admitted,
    Enrolled,
    Attending,
    Reported,
    Reviewed,
    Retained,
    Graduated,
    Withdrawn,
}

impl EnrollmentStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATUS_APPLIED => Some(Self::Applied),
            STATUS_ADMITTED => Some(Self::Admitted),
            STATUS_ENROLLED => Some(Self::Enrolled),
            STATUS_ATTENDING => Some(Self::Attending),
            STATUS_REPORTED => Some(Self::Reported),
            STATUS_REVIEWED => Some(Self::Reviewed),
            STATUS_RETAINED => Some(Self::Retained),
            STATUS_GRADUATED => Some(Self::Graduated),
            STATUS_WITHDRAWN => Some(Self::Withdrawn),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Applied => STATUS_APPLIED,
            Self::Admitted => STATUS_ADMITTED,
            Self::Enrolled => STATUS_ENROLLED,
            Self::Attending => STATUS_ATTENDING,
            Self::Reported => STATUS_REPORTED,
            Self::Reviewed => STATUS_REVIEWED,
            Self::Retained => STATUS_RETAINED,
            Self::Graduated => STATUS_GRADUATED,
            Self::Withdrawn => STATUS_WITHDRAWN,
        }
    }

    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Retained | Self::Graduated | Self::Withdrawn)
    }

    pub fn is_published(self) -> bool {
        matches!(self, Self::Reviewed | Self::Retained | Self::Graduated)
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        match self {
            Self::Applied => matches!(target, Self::Admitted | Self::Withdrawn),
            Self::Admitted => matches!(target, Self::Enrolled | Self::Withdrawn),
            Self::Enrolled => matches!(target, Self::Attending | Self::Withdrawn),
            Self::Attending => matches!(target, Self::Reported | Self::Withdrawn),
            Self::Reported => matches!(target, Self::Reviewed | Self::Attending),
            Self::Reviewed => matches!(target, Self::Retained | Self::Graduated | Self::Withdrawn),
            Self::Retained | Self::Graduated | Self::Withdrawn => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionOutcome {
    Present,
    Absent,
    Late,
    Excused,
}

impl SessionOutcome {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            OUTCOME_PRESENT => Some(Self::Present),
            OUTCOME_ABSENT => Some(Self::Absent),
            OUTCOME_LATE => Some(Self::Late),
            OUTCOME_EXCUSED => Some(Self::Excused),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Present => OUTCOME_PRESENT,
            Self::Absent => OUTCOME_ABSENT,
            Self::Late => OUTCOME_LATE,
            Self::Excused => OUTCOME_EXCUSED,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: EducationScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Enrollment {
    pub enrollment_id: String,
    pub student_ref: String,
    pub class_ref: String,
    pub term_ref: String,
    pub status: EnrollmentStatus,
    pub revision: i32,
    pub invoice_ref: Option<String>,
    pub guardian_ref: Option<String>,
    pub created_by: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRecord {
    pub session_id: String,
    pub outcome: SessionOutcome,
    pub note: Option<String>,
    pub appointment_ref: String,
    pub recorded_by: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AssessmentResult {
    pub result_id: String,
    pub assessment_ref: String,
    pub score: f64,
    pub out_of: f64,
    pub letter_ref: Option<String>,
    pub note: Option<String>,
    pub recorded_by: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRecord {
    pub review_id: String,
    pub decision: String,
    pub note: Option<String>,
    pub reviewer_ref: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentDetail {
    pub enrollment: Enrollment,
    pub sessions: Vec<SessionRecord>,
    pub results: Vec<AssessmentResult>,
    pub review: Option<ReviewRecord>,
    pub results_published: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationAuditEntry {
    pub entry_id: String,
    pub enrollment_id: String,
    pub action: String,
    pub actor_ref: String,
    pub command_id: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum EducationCommand {
    AdmitApplicant {
        command_id: String,
        enrollment_id: String,
        student_ref: String,
        class_ref: String,
        term_ref: String,
        guardian_ref: Option<String>,
    },
    ConfirmEnrollment {
        command_id: String,
        enrollment_id: String,
    },
    EnrollStudent {
        command_id: String,
        enrollment_id: String,
    },
    RecordSession {
        command_id: String,
        enrollment_id: String,
        session_id: String,
        outcome: SessionOutcome,
        note: Option<String>,
        appointment_ref: String,
    },
    RecordResult {
        command_id: String,
        enrollment_id: String,
        result_id: String,
        assessment_ref: String,
        score: f64,
        out_of: f64,
        letter_ref: Option<String>,
        note: Option<String>,
    },
    ReviewRecord {
        command_id: String,
        enrollment_id: String,
        review_id: String,
        decision: String,
        note: Option<String>,
    },
    RecordRetention {
        command_id: String,
        enrollment_id: String,
    },
    GraduateEnrollment {
        command_id: String,
        enrollment_id: String,
    },
    WithdrawStudent {
        command_id: String,
        enrollment_id: String,
        reason: String,
    },
}

impl EducationCommand {
    pub fn command_id(&self) -> &str {
        match self {
            Self::AdmitApplicant { command_id, .. } => command_id,
            Self::ConfirmEnrollment { command_id, .. } => command_id,
            Self::EnrollStudent { command_id, .. } => command_id,
            Self::RecordSession { command_id, .. } => command_id,
            Self::RecordResult { command_id, .. } => command_id,
            Self::ReviewRecord { command_id, .. } => command_id,
            Self::RecordRetention { command_id, .. } => command_id,
            Self::GraduateEnrollment { command_id, .. } => command_id,
            Self::WithdrawStudent { command_id, .. } => command_id,
        }
    }

    pub fn enrollment_id(&self) -> &str {
        match self {
            Self::AdmitApplicant { enrollment_id, .. } => enrollment_id,
            Self::ConfirmEnrollment { enrollment_id, .. } => enrollment_id,
            Self::EnrollStudent { enrollment_id, .. } => enrollment_id,
            Self::RecordSession { enrollment_id, .. } => enrollment_id,
            Self::RecordResult { enrollment_id, .. } => enrollment_id,
            Self::ReviewRecord { enrollment_id, .. } => enrollment_id,
            Self::RecordRetention { enrollment_id, .. } => enrollment_id,
            Self::GraduateEnrollment { enrollment_id, .. } => enrollment_id,
            Self::WithdrawStudent { enrollment_id, .. } => enrollment_id,
        }
    }

    pub fn action(&self) -> &'static str {
        match self {
            Self::AdmitApplicant { .. } => "admit_applicant",
            Self::ConfirmEnrollment { .. } => "confirm_enrollment",
            Self::EnrollStudent { .. } => "enroll_student",
            Self::RecordSession { .. } => "record_session",
            Self::RecordResult { .. } => "record_result",
            Self::ReviewRecord { .. } => "review_record",
            Self::RecordRetention { .. } => "record_retention",
            Self::GraduateEnrollment { .. } => "graduate_enrollment",
            Self::WithdrawStudent { .. } => "withdraw_student",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationReceipt {
    pub command_id: String,
    pub action: String,
    pub enrollment_id: String,
    pub status: EnrollmentStatus,
    pub revision: i32,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EducationCommandStatus {
    pub command_id: String,
    pub action: String,
    pub enrollment_id: String,
    pub status: String,
    pub revision: Option<i32>,
    pub recorded_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrollmentPageQuery {
    pub status: Option<EnrollmentStatus>,
    pub limit: i64,
    pub cursor: Option<String>,
}

fn trimmed(s: &str) -> &str {
    s.trim()
}

fn bounded_string(s: &str, max: usize) -> Option<String> {
    let t = trimmed(s);
    if t.is_empty() || t.len() > max {
        None
    } else {
        Some(t.to_string())
    }
}

pub fn parse_page_query(
    limit: Option<i64>,
    cursor: Option<&str>,
    status: Option<&str>,
) -> Result<EnrollmentPageQuery, ValidationError> {
    let l = limit.unwrap_or(50);
    if l <= 0 || l > 200 {
        return Err(ValidationError("limit"));
    }

    let parsed_status = match status {
        Some(s) if !trimmed(s).is_empty() => {
            Some(EnrollmentStatus::parse(trimmed(s)).ok_or(ValidationError("status"))?)
        }
        _ => None,
    };

    let parsed_cursor = cursor.and_then(|c| bounded_string(c, STRING_MAX));

    Ok(EnrollmentPageQuery {
        status: parsed_status,
        limit: l,
        cursor: parsed_cursor,
    })
}

pub fn parse_command(raw: &Value) -> Result<EducationCommand, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("command"))?;
    let action = object
        .get("action")
        .and_then(|v| v.as_str())
        .ok_or(ValidationError("action"))?;

    let command_id = object
        .get("commandId")
        .and_then(|v| v.as_str())
        .and_then(|v| bounded_string(v, STRING_MAX))
        .ok_or(ValidationError("commandId"))?;

    match action {
        "admit_applicant" => {
            let enr_obj = object
                .get("enrollment")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("enrollment"))?;

            let enrollment_id = enr_obj
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            let student_ref = enr_obj
                .get("studentRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("studentRef"))?;

            let class_ref = enr_obj
                .get("classRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("classRef"))?;

            let term_ref = enr_obj
                .get("termRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("termRef"))?;

            let guardian_ref = match enr_obj.get("guardianRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > STRING_MAX {
                        return Err(ValidationError("guardianRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("guardianRef")),
            };

            Ok(EducationCommand::AdmitApplicant {
                command_id,
                enrollment_id,
                student_ref,
                class_ref,
                term_ref,
                guardian_ref,
            })
        }

        "confirm_enrollment" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            Ok(EducationCommand::ConfirmEnrollment {
                command_id,
                enrollment_id,
            })
        }

        "enroll_student" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            Ok(EducationCommand::EnrollStudent {
                command_id,
                enrollment_id,
            })
        }

        "record_session" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            let ses_obj = object
                .get("session")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("session"))?;

            let session_id = ses_obj
                .get("sessionId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("sessionId"))?;

            let outcome_str = ses_obj
                .get("outcome")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("outcome"))?;
            let outcome = SessionOutcome::parse(outcome_str).ok_or(ValidationError("outcome"))?;

            let appointment_ref = ses_obj
                .get("appointmentRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("appointmentRef"))?;

            let note = match ses_obj.get("note") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > NOTE_MAX {
                        return Err(ValidationError("note"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("note")),
            };

            Ok(EducationCommand::RecordSession {
                command_id,
                enrollment_id,
                session_id,
                outcome,
                note,
                appointment_ref,
            })
        }

        "record_result" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            let res_obj = object
                .get("result")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("result"))?;

            let result_id = res_obj
                .get("resultId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("resultId"))?;

            let assessment_ref = res_obj
                .get("assessmentRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("assessmentRef"))?;

            let score = res_obj
                .get("score")
                .and_then(|v| v.as_f64())
                .filter(|&s| s >= 0.0)
                .ok_or(ValidationError("score"))?;

            let out_of = res_obj
                .get("outOf")
                .and_then(|v| v.as_f64())
                .filter(|&o| o > 0.0)
                .ok_or(ValidationError("outOf"))?;

            if score > out_of {
                return Err(ValidationError("score_exceeds_maximum"));
            }

            let letter_ref = match res_obj.get("letterRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > LETTER_REF_MAX {
                        return Err(ValidationError("letterRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("letterRef")),
            };

            let note = match res_obj.get("note") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > NOTE_MAX {
                        return Err(ValidationError("note"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("note")),
            };

            Ok(EducationCommand::RecordResult {
                command_id,
                enrollment_id,
                result_id,
                assessment_ref,
                score,
                out_of,
                letter_ref,
                note,
            })
        }

        "review_record" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            let rev_obj = object
                .get("review")
                .and_then(|v| v.as_object())
                .ok_or(ValidationError("review"))?;

            let review_id = rev_obj
                .get("reviewId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("reviewId"))?;

            let decision_str = rev_obj
                .get("decision")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("decision"))?;

            if decision_str != DECISION_APPROVED && decision_str != DECISION_RETURNED {
                return Err(ValidationError("decision"));
            }

            let note = match rev_obj.get("note") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > NOTE_MAX {
                        return Err(ValidationError("note"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("note")),
            };

            Ok(EducationCommand::ReviewRecord {
                command_id,
                enrollment_id,
                review_id,
                decision: decision_str.to_string(),
                note,
            })
        }

        "record_retention" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            Ok(EducationCommand::RecordRetention {
                command_id,
                enrollment_id,
            })
        }

        "graduate_enrollment" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            Ok(EducationCommand::GraduateEnrollment {
                command_id,
                enrollment_id,
            })
        }

        "withdraw_student" => {
            let enrollment_id = object
                .get("enrollmentId")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, STRING_MAX))
                .ok_or(ValidationError("enrollmentId"))?;

            let reason = object
                .get("reason")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_string(v, REASON_MAX))
                .ok_or(ValidationError("reason"))?;

            Ok(EducationCommand::WithdrawStudent {
                command_id,
                enrollment_id,
                reason,
            })
        }

        _ => Err(ValidationError("unknown_action")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn transitions_admit_and_graduate() {
        assert!(EnrollmentStatus::Applied.can_transition_to(EnrollmentStatus::Admitted));
        assert!(EnrollmentStatus::Admitted.can_transition_to(EnrollmentStatus::Enrolled));
        assert!(EnrollmentStatus::Enrolled.can_transition_to(EnrollmentStatus::Attending));
        assert!(EnrollmentStatus::Attending.can_transition_to(EnrollmentStatus::Reported));
        assert!(EnrollmentStatus::Reported.can_transition_to(EnrollmentStatus::Reviewed));
        assert!(EnrollmentStatus::Reviewed.can_transition_to(EnrollmentStatus::Graduated));
        assert!(EnrollmentStatus::Reviewed.can_transition_to(EnrollmentStatus::Retained));
        assert!(EnrollmentStatus::Reviewed.can_transition_to(EnrollmentStatus::Withdrawn));
        // Return to attending allowed from reported
        assert!(EnrollmentStatus::Reported.can_transition_to(EnrollmentStatus::Attending));
        // Terminal states cannot transition
        assert!(!EnrollmentStatus::Graduated.can_transition_to(EnrollmentStatus::Enrolled));
        assert!(!EnrollmentStatus::Retained.can_transition_to(EnrollmentStatus::Enrolled));
        assert!(!EnrollmentStatus::Withdrawn.can_transition_to(EnrollmentStatus::Enrolled));
    }

    #[test]
    fn command_parsing_score_bound() {
        let valid = json!({
            "action": "record_result",
            "commandId": "cmd-01",
            "enrollmentId": "enr-01",
            "result": {
                "resultId": "res-01",
                "assessmentRef": "midterm-math",
                "score": 18.5,
                "outOf": 20.0
            }
        });
        assert!(parse_command(&valid).is_ok());

        let invalid = json!({
            "action": "record_result",
            "commandId": "cmd-02",
            "enrollmentId": "enr-01",
            "result": {
                "resultId": "res-02",
                "assessmentRef": "midterm-math",
                "score": 25.0,
                "outOf": 20.0
            }
        });
        assert_eq!(
            parse_command(&invalid),
            Err(ValidationError("score_exceeds_maximum"))
        );
    }
}
