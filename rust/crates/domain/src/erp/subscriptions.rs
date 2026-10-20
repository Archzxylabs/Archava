//! The subscriptions domain — the Archxy 2 port of
//! `packages/subscriptions/src/{contracts,service}.ts`: the recurring
//! subscription lifecycle, explicit entitlements/quotas, metered usage with
//! idempotency, and the paid-activation rule the R07 area must correct.
//!
//! ## What this port keeps from the baseline
//!
//! The five command arms (`create_subscription`, `record_payment`,
//! `consume_usage`, `change_status`, `grant_entitlement`), the strict
//! object grammar (`deny_unknown_fields` = the TS `.strict()`), the
//! vocabularies (statuses, intervals, entitlement types and statuses), the
//! receipt shape (including the `idempotencyReplay` member every ERP
//! ledger now carries) and the `unknown`/`recorded` command-status union.
//!
//! ## The corrected rule (audit A05 — P1)
//!
//! The baseline lets a `draft` subscription's entitlements be consumed: the
//! repository refuses `past_due`/`paused`/`canceled`/`expired` but never
//! `draft`, so a PAID plan with `unitPrice > 0` and no `record_payment`
//! still meters usage — the audit's counterexample, verbatim.
//!
//! This port makes the policy explicit and deterministic:
//!
//! - **Consumption requires a PAID, RUNNING subscription.** The owning
//!   subscription must be `active`; `draft` (never paid) is refused, and
//!   so are `past_due`, `paused`, `canceled` and `expired`. A free trial or
//!   grace period is NOT invented here: it would need its own audited
//!   authority, which this ledger does not have.
//! - **Activation requires a payment record.** `change_status` may move a
//!   subscription to `active` only when a payment record already exists
//!   (`payment_reference IS NOT NULL`) — the `paused → active` resume path.
//!   A never-paid `draft` cannot be pushed to `active`; `record_payment`
//!   is the only activation path, exactly as the baseline's own lifecycle
//!   diagram says.
//! - **A manual payment reference is not provider-verified settlement.**
//!   `record_payment` records a human-confirmed payment; it is NOT a
//!   billing settlement fact and this port never claims it is one. The
//!   R07 billing ledger keeps that authority for itself.
//!
//! The corrected deltas are recorded in `docs/rust/KNOWN_DIFFERENCES.md`
//! (D19) and in `docs/rust/archxy2/ERP_ACCEPTANCE_MATRIX.md`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::purchasing::ValidationError;
use super::{bounded_title, trimmed, valid_uuid};

/// The permission vocabulary migration 001's CHECK allows.
pub const SUBSCRIPTION_PERMISSIONS: &[&str] = &[
    "subscriptions.read",
    "subscriptions.write",
    "subscriptions.payment",
    "subscriptions.usage",
    "audit.read",
];

/// The subscription status vocabulary (the TS `SUBSCRIPTION_STATUSES`).
pub const STATUS_DRAFT: &str = "draft";
pub const STATUS_ACTIVE: &str = "active";
pub const STATUS_PAST_DUE: &str = "past_due";
pub const STATUS_PAUSED: &str = "paused";
pub const STATUS_CANCELED: &str = "canceled";
pub const STATUS_EXPIRED: &str = "expired";

/// The billing intervals (the TS `BILLING_INTERVALS`).
pub const INTERVAL_WEEKLY: &str = "weekly";
pub const INTERVAL_MONTHLY: &str = "monthly";
pub const INTERVAL_QUARTERLY: &str = "quarterly";
pub const INTERVAL_YEARLY: &str = "yearly";

/// The entitlement types (the TS `ENTITLEMENT_TYPES`).
pub const ENTITLEMENT_METERED: &str = "metered";
pub const ENTITLEMENT_SEAT_COUNT: &str = "seat_count";
pub const ENTITLEMENT_BOOLEAN_ACCESS: &str = "boolean_access";
pub const ENTITLEMENT_PASS_BUNDLE: &str = "pass_bundle";

/// The entitlement statuses (the TS `ENTITLEMENT_STATUSES`).
pub const ENTITLEMENT_ACTIVE: &str = "active";
pub const ENTITLEMENT_SUSPENDED: &str = "suspended";
pub const ENTITLEMENT_REVOKED: &str = "revoked";
pub const ENTITLEMENT_EXHAUSTED: &str = "exhausted";

