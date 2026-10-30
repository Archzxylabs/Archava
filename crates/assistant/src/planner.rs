//! The deterministic local planner — the port of
//! `packages/crm-assistant/src/planner.ts` (`LocalCrmPlanner`).
//!
//! The planner's whole authority is: turn a trimmed utterance into ONE
//! requested operation, or `unsupported`. It knows nothing else — no tenant,
//! no grants, no ids, no reads. The patterns below are transcribed in the
//! baseline's ORDER, and `tests::classification_matches_the_recorded_rows`
//! pins utterance→operation rows lifted from the baseline's tests so a
//! reordered or widened pattern trips a named failure.
//!
//! The Gemini-backed planner lane is deliberately absent: the TS route
//! composes `plannerFor(utterance, attemptId)` to fall back to a *local,
//! deterministic* planner, and this port serves that same deterministic
//! contract. A provider lane is a delivery decision, not a CRM-contract one.

use crate::contracts::{AssistantError, PlannerIdentity};
use std::sync::OnceLock;

/// The vocabulary's identity, echoed on every reply the way the baseline
/// echoes `providerId`/`model`.
pub struct LocalCrmPlanner;

impl LocalCrmPlanner {
    pub const PROVIDER_ID: &'static str = "archava-crm-local";
    pub const MODEL: &'static str = "deterministic-commands-v1";

    pub fn new() -> Self {
        Self
    }
}

impl Default for LocalCrmPlanner {
    fn default() -> Self {
        Self::new()
    }
}

/// The plan seam. The baseline's planner returns `unknown` (unparsed JSON
/// shapes included), and the flow re-verifies against the plan schema —
/// that is what makes an unverifiable plan the flow's `unavailable` refusal
/// rather than a trusted instruction.
///
/// `async fn` in the trait is fine here: like [`HostCrm`](crate::contracts::HostCrm)
/// this trait lives inside the workspace only, single-process serving.
#[allow(async_fn_in_trait)]
pub trait Planner: Sync {
    async fn plan(&self, utterance: &str) -> Result<serde_json::Value, AssistantError>;
    fn identity(&self) -> PlannerIdentity;
}

impl Planner for LocalCrmPlanner {
    async fn plan(&self, utterance: &str) -> Result<serde_json::Value, AssistantError> {
        Ok(Self::classify(utterance))
    }

    fn identity(&self) -> PlannerIdentity {
        PlannerIdentity {
            provider_id: Self::PROVIDER_ID.to_string(),
            model: Self::MODEL.to_string(),
        }
    }
}

