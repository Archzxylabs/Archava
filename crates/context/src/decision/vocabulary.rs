//! Cues for the deterministic provider: English, Indonesian, and the mixture.
//! Port of `packages/decision/src/vocabulary.ts`.
//!
//! The reference tenant publishes Indonesian copy and takes questions in both
//! languages — and in practice a visitor writes "berapa harga untuk deluxe" in
//! one message and "can you book it" in the next. A cue set that only knows
//! English does not classify the tenant it ships with, and one that only knows
//! Indonesian fails the mixed case the corpus is full of.
//!
//! What is deliberately *not* here is Foundation's own keyword table. This
//! provider is a stand-in for a live model, not a second implementation of
//! `classifyKnowledgeNeed`: it answers with a confidence derived from how many
//! cues fired, and it is allowed to be cruder. Duplicating the structured-truth
//! keywords would make the CI baseline too good — it would agree with
//! production almost always, and the disagreement rate is the signal the eval
//! corpus measures.
//!
//! Matching is on normalized word segments, so `harga` does not fire inside
//! `hargai`, and a cue is counted once per task however often it appears.
//! Nothing here reads a tenant's schema, a store, a clock, or a network.

use super::tasks::DecisionTask;

/// Cues for one candidate answer of a task.
#[derive(Debug, Clone, PartialEq)]
pub struct CueSet {
    /// Words or short phrases that vote for this answer.
    pub toward: &'static [&'static str],
    /// Words or short phrases that vote against it, so a mixed sentence can
    /// lean.
    pub away: &'static [&'static str],
}

/// One candidate answer inside a task's row: the label and its cues.
pub struct CueRow {
    pub label: &'static str,
    pub set: CueSet,
}

/// Cues per task, keyed by task id.
///
/// `missing` is the point: a task the catalogue gains and this table does not
/// is a task the baseline refuses rather than guesses at (`cues_for` returns
/// nothing, and the provider reports a refusal with that reason). The catalogue
/// test asserts the same thing at run time.
///
/// Transcribed row for row from the baseline; the parity tests compare the
/// whole structure against a frozen expectation table, so a changed cue trips a
/// named witness.
pub fn cues_for(task: DecisionTask) -> Option<&'static [CueRow]> {
    match task {
        DecisionTask::IntentClassification => Some(INTENT_CLASSIFICATION_CUES),
        DecisionTask::KnowledgeRouting => Some(KNOWLEDGE_ROUTING_CUES),
        DecisionTask::Clarification => Some(CLARIFICATION_CUES),
        DecisionTask::HandoffRecommendation => Some(HANDOFF_RECOMMENDATION_CUES),
        // No entry for `evidence_sufficiency`: the score is measured from what
        // the request carried, and a word that predicted it would be the same
        // lie a language model would tell. See `score_coverage` in the rule
        // provider.
        DecisionTask::EvidenceSufficiency => None,
    }
}

