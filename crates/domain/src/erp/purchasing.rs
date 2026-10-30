//! The purchasing cohort's contract — ported from
//! `packages/purchasing/src/contracts.ts` (frozen) with the workflow rules of
//! `packages/purchasing-pg/src/repository.ts`.
//!
//! Everything the TS schema enforces, the parse layer here enforces with the
//! same accept/refuse boundary:
//!
//! - ids are uuid-shaped strings; `tenantId`/principal ids are kebab slugs;
//! - `title`/`name` trim to 1..=160 characters and `reason` to 1..=2000;
//! - revisions are 1..=2147483647, costs 0..=1_000_000_000, quantities
//!   1..=1_000_000, order totals 0..=500_000_000_000_000;
//! - line inputs are 1..=50 with unique offer ids; receipt findings 1..=50
//!   with unique line ids, each carrying `accepted+rejected in 1..=1000000`
//!   (the same refine the schema draws);
//! - the command union discriminates on `operation` with EXACT member shapes.
//!
//! Known divergence, recorded rather than silent: JS string length counts
//! UTF-16 code units; this port counts characters (`str::len`). A long
//! multi-code-unit name could pass TS and refuse here — expected to never
//! matter at these bounds, and the refusal direction is safe.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;

use super::{
    bounded_title, canonical_json, valid_code, valid_principal_id, valid_uuid, ErpError, ErpScope,
};

/// The permission vocabulary the purchasing schema's CHECK constrains
/// (`memberships.permissions <@ ARRAY[...]`).
pub const PERMISSION_VOCABULARY: &[&str] = &[
    "suppliers.read",
    "suppliers.write",
    "offers.read",
    "offers.write",
    "orders.read",
    "orders.write",
    "receipts.read",
    "receipts.write",
    "audit.read",
];

/// The state vocabulary the SQL CHECK carries (`purchaseOrderSchema`).
pub const ORDER_STATES: &[&str] = &[
    "draft",
    "confirmed",
    "partial",
    "received",
    "cancelled",
    "short_closed",
];

/// A caller-supplied value the contract refuses. Field names are the schema's,
/// so an operator sees the same word the baseline reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("{0}")]
pub struct ValidationError(pub &'static str);

// Deliberately no `From<ValidationError> for ErpError`: a validation refusal is
// a caller-mistake class, not a conflict, and an implicit mapping would let a
// `?` at a domain boundary reclassify it. The host maps it to its own
// invalid-submission refusal explicitly.

// --- The cast the route hands over: the caller's own principal --------------

/// The caller's identity as the contract sees it —
/// `purchasePrincipalSchema`'s discriminated union. Serializes with the wire
/// field names because the fingerprint digests exactly this document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    Human,
    Service,
}

impl PrincipalKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Human => "human",
            Self::Service => "service",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PurchasePrincipal {
    pub kind: PrincipalKind,
    #[serde(rename = "id")]
    pub id: String,
    /// `onBehalfOf` — present exactly when `kind` is service, which is what
    /// makes the document shape identical to the TS union's members.
    #[serde(rename = "onBehalfOf", skip_serializing_if = "Option::is_none")]
    pub on_behalf_of: Option<String>,
}

impl PurchasePrincipal {
    /// A human actor taking the action as themselves.
    pub fn human(id: impl Into<String>) -> Self {
        Self {
            kind: PrincipalKind::Human,
            id: id.into(),
            on_behalf_of: None,
        }
    }

    /// A service actor carrying a human principal named on behalf of —
    /// permission-wise, a service principal may never widen past that human.
    pub fn service(id: impl Into<String>, on_behalf_of: impl Into<String>) -> Self {
        Self {
            kind: PrincipalKind::Service,
            id: id.into(),
            on_behalf_of: Some(on_behalf_of.into()),
        }
    }

