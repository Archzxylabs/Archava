//! Laya's System 1 gate, mounted on the conversation route in shadow mode.
//!
//! The dual-brain split this workspace records is: high-frequency, bounded
//! questions belong to a reflexive layer (Laya, System 1), and an ambiguous
//! turn escalates to the conversational agent (Ava, System 2). The layer itself
//! lives in `archava-context` — the closed task catalogue, the orchestrator,
//! the 0.85 act-vs-escalate floor and the 25-option cardinality bound — and
//! until this module no crate in the workspace depended on it, so no live turn
//! ever asked the gate anything.
//!
//! What is mounted here is deliberately the *measurement* half:
//! [`AssistMode::Shadow`] calls the provider once per turn and records, per
//! task, whether its candidate would have cleared every gate (`would_apply`) and
//! how it compared with the deterministic baseline (`agreement`) — while the
//! answer in force stays the baseline. A shadow run is a measurement rather
//! than a shrug precisely because it reports the gates the candidate cleared
//! without opening them.
//!
//! The route's reply cannot move, and that is structural rather than careful:
//!
//! - every effective decision a shadow run returns has source
//!   `archava_context::AnswerSource::Baseline`; and
//! - [`evaluate_decision_turn`] only ever raises `clarify`,
//!   `recommend_handoff` or the structured-truth escalation from
//!   *provider-sourced* answers, which a shadow run never produces.
//!
//! Promotion to `AssistMode::Assist` is an authority question — which tasks may
//! act, at which confidence floor, for which tenant — not a code change, which
//! is why nothing here reads a mode from the environment or a config from the
//! database. Until that question is answered, the gate measures and the flow
//! decides.
//!
//! Failure is inert by design: a malformed turn (an empty tenant id or locale)
//! or a misconfigured orchestrator logs and returns `None`, and the
//! conversation route carries on — a decision integration must not interrupt
//! the Foundation turn.

use archava_context::decision::provider::LayaDecisionProvider;
use archava_context::{
    classify_knowledge_need, evaluate_decision_turn, AssistMode, DecisionError, DecisionRun,
    DecisionTurnInput, DefaultOrchestrator, TraceOutcome, DECISION_TASKS,
};
use serde_json::{json, Value};

/// The locale the gate asks in.
///
/// The cue tables are bilingual and the workspace ships Indonesian-first copy,
/// so this is the honest default. The provider seam reads the field as a
/// required non-empty string, not as a dialect selector — nothing in the
/// catalogue branches on it.
const LOCALE: &str = "id-ID";

/// The §16 projection this seam can offer: the locale alone.
///
/// The conversation route has no page shape and no retrieval grounding to hand
/// over — those belong to the visitor-turn pipeline this route is not. The
/// empty projection is the honest one: the evidence-sufficiency task then scores
/// what was actually supplied, which is nothing, instead of a shape that was
/// never there.
fn projection() -> Value {
    json!({ "locale": LOCALE })
}

/// The gate as this route builds it: Laya, shadow, whole catalogue.
///
/// The full catalogue is the enabled set because an empty set means "nothing
/// may be replaced" — correct for assist, but it would also silence the
/// measurement, since the orchestrator pre-filters the ask in every mode but
/// `off`.
fn shadow_orchestrator() -> Result<DefaultOrchestrator<LayaDecisionProvider>, DecisionError> {
    DefaultOrchestrator::new(
        Some(LayaDecisionProvider::default()),
        AssistMode::Shadow,
        DECISION_TASKS,
        &[],
        None,
        None,
        None,
    )
}

/// The turn this route hands the gate, built the way the turn seam documents.
fn turn_input(tenant_id: &str, utterance: &str) -> DecisionTurnInput {
    DecisionTurnInput {
        tenant_id: tenant_id.to_string(),
        utterance: utterance.to_string(),
        locale: LOCALE.to_string(),
        context: projection(),
        classification: classify_knowledge_need(utterance),
        // No retrieval lane runs on this route, so there is no published
        // grounding to expose and no trusted subject to escalate into.
        grounding: Vec::new(),
        handoff_requested: false,
        structured_truth_candidates: Vec::new(),
    }
}

