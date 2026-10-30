//! The fulfillment domain — the Archxy 2 port of `packages/fulfillment/src/*`:
//! bounded dispatch work orders, production orders, custody tracking, work lines, findings and reconciliation.
//!
//! ## Invariants preserved
//!
//! - Closed vocabularies for job kinds (`dispatch`, `production`), statuses (`draft`, `released`,
//!   `in_progress`, `completed`, `canceled`), line directions (`consume`, `produce`),
//!   line dispositions (`normal`, `scrap`), finding categories (`condition`, `quantity`, `delivery`),
//!   finding dispositions (`accepted`, `partial`, `rejected`, `lost`, `damaged`),
//!   and custody events (`received`, `stored`, `picked`, `packed`, `dispatched`, `delivered`, `returned`).
//! - Strictly monotonic transitions:
//!   - `draft` -> `released`, `canceled`
//!   - `released` -> `in_progress`, `canceled`
//!   - `in_progress` -> `completed`, `canceled`
//!   - `completed` and `canceled` are terminal.
//! - Sequential custody events: begins with `received`, strictly monotonic ascending stage
//!   (`received: 0` -> `stored: 1` -> `picked: 2` -> `packed: 3` -> `dispatched: 4` -> `delivered: 5`).
//!   `returned` requires prior `dispatched` event.
//! - Finding exceptions (`partial`, `rejected`, `lost`, `damaged`) increment `unreconciled_count`.
//!   Accepted finding with `reconciles_id` decrements `unreconciled_count`.
//! - Quality gate on `change_job_status` to `completed`: strictly requires `unreconciled_count == 0`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::purchasing::ValidationError;
use super::{bounded_title, trimmed, valid_uuid, ErpScope};

pub const FULFILLMENT_PERMISSIONS: &[&str] = &[
    "fulfillment.read",
    "fulfillment.write",
    "fulfillment.dispatch",
    "fulfillment.produce",
    "audit.read",
];

// Job kinds
pub const KIND_DISPATCH: &str = "dispatch";
pub const KIND_PRODUCTION: &str = "production";

// Job statuses
pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_RELEASED: &str = "released";
pub const STATUS_IN_PROGRESS: &str = "in_progress";
pub const STATUS_COMPLETED: &str = "completed";
pub const STATUS_CANCELED: &str = "canceled";

// Line directions
pub const DIRECTION_CONSUME: &str = "consume";
pub const DIRECTION_PRODUCE: &str = "produce";

// Line dispositions
pub const DISPOSITION_NORMAL: &str = "normal";
pub const DISPOSITION_SCRAP: &str = "scrap";

// Finding categories
pub const CATEGORY_CONDITION: &str = "condition";
pub const CATEGORY_QUANTITY: &str = "quantity";
pub const CATEGORY_DELIVERY: &str = "delivery";

// Finding dispositions
pub const DISPOSITION_ACCEPTED: &str = "accepted";
pub const DISPOSITION_PARTIAL: &str = "partial";
pub const DISPOSITION_REJECTED: &str = "rejected";
pub const DISPOSITION_LOST: &str = "lost";
pub const DISPOSITION_DAMAGED: &str = "damaged";

// Custody events
pub const CUSTODY_RECEIVED: &str = "received";
pub const CUSTODY_STORED: &str = "stored";
pub const CUSTODY_PICKED: &str = "picked";
pub const CUSTODY_PACKED: &str = "packed";
pub const CUSTODY_DISPATCHED: &str = "dispatched";
pub const CUSTODY_DELIVERED: &str = "delivered";
pub const CUSTODY_RETURNED: &str = "returned";

// Bounds
pub const JOB_CODE_MAX: usize = 50;
pub const TITLE_MAX: usize = 200;
pub const OWNER_REF_MAX: usize = 80;
pub const ITEM_REF_MAX: usize = 40;
pub const EVIDENCE_REF_MAX: usize = 300;
pub const REASON_MAX: usize = 500;
pub const NOTE_MAX: usize = 500;
pub const PAGE_DEFAULT_LIMIT: i64 = 50;
pub const PAGE_MAX_LIMIT: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobKind {
    Dispatch,
    Production,
}

impl JobKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            KIND_DISPATCH => Some(Self::Dispatch),
            KIND_PRODUCTION => Some(Self::Production),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dispatch => KIND_DISPATCH,
            Self::Production => KIND_PRODUCTION,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    Draft,
    Released,
    InProgress,
    Completed,
    Canceled,
}

