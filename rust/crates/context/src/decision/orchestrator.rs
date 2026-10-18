//! The orchestrator: the only place a provider's answer can become effective.
//! Port of `packages/decision/src/{validate,orchestrator}.ts`.
//!
//! Everything upstream decides *questions*; this decides *whether an answer is
//! allowed to matter*. Each task is judged twice, by two questions that fail
//! separately — is the answer usable at all (validation, at run time), and is
//! this task *permitted* to accept it (the task's own confidence floor and its
//! own disagreement policy, never a shared number) — and a third question every
//! time: what did the caller already have? The baseline is handed in per task
//! by the caller, because only the caller knows what it would have done.
//!
//! Nothing here retries: one provider call per turn, and the answer is whatever
//! came back on the first one. Every failure mode lands on the baseline with a
//! reason attached; nothing a provider does is ever thrown to the caller.
//!
//! `validate.ts` rides in this file (the `request_problem` /
//! `question_shape_problem` / `check_decision_response` functions below): the
//! source keeps the boundary in the package it judges, and the port keeps that
//! physical fact too — those three functions are its exact surface.
//!
//! Port differences, each deliberate:
//! - The TS boundary reads `unknown` wire shapes (`typeof === 'object'`,
//!   array-presence, number-ness). Those shapes cannot exist in this port: the
//!   reply is a typed [`DecisionResult`], so whole-reply and per-item shape
//!   problems are unrepresentable and the checks that remain are the ones a
//!   typed reply can still get wrong — identity, orphan ids, duplicates,
//!   per-question shape, completeness.
//! - The TS mode string and enabled-task ids are checked against two
//!   vocabularies at run time. Here they are enum values; the same malformed
//!   config does not parse.
//! - The TS timeout races a timer against the call. This port's call is
//!   synchronous, so the race is judged after the call returns: the timer
//!   fires first exactly when the call is not done by then.
//! - The TS async provider port becomes a `Result`-returning trait. The
//!   failure channel is the `Err` arm, and its text is never surfaced — a
//!   provider's error may carry credentials or payload fragments, so the
//!   refusal is context-blind on purpose (`describe` in the source).
//! - `health` in the source is typd `unknown`, and only an explicit
//!   `ready: false` is read as unusable. The trait's `ready()` is that explicit
//!   declaration; the "unreadable object carries on" branch is unrepresentable.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::policy::{
    agreement_of, confidence_problem, disagreement_problem, task_policy, Agreement, AssistMode,
    TaskPolicy,
};
use super::tasks::{
    baseline_problem, decision_question_for, task_answer_problem, BaselineAnswer, Decision,
    DecisionKind, DecisionQuestion, DecisionRequest, DecisionResult, DecisionTask,
};

/// A misconfigured orchestrator (the caller's own bug), as opposed to a
/// provider that could not answer — that one lands on the baseline instead.
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionError(String);

impl std::fmt::Display for DecisionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DecisionError {}

/// Who a reply is claimed to be from, and who it was asked of.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProviderIdentity {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub model: String,
}

/// The port's provider seam.
///
/// The source's port is async by construction ("the honest port for a bounded
/// judgment that costs a network call is one that can fail without anyone
/// holding an event loop hostage"). The Rust seam is a synchronous
/// `Result`-returning trait: the failure channel is the `Err` arm. A real
/// HTTP-backed provider arrives with its own async runtime and wraps this
/// trait, the way the source's server-only vendor client wraps its transport.
///
/// Implementations must be side-effect free with respect to the platform: they
/// may call a model; they must not write to a store, dispatch an action, or
/// mutate their arguments — anything with an effect belongs behind an action,
/// where the policy gate can see it.
pub trait DecisionProvider {
    /// `(providerId, model)` — a name, never a credential.
    fn identity(&self) -> (String, String);

    /// The explicit `ready: false` of the source's health object. This port's
    /// seam declares health as a boolean; the source's "health object I cannot
    /// read carries on" branch is unrepresentable for the same reason a typed
    /// reply is.
    fn ready(&self) -> bool;