/// Field bounds (the TS schemas' own numbers).
pub const PLAN_CODE_MAX: usize = 100;
pub const PLAN_NAME_MAX: usize = 200;
pub const FEATURE_CODE_MAX: usize = 100;
pub const FEATURE_NAME_MAX: usize = 200;
pub const PAYMENT_REFERENCE_MAX: usize = 200;
pub const EVIDENCE_REF_MAX: usize = 300;
pub const IDEMPOTENCY_KEY_MAX: usize = 100;
pub const REASON_MAX: usize = 500;
pub const CURRENCY_LEN: usize = 3;

/// The integer ceiling every money/quantity cell shares.
pub const SAFE_INTEGER_MAX: i64 = 9_007_199_254_740_991;

// --- Vocabularies -------------------------------------------------------------

/// The subscription status — the TS enum, parsed without invention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscriptionStatus {
    Draft,
    Active,
    PastDue,
    Paused,
    Canceled,
    Expired,
}

impl SubscriptionStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            STATUS_DRAFT => Some(Self::Draft),
            STATUS_ACTIVE => Some(Self::Active),
            STATUS_PAST_DUE => Some(Self::PastDue),
            STATUS_PAUSED => Some(Self::Paused),
            STATUS_CANCELED => Some(Self::Canceled),
            STATUS_EXPIRED => Some(Self::Expired),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Draft => STATUS_DRAFT,
            Self::Active => STATUS_ACTIVE,
            Self::PastDue => STATUS_PAST_DUE,
            Self::Paused => STATUS_PAUSED,
            Self::Canceled => STATUS_CANCELED,
            Self::Expired => STATUS_EXPIRED,
        }
    }
}

/// The entitlement status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementStatus {
    Active,
    Suspended,
    Revoked,
    Exhausted,
}

impl EntitlementStatus {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            ENTITLEMENT_ACTIVE => Some(Self::Active),
            ENTITLEMENT_SUSPENDED => Some(Self::Suspended),
            ENTITLEMENT_REVOKED => Some(Self::Revoked),
            ENTITLEMENT_EXHAUSTED => Some(Self::Exhausted),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => ENTITLEMENT_ACTIVE,
            Self::Suspended => ENTITLEMENT_SUSPENDED,
            Self::Revoked => ENTITLEMENT_REVOKED,
            Self::Exhausted => ENTITLEMENT_EXHAUSTED,
        }
    }
}

/// The entitlement type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntitlementType {
    Metered,
    SeatCount,
    BooleanAccess,
    PassBundle,
}

impl EntitlementType {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            ENTITLEMENT_METERED => Some(Self::Metered),
            ENTITLEMENT_SEAT_COUNT => Some(Self::SeatCount),
            ENTITLEMENT_BOOLEAN_ACCESS => Some(Self::BooleanAccess),
            ENTITLEMENT_PASS_BUNDLE => Some(Self::PassBundle),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Metered => ENTITLEMENT_METERED,
            Self::SeatCount => ENTITLEMENT_SEAT_COUNT,
            Self::BooleanAccess => ENTITLEMENT_BOOLEAN_ACCESS,
            Self::PassBundle => ENTITLEMENT_PASS_BUNDLE,
        }
    }
}

// --- The corrected policy (A05) -----------------------------------------------

/// Why a consumption is refused — the baseline's own vocabulary, kept so the
/// caller can tell the difference between "not paid yet", "past due" and
/// "shut off".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumptionRefusal {
    /// The owning subscription has never been paid (audit A05).
    NotPaid,
    /// The owning subscription is `past_due` (the TS's own refusal).
    PastDue,
    /// The entitlement itself is suspended (the TS's `suspended`).
    EntitlementSuspended,
    /// The entitlement itself is revoked (the TS's `forbidden`).
    EntitlementRevoked,
    /// The entitlement's quota is exhausted (the TS's `exhausted`).
    EntitlementExhausted,
    /// The owning subscription is not running — `paused`, `canceled` or
    /// `expired` (the TS's `suspended`).
    SubscriptionNotRunning(&'static str),
}

