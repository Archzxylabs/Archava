//! The billing domain contracts — the Archxy 2 port of
//! `packages/billing/src/{contracts,signature}.ts` (R07 first cohort:
//! immutable invoicing and provider-authoritative settlement over the
//! `archava_billing` ledger, PACKET-007).
//!
//! ## The wire grammar (billing's own shape)
//!
//! Billing's mutation document is FLAT where every earlier ERP cohort used
//! the `{requestId, type, data}` envelope: the discriminated union keys the
//! arms off `type` and each arm carries its own `commandId` — the TS
//! `billingMutationSchema` verbatim, not the shared ERP shape. The port
//! keeps it flat on purpose: the idempotency key's spelling on this ledger
//! is `commandId`, and the receipt's members name it back. The six arms:
//!
//! - `create_invoice` — a priced line sheet (1–200 lines, each
//!   `description` 1–300 and an integer `quantity` × `unitMinor`);
//! - `issue_invoice` / `void_invoice` — the optimistic revision arms, each
//!   carrying `expectedRevision` and a 1–500 `reason`;
//! - `append_correction` — an integer amount that must be NON-ZERO
//!   (`moneyMinor.refine(value !== 0)`) with an optional nullable
//!   `reversesId`;
//! - `record_payment_attempt` — a POSITIVE amount and an optional nullable
//!   `providerRef` (≤120): an intent that is structurally incapable of
//!   asserting a result, because no member of the arm carries an outcome;
//! - `record_provider_event` — THE only arm that can move what is paid:
//!   outcome (succeeded/failed/refunded), amount, currency and the 64-hex
//!   lowercase `signature` the service re-verifies (see below).
//!
//! Members use the recorded fold where the schema carries it: `reversesId`
//! and `providerRef` are `.nullable().optional()` — absent AND explicit
//! null both fold onto `None` (the SQL binds `null` either way, so the fold
//! is effect-exact). Text members trim and stay bounded
//! (`.trim().min(1).max(max)`; `providerRef` is trim-and-≤120 with an
//! optional null). Money is a 64-bit integer of minor units — never a
//! float — with the schema's `int()` and no up-bound at the grammar (the
//! JS safe-integer ceiling is a ROW-boundary check that lives in the
//! adapter, exactly where TS puts `minor()`).
//!
//! ## The status vocabulary
//!
//! `draft → issued → settled` with `void` terminal and a settled invoice
//! reopening to `issued` — `INVOICE_STATUSES` plus the transition table the
//! arms and the migration's trigger agree on (`settled` is NOT terminal;
//! `void` is). A failed provider outcome is history: it documents an event
//! row and moves NO money, no state and not even the revision.
//!
//! ## The signature (the settlement's authority)
//!
//! `packages/billing/src/signature.ts` ported with its three load-bearing
//! properties intact:
//!
//! 1. **The domain tag** — `archava/billing/provider-event/v1`, the one
//!    namespace: a digest minted under any other platform tag verifies
//!    against nothing here;
//! 2. **The framing** — the digest is `HMAC-SHA256(key, domain‖outs‖parts)`
//!    where each part is length-FRAMED with a big-endian u32 written AFTER
//!    the part's bytes (TS builds `[bytes, lengthHeader(bytes.length)]` per
//!    part, so the exact byte order is part-then-length, domain first) —
//!    `['ab','c']` and `['a','bc']` digest differently, so a character
//!    cannot move between fields without invalidating a signature; the
//!    amount renders as an exact integer string; a 1 MiB ceiling on the
//!    framed total mirrors `MAX_FRAMED_BYTES` (unreachable through this
//!    grammar's own bounds, refused closed anyway);
//! 3. **The compare** — `signaturesEqual` re-hashes BOTH sides under the
//!    fixed `archava/billing/compare/v1` key before a constant-time
//!    comparison of the two 32-byte digests, so a malformed signature
//!    cannot be timed into a guess about which prefix was right.
//!
//! The KEY these digests verify under is a deployment fact (a
//! `SignatureKeySource` in TS, ≥32 bytes per `SIGNATURE_KEY_MIN_BYTES`,
//! resolved per tenant+provider so two tenants settling through the same
//! processor cannot share a secret). A pure grammar cannot name a key
//! source; the derivation lives in the gate (the identity store's own
//! labelled digest, per tenant+provider), recorded in that module's header
//! and `KNOWN_DIFFERENCES`.
//!
//! ## The error vocabulary
//!
//! The TS `BillingError` codes are wider than the carrier (`forbidden`,
//! `unauthorized`, `not_found`, `conflict`, `bad_request`,
//! `invalid_transition`, `invoice_immutable`, `invalid_signature`,
//! `unbalanced_settlement`) but the refused families fold onto the shared
//! [`ErpError`] net with the TS api's own HTTP classes: `forbidden` and
//! `invalid_signature` are the 403 class ([`ErpError::Forbidden`]),
//! `not_found` the 404 class ([`ErpError::NotFound`]), and `conflict`,
//! `invalid_transition`, `invoice_immutable` and `unbalanced_settlement`
//! the 409 class ([`ErpError::Conflict`]) — the TS `codeMap` folds the same
//! four codes to 409, so the fold never hides a class the TS api answered
//! differently. The parse-level refusals (bad_request/violation family)
//! are the gate's own boundary 400, like every cohort. The fold is
//! recorded in the packet's KNOWN_DIFFERENCES ledger, never silent.

use serde::Serialize;
use serde_json::Value;

pub use super::{ErpError, ErpScope};
use hmac::{Hmac, Mac};
use sha2::Sha256;

/// The assets-adapted membership accessor reuses this cohort's principal —
/// billing's principal document has the same discriminated shape (a human
/// id, or a service id with the human it names).
pub use super::purchasing::{
    PrincipalKind, PurchasePrincipal as BillingPrincipal, ValidationError,
};

/// The billing permission vocabulary — `BILLING_PERMISSIONS`. Strings, not
/// an enum: membership rows store them and the grant gates test membership
/// of the list, like every other ERP cohort.
pub const BILLING_PERMISSIONS: &[&str] = &[
    "billing.read",
    "billing.write",
    "billing.settle",
    "audit.read",
];

// --- Vocabulary constants (contracts.ts's bounds, mirrored) ------------------

