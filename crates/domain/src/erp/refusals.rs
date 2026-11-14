//! The refusal reason catalogue: what a database guard refused, in words a
//! person can act on.
//!
//! Business rules are enforced by database guards that raise one sentence
//! each. `config/refusal-reasons.v1.json` names the guards whose refusal a
//! user plausibly meets: the key is the guard's exact message (applied
//! migrations are frozen by checksum, so the text is a stable key) and the
//! entry carries a stable code plus an English and an Indonesian sentence.
//! A guard that is not in the catalogue keeps its module's general answer.
//!
//! `embedded/` is what this crate compiles in (`tools/rust/domain-assets.mjs
//! --write` refreshes it). See `docs/architecture/REFUSAL_REASONS.md`.
use serde::Deserialize;
use std::collections::BTreeMap;
use std::sync::OnceLock;

/// One catalogued refusal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The exact message the database guard raises.
    pub guard: String,
    /// Stable snake_case identifier; clients may branch on it.
    pub code: String,
    /// What was refused and what to do next, in English.
    pub message: String,
    /// The same sentence in Indonesian.
    pub message_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    code: String,
    message: String,
    message_id: String,
}

#[derive(Deserialize)]
struct Document {
    reasons: BTreeMap<String, Entry>,
}

/// Every catalogued refusal, keyed by guard message.
pub fn catalogue() -> &'static BTreeMap<String, Refusal> {
    static DATA: OnceLock<BTreeMap<String, Refusal>> = OnceLock::new();
    DATA.get_or_init(|| {
        let document: Document =
            serde_json::from_str(include_str!("../../embedded/refusal-reasons.v1.json"))
                .expect("the embedded refusal reason catalogue is valid");
        document
            .reasons
            .into_iter()
            .map(|(guard, entry)| {
                let refusal = Refusal {
                    guard: guard.clone(),
                    code: entry.code,
                    message: entry.message,
                    message_id: entry.message_id,
                };
                (guard, refusal)
            })
            .collect()
    })
}

/// The refusal a guard message stands for. The whole message must equal a
/// catalogue key: a longer message that merely contains one does not match.
pub fn for_guard(message: &str) -> Option<&'static Refusal> {
    catalogue().get(message)
}

/// The refusal a code names, for a rule the adapter decides before the
/// database guard would.
pub fn for_code(code: &str) -> Option<&'static Refusal> {
    catalogue().values().find(|refusal| refusal.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn snake_case(code: &str) -> bool {
        !code.is_empty()
            && code.len() <= 64
            && code.starts_with(|c: char| c.is_ascii_lowercase())
            && !code.ends_with('_')
            && !code.contains("__")
            && code
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
    }

    #[test]
    fn every_code_is_unique_and_snake_case() {
        let mut seen = BTreeSet::new();
        assert!(!catalogue().is_empty());
        for refusal in catalogue().values() {
            assert!(snake_case(&refusal.code), "{}", refusal.code);
            assert!(seen.insert(&refusal.code), "duplicate {}", refusal.code);
        }
    }

    #[test]
    fn every_entry_has_a_guard_and_both_sentences() {
        for refusal in catalogue().values() {
            assert_eq!(refusal.guard.trim(), refusal.guard, "{}", refusal.code);
            // A guard raised with format arguments has no fixed text to key on.
            assert!(!refusal.guard.contains('%'), "{}", refusal.code);
            for sentence in [&refusal.message, &refusal.message_id] {
                assert!(sentence.len() >= 20, "{}", refusal.code);
                assert!(sentence.ends_with('.'), "{}", refusal.code);
            }
            assert_ne!(refusal.message, refusal.message_id, "{}", refusal.code);
        }
    }

    #[test]
    fn a_guard_matches_only_as_its_whole_message() {
        let known = "Resolve draft journals before closing";
        let refusal = for_guard(known).expect("catalogued");
        assert_eq!(refusal.code, "period_close_draft_journals");
        assert_eq!(for_code("period_close_draft_journals"), Some(refusal));
        assert_eq!(for_guard(&format!("{known} now")), None);
        assert_eq!(for_guard(&format!("x {known}")), None);
        assert_eq!(for_guard(&known.to_lowercase()), None);
        assert_eq!(for_guard(""), None);
        assert_eq!(for_code("no_such_code"), None);
    }
}