    /// One call. `Err` is the provider's business; the orchestrator holds the
    /// turn on the baseline and never surfaces the error text.
    fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, String>;
}

/// One decision the boundary accepted, already checked against the task's own
/// answer set.
#[derive(Debug, Clone)]
pub struct CheckedDecision {
    pub decision: Decision,
    pub answer: Value,
    pub confidence: f64,
}

/// A reply the boundary accepted, in full. Every id the request asked appears
/// exactly once — here or in `refusals`.
#[derive(Debug, Clone)]
pub struct CheckedResponse {
    pub decisions: HashMap<DecisionTask, CheckedDecision>,
    /// The provider's own account of why it did not answer, verbatim, per task.
    pub refusals: HashMap<DecisionTask, String>,
}

/// Whether a question cannot be answered in the shape it was given.
///
/// Each rule is the one that makes its kind meaningful: a `choice` with no
/// options is a question with no answer set; a `score` with reversed bounds is
/// a range with no interior.
///
/// Not a thrown error because the caller wants to name *which* question was
/// malformed, and a specific sentence beats a stack. (The source also refuses a
/// blank id; that shape cannot exist here — the id is a [`DecisionTask`], not
/// an unchecked string.)
pub fn question_shape_problem(question: &DecisionQuestion) -> Option<String> {
    if question.prompt.trim().is_empty() {
        return Some(format!("Question \"{}\" needs a prompt.", question.id));
    }
    if question.kind == DecisionKind::Choice
        && question
            .options
            .as_ref()
            .is_none_or(|options| options.is_empty())
    {
        return Some(format!(
            "Question \"{}\" is a choice with no options; a closed answer set is the whole point.",
            question.id
        ));
    }
    if question.kind == DecisionKind::Score {
        let Some(bounds) = question.bounds else {
            return Some(format!(
                "Question \"{}\" is a score with no bounds.",
                question.id
            ));
        };
        if bounds.0 > bounds.1 {
            return Some(format!(
                "Question \"{}\" has bounds {}..{}; the range is empty.",
                question.id, bounds.0, bounds.1
            ));
        }
    }
    None
}

/// Whether a request is worth sending.
///
/// Checked rather than trusted, because the catalogue — not the caller —
/// decides which ids exist. A request carrying a task outside the catalogue
/// would be answered in a vocabulary the evaluator cannot read, and the mistake
/// would surface as a malformed reply rather than as the malformed request it
/// was. (A request carrying an unknown id is unrepresentable here: the id type
/// *is* the catalogue. The remaining checks — shape, one question per task,
/// kind agreeing with the catalogue — are the ones a hand-built request can
/// still get wrong.)
pub fn request_problem(request: &DecisionRequest) -> Option<String> {
    if request.tenant_id.trim().is_empty() {
        return Some("a decision request needs a tenant id".to_string());
    }
    if request.locale.trim().is_empty() {
        return Some("a decision request needs a locale".to_string());
    }
    if request.questions.is_empty() {
        return Some("a decision request needs at least one question".to_string());
    }
    let mut seen: Vec<DecisionTask> = Vec::new();
    for question in &request.questions {
        if let Some(problem) = question_shape_problem(question) {
            return Some(problem);
        }
        if seen.contains(&question.id) {
            return Some(format!(
                "question {} was asked twice in one request",
                question.id
            ));
        }
        seen.push(question.id);
        let definition = super::tasks::definition_for(question.id);
        if definition.kind != question.kind {
            return Some(format!(
                "question {} was asked as a {}, not a {}",
                question.id,
                question.kind.as_str(),
                definition.kind.as_str()
            ));
        }
    }
    None
}

