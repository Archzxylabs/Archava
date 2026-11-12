# Local gateway clearing and shared-book acceptance — 2026-11-12

The supported manual-bank profile now passes the native fee/refund workflow and
the unchanged one-company/book B01 oracle. This accepts the bounded local flows
below; whole-product readiness, full financial close and complete live-provider
acceptance remain open.

## Exact source and package

- Source: `c7e4b8cff4aa4a3c440fec070cdaa23ba5d2cd58708434f5a79f043fe53113e1`.
- Package: `dist/native-candidate-20261112-prd-gateway-clearing-02`, 86 verified artifacts.
- Release-profile SHA: `412b4fcc5a8a18eabd64afe31cd10f94333fd278a9bda3e28be924bd15771d53`.
- 74 SQL files, 67 mounted migrations; Accounting010 appends after the accepted supplier-payment profile without editing prior bytes.
- Full current-source gate: actual exit zero, 1400 Rust tests/99 suites, zero ignored, 36 helper tests, formatting, Clippy with warnings denied and both WASM checks.
- Native focused gate: actual zero/5 checks. Broad gate: actual zero/215 checks, including 77 observed product requirements; no browser exceptions and cleanup zero.

Local source-bound records are `clearing-release-job-03.json`,
`clearing-validation-job-04.json`, `clearing-final-full-gate-job-01.json` and
`clearing-verification-final-01.json` under `target/prd-completion-20261107`.
The validation controller's aggregate exit is one because its real-model step
failed on upstream429; the complete controller is not an all-pass result.

## Actual financial journeys

B01 starts with bank/equity1000000 whole IDR, buys ten units at100000, sells four
at150000, records the issued invoice and authenticated owned-gateway collection,
then posts and matches actual bank collection and supplier payment through the
native review forms. Before the physical return, bank600000, inventory600000,
profit200000, stock6 and AR/AP/clearing zero are checked independently.

One original-cost unit is then restocked; the verified refund, customer correction
and actual refund bank row are recognized separately. Final bank450000,
inventory700000, assets and equity1150000, profit150000, stock7 and AR/AP/clearing
zero match the unchanged oracle. All eight captured sources are posted; all three
actual bank rows are matched. Book units multiply whole IDR by100 exactly.
The synthetic refund bank row is dated the following day to model cash lag; the
test's explicit report range includes that date. It is not a current-day balance
claim.

A separate company/book collects30000 IDR with an explicit200 fee, refunds10000
with an explicit100 fee, and records the actual net bank movements. Native
three-leg reviews, separate posting and exact cash-leg matches produce total
expense300, net profit19700, bank29700 including an explicit10000 opening,
AR/clearing zero and no repeated revenue recognition. Mobile screenshots were
actually inspected: gross, fee, net bank money and all three journal legs are
readable at390px. These are owned authenticated HTTP gateway fixtures, not a live
payment provider or actual money transfer.

The earlier [native fee form defect](PRD_GATEWAY_FEE_NATIVE_FINDING_2026-11-12.md)
is preserved as failed evidence. Its fix initializes the optional account/version
fields before selection; a zero fee clears that account.

## Restart, installation and recovery

The actual production executable rereads both books' exact reports after restart
and recovers original draft receipts at revision one with unchanged source/bank
references. Encrypted restore matches row counts and digests across all28 financial
tables, including four supplier-payment/return rows and four clearing rows.
External effects remain held; archives are0600 with no plaintext staging left.

Two real prior packages upgrade by eight and six declared migrations while
preserving15 and19 original financial tables, old commands and scoped authority.
Existing users receive no implicit bank/payment/clearing permissions or history.
The prior defective-bank profile adds two migrations, derives its original
reference from immutable evidence and preserves the historical receipt/audit.
Fresh installed owners exercise actual supplier payments and clearing reads
without administrative grant repairs; this fresh clearing check covers reads and
permissions, not a separate positive verified-source settlement.

Two independent cold consumers pass12 tests; two independently built persistent
clients pass11 checks against the native package. They use shipped archives,
separate roots/targets and owned synthetic databases. Core archive SHA is
`91e849f5e33453556e1d73c1c6b18e6ad2a1b2fd6036764468e8c3236fc5769b`;
domain archive SHA is
`66847980ac6502175553b1ab40ddf051e4c65bb7b95658046480e65457f7ee70`.

## Remaining scope

The same real Gemini profile passed the original English CRM label update with
one provider call and no reads, plus reviewed procurement/replan and clarification.
Its fifth planning request received upstream429/host503; the sixth read-only
request was not attempted. Eight checks passed, but the six-turn live suite did
not. No retry loop, key/model substitution or deterministic-provider replacement
was used.

Accounting still lacks a visible Operations tab in this source; the tested
financial helpers use direct routes. That discovery fix is staged with future
due terms/aging. Complete source-to-GL/inventory/bank close, formal credit
documents, opening imports, jurisdictional review, grouped/FX payouts and any
promised real integrations remain separate. See
[gateway clearing](../modules/GATEWAY_BANK_CLEARING.md) and the
[whole delivery register](PRD_DELIVERY_REGISTER.v1.json). The215 local checks and
77 observed product requirements do not accept every PRD requirement.