    /// The strict parse of a principal envelope the route received.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("principal"))?;
        let kind = object
            .get("kind")
            .and_then(Value::as_str)
            .ok_or(ValidationError("principal"))?;
        let id = {
            let raw = object
                .get("id")
                .and_then(Value::as_str)
                .ok_or(ValidationError("principal"))?;
            if !valid_principal_id(raw) {
                return Err(ValidationError("principal"));
            }
            raw.to_string()
        };
        let fields = object
            .keys()
            .filter(|key| key.as_str() != "kind" && key.as_str() != "id")
            .count();
        match kind {
            "human" => {
                if fields != 0 {
                    return Err(ValidationError("principal"));
                }
                Ok(Self {
                    kind: PrincipalKind::Human,
                    id,
                    on_behalf_of: None,
                })
            }
            "service" => {
                if fields != 1 {
                    return Err(ValidationError("principal"));
                }
                let raw = object
                    .get("onBehalfOf")
                    .and_then(Value::as_str)
                    .ok_or(ValidationError("principal"))?;
                if !valid_principal_id(raw) {
                    return Err(ValidationError("principal"));
                }
                Ok(Self {
                    kind: PrincipalKind::Service,
                    id,
                    on_behalf_of: Some(raw.to_string()),
                })
            }
            _ => Err(ValidationError("principal")),
        }
    }
}

// --- Input lines -----------------------------------------------------------

/// One `{offerId, quantity}` quote line — `purchaseLinesInputSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LineInput {
    pub offer_id: String,
    pub quantity: i64,
}

/// One physical-inspection finding — `goodsReceiptInputSchema`.
///
/// The refine is structural here, not an afterthought: `accepted+rejected`
/// must be in 1..=1000000, and the checked sum is exactly the number the
/// partial-receipt rule uses (a zero-zero finding never rides a receipt).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReceiptFinding {
    pub line_id: String,
    pub accepted: i64,
    pub rejected: i64,
}

// --- The command union -----------------------------------------------------

/// The purchasing command vocabulary, one variant per union member.
///
/// Arrow keys serialize with the wire field names so fingerprinting fuses the
/// same document shape the baseline hashes (`camelCase` input fields).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "operation")]
pub enum PurchaseCommand {
    #[serde(rename = "supplier.create")]
    SupplierCreate {
        #[serde(rename = "code")]
        code: String,
        #[serde(rename = "name")]
        name: String,
    },
    #[serde(rename = "supplier.set-active")]
    SupplierSetActive {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
        #[serde(rename = "active")]
        active: bool,
    },
    #[serde(rename = "offer.create")]
    OfferCreate {
        #[serde(rename = "supplierId")]
        supplier_id: String,
        #[serde(rename = "productId")]
        product_id: String,
        #[serde(rename = "unitCost")]
        unit_cost: i64,
    },
    #[serde(rename = "offer.set-cost")]
    OfferSetCost {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
        #[serde(rename = "unitCost")]
        unit_cost: i64,
    },
    #[serde(rename = "offer.set-active")]
    OfferSetActive {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
        #[serde(rename = "active")]
        active: bool,
    },
    #[serde(rename = "order.create")]
    OrderCreate {
        #[serde(rename = "supplierId")]
        supplier_id: String,
        #[serde(rename = "lines")]
        lines: Vec<LineInput>,
    },
    #[serde(rename = "order.replace-lines")]
    OrderReplaceLines {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
        #[serde(rename = "lines")]
        lines: Vec<LineInput>,
    },
    #[serde(rename = "order.confirm")]
    OrderConfirm {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
    },
    #[serde(rename = "order.cancel")]
    OrderCancel {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
    },
    #[serde(rename = "order.short-close")]
    OrderShortClose {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
    },
    #[serde(rename = "order.receive")]
    OrderReceive {
        #[serde(rename = "id")]
        id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        #[serde(rename = "reason")]
        reason: String,
        #[serde(rename = "lines")]
        lines: Vec<ReceiptFinding>,
        #[serde(
            rename = "landedFeeMinor",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        landed_fee_minor: Option<i64>,
        #[serde(
            rename = "allocationBasis",
            default,
            skip_serializing_if = "Option::is_none"
        )]
        allocation_basis: Option<super::inventory_valuation::Basis>,
    },
}

