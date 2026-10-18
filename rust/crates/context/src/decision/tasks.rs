//! The task catalogue: the bounded questions a decision provider may be asked.
//! Port of `packages/decision/src/tasks.ts`.
//!
//! A provider is asked *questions*, not given a job. The port carries three
//! kinds — boolean, choice, score — and nothing else can cross it, so a task is
//! nothing more than a named, typed, closed-answer question. That is
//! deliberate: the moment a task could ask for prose it would be a brain, and
//! the authority hierarchy would have a provider in it that writes the answer.
//!
//! Five tasks start the catalogue. Adding a sixth is a new question with a new
//! bounded answer set, a new baseline and a new confidence policy — a decision
//! someone makes, not a default.
//!
//! Every answer set is closed, every one has an honest way to say "this is not
//! mine to answer", and none of them can carry an instruction.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The three shapes a question may take (from `@archava/adapters`, folded here
/// because this crate does not import the adapters crate).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionKind {
    Boolean,
    Choice,
    Score,
}

impl DecisionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionKind::Boolean => "boolean",
            DecisionKind::Choice => "choice",
            DecisionKind::Score => "score",
        }
    }
}

/// The five stable ids.
///
/// Spelled once, here. They appear in an audit trail, in a tenant's config and
/// in an evaluation corpus, so they are data rather than identifiers: renaming
/// one is a migration, and a synonym added later is an alias, not a sixth
/// task.
pub const DECISION_TASKS: &[DecisionTask] = &[
    DecisionTask::IntentClassification,
    DecisionTask::KnowledgeRouting,
    DecisionTask::Clarification,
    DecisionTask::HandoffRecommendation,
    DecisionTask::EvidenceSufficiency,
];

/// A bounded question the decision layer may ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DecisionTask {
    /// What the visitor is doing, which decides the posture of the reply.
    #[serde(rename = "intent_classification")]
    IntentClassification,
    /// Which source is allowed to answer, per §17.
    #[serde(rename = "knowledge_routing")]
    KnowledgeRouting,
    /// Whether a clarifying question is worth one round trip.
    #[serde(rename = "clarification")]
    Clarification,
    /// Whether a human should be told about this turn. Never an action.
    #[serde(rename = "handoff_recommendation")]
    HandoffRecommendation,
    /// How much of the answer the grounded facts already cover, in [0, 1].
    #[serde(rename = "evidence_sufficiency")]
    EvidenceSufficiency,
}

impl DecisionTask {
    pub fn as_str(self) -> &'static str {
        match self {
            DecisionTask::IntentClassification => "intent_classification",
            DecisionTask::KnowledgeRouting => "knowledge_routing",
            DecisionTask::Clarification => "clarification",
            DecisionTask::HandoffRecommendation => "handoff_recommendation",
            DecisionTask::EvidenceSufficiency => "evidence_sufficiency",
        }
    }

    /// The catalogue's ids, in declaration order.
    pub const ALL_STR: &'static [&'static str] = &[
        "intent_classification",
        "knowledge_routing",
        "clarification",
        "handoff_recommendation",
        "evidence_sufficiency",
    ];

    /// Narrows an identifier to the catalogue, or nothing.
    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL_STR
            .iter()
            .position(|name| *name == value)
            .map(|index| DECISION_TASKS[index])
    }
}

impl std::fmt::Display for DecisionTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// One task, described: the question asked, and the answers it may return.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskDefinition {
    pub id: DecisionTask,
    pub kind: DecisionKind,
    /// Shown to the provider verbatim. A prompt that names a tenant schema is a
    /// leak.
    pub prompt: &'static str,
    /// The closed answer set for a `choice` task, in ascending escalation
    /// order.
    pub options: &'static [&'static str],
    /// The inclusive range for a `score` task.
    pub bounds: (f64, f64),
    /// Why the task exists, and what it may never decide.
    pub why: &'static str,
}

/// The catalogue, verbatim from the baseline (the `why` texts are part of the
/// contract: they are what a reviewer reads, and the config UI shows them).
pub const DECISION_TASK_DEFINITIONS: &[TaskDefinition] = &[
    TaskDefinition {
        id: DecisionTask::IntentClassification,
        kind: DecisionKind::Choice,
        prompt: "Which kind of turn is this utterance?",
        options: &[
            "availability_inquiry",
            "pricing_inquiry",
            "policy_inquiry",
            "status_inquiry",
            "support_request",
            "recommendation_request",
            "comparison_request",
            "handoff_request",
            "purchase_request",
            "general_inquiry",
        ],
        bounds: (0.0, 1.0),
        why: "The posture of a reply follows from what the visitor is doing, and a turn that \
is mostly one thing with a second thing attached is exactly the case a lexical \
classifier gets wrong in one direction. `general_inquiry` covers an utterance \
outside the specific catalogue without granting any action. It authorizes \
nothing — least of all a page action.",
    },
    TaskDefinition {
        id: DecisionTask::KnowledgeRouting,
        kind: DecisionKind::Choice,
        prompt: "Which source may answer this utterance, structured truth or retrieval?",
        options: &["retrieval_knowledge", "structured_truth"],
        bounds: (0.0, 1.0),
        why: "§17 puts live values in the hands of the live system and published copy in the \
hands of retrieval, and the two are not interchangeable: a stale price quoted \
from a chunk is a wrong answer wearing a citation. The order of `options` is \
the escalation ladder — retrieval is the floor, structured truth is above it — \
so the policy can refuse a move in one direction while permitting the other.",
    },
    TaskDefinition {
        id: DecisionTask::Clarification,
        kind: DecisionKind::Boolean,
        prompt: "Should the assistant ask one clarifying question before answering?",
        options: &[],
        bounds: (0.0, 1.0),
        why: "A clarifying question costs the visitor a round trip, so it has to be worth one. \
This task asks whether it is — when an utterance could be about two different \
things and answering either way would be a guess. It never asks the question on \
the provider's behalf: the words come from the brain, and a provider that \
wanted to write them would need a kind this port does not have.",
    },
    TaskDefinition {
        id: DecisionTask::HandoffRecommendation,
        kind: DecisionKind::Boolean,
        prompt: "Should a member of staff be told about this turn?",
        options: &[],
        bounds: (0.0, 1.0),
        why: "A turn a visitor wants to have with a human is a turn a human should hear about, \
and detecting it is fuzzy in a way a keyword match is not — \"this is the third \
time I am asking\" carries no keyword. What the flag does is limited on \
purpose: it tells a human that a turn happened. It never books, never \
confirms, never grants a permission ActionPolicy did not already grant, and \
never sends anything the visitor did not ask for.",
    },
    TaskDefinition {
        id: DecisionTask::EvidenceSufficiency,
        kind: DecisionKind::Score,
        prompt: "How much of the answer do the supplied facts already cover, from 0 to 1?",
        options: &[],
        bounds: (0.0, 1.0),
        why: "A score rather than a boolean because \"enough\" has a gradient a caller may want \
to act on: a turn with 0.4 coverage is not a turn to answer from nothing. The \
facts are the projected ones in the request — ids, labels, counts, flags — \
never the utterance and never the tenant's schema, so the highest score here is \
a statement about what was supplied and not a claim about the world.",
    },
];

