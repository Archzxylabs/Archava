//! The proposals domain contracts — the Archxy 2 port of
//! `packages/proposals/src/contracts.ts` (R04: proposals + scope versions +
//! change requests, the largest grammar of the cohort).
//!
//! ## The wire grammar
//!
//! `proposalMutationSchema` is a 14-arm discriminated union tagged by
//! `type` whose members carry `commandId` (NOT requestId) and FLAT fields.
//! Optional members ride zod `.default()` fills (`estimatedValue` → 0,
//! `deliverableType` → 'service', `reason` → null, deltas → 0), so absence
//! IS the default — the parse fills them exactly when absent and refuses
//! present-but-invalid. Scope items are strict `{title,description,
//! estimatedDays,estimatedValue,deliverableType}` drafts, 1..=100.
//!
//! ## The states
//!
//! Proposals walk `draft → submitted → accepted|rejected|withdrawn`
//! (`rejected → draft` is the sole backward move). Change requests walk
//! `draft → submitted → approved|rejected|withdrawn`. The pg kernel guards
//! with direct state checks (`scope_frozen` when an edit arm lands on a non-
//! draft proposal, `invalid_state` for a wrong arm state); the tables are
//! the contract record the adapters reproduce.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::purchasing::PurchasePrincipal as ProposalPrincipal;
use super::purchasing::ValidationError;
use super::purchasing::REVISION_MAX;
use super::{bounded_title, valid_principal_id, valid_uuid, ErpScope};

/// The proposals ledger's permission vocabulary — the schema CHECK's
/// 7-word array (`permissions <@ ARRAY[...]`).
pub const PROPOSAL_PERMISSIONS: &[&str] = &[
    "proposals.read",
    "proposals.write",
    "proposals.decide",
    "changerequests.read",
    "changerequests.write",
    "changerequests.decide",
    "audit.read",
];

/// `PROPOSAL_STATES` — the column CHECK's vocabulary.
pub const PROPOSAL_STATES: &[&str] = &["draft", "submitted", "accepted", "rejected", "withdrawn"];

/// `CHANGE_REQUEST_STATES`.
pub const CHANGE_REQUEST_STATES: &[&str] =
    &["draft", "submitted", "approved", "rejected", "withdrawn"];

/// `deliverableTypeSchema` — the scope item's deliverable kind.
pub const DELIVERABLE_TYPES: &[&str] = &["milestone", "service", "license", "contingency"];

/// The proposal state table (`PROPOSAL_TRANSITIONS`).
pub fn proposal_transition_allowed(from: &str, to: &str) -> bool {
    match from {
        "draft" => matches!(to, "submitted" | "withdrawn"),
        "submitted" => matches!(to, "accepted" | "rejected" | "withdrawn"),
        "rejected" => matches!(to, "draft"),
        _ => false,
    }
}

/// The change-request state table (`CHANGE_REQUEST_TRANSITIONS`).
pub fn change_request_transition_allowed(from: &str, to: &str) -> bool {
    match from {
        "draft" => matches!(to, "submitted" | "withdrawn"),
        "submitted" => matches!(to, "approved" | "rejected" | "withdrawn"),
        _ => false,
    }
}

/// The per-arm permission — `requiredProposalPermission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalKind {
    CreateProposal,
    UpdateProposal,
    SetScopeItems,
    SubmitProposal,
    AcceptProposal,
    RejectProposal,
    WithdrawProposal,
    LinkProject,
    CreateChangeRequest,
    UpdateChangeRequest,
    SubmitChangeRequest,
    ApproveChangeRequest,
    RejectChangeRequest,
    WithdrawChangeRequest,
}

