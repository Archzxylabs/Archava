//! Structured truth vs retrieval knowledge (§17) — port of
//! `packages/knowledge/src/structured-truth.ts`.
//!
//! The rule the PRD states as an absolute: "Do not answer these from stale
//! vector retrieval when a live system exists."
//!
//! So the split is not a preference the model may weigh. It is a classification
//! made *before* retrieval: a question about price, stock, availability,
//! booking status, an order, a payment, a shipment or an account must be
//! answered from a live authoritative source, and retrieval may only supply
//! supporting copy — never the answer itself.

use serde::{Deserialize, Serialize};

/// The eight subjects, in declaration order (the order a report keeps).
pub const STRUCTURED_TRUTH_SUBJECTS: &[StructuredTruthSubject] = &[
    StructuredTruthSubject::Price,
    StructuredTruthSubject::Stock,
    StructuredTruthSubject::Availability,
    StructuredTruthSubject::BookingStatus,
    StructuredTruthSubject::CustomerOrOrder,
    StructuredTruthSubject::PaymentStatus,
    StructuredTruthSubject::ShippingStatus,
    StructuredTruthSubject::AccountState,
];

use StructuredTruthSubject as Subject;

/// A live-value subject: the thing being asked about that a live system owns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuredTruthSubject {
    /// Prices, rates, fees, quotes.
    #[serde(rename = "price")]
    Price,
    /// Published stock and inventory counts.
    #[serde(rename = "stock")]
    Stock,
    /// Bookability, vacancy, free slots.
    #[serde(rename = "availability")]
    Availability,
    /// The visitor's own reservation state.
    #[serde(rename = "booking_status")]
    BookingStatus,
    /// The visitor's orders, profile and history.
    #[serde(rename = "customer_or_order")]
    CustomerOrOrder,
    /// Payments, invoices, refunds.
    #[serde(rename = "payment_status")]
    PaymentStatus,
    /// Shipments, deliveries, couriers.
    #[serde(rename = "shipping_status")]
    ShippingStatus,
    /// Subscription state, quotas, dunning.
    #[serde(rename = "account_state")]
    AccountState,
}

impl StructuredTruthSubject {
    pub fn as_str(self) -> &'static str {
        match self {
            Subject::Price => "price",
            Subject::Stock => "stock",
            Subject::Availability => "availability",
            Subject::BookingStatus => "booking_status",
            Subject::CustomerOrOrder => "customer_or_order",
            Subject::PaymentStatus => "payment_status",
            Subject::ShippingStatus => "shipping_status",
            Subject::AccountState => "account_state",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL_STR
            .iter()
            .position(|name| *name == value)
            .map(|index| STRUCTURED_TRUTH_SUBJECTS[index])
    }

    /// The stable strings, in declaration order.
    pub const ALL_STR: &'static [&'static str] = &[
        "price",
        "stock",
        "availability",
        "booking_status",
        "customer_or_order",
        "payment_status",
        "shipping_status",
        "account_state",
    ];
}

impl std::fmt::Display for StructuredTruthSubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Which source may answer: the live system, or the published corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeNeed {
    StructuredTruth,
    RetrievalKnowledge,
}

impl KnowledgeNeed {
    /// The stable strings the routing baseline carries as answers.
    pub fn as_str(self) -> &'static str {
        match self {
            KnowledgeNeed::StructuredTruth => "structured_truth",
            KnowledgeNeed::RetrievalKnowledge => "retrieval_knowledge",
        }
    }
}

/// What a question needs, decided before any retrieval happens.
///
/// `subjects` are present when the question touches structured truth; `matched`
/// names the signals that fired, for the audit trail.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct KnowledgeClassification {
    pub need: KnowledgeNeed,
    pub subjects: Vec<StructuredTruthSubject>,
    pub matched: Vec<String>,
}