/// The shape of one decision, against the question it must answer.
fn decision_problem(question: &DecisionQuestion, decision: &Decision) -> Option<String> {
    if decision.id != question.id {
        return Some(format!("answer for {} carries a different id", question.id));
    }
    if decision.kind != question.kind {
        return Some(format!(
            "answer for {} is a {}, not a {}",
            question.id,
            decision.kind.as_str(),
            question.kind.as_str()
        ));
    }
    if !decision.confidence.is_finite() {
        return Some(format!(
            "confidence for {} must be a finite number",
            question.id
        ));
    }
    if decision.confidence < 0.0 || decision.confidence > 1.0 {
        return Some(format!("confidence for {} is outside [0, 1]", question.id));
    }
    match question.kind {
        DecisionKind::Choice => {
            let Some(answer) = decision.answer.as_str() else {
                return Some(format!(
                    "answer for {} must be one of its options",
                    question.id
                ));
            };
            let options = question.options.as_deref().unwrap_or(&[]);
            if !options.iter().any(|option| option.as_str() == answer) {
                return Some(format!(
                    "answer for {} is not in its answer set",
                    question.id
                ));
            }
            None
        }
        DecisionKind::Score => {
            let Some(answer) = decision.answer.as_f64().filter(|value| value.is_finite()) else {
                return Some(format!(
                    "answer for {} must be a finite number",
                    question.id
                ));
            };
            let (low, high) = question.bounds.unwrap_or((0.0, 1.0));
            if answer < low || answer > high {
                return Some(format!("answer for {} is outside its range", question.id));
            }
            None
        }
        DecisionKind::Boolean => {
            if decision.answer.is_boolean() {
                None
            } else {
                Some(format!("answer for {} must be a boolean", question.id))
            }
        }
    }
}

/// Check a provider's reply against the request it is meant to answer.
///
/// `expected` is the provider that was asked. A reply naming a different one is
/// refused: the alternative is a candidate attributed to a model that never
/// produced it, which is worse than no candidate, because a trace that lies is
/// a trace a reader has no way to catch.
///
/// The unit of trust is a *complete* answer. There is no partial acceptance,
/// because a caller holding a partial answer cannot tell which of the five it
/// is missing, and the one it is missing is the one that mattered. A failure
/// closes the whole reply, not the offending item, and the reason carries no
/// provider output — so it can travel into a trace without becoming
/// a channel for anything the provider said.
pub fn check_decision_response(
    response: &DecisionResult,
    request: &DecisionRequest,
    expected: &ProviderIdentity,
) -> Result<CheckedResponse, String> {
    if let Some(asked) = request_problem(request) {
        return Err(asked);
    }
    if expected.provider_id.trim().is_empty() || expected.model.trim().is_empty() {
        return Err(
            "a provider without an identity cannot be traced, so it cannot answer".to_string(),
        );
    }
    if response.provider_id != expected.provider_id || response.model != expected.model {
        return Err(
            "a provider replied under a different identity than it was asked with".to_string(),
        );
    }

    let mut decisions: HashMap<DecisionTask, CheckedDecision> = HashMap::new();
    for decision in &response.decisions {
        let Some(question) = request.questions.iter().find(|q| q.id == decision.id) else {
            return Err(format!(
                "a provider answered a question that was not asked: {}",
                decision.id
            ));
        };
        if decisions.contains_key(&decision.id) {
            return Err(format!(
                "a provider answered {} twice in one reply",
                decision.id
            ));
        }
        if let Some(problem) = decision_problem(question, decision) {
            return Err(problem);
        }
        // The definition-level answer set, after the question-level one: a
        // reply that satisfies its question but names something the catalogue
        // does not carry closes the reply too.
        if task_answer_problem(decision.id, &decision.answer).is_some() {
            return Err(format!(
                "answer for {} is not one of its answers",
                decision.id
            ));
        }
        decisions.insert(
            decision.id,
            CheckedDecision {
                decision: decision.clone(),
                answer: decision.answer.clone(),
                confidence: decision.confidence,
            },
        );
    }

    let mut refusals: HashMap<DecisionTask, String> = HashMap::new();
    for refusal in &response.refused {
        if !request
            .questions
            .iter()
            .any(|question| question.id == refusal.id)
        {
            return Err(format!(
                "a provider refused a question that was not asked: {}",
                refusal.id
            ));
        }
        if decisions.contains_key(&refusal.id) || refusals.contains_key(&refusal.id) {
            return Err(format!("a provider answered and refused {}", refusal.id));
        }
        if refusal.reason.trim().is_empty() {
            return Err(format!(
                "a refusal for {} needs a reason somebody can read",
                refusal.id
            ));
        }
        refusals.insert(refusal.id, refusal.reason.clone());
    }

    for question in &request.questions {
        if !decisions.contains_key(&question.id) && !refusals.contains_key(&question.id) {
            return Err(format!("a provider left {} unanswered", question.id));
        }
    }

    Ok(CheckedResponse {
        decisions,
        refusals,
    })
}

