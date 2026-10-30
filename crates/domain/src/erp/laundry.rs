//! The laundry domain — the Archxy 2 port of `packages/laundry/src/*`:
//! the laundry intake and processing lifecycle (weight-based vs garment item intake,
//! sequential processing events, quality findings with reconciliation exception gate,
//! and immutable audit/commands history).
//!
//! ## What this port keeps from the baseline
//!
//! - All four mutation arms (`create_order`, `record_process`, `record_finding`,
//!   `change_order_status`).
//! - Closed vocabularies for order kinds (`weight`, `item`), order statuses
//!   (`received`, `processing`, `ready`, `returned`, `canceled`), line units
//!   (`piece`, `gram`), process events (intake, tag, wash, dry, press, quality_check,
//!   ready, handover), finding categories (`stain`, `damage`, `missing`, `count`),
//!   and finding dispositions (`accepted`, `partial`, `rejected`).
//! - Processing sequence ordering: processing chain must begin with `intake` and
//!   each subsequent event must strictly advance in stage index.
//! - Machine events (`wash`, `dry`, `press`) require dedicated `laundry.process` permission.
//! - Strict quality gate: moving to `ready` is forbidden if `unreconciled_count > 0`
//!   or if no `quality_check` process event was recorded.
//! - Quality exception reconciliation: findings with `partial` or `rejected` increment
//!   unreconciled count, while a finding referencing `reconciles_id` decrements it.
//! - Wire contracts: receipts, detail projection, keyset/offset list queries.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::purchasing::ValidationError;
use super::{bounded_title, trimmed, valid_uuid, ErpScope};

pub const LAUNDRY_PERMISSIONS: &[&str] = &[
    "laundry.read",
    "laundry.write",
    "laundry.process",
    "audit.read",
];

// Order Kinds
pub const KIND_WEIGHT: &str = "weight";
pub const KIND_ITEM: &str = "item";

// Order Statuses
pub const STATUS_RECEIVED: &str = "received";
pub const STATUS_PROCESSING: &str = "processing";
pub const STATUS_READY: &str = "ready";
pub const STATUS_RETURNED: &str = "returned";
pub const STATUS_CANCELED: &str = "canceled";

// Line Units
pub const UNIT_PIECE: &str = "piece";
pub const UNIT_GRAM: &str = "gram";

// Process Events
pub const EVENT_INTAKE: &str = "intake";
pub const EVENT_TAG: &str = "tag";
pub const EVENT_WASH: &str = "wash";
pub const EVENT_DRY: &str = "dry";
pub const EVENT_PRESS: &str = "press";
pub const EVENT_QUALITY_CHECK: &str = "quality_check";
pub const EVENT_READY: &str = "ready";
pub const EVENT_HANDOVER: &str = "handover";

// Finding Categories
pub const CATEGORY_STAIN: &str = "stain";
pub const CATEGORY_DAMAGE: &str = "damage";
pub const CATEGORY_MISSING: &str = "missing";
pub const CATEGORY_COUNT: &str = "count";

// Finding Dispositions
pub const DISPOSITION_ACCEPTED: &str = "accepted";
pub const DISPOSITION_PARTIAL: &str = "partial";
pub const DISPOSITION_REJECTED: &str = "rejected";

// Field bounds
pub const ORDER_CODE_MAX: usize = 50;
pub const CUSTOMER_REF_MAX: usize = 80;
pub const ITEM_REF_MAX: usize = 80;
pub const NOTE_MAX: usize = 500;
pub const REASON_MAX: usize = 500;
pub const EVIDENCE_REF_MAX: usize = 300;
pub const MAX_LINES_PER_ORDER: usize = 50;
pub const PAGE_DEFAULT_LIMIT: i64 = 50;
pub const PAGE_MAX_LIMIT: i64 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderKind {
    Weight,
    Item,
}

impl OrderKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            KIND_WEIGHT => Some(Self::Weight),
            KIND_ITEM => Some(Self::Item),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Weight => KIND_WEIGHT,
            Self::Item => KIND_ITEM,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OrderStatus {
    Received,
    Processing,
    Ready,
    Returned,
    Canceled,
}

