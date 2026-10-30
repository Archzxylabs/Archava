# Commerce module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain entry](../../crates/domain/src/lib.rs), [commerce adapter](../../crates/adapters/src/erp/commerce_kernel.rs), [HTTP commands](../../crates/host/src/router.rs).

Profile 1.7.0 adds `@archava/commerce` and `@archava/commerce-pg` 0.1.0. Independent
retail and wholesale consumers install identical code for the bounded
`commerce_orders` scope. Orders, physical inventory and returns remain distinct
from invoices, payment verification and refunds.

## Owned records and units

The domain owns catalog products, stock balances, order headers/lines, temporary
allocations, immutable stock movements, received returns, memberships, command
receipts and audit. Ten business tables have forced tenant RLS; migrations have
their own checksummed ledger. No CRM, AI, payment provider or new third-party
dependency is required by the standalone packages.

Products have a tenant-unique uppercase SKU, immutable name/base unit and tracking
policy, active flag, integer IDR price and catalog revision. Units are `unit`,
`gram` and `millilitre`; quantities are integers in the selected base unit. No
implicit conversion or fractional rounding is performed. This version has one
stock pool per tenant, without locations, lots, serial numbers or valuation.

Stock starts at zero. A human adjustment records a signed quantity and reason.
The balance and its independent revision derive from the immutable movement
ledger. Reducing stock below current unexpired reservations is refused. Turning
off stock tracking later is prohibited; untracked products do not accept stock
adjustments. Catalog updates do not change stock revisions.

Order creation copies the backend's product SKU, title, unit, tracking policy,
price and revision. The total is the integer sum of line price × quantity. A caller
supplies product IDs and quantities, never a price, total, paid flag, owner, expiry
or managed-domain flag. Catalog price changes affect newly created/replaced
quotes; existing draft and reserved quotes retain their snapshots. Draft lines
can be replaced explicitly with a reason; reserved/fulfilled quotes are immutable.

Bounds: at most 50 unique products per order, quantity 1–10,000 per line, unit
price 0–1,000,000,000 IDR, total at most 500,000,000,000,000 IDR, and on-hand
quantity at most 1,000,000,000 base units. Totals stay within exact JavaScript
integer arithmetic and PostgreSQL bigint bounds. These are module limits, not
ARCHZXY's implementation pricebook, which remains unchanged.

## Lifecycle and stock effects

| Command                                   | Rule/effect                                                                                                           |
| ----------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `product.create`                          | Catalog-write and all scope; server identity, zero initial stock                                                      |
| `product.set-price`, `product.set-active` | Current catalog revision and reason; immutable SKU/unit/tracking                                                      |
| `stock.adjust`                            | Stock-write, all scope, current stock revision, nonzero signed quantity and reason                                    |
| `order.create`                            | Human order-write/read and catalog-read; backend quote; no stock effect                                               |
| `order.replace-lines`                     | Draft only, current order revision and reason; new backend snapshot                                                   |
| `order.reserve`                           | Draft, active products, sufficient stock for every tracked line; TTL 1–1,800 seconds, default 900                     |
| `order.fulfill`                           | Reserved and unexpired after product locks; consume each allocation once                                              |
| `order.cancel`                            | Draft/reserved including expired hold; release allocation without changing on-hand quantity                           |
| `order.return`                            | Fulfilled, current revision, received line quantities/dispositions and reason; no quantity beyond delivered remainder |

Reserved stock leaves on-hand quantity intact. A held allocation expires by
database time, without deleting history or requiring a background worker. Read
results expose `effectiveState: expired` and observation time. Retrying the
original reservation returns its original receipt without extending it. Expired
orders cannot fulfill or re-reserve; create another draft if still required.

Fulfillment removes stock through immutable consumption movements. Cancellation
is terminal and releases reserved capacity. Existing reserved orders can fulfill
after catalog deactivation; deactivation prevents new quotes/reservations, not
fulfillment of already reserved quantities. There is no inference of customer
payment, delivery-carrier confirmation, refund or accounting revenue.

A return records what an operator physically received. `restock` adds received
tracked goods back through a movement; `discard` records the return without adding
stock. Partial returns retain fulfilled state; all lines received moves the order
to returned. Original prices/totals stay unchanged. Refund/credit processing is a
separate domain. Restocking additionally requires stock-write permission. A failed
return, overflow or stale revision rolls back its records and movements atomically.

## Transactions, permissions and extensibility