impl ProposalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateProposal => "create_proposal",
            Self::UpdateProposal => "update_proposal",
            Self::SetScopeItems => "set_scope_items",
            Self::SubmitProposal => "submit_proposal",
            Self::AcceptProposal => "accept_proposal",
            Self::RejectProposal => "reject_proposal",
            Self::WithdrawProposal => "withdraw_proposal",
            Self::LinkProject => "link_project",
            Self::CreateChangeRequest => "create_change_request",
            Self::UpdateChangeRequest => "update_change_request",
            Self::SubmitChangeRequest => "submit_change_request",
            Self::ApproveChangeRequest => "approve_change_request",
            Self::RejectChangeRequest => "reject_change_request",
            Self::WithdrawChangeRequest => "withdraw_change_request",
        }
    }

    /// `requiredProposalPermission`'s switch.
    pub fn permission(self) -> &'static str {
        match self {
            Self::CreateProposal
            | Self::UpdateProposal
            | Self::SetScopeItems
            | Self::SubmitProposal
            | Self::WithdrawProposal
            | Self::LinkProject => "proposals.write",
            Self::AcceptProposal | Self::RejectProposal => "proposals.decide",
            Self::CreateChangeRequest
            | Self::UpdateChangeRequest
            | Self::SubmitChangeRequest
            | Self::WithdrawChangeRequest => "changerequests.write",
            Self::ApproveChangeRequest | Self::RejectChangeRequest => "changerequests.decide",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "create_proposal" => Self::CreateProposal,
            "update_proposal" => Self::UpdateProposal,
            "set_scope_items" => Self::SetScopeItems,
            "submit_proposal" => Self::SubmitProposal,
            "accept_proposal" => Self::AcceptProposal,
            "reject_proposal" => Self::RejectProposal,
            "withdraw_proposal" => Self::WithdrawProposal,
            "link_project" => Self::LinkProject,
            "create_change_request" => Self::CreateChangeRequest,
            "update_change_request" => Self::UpdateChangeRequest,
            "submit_change_request" => Self::SubmitChangeRequest,
            "approve_change_request" => Self::ApproveChangeRequest,
            "reject_change_request" => Self::RejectChangeRequest,
            "withdraw_change_request" => Self::WithdrawChangeRequest,
            _ => return None,
        })
    }
}

/// The refusal vocabulary `ProposalError` carries — API strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProposalRefusal {
    Forbidden,
    NotFound,
    Conflict,
    ScopeFrozen,
    InvalidState,
    ActiveChangeRequestExists,
    InvalidRequest,
}

impl ProposalRefusal {
    /// The wire spelling the API contract exposes.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::ScopeFrozen => "scope_frozen",
            Self::InvalidState => "invalid_state",
            Self::ActiveChangeRequestExists => "active_change_request_exists",
            Self::InvalidRequest => "invalid_request",
        }
    }
}

/// A resolved proposals-side access — the parsed grant plus the owner id
/// the `assigned` scope narrows to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl ProposalAccess {
    /// The `grant()` rule.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// The scope rule: `assigned` confines to the access's owner.
    pub fn sees(&self, owner_id: &str) -> bool {
        self.scope.covers_all() || self.owner_id == owner_id
    }
}

// --- Scope item drafts ------------------------------------------------------

/// One scope item draft — `scopeItemDraftInputSchema` with its default
/// fills. `estimatedValue` and `deliverableType` serialize ALWAYS (the
/// defaults), `estimatedDays` only when present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopeItemDraft {
    pub title: String,
    pub description: String,
    #[serde(rename = "estimatedDays", skip_serializing_if = "Option::is_none")]
    pub estimated_days: Option<i64>,
    #[serde(rename = "estimatedValue")]
    pub estimated_value: i64,
    #[serde(rename = "deliverableType")]
    pub deliverable_type: String,
}

impl ScopeItemDraft {
    fn parse(value: &Value) -> Result<Self, ValidationError> {
        let object = value.as_object().ok_or(ValidationError("scopeItems"))?;
        for key in object.keys() {
            if ![
                "title",
                "description",
                "estimatedDays",
                "estimatedValue",
                "deliverableType",
            ]
            .contains(&key.as_str())
            {
                return Err(ValidationError("scopeItems"));
            }
        }
        let title = object
            .get("title")
            .and_then(Value::as_str)
            .and_then(|raw| bounded_title(raw, 160))
            .ok_or(ValidationError("scopeItems"))?;
        let description = object
            .get("description")
            .and_then(Value::as_str)
            .and_then(|raw| bounded_title(raw, 2000))
            .ok_or(ValidationError("scopeItems"))?;
        let estimated_days = match object.get("estimatedDays") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_i64()
                    .filter(|number| *number >= 0 && *number <= REVISION_MAX)
                    .ok_or(ValidationError("scopeItems"))?,
            ),
        };
        let estimated_value = match object.get("estimatedValue") {
            None => 0,
            Some(value) => value
                .as_i64()
                .filter(|number| *number >= 0 && *number <= REVISION_MAX)
                .ok_or(ValidationError("scopeItems"))?,
        };
        let deliverable_type = match object.get("deliverableType") {
            None => "service",
            Some(value) => {
                let raw = value.as_str().ok_or(ValidationError("scopeItems"))?;
                if !DELIVERABLE_TYPES.contains(&raw) {
                    return Err(ValidationError("scopeItems"));
                }
                raw
            }
        }
        .to_string();
        Ok(Self {
            title,
            description,
            estimated_days,
            estimated_value,
            deliverable_type,
        })
    }
}

