# Restaurant tables, recipes and kitchen tickets

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/foodservice.rs), [persistence](../../crates/adapters/src/erp/foodservice.rs), [HTTP](../../crates/host/src/erp/foodservice.rs), [integration tests](../../crates/host/tests/erp_foodservice.rs).

Profile 1.8.0 introduces `@archava/foodservice` and `foodservice-pg` 0.1.0.
Commerce 0.2.0 adds private managed menu products through additive migration 002;
its original migration 001 and public generic product contracts remain unchanged.
This is a bounded whole-ticket domain for independent restaurant and catering
consumers. No full restaurant kit, payment or POS hardware is accepted by this scope.

## Ownership and composition

The pure package owns strict contracts, service, API, private read tools and a shared
owner dashboard. It has no PostgreSQL, CRM, identity, AI or provider dependency.
The adapter owns `archava_foodservice`: nine forced-RLS business tables plus its
checksummed ledger. Tables, menus, sealed recipe versions/components, tickets,
frozen ticket lines, independent memberships, commands and audit belong here.

Commerce owns products, backend IDR prices, immutable order quotes, allocations,
physical stock movements/balances and its own receipts/audit. Managed food portions
are non-stock-tracked `unit` products, hidden/refused by generic catalog/order APIs.
Ingredient products remain ordinary tracked unit/gram/millilitre catalog items.
The private commerce kernel runs inside the food adapter's existing tenant
transaction after current authorization and command claims. It is a trusted-server
SPI, never a browser/LLM permission bypass or arbitrary SQL interface.

The local owner host requires `contacts`, `commerce` and `foodservice`; contacts
supplies host workspace membership rather than a dependency of the domain itself.
Assembly v6 adds the optional food-service flag; v1–v5 retain their old fields and
four-module defaults. The kit planner supports K02 `foodservice_tickets` only after
verifying the installed profile and enabled dependencies. Full K02 and the other
39 operating kits retain explicit gaps.

## Deterministic workflow

Tables have immutable code/title/seat limits, active state and revisions. Each
physical table has at most one open ticket. Dine-in requires an active table and
1–40 guests within its seat limit; takeaway carries no table. Assigned operators
can inspect occupied/free table state without seeing other operators' guest data.
A table with an open ticket cannot be disabled. Stock hold expiry does not free it.

Menu creation links an owned portion product to a kitchen/bar station and recipe.
Each recipe has 1–20 unique tracked ingredients, with integer base quantities
1–1,000,000 per portion. Versions are sealed before commit and never edited,
reopened or extended later. Replace-recipe creates the next version with a reason.
Prices and active-state changes require explicit current revisions and reasons.

Ticket creation freezes backend menu prices, station and recipe versions in
1–20 unique menu lines, 1–100 portions each. It never consumes/reserves ingredients.
Future menu price/recipe changes do not affect these frozen lines. Per-ticket
ingredient aggregation is limited to 100 products and stock bounds of 1 billion.
Prices use integer IDR; the resulting estimate supplies no financial settlement.

```text
draft → queued → preparing → ready → served → closed
   ↘       ↘          ↘        ↘       ↘
                      cancelled
```

Queue reserves the frozen recipe's aggregated ingredient quantities in one
transaction using sorted shared product locks. Stock remains physically on hand.
The hold lasts 1–1,800 seconds (default 900), evaluated with the PostgreSQL clock
after required locks. Availability reads are observations; a committed reservation
is the authoritative claim. An expired queue cannot silently resume: cancel the
old ticket and open a new one.

Starting preparation consumes ingredients once, changes the shared order to
fulfilled and commits the food ticket, both relevant audits and receipt in one
transaction. Kitchen ready, service served and service close are separate explicit
transitions. Close frees the table and records no invoice, payment or signature.

Before preparation, cancellation releases stock reservations. After preparation,
it records a waste reason and retains consumed ingredients; cooked food is never
restored as raw ingredients. All cancellations free the table. Closed/cancelled
tickets cannot be resurrected. Recipe shortage, conflicting revisions, expired
holds and failure after ingredient consumption roll back all partial domain and
stock writes, including command claims.