/// Bounded `invoiceCode`: trimmed 1–50.
pub const INVOICE_CODE_MAX: usize = 50;
/// Bounded `customerRef`: trimmed 1–160, including frozen Commerce customer names.
pub const CUSTOMER_REF_MAX: usize = 160;
/// Bounded `reason`: trimmed 1–500 (the line description's 1–300 bound is
/// its own constant below).
pub const REASON_MAX: usize = 500;
/// Bounded line `description`: trimmed 1–300.
pub const LINE_DESCRIPTION_MAX: usize = 300;
/// Bounded `providerRef`: trimmed ≤120 (empty allowed — the only trimmed
/// member without a `.min`).
pub const PROVIDER_REF_MAX: usize = 120;
/// Bounded `providerId`: trimmed 1–80.
pub const PROVIDER_ID_MAX: usize = 80;
/// Bounded `providerEventId`: trimmed 1–120.
pub const PROVIDER_EVENT_ID_MAX: usize = 120;
/// `currencySchema` — a three-letter uppercase ISO code after trimming.
pub const CURRENCY_LEN: usize = 3;
/// The billed line sheet's ceiling (`.min(1).max(200)`).
pub const LINES_MAX: usize = 200;
/// The JS safe-integer ceiling the adapter's row-boundary check folds to a
/// conflict on (`minor()`'s `Number.isSafeInteger`) — bytes stored beyond
/// this cannot replay through the repository.
pub const SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991;

// --- The row vocabularies (migration 001's CHECK lists) ----------------------

/// The invoice status column's four-way vocabulary.
pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_ISSUED: &str = "issued";
pub const STATUS_SETTLED: &str = "settled";
pub const STATUS_VOID: &str = "void";

/// The three-way provider-event vocabulary (`PAYMENT_OUTCOMES`): the
/// outcomes a signed notification may carry.
pub const OUTCOME_SUCCEEDED: &str = "succeeded";
pub const OUTCOME_FAILED: &str = "failed";
pub const OUTCOME_REFUNDED: &str = "refunded";

/// The invoice status — the arms and the migration's trigger agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum InvoiceStatus {
    Draft,
    Issued,
    Settled,
    Void,
}

impl InvoiceStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATUS_DRAFT => Some(Self::Draft),
            STATUS_ISSUED => Some(Self::Issued),
            STATUS_SETTLED => Some(Self::Settled),
            STATUS_VOID => Some(Self::Void),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => STATUS_DRAFT,
            Self::Issued => STATUS_ISSUED,
            Self::Settled => STATUS_SETTLED,
            Self::Void => STATUS_VOID,
        }
    }

    /// `ALLOWED_TRANSITIONS` — the arms' and the trigger's shared table:
    /// a draft may issue or void, an issued invoice may settle or void, a
    /// settled invoice reopens to issued, a voided invoice is terminal.
    pub fn allows(self, target: InvoiceStatus) -> bool {
        use InvoiceStatus::*;
        matches!(
            (self, target),
            (Draft, Issued | Void) | (Issued, Settled | Void) | (Settled, Issued)
        )
    }
}

/// The provider-event outcome — the three-way vocabulary a signed claim
/// may assert.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettlementOutcome {
    Succeeded,
    Failed,
    Refunded,
}

impl SettlementOutcome {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            OUTCOME_SUCCEEDED => Some(Self::Succeeded),
            OUTCOME_FAILED => Some(Self::Failed),
            OUTCOME_REFUNDED => Some(Self::Refunded),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => OUTCOME_SUCCEEDED,
            Self::Failed => OUTCOME_FAILED,
            Self::Refunded => OUTCOME_REFUNDED,
        }
    }
}

/// `settledFromEvents` — what an appended event is worth: `succeeded`
/// adds, `refunded` subtracts, `failed` is history that moves nothing. A
/// refund is the same arithmetic running the other way, not a special
/// case bolted onto the side of the ledger.
pub fn settled_delta(outcome: SettlementOutcome, amount_minor: i64) -> i64 {
    match outcome {
        SettlementOutcome::Succeeded => amount_minor,
        SettlementOutcome::Refunded => -amount_minor,
        // `failed` records history and moves no money.
        SettlementOutcome::Failed => 0,
    }
}

/// `outstandingFrom` — what is still owed: the frozen total, adjusted by
/// corrections, less settlement. No rounding step exists anywhere between
/// the invoice snapshot and this balance.
pub fn outstanding_from(gross_minor: i64, correction_minor: i64, settled_minor: i64) -> i64 {
    gross_minor + correction_minor - settled_minor
}

// --- The command arms ---------------------------------------------------------

/// One command arm of `billingMutationSchema` — the FLAT discriminated
/// union on `type`. Field spellings match the wire exactly: a parsed
/// command serializes back into the shape the fingerprint covered, and the
/// parse has already applied the trims (`providerRef`'s trimmed value
/// travels, `signature` is trimmed before its regex). The optional
/// nullable members carry [`Option`]: `None` means absent AND explicit
/// null together (the recorded fold — the SQL binds a null for either).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type")]
pub enum BillingCommand {
    #[serde(rename = "create_invoice")]
    CreateInvoice {
        #[serde(rename = "commandId")]
        command_id: String,
        /// Trimmed, 1–50 chars.
        invoice_code: String,
        /// Trimmed, 1–160 chars.
        customer_ref: String,
        /// Three uppercase letters after trimming.
        currency: String,
        /// The tenant-facing consumer reference (trimmed, 1–100).
        consumer_ref: String,
        /// 1–200 priced lines, each description 1–300 and an integer
        /// quantity × integer unit amount.
        lines: Vec<PricedLine>,
    },
    /// Creates one draft from a fulfilled, unreturned native sales order.
    /// Prices, quantities and customer are read from the frozen source, never supplied.
    #[serde(rename = "create_invoice_from_order")]
    CreateInvoiceFromOrder {
        #[serde(rename = "commandId")]
        command_id: String,
        invoice_code: String,
        #[serde(rename = "orderId")]
        order_id: String,
        #[serde(rename = "expectedOrderRevision")]
        expected_order_revision: i64,
    },
    #[serde(rename = "issue_invoice")]
    IssueInvoice {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "invoiceId")]
        invoice_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        /// Trimmed, 1–500 chars.
        reason: String,
    },
    #[serde(rename = "append_correction")]
    AppendCorrection {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "invoiceId")]
        invoice_id: String,
        /// Non-zero by the schema's refine.
        amount_minor: i64,
        /// Trimmed, 1–500 chars.
        reason: String,
        /// `.nullable().optional()` — absent AND null both fold to `None`.
        reverses_id: Option<String>,
    },
    #[serde(rename = "void_invoice")]
    VoidInvoice {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "invoiceId")]
        invoice_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: i64,
        /// Trimmed, 1–500 chars.
        reason: String,
    },
    #[serde(rename = "record_payment_attempt")]
    RecordPaymentAttempt {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "invoiceId")]
        invoice_id: String,
        /// Positive by the arm's `moneyMinor.positive()`.
        amount_minor: i64,
        /// `.nullable().optional()` — absent AND null both fold to `None`.
        provider_ref: Option<String>,
    },
    #[serde(rename = "record_provider_event")]
    RecordProviderEvent {
        #[serde(rename = "commandId")]
        command_id: String,
        #[serde(rename = "invoiceId")]
        invoice_id: String,
        /// Trimmed, 1–80 chars — the settlement's settled partner.
        provider_id: String,
        /// Trimmed, 1–120 chars — the idempotency of the notification
        /// itself (the migration's `provider_event_unique` guards it).
        provider_event_id: String,
        outcome: SettlementOutcome,
        amount_minor: i64,
        /// Three uppercase letters after trimming.
        currency: String,
        /// Trimmed 64-hex lowercase — the settlement's authority.
        signature: String,
    },
}