impl PurchaseCommand {
    /// The operation string, the union's discriminator and the audit rows'
    /// `operation` value.
    pub fn operation(&self) -> &'static str {
        match self {
            Self::SupplierCreate { .. } => "supplier.create",
            Self::SupplierSetActive { .. } => "supplier.set-active",
            Self::OfferCreate { .. } => "offer.create",
            Self::OfferSetCost { .. } => "offer.set-cost",
            Self::OfferSetActive { .. } => "offer.set-active",
            Self::OrderCreate { .. } => "order.create",
            Self::OrderReplaceLines { .. } => "order.replace-lines",
            Self::OrderConfirm { .. } => "order.confirm",
            Self::OrderCancel { .. } => "order.cancel",
            Self::OrderShortClose { .. } => "order.short-close",
            Self::OrderReceive { .. } => "order.receive",
        }
    }

    /// The command's reason, when its member caries one.
    pub fn reason(&self) -> Option<&str> {
        match self {
            Self::SupplierSetActive { reason, .. }
            | Self::OfferSetCost { reason, .. }
            | Self::OfferSetActive { reason, .. }
            | Self::OrderReplaceLines { reason, .. }
            | Self::OrderConfirm { reason, .. }
            | Self::OrderCancel { reason, .. }
            | Self::OrderShortClose { reason, .. }
            | Self::OrderReceive { reason, .. } => Some(reason),
            Self::SupplierCreate { .. } | Self::OfferCreate { .. } | Self::OrderCreate { .. } => {
                None
            }
        }
    }

    /// The canonical JSON the fingerprint digest runs over: the typed command
    /// serialized with the wire field names, key-sorted, compact.
    pub fn canonical_form(&self) -> String {
        let value =
            serde_json::to_value(self).expect("a typed command must serialize to its own JSON");
        canonical_json(&value)
    }
}

/// The revision cell every state-changing command shares with the SQL
/// CHECKs: a command whose `expectedRevision` is already at the ceiling
/// refuses (`expectedRevision >= 2147483647` on the TS side) so the +1 bump
/// can never wrap the int4 column. `pub` because the SQL adapters re-check
/// the same ceiling from this one constant, never from a re-typed literal.
pub const REVISION_MAX: i64 = 2_147_483_647;

/// The `unitCost`/`price` cell every schema draws
/// (`z.number().int().min(0).max(1000000000)` and the SQL `CHECK(unit_cost
/// BETWEEN 0 AND 1000000000)`).
pub const COST_MAX: i64 = 1_000_000_000;
const QUANTITY_MAX: i64 = 1_000_000;

/// The order-total cell the `total_estimate` column and the quote check
/// share (`Number.isSafeInteger && total <= 500000000000000` on the TS side).
/// `pub` so the SQL adapter enforces the same bound from ONE constant.
pub const TOTAL_MAX: i64 = 500_000_000_000_000;

fn as_i64(value: &Value, field: &'static str, min: i64, max: i64) -> Result<i64, ValidationError> {
    let number = value
        .as_i64()
        .filter(|number| *number >= min && *number <= max)
        .ok_or(ValidationError(field))?;
    Ok(number)
}

fn as_bool(value: &Value, field: &'static str) -> Result<bool, ValidationError> {
    value.as_bool().ok_or(ValidationError(field))
}

fn as_uuid(value: &Value, field: &'static str) -> Result<String, ValidationError> {
    let raw = value.as_str().ok_or(ValidationError(field))?;
    if !valid_uuid(raw) {
        return Err(ValidationError(field));
    }
    Ok(raw.to_string())
}

