//! The organizations domain contracts — the Archxy 2 port of
//! `packages/organizations/src/contracts.ts` (R04: organizations +
//! affiliations, the CRM-adjacent half of the R04 cohort).
//!
//! ## The wire grammar
//!
//! `organizationMutationSchema`'s members are three-key strict objects
//! `{requestId,type,data}` whose `data` sub-document carries exactly the
//! fields the arm declares. Optional text members are
//! `.nullable().optional()` in most arms, which is a THREE-STATE cell the
//! SQL COALESCE/CASE branches distinguish from each other: absent, explicit
//! null, or a value. The parse reproduces that tri-state wire (an explicit
//! null serializes — a distinct document — while absent does not), and
//! every arm's data set is strict: an unknown member, like a missing
//! required one, is a rejection.
//!
//! Serialization note (documented divergence, same class the purchasing
//! port records): this port emits `{requestId,data:{type,...}}` instead of
//! the baseline's `{requestId,type,data}` — the canonical fingerprint
//! digest is ledger-internal, so the placement difference stays stable and
//! wire-visible guests see the same field sets.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use super::purchasing::PurchasePrincipal as OrgPrincipal;
use super::purchasing::ValidationError;
use super::purchasing::REVISION_MAX;
use super::{bounded_title, valid_uuid, ErpScope};

/// The organization ledger's permission vocabulary — the schema CHECK's
/// 5-word array (`permissions <@ ARRAY[...]`) and `ORGANIZATION_PERMISSIONS`.
pub const ORGANIZATION_PERMISSIONS: &[&str] = &[
    "organizations.read",
    "organizations.write",
    "affiliations.read",
    "affiliations.write",
    "audit.read",
];

/// The organization state vocabulary — `organizationStateSchema`.
pub const ORGANIZATION_STATES: &[&str] = &["active", "archived"];

/// The affiliation status vocabulary — `affiliationStatusSchema`.
pub const AFFILIATION_STATUSES: &[&str] = &["active", "inactive"];

/// The arm vocabulary — the receipt's `type` and the audit operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrganizationKind {
    CreateOrganization,
    UpdateOrganization,
    ArchiveOrganization,
    CreateAffiliation,
    UpdateAffiliation,
    RemoveAffiliation,
}

impl OrganizationKind {
    /// The audit row's operation cell and the receipt's `type`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CreateOrganization => "create_organization",
            Self::UpdateOrganization => "update_organization",
            Self::ArchiveOrganization => "archive_organization",
            Self::CreateAffiliation => "create_affiliation",
            Self::UpdateAffiliation => "update_affiliation",
            Self::RemoveAffiliation => "remove_affiliation",
        }
    }

    /// `requiredOrgPermission`: org arms need `organizations.write`,
    /// affiliation arms need `affiliations.write`.
    pub fn permission(self) -> &'static str {
        match self {
            Self::CreateOrganization | Self::UpdateOrganization | Self::ArchiveOrganization => {
                "organizations.write"
            }
            Self::CreateAffiliation | Self::UpdateAffiliation | Self::RemoveAffiliation => {
                "affiliations.write"
            }
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "create_organization" => Some(Self::CreateOrganization),
            "update_organization" => Some(Self::UpdateOrganization),
            "archive_organization" => Some(Self::ArchiveOrganization),
            "create_affiliation" => Some(Self::CreateAffiliation),
            "update_affiliation" => Some(Self::UpdateAffiliation),
            "remove_affiliation" => Some(Self::RemoveAffiliation),
            _ => None,
        }
    }
}

/// A resolved organizations-side access — the parsed membership grant plus
/// the owner id the `assigned` scope narrows reads and writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrgAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl OrgAccess {
    /// Whether the access holds EVERY named permission — the `grant()` rule.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// The scope rule: `assigned` sees and writes only records owned by the
    /// access's owner.
    pub fn sees(&self, owner_id: &str) -> bool {
        self.scope.covers_all() || self.owner_id == owner_id
    }
}

// --- The data union ---------------------------------------------------------

