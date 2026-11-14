# Historical plan disclosure finding — 9 November 2026

A synthetic HTTP/PostgreSQL regression confirmed that historical action inputs
could disclose derived record text after row-level access changed. This was
separate from the previously tested company/category projection and grant
revocation cases. It does not involve real customer data or transmission to
Google: the proof captures actual requests at an owned loopback provider.

## Reproduction

Bob creates a Support case and assigns it to Alice. Alice has assigned-only
access. Her natural-language plan includes a synthetic marker derived from the
case description. Bob then reassigns the case without changing Alice's
membership or the Owner configuration.

Alice's current case detail is refused and the next provider context contains no
case rows. However, `previousGoal.steps[].input.reason` still includes the old
marker. The membership fingerprint did not represent this record assignment
change. A new disclosure of historical input cannot rely on that fingerprint
alone.

`prior-reassignment-negative-02.log` and `03.log` both fail the actual-wire
assertion with exit 101. The latter prints the small synthetic `previousGoal`
received by the provider. Run 01 failed only because the new test initially
expected a wrapped receipt; Support returns its receipt directly. That setup
failure is preserved and is not counted as security evidence.

The old production path was equivalent to source `d4f032…`, which had passed
1,336 Rust tests and the 191-check native browser run. Those bounded checks did
not exercise this particular historical row-scope transition. The failing
source, test and logs are archived in
`target/prd-completion-20261107/prior-reassignment-finding-01/manifest.json`.

## Implemented correction

Provider history now withholds every historical action input, identifier,
objective, preference, summary and raw outcome. Fresh authorized reads supply
business values and target records. Historical note selection does not bypass
this boundary. Local user-owned goal retention is separate and was not changed
or accepted by this provider-boundary fix.

With selected business-fact permission, the provider can receive only registered
tool names, bounded step indices and trusted completion flags. A completion flag
requires the matching approved plan hash, mission identity, step index/tool,
passed checkpoint and verified business payload. Overall goal state alone cannot
prove a completed step. Raw receipts and historical record IDs are withheld.

The original reassignment regression now passes. Eight privacy units also pass,
including partial completion, missing approval, a mismatched mission and an
unverified passed checkpoint. The combined current Ava suite passes 34 scenarios.
These focused results still require the fresh full/native release acceptance
underway; the earlier native package does not accept this correction.

## Concurrent live-model finding

The separate real-Gemini run on `d4f032…` passed the initial supplier/offer plan,
explicit native approval, independent SQL result, replay, receipt-backed report
and stored preference checks. It then failed its second planning turn with
`unknown_agent_action_field`. The particular extra field was not captured; its
name is not inferred from that response. The failed run remains failed, with
147 root checks, no browser errors and PostgreSQL cleanup 0.

The current repair keeps selected action contracts closed. One remaining
planning round may use their exact schemas and safe contract guidance. It never
resends rejected prose or business values, approves a proposal or extends the
two-round budget. Model primitive values are checked separately from the native
form parser. An owned regression proves an extra-field repair, exact approved
price change and refusal of repeated numeric-string money without a third call.

A new two-turn developer probe with real Gemini then creates one offer at 3500
and changes it to 4200 only after explicit approvals. Both plans use one model
round, and independent SQL verifies the result. This supports the new path;
six-turn native browser acceptance is still required. See
[the schema investigation](PRD_GEMINI_SCHEMA_FINDING_2026-11-09.md) and
[the delivery register](PRD_DELIVERY_REGISTER.v1.json). Complete historical
memory, full business outcome semantics and whole-product readiness remain open.

## Additional in-flight record-scope regression

A further owned-provider test reassigns the same kind of Support case during the
first model request, then requests a Commerce lookup. Memberships remain unchanged.
The second provider input originally retained the unrelated old Support snapshot,
even though current case detail was already refused. The negative reproduction
prints the actual second synthetic context and is preserved in
`target/prd-completion-20261107/inflight-scope-finding-01/manifest.json`.

Later planning rounds now start with fresh authorized source reads, then replay
only already grounded typed lookups through current authority. Previously known
IDs allow a read attempt; they do not authorize returning or disclosing a row.
The original regression now passes. Data already sent in the first authorized
request cannot be recalled. This correction does not claim retroactive deletion
of local personal history or a completed real-provider acceptance run.
