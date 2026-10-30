//! The server-side action policy gate — a direct port of `packages/acl/src/policy.ts`.
//!
//! This is the single chokepoint every command passes through before it can
//! touch an external system. It is fail-closed: anything unrecognised, disabled,
//! under-capability, wrong-role, page-unavailable, or unconfirmed is denied.
//!
//! Two rules are structural here, not conventions, and both are carried from the
//! original's header because a port that dropped them would look entirely
//! reasonable while being wrong:
//!
//! 1. **PRD §18 — capability tier does not override risk rules.** A Transact
//!    client still requires appropriate confirmation for L4 actions. So the
//!    confirmation branches below decide from `action.confirmation` alone and
//!    never consult capability. A port that let a Transact client skip
//!    `user_confirm` would be widening permissions in the name of consistency
//!    with the tier model.
//!
//! 2. **PRD §16 / the Context Graph contract — page context can only narrow.**
//!    `available_on_page == Some(false)` removes an action; a page can never add
//!    one. Encoded here as an `Option<bool>` where `None` means the page said
//!    nothing, which is not a denial — the same three-state distinction
//!    `policy.ts:64-67` draws.
//!
//! ## The order of the branches is load-bearing
//!
//! The original explains this at `policy.ts:191-201`, and the explanation cannot
//! be recovered from the code, so it is carried here:
//!
//! > A refusal, unless this very turn already confirmed it. The placement is the
//! > whole mechanism. Put this *after* the confirmation branch and a declined,
//! > unconfirmed action answers `confirmation_required` — which is the re-ask
//! > loop this exists to stop, arriving through the back door. Put it *before*
//! > the business-rule denials above and a decline would relabel an action the
//! > visitor was never entitled to as "you said no", which reads as the
//! > visitor's own choice when it was the client's limit.
//!
//! So: after the denials a visitor's refusal cannot soften, before the questions
//! the refusal is the answer to. [`CommandAuthorizer::evaluate`] keeps that
//! order exactly, and `a_decline_is_not_re_asked_but_a_confirmation_in_the_same_turn_wins`
//! pins both halves.

use archava_core::actions::{
    ActionDefinition, ActionLevel, ActionRegistry, ActionRegistryError, ConfirmationMode,
};
use archava_core::roles::{satisfies_capability, CapabilityTier, RoleName};
use archava_core::tenant::TenantId;

/// Actions at or above this level can never run autonomously.
///
/// `policy.ts:46`.
pub const NEVER_AUTONOMOUS_LEVEL: ActionLevel = ActionLevel::L5;

/// Every reason the gate can refuse a command.
///
/// The seven reasons are the union at `policy.ts:48-56`, in that order. A reason
/// here with no branch producing it would be claiming a denial the gate never
/// issues; a branch with no reason here would be a refusal a client could not
/// handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionDenial {
    UnknownAction,
    ActionDisabledForClient,
    RoleNotAllowed,
    CapabilityInsufficient,
    ActionUnavailableOnPage,
    AdminActionRequiresHuman,
    DeclinedByVisitor,
}

impl ActionDenial {
    /// The wire name, matching the TypeScript string-literal union.
    ///
    /// These strings are part of the API contract, not incidental formatting:
    /// clients already handle these exact values.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnknownAction => "unknown_action",
            Self::ActionDisabledForClient => "action_disabled_for_client",
            Self::RoleNotAllowed => "role_not_allowed",
            Self::CapabilityInsufficient => "capability_insufficient",
            Self::ActionUnavailableOnPage => "action_unavailable_on_page",
            Self::AdminActionRequiresHuman => "admin_action_requires_human",
            Self::DeclinedByVisitor => "declined_by_visitor",
        }
    }
}

impl std::fmt::Display for ActionDenial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A command that has passed the gate.
///
/// Carries the resolved [`ActionDefinition`] rather than the action id, so a
/// caller cannot substitute a different action after authorization. The value it
/// holds is the one the gate actually evaluated.
///
/// Not `Copy`, because `TenantId` owns a `String`. That is the correct trade:
/// copying a tenant scope by value would be free of any sign that it happened,
/// and this type is not in a hot path where the allocation would matter.
#[derive(Debug, Clone)]
pub struct AuthorizedCommand {
    pub tenant: TenantId,
    pub role: RoleName,
    pub capability: CapabilityTier,
    pub action: ActionDefinition,
}