/// Where an effective answer came from. Never invented, always attributable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AnswerSource {
    Baseline,
    Provider,
}

/// The answer a caller should act on, for one task.
///
/// Erased form of the source's correlated union: the task and a `Value` the
/// boundary (or `task_answer_problem`, for baselines) already narrowed to what
/// that task may answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EffectiveDecision {
    pub task: DecisionTask,
    pub answer: Value,
    pub source: AnswerSource,
    /// Why this is the answer, in the words the trace carries.
    pub basis: String,
}

/// What a provider offered for one task, before policy looked at it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct CandidateDecision {
    pub task: DecisionTask,
    pub answer: Value,
    pub confidence: f64,
    pub agreement: Agreement,
}

/// What became of the provider's side of one task.
///
/// `not_called` is a real outcome and not an absence: it is the record that
/// mode `off` cost nothing, which is a claim worth being able to check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceOutcome {
    NotCalled,
    Answered,
    Refused,
    Failed,
    Timeout,
    Malformed,
}

/// One line of the audit trail, per task.
///
/// Metadata only, by construction. There is no field here that could hold an
/// utterance, a piece of evidence, a credential, or a tenant's state — not
/// because a caller is trusted to fill it in carefully, but because the shape
/// has nowhere to put them. The tenant id is absent for the same reason in the
/// other direction: a trace is emitted into a turn the caller is already
/// holding, and a log line that names a tenant is a log line that can be read
/// across tenants.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionTrace {
    pub task: DecisionTask,
    pub mode: AssistMode,
    #[serde(rename = "providerId")]
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub outcome: TraceOutcome,
    /// Why the effective answer is the effective one. Always readable.
    pub reason: String,
    pub confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate: Option<CandidateDecision>,
    pub effective: EffectiveDecision,
    /// How the candidate compares with the baseline, or null if there was none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agreement: Option<Agreement>,
    /// Whether the provider's answer is the one in force. A provider that
    /// agreed with the baseline is still applied — it is the provider that
    /// answered, and `agreement` says whether that made a difference.
    pub applied: bool,
    /// Whether assist *would* apply the candidate, had the mode and the config
    /// permitted it. This is what makes a shadow run a measurement rather a
    /// shrug: it reports the gates the candidate cleared, without opening them.
    #[serde(rename = "wouldApply")]
    pub would_apply: bool,
    pub latency_ms: Option<u64>,
}

/// A turn, as the decision is asked about it. `Omit<DecisionRequest,
/// 'questions'>` in the source: the orchestrator builds the questions.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionTurn {
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    pub utterance: String,
    pub locale: String,
    /// §16-masked facts a decision may weigh. Whatever is here has already
    /// been through the field masker, so a provider cannot be handed a secret
    /// because a caller forgot to strip it.
    pub evidence: Value,
}

/// The whole batched run.
#[derive(Debug, Clone, Serialize)]
pub struct DecisionRun {
    #[serde(rename = "providerId")]
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub mode: AssistMode,
    /// One per asked task, in the order the baselines were given.
    pub decisions: Vec<EffectiveDecision>,
    pub traces: Vec<DecisionTrace>,
    pub latency_ms: Option<u64>,
}

