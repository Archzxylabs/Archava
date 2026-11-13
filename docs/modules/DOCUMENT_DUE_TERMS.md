# Original-document due terms and dated subledger reports

The mounted native book supports explicit due terms for an original issued
customer invoice or matched supplier bill. `invoice_terms.set` and
`supplier_terms.set` have separate source collections because Billing and
Purchasing UUID namespaces can overlap. The server derives the document kind,
company, book, source and current document revision. A request supplies the
observed document/terms versions, an effective business date, an explicit
`due_on` date or `unknown` condition, and a review reason. Missing input never
chooses an agreement or clears an existing date.

Accounting011/012 append immutable `document_terms` versions, exact receipt and
audit claims, FORCE RLS, write fences and startup metadata checks. They grant
no existing user new authority and import no old agreements. Applied011 bytes
were preserved when the first integration run exposed the audit-kind omission;
append012 permits the new resource. Terms change dates only. They do not alter
money, allocate a payment or post a journal. A source may receive dates before
posting, including a zero-value original that later receives a correction.

## Authority and review

Read and write require current human book/company access and explicit
`terms.read` / `terms.write`; supplier data additionally requires
`payables.read`. Writes serialize against the book, require an open effective
period, retain original-source identity and append the next terms version.
Historical versions cannot be edited or deleted. Receipt recovery rechecks
current authority and returns the original immutable condition even after a
later change.

The native Accounting tab is available in Operations. Source selection loads
the current document and terms versions. The form starts with no condition
selected, clears the proposal when its source changes, and removes the date
when the reviewer chooses unknown. The review shows the original document,
source date, latest agreement/effective date and proposed agreement/effective
date. A matching acknowledgement binds the source, document kind, both
versions, proposed condition and effective date.

`invoice_term_sources`, `supplier_term_sources` and `document_terms` are private
native collections. The closed provider projection withholds them even when
all existing data classes are enabled. A current payable read without
`terms.read` reports date visibility as unavailable rather than claiming that
the actual due agreement is unknown.

## Reports through an explicit cutoff

`GET /erp/accounting/subledger-report` accepts the session tenant, book ID,
`receivable` / `payable` side, explicit `asOf` date and optional UUID cursor /
page limit. It requires `reports.read` and `terms.read`, plus `payables.read`
for supplier positions. The read holds the authorized book's SHARE lock.

Customer positions sum posted original issue, correction and void effects,
less signed verified collection/refund effects through the same journal date.
Supplier positions sum posted bill/void effects, less posted supplier payments
and plus posted returns. Draft payments/returns do not change historical paid
amounts. An operating void without its posted financial correction cannot
erase recognized liability. Gateway-to-bank clearing does not change AR again.
Current operating balances and current payable reservation summaries are not
used as historical balances.

Every qualifying original document participates in totals before detail is
paged. Positive balances are separated into unknown dates, not overdue,
1–30 / 31–60 / 61–90 / over90 days overdue. Negative credits remain separate;
they do not reduce an overdue bucket. Exact integer strings preserve totals
beyond JavaScript's safe integer range.

The independent control comparison derives expected balances from source
amounts and structured settlements, with the original historical account
mapping and currently declared role accounts. It compares each account with
actual posted GL balances and checks that the document total matches the
expected controls. Generic openings/adjustments remain visible differences;
there is no balancing plug. A positive unpaid balance is valid. Captured
monetary effects not posted through the cutoff are disclosed separately.

The selected due agreement is the latest immutable revision effective by the
business cutoff, under current knowledge. Its revision and recorded timestamp
are disclosed. A later authorized correction can change that business-date
view; this is not historical knowledge-time reconstruction. Future-only
capture does not prove that legacy opening documents were imported.

## Current validation boundary

The isolated source `dcf6d93393066aac010aa47f63b55da1b62c5310199540ebe0696e1e85df405e`
passed all55 Accounting PostgreSQL tests, all45 Slice library tests, Clippy for
Domain/Adapters/Host/Slice with all targets and warnings denied, and both WASM
checks. The SQL tests cover original sources, date/version races, unknown
clearing, replay, revocation, scope, RLS, missing claims, startup guard tampering,
later-dated payments/returns, unposted voids, historical mappings, generic
control differences and101 real invoice documents with exact totals beyond
JavaScript integer precision.

That source is selectively merged, preserving the previous gateway-fee fix.
The root structure gate passes. A fresh complete workspace gate, native package,
real Chromium review/report, installed upgrade/recovery and consumer evidence
are still required for this merged release. Earlier
[gateway acceptance](../product/PRD_GATEWAY_CLEARING_ACCEPTANCE_2026-11-12.md)
belongs to its previous source. Full financial close, legacy imports, formal
credit documents, statutory review and real integrations remain separate in the
[whole-product delivery register](../product/PRD_DELIVERY_REGISTER.v1.json).


A follow-up serialization probe exposed a native detail-label defect: the four
overdue enum variants serialize without extra underscores around digits, while
the first UI mapping expected those separators. API buckets, exact totals and
controls were correct; the overdue detail label would display unavailable.
Source `e1a36367ce05ec6a4723876a61a78efd079b8b281dcd7d8bca5a76633a97a3c0`
corrects that mapping and tests all eight real Domain enum serializations against
the customer labels. All46 Slice library tests and Slice all-target Clippy pass
in the isolated check. Its fresh complete release remains pending. The earlier
full-workspace run belongs to its original compiled source and its controller
must reject source drift before packaging. No earlier pass is reused as final
acceptance of this correction.

The corrected source subsequently passes the complete root gate with actual
exit zero: 1425 Rust tests in99 suites, zero ignored, 36 helper tests, workspace
Clippy with warnings denied and both WASM checks. Source inputs stayed unchanged.
The fresh release build/package and browser/install/recovery/consumer pipeline
are still running. A full source gate is not a completed native acceptance or
completion of the whole PRD.

The later [source-bound local acceptance](../product/PRD_DUE_TERMS_ACCEPTANCE_2026-11-12.md) completes the fresh package, focused7/5 and broad221 Chromium checks, production restart, declared prior upgrades,29-table encrypted recovery and independent cold/persistent consumers for sourcee1a36367. Real Gemini and the remaining full-profile/whole-product requirements stay separate. Earlier in-progress boundaries above remain historical.