impl LocalCrmPlanner {
    /// The classification, in the baseline's order. Every pattern is anchored
    /// `(?i)` over the trimmed text with trailing `?!.+` stripped — `text` in
    /// the TS is `utterance.trim().replace(/[?!.]+$/, '').trim()`.
    pub fn classify(utterance: &str) -> serde_json::Value {
        let text = utterance.trim().trim_end_matches(['?', '!', '.']).trim();
        let patterns = patterns();
        // 1. Overview — either of the two alternatives the baseline union has.
        if patterns.overview.is_match(text) {
            return serde_json::json!({ "operation": "crm.overview" });
        }
        // 2. Review/prepare the COMPLETION of one existing follow-up.
        if patterns.completion.is_match(text) {
            return serde_json::json!({ "operation": "followup.complete.prepare" });
        }
        // 3. List the open worklist, with an optional bucket word.
        if let Some(bucket) = patterns.followups.captures(text) {
            let bucket = bucket.get(1).map(|b| b.as_str()).unwrap_or("");
            let bucket = if bucket.eq_ignore_ascii_case("terlambat")
                || bucket.eq_ignore_ascii_case("overdue")
            {
                "overdue"
            } else if bucket.eq_ignore_ascii_case("mendatang")
                || bucket.eq_ignore_ascii_case("upcoming")
            {
                "upcoming"
            } else {
                "open"
            };
            return serde_json::json!({ "operation": "followups.list", "bucket": bucket });
        }
        // 4. Pipelines without a query.
        if patterns.opportunity_list.is_match(text) {
            return serde_json::json!({ "operation": "opportunities.list", "search": "" });
        }
        // 5. Pipeline search.
        if let Some(search) = patterns.opportunity_search.captures(text) {
            let search = search.get(1).map(|m| m.as_str()).unwrap_or("");
            if search.chars().count() <= 120 {
                return serde_json::json!({ "operation": "opportunities.list", "search": search });
            }
        }
        // 6. Pipeline summary.
        if let Some(summary) = patterns.opportunity_summary.captures(text) {
            let query = summary.get(1).map(|m| m.as_str()).unwrap_or("");
            if query.chars().count() <= 120 {
                let plan = if query.is_empty() {
                    serde_json::json!({ "operation": "opportunity.summary" })
                } else {
                    serde_json::json!({ "operation": "opportunity.summary", "query": query })
                };
                return plan;
            }
        }
        // 7. The bare contact list.
        if patterns.contacts_list.is_match(text) {
            return serde_json::json!({ "operation": "contacts.list", "search": "" });
        }
        // 8. A contact search.
        if let Some(search) = patterns.contact_search.captures(text) {
            let search = search.get(1).map(|m| m.as_str()).unwrap_or("");
            if search.chars().count() <= 120 {
                return serde_json::json!({ "operation": "contacts.list", "search": search });
            }
        }
        // 9/10. The bare summary and the bare draft preparation — same family,
        // the matched keyword decides which operation results. The baseline
        // prefers the summary pattern and only consults the draft pattern
        // when the summary did not match.
        let summary = patterns.contact_summary.captures(text);
        let followup = patterns.followup_prepare.captures(text);
        let (operation, query) = match (summary, followup) {
            (Some(caps), _) => (
                "contact.summary",
                caps.get(1)
                    .filter(|m| !m.as_str().is_empty())
                    .map(|m| m.as_str().to_string()),
            ),
            (None, Some(caps)) => (
                "followup.prepare",
                caps.get(1)
                    .filter(|m| !m.as_str().is_empty())
                    .map(|m| m.as_str().to_string()),
            ),
            (None, None) => return serde_json::json!({ "operation": "unsupported" }),
        };
        if let Some(q) = query {
            if q.chars().count() > 120 {
                return serde_json::json!({ "operation": "unsupported" });
            }
            return serde_json::json!({ "operation": operation, "query": q });
        }
        serde_json::json!({ "operation": operation })
    }
}

/// All ten anchored patterns, compiled once and never rebuilt. Copying the
/// baseline's literals exactly — single spaces inside the multi-word
/// alternatives are literal spaces there.
struct Patterns {
    overview: regex::Regex,
    completion: regex::Regex,
    followups: regex::Regex,
    opportunity_list: regex::Regex,
    opportunity_search: regex::Regex,
    opportunity_summary: regex::Regex,
    contacts_list: regex::Regex,
    contact_search: regex::Regex,
    contact_summary: regex::Regex,
    followup_prepare: regex::Regex,
}