/// The consumption right an entitlement carries, given its own status and
/// its owning subscription's status.
///
/// `None` means: no subscription owns the entitlement (a standalone
/// operator-granted entitlement) — it stands on its own status alone.
pub fn consumption_refusal(
    entitlement_status: EntitlementStatus,
    subscription_status: Option<SubscriptionStatus>,
) -> Option<ConsumptionRefusal> {
    match entitlement_status {
        EntitlementStatus::Suspended => Some(ConsumptionRefusal::EntitlementSuspended),
        EntitlementStatus::Revoked => Some(ConsumptionRefusal::EntitlementRevoked),
        EntitlementStatus::Exhausted => Some(ConsumptionRefusal::EntitlementExhausted),
        EntitlementStatus::Active => match subscription_status {
            // The A05 correction: a draft subscription is NOT paid, so it
            // grants no consumption right. The baseline let this through.
            Some(SubscriptionStatus::Draft) => Some(ConsumptionRefusal::NotPaid),
            Some(SubscriptionStatus::PastDue) => Some(ConsumptionRefusal::PastDue),
            Some(SubscriptionStatus::Paused) => {
                Some(ConsumptionRefusal::SubscriptionNotRunning(STATUS_PAUSED))
            }
            Some(SubscriptionStatus::Canceled) => {
                Some(ConsumptionRefusal::SubscriptionNotRunning(STATUS_CANCELED))
            }
            Some(SubscriptionStatus::Expired) => {
                Some(ConsumptionRefusal::SubscriptionNotRunning(STATUS_EXPIRED))
            }
            Some(SubscriptionStatus::Active) => None,
            None => None,
        },
    }
}

/// Why a status change is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivationRefusal {
    /// The target is not a status this ledger moves to (the TS's target
    /// vocabulary is `active`/`paused`/`canceled`/`past_due`).
    UnknownTarget,
    /// The current status cannot move to the target at all.
    IllegalTransition(&'static str, &'static str),
    /// Moving to `active` without a payment record — the A05 correction's
    /// activation half.
    ActivationWithoutPayment,
    /// The target equals the current status (the TS's `already_active`).
    AlreadyActive,
}

/// The transition table `change_status` walks. Deterministic, explicit, and
/// recorded: `draft` may be canceled or paused (never activated without a
/// payment); `active`/`past_due`/`paused` may pause, fall due or cancel;
/// `active` may also be resumed — but only with a payment record on file;
/// `canceled` and `expired` are terminal.
pub fn activation_refusal(
    current: SubscriptionStatus,
    target: &str,
    has_payment_record: bool,
) -> Option<ActivationRefusal> {
    let target_status = match target {
        STATUS_ACTIVE => SubscriptionStatus::Active,
        STATUS_PAUSED => SubscriptionStatus::Paused,
        STATUS_CANCELED => SubscriptionStatus::Canceled,
        STATUS_PAST_DUE => SubscriptionStatus::PastDue,
        _ => return Some(ActivationRefusal::UnknownTarget),
    };
    if current == target_status {
        return Some(ActivationRefusal::AlreadyActive);
    }
    // Terminal first: a canceled or expired subscription moves nowhere,
    // whatever the payment record says.
    if matches!(
        current,
        SubscriptionStatus::Canceled | SubscriptionStatus::Expired
    ) {
        return Some(ActivationRefusal::IllegalTransition(
            current.as_str(),
            target_status.as_str(),
        ));
    }
    // The activation half of the A05 correction: `active` is reachable only
    // with a payment record on file. `record_payment` is the activation
    // path; `change_status` may only RESUME a subscription that was paid
    // before (the `paused → active` path).
    if target_status == SubscriptionStatus::Active && !has_payment_record {
        return Some(ActivationRefusal::ActivationWithoutPayment);
    }
    let allowed = match current {
        SubscriptionStatus::Draft => matches!(
            target_status,
            SubscriptionStatus::Active | SubscriptionStatus::Paused | SubscriptionStatus::Canceled
        ),
        SubscriptionStatus::Active | SubscriptionStatus::PastDue => matches!(
            target_status,
            SubscriptionStatus::Active
                | SubscriptionStatus::Paused
                | SubscriptionStatus::Canceled
                | SubscriptionStatus::PastDue
        ),
        SubscriptionStatus::Paused => matches!(
            target_status,
            SubscriptionStatus::Active | SubscriptionStatus::Canceled
        ),
        SubscriptionStatus::Canceled | SubscriptionStatus::Expired => false,
    };
    if !allowed {
        return Some(ActivationRefusal::IllegalTransition(
            current.as_str(),
            target_status.as_str(),
        ));
    }
    None
}

// --- The commands -------------------------------------------------------------

/// One entitlement the `create_subscription` arm may open with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InitialEntitlement {
    pub feature_code: String,
    pub feature_name: String,
    pub entitlement_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allocated_quantity: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
}

