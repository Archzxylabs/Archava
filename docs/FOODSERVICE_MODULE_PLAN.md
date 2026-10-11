# Food-service implementation plan

This is the next executable domain slice after commerce acceptance, not an
accepted restaurant kit. The user requested continued execution across milestones.
Restaurant rules compose with the shared catalog/order/stock kernel; they do not
fork retail inventory or treat a restaurant ticket as a generic CRM record.

## Domain ownership

`foodservice` owns dining tables, menu items, versioned recipes/components, tickets,
frozen ticket lines, domain permissions, audit and command receipts. Commerce owns
menu prices/quoted order lines and ingredient stock movements. A ticket references
a managed commerce order. Both commit in one PostgreSQL transaction.

Menu products are non-stock-tracked portions managed by the food-service domain.
An additive commerce migration introduces private product management metadata;
generic catalog/order APIs cannot sell or alter managed menu products. Public
generic product contracts retain their original fields. Ingredient products remain
ordinary tracked catalog items in immutable base units (unit/gram/millilitre).

Recipe versions/components are append-only. Ticket creation freezes backend menu
prices and recipe versions. Changing a recipe or menu price affects future
tickets, with no silent changes to an existing ticket. No caller supplies amounts,
ingredient allocations, paid flags, inventory overrides or arbitrary scripts.

## Initial executable workflow

1. Authorized operator creates tracked ingredients and receives stock through
   commerce's existing ledger.
2. Operator creates menu items with deterministic prices, station and versioned
   ingredient quantities, and creates dining tables with seat limits.
3. Operator opens a dine-in or takeaway ticket. Dine-in requires an active table,
   sufficient seats and no other open ticket. Table occupancy remains physical
   service state until an explicit close/cancel; stock expiry does not free a table.
4. Queue the ticket: compose all recipe quantities, then reserve every ingredient
   atomically using commerce's expiry and sorted product locks.
5. Start preparation: consume ingredients once through the shared kernel. A
   late/expired stock reservation refuses preparation; the operator cancels and
   creates another ticket instead of silently resuscitating the old hold.
6. Mark ready, serve, then close the service. These are kitchen/service transitions,
   not financial settlement. Prices and consumed quantities remain fixed.
7. Before preparation, cancellation releases reservations. After preparation,
   cancellation records a waste reason and retains consumed stock; prepared food
   is never automatically restored as raw ingredients.

The first scope is a whole-ticket lifecycle. Per-item stations, modifiers,
additional courses, item cancellation, split bills, payment, tax, kitchen hardware,
offline POS and online self-order remain additional work. Station metadata/read
filters do not imply independently completed item preparation. This distinction
matches the separate preparation concept documented in
[Odoo's preparation display](https://www.odoo.com/documentation/19.0/applications/sales/point_of_sale/preparation.html),
while the exact lifecycle above is Archava's own proposed policy.

## Authority and verification

The local host composes food-service with contacts/workspace identity and commerce.
Enabling flags enrolls no account. Table/menu administration requires all scope;
assigned operators see/manage only their tickets. Current food-service and required
commerce grants are checked in the same transaction. Service reads intersect the
human/service scopes; writes/recovery remain human-only and tools read-only.

Acceptance needs two independent installed restaurant/catering contexts, real
PostgreSQL recipe/price snapshots, table/stock races, atomic rollback, expiry after
lock waits, preparation/waste semantics, scopes, restart/restore, actual owner host
and Chromium recovery/refusal evidence. Full K02 operations retain their declared
gaps until their own acceptance. The plan itself grants no readiness or authority.