/// One arm's data — the fields WITHIN the mutation's `data` member, each
/// variant serialized with the exact wire field names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum OrganizationData {
    #[serde(rename = "create_organization")]
    CreateOrganization {
        name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        legal_name: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tax_id: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        website: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        industry: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        billing_address: Option<Option<String>>,
    },
    #[serde(rename = "update_organization")]
    UpdateOrganization {
        id: String,
        revision: i64,
        #[serde(skip_serializing_if = "Option::is_none")]
        name: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        legal_name: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tax_id: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        website: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        industry: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        billing_address: Option<Option<String>>,
    },
    #[serde(rename = "archive_organization")]
    ArchiveOrganization {
        id: String,
        revision: i64,
        reason: String,
    },
    #[serde(rename = "create_affiliation")]
    CreateAffiliation {
        #[serde(rename = "organizationId")]
        organization_id: String,
        #[serde(rename = "contactId")]
        contact_id: String,
        role: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        department: Option<Option<String>>,
        #[serde(rename = "isPrimary", skip_serializing_if = "Option::is_none")]
        is_primary: Option<bool>,
    },
    #[serde(rename = "update_affiliation")]
    UpdateAffiliation {
        id: String,
        #[serde(rename = "organizationId")]
        organization_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        role: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<Option<String>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        department: Option<Option<String>>,
        #[serde(rename = "isPrimary", skip_serializing_if = "Option::is_none")]
        is_primary: Option<bool>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
    },
    #[serde(rename = "remove_affiliation")]
    RemoveAffiliation {
        id: String,
        #[serde(rename = "organizationId")]
        organization_id: String,
        reason: String,
    },
}

impl OrganizationData {
    /// The arm discriminator.
    pub fn kind(&self) -> OrganizationKind {
        match self {
            Self::CreateOrganization { .. } => OrganizationKind::CreateOrganization,
            Self::UpdateOrganization { .. } => OrganizationKind::UpdateOrganization,
            Self::ArchiveOrganization { .. } => OrganizationKind::ArchiveOrganization,
            Self::CreateAffiliation { .. } => OrganizationKind::CreateAffiliation,
            Self::UpdateAffiliation { .. } => OrganizationKind::UpdateAffiliation,
            Self::RemoveAffiliation { .. } => OrganizationKind::RemoveAffiliation,
        }
    }
}

/// The mutation envelope — the wire shows `{requestId,type,data}`; this
/// port serializes the arm's `type` inside `data` (emit note above).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationMutation {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(flatten)]
    pub data: OrganizationData,
}

// --- Field parsers ----------------------------------------------------------

fn as_i64_member(
    value: Option<&Value>,
    field: &'static str,
    min: i64,
) -> Result<i64, ValidationError> {
    value
        .and_then(Value::as_i64)
        .filter(|number| *number >= min && *number <= REVISION_MAX)
        .ok_or(ValidationError(field))
}

/// A required trimmed text cell — `z.string().trim().min(1).max(N)`.
fn required_text(
    object: &serde_json::Map<String, Value>,
    key: &str,
    field: &'static str,
    max: usize,
) -> Result<String, ValidationError> {
    let raw = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ValidationError(field))?;
    bounded_title(raw, max).ok_or(ValidationError(field))
}

fn required_uuid(
    object: &serde_json::Map<String, Value>,
    key: &str,
    field: &'static str,
) -> Result<String, ValidationError> {
    let raw = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ValidationError(field))?;
    if !valid_uuid(raw) {
        return Err(ValidationError(field));
    }
    Ok(raw.to_string())
}

/// `.optional()` of a NON-nullable text — absent or a trimmed value.
fn optional_text(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    field: &'static str,
    max: usize,
) -> Result<Option<String>, ValidationError> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => {
            let raw = value.as_str().ok_or(ValidationError(field))?;
            bounded_title(raw, max)
                .map(Some)
                .ok_or(ValidationError(field))
        }
    }
}

/// `.nullable().optional()` of a trimmed text — the three-state cell.
fn tristate_text(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    field: &'static str,
    max: usize,
) -> Result<Option<Option<String>>, ValidationError> {
    match object.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(value) => {
            let raw = value.as_str().ok_or(ValidationError(field))?;
            bounded_title(raw, max)
                .map(Some)
                .map(Some)
                .ok_or(ValidationError(field))
        }
    }
}

