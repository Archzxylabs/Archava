//! The bounded decision catalogue — port of
//! `packages/decision/src/{tasks,policy,vocabulary,rule-provider,ranking,
//! validate,orchestrator}.ts` and the turn seam
//! `packages/assistant/src/decision.ts`.
//!
//! Submodule map:
//!
//! | Rust | TypeScript source |
//! | --- | --- |
//! | [`tasks`] | `packages/decision/src/tasks.ts` |
//! | [`policy`] | `packages/decision/src/policy.ts` |
//! | [`vocabulary`] | `packages/decision/src/vocabulary.ts` |
//! | [`provider`] | `packages/decision/src/rule-provider.ts` |
//! | [`orchestrator`] | `packages/decision/src/{validate,orchestrator}.ts`,
//!   `ranking.ts` inlined (38 lines, no runtime task uses it) |
//! | [`turn`] | `packages/assistant/src/decision.ts` |
//!
//! One rule carries the whole layer: a provider is asked *questions* with
//! closed answer sets, and the orchestrator decides *whether an answer is
//! allowed to matter*. Nothing here can ask a provider for prose, and nothing
//! here can hand a provider an instruction.

pub mod orchestrator;
pub mod policy;
pub mod provider;
pub mod tasks;
pub mod turn;
pub mod vocabulary;

pub use orchestrator::{
    check_decision_response, question_shape_problem, request_problem, run_decision_turn,
    AnswerSource, CandidateDecision, CheckedDecision, CheckedResponse, DecisionError,
    DecisionProvider, DecisionRun, DecisionTrace, DecisionTurn, DefaultOrchestrator,
    EffectiveDecision, NowFn, OnTraceFn, ProviderIdentity, TraceOutcome,
};
pub use policy::{
    agreement_of, answer_rank, confidence_problem, disagreement_problem, task_policy,
    task_policy_problem, task_policy_why, Agreement, AssistMode, DisagreementPolicy, TaskPolicy,
    TASK_POLICIES,
};
pub use provider::{
    confidence_for, report_from_cues, rule_report_problem, score_coverage, RuleDecisionProvider,
    RuleReport, ThrowingProvider,
};
pub use tasks::{
    baseline_problem, decision_question_for, definition_for, task_answer_problem, BaselineAnswer,
    Decision, DecisionKind, DecisionQuestion, DecisionRefusal, DecisionRequest, DecisionResult,
    DecisionTask, TaskDefinition, DECISION_TASKS, DECISION_TASK_DEFINITIONS,
};
pub use turn::{
    decision_evidence, decision_utterance, evaluate_decision_turn, DecisionTurnInput,
    DecisionTurnResult,
};
pub use vocabulary::{mentions, normalize_cue, normalize_utterance};