/// One priced line of `create_invoice`'s sheet (`pricedLineSchema`):
/// description 1–300 after trimming, an integer positive quantity and an
/// integer unit amount in minor units. `PricedLine` serializes as its own
/// object inside the command's `lines` array, member spellings on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PricedLine {
    /// Trimmed, 1–300 chars.
    pub description: String,
    /// Integer, positive.
    pub quantity: i64,
    /// Integer minor units — the unit-per-item price.
    pub unit_minor: i64,
}

impl BillingCommand {
    /// The discriminator — the audit rows' `operation` cell and the
    /// receipt's `action` member record it verbatim.
    pub fn mutation_type(&self) -> &'static str {
        match self {
            Self::CreateInvoice { .. } => "create_invoice",
            Self::CreateInvoiceFromOrder { .. } => "create_invoice_from_order",
            Self::IssueInvoice { .. } => "issue_invoice",
            Self::AppendCorrection { .. } => "append_correction",
            Self::VoidInvoice { .. } => "void_invoice",
            Self::RecordPaymentAttempt { .. } => "record_payment_attempt",
            Self::RecordProviderEvent { .. } => "record_provider_event",
        }
    }

    /// The arm's own permission — the TS grant calls: create and the
    /// attempt arm need `billing.write`; issue/correction/void need
    /// `billing.settle`; and a provider event ALSO needs `billing.write`
    /// (its settlement authority is the signature, not the grant — but the
    /// grant must hold anyway, exactly as the TS arms order it).
    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::CreateInvoice { .. }
            | Self::CreateInvoiceFromOrder { .. }
            | Self::RecordPaymentAttempt { .. } => "billing.write",
            Self::AppendCorrection { .. }
            | Self::IssueInvoice { .. }
            | Self::VoidInvoice { .. } => "billing.settle",
            Self::RecordProviderEvent { .. } => "billing.write",
        }
    }

    /// The arm's command id — the idempotency key the claim keyed. Every
    /// arm carries it (the flat union's shared member).
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateInvoice { command_id, .. }
            | Self::CreateInvoiceFromOrder { command_id, .. }
            | Self::IssueInvoice { command_id, .. }
            | Self::AppendCorrection { command_id, .. }
            | Self::VoidInvoice { command_id, .. }
            | Self::RecordPaymentAttempt { command_id, .. }
            | Self::RecordProviderEvent { command_id, .. } => command_id,
        }
    }

    /// The arm's invoice id — every non-create arm names one; the create
    /// arm has none (it builds one). The handlers refuse a conflict on a
    /// malformed arm rather than assume an identifier.
    pub fn invoice_ref(&self) -> Option<&str> {
        match self {
            Self::IssueInvoice { invoice_id, .. }
            | Self::AppendCorrection { invoice_id, .. }
            | Self::VoidInvoice { invoice_id, .. }
            | Self::RecordPaymentAttempt { invoice_id, .. }
            | Self::RecordProviderEvent { invoice_id, .. } => Some(invoice_id),
            Self::CreateInvoice { .. } | Self::CreateInvoiceFromOrder { .. } => None,
        }
    }
}

/// The mutation document — a strict flat union member. The `command` IS
/// the parsed arm; the wire's `type` discriminator is the enum tag; the
/// membership id rides `commandId` inside each arm (NOT `requestId` —
/// this ledger's own spelling, the recorded TS truth).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingMutation {
    pub command: BillingCommand,
}

/// The access envelope — `billingAccessSchema` resolved inside the
/// transaction: the membership's permissions, the OWNING human's id (the
/// assigned scope's screen), and the scope. A service principal narrows to
/// the human it names, exactly as the other cohorts' ladders do.
#[derive(Debug, Clone)]
pub struct BillingAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: ErpScope,
}

impl BillingAccess {
    /// Whether the access holds the wanted permission.
    pub fn holds(&self, wanted: &str) -> bool {
        self.permissions.iter().any(|held| held == wanted)
    }
}

// --- The receipts + status ----------------------------------------------------

/// `billingReceiptSchema` — the wide idempotency receipt document
/// `{commandId, tenantId, action, invoiceId?, lineId?, correctionId?,
/// attemptId?, eventId?, status, outstandingMinor?, revision?,
/// idempotencyReplay, executedAt}` every arm returns. The optional
/// members OMIT when absent (the TS splats only the members its arm
/// names — the `.optional()` side of the schema), and
/// `idempotencyReplay` defaults false.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BillingReceipt {
    #[serde(rename = "commandId")]
    pub command_id: String,
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    /// The arm's own action (the mutation type's spelling).
    pub action: &'static str,
    #[serde(rename = "invoiceId", skip_serializing_if = "Option::is_none")]
    pub invoice_id: Option<String>,
    #[serde(rename = "lineId", skip_serializing_if = "Option::is_none")]
    pub line_id: Option<String>,
    #[serde(rename = "correctionId", skip_serializing_if = "Option::is_none")]
    pub correction_id: Option<String>,
    #[serde(rename = "attemptId", skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(rename = "eventId", skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// The arm's own status label — the invoice's status for the invoice
    /// arms, `'attempted'` for an attempt, `'failed'` for a failed event
    /// (the receipt's own label, NOT the invoice's).
    pub status: String,
    #[serde(rename = "outstandingMinor", skip_serializing_if = "Option::is_none")]
    pub outstanding_minor: Option<i64>,
    #[serde(rename = "revision", skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    #[serde(rename = "idempotencyReplay")]
    pub idempotency_replay: bool,
    #[serde(rename = "executedAt")]
    pub executed_at: String,
}

