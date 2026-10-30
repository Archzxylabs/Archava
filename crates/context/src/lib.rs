//! Visitor-assistant turn context — knowledge, consent, masking, decisions.
//!
//! The R09 slice that lets a provider-driven visitor turn be BUILT instead of
//! guessed: the context assembly, the retrieval split, the consent gate and the
//! masking boundary, plus the closed decision catalogue a turn's bounded
//! judgment reads.
//!
//! | Rust | TypeScript source |
//! | --- | --- |
//! | [`knowledge`] | `packages/knowledge/src/{schema,structured-truth,text,store,retrieval}.ts` |
//! | [`consent`] | `packages/sdk/src/consent.ts` |
//! | [`mask`] | `packages/acl/src/mask.ts` |
//! | [`decision`] | `packages/decision/src/{tasks,policy,vocabulary,rule-provider,validate,orchestrator}.ts` + `packages/assistant/src/decision.ts` |
//! | [`turn_context`] | `packages/assistant/src/turn.ts` (projection + boundary) |
//!
//! The same three rules hold here as in every slice before it. Values come in
//! untrusted (an SDK record, a decrementing config, a provider's parsed reply)
//! and the modules that carry them validate at the boundary, not at each call
//! site. Fail-closed is a decision, not a default. And nothing in this crate
//! reads the clock, the network or the filesystem — the turn that reads either
//! cannot be replayed.

pub mod consent;
pub mod decision;
pub mod knowledge;
pub mod mask;
pub mod turn_context;

pub use consent::{
    grant_consent, has_consent, missing_consent, parse_consent, revoke_consent, ConsentDomain,
    ConsentState, CONSENT_DOMAINS, DENIED_CONSENT,
};
pub use decision::vocabulary::{cues_for, CueRow, CueSet};
pub use decision::{
    agreement_of, answer_rank, baseline_problem, check_decision_response, confidence_for,
    confidence_problem, decision_evidence, decision_question_for, decision_utterance,
    definition_for, disagreement_problem, evaluate_decision_turn, mentions, normalize_cue,
    normalize_utterance, question_shape_problem, report_from_cues, request_problem,
    rule_report_problem, run_decision_turn, score_coverage, task_answer_problem, task_policy,
    task_policy_problem, task_policy_why, Agreement, AnswerSource, AssistMode, BaselineAnswer,
    CandidateDecision, CheckedDecision, CheckedResponse, Decision, DecisionError, DecisionKind,
    DecisionProvider, DecisionQuestion, DecisionRefusal, DecisionRequest, DecisionResult,
    DecisionRun, DecisionTask, DecisionTrace, DecisionTurn, DecisionTurnInput, DecisionTurnResult,
    DefaultOrchestrator, DisagreementPolicy, EffectiveDecision, NowFn, OnTraceFn, ProviderIdentity,
    RuleDecisionProvider, RuleReport, TaskDefinition, TaskPolicy, TraceOutcome, DECISION_TASKS,
    DECISION_TASK_DEFINITIONS, TASK_POLICIES,
};
pub use knowledge::retrieval::{
    parse_retrieval_query, plan_retrieval, retrieve_context, to_context_chunks,
    KnowledgeContextChunk, RetrievalContextRequest, RetrievalOutcome, RetrievalPlan,
};
pub use knowledge::schema::{
    Chunk as KnowledgeChunk, Document as KnowledgeDocument, KnowledgeKind, Provenance,
    RetrievalQuery, KNOWLEDGE_KINDS,
};
pub use knowledge::store::{KnowledgeStore, KnowledgeStoreStats, ScoredChunk, SearchOptions};
pub use knowledge::structured_truth::{
    attribute, classify_knowledge_need, requires_structured_truth, AnswerBasis, AttributedAnswer,
    KnowledgeClassification, KnowledgeNeed, StructuredTruthSubject, STRUCTURED_TRUTH_SUBJECTS,
};
pub use knowledge::{
    KnowledgeIsolationError, KnowledgeRetrievalError, KnowledgeSchemaError, KnowledgeTenantError,
};
pub use mask::{
    mask_sensitive_fields, mask_string, resolve_mask_rules, MaskOptions, MaskRule, MaskStrategy,
    MaskedResult, REDACTED_LABEL,
};
pub use turn_context::{
    enabled_page_actions, mask_at_boundary, project_context_graph, ProjectedGraph,
};
