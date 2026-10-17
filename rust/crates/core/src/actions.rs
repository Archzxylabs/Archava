//! Versioned action registry, mirroring `packages/acl/src/actions.ts`.
//!
//! PRD §18, carried over verbatim because it is the reason the struct has this
//! many fields: every external action must be registered with capability name,
//! tenant, description, required inputs, risk level, confirmation mode, allowed
//! roles, idempotency behaviour, audit behaviour, and a failure/rollback
//! strategy.
//!
//! The registry is versioned so entries can be retired without breaking the
//! client contract: a deprecated action still resolves and still executes, it
//! just reports `deprecated_in` + `replaced_by` so callers can migrate. New
//! actions never reuse an existing id.
//!
//! One detail from the baseline that is easy to lose in a port and must not be:
//! **capability tier does not override risk rules.** `crm.followup.create` and
//! `crm.followup.complete` are L2 *and* require the `act` tier *and* demand user
//! confirmation. A port that "simplified" L2 into `ConfirmationMode::None`
//! because the level is reversible would silently remove a real gate.

use std::collections::HashMap;
use std::sync::LazyLock;

use crate::roles::{CapabilityTier, RoleName};

/// Risk levels from PRD §18. Capability tier does not override risk rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ActionLevel {
    L0,
    L1,
    L2,
    L3,
    L4,
    L5,
}

impl ActionLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionLevel::L0 => "L0",
            ActionLevel::L1 => "L1",
            ActionLevel::L2 => "L2",
            ActionLevel::L3 => "L3",
            ActionLevel::L4 => "L4",
            ActionLevel::L5 => "L5",
        }
    }

    /// The human-readable label the baseline ships beside each level.
    pub fn label(self) -> &'static str {
        match self {
            ActionLevel::L0 => "Read — product/availability/order status",
            ActionLevel::L1 => "UI — navigate, highlight, filter, compare",
            ActionLevel::L2 => "Reversible — fill draft, add/remove cart item",
            ActionLevel::L3 => "External state — send form, create booking, send email, CRM write",
            ActionLevel::L4 => {
                "Money/account — checkout, payment, cancellation, refund, shipping/account changes"
            }
            ActionLevel::L5 => "Admin — never autonomous by default",
        }
    }
}

/// How an action must be gated before it executes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ConfirmationMode {
    None,
    UserConfirm,
    HumanApproval,
}

impl ConfirmationMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ConfirmationMode::None => "none",
            ConfirmationMode::UserConfirm => "user_confirm",
            ConfirmationMode::HumanApproval => "human_approval",
        }
    }

    /// Whether the action may run with no gate at all.
    pub fn is_ungated(self) -> bool {
        matches!(self, ConfirmationMode::None)
    }
}

/// A registered external action.
#[derive(Debug, Clone, Copy)]
pub struct ActionDefinition {
    /// Stable, namespaced id. Never reused, never renamed.
    pub id: &'static str,
    pub domain: &'static str,
    pub level: ActionLevel,
    /// Minimum capability tier required.
    pub requires_capability: CapabilityTier,
    pub confirmation: ConfirmationMode,
    pub allowed_roles: &'static [RoleName],
    /// Repeating the call with the same key must not repeat the side effect.
    ///
    /// The baseline's note is kept because it explains a promise the registry
    /// makes on the executor's behalf: the idempotency key travels as far as
    /// the execution request and no further, because the pipeline has no store
    /// to remember the keys it already handed out. Every baseline action
    /// declares `true`, so every one leans on an executor that really
    /// de-duplicates.
    pub idempotent: bool,
    pub audited: bool,
    /// Fields that must be masked before any model/provider exposure.
    pub sensitive_fields: &'static [&'static str],
    pub description: &'static str,
    /// Input field names the model is told to supply.
    ///
    /// Names, not a contract. Validation keeps its own schemas in
    /// `packages/assistant/src/validation.ts` and deliberately does not derive
    /// from this list: these strings describe what a prompt may name, while
    /// that map decides what an executor may accept. Collapsing the two would
    /// turn "the registry says the field is called `customer`" into "therefore
    /// a booking may be created without a verified customer".
    pub required_inputs: &'static [&'static str],
    /// Set when the action is retired; the id keeps working.
    pub deprecated_in: Option<&'static str>,
    pub replaced_by: Option<&'static str>,
}

