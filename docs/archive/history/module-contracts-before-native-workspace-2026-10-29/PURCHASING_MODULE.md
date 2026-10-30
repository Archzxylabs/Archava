# Purchasing module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/purchasing.rs), [persistence](../../crates/adapters/src/erp/purchasing.rs), [HTTP](../../crates/host/src/erp/purchasing.rs), [integration tests](../../crates/host/tests/erp_purchasing.rs).

This contract covers the local native supplier/order/physical-receipt implementation.
Release 1.10.0 accepted with fresh isolated domain, host, browser, upgrade and owner evidence.
It extends shared commerce without changing the ARCHZXY pricebook.
Full wholesale, restaurant, workshop and production systems retain their other
requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/purchasing` owns strict contracts, human service commands, private read
tools, HTTP API and reusable DOM dashboard. It depends only on pinned Zod and
needs no CRM, AI or provider. `@archava/purchasing-pg` owns nine forced-RLS business
tables: memberships, suppliers, offers, orders, lines, receipts, receipt lines,
commands and audit. Its checked migration ledger is separate.

Commerce 0.3.0 remains the sole owner of product identity, base units, stock
balances and immutable movements. Purchasing reads bounded product projections
and uses the private `receivePurchaseStock` transaction port. It never directly
writes commerce tables or maintains another physical stock balance. Additive
commerce migration 003 preserves migration 001/002 and historical quantities,
quotes, audit and receipts; inbound movements add purchase provenance.

The private port belongs to trusted server composition, not a public model tool.
Purchasing authorization, physical findings and commerce stock claims/audit share
one PostgreSQL transaction. Sorted product locks serialize inbound goods against
sales, reservations, returns, adjustments and restaurant consumption. Deferred
checks bind exact accepted quantities to inbound movements and both audits.

## Supplier offers and purchase quotes

Supplier codes/names and product base units are immutable identities. A supplier
may be enabled/disabled. Each supplier/product pair has one versioned offer with
backend-configured integer IDR cost per `unit`, `gram` or `millilitre`. This cost is
distinct from commerce's customer selling price and the company service pricebook.
LLMs never supply costs or totals to an order.

Draft orders snapshot the selected supplier, product/offer revisions, units,
quantities and costs. A confirmation checks that the offers still match the
reviewed snapshot. Changed or disabled offers refuse confirmation. An explicit
`order.replace-lines` refreshes the draft quote; the operator reviews the result
before confirming. Confirmation does not silently change a displayed price.
Later changes affect future drafts and do not alter confirmed purchase lines.

An order has 1–50 unique stock products. Quantity is 1–1,000,000 base units per
line; unit cost is 0–1,000,000,000 IDR; total estimate is at most 500,000,000,000,000
IDR and must be a safe integer. No conversion, freight, tax, landed cost, supplier
invoice, valuation or verified payment is inferred from this estimate.

## Physical receipt policy

```text
draft → confirmed → partial → received
  ↓         ↓          ↓
cancelled  cancelled  short_closed
            ↓
       short_closed
