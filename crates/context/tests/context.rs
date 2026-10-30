//! P-ASSISTANT-CONTEXT witnesses.
//!
//! One file per the port contract: the producer (`tools/rust/assistant-context.mjs`)
//! runs `cargo test -p archava-context --test context`, greps `test <name> ... ok`
//! by name, and the manifest (`docs/archive/rust/required-coverage.json`) lists the same
//! names — a name on one side and not the other is drift, and drift is an error.
//!
//! Three disciplines carried over from the slices before this one:
//!
//! 1. Assertions are exact. Reason strings, refusal texts and notice formats are
//!    quoted from the TypeScript sources, because a paraphrase passes a test and
//!    fails a reader.
//! 2. The provider seam is a stub (`StaticProvider` / `CapturingProvider` below):
//!    the orchestrator's judgment witnesses need a provider whose verdict is
//!    chosen by the test, not by cue tables. The rule provider's own witnesses
//!    drive the real tables, so both sides of the seam get exercised.
//! 3. Only the deadline witness sleeps, and only because a provider that "takes
//!    60 ms" against a one-millisecond deadline is the deterministic reading of
//!    "the timer fires first".

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use archava_context::{
    agreement_of, baseline_problem, check_decision_response, classify_knowledge_need,
    confidence_for, confidence_problem, decision_evidence, decision_question_for,
    decision_utterance, definition_for, disagreement_problem, enabled_page_actions,
    evaluate_decision_turn, grant_consent, has_consent, mask_at_boundary, mask_sensitive_fields,
    mask_string, mentions, missing_consent, normalize_cue, normalize_utterance, parse_consent,
    parse_retrieval_query, project_context_graph, request_problem, requires_structured_truth,
    resolve_mask_rules, retrieve_context, revoke_consent, rule_report_problem, run_decision_turn,
    score_coverage, task_answer_problem, task_policy, task_policy_problem, task_policy_why,
    to_context_chunks, Agreement, AnswerSource, AssistMode, BaselineAnswer, CheckedResponse,
    Decision, DecisionKind, DecisionProvider, DecisionQuestion, DecisionRefusal, DecisionRequest,
    DecisionResult, DecisionTask, DecisionTurnInput, DefaultOrchestrator, DisagreementPolicy,
    KnowledgeClassification, KnowledgeContextChunk, KnowledgeKind, KnowledgeNeed, KnowledgeStore,
    KnowledgeStoreStats, MaskOptions, RetrievalContextRequest, RuleReport, SearchOptions,
    StructuredTruthSubject, TraceOutcome, DECISION_TASKS, DECISION_TASK_DEFINITIONS,
    REDACTED_LABEL, TASK_POLICIES,
};
use archava_core::graph::{parse_context_graph, ContextGraph};
use archava_core::tenant::TenantScopeError;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Shared builders
// ---------------------------------------------------------------------------

const STUB_ID: &str = "stub-provider";
const STUB_MODEL: &str = "static-v1";
const ACME: &str = "acme";
const BEACH: &str = "beach-hotel";

/// A canned provider: the reply is whatever the test staged, and the failure and
/// latency paths are explicit flags rather than side effects.
struct StaticProvider {
    reply: Option<DecisionResult>,
    sleeping: bool,
    failing: bool,
    ready: bool,
}

impl StaticProvider {
    fn with_reply(reply: DecisionResult) -> Self {
        StaticProvider {
            reply: Some(reply),
            sleeping: false,
            failing: false,
            ready: true,
        }
    }
    fn asleep(reply: DecisionResult) -> Self {
        StaticProvider {
            reply: Some(reply),
            sleeping: true,
            failing: false,
            ready: true,
        }
    }
    fn failing() -> Self {
        StaticProvider {
            reply: None,
            sleeping: false,
            failing: true,
            ready: true,
        }
    }
    fn unready(reply: DecisionResult) -> Self {
        StaticProvider {
            reply: Some(reply),
            sleeping: false,
            failing: false,
            ready: false,
        }
    }
}

/// Observes what the turn actually handed the provider — the witness for
/// "the redaction happens before the provider sees anything". The cell is
/// shared through an `Rc` so the copy the orchestrator owns writes where the
/// test reads.
#[derive(Clone)]
struct CapturingProvider {
    captured: Rc<RefCell<Option<DecisionRequest>>>,
}

impl DecisionProvider for StaticProvider {
    fn identity(&self) -> (String, String) {
        (STUB_ID.to_string(), STUB_MODEL.to_string())
    }

    fn ready(&self) -> bool {
        self.ready
    }

    fn decide(&self, _request: &DecisionRequest) -> Result<DecisionResult, String> {
        if self.sleeping {
            std::thread::sleep(Duration::from_millis(60));
        }
        if self.failing {
            // Deliberately credential-shaped: the trace must never repeat it.
            return Err("auth exploded: sk_live_SECRET_NUMBER_9".to_string());
        }
        Ok(self.reply.clone().expect("this test stages a reply"))
    }
}

impl DecisionProvider for CapturingProvider {
    fn identity(&self) -> (String, String) {
        ("capturing".to_string(), "spy-v1".to_string())
    }

    fn ready(&self) -> bool {
        true
    }

    fn decide(&self, request: &DecisionRequest) -> Result<DecisionResult, String> {
        *self.captured.borrow_mut() = Some(request.clone());
        Err("the capture was the point".to_string())
    }
}

fn answered(task: DecisionTask, answer: Value, confidence: f64) -> Decision {
    Decision {
        id: task,
        kind: definition_for(task).kind,
        answer,
        confidence,
    }
}

fn refusal(task: DecisionTask, reason: &str) -> DecisionRefusal {
    DecisionRefusal {
        id: task,
        reason: reason.to_string(),
    }
}

fn reply_of(decisions: Vec<Decision>, refused: Vec<DecisionRefusal>) -> DecisionResult {
    DecisionResult {
        provider_id: STUB_ID.to_string(),
        model: STUB_MODEL.to_string(),
        decisions,
        refused,
    }
}

fn baseline(task: DecisionTask, answer: Value, source: &str) -> BaselineAnswer {
    BaselineAnswer {
        task,
        answer,
        source: source.to_string(),
    }
}

fn one_question(task: DecisionTask) -> DecisionQuestion {
    decision_question_for(task)
}

/// Built the way `parse_knowledge_document` reads it: required strings, a kind
/// from the vocabulary, and no provenance block (it is validated when given and
/// derived at ingest regardless).
fn document_json(tenant: &str, id: &str, title: &str, content: &str) -> Value {
    json!({
        "id": id,
        "tenantId": tenant,
        "kind": "faq",
        "title": title,
        "content": content,
        "updatedAt": "2026-01-15",
    })
}

fn ingest_faq(store: &mut KnowledgeStore, tenant: &str, id: &str, title: &str, content: &str) {
    store
        .ingest(&document_json(tenant, id, title, content))
        .expect("the fixture document parses");
}

fn grounding_chunk(id: &str, text: &str) -> KnowledgeContextChunk {
    KnowledgeContextChunk {
        source_id: id.to_string(),
        source_title: "Fixture".to_string(),
        source_kind: KnowledgeKind::Faq,
        text: text.to_string(),
    }
}

/// A minimal context graph: page, session, no entities, actions on a switch.
fn graph_session(actions: &[(&str, bool)]) -> ContextGraph {
    let raw = json!({
        "tenantId": "acme",
        "page": { "route": "/rooms/deluxe", "kind": "room", "locale": "en" },
        "session": { "authenticated": true, "role": "guest", "customerRef": "cus_12345" },
        "entities": [],
        "availableActions": actions
            .iter()
            .map(|(name, enabled)| json!({ "name": name, "enabled": enabled }))
            .collect::<Vec<Value>>(),
    });
    parse_context_graph(&raw).expect("the fixture graph parses")
}

// ---------------------------------------------------------------------------
// Knowledge classification (packages/knowledge structured-truth §17)
// ---------------------------------------------------------------------------

#[test]
fn classification_keeps_live_values_in_the_live_system() {
    let classification = classify_knowledge_need("berapa harga kamar deluxe?");
    assert_eq!(classification.need, KnowledgeNeed::StructuredTruth);
    assert_eq!(classification.subjects, vec![StructuredTruthSubject::Price]);
    assert!(requires_structured_truth("how much is the deposit"));
    assert!(!requires_structured_truth("what time is breakfast served"));
}

#[test]
fn classification_reads_a_possessive_as_customer_data() {
    // The keyword branch outranks the possessive one: "booking" names a live
    // subject, so the possessive marker never gets read here.
    let booking = classify_knowledge_need("where is my booking?");
    assert_eq!(booking.need, KnowledgeNeed::StructuredTruth);
    assert_eq!(
        booking.subjects,
        vec![StructuredTruthSubject::BookingStatus]
    );
    assert_eq!(booking.matched, vec!["booking_status".to_string()]);

    // The possessive branch answers live-data questions whose nouns carry
    // no keyword of their own.
    let mine = classify_knowledge_need("where is my answer ready?");
    assert_eq!(mine.need, KnowledgeNeed::StructuredTruth);
    assert_eq!(mine.subjects, vec![StructuredTruthSubject::CustomerOrOrder]);
    assert_eq!(mine.matched, vec!["possessive".to_string()]);
}