/// The keyword triage table, verbatim from the baseline.
///
/// Deliberately lexical and deliberately generous: a false positive only means
/// the answer comes from the live system, which is safer than the reverse.
const SUBJECT_KEYWORDS: &[(Subject, &[&str])] = &[
    (
        Subject::Price,
        &[
            "price",
            "pricing",
            "cost",
            "costs",
            "fee",
            "fees",
            "rate",
            "rates",
            "charge",
            "charges",
            "discount",
            "quote",
            // A bare 'how much' is not here. It answers a price question only
            // when a copula follows it — "how much is the suite" — but it is
            // equally how a visitor asks a quantity that the tenant publishes:
            // "how much coffee is served at breakfast" is a menu question, and
            // a bare 'how much' routes it to the price port, which then says it
            // cannot answer. Each entry below is the question form, not the two
            // words.
            "how much is",
            "how much are",
            "how much does",
            "how much do",
            "how much for",
            "how much would",
            "biaya",
            "harga",
        ],
    ),
    // Inventory counts phrased without saying the word "stock". "How many
    // rooms do you have" is a stock question in every tenant's voice, and
    // without these it fell through to retrieval. Note what is deliberately
    // absent: a bare 'how many', which would also swallow "how many nights is
    // the stay" — an availability question answered from a stock table. Each
    // keyword names the thing being counted, so the count and the thing stay
    // together.
    (
        Subject::Stock,
        &[
            "stock",
            "inventory",
            "in stock",
            "out of stock",
            "available units",
            "stok",
            "how many rooms",
            "how many units",
            "how many properties",
            "how many villas",
            "rooms available",
            "units available",
            "rooms left",
            "units left",
        ],
    ),
    // Availability asked for with the word for the act, not the noun. `book`
    // on its own is deliberately not here: that is either a `booking_status`
    // question about the visitor's own reservation or a page action, and
    // neither is what this subject answers.
    (
        Subject::Availability,
        &[
            "availability",
            "available",
            "vacancy",
            "vacant",
            "open slot",
            "slots",
            "tersedia",
            "what can i book",
            "can i book",
            "can you book",
            "could you book",
            "bookable",
            "rooms free",
            "any rooms left",
        ],
    ),
    (
        Subject::BookingStatus,
        &[
            "booking status",
            "reservation status",
            "my booking",
            "is my booking",
            "confirmed booking",
            "booking confirmation",
            "pemesanan saya",
            "booking saya",
        ],
    ),
    (
        Subject::CustomerOrOrder,
        &[
            "my order",
            "order status",
            "order id",
            "track my order",
            "customer data",
            "my profile",
            "my history",
        ],
    ),
    (
        Subject::PaymentStatus,
        &[
            "payment status",
            "payment confirmation",
            "did my payment",
            "invoice status",
            "my invoice",
            "payment failed",
            "refund status",
            "dana pengembalian saya",
        ],
    ),
    (
        Subject::ShippingStatus,
        &[
            "shipping status",
            "delivery status",
            "track my shipment",
            "where is my parcel",
            "shipped",
            "courier",
        ],
    ),
    (
        Subject::AccountState,
        &[
            "account state",
            "my subscription",
            "my plan",
            "usage this month",
            "quota",
            "my contract",
            "dunning",
        ],
    ),
];

/// Anything that looks like a question about this tenant's own live data.
///
/// `/\b(my|our)\b/i` on the raw query — not on the keyword haystack, which has
/// already folded the punctuation a `\b` boundary would count.
fn possessive(query: &str) -> bool {
    let characters: Vec<char> = query.to_lowercase().chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric();
    for want in ["my", "our"] {
        let want: Vec<char> = want.chars().collect();
        for index in 0..characters.len() {
            if characters[index..].starts_with(&want[..]) {
                let before_ok = index == 0 || !is_word(characters[index - 1]);
                let end = index + want.len();
                let after_ok = end == characters.len() || !is_word(characters[end]);
                if before_ok && after_ok {
                    return true;
                }
            }
        }
    }
    false
}