const GATES_OPEN: &str = "every gate this task set for itself is open";
const TIMED_OUT: &str = "the provider did not answer in time";
const NOT_READY: &str = "the provider reported itself not ready";

/// The one provider call a turn is allowed to make, and every way it settles.
enum Reply {
    NotCalled {
        reason: &'static str,
    },
    Timeout {
        reason: &'static str,
    },
    /// A provider failure or not-ready verdict. The reason names what
    /// happened and never carries provider output.
    Failed {
        reason: String,
    },
    Malformed {
        problem: String,
    },
    Answered {
        checked: CheckedResponse,
    },
}

impl Reply {
    fn kind_outcome(&self) -> TraceOutcome {
        match self {
            Reply::NotCalled { .. } => TraceOutcome::NotCalled,
            Reply::Timeout { .. } => TraceOutcome::Timeout,
            Reply::Failed { .. } => TraceOutcome::Failed,
            Reply::Malformed { .. } => TraceOutcome::Malformed,
            Reply::Answered { .. } => TraceOutcome::Answered,
        }
    }

    fn reason(&self) -> String {
        match self {
            Reply::NotCalled { reason } | Reply::Timeout { reason } => (*reason).to_string(),
            Reply::Failed { reason } | Reply::Malformed { problem: reason } => reason.clone(),
            Reply::Answered { .. } => String::new(),
        }
    }
}

const DEFAULT_TIMEOUT_MS: u64 = 3_000;

/// The orchestrator: constructed once, run per turn.
///
/// Generic over the provider implementation. Construction throws for a
/// misconfiguration only, and everything a provider does wrong lands on the
/// baseline instead.
pub struct DefaultOrchestrator<P: DecisionProvider> {
    provider: Option<P>,
    mode: AssistMode,
    /// The tasks assist may touch, in configured order, de-duplicated. Empty is
    /// not a synonym for "all": it means nothing may be replaced, which is the
    /// safe reading of a config that says nothing. (The source fails closed on
    /// an id outside the catalogue; that spelling mistake does not parse here.)
    enabled: Vec<DecisionTask>,
    /// Per-task floors a tenant may tighten. Never lowered from the task's own.
    minimum_confidence: HashMap<DecisionTask, f64>,
    /// Milliseconds before a provider is abandoned. Defaults to 3 seconds.
    timeout_ms: u64,
    /// Milliseconds since an arbitrary origin, for latency. The default reads
    /// the system clock; a constructor-injected closure stands in for the
    /// source's `now` option.
    now: NowFn,
    /// The source's per-trace observability hook, called for every trace in
    /// ask order.
    on_trace: Option<OnTraceFn>,
}

/// The source's `now` option: an injected clock reading milliseconds since
/// the epoch, defaulting to the system clock.
pub type NowFn = Box<dyn Fn() -> u64>;

/// The source's per-trace observability hook, called for every trace in ask
/// order.
pub type OnTraceFn = Box<dyn Fn(&DecisionTrace)>;

