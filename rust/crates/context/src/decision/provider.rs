//! The deterministic provider: the CI and evaluation baseline. Port of
//! `packages/decision/src/rule-provider.ts`.
//!
//! It answers the same questions a live model would, from the same request,
//! with no network, no credentials and no clock — which is what makes a
//! reproducible evaluation possible. It is also not a model, and it does not
//! pretend to be: where a model reasons about a sentence, this counts cues and
//! reports how many fired. The confidence describes the counting; it is not a
//! claim about the world.
//!
//! Three rules keep it honest enough to be a baseline rather than a competitor.
//!
//! 1. It answers only what it was asked, by task id. A question it has no cue
//!    table for is *refused*, not guessed — the port's `refusals` list exists
//!    precisely so a provider can decline.
//! 2. Its confidence comes from agreement, not from assertion. One cue is worth
//!    less than three, and no number of cues is worth 1.0: a heuristic cannot
//!    be certain of anything, and reporting certainty would make every floor
//!    the catalogue sets meaningless.
//! 3. It never leaves the shape it was handed. The reply carries the identity
//!    it was built with, one entry per asked question, and nothing else.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use serde_json::Value::Object;

use super::orchestrator::request_problem;
use super::tasks::{
    definition_for, task_answer_problem, Decision, DecisionQuestion, DecisionRefusal,
    DecisionRequest, DecisionResult, DecisionTask,
};
use super::vocabulary::{cues_for, mentions, normalize_cue, normalize_utterance};

/// What this provider answers, and how much the answer is worth.
///
/// A refusal is a different fact from a weak answer — the first is a refusal
/// the caller can read, and collapsing the two is how a zero-confidence default
/// gets applied as though a provider had chosen it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RuleReport {
    #[serde(rename_all = "snake_case")]
    Answered {
        task: DecisionTask,
        answer: Value,
        /// Bounded 0..1, and never 1. See rule 2 above.
        confidence: f64,
        /// How the answer was reached, for a test that wants to know why.
        basis: String,
    },
    #[serde(rename_all = "snake_case")]
    Refused {
        task: DecisionTask,
        /// Why, in the words a reader of the trace will see.
        basis: String,
    },
}

/// The provider identity the reply carries (the boundary checks it against the
/// provider that was asked; a provider reporting itself as something else would
/// make the trace wrong on purpose).
#[derive(Debug, Clone, PartialEq)]
pub struct RuleDecisionProvider {
    pub provider_id: String,
    pub model: String,
}

impl Default for RuleDecisionProvider {
    fn default() -> Self {
        Self {
            provider_id: "rule-baseline".to_string(),
            model: "cue-heuristic/v1".to_string(),
        }
    }
}

impl RuleDecisionProvider {
    pub fn new(provider_id: &str, model: &str) -> Self {
        Self {
            provider_id: provider_id.to_string(),
            model: model.to_string(),
        }
    }

    /// The reply the port expects, for a request that was already validated.
    ///
    /// Synchronous, and honest about it: nothing here awaits, and a provider
    /// that answers synchronously should say so rather than dress a finished
    /// answer as a pending one. (The TS seam wraps this in `Promise.resolve`
    /// and returns the reply as a promise — the port's `decide` returns the
    /// value directly; the orchestrator's `ask` accounts for both timings in
    /// its latency.)
    pub fn decide(&self, request: &DecisionRequest) -> DecisionResult {
        let utterance = normalize_utterance(&request.utterance);
        let mut decisions = Vec::new();
        let mut refused = Vec::new();
        for question in &request.questions {
            match self.answer_question(question, &utterance, &request.evidence) {
                Ok(decision) => decisions.push(decision),
                Err(reason) => refused.push(DecisionRefusal {
                    id: question.id,
                    reason,
                }),
            }
        }
        DecisionResult {
            provider_id: self.provider_id.clone(),
            model: self.model.clone(),
            decisions,
            refused,
        }
    }

    /// What this provider would answer about one task, and why — the report
    /// wrapped so every refusal is offered with its stated basis.
    fn answer_question(
        &self,
        question: &DecisionQuestion,
        utterance: &str,
        evidence: &Value,
    ) -> Result<Decision, String> {
        let task = question.id;
        match self.report_task(task, utterance, evidence) {
            RuleReport::Refused { basis, .. } => Err(basis),
            RuleReport::Answered {
                answer, confidence, ..
            } => {
                if let Some(_problem) = task_answer_problem(task, &answer) {
                    return Err(format!("{task} answered outside its own answer set"));
                }
                let definition = definition_for(task);
                if question.kind != definition.kind {
                    return Err(format!(
                        "{task} answered as a {}, not a {}",
                        question.kind.as_str(),
                        definition.kind.as_str()
                    ));
                }
                Ok(Decision {
                    id: task,
                    kind: question.kind,
                    answer,
                    confidence,
                })
            }
        }
    }

