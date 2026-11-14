# Ava live Gemini acceptance — 5 November 2026

**Live end-to-end acceptance passed locally** against Google's actual
GenerateContent service with `gemini-3-flash-preview` and the explicitly selected
`minimal` thinking profile. Real Chromium drove the production native binary,
SSR/WASM UI and an owned disposable PostgreSQL database with synthetic business
records. Six natural-language planning turns passed all eleven live-agent checks.

The earlier [functional completion](AGENTIC_COMPLETION_2026-11-05.md) remains the
historical result for its source. Its owned model fixtures did not certify live
reasoning. This work adds explicit real-provider acceptance and fixes found by it.

## Live scenarios

The final browser run exercised six natural-language planning turns:

| Scenario | Browser and independent database result |
| --- | --- |
| Indonesian procurement goal | Model selected supplier creation then an offer for an observed product, binding the first verified receipt; approval produced one supplier and one offer at 3,500 IDR |
| Follow-up in the same conversation | New child plan selected only the existing offer's cost update; new approval stored 4,200 IDR without duplicate supplier/offer |
| English CRM goal | Authorized contact update stored the requested organization after approval |
| Required business data absent | Clarification with no executable steps |
| Prompt asks to bypass approval | No approval recorded and no requested product created |
| Grounded read-only question | Correct observed SKU and 10,000 IDR selling price, no action steps |

Original request and approval replay also produced no duplicate offer. Earlier
backend diagnostics passed the six cases using Gemini 2.5 Flash and used an
owned, unchanged HTTP passthrough to Google's verified HTTPS service to inspect
proposals; it did not synthesize model responses.
The checked-in developer probe and final browser launcher require Google's
canonical HTTPS endpoint directly. No observer endpoint is accepted by them.

The browser acceptance additionally checked Home submission, actual Desk approval,
independent SQL effects/revisions, receipt-backed reports, saved preferences,
replay without another planning attempt, and restored results after navigation.
Its live result is recorded separately from the base browser checks. All planning
responses left business records unchanged; only approved steps produced receipts.
Memory, report and original-request replay added no model planning attempts.

## Integration findings

- The existing personal configuration selected an audio realtime model whose
  supported methods exclude GenerateContent. The key itself worked with an
  explicit text model. The private configuration was not changed or copied into
  this repository. The launcher maps only the key into its child environment.
- Early live output combined clarification/approval questions with draft steps.
  Clarification now safely drops draft actions. Ready plans explicitly use empty
  questions; the host collects approval independently.
- Quoted JSON action inputs produced malformed nested references, incorrect tool
  names and dependency indices. The provider protocol now uses actual typed input
  objects, exact registered tool names, and explicit zero-based receipt bindings.