```

A human records explicit accepted and rejected quantities plus a reference/finding.
Each finding must account for at least one unit. Cumulative accepted **plus rejected**
units cannot exceed the ordered quantity. Only accepted units enter usable stock.
Rejected units remain immutable inspection findings and produce no usable stock.

For this bounded implementation, `received` means every ordered unit has an
accepted/rejected finding. It does not mean all units were accepted, invoiced or
paid. A replacement for rejected goods uses a new purchase order; reject/return/
replacement logistics and amendments are separate extensions. The panel always
shows accepted and rejected totals separately.

Receipts may arrive in several commands. All-rejected findings still retain
source/audit evidence with zero movements. Stock-capacity overflow or an invalid
line aborts the complete receipt, including every other product in that command.

Cancellation applies to drafts/confirmed orders without findings. Short closure
applies to confirmed/partial orders and prevents later receipts while preserving
earlier stock and findings. A partial order cannot be cancelled to erase physical
history. Completed/cancelled/short-closed orders accept no more goods. Inactive
suppliers/products do not erase or prevent authorized recording of goods already
ordered; immutable stock-tracking policy still applies.

## Permission and recovery boundaries

Independent purchasing grants cover suppliers, offers, orders, receipt findings
and audit. Supplier/offer writes require global purchasing and commerce scope.
Orders are global or restricted to their immutable creator. Private order detail
holds a shared row lock through all projections so state and findings stay coherent.

Every transaction also locks current commerce membership. Commerce `catalog.read`
is required to enter the composed service. A physical receipt additionally requires
current global commerce `stock.read`/`stock.write` and purchasing order/receipt
read/write. Purchasing-only membership does not grant stock authority. Current
service/human grants intersect for reads; writes and command recovery are human-only.

The tenant, owner, identity, state, price, total, grants and payment claims are never
accepted from untrusted payload fields. Strict bounded schemas reject them. The
host adds session verification, configured workspace access, same-origin checks
and CSRF. The domain additionally authorizes current resource access.

A command UUID binds tenant, actor, canonical body and private HMAC fingerprint.
Duplicate submissions recover the original result without adding goods. Changed
payload or actor refuses the UUID. A revision race has one winner; every losing
claim, finding, audit and movement rolls back. Read-only recovery rechecks current
operation/domain grants and creator scope. A purchasing stock claim cannot be
replayed through the generic commerce command API.

The browser retains unresolved commands in memory. Lost responses and denied
retries keep the original UUID and block new mutations until recovery. Logout or
changing workspace destroys this state. Durable reload/cross-device recovery and
natural-language purchasing execution are not yet included.

## Local activation

Back up and stop the local host before setup/migration. Preserve all `.local`
settings, identity/fingerprint keys, native/provider control and archives. Migration
enrolls no users and preserves existing flags/grants. Enable `commerce` and
`purchasing` through a private module-selection file, keeping `contacts: true` and
all other selected/default flags explicit. Assembly contract v8 is required for a
new purchasing selection; v1–v7 retain their old contracts.

```bash
pnpm crm:setup
pnpm crm:modules /absolute/path/private-modules.json
pnpm crm:user purchasing /absolute/path/private-purchasing-grants.json
pnpm crm:user commerce /absolute/path/private-commerce-grants.json
pnpm crm:start
```

Each grant file uses an existing active identity/workspace, with `username`,
`tenants`, `access` (`read`, `write`, `none`) and `scope` (`all`, `assigned`). These
commands update only their own memberships. A stock receiver needs explicit global
commerce write access; enabling purchasing never supplies it automatically.

The local owner host composes the reusable dashboard. Supplier/offer/product
selectors use pagination up to 1,000 entries; larger catalogs use the paginated API.
Order pages are 50 items, APIs at most 100; detail shows the latest 100 receipt
findings/history entries with explicit truncation. No infinite list is implied.

## API and tools

`/api/purchasing/:tenant/` supplies authenticated `GET access`, `suppliers`,
`offers`, `orders`, `orders/:id`, and `commands/:uuid`; human `POST commands`; and
private read-only `POST tools`. Lists accept strict cursor/limit filters; suppliers
and offers accept active/supplier filters, orders accept state/supplier filters.
Bodies are bounded to 16 KiB. Errors expose codes without database/private details.

Read tools are `purchasing.suppliers`, `purchasing.offers`, `purchasing.orders` and
`purchasing.detail`. They never create an order, send a supplier message, receive
stock, mark payment or call a provider.

Narrow `purchase_receipts` kit scopes target K02/K04/K18/K23. Dependency resolution
requires verified purchasing, commerce and local host identity packages. That
scope does not accept any entire operating kit or regulated procurement.

## Verification and remaining requirements

Required evidence includes two cold-installed wholesale/restaurant consumers,
unchanged packages and browser-safe bundle, real PostgreSQL duplicates/revision
races, stale quotes, rejected goods, over-receipt, stock/total overflow, atomic
cross-domain rollback, current grants/RLS, non-empty prior release upgrade,
restart/fresh restore and actual compiled host/Chromium operation. Run
`pnpm purchasing:verify` and `pnpm purchasing:verify-host` sequentially because
both prepare shared artifacts. Existing module regressions remain required.

Native inventory valuation, warehouse/lot/expiry, unit conversion, tax/payables,
supplier messages, purchase returns/replacements, amendment/approval policies and
live financial/provider integrations need separate acceptance. Specialist domains
also need an explicitly selected authoritative backend and domain review. The
[original plan](../archive/history/plans/PURCHASING_MODULE_PLAN.md) records research and proposal history.