    /// The report for one task: coverage is measured, everything else counts
    /// cues.
    pub fn report_task(&self, task: DecisionTask, utterance: &str, evidence: &Value) -> RuleReport {
        if task == DecisionTask::EvidenceSufficiency {
            let answer = score_coverage(evidence);
            return RuleReport::Answered {
                task,
                answer: Value::from(answer),
                confidence: 0.5,
                basis: "measured from the supplied facts".to_string(),
            };
        }
        let Some(table) = cues_for(task) else {
            return RuleReport::Refused {
                task,
                basis: "this question has no cue table here".to_string(),
            };
        };
        report_from_cues(task, utterance, table)
    }

    /// Every task this provider would answer for a request, as reports.
    ///
    /// The evaluation harness uses this rather than `decide`, because a harness
    /// measuring agreement needs the refusals as first-class reports. A
    /// malformed request yields nothing, for the same reason
    /// [`crate::decision::orchestrator`] refuses one: there is no honest answer
    /// to a question that was not properly asked.
    pub fn report(&self, request: &DecisionRequest) -> Vec<RuleReport> {
        if request_problem(request).is_some() {
            return Vec::new();
        }
        let utterance = normalize_utterance(&request.utterance);
        request
            .questions
            .iter()
            .map(|question| self.report_task(question.id, &utterance, &request.evidence))
            .collect()
    }
}

impl super::orchestrator::DecisionProvider for RuleDecisionProvider {
    fn identity(&self) -> (String, String) {
        (self.provider_id.clone(), self.model.clone())
    }

    fn ready(&self) -> bool {
        // `deterministic, offline, cue based` — the health record the baseline
        // carries as a literal.
        true
    }

    fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, String> {
        Ok(RuleDecisionProvider::decide(self, request))
    }
}

/// Count cues, then state the answer and how much the count is worth.
///
/// The confidence formula decides what the baseline can prove, so it is written
/// out rather than hidden in a constant:
///
/// - Nothing fires: a refusal. A provider with nothing to read says nothing.
/// - The tally ties or is negative: the same. A tie is not an answer.
/// - One cue decides it: 0.3, below every task's floor. One word is a hint.
/// - Two cues decide it: 0.65, above most floors and below routing's.
/// - Three or more: 0.85, high enough to apply where a task allows it.
///
/// The ceiling stops short of 1.0 on purpose. See rule 2 above.
pub fn report_from_cues(
    task: DecisionTask,
    utterance: &str,
    table: &[super::vocabulary::CueRow],
) -> RuleReport {
    let mut tallies: Vec<(String, i64, usize)> = Vec::new(); // (label, tally, fired)
    for row in table {
        let mut tally = 0i64;
        let mut fired = 0usize;
        for cue in row.set.toward {
            if mentions(utterance, &normalize_cue(cue)) {
                tally += 1;
                fired += 1;
            }
        }
        for cue in row.set.away {
            if mentions(utterance, &normalize_cue(cue)) {
                tally -= 1;
            }
        }
        tallies.push((row.label.to_string(), tally, fired));
    }
    tallies.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| right.2.cmp(&left.2)));

    let Some((best_label, best_tally, best_cues)) = tallies.first().cloned() else {
        return RuleReport::Refused {
            task,
            basis: "this question has no answers".to_string(),
        };
    };
    if best_tally <= 0 {
        return RuleReport::Refused {
            task,
            basis: "no cue decided it".to_string(),
        };
    }
    if let Some((_, runner_tally, _)) = tallies.get(1) {
        if *runner_tally == best_tally {
            return RuleReport::Refused {
                task,
                basis: "the cues tied".to_string(),
            };
        }
    }
    // The boolean tasks are asked as yes/no with a single cue label.
    let candidate = match task {
        DecisionTask::Clarification | DecisionTask::HandoffRecommendation => {
            Value::Bool(best_label == "true")
        }
        // The label is still needed for the refusal and the basis lines below.
        _ => Value::String(best_label.clone()),
    };
    let problem = task_answer_problem(task, &candidate);
    if problem.is_some() {
        return RuleReport::Refused {
            task,
            basis: format!("{} is not an answer this task offers", best_label),
        };
    }
    RuleReport::Answered {
        task,
        answer: candidate,
        confidence: confidence_for(best_cues),
        basis: format!("{} cue(s) for {}", best_cues, best_label),
    }
}

/// How many cues fired, and how much that is worth. See the note above.
pub fn confidence_for(cue_count: usize) -> f64 {
    match cue_count {
        0 => 0.0,
        1 => 0.3,
        2 => 0.65,
        _ => 0.85,
    }
}