impl OrderStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATUS_RECEIVED => Some(Self::Received),
            STATUS_PROCESSING => Some(Self::Processing),
            STATUS_READY => Some(Self::Ready),
            STATUS_RETURNED => Some(Self::Returned),
            STATUS_CANCELED => Some(Self::Canceled),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Received => STATUS_RECEIVED,
            Self::Processing => STATUS_PROCESSING,
            Self::Ready => STATUS_READY,
            Self::Returned => STATUS_RETURNED,
            Self::Canceled => STATUS_CANCELED,
        }
    }

    pub fn is_open(self) -> bool {
        !matches!(self, Self::Returned | Self::Canceled)
    }

    pub fn can_transition_to(self, target: Self) -> bool {
        match self {
            Self::Received => matches!(target, Self::Processing | Self::Canceled),
            Self::Processing => matches!(target, Self::Ready | Self::Canceled),
            Self::Ready => matches!(target, Self::Returned),
            Self::Returned | Self::Canceled => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineUnit {
    Piece,
    Gram,
}

impl LineUnit {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            UNIT_PIECE => Some(Self::Piece),
            UNIT_GRAM => Some(Self::Gram),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Piece => UNIT_PIECE,
            Self::Gram => UNIT_GRAM,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessEvent {
    Intake,
    Tag,
    Wash,
    Dry,
    Press,
    QualityCheck,
    Ready,
    Handover,
}

impl ProcessEvent {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            EVENT_INTAKE => Some(Self::Intake),
            EVENT_TAG => Some(Self::Tag),
            EVENT_WASH => Some(Self::Wash),
            EVENT_DRY => Some(Self::Dry),
            EVENT_PRESS => Some(Self::Press),
            EVENT_QUALITY_CHECK => Some(Self::QualityCheck),
            EVENT_READY => Some(Self::Ready),
            EVENT_HANDOVER => Some(Self::Handover),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Intake => EVENT_INTAKE,
            Self::Tag => EVENT_TAG,
            Self::Wash => EVENT_WASH,
            Self::Dry => EVENT_DRY,
            Self::Press => EVENT_PRESS,
            Self::QualityCheck => EVENT_QUALITY_CHECK,
            Self::Ready => EVENT_READY,
            Self::Handover => EVENT_HANDOVER,
        }
    }

    pub fn stage(self) -> i32 {
        match self {
            Self::Intake => 0,
            Self::Tag => 1,
            Self::Wash => 2,
            Self::Dry => 3,
            Self::Press => 4,
            Self::QualityCheck => 5,
            Self::Ready => 6,
            Self::Handover => 7,
        }
    }

    pub fn is_machine(self) -> bool {
        matches!(self, Self::Wash | Self::Dry | Self::Press)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingCategory {
    Stain,
    Damage,
    Missing,
    Count,
}

impl FindingCategory {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            CATEGORY_STAIN => Some(Self::Stain),
            CATEGORY_DAMAGE => Some(Self::Damage),
            CATEGORY_MISSING => Some(Self::Missing),
            CATEGORY_COUNT => Some(Self::Count),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stain => CATEGORY_STAIN,
            Self::Damage => CATEGORY_DAMAGE,
            Self::Missing => CATEGORY_MISSING,
            Self::Count => CATEGORY_COUNT,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FindingDisposition {
    Accepted,
    Partial,
    Rejected,
}

impl FindingDisposition {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            DISPOSITION_ACCEPTED => Some(Self::Accepted),
            DISPOSITION_PARTIAL => Some(Self::Partial),
            DISPOSITION_REJECTED => Some(Self::Rejected),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Accepted => DISPOSITION_ACCEPTED,
            Self::Partial => DISPOSITION_PARTIAL,
            Self::Rejected => DISPOSITION_REJECTED,
        }
    }

    pub fn is_exception(self) -> bool {
        matches!(self, Self::Partial | Self::Rejected)
    }
}

/// Validates ISO 4217 3-letter currency code (e.g. "USD", "IDR").
pub fn valid_currency(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 3 && bytes.iter().all(|b| b.is_ascii_uppercase())
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineInput {
    pub item_ref: String,
    pub quantity: i32,
    pub unit: LineUnit,
    pub unit_price_minor: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaundryMutation {
    CreateOrder {
        command_id: String,
        order_code: String,
        kind: OrderKind,
        customer_ref: Option<String>,
        currency: String,
        lines: Vec<LineInput>,
    },
    RecordProcess {
        command_id: String,
        order_id: String,
        event: ProcessEvent,
        note: Option<String>,
        evidence_ref: Option<String>,
    },
    RecordFinding {
        command_id: String,
        order_id: String,
        category: FindingCategory,
        disposition: FindingDisposition,
        quantity: Option<i32>,
        reason: String,
        evidence_ref: Option<String>,
        reconciles_id: Option<String>,
    },
    ChangeOrderStatus {
        command_id: String,
        order_id: String,
        expected_revision: i32,
        target_status: OrderStatus,
        reason: String,
    },
}

impl LaundryMutation {
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateOrder { command_id, .. } => command_id,
            Self::RecordProcess { command_id, .. } => command_id,
            Self::RecordFinding { command_id, .. } => command_id,
            Self::ChangeOrderStatus { command_id, .. } => command_id,
        }
    }

    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::RecordProcess { event, .. } if event.is_machine() => "laundry.process",
            _ => "laundry.write",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaundryReceipt {
    pub command_id: String,
    pub tenant_id: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finding_id: Option<String>,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<i32>,
    pub idempotency_replay: bool,
    pub executed_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LaundryCommandStatus {
    Unknown,
    Recorded { receipt: Box<LaundryReceipt> },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaundryAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaundryOrder {
    pub id: String,
    pub tenant_id: String,
    pub order_code: String,
    pub kind: OrderKind,
    pub customer_ref: Option<String>,
    pub status: OrderStatus,
    pub currency: String,
    pub total_minor: i32,
    pub item_count: i32,
    pub weight_grams: i32,
    pub unreconciled_count: i32,
    pub revision: i32,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderLine {
    pub id: String,
    pub tenant_id: String,
    pub order_id: String,
    pub item_ref: String,
    pub quantity: i32,
    pub unit: LineUnit,
    pub unit_price_minor: i32,
    pub line_total_minor: i32,
    pub recorded_by: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessRecord {
    pub id: String,
    pub tenant_id: String,
    pub order_id: String,
    pub event: ProcessEvent,
    pub note: Option<String>,
    pub evidence_ref: Option<String>,
    pub recorded_by: String,
    pub recorded_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub id: String,
    pub tenant_id: String,
    pub order_id: String,
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
pub struct OrderDetail {
    pub order: LaundryOrder,
    pub lines: Vec<OrderLine>,
    pub process: Vec<ProcessRecord>,
    pub findings: Vec<Finding>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderPageQuery {
    pub limit: i64,
    pub offset: i64,
    pub status: Option<OrderStatus>,
    pub kind: Option<OrderKind>,
}

// --- Strict Parsers ---

pub fn parse_mutation(raw: &Value) -> Result<LaundryMutation, ValidationError> {
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
        "create_order" => {
            let order_code = object
                .get("orderCode")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, ORDER_CODE_MAX))
                .ok_or(ValidationError("orderCode"))?;

            let kind_str = object
                .get("kind")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("kind"))?;
            let kind = OrderKind::parse(kind_str).ok_or(ValidationError("kind"))?;

            let customer_ref = match object.get("customerRef") {
                Some(Value::Null) | None => None,
                Some(Value::String(s)) => {
                    let trimmed_s = trimmed(s);
                    if trimmed_s.is_empty() {
                        None
                    } else if trimmed_s.len() > CUSTOMER_REF_MAX {
                        return Err(ValidationError("customerRef"));
                    } else {
                        Some(trimmed_s.to_string())
                    }
                }
                _ => return Err(ValidationError("customerRef")),
            };

            let currency = object
                .get("currency")
                .and_then(|v| v.as_str())
                .filter(|v| valid_currency(v))
                .ok_or(ValidationError("currency"))?
                .to_string();

            let lines_arr = object
                .get("lines")
                .and_then(|v| v.as_array())
                .ok_or(ValidationError("lines"))?;
            if lines_arr.is_empty() || lines_arr.len() > MAX_LINES_PER_ORDER {
                return Err(ValidationError("lines"));
            }

            let mut lines = Vec::with_capacity(lines_arr.len());
            for line_val in lines_arr {
                let line_obj = line_val.as_object().ok_or(ValidationError("lines"))?;
                let item_ref = line_obj
                    .get("itemRef")
                    .and_then(|v| v.as_str())
                    .and_then(|v| bounded_title(v, ITEM_REF_MAX))
                    .ok_or(ValidationError("itemRef"))?;

                let quantity = line_obj
                    .get("quantity")
                    .and_then(|v| v.as_i64())
                    .filter(|&q| q > 0 && q <= i32::MAX as i64)
                    .map(|q| q as i32)
                    .ok_or(ValidationError("quantity"))?;

                let unit_str = line_obj
                    .get("unit")
                    .and_then(|v| v.as_str())
                    .ok_or(ValidationError("unit"))?;
                let unit = LineUnit::parse(unit_str).ok_or(ValidationError("unit"))?;

                let unit_price_minor = line_obj
                    .get("unitPriceMinor")
                    .and_then(|v| v.as_i64())
                    .filter(|&p| p >= 0 && p <= i32::MAX as i64)
                    .map(|p| p as i32)
                    .ok_or(ValidationError("unitPriceMinor"))?;

                lines.push(LineInput {
                    item_ref,
                    quantity,
                    unit,
                    unit_price_minor,
                });
            }

            Ok(LaundryMutation::CreateOrder {
                command_id,
                order_code,
                kind,
                customer_ref,
                currency,
                lines,
            })
        }
        "record_process" => {
            let order_id = object
                .get("orderId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("orderId"))?
                .to_string();

            let event_str = object
                .get("event")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("event"))?;
            let event = ProcessEvent::parse(event_str).ok_or(ValidationError("event"))?;

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

            Ok(LaundryMutation::RecordProcess {
                command_id,
                order_id,
                event,
                note,
                evidence_ref,
            })
        }
        "record_finding" => {
            let order_id = object
                .get("orderId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("orderId"))?
                .to_string();

            let cat_str = object
                .get("category")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("category"))?;
            let category = FindingCategory::parse(cat_str).ok_or(ValidationError("category"))?;

            let disp_str = object
                .get("disposition")
                .and_then(|v| v.as_str())
                .ok_or(ValidationError("disposition"))?;
            let disposition =
                FindingDisposition::parse(disp_str).ok_or(ValidationError("disposition"))?;

            let quantity = match object.get("quantity") {
                Some(Value::Null) | None => None,
                Some(Value::Number(n)) => {
                    let val = n.as_i64().ok_or(ValidationError("quantity"))?;
                    if val <= 0 || val > i32::MAX as i64 {
                        return Err(ValidationError("quantity"));
                    }
                    Some(val as i32)
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

            Ok(LaundryMutation::RecordFinding {
                command_id,
                order_id,
                category,
                disposition,
                quantity,
                reason,
                evidence_ref,
                reconciles_id,
            })
        }
        "change_order_status" => {
            let order_id = object
                .get("orderId")
                .and_then(|v| v.as_str())
                .filter(|v| valid_uuid(v))
                .ok_or(ValidationError("orderId"))?
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
                OrderStatus::parse(target_str).ok_or(ValidationError("targetStatus"))?;

            let reason = object
                .get("reason")
                .and_then(|v| v.as_str())
                .and_then(|v| bounded_title(v, REASON_MAX))
                .ok_or(ValidationError("reason"))?;

            Ok(LaundryMutation::ChangeOrderStatus {
                command_id,
                order_id,
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
) -> Result<OrderPageQuery, ValidationError> {
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
            Some(OrderStatus::parse(s.trim()).ok_or(ValidationError("status"))?)
        }
        _ => None,
    };

    let kind = match kind_param {
        Some(k) if !k.trim().is_empty() => {
            Some(OrderKind::parse(k.trim()).ok_or(ValidationError("kind"))?)
        }
        _ => None,
    };

    Ok(OrderPageQuery {
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
    fn test_order_status_transitions() {
        assert!(OrderStatus::Received.can_transition_to(OrderStatus::Processing));
        assert!(OrderStatus::Received.can_transition_to(OrderStatus::Canceled));
        assert!(!OrderStatus::Received.can_transition_to(OrderStatus::Ready));
        assert!(!OrderStatus::Received.can_transition_to(OrderStatus::Returned));

        assert!(OrderStatus::Processing.can_transition_to(OrderStatus::Ready));
        assert!(OrderStatus::Processing.can_transition_to(OrderStatus::Canceled));
        assert!(!OrderStatus::Processing.can_transition_to(OrderStatus::Returned));

        assert!(OrderStatus::Ready.can_transition_to(OrderStatus::Returned));
        assert!(!OrderStatus::Ready.can_transition_to(OrderStatus::Canceled));
        assert!(!OrderStatus::Ready.can_transition_to(OrderStatus::Processing));

        assert!(!OrderStatus::Returned.can_transition_to(OrderStatus::Received));
        assert!(!OrderStatus::Canceled.can_transition_to(OrderStatus::Received));
    }

    #[test]
    fn test_process_event_stages() {
        assert_eq!(ProcessEvent::Intake.stage(), 0);
        assert_eq!(ProcessEvent::Tag.stage(), 1);
        assert_eq!(ProcessEvent::Wash.stage(), 2);
        assert_eq!(ProcessEvent::Dry.stage(), 3);
        assert_eq!(ProcessEvent::Press.stage(), 4);
        assert_eq!(ProcessEvent::QualityCheck.stage(), 5);
        assert_eq!(ProcessEvent::Ready.stage(), 6);
        assert_eq!(ProcessEvent::Handover.stage(), 7);

        assert!(ProcessEvent::Wash.is_machine());
        assert!(ProcessEvent::Dry.is_machine());
        assert!(ProcessEvent::Press.is_machine());
        assert!(!ProcessEvent::Intake.is_machine());
        assert!(!ProcessEvent::QualityCheck.is_machine());
    }

    #[test]
    fn test_finding_disposition_exception() {
        assert!(!FindingDisposition::Accepted.is_exception());
        assert!(FindingDisposition::Partial.is_exception());
        assert!(FindingDisposition::Rejected.is_exception());
    }

    #[test]
    fn test_mutation_parsing() {
        let valid_create = json!({
            "type": "create_order",
            "commandId": "00000000-0000-0000-0000-000000000001",
            "orderCode": "LND-2026-001",
            "kind": "weight",
            "currency": "USD",
            "lines": [
                {
                    "itemRef": "wash-and-fold-kg",
                    "quantity": 5000,
                    "unit": "gram",
                    "unitPriceMinor": 20
                }
            ]
        });
        let parsed = parse_mutation(&valid_create).unwrap();
        assert_eq!(parsed.command_id(), "00000000-0000-0000-0000-000000000001");
        assert_eq!(parsed.required_permission(), "laundry.write");

        let machine_process = json!({
            "type": "record_process",
            "commandId": "00000000-0000-0000-0000-000000000002",
            "orderId": "00000000-0000-0000-0000-000000000010",
            "event": "wash"
        });
        let parsed_proc = parse_mutation(&machine_process).unwrap();
        assert_eq!(parsed_proc.required_permission(), "laundry.process");
    }
}
