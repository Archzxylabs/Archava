# Billing

Tier 1. Native operator workspace implemented; acceptance is tracked against current execution evidence.

The working journey is **Invoice draft → issue → correction / void → verified gateway settlement/refund**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain | [crates/domain/src/erp/billing.rs](../../crates/domain/src/erp/billing.rs) |
| PostgreSQL adapter | [crates/adapters/src/erp/billing.rs](../../crates/adapters/src/erp/billing.rs) |
| HTTP boundary | [crates/host/src/erp/billing.rs](../../crates/host/src/erp/billing.rs) |
| Integration tests | [crates/host/tests/erp_billing.rs](../../crates/host/tests/erp_billing.rs) |

The native UI is `/app/erp?module=invoices`. It reads current database projections, submits authorized commands, and shows a saved result only after a matching receipt. The server resolves tenant membership and permissions; browser controls do not grant authority. Unknown transport outcomes retain their request reference for status recovery.

Invoices show ledger balances and frozen lines. The operator can create and issue drafts, append corrections and void eligible invoices. Currency follows each invoice: totals, lines and the correction input use the invoice's currency and decimals ([money units](MONEY_UNITS.md)); the manual creation form has no company and stays IDR whole Rupiah. Only a verified provider event can change settlement totals. The inline payment panel creates and recovers durable Midtrans correlations, opens a real gateway checkout URL when available, and checks authenticated gateway status before settlement/refund. Unapplied funds remain visible for reconciliation. See [payment architecture and setup](../architecture/PAYMENTS.md). Live merchant acceptance remains deferred by the owner; no “mark paid” UI asserts payment.

For orders explicitly linked to a CRM customer, invoice conversion also retains
the source contact UUID/revision and historical customer name. The current CRM
profile may change or require separate access; it does not rewrite the invoice.
See [customer identity](CUSTOMER_IDENTITY.md).

## Invoice from a fulfilled sales order

The native invoice screen offers **Invoice a fulfilled order**. The registered
`billing.create_invoice_from_order` action accepts an invoice number, source
`orderId` and `expectedOrderRevision`; customer, currency, quantities and prices
come from the frozen Commerce order. Current `billing.write` and Commerce
`orders.read`, active human membership, source ownership scope and enabled modules
are required. Execution and request replay recheck source visibility.

The picker uses the bounded Commerce read: up to 100 recent matching orders,
with an explicit customer search and refresh. It does not claim to list every
historical order at once. After conversion, the billed selection is cleared.

The source must be a fulfilled native sales order without recorded returns.
Managed foodservice orders and stale revisions are refused. A subsequent catalogue
price change does not change the invoice. The draft and its source line references
commit with the command, audit and receipt. A source order can have one linked
invoice, including when that invoice is later voided; reissue/replacement policy
is future scope. Response loss can be recovered through the existing billing
status endpoint; reading an owned invoice receipt requires current billing read
access, independently of the source-order grant.

[Migration 005](../../migrations/billing-pg/005_order_invoice_provenance.sql)
adds tenant-bound foreign keys, unique source references, immutable provenance
and deferred checks against the complete source line sheet. Existing manual
invoices retain null provenance; their free-text references are not treated as
verified order links. Customer references now support the same 160-character
limit as Commerce customer names. Both creation paths share the
[invoice adapter](../../crates/adapters/src/erp/billing_invoice.rs).

## Tax on an invoice from an order

Stage B of [tax on documents](../architecture/TAX.md). When the source order names a
tax code, `create_invoice_from_order` looks up the confirmed output version of that
code that applies on the invoice date and stores it with the invoice. Billing has no
document date; the invoice date is the database's date of `created_at`. Without an
applicable version the command is refused (`invoice_tax_code_not_applicable`) and no
invoice is created.

| Column | Meaning |
| --- | --- |
| `gross_minor` | The amount the customer owes, as before: order total plus tax with an exclusive basis, the order total with an inclusive one |
| `tax_minor` | The tax part of `gross_minor`; `0` for an untaxed invoice. The net is `gross_minor − tax_minor` |
| `tax_code_id`, `tax_code`, `tax_code_version`, `tax_kind`, `tax_rate_*`, `tax_base_*`, `price_includes_tax` | What the invoice was taxed with; all null for an untaxed invoice; immutable |
| `correction_tax_minor` | The tax part of `correction_minor` |
| `invoice_corrections.tax_minor` | The tax part of one correction |