/// The keyword haystack: lowercased, everything outside `[a-z0-9'’]` folded to
/// a space, runs of spaces collapsed, one space on each side.
fn haystack(query: &str) -> String {
    let mut folded = String::new();
    let mut last_was_space = true; // leading space, matching the template `' '+…+' '`
    for character in query.to_lowercase().chars() {
        if character.is_ascii_lowercase()
            || character.is_ascii_digit()
            || character == '\''
            || character == '’'
        {
            folded.push(character);
            last_was_space = false;
        } else if !last_was_space {
            folded.push(' ');
            last_was_space = true;
        }
    }
    let trimmed = folded.trim_end();
    format!(" {} ", trimmed)
}

/// One keyword as a word-bounded pattern test over the folded haystack.
///
/// The matching is `(^|[^a-z0-9])KEYWORD([^a-z0-9]|$)`. A keyword is a claim
/// about a *word* the visitor used, so it is matched as one — `fee` does not
/// match inside "coffee" and `book` does not match inside "bookable".
///
/// Keywords are all ASCII `[a-z0-9 ]`, so the word boundaries are decided on
/// bytes; the folded haystack keeps any non-`[a-z0-9]` character (including
/// multi-byte ones) as a boundary because those bytes are never
/// `[a-z0-9]`.
fn keyword_fires(folded: &str, keyword: &str) -> bool {
    let hay = folded.as_bytes();
    let needle = keyword.as_bytes();
    let is_word = |byte: u8| -> bool { byte.is_ascii_lowercase() || byte.is_ascii_digit() };
    let mut cursor = 0usize;
    while cursor + needle.len() <= hay.len() {
        let Some(found) = hay[cursor..]
            .windows(needle.len())
            .position(|w| w == needle)
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

/// Classify what a visitor question needs before any retrieval happens.
pub fn classify_knowledge_need(query: &str) -> KnowledgeClassification {
    let folded = haystack(query);
    let mut matched: Vec<Subject> = Vec::new();

    for (subject, keywords) in SUBJECT_KEYWORDS {
        if keywords
            .iter()
            .any(|keyword| keyword_fires(&folded, keyword))
        {
            matched.push(*subject);
        }
    }

    if !matched.is_empty() {
        return KnowledgeClassification {
            need: KnowledgeNeed::StructuredTruth,
            subjects: matched.clone(),
            matched: matched
                .iter()
                .map(|subject| subject.as_str().to_string())
                .collect(),
        };
    }

    // A possessive question ("where is my order?") is about live data even when
    // the noun is not on the keyword list.
    if possessive(query) {
        return KnowledgeClassification {
            need: KnowledgeNeed::StructuredTruth,
            subjects: vec![Subject::CustomerOrOrder],
            matched: vec!["possessive".to_string()],
        };
    }

    KnowledgeClassification {
        need: KnowledgeNeed::RetrievalKnowledge,
        subjects: Vec::new(),
        matched: Vec::new(),
    }
}

/// True when the answer must come from a live system, never from retrieval.
pub fn requires_structured_truth(query: &str) -> bool {
    classify_knowledge_need(query).need == KnowledgeNeed::StructuredTruth
}

/// How an answer was sourced, for evals and debugging (§17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerBasis {
    StructuredTruth,
    Retrieval,
}

/// The provenance that travels with an answer. Keeping it on the answer — not
/// only on the chunks — is what makes "which source produced this line?"
/// answerable after the fact.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AttributedAnswer<T> {
    pub value: T,
    pub basis: AnswerBasis,
    pub citations: Vec<String>,
}

pub fn attribute<T>(value: T, basis: AnswerBasis, citations: &[&str]) -> AttributedAnswer<T> {
    AttributedAnswer {
        value,
        basis,
        citations: citations
            .iter()
            .map(|citation| citation.to_string())
            .collect(),
    }
}
