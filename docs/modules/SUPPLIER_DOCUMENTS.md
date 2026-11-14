# Supplier goods documents

Purchasing captures a supplier's explicitly supplied goods claim against one
confirmed company-owned PO, then checks that claim against PO prices and
unallocated accepted receipt quantities. This prepares operational review evidence.
Capture and discrepancy review do not by themselves post an official payable,
settle a supplier or move physical stock. A claim may carry the
supplier's tax ([below](#tax-on-a-supplier-document)); capture stores it and posts nothing. A separately configured
[purchase posting policy](PURCHASE_POSTING.md) binds future confirmed POs to their
book and captures accepted goods, matched claims and void effects atomically.
An authorized accountant still reviews and separately posts each source. The
Purchasing reader receives `accountingStatus: not_inferred`, rather than an
unconditional claim that a document is unposted or already paid. Financial
records require current Accounting and company authority.


## Commands and states

All four use the existing Purchasing command envelope, durable request receipt,
actor/fingerprint binding, current authorization and transaction. Each command
returns its original ID/revision on replay, even after later goods arrive or the
claim changes. A held check is a committed review result; it is not an approval.

| Command | Reviewed input and result |
| --- | --- |
| `document.capture` | PO ID/revision, supplier ID, vendor reference/date/currency, declared goods total and 1–50 unique PO-line quantities/unit prices, optionally `taxCode` with `taxMinor`; creates `captured` version one |
| `document.revise` | Document and current PO revisions, corrected date/amount/lines and reason, optionally `taxCode` with `taxMinor`; `captured`, `held` or `void` becomes `captured` with a new version |
| `document.match` | Document/current PO revisions and review reason; exact evidence becomes `matched`, any supported discrepancy becomes `held` without allocations |
| `document.void` | Document revision and reason; releases active receipt allocations, records `void` and retains all previous evidence |

A matched claim must be voided before correction. Reopening a void claim is an
explicit correction to the same vendor reference; it does not create a duplicate
supplier invoice. Tenant/company/supplier/vendor reference uniqueness survives
void. References are normalized uppercase ASCII with collapsed spaces (1–120
characters). Dates are strict Gregorian `YYYY-MM-DD`, 1900–9999. The currency
is the existing PO's documentary currency ([trade currency](../architecture/MULTI_CURRENCY.md)),
never one selected on the claim: a well-formed three-letter code that differs
from the PO's is refused as a conflict, on capture and on revision. Quantity, unit-price and total bounds follow the Purchasing contract.
Vendor reference, supplier, source PO, owner, company identity and captured
company revision/code/name remain immutable.

The operator enters actual supplier claims; the application does not infer those
claims from ordered or received quantities. Matching checks base goods only.
Discounts, freight and other charges need separate review; no jurisdiction,
tax treatment or tolerance is guessed. Total or price discrepancies are retained
for review rather than silently changed to match the PO.

## Tax on a supplier document

Stage D of [tax on documents](../architecture/TAX.md). A claim may name one input
tax code of its company's book and carry the tax amount printed on the supplier's
document:

| Member | Meaning |
| --- | --- |
| `taxCode` | The code text of an input tax code of the company's book |
| `taxMinor` | The supplier's tax amount in the document's unit (whole Rupiah for IDR, the ISO minor unit otherwise), `0` or more |

Both members or neither (`supplier_document_tax_code_and_amount_together`). The
goods total and the claimed line prices stay the net of goods; the amount owed to
the supplier is `declaredTotal + taxMinor`. There is no price basis.

The version stored is the confirmed input version that applies on the document's
own `documentDate`. Without one the command is refused and nothing is written:
`supplier_document_tax_code_unknown` (no such code in the book),
`supplier_document_tax_code_not_input` (an output code),
`supplier_document_tax_code_not_confirmed` (entered, not confirmed) or
`supplier_document_tax_code_not_applicable` (no confirmed version covers the date).

The document stores the version (code id, code, version, rate and base fraction),
the entered tax and the tax Archava computes from the goods total with the
exclusive rule, `round(declaredTotal × base × rate)`. The entered amount is the one
owed and, once matched, posted. It need not equal the computed one; it may not be
negative or exceed the goods total (`supplier_document_tax_amount_out_of_bounds`).
The read returns both and the difference:

```json
"tax": {"codeId":"…","code":"VAT-IN","version":1,"kind":"input",
  "rate":{"numerator":11,"denominator":100},"base":{"numerator":1,"denominator":1},
  "enteredMinor":15399,"computedMinor":15400,"differenceMinor":-1,"totalMinor":155399,
  "enteredByCompany":true,"verifiedByArchava":false}
```

A document without tax has no `tax` member and is read as before. Each entry of
`versions` carries the `tax` (code, version, entered and computed amount) of its
own snapshot when it has one.