/// The five command arms — the TS `subscriptionMutationSchema` union.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SubscriptionCommand {
    CreateSubscription {
        command_id: String,
        contact_id: String,
        plan_code: String,
        plan_name: String,
        billing_interval: String,
        unit_price: i64,
        currency: String,
        #[serde(default = "default_quantity")]
        quantity: i64,
        #[serde(default)]
        initial_entitlements: Vec<InitialEntitlement>,
    },
    RecordPayment {
        command_id: String,
        subscription_id: String,
        payment_reference: String,
        amount_paid: i64,
        period_start: String,
        period_end: String,
    },
    ConsumeUsage {
        command_id: String,
        entitlement_id: String,
        units: i64,
        idempotency_key: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        evidence_ref: Option<String>,
    },
    ChangeStatus {
        command_id: String,
        subscription_id: String,
        expected_revision: i64,
        target_status: String,
        reason: String,
    },
    GrantEntitlement {
        command_id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        subscription_id: Option<String>,
        contact_id: String,
        feature_code: String,
        feature_name: String,
        entitlement_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        allocated_quantity: Option<i64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        valid_until: Option<String>,
    },
}

fn default_quantity() -> i64 {
    1
}

impl SubscriptionCommand {
    /// The command id — the claim's key.
    pub fn command_id(&self) -> &str {
        match self {
            Self::CreateSubscription { command_id, .. }
            | Self::RecordPayment { command_id, .. }
            | Self::ConsumeUsage { command_id, .. }
            | Self::ChangeStatus { command_id, .. }
            | Self::GrantEntitlement { command_id, .. } => command_id,
        }
    }

    /// The arm's own permission gate — the TS `grant(access, …)` call.
    pub fn required_permission(&self) -> &'static str {
        match self {
            Self::CreateSubscription { .. }
            | Self::ChangeStatus { .. }
            | Self::GrantEntitlement { .. } => "subscriptions.write",
            Self::RecordPayment { .. } => "subscriptions.payment",
            Self::ConsumeUsage { .. } => "subscriptions.usage",
        }
    }
}

/// The parsed mutation — the arm plus the metadata the arms carry.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionMutation {
    pub command: SubscriptionCommand,
}

// --- The access envelope ------------------------------------------------------

/// The caller's effective access — the TS `SubscriptionAccess`.
#[derive(Debug, Clone, PartialEq)]
pub struct SubscriptionAccess {
    pub permissions: Vec<String>,
    pub owner_id: String,
    pub scope: String,
}

impl SubscriptionAccess {
    pub fn holds(&self, wanted: &str) -> bool {
        self.permissions.iter().any(|held| held == wanted)
    }
}

// --- The receipt --------------------------------------------------------------

/// The receipt document — the TS `subscriptionReceiptSchema`.
///
/// The optional members are `Option<Option<…>>` because the TS spells
/// them `.nullable().optional()`: absent, explicit null, or a value are
/// three different documents and the port keeps the difference (absent
/// members are omitted from the JSON, null members ride as null).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionReceipt {
    pub command_id: String,
    pub tenant_id: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subscription_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entitlement_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_event_id: Option<Option<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<i64>,
    pub status: String,
    pub idempotency_replay: bool,
    pub executed_at: String,
}

impl SubscriptionReceipt {
    /// Parse a stored receipt document (the command-status read's job).
    pub fn parse(raw: &Value, replay: bool) -> Result<Self, ValidationError> {
        let object = raw.as_object().ok_or(ValidationError("receipt"))?;
        let command_id = object
            .get("commandId")
            .and_then(Value::as_str)
            .filter(|value| valid_uuid(value))
            .ok_or(ValidationError("commandId"))?;
        let tenant_id = object
            .get("tenantId")
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 100)
            .ok_or(ValidationError("tenantId"))?;
        let action = object
            .get("action")
            .and_then(Value::as_str)
            .ok_or(ValidationError("action"))?;
        let status = object
            .get("status")
            .and_then(Value::as_str)
            .ok_or(ValidationError("status"))?;
        let executed_at = object
            .get("executedAt")
            .and_then(Value::as_str)
            .filter(|value| valid_datetime_utc(value))
            .ok_or(ValidationError("executedAt"))?;
        let optional_id = |key: &'static str| -> Result<Option<Option<String>>, ValidationError> {
            match object.get(key) {
                None | Some(Value::Null) => Ok(Some(None)),
                Some(Value::String(value)) if valid_uuid(value) => Ok(Some(Some(value.clone()))),
                _ => Err(ValidationError(key)),
            }
        };
        let revision = match object.get("revision") {
            None => None,
            Some(Value::Number(number)) => {
                let value = number.as_i64().ok_or(ValidationError("revision"))?;
                if value < 0 {
                    return Err(ValidationError("revision"));
                }
                Some(value)
            }
            _ => return Err(ValidationError("revision")),
        };
        Ok(Self {
            command_id: command_id.to_string(),
            tenant_id: tenant_id.to_string(),
            action: action.to_string(),
            subscription_id: optional_id("subscriptionId")?,
            entitlement_id: optional_id("entitlementId")?,
            usage_event_id: optional_id("usageEventId")?,
            revision,
            status: status.to_string(),
            idempotency_replay: replay,
            executed_at: executed_at.to_string(),
        })
    }
}