#[test]
fn classification_leaves_published_copy_to_retrieval() {
    let classification = classify_knowledge_need("what time is breakfast served?");
    assert_eq!(classification.need, KnowledgeNeed::RetrievalKnowledge);
    assert!(classification.subjects.is_empty());
    assert!(classification.matched.is_empty());
}

// ---------------------------------------------------------------------------
// Knowledge store (§23 tenant scope)
// ---------------------------------------------------------------------------

#[test]
fn store_document_refuses_unknown_keys_and_reads_known_ones() {
    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        BEACH,
        "rental-terms",
        "Rental terms",
        "Deposits are refundable within seven days.",
    );

    let read = store
        .document(BEACH, "rental-terms")
        .expect("the record exists");
    assert_eq!(read.id, "rental-terms");
    assert_eq!(read.kind, KnowledgeKind::Faq);

    let missing = store.document(ACME, "rental-terms").unwrap_err();
    assert!(
        matches!(missing, TenantScopeError::Missing { expected } if expected == ACME),
        "\"not found\" and \"not yours\" are different facts, and a typo is still an absence"
    );
}

#[test]
fn store_chunks_refuse_cross_tenant_ids_with_distinct_facts() {
    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        BEACH,
        "rental-terms",
        "Rental terms",
        "Deposits are refundable within seven days.",
    );

    let foreign = store.chunk(ACME, "rental-terms.c0").unwrap_err();
    assert_eq!(
        foreign.to_string(),
        format!("Tenant scope violation: chunk \"rental-terms.c0\" belongs to tenant \"{BEACH}\", not \"{ACME}\".")
    );
    let missing = store.chunk(ACME, "nope.c0").unwrap_err();
    assert_eq!(
        missing.to_string(),
        format!("Chunk \"nope.c0\" does not exist in tenant \"{ACME}\".")
    );
}

#[test]
fn store_search_names_unknown_tenants() {
    let empty = KnowledgeStore::default();
    let refused = empty
        .search("harga", ACME, 4, &SearchOptions::default())
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        format!("Knowledge store is empty; no tenant \"{ACME}\" exists.")
    );

    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        ACME,
        "rental-terms",
        "Rental terms",
        "Deposits are refundable within seven days.",
    );
    let refused = store
        .search("harga", BEACH, 4, &SearchOptions::default())
        .unwrap_err();
    assert_eq!(
        refused.to_string(),
        format!("No knowledge documents for tenant \"{BEACH}\". Registered tenants: {ACME}.")
    );
}

#[test]
fn store_search_never_reads_another_partition() {
    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        ACME,
        "acme-terms",
        "Acme terms",
        "Harga deposits are refundable within seven days.",
    );
    ingest_faq(
        &mut store,
        BEACH,
        "beach-terms",
        "Beach terms",
        "Harga deposits are refundable within seven days.",
    );

    let hits = store
        .search("harga", ACME, 5, &SearchOptions::default())
        .expect("acme is a known tenant");
    assert!(!hits.is_empty(), "the seeded corpus mentions harga");
    assert!(
        hits.iter().all(|hit| hit.tenant_id == ACME),
        "beach's equal-text chunks must never appear in acme's read"
    );
}

#[test]
fn store_stats_count_the_tenant_they_name() {
    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        ACME,
        "acme-terms",
        "Acme terms",
        "One line. Two lines here.",
    );
    let stats: KnowledgeStoreStats = store.stats(ACME).expect("a known tenant");
    assert_eq!(stats.documents, 1);
    assert!(
        stats.chunks >= 1,
        "seeded content lands as at least one chunk"
    );
}

// ---------------------------------------------------------------------------
// Retrieval (plan before store, §17 + §23)
// ---------------------------------------------------------------------------

#[test]
fn retrieval_plan_puts_price_behind_structured_truth() {
    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        ACME,
        "rental-terms",
        "Rental terms",
        "The nightly harga is published on the room page.",
    );
    let request = RetrievalContextRequest {
        question: "berapa harga kamar deluxe?".to_string(),
        tenant_id: ACME.to_string(),
        limit: None,
        kinds: None,
    };
    let outcome = retrieve_context(&request, &store).expect("a parsed request over an open tenant");
    assert_eq!(outcome.plan.need, KnowledgeNeed::StructuredTruth);
    assert_eq!(outcome.plan.subjects, vec![StructuredTruthSubject::Price]);
    assert!(outcome.defer_to_structured_truth);
    assert!(
        !outcome.context.is_empty(),
        "retrieval still supplies supporting copy"
    );
}

#[test]
fn retrieval_limit_defaults_to_eight() {
    let request = RetrievalContextRequest {
        question: "what time is breakfast?".to_string(),
        tenant_id: ACME.to_string(),
        limit: None,
        kinds: None,
    };
    let parsed = parse_retrieval_query(&request).expect("well-formed");
    assert_eq!(
        parsed.limit, 8,
        "the default is eight chunks, not the caller's infinity"
    );
}

#[test]
fn retrieval_chunks_carry_provenance_not_scores() {
    let mut store = KnowledgeStore::default();
    ingest_faq(
        &mut store,
        ACME,
        "acme-terms",
        "Acme terms",
        "Harga deposits are refundable within seven days.",
    );
    let request = RetrievalContextRequest {
        question: "harga".to_string(),
        tenant_id: ACME.to_string(),
        limit: Some(2),
        kinds: None,
    };
    let outcome = retrieve_context(&request, &store).expect("a parsed request over an open tenant");
    let chunk = outcome
        .context
        .first()
        .expect("the seeded corpus answers harga");
    assert_eq!(chunk.source_id, "acme-terms");
    assert_eq!(chunk.source_kind, KnowledgeKind::Faq);
    assert!(!chunk.source_title.is_empty());
    assert!(!chunk.text.is_empty());
    assert!(to_context_chunks(&[]).is_empty());
}

// ---------------------------------------------------------------------------
// Consent (packages/sdk/src/consent.ts)
// ---------------------------------------------------------------------------

#[test]
fn consent_gate_reads_only_explicit_true() {
    assert_eq!(parse_consent(None), archava_context::DENIED_CONSENT);
    assert_eq!(
        parse_consent(Some(&json!({ "identity": "true", "page_context": 1 }))),
        archava_context::DENIED_CONSENT,
        "strings and numbers are refusals at the gate, not coercions"
    );
    let granted = parse_consent(Some(&json!({ "identity": true })));
    assert!(has_consent(
        granted,
        archava_context::ConsentDomain::Identity
    ));
    assert!(!has_consent(
        granted,
        archava_context::ConsentDomain::PageContext
    ));
}

#[test]
fn consent_withdrawal_takes_effect_on_the_next_turn() {
    let granted = grant_consent(
        archava_context::DENIED_CONSENT,
        archava_context::ConsentDomain::PageContext,
    );
    assert!(has_consent(
        granted,
        archava_context::ConsentDomain::PageContext
    ));
    let revoked = revoke_consent(granted, archava_context::ConsentDomain::PageContext);
    assert!(!has_consent(
        revoked,
        archava_context::ConsentDomain::PageContext
    ));
    assert_eq!(
        missing_consent(revoked, archava_context::CONSENT_DOMAINS),
        vec![
            archava_context::ConsentDomain::PageContext,
            archava_context::ConsentDomain::Identity,
            archava_context::ConsentDomain::Analytics,
        ],
        "starting from a denied state, a withdrawn domain is one of three absences"
    );
}

#[test]
fn consent_missing_is_reported_not_thrown() {
    let state = parse_consent(Some(&json!({ "analytics": true })));
    assert_eq!(
        missing_consent(state, archava_context::CONSENT_DOMAINS),
        vec![
            archava_context::ConsentDomain::PageContext,
            archava_context::ConsentDomain::Identity,
        ],
        "declared order, not a set's accident"
    );
}

// ---------------------------------------------------------------------------
// Masking (§16 — packages/acl/src/mask.ts)
// ---------------------------------------------------------------------------

#[test]
fn mask_walk_redacts_structured_values_over_classified_names() {
    let masked = mask_sensitive_fields(
        &json!({ "customerEmail": { "raw": "person@example.com" } }),
        &MaskOptions::default(),
    );
    assert_eq!(masked.value["customerEmail"], json!(REDACTED_LABEL));
    assert_eq!(
        masked.notices,
        vec!["$.customerEmail withheld before provider exposure (structured value)".to_string()]
    );
    assert!(masked.masked);
}

#[test]
fn mask_walk_classifies_fragments_whole_names_and_partials() {
    let masked = mask_sensitive_fields(
        &json!({
            "refreshToken": "tok_expired",
            "email": "person@example.com",
            "integrationTokensEnabled": true
        }),
        &MaskOptions::default(),
    );
    assert_eq!(masked.value["refreshToken"], json!(REDACTED_LABEL));
    assert_eq!(
        masked.value["email"],
        json!("pe••••••••••••"),
        "email is identifiable but usable: a partial, not a redaction"
    );
    assert_eq!(
        masked.value["integrationTokensEnabled"],
        json!(true),
        "a boolean flag is signal, not a credential"
    );
    let joined = masked.notices.join("\n");
    assert!(joined.contains("$.refreshToken redacted before provider exposure"));
    assert!(joined.contains("$.email partially masked (16 characters withheld)"));
}