/// Coverage of the supplied facts, on [0, 1].
///
/// Measured from the evidence and not from the utterance, because that is what
/// the task asks about: how much of the answer the supplied facts already
/// cover. An utterance cannot know that, and a score derived from the words
/// alone would be fluency reported as coverage.
///
/// The tenant id and the locale are on every request, so they are not evidence.
/// What is counted is how many named facts arrived, damped: five facts is full
/// coverage for the questions this package asks, and beyond that there is
/// nothing left to measure. Empty evidence scores 0, which is honest — nothing
/// was supplied — and no score here reaches 1.0, because a heuristic can say
/// that some facts arrived, not that they suffice.
///
/// The 0.5 confidence is the count's worth, not the facts' worth: that facts
/// arrived is arithmetic, that they suffice is not. It sits below the floor
/// this task set for itself, which is the right place for it — a baseline whose
/// coverage estimate can already replace a caller's own judgement is a baseline
/// that has stopped being one.
pub fn score_coverage(evidence: &Value) -> f64 {
    let Object(object) = evidence else {
        return 0.0;
    };
    let count = object.len();
    if count == 0 {
        return 0.0;
    }
    (count as f64 / 5.0).min(0.8)
}

/// Whether a report is the honest baseline it claims to be.
pub fn rule_report_problem(report: &RuleReport) -> Option<String> {
    match report {
        RuleReport::Refused { task, basis } => {
            if basis.trim().is_empty() {
                Some(format!("report for {task} needs a stated basis"))
            } else {
                None
            }
        }
        RuleReport::Answered {
            task,
            answer,
            confidence,
            basis,
        } => {
            if !confidence.is_finite() || *confidence < 0.0 || *confidence > 1.0 {
                return Some(format!("report for {task} has a confidence outside [0, 1]"));
            }
            if *confidence == 1.0 {
                return Some(format!("report for {task} claims certainty"));
            }
            if basis.trim().is_empty() {
                return Some(format!("report for {task} needs a stated basis"));
            }
            task_answer_problem(*task, answer).map(Some).unwrap_or(None)
        }
    }
}

/// A provider that throws. Only tests use this.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrowingProvider;

impl super::orchestrator::DecisionProvider for ThrowingProvider {
    fn identity(&self) -> (String, String) {
        ("throwing".to_string(), "always-fails".to_string())
    }

    fn ready(&self) -> bool {
        true
    }

    fn decide(&self, _request: &DecisionRequest) -> Result<DecisionResult, String> {
        Err("this provider always fails".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::super::tasks::decision_question_for;
    use super::*;
    use serde_json::json;

    fn provider() -> RuleDecisionProvider {
        RuleDecisionProvider::default()
    }

    #[test]
    fn counts_cues_and_never_claims_certainty() {
        let report = provider().report_task(
            DecisionTask::KnowledgeRouting,
            &normalize_utterance("berapa harga kamar deluxe?"),
            &json!({}),
        );
        let RuleReport::Answered {
            confidence, basis, ..
        } = report
        else {
            panic!("harga should route to structured truth");
        };
        // "harga" and "berapa" fire; nothing in the retrieval row does.
        assert_eq!(confidence, confidence_for(2));
        assert_eq!(basis, "2 cue(s) for structured_truth");
        assert!(confidence < 1.0);
    }

    #[test]
    fn refuses_when_no_cue_decides_it() {
        let report = provider().report_task(
            DecisionTask::KnowledgeRouting,
            &normalize_utterance("weather is nice today"),
            &json!({}),
        );
        assert!(
            matches!(report, RuleReport::Refused { basis, .. } if basis == "no cue decided it")
        );
    }

    #[test]
    fn coverage_is_damped_and_measured_from_facts() {
        assert_eq!(score_coverage(&json!({})), 0.0);
        assert_eq!(score_coverage(&json!({"page": {}})), 1.0 / 5.0);
        // `Math.min(count / 5, 0.8)`: four names hits the cap, five cannot
        // raise it.
        assert_eq!(score_coverage(&json!({"a":1,"b":2,"c":3,"d":4})), 0.8);
        assert_eq!(score_coverage(&json!({"a":1,"b":2,"c":3,"d":4,"e":5})), 0.8);
        assert_eq!(
            score_coverage(&json!({"a":1,"b":2,"c":3,"d":4,"e":5,"f":6})),
            0.8
        );
    }

    #[test]
    fn reports_are_honest_about_what_they_claim() {
        let provider = provider();
        for report in provider.report(&sample_request()) {
            assert_eq!(rule_report_problem(&report), None, "{report:?}");
        }
    }

    fn sample_request() -> DecisionRequest {
        DecisionRequest {
            tenant_id: "acme".to_string(),
            utterance: "my booking status".to_string(),
            locale: "en".to_string(),
            evidence: json!({"page": {"kind": "rooms"}}),
            questions: [
                DecisionTask::IntentClassification,
                DecisionTask::KnowledgeRouting,
                DecisionTask::HandoffRecommendation,
                DecisionTask::EvidenceSufficiency,
            ]
            .iter()
            .map(|task| decision_question_for(*task))
            .collect(),
        }
    }
}