impl JobStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATUS_DRAFT => Some(Self::Draft),
            STATUS_RELEASED => Some(Self::Released),
            STATUS_IN_PROGRESS => Some(Self::InProgress),
            STATUS_COMPLETED => Some(Self::Completed),
            STATUS_CANCELED => Some(Self::Canceled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => STATUS_DRAFT,
            Self::Released => STATUS_RELEASED,
            Self::InProgress => STATUS_IN_PROGRESS,
            Self::Completed => STATUS_COMPLETED,
            Self::Canceled => STATUS_CANCELED,
        }
    }

    pub fn is_closed(self) -> bool {
        matches!(self, Self::Completed | Self::Canceled)
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        match self {
            Self::Draft => matches!(target, Self::Released | Self::Canceled),
            Self::Released => matches!(target, Self::InProgress | Self::Canceled),
            Self::InProgress => matches!(target, Self::Completed | Self::Canceled),
            Self::Completed | Self::Canceled => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineDirection {
    Consume,
    Produce,
}

impl LineDirection {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            DIRECTION_CONSUME => Some(Self::Consume),
            DIRECTION_PRODUCE => Some(Self::Produce),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Consume => DIRECTION_CONSUME,
            Self::Produce => DIRECTION_PRODUCE,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineDisposition {
    Normal,
    Scrap,
}

impl LineDisposition {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            DISPOSITION_NORMAL => Some(Self::Normal),
            DISPOSITION_SCRAP => Some(Self::Scrap),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normal => DISPOSITION_NORMAL,
            Self::Scrap => DISPOSITION_SCRAP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Condition,
    Quantity,
    Delivery,
}

impl FindingCategory {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            CATEGORY_CONDITION => Some(Self::Condition),
            CATEGORY_QUANTITY => Some(Self::Quantity),
            CATEGORY_DELIVERY => Some(Self::Delivery),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Condition => CATEGORY_CONDITION,
            Self::Quantity => CATEGORY_QUANTITY,
            Self::Delivery => CATEGORY_DELIVERY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingDisposition {
    Accepted,
    Partial,
    Rejected,
    Lost,
    Damaged,
}

impl FindingDisposition {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            DISPOSITION_ACCEPTED => Some(Self::Accepted),
            DISPOSITION_PARTIAL => Some(Self::Partial),
            DISPOSITION_REJECTED => Some(Self::Rejected),
            DISPOSITION_LOST => Some(Self::Lost),
            DISPOSITION_DAMAGED => Some(Self::Damaged),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => DISPOSITION_ACCEPTED,
            Self::Partial => DISPOSITION_PARTIAL,
            Self::Rejected => DISPOSITION_REJECTED,
            Self::Lost => DISPOSITION_LOST,
            Self::Damaged => DISPOSITION_DAMAGED,
        }
    }

    pub fn is_exception(self) -> bool {
        matches!(
            self,
            Self::Partial | Self::Rejected | Self::Lost | Self::Damaged
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyEvent {
    Received,
    Stored,
    Picked,
    Packed,
    Dispatched,
    Delivered,
    Returned,
}

impl CustodyEvent {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            CUSTODY_RECEIVED => Some(Self::Received),
            CUSTODY_STORED => Some(Self::Stored),
            CUSTODY_PICKED => Some(Self::Picked),
            CUSTODY_PACKED => Some(Self::Packed),
            CUSTODY_DISPATCHED => Some(Self::Dispatched),
            CUSTODY_DELIVERED => Some(Self::Delivered),
            CUSTODY_RETURNED => Some(Self::Returned),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => CUSTODY_RECEIVED,
            Self::Stored => CUSTODY_STORED,
            Self::Picked => CUSTODY_PICKED,
            Self::Packed => CUSTODY_PACKED,
            Self::Dispatched => CUSTODY_DISPATCHED,
            Self::Delivered => CUSTODY_DELIVERED,
            Self::Returned => CUSTODY_RETURNED,
        }
    }

    pub fn stage(self) -> Option<i32> {
        match self {
            Self::Received => Some(0),
            Self::Stored => Some(1),
            Self::Picked => Some(2),
            Self::Packed => Some(3),
            Self::Dispatched => Some(4),
            Self::Delivered => Some(5),
            Self::Returned => None,
        }
    }

    pub fn is_dispatch_stage(self) -> bool {
        matches!(self, Self::Dispatched | Self::Delivered | Self::Returned)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub tenant_id: String,
    pub job_code: String,
    pub kind: JobKind,
    pub title: String,
    pub owner_ref: Option<String>,
    pub status: JobStatus,
    pub consumed_count: i32,
    pub produced_count: i32,
    pub scrap_count: i32,
    pub unreconciled_count: i32,
    pub revision: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLine {
    pub id: String,
    pub tenant_id: String,
    pub job_id: String,
    pub direction: LineDirection,
    pub item_ref: String,
    pub quantity: i32,
    pub disposition: LineDisposition,
    pub evidence_ref: Option<String>,
    pub recorded_by: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub tenant_id: String,
    pub job_id: String,
    pub category: FindingCategory,
    pub disposition: FindingDisposition,
    pub quantity: Option<i32>,
    pub reason: String,
    pub evidence_ref: Option<String>,
    pub reconciles_id: Option<String>,
    pub recorded_by: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CustodyRecord {
    pub id: String,
    pub tenant_id: String,
    pub job_id: String,
    pub event: CustodyEvent,
    pub from_ref: Option<String>,
    pub to_ref: Option<String>,
    pub evidence_ref: Option<String>,
    pub note: Option<String>,
    pub recorded_by: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobDetail {
    pub job: Job,
    pub lines: Vec<JobLine>,
    pub findings: Vec<Finding>,
    pub custody: Vec<CustodyRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FulfillmentMutation {
    CreateJob {
        command_id: String,
        job_code: String,
        kind: JobKind,
        title: String,
        owner_ref: Option<String>,
    },
    RecordLine {
        command_id: String,
        job_id: String,
        direction: LineDirection,
        item_ref: String,
        quantity: i32,
        disposition: LineDisposition,
        evidence_ref: Option<String>,
    },
    RecordFinding {
        command_id: String,
        job_id: String,
        category: FindingCategory,
        disposition: FindingDisposition,
        quantity: Option<i32>,
        reason: String,
        evidence_ref: Option<String>,
        reconciles_id: Option<String>,
    },
    RecordCustody {
        command_id: String,
        job_id: String,
        event: CustodyEvent,
        from_ref: Option<String>,
        to_ref: Option<String>,
        evidence_ref: Option<String>,
        note: Option<String>,
    },
    ChangeJobStatus {
        command_id: String,
        job_id: String,
        expected_revision: i32,
        target_status: JobStatus,
        reason: String,
    },
}

impl FulfillmentMutation {
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateJob { command_id, .. } => command_id,
            Self::RecordLine { command_id, .. } => command_id,
            Self::RecordFinding { command_id, .. } => command_id,
            Self::RecordCustody { command_id, .. } => command_id,
            Self::ChangeJobStatus { command_id, .. } => command_id,
        }
    }

    pub fn action(&self) -> &'static str {
        match *self {
            Self::CreateJob { .. } => "create_job",
            Self::RecordLine { .. } => "record_line",
            Self::RecordFinding { .. } => "record_finding",
            Self::RecordCustody { .. } => "record_custody",
            Self::ChangeJobStatus { .. } => "change_job_status",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FulfillmentReceipt {
    pub command_id: String,
    pub tenant_id: String,
    pub action: String,
    pub job_id: Option<String>,
    pub line_id: Option<String>,
    pub finding_id: Option<String>,
    pub custody_id: Option<String>,
    pub status: String,
    pub revision: Option<i32>,
    pub idempotency_replay: bool,
    pub executed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum FulfillmentCommandStatus {
    Unknown,
    Recorded { receipt: Box<FulfillmentReceipt> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobPageQuery {
    pub limit: i64,
    pub offset: i64,
    pub status: Option<JobStatus>,
    pub kind: Option<JobKind>,
}

pub fn parse_mutation(raw: &Value) -> Result<FulfillmentMutation, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("mutation"))?;
    let mutation_type = object
        .get("type")
        .and_then(|v| v.as_str())
        .ok_or(ValidationError("type"))?;

    let command_id = object
        .get("commandId")
        .and_then(|v| v.as_str())
        .filter(|v| valid_uuid(v))
        .ok_or(ValidationError("commandId"))?
        .to_string();

    match mutation_type {
        "create_job" => {
            let job_code = object
                .get("jobCode")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, JOB_CODE_MAX))
                .ok_or(ValidationError("jobCode"))?;

            let kind_str = object
                .get("kind")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("kind"))?;
            let kind = JobKind::parse(kind_str).ok_or(ValidationError("kind"))?;

            let title = object
                .get("title")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, TITLE_MAX))
                .ok_or(ValidationError("title"))?;

            let owner_ref = match object.get("ownerRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > OWNER_REF_MAX {
                        return Err(ValidationError("ownerRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("ownerRef")),
            };

            Ok(FulfillmentMutation::CreateJob {
                command_id,
                job_code,
                kind,
                title,
                owner_ref,
            })
        }

        "record_line" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("jobId"))?
                .to_string();

            let direction_str = object
                .get("direction")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("direction"))?;
            let direction =
                LineDirection::parse(direction_str).ok_or(ValidationError("direction"))?;

            let item_ref = object
                .get("itemRef")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ITEM_REF_MAX))
                .ok_or(ValidationError("itemRef"))?;

            let quantity = object
                .get("quantity")
                .and_then(|v| v.as_i64())
                .filter(|&q| q > 0 && q <= i32::MAX as i64)
                .map(|q| q as i32)
                .ok_or(ValidationError("quantity"))?;

            let disposition = match object.get("disposition") {
                Some(Value::String(s)) => {
                    LineDisposition::parse(s).ok_or(ValidationError("disposition"))?
                }
                Some(Value::Null) | None => LineDisposition::Normal,
                _ => return Err(ValidationError("disposition")),
            };

            let evidence_ref = match object.get("evidenceRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > EVIDENCE_REF_MAX {
                        return Err(ValidationError("evidenceRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("evidenceRef")),
            };

            Ok(FulfillmentMutation::RecordLine {
                command_id,
                job_id,
                direction,
                item_ref,
                quantity,
                disposition,
                evidence_ref,
            })
        }

        "record_finding" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("jobId"))?
                .to_string();

            let category_str = object
                .get("category")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("category"))?;
            let category =
                FindingCategory::parse(category_str).ok_or(ValidationError("category"))?;

            let disposition_str = object
                .get("disposition")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("disposition"))?;
            let disposition =
                FindingDisposition::parse(disposition_str).ok_or(ValidationError("disposition"))?;

            let quantity = match object.get("quantity") {
                Some(Value::Null) | None => None,
                Some(Value::Number(n)) => {
                    let q = n.as_i64().filter(|&v| v > 0 && v <= i32::MAX as i64);
                    match q {
                        Some(v) => Some(v as i32),
                        None => return Err(ValidationError("quantity")),
                    }
                }
                _ => return Err(ValidationError("quantity")),
            };

            let reason = object
                .get("reason")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, REASON_MAX))
                .ok_or(ValidationError("reason"))?;

            let evidence_ref = match object.get("evidenceRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > EVIDENCE_REF_MAX {
                        return Err(ValidationError("evidenceRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("evidenceRef")),
            };

            let reconciles_id = match object.get("reconcilesId") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) if valid_uuid(s) => Some(s.to_string()),
                Some(Value::String(_)) => return Err(ValidationError("reconcilesId")),
                _ => return Err(ValidationError("reconcilesId")),
            };

            if reconciles_id.is_some() && disposition != FindingDisposition::Accepted {
                return Err(ValidationError("reconcilesId"));
            }

            Ok(FulfillmentMutation::RecordFinding {
                command_id,
                job_id,
                category,
                disposition,
                quantity,
                reason,
                evidence_ref,
                reconciles_id,
            })
        }

        "record_custody" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("jobId"))?
                .to_string();

            let event_str = object
                .get("event")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("event"))?;
            let event = CustodyEvent::parse(event_str).ok_or(ValidationError("event"))?;

            let from_ref = match object.get("fromRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > OWNER_REF_MAX {
                        return Err(ValidationError("fromRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("fromRef")),
            };

            let to_ref = match object.get("toRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > OWNER_REF_MAX {
                        return Err(ValidationError("toRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("toRef")),
            };

            let evidence_ref = match object.get("evidenceRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > EVIDENCE_REF_MAX {
                        return Err(ValidationError("evidenceRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("evidenceRef")),
            };

            let note = match object.get("note") {
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

            Ok(FulfillmentMutation::RecordCustody {
                command_id,
                job_id,
                event,
                from_ref,
                to_ref,
                evidence_ref,
                note,
            })
        }

        "change_job_status" => {
            let job_id = object
                .get("jobId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("jobId"))?
                .to_string();

            let expected_revision = object
                .get("expectedRevision")
                .and_then(|v| v.as_i64())
                .filter(|&r| r >= 0 && r <= i32::MAX as i64)
                .map(|r| r as i32)
                .ok_or(ValidationError("expectedRevision"))?;

            let target_str = object
                .get("targetStatus")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("targetStatus"))?;
            let target_status =
                JobStatus::parse(target_str).ok_or(ValidationError("targetStatus"))?;

            let reason = object
                .get("reason")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, REASON_MAX))
                .ok_or(ValidationError("reason"))?;

            Ok(FulfillmentMutation::ChangeJobStatus {
                command_id,
                job_id,
                expected_revision,
                target_status,
                reason,
            })
        }

        _ => Err(ValidationError("type")),
    }
}

pub fn parse_page_query(
    limit_param: Option<i64>,
    offset_param: Option<i64>,
    status_param: Option<&str>,
    kind_param: Option<&str>,
) -> Result<JobPageQuery, ValidationError> {
    let limit = match limit_param {
        Some(l) if (1..=PAGE_MAX_LIMIT).contains(&l) => l,
        Some(_) => return Err(ValidationError("limit")),
        None => PAGE_DEFAULT_LIMIT,
    };

    let offset = match offset_param {
        Some(o) if o >= 0 => o,
        Some(_) => return Err(ValidationError("offset")),
        None => 0,
    };

    let status = match status_param {
        Some(s) if !s.trim().is_empty() => {
            Some(JobStatus::parse(s.trim()).ok_or(ValidationError("status"))?)
        }
        _ => None,
    };

    let kind = match kind_param {
        Some(k) if !k.trim().is_empty() => {
            Some(JobKind::parse(k.trim()).ok_or(ValidationError("kind"))?)
        }
        _ => None,
    };

    Ok(JobPageQuery {
        limit,
        offset,
        status,
        kind,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_job_status_lifecycle() {
        assert!(JobStatus::Draft.can_transition_to(JobStatus::Released));
        assert!(JobStatus::Draft.can_transition_to(JobStatus::Canceled));
        assert!(!JobStatus::Draft.can_transition_to(JobStatus::InProgress));
        assert!(!JobStatus::Draft.can_transition_to(JobStatus::Completed));

        assert!(JobStatus::Released.can_transition_to(JobStatus::InProgress));
        assert!(JobStatus::Released.can_transition_to(JobStatus::Canceled));
        assert!(!JobStatus::Released.can_transition_to(JobStatus::Completed));

        assert!(JobStatus::InProgress.can_transition_to(JobStatus::Completed));
        assert!(JobStatus::InProgress.can_transition_to(JobStatus::Canceled));

        assert!(JobStatus::Completed.is_closed());
        assert!(JobStatus::Canceled.is_closed());
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Draft));
    }

    #[test]
    fn test_custody_stages() {
        assert_eq!(CustodyEvent::Received.stage(), Some(0));
        assert_eq!(CustodyEvent::Stored.stage(), Some(1));
        assert_eq!(CustodyEvent::Picked.stage(), Some(2));
        assert_eq!(CustodyEvent::Packed.stage(), Some(3));
        assert_eq!(CustodyEvent::Dispatched.stage(), Some(4));
        assert_eq!(CustodyEvent::Delivered.stage(), Some(5));
        assert_eq!(CustodyEvent::Returned.stage(), None);

        assert!(CustodyEvent::Dispatched.is_dispatch_stage());
        assert!(CustodyEvent::Delivered.is_dispatch_stage());
        assert!(CustodyEvent::Returned.is_dispatch_stage());
        assert!(!CustodyEvent::Received.is_dispatch_stage());
    }

    #[test]
    fn test_finding_disposition_exception() {
        assert!(!FindingDisposition::Accepted.is_exception());
        assert!(FindingDisposition::Partial.is_exception());
        assert!(FindingDisposition::Rejected.is_exception());
        assert!(FindingDisposition::Lost.is_exception());
        assert!(FindingDisposition::Damaged.is_exception());
    }

    #[test]
    fn test_mutation_parsing() {
        let valid_create = json!({
            "type": "create_job",
            "commandId": "00000000-0000-0000-0000-000000000001",
            "jobCode": "FUL-2026-001",
            "kind": "dispatch",
            "title": "Dispatch electronic components to Site B"
        });
        let parsed = parse_mutation(&valid_create).unwrap();
        assert_eq!(parsed.action(), "create_job");

        // Reconciles_id requires accepted disposition
        let bad_finding = json!({
            "type": "record_finding",
            "commandId": "00000000-0000-0000-0000-000000000002",
            "jobId": "00000000-0000-0000-0000-000000000010",
            "category": "condition",
            "disposition": "damaged",
            "reason": "Water damage",
            "reconcilesId": "00000000-0000-0000-0000-000000000020"
        });
        assert!(parse_mutation(&bad_finding).is_err());
    }
}
