# Original due terms and dated subledgers — local acceptance, 2026-11-12

This accepts the recorded local source/profile below. Whole-product completion,
complete financial close and real-provider acceptance remain separate.

- Source: `e1a36367ce05ec6a4723876a61a78efd079b8b281dcd7d8bca5a76633a97a3c0`, 546 public inputs.
- Native package: `dist/native-candidate-20261112-prd-due-terms-02`, 88 independently verified artifacts.
- Release-profile SHA: `2380830e9fdc0fb0e3f120efa7ac7b1e89c1e0717c84cf2c9b102063ac89c5e6`.
- Full gate: actual zero, 1425 Rust tests/99 suites, zero ignored, 36 helper tests, formatting, all-target workspace Clippy with warnings denied and both WASM checks.
- Focused terms browser: actual zero/7 checks. Focused original clearing browser: actual zero/5 checks. Broad browser: actual zero/221 checks, including all77 observed product probes; no browser exceptions and fixture cleanup zero.

Source-bound local records are `finance-terms-release-job-02.json`,
`finance-terms-validation-job-02.json` and
`finance-terms-verification-final-02.json` under
`target/prd-completion-20261107`. The controller checks source and46 frozen helper
files before/after jobs. Current source and helpers remain unchanged.

## Actual dates, sources and reports

The Accounting entry is discovered by clicking its actual Operations tab.
An owned HTTP/SQL/native journey creates two real issued, posted customer
invoices in a separate company/book, totaling12500 whole IDR. Their initial
unknown-date balance is1250000 book minor units. Reviewing the first invoice's
explicit date moves10000 IDR into the1–30-day overdue bucket while2500 IDR stays
unknown. Independent original-source and GL controls agree; terms change no
amount or payment balance.

Native source selection binds the live document and terms versions. Changing
source clears the proposed condition and effective date. Explicitly choosing
unknown removes the old date. The current report then shows12500 IDR unknown,
while recovery still returns the original known-date receipt at revision one.
The original supplier bill in the unchanged B01 book receives its own reviewed
dates and remains financially settled. Four immutable terms rows are checked
independently in SQL.

The390px date review and actual native report screenshots were opened and
inspected. Source date, document version, prior/proposed terms and effective
date are readable. The report shows10000 IDR overdue and2500 IDR unknown before
the clear, then12500 IDR unknown with zero control difference. Actual mobile
gateway fee journal screenshots were also rechecked: clearing30000, bank29800
and expense200 are separately readable. The earlier
[gateway acceptance](PRD_GATEWAY_CLEARING_ACCEPTANCE_2026-11-12.md) B01 oracle
remains unchanged, including450000 bank/700000 inventory/150000 profit and
stock7 at the explicitly inclusive cutoff.

Actual PostgreSQL tests cover posted versus draft/captured corrections, an
operating supplier void before financial posting, later-dated supplier payments
and returns, effective-date terms, historical/current account mappings and a
generic AR adjustment that remains an explicit100-minor control difference.
A101-invoice test uses valid existing price/quantity/line limits and checks
100+1 pages with unchanged whole-book totals of12000000294000000 minor units.
Claim forgery without matching receipt/audit, tenant isolation, scoped/service
actors, revocation and disabled startup guards fail closed.

## Restart, installed profiles and recovery

The production executable rereads the dated report after restart and recovers
the old known-date receipt despite the newer unknown-date version. Encrypted
restore preserves counts/digests across all29 financial tables, including four
nonempty terms rows, four supplier settlements and four gateway settlements.
Restored authenticated reports retain1250000 minor outstanding, current unknown
terms and the original known-date revision-one receipt. External effects remain
held, backup files are0600 and plaintext staging is removed.

Two actual prior packages upgrade by ten and eight declared migrations,
preserving15 and19 prior financial tables. Existing users receive no implicit
terms grants or historical agreements. Fresh installed owners write actual
supplier dates and read the as-of report using the installed non-owner runtime,
without administrator grant repair. The legacy bank-receipt profile adds four
migrations and preserves its original receipt/audit and derived original match
reference.

Two independently rooted/targeted cold consumers pass12 tests against the same
shipped Core/Domain archives. Two independently built persistent clients pass
11 native checks; owned PostgreSQL cleanup is zero and scratch is removed.
Core archive SHA is
`91e849f5e33453556e1d73c1c6b18e6ad2a1b2fd6036764468e8c3236fc5769b`;
Domain archive SHA is
`8a4d04e1356e60febdd544a5ce086f467340b1d4ca52c81ba7a61959da366d11`.

Five local read repetitions on10000 contacts/2000 products have zero request
errors. Contact p95 values are3.22/3.50/3.19/3.14/3.04ms; Commerce values are
16.11/16.30/16.26/15.19/15.78ms, under the unchanged50ms local target. These
synthetic measurements do not establish production hardware sizing or latency.

## Retained findings and limits

The first projection used `created_at` instead of actual `recorded_at`; focused
SQL execution exposed it and the corrected source passes. Applied Accounting011
was not edited when its missing audit-kind permission was discovered; append012
fixes that allowlist. Earlier failed outputs remain retained.

A compiled Domain serialization probe then exposed four incorrect native
overdue labels. The fix tests all eight enum-to-JSON-to-customer-label mappings.
The preceding source's full gate passes1424 tests, but its release controller
correctly rejects source drift before building/packaging. It is retained as
rejected-profile evidence, not counted as acceptance of the corrected source.

No real Gemini request was attempted in this pipeline after the previously
observed upstream429. The earlier six-turn live suite remains incomplete.
Terms use current immutable history selected by effective business date; they
are not historical knowledge-time reconstruction. Complete inventory/GL/bank
cooperation, selected-profile financial close, opening imports, formal credit
documents, jurisdictional review and promised real integrations remain in the
[delivery register](PRD_DELIVERY_REGISTER.v1.json). The77 observed product probes
do not complete all35 PRD requirements. No public deployment was attempted.