/// The definition of one task. Panicking is fine only where the catalogue
/// itself is wrong; callers pass catalogue ids, not input.
pub fn definition_for(task: DecisionTask) -> &'static TaskDefinition {
    DECISION_TASK_DEFINITIONS
        .iter()
        .find(|definition| definition.id == task)
        .expect("the catalogue covers every task")
}

/// The question for one task, built from the catalogue rather than hand-written.
pub fn decision_question_for(task: DecisionTask) -> DecisionQuestion {
    let definition = definition_for(task);
    DecisionQuestion {
        id: task,
        kind: definition.kind,
        prompt: definition.prompt.to_string(),
        options: if definition.kind == DecisionKind::Choice {
            Some(
                definition
                    .options
                    .iter()
                    .map(|option| option.to_string())
                    .collect(),
            )
        } else {
            None
        },
        bounds: if definition.kind == DecisionKind::Score {
            Some(definition.bounds)
        } else {
            None
        },
    }
}

/// What a provider candidate is made of, before the policy judges it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub id: DecisionTask,
    pub kind: DecisionKind,
    pub answer: Value,
    pub confidence: f64,
}

/// The provider declining one question: "I did not answer" — a refusal the
/// caller can read, and never a zero-confidence default.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionRefusal {
    pub id: DecisionTask,
    /// Why, in the words a reader of the trace will see.
    pub reason: String,
}

/// One question, exactly the shape the boundary hands to a provider.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionQuestion {
    pub id: DecisionTask,
    pub kind: DecisionKind,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bounds: Option<(f64, f64)>,
}

/// A turn, as the decision layer is handed it. The questions are NOT the
/// caller's to write — the orchestrator builds them from the catalogue.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionRequest {
    #[serde(rename = "tenantId")]
    pub tenant_id: String,
    pub utterance: String,
    pub locale: String,
    /// The projected evidence: no form values or customer state.
    pub evidence: Value,
    pub questions: Vec<DecisionQuestion>,
}

/// What a provider returns: identity, one entry per asked question, nothing
/// else.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DecisionResult {
    #[serde(rename = "providerId")]
    pub provider_id: String,
    pub model: String,
    pub decisions: Vec<Decision>,
    pub refused: Vec<DecisionRefusal>,
}

/// The deterministic answer, carried explicitly.
///
/// The orchestrator never invents a fallback. A caller hands it the answer
/// Foundation would have given with no provider at all — from code, or from the
/// structured source — and every failure path returns that same value. A
/// default baseline inside this package would be a second opinion wearing the
/// clothes of the first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BaselineAnswer {
    pub task: DecisionTask,
    pub answer: Value,
    /// Where the deterministic answer came from. Audit trail, not logic.
    pub source: String,
}

/// Whether `value` is an answer this task may return. Runtime, not type-pretty:
/// a provider's reply arrives as parsed JSON at a boundary no type protects,
/// and a `choice` answer naming an option nobody listed is how a closed answer
/// set stops being closed.
pub fn task_answer_problem(task: DecisionTask, value: &Value) -> Option<String> {
    let definition = definition_for(task);
    match definition.kind {
        DecisionKind::Boolean => {
            if value.is_boolean() {
                None
            } else {
                Some(format!("answer for {task} must be a boolean"))
            }
        }
        DecisionKind::Choice => {
            let Some(answer) = value.as_str() else {
                return Some(format!("answer for {task} must be one of its options"));
            };
            if definition.options.contains(&answer) {
                None
            } else {
                Some(format!("answer for {task} is not in its answer set"))
            }
        }
        DecisionKind::Score => {
            let Some(answer) = value.as_f64() else {
                return Some(format!("answer for {task} must be a finite number"));
            };
            let (low, high) = definition.bounds;
            if answer < low || answer > high {
                return Some(format!("answer for {task} is outside its range"));
            }
            None
        }
    }
}

/// Whether a baseline is usable at all, before any provider is consulted.
pub fn baseline_problem(baseline: &BaselineAnswer) -> Option<String> {
    let problem = task_answer_problem(baseline.task, &baseline.answer);
    if let Some(problem) = problem {
        return Some(problem);
    }
    if baseline.source.trim().is_empty() {
        return Some(format!("baseline for {} needs a source", baseline.task));
    }
    None
}