/// One shadow pass over one turn: run the gate, log the measurement, and hand
/// the run back for a test to assert on.
///
/// `None` means the turn could not be measured at all — a misconfigured
/// orchestrator, or a turn the seam itself refuses. Both are logged and never
/// thrown: the route must not fail because its classifier did.
pub(crate) fn shadow_pass(tenant_id: &str, utterance: &str) -> Option<DecisionRun> {
    let orchestrator = match shadow_orchestrator() {
        Ok(orchestrator) => orchestrator,
        Err(error) => {
            tracing::info!(%error, "decision gate misconfigured; the turn is unmeasured");
            return None;
        }
    };
    let result = evaluate_decision_turn(turn_input(tenant_id, utterance), Some(&orchestrator));
    let Some(run) = result.run else {
        // The seam's own refusal (an empty tenant id or locale) — the second
        // failure path this module documents. It is logged for the same reason
        // the misconfiguration above is: an unmeasured turn must be visible as
        // a gap in the measurement, not as silence that looks like agreement.
        tracing::info!(
            tenant = tenant_id,
            "the decision seam refused this turn; it is unmeasured and the route \
             carries on"
        );
        return None;
    };
    let would_apply: Vec<&'static str> = run
        .traces
        .iter()
        .filter(|trace| trace.would_apply)
        .map(|trace| trace.task.as_str())
        .collect();
    let declined: Vec<&'static str> = run
        .traces
        .iter()
        .filter(|trace| trace.outcome == TraceOutcome::Refused)
        .map(|trace| trace.task.as_str())
        .collect();
    tracing::info!(
        tenant = tenant_id,
        provider = run.provider_id.as_deref().unwrap_or("none"),
        model = run.model.as_deref().unwrap_or("none"),
        mode = run.mode.as_str(),
        intent = ?result.intent,
        would_apply = ?would_apply,
        declined = ?declined,
        latency_ms = ?run.latency_ms,
        "system 1 decision pass (shadow: measured, never applied)"
    );
    Some(run)
}

#[cfg(test)]
mod tests {
    use super::*;
    use archava_context::AnswerSource;

    /// A cue-rich utterance: four pricing cues fire, so the provider is past
    /// the 0.85 floor for the tasks it can answer at all.
    const MEASURABLE: &str = "berapa harga biaya tarif paket catering untuk seratus orang?";

    #[test]
    fn the_gate_runs_the_whole_catalogue_and_applies_nothing() {
        let run = shadow_pass("acme", MEASURABLE).expect("a valid turn must be measured");

        assert_eq!(run.mode, AssistMode::Shadow);
        assert_eq!(run.provider_id.as_deref(), Some("convai-laya"));
        assert_eq!(run.model.as_deref(), Some("laya-typed-decisions"));
        assert_eq!(run.traces.len(), DECISION_TASKS.len());
        assert_eq!(run.decisions.len(), DECISION_TASKS.len());

        // Shadow: the provider answers, and the baseline stays in force.
        assert!(
            run.traces
                .iter()
                .all(|trace| !trace.applied && trace.effective.source == AnswerSource::Baseline),
            "shadow must never put a provider answer in force"
        );
        assert!(
            run.decisions
                .iter()
                .all(|decision| decision.source == AnswerSource::Baseline),
            "every effective decision must be the baseline in shadow mode"
        );

        // The gate was actually consulted: every task came back answered or
        // refused by the provider, and at least one carried a candidate.
        assert!(run.traces.iter().all(|trace| matches!(
            trace.outcome,
            TraceOutcome::Answered | TraceOutcome::Refused
        )));
        assert!(
            run.traces.iter().any(|trace| trace.candidate.is_some()),
            "the provider must be called in shadow mode, or nothing is measured"
        );
        assert!(
            run.traces.iter().any(|trace| trace.would_apply),
            "the measurement must report a candidate that cleared every gate, not \
             merely a refusal — that is what makes a shadow run worth reading"
        );
    }

    #[test]
    fn shadow_cannot_change_the_turn() {
        let orchestrator = shadow_orchestrator().expect("the gate is not misconfigured");
        let result = evaluate_decision_turn(turn_input("acme", MEASURABLE), Some(&orchestrator));

        assert!(result.run.is_some(), "the pass must produce a run to read");
        assert!(!result.runtime_changed);
        assert!(!result.clarify);
        assert!(!result.recommend_handoff);
        assert!(!result.evidence_insufficient);
        assert!(
            result.intent.is_some(),
            "the effective intent still comes through — from the baseline"
        );
    }

    #[test]
    fn a_turn_the_seam_refuses_degrades_to_nothing() {
        assert!(
            shadow_pass("", MEASURABLE).is_none(),
            "an unmeasurable turn must return None rather than panic or throw"
        );
    }
}
