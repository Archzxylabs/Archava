# Gemini planner schema finding — 9 November 2026

The real-provider failure recorded in the [native AI boundary acceptance](PRD_AI_NATIVE_BOUNDARY_ACCEPTANCE_2026-11-09.md)
was investigated separately. That failed run remains failed. The investigation
uses synthetic business records, the explicitly selected
`gemini-3-flash-preview` model with `minimal` thinking, Google's canonical
endpoint, and the existing private credential file. Credentials are never
printed or copied into evidence. These diagnostics are not whole-product or
live browser acceptance.

## Observed failures

The original transport returned one undifferentiated unavailable error. Safe
failure categories now distinguish configuration, capacity, timeout, transport,
HTTP status, rejected schema, oversized/invalid response, truncation and invalid
proposal. Upstream response text and request values are not retained in the
returned error. An owned HTTP/PostgreSQL regression proves failed planning
consumes its original budget, request status reports no committed turn, replay
does not call the provider again, and explicit manual work still functions.

An actual diagnostic then exposed upstream HTTP 400. A synthetic public-schema
experiment received HTTP 400 for the 62,511-byte combined 33-tool schema; a
small two-tool schema and a small generic envelope each received HTTP 200.
This isolates a rejected schema combination, without establishing an exact
provider size limit or its internal compiler cause. Google's documentation
also says sufficiently large or deeply nested schemas may be rejected.
See [Google structured output](https://ai.google.dev/gemini-api/docs/structured-output).

The generic envelope exposed another concrete defect: a supplier-result binding
arrived as a scalar instead of the required nested object. The host rejected it
before committing a goal or business effect. A format correction still failed;
safe structural diagnostics retained only the known field name, bounded step
indices, dependency indices and type flags. Rejected prose and business values
were not reflected into the next model request.

## Current correction and validation status

The decoding envelope now describes each registered input field once, with its
explicit nested types, rather than duplicating a complete step branch for every
action. The selected action's full contract remains in the approved input.
The host checks that action's required fields, unknown fields, monetary
validity, observed references, dependencies and domain envelope before recording
a proposal. Monetary values also pass the registered domain envelope, whose
existing form parser accepts exact integer strings; this is not an independent
strict JSON Schema validator for every primitive. A field allowed by another
action is not allowed by the selected action. Reviewed defaults must be stated
explicitly by the model.

At most one binding/dependency format correction can use the remaining planning
round. It shares the existing two-round budget with read expansion. There is no
third round, transport retry, approval or model substitution. Sharing policy and
current authority are checked around every call. Correction metadata excludes
failed output text, identifiers and business values.

The developer probe then returned a ready two-step supplier/offer proposal from
real Gemini. It requested no execution or approval. Two schema unit tests passed.
The preceding correction implementation passed 32 owned Ava integration tests;
the final shared-field schema and strengthened negative case still need the
fresh full gate and native/browser/live acceptance underway on source
`d4f032453b612b34a9b2414143adf1d2771b4277f20fc0b2afa9918f957d45f1`.
An earlier native package does not accept these new changes.

Evidence is retained under `target/prd-completion-20261107/`: the numbered
`planner-live-diagnostic-01` through `06` logs, public-schema/variant experiment
JSON, failure recovery and envelope integration logs, and
`planner-shared-source-01.json`. Failures were not overwritten. Six-turn live
execution, broader model evaluation, cost/availability reporting and all other
open [delivery requirements](PRD_DELIVERY_REGISTER.v1.json) remain separate.

## Subsequent source-bound recheck

Source `d4f032…` subsequently passed the full gate: 1,336 Rust tests/99 suites,
36 helper tests, formatting, Clippy and both WASM boundaries. Its new 81-artifact
native package passed 191 browser checks, all 77 observed product probes,
30 Ava checks, five load repetitions, encrypted restore and both previous-profile
upgrade/rollback cases. These bounded results remain tied to that source.

The separate six-turn Gemini run passed its first five live checks, including
an approved supplier/offer and independent SQL/replay checks, then failed the
second planning turn with `unknown_agent_action_field`. It remains failed;
the unknown field's name was not retained. The [next finding](PRD_PRIOR_CONTEXT_FINDING_2026-11-09.md)
records the closed-contract correction, a separately confirmed historical
disclosure problem and focused rechecks. The newer implementation requires
another fresh release acceptance; no earlier passing package receives it
automatically.
