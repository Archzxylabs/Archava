//! Customer Relationship Management (CRM) domain contracts.
//!
//! Positioned as a first-class enterprise capability of the Archava ERP Monolith,
//! providing pure domain contracts for contact management, customer interaction
//! activities, task follow-up lifecycles, and pipeline opportunity stage transitions.

use serde::{Deserialize, Serialize};

// --- Constants & Permission Vocabulary -----------------------------------------

/// The permission names the CRM schema CHECK constrains.
pub struct Permission;

impl Permission {
    pub const CONTACTS_READ: &'static str = "contacts.read";
    pub const CONTACTS_WRITE: &'static str = "contacts.write";
    pub const ACTIVITIES_READ: &'static str = "activities.read";
    pub const ACTIVITIES_WRITE: &'static str = "activities.write";
    pub const FOLLOWUPS_READ: &'static str = "followups.read";
    pub const FOLLOWUPS_WRITE: &'static str = "followups.write";
    pub const PIPELINE_READ: &'static str = "pipeline.read";
    pub const PIPELINE_WRITE: &'static str = "pipeline.write";
    pub const AUDIT_READ: &'static str = "audit.read";
}

/// Every permission value the CRM schema's CHECK admits.
pub const PERMISSION_VOCABULARY: [&str; 9] = [
    Permission::CONTACTS_READ,
    Permission::CONTACTS_WRITE,
    Permission::ACTIVITIES_READ,
    Permission::ACTIVITIES_WRITE,
    Permission::FOLLOWUPS_READ,
    Permission::FOLLOWUPS_WRITE,
    Permission::PIPELINE_READ,
    Permission::PIPELINE_WRITE,
    Permission::AUDIT_READ,
];

// --- Domain types & Vocabularies -----------------------------------------------

/// One customer interaction activity kind (`note`, `call`, `meeting`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityKind {
    Note,
    Call,
    Meeting,
}

impl ActivityKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActivityKind::Note => "note",
            ActivityKind::Call => "call",
            ActivityKind::Meeting => "meeting",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "note" => Some(ActivityKind::Note),
            "call" => Some(ActivityKind::Call),
            "meeting" => Some(ActivityKind::Meeting),
            _ => None,
        }
    }
}

/// One pipeline stage in the opportunity sales funnel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PipelineStage {
    New,
    Qualified,
    Proposal,
    Won,
    Lost,
}

impl PipelineStage {
    pub fn as_str(self) -> &'static str {
        match self {
            PipelineStage::New => "new",
            PipelineStage::Qualified => "qualified",
            PipelineStage::Proposal => "proposal",
            PipelineStage::Won => "won",
            PipelineStage::Lost => "lost",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "new" => Some(PipelineStage::New),
            "qualified" => Some(PipelineStage::Qualified),
            "proposal" => Some(PipelineStage::Proposal),
            "won" => Some(PipelineStage::Won),
            "lost" => Some(PipelineStage::Lost),
            _ => None,
        }
    }
}

/// Validates whether a state transition between pipeline stages is permissible.
pub fn transition_allowed(from: PipelineStage, to: PipelineStage) -> bool {
    use PipelineStage::{Lost, New, Proposal, Qualified, Won};
    matches!(
        (from, to),
        (New, Qualified | Lost)
            | (Qualified, New | Proposal | Lost)
            | (Proposal, Qualified | Won | Lost)
    )
}

/// Contact creation payload data.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactFields {
    pub full_name: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub organization: Option<String>,
}

/// Contact patch payload data with tri-state values: absent (keep), null (clear), value (set).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContactPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub full_name: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    pub email: Option<Option<String>>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "double_option"
    )]
    pub organization: Option<Option<String>>,
}

impl ContactPatch {
    pub fn is_empty(&self) -> bool {
        self.full_name.is_none() && self.email.is_none() && self.organization.is_none()
    }
}

fn double_option<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer).map(Some)
}

// --- Validation helpers --------------------------------------------------------

/// Checks bounded unicode character length.
pub fn text_len(value: &str, minimum: usize, maximum: usize) -> bool {
    let length = value.chars().count();
    (minimum..=maximum).contains(&length)
}

/// Basic email validity predicate conforming to CRM domain constraints.
pub fn valid_email(value: &str) -> bool {
    let mut parts = value.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    parts.next().is_none()
        && !local.is_empty()
        && !domain.is_empty()
        && !value.chars().any(char::is_whitespace)
        && value.chars().count() <= 254
}
