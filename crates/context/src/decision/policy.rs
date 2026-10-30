//! Task policy: the rules that decide whether a provider's answer may replace
//! the deterministic one. Port of `packages/decision/src/policy.ts`.
//!
//! Two questions live here, and they are asked separately because they fail
//! separately. A provider can be confident and still not be permitted, and it
//! can be permitted and still not confident enough. Collapsing them into one
//! number is how a "confidence" becomes a permission.
//!
//! There is deliberately no global threshold. A single number deciding every
//! task would be right for none of them: the cost of a wrong routing answer — a
//! stale price read from a chunk — is not the cost of a wrong handoff flag,
//! which is that a human reads one extra turn. Each task states its own floor
//! and its own reason, so the reason is reviewable and the floor can be moved
//! for a stated one.

use serde::{Deserialize, Serialize};

/// The three modes.
///
/// `off` makes no provider call at all: it is the absence of the feature, and
/// it must not be able to cost a network round trip. `shadow` calls the
/// provider and returns the baseline as effective — it exists so a provider can
/// be measured against Foundation before it is allowed to change anything.
/// `assist` may apply a provider's answer, under the rules below.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistMode {
    Off,
    Shadow,
    Assist,
}

impl AssistMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AssistMode::Off => "off",
            AssistMode::Shadow => "shadow",
            AssistMode::Assist => "assist",
        }
    }

    pub const ALL_STR: &'static [&'static str] = &["off", "shadow", "assist"];

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL_STR
            .iter()
            .position(|name| *name == value)
            .map(|index| [AssistMode::Off, AssistMode::Shadow, AssistMode::Assist][index])
    }
}

/// What a disagreement means for one task.
///
/// - `allow`: the candidate may replace the baseline in either direction. Only
///   for an answer that is advisory and never on a critical path.
/// - `escalate`: the candidate may replace the baseline only when it is an
///   escalation — equal or higher on the task's ladder. The safe direction for
///   this task is one-way.
/// - `refuse`: the candidate may never replace the baseline, so the task can be
///   evaluated and traced while still being held at Foundation behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisagreementPolicy {
    Allow,
    Escalate,
    Refuse,
}

/// The policy of one task: its own floor, its own direction, its own reason.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TaskPolicy {
    /// The confidence the task itself requires, in [0, 1].
    pub minimum_confidence: f64,
    pub disagreement: DisagreementPolicy,
}

impl TaskPolicy {
    /// Why this task has this policy, in the terms §17 or §3 would use.
    pub fn why(self, task: DecisionTask) -> &'static str {
        task_policy_why(task)
    }
}

use super::tasks::DecisionTask;

pub const TASK_POLICIES: &[(&str, TaskPolicy)] = &[
    (
        "intent_classification",
        TaskPolicy {
            minimum_confidence: 0.7,
            disagreement: DisagreementPolicy::Allow,
        },
    ),
    (
        "knowledge_routing",
        TaskPolicy {
            minimum_confidence: 0.75,
            disagreement: DisagreementPolicy::Escalate,
        },
    ),
    (
        "clarification",
        TaskPolicy {
            minimum_confidence: 0.6,
            disagreement: DisagreementPolicy::Escalate,
        },
    ),
    (
        "handoff_recommendation",
        TaskPolicy {
            minimum_confidence: 0.55,
            disagreement: DisagreementPolicy::Escalate,
        },
    ),
    (
        "evidence_sufficiency",
        TaskPolicy {
            minimum_confidence: 0.6,
            disagreement: DisagreementPolicy::Allow,
        },
    ),
];

pub fn task_policy(task: DecisionTask) -> TaskPolicy {
    TASK_POLICIES
        .iter()
        .find(|(name, _)| *name == task.as_str())
        .map(|(_, policy)| *policy)
        .expect("the policy table covers every task")
}

