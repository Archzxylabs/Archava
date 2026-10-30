//! The turn seam between the assistant pipeline and the decision layer. Port
//! of `packages/assistant/src/decision.ts`.
//!
//! The decision layer is an add-on: a turn works completely without it. When it
//! is present, what it gets to read is deliberately small — the §16 projection,
//! a redacted utterance, and the first three chunks of published retrieval
//! evidence with their text truncated — because a model's input is a leak
//! surface, and every fact it receives is a fact the vendor logged. The
//! post-processing is conservative in the same direction: a provider can route
//! a retrieval turn to a structured-truth subject (strictly safer: a named
//! subject beats guessing from published copy), can raise the clarify and
//! handoff flags (asking again costs a round trip, dropping a flag loses the
//! turn), and can withhold an unsupported retrieval answer — and it can do
//! nothing else. It can never lower a flag, never invent a subject that no
//! trusted code supplied, and never override a source decision downward.
//!
//! Port difference: the source's `evaluateDecisionTurn` takes the orchestrator
//! inside its input object; this port passes it as an explicit argument,
//! because a trait-generic reference is not data the turn carries. Semantics
//! are unchanged: an absent orchestrator, or one whose run errors, is the
//! same no-decision result as the source's — a Foundation turn with nothing
//! changed and no flags.

use serde_json::{json, Value};

use super::orchestrator::{
    AnswerSource, DecisionProvider, DecisionRun, DecisionTurn, DefaultOrchestrator,
};
use super::tasks::{BaselineAnswer, DecisionTask};
use crate::knowledge::retrieval::KnowledgeContextChunk;
use crate::knowledge::structured_truth::{
    KnowledgeClassification, KnowledgeNeed, StructuredTruthSubject,
};

/// Redact common secrets and personal contact values before external
/// evaluation.
///
/// Four passes, in the source's order — ownership first, then money-adjacent
/// numbers, then credentials, then phones — each a scanner with the same
/// verdicts the regex would give. The patterns are pure ASCII, so scanning
/// `char`s matches the source's UTF-16 semantics exactly: every non-ASCII
/// character is non-word to a `\b` on both sides, and none of the classes can
/// match it, so there is no half-surrogate seam to get wrong.
pub fn decision_utterance(utterance: &str) -> String {
    let after_email = redact_emails(utterance);
    let after_card = redact_number_runs(&after_email);
    let after_credential = redact_credentials(&after_card);
    redact_phone_runs(&after_credential)
}

/// Pass 1: `\b[A-Z0-9._%+-]+@[A-Z0-9.-]+\.[A-Z]{2,}\b` /gi → `[email]`.
fn redact_emails(text: &str) -> String {
    redact_spans(text, EMAIL_LABEL, |chars, start| {
        let local = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-');
        let domain = |c: char| c.is_ascii_alphanumeric() || c == '.' || c == '-';
        let letter = |c: char| c.is_ascii_alphabetic();
        if !local(chars[start]) || !boundary_at(chars, start) {
            return None;
        }
        // The local part: a run of address characters ending exactly at `@`.
        let mut at = start;
        while at < chars.len() && local(chars[at]) {
            at += 1;
        }
        if at == start || at >= chars.len() || chars[at] != '@' {
            return None;
        }
        // The domain: a run the engine split greedily, from the right, into a
        // prefix, the final dot, and a TLD of at least two letters.
        let domain_start = at + 1;
        let mut domain_end = domain_start;
        while domain_end < chars.len() && domain(chars[domain_end]) {
            domain_end += 1;
        }
        if domain_end == domain_start {
            return None;
        }
        for dot in (domain_start + 1..domain_end).rev() {
            if chars[dot] != '.' {
                continue;
            }
            let mut tld_end = dot + 1;
            while tld_end < domain_end && letter(chars[tld_end]) {
                tld_end += 1;
            }
            if tld_end - (dot + 1) < 2 {
                continue;
            }
            if tld_end < chars.len() && is_word(chars[tld_end]) {
                continue;
            }
            return Some(tld_end);
        }
        None
    })
}

