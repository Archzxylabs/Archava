# Repeated CRM organization-label planning failure — 2026-11-11

The real Gemini browser run still failed a supported contact-label update after
the field's static semantics were clarified. The focused live diagnostic passing
does not establish that the full populated workflow works.

The failed native source is
`6b67fab8be6f01b9215015d110ecbe4a2a698b965a7abb38ff9c7665dc346ab5`.
`target/e2e/prd-payables-live-20261111-02` and durable validation job 03 retain the
failure with actual process exit one. The same real run had already completed
reviewed supplier/offer creation, replay, durable preference recall and a new
approved offer-cost replan. Planning itself left the observed business row counts
and digests unchanged.

The next request asked Ava to set contact Ada Lovelace's organization to a supplied
new text label and prepare the change for approval. The current human could read
the target through the real contact search endpoint. Planning returned 502,
`agent_lookup_budget_exceeded`: two model calls/rounds had requested the same
Organizations collection read, with no new request on the second round. The
maximum remained three calls. Query text was intentionally withheld, so the
retained metadata does not prove which search term the model repeated.

The public CRM contract correctly distinguishes this free-text contact field
from managed Organizations and verified affiliations. The new description is
preserved in the planner schema. A separate strengthened probe with 110 contacts
and 110 Organizations had passed with three calls, but included a distinct
managed organization named Ada Lovelace. Whether an empty Organizations match
affects the failure is an untested hypothesis, not a verified root cause.

Next work must reproduce and repair the behavior while preserving current read
authority, closed provider projection, private query/history withholding,
approval and the three-call/eight-read limits. Do not increase budgets, substitute
a model/key, alter the failed customer request, narrow the acceptance fixture, or
count deterministic provider fixtures as real-model acceptance. The complete live
journey must be repeated after the fix. The wider agentic PRD remains partial.

## Follow-up diagnostics — 2026-11-12

The separate probe with 110 unrelated Organizations and no same-name organization
passed with three provider calls and two reads (diagnostic11, actual exit zero).
A further diagnostic linked all 110 source organizations to managed companies
while leaving company disclosure unapproved. Its retained log also records a
single successful revision-two update; the original process exit was not
collected after context rollover. Neither diagnostic reproduced the full
populated browser failure, so neither establishes its root cause.

The next combined source preserves public action-level descriptions in planner
contracts and documents contact organization labels at both action and field
level. Generic planning instructions select the action by its documented meaning
before requesting the facts that action needs. Privacy, current authority,
approval and the three-call/eight-read caps remain enforced. Full unchanged live
acceptance is pending; the candidate is not yet a proven repair.

## Current-source live result — 2026-11-12

Source `c7e4b8cff4aa4a3c440fec070cdaa23ba5d2cd58708434f5a79f043fe53113e1`
passed the unchanged English contact-label request in the full populated native
browser journey: one real Gemini call, no lookup, approval and exactly one contact
revision update. This verifies that supported case after the semantic contract
changes; it does not isolate a statistical root cause or guarantee all phrasings.

The same official `gemini-3-flash-preview`/minimal run passed eight checks through
four planning turns, then upstream429 became host503 on the fifth request. The
sixth read-only turn was not attempted. The live suite remains failed/verifiedfalse
and was not retried or replaced by another key/model. Local focused/broad/native
recovery and consumer checks passed separately. See the
[current bounded acceptance](PRD_GATEWAY_CLEARING_ACCEPTANCE_2026-11-12.md).