## Identity, permissions and recovery

Food permissions: `tables.read/write`, `menu.read/write`, `tickets.read/write`,
`kitchen.write`, `audit.read`. Current food and commerce memberships are checked in
the same transaction, including human/service kind, active state and delegated
scope intersection. Baseline commerce access requires `catalog.read` and
`orders.read`; recipes need `stock.read`, ticket effects need `orders.write`, menu
administration needs `catalog.write`. Table/menu administration requires all scope.
Assigned food or commerce scope restricts tickets to the initiating human's owner ID.

Writes and receipt recovery are human-only. Preparation/ready requires kitchen
write; create/queue/serve/close/cancel requires ticket write. Reads and tool calls
never authorize mutation. Public visitors do not receive these tools. Identity,
tenant, role, permissions, prices, paid flags and ingredient allocation overrides
cannot be supplied in command input.

Every command uses an actor/tenant/body-bound HMAC fingerprint and UUID. Replay and
status recovery recheck current required permissions. Food commands/audit commit
atomically; preparation also has a shared commerce movement receipt. Lost browser
responses preserve the same UUID in memory and block a fresh change until resolved.
Denied retry after an unknown outcome retains the pending UUID. Logout/disposal
clears private panel state. Tab reload persistence is additional work.

Forced RLS isolates the transaction tenant; immutable history and deferred
recipe/order/allocation checks protect domain integrity. These controls do not
establish security against a compromised database role or host trust root.

## Local activation and API

Stop the host before build/setup or operator changes. Use the existing private
operator workflow; never commit credentials or account input:

```sh
pnpm crm:build
pnpm crm:setup
pnpm crm:doctor
pnpm crm:modules /absolute/private/modules.json
pnpm crm:user commerce /absolute/private/commerce-account.json
pnpm crm:user foodservice /absolute/private/food-account.json
pnpm crm:start
```

Modules JSON retains the four existing required flags and adds `commerce: true`,
`foodservice: true`. Account inputs use existing `username`, configured `tenants`,
`access: read|write|none`, `scope: all|assigned`. Flags and migrations enroll no
account; food and commerce grants must each be configured explicitly.

Base `/api/foodservice/:tenant` offers GET `access`, `tables`, `menu`, `menu/:id`,
`tickets`, `tickets/:id`, `commands/:uuid`; POST `commands`, `tools`. Host cookie
requests require existing same-origin/CSRF identity. Pagination uses `limit` 1–100
and `after` UUID, plus explicit active or ticket-state filter. Unknown/repeated
query fields and bodies over 16 KiB are refused; dependency errors remain opaque.
Private tools are `foodservice.tables/menu/menuDetail/tickets/detail` only.

Shared dashboard supports table/menu creation, an ingredient recipe builder,
price/active/version updates, cart, dine-in/takeaway ticket, kitchen/service
transitions, cancellation/waste and UUID reconciliation. Ingredient selection
supports up to 1,000 catalog items; larger catalogs use the paginated API.

## Verification and remaining scope

`pnpm foodservice:verify` cold-installs identical artifacts into restaurant and
catering consumers and tests real PostgreSQL migration, concurrency, immutable
snapshots, rollback, expiry, current grants, SQL boundaries, restart and restore.
`pnpm foodservice:verify-host` uses an isolated compiled owner host and Chromium
with actual operator enrollment, real API effects, lost responses, viewers, module
toggles, backup and explicit identity recovery. These proofs make zero provider
calls and use fictional records rather than the original owner's business data.

The accepted scope is a whole-ticket workflow. Per-item completion/station queues,
modifiers/courses, additional orders on an open ticket, partial item cancellation,
split bills, payments/refunds/taxes, offline POS, hardware and public self-order are
additional requirements. A station label does not implement independent item
preparation. Existing mature POS integration remains an option for broader needs.
The [programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md) retains all unfinished kits.