#[test]
fn mask_walk_keeps_null_and_boolean_but_not_numbers() {
    let masked = mask_sensitive_fields(
        &json!({ "otp": null, "passphraseEnabled": false, "pin": 424242 }),
        &MaskOptions::default(),
    );
    assert_eq!(masked.value["otp"], Value::Null, "nothing to withhold");
    assert_eq!(masked.value["passphraseEnabled"], json!(false));
    assert_eq!(masked.value["pin"], json!(REDACTED_LABEL));
    assert_eq!(
        masked.notices,
        vec!["$.pin withheld before provider exposure (non-text value)".to_string()]
    );
}

#[test]
fn partial_masking_keeps_whole_characters_and_its_notice() {
    // "💩" is one character and two UTF-16 units: the baseline slices units,
    // the port slices to the whole-character superset — the emoji survives
    // intact either way, and the withheld count follows from what remains.
    let masked = mask_string("💩x@example.com", "email", &MaskOptions::default());
    assert_eq!(masked.value, json!("💩••••••••••••"));
    assert_eq!(
        masked.notices,
        vec!["$.email partially masked (13 characters withheld)".to_string()]
    );
    assert!(masked.masked);
}

#[test]
fn mask_string_defaults_to_redaction_for_unknown_fields() {
    let masked = mask_string(
        "load-bearing secret",
        "callStatusNote",
        &MaskOptions::default(),
    );
    assert_eq!(masked.value, json!(REDACTED_LABEL));
    assert_eq!(
        masked.notices,
        vec!["$.callStatusNote redacted before provider exposure".to_string()]
    );
}

#[test]
fn resolve_mask_rules_narrows_partials_and_redactions() {
    let rules = resolve_mask_rules(
        &["customerEmail".to_string(), "mvnoAccountPin".to_string()],
        &[],
    );
    let strategy_of = |field: &str| {
        rules
            .iter()
            .find(|rule| rule.field == field)
            .map(|rule| rule.strategy.as_str())
            .expect("every resolved source produced a rule")
    };
    assert_eq!(
        strategy_of("customerEmail"),
        "partial",
        "declared partial lands partial"
    );
    assert_eq!(strategy_of("mvnoAccountPin"), "redact");
    assert_eq!(
        strategy_of("email"),
        "partial",
        "the platform names list lands partial"
    );
    assert_eq!(strategy_of("customerphone"), "partial");
    assert_eq!(
        strategy_of("dateofbirth"),
        "redact",
        "a name is sensitive even without a fragment"
    );
    assert_eq!(strategy_of("rawform"), "redact");
}

// ---------------------------------------------------------------------------
// Mask at the turn boundary (packages/assistant/src/turn.ts)
// ---------------------------------------------------------------------------

#[test]
fn the_boundary_masks_client_fields_at_the_turn_seam() {
    let graph = graph_session(&[("order.status.read", true)]);
    let projected = mask_at_boundary(&graph, &["customerRef".to_string()]);
    assert_eq!(
        projected.value["session"]["customerRef"],
        json!(REDACTED_LABEL)
    );
    assert_eq!(
        projected.value["session"]["role"],
        json!("guest"),
        "unclassified neighbours survive"
    );
    assert_eq!(projected.value["session"]["authenticated"], json!(true));
    assert!(
        projected
            .notices
            .iter()
            .any(|notice| notice.contains("customerRef")),
        "every withheld field is reported, not silently dropped"
    );
}

#[test]
fn the_boundary_masks_enabled_action_fields() {
    let graph = graph_session(&[("order.status.read", true)]);
    let projected = mask_at_boundary(&graph, &[]);
    assert_eq!(
        projected.value,
        project_context_graph(&graph),
        "order.status.read declares customerEmail/customerPhone; the projection carries \
         neither name, so the declared rules must change nothing"
    );
    assert!(projected.notices.is_empty());
}

#[test]
fn the_projection_withholds_everything_but_shape() {
    let graph = graph_session(&[("order.status.read", true)]);
    let projection = project_context_graph(&graph);
    assert_eq!(projection["route"], json!("/rooms/deluxe"));
    assert_eq!(projection["pageKind"], json!("room"));
    assert_eq!(projection["section"], Value::Null);
    assert_eq!(projection["cart"], Value::Null);
    assert_eq!(projection["form"], Value::Null);
    assert_eq!(projection["checkout"], Value::Null);
    assert_eq!(
        projection["session"]["customerRef"],
        json!("cus_12345"),
        "the projection itself is unmasked; the boundary is the second line"
    );
}

#[test]
fn enabled_page_actions_are_the_enabled_ones() {
    let graph = graph_session(&[
        ("order.status.read", true),
        ("page.navigate", false),
        ("lead.capture", true),
    ]);
    assert_eq!(
        enabled_page_actions(&graph),
        vec!["order.status.read".to_string(), "lead.capture".to_string()]
    );
}

// ---------------------------------------------------------------------------
// Decision catalogue and policy (packages/decision/src/{tasks,policy}.ts)
// ---------------------------------------------------------------------------

#[test]
fn catalog_declares_the_five_tasks() {
    assert_eq!(
        DECISION_TASKS
            .iter()
            .map(|task| task.as_str())
            .collect::<Vec<_>>(),
        vec![
            "intent_classification",
            "knowledge_routing",
            "clarification",
            "handoff_recommendation",
            "evidence_sufficiency",
        ]
    );
}

#[test]
fn the_routing_ladder_ascends_toward_structured_truth() {
    let routing = DECISION_TASK_DEFINITIONS
        .iter()
        .find(|definition| definition.id == DecisionTask::KnowledgeRouting)
        .expect("the definitions table covers the five tasks");
    assert_eq!(routing.kind, DecisionKind::Choice);
    assert_eq!(
        routing.options,
        vec!["retrieval_knowledge", "structured_truth"],
        "retrieval is the floor and structured truth is above it — the option order IS the policy"
    );
}

#[test]
fn floors_and_directions_are_pinned_per_task() {
    let names: Vec<&str> = TASK_POLICIES.iter().map(|(name, _)| *name).collect();
    assert_eq!(
        names,
        vec![
            "intent_classification",
            "knowledge_routing",
            "clarification",
            "handoff_recommendation",
            "evidence_sufficiency",
        ]
    );
    let floors: Vec<f64> = TASK_POLICIES
        .iter()
        .map(|(_, policy)| policy.minimum_confidence)
        .collect();
    let expected = [0.7, 0.75, 0.6, 0.55, 0.6];
    for (got, want) in floors.iter().zip(expected.iter()) {
        assert!((got - want).abs() < 1e-9, "floor {} != {}", got, want);
    }
    let directions: Vec<DisagreementPolicy> = TASK_POLICIES
        .iter()
        .map(|(_, policy)| policy.disagreement)
        .collect();
    assert_eq!(
        directions,
        vec![
            DisagreementPolicy::Allow,
            DisagreementPolicy::Escalate,
            DisagreementPolicy::Escalate,
            DisagreementPolicy::Escalate,
            DisagreementPolicy::Allow,
        ]
    );
}

#[test]
fn every_policy_carries_a_reason_and_a_floor_in_range() {
    for task in DECISION_TASKS {
        assert_eq!(task_policy_problem(*task), None);
        assert!(
            !task_policy_why(*task).trim().is_empty(),
            "{task} has a reviewable reason"
        );
        assert_eq!(task_policy(*task).why(*task), task_policy_why(*task));
        let policy = task_policy(*task);
        assert!(policy.minimum_confidence.is_finite());
        assert!((0.0..=1.0).contains(&policy.minimum_confidence));
    }
}

#[test]
fn confidence_problem_reads_the_effective_floor() {
    let policy = archava_context::TaskPolicy {
        minimum_confidence: 0.7,
        disagreement: DisagreementPolicy::Allow,
    };
    assert_eq!(
        confidence_problem(&policy, f64::NAN),
        Some("a provider answered with a confidence that is not a number".to_string())
    );
    assert_eq!(
        confidence_problem(&policy, 1.4),
        Some("a provider answered with a confidence outside [0, 1]".to_string())
    );
    assert_eq!(
        confidence_problem(&policy, 0.65),
        Some("below the floor this task set for itself".to_string())
    );
    assert_eq!(confidence_problem(&policy, 0.7), None);
    // The orchestrator's effective floor is `max(catalogue, configured)`:
    // a tenant may tighten, never loosen.
    let tightened = archava_context::TaskPolicy {
        minimum_confidence: 0.9,
        disagreement: DisagreementPolicy::Allow,
    };
    assert_eq!(
        confidence_problem(&tightened, 0.85),
        Some("below the floor this task set for itself".to_string())
    );
}