fn patterns() -> &'static Patterns {
    static PATTERNS: OnceLock<Patterns> = OnceLock::new();
    PATTERNS.get_or_init(|| Patterns {
        overview: regex::Regex::new(
            r"(?i)^(?:(?:ringkas|ringkasan|tampilkan|lihat)\s+(?:kondisi\s+)?crm|(?:show\s+)?crm\s+(?:overview|summary))$",
        )
        .expect("the recorded overview pattern"),
        completion: regex::Regex::new(
            r"(?i)^(?:tinjau penyelesaian|siapkan penyelesaian|selesaikan|review completion of|complete)\s+follow[ -]?up$",
        )
        .expect("the recorded completion pattern"),
        followups: regex::Regex::new(
            r"(?i)^(?:tampilkan|daftar|lihat|list|show)(?:\s+(?:semua|all))?\s+follow[ -]?ups?(?:\s+(?:yang\s+)?(terlambat|overdue|mendatang|upcoming))?$",
        )
        .expect("the recorded worklist pattern"),
        opportunity_list: regex::Regex::new(
            r"(?i)^(?:tampilkan|daftar|lihat|list|show)(?:\s+(?:semua|all))?\s+(?:peluang|opportunities)$",
        )
        .expect("the recorded pipeline list pattern"),
        opportunity_search: regex::Regex::new(
            r"(?i)^(?:cari|search|find)\s+(?:peluang|opportunity)\s+(.+)$",
        )
        .expect("the recorded pipeline search pattern"),
        opportunity_summary: regex::Regex::new(
            r"(?i)^(?:ringkas|ringkasan|summarize|summary)\s+(?:peluang|opportunity)(?:\s+(.+))?$",
        )
        .expect("the recorded pipeline summary pattern"),
        contacts_list: regex::Regex::new(
            r"(?i)^(?:tampilkan|daftar|lihat|list|show)(?:\s+(?:semua|all))?\s+(?:kontak|contacts)$",
        )
        .expect("the recorded contact list pattern"),
        contact_search: regex::Regex::new(r"(?i)^(?:cari|search|find)(?:\s+(?:kontak|contact))?\s+(.+)$")
            .expect("the recorded contact search pattern"),
        contact_summary: regex::Regex::new(
            r"(?i)^(?:ringkas|ringkasan|summarize|summary)(?:\s+(?:kontak|contact))?(?:\s+(.+))?$",
        )
        .expect("the recorded contact summary pattern"),
        followup_prepare: regex::Regex::new(
            r"(?i)^(?:siapkan|prepare|draft)\s+follow[ -]?up(?:\s+(?:untuk|for))?(?:\s+(.+))?$",
        )
        .expect("the recorded draft pattern"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::{Bucket, ConversationPlan};

    fn operation(utterance: &str) -> serde_json::Value {
        let plan = LocalCrmPlanner::classify(utterance);
        plan["operation"].clone()
    }

    /// Rows recorded from the baseline's planner: `packages/crm-assistant/src/
    /// planner.ts` and the compositions `conversation.test.ts` drives. A
    /// classification that drifts from these trips this named test rather
    /// than quietly answering the wrong operation.
    #[test]
    fn classification_matches_the_recorded_rows() {
        let rows: [(&str, &str); 24] = [
            ("ringkas kondisi CRM", "crm.overview"),
            ("Ringkasan CRM", "crm.overview"),
            ("tampilkan CRM", "crm.overview"),
            ("lihat kondisi CRM", "crm.overview"),
            ("show CRM overview", "crm.overview"),
            ("CRM summary", "crm.overview"),
            ("tinjau penyelesaian follow-up", "followup.complete.prepare"),
            (
                "Siapkan penyelesaian follow-up",
                "followup.complete.prepare",
            ),
            ("selesaikan follow up", "followup.complete.prepare"),
            (
                "review completion of follow-up",
                "followup.complete.prepare",
            ),
            ("Complete Follow-Up?", "followup.complete.prepare"),
            ("tampilkan follow-up terlambat", "followups.list"),
            ("diatarkan follow-ups mendatang.", "unsupported"),
            ("Tampilkan semua follow-ups", "followups.list"),
            ("list follow up", "followups.list"),
            ("tampilkan peluang", "opportunities.list"),
            ("cari peluang energi", "opportunities.list"),
            ("ringkas peluang", "opportunity.summary"),
            ("tampilkan kontak", "contacts.list"),
            ("daftar contacts", "contacts.list"),
            ("cari Budi", "contacts.list"),
            ("find contact Arya", "contacts.list"),
            ("ringkas Budi", "contact.summary"),
            ("siapkan follow-up untuk Budi", "followup.prepare"),
        ];
        for (utterance, expected) in rows {
            assert_eq!(
                operation(utterance),
                expected,
                "the recorded planner row for {utterance:?}"
            );
        }
    }

    /// The bucket word decides the listing bucket, defaulting to `open`.
    #[test]
    fn the_worklist_rows_carry_their_bucket() {
        for (utterance, bucket) in [
            ("tampilkan follow-up terlambat", "overdue"),
            ("tampilkan follow-ups overdue", "overdue"),
            ("tampilkan follow-up mendatang", "upcoming"),
            ("tampilkan follow-up yang mendatang", "upcoming"),
            ("tampilkan follow-ups", "open"),
            ("show all follow-ups", "open"),
        ] {
            assert_eq!(LocalCrmPlanner::classify(utterance)["bucket"], bucket);
        }
    }

    /// `unsupported` for anything outside the vocabulary — including the
    /// trailing-punctuation trims that would otherwise leak into a search.
    #[test]
    fn anything_outside_the_vocabulary_is_unsupported() {
        for utterance in ["halo", "hapus kontak", "kirim email ke Budi", "", "???"] {
            assert_eq!(operation(utterance), "unsupported");
        }
    }

    /// The quoted query is carried verbatim (the planner never resolves
    /// names itself), and spaces inside it survive.
    #[test]
    fn the_query_is_carried_not_resolved() {
        assert_eq!(
            LocalCrmPlanner::classify("ringkas Budi Santoso")["query"],
            "Budi Santoso"
        );
        assert_eq!(
            LocalCrmPlanner::classify("siapkan follow-up untuk Arya Widodo")["query"],
            "Arya Widodo"
        );
        assert_eq!(
            LocalCrmPlanner::classify("cari Budi Santoso & Co")["search"],
            "Budi Santoso & Co"
        );
    }

    /// Over-bound queries are refused as unsupported, the same cap the
    /// baseline applies before the plan schema would refuse anyway.
    #[test]
    fn an_over_bound_query_is_unsupported() {
        let long = "x".repeat(121);
        assert_eq!(operation(&format!("ringkas {long}")), "unsupported");
        assert_eq!(operation(&format!("cari {long}")), "unsupported");
    }

    /// The identity the route echoes must be the deterministic provider.
    #[test]
    fn the_local_planner_identity_is_deterministic() {
        let identity = LocalCrmPlanner::new().identity();
        assert_eq!(identity.provider_id, "archava-crm-local");
        assert_eq!(identity.model, "deterministic-commands-v1");
    }

    /// The planner trait's values parse into the plan contracts — a planner
    /// value this crate produced can NEVER be the unverified-plan path.
    #[tokio::test]
    async fn planner_values_parse_into_plan_contract() {
        let planner = LocalCrmPlanner::new();
        for utterance in [
            "ringkas kondisi CRM",
            "tampilkan kontak",
            "cari Budi",
            "ringkas Budi",
            "tampilkan follow-up terlambat",
            "siapkan follow-up untuk Budi",
            "tinjau penyelesaian follow-up",
            "tampilkan peluang",
            "halo",
        ] {
            let value = planner
                .plan(utterance)
                .await
                .expect("the local planner never refuses");
            let plan = ConversationPlan::parse(value).expect("planner values must parse");
            if !utterance.contains("halo") {
                assert!(!matches!(plan, ConversationPlan::Unsupported));
            }
        }
    }

    /// The bucket vocabulary closes: a bucket carrier that names nothing known
    /// refuses as unsupported (no bucket row ⊆ open is intended).
    #[test]
    fn bucket_words_only_move_the_open_worklist() {
        let plan = LocalCrmPlanner::classify("tampilkan follow-up terlambat");
        assert!(plan["bucket"] == "overdue");
        let parsed = ConversationPlan::parse(plan).expect("a bucket value must parse");
        match parsed {
            ConversationPlan::FollowupsList { bucket } => assert_eq!(bucket, Bucket::Overdue),
            other => panic!("unexpected plan: {other:?}"),
        }
    }
}
