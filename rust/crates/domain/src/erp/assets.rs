//! The assets domain contracts — the Archxy 2 port of
//! `packages/assets/src/contracts.ts` (R05 third cohort: asset custody and
//! maintenance over the `archava_assets` ledger, PACKET-006).
//!
//! ## The wire grammar
//!
//! A strict `{requestId, type, data}` mutation whose `type` is the
//! discriminated union's tag and whose `data` carries exactly that arm's
//! members — five arms (create_asset, update_asset, handover_asset,
//! record_maintenance, retire_asset). The parse bakes in every zod
//! transform the TS document carries:
//!
//! - `code`/`name`/`category` are `.trim()`-bounded (1–50/1–160/1–80,
//!   the trimmed value travels), `description` and `reason` ride the
//!   same 1–160 `nameText` family;
//! - `serialNumber`/`model`/`customerContactId`/`initialLocation`/
//!   `toCustodianId`/`location`/`notes`/`partsUsed` are
//!   `.nullable().optional()` — an absent key, an explicit null and a
//!   valid string are all in grammar, and this port folds absent and
//!   null onto the same `None`: the repository's SQL keeps the current
//!   cell for either (the `CASE WHEN $n::text IS NOT NULL` /
//!   `COALESCE($n, …)` families), so the fold is effect-exact. The
//!   recorded consequence: the fingerprint is computed over the typed
//!   document, ledger-internal like every other ERP digest;
//! - `condition` is `.default('good')` — an absent member parses as the
//!   default;
//! - `ownership` is the two-way `business_owned | customer_owned`
//!   vocabulary, and `create_asset` carries the schema's refine:
//!   `customer_owned` REQUIRES `customerContactId`.
//!
//! The template/retire reasons are 1–160 (`nameText`), not the audit
//! table's 2000 reason column — the schema bounds carry.
//!
//! ## The error vocabulary
//!
//! The TS `AssetError` list is wider than the carrier
//! (`forbidden`, `not_found`, `conflict`, `invalid_request`, …) but the
//! refused families fold onto the shared [`ErpError`] net (403/404/409)
//! the other ERP cohorts ride, with the TS 400-class verdicts —
//! `invalid_condition` and the transaction-time `invalid_request` —
//! surfaced through the [`ErpError::Unavailable`] arm, which THIS module's
//! gate answers with the baseline's 400 `invalid_submission` body. The
//! newarm-detail family (already checked out, not in custody, archived,
//! stale revision) stays conflict exactly as the TS api's code map does.
//! The fold is recorded in the packet's KNOWN_DIFFERENCES ledger, never
//! silent.

use serde::Serialize;
use serde_json::Value;

pub use super::purchasing::{PrincipalKind, PurchasePrincipal as AssetPrincipal, ValidationError};
pub use super::{ErpError, ErpScope};

/// The assets permission vocabulary — `ASSET_PERMISSIONS`. Strings, not an
/// enum: membership rows store them and the grant gates test membership of
/// the list, like every other ERP cohort.
pub const ASSET_PERMISSIONS: &[&str] = &[
    "assets.read",
    "assets.write",
    "assets.handover",
    "assets.maintenance",
    "audit.read",
];

// --- Vocabulary constants (contracts.ts's bounds, mirrored) ------------------

/// Bounded `code`: trimmed 1–50.
pub const ASSET_CODE_MAX: usize = 50;
/// Bounded `name` AND the `description`/`reason` 1–160 family (`nameText`).
pub const ASSET_NAME_MAX: usize = 160;
/// Bounded `category`: trimmed 1–80.
pub const ASSET_CATEGORY_MAX: usize = 80;
/// The serial-number/model/customer-contact/custodian 1–80 family.
pub const ASSET_TEXT80_MAX: usize = 80;
/// The `location`/`initialLocation` 1–160 family.
pub const ASSET_LOCATION_MAX: usize = 160;
/// Bounded `notes`/`partsUsed`: trimmed 1–500
pub const ASSET_NOTES_MAX: usize = 500;
/// The revision ceiling every mutation's optimistic check guards
/// (`z.number().int().positive().max(2147483647)`).
pub const ASSET_REVISION_MAX: i64 = 2_147_483_647;

// --- The row vocabularies (migration 001's CHECK lists) ----------------------