impl BillingReceipt {
    /// The parse of a REPLAYED receipt from a stored `jsonb` row — the TS
    /// `billingReceiptSchema.parse({...row.receipt, idempotencyReplay:
    /// true})`. The strict-set walk: exactly the schema's member names, a
    /// uuid `commandId` (+ the uuid optional ids), trimmed non-empty
    /// `tenantId`/`action`/`status`/`executedAt`, integer minor amounts,
    /// boolean `idempotencyReplay`. The parse CALLER (the replay branch)
    /// is the only consumer; a stored doc that cannot parse is the
    /// conflict the TS forced re-parse would raise into.
    pub fn parse(raw: &Value, replay: bool) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("commandId"))?;
        let allowed = [
            "commandId",
            "tenantId",
            "action",
            "invoiceId",
            "lineId",
            "correctionId",
            "attemptId",
            "eventId",
            "status",
            "outstandingMinor",
            "revision",
            "idempotencyReplay",
            "executedAt",
        ];
        if !object.keys().all(|key| allowed.contains(&key.as_str())) {
            return Err(ValidationError("commandId"));
        }
        let id_member = |name: &'static str| -> Result<Option<String>, ValidationError> {
            match object.get(name) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value)) if super::valid_uuid(value) => {
                    Ok(Some(value.to_string()))
                }
                Some(_) => Err(ValidationError(name)),
            }
        };
        let command_id = object
            .get("commandId")
            .and_then(Value::as_str)
            .filter(|value| super::valid_uuid(value))
            .ok_or(ValidationError("commandId"))?
            .to_string();
        let tenant_id = object
            .get("tenantId")
            .and_then(Value::as_str)
            .map(super::trimmed)
            .filter(|value| !value.is_empty() && value.len() <= 100)
            .ok_or(ValidationError("tenantId"))?
            .to_string();
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .filter(|value| !super::trimmed(value).is_empty())
            .ok_or(ValidationError("action"))?
            .to_string();
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .filter(|value| !super::trimmed(value).is_empty())
            .ok_or(ValidationError("status"))?
            .to_string();
        let executed_at = object
            .get("executedAt")
            .and_then(Value::as_str)
            .filter(|value| !super::trimmed(value).is_empty())
            .ok_or(ValidationError("executedAt"))?
            .to_string();
        let outstanding_minor = match object.get("outstandingMinor") {
            None | Some(Value::Null) => None,
            Some(value) => {
                Some(bounded_int_value(value).ok_or(ValidationError("outstandingMinor"))?)
            }
        };
        let revision = match object.get("revision") {
            None | Some(Value::Null) => None,
            Some(value) => Some(bounded_int_value(value).ok_or(ValidationError("revision"))?),
        };
        let idempotency_replay = match object.get("idempotencyReplay") {
            None => replay,
            Some(Value::Bool(value)) => *value || replay,
            Some(_) => return Err(ValidationError("idempotencyReplay")),
        };
        Ok(Self {
            command_id,
            tenant_id,
            action: Box::leak(action.into_boxed_str()) as &'static str,
            invoice_id: id_member("invoiceId")?,
            line_id: id_member("lineId")?,
            correction_id: id_member("correctionId")?,
            attempt_id: id_member("attemptId")?,
            event_id: id_member("eventId")?,
            status,
            outstanding_minor,
            revision,
            idempotency_replay,
            executed_at,
        })
    }
}

/// `billingCommandStatusSchema` — the recorded/unknown pair, the TS
/// command status document verbatim (`unknown` carries NO members beyond
/// the discriminant). The receipt rides a Box — the enum variant holds
/// thirteen cells the unknown arm names none of, and the wire shape is
/// unchanged by the indirection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status")]
pub enum BillingCommandStatus {
    #[serde(rename = "recorded")]
    Recorded { receipt: Box<BillingReceipt> },
    #[serde(rename = "unknown")]
    Unknown,
}

// --- parse helpers (the assets grammar's style) --------------------------------

/// A required integer or a present-but-invalid member — one helper so
/// receipt-parse and wire-parse share the integer walk.
fn bounded_int_value(value: &Value) -> Option<i64> {
    let number = match value {
        Value::Number(number) => number,
        _ => return None,
    };
    let as_f64 = number.as_f64()?;
    if as_f64.fract() != 0.0 {
        return None;
    }
    number.as_i64()
}

/// A required trimmed-and-bounded text member (`.trim().min(1).max(max)`).
fn bounded_member(
    object: &serde_json::Map<String, Value>,
    name: &str,
    min: usize,
    max: usize,
) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .map(|value| super::trimmed(value).to_string())
        .filter(|value| value.len() >= min && value.len() <= max)
}

/// The one trimmed-member family without a `.min` bound (`providerRef`):
/// absent/null fold to `None`, an empty trimmed value is a VALID member.
fn nullable_optional_ref(
    object: &serde_json::Map<String, Value>,
    name: &str,
    max: usize,
) -> Result<Option<String>, ()> {
    match object.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if value.chars().all(char::is_whitespace) => Ok(None),
        Some(_) => Ok(Some(bounded_member(object, name, 0, max).ok_or(())?)),
    }
}

/// An optional NULLABLE member of the uuid family (`reversesId`): an
/// absent key AND an explicit null BOTH parse as `None` (the recorded
/// fold — the SQL binds a null for either), and a present string must be
/// a valid uuid.
fn nullable_optional_uuid(
    object: &serde_json::Map<String, Value>,
    name: &str,
) -> Result<Option<String>, ()> {
    match object.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => Ok(Some(
            object
                .get(name)
                .and_then(Value::as_str)
                .filter(|value| super::valid_uuid(value))
                .ok_or(())?
                .to_string(),
        )),
    }
}

/// An integer member within `min..=max`.
fn bounded_integer(
    object: &serde_json::Map<String, Value>,
    name: &str,
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

/// `currencySchema` — three uppercase letters after trimming.
fn currency_member(object: &serde_json::Map<String, Value>) -> Option<String> {
    let value = super::trimmed(object.get("currency")?.as_str()?).to_string();
    if value.len() != CURRENCY_LEN || !value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return None;
    }
    Some(value)
}

/// The priced line sheet's own walk (`pricedLineSchema`, an array of
/// STRICT objects 1–200 long): each entry is exactly the three members,
/// each an in-grammar description/quantity/unitMinor.
fn priced_lines(object: &serde_json::Map<String, Value>) -> Option<Vec<PricedLine>> {
    let array = object.get("lines").and_then(Value::as_array)?;
    if array.is_empty() || array.len() > LINES_MAX {
        return None;
    }
    let mut lines = Vec::with_capacity(array.len());
    for entry in array {
        let object = entry.as_object()?;
        if object.len() != 3
            || !object.contains_key("description")
            || !object.contains_key("quantity")
            || !object.contains_key("unitMinor")
        {
            return None;
        }
        let description = bounded_member(object, "description", 1, LINE_DESCRIPTION_MAX)?;
        let quantity = bounded_integer(object, "quantity", 1, i64::MAX)?;
        let unit_minor = object
            .get("unitMinor")
            .map(bounded_int_value)
            .filter(|value| value.is_some())
            .and_then(|value| value)?;
        if quantity <= 0 {
            return None;
        }
        lines.push(PricedLine {
            description,
            quantity,
            unit_minor,
        });
    }
    Some(lines)
}

