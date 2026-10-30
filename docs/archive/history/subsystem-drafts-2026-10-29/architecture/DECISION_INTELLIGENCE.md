# Decision Intelligence implementation

This document describes the code behind PRD §39. It is an internal bounded judgment layer, separate from conversation (`BrainProvider`), permission (`ActionPolicy`), validation, structured truth, and execution.

## Package and request path

| Part                 | Location                                                               | Responsibility                                                                                                         |
| -------------------- | ---------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| Neutral port         | `packages/adapters/src/decision.ts`                                    | `DecisionProvider`, typed Boolean/Choice/Score questions and answers                                                   |
| Catalogue and policy | `packages/decision/src/tasks.ts`, `policy.ts`, `ranking.ts`            | Five closed tasks, per-task confidence floors and a bounded future ranking primitive                                   |
| Orchestrator         | `packages/decision/src/orchestrator.ts`                                | One batched call, off/shadow/assist, response validation, timeout, fallback and metadata traces                        |
| Offline provider     | `packages/decision/src/rule-provider.ts`                               | Deterministic cue baseline and interchangeability proof                                                                |
| External provider    | `packages/adapters/src/jev-decision.ts` via `@archava/adapters/server` | Server-only TypeSafe HTTP adapter and transport normalization                                                          |
| Turn seam            | `packages/assistant/src/decision.ts`, `turn.ts`                        | Project/redact context, deterministic baselines, limited assist effects and events                                     |
| Tenant schema        | `packages/config/src/client/schema.ts`                                 | Optional `decision` config; absence preserves Foundation behavior                                                      |
| Reference            | `apps/web/src/slice.ts`, chat inspector                                | Offline rule mode and debug traces when configured; no browser credential                                              |
| Evaluation           | `tools/decision-eval`                                                  | 51 utterance cases plus a separate synthetic evidence set; accuracy, per-label counts, confusion and critical failures |

A turn classifies structured truth deterministically, retrieves published content, projects and masks context, then asks the orchestrator only for configured tasks. The resulting route may escalate to a trusted live-system subject. A provider cannot invent a subject, downgrade an already protected live question, authorize an action, or claim a side effect. The existing Brain, ActionPolicy, input validator, resolver, and executor still run in their own boundaries.

## Modes and confidence

`off` makes zero provider calls and returns Foundation baselines. `shadow` calls the provider but leaves visible behavior at the baseline; traces record candidate, confidence and disagreement. `assist` applies only enabled tasks that pass runtime schema validation, the task's built-in floor, an optional stricter tenant floor, and its direction rule. Low confidence, refusal, provider error, malformed output or timeout falls back to baseline. The default timeout is three seconds in the orchestrator; the Jev transport has its own bound. Traces contain no utterance or provider payload. The reference browser has no Jev connection, even if a config names Jev; it falls back safely. A production server host must inject a server-side provider and keep credentials there.

The first five tasks are intent, routing, clarification, handoff, and evidence sufficiency. Intent is an advisory/debug label; it never creates a permission. Clarification may ask a deterministic question. Handoff may add a recommendation but cannot suppress mandatory handoff. Evidence sufficiency can withhold an unsupported retrieval answer; it cannot certify a factual claim. Knowledge routing may escalate only when trusted subject candidates were provided by the host. All proposed future tasks remain Planned.

## Safety and privacy

Tenant ID is required for every decision request. The host projects context to page kind, locale, and entity count; the request includes at most three published excerpts of 800 characters each. Email addresses, long number sequences, and common token patterns are redacted from utterance and excerpts. This projection does not make arbitrary user text safe for unrestricted external processing: production activation still needs tenant consent, applicable data terms, and provider-specific retention review. No cross-tenant decision cache or global mutable customer state is used.

The turn emits `decision_evaluated`, `decision_fallback_used`, `decision_low_confidence`, and `decision_provider_failure` as metadata. The optional `?inspect` reference UI shows mode, task, provider, candidate, confidence, disagreement and fallback. Ordinary customer UI does not display provider plumbing.

## Evaluation and operations

Run `pnpm decision:eval` for the offline Rule provider and effective protected routing. The corpus has 51 cases: 22 English, 17 Indonesian, 12 mixed. It includes the six Foundation examples, ambiguity, recommendation/comparison, handoff, unsupported claims, price plus availability, support, purchase, and transactional status. The report prints per-task and per-label accuracy and a separate structured-truth downgrade counter. Rule judgments are intentionally crude; unanswered questions and weak evidence coverage are reported rather than hidden. The score is not a Jev score. The current result is recorded in `docs/BUILD_STATUS.md` after gates.

The optional `pnpm decision:jev-smoke` makes one live call using `TYPESAFE_API_KEY` from the server environment and synthetic text only. It is not part of CI. See [Jev provider research](../research/JEV_PROVIDER_RESEARCH.md) for current official API links, limits, privacy status and mock/live test distinction.

The separate `pnpm decision:jev-eval -- --live --max-cases 5` command runs two
bounded synthetic multilingual SHADOW evaluations when the owner provisions a
server key. The 51 utterance cases score four tasks; the separate labelled
evidence set scores evidence sufficiency. Neither set has been run against live
Jev. See [`JEV_EVAL_READINESS.md`](../research/JEV_EVAL_READINESS.md).