#[test]
fn escalation_moves_only_up_the_declared_ladder() {
    let structured = json!("structured_truth");
    let retrieval = json!("retrieval_knowledge");
    assert_eq!(
        agreement_of(DecisionTask::KnowledgeRouting, &retrieval, &retrieval),
        Agreement::Equal
    );
    assert_eq!(
        agreement_of(DecisionTask::KnowledgeRouting, &retrieval, &structured),
        Agreement::Escalated
    );
    assert_eq!(
        agreement_of(DecisionTask::KnowledgeRouting, &structured, &retrieval),
        Agreement::Downgraded
    );
    // A malformed candidate cannot read as an escalation (rank -1 descends).
    assert_eq!(
        agreement_of(DecisionTask::KnowledgeRouting, &retrieval, &json!(42)),
        Agreement::Downgraded
    );
    // The intent ladder: pricing_inquiry sits below status_inquiry.
    assert_eq!(
        agreement_of(
            DecisionTask::IntentClassification,
            &json!("pricing_inquiry"),
            &json!("status_inquiry")
        ),
        Agreement::Escalated
    );
}

#[test]
fn disagreement_policy_blocks_only_the_unsafe_direction() {
    let task = DecisionTask::KnowledgeRouting;
    let escalate = archava_context::TaskPolicy {
        minimum_confidence: 0.75,
        disagreement: DisagreementPolicy::Escalate,
    };
    let structured = json!("structured_truth");
    let retrieval = json!("retrieval_knowledge");
    assert_eq!(
        disagreement_problem(task, &escalate, &retrieval, &structured),
        None
    );
    assert_eq!(
        disagreement_problem(task, &escalate, &structured, &retrieval),
        Some(format!(
            "{task} does not descend: a provider may not lower what Foundation answered"
        ))
    );
    let refuse = archava_context::TaskPolicy {
        minimum_confidence: 0.75,
        disagreement: DisagreementPolicy::Refuse,
    };
    assert_eq!(
        disagreement_problem(task, &refuse, &structured, &structured),
        Some(format!("{task} holds its deterministic answer")),
        "refuse is the same refusal for every candidate — that is the point of having it"
    );
    let allow = archava_context::TaskPolicy {
        minimum_confidence: 0.75,
        disagreement: DisagreementPolicy::Allow,
    };
    assert_eq!(
        disagreement_problem(task, &allow, &structured, &retrieval),
        None
    );
}

#[test]
fn a_baseline_answer_may_not_be_wrong_twice() {
    // The boundary narrows the question's answer set and the task's answer set
    // to the same closed set; a baseline misses it at exactly one place, and
    // the source must be readable regardless of the answer.
    assert_eq!(
        baseline_problem(&baseline(
            DecisionTask::KnowledgeRouting,
            json!("structured_truth"),
            "someone"
        )),
        None
    );
    assert_eq!(
        baseline_problem(&baseline(
            DecisionTask::KnowledgeRouting,
            json!("scorched_earth"),
            "someone"
        )),
        Some("answer for knowledge_routing is not in its answer set".to_string())
    );
    assert_eq!(
        baseline_problem(&baseline(
            DecisionTask::EvidenceSufficiency,
            json!(3.5),
            "someone"
        )),
        Some("answer for evidence_sufficiency is outside its range".to_string())
    );
    assert_eq!(
        baseline_problem(&baseline(
            DecisionTask::Clarification,
            json!("maybe"),
            "someone"
        )),
        Some("answer for clarification must be a boolean".to_string())
    );
    assert_eq!(
        baseline_problem(&baseline(
            DecisionTask::IntentClassification,
            json!("pricing_inquiry"),
            "   "
        )),
        Some("baseline for intent_classification needs a source".to_string())
    );
}

#[test]
fn task_answer_problem_covers_the_three_kinds() {
    assert_eq!(
        task_answer_problem(DecisionTask::EvidenceSufficiency, &json!(0.5)),
        None
    );
    assert_eq!(
        task_answer_problem(DecisionTask::EvidenceSufficiency, &json!(-0.1)),
        Some("answer for evidence_sufficiency is outside its range".to_string())
    );
    assert_eq!(
        task_answer_problem(DecisionTask::Clarification, &json!(1)),
        Some("answer for clarification must be a boolean".to_string())
    );
    assert_eq!(
        task_answer_problem(DecisionTask::IntentClassification, &json!(null)),
        Some("answer for intent_classification must be one of its options".to_string())
    );
    assert_eq!(
        task_answer_problem(DecisionTask::IntentClassification, &json!("not_an_option")),
        Some("answer for intent_classification is not in its answer set".to_string())
    );
}

// ---------------------------------------------------------------------------
// Rule provider (packages/decision/src/rule-provider.ts)
// ---------------------------------------------------------------------------

#[test]
fn the_rule_provider_counts_cues_never_asserts_certainty() {
    assert!((confidence_for(0) - 0.0).abs() < 1e-9);
    assert!((confidence_for(1) - 0.3).abs() < 1e-9);
    assert!((confidence_for(2) - 0.65).abs() < 1e-9);
    assert!((confidence_for(3) - 0.85).abs() < 1e-9);
    assert!(
        (confidence_for(9) - 0.85).abs() < 1e-9,
        "the count caps; no heuristic is certain"
    );
}

#[test]
fn coverage_scores_only_the_supplied_facts() {
    assert!(
        (score_coverage(&json!({})) - 0.0).abs() < 1e-9,
        "empty evidence scores 0, honestly"
    );
    assert!((score_coverage(&json!({ "page": 1 })) - 0.2).abs() < 1e-9);
    assert!(
        (score_coverage(&json!({ "page": 1, "cart": 1, "form": 1, "session": 1 })) - 0.8).abs()
            < 1e-9,
        "four of five named facts is the cap"
    );
    assert!(
        (score_coverage(&json!({
            "a": 1, "b": 1, "c": 1, "d": 1, "e": 1, "f": 1, "g": 1, "h": 1, "i": 1
        })) - 0.8)
            .abs()
            < 1e-9,
        "nine keys reach the same cap: coverage is a floor for caution, not a reward"
    );
    assert!((score_coverage(&json!(null)) - 0.0).abs() < 1e-9);
    assert!((score_coverage(&json!("not an object")) - 0.0).abs() < 1e-9);
}

#[test]
fn rule_reports_need_a_basis_and_honest_confidence() {
    let answered = RuleReport::Answered {
        task: DecisionTask::IntentClassification,
        answer: json!("pricing_inquiry"),
        confidence: 1.0,
        basis: "the count says so".to_string(),
    };
    assert_eq!(
        rule_report_problem(&answered),
        Some("report for intent_classification claims certainty".to_string())
    );
    let over = RuleReport::Answered {
        task: DecisionTask::IntentClassification,
        answer: json!("pricing_inquiry"),
        confidence: 1.5,
        basis: "reason".to_string(),
    };
    assert_eq!(
        rule_report_problem(&over),
        Some("report for intent_classification has a confidence outside [0, 1]".to_string())
    );
    let basis_free = RuleReport::Answered {
        task: DecisionTask::IntentClassification,
        answer: json!("pricing_inquiry"),
        confidence: 0.8,
        basis: "   ".to_string(),
    };
    assert_eq!(
        rule_report_problem(&basis_free),
        Some("report for intent_classification needs a stated basis".to_string())
    );
    let refused = RuleReport::Refused {
        task: DecisionTask::IntentClassification,
        basis: String::new(),
    };
    assert_eq!(
        rule_report_problem(&refused),
        Some("report for intent_classification needs a stated basis".to_string())
    );
}

#[test]
fn cue_provider_refuses_when_nothing_or_a_tie_decides() {
    let intent_table = archava_context::cues_for(DecisionTask::IntentClassification)
        .expect("the intent task has a cue table");
    // Not even one cue fired: a provider with nothing to read says nothing.
    let silent = archava_context::report_from_cues(
        DecisionTask::IntentClassification,
        &normalize_utterance("greetings"),
        intent_table,
    );
    match silent {
        RuleReport::Refused { basis, .. } => assert_eq!(basis, "no cue decided it"),
        _ => panic!("zero-tally utterances refuse, they do not answer"),
    }
    // "cancellation" and "rules" vote policy +2, "harga" votes them away once
    // (-1) while pricing reads +1 — two rows tie at 1, so nobody answers.
    let tied = archava_context::report_from_cues(
        DecisionTask::IntentClassification,
        &normalize_utterance("cancellation rules harga"),
        intent_table,
    );
    match tied {
        RuleReport::Refused { basis, .. } => assert_eq!(basis, "the cues tied"),
        _ => panic!("a tied tally is an ambiguity, not a decision"),
    }
    // Word-bounded matching: "hargai" is not "harga".
    let overshoot = archava_context::report_from_cues(
        DecisionTask::IntentClassification,
        &normalize_utterance("saya hargai usaha anda"),
        intent_table,
    );
    match overshoot {
        RuleReport::Refused { basis, .. } => assert_eq!(basis, "no cue decided it"),
        _ => panic!("substrings of a cue are not the cue"),
    }
}