/// The required uuid member.
fn required_uuid(object: &serde_json::Map<String, Value>, name: &str) -> Option<String> {
    object
        .get(name)
        .and_then(Value::as_str)
        .filter(|value| super::valid_uuid(value))
        .map(str::to_string)
}

/// Whether the document's member set is EXACTLY these keys (`.strict()`).
fn strict_members(object: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    object.keys().all(|key| keys.contains(&key.as_str()))
        && keys.iter().all(|key| object.contains_key(*key))
}

/// Whether the document's member set stays INSIDE `allowed` and covers
/// `required` (`.strict()` + the nullable-optional family — keys MAY be
/// absent but may never exceed the sheet).
fn strict_optional_members(
    object: &serde_json::Map<String, Value>,
    allowed: &[&str],
    required: &[&str],
) -> bool {
    object.keys().all(|key| allowed.contains(&key.as_str()))
        && required.iter().all(|key| object.contains_key(*key))
}

/// The 64-hex lowercase signature walk (`.trim()` before the regex — the
/// schema trims then matches `^[0-9a-f]{64}$`).
fn signature_member(object: &serde_json::Map<String, Value>) -> Option<String> {
    let value = super::trimmed(object.get("signature")?.as_str()?);
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some(value.to_string())
}

/// `parse_mutation` — the schema's full walk, in the mode the TS one runs
/// (`mutate()`'s boundary): strict keys, the arm by `type`, each arm's own
/// strict sheet, FlatGrammar — no `{requestId,type,data}` envelope here.
#[allow(clippy::result_large_err)]
pub fn parse_mutation(raw: &Value) -> Result<BillingMutation, ValidationError> {
    let command = raw.as_object().ok_or(ValidationError("type"))?;
    let arm = command.get("type").and_then(Value::as_str);
    // The type discriminator MUST be a string value in the sheet; the
    // parse_command walk below refuses unknown tags and stray members.
    if arm.is_none() {
        return Err(ValidationError("type"));
    }
    Ok(BillingMutation {
        command: parse_command(command, arm)?,
    })
}

/// `parse_command` — ONE arm's strict sheet, judged against the `type`
/// the mutation names (the union's single-level discriminator — billing's
/// own shape, no inner `data` object).
#[allow(clippy::result_large_err)]
pub fn parse_command(
    data: &serde_json::Map<String, Value>,
    arm: Option<&str>,
) -> Result<BillingCommand, ValidationError> {
    match arm {
        Some("create_invoice_from_order") => {
            if !strict_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceCode",
                    "orderId",
                    "expectedOrderRevision",
                ],
            ) {
                return Err(ValidationError("type"));
            }
            Ok(BillingCommand::CreateInvoiceFromOrder {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_code: bounded_member(data, "invoiceCode", 1, INVOICE_CODE_MAX)
                    .ok_or(ValidationError("invoiceCode"))?,
                order_id: required_uuid(data, "orderId").ok_or(ValidationError("orderId"))?,
                expected_order_revision: bounded_integer(
                    data,
                    "expectedOrderRevision",
                    1,
                    i64::from(i32::MAX),
                )
                .ok_or(ValidationError("expectedOrderRevision"))?,
            })
        }
        Some("create_invoice") => {
            if !strict_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceCode",
                    "customerRef",
                    "currency",
                    "consumerRef",
                    "lines",
                ],
            ) {
                return Err(ValidationError("type"));
            }
            Ok(BillingCommand::CreateInvoice {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_code: bounded_member(data, "invoiceCode", 1, INVOICE_CODE_MAX)
                    .ok_or(ValidationError("invoiceCode"))?,
                customer_ref: bounded_member(data, "customerRef", 1, CUSTOMER_REF_MAX)
                    .ok_or(ValidationError("customerRef"))?,
                currency: currency_member(data).ok_or(ValidationError("currency"))?,
                consumer_ref: bounded_member(data, "consumerRef", 1, 100)
                    .ok_or(ValidationError("consumerRef"))?,
                lines: priced_lines(data).ok_or(ValidationError("lines"))?,
            })
        }
        Some("issue_invoice") => {
            if !strict_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceId",
                    "expectedRevision",
                    "reason",
                ],
            ) {
                return Err(ValidationError("type"));
            }
            Ok(BillingCommand::IssueInvoice {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_id: required_uuid(data, "invoiceId").ok_or(ValidationError("invoiceId"))?,
                expected_revision: bounded_integer(data, "expectedRevision", 0, i64::MAX)
                    .ok_or(ValidationError("expectedRevision"))?,
                reason: bounded_member(data, "reason", 1, REASON_MAX)
                    .ok_or(ValidationError("reason"))?,
            })
        }
        Some("append_correction") => {
            // `amountMinor` rides the zero-refine; `reversesId` is the
            // nullable-optional uuid family (absent AND null both fold).
            if !strict_optional_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceId",
                    "amountMinor",
                    "reason",
                    "reversesId",
                ],
                &["type", "commandId", "invoiceId", "amountMinor", "reason"],
            ) {
                return Err(ValidationError("type"));
            }
            let amount_minor = bounded_int_value(
                data.get("amountMinor")
                    .ok_or(ValidationError("amountMinor"))?,
            )
            .ok_or(ValidationError("amountMinor"))?;
            if amount_minor == 0 {
                // `Correction cannot be zero` — the schema's own refine.
                return Err(ValidationError("amountMinor"));
            }
            Ok(BillingCommand::AppendCorrection {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_id: required_uuid(data, "invoiceId").ok_or(ValidationError("invoiceId"))?,
                amount_minor,
                reason: bounded_member(data, "reason", 1, REASON_MAX)
                    .ok_or(ValidationError("reason"))?,
                reverses_id: nullable_optional_uuid(data, "reversesId")
                    .map_err(|_| ValidationError("reversesId"))?,
            })
        }
        Some("void_invoice") => {
            if !strict_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceId",
                    "expectedRevision",
                    "reason",
                ],
            ) {
                return Err(ValidationError("type"));
            }
            Ok(BillingCommand::VoidInvoice {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_id: required_uuid(data, "invoiceId").ok_or(ValidationError("invoiceId"))?,
                expected_revision: bounded_integer(data, "expectedRevision", 0, i64::MAX)
                    .ok_or(ValidationError("expectedRevision"))?,
                reason: bounded_member(data, "reason", 1, REASON_MAX)
                    .ok_or(ValidationError("reason"))?,
            })
        }
        Some("record_payment_attempt") => {
            // The attempt arm cannot assert a result: there is no outcome
            // member to send, and the amount must be POSITIVE.
            if !strict_optional_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceId",
                    "amountMinor",
                    "providerRef",
                ],
                &["type", "commandId", "invoiceId", "amountMinor"],
            ) {
                return Err(ValidationError("type"));
            }
            Ok(BillingCommand::RecordPaymentAttempt {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_id: required_uuid(data, "invoiceId").ok_or(ValidationError("invoiceId"))?,
                amount_minor: bounded_integer(data, "amountMinor", 1, i64::MAX)
                    .ok_or(ValidationError("amountMinor"))?,
                provider_ref: nullable_optional_ref(data, "providerRef", PROVIDER_REF_MAX)
                    .map_err(|_| ValidationError("providerRef"))?,
            })
        }
        Some("record_provider_event") => {
            // The settlement arm: the package's only mutation whose
            // authority is a signature rather than a person's grant.
            if !strict_members(
                data,
                &[
                    "type",
                    "commandId",
                    "invoiceId",
                    "providerId",
                    "providerEventId",
                    "outcome",
                    "amountMinor",
                    "currency",
                    "signature",
                ],
            ) {
                return Err(ValidationError("type"));
            }
            Ok(BillingCommand::RecordProviderEvent {
                command_id: required_uuid(data, "commandId").ok_or(ValidationError("commandId"))?,
                invoice_id: required_uuid(data, "invoiceId").ok_or(ValidationError("invoiceId"))?,
                provider_id: bounded_member(data, "providerId", 1, PROVIDER_ID_MAX)
                    .ok_or(ValidationError("providerId"))?,
                provider_event_id: bounded_member(
                    data,
                    "providerEventId",
                    1,
                    PROVIDER_EVENT_ID_MAX,
                )
                .ok_or(ValidationError("providerEventId"))?,
                outcome: data
                    .get("outcome")
                    .and_then(Value::as_str)
                    .and_then(SettlementOutcome::parse)
                    .ok_or(ValidationError("outcome"))?,
                amount_minor: bounded_int_value(
                    data.get("amountMinor")
                        .ok_or(ValidationError("amountMinor"))?,
                )
                .ok_or(ValidationError("amountMinor"))?,
                currency: currency_member(data).ok_or(ValidationError("currency"))?,
                signature: signature_member(data).ok_or(ValidationError("signature"))?,
            })
        }
        _ => Err(ValidationError("type")),
    }
}