impl<P: DecisionProvider> DefaultOrchestrator<P> {
    pub fn new(
        provider: Option<P>,
        mode: AssistMode,
        enabled_tasks: &[DecisionTask],
        minimum_confidence: &[(DecisionTask, f64)],
        timeout_ms: Option<u64>,
        now: Option<NowFn>,
        on_trace: Option<OnTraceFn>,
    ) -> Result<Self, DecisionError> {
        if mode != AssistMode::Off && provider.is_none() {
            return Err(DecisionError(format!(
                "mode {} needs a provider, and none was configured",
                mode.as_str()
            )));
        }
        let timeout_ms = match timeout_ms {
            None => DEFAULT_TIMEOUT_MS,
            // Finite and positive is the source's check; `!isFinite` and
            // negative do not exist in this integer type.
            Some(0) => {
                return Err(DecisionError(
                    "decision timeout must be a positive finite number of milliseconds".to_string(),
                ));
            }
            Some(value) => value,
        };
        let mut minimum_confidence_map = HashMap::new();
        for (task, value) in minimum_confidence {
            // The source rejects a floor it cannot read or one outside [0, 1];
            // `!isFinite` and negatives do not exist in this float check.
            if !value.is_finite() || *value < 0.0 || *value > 1.0 {
                return Err(DecisionError(
                    "invalid decision task confidence floor".to_string(),
                ));
            }
            minimum_confidence_map.insert(*task, *value);
        }
        let mut enabled = Vec::new();
        for task in enabled_tasks {
            if !enabled.contains(task) {
                enabled.push(*task);
            }
        }
        Ok(Self {
            provider,
            mode,
            enabled,
            minimum_confidence: minimum_confidence_map,
            timeout_ms,
            now: now.unwrap_or_else(|| Box::new(system_millis)),
            on_trace,
        })
    }

    /// Answer every task in `baselines`, applying a provider where policy
    /// allows.
    ///
    /// Throws for a malformed turn or a malformed baseline set, because those
    /// are the caller's own bugs and a turn that cannot be asked is not a turn
    /// to answer from nothing. Nothing a provider does is ever thrown: a
    /// provider's failure is the provider's business, and it lands on the
    /// baseline with a reason attached.
    pub fn run(
        &self,
        turn: &DecisionTurn,
        baselines: &[BaselineAnswer],
    ) -> Result<DecisionRun, DecisionError> {
        if let Some(problem) = turn_problem(turn) {
            return Err(DecisionError(problem));
        }
        // In mode `off` every task is asked of the baseline set as given; in
        // the assist modes only the tasks assist is configured for are asked —
        // a baseline outside `enabled` is dropped without validation, the
        // source's filter happening before the map.
        let mut asked: Vec<(&BaselineAnswer, DecisionQuestion)> = Vec::new();
        for baseline in baselines {
            if self.mode != AssistMode::Off && !self.enabled.contains(&baseline.task) {
                continue;
            }
            if let Some(problem) = baseline_problem(baseline) {
                return Err(DecisionError(problem));
            }
            if asked.iter().any(|(other, _)| other.task == baseline.task) {
                return Err(DecisionError(format!(
                    "a turn was given two baselines for {}",
                    baseline.task
                )));
            }
            asked.push((baseline, decision_question_for(baseline.task)));
        }
        let (provider_id, model) = self.identity();
        if asked.is_empty() {
            return Ok(DecisionRun {
                provider_id,
                model,
                mode: self.mode,
                decisions: Vec::new(),
                traces: Vec::new(),
                latency_ms: None,
            });
        }

        let request = DecisionRequest {
            tenant_id: turn.tenant_id.clone(),
            utterance: turn.utterance.clone(),
            locale: turn.locale.clone(),
            evidence: turn.evidence.clone(),
            questions: asked.iter().map(|(_, question)| question.clone()).collect(),
        };

        let started = (self.now)();
        let reply = self.ask(&request);
        let latency = match reply.kind_outcome() {
            TraceOutcome::NotCalled => None,
            _ => Some((self.now)().saturating_sub(started)),
        };

        let mut decisions = Vec::new();
        let mut traces = Vec::new();
        for (baseline, _) in &asked {
            let (decision, trace) = self.outcome_for(baseline.task, baseline, &reply, latency);
            decisions.push(decision);
            traces.push(trace.clone());
            if let Some(on_trace) = &self.on_trace {
                on_trace(&trace);
            }
        }

        Ok(DecisionRun {
            provider_id,
            model,
            mode: self.mode,
            decisions,
            traces,
            latency_ms: latency,
        })
    }

