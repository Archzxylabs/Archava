# Company-owned trade

This contract extends the [managed-company registry](MANAGED_COMPANIES.md) to
explicit product ownership, physical stock, purchasing, sales, source invoices and
human access. The registry records a reviewed declaration; it does not verify
legal status or select an official accounting engine. Local acceptance is tracked
in [PRD execution](../product/PRD_EXECUTION_WORK.md).

## Identity and stock

A new product belongs either to one managed company or to the unassigned legacy
workspace. That choice is immutable. Product IDs partition the existing stock
balances and FIFO cost layers; every sale and PO line must match its header's
company. Legacy foodservice and other product consumers stay in the unassigned
scope. SKU codes remain unique across the workspace, including different
companies. A company-owned product, sales order or purchase order, and a
supplier offer on a company's product, takes the currency of its company's
accounting book at insert, and IDR when the company has no book
([trade currency](../architecture/MULTI_CURRENCY.md), stages 1-2); a supplier
document carries its purchase order's currency. Once a company owns a product,
sales order, purchase order or invoice it can open only an IDR book: a book in
another currency is refused, so a non-IDR book is created before the first record. An invoice made
from an order takes the order's currency; the payment gateway accepts IDR
invoices only. The screens show each amount and read each price, cost, fee and
claim in the record's own currency
([money units](MONEY_UNITS.md)); a new product uses the trade currency the
Commerce workspace read returns for the selected company. No jurisdiction is
inferred from a currency.

Products, sales orders, purchase orders and invoices capture company ID, reviewed
revision, code and official name. Database foreign keys bind that revision to
immutable company history. New product/order creation and explicit counted stock
adjustments require an active company at the reviewed revision. Database guards
also reject changed identity, mismatched lines and movements attributed to a
different company's product. Count adjustments require explicit company input;
the adapter never silently infers it from an arbitrary product ID.

Existing commitments preserve their original company version. Confirmation,
receipt, reservation, fulfillment, cancellation, return and order-to-invoice
conversion can complete after company deactivation, subject to the existing
business rules and current permissions. New business and active product/offer
changes cannot use an inactive company. Replaying a committed command returns its
original receipt after checking current visibility; inactivity or a later company
revision does not rewrite a historical effect.

New commands:

| Module | Command | Additional reviewed input |
| --- | --- | --- |
| Commerce | `product.create_for_company` | `companyId`, `expectedCompanyRevision` |
| Commerce | `stock.adjust_for_company` | `companyId`, `expectedCompanyRevision` |
| Commerce | `order.create_for_company` | Company ID/revision and published customer party ID/revision; optionally `taxCode` with `priceIncludesTax` |
| Purchasing | `order.create-for-company` | Company ID/revision, supplier and offer lines |

A company sales order may name one output tax code of its company's book and say
whether its prices include the tax ([tax on documents](../architecture/TAX.md),
stage B). The two members are stated together; the code must be confirmed and apply
on the day the order is created, otherwise the order is refused with a specific
reason. The order stores the code and the basis only
([Commerce 012](../../migrations/commerce-pg/012_sales_order_tax.sql)): prices and the
order total are unchanged and no rate is stored. The invoice made from the order takes
the version that applies on the invoice date ([Billing](BILLING_MODULE.md#tax-on-an-invoice-from-an-order)).
An order without a company cannot carry a tax code. The Commerce workspace read
returns `taxCode` and `priceIncludesTax` per order and, for the selected company,
`taxCodes`: the confirmed output versions that apply today. That list needs the
`orders.read` grant and access to the company, not an Accounting grant. Tax codes are
entered by the company; Archava verifies no rate. The checkout document is not signed
for an order that names a tax code.

Supplier and party masters remain workspace records. Offers derive company from
their immutable product; new offers/POs retain the current supplier-party checks.
An invoice converted from a fulfilled sale copies its historical company/customer
snapshot and exact source line prices. Manual invoice creation remains an
unassigned legacy path. A physical return does not silently issue a financial
credit or change a settled invoice.

## Current authority and scoped reads

A human must currently hold `companies.read` and cover the company's owner under
Organizations scope in addition to the relevant trade module grants and document
scope. The rule applies to company workspace reads, mutations, replay, status,
payment-link requests/history and invoice email. Published company reads do not
require raw organization access. Company status is historical context for existing
documents, not a substitute for current authorization.

The Commerce, Purchasing and Billing workspace reads accept optional `companyId`.
An omitted value returns only unassigned legacy documents/products/offers. An
explicit value checks company access and returns only that company. The API never
interprets an omitted company as permission to aggregate all companies. Supplier
masters remain shared under their existing read grants. Legacy import SKU lookup
also excludes company products. No historical document is automatically assigned.

Provider callbacks retain their existing service grants, signature verification,
amount/currency checks and idempotency. They derive the company from the immutable
invoice rather than needing an interactive human company grant. Human follow-up
reads remain protected. Invoice email captures the historical company issuer in
its immutable text and escaped HTML payload; it retains the configured sender.

## Native operations and Ava

Catalog, Sales, Purchasing and Invoices expose a paginated company picker with
explicit version refresh and inactive history. Changing company clears the current
records, product/offer choices, cart and selected document. Late responses cannot
replace another company's records. A failed selected-company refresh keeps its
reviewed scope instead of silently falling back to legacy data. Existing document
details show captured company identity.

Ava accepts an optional company for a new trade goal. Follow-ups inherit the
original reviewed company; conflicting company input refuses. Authorized trade
context and later source guard reads use that scope. The Organizations membership
participates in the approval/execution access fingerprint. Model and explicit
company plans must use observed references, company-specific creation commands
and the selected company. IDs from verified prior steps can bind later actions.
Approval and domain handlers remain authoritative. The bounded local planner's
legacy replenishment suggestions do not get relabeled as company suggestions.

## Persistence and verification

[Commerce 009](../../migrations/commerce-pg/009_company_owned_trade.sql),
[Purchasing 005](../../migrations/purchasing-pg/005_company_owned_trade.sql) and
[Billing 008](../../migrations/billing-pg/008_company_owned_trade.sql) append to the
registered ledgers after Organizations 004. Old profile fixtures cap dependent
ledgers before the new foreign keys. Repeated upgrade preserves original legacy
rows and receipts and creates no inferred company assignments.

Sources: [company authority adapter](../../crates/adapters/src/erp/company_trade.rs),
[Commerce kernel](../../crates/adapters/src/erp/commerce_kernel.rs),
[company selector](../../crates/slice/src/operations/company_context.rs),
[router/database tests](../../crates/host/tests/workspace_flows/company_trade.rs),
[Ava tests](../../crates/host/tests/agent_runtime.rs),
[native browser and independent SQL journey](../../tools/e2e/company-trade.mjs).

Company/account/book mapping, legal verification, intercompany transfers,
consolidation, complete AP/AR/GL, bank reconciliation, close, jurisdictional tax,
production/pilot and market parity require separate contracts and acceptance.