/// The `why` field of the baseline's table, keyed by task — carried as a
/// separate constant because a &'static per-enum match reads better than a
/// third table column, and the tests pin them anyway.
pub fn task_policy_why(task: DecisionTask) -> &'static str {
    match task {
        DecisionTask::IntentClassification => {
            "This answer shapes the posture of a reply and nothing else, so a disagreement is \
worth an audit line rather than a refusal. It cannot open an action: the \
permitted set comes from ActionPolicy, and a posture that asked for one would \
still be refused by it. The floor is high because a posture applied to the \
wrong turn is a turn that reads as unserious."
        }
        DecisionTask::KnowledgeRouting => {
            "The one task where the direction of a disagreement is a correctness question. §17: \
a live-value turn is answered from the live system or not at all, so a provider \
that would route a structured-truth turn to retrieval is asking the assistant \
to quote a stale price under a citation. The other direction is allowed and \
useful — an ambiguous utterance may be escalated to an identified \
structured-truth subject, which is strictly safer than guessing from published \
copy. The floor is the highest of the five because a source change outranks a \
wording change."
        }
        DecisionTask::Clarification => {
            "The safe direction is the one that costs a round trip. A provider may raise the \
flag — if it is not certain this should be asked, Foundation did not ask — but \
it may not lower one Foundation raised, because suppressing the clarifying \
question is exactly how a guess gets delivered as an answer. The floor is low: \
an unnecessary question costs one turn, and a wrong answer costs the trust."
        }
        DecisionTask::HandoffRecommendation => {
            "The flag tells a human that a turn happened; that is the whole of its power. \
Raising it is cheap — a human reads a line and moves on — and lowering it is the \
failure that cannot be undone, because nobody learns the turn was dropped. The \
floor is the lowest of the five because the cost is asymmetric, not because \
detection is easy."
        }
        DecisionTask::EvidenceSufficiency => {
            "A coverage estimate is advisory in both directions, and the safe direction is the \
lower one: a provider lowering the score makes the caller more cautious, which \
is the direction Foundation would rather be wrong in. Allowing the raise needs \
a boundary elsewhere, and it has one — the score is a fact about the supplied \
projected facts, never about the world, and §17's source rules run \
independently of it. Nothing grants a permission or confirms an action on the \
strength of this number."
        }
    }
}

/// Whether a declared policy is usable at all. Asserted by the policy tests.
pub fn task_policy_problem(task: DecisionTask) -> Option<String> {
    let policy = task_policy(task);
    let confidence = policy.minimum_confidence;
    if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        return Some(format!(
            "policy for {task} needs a confidence floor inside [0, 1]"
        ));
    }
    if task_policy_why(task).trim().is_empty() {
        return Some(format!("policy for {task} needs a reason"));
    }
    None
}

/// Confidence alone: does it clear the floor this task set for itself?
pub fn confidence_problem(policy: &TaskPolicy, confidence: f64) -> Option<String> {
    if !confidence.is_finite() {
        return Some("a provider answered with a confidence that is not a number".to_string());
    }
    if !(0.0..=1.0).contains(&confidence) {
        return Some("a provider answered with a confidence outside [0, 1]".to_string());
    }
    if confidence >= policy.minimum_confidence {
        None
    } else {
        Some("below the floor this task set for itself".to_string())
    }
}

/// Where an answer sits on the ladder escalation is measured against.
///
/// Total, and free of casts and coercion: anything that is not an answer this
/// task may return ranks below every real answer, because a malformed value
/// cannot read as an escalation. The ranking is a property of the declared
/// answer set — the order `options` was written in — and not of the provider,
/// so no provider can redefine what "escalating" means.
pub fn answer_rank(task: DecisionTask, answer: &serde_json::Value) -> i64 {
    let definition = super::tasks::definition_for(task);
    match definition.kind {
        super::tasks::DecisionKind::Boolean => match answer.as_bool() {
            Some(true) => 1,
            Some(false) => 0,
            None => -1,
        },
        super::tasks::DecisionKind::Score => answer
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| (value * 1_000_000.0) as i64)
            .unwrap_or(-1),
        super::tasks::DecisionKind::Choice => definition
            .options
            .iter()
            .position(|option| Some(*option) == answer.as_str())
            .map(|position| position as i64)
            .unwrap_or(-1),
    }
}

/// How a candidate compares with the baseline, on the task's own ladder.
///
/// Equality is by value, so a choice answer that names the same option is an
/// agreement regardless of confidence, and a boolean that repeats the baseline
/// is an agreement too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Agreement {
    Equal,
    Escalated,
    Downgraded,
}

impl Agreement {
    pub fn as_str(self) -> &'static str {
        match self {
            Agreement::Equal => "equal",
            Agreement::Escalated => "escalated",
            Agreement::Downgraded => "downgraded",
        }
    }
}

pub fn agreement_of(
    task: DecisionTask,
    baseline: &serde_json::Value,
    candidate: &serde_json::Value,
) -> Agreement {
    let from = answer_rank(task, baseline);
    let to = answer_rank(task, candidate);
    if to == from {
        return Agreement::Equal;
    }
    if to > from {
        Agreement::Escalated
    } else {
        Agreement::Downgraded
    }
}

/// Disagreement alone: does this task's policy permit the move?
///
/// Asked only after validation, so a candidate here is already a value the task
/// may return. `refuse` answers the same way for every candidate, which is the
/// point of having it.
pub fn disagreement_problem(
    task: DecisionTask,
    policy: &TaskPolicy,
    baseline: &serde_json::Value,
    candidate: &serde_json::Value,
) -> Option<String> {
    if policy.disagreement == DisagreementPolicy::Refuse {
        return Some(format!("{task} holds its deterministic answer"));
    }
    if policy.disagreement == DisagreementPolicy::Allow {
        return None;
    }
    if agreement_of(task, baseline, candidate) == Agreement::Downgraded {
        return Some(format!(
            "{task} does not descend: a provider may not lower what Foundation answered"
        ));
    }
    None
}