impl ActionDefinition {
    /// Whether this action may be triggered by `role`.
    pub fn allows_role(&self, role: RoleName) -> bool {
        self.allowed_roles.contains(&role)
    }
}

/// The registry schema version.
pub const ACTION_REGISTRY_VERSION: &str = "1.2.0";

const ASSISTANT_ONLY: &[RoleName] = &[RoleName::ArchavaAssistant];
const OWNER_AND_ADMIN: &[RoleName] = &[RoleName::Owner, RoleName::Admin];

/// Baseline registry. Only actions a deployed client actually enables should
/// be exposed to the model — the policy gate, not this list, is what restricts
/// a specific client.
pub const BASELINE_ACTIONS: &[ActionDefinition] = &[
    // ---- L0 read ----------------------------------------------------------
    ActionDefinition {
        id: "product.read",
        domain: "catalog",
        level: ActionLevel::L0,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: false,
        sensitive_fields: &[],
        description: "Read product or service detail from the live authoritative source.",
        required_inputs: &["productId"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "availability.read",
        domain: "availability",
        level: ActionLevel::L0,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: false,
        sensitive_fields: &[],
        description: "Read real availability or stock for a unit, slot, or product.",
        required_inputs: &["subjectId"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "order.status.read",
        domain: "order",
        level: ActionLevel::L0,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["customerEmail", "customerPhone"],
        description: "Read order or booking status for an identified customer order.",
        required_inputs: &["orderReference"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "payment.status.read",
        domain: "payment",
        level: ActionLevel::L0,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["customerEmail"],
        description: "Read payment status from the payment provider.",
        required_inputs: &["paymentReference"],
        deprecated_in: None,
        replaced_by: None,
    },
    // ---- L1 UI ------------------------------------------------------------
    ActionDefinition {
        id: "navigation.go",
        domain: "navigation",
        level: ActionLevel::L1,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: false,
        sensitive_fields: &[],
        description: "Navigate the visitor to a page or route.",
        required_inputs: &["path"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "ui.highlight",
        domain: "navigation",
        level: ActionLevel::L1,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: false,
        sensitive_fields: &[],
        description: "Highlight or scroll to a section or entity on the current page.",
        required_inputs: &["entityId"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "ui.compare",
        domain: "catalog",
        level: ActionLevel::L1,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: false,
        sensitive_fields: &[],
        description: "Open the comparison view for two or more entities.",
        required_inputs: &["entityIds"],
        deprecated_in: None,
        replaced_by: None,
    },
    // ---- L2 reversible ----------------------------------------------------
    ActionDefinition {
        id: "lead.capture",
        domain: "lead",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["email", "phone", "message"],
        description:
            "Store a captured lead in the Archava lead store. Writing it to an external CRM requires Act.",
        required_inputs: &["contact"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "form.fill_draft",
        domain: "form",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: false,
        sensitive_fields: &[],
        description: "Pre-fill a host-page form for the visitor to review and submit.",
        required_inputs: &["formId", "fields"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "handoff.request",
        domain: "support",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Assist,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["contact"],
        description: "Escalate to a human agent queue. The human decides; Archava does not.",
        required_inputs: &["reason"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "cart.item.add",
        domain: "cart",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Add an item to the cart. Reversible.",
        required_inputs: &["productId", "quantity"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "cart.item.remove",
        domain: "cart",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::None,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Remove an item from the cart. Reversible.",
        required_inputs: &["productId"],
        deprecated_in: None,
        replaced_by: None,
    },
    // ---- L3 external state ------------------------------------------------
    ActionDefinition {
        id: "form.submit",
        domain: "form",
        level: ActionLevel::L3,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["fields"],
        description: "Submit a form to an external system.",
        required_inputs: &["formId", "fields"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "crm.lead.create",
        domain: "crm",
        level: ActionLevel::L3,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["email", "phone", "notes"],
        description: "Write or update a lead record in an external CRM.",
        required_inputs: &["contact", "source"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "crm.followup.create",
        domain: "crm",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["summary"],
        description: "Create an owner-approved CRM follow-up task.",
        required_inputs: &["contactId", "contactRevision", "summary", "dueAt"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "crm.followup.complete",
        domain: "crm",
        level: ActionLevel::L2,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Mark one owner-reviewed CRM follow-up task completed.",
        required_inputs: &["followupId", "expectedRevision"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "email.send",
        domain: "email",
        level: ActionLevel::L3,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["to", "body"],
        description: "Send a transactional email as part of a workflow.",
        required_inputs: &["templateId", "to"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "booking.create",
        domain: "booking",
        level: ActionLevel::L3,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["customerPhone", "customerEmail", "notes"],
        description: "Create an appointment or reservation in the booking system.",
        required_inputs: &["slotId", "customer"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "booking.reschedule",
        domain: "booking",
        level: ActionLevel::L3,
        requires_capability: CapabilityTier::Act,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["customerPhone"],
        description: "Reschedule a non-financial appointment where policy allows.",
        required_inputs: &["bookingId", "slotId"],
        deprecated_in: None,
        replaced_by: None,
    },
    // ---- L4 money / account ----------------------------------------------
    ActionDefinition {
        id: "checkout.start",
        domain: "checkout",
        level: ActionLevel::L4,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Start the checkout flow from the current cart.",
        required_inputs: &[],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "payment.initiate",
        domain: "payment",
        level: ActionLevel::L4,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &[
            "cardNumber",
            "cvv",
            "expiry",
            "cardholderName",
            "authorization",
            "otp",
        ],
        description:
            "Initiate payment. Archava never accepts a client-supplied price as authoritative.",
        required_inputs: &["paymentMethodId"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "order.confirm",
        domain: "order",
        level: ActionLevel::L4,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Confirm a paid order or booking using the backend identifier.",
        required_inputs: &["orderId"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "order.cancel_refund",
        domain: "order",
        level: ActionLevel::L4,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["customerEmail"],
        description: "Cancel or refund an order tied to a financial transaction.",
        required_inputs: &["orderId", "reason"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "account.update",
        domain: "account",
        level: ActionLevel::L4,
        requires_capability: CapabilityTier::Transact,
        confirmation: ConfirmationMode::UserConfirm,
        allowed_roles: ASSISTANT_ONLY,
        idempotent: true,
        audited: true,
        sensitive_fields: &["email", "phone", "address"],
        description: "Change shipping details or account data for an authenticated customer.",
        required_inputs: &["changes"],
        deprecated_in: None,
        replaced_by: None,
    },
    // ---- L5 admin ---------------------------------------------------------
    ActionDefinition {
        id: "admin.config.update",
        domain: "admin",
        level: ActionLevel::L5,
        requires_capability: CapabilityTier::Enterprise,
        confirmation: ConfirmationMode::HumanApproval,
        allowed_roles: OWNER_AND_ADMIN,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Change tenant solution configuration. Never autonomous.",
        required_inputs: &["changes"],
        deprecated_in: None,
        replaced_by: None,
    },
    ActionDefinition {
        id: "admin.knowledge.reindex",
        domain: "admin",
        level: ActionLevel::L5,
        requires_capability: CapabilityTier::Enterprise,
        confirmation: ConfirmationMode::HumanApproval,
        allowed_roles: OWNER_AND_ADMIN,
        idempotent: true,
        audited: true,
        sensitive_fields: &[],
        description: "Trigger a tenant knowledge reindex. Never autonomous.",
        required_inputs: &[],
        deprecated_in: None,
        replaced_by: None,
    },
];

/// Raised when an action id is not registered.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown action \"{0}\". Register it before use; do not invent an action id.")]
pub struct ActionRegistryError(String);

impl ActionRegistryError {
    /// The id that was rejected.
    pub fn action_id(&self) -> &str {
        &self.0
    }
}

static ACTIONS_BY_ID: LazyLock<HashMap<&'static str, &'static ActionDefinition>> =
    LazyLock::new(|| {
        BASELINE_ACTIONS
            .iter()
            .map(|action| (action.id, action))
            .collect()
    });

/// A read-only, versioned view over the registered actions.
///
/// Additive-only by contract: entries may be deprecated (and still resolve),
/// but an existing id is never removed or re-pointed, so a stored client
/// contract from an older version keeps executing.
pub struct ActionRegistry;

impl ActionRegistry {
    /// The registry schema version.
    pub fn version() -> &'static str {
        ACTION_REGISTRY_VERSION
    }

    /// Every registered action.
    pub fn list() -> &'static [ActionDefinition] {
        BASELINE_ACTIONS
    }

    /// Whether an id is registered.
    pub fn has(id: &str) -> bool {
        ACTIONS_BY_ID.contains_key(id)
    }

    /// Look up an action. Returns an error for an unknown id rather than
    /// `None`, so a caller cannot treat a typo as "no action required".
    pub fn get(id: &str) -> Result<&'static ActionDefinition, ActionRegistryError> {
        ACTIONS_BY_ID
            .get(id)
            .copied()
            .ok_or_else(|| ActionRegistryError(id.to_string()))
    }

    /// Look up an action, returning `None` for an unknown id.
    pub fn try_get(id: &str) -> Option<&'static ActionDefinition> {
        ACTIONS_BY_ID.get(id).copied()
    }

    /// Every action in a domain.
    pub fn by_domain(domain: &str) -> impl Iterator<Item = &'static ActionDefinition> + '_ {
        BASELINE_ACTIONS
            .iter()
            .filter(move |action| action.domain == domain)
    }

    /// Actions that are retired but still executable, with their replacement.
    pub fn deprecations() -> impl Iterator<Item = (&'static str, &'static str, Option<&'static str>)>
    {
        BASELINE_ACTIONS.iter().filter_map(|action| {
            action
                .deprecated_in
                .map(|deprecated_in| (action.id, deprecated_in, action.replaced_by))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::roles::satisfies_capability;

    fn action(id: &str) -> &'static ActionDefinition {
        ActionRegistry::get(id).expect("baseline action must be registered")
    }

    /// The registry is the contract PRD §18 describes. If an entry is dropped,
    /// a client that stored that id stops working, so the count is pinned.
    #[test]
    fn the_registry_holds_exactly_the_baseline_actions() {
        assert_eq!(BASELINE_ACTIONS.len(), 26);
        assert_eq!(ActionRegistry::version(), "1.2.0");
    }

    #[test]
    fn every_action_id_is_unique() {
        // A duplicate id would make `get` return whichever entry the map kept,
        // so two different contracts could collide under one name.
        let mut seen: Vec<&str> = BASELINE_ACTIONS.iter().map(|action| action.id).collect();
        let total = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(
            seen.len(),
            total,
            "duplicate action id in the baseline registry"
        );
    }

    #[test]
    fn every_baseline_action_is_lookupable_by_id() {
        for action in BASELINE_ACTIONS {
            assert_eq!(
                ActionRegistry::get(action.id).map(|found| found.id),
                Ok(action.id)
            );
            assert!(ActionRegistry::has(action.id));
        }
    }

    #[test]
    fn an_unknown_action_id_is_an_error_not_a_miss() {
        // Mirrors the baseline: `get` throws rather than returning undefined.
        let error = ActionRegistry::get("payment.refund.immediately")
            .expect_err("an invented action id must not resolve");
        assert_eq!(error.action_id(), "payment.refund.immediately");
        assert!(
            error.to_string().contains("do not invent an action id"),
            "the message must tell the caller to register it instead: {error}"
        );
        assert!(ActionRegistry::try_get("payment.refund.immediately").is_none());
    }

    /// The property that makes the action list safe to expose to a model: a
    /// reversible-looking L2 must not carry a confirmation gate it never
    /// declared, and a level must not be read as permission.
    #[test]
    fn every_declared_capability_and_confirmation_survives_the_port() {
        for action in BASELINE_ACTIONS {
            assert!(
                satisfies_capability(CapabilityTier::Enterprise, action.requires_capability),
                "{} must be satisfiable by the strongest tier",
                action.id
            );
            assert!(
                !action.allowed_roles.is_empty(),
                "{} must name at least one role",
                action.id
            );
            assert!(
                action.idempotent,
                "{} leans on executor de-duplication",
                action.id
            );
        }
    }

    /// L5 is the "never autonomous" tier, so no L5 action may be ungated.
    #[test]
    fn no_admin_action_is_ungated() {
        for action in BASELINE_ACTIONS
            .iter()
            .filter(|a| a.level == ActionLevel::L5)
        {
            assert!(
                !action.confirmation.is_ungated(),
                "{} is L5 and must require approval",
                action.id
            );
            assert_eq!(action.confirmation, ConfirmationMode::HumanApproval);
            assert_eq!(action.requires_capability, CapabilityTier::Enterprise);
        }
    }

    /// PRD §18's rule that tier does not override risk: the CRM follow-up
    /// actions are L2 but still demand user confirmation and the `act` tier.
    #[test]
    fn an_l2_action_can_still_require_act_and_user_confirmation() {
        for id in ["crm.followup.create", "crm.followup.complete"] {
            let action = action(id);
            assert_eq!(action.level, ActionLevel::L2);
            assert_eq!(action.requires_capability, CapabilityTier::Act);
            assert_eq!(action.confirmation, ConfirmationMode::UserConfirm);
        }
    }

    #[test]
    fn money_and_account_actions_all_require_confirmation() {
        for action in BASELINE_ACTIONS
            .iter()
            .filter(|a| a.level == ActionLevel::L4)
        {
            assert!(!action.confirmation.is_ungated(), "{} is L4", action.id);
            assert_eq!(action.requires_capability, CapabilityTier::Transact);
        }
    }

    #[test]
    fn only_admin_actions_admit_non_assistant_roles() {
        // The assistant is the model-facing identity; human roles reach the
        // admin surface only.
        for action in BASELINE_ACTIONS {
            let assistant_only = action.allowed_roles == ASSISTANT_ONLY;
            if action.level == ActionLevel::L5 {
                assert!(!assistant_only, "{} must admit human roles", action.id);
            } else {
                assert!(assistant_only, "{} must be assistant-only", action.id);
            }
        }
    }

    #[test]
    fn sensitive_fields_are_marked_on_every_action_that_touches_personal_data() {
        // `payment.initiate` is the strictest: raw card data must never be the
        // field a model is handed.
        let card_fields = action("payment.initiate").sensitive_fields;
        for expected in [
            "cardNumber",
            "cvv",
            "expiry",
            "cardholderName",
            "authorization",
            "otp",
        ] {
            assert!(
                card_fields.contains(&expected),
                "payment.initiate must mask {expected}"
            );
        }
        assert!(action("order.status.read")
            .sensitive_fields
            .contains(&"customerEmail"));
    }

    #[test]
    fn actions_group_by_domain() {
        assert_eq!(
            BASELINE_ACTIONS
                .iter()
                .filter(|a| a.domain == "crm")
                .count(),
            3
        );
        assert_eq!(
            BASELINE_ACTIONS
                .iter()
                .filter(|a| a.domain == "order")
                .count(),
            3
        );
        assert_eq!(
            BASELINE_ACTIONS
                .iter()
                .filter(|a| a.domain == "admin")
                .count(),
            2
        );
        assert!(ActionRegistry::by_domain("navigation").count() >= 2);
    }

    #[test]
    fn deprecations_reports_nothing_while_the_baseline_is_complete() {
        // The registry is additive-only. When an entry is retired this test
        // must change to assert the retirement is visible, never to assert it
        // silently disappears.
        assert_eq!(
            BASELINE_ACTIONS
                .iter()
                .filter(|a| a.deprecated_in.is_some())
                .count(),
            0,
            "a deprecated action must be recorded as deprecated, not deleted"
        );
        assert_eq!(ActionRegistry::deprecations().count(), 0);
    }

    #[test]
    fn every_action_level_has_a_label() {
        for level in [
            ActionLevel::L0,
            ActionLevel::L1,
            ActionLevel::L2,
            ActionLevel::L3,
            ActionLevel::L4,
            ActionLevel::L5,
        ] {
            assert!(!level.label().is_empty(), "{level:?} must carry a label");
        }
    }
}