#[test]
fn the_provider_answers_intent_from_its_cues() {
    let intent_table = archava_context::cues_for(DecisionTask::IntentClassification)
        .expect("the intent task has a cue table");
    let report = archava_context::report_from_cues(
        DecisionTask::IntentClassification,
        &normalize_utterance("berapa harga kamar deluxe"),
        intent_table,
    );
    match report {
        RuleReport::Answered {
            answer,
            confidence,
            basis,
            ..
        } => {
            assert_eq!(answer, json!("pricing_inquiry"));
            assert!(
                (confidence - 0.65).abs() < 1e-9,
                "harga and berapa harga = two fired cues"
            );
            assert_eq!(basis, "2 cue(s) for pricing_inquiry");
        }
        _ => panic!("the pinned utterance fires the pricing cues"),
    }
}

#[test]
fn the_provider_answers_coverage_from_the_facts() {
    let provider = archava_context::RuleDecisionProvider::default();
    let report = provider.report_task(
        DecisionTask::EvidenceSufficiency,
        "words are not coverage",
        &json!({ "page": 1, "cart": 1, "form": 1, "session": 1 }),
    );
    match report {
        RuleReport::Answered {
            answer,
            confidence,
            basis,
            ..
        } => {
            assert!(answer
                .as_f64()
                .is_some_and(|score| (score - 0.8).abs() < 1e-9));
            assert!((confidence - 0.5).abs() < 1e-9);
            assert_eq!(basis, "measured from the supplied facts");
        }
        _ => panic!("the coverage task is measured, never cue-counted"),
    }
}

#[test]
fn cue_matching_treats_punctuation_as_word_boundaries() {
    assert!(mentions(
        &normalize_utterance("berapa harga"),
        &normalize_cue("berapa harga")
    ));
    assert!(mentions(
        &normalize_utterance("berapa-harga!"),
        &normalize_cue("harga")
    ));
    assert!(
        !mentions(&normalize_utterance("hargaini"), &normalize_cue("harga")),
        "no substring leakage"
    );
    assert!(mentions(
        &normalize_utterance("585, harga"),
        &normalize_cue("harga")
    ));
}

#[test]
fn normalisation_folds_case_and_separators() {
    assert_eq!(normalize_utterance("  Berapa-HARGA??  "), "berapa harga");
    assert_eq!(normalize_cue("How much IS it"), "how much is it");
}

// ---------------------------------------------------------------------------
// Boundary: request/response validation (validate.ts, at the seam)
// ---------------------------------------------------------------------------

fn reference_request() -> DecisionRequest {
    DecisionRequest {
        tenant_id: ACME.to_string(),
        utterance: "berapa harga".to_string(),
        locale: "en".to_string(),
        evidence: json!({ "page": { "kind": "room" } }),
        questions: vec![one_question(DecisionTask::IntentClassification)],
    }
}

#[test]
fn the_request_boundary_checks_shape_uniqueness_and_kinds() {
    let mut no_tenant = reference_request();
    no_tenant.tenant_id = "  ".to_string();
    assert_eq!(
        request_problem(&no_tenant),
        Some("a decision request needs a tenant id".to_string())
    );
    let mut no_locale = reference_request();
    no_locale.locale = " ".to_string();
    assert_eq!(
        request_problem(&no_locale),
        Some("a decision request needs a locale".to_string())
    );
    let mut no_questions = reference_request();
    no_questions.questions = Vec::new();
    assert_eq!(
        request_problem(&no_questions),
        Some("a decision request needs at least one question".to_string())
    );
    let mut blank_prompt = reference_request();
    blank_prompt.questions[0].prompt = "   ".to_string();
    assert_eq!(
        request_problem(&blank_prompt),
        Some("Question \"intent_classification\" needs a prompt.".to_string())
    );
    let mut twice = reference_request();
    twice.questions.insert(0, twice.questions[0].clone());
    assert_eq!(
        request_problem(&twice),
        Some("question intent_classification was asked twice in one request".to_string())
    );
    let mut wrong_shape = reference_request();
    wrong_shape.questions[0].kind = DecisionKind::Boolean;
    assert_eq!(
        request_problem(&wrong_shape),
        Some("question intent_classification was asked as a boolean, not a choice".to_string())
    );
    assert_eq!(request_problem(&reference_request()), None);
}

#[test]
fn question_shape_problems_name_the_unusable_question() {
    let no_options = DecisionQuestion {
        id: DecisionTask::IntentClassification,
        kind: DecisionKind::Choice,
        prompt: "Which kind?".to_string(),
        options: Some(Vec::new()),
        bounds: None,
    };
    assert_eq!(
        archava_context::question_shape_problem(&no_options),
        Some(
            "Question \"intent_classification\" is a choice with no options; a closed answer set is the whole point."
                .to_string()
        )
    );
    let no_bounds = DecisionQuestion {
        id: DecisionTask::EvidenceSufficiency,
        kind: DecisionKind::Score,
        prompt: "How sufficient?".to_string(),
        options: None,
        bounds: None,
    };
    assert_eq!(
        archava_context::question_shape_problem(&no_bounds),
        Some("Question \"evidence_sufficiency\" is a score with no bounds.".to_string())
    );
    let reversed = DecisionQuestion {
        id: DecisionTask::EvidenceSufficiency,
        kind: DecisionKind::Score,
        prompt: "How sufficient?".to_string(),
        options: None,
        bounds: Some((5.0, 1.0)),
    };
    assert_eq!(
        archava_context::question_shape_problem(&reversed),
        Some("Question \"evidence_sufficiency\" has bounds 5..1; the range is empty.".to_string())
    );
}

/// A two-question request whose second answer is a number: for the score-shaped
/// boundary witnesses.
fn request_with_score() -> DecisionRequest {
    let mut request = reference_request();
    request
        .questions
        .push(one_question(DecisionTask::EvidenceSufficiency));
    request
}

fn stub_identity() -> archava_context::ProviderIdentity {
    archava_context::ProviderIdentity {
        provider_id: STUB_ID.to_string(),
        model: STUB_MODEL.to_string(),
    }
}

#[test]
fn the_response_boundary_refuses_orphans_and_blanks() {
    // Two questions: the incomplete/blank-refusal cases need a second task to
    // be silent about.
    let mut request = reference_request();
    request
        .questions
        .push(one_question(DecisionTask::KnowledgeRouting));
    let expected = stub_identity();
    let intent_ok = answered(
        DecisionTask::IntentClassification,
        json!("pricing_inquiry"),
        0.9,
    );
    let routing_ok = answered(
        DecisionTask::KnowledgeRouting,
        json!("structured_truth"),
        0.9,
    );

    let mislabelled = DecisionResult {
        provider_id: "other-provider".to_string(),
        model: STUB_MODEL.to_string(),
        decisions: vec![intent_ok.clone()],
        refused: Vec::new(),
    };
    assert_eq!(
        check_decision_response(&mislabelled, &request, &expected).unwrap_err(),
        "a provider replied under a different identity than it was asked with"
    );

    let complete = reply_of(vec![intent_ok.clone(), routing_ok.clone()], Vec::new());
    assert!(check_decision_response(&complete, &request, &expected).is_ok());

    let orphan = reply_of(
        vec![
            intent_ok.clone(),
            routing_ok.clone(),
            answered(DecisionTask::EvidenceSufficiency, json!(0.5), 0.5),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&orphan, &request, &expected).unwrap_err(),
        "a provider answered a question that was not asked: evidence_sufficiency"
    );

    let refused_orphan = reply_of(
        vec![intent_ok.clone(), routing_ok.clone()],
        vec![refusal(DecisionTask::EvidenceSufficiency, "n/a")],
    );
    assert_eq!(
        check_decision_response(&refused_orphan, &request, &expected).unwrap_err(),
        "a provider refused a question that was not asked: evidence_sufficiency"
    );

    let duplicated = reply_of(
        vec![intent_ok.clone(), intent_ok.clone(), routing_ok],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&duplicated, &request, &expected).unwrap_err(),
        "a provider answered intent_classification twice in one reply"
    );

    let both = reply_of(
        vec![intent_ok.clone()],
        vec![refusal(
            DecisionTask::IntentClassification,
            "changed my mind",
        )],
    );
    assert_eq!(
        check_decision_response(&both, &request, &expected).unwrap_err(),
        "a provider answered and refused intent_classification"
    );

    let mute = reply_of(
        vec![intent_ok.clone()],
        vec![refusal(DecisionTask::KnowledgeRouting, "   ")],
    );
    assert_eq!(
        check_decision_response(&mute, &request, &expected).unwrap_err(),
        "a refusal for knowledge_routing needs a reason somebody can read"
    );

    let silent = reply_of(vec![intent_ok], Vec::new());
    assert_eq!(
        check_decision_response(&silent, &request, &expected).unwrap_err(),
        "a provider left knowledge_routing unanswered"
    );
}

