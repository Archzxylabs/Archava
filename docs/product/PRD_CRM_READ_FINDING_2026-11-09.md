# CRM read progression and provider limit — 9 November 2026

Source `60e65f…` passed the full gate with **1,341 Rust tests/99 suites**,
36 helper tests, formatting, Clippy and both WASM boundaries. Its 81-artifact
package passed **191 native browser checks, 77 observed product probes and
30 Ava checks**, with empty error lists and PostgreSQL cleanup 0. Encrypted
restore and both prior-profile upgrade/rollback cases passed. These bounded
results do not accept later source changes or the full PRD.

The real-Gemini run `target/e2e/prd-agent-contract-live-20261109-01` passed its
first six live checks: approved supplier/offer, actual SQL result, replay,
receipt-backed report, preferences and the formerly failing approved follow-up
to 4200. The third English CRM planning turn then failed with
`agent_lookup_budget_exceeded`. The run remains **failed**: 148 root checks,
three planning turns, no page errors, cleanup 0. The exact sequence of requested
reads was not retained, so its particular cause is not inferred from that error.
Complete row digests, including values and revisions, strengthened the planning
side-effect oracle beyond row counts in this run.

## Provider availability

A separate finite CRM diagnostic prepared an owned fixture with 110 unrelated
contacts ahead of the target. Its first inference request received upstream
**HTTP 429**, exposed safely as HTTP 503 with `upstreamStatus:429`.
`planner-live-diagnostic-08.log` preserves this result. It did not diagnose the
CRM model's read sequence. No subsequent Google inference call was made during
the local corrections below. No model/key substitution made the failed run pass.
This evidence does not determine whether the account limit is daily or temporary,
or its reset time.

## Local correction and evidence

The total budget remains **three provider calls per turn**, hence at most
18 for the six-turn profile. Optional model routing consumes one slot. Local
routing leaves three planning slots, permitting a distinct search, grounded
exact detail read and final plan. At most one format correction can share the
remaining slots. There are still at most eight unique typed read queries, and a
request adding no new read is refused. This is finite read progression, not a
transport retry or an extra fourth call.

Model input explicitly states the current round, remaining query budget and
whether another read is possible. Lookup coverage reports only public collection
names/status and counts computed after approved field/company projection;
query text, paths and old identifiers remain withheld. Successful and refused
plans expose routing/planning/total call counts for safe diagnosis.

The owned HTTP/PostgreSQL regression places the target beyond the first page,
checks the three actual provider requests, and verifies no write before human
approval, one exact organization/revision change, and replay without inference.
It also proves routing plus two planning requests cannot obtain a fourth call,
and a repeated identical expansion stops after two. This case and the complete
35-scenario Ava suite passed before the subsequent snapshot correction.

## Current authority between rounds

A further owned test reassigns a Support case during the first model request,
then expands Commerce. The second request originally replayed the unrelated stale
Support snapshot. Its actual synthetic payload is archived in
`inflight-scope-finding-01/manifest.json` under the task evidence directory.

Later rounds now refresh all authorized source reads and previously grounded
typed references before disclosure. A known ID permits a scoped read attempt;
current endpoint authority still determines whether its row can be returned.
The focused negative reproduction now passes, as do 21 agent library units.
The combined regression and current release acceptance remain in progress.
See [the historical-context finding](PRD_PRIOR_CONTEXT_FINDING_2026-11-09.md).

The next real-provider harness also verifies that the intended CRM record is
readable by the current human before invoking the model. An administrative SQL
row alone is insufficient. Real six-turn acceptance remains open while the
provider is limited. Manual work and other local PRD implementation continue;
see [the delivery register](PRD_DELIVERY_REGISTER.v1.json).

## Subsequent local checkpoint

The combined Ava regression subsequently passed **36 tests, zero failed or
ignored**, in `fresh-round-agent-01.log` (99.58 seconds). Workspace all-target
Clippy also passed with warnings denied in `fresh-round-clippy-01.log`.
The source snapshot is
`7d23728e3c7eda273f200d057e16b319f984e88c781897ed2583713633f44c81`
(516 inputs), recorded in `fresh-round-source-01.json`. This local checkpoint
covers the fresh-read and three-total-call corrections; it has no fresh release
build, native browser acceptance or successful six-turn real-provider run yet.