fn as_reason(value: &Value) -> Result<String, ValidationError> {
    // The schema's `.trim().min(1).max(2000)`.
    let raw = value.as_str().ok_or(ValidationError("reason"))?;
    bounded_title(raw, 2000).ok_or(ValidationError("reason"))
}

/// Parses an array of strictly-shaped `{offerId,quantity}` quote lines.
fn parse_line_inputs(value: &Value) -> Result<Vec<LineInput>, ValidationError> {
    let array = value.as_array().ok_or(ValidationError("lines"))?;
    if array.is_empty() || array.len() > 50 {
        return Err(ValidationError("lines"));
    }
    let mut parsed = Vec::with_capacity(array.len());
    for item in array {
        let object = item.as_object().ok_or(ValidationError("lines"))?;
        if object.len() != 2 {
            return Err(ValidationError("lines"));
        }
        let offer_id = as_uuid(
            object.get("offerId").ok_or(ValidationError("lines"))?,
            "lines",
        )?;
        let quantity = as_i64(
            object.get("quantity").ok_or(ValidationError("lines"))?,
            "lines",
            1,
            QUANTITY_MAX,
        )?;
        parsed.push(LineInput { offer_id, quantity });
    }
    // The schema's `superRefine`: offers must be unique.
    let mut seen: Vec<&str> = Vec::with_capacity(parsed.len());
    for line in &parsed {
        if seen.contains(&line.offer_id.as_str()) {
            return Err(ValidationError("lines-duplicate"));
        }
        seen.push(&line.offer_id);
    }
    Ok(parsed)
}

/// Parses an array of strictly-shaped receipt findings.
fn parse_receipt_findings(value: &Value) -> Result<Vec<ReceiptFinding>, ValidationError> {
    let array = value.as_array().ok_or(ValidationError("lines"))?;
    if array.is_empty() || array.len() > 50 {
        return Err(ValidationError("lines"));
    }
    let mut parsed = Vec::with_capacity(array.len());
    for item in array {
        let object = item.as_object().ok_or(ValidationError("lines"))?;
        if object.len() != 3 {
            return Err(ValidationError("lines"));
        }
        let line_id = as_uuid(
            object.get("lineId").ok_or(ValidationError("lines"))?,
            "lines",
        )?;
        let accepted = as_i64(
            object.get("accepted").ok_or(ValidationError("lines"))?,
            "lines",
            0,
            QUANTITY_MAX,
        )?;
        let rejected = as_i64(
            object.get("rejected").ok_or(ValidationError("lines"))?,
            "lines",
            0,
            QUANTITY_MAX,
        )?;
        // The schema refine: a finding must carry work, and never overflow
        // the line maximum — this is the partial-receipt floor, not a form
        // default.
        if accepted + rejected < 1 || accepted + rejected > QUANTITY_MAX {
            return Err(ValidationError("lines-sum"));
        }
        parsed.push(ReceiptFinding {
            line_id,
            accepted,
            rejected,
        });
    }
    let mut seen: Vec<&str> = Vec::with_capacity(parsed.len());
    for finding in &parsed {
        if seen.contains(&finding.line_id.as_str()) {
            return Err(ValidationError("lines-duplicate"));
        }
        seen.push(&finding.line_id);
    }
    Ok(parsed)
}

/// The parsed `update` triple every state-changing command carries.
fn parse_update(
    object: &serde_json::Map<String, Value>,
) -> Result<(String, i64, String), ValidationError> {
    let id = as_uuid(object.get("id").ok_or(ValidationError("id"))?, "id")?;
    let expected_revision = as_i64(
        object
            .get("expectedRevision")
            .ok_or(ValidationError("expectedRevision"))?,
        "expectedRevision",
        1,
        REVISION_MAX,
    )?;
    let reason = as_reason(object.get("reason").ok_or(ValidationError("reason"))?)?;
    Ok((id, expected_revision, reason))
}