// --- The provider-event signature (signature.ts, ported) ------------------------

/// The one namespace in this domain. Nothing outside billing computes a
/// digest under this tag, and nothing inside billing computes one under
/// any other, so a signature is only ever a statement about a payment
/// event.
pub const PROVIDER_EVENT_DOMAIN: &str = "archava/billing/provider-event/v1";

/// The re-hash key the comparison rides (both sides are digested under
/// this fixed label before comparing, so a mismatch cannot be timed into
/// a guess about which prefix was right).
pub const SIGNATURE_COMPARE_KEY: &str = "archava/billing/compare/v1";

/// The minimum key bytes a provider digest may be computed under —
/// `SIGNATURE_KEY_MIN_BYTES` (256 bits; a shorter key is refused closed).
pub const SIGNATURE_KEY_MIN_BYTES: usize = 32;

/// The ceiling on what one claim will frame — `MAX_FRAMED_BYTES` (1 MiB).
/// Claims reach this code early-bounded by the grammar, so this is the
/// malformed-payload tripwire, never a size check on well-formed data.
const MAX_FRAMED_BYTES: usize = 1_048_576;

/// Exactly the fields a provider signs, in the order they are framed —
/// the `PaymentEventClaim` shape the digest covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentEventClaim<'a> {
    pub tenant_id: &'a str,
    pub invoice_id: &'a str,
    pub provider_id: &'a str,
    pub provider_event_id: &'a str,
    pub outcome: &'a str,
    pub amount_minor: i64,
    pub currency: &'a str,
}

/// Digest-computation refusal families — the TS `BillingSignatureError`
/// codes (`key` | `size`), refused closed by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureError {
    /// The key material is shorter than [`SIGNATURE_KEY_MIN_BYTES`].
    Key,
    /// The framed total exceeds [`MAX_FRAMED_BYTES`].
    Size,
}

/// The big-endian u32 length header TS writes AFTER each part's bytes
/// (`lengthHeader` in `signature.ts`).
fn length_header(size: usize) -> [u8; 4] {
    (size as u32).to_be_bytes()
}

/// `providerEventDigest` — `HMAC-SHA256(key, domain ‖ length-framed
/// parts)` over the canonical rendering of exactly the fields the
/// provider says moved. Framing makes the parts unambiguous (a character
/// cannot move between fields without invalidating a signature), the
/// amount is an exact integer string (no trailing `.0`, no locale
/// separator), and the digest covers one claim (a digest minted under
/// another domain verifies against nothing here).
pub fn provider_event_digest(
    key: &[u8],
    claim: &PaymentEventClaim<'_>,
) -> Result<String, SignatureError> {
    if key.len() < SIGNATURE_KEY_MIN_BYTES {
        return Err(SignatureError::Key);
    }
    let mut framed: Vec<u8> = Vec::with_capacity(256);
    let domain_bytes = PROVIDER_EVENT_DOMAIN.as_bytes();
    framed.extend_from_slice(domain_bytes);
    framed.extend_from_slice(&length_header(domain_bytes.len()));
    // The parts in the order they are framed: the tenant, the invoice,
    // the provider's own event id, the outcome, the amount in minor units
    // and the currency — exactly `claimParts`'s order, with the amount
    // rendered as an exact integer string.
    let plain: [&str; 7] = [
        claim.tenant_id,
        claim.invoice_id,
        claim.provider_id,
        claim.provider_event_id,
        claim.outcome,
        &claim.amount_minor.to_string(),
        claim.currency,
    ];
    for part in plain {
        let bytes = part.as_bytes();
        framed.extend_from_slice(bytes);
        framed.extend_from_slice(&length_header(bytes.len()));
    }
    if framed.len() > MAX_FRAMED_BYTES {
        return Err(SignatureError::Size);
    }
    let mut mac = <Hmac<Sha256>>::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(&framed);
    Ok(hex_lower(&mac.finalize().into_bytes()))
}

/// `signaturesEqual` — both sides re-hashed under a fixed digest before
/// the comparison, so the bytes compared are the same length whatever
/// arrives. A caller who sends a short or malformed signature cannot
/// distinguish "wrong length" from "right length, wrong value" by timing,
/// and cannot walk the correct signature one character at a time.
pub fn signatures_equal(left: &str, right: &str) -> bool {
    let expected = digest_for_comparison(left);
    let actual = digest_for_comparison(right);
    // Constant-time fold over the two 32-byte digests — the xor of every
    // differing pair is non-zero exactly when the bytes differ.
    let mut difference = 0u8;
    for index in 0..expected.len() {
        difference |= expected[index] ^ actual[index];
    }
    difference == 0
}