The tax belongs to a claim version. A match, a hold and a void keep it; a
correction (`document.revise`) states the code and amount of the new version, or
none, and nothing is carried over from the version it replaces. Matching is
unchanged: quantities, unit prices and the net total decide, and tax is no part of
the evidence.

PostgreSQL holds the same rules ([Purchasing 008](../../migrations/purchasing-pg/008_supplier_document_tax.sql)):
the snapshot must be the version that applies on the document date, the computed
tax is recomputed, the bound is checked and the tax cannot change outside a
captured version, also by direct SQL as the serving role. Existing documents and
their stored versions are untaxed and are not rewritten.

For the selected company the Purchasing workspace read returns `taxCodes`: code,
version, rate, base and date range of every confirmed or retired input version of
the company's book (the document date chooses among them), and `taxNotice`. It
needs `documents.read` and access to the company, not the Accounting `tax.read`
grant, and it carries no account and no person.

The claim form and the correction form offer those codes and a tax amount field in
the document's currency, show which version applies on the entered date, the
computed and the entered tax, the difference and the amount owed, and say that tax
codes are entered by the company and not verified by Archava. The document detail
shows goods, entered tax, computed tax, difference and the amount owed.

How a matched, taxed claim posts is in
[purchase posting](PURCHASE_POSTING.md#tax-on-a-supplier-bill). Witnesses:
[tax_purchases.rs](../../crates/host/tests/erp_accounting/tax_purchases.rs).
The browser journey with tax was not run.

## Matching and database invariants

The exact-goods policy requires claimed unit prices to equal immutable PO prices,
the declared total to equal the claim lines, and sufficient unallocated accepted
goods for every claimed line. Partial supplier documents can allocate part of a
receipt. Rejected goods never create capacity. Each allocation names a physical
receipt request/PO line and the matching document revision. No partial allocation
survives a held result. A check is bounded to 500 allocation rows; a claim requiring
more is held with an explicit allocation-limit reason.

Commands lock the source PO before the document. Physical receipt writes and
competing matches share that PO lock. PostgreSQL independently checks source
identity, immutable claims outside correction, state transitions, accepted
capacity, exact matched completeness, command/audit binding and complete version
snapshots. Allocation quantities/history cannot be edited or deleted; release
names a later void version. Version snapshots are append-only. All three new
tables have `FORCE ROW LEVEL SECURITY`; runtime grants are narrow and tenant binds
remain explicit. The new migration does not backfill or rewrite legacy documents.

Match evidence records current PO revision, claimed/computed total, ordered price,
accepted/rejected/already allocated/available quantity and durable discrepancy
reasons. Later receipts require a fresh reviewed PO revision and new check key;
they do not rewrite the original held receipt. Native history displays the latest
20 versions with receipt references; all versions remain stored.

## Authority, native operation and Ava

New `documents.read`/`documents.write` permissions are explicit; the migration does
not add them to existing memberships. Reads also require `orders.read` and
`receipts.read`. Mutations/replay/status require both document permissions plus
those source reads, current company read/scope and current PO ownership scope.
Service principals cannot submit these human commands. Published supplier/company
inactivity does not prevent reviewing an existing historical commitment; current
access is still required. Legacy unassigned POs cannot be silently attributed.

Purchasing includes a native claim form, held findings, correction, exact match,
void and recent history. Company changes clear selected documents. Registered
action searches retain company scope, ignore stale responses and clear refused
reference data. The same registered contracts are available to Ava; observed
source IDs/revisions and host review remain mandatory. A model cannot declare a
supplier paid or an accounting posting completed through these operations.

## Source and verification

- [Domain grammar](../../crates/domain/src/erp/purchasing_documents.rs),
  [Purchasing command boundary](../../crates/domain/src/erp/purchasing.rs).
- [Transactional adapter](../../crates/adapters/src/erp/supplier_documents.rs),
  [database guards](../../migrations/purchasing-pg/006_unposted_supplier_documents.sql),
  [tax guard](../../migrations/purchasing-pg/008_supplier_document_tax.sql).
- [Native workflow](../../crates/slice/src/operations/supplier_documents.rs),
  [registered contracts](../../config/operator-actions.v1.json).
- [PostgreSQL/router witnesses](../../crates/host/tests/workspace_flows/supplier_documents.rs),
  [Ava execution witnesses](../../crates/host/tests/agent_runtime.rs),
  [native Chromium/SQL journey](../../tools/e2e/supplier-documents.mjs).

Tests and artifact acceptance are recorded separately in the
[execution tracker](../product/PRD_EXECUTION_WORK.md). A command, form or migration
existing alone does not establish acceptance of full AP, procure-to-pay, public
production, live supplier integration or market parity.