#[test]
fn the_response_boundary_narrows_answers_to_their_tasks() {
    let request = request_with_score();
    let expected = stub_identity();
    let evidence_ok = answered(DecisionTask::EvidenceSufficiency, json!(0.5), 0.5);
    let intent_ok = answered(
        DecisionTask::IntentClassification,
        json!("pricing_inquiry"),
        0.9,
    );

    let outside = reply_of(
        vec![
            answered(
                DecisionTask::IntentClassification,
                json!("not_an_option"),
                0.9,
            ),
            evidence_ok.clone(),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&outside, &request, &expected).unwrap_err(),
        "answer for intent_classification is not in its answer set"
    );

    let wrong_kind = reply_of(
        vec![
            Decision {
                id: DecisionTask::IntentClassification,
                kind: DecisionKind::Boolean,
                answer: json!(true),
                confidence: 0.9,
            },
            evidence_ok.clone(),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&wrong_kind, &request, &expected).unwrap_err(),
        "answer for intent_classification is a boolean, not a choice"
    );

    let range_broken = reply_of(
        vec![
            intent_ok.clone(),
            answered(DecisionTask::EvidenceSufficiency, json!(2.5), 0.9),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&range_broken, &request, &expected).unwrap_err(),
        "answer for evidence_sufficiency is outside its range"
    );

    let overconfident = reply_of(
        vec![
            answered(
                DecisionTask::IntentClassification,
                json!("pricing_inquiry"),
                1.7,
            ),
            evidence_ok.clone(),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&overconfident, &request, &expected).unwrap_err(),
        "confidence for intent_classification is outside [0, 1]"
    );

    let not_a_number = reply_of(
        vec![
            answered(
                DecisionTask::IntentClassification,
                json!("pricing_inquiry"),
                f64::NAN,
            ),
            evidence_ok.clone(),
        ],
        Vec::new(),
    );
    assert_eq!(
        check_decision_response(&not_a_number, &request, &expected).unwrap_err(),
        "confidence for intent_classification must be a finite number"
    );

    let ok = reply_of(vec![intent_ok, evidence_ok], Vec::new());
    let CheckedResponse {
        decisions,
        refusals,
    } = check_decision_response(&ok, &request, &expected).expect("the reply is complete");
    assert!(decisions.contains_key(&DecisionTask::IntentClassification));
    assert!(decisions.contains_key(&DecisionTask::EvidenceSufficiency));
    assert!(refusals.is_empty());
}

// ---------------------------------------------------------------------------
// Orchestrator judgment (packages/decision/src/orchestrator.ts)
// ---------------------------------------------------------------------------

fn intent_reply(answer: Value, confidence: f64) -> DecisionResult {
    reply_of(
        vec![answered(
            DecisionTask::IntentClassification,
            answer,
            confidence,
        )],
        Vec::new(),
    )
}

fn orchestrator_with(
    provider: StaticProvider,
    mode: AssistMode,
    enabled: &[DecisionTask],
    timeout_ms: Option<u64>,
) -> DefaultOrchestrator<StaticProvider> {
    DefaultOrchestrator::new(Some(provider), mode, enabled, &[], timeout_ms, None, None)
        .expect("static fixtures are well-formed")
}

fn intent_turn() -> archava_context::DecisionTurn {
    archava_context::DecisionTurn {
        tenant_id: ACME.to_string(),
        utterance: "berapa harga".to_string(),
        locale: "en".to_string(),
        evidence: json!({ "page": { "kind": "room" } }),
    }
}

fn intent_baseline() -> BaselineAnswer {
    baseline(
        DecisionTask::IntentClassification,
        json!("pricing_inquiry"),
        "foundation_classification",
    )
}

fn intent_run(
    orchestrator: &DefaultOrchestrator<StaticProvider>,
) -> Result<archava_context::DecisionRun, archava_context::DecisionError> {
    orchestrator.run(&intent_turn(), &[intent_baseline()])
}

#[test]
fn off_mode_makes_no_provider_call() {
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(provider, AssistMode::Off, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    assert_eq!(run.mode, AssistMode::Off);
    assert!(run.latency_ms.is_none(), "no call, no latency to report");
    assert_eq!(run.traces.len(), 1);
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::NotCalled);
    assert_eq!(trace.reason, "this mode makes no provider call");
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
    assert!(!trace.applied);
}

#[test]
fn assist_mode_refuses_to_construct_without_a_provider() {
    let error = match DefaultOrchestrator::<StaticProvider>::new(
        None,
        AssistMode::Assist,
        DECISION_TASKS,
        &[],
        None,
        None,
        None,
    ) {
        Err(error) => error,
        Ok(_) => panic!("assist without a provider must not construct"),
    };
    assert_eq!(
        error.to_string(),
        "mode assist needs a provider, and none was configured"
    );
}

#[test]
fn assist_applies_the_answer_when_the_gates_open() {
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    assert_eq!(run.provider_id.as_deref(), Some(STUB_ID));
    assert_eq!(run.model.as_deref(), Some(STUB_MODEL));
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Answered);
    assert!(trace.applied);
    assert!(trace.would_apply);
    assert_eq!(trace.effective.task, DecisionTask::IntentClassification);
    assert_eq!(trace.effective.answer, json!("pricing_inquiry"));
    assert_eq!(trace.effective.source, AnswerSource::Provider);
    assert_eq!(
        trace.effective.basis,
        "stub-provider answered intent_classification at 0.9"
    );
    assert!(run.latency_ms.is_some());
    assert!(trace.latency_ms.is_some());
    assert_eq!(
        trace.agreement,
        Some(Agreement::Equal),
        "the provider may agree with the baseline and still be the one that answered"
    );
}

#[test]
fn shadow_mode_records_what_would_have_applied() {
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(provider, AssistMode::Shadow, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Answered);
    assert!(!trace.applied, "shadow measures; it does not govern");
    assert!(trace.would_apply);
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
    assert_eq!(trace.reason, "shadow mode keeps the deterministic answer");
    assert!(trace.candidate.is_some());
    assert_eq!(trace.agreement, Some(Agreement::Equal));
}

#[test]
fn a_floor_gates_the_answer_it_holds() {
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.3));
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert!(!trace.applied);
    assert!(!trace.would_apply);
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
    assert_eq!(trace.reason, "below the floor this task set for itself");
    assert!(
        trace.candidate.is_some(),
        "the held answer stays on the record"
    );
    assert!(trace.confidence.is_some());
}

#[test]
fn the_escalation_policy_blocks_a_descent() {
    let downgrade = reply_of(
        vec![answered(
            DecisionTask::KnowledgeRouting,
            json!("retrieval_knowledge"),
            0.9,
        )],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(downgrade);
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = orchestrator
        .run(
            &intent_turn(),
            &[baseline(
                DecisionTask::KnowledgeRouting,
                json!("structured_truth"),
                "foundation_classification",
            )],
        )
        .expect("well-formed inputs");
    let trace = &run.traces[0];
    assert!(!trace.applied);
    assert!(!trace.would_apply);
    assert_eq!(
        trace.reason,
        "knowledge_routing does not descend: a provider may not lower what Foundation answered"
    );
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
}

#[test]
fn the_escalation_policy_permits_the_ascent() {
    let upgrade = reply_of(
        vec![answered(
            DecisionTask::KnowledgeRouting,
            json!("structured_truth"),
            0.9,
        )],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(upgrade);
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = orchestrator
        .run(
            &intent_turn(),
            &[baseline(
                DecisionTask::KnowledgeRouting,
                json!("retrieval_knowledge"),
                "foundation_classification",
            )],
        )
        .expect("well-formed inputs");
    let trace = &run.traces[0];
    assert!(trace.applied);
    assert_eq!(trace.effective.source, AnswerSource::Provider);
    assert_eq!(trace.reason, "every gate this task set for itself is open");
}

#[test]
fn an_unready_provider_holds_the_turn() {
    let provider = StaticProvider::unready(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Failed);
    assert_eq!(trace.reason, "the provider reported itself not ready");
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
    assert!(run.latency_ms.is_some(), "the attempt still cost time");
}

#[test]
fn provider_failures_never_become_context() {
    let provider = StaticProvider::failing();
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Failed);
    assert_eq!(trace.reason, "the decision provider failed");
    assert!(
        !trace.reason.contains("sk_live_SECRET_NUMBER_9"),
        "the provider's own error text is context the trace must not repeat"
    );
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
}

#[test]
fn a_malformed_reply_closes_the_whole_reply() {
    // One extra answer poisons the reply the intent answer rode in on: no
    // partial acceptance.
    let greedy = reply_of(
        vec![
            answered(
                DecisionTask::IntentClassification,
                json!("pricing_inquiry"),
                0.9,
            ),
            answered(
                DecisionTask::KnowledgeRouting,
                json!("structured_truth"),
                0.9,
            ),
        ],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(greedy);
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Malformed);
    assert_eq!(
        trace.reason,
        "a provider answered a question that was not asked: knowledge_routing"
    );
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
}

#[test]
fn a_refused_task_preserves_the_baseline() {
    let declining = reply_of(
        Vec::new(),
        vec![refusal(
            DecisionTask::IntentClassification,
            "no cue fired for this one",
        )],
    );
    let provider = StaticProvider::with_reply(declining);
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Refused);
    assert_eq!(trace.reason, "the provider refused this task");
    assert!(trace.candidate.is_none());
    assert!(trace.confidence.is_none());
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
}

#[test]
fn the_provider_call_respects_its_deadline() {
    let provider = StaticProvider::asleep(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, Some(1));
    let run = intent_run(&orchestrator).expect("well-formed inputs");
    let trace = &run.traces[0];
    assert_eq!(trace.outcome, TraceOutcome::Timeout);
    assert_eq!(trace.reason, "the provider did not answer in time");
    assert_eq!(trace.effective.source, AnswerSource::Baseline);
    assert!(run.latency_ms.is_some());
}

