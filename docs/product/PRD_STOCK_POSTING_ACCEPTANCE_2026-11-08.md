# Original stock cost and shared financial journey — 8 November 2026

Source `a5389fba175725c0a987591814d5f7ccf48b43861e4ac2d728dc65e6433d24f0`
contains 506 serving inputs. Distribution
`dist/native-candidate-20261108-prd-stock-readable-01` verified 80 artifacts.
The evidence binding is
`target/prd-completion-20261107/stock-readable-verification-01.json`.
This accepts the bounded local behaviors below, not the whole PRD, production
hosting, live provider operation or a complete financial ERP.

## Verification scope

The current source passed 611 Rust library tests across 22 result suites,
36 helper tests, formatting, workspace/all-target Clippy and both WASM checks.
The preceding source `03021cfedccbb1afe302609d2f41bc3b8a46e5fa9524e40b1b81396a0de22390`
passed the full 1,302 Rust tests/99 suites, including 28 Accounting router and
PostgreSQL tests. The exact serving-input difference is limited to
`crates/slice/src/operations/accounting.rs` and `static/workspace.css`:
financial review markup and responsive presentation. Backend, Domain, migration,
permission and integration-test inputs are unchanged. That full run remains
attributed to its original source; it is not represented as a full current-source
run. The current native server, preview and WASM were rebuilt together.

The final native/Chromium result
`target/e2e/prd-stock-readable-20261108-03/result.json` passed **181 root checks,
77 scoped product probes and 30 Ava checks**, with no page errors and owned
PostgreSQL cleanup exit 0. Its 70 public tooling/client inputs are preserved in
`target/prd-completion-20261107/stock-readable-helpers-03`.

The browser runner was deliberately given an invalid inherited preview
attachment. It ignored that attachment, created a fresh disposable fixture and
cleaned up its own database. This test never selected an application/shared
database. The standalone exploratory scene's initial missing fixture grants
failed with 403; its failure and successful corrected authorization setup are
retained separately. No administrator fabricated its financial sources.

## Accepted financial behavior

The [stock cost bridge](../modules/STOCK_COST_POSTING.md) uses an explicitly
configured native IDR book and reviewed future whole-Rupiah source policies.
Actual acquired layers retain their original inventory accounts. Fulfillment
captures actual original costs; restock reverses the original recognized COGS
account after a later policy change; discard creates no inventory/COGS reversal.
Unknown origin or unrecognized historical COGS is refused atomically.

Source drafting verifies every original account allocation and current reviewed
account revision. Caller money, forged source claims, stale accounts, duplicate
live drafts and revoked authority refuse. Posting is separate. Router tests
prove an independent second human can post while the author cannot under the
independent-approver policy. Browser book fixtures explicitly use the
single-operator policy; they do not claim independent-approver UI acceptance.
First policy setup and concurrent fulfillment/policy updates share a tested
company-before-book fence. Captured sources block close until posted; a closed
period refuses a stock change with complete operating rollback.

Purchase and stock source reviews show exact amounts, source date, original
named accounts and proposed debit/credit before closed technical references.
The [original mobile finding](PRD_STOCK_SOURCE_FINDING_2026-11-08.md) is preserved.
Actual updated screenshots show complete mobile account/debit/credit cards at
390 px and intact desktop currency/amount values. The stronger browser assertion
checks internal journal width as well as document width. This is scoped visual
acceptance, not general accessibility certification.

## One company, one book

A held-out API scene was added to the maintained
[store finance cycle](../../tools/e2e/store-finance-cycle.mjs) regression. It uses
all three source policies in one real company/book and the owned HTTP gateway:

1. Receive two units at IDR 7,000 plus IDR 101 acquisition fee; match the actual
   IDR 14,000 supplier goods claim.
2. Fulfill both units, issue the actual IDR 30,000 customer invoice and verify
   its IDR 30,000 collection through the gateway transport.
3. Restock one original unit at IDR 7,051, verify a cumulative IDR 15,000 refund,
   then record the reviewed IDR 15,000 customer invoice correction.
4. Separately draft/post all eight captured monetary sources and close the
   captured-source period.

The independent whole-Rupiah oracle and API reports agree:

| Control | Final amount |
| --- | ---: |
| Physical stock | 1 unit |
| Inventory | IDR 7,051 |
| Payment clearing | IDR 15,000 |
| Customer receivable | IDR 0 |
| Supplier payable | IDR 14,000 |
| Separate fee accrual | IDR 101 |
| Net revenue | IDR 15,000 |
| Net COGS | IDR 7,050 |
| Profit | IDR 7,950 |
| Assets | IDR 22,051 |
| Liabilities | IDR 14,101 |

All eight monetary sources are posted; trial balance and balance sheet balance.
The real production binary restart preserves these reports. Payment clearing
is not bank cash: no bank balance or full bank reconciliation is invented.
The correction is the supported invoice adjustment; this does not accept a
complete formal credit-note/allocation lifecycle or supplier payment.

## Recovery, reuse and measured load

Encrypted restore compared all 19 financial table digests and served an
independently checked stock report after login. Actual prior distribution
`dist/native-candidate-20261108-prd-purchase-review-01` created its own purchase
receipt/bill journals, acquired layers, fulfillment and legacy restock. Upgrade
applied exactly the two declared migrations, preserved its 15 original financial
tables and original cost records, attached only the exact acquired-layer
metadata, and invented no historical COGS recognition. A second apply did
nothing. Its encrypted backup restored into another database and served with
the prior binary.

Two fresh independent public-crate consumers passed five tests each. Both use
identical packaged Core/Domain contracts with different labels and
Projects/Commerce extensions. Two freshly built persistent clients then passed
11 configured native scenarios on separate databases, including current
permission revocation, stale/forged observations, durable replay, actual restart
and cross-installation isolation. PostgreSQL cleanup exited 0 and owned scratch
was removed. These are CLI/public-contract composition proofs, not complete
industry applications, every Owner workflow or every CFG-03 obligation.

The final local workload used 10,000 synthetic contacts, 2,000 seeded products,
five read repetitions and 200 unique contact writes. Commerce p95 was
13.81/13.31/14.62/14.15/13.54 ms; Contacts p95 was
3.21/3.25/2.62/2.61/2.55 ms; writes p95 was 4.89 ms. Every measured target passed
with zero request errors. Hardware and exact workload are in the result; these
measurements do not certify production sizing or availability.

Remaining delivery includes approved opening/import, counted stock adjustments,
full inventory/bank/subledger reconciliation, AP/AR terms/aging and payments,
formal credit documents, localization, complete per-model/hybrid journeys,
Owner data-sharing/retention controls and financial Ava planning/preview.
Stored Accounting records remain withheld from the provider planning context.
The scoped Ava checks above do not accept agentic financial execution. Actual
customer/provider/host acceptance remains separate from these synthetic proofs.