// --- The command status -------------------------------------------------------

/// The command-status document — the TS `subscriptionCommandStatusSchema`.
/// The receipt rides a Box for the same reason the billing status does: the
/// `unknown` arm names none of the recorded arm's cells, and the wire shape
/// is unchanged by the indirection.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum SubscriptionCommandStatus {
    Unknown,
    Recorded { receipt: Box<SubscriptionReceipt> },
}

// --- The read projections -----------------------------------------------------

/// A subscription row — the TS `subscriptionSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Subscription {
    pub id: String,
    pub tenant_id: String,
    pub contact_id: String,
    pub plan_code: String,
    pub plan_name: String,
    pub status: String,
    pub billing_interval: String,
    pub unit_price: i64,
    pub currency: String,
    pub quantity: i64,
    pub current_period_start: Option<String>,
    pub current_period_end: Option<String>,
    pub cancel_at_period_end: bool,
    pub payment_reference: Option<String>,
    pub owner_id: String,
    pub revision: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// An entitlement row — the TS `entitlementSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Entitlement {
    pub id: String,
    pub tenant_id: String,
    pub subscription_id: Option<String>,
    pub contact_id: String,
    pub feature_code: String,
    pub feature_name: String,
    pub entitlement_type: String,
    pub allocated_quantity: Option<i64>,
    pub consumed_quantity: i64,
    pub status: String,
    pub valid_from: String,
    pub valid_until: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// A usage event row — the TS `usageEventSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEvent {
    pub id: String,
    pub tenant_id: String,
    pub entitlement_id: String,
    pub subscription_id: Option<String>,
    pub contact_id: String,
    pub units_consumed: i64,
    pub idempotency_key: String,
    pub evidence_ref: Option<String>,
    pub recorded_by: String,
    pub recorded_at: String,
}

/// The detail projection — the TS `subscriptionDetailSchema`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubscriptionDetail {
    pub subscription: Subscription,
    pub entitlements: Vec<Entitlement>,
}

// --- The strict parse ---------------------------------------------------------