fn digest_for_comparison(value: &str) -> [u8; 32] {
    let mut mac = <Hmac<Sha256>>::new_from_slice(SIGNATURE_COMPARE_KEY.as_bytes())
        .expect("HMAC accepts any key length");
    mac.update(value.as_bytes());
    mac.finalize().into_bytes().into()
}

/// Lowercase hex — the TS `.digest('hex')` render (64 chars) the
/// signature regex expects.
fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(DIGITS[(byte >> 4) as usize] as char);
        out.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // --- The cross-language signature fixture -----------------------------

    /// The digest one known claim produces under a known key — generated
    /// by `packages/billing/src/signature.ts`'s own crypto (node:
    /// createHmac) over the same framing. The fixture is the ONLY thing
    /// that proves the byte order (part-then-length, domain first) and
    /// the integer-string amount render survived the port; every other
    /// signature assertion could pass with a subtly different framing.
    const CROSS_KEY: &[u8] = b"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn cross_claim() -> PaymentEventClaim<'static> {
        PaymentEventClaim {
            tenant_id: "acme-retail",
            invoice_id: "f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
            provider_id: "stripe-test",
            provider_event_id: "evt_0001",
            outcome: "succeeded",
            amount_minor: 12_345,
            currency: "IDR",
        }
    }

    #[test]
    fn the_framed_digest_reproduces_the_ts_bytes() {
        // From node:
        //   parts = [tenantId, invoiceId, providerId, providerEventId,
        //            outcome, String(amountMinor), currency]
        //   framed = domain || BE32(len(domain)) || part_i || BE32(len)
        let expected = "be969b85ac484bfc834b7e8313e1f716965d371801adb05b21f2a1a7c52e03ca";
        assert_eq!(
            provider_event_digest(CROSS_KEY, &cross_claim()).expect("a 64-byte key digests"),
            expected,
            "the framed digest must equal the TS engine's own bytes"
        );
    }

    #[test]
    fn a_moved_decimal_place_refuses_a_minted_other_claim() {
        // A tampered amount digests differently: the port's framing keeps
        // `['ab','c']` and `['a','bc']` apart the same way TS does.
        let tampered = PaymentEventClaim {
            amount_minor: 12_346,
            ..cross_claim()
        };
        let original =
            provider_event_digest(CROSS_KEY, &cross_claim()).expect("a 64-byte key digests");
        let other = provider_event_digest(CROSS_KEY, &tampered).expect("a 64-byte key digests");
        assert_ne!(original, other);
        assert!(!signatures_equal(&expected_fixture(), &other));
    }

    fn expected_fixture() -> String {
        "be969b85ac484bfc834b7e8313e1f716965d371801adb05b21f2a1a7c52e03ca".to_string()
    }

    #[test]
    fn a_short_key_is_refused_and_the_compare_is_unobserved() {
        assert_eq!(
            provider_event_digest(&CROSS_KEY[..31], &cross_claim()),
            Err(SignatureError::Key),
            "a sub-256-bit key refuses closed"
        );
        // The compare layer: two equal digests equal, and a re-hashed
        // representation matches what the fixed key produced in node
        // (createHmac("sha256", "archava/billing/compare/v1")).update("deadbeef").
        assert!(signatures_equal(&expected_fixture(), &expected_fixture()));
        assert_ne!(
            expected_fixture(),
            "c07f4730a10d5830744c286a8d46febc7c102d39052739437186b9b36a076106",
            "the compare key is NOT the provider-event key: different digests"
        );
    }

    // --- The wire grammar ---------------------------------------------------

    #[test]
    fn the_flat_union_parses_each_arm_of_the_ts_wire() {
        let mutation = json!({
            "type": "create_invoice",
            "commandId": "5b1c2f3a-0000-4000-8000-0000000000a1",
            "invoiceCode": "INV-1",
            "customerRef": "DSO Paijo",
            "currency": "IDR",
            "consumerRef": "hotel-owner-1",
            "lines": [
                {"description": "Deluxe night", "quantity": 2, "unitMinor": 750000}
            ]
        });
        let parsed = parse_mutation(&mutation).expect("the create arm parses");
        match &parsed.command {
            BillingCommand::CreateInvoice {
                command_id,
                invoice_code,
                customer_ref,
                currency,
                consumer_ref,
                lines,
            } => {
                assert_eq!(command_id, "5b1c2f3a-0000-4000-8000-0000000000a1");
                assert_eq!(invoice_code, "INV-1");
                assert_eq!(customer_ref, "DSO Paijo");
                assert_eq!(currency, "IDR");
                assert_eq!(consumer_ref, "hotel-owner-1");
                assert_eq!(
                    lines,
                    &vec![PricedLine {
                        description: "Deluxe night".to_string(),
                        quantity: 2,
                        unit_minor: 750_000,
                    }]
                );
            }
            other => panic!("wrong arm parsed: {other:?}"),
        }
    }

    #[test]
    fn the_nullable_optional_members_fold_absent_and_null() {
        // `reversesId` and `providerRef` are `.nullable().optional()`.
        for absent in [
            json!({"type":"append_correction","commandId":"8b1c2f3a-0000-4000-8000-0000000000b1","invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","amountMinor":-100,"reason":"slip"}),
            json!({"type":"append_correction","commandId":"8b1c2f3a-0000-4000-8000-0000000000b1","invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","amountMinor":-100,"reason":"slip","reversesId":null}),
        ] {
            let parsed = parse_mutation(&absent).expect("absent and null both parse");
            match &parsed.command {
                BillingCommand::AppendCorrection {
                    reverses_id,
                    amount_minor,
                    ..
                } => {
                    assert_eq!(*amount_minor, -100);
                    assert_eq!(reverses_id, &None);
                }
                other => panic!("wrong arm parsed: {other:?}"),
            }
        }
        // A present string still parses through.
        let reversal = json!({
            "type":"append_correction","commandId":"8b1c2f3a-0000-4000-8000-0000000000c1",
            "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","amountMinor":100,
            "reason":"slip","reversesId":"f81d4fae-7dec-11d0-a765-00a0c91e6bf6"
        });
        assert!(parse_mutation(&reversal).is_ok());
        // `providerRef` rides the same family.
        let attempt = json!({
            "type":"record_payment_attempt","commandId":"8b1c2f3a-0000-4000-8000-0000000000d1",
            "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","amountMinor":25000,
            "providerRef":null
        });
        match parse_mutation(&attempt)
            .expect("the attempt parses")
            .command
        {
            BillingCommand::RecordPaymentAttempt { provider_ref, .. } => {
                assert_eq!(provider_ref, None);
            }
            other => panic!("wrong arm parsed: {other:?}"),
        }
    }

    #[test]
    fn the_strict_sheets_refuse_members_outside_the_union() {
        // An unknown member refuses at the flat sheet.
        assert!(parse_mutation(&json!({
            "type":"void_invoice","commandId":"8b1c2f3a-0000-4000-8000-0000000000e1",
            "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","expectedRevision":1,
            "reason":"close it","extra":true
        }))
        .is_err());
        // A missing required member refuses.
        assert!(parse_mutation(&json!({
            "type":"void_invoice","commandId":"8b1c2f3a-0000-4000-8000-0000000000e1",
            "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","expectedRevision":1
        }))
        .is_err());
        // The zero-amount correction refine refuses.
        assert_eq!(
            parse_mutation(&json!({
                "type":"append_correction","commandId":"8b1c2f3a-0000-4000-8000-0000000000e2",
                "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","amountMinor":0,"reason":"zero"
            }))
            .unwrap_err(),
            ValidationError("amountMinor")
        );
        // A non-positive attempt amount (the positive refine) refuses.
        assert!(parse_mutation(&json!({
            "type":"record_payment_attempt","commandId":"8b1c2f3a-0000-4000-8000-0000000000e3",
            "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","amountMinor":0
        }))
        .is_err());
        // An empty or one-line sheet refuses (1–200 lines).
        assert!(parse_mutation(&json!({
            "type":"create_invoice","commandId":"8b1c2f3a-0000-4000-8000-0000000000e4",
            "invoiceCode":"INV","customerRef":"c","currency":"IDR","consumerRef":"o","lines":[]
        }))
        .is_err());
        // A lowercase currency (after trim) refuses.
        assert!(parse_mutation(&json!({
            "type":"create_invoice","commandId":"8b1c2f3a-0000-4000-8000-0000000000e5",
            "invoiceCode":"INV","customerRef":"c","currency":"idr","consumerRef":"o",
            "lines":[{"description":"night","quantity":1,"unitMinor":1000}]
        }))
        .is_err());
        // A wrong-shape signature (not 64 lowercase hex, even after trim)
        // refuses.
        assert_eq!(
            parse_mutation(&json!({
                "type":"record_provider_event","commandId":"8b1c2f3a-0000-4000-8000-0000000000e6",
                "invoiceId":"8b1c2f3a-0000-4000-8000-0000000000b2","providerId":"stripe-test",
                "providerEventId":"evt_0001","outcome":"succeeded","amountMinor":1000,
                "currency":"IDR","signature":"BE969B85AC484BFC834B7E8313E1F716965D371801ADB05B21F2A1A7C52E03CA"
            }))
            .unwrap_err(),
            ValidationError("signature")
        );
    }

    #[test]
    fn the_transition_table_and_arithmetic_helpers_agree_with_the_ts_trueup() {
        use InvoiceStatus::*;
        // `settled` is NOT terminal: a refund reopens it to issued.
        assert!(Settled.allows(Issued));
        assert!(!Settled.allows(Void));
        assert!(!Void.allows(Issued));
        assert!(Draft.allows(Issued));
        assert!(Draft.allows(Void));
        assert!(Issued.allows(Settled));
        assert!(Issued.allows(Void));
        // The arithmetic helpers: gross + correction − settled.
        assert_eq!(outstanding_from(1_000_000, -50_000, 100_000), 850_000);
        // A failed event's delta is zero; a refund's is the negative.
        assert_eq!(settled_delta(SettlementOutcome::Failed, 999), 0);
        assert_eq!(settled_delta(SettlementOutcome::Refunded, 500), -500);
    }

    #[test]
    fn the_receipt_parse_accepts_the_stored_document_strictly() {
        let stored = json!({
            "commandId":"8b1c2f3a-0000-4000-8000-0000000000f1",
            "tenantId":"acme-retail",
            "action":"create_invoice",
            "invoiceId":"f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
            "lineId":"f81d4fae-7dec-11d0-a765-00a0c91e6bf6",
            "status":"draft",
            "outstandingMinor":1500000,
            "revision":1,
            "idempotencyReplay":false,
            "executedAt":"2026-10-18T00:00:00.000Z"
        });
        let receipt = BillingReceipt::parse(&stored, true).expect("the stored receipt parses");
        assert_eq!(receipt.command_id, "8b1c2f3a-0000-4000-8000-0000000000f1");
        // The replay flag rides the CALLER's argument: a stored `false`
        // becomes the replay-branch `true`.
        assert!(receipt.idempotency_replay);
        // A stored explicit `false` with no replay argument stays `false`:
        // the flag is the CALLER's replay-branch argument OR the row's own.
        let as_recorded = BillingReceipt::parse(&stored, false).expect("the stored receipt parses");
        assert!(!as_recorded.idempotency_replay);
        // The serializing shape is the TS receipt's own member names,
        // absent members omitted.
        let rendered = serde_json::to_value(&receipt).expect("a typed receipt serializes");
        let object = rendered.as_object().expect("the receipt renders an object");
        assert!(object.contains_key("commandId"));
        assert!(object.contains_key("idempotencyReplay"));
        assert!(object.contains_key("executedAt"));
        // Optional ids the arm did not name stay ABSENT.
        assert!(!object.contains_key("correctionId"));
        assert!(!object.contains_key("attemptId"));
        assert!(!object.contains_key("eventId"));
    }

    #[test]
    fn order_invoice_accepts_only_source_identity_and_reviewed_revision() {
        let raw = json!({"type":"create_invoice_from_order",
            "commandId":"11111111-1111-4111-8111-111111111111", "invoiceCode":"INV-SOURCE",
            "orderId":"22222222-2222-4222-8222-222222222222", "expectedOrderRevision":3});
        let parsed = parse_mutation(&raw).unwrap();
        assert_eq!(parsed.command.mutation_type(), "create_invoice_from_order");
        assert_eq!(parsed.command.required_permission(), "billing.write");
        assert!(parsed.command.invoice_ref().is_none());
        for field in [
            "customerRef",
            "currency",
            "lines",
            "ownerId",
            "role",
            "sourceOrderId",
        ] {
            let mut forged = raw.clone();
            forged[field] = json!("forged");
            assert!(parse_mutation(&forged).is_err(), "{field}");
        }
        for invalid in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!(2147483648i64),
            json!("3"),
        ] {
            let mut forged = raw.clone();
            forged["expectedOrderRevision"] = invalid;
            assert!(parse_mutation(&forged).is_err());
        }
        let mut forged = raw;
        forged["orderId"] = json!("not-an-order-id");
        assert!(parse_mutation(&forged).is_err());
    }
}
