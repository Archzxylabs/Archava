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

Invoices show ledger balances and frozen lines. The operator can create and issue drafts, append corrections and void eligible invoices. Currency follows each invoice; the current creation UI uses IDR whole Rupiah. Only a verified provider event can change settlement totals. The inline payment panel creates and recovers durable Midtrans correlations, opens a real gateway checkout URL when available, and checks authenticated gateway status before settlement/refund. Unapplied funds remain visible for reconciliation. See [payment architecture and setup](../architecture/PAYMENTS.md). Live merchant acceptance remains deferred by the owner; no “mark paid” UI asserts payment.

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

Later returns do not rewrite the original snapshot or automatically create a
credit/refund. An authorized operator reviews a correction through the existing
billing lifecycle. Master-party identity, credit-note document linkage, GL/AR,
bank reconciliation and full B01 acceptance remain separate requirements in the
[active PRD](../product/PRD.md). Current execution evidence belongs to the
[PRD implementation tracker](../product/PRD_EXECUTION_WORK.md).

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/BILLING_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