An order row serializes edits. Product locks are taken in UUID order to avoid
inconsistent lock order across multi-product commands; see PostgreSQL's
[row locking and deadlocks](https://www.postgresql.org/docs/current/explicit-locking.html#LOCKING-DEADLOCKS).
Expiry uses the database clock after acquiring product locks. Reservation failure
on a later product rolls back all earlier allocations, the order, audit and
receipt. SQL guards protect stock derivation, confirmed quotes, received quantities
and terminal transitions; deferred integrity checks validate a complete order at
commit. Stock reservation is a separate operation, a distinction also documented
in [Odoo's reservation flow](https://www.odoo.com/documentation/19.0/applications/inventory_and_mrp/inventory/shipping_receiving/reservation_methods/at_confirmation.html).
This module's explicit draft/reserve/fulfill policy is its own contract.

Command UUIDs bind actor, tenant and canonical input with a stable HMAC key.
Duplicates recheck current write permission and record scope, then return the
original receipt. Receipt recovery is human and actor bound. Audit, committed
receipts, movements and received-return records are immutable. Runtime roles cannot
enroll users or delete orders/products. The server owns database credentials and
tenant context; RLS is not a sandbox for a compromised private database role.

Permissions: catalog read/write, stock read/write, orders read/write, returns write
and audit read. Assigned operators may manage their own orders. Catalog and direct
stock administration require all scope. Service reads intersect current service
and human permissions/scope. Machine writes and machine receipt recovery are
refused; the tool registry contains reads only.

The private `commerce-pg/kernel` exports transaction primitives for future domain
composition. They require a caller-owned tenant transaction, prior authorization
and a claimed command. Managed food-service orders cannot be mutated through the
generic commerce service; a future kitchen domain must own its transitions. No
restaurant operation is accepted merely from this kernel.

## APIs, assembly and local operation

Routes under `/api/commerce/:tenant`:

| Route                               | Result                                                                        |
| ----------------------------------- | ----------------------------------------------------------------------------- |
| `GET /access`                       | Current domain permissions/scope                                              |
| `GET /products`, `/products/:id`    | Bounded catalog or product; optional active filter                            |
| `GET /stock/:id`                    | On hand/reserved/available, stock revision and authoritative observation time |
| `GET /stock/:id/movements?limit=20` | Latest movement history; additional audit grant; explicit truncation          |
| `GET /orders`, `/orders/:id`        | Bounded order page or frozen lines, latest returns/audit                      |
| `POST /commands`                    | Strict `{requestId,command}`; atomic mutation receipt                         |
| `GET /commands/:requestId`          | Actor-bound receipt or current not-committed observation                      |
| `POST /tools`                       | Bound reads only: products, product, stock, orders, detail                    |

Lists default to 20, maximum 100, with UUID cursors; movement history is bounded
recent history, without a cursor. Detail returns at most 50 returns/audit rows,
with explicit truncation; both histories are null without audit grant. Unknown or
duplicate fields, authority overrides, invalid units and bodies over 16 KiB are
refused. Errors expose no SQL or credentials. Availability is a snapshot, not a
stock promise: successful reservation alone allocates goods.

Assembly v5 adds optional `commerce`; v1–v4 retain their original fields and
four-module defaults. The local owner host retains contacts as its workspace
prerequisite, with independent commerce grants and session/CSRF/origin checks.

```json
{
  "schemaVersion": "1.0.0",
  "catalogueVersion": "1.0.0",
  "profile": "1.7.0",
  "selections": [{ "id": "K03", "scope": "commerce_orders" }]
}
```

K04 supports this same narrow order/stock scope. Full retail, wholesale and
restaurant kits retain their additional capability requirements. Use
`pnpm kits:plan REQUEST.json` then `pnpm library:assemble REQUEST.json OUTPUT --kit`.

With the existing host stopped, build/setup first. Pass a privately owned mode-0600
JSON containing desired existing flags plus `commerce: true` to `pnpm crm:modules`.
Separately pass `{"username":"ACTUAL_ACCOUNT","tenants":["studio-demo"],"access":"write","scope":"all"}`
in a private file to `pnpm crm:user commerce FILE.json`. Read grants and revocation
use `read`/`none`. Setup/enabling never enrolls existing accounts automatically.
Start via `pnpm crm:start`. Disabling hides the module while retaining all records
and receipts. The panel supports products, stock adjustment, cart/draft, reserve,
fulfill, cancel, price/active changes and received returns.

Browser uncertain writes retain their UUID/body in memory for status checking or
retry, including after a denied retry. Reload/logout/tenant change discards this
memory; durable tab-reload recovery is not implemented. Text uses `textContent`,
disposed requests abort and logout clears customer data. Backups include the owned
schema; restore uses a fresh database and explicit identity recovery.

## Evidence and remaining work

`pnpm commerce:verify` cold-installs retail/wholesale consumers and verifies actual
PostgreSQL duplicate/last-unit races, multi-product rollback, current scopes,
expiry during a proven lock wait, fixed quotes, partial restock/discard, SQL guards,
restart and fresh restoration. `pnpm commerce:verify-host` exercises the compiled
owner host and Chromium, including response loss, viewer refusal, lifecycle and
backup restoration. Synthetic data only; zero provider calls.

Remaining: invoices/payment/refunds, tax/discount rules, customer checkout, shipping
integration, partial fulfillment/backorders, exchanges, variant grouping, customer
credit terms, multi-location/lot/serial stock, valuation, offline POS and restaurant
table/recipe/kitchen rules. These remain explicit kit/domain work. This slice is
part of uninterrupted execution in the [programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).