    /// The one provider call, and everything that can go wrong with it.
    ///
    /// The source's call is async with an expiry racing it. This port's call is
    /// synchronous, so the expiry is judged after the call returns: the timer
    /// fires first exactly when the call is not done by then.
    fn ask(&self, request: &DecisionRequest) -> Reply {
        let provider = match &self.provider {
            None => {
                return Reply::NotCalled {
                    reason: "this mode makes no provider call",
                }
            }
            Some(provider) if self.mode == AssistMode::Off => {
                return Reply::NotCalled {
                    reason: "this mode makes no provider call",
                };
            }
            Some(provider) => provider,
        };
        if !provider.ready() {
            return Reply::Failed {
                reason: NOT_READY.to_string(),
            };
        }
        let (provider_id, model) = provider.identity();
        let expected = ProviderIdentity { provider_id, model };

        let started = (self.now)();
        let outcome = provider.decide(request);
        if (self.now)().saturating_sub(started) >= self.timeout_ms {
            return Reply::Timeout { reason: TIMED_OUT };
        }
        match outcome {
            // Provider error text may contain credentials or payload fragments.
            // The refusal is context-blind on purpose (the source's `describe`).
            Err(_) => Reply::Failed {
                reason: "the decision provider failed".to_string(),
            },
            Ok(response) => match check_decision_response(&response, request, &expected) {
                Err(problem) => Reply::Malformed { problem },
                Ok(checked) => Reply::Answered { checked },
            },
        }
    }

    fn identity(&self) -> (Option<String>, Option<String>) {
        match &self.provider {
            Some(provider) => {
                let (id, model) = provider.identity();
                (Some(id), Some(model))
            }
            None => (None, None),
        }
    }

    /// Every gate a candidate must clear, or the first one that held it.
    ///
    /// Ordered so the reason names the most useful thing: a task nobody
    /// configured is reported as unconfigured rather than as a floor failure,
    /// and a floor failure is reported as a floor rather than as a downgrade.
    fn gate_reason(
        &self,
        task: DecisionTask,
        baseline: &BaselineAnswer,
        candidate: &CandidateDecision,
    ) -> Option<String> {
        if !self.enabled.contains(&task) {
            return Some(format!("{task} is not configured for assist"));
        }
        let policy = task_policy(task);
        let configured = self.minimum_confidence.get(&task).copied().unwrap_or(0.0);
        let effective = TaskPolicy {
            minimum_confidence: policy.minimum_confidence.max(configured),
            disagreement: policy.disagreement,
        };
        if let Some(problem) = confidence_problem(&effective, candidate.confidence) {
            return Some(problem);
        }
        disagreement_problem(task, &policy, &baseline.answer, &candidate.answer)
    }

