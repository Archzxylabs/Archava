# Jev provider research and SHADOW evaluation

> **Status: historical provider research — source review dated 27 September 2026; consolidated 29 October 2026.** This records the earlier TypeScript adapter and evaluation harness. External facts were not revalidated, no live request was made for this consolidation, and the commands below are historical rather than current Rust setup instructions. Current decision boundaries are in [architecture](../architecture/DECISION_INTELLIGENCE.md).

## Recorded provider contract

<a id="selected-route"></a>
<a id="behavior-and-limits"></a>
<a id="verification-status"></a>

The earlier `JevDecisionProvider` used [TypeSafe AI's direct API](https://docs.typesafe.ai/api): `POST https://api.typesafe.ai/v1/systemone`, Bearer authentication, one `state` and a map of typed questions. Its configured model was `jev-latest`; the [models page](https://docs.typesafe.ai/models) resolved that rolling alias to `jev-1.13.0` at the original review. The adapter returned the configured alias for provider-identity matching, checked the resolved version, and did not expose that resolved version in trace metadata.

The direct wire schema matched the bounded provider port without adding a runtime dependency. [Vercel's Jev gateway](https://vercel.com/ai-gateway/models/jev) would require separate authentication/request mapping; substituting the endpoint alone was unsupported. No AI SDK `experimental_evaluate` signature was verified.

- Five catalogue tasks could share a request. The SHADOW harness batched four bounded questions per case.
- The server injected `TYPESAFE_API_KEY`; the adapter did not read browser storage or load the environment itself. Ordinary CI did not need a key.
- `choice` values had to match configured options; `score` levels were mapped into the requested bounded interval. `noul` became a boolean at 0.5 with an adapter-derived two-outcome confidence.
- Validation rejected malformed/missing answers, bounded transport time and made no retry. The orchestrator retained deterministic baselines on failure, refusal, malformed output, timeout or insufficient confidence. [Confidence semantics](https://docs.typesafe.ai/confidence) were distinct from task policy and authorization.
- Mock tests covered wire shape, batching, normalization, malformed data, HTTP errors, timeout, missing credentials, and matching a resolved version to the rolling alias. They did not establish live accuracy.

## Recorded SHADOW evaluation

<a id="route-and-command"></a>

### Data and measurement

Only committed synthetic utterances, synthetic evidence excerpts, a synthetic tenant id, locale, and a minimal synthetic page descriptor are sent. The 51-case utterance corpus scores intent classification, knowledge routing, clarification, and handoff recommendation; it still has no evidence payload and does not score evidence sufficiency. A separate 28-case labelled evidence set (10 English, 9 Indonesian, 9 mixed) scores evidence sufficiency over supported, missing, overlapping, conflicting, stale, and protected-truth examples. No live tenant form values, customer records, payment data, or secrets are sent.

The output contains separate case and provider-call counts for both sets, language counts, accuracy, per-label and per-locale results, raw candidate versus effective baseline, confidence-related trace outcomes, disagreements, refusal/fallback/failure counts, and critical errors. It prints no utterances or raw response bodies. SHADOW always returns the deterministic baseline as effective. A raw structured-truth downgrade, an effective downgrade, a malformed evidence result, or published evidence marked sufficient for a protected live fact fails the live command; other evidence mistakes remain reported for review. Transport/schema failures and refusals also fail the command. A partial run reports only the cases actually run.

The assistant's deterministic baselines are mirrored in the harnesses; routing comes directly from the Foundation classifier. A future change to assistant baseline logic must update these mirrors or move the baseline factory to a shared, tested API. The offline mocked tests verify shadow invariance, failure accounting and metrics wiring. They do **not** establish live Jev accuracy.

### Promotion boundary

<a id="promotion-gate"></a>

Keep Jev in SHADOW until representative full runs have been measured, all three language groups reviewed, task-specific accuracy and confidence reviewed against the Rule baseline, provider failure rates understood, and no protected structured-truth downgrade can affect runtime. The 28-case evidence set is an initial fixture, not a promotion threshold by itself. A task may enter ASSIST only through its existing task-specific policy. Jev confidence never grants ActionPolicy permission, confirms a user action, validates input, or executes a booking or payment.

## Historical runners and request limits

These pnpm/tsx commands describe the former workspace. They are retained to interpret its evidence and are not commands for the current Rust deployment.

| Runner | Recorded behavior |
| --- | --- |
| `pnpm decision:jev-smoke` | One synthetic English room-recommendation utterance and intent question, one SHADOW request, metadata output, nonzero exit without a valid answer |
| `pnpm decision:eval` | Offline deterministic/effective-routing metrics over 51 English, Indonesian and mixed-language utterance cases; no live Jev evaluation |
| `pnpm exec tsx tools/decision-eval/live-jev-eval.ts --live --max-cases 5` | Explicit live opt-in and server key; at most 10 calls for five utterance plus five evidence cases; a smoke rather than a quality assessment |

The recorded live runner accepted `--max-cases` from 1–28 (default 5) for each set, and `--pace-ms` from 250–60000 (default 500). A full capped run made at most 56 calls: 28 utterances and 28 separate evidence cases. The remaining 23 utterances belonged to the offline 51-case suite. Short runs selected English, Indonesian and mixed-language cases round robin. Ordinary test/build/verify/E2E commands did not invoke this runner. No automatic retry was made; failures were reported and calls were paced.

## Dated vendor assumptions and privacy

<a id="official-limits-cost-and-privacy"></a>

The original [API reference](https://docs.typesafe.ai/api) described 401, 422, 429 and 529 errors, with backoff guidance for rate/service limits. The [models page](https://docs.typesafe.ai/models) then listed $0.042 per million input tokens, free output tokens, 1,200 requests/minute and 250,000 tokens/second for Jev 1.13. Those figures and aliases can change and were not runtime assumptions.

The original [legal guidance](https://docs.typesafe.ai/legal) stated no training on customer requests/responses and an enterprise zero-data-retention offering. Applicable tenant contracts, retention and ZDR entitlement were not verified. Historical live-evaluation plans required the owner's server key and confirmation of the applicable terms, used projected/redacted synthetic context, and kept keys out of source, browser bundles and shell history. A provider confidence score did not grant permission, confirm an action, validate business input or execute a booking/payment.