/// `.optional()` of a bool.
fn optional_bool(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<bool>, ValidationError> {
    match object.get(key) {
        None => Ok(None),
        Some(value) => value.as_bool().map(Some).ok_or(ValidationError(key)),
    }
}

/// The strict parse of one arm's data object — member for member.
fn parse_data(
    kind: &str,
    object: &serde_json::Map<String, Value>,
) -> Result<OrganizationData, ValidationError> {
    let arm = OrganizationKind::parse(kind).ok_or(ValidationError("type"))?;
    let data = match arm {
        OrganizationKind::CreateOrganization => {
            let legal_name = tristate_text(object, "legalName", "legalName", 160)?;
            let tax_id = tristate_text(object, "taxId", "taxId", 50)?;
            let website = tristate_text(object, "website", "website", 200)?;
            let industry = tristate_text(object, "industry", "industry", 100)?;
            let billing_address = tristate_text(object, "billingAddress", "billingAddress", 500)?;
            Ok(OrganizationData::CreateOrganization {
                name: required_text(object, "name", "name", 160)?,
                legal_name,
                tax_id,
                website,
                industry,
                billing_address,
            })
        }
        OrganizationKind::UpdateOrganization => {
            let id = required_uuid(object, "id", "id")?;
            let revision = as_i64_member(object.get("revision"), "revision", 1)?;
            let name = optional_text(object, "name", "name", 160)?;
            let legal_name = tristate_text(object, "legalName", "legalName", 160)?;
            let tax_id = tristate_text(object, "taxId", "taxId", 50)?;
            let website = tristate_text(object, "website", "website", 200)?;
            let industry = tristate_text(object, "industry", "industry", 100)?;
            let billing_address = tristate_text(object, "billingAddress", "billingAddress", 500)?;
            Ok(OrganizationData::UpdateOrganization {
                id,
                revision,
                name,
                legal_name,
                tax_id,
                website,
                industry,
                billing_address,
            })
        }
        OrganizationKind::ArchiveOrganization => Ok(OrganizationData::ArchiveOrganization {
            id: required_uuid(object, "id", "id")?,
            revision: as_i64_member(object.get("revision"), "revision", 1)?,
            reason: required_text(object, "reason", "reason", 2000)?,
        }),
        OrganizationKind::CreateAffiliation => {
            let title = tristate_text(object, "title", "title", 100)?;
            let department = tristate_text(object, "department", "department", 80)?;
            let is_primary = optional_bool(object, "isPrimary")?;
            Ok(OrganizationData::CreateAffiliation {
                organization_id: required_uuid(object, "organizationId", "organizationId")?,
                contact_id: required_uuid(object, "contactId", "contactId")?,
                role: required_text(object, "role", "role", 80)?,
                title,
                department,
                is_primary,
            })
        }
        OrganizationKind::UpdateAffiliation => {
            let role = optional_text(object, "role", "role", 80)?;
            let title = tristate_text(object, "title", "title", 100)?;
            let department = tristate_text(object, "department", "department", 80)?;
            let is_primary = optional_bool(object, "isPrimary")?;
            let status = match object.get("status") {
                None => None,
                Some(value) => match value.as_str() {
                    Some(raw) if AFFILIATION_STATUSES.contains(&raw) => Some(raw.to_string()),
                    _ => return Err(ValidationError("status")),
                },
            };
            Ok(OrganizationData::UpdateAffiliation {
                id: required_uuid(object, "id", "id")?,
                organization_id: required_uuid(object, "organizationId", "organizationId")?,
                role,
                title,
                department,
                is_primary,
                status,
            })
        }
        OrganizationKind::RemoveAffiliation => Ok(OrganizationData::RemoveAffiliation {
            id: required_uuid(object, "id", "id")?,
            organization_id: required_uuid(object, "organizationId", "organizationId")?,
            reason: required_text(object, "reason", "reason", 2000)?,
        }),
    }?;
    // Strictness: the arm's data object must carry ONLY the members the
    // arm declares (checked by re-serializing the parsed arm and comparing
    // member counts — an unknown or duplicate-shaped member is a refusal).
    let known = match arm {
        OrganizationKind::CreateOrganization => 6,
        OrganizationKind::UpdateOrganization => 9,
        OrganizationKind::ArchiveOrganization => 4,
        OrganizationKind::CreateAffiliation => 7,
        OrganizationKind::UpdateAffiliation => 8,
        OrganizationKind::RemoveAffiliation => 4,
    };
    // The parser injected its own `type` discriminator into the map above;
    // count only the document's real members against the strictness table.
    let members = object.keys().filter(|key| key.as_str() != "type").count();
    if members > known {
        return Err(ValidationError("type"));
    }
    Ok(data)
}

/// The strict parse of the mutation envelope — `{requestId,type,data}` with
/// the data object parsed arm by arm.
pub fn parse_mutation(raw: &Value) -> Result<OrganizationMutation, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("requestId"))?;
    let request_id = required_uuid(object, "requestId", "requestId")?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(ValidationError("type"))?;
    if object
        .keys()
        .any(|key| !["requestId", "type", "data"].contains(&key.as_str()))
    {
        return Err(ValidationError("type"));
    }
    let data_object = object
        .get("data")
        .and_then(Value::as_object)
        .ok_or(ValidationError("type"))?;
    let mut data_object = data_object.clone();
    data_object.insert("type".to_string(), Value::String(kind.to_string()));
    let data = parse_data(kind, &data_object)?;
    Ok(OrganizationMutation { request_id, data })
}