/// Pass 2: `\b(?:\d[ -]*?){13,19}\b` /g → `[number]`.
///
/// The span of a match always ends exactly at a digit: a `\b` after a digit
/// holds unless the next character is word-shaped, and a separator (` ` or
/// `-`) is not word-shaped, so the engine's lazy separator can never be the
/// escape hatch. The match therefore reaches back from the last digit whose
/// boundary holds, down to the 13th.
fn redact_number_runs(text: &str) -> String {
    redact_spans(text, NUMBER_LABEL, |chars, start| {
        let digit = |c: char| c.is_ascii_digit();
        if !digit(chars[start]) || !boundary_at(chars, start) {
            return None;
        }
        // Greedy extension: consume digit groups (a digit, then the separator
        // run it takes to reach the next digit), capped at 19.
        let mut digit_ends: Vec<usize> = Vec::new();
        let mut pos = start;
        while digit_ends.len() < 19 && pos < chars.len() && digit(chars[pos]) {
            pos += 1;
            digit_ends.push(pos);
            while pos < chars.len() && (chars[pos] == ' ' || chars[pos] == '-') {
                pos += 1;
            }
        }
        if digit_ends.len() < 13 {
            return None;
        }
        // Backtrack, from the largest available group count.
        digit_ends
            .iter()
            .rev()
            .copied()
            .find(|&end| end == chars.len() || !is_word(chars[end]))
    })
}

/// Pass 3: `\b(?:sk|pk|tok)_[A-Za-z0-9_-]{8,}\b` /g → `[credential]`.
///
/// The answer body is greedy and can end on any of its characters — but only
/// on a word-shaped one, because a trailing `-` is not `\w` and the `\b` after
/// it cannot hold. So the match ends at the last word char that keeps eight
/// characters of answer body intact.
fn redact_credentials(text: &str) -> String {
    redact_spans(text, CREDENTIAL_LABEL, |chars, start| {
        if !boundary_at(chars, start) {
            return None;
        }
        let prefix_len = ["sk_", "pk_", "tok_"].iter().find_map(|prefix| {
            let want: Vec<char> = prefix.chars().collect();
            chars[start..].starts_with(&want[..]).then_some(want.len())
        });
        let prefix_len = prefix_len?;
        let body_start = start + prefix_len;
        let mut body_end = body_start;
        while body_end < chars.len()
            && (chars[body_end].is_ascii_alphanumeric()
                || chars[body_end] == '_'
                || chars[body_end] == '-')
        {
            body_end += 1;
        }
        if body_end - body_start < 8 {
            return None;
        }
        for end in (body_start + 8..=body_end).rev() {
            let before_w = is_word(chars[end - 1]);
            let at_w = end < chars.len() && is_word(chars[end]);
            if before_w != at_w {
                return Some(end);
            }
        }
        None
    })
}

/// Pass 4: `(?:\+?\d[ -]?){10,15}` /g → `[number]`.
///
/// No word boundaries at all — a phone number may sit inside a larger run of
/// digits, and every substring count of it is a fact about the visitor. Each
/// group is one optional `+`, one digit, and at most one separator; the match
/// spans the greediest ten-to-fifteen groups available and ends with the last
/// digit, its separator retracted when no further group follows.
fn redact_phone_runs(text: &str) -> String {
    redact_spans(text, NUMBER_LABEL, |chars, start| {
        let digit = |c: char| c.is_ascii_digit();
        let mut count = 0usize;
        let mut last_digit_end = None;
        let mut pos = start;
        while count < 15 && pos < chars.len() {
            let mut group_end = pos;
            if chars[group_end] == '+' {
                group_end += 1;
                if group_end >= chars.len() || !digit(chars[group_end]) {
                    break;
                }
            } else if !digit(chars[group_end]) {
                break;
            }
            group_end += 1;
            count += 1;
            last_digit_end = Some(group_end);
            if group_end < chars.len() && (chars[group_end] == ' ' || chars[group_end] == '-') {
                group_end += 1;
            }
            pos = group_end;
        }
        match last_digit_end {
            Some(end) if count >= 10 => Some(end),
            _ => None,
        }
    })
}

/// One pass: replace every match the scanner returns, left to right, and
/// resume scanning after each replacement — the source's global-regex
/// `lastIndex` semantics.
fn redact_spans(
    text: &str,
    replacement: &str,
    scanner: impl Fn(&[char], usize) -> Option<usize>,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut copied = 0usize;
    let mut at = 0usize;
    while at < chars.len() {
        if let Some(end) = scanner(&chars, at) {
            out.extend(chars[copied..at].iter());
            out.push_str(replacement);
            copied = end;
            at = end;
            continue;
        }
        at += 1;
    }
    out.extend(chars[copied..].iter());
    out
}

/// Word-shaped, as `\b` means it in the source: letters, digits, `_`.
fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether a word boundary sits at position `at`, exactly one side word-shaped.
fn boundary_at(chars: &[char], at: usize) -> bool {
    let before = at > 0 && is_word(chars[at - 1]);
    let here = at < chars.len() && is_word(chars[at]);
    before != here
}