- Applying large array bounds to the combined typed schema caused Google's
  `400 INVALID_ARGUMENT` constraint-state rejection. The decoding grammar keeps
  structural types and tool/input alternatives, while the host and domain retain
  all limits and authoritative validation. Google's
  [schema simplification guidance](https://ai.google.dev/gemini-api/docs/generate-content/structured-output#simplifying-complex-schemas)
  describes this multiplication of grammar states.
- One diagnostic seeded a product with stock tracking disabled. Purchasing
  correctly refused its supplier offer after recording the supplier receipt.
  The procurement fixture now uses a stock-tracked product, as its domain requires.
  No domain rule was relaxed to make acceptance pass.
- A supplier binding example appeared on every ID field. An earlier Flash-Lite
  candidate used a supplier receipt as the product ID; the browser test caught
  this before approval. Field guidance now identifies the expected entity, and
  the supplier-specific example appears only on supplier ID fields.
- Gemini 3 Flash's default high reasoning exceeded the original ten-second
  transport timeout on the adversarial approval prompt. A direct diagnostic also
  exceeded thirty seconds. With its explicitly selected `minimal` reasoning
  profile, the same request returned a review-only plan in 4.07 seconds; the
  dependent supplier/offer plan returned in 3.49 seconds. The runtime now accepts
  an optional validated `GEMINI_AGENT_THINKING_LEVEL` for supported Gemini 3
  models and keeps a bounded twenty-second transport deadline. Provider reasoning defaults remain
  unchanged when the profile is unset; Gemini 2.5 cannot receive that API field.
  The profile follows Google's [thinking controls](https://ai.google.dev/gemini-api/docs/thinking).
- Live browser turns are paced twenty seconds apart for provider per-minute
  limits. The browser's API-evaluation deadline allows the host's two bounded
  provider calls. This does not retry failed requests or relax semantic, database
  or approval assertions.

Provider availability was also observed directly. Gemini 2.5 Flash reached this
account's free-tier daily limit of twenty requests during diagnosis; the next
CRM browser request returned unavailable without changing business data. Gemini
2.5 Pro had zero free-tier quota, and Gemini 3.8 Flash reported high demand.
Listing a model does not establish usable account quota or available capacity.
These rejected requests are preserved in the local evidence, not counted as
completed scenarios.

## Reproduction

Use pinned Rust 1.96.0, Node 22.22.3 and wasm-bindgen-cli 0.2.129. Clear the
unavailable local sccache wrapper for Cargo/helper commands:

```bash
RUSTC_WRAPPER= node tools/rust/verify.mjs --offline --wasm
node --test tools/test/product-audit.test.mjs
RUSTC_WRAPPER= node tools/rust/build-slice.mjs
E2E_BROWSER=/usr/bin/brave E2E_BROWSER_NO_SANDBOX=1 \
  node tools/e2e/run-agent-live.mjs \
  --credentials /absolute/path/to/private.env \
  --model gemini-3-flash-preview --thinking-level minimal \
  --output target/e2e/live-new-run
```

The credential file must contain a literal `GEMINI_API_KEY` or
`GEMINI_AGENT_API_KEY`. Do not put the value in command arguments or chat.
Inference uses the actual provider's quota; business data are synthetic and all
databases/browser profiles are task-owned. The runner caps planning at six turns
and the host at two provider calls per turn. No deterministic planner diagnostic
flags may be combined with this acceptance. See the [tool instructions](../../tools/README.md#live-gemini-acceptance).

`--thinking-level` maps only to the child runtime. It is optional; omit it for
Gemini 2.5 or provider defaults. No permanent runtime or private credential file
was changed by selecting this acceptance profile.

## Evidence and remaining scope

| Verification | Observed result |
| --- | --- |
| Full current-source Rust gate | PASS; **1,146 tests**, 98 suites, zero failed/ignored; structure, docs, format, Clippy and WASM boundaries |
| Node helpers | PASS; 25 gate tests and 3 unchanged product-audit helper tests |
| Final Chromium/native run | PASS; **49 checks**, including **11 live-agent checks** over six model planning turns |
| JavaScript/WASM errors | Zero |
| Browser/database/server cleanup | PASS; fixture cleanup exit zero |
| Credential/evidence check | PASS; private provider key absent from 252 evidence/package files; no task-owned services left running |
| Fresh release | PASS; production server, disposable preview, WASM/bindgen and source/artifact provenance |
| Native distribution | PASS; **59 artifacts** recomputed and verified by Ops |

Final native candidate: `dist/native-candidate-20261105-agent-live-02`.

Source SHA-256:
`c299a8e192bcdbbe68a270ea7ada189cd6a856790c6a4a6c8e940c7c16e2a8dd`.

Final evidence:

- `target/agent-live-20261105/gate-full-03.log`
- `target/agent-live-20261105/thinking-protocol-tests.log`
- `target/agent-live-20261105/helper-tests-02.log`
- `target/agent-live-20261105/build-03.log`
- `target/agent-live-20261105/package-02.log`
- `target/agent-live-20261105/final-verification.json`
- `target/e2e/agent-live-20261105-05/result.json`
- `target/e2e/agent-live-20261105-05/agent-live.json`
- Reviewed plan and verified outcome screenshots in the same browser directory.

Earlier backend evidence remains under
`target/agent-live-20261105/backend-observed-flash-05.log`. Failed browser attempts
`target/e2e/agent-live-20261105-01/` through `-04/` are retained separately, including
the quota, grounding and timeout failures. Interrupted build/gate `-02` logs also
remain; its task-owned interrupted PostgreSQL fixture was explicitly stopped.

This sample validates the tested model/profile and six business goals. It does
not certify every available model, every possible business goal, transcription/audio
quality, real integration delivery, public deployment or production sizing.
The previously measured Commerce p95 target miss remains open. Website and
deployment remain deferred as requested; no public DNS, real customer data or
external business communications changed.