/// The scope item DRAFT array — 1..=100 members, each parsed item by item.
fn parse_scope_items(value: &Option<&Value>) -> Result<Vec<ScopeItemDraft>, ValidationError> {
    let value = value.ok_or(ValidationError("scopeItems"))?;
    let array = value.as_array().ok_or(ValidationError("scopeItems"))?;
    if array.is_empty() || array.len() > 100 {
        return Err(ValidationError("scopeItems"));
    }
    array.iter().map(ScopeItemDraft::parse).collect()
}

// --- The command union ------------------------------------------------------

/// One typed proposal mutation — the 14 arms of `proposalMutationSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum ProposalData {
    #[serde(rename = "create_proposal")]
    CreateProposal {
        #[serde(rename = "commandId")]
        command_id: String,
        title: String,
        summary: String,
        #[serde(rename = "organizationId")]
        organization_id: Value,
        #[serde(rename = "contactId")]
        contact_id: Value,
        #[serde(rename = "leadId")]
        lead_id: Value,
        currency: String,
        #[serde(rename = "scopeItems")]
        scope_items: Vec<ScopeItemDraft>,
    },
    #[serde(rename = "update_proposal")]
    UpdateProposal {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        title: String,
        summary: String,
        #[serde(rename = "organizationId", skip_serializing_if = "Option::is_none")]
        organization_id: Option<Value>,
        #[serde(rename = "contactId", skip_serializing_if = "Option::is_none")]
        contact_id: Option<Value>,
        #[serde(rename = "leadId", skip_serializing_if = "Option::is_none")]
        lead_id: Option<Value>,
    },
    #[serde(rename = "set_scope_items")]
    SetScopeItems {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "scopeItems")]
        scope_items: Vec<ScopeItemDraft>,
    },
    #[serde(rename = "submit_proposal")]
    SubmitProposal {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "accept_proposal")]
    AcceptProposal {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "linkedProjectId")]
        linked_project_id: Value,
    },
    #[serde(rename = "reject_proposal")]
    RejectProposal {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: Value,
    },
    #[serde(rename = "withdraw_proposal")]
    WithdrawProposal {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: Value,
    },
    #[serde(rename = "link_project")]
    LinkProject {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "projectId")]
        project_id: String,
    },
    #[serde(rename = "create_change_request")]
    CreateChangeRequest {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "proposalId")]
        proposal_id: String,
        title: String,
        reason: String,
        #[serde(rename = "deltaSummary")]
        delta_summary: String,
        #[serde(rename = "timeDeltaDays")]
        time_delta_days: i64,
        #[serde(rename = "costDeltaValue")]
        cost_delta_value: i64,
        #[serde(rename = "newScopeItems")]
        new_scope_items: Vec<ScopeItemDraft>,
    },
    #[serde(rename = "update_change_request")]
    UpdateChangeRequest {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "changeRequestId")]
        change_request_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        title: String,
        reason: String,
        #[serde(rename = "deltaSummary")]
        delta_summary: String,
        #[serde(rename = "timeDeltaDays")]
        time_delta_days: i64,
        #[serde(rename = "costDeltaValue")]
        cost_delta_value: i64,
        #[serde(rename = "newScopeItems")]
        new_scope_items: Vec<ScopeItemDraft>,
    },
    #[serde(rename = "submit_change_request")]
    SubmitChangeRequest {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "changeRequestId")]
        change_request_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "approve_change_request")]
    ApproveChangeRequest {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "changeRequestId")]
        change_request_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
    },
    #[serde(rename = "reject_change_request")]
    RejectChangeRequest {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "changeRequestId")]
        change_request_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: Value,
    },
    #[serde(rename = "withdraw_change_request")]
    WithdrawChangeRequest {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "changeRequestId")]
        change_request_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        reason: Value,
    },
}