/// The asset status column's five-way vocabulary the arms derive against
/// (the create arm's customer-owned start is `in_custody`).
pub const STATUS_AVAILABLE: &str = "available";
pub const STATUS_IN_CUSTODY: &str = "in_custody";
pub const STATUS_UNDER_MAINTENANCE: &str = "under_maintenance";
pub const STATUS_RETIRED: &str = "retired";
pub const STATUS_RETURNED_TO_CUSTOMER: &str = "returned_to_customer";

/// The three query states the maintenance arm's recovery check treats as
/// serviceable (`excellent | good | fair`).
pub const CONDITION_SERVICEABLE: &[&str] = &["excellent", "good", "fair"];

/// Ownership — the two-way vocabulary the arms branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetOwnership {
    BusinessOwned,
    CustomerOwned,
}

impl AssetOwnership {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "business_owned" => Some(Self::BusinessOwned),
            "customer_owned" => Some(Self::CustomerOwned),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::BusinessOwned => "business_owned",
            Self::CustomerOwned => "customer_owned",
        }
    }
}

/// Custody condition — the five-way vocabulary the create/handover/
/// maintenance data carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetCondition {
    Excellent,
    Good,
    Fair,
    Damaged,
    Unserviceable,
}

impl AssetCondition {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "excellent" => Some(Self::Excellent),
            "good" => Some(Self::Good),
            "fair" => Some(Self::Fair),
            "damaged" => Some(Self::Damaged),
            "unserviceable" => Some(Self::Unserviceable),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Excellent => "excellent",
            Self::Good => "good",
            Self::Fair => "fair",
            Self::Damaged => "damaged",
            Self::Unserviceable => "unserviceable",
        }
    }

    /// The maintenance arm's serviceable classes (`excellent|good|fair`) —
    /// the states whose custody release flips an `under_maintenance`
    /// business-owned asset back to `available`.
    pub fn serviceable(self) -> bool {
        CONDITION_SERVICEABLE.contains(&self.as_str())
    }
}

/// Custody event type — the five-way handover vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CustodyEventType {
    Dispatch,
    Checkin,
    Transfer,
    ServiceIntake,
    CustomerReturn,
}

impl CustodyEventType {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "dispatch" => Some(Self::Dispatch),
            "checkin" => Some(Self::Checkin),
            "transfer" => Some(Self::Transfer),
            "service_intake" => Some(Self::ServiceIntake),
            "customer_return" => Some(Self::CustomerReturn),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::Checkin => "checkin",
            Self::Transfer => "transfer",
            Self::ServiceIntake => "service_intake",
            Self::CustomerReturn => "customer_return",
        }
    }
}

/// Maintenance service type — the three-way vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MaintenanceType {
    Preventive,
    Corrective,
    Inspection,
}

impl MaintenanceType {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "preventive" => Some(Self::Preventive),
            "corrective" => Some(Self::Corrective),
            "inspection" => Some(Self::Inspection),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Preventive => "preventive",
            Self::Corrective => "corrective",
            Self::Inspection => "inspection",
        }
    }
}

// --- The command arms ---------------------------------------------------------

/// One command arm of `assetMutationSchema` — the discriminated union on
/// `type`. Field spellings match the wire exactly: a parsed command
/// serializes back into the shape the fingerprint covered, and the absent
/// `condition` is already filled by the parse, exactly as the zod default
/// fills it. The optional nullable members carry [`Option`]: `None` means
/// "not changing this cell" (absent AND explicit null — the SQL keeps the
/// current cell for either, the recorded fold).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum AssetCommand {
    #[serde(rename = "create_asset")]
    CreateAsset {
        /// Trimmed, 1–50 chars.
        code: String,
        /// Trimmed, 1–160 chars.
        name: String,
        /// Trimmed, 1–80 chars.
        category: String,
        serial_number: Option<String>,
        model: Option<String>,
        ownership: AssetOwnership,
        /// The refine pins this non-null when ownership is
        /// `customer_owned`.
        customer_contact_id: Option<String>,
        /// `.default('good')` — already filled by the parse.
        condition: AssetCondition,
        initial_location: Option<String>,
    },
    #[serde(rename = "update_asset")]
    UpdateAsset {
        #[serde(rename = "assetId")]
        asset_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        /// The name/category helpers REFUSE an explicit null (the schema's
        /// non-nullable `.optional()` family).
        name: Option<String>,
        category: Option<String>,
        serial_number: Option<String>,
        model: Option<String>,
        #[serde(rename = "currentLocation")]
        current_location: Option<String>,
    },
    #[serde(rename = "handover_asset")]
    HandoverAsset {
        #[serde(rename = "assetId")]
        asset_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "eventType")]
        event_type: CustodyEventType,
        to_custodian_id: Option<String>,
        #[serde(rename = "conditionAfter")]
        condition_after: AssetCondition,
        location: Option<String>,
        notes: Option<String>,
    },
    #[serde(rename = "record_maintenance")]
    RecordMaintenance {
        #[serde(rename = "assetId")]
        asset_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "serviceType")]
        service_type: MaintenanceType,
        /// Trimmed, 1–160 chars (`nameText` rides).
        description: String,
        parts_used: Option<String>,
        #[serde(rename = "conditionAfter")]
        condition_after: AssetCondition,
        location: Option<String>,
    },
    #[serde(rename = "retire_asset")]
    RetireAsset {
        #[serde(rename = "assetId")]
        asset_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        /// Trimmed, 1–160 chars (`nameText` rides).
        reason: String,
    },
}