/// Pass labels, verbatim in the source.
const EMAIL_LABEL: &str = "[email]";
const NUMBER_LABEL: &str = "[number]";
const CREDENTIAL_LABEL: &str = "[credential]";

/// A maximal whole-character prefix whose UTF-16 length fits `limit` units.
///
/// The source slices by UTF-16 code units (`text.slice(0, 800)`), which can
/// split a surrogate pair; this port clips on whole characters instead — the
/// same bounded-exposure rule, never shorter than promised by count, and never
/// a broken scalar in the vendor's log.
fn text_prefix(text: &str, limit: usize) -> &str {
    let mut units = 0usize;
    let mut end = 0usize;
    for (index, c) in text.char_indices() {
        let width = c.len_utf16();
        if units + width > limit {
            break;
        }
        units += width;
        end = index + c.len_utf8();
    }
    &text[..end]
}

/// One batched evaluation. The only runtime effects are conservative: protected
/// truth cannot be downgraded, a missing truth subject cannot be invented, and
/// evidence scores can withhold an unsupported retrieval answer but not create
/// one.
///
/// The input mirrors the source's `DecisionTurnInput`; the orchestrator is an
/// explicit argument rather than an input field (see the module header), and
/// absent is legal.
#[derive(Debug, Clone)]
pub struct DecisionTurnInput {
    pub tenant_id: String,
    pub utterance: String,
    pub locale: String,
    /// The §16 projection of the turn's context — no form values, no
    /// customer state.
    pub context: Value,
    pub classification: KnowledgeClassification,
    /// Published retrieval chunks already gathered for this turn. Only their
    /// source ids and truncated text are exposed, and only the first three.
    pub grounding: Vec<KnowledgeContextChunk>,
    pub handoff_requested: bool,
    /// Subjects supplied by trusted tenant code, never invented by a model.
    pub structured_truth_candidates: Vec<StructuredTruthSubject>,
}

/// What one turn's decision pass decided, alongside what Foundation already
/// said — every field a caller must read to stitch the two together.
#[derive(Debug, Clone, serde::Serialize)]
pub struct DecisionTurnResult {
    /// `None` when no orchestrator was configured, or when its run errored —
    /// a decision integration must not interrupt the Foundation turn.
    pub run: Option<DecisionRun>,
    pub classification: KnowledgeClassification,
    pub clarify: bool,
    pub recommend_handoff: bool,
    pub evidence_insufficient: bool,
    pub intent: Option<String>,
    pub runtime_changed: bool,
}

/// The source's no-op result: the turn as classification handed it in, with
/// every provider-shaped field absent.
fn unmeasured(input: DecisionTurnInput) -> DecisionTurnResult {
    DecisionTurnResult {
        run: None,
        classification: input.classification,
        clarify: false,
        recommend_handoff: false,
        evidence_insufficient: false,
        intent: None,
        runtime_changed: false,
    }
}

/// The Foundation baseline for `intent_classification`: a posture read from
/// the classification's subjects, in the order the posture ladder was written.
fn baseline_intent(classification: &KnowledgeClassification) -> BaselineAnswer {
    let subjects: Vec<&str> = classification
        .subjects
        .iter()
        .map(|subject| subject.as_str())
        .collect();
    let answer = if subjects.contains(&"price") {
        "pricing_inquiry"
    } else if subjects.contains(&"availability") || subjects.contains(&"stock") {
        "availability_inquiry"
    } else if subjects
        .iter()
        .any(|subject| subject.ends_with("_status") || *subject == "customer_or_order")
    {
        "status_inquiry"
    } else {
        "general_inquiry"
    };
    BaselineAnswer {
        task: DecisionTask::IntentClassification,
        answer: Value::String(answer.to_string()),
        source: "foundation_classification".to_string(),
    }
}

/// Whether a run's effective answer for one task is the provider's `true`.
/// (`answer === true` in the source: a boolean and nothing else.)
fn provider_said_true(run: &DecisionRun, task: DecisionTask) -> bool {
    run.decisions.iter().any(|decision| {
        decision.task == task
            && decision.source == AnswerSource::Provider
            && decision.answer == json!(true)
    })
}