Lines are the order's priced lines; tax is a document amount, not a line. Outstanding,
settlement, the payment gateway and refunds work on what is owed and are unchanged.
A correction is an amount owed less or more; its tax part is the tax contained in the
corrected total less the tax carried before, so the parts add up and a full reversal
leaves net, tax and gross at zero.

[Migration 009](../../migrations/billing-pg/009_invoice_tax_snapshot.sql) adds the
columns, an insert/update guard on invoices, an insert guard on corrections and
replaces the provenance function of migration 005 in place. The database recomputes
the tax; an invoice whose tax disagrees with its order, its tax code version or the
arithmetic is refused, also for a direct insert by the serving role. A manual invoice
cannot carry tax.

For a company whose book has an invoice posting policy the tax is posted to the
ledger: the invoice bridge captures the tax of the issue, of each correction and of a
void, and the reviewed journal credits it to the tax code's control account
([invoice source posting](INVOICE_POSTING.md#tax-on-a-captured-source)).
[Migration 010](../../migrations/billing-pg/010_taxed_invoice_posting.sql) removes the
refusal that stood in for this (`invoice_tax_posting_unavailable` no longer exists).
Issuing is refused while the tax code's control account is inactive
(`invoice_tax_control_account_changed`); the invoice stays a draft until the account
is reactivated.

The workspace read returns `taxMinor`, `netMinor`, `correctionTaxMinor` and `tax`
(the snapshot, or null) per invoice and a `taxNotice`. The invoice screen shows net,
tax with its code, exact rate, base and basis, and total after corrections; the
invoice-from-order form shows the same for the selected order with the version that
applies today; the invoice mail adds `Net`, `Tax (…)` and `Total` lines. Each of them
says that tax codes are entered by the company and not verified by Archava. An
untaxed invoice is shown and mailed as before. Tests:
[tax_sales.rs](../../crates/host/tests/erp_accounting/tax_sales.rs). Not exercised in
a browser.

## Tax ids and the tax invoice reference

Part of the [Indonesia tax pack](TAX_INDONESIA_PACK.md); every value is text the
company entered, stored as entered and not verified.
[Migration 011](../../migrations/billing-pg/011_invoice_tax_identifiers.sql) adds five
columns to an invoice and one guard.

| Column | Meaning |
| --- | --- |
| `seller_tax_id`, `buyer_tax_id` | The tax id of the invoice's company and of its customer party as they were when the invoice was created; null when there is none and on every untaxed invoice; immutable |
| `tax_reference` | The reference of a tax invoice issued outside Archava; 1 to 100 characters of free text |
| `tax_transaction_code` | The company's own short classification of the transaction; optional, 1 to 10 characters |
| `tax_reference_version` | `0` without a reference, `1` once recorded, one more for every replacement |

`create_invoice_from_order` stores the two tax ids for a taxed invoice. A later change
of the company's or the party's tax id does not change the invoice.

`record_tax_reference` (`invoiceId`, `expectedRevision`, `reference`, optional
`transactionCode`) sets the reference of a taxed invoice that is issued or settled.
`replace_tax_reference` takes the same members and a `reason`. Both need
`billing.settle`, raise the invoice revision by one, change no amount and no status,
and follow the usual receipt, replay and audit contract. The earlier value of a
replaced reference stays in the audit row of the replacing command; the audit table is
immutable. Archava never generates a reference. Refusals:
`invoice_tax_reference_untaxed`, `invoice_tax_reference_not_issued`,
`invoice_tax_reference_already_recorded` and `invoice_tax_reference_missing`.

The workspace read returns `sellerTaxId`, `buyerTaxId` and `taxReference` (or null).
The invoice screen shows them for a taxed invoice with the form that records or
replaces the reference; the invoice mail adds `Seller tax id`, `Buyer tax id` and
`Tax invoice reference` lines when present. An untaxed invoice is read, shown and
mailed as before. Tests:
[tax_documents.rs](../../crates/host/tests/erp_accounting/tax_documents.rs). Not
exercised in a browser.

Later returns do not rewrite the original snapshot or automatically create a
credit/refund. An authorized operator reviews a correction through the existing
billing lifecycle. Master-party identity, credit-note document linkage, GL/AR,
bank reconciliation and full B01 acceptance remain separate requirements in the
[active PRD](../product/PRD.md). Current execution evidence belongs to the
[PRD implementation tracker](../product/PRD_EXECUTION_WORK.md).

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/BILLING_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
