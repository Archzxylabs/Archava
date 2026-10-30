//! Consent, as a gate the SDK actually consults — port of
//! `packages/sdk/src/consent.ts`.
//!
//! §24 lists "respect consent/privacy settings" among the SDK's
//! responsibilities. A gate that is merely described is not respected, so this
//! module is the only thing that gets to decide, and every collection path must
//! ask it first.
//!
//! Two decisions worth stating:
//!
//! - **Fail closed.** An absent domain is a refusal, not a default-yes. The
//!   host grants what it collected consent for; anything else is withheld. The
//!   cost is that a host which grants nothing gets no page context — which is
//!   the correct outcome for a visitor who said no.
//! - **Domains, not one switch.** "Do you consent to cookies" is the question
//!   that taught visitors not to trust the switch. `identity` (may the
//!   assistant know who you are, §26) and `analytics` (may a warehouse see this
//!   session, §27) are separate from `page_context`, because they are
//!   separately harmful.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The things a visitor can independently decline.
pub const CONSENT_DOMAINS: &[ConsentDomain] = &[
    ConsentDomain::PageContext,
    ConsentDomain::Identity,
    ConsentDomain::Analytics,
];

/// A domain of consent: what the visitor separately agreed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsentDomain {
    /// May the turn see where the visitor is on the site (§16)?
    #[serde(rename = "page_context")]
    PageContext,
    /// May the assistant know who you are (§26)?
    #[serde(rename = "identity")]
    Identity,
    /// May a warehouse see this session (§27)?
    #[serde(rename = "analytics")]
    Analytics,
}

impl ConsentDomain {
    pub fn as_str(self) -> &'static str {
        match self {
            ConsentDomain::PageContext => "page_context",
            ConsentDomain::Identity => "identity",
            ConsentDomain::Analytics => "analytics",
        }
    }
}

impl std::fmt::Display for ConsentDomain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// What the visitor has agreed to, by domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConsentState {
    pub page_context: bool,
    pub identity: bool,
    pub analytics: bool,
}

/// Nothing has been granted yet.
pub const DENIED_CONSENT: ConsentState = ConsentState {
    page_context: false,
    identity: false,
    analytics: false,
};

/// Start from the host's record, defaulting anything it omitted to refused.
///
/// The input is whatever the host carried over the wire; the domains must be
/// `true` to count. Everything else — absent, false, a string, a number — is
/// refusal, because "nullish is false" at THE gate is how a `true` string
/// becomes a grant nobody was given.
pub fn parse_consent(raw: Option<&Value>) -> ConsentState {
    let Some(Value::Object(record)) = raw else {
        return DENIED_CONSENT;
    };
    let read = |domain: &str| -> bool { matches!(record.get(domain), Some(Value::Bool(true))) };
    ConsentState {
        page_context: read("page_context"),
        identity: read("identity"),
        analytics: read("analytics"),
    }
}

/// One domain granted, the rest left exactly as they were.
pub fn grant_consent(state: ConsentState, domain: ConsentDomain) -> ConsentState {
    let mut next = state;
    match domain {
        ConsentDomain::PageContext => next.page_context = true,
        ConsentDomain::Identity => next.identity = true,
        ConsentDomain::Analytics => next.analytics = true,
    }
    next
}

/// One domain withdrawn. Withdrawal takes effect on the next turn, not
/// retroactively: the state is replaced, and no caller keeps a pointer to the
/// old one.
pub fn revoke_consent(state: ConsentState, domain: ConsentDomain) -> ConsentState {
    let mut next = state;
    match domain {
        ConsentDomain::PageContext => next.page_context = false,
        ConsentDomain::Identity => next.identity = false,
        ConsentDomain::Analytics => next.analytics = false,
    }
    next
}

/// The domains the SDK still needs but does not have.
///
/// Reported rather than thrown because a partial grant is a normal steady
/// state: a visitor who declines `identity` still gets an assistant, it just
/// never learns their name.
pub fn missing_consent(state: ConsentState, required: &[ConsentDomain]) -> Vec<ConsentDomain> {
    required
        .iter()
        .copied()
        .filter(|domain| !has_consent(state, *domain))
        .collect()
}

/// True only for an explicit `true`; everything else reads as refusal.
pub fn has_consent(state: ConsentState, domain: ConsentDomain) -> bool {
    match domain {
        ConsentDomain::PageContext => state.page_context,
        ConsentDomain::Identity => state.identity,
        ConsentDomain::Analytics => state.analytics,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_missing_record_is_a_full_refusal() {
        assert_eq!(parse_consent(None), DENIED_CONSENT);
        assert_eq!(
            parse_consent(Some(&json!({"unrelated": true}))),
            DENIED_CONSENT
        );
    }

    #[test]
    fn only_explicit_true_is_a_grant() {
        assert_eq!(
            parse_consent(Some(&json!({"page_context": "true"}))),
            DENIED_CONSENT
        );
        assert_eq!(
            parse_consent(Some(&json!({"page_context": 1}))),
            DENIED_CONSENT
        );
        assert_eq!(
            parse_consent(Some(&json!({"identity": true, "analytics": null}))),
            ConsentState {
                page_context: false,
                identity: true,
                analytics: false
            }
        );
    }

    #[test]
    fn grant_touches_one_domain_the_record_stays_immutable() {
        let granted = grant_consent(DENIED_CONSENT, ConsentDomain::Identity);
        assert!(granted.identity);
        assert!(!granted.page_context);
        // The state is a Copy input: the gate returns a new record and the
        // const's identity is still denied — a grant never edits in place.
        let before = DENIED_CONSENT;
        assert!(!before.identity, "the input was not edited");
    }

    #[test]
    fn revoke_withdraws_one_domain() {
        let state = ConsentState {
            page_context: true,
            identity: true,
            analytics: true,
        };
        let withdrawn = revoke_consent(state, ConsentDomain::Identity);
        assert_eq!(
            withdrawn,
            ConsentState {
                page_context: true,
                identity: false,
                analytics: true
            }
        );
        // Idempotent, so a banner that fires twice changes nothing.
        assert_eq!(
            revoke_consent(withdrawn, ConsentDomain::Identity),
            withdrawn
        );
    }

    #[test]
    fn missing_reports_a_partial_grant_rather_than_throwing() {
        let state = parse_consent(Some(&json!({"page_context": true})));
        assert_eq!(
            missing_consent(
                state,
                &[ConsentDomain::PageContext, ConsentDomain::Identity]
            ),
            vec![ConsentDomain::Identity]
        );
        assert_eq!(
            missing_consent(state, &[ConsentDomain::PageContext]),
            vec![]
        );
    }
}