/// Parse the mutation document — the TS `subscriptionMutationSchema`.
///
/// The grammar is STRICT: an unknown member is a refusal, exactly like the
/// TS `.strict()` objects. Defaults follow the TS: `quantity` defaults to
/// 1, `initialEntitlements` to the empty list.
pub fn parse_mutation(raw: &Value) -> Result<SubscriptionMutation, ValidationError> {
    let command = raw.as_object().ok_or(ValidationError("type"))?;
    let arm = command.get("type").and_then(Value::as_str);
    let Some(arm) = arm else {
        return Err(ValidationError("type"));
    };
    let parsed = match arm {
        "create_subscription" => SubscriptionCommand::CreateSubscription {
            command_id: uuid_member(command, "commandId")?,
            contact_id: uuid_member(command, "contactId")?,
            plan_code: title_member(command, "planCode", PLAN_CODE_MAX)?,
            plan_name: title_member(command, "planName", PLAN_NAME_MAX)?,
            billing_interval: vocabulary_member(
                command,
                "billingInterval",
                &[
                    INTERVAL_WEEKLY,
                    INTERVAL_MONTHLY,
                    INTERVAL_QUARTERLY,
                    INTERVAL_YEARLY,
                ],
            )?,
            unit_price: money_member(command, "unitPrice")?,
            currency: currency_member(command, "currency")?,
            quantity: match command.get("quantity") {
                None => 1,
                Some(Value::Number(number)) => {
                    let value = number.as_i64().ok_or(ValidationError("quantity"))?;
                    if value <= 0 {
                        return Err(ValidationError("quantity"));
                    }
                    value
                }
                _ => return Err(ValidationError("quantity")),
            },
            initial_entitlements: match command.get("initialEntitlements") {
                None => Vec::new(),
                Some(Value::Array(items)) => {
                    let mut parsed = Vec::with_capacity(items.len());
                    for item in items {
                        parsed.push(parse_initial_entitlement(item)?);
                    }
                    parsed
                }
                _ => return Err(ValidationError("initialEntitlements")),
            },
        },
        "record_payment" => SubscriptionCommand::RecordPayment {
            command_id: uuid_member(command, "commandId")?,
            subscription_id: uuid_member(command, "subscriptionId")?,
            payment_reference: title_member(command, "paymentReference", PAYMENT_REFERENCE_MAX)?,
            amount_paid: money_member(command, "amountPaid")?,
            period_start: datetime_member(command, "periodStart")?,
            period_end: datetime_member(command, "periodEnd")?,
        },
        "consume_usage" => SubscriptionCommand::ConsumeUsage {
            command_id: uuid_member(command, "commandId")?,
            entitlement_id: uuid_member(command, "entitlementId")?,
            units: match command.get("units") {
                Some(Value::Number(number)) => {
                    let value = number.as_i64().ok_or(ValidationError("units"))?;
                    if value <= 0 {
                        return Err(ValidationError("units"));
                    }
                    value
                }
                _ => return Err(ValidationError("units")),
            },
            idempotency_key: title_member(command, "idempotencyKey", IDEMPOTENCY_KEY_MAX)?,
            evidence_ref: optional_text_member(command, "evidenceRef", EVIDENCE_REF_MAX)?,
        },
        "change_status" => SubscriptionCommand::ChangeStatus {
            command_id: uuid_member(command, "commandId")?,
            subscription_id: uuid_member(command, "subscriptionId")?,
            expected_revision: match command.get("expectedRevision") {
                Some(Value::Number(number)) => {
                    let value = number.as_i64().ok_or(ValidationError("expectedRevision"))?;
                    if value < 0 {
                        return Err(ValidationError("expectedRevision"));
                    }
                    value
                }
                _ => return Err(ValidationError("expectedRevision")),
            },
            target_status: vocabulary_member(
                command,
                "targetStatus",
                &[
                    STATUS_ACTIVE,
                    STATUS_PAUSED,
                    STATUS_CANCELED,
                    STATUS_PAST_DUE,
                ],
            )?,
            reason: title_member(command, "reason", REASON_MAX)?,
        },
        "grant_entitlement" => SubscriptionCommand::GrantEntitlement {
            command_id: uuid_member(command, "commandId")?,
            subscription_id: optional_uuid_member(command, "subscriptionId")?,
            contact_id: uuid_member(command, "contactId")?,
            feature_code: title_member(command, "featureCode", FEATURE_CODE_MAX)?,
            feature_name: title_member(command, "featureName", FEATURE_NAME_MAX)?,
            entitlement_type: vocabulary_member(
                command,
                "entitlementType",
                &[
                    ENTITLEMENT_METERED,
                    ENTITLEMENT_SEAT_COUNT,
                    ENTITLEMENT_BOOLEAN_ACCESS,
                    ENTITLEMENT_PASS_BUNDLE,
                ],
            )?,
            allocated_quantity: optional_money_member(command, "allocatedQuantity")?,
            valid_until: optional_datetime_member(command, "validUntil")?,
        },
        _ => return Err(ValidationError("type")),
    };
    // The strict walk: every member of the arm must be one this grammar
    // knows. The TS `.strict()` objects refuse the same extra members.
    let allowed: &[&str] = match &parsed {
        SubscriptionCommand::CreateSubscription { .. } => &[
            "type",
            "commandId",
            "contactId",
            "planCode",
            "planName",
            "billingInterval",
            "unitPrice",
            "currency",
            "quantity",
            "initialEntitlements",
        ],
        SubscriptionCommand::RecordPayment { .. } => &[
            "type",
            "commandId",
            "subscriptionId",
            "paymentReference",
            "amountPaid",
            "periodStart",
            "periodEnd",
        ],
        SubscriptionCommand::ConsumeUsage { .. } => &[
            "type",
            "commandId",
            "entitlementId",
            "units",
            "idempotencyKey",
            "evidenceRef",
        ],
        SubscriptionCommand::ChangeStatus { .. } => &[
            "type",
            "commandId",
            "subscriptionId",
            "expectedRevision",
            "targetStatus",
            "reason",
        ],
        SubscriptionCommand::GrantEntitlement { .. } => &[
            "type",
            "commandId",
            "subscriptionId",
            "contactId",
            "featureCode",
            "featureName",
            "entitlementType",
            "allocatedQuantity",
            "validUntil",
        ],
    };
    for key in command.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(ValidationError("strict"));
        }
    }
    Ok(SubscriptionMutation { command: parsed })
}