impl AssetCommand {
    /// The discriminator — the audit rows' `operation` cell and the
    /// receipt's `type` member record it verbatim.
    pub fn mutation_type(&self) -> &'static str {
        match self {
            Self::CreateAsset { .. } => "create_asset",
            Self::UpdateAsset { .. } => "update_asset",
            Self::HandoverAsset { .. } => "handover_asset",
            Self::RecordMaintenance { .. } => "record_maintenance",
            Self::RetireAsset { .. } => "retire_asset",
        }
    }

    /// The mutation's own permission — the TS `requiredAssetPermission`:
    /// create/update/retire → `assets.write`; handover → `assets.handover`;
    /// record maintenance → `assets.maintenance`.
    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::CreateAsset { .. } | Self::UpdateAsset { .. } | Self::RetireAsset { .. } => {
                "assets.write"
            }
            Self::HandoverAsset { .. } => "assets.handover",
            Self::RecordMaintenance { .. } => "assets.maintenance",
        }
    }

    /// The arm's asset id — the mutation surface has no property layer, so
    /// the typed extraction reaches the arm directly (the handlers refuse
    /// a conflict on a malformed arm rather than assume an identifier).
    pub fn asset_ref(&self) -> &str {
        match self {
            Self::UpdateAsset { asset_id, .. }
            | Self::HandoverAsset { asset_id, .. }
            | Self::RecordMaintenance { asset_id, .. }
            | Self::RetireAsset { asset_id, .. } => asset_id,
            Self::CreateAsset { .. } => "",
        }
    }

    /// The audit reason — only the retire arm carries one (the TS audit
    /// insert names `reason` on the retire row, `null` everywhere else).
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::RetireAsset { reason, .. } => Some(reason),
            _ => None,
        }
    }
}

/// The strict mutation document — `{requestId, type, data}`. The `command`
/// is the parsed arm itself; the wire's `type` discriminator is the enum
/// tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetMutation {
    pub request_id: String,
    pub command: AssetCommand,
}

/// The access envelope — `assetAccessSchema` resolved inside the
/// transaction: the membership's permissions, the OWNING human's id (the
/// assigned scope's screen), and the scope. A service principal narrows to
/// the human it names, exactly as the other cohorts' ladders do.
#[derive(Debug, Clone)]
pub struct AssetAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl AssetAccess {
    /// Whether the access holds ANY of the wanted permissions.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        self.permissions
            .iter()
            .any(|held| wanted.contains(&held.as_str()))
    }
}

// --- The receipts + status ----------------------------------------------------

/// `assetReceiptSchema` — the flat five-member document
/// `{requestId, type, assetId, revision, committedAt}` every arm returns
/// (the TS api answers `service.mutate` verbatim, so the gate's OK body is
/// exactly this document, no envelope).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AssetReceipt {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "type")]
    pub command_type: &'static str,
    #[serde(rename = "assetId")]
    pub asset_id: String,
    pub revision: i64,
    #[serde(rename = "committedAt")]
    pub committed_at: String,
}