    /// Answer one task: run the gates in order, and say which one held.
    fn outcome_for(
        &self,
        task: DecisionTask,
        baseline: &BaselineAnswer,
        reply: &Reply,
        latency_ms: Option<u64>,
    ) -> (EffectiveDecision, DecisionTrace) {
        let (provider_id, model) = self.identity();
        let held_answer = EffectiveDecision {
            task,
            answer: baseline.answer.clone(),
            source: AnswerSource::Baseline,
            basis: baseline.source.clone(),
        };
        let held = |outcome: TraceOutcome, reason: String| -> (EffectiveDecision, DecisionTrace) {
            (
                held_answer.clone(),
                DecisionTrace {
                    task,
                    mode: self.mode,
                    provider_id: provider_id.clone(),
                    model: model.clone(),
                    outcome,
                    reason,
                    confidence: None,
                    candidate: None,
                    effective: held_answer.clone(),
                    agreement: None,
                    applied: false,
                    would_apply: false,
                    latency_ms,
                },
            )
        };

        if reply.kind_outcome() != TraceOutcome::Answered {
            return held(reply.kind_outcome(), reply.reason());
        }

        let Reply::Answered { checked } = reply else {
            unreachable!("matched one paragraph above")
        };
        let Some(checked) = checked.decisions.get(&task) else {
            // The boundary leaves a task in one of these two states, never
            // neither. Which one it was is the whole difference between
            // "declined" and "silent".
            let reason = if checked.refusals.contains_key(&task) {
                "the provider refused this task"
            } else {
                "the provider did not answer this task"
            };
            return held(TraceOutcome::Refused, reason.to_string());
        };

        let candidate = CandidateDecision {
            task,
            answer: checked.answer.clone(),
            confidence: checked.confidence,
            agreement: agreement_of(task, &baseline.answer, &checked.answer),
        };
        // Read before the candidate is moved into a trace below.
        let candidate_agreement = candidate.agreement;
        let opened = if self.enabled.contains(&task) {
            self.gate_reason(task, baseline, &candidate)
        } else {
            // Unreachable in the assist modes — `asked` is pre-filtered — and
            // the refusal line a caller would read if a future mode ever got
            // here without the filter.
            None
        };

        if self.mode == AssistMode::Shadow {
            return (
                held_answer.clone(),
                DecisionTrace {
                    task,
                    mode: self.mode,
                    provider_id: provider_id.clone(),
                    model: model.clone(),
                    outcome: TraceOutcome::Answered,
                    reason: match &opened {
                        None => "shadow mode keeps the deterministic answer".to_string(),
                        Some(opened) => format!("shadow mode keeps it: {opened}"),
                    },
                    confidence: Some(candidate.confidence),
                    candidate: Some(candidate.clone()),
                    effective: held_answer.clone(),
                    agreement: Some(candidate.agreement),
                    applied: false,
                    would_apply: opened.is_none(),
                    latency_ms,
                },
            );
        }

        if let Some(held_reason) = opened {
            return (
                held_answer.clone(),
                DecisionTrace {
                    task,
                    mode: self.mode,
                    provider_id: provider_id.clone(),
                    model: model.clone(),
                    outcome: TraceOutcome::Answered,
                    reason: held_reason,
                    confidence: Some(candidate.confidence),
                    candidate: Some(candidate),
                    effective: held_answer.clone(),
                    agreement: Some(candidate_agreement),
                    applied: false,
                    would_apply: false,
                    latency_ms,
                },
            );
        }

        let applied = EffectiveDecision {
            task,
            answer: checked.answer.clone(),
            source: AnswerSource::Provider,
            basis: format!(
                "{} answered {task} at {}",
                provider_id
                    .clone()
                    .unwrap_or_else(|| "a provider".to_string()),
                candidate.confidence
            ),
        };
        (
            applied.clone(),
            DecisionTrace {
                task,
                mode: self.mode,
                provider_id,
                model,
                outcome: TraceOutcome::Answered,
                reason: GATES_OPEN.to_string(),
                confidence: Some(candidate.confidence),
                candidate: Some(candidate),
                effective: applied,
                agreement: Some(candidate_agreement),
                applied: true,
                would_apply: true,
                latency_ms,
            },
        )
    }
}

/// Whether a turn is worth asking anything about.
fn turn_problem(turn: &DecisionTurn) -> Option<String> {
    if turn.tenant_id.trim().is_empty() {
        return Some("a decision turn needs a tenant id".to_string());
    }
    if turn.locale.trim().is_empty() {
        return Some("a decision turn needs a locale".to_string());
    }
    // The source also refuses a non-string utterance; the utterance is typed.
    None
}

/// One turn through the deterministic provider and the catalogue's own
/// baselines, with assist enabled for the whole catalogue — the seam
/// [`super::turn::evaluate_decision_turn`] wraps for its orchestrated half.
/// Kept here so the judgment and its one-call witness share a module.
pub fn run_decision_turn(
    provider: &super::provider::RuleDecisionProvider,
    turn: &DecisionTurn,
    baselines: &[BaselineAnswer],
) -> Result<DecisionRun, DecisionError> {
    let orchestrator = DefaultOrchestrator::new(
        Some(provider.clone()),
        AssistMode::Assist,
        super::tasks::DECISION_TASKS,
        &[],
        None,
        None,
        None,
    )?;
    orchestrator.run(turn, baselines)
}

fn system_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_millis() as u64)
        .unwrap_or(0)
}