/// The strict parse of the command union. Member shapes are matched exactly
/// (`z.discriminatedUnion(...).strict()` semantics: exactly the declared
/// fields), and every field carries the same bound the schema draws.
pub fn parse_command(raw: &Value) -> Result<PurchaseCommand, ValidationError> {
    let object = raw.as_object().ok_or(ValidationError("operation"))?;
    let operation = object
        .get("operation")
        .and_then(Value::as_str)
        .ok_or(ValidationError("operation"))?;
    let rest = object
        .iter()
        .filter(|(key, _)| key.as_str() != "operation")
        .count();
    match operation {
        "supplier.create" => {
            if rest != 2 {
                return Err(ValidationError("operation"));
            }
            let code = object
                .get("code")
                .and_then(Value::as_str)
                .filter(|code| valid_code(code))
                .ok_or(ValidationError("code"))?;
            let name = bounded_title(
                object.get("name").and_then(Value::as_str).unwrap_or(""),
                160,
            )
            .ok_or(ValidationError("name"))?;
            Ok(PurchaseCommand::SupplierCreate {
                code: code.to_string(),
                name,
            })
        }
        "supplier.set-active" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_update(object)?;
            let active = as_bool(
                object.get("active").ok_or(ValidationError("active"))?,
                "active",
            )?;
            Ok(PurchaseCommand::SupplierSetActive {
                id,
                expected_revision,
                reason,
                active,
            })
        }
        "offer.create" => {
            if rest != 3 {
                return Err(ValidationError("operation"));
            }
            let supplier_id = as_uuid(
                object
                    .get("supplierId")
                    .ok_or(ValidationError("supplierId"))?,
                "supplierId",
            )?;
            let product_id = as_uuid(
                object
                    .get("productId")
                    .ok_or(ValidationError("productId"))?,
                "productId",
            )?;
            let unit_cost = as_i64(
                object.get("unitCost").ok_or(ValidationError("unitCost"))?,
                "unitCost",
                0,
                COST_MAX,
            )?;
            Ok(PurchaseCommand::OfferCreate {
                supplier_id,
                product_id,
                unit_cost,
            })
        }
        "offer.set-cost" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_update(object)?;
            let unit_cost = as_i64(
                object.get("unitCost").ok_or(ValidationError("unitCost"))?,
                "unitCost",
                0,
                COST_MAX,
            )?;
            Ok(PurchaseCommand::OfferSetCost {
                id,
                expected_revision,
                reason,
                unit_cost,
            })
        }
        "offer.set-active" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_update(object)?;
            let active = as_bool(
                object.get("active").ok_or(ValidationError("active"))?,
                "active",
            )?;
            Ok(PurchaseCommand::OfferSetActive {
                id,
                expected_revision,
                reason,
                active,
            })
        }
        "order.create" => {
            if rest != 2 {
                return Err(ValidationError("operation"));
            }
            let supplier_id = as_uuid(
                object
                    .get("supplierId")
                    .ok_or(ValidationError("supplierId"))?,
                "supplierId",
            )?;
            let lines = parse_line_inputs(object.get("lines").ok_or(ValidationError("lines"))?)?;
            Ok(PurchaseCommand::OrderCreate { supplier_id, lines })
        }
        "order.replace-lines" => {
            if rest != 4 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_update(object)?;
            let lines = parse_line_inputs(object.get("lines").ok_or(ValidationError("lines"))?)?;
            Ok(PurchaseCommand::OrderReplaceLines {
                id,
                expected_revision,
                reason,
                lines,
            })
        }
        "order.confirm" | "order.cancel" | "order.short-close" => {
            if rest != 3 {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_update(object)?;
            Ok(match operation {
                "order.confirm" => PurchaseCommand::OrderConfirm {
                    id,
                    expected_revision,
                    reason,
                },
                "order.cancel" => PurchaseCommand::OrderCancel {
                    id,
                    expected_revision,
                    reason,
                },
                _ => PurchaseCommand::OrderShortClose {
                    id,
                    expected_revision,
                    reason,
                },
            })
        }
        "order.receive" => {
            if object.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "operation"
                        | "id"
                        | "expectedRevision"
                        | "reason"
                        | "lines"
                        | "landedFeeMinor"
                        | "allocationBasis"
                )
            }) {
                return Err(ValidationError("operation"));
            }
            let (id, expected_revision, reason) = parse_update(object)?;
            let lines =
                parse_receipt_findings(object.get("lines").ok_or(ValidationError("lines"))?)?;
            let landed_fee_minor = object
                .get("landedFeeMinor")
                .map(|v| as_i64(v, "landedFeeMinor", 0, 500_000_000_000_000))
                .transpose()?;
            let allocation_basis = object
                .get("allocationBasis")
                .map(|v| {
                    serde_json::from_value(v.clone())
                        .map_err(|_| ValidationError("allocationBasis"))
                })
                .transpose()?;
            Ok(PurchaseCommand::OrderReceive {
                id,
                expected_revision,
                reason,
                lines,
                landed_fee_minor,
                allocation_basis,
            })
        }
        _ => Err(ValidationError("operation")),
    }
}