impl AssetReceipt {
    /// The parse of a REPLAYED receipt from a stored row — the TS
    /// `assetReceiptSchema.parse(row.receipt)`; every member is required,
    /// the set is strict, and `requestId` must re-match the request the
    /// row is keyed by is NOT a schema rule (the schema judges the
    /// document alone) — the caller's `requestId`/`request_id` holds.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("requestId"))?;
        let request_id = object
            .get("requestId")
            .and_then(Value::as_str)
            .filter(|value| super::valid_uuid(value))
            .ok_or(ValidationError("requestId"))?
            .to_string();
        let command_type = match object.get("type").and_then(Value::as_str) {
            Some("create_asset") => "create_asset",
            Some("update_asset") => "update_asset",
            Some("handover_asset") => "handover_asset",
            Some("record_maintenance") => "record_maintenance",
            Some("retire_asset") => "retire_asset",
            _ => return Err(ValidationError("type")),
        };
        Ok(Self {
            request_id,
            command_type,
            asset_id: object
                .get("assetId")
                .and_then(Value::as_str)
                .filter(|value| super::valid_uuid(value))
                .ok_or(ValidationError("assetId"))?
                .to_string(),
            revision: object
                .get("revision")
                .and_then(Value::as_i64)
                .filter(|value| (1..=ASSET_REVISION_MAX).contains(value))
                .ok_or(ValidationError("revision"))?,
            committed_at: object
                .get("committedAt")
                .and_then(Value::as_str)
                .filter(|value| !value.is_empty())
                .ok_or(ValidationError("committedAt"))?
                .to_string(),
        })
    }
}

/// `assetCommandStatusSchema` — the committed/not_committed pair, the TS
/// commandStatus document verbatim (`not_committed` carries NO request id;
/// the accommodation port's optional  `requestId` member is not part of
/// this cohort's contract).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum AssetCommandStatus {
    #[serde(rename = "committed")]
    Committed { receipt: AssetReceipt },
    #[serde(rename = "not_committed")]
    NotCommitted,
}

// --- parse helpers (the accommodation grammar's style) --------------------------

/// A required UUID member.
fn required_uuid(object: &serde_json::Map<String, Value>, name: &'static str) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| super::valid_uuid(value))
        .map(str::to_string)
}

/// A trimmed bounded text member (`.trim().min(1).max(max)`).
fn bounded_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
    max: usize,
) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .map(|value| super::trimmed(value).to_string())
        .filter(|value| !value.is_empty() && value.len() <= max)
}

/// An integer member within `min..=max`.
fn bounded_integer(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
    min: i64,
    max: i64,
) -> Option<i64> {
    let value = object.get(name)?;
    let number = match value {
        Value::Number(number) => number,
        _ => return None,
    };
    let as_f64 = number.as_f64()?;
    if as_f64.fract() != 0.0 || as_f64 < min as f64 || as_f64 > max as f64 {
        return None;
    }
    number.as_i64()
}

/// Whether the document's member set is EXACTLY these keys (`.strict()`).
fn strict_members(object: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    object.keys().all(|key| keys.contains(&key.as_str()))
        && keys.iter().all(|key| object.contains_key(*key))
}

/// Whether the document's member set stays INSIDE `allowed` and covers
/// `required` (`.strict()` + `.optional()` — keys MAY be absent but may
/// never exceed the union; `required` defaults ride separately).
fn strict_optional_members(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    required: &[&str],
) -> bool {
    object.keys().all(|key| allowed.contains(&key.as_str()))
        && required.iter().all(|key| object.contains_key(*key))
}

/// The optional NON-nullable text member (`nameText.optional()` of
/// `.trim().min(1).max(max)`): an ABSENT key parses as `None`; an explicit
/// null is OUT of grammar (the schema has no `.nullable()` here) and a
/// present string must be a valid trimmed value.
fn optional_trim_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
    max: usize,
) -> Result<Option<String>, ()> {
    match object.get(name) {
        None => Ok(None),
        Some(Value::Null) => Err(()),
        Some(_) => Ok(Some(bounded_member(object, name, max).ok_or(())?)),
    }
}

/// The optional NULLABLE text member (`.nullable().optional()`): an absent
/// key AND an explicit null BOTH parse as `None` (the recorded fold — the
/// repository keeps the current cell for either), and a present string
/// must be a valid trimmed value.
fn nullable_optional_member(
    object: &serde_json::Map<String, Value>,
    name: &'static str,
    max: usize,
) -> Result<Option<String>, ()> {
    match object.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => Ok(Some(bounded_member(object, name, max).ok_or(())?)),
    }
}

/// `parse_mutation` — the schema's full walk, in the mode the TS one runs
/// (`mutate()`'s boundary): strict keys, uuid requestId, the arm by `type`,
/// each arm's own strict data sheet.
#[allow(clippy::result_large_err)]
pub fn parse_mutation(raw: &Value) -> Result<AssetMutation, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("requestId"))?;
    let request_id = required_uuid(object, "requestId").ok_or(ValidationError("requestId"))?;
    if !strict_members(object, &["requestId", "type", "data"]) {
        return Err(ValidationError("type"));
    }
    let command = object
        .get("data")
        .and_then(Value::as_object)
        .ok_or(ValidationError("data"))?;
    let command = parse_command(command, object.get("type").and_then(Value::as_str))?;
    Ok(AssetMutation {
        request_id,
        command,
    })
}