impl ProposalData {
    /// The arm discriminator.
    pub fn kind(&self) -> ProposalKind {
        match self {
            Self::CreateProposal { .. } => ProposalKind::CreateProposal,
            Self::UpdateProposal { .. } => ProposalKind::UpdateProposal,
            Self::SetScopeItems { .. } => ProposalKind::SetScopeItems,
            Self::SubmitProposal { .. } => ProposalKind::SubmitProposal,
            Self::AcceptProposal { .. } => ProposalKind::AcceptProposal,
            Self::RejectProposal { .. } => ProposalKind::RejectProposal,
            Self::WithdrawProposal { .. } => ProposalKind::WithdrawProposal,
            Self::LinkProject { .. } => ProposalKind::LinkProject,
            Self::CreateChangeRequest { .. } => ProposalKind::CreateChangeRequest,
            Self::UpdateChangeRequest { .. } => ProposalKind::UpdateChangeRequest,
            Self::SubmitChangeRequest { .. } => ProposalKind::SubmitChangeRequest,
            Self::ApproveChangeRequest { .. } => ProposalKind::ApproveChangeRequest,
            Self::RejectChangeRequest { .. } => ProposalKind::RejectChangeRequest,
            Self::WithdrawChangeRequest { .. } => ProposalKind::WithdrawChangeRequest,
        }
    }

    /// The arm's command id (`commandId`), the idempotency key.
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateProposal { command_id, .. }
            | Self::UpdateProposal { command_id, .. }
            | Self::SetScopeItems { command_id, .. }
            | Self::SubmitProposal { command_id, .. }
            | Self::AcceptProposal { command_id, .. }
            | Self::RejectProposal { command_id, .. }
            | Self::WithdrawProposal { command_id, .. }
            | Self::LinkProject { command_id, .. }
            | Self::CreateChangeRequest { command_id, .. }
            | Self::UpdateChangeRequest { command_id, .. }
            | Self::SubmitChangeRequest { command_id, .. }
            | Self::ApproveChangeRequest { command_id, .. }
            | Self::RejectChangeRequest { command_id, .. }
            | Self::WithdrawChangeRequest { command_id, .. } => command_id,
        }
    }
}

/// A mutation ALREADY passed the strict parse — kept as a document for the
/// fingerprint digest and the adapter flow.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProposalMutation {
    pub data: ProposalData,
}

/// The slug grammar the tenant-ish string members (`contactId`, `leadId`)
/// share — `.nullable()` where present.
fn slug_member(value: &Value, field: &'static str) -> Result<Value, ValidationError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let raw = value.as_str().ok_or(ValidationError(field))?;
    if !valid_principal_id(raw) {
        return Err(ValidationError(field));
    }
    Ok(Value::String(raw.to_string()))
}

fn uuid_member(value: &Value, field: &'static str) -> Result<Value, ValidationError> {
    if matches!(value, Value::Null) {
        return Ok(Value::Null);
    }
    let raw = value.as_str().ok_or(ValidationError(field))?;
    if !valid_uuid(raw) {
        return Err(ValidationError(field));
    }
    Ok(Value::String(raw.to_string()))
}

/// `optionalText2000` — `.nullable()` with the default fill `null`.
fn reason_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Value, ValidationError> {
    match object.get(key) {
        None => Ok(Value::Null),
        Some(Value::Null) => Ok(Value::Null),
        Some(value) => {
            let raw = value.as_str().ok_or(ValidationError(key))?;
            bounded_title(raw, 2000)
                .map(Value::String)
                .ok_or(ValidationError(key))
        }
    }
}

fn int_delta_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    default: Option<i64>,
) -> Result<i64, ValidationError> {
    match object.get(key) {
        None => default.ok_or(ValidationError(key)),
        Some(value) => value
            .as_i64()
            .filter(|number| *number >= -2_147_483_647 && *number <= 2_147_483_647)
            .ok_or(ValidationError(key)),
    }
}

fn revision_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<i64, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_i64)
        .filter(|number| *number >= 1 && *number <= REVISION_MAX)
        .ok_or(ValidationError(key))
}