/// A request to run an action.
///
/// Mirrors `ActionRequest` (`policy.ts:59-88`), including the semantics that are
/// easy to get wrong: `available_on_page`, `confirmed`, `declined` and
/// `human_approved` are all `Option<bool>` because "unset" is a distinct state
/// from `false` throughout the original — `available_on_page: undefined` is not a
/// denial, and `confirmed != Some(true)` is what lets a change of mind win.
#[derive(Debug, Clone)]
pub struct ActionRequest {
    pub tenant: TenantId,
    pub role: RoleName,
    pub capability: CapabilityTier,
    pub action_id: String,
    /// Page-awareness narrowing. `Some(false)` means the host page declared this
    /// action unavailable; `None` means the page said nothing, which is not a
    /// denial.
    pub available_on_page: Option<bool>,
    /// The visitor confirmed a `user_confirm` action.
    pub confirmed: Option<bool>,
    /// The visitor declined this action earlier in the session.
    ///
    /// The mirror of `confirmed`, and it exists for the same reason: without it
    /// the gate has nowhere to put a refusal except back into
    /// `confirmation_required`, and an action the visitor said no to is asked
    /// again on every later turn — a confirm loop that does not converge, because
    /// nothing changes between turns.
    ///
    /// A decline is *overridden*, not erased, by a confirmation in the same turn.
    pub declined: Option<bool>,
    /// A human operator approved an L5 action.
    pub human_approved: Option<bool>,
    /// Tenant-level action allow-list, when the client config narrows further.
    pub enabled_action_ids: Option<Vec<String>>,
}

/// The gate's answer.
///
/// Three outcomes, matching `PolicyResult` (`policy.ts:90-103`). The middle one
/// is not an error: it is a *question*, and the caller is expected to surface
/// `prompt` and ask again with `confirmed` or `human_approved` set.
///
/// `PartialEq` is deliberately not derived: [`ActionDefinition`] does not
/// implement it, and faking one here to make tests terser would compare
/// definitions by pointer identity rather than by content. The accessors below
/// exist so tests assert on the fields that actually matter.
#[derive(Debug, Clone)]
pub enum PolicyDecision {
    Allowed {
        action: ActionDefinition,
    },
    ConfirmationRequired {
        action: ActionDefinition,
        mode: ConfirmationMode,
        prompt: &'static str,
    },
    Denied {
        action: Option<ActionDefinition>,
        reason: ActionDenial,
        message: String,
    },
}

impl PolicyDecision {
    /// The resolved action, when one was found.
    pub fn action(&self) -> Option<&ActionDefinition> {
        match self {
            Self::Allowed { action }
            | Self::ConfirmationRequired { action, .. }
            | Self::Denied {
                action: Some(action),
                ..
            } => Some(action),
            Self::Denied { action: None, .. } => None,
        }
    }

    /// The denial reason, when this is a refusal.
    pub fn denial_reason(&self) -> Option<ActionDenial> {
        match self {
            Self::Denied { reason, .. } => Some(*reason),
            _ => None,
        }
    }

    /// The refusal message, when this is a refusal.
    pub fn message(&self) -> Option<&str> {
        match self {
            Self::Denied { message, .. } => Some(message),
            _ => None,
        }
    }

    /// The confirmation mode being asked for, when this is a question.
    pub fn confirmation_mode(&self) -> Option<ConfirmationMode> {
        match self {
            Self::ConfirmationRequired { mode, .. } => Some(*mode),
            _ => None,
        }
    }
}

/// The visitor-facing prompt for each action level.
///
/// Verbatim from `CONFIRMATION_PROMPTS` (`policy.ts:105-112`). These strings are
/// user-visible, so a rewrite is a UX change and not a refactor.
fn confirmation_prompt(level: ActionLevel) -> &'static str {
    match level {
        ActionLevel::L0 => "This reads live data.",
        ActionLevel::L1 => "This changes what you see on the page.",
        ActionLevel::L2 => "This change is reversible. Shall I proceed?",
        ActionLevel::L3 => {
            "This sends data to an external system and cannot be undone by me. Shall I proceed?"
        }
        ActionLevel::L4 => {
            "This involves a payment or an account change. Please confirm before I continue."
        }
        ActionLevel::L5 => "This is an administrative change and needs a human to approve it.",
    }
}