fn parse_initial_entitlement(item: &Value) -> Result<InitialEntitlement, ValidationError> {
    let object = item
        .as_object()
        .ok_or(ValidationError("initialEntitlements"))?;
    for key in object.keys() {
        if !matches!(
            key.as_str(),
            "featureCode" | "featureName" | "entitlementType" | "allocatedQuantity" | "validUntil"
        ) {
            return Err(ValidationError("strict"));
        }
    }
    Ok(InitialEntitlement {
        feature_code: title_member(object, "featureCode", FEATURE_CODE_MAX)?,
        feature_name: title_member(object, "featureName", FEATURE_NAME_MAX)?,
        entitlement_type: vocabulary_member(
            object,
            "entitlementType",
            &[
                ENTITLEMENT_METERED,
                ENTITLEMENT_SEAT_COUNT,
                ENTITLEMENT_BOOLEAN_ACCESS,
                ENTITLEMENT_PASS_BUNDLE,
            ],
        )?,
        allocated_quantity: optional_money_member(object, "allocatedQuantity")?,
        valid_until: optional_datetime_member(object, "validUntil")?,
    })
}

fn uuid_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_uuid(value))
        .map(str::to_string)
        .ok_or(ValidationError(key))
}

fn optional_uuid_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<String>, ValidationError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if valid_uuid(value) => Ok(Some(value.clone())),
        _ => Err(ValidationError(key)),
    }
}

fn title_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    max: usize,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .and_then(|value| bounded_title(value, max))
        .ok_or(ValidationError(key))
}

fn optional_text_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    max: usize,
) -> Result<Option<String>, ValidationError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => {
            let trimmed = trimmed(value);
            if trimmed.len() > max {
                return Err(ValidationError(key));
            }
            Ok(Some(trimmed.to_string()))
        }
        _ => Err(ValidationError(key)),
    }
}

fn vocabulary_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
    allowed: &[&str],
) -> Result<String, ValidationError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ValidationError(key))?;
    if allowed.contains(&value) {
        Ok(value.to_string())
    } else {
        Err(ValidationError(key))
    }
}

fn money_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<i64, ValidationError> {
    match object.get(key) {
        Some(Value::Number(number)) => {
            let value = number.as_i64().ok_or(ValidationError(key))?;
            if !(0..=SAFE_INTEGER_MAX).contains(&value) {
                return Err(ValidationError(key));
            }
            Ok(value)
        }
        _ => Err(ValidationError(key)),
    }
}

fn optional_money_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<i64>, ValidationError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(number)) => {
            let value = number.as_i64().ok_or(ValidationError(key))?;
            if !(0..=SAFE_INTEGER_MAX).contains(&value) {
                return Err(ValidationError(key));
            }
            Ok(Some(value))
        }
        _ => Err(ValidationError(key)),
    }
}

fn currency_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ValidationError> {
    let value = object
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ValidationError(key))?;
    // `/^[A-Z]{3}$/` — exactly three uppercase letters.
    if value.len() == CURRENCY_LEN && value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        Ok(value.to_string())
    } else {
        Err(ValidationError(key))
    }
}

fn datetime_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<String, ValidationError> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|value| valid_datetime_utc(value))
        .map(str::to_string)
        .ok_or(ValidationError(key))
}

fn optional_datetime_member(
    object: &serde_json::Map<String, Value>,
    key: &'static str,
) -> Result<Option<String>, ValidationError> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) if valid_datetime_utc(value) => Ok(Some(value.clone())),
        _ => Err(ValidationError(key)),
    }
}