/// `parse_command` — ONE arm's strict data sheet, judged against the
/// `type` the mutation names (the union's two-level discriminator).
#[allow(clippy::result_large_err)]
pub fn parse_command(
    data: &serde_json::Map<String, Value>,
    arm: Option<&str>,
) -> Result<AssetCommand, ValidationError> {
    match arm {
        Some("create_asset") => {
            // `.strict()` with the optional family: anything beyond the
            // nine members refuses; code/name/category/ownership must be
            // present; condition is `.default('good')` (MAY be absent) and
            // the four `.nullable().optional()` members MAY be absent (or
            // null — the grammar they built).
            if !strict_optional_members(
                data,
                &[
                    "code",
                    "name",
                    "category",
                    "serialNumber",
                    "model",
                    "ownership",
                    "customerContactId",
                    "condition",
                    "initialLocation",
                ],
                &["code", "name", "category", "ownership"],
            ) {
                return Err(ValidationError("type"));
            }
            let code =
                bounded_member(data, "code", ASSET_CODE_MAX).ok_or(ValidationError("code"))?;
            let name =
                bounded_member(data, "name", ASSET_NAME_MAX).ok_or(ValidationError("name"))?;
            let category = bounded_member(data, "category", ASSET_CATEGORY_MAX)
                .ok_or(ValidationError("category"))?;
            let serial_number = nullable_optional_member(data, "serialNumber", ASSET_TEXT80_MAX)
                .map_err(|_| ValidationError("serialNumber"))?;
            let model = nullable_optional_member(data, "model", ASSET_TEXT80_MAX)
                .map_err(|_| ValidationError("model"))?;
            let ownership = data
                .get("ownership")
                .and_then(Value::as_str)
                .and_then(AssetOwnership::parse)
                .ok_or(ValidationError("ownership"))?;
            let customer_contact_id =
                nullable_optional_member(data, "customerContactId", ASSET_TEXT80_MAX)
                    .map_err(|_| ValidationError("customerContactId"))?;
            // `.default('good')`.
            let condition = match data.get("condition") {
                None => AssetCondition::Good,
                Some(_) => data
                    .get("condition")
                    .and_then(Value::as_str)
                    .and_then(AssetCondition::parse)
                    .ok_or(ValidationError("condition"))?,
            };
            let initial_location =
                nullable_optional_member(data, "initialLocation", ASSET_LOCATION_MAX)
                    .map_err(|_| ValidationError("initialLocation"))?;
            // The schema's refine: `customer_owned` REQUIRES a non-empty
            // customerContactId.
            if ownership == AssetOwnership::CustomerOwned
                && customer_contact_id
                    .as_deref()
                    .map(|contact| !contact.is_empty())
                    != Some(true)
            {
                return Err(ValidationError("customerContactId"));
            }
            Ok(AssetCommand::CreateAsset {
                code,
                name,
                category,
                serial_number,
                model,
                ownership,
                customer_contact_id,
                condition,
                initial_location,
            })
        }
        Some("update_asset") => {
            // assetId/expectedRevision carry the walk; the five text cells
            // are `.optional()` (their null-family rules ride the member
            // helpers below).
            if !strict_optional_members(
                data,
                &[
                    "assetId",
                    "expectedRevision",
                    "name",
                    "category",
                    "serialNumber",
                    "model",
                    "currentLocation",
                ],
                &["assetId", "expectedRevision"],
            ) {
                return Err(ValidationError("type"));
            }
            let asset_id = required_uuid(data, "assetId").ok_or(ValidationError("assetId"))?;
            let expected_revision =
                bounded_integer(data, "expectedRevision", 1, ASSET_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let name = optional_trim_member(data, "name", ASSET_NAME_MAX)
                .map_err(|_| ValidationError("name"))?;
            let category = optional_trim_member(data, "category", ASSET_CATEGORY_MAX)
                .map_err(|_| ValidationError("category"))?;
            let serial_number = nullable_optional_member(data, "serialNumber", ASSET_TEXT80_MAX)
                .map_err(|_| ValidationError("serialNumber"))?;
            let model = nullable_optional_member(data, "model", ASSET_TEXT80_MAX)
                .map_err(|_| ValidationError("model"))?;
            let current_location =
                nullable_optional_member(data, "currentLocation", ASSET_LOCATION_MAX)
                    .map_err(|_| ValidationError("currentLocation"))?;
            Ok(AssetCommand::UpdateAsset {
                asset_id,
                expected_revision,
                name,
                category,
                serial_number,
                model,
                current_location,
            })
        }
        Some("handover_asset") => {
            // eventType/conditionAfter are required; toCustodianId/
            // location/notes are the nullable-optional family.
            if !strict_optional_members(
                data,
                &[
                    "assetId",
                    "expectedRevision",
                    "eventType",
                    "toCustodianId",
                    "conditionAfter",
                    "location",
                    "notes",
                ],
                &["assetId", "expectedRevision", "eventType", "conditionAfter"],
            ) {
                return Err(ValidationError("type"));
            }
            let asset_id = required_uuid(data, "assetId").ok_or(ValidationError("assetId"))?;
            let expected_revision =
                bounded_integer(data, "expectedRevision", 1, ASSET_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let event_type = data
                .get("eventType")
                .and_then(Value::as_str)
                .and_then(CustodyEventType::parse)
                .ok_or(ValidationError("eventType"))?;
            let to_custodian_id = nullable_optional_member(data, "toCustodianId", ASSET_TEXT80_MAX)
                .map_err(|_| ValidationError("toCustodianId"))?;
            let condition_after = data
                .get("conditionAfter")
                .and_then(Value::as_str)
                .and_then(AssetCondition::parse)
                .ok_or(ValidationError("conditionAfter"))?;
            let location = nullable_optional_member(data, "location", ASSET_LOCATION_MAX)
                .map_err(|_| ValidationError("location"))?;
            let notes = nullable_optional_member(data, "notes", ASSET_NOTES_MAX)
                .map_err(|_| ValidationError("notes"))?;
            Ok(AssetCommand::HandoverAsset {
                asset_id,
                expected_revision,
                event_type,
                to_custodian_id,
                condition_after,
                location,
                notes,
            })
        }
        Some("record_maintenance") => {
            // serviceType/description/conditionAfter are required;
            // partsUsed/location are the nullable-optional family.
            if !strict_optional_members(
                data,
                &[
                    "assetId",
                    "expectedRevision",
                    "serviceType",
                    "description",
                    "partsUsed",
                    "conditionAfter",
                    "location",
                ],
                &[
                    "assetId",
                    "expectedRevision",
                    "serviceType",
                    "description",
                    "conditionAfter",
                ],
            ) {
                return Err(ValidationError("type"));
            }
            let asset_id = required_uuid(data, "assetId").ok_or(ValidationError("assetId"))?;
            let expected_revision =
                bounded_integer(data, "expectedRevision", 1, ASSET_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let service_type = data
                .get("serviceType")
                .and_then(Value::as_str)
                .and_then(MaintenanceType::parse)
                .ok_or(ValidationError("serviceType"))?;
            let description = bounded_member(data, "description", ASSET_NAME_MAX)
                .ok_or(ValidationError("description"))?;
            let parts_used = nullable_optional_member(data, "partsUsed", ASSET_NOTES_MAX)
                .map_err(|_| ValidationError("partsUsed"))?;
            let condition_after = data
                .get("conditionAfter")
                .and_then(Value::as_str)
                .and_then(AssetCondition::parse)
                .ok_or(ValidationError("conditionAfter"))?;
            let location = nullable_optional_member(data, "location", ASSET_LOCATION_MAX)
                .map_err(|_| ValidationError("location"))?;
            Ok(AssetCommand::RecordMaintenance {
                asset_id,
                expected_revision,
                service_type,
                description,
                parts_used,
                condition_after,
                location,
            })
        }
        Some("retire_asset") => {
            if !strict_members(data, &["assetId", "expectedRevision", "reason"]) {
                return Err(ValidationError("type"));
            }
            let asset_id = required_uuid(data, "assetId").ok_or(ValidationError("assetId"))?;
            let expected_revision =
                bounded_integer(data, "expectedRevision", 1, ASSET_REVISION_MAX)
                    .ok_or(ValidationError("expectedRevision"))?;
            let reason =
                bounded_member(data, "reason", ASSET_NAME_MAX).ok_or(ValidationError("reason"))?;
            Ok(AssetCommand::RetireAsset {
                asset_id,
                expected_revision,
                reason,
            })
        }
        _ => Err(ValidationError("type")),
    }
}