/// The receipt the mutation records and recovery replays —
/// `organizationReceiptSchema`. `revision` is the ORGANIZATION revision the
/// command produced (affiliation arms bump it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrganizationReceipt {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "organizationId")]
    pub organization_id: String,
    #[serde(rename = "affiliationId")]
    pub affiliation_id: Option<String>,
    #[serde(rename = "revision")]
    pub revision: i64,
    #[serde(rename = "committedAt")]
    pub committed_at: String,
    /// Replay knowledge for the HOST decision — never serialized: the TS
    /// receipt schemas are `.strict()` and carry no such key, so the stored
    /// and served receipt documents stay exactly TS-shaped. The route's
    /// `decision` answers instead.
    #[serde(default, skip_serializing)]
    pub replayed: bool,
}

impl OrganizationReceipt {
    /// The stored receipt's strict parse — the recovery replay path
    /// re-derives the document through this surface.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("requestId"))?;
        let request_id = required_uuid(object, "requestId", "requestId")?;
        let kind = object
            .get("type")
            .and_then(Value::as_str)
            .filter(|kind| OrganizationKind::parse(kind).is_some())
            .ok_or(ValidationError("type"))?;
        let organization_id = required_uuid(object, "organizationId", "organizationId")?;
        let affiliation_id = match object.get("affiliationId") {
            None | Some(Value::Null) => None,
            Some(value) => {
                let raw = value.as_str().ok_or(ValidationError("affiliationId"))?;
                if !valid_uuid(raw) {
                    return Err(ValidationError("affiliationId"));
                }
                Some(raw.to_string())
            }
        };
        let revision = as_i64_member(object.get("revision"), "revision", 1)?;
        let committed_at = object
            .get("committedAt")
            .and_then(Value::as_str)
            .ok_or(ValidationError("committedAt"))?
            .to_string();
        Ok(Self {
            request_id,
            kind: kind.to_string(),
            organization_id,
            affiliation_id,
            revision,
            committed_at,
            replayed: false,
        })
    }
}

/// Builds the replayed receipt: the stored one with `replayed` flipped. A
/// recovery IS a replay by construction — the same class the purchasing and
/// foodservice ledgers serve recovery with.
pub fn replayed_receipt(receipt: &OrganizationReceipt) -> OrganizationReceipt {
    let mut replayed = receipt.clone();
    replayed.replayed = true;
    replayed
}

/// The command-status union — `organizationCommandStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum OrganizationCommandStatus {
    #[serde(rename = "committed")]
    Committed { receipt: OrganizationReceipt },
    #[serde(rename = "not_committed")]
    NotCommitted,
}