/// The ISO-8601 UTC instant the TS `.datetime()` accepts:
/// `YYYY-MM-DDTHH:MM:SS[.fff]Z` — a `Z` suffix, no offset, real calendar
/// fields.
pub fn valid_datetime_utc(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() < 20 || bytes.len() > 30 {
        return false;
    }
    let digits = |range: std::ops::Range<usize>| bytes[range].iter().all(u8::is_ascii_digit);
    if !digits(0..4) || bytes[4] != b'-' || !digits(5..7) || bytes[7] != b'-' || !digits(8..10) {
        return false;
    }
    if bytes[10] != b'T'
        || !digits(11..13)
        || bytes[13] != b':'
        || !digits(14..16)
        || bytes[16] != b':'
    {
        return false;
    }
    if !digits(17..19) {
        return false;
    }
    let mut cursor = 19;
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        let start = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == start {
            return false;
        }
    }
    if bytes.get(cursor) != Some(&b'Z') {
        return false;
    }
    if cursor + 1 != bytes.len() {
        return false;
    }
    let month: u32 = value[5..7].parse().unwrap_or(0);
    let day: u32 = value[8..10].parse().unwrap_or(0);
    let hour: u32 = value[11..13].parse().unwrap_or(0);
    let minute: u32 = value[14..16].parse().unwrap_or(0);
    let second: u32 = value[17..19].parse().unwrap_or(0);
    (1..=12).contains(&month) && (1..=31).contains(&day) && hour < 24 && minute < 60 && second < 60
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn mutation(value: Value) -> Result<SubscriptionMutation, ValidationError> {
        parse_mutation(&value)
    }

    #[test]
    fn create_subscription_defaults_follow_the_baseline() {
        let parsed = mutation(json!({
            "type": "create_subscription",
            "commandId": "6b240200-9999-4999-9999-000000000001",
            "contactId": "6b240200-9999-4999-9999-000000000002",
            "planCode": "saas-team",
            "planName": "Team plan",
            "billingInterval": "monthly",
            "unitPrice": 100000,
            "currency": "IDR",
            "initialEntitlements": [{
                "featureCode": "api-calls",
                "featureName": "API calls",
                "entitlementType": "metered"
            }]
        }))
        .expect("the create arm parses");
        match parsed.command {
            SubscriptionCommand::CreateSubscription {
                quantity,
                initial_entitlements,
                ..
            } => {
                assert_eq!(quantity, 1);
                assert_eq!(initial_entitlements.len(), 1);
                assert_eq!(initial_entitlements[0].allocated_quantity, None);
                assert_eq!(initial_entitlements[0].valid_until, None);
            }
            other => panic!("unexpected arm: {other:?}"),
        }
    }

    #[test]
    fn the_strict_grammar_refuses_an_extra_member() {
        let refused = mutation(json!({
            "type": "create_subscription",
            "commandId": "6b240200-9999-4999-9999-000000000001",
            "contactId": "6b240200-9999-4999-9999-000000000002",
            "planCode": "saas-team",
            "planName": "Team plan",
            "billingInterval": "monthly",
            "unitPrice": 100000,
            "currency": "IDR",
            "status": "active"
        }));
        assert!(refused.is_err(), "an extra member is a refusal");
    }

    #[test]
    fn the_bounds_hold() {
        assert!(mutation(json!({
            "type": "consume_usage",
            "commandId": "6b240200-9999-4999-9999-000000000003",
            "entitlementId": "6b240200-9999-4999-9999-000000000004",
            "units": 0,
            "idempotencyKey": "k"
        }))
        .is_err());
        assert!(mutation(json!({
            "type": "create_subscription",
            "commandId": "6b240200-9999-4999-9999-000000000001",
            "contactId": "6b240200-9999-4999-9999-000000000002",
            "planCode": "saas-team",
            "planName": "Team plan",
            "billingInterval": "monthly",
            "unitPrice": 100000,
            "currency": "idr"
        }))
        .is_err());
    }

    #[test]
    fn the_datetime_rule_is_the_baselines() {
        assert!(valid_datetime_utc("2026-01-02T03:04:05Z"));
        assert!(valid_datetime_utc("2026-01-02T03:04:05.123Z"));
        assert!(!valid_datetime_utc("2026-01-02T03:04:05+07:00"));
        assert!(!valid_datetime_utc("2026-01-02 03:04:05Z"));
        assert!(!valid_datetime_utc("2026-13-02T03:04:05Z"));
    }

    #[test]
    fn an_unpaid_draft_grants_no_consumption_right() {
        // The audit A05 counterexample: a draft subscription is NOT paid.
        assert_eq!(
            consumption_refusal(EntitlementStatus::Active, Some(SubscriptionStatus::Draft)),
            Some(ConsumptionRefusal::NotPaid)
        );
        assert_eq!(
            consumption_refusal(EntitlementStatus::Active, Some(SubscriptionStatus::Active)),
            None
        );
        assert_eq!(
            consumption_refusal(EntitlementStatus::Active, Some(SubscriptionStatus::PastDue)),
            Some(ConsumptionRefusal::PastDue)
        );
        // A standalone entitlement stands on its own status alone.
        assert_eq!(consumption_refusal(EntitlementStatus::Active, None), None);
    }

    #[test]
    fn activation_needs_a_payment_record() {
        assert_eq!(
            activation_refusal(SubscriptionStatus::Draft, STATUS_ACTIVE, false),
            Some(ActivationRefusal::ActivationWithoutPayment)
        );
        assert_eq!(
            activation_refusal(SubscriptionStatus::Paused, STATUS_ACTIVE, true),
            None
        );
        assert_eq!(
            activation_refusal(SubscriptionStatus::Canceled, STATUS_ACTIVE, true),
            Some(ActivationRefusal::IllegalTransition(
                STATUS_CANCELED,
                STATUS_ACTIVE
            ))
        );
        assert_eq!(
            activation_refusal(SubscriptionStatus::Active, STATUS_ACTIVE, true),
            Some(ActivationRefusal::AlreadyActive)
        );
        assert_eq!(
            activation_refusal(SubscriptionStatus::Draft, "expired", true),
            Some(ActivationRefusal::UnknownTarget)
        );
    }
}