const INTENT_CLASSIFICATION_CUES: &[CueRow] = &[
    CueRow {
        label: "general_inquiry",
        set: CueSet {
            toward: &[
                "breakfast",
                "pool",
                "wifi",
                "gym",
                "parking",
                "spa",
                "facility",
                "facilities",
                "sarapan",
                "kolam",
                "parkir",
                "fasilitas",
                "lobby",
                "room",
                "kamar",
                "suite",
                "bed",
                "kasur",
                "towel",
                "handuk",
                "location",
                "near",
                "lokasi",
                "dekat",
                "restoran",
                "restaurant",
            ],
            away: &[
                "price", "biaya", "harga", "book", "pesan", "cancel", "batal", "pay", "bayar",
            ],
        },
    },
    CueRow {
        label: "policy_inquiry",
        set: CueSet {
            toward: &[
                "policy",
                "cancellation",
                "refund",
                "check in",
                "check out",
                "rules",
                "allowed",
                "kebijakan",
                "pembatalan",
                "aturan",
                "boleh",
                "late",
                "terlambat",
                "deposit",
            ],
            away: &["price", "biaya", "harga", "book", "pesan", "room", "kamar"],
        },
    },
    CueRow {
        label: "availability_inquiry",
        set: CueSet {
            toward: &[
                "book",
                "booking",
                "reserve",
                "reservation",
                "availability",
                "available",
                "pesan",
                "pemesanan",
                "reservasi",
                "tersedia",
                "kosong",
                "kamar kosong",
                "i want",
                "saya ingin",
                "pengen",
            ],
            away: &[
                "price",
                "biaya",
                "harga",
                "pool",
                "kolam",
                "breakfast",
                "sarapan",
            ],
        },
    },
    CueRow {
        label: "support_request",
        set: CueSet {
            toward: &[
                "help",
                "problem",
                "issue",
                "broken",
                "wrong",
                "complaint",
                "staff",
                "human",
                "agent",
                "manager",
                "bantuan",
                "masalah",
                "rusak",
                "komplain",
                "keluhan",
                "staf",
                "manajer",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "pricing_inquiry",
        set: CueSet {
            toward: &[
                "price",
                "pricing",
                "cost",
                "rate",
                "fee",
                "harga",
                "biaya",
                "tarif",
                "berapa harga",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "status_inquiry",
        set: CueSet {
            toward: &[
                "my booking",
                "my order",
                "payment status",
                "booking status",
                "status pesanan",
                "status pembayaran",
                "pesanan saya",
                "reservasi saya",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "recommendation_request",
        set: CueSet {
            toward: &[
                "recommend",
                "suggest",
                "best for",
                "which room suits",
                "rekomendasi",
                "sarankan",
                "yang cocok",
                "pilihkan",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "comparison_request",
        set: CueSet {
            toward: &[
                "compare",
                "comparison",
                "versus",
                "difference",
                "bandingkan",
                "perbedaan",
                "lebih baik",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "handoff_request",
        set: CueSet {
            toward: &[
                "human",
                "person",
                "staff",
                "agent",
                "manager",
                "manusia",
                "staf",
                "manajer",
                "orang asli",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "purchase_request",
        set: CueSet {
            toward: &[
                "purchase",
                "buy",
                "checkout",
                "pay now",
                "beli",
                "bayar sekarang",
                "lanjut pembayaran",
            ],
            away: &[],
        },
    },
];

const KNOWLEDGE_ROUTING_CUES: &[CueRow] = &[
    CueRow {
        label: "structured_truth",
        set: CueSet {
            toward: &[
                "price",
                "pricing",
                "cost",
                "fee",
                "rate",
                "charge",
                "quote",
                "biaya",
                "harga",
                "berapa",
                "how much is",
                "how much are",
                "stock",
                "available",
                "availability",
                "tersedia",
                "ketersediaan",
                "kosong",
                "sisa kamar",
                "payment",
                "paid",
                "invoice",
                "pembayaran",
                "tagihan",
                "shipping",
                "kirim",
                "pengiriman",
                "status pesanan",
                "pesanan saya",
                "my booking",
                "status pembatalan",
            ],
            away: &[],
        },
    },
    CueRow {
        label: "retrieval_knowledge",
        set: CueSet {
            toward: &[
                "policy",
                "facility",
                "facilities",
                "breakfast",
                "explain",
                "tell me about",
                "included",
                "service",
                "rules",
                "hours",
                "kebijakan",
                "fasilitas",
                "sarapan",
                "jelaskan",
                "tentang",
                "apa itu",
                "aturan",
                "layanan",
            ],
            away: &[],
        },
    },
];

const CLARIFICATION_CUES: &[CueRow] = &[CueRow {
    label: "true",
    set: CueSet {
        toward: &[
            "or",
            "maybe",
            "not sure",
            "either",
            "which one",
            "both",
            "atau",
            "mungkin",
            "mana",
            "salah satu",
            "yang mana",
        ],
        away: &[],
    },
}];

const HANDOFF_RECOMMENDATION_CUES: &[CueRow] = &[CueRow {
    label: "true",
    set: CueSet {
        toward: &[
            "speak to",
            "talk to",
            "real person",
            "human",
            "agent",
            "manager",
            "staff",
            "complaint",
            "complain",
            "frustrated",
            "angry",
            "unacceptable",
            "third time",
            "bicarakan",
            "ngomong",
            "orang",
            "manusia",
            "staf",
            "manajer",
            "berkali",
            "marah",
            "nggak bisa",
            "tidak bisa",
            "komplain",
            "keluhan",
        ],
        away: &[],
    },
}];

/// A cue occupying whole word boundaries.
///
/// The obvious alternative — substring search — makes `pool` fire inside
/// `swimming` and `book` inside `facebook`, which for this package is a booking
/// intent invented from a facility question. The cost of the check is two
/// comparisons.
pub fn mentions(lowercase_utterance: &str, cue: &str) -> bool {
    // The baseline's definition: `indexOf`, then the characters before and
    // after must be undefined or non-`[a-z0-9]` — and every occurrence is
    // considered until one lands on both edges.
    let hay = lowercase_utterance.as_bytes();
    let needle = cue.as_bytes();
    let is_word = |byte: u8| -> bool { byte.is_ascii_lowercase() || byte.is_ascii_digit() };
    let mut cursor = 0usize;
    while cursor + needle.len() <= hay.len() {
        let Some(found) = hay[cursor..]
            .windows(needle.len())
            .position(|window| window == needle)
        else {
            return false;
        };
        let start = cursor + found;
        let end = start + needle.len();
        let before_ok = start == 0 || !is_word(hay[start - 1]);
        let after_ok = end == hay.len() || !is_word(hay[end]);
        if before_ok && after_ok {
            return true;
        }
        cursor = start + 1;
    }
    false
}

/// An utterance normalized: lowercased, punctuation folded to spaces.
///
/// Normalizing once, here, means the cue table is written in the language it
/// appears in — "berapa harga", not "berapa,   harga" — and the same folding is
/// available to a phrase cue so both sides of a match agree.
pub fn normalize_utterance(utterance: &str) -> String {
    fold_text(utterance)
}

/// The normalized form of a cue, for a multi-word phrase.
pub fn normalize_cue(cue: &str) -> String {
    fold_text(cue)
}

/// `[^\p{L}\p{N}]+` folded to spaces, lowercased, trimmed — the baseline's
/// normalizer. `\p{L}` is any Unicode letter and `\p{N}` any number character;
/// `char::is_alphabetic` covers the letters and `char::is_numeric` the numeric
/// property classes.
fn fold_text(text: &str) -> String {
    let mut folded = String::with_capacity(text.len());
    let mut in_gap = true; // leading gap trimmed by construction
    for character in text.to_lowercase().chars() {
        if character.is_alphabetic() || character.is_numeric() {
            if in_gap && !folded.is_empty() {
                folded.push(' ');
            }
            in_gap = false;
            folded.push(character);
        } else {
            in_gap = true;
        }
    }
    folded.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_fires_at_word_edges_only() {
        assert!(mentions("the pool is heated", "pool"));
        assert!(!mentions("swimming pool rules", "ok pool"));

        // `pool` inside `swimming` does not fire: `swimming` ends without the
        // boundary (`pool ool` noise not present) — the real pairs from the
        // baseline note:
        assert!(!mentions("swimming", "pool"));
        assert!(!mentions("facebook", "book"));
        assert!(
            mentions("facebook book page", "book"),
            "the second occurrence is word-bounded"
        );
    }

    #[test]
    fn normalization_folds_punctuation_to_single_spaces() {
        assert_eq!(normalize_utterance("berapa,  harga??!"), "berapa harga");
        assert_eq!(normalize_cue("berapa  harga"), "berapa harga");
        // Letters of any script survive; digits survive.
        assert_eq!(normalize_utterance("Café 2"), "café 2");
    }

    #[test]
    fn every_task_with_cues_has_nonempty_toward_lists() {
        for task in [
            DecisionTask::IntentClassification,
            DecisionTask::KnowledgeRouting,
            DecisionTask::Clarification,
            DecisionTask::HandoffRecommendation,
        ] {
            let rows = cues_for(task).expect("these four have cue tables");
            assert!(!rows.is_empty());
            for row in rows {
                assert!(
                    !row.set.toward.is_empty(),
                    "{task} row {} lost its cues",
                    row.label
                );
            }
        }
        assert!(cues_for(DecisionTask::EvidenceSufficiency).is_none());
    }
}