/// One mutation submission: the idempotency key plus its command —
/// `purchaseMutationSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PurchaseMutation {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "command")]
    pub command: PurchaseCommand,
}

impl PurchaseMutation {
    /// The strict parse of the mutation envelope.
    pub fn parse(raw: &Value) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("requestId"))?;
        if object.len() != 2 {
            return Err(ValidationError("requestId"));
        }
        let request_id = as_uuid(
            object
                .get("requestId")
                .ok_or(ValidationError("requestId"))?,
            "requestId",
        )?;
        let command = parse_command(object.get("command").ok_or(ValidationError("command"))?)?;
        Ok(Self {
            request_id,
            command,
        })
    }
}

// --- Durable receipts and status -------------------------------------------

/// The receipt the mutation records and recovery replays —
/// `purchaseReceiptSchema`. Serialized with the wire key order kept stable
/// because a stored receipt must round-trip exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PurchaseReceipt {
    #[serde(rename = "requestId")]
    pub request_id: String,
    #[serde(rename = "operation")]
    pub operation: String,
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "revision")]
    pub revision: i64,
    #[serde(rename = "replayed")]
    pub replayed: bool,
}

/// The command-status union — `purchaseStatusSchema`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state")]
pub enum PurchaseCommandStatus {
    #[serde(rename = "unknown")]
    Unknown,
    #[serde(rename = "committed")]
    Committed { receipt: PurchaseReceipt },
}

/// Builds the replayed receipt: the stored one with `replayed` flipped. A
/// recovery IS a replay by construction — the fact a caller rediscovers the
/// effect after a lost response is exactly the rather recovered state.
pub fn replayed_receipt(receipt: &PurchaseReceipt) -> PurchaseReceipt {
    let mut replayed = receipt.clone();
    replayed.replayed = true;
    replayed
}

// --- Authorization (the pure grant rule) -----------------------------------

/// A resolved purchasing-side access — the parsed membership grant plus the
/// owner id the scope narrows reads and writes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PurchaseAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl PurchaseAccess {
    /// Whether the access holds EVERY named permission — the `grant()` rule:
    /// ``p.some(x => !a.permissions.includes(x))`` throws, so a missing
    /// permission is a refusal even when one of the others is held.
    fn grants(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// Public form of [`Self::grants`] for adapters that must reproduce the
    /// repository's `grant()`/`ownedOrder` checks.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        self.grants(wanted)
    }

    /// The repository's `global(a)` for read/write paths that demand an
    /// unassigned grant.
    pub fn is_global(&self) -> bool {
        self.scope.covers_all()
    }
}

/// A resolved commerce-side access, as the stock-binding rules see it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommerceAccess {
    pub permissions: Vec<String>,
    pub scope: ErpScope,
}