/// The gate.
///
/// Holds no state. The registry is static (`BASELINE_ACTIONS` in
/// `archava_core::actions`), and there is no database access by design: every
/// input is either a caller-supplied identity claim or a static contract. That
/// is what makes the whole gate testable without a database, and therefore
/// testable on every machine rather than only ones with one.
#[derive(Debug, Default, Clone, Copy)]
pub struct CommandAuthorizer;

impl CommandAuthorizer {
    pub fn new() -> Self {
        Self
    }

    /// The registry version this gate enforces.
    pub fn version(&self) -> &'static str {
        ActionRegistry::version()
    }

    /// Resolves an action id, failing closed on anything unregistered.
    pub fn describe(&self, action_id: &str) -> Result<ActionDefinition, ActionRegistryError> {
        ActionRegistry::get(action_id).copied()
    }

    /// Evaluates one request.
    ///
    /// The branch order is the mechanism, not a style choice — see the module
    /// header. Do not reorder these without re-reading `policy.ts:189-224`.
    pub fn evaluate(&self, request: &ActionRequest) -> PolicyDecision {
        let Some(action) = ActionRegistry::try_get(&request.action_id) else {
            return PolicyDecision::Denied {
                action: None,
                reason: ActionDenial::UnknownAction,
                message: format!(
                    "Action \"{}\" is not registered. Unregistered actions are never executed.",
                    request.action_id
                ),
            };
        };

        // A deprecated action still runs — deprecation is a migration signal, not
        // a revocation — but the caller is told which id replaces it.
        let deprecation_note = match (action.deprecated_in, action.replaced_by) {
            (Some(version), Some(replacement)) => {
                format!(" (deprecated in {version}, use \"{replacement}\")")
            }
            (Some(version), None) => format!(" (deprecated in {version})"),
            (None, _) => String::new(),
        };

        if let Some(enabled) = &request.enabled_action_ids {
            if !enabled.iter().any(|id| id == action.id) {
                return PolicyDecision::Denied {
                    action: Some(*action),
                    reason: ActionDenial::ActionDisabledForClient,
                    message: format!(
                        "Action \"{}\" is not enabled for this client configuration{}.",
                        action.id, deprecation_note
                    ),
                };
            }
        }

        if !action.allows_role(request.role) {
            return PolicyDecision::Denied {
                action: Some(*action),
                reason: ActionDenial::RoleNotAllowed,
                message: format!(
                    "Role \"{}\" may not invoke \"{}\"{}. Allowed: {}.",
                    request.role,
                    action.id,
                    deprecation_note,
                    action
                        .allowed_roles
                        .iter()
                        .map(|role| role.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            };
        }

        if !satisfies_capability(request.capability, action.requires_capability) {
            return PolicyDecision::Denied {
                action: Some(*action),
                reason: ActionDenial::CapabilityInsufficient,
                message: format!(
                    "\"{}\" needs the {} capability; this client is {}{}.",
                    action.id, action.requires_capability, request.capability, deprecation_note
                ),
            };
        }

        if action.level >= NEVER_AUTONOMOUS_LEVEL && request.human_approved != Some(true) {
            return PolicyDecision::Denied {
                action: Some(*action),
                reason: ActionDenial::AdminActionRequiresHuman,
                message: format!(
                    "\"{}\" is {} and never runs autonomously. A human must approve it{}.",
                    action.id,
                    action.level.as_str(),
                    deprecation_note
                ),
            };
        }

        if request.available_on_page == Some(false) {
            return PolicyDecision::Denied {
                action: Some(*action),
                reason: ActionDenial::ActionUnavailableOnPage,
                message: format!(
                    "The current page does not offer \"{}\"{}.",
                    action.id, deprecation_note
                ),
            };
        }

        // Placed here on purpose. See the module header: after the denials a
        // visitor's refusal cannot soften, before the questions the refusal is
        // the answer to. `request.confirmed != Some(true)` is what lets a change
        // of mind win — a confirmation arriving in the same turn skips this
        // branch and falls through to the confirmation branches below, which it
        // satisfies.
        if request.declined == Some(true) && request.confirmed != Some(true) {
            return PolicyDecision::Denied {
                action: Some(*action),
                reason: ActionDenial::DeclinedByVisitor,
                message: format!(
                    "\"{}\" was declined earlier in this session{}.",
                    action.id, deprecation_note
                ),
            };
        }

        if action.confirmation == ConfirmationMode::HumanApproval
            && request.human_approved != Some(true)
        {
            return PolicyDecision::ConfirmationRequired {
                action: *action,
                mode: ConfirmationMode::HumanApproval,
                prompt: confirmation_prompt(action.level),
            };
        }

        if action.confirmation == ConfirmationMode::UserConfirm && request.confirmed != Some(true) {
            return PolicyDecision::ConfirmationRequired {
                action: *action,
                mode: ConfirmationMode::UserConfirm,
                prompt: confirmation_prompt(action.level),
            };
        }

        PolicyDecision::Allowed { action: *action }
    }

    /// The action ids this caller could be authorised for, with everything
    /// confirmed.
    ///
    /// The port of `permitted` (`policy.ts:254-265`). Its header comment states a
    /// rule that is easy to lose in a rewrite, so it is carried here:
    ///
    /// > the tool list the brain sees is of actions an operator or a visitor
    /// > could authorise. A decline deliberately does not narrow this list. It is
    /// > session state, and this is a function of capability, role and the page —
    /// > the three things that say what the client in general may do. Folding in
    /// > what one visitor refused would make the tool list change with the
    /// > conversation rather than with the contract, which is a different
    /// > function with the same name.
    ///
    /// Hence `declined` is deliberately not consulted here, while `confirmed` and
    /// `human_approved` are forced to `Some(true)` so the confirmation branches do
    /// not filter the list.
    pub fn permitted_actions(&self, request: &ActionRequest) -> Vec<String> {
        ActionRegistry::list()
            .iter()
            .filter(|action| {
                let probe = ActionRequest {
                    action_id: action.id.to_string(),
                    confirmed: Some(true),
                    human_approved: Some(true),
                    ..request.clone()
                };
                matches!(self.evaluate(&probe), PolicyDecision::Allowed { .. })
            })
            .map(|action| action.id.to_string())
            .collect()
    }

    /// Evaluates a request and returns the authorized command, or the refusal.
    ///
    /// The refusal is boxed. `PolicyDecision` is over 150 bytes because it carries
    /// a resolved [`ActionDefinition`], and an `Err` of that size would be copied
    /// by value through every `?` on the caller's path — clippy's
    /// `result_large_err` is right to object. One heap allocation on the refusal
    /// path is the cheaper trade, and the refusal path is the one that is already
    /// doing string formatting for the operator-facing message.
    pub fn authorize(
        &self,
        request: &ActionRequest,
    ) -> Result<AuthorizedCommand, Box<PolicyDecision>> {
        match self.evaluate(request) {
            PolicyDecision::Allowed { action } => Ok(AuthorizedCommand {
                tenant: request.tenant.clone(),
                role: request.role,
                capability: request.capability,
                action,
            }),
            refusal => Err(Box::new(refusal)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(action_id: &str, capability: CapabilityTier, role: RoleName) -> ActionRequest {
        ActionRequest {
            tenant: TenantId::new("acme-retail").expect("fixture tenant"),
            role,
            capability,
            action_id: action_id.to_string(),
            available_on_page: None,
            confirmed: None,
            declined: None,
            human_approved: None,
            enabled_action_ids: None,
        }
    }

    // --- Fail-closed on the unknown ----------------------------------------

    #[test]
    fn an_unregistered_action_is_denied_rather_than_executed() {
        let gate = CommandAuthorizer::new();
        let decision = gate.evaluate(&request(
            "admin.destroy_everything",
            CapabilityTier::Enterprise,
            RoleName::Owner,
        ));
        // `ActionDefinition` does not implement `PartialEq`, so the absence of a
        // resolved action is asserted rather than compared to a value.
        assert!(
            decision.action().is_none(),
            "an unregistered action must not resolve to a definition"
        );
        assert_eq!(decision.denial_reason(), Some(ActionDenial::UnknownAction));
        assert_eq!(
            decision.message(),
            Some(
                "Action \"admin.destroy_everything\" is not registered. \
                 Unregistered actions are never executed."
            )
        );
    }

    // --- The two structural rules ------------------------------------------

    /// PRD §18. The whole point of the tier model is that it does not buy a
    /// bypass of the confirmation gate.
    #[test]
    fn a_transact_client_still_needs_confirmation_for_an_l4_action() {
        let gate = CommandAuthorizer::new();
        let decision = gate.evaluate(&request(
            "checkout.start",
            CapabilityTier::Transact,
            RoleName::ArchavaAssistant,
        ));
        assert_eq!(
            decision.confirmation_mode(),
            Some(ConfirmationMode::UserConfirm)
        );
        assert_eq!(
            decision.action().map(|action| action.id),
            Some("checkout.start")
        );
        assert_eq!(
            decision.message(),
            None,
            "a confirmation question is not a refusal and carries no denial message"
        );
    }

    /// PRD §16. A page can only narrow, never widen.
    #[test]
    fn a_page_can_remove_an_action_but_never_add_one() {
        let gate = CommandAuthorizer::new();

        let mut denied = request(
            "product.read",
            CapabilityTier::Assist,
            RoleName::ArchavaAssistant,
        );
        denied.available_on_page = Some(false);
        assert_eq!(
            gate.evaluate(&denied).denial_reason(),
            Some(ActionDenial::ActionUnavailableOnPage)
        );

        // And a page saying nothing is not a denial.
        let mut silent = request(
            "product.read",
            CapabilityTier::Assist,
            RoleName::ArchavaAssistant,
        );
        silent.available_on_page = None;
        assert!(matches!(
            gate.evaluate(&silent),
            PolicyDecision::Allowed { .. }
        ));
    }

    // --- Branch ordering ---------------------------------------------------

    /// The decline branch sits after the business-rule denials, so a decline
    /// cannot relabel a client limit as the visitor's own choice.
    #[test]
    fn a_decline_does_not_soften_a_role_denial() {
        let gate = CommandAuthorizer::new();
        let mut req = request("product.read", CapabilityTier::Assist, RoleName::Viewer);
        req.declined = Some(true);
        assert_eq!(
            gate.evaluate(&req).denial_reason(),
            Some(ActionDenial::RoleNotAllowed)
        );
    }

    /// And before the confirmation branches, so a decline is not re-asked.
    #[test]
    fn a_decline_is_not_re_asked_but_a_confirmation_in_the_same_turn_wins() {
        let gate = CommandAuthorizer::new();

        let mut declined = request(
            "lead.capture",
            CapabilityTier::Act,
            RoleName::ArchavaAssistant,
        );
        declined.declined = Some(true);
        assert_eq!(
            gate.evaluate(&declined).denial_reason(),
            Some(ActionDenial::DeclinedByVisitor)
        );

        // Changing one's mind in the same turn must win.
        let mut changed = declined;
        changed.confirmed = Some(true);
        assert!(matches!(
            gate.evaluate(&changed),
            PolicyDecision::Allowed { .. }
        ));
    }

    /// L5 is gated by level. A higher capability tier does not remove that gate.
    #[test]
    fn an_l5_action_never_runs_autonomously_regardless_of_capability() {
        let gate = CommandAuthorizer::new();

        let mut req = request(
            "admin.config.update",
            CapabilityTier::Enterprise,
            RoleName::Owner,
        );
        assert_eq!(
            gate.evaluate(&req).denial_reason(),
            Some(ActionDenial::AdminActionRequiresHuman)
        );
        assert_eq!(
            gate.evaluate(&req).message(),
            Some(
                "\"admin.config.update\" is L5 and never runs autonomously. \
                 A human must approve it."
            )
        );

        req.human_approved = Some(true);
        assert!(matches!(
            gate.evaluate(&req),
            PolicyDecision::Allowed { .. }
        ));
    }

    /// The capability fail-safe: an under-tiered client is denied, never
    /// half-allowed.
    #[test]
    fn an_under_capability_client_is_denied() {
        let gate = CommandAuthorizer::new();
        let req = request(
            "payment.initiate",
            CapabilityTier::Act,
            RoleName::ArchavaAssistant,
        );
        let decision = gate.evaluate(&req);
        assert_eq!(
            decision.denial_reason(),
            Some(ActionDenial::CapabilityInsufficient)
        );
        assert_eq!(
            decision.message(),
            Some("\"payment.initiate\" needs the transact capability; this client is act.")
        );
    }

    /// A tier the ranking does not recognise must under-privilege rather than
    /// over-privilege. `satisfies_capability` is the core implementation, but the
    /// property it must preserve is asserted here where the gate uses it.
    #[test]
    fn the_capability_ranking_cannot_be_widened_by_an_unrecognised_tier() {
        assert!(satisfies_capability(
            CapabilityTier::Transact,
            CapabilityTier::Act
        ));
        assert!(!satisfies_capability(
            CapabilityTier::Assist,
            CapabilityTier::Act
        ));
        assert!(satisfies_capability(
            CapabilityTier::Act,
            CapabilityTier::Act
        ));
    }

    // --- Client configuration ----------------------------------------------

    #[test]
    fn a_client_configuration_can_narrow_the_action_set() {
        let gate = CommandAuthorizer::new();
        let mut req = request(
            "product.read",
            CapabilityTier::Assist,
            RoleName::ArchavaAssistant,
        );
        req.enabled_action_ids = Some(vec!["lead.capture".to_string()]);
        assert_eq!(
            gate.evaluate(&req).denial_reason(),
            Some(ActionDenial::ActionDisabledForClient)
        );
    }

    /// `permitted` is a function of capability, role and page — never of what one
    /// visitor refused.
    #[test]
    fn the_permitted_list_ignores_a_single_visitors_decline() {
        let gate = CommandAuthorizer::new();
        let base = request(
            "product.read",
            CapabilityTier::Act,
            RoleName::ArchavaAssistant,
        );

        let mut declined = base.clone();
        declined.declined = Some(true);
        assert_eq!(
            gate.permitted_actions(&declined),
            gate.permitted_actions(&base),
            "a decline must not narrow the tool list"
        );
    }

    // --- Authorization result ----------------------------------------------

    #[test]
    fn authorize_carries_the_resolved_action_not_an_id_to_re_resolve() {
        let gate = CommandAuthorizer::new();
        let req = request(
            "product.read",
            CapabilityTier::Assist,
            RoleName::ArchavaAssistant,
        );
        let authorized = gate.authorize(&req).expect("an L0 read is allowed");
        assert_eq!(authorized.tenant.as_str(), "acme-retail");
        assert_eq!(authorized.action.id, "product.read");
        assert_eq!(authorized.capability, CapabilityTier::Assist);
        assert_eq!(authorized.role, RoleName::ArchavaAssistant);
    }

    #[test]
    fn authorize_surfaces_the_confirmation_question_rather_than_allowing() {
        let gate = CommandAuthorizer::new();
        // `form.submit`, not `lead.capture`: the latter is L2 with
        // `ConfirmationMode::None` and is simply allowed, so it would prove
        // nothing about the confirmation path. `form.submit` is the baseline's
        // first `user_confirm` action (`packages/acl/src/actions.ts:246-258`).
        let req = request(
            "form.submit",
            CapabilityTier::Act,
            RoleName::ArchavaAssistant,
        );
        // A confirmation question is not a refusal, and `authorize` must not
        // report it as one: the caller is expected to ask again with the
        // confirmation, not to treat the gate as broken. The outer parentheses
        // matter — `matches!(x, Err(e) if cond(e))` parses the guard as a
        // separate clause, so `e` is out of scope and the match silently becomes
        // a bare `Err(_)` test.
        assert!(matches!(
            gate.authorize(&req),
            Err(refusal) if (matches!(*refusal, PolicyDecision::ConfirmationRequired { .. }))
        ));
    }

    #[test]
    fn every_denial_reason_has_a_distinct_wire_name() {
        let reasons = [
            ActionDenial::UnknownAction,
            ActionDenial::ActionDisabledForClient,
            ActionDenial::RoleNotAllowed,
            ActionDenial::CapabilityInsufficient,
            ActionDenial::ActionUnavailableOnPage,
            ActionDenial::AdminActionRequiresHuman,
            ActionDenial::DeclinedByVisitor,
        ];
        let names: Vec<&str> = reasons.iter().map(|reason| reason.as_str()).collect();
        let mut unique = names.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(
            unique.len(),
            reasons.len(),
            "two denial reasons must not share a wire name: {names:?}"
        );
    }
}