pub fn evaluate_decision_turn<P: DecisionProvider>(
    input: DecisionTurnInput,
    orchestrator: Option<&DefaultOrchestrator<P>>,
) -> DecisionTurnResult {
    let Some(orchestrator) = orchestrator else {
        return unmeasured(input);
    };

    let baselines = vec![
        baseline_intent(&input.classification),
        BaselineAnswer {
            task: DecisionTask::KnowledgeRouting,
            answer: Value::String(input.classification.need.as_str().to_string()),
            source: "classifyKnowledgeNeed".to_string(),
        },
        BaselineAnswer {
            task: DecisionTask::Clarification,
            answer: json!(false),
            source: "foundation_turn".to_string(),
        },
        BaselineAnswer {
            task: DecisionTask::HandoffRecommendation,
            answer: Value::Bool(input.handoff_requested),
            source: "handoffRequested".to_string(),
        },
        BaselineAnswer {
            task: DecisionTask::EvidenceSufficiency,
            answer: json!(if input.grounding.is_empty() { 0 } else { 1 }),
            source: "retrieval_presence".to_string(),
        },
    ];
    let turn = DecisionTurn {
        tenant_id: input.tenant_id.clone(),
        utterance: decision_utterance(&input.utterance),
        locale: input.locale.clone(),
        evidence: decision_evidence(&input.context, &input.grounding),
    };
    let Ok(run) = orchestrator.run(&turn, &baselines) else {
        return unmeasured(input);
    };

    let route = run
        .decisions
        .iter()
        .find(|decision| decision.task == DecisionTask::KnowledgeRouting);
    let may_escalate = input.classification.need == KnowledgeNeed::RetrievalKnowledge
        && route.is_some_and(|route| route.source == AnswerSource::Provider)
        && route.is_some_and(|route| route.answer.as_str() == Some("structured_truth"))
        && !input.structured_truth_candidates.is_empty();

    let classification = if may_escalate {
        // An escalation replaces the whole classification: the need sharpens,
        // the subjects become exactly what trusted code supplied — deduped by
        // first occurrence, the source's Set semantics — and the audit line
        // names the seam that changed it. The old `matched` signals describe
        // the turn the provider saw, not the turn it changed; they do not
        // survive the replacement.
        let mut subjects: Vec<StructuredTruthSubject> = Vec::new();
        for subject in &input.structured_truth_candidates {
            if !subjects.contains(subject) {
                subjects.push(*subject);
            }
        }
        KnowledgeClassification {
            need: KnowledgeNeed::StructuredTruth,
            subjects,
            matched: vec!["decision_escalation".to_string()],
        }
    } else {
        input.classification.clone()
    };

    let clarify = provider_said_true(&run, DecisionTask::Clarification);
    let recommend_handoff = provider_said_true(&run, DecisionTask::HandoffRecommendation);
    let evidence_insufficient = classification.need == KnowledgeNeed::RetrievalKnowledge
        && !input.grounding.is_empty()
        && run.decisions.iter().any(|decision| {
            decision.task == DecisionTask::EvidenceSufficiency
                && decision.source == AnswerSource::Provider
                && decision
                    .answer
                    .as_f64()
                    .is_some_and(|answer| answer.is_finite() && answer < 0.5)
        });
    // Any source counts: a provider answer and a baseline answer are both
    // "the posture", and the caller reads the run for which was which.
    let intent = run
        .decisions
        .iter()
        .find(|decision| decision.task == DecisionTask::IntentClassification)
        .and_then(|decision| decision.answer.as_str().map(str::to_string));
    let runtime_changed = may_escalate || clarify || recommend_handoff || evidence_insufficient;

    DecisionTurnResult {
        run: Some(run),
        classification,
        clarify,
        recommend_handoff,
        evidence_insufficient,
        intent,
        runtime_changed,
    }
}

/// The decision layer receives only projected context and limited published
/// evidence: page shape and locale, an entity count, and up to three chunks of
/// published retrieval content with source ids and redacted, bounded text.
///
/// No form values and no customer state reach a vendor through this seam. The
/// count is also deliberately a count: how many subjects the page carried is
/// a shape the vendor may weigh; the subjects themselves are not.
pub fn decision_evidence(context: &Value, grounding: &[KnowledgeContextChunk]) -> Value {
    json!({
        "page": {
            "kind": context.get("pageKind"),
            "locale": context.get("locale"),
            "entityCount": context
                .get("entities")
                .and_then(Value::as_array)
                .map_or(0, Vec::len),
        },
        "publishedEvidence": grounding
            .iter()
            .take(3)
            .map(|chunk| {
                json!({
                    "sourceId": chunk.source_id,
                    "text": decision_utterance(text_prefix(&chunk.text, 800)),
                })
            })
            .collect::<Vec<Value>>(),
    })
}
