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
companies. This surface retains its existing IDR trade rules; a company currency
and jurisdiction are not inferred from them.

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
| Commerce | `order.create_for_company` | Company ID/revision and published customer party ID/revision |
| Purchasing | `order.create-for-company` | Company ID/revision, supplier and offer lines |

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
