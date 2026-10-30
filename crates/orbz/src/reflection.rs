//! Continuous Learning & Autonomous Reflection loop for Ava Orbz.
//! Implements Hermes/Letta's tacit knowledge extraction:
//! Evaluates dialogue turns to extract unwritten business rules, vendor preferences,
//! and customer habits, while enforcing L4 confirmation gates on policy changes.

use crate::security::{AuthorityTier, OrbzActor, OrbzScope};
use serde::{Deserialize, Serialize};

/// A synthesized business fact extracted from conversation turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SynthesizedFact {
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub scope: OrbzScope,
    pub requires_l4_confirmation: bool,
    pub summary: String,
}

pub struct ReflectionSynthesizer;

impl ReflectionSynthesizer {
    /// Inspects an utterance and actor to determine if a tacit business rule was declared.
    pub fn extract_candidate(utterance: &str, actor: &OrbzActor) -> Option<SynthesizedFact> {
        let trimmed = utterance.trim();
        let lower = trimmed.to_lowercase();

        // Pattern 1: Supplier preference / substitution
        // "mulai sekarang kalau pesan [X] ke [Y]" or "ganti supplier [X] ke [Y]"
        if (lower.contains("supplier") || lower.contains("vendor") || lower.contains("pesan"))
            && (lower.contains("pindah") || lower.contains("ganti") || lower.contains("ke"))
        {
            // Only managers or owners can establish vendor rules
            if actor.authority >= AuthorityTier::L3 {
                return Some(SynthesizedFact {
                    subject: "VendorSelection".into(),
                    predicate: "PreferredSupplierDirective".into(),
                    object: trimmed.into(),
                    scope: OrbzScope::Department("purchasing".into()),
                    requires_l4_confirmation: false, // Tactical heuristic, auto-memorized
                    summary: format!("Tactical vendor preference: {}", trimmed),
                });
            }
        }

        // Pattern 2: Financial policy & discount mutations
        // "diskon member jadi [X]%" or "harga [X] dinaikin jadi [Y]"
        if lower.contains("diskon") || lower.contains("harga") || lower.contains("pajak") {
            // Must have owner or executive authority
            if actor.is_owner || actor.authority >= AuthorityTier::L4 {
                return Some(SynthesizedFact {
                    subject: "PricingPolicy".into(),
                    predicate: "PriceOrDiscountMutation".into(),
                    object: trimmed.into(),
                    scope: OrbzScope::Public,
                    requires_l4_confirmation: true, // Financial mutation: MUST be confirmed via L4
                    summary: format!("Pricing policy change proposal: {}", trimmed),
                });
            }
        }

        // Pattern 3: Customer habits / preferences (e.g. "pelanggan ini alergi...")
        if lower.contains("alergi") || lower.contains("suka") || lower.contains("langganan") {
            return Some(SynthesizedFact {
                subject: "CustomerHabit".into(),
                predicate: "PersonalPreference".into(),
                object: trimmed.into(),
                scope: OrbzScope::Entity {
                    entity_type: "customer".into(),
                    entity_id: "auto".into(),
                },
                requires_l4_confirmation: false,
                summary: format!("Customer preference note: {}", trimmed),
            });
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_vendor_preference_by_manager() {
        let manager = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_mgr".into(),
            authority: AuthorityTier::L3,
            department: Some("purchasing".into()),
            is_owner: false,
        };

        let fact = ReflectionSynthesizer::extract_candidate(
            "Mulai sekarang kalau pesan telur pindah ke UD Barokah ya",
            &manager,
        );

        assert!(fact.is_some());
        let f = fact.unwrap();
        assert_eq!(f.subject, "VendorSelection");
        assert!(!f.requires_l4_confirmation);
    }

    #[test]
    fn test_cashier_cannot_mutate_pricing_policy() {
        let cashier = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_cashier".into(),
            authority: AuthorityTier::L2,
            department: Some("pos".into()),
            is_owner: false,
        };

        let fact =
            ReflectionSynthesizer::extract_candidate("Diskon member dinaikkan jadi 50%", &cashier);

        assert!(fact.is_none(), "Cashier cannot declare pricing mutations");
    }

    #[test]
    fn test_owner_pricing_mutation_triggers_l4_gate() {
        let owner = OrbzActor {
            tenant_id: "ten_01".into(),
            user_id: "usr_owner".into(),
            authority: AuthorityTier::L5,
            department: None,
            is_owner: true,
        };

        let fact = ReflectionSynthesizer::extract_candidate(
            "Mulai besok diskon member dinaikkan jadi 15%",
            &owner,
        );

        assert!(fact.is_some());
        let f = fact.unwrap();
        assert!(f.requires_l4_confirmation, "Must require confirmation");
    }
}