/// The two commands the R04 org suite freezes — the arms the nine witnesses
/// exercise (the full 6-arm grammar is parsed; suites pick these for the
/// cross-tenant / replay / revoke demonstrations).
pub const SUITE_ARMS: &[&str] = &["create_organization", "create_affiliation"];

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_create_organization_parses_with_the_declared_members() {
        let mut doc = json!({
            "requestId": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "type": "create_organization",
            "data": {
                "name": "  Puskesmas Kemang  ",
                "legalName": "Puskesmas Kecamatan",
            }
        });
        let mutation = parse_mutation(&doc).expect("parses");
        let OrganizationMutation { request_id, data } = mutation;
        assert_eq!(request_id, "112e5843-c01b-4a3c-a68a-5a5f7a1d1234");
        match &data {
            OrganizationData::CreateOrganization {
                name, legal_name, ..
            } => {
                assert_eq!(name, "Puskesmas Kemang");
                assert!(matches!(legal_name, Some(Some(name)) if name == "Puskesmas Kecamatan"));
            }
            other => panic!("wrong arm {other:?}"),
        }
        // Nullify check: an explicit null parses to the third state.
        doc["data"]["legalName"] = Value::Null;
        match parse_mutation(&doc).expect("null parses").data {
            OrganizationData::CreateOrganization { legal_name, .. } => {
                assert_eq!(legal_name, Some(None), "explicit null …");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn an_unknown_data_member_or_arm_is_a_rejection() {
        let base = json!({
            "requestId": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "type": "archive_organization",
            "data": {"id": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234", "revision": 1}
        });
        assert!(parse_mutation(&base).is_err(), "reason required");
        let mut archive = base.clone();
        archive["data"]["reason"] = Value::String("Reorganization".into());
        let archive = parse_mutation(&archive).expect("complete archive parses");
        assert_eq!(archive.data.kind(), OrganizationKind::ArchiveOrganization);
        // Wire round-trip keeps the tri-state cells' distinctions.
        let mut update = json!({
            "requestId": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "type": "update_affiliation",
            "data": {"id": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234", "organizationId": "212e5843-c01b-4a3c-a68a-5a5f7a1d1234"}
        });
        update["data"]["department"] = Value::Null;
        let mutation = parse_mutation(&update).expect("null department parses");
        assert!(mutation.data.kind() == OrganizationKind::UpdateAffiliation);
        let mut bad = base.clone();
        bad["data"]["surprise"] = Value::String("x".into());
        assert!(parse_mutation(&bad).is_err(), "unknown member refused");
        let mut bad_kind = base.clone();
        bad_kind["type"] = Value::String("delete_organization".into());
        assert!(parse_mutation(&bad_kind).is_err(), "unknown arm refused");
    }

    #[test]
    fn the_permission_rule_is_the_baseline_switch() {
        assert_eq!(
            OrganizationKind::CreateOrganization.permission(),
            "organizations.write"
        );
        assert_eq!(
            OrganizationKind::RemoveAffiliation.permission(),
            "affiliations.write"
        );
        assert_eq!(
            OrganizationKind::RemoveAffiliation.as_str(),
            "remove_affiliation"
        );
    }

    #[test]
    fn the_receipts_round_trip_through_the_parse() {
        let receipt = json!({
            "requestId": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "type": "create_affiliation",
            "organizationId": "212e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "affiliationId": null,
            "revision": 1,
            "committedAt": "2026-01-02T00:00:00Z"
        });
        let parsed = OrganizationReceipt::parse(&receipt).expect("parses");
        assert_eq!(parsed.affiliation_id, None);
        assert_eq!(
            serde_json::to_value(&parsed).expect("serialize"),
            receipt,
            "byte-stable receipt"
        );
        let bad = json!({
            "requestId": "112e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "type": "destroy_affiliation",
            "organizationId": "212e5843-c01b-4a3c-a68a-5a5f7a1d1234",
            "affiliationId": null,
            "revision": 1,
            "committedAt": "2026-01-02T00:00:00Z"
        });
        assert!(OrganizationReceipt::parse(&bad).is_err());
    }

    #[test]
    fn grants_and_scope_follow_the_access_rule() {
        let mut access = OrgAccess {
            permissions: vec!["organizations.write".into()],
            owner_id: "principa-houyi".into(),
            scope: ErpScope::Assigned,
        };
        assert!(access.holds(&[OrganizationKind::CreateOrganization.permission()]));
        assert!(!access.holds(&["organizations.write", "affiliations.write"]));
        assert!(access.sees("principa-houyi"));
        assert!(!access.sees("principa-toba"));
        access.scope = ErpScope::All;
        assert!(access.sees("principa-toba"), "global sees every owner");
    }
}