impl CommerceAccess {
    /// Same all-must-held rule as the purchasing side (see [`PurchaseAccess::grants`]).
    fn grants(&self, wanted: &[&str]) -> bool {
        wanted
            .iter()
            .all(|needed| self.permissions.iter().any(|held| held == needed))
    }

    /// Public form of [`Self::grants`] for adapters that must reproduce the
    /// repository's `grant(a, permission)`/read-path checks.
    pub fn holds(&self, wanted: &[&str]) -> bool {
        self.grants(wanted)
    }
}

/// One context: both ledger grants resolved in ONE transaction, after the
/// service-on-behalf intersection has been applied.
#[derive(Debug, Clone)]
pub struct PurchaseContext {
    pub purchasing: PurchaseAccess,
    pub commerce: CommerceAccess,
}

impl PurchaseContext {
    /// Applies the commerce scope rule the baseline closes the transaction
    /// with: an `assigned` commerce grant narrows the purchasing side too —
    /// `if (a.commerce.scope === 'assigned') a.scope = 'assigned'`.
    pub fn narrowed(self) -> Self {
        let narrowed = match self.commerce.scope {
            ErpScope::Assigned => ErpScope::Assigned,
            ErpScope::All => self.purchasing.scope,
        };
        Self {
            purchasing: PurchaseAccess {
                permissions: self.purchasing.permissions,
                owner_id: self.purchasing.owner_id,
                scope: narrowed,
            },
            commerce: self.commerce,
        }
    }
}

/// The pure authorize rule — port of `authorize(a, c)` in
/// `packages/purchasing-pg/src/repository.ts:68-89`.
///
/// Supplier and offer operations additionally require the unassigned scope
/// (`global(a)`); `order.create`/`replace-lines`/`confirm` add supplier and
/// offer read; `order.receive` is the cross-ledger gate: purchasing
/// receipts grants, commerce stock grants, and a commerce scope that covers
/// the whole tenant, because the physical-stock write it performs is not a
/// personally-assigned act.
pub fn authorize_operation(context: &PurchaseContext, operation: &str) -> Result<(), ErpError> {
    let purchaser = &context.purchasing;
    let commerce = &context.commerce;
    let global = || -> Result<(), ErpError> {
        if purchaser.scope.covers_all() && commerce.scope.covers_all() {
            Ok(())
        } else {
            Err(ErpError::Forbidden)
        }
    };
    if operation.starts_with("supplier.") {
        if !purchaser.grants(&["suppliers.read", "suppliers.write"]) {
            return Err(ErpError::Forbidden);
        }
        return global();
    }
    if operation.starts_with("offer.") {
        if !purchaser.grants(&["offers.read", "offers.write", "suppliers.read"]) {
            return Err(ErpError::Forbidden);
        }
        return global();
    }
    if !purchaser.grants(&["orders.read", "orders.write"]) {
        return Err(ErpError::Forbidden);
    }
    if matches!(
        operation,
        "order.create" | "order.replace-lines" | "order.confirm"
    ) && !purchaser.grants(&["suppliers.read", "offers.read"])
    {
        return Err(ErpError::Forbidden);
    }
    if operation == "order.receive" {
        if !purchaser.grants(&["receipts.read", "receipts.write"]) {
            return Err(ErpError::Forbidden);
        }
        if !commerce.grants(&["stock.read", "stock.write"]) {
            return Err(ErpError::Forbidden);
        }
        // Same narrowed-scope refusal, before any write.
        if !commerce.scope.covers_all() {
            return Err(ErpError::Forbidden);
        }
    }
    Ok(())
}

/// The recovery authorization — `recovery()` re-runs the CURRENT grant rule
/// over the stored receipt's operation, and for the order-retaining branches
/// re-checks that the order is one the CURRENT scope reaches. An old receipt
/// is not a permanent grant: revocation is visible to recovery too.
pub fn authorize_recovery(
    context: &PurchaseContext,
    receipt: &PurchaseReceipt,
) -> Result<(), ErpError> {
    authorize_operation(context, receipt.operation.as_str())
}