fn title_field(
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

/// The strict parse of one arm — member for member.
fn parse_arm(
    kind: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<ProposalData, ValidationError> {
    let arm = ProposalKind::parse(kind).ok_or(ValidationError("type"))?;
    Ok(match arm {
        ProposalKind::CreateProposal => {
            let organization_id = uuid_member(
                object.get("organizationId").unwrap_or(&Value::Null),
                "organizationId",
            )?;
            let contact_id =
                slug_member(object.get("contactId").unwrap_or(&Value::Null), "contactId")?;
            let lead_id = slug_member(object.get("leadId").unwrap_or(&Value::Null), "leadId")?;
            let currency = match object.get("currency") {
                None => "IDR".to_string(),
                Some(value) => {
                    let raw = value.as_str().ok_or(ValidationError("currency"))?;
                    let trimmed = raw.trim();
                    if trimmed.len() != 3 || !trimmed.chars().all(|c| c.is_ascii_uppercase()) {
                        return Err(ValidationError("currency"));
                    }
                    trimmed.to_string()
                }
            };
            let scope_items = parse_scope_items(&object.get("scopeItems"))?;
            ProposalData::CreateProposal {
                command_id: required_command_id(object)?,
                title: title_field(object, "title", 160)?,
                summary: title_field(object, "summary", 2000)?,
                organization_id,
                contact_id,
                lead_id,
                currency,
                scope_items,
            }
        }
        ProposalKind::UpdateProposal => {
            let organization_id = match object.get("organizationId") {
                None => None,
                Some(value) => Some(uuid_member(value, "organizationId")?),
            };
            let contact_id = match object.get("contactId") {
                None => None,
                Some(value) => Some(slug_member(value, "contactId")?),
            };
            let lead_id = match object.get("leadId") {
                None => None,
                Some(value) => Some(slug_member(value, "leadId")?),
            };
            ProposalData::UpdateProposal {
                command_id: required_command_id(object)?,
                proposal_id: required_uuid_target(object, "proposalId")?,
                expected_revision: revision_member(object, "expectedRevision")?,
                title: title_field(object, "title", 160)?,
                summary: title_field(object, "summary", 2000)?,
                organization_id,
                contact_id,
                lead_id,
            }
        }
        ProposalKind::SetScopeItems => ProposalData::SetScopeItems {
            command_id: required_command_id(object)?,
            proposal_id: required_uuid_target(object, "proposalId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
            scope_items: parse_scope_items(&object.get("scopeItems"))?,
        },
        ProposalKind::SubmitProposal => ProposalData::SubmitProposal {
            command_id: required_command_id(object)?,
            proposal_id: required_uuid_target(object, "proposalId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
        },
        ProposalKind::AcceptProposal => {
            let linked_project_id = uuid_member(
                object.get("linkedProjectId").unwrap_or(&Value::Null),
                "linkedProjectId",
            )?;
            ProposalData::AcceptProposal {
                command_id: required_command_id(object)?,
                proposal_id: required_uuid_target(object, "proposalId")?,
                expected_revision: revision_member(object, "expectedRevision")?,
                linked_project_id,
            }
        }
        ProposalKind::RejectProposal => ProposalData::RejectProposal {
            command_id: required_command_id(object)?,
            proposal_id: required_uuid_target(object, "proposalId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
            reason: reason_member(object, "reason")?,
        },
        ProposalKind::WithdrawProposal => ProposalData::WithdrawProposal {
            command_id: required_command_id(object)?,
            proposal_id: required_uuid_target(object, "proposalId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
            reason: reason_member(object, "reason")?,
        },
        ProposalKind::LinkProject => ProposalData::LinkProject {
            command_id: required_command_id(object)?,
            proposal_id: required_uuid_target(object, "proposalId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
            project_id: required_uuid_target(object, "projectId")?,
        },
        ProposalKind::CreateChangeRequest => {
            let new_scope_items = parse_scope_items(&object.get("newScopeItems"))?;
            ProposalData::CreateChangeRequest {
                command_id: required_command_id(object)?,
                proposal_id: required_uuid_target(object, "proposalId")?,
                title: title_field(object, "title", 160)?,
                reason: title_field(object, "reason", 2000)?,
                delta_summary: title_field(object, "deltaSummary", 2000)?,
                time_delta_days: int_delta_member(object, "timeDeltaDays", Some(0))?,
                cost_delta_value: int_delta_member(object, "costDeltaValue", Some(0))?,
                new_scope_items,
            }
        }
        ProposalKind::UpdateChangeRequest => {
            let new_scope_items = parse_scope_items(&object.get("newScopeItems"))?;
            ProposalData::UpdateChangeRequest {
                command_id: required_command_id(object)?,
                change_request_id: required_uuid_target(object, "changeRequestId")?,
                expected_revision: revision_member(object, "expectedRevision")?,
                title: title_field(object, "title", 160)?,
                reason: title_field(object, "reason", 2000)?,
                delta_summary: title_field(object, "deltaSummary", 2000)?,
                time_delta_days: int_delta_member(object, "timeDeltaDays", None)?,
                cost_delta_value: int_delta_member(object, "costDeltaValue", None)?,
                new_scope_items,
            }
        }
        ProposalKind::SubmitChangeRequest => ProposalData::SubmitChangeRequest {
            command_id: required_command_id(object)?,
            change_request_id: required_uuid_target(object, "changeRequestId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
        },
        ProposalKind::ApproveChangeRequest => ProposalData::ApproveChangeRequest {
            command_id: required_command_id(object)?,
            change_request_id: required_uuid_target(object, "changeRequestId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
        },
        ProposalKind::RejectChangeRequest => ProposalData::RejectChangeRequest {
            command_id: required_command_id(object)?,
            change_request_id: required_uuid_target(object, "changeRequestId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
            reason: reason_member(object, "reason")?,
        },
        ProposalKind::WithdrawChangeRequest => ProposalData::WithdrawChangeRequest {
            command_id: required_command_id(object)?,
            change_request_id: required_uuid_target(object, "changeRequestId")?,
            expected_revision: revision_member(object, "expectedRevision")?,
            reason: reason_member(object, "reason")?,
        },
    })
}

fn required_command_id(object: &serde_json::Map<String, Value>) -> Result<String, ValidationError> {
    object
        .get("commandId")
        .and_then(Value::as_str)
        .filter(|raw| valid_uuid(raw))
        .ok_or(ValidationError("commandId"))
        .map(str::to_string)
}

fn required_uuid_target(
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

/// The strict parse of the mutation — the discriminator unions it. The
/// `.strict()` shape check is the ALLOWED-KEY set per arm (absent members
/// that zod defaults would fill stay absent on the submitted wire, which is
/// what makes the default fills lossless instead of silent).
pub fn parse_mutation(raw: &Value) -> Result<ProposalMutation, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("type"))?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ValidationError("type"))?;
    ensure_allowed_members(kind, object)?;
    let arm = parse_arm(kind, object)?;
    Ok(ProposalMutation { data: arm })
}

/// The member set each arm's `.strict()` object declares.
fn arm_members(arm: ProposalKind) -> &'static [&'static str] {
    match arm {
        ProposalKind::CreateProposal => &[
            "type",
            "commandId",
            "title",
            "summary",
            "organizationId",
            "contactId",
            "leadId",
            "currency",
            "scopeItems",
        ],
        ProposalKind::UpdateProposal => &[
            "type",
            "commandId",
            "proposalId",
            "expectedRevision",
            "title",
            "summary",
            "organizationId",
            "contactId",
            "leadId",
        ],
        ProposalKind::SetScopeItems => &[
            "type",
            "commandId",
            "proposalId",
            "expectedRevision",
            "scopeItems",
        ],
        ProposalKind::SubmitProposal => &["type", "commandId", "proposalId", "expectedRevision"],
        ProposalKind::AcceptProposal => &[
            "type",
            "commandId",
            "proposalId",
            "expectedRevision",
            "linkedProjectId",
        ],
        ProposalKind::RejectProposal | ProposalKind::WithdrawProposal => &[
            "type",
            "commandId",
            "proposalId",
            "expectedRevision",
            "reason",
        ],
        ProposalKind::LinkProject => &[
            "type",
            "commandId",
            "proposalId",
            "expectedRevision",
            "projectId",
        ],
        ProposalKind::CreateChangeRequest => &[
            "type",
            "commandId",
            "proposalId",
            "title",
            "reason",
            "deltaSummary",
            "timeDeltaDays",
            "costDeltaValue",
            "newScopeItems",
        ],
        ProposalKind::UpdateChangeRequest => &[
            "type",
            "commandId",
            "changeRequestId",
            "expectedRevision",
            "title",
            "reason",
            "deltaSummary",
            "timeDeltaDays",
            "costDeltaValue",
            "newScopeItems",
        ],
        ProposalKind::SubmitChangeRequest | ProposalKind::ApproveChangeRequest => {
            &["type", "commandId", "changeRequestId", "expectedRevision"]
        }
        ProposalKind::RejectChangeRequest | ProposalKind::WithdrawChangeRequest => &[
            "type",
            "commandId",
            "changeRequestId",
            "expectedRevision",
            "reason",
        ],
    }
}

fn ensure_allowed_members(
    kind: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<(), ValidationError> {
    let arm = ProposalKind::parse(kind).ok_or(ValidationError("type"))?;
    let allowed = arm_members(arm);
    let unknown = object.keys().find(|key| !allowed.contains(&key.as_str()));
    match unknown {
        None => Ok(()),
        Some(_key) => Err(ValidationError("type")),
    }
}

/// The receipt the mutation records and recovery replays —
/// `commandReceiptSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposalReceipt {
    #[serde(rename = "commandId")]
    pub command_id: String,
    #[serde(rename = "committedAt")]
    pub committed_at: String,
    #[serde(rename = "proposalId", skip_serializing_if = "Option::is_none")]
    pub proposal_id: Option<String>,
    #[serde(rename = "changeRequestId", skip_serializing_if = "Option::is_none")]
    pub change_request_id: Option<String>,
    #[serde(rename = "scopeVersion", skip_serializing_if = "Option::is_none")]
    pub scope_version: Option<i64>,
    #[serde(rename = "revision")]
    pub revision: i64,
    #[serde(rename = "status")]
    pub status: String,
    /// Replay knowledge for the HOST decision — never serialized: the TS
    /// receipt schemas are `.strict()` and carry no such key, so the stored
    /// and served receipt documents stay exactly TS-shaped. The route's
    /// `decision` answers instead.
    #[serde(default, skip_serializing)]
    pub replayed: bool,
}

impl ProposalReceipt {
    /// The stored receipt's strict parse — the recovery replay path.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("commandId"))?;
        let command_id = object
            .get("commandId")
            .and_then(Value::as_str)
            .filter(|raw| valid_uuid(raw))
            .ok_or(ValidationError("commandId"))?
            .to_string();
        let committed_at = object
            .get("committedAt")
            .and_then(Value::as_str)
            .ok_or(ValidationError("committedAt"))?
            .to_string();
        let optional_uuid = |key: &'static str| -> Result<Option<String>, ValidationError> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(value) => {
                    let raw = value.as_str().ok_or(ValidationError(key))?;
                    if !valid_uuid(raw) {
                        return Err(ValidationError(key));
                    }
                    Ok(Some(raw.to_string()))
                }
            }
        };
        let proposal_id = optional_uuid("proposalId")?;
        let change_request_id = optional_uuid("changeRequestId")?;
        let scope_version = match object.get("scopeVersion") {
            None | Some(Value::Null) => None,
            Some(value) => Some(
                value
                    .as_i64()
                    .filter(|number| *number >= 1 && *number <= REVISION_MAX)
                    .ok_or(ValidationError("scopeVersion"))?,
            ),
        };
        let revision = object
            .get("revision")
            .and_then(Value::as_i64)
            .filter(|number| *number >= 1 && *number <= REVISION_MAX)
            .ok_or(ValidationError("revision"))?;
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .filter(|status| ["committed", "conflict"].contains(status))
            .ok_or(ValidationError("status"))?
            .to_string();
        Ok(Self {
            command_id,
            committed_at,
            proposal_id,
            change_request_id,
            scope_version,
            revision,
            status,
            replayed: false,
        })
    }
}

/// Builds the replayed receipt: the stored one with `replayed` flipped. A
/// recovery IS a replay by construction — the same class the purchasing and
/// foodservice ledgers serve recovery with.
pub fn replayed_receipt(receipt: &ProposalReceipt) -> ProposalReceipt {
    let mut replayed = receipt.clone();
    replayed.replayed = true;
    replayed
}

/// The command-status union — `commandStatusResultSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum ProposalCommandStatus {
    #[serde(rename = "committed")]
    Committed { receipt: ProposalReceipt },
    #[serde(rename = "unknown")]
    Unknown,
}

/// The typed-time the adapter stamps receipts with — an offset ISO-8601
/// instant string, exactly `instantSchema`.
pub fn instant_string(now: &std::time::SystemTime) -> String {
    // The host's clock helper ShapesRFC3339: seconds precision, Z suffix.
    let epoch = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = epoch / 86_400;
    let day_seconds = epoch % 86_400;
    let (year, month, day) = civil_from_days(days as i64);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        month,
        day,
        day_seconds / 3600,
        (day_seconds % 3600) / 60,
        day_seconds % 60
    )
}

/// Howard Hinnant's civil-from-days — the same formula the CRM clock uses.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (152 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn command_id(base: i64) -> String {
        format!("112e5843-c01b-4a3c-a68a-5a5f7a1d{base:04x}")
    }

    #[test]
    fn a_create_proposal_parses_with_its_default_fills() {
        let doc = json!({
            "type": "create_proposal",
            "commandId": command_id(1),
            "title": "Portal Survei",
            "summary": "Bangun portal survei warga",
            "scopeItems": [
                {"title": "Desain", "description": "Wireframe dan riset"}
            ]
        });
        let mutation = parse_mutation(&doc).expect("parses");
        match mutation.data {
            ProposalData::CreateProposal {
                organization_id,
                contact_id,
                lead_id,
                currency,
                scope_items,
                ..
            } => {
                assert_eq!(organization_id, Value::Null, "default fill null");
                assert_eq!(contact_id, Value::Null);
                assert_eq!(lead_id, Value::Null);
                assert_eq!(currency, "IDR");
                assert_eq!(scope_items.len(), 1);
                let only = &scope_items[0];
                assert_eq!(only.estimated_value, 0, "default fill zero");
                assert_eq!(only.estimated_days, None);
                assert_eq!(only.deliverable_type, "service");
            }
            other => panic!("wrong arm {other:?}"),
        }
    }

    #[test]
    fn the_reject_arm_fills_an_absent_reason_with_null() {
        let doc = json!({
            "type": "reject_proposal",
            "commandId": command_id(2),
            "proposalId": command_id(3),
            "expectedRevision": 3
        });
        let mutation = parse_mutation(&doc).expect("parses");
        match mutation.data {
            ProposalData::RejectProposal { reason, .. } => assert_eq!(reason, Value::Null),
            other => panic!("wrong arm {other:?}"),
        }
        let with = json!({
            "type": "reject_proposal",
            "commandId": command_id(2),
            "proposalId": command_id(3),
            "expectedRevision": 3,
            "reason": "Anggaran berubah"
        });
        match parse_mutation(&with).expect("parses").data {
            ProposalData::RejectProposal { reason, .. } => {
                assert_eq!(reason, Value::String("Anggaran berubah".into()))
            }
            other => panic!("wrong arm {other:?}"),
        }
    }

    #[test]
    fn scope_items_are_strict_and_bounded() {
        let doc = json!({
            "type": "set_scope_items",
            "commandId": command_id(4),
            "proposalId": command_id(5),
            "expectedRevision": 2,
            "scopeItems": [
                {"title": "A", "description": "B", "surprise": true}
            ]
        });
        assert!(parse_mutation(&doc).is_err(), "unknown member refused");
        let too_many = json!({
            "type": "set_scope_items",
            "commandId": command_id(4),
            "proposalId": command_id(5),
            "expectedRevision": 2,
            "scopeItems": []
        });
        assert!(parse_mutation(&too_many).is_err(), "empty scope refused");
    }

    #[test]
    fn the_permission_rule_is_the_baseline_switch() {
        assert_eq!(ProposalKind::CreateProposal.permission(), "proposals.write");
        assert_eq!(ProposalKind::LinkProject.permission(), "proposals.write");
        assert_eq!(
            ProposalKind::AcceptProposal.permission(),
            "proposals.decide"
        );
        assert_eq!(
            ProposalKind::CreateChangeRequest.permission(),
            "changerequests.write"
        );
        assert_eq!(
            ProposalKind::ApproveChangeRequest.permission(),
            "changerequests.decide"
        );
        assert_eq!(
            ProposalKind::WithdrawChangeRequest.permission(),
            "changerequests.write"
        );
    }

    #[test]
    fn the_state_tables_reproduce_the_frozen_transitions() {
        assert!(proposal_transition_allowed("draft", "submitted"));
        assert!(proposal_transition_allowed("rejected", "draft"));
        assert!(!proposal_transition_allowed("accepted", "withdrawn"));
        assert!(!proposal_transition_allowed("accepted", "draft"));
        assert!(change_request_transition_allowed("submitted", "approved"));
        assert!(!change_request_transition_allowed("approved", "draft"));
    }

    #[test]
    fn the_receipt_round_trips_and_refuses_strangers() {
        let receipt = json!({
            "commandId": command_id(1),
            "committedAt": "2026-01-02T00:00:00Z",
            "proposalId": command_id(3),
            "revision": 2,
            "status": "committed"
        });
        let parsed = ProposalReceipt::parse(&receipt).expect("parses");
        assert_eq!(parsed.scope_version, None);
        assert_eq!(serde_json::to_value(&parsed).expect("serialize"), receipt);
        let bad = json!({
            "commandId": command_id(1),
            "committedAt": "2026-01-02T00:00:00Z",
            "revision": 2,
            "status": "concluded"
        });
        assert!(
            ProposalReceipt::parse(&bad).is_err(),
            "unknown status refused"
        );
    }
}