#[test]
fn duplicate_baselines_are_caller_bugs() {
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, DECISION_TASKS, None);
    let error = orchestrator
        .run(
            &intent_turn(),
            &[
                intent_baseline(),
                baseline(
                    DecisionTask::IntentClassification,
                    json!("general_inquiry"),
                    "other",
                ),
            ],
        )
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "a turn was given two baselines for intent_classification"
    );
}

#[test]
fn baselines_outside_the_enabled_set_are_dropped_unexamined() {
    // The routing baseline carries a value no task returns and a blank source —
    // both would fail validation if it were examined. It is dropped instead,
    // so the turn succeeds with one decision.
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = orchestrator_with(
        provider,
        AssistMode::Assist,
        &[DecisionTask::IntentClassification],
        None,
    );
    let run = orchestrator
        .run(
            &intent_turn(),
            &[
                baseline(DecisionTask::KnowledgeRouting, json!("not-a-source"), "   "),
                intent_baseline(),
            ],
        )
        .expect("the dropped baseline was never asked");
    assert_eq!(run.decisions.len(), 1);
    assert_eq!(run.decisions[0].task, DecisionTask::IntentClassification);
}

#[test]
fn run_decision_turn_runs_the_catalogue_once_per_turn() {
    let provider = archava_context::RuleDecisionProvider::default();
    let run = run_decision_turn(
        &provider,
        &archava_context::DecisionTurn {
            tenant_id: ACME.to_string(),
            utterance: "berapa harga kamar deluxe".to_string(),
            locale: "en".to_string(),
            evidence: json!({ "page": { "kind": "room", "locale": "en" } }),
        },
        &[
            baseline(
                DecisionTask::IntentClassification,
                json!("general_inquiry"),
                "foundation_classification",
            ),
            baseline(
                DecisionTask::KnowledgeRouting,
                json!("retrieval_knowledge"),
                "classifyKnowledgeNeed",
            ),
            baseline(DecisionTask::Clarification, json!(false), "foundation_turn"),
            baseline(
                DecisionTask::HandoffRecommendation,
                json!(false),
                "handoffRequested",
            ),
            baseline(
                DecisionTask::EvidenceSufficiency,
                json!(1),
                "retrieval_presence",
            ),
        ],
    )
    .expect("the deterministic provider answers what it was asked");
    assert_eq!(
        run.decisions.len(),
        5,
        "one effective answer per asked task, always"
    );
    assert_eq!(run.traces.len(), 5);
    assert_eq!(run.provider_id.as_deref(), Some("rule-baseline"));
    assert_eq!(run.mode, AssistMode::Assist);
    assert!(run.latency_ms.is_some());
}

// ---------------------------------------------------------------------------
// Turn seam: utterance redaction, evidence, evaluate
// (packages/assistant/src/decision.ts + turn.ts)
// ---------------------------------------------------------------------------

#[test]
fn utterance_masks_emails() {
    assert_eq!(
        decision_utterance("write to reservations@acme.test"),
        "write to [email]"
    );
    assert_eq!(decision_utterance(".a@b.com"), ".[email]");
    assert_eq!(decision_utterance("user_a@sub.company.com."), "[email].");
    assert_eq!(decision_utterance("plain words"), "plain words");
}

#[test]
fn utterance_masks_cards_with_separators() {
    assert_eq!(decision_utterance("4111 1111 1111 1111"), "[number]");
    assert_eq!(decision_utterance("4111-1111-1111-1111"), "[number]");
    assert_eq!(decision_utterance("1234567890123456"), "[number]");
    assert_eq!(
        decision_utterance("order 123456789012"),
        "order [number]",
        "twelve digits is no card (the card pass needs thirteen) — but the number pass \
         takes any run of ten, so it is still masked"
    );
    assert_eq!(
        decision_utterance("1234567890123456789012"),
        "[number]6789012",
        "a twenty-two digit run: the card pass never finds a boundary, the number pass \
         takes its first fifteen and leaves the tail"
    );
}

#[test]
fn utterance_masks_credentials() {
    assert_eq!(
        decision_utterance("here is key sk_live_abcdefgh"),
        "here is key [credential]"
    );
    assert_eq!(
        decision_utterance("pk_test_ABCDEFGH--"),
        "[credential]--",
        "a trailing dash is outside the word, so it survives"
    );
    assert_eq!(
        decision_utterance("short pk_abcde"),
        "short pk_abcde",
        "below eight characters"
    );
}

#[test]
fn utterance_masks_phone_runs() {
    // Eleven digits: below the card pass's thirteen, so the number pass owns
    // it — and it eats the leading '+' as its first group's optional sign.
    assert_eq!(
        decision_utterance("call +62 812 345 67 now"),
        "call [number] now"
    );
    assert_eq!(
        decision_utterance("62 0812 345"),
        "62 0812 345",
        "nine digits is a counter, not a phone"
    );
    assert_eq!(decision_utterance("+628123456789"), "[number]");
}

#[test]
fn utterance_handles_mixed_secrets() {
    // The phone here is twelve digits — thirteen would hand it to the card
    // pass, which runs first, and the '+' would then survive outside the
    // label.
    let medium = decision_utterance(
        "email us at help@acme.test or call +62 812 3456 789, card 4111 1111 1111 1111, key sk_live_abcdefgh",
    );
    assert_eq!(
        medium,
        "email us at [email] or call [number], card [number], key [credential]"
    );
}

#[test]
fn the_decision_evidence_names_only_the_page_and_three_publisheds() {
    let chunks: Vec<KnowledgeContextChunk> = (0..5)
        .map(|index| {
            grounding_chunk(
                &format!("faq-{index}"),
                &format!("chunk {index} contact reservations@acme.test for details"),
            )
        })
        .collect();
    let evidence = decision_evidence(
        &json!({
            "pageKind": "room",
            "locale": "en",
            "entities": [{ "id": "e1" }, { "id": "e2" }, { "id": "e3" }],
            "form": { "name": "leaky" },
        }),
        &chunks,
    );
    assert_eq!(evidence["page"]["kind"], json!("room"));
    assert_eq!(evidence["page"]["locale"], json!("en"));
    assert_eq!(evidence["page"]["entityCount"], json!(3));
    assert!(
        evidence.get("entities").is_none(),
        "a count is a shape the provider may weigh, the entities are not"
    );
    assert!(evidence.get("form").is_none(), "form values never cross");
    let published = evidence["publishedEvidence"].as_array().expect("a list");
    assert_eq!(published.len(), 3, "five grounded chunks, three published");
    for (index, entry) in published.iter().enumerate() {
        assert_eq!(entry["sourceId"], json!(format!("faq-{index}")));
        let text = entry["text"].as_str().expect("the chunk text");
        assert!(text.chars().count() <= 800);
        assert!(text.contains("[email]"));
        assert!(!text.contains("reservations@"));
    }
}

#[test]
fn the_turn_seam_redacts_before_a_provider_is_consulted() {
    let captured_cell: Rc<RefCell<Option<DecisionRequest>>> = Rc::new(RefCell::new(None));
    let provider = CapturingProvider {
        captured: captured_cell.clone(),
    };
    let orchestrator = DefaultOrchestrator::new(
        Some(provider.clone()),
        AssistMode::Assist,
        &[DecisionTask::IntentClassification],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let input = DecisionTurnInput {
        tenant_id: ACME.to_string(),
        utterance: "write to reservations@acme.test about room 4111 1111 1111 1111".to_string(),
        locale: "en".to_string(),
        context: json!({
            "pageKind": "room",
            "locale": "en",
            "entities": [{ "id": "e1" }, { "id": "e2" }],
        }),
        classification: KnowledgeClassification {
            need: KnowledgeNeed::RetrievalKnowledge,
            subjects: Vec::new(),
            matched: Vec::new(),
        },
        grounding: vec![],
        handoff_requested: false,
        structured_truth_candidates: vec![],
    };
    let outcome = evaluate_decision_turn(input, Some(&orchestrator));
    assert!(
        outcome.run.is_some(),
        "a provider failure is a completed run with a failed trace, not an interrupted turn"
    );
    assert!(!outcome.clarify);
    assert!(!outcome.recommend_handoff);
    assert!(!outcome.evidence_insufficient);
    assert_eq!(
        outcome.intent.as_deref(),
        Some("general_inquiry"),
        "the held intent baseline is the effective answer the caller reads"
    );
    assert!(!outcome.runtime_changed);
    let captured = captured_cell
        .borrow()
        .clone()
        .expect("the provider was consulted");
    assert_eq!(captured.utterance, "write to [email] about room [number]");
    assert_eq!(captured.evidence["page"]["entityCount"], json!(2));
    assert!(captured.evidence.get("form").is_none());
    assert_eq!(captured.tenant_id, ACME);
    assert_eq!(captured.locale, "en");
}

#[test]
fn a_provider_is_not_consulted_when_nothing_may_be_replaced() {
    // An empty enabled set means nothing to ask: the provider is never
    // consulted, however much the context shadows.
    let captured_cell: Rc<RefCell<Option<DecisionRequest>>> = Rc::new(RefCell::new(None));
    let provider = CapturingProvider {
        captured: captured_cell.clone(),
    };
    let orchestrator = DefaultOrchestrator::new(
        Some(provider.clone()),
        AssistMode::Assist,
        &[],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let input = evaluation_input(retrieval_classification(), vec![]);
    evaluate_decision_turn(input, Some(&orchestrator));
    assert!(
        captured_cell.borrow().is_none(),
        "a provider behind a closed hangar door does not get asked"
    );
}

fn evaluation_input(
    classification: KnowledgeClassification,
    candidates: Vec<StructuredTruthSubject>,
) -> DecisionTurnInput {
    DecisionTurnInput {
        tenant_id: ACME.to_string(),
        utterance: "what are your policies?".to_string(),
        locale: "en".to_string(),
        context: json!({ "pageKind": "policy", "locale": "en" }),
        classification,
        grounding: vec![grounding_chunk("faq-0", "policies are published")],
        handoff_requested: false,
        structured_truth_candidates: candidates,
    }
}

fn retrieval_classification() -> KnowledgeClassification {
    classify_knowledge_need("what are your policies?")
}

#[test]
fn evaluate_turn_with_no_orchestrator_changes_nothing() {
    let input = evaluation_input(retrieval_classification(), vec![]);
    let outcome = evaluate_decision_turn::<StaticProvider>(input, None);
    assert!(outcome.run.is_none());
    assert_eq!(
        outcome.classification.need,
        KnowledgeNeed::RetrievalKnowledge
    );
    assert!(!outcome.clarify);
    assert!(!outcome.recommend_handoff);
    assert!(!outcome.evidence_insufficient);
    assert!(outcome.intent.is_none());
    assert!(!outcome.runtime_changed);
}

#[test]
fn an_escalation_replaces_the_classification() {
    let reply = reply_of(
        vec![answered(
            DecisionTask::KnowledgeRouting,
            json!("structured_truth"),
            0.9,
        )],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(reply);
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[DecisionTask::KnowledgeRouting],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let input = evaluation_input(
        retrieval_classification(),
        vec![StructuredTruthSubject::Price],
    );
    let outcome = evaluate_decision_turn(input, Some(&orchestrator));
    assert_eq!(outcome.classification.need, KnowledgeNeed::StructuredTruth);
    assert_eq!(
        outcome.classification.subjects,
        vec![StructuredTruthSubject::Price]
    );
    assert_eq!(
        outcome.classification.matched,
        vec!["decision_escalation".to_string()]
    );
    assert!(outcome.runtime_changed);
    assert!(!outcome.clarify);
    assert!(!outcome.recommend_handoff);
    assert!(!outcome.evidence_insufficient);
}

#[test]
fn an_escalation_dedupes_the_candidates_first_seen_wins() {
    let reply = reply_of(
        vec![answered(
            DecisionTask::KnowledgeRouting,
            json!("structured_truth"),
            0.9,
        )],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(reply);
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[DecisionTask::KnowledgeRouting],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let input = evaluation_input(
        retrieval_classification(),
        vec![
            StructuredTruthSubject::Price,
            StructuredTruthSubject::Price,
            StructuredTruthSubject::Stock,
        ],
    );
    let outcome = evaluate_decision_turn(input, Some(&orchestrator));
    assert_eq!(
        outcome.classification.subjects,
        vec![StructuredTruthSubject::Price, StructuredTruthSubject::Stock],
        "first occurrence wins, the set semantics"
    );
}

#[test]
fn an_escalation_needs_the_provider_route() {
    // Nothing may be replaced, so the route stays Foundation's — the same need,
    // no escalation, nothing changed, however good the candidates look.
    let provider = StaticProvider::with_reply(reply_of(
        vec![answered(
            DecisionTask::KnowledgeRouting,
            json!("structured_truth"),
            0.9,
        )],
        Vec::new(),
    ));
    let orchestrator = orchestrator_with(provider, AssistMode::Assist, &[], None);
    let input = evaluation_input(
        retrieval_classification(),
        vec![StructuredTruthSubject::Price],
    );
    let outcome = evaluate_decision_turn(input, Some(&orchestrator));
    assert_eq!(
        outcome.classification.need,
        KnowledgeNeed::RetrievalKnowledge
    );
    assert!(outcome.classification.matched.is_empty());
    assert!(!outcome.runtime_changed);
}

#[test]
fn an_escalation_waits_for_candidates() {
    // Routing may be applied on its own without disturbing the classification:
    // the escalation needs candidate subjects to escalate INTO.
    let provider = StaticProvider::with_reply(reply_of(
        vec![answered(
            DecisionTask::KnowledgeRouting,
            json!("structured_truth"),
            0.9,
        )],
        Vec::new(),
    ));
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[DecisionTask::KnowledgeRouting],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let input = evaluation_input(retrieval_classification(), vec![]);
    let outcome = evaluate_decision_turn(input, Some(&orchestrator));
    assert_eq!(
        outcome.classification.need,
        KnowledgeNeed::RetrievalKnowledge
    );
    assert!(!outcome.runtime_changed);
}

#[test]
fn provider_true_flags_are_one_way() {
    let reply = reply_of(
        vec![
            answered(DecisionTask::Clarification, json!(true), 0.9),
            answered(DecisionTask::HandoffRecommendation, json!(true), 0.9),
        ],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(reply);
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[
            DecisionTask::Clarification,
            DecisionTask::HandoffRecommendation,
        ],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let outcome = evaluate_decision_turn(
        evaluation_input(retrieval_classification(), vec![]),
        Some(&orchestrator),
    );
    assert!(outcome.clarify);
    assert!(outcome.recommend_handoff);
    assert!(outcome.runtime_changed);
    assert!(
        outcome.intent.is_none(),
        "an unasked task has no answer to read"
    );
}

#[test]
fn low_evidence_scores_are_held_by_the_floor() {
    // 0.4 sits below evidence_sufficiency's 0.6 floor, so the provider score is
    // held and the answer stays Foundation's 1. That is also why the `< 0.5`
    // withhold documented in the source is unreachable through the catalogue:
    // a provider-sourced number under 0.5 cannot clear the floor in either
    // language.
    let reply = reply_of(
        vec![answered(DecisionTask::EvidenceSufficiency, json!(0.4), 0.4)],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(reply);
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[DecisionTask::EvidenceSufficiency],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let outcome = evaluate_decision_turn(
        evaluation_input(retrieval_classification(), vec![]),
        Some(&orchestrator),
    );
    assert!(!outcome.evidence_insufficient);
    assert!(!outcome.runtime_changed);
    let decision = &outcome.run.as_ref().expect("the run happened").decisions[0];
    assert_eq!(decision.task, DecisionTask::EvidenceSufficiency);
    assert_eq!(decision.source, AnswerSource::Baseline);
}

#[test]
fn strong_evidence_scores_flow_but_change_nothing() {
    // 0.9 clears the floor and withholds nothing — the value flows, no flag
    // moves.
    let reply = reply_of(
        vec![answered(DecisionTask::EvidenceSufficiency, json!(0.9), 0.9)],
        Vec::new(),
    );
    let provider = StaticProvider::with_reply(reply);
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[DecisionTask::EvidenceSufficiency],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let outcome = evaluate_decision_turn(
        evaluation_input(retrieval_classification(), vec![]),
        Some(&orchestrator),
    );
    assert!(!outcome.evidence_insufficient);
    assert!(!outcome.runtime_changed);
    let decision = &outcome.run.as_ref().expect("the run happened").decisions[0];
    assert_eq!(decision.source, AnswerSource::Provider);
    assert_eq!(decision.answer, json!(0.9));
}

#[test]
fn intent_is_readable_from_any_source() {
    // Provider-sourced: the applied intent answer is the one the caller reads.
    let provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.9));
    let orchestrator = DefaultOrchestrator::new(
        Some(provider),
        AssistMode::Assist,
        &[DecisionTask::IntentClassification],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let input = evaluation_input(
        KnowledgeClassification {
            need: KnowledgeNeed::RetrievalKnowledge,
            subjects: vec![StructuredTruthSubject::Price],
            matched: vec![],
        },
        vec![],
    );
    let outcome = evaluate_decision_turn(input, Some(&orchestrator));
    assert_eq!(outcome.intent.as_deref(), Some("pricing_inquiry"));

    // Baseline-sourced: a candidate below the floor leaves Foundation's answer
    // in force, and the caller still reads an intent.
    let held_provider = StaticProvider::with_reply(intent_reply(json!("pricing_inquiry"), 0.3));
    let held_orchestrator = DefaultOrchestrator::new(
        Some(held_provider),
        AssistMode::Assist,
        &[DecisionTask::IntentClassification],
        &[],
        None,
        None,
        None,
    )
    .expect("well-formed");
    let outcome = evaluate_decision_turn(
        evaluation_input(retrieval_classification(), vec![]),
        Some(&held_orchestrator),
    );
    assert_eq!(
        outcome.intent.as_deref(),
        Some("general_inquiry"),
        "the held baseline is the effective answer, and the run still reports it"
    );
}
