# Commerce

Tier 1. Native operator workspace implemented; acceptance is tracked against current execution evidence.

The working journey is **Catalog → stock adjustment → draft → reservation → fulfillment → return**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain | [crates/domain/src/erp/commerce.rs](../../crates/domain/src/erp/commerce.rs) |
| PostgreSQL adapter | [crates/adapters/src/erp/commerce.rs](../../crates/adapters/src/erp/commerce.rs) |
| HTTP boundary | [crates/host/src/erp/commerce.rs](../../crates/host/src/erp/commerce.rs) |
| Integration tests | [crates/host/tests/erp_commerce.rs](../../crates/host/tests/erp_commerce.rs) |

The native UI is `/app/erp?module=catalog` and `/app/erp?module=sales`. It reads current database projections, submits authorized commands, and shows a saved result only after a matching receipt. The server resolves tenant membership and permissions; browser controls do not grant authority. Unknown transport outcomes retain their request reference for status recovery.

The native command boundary supports product creation, price/availability updates, counted stock adjustments, multi-line drafts, timed reservation, fulfillment, cancellation and partial/full returns with restock or discard. Product identity and stock type are immutable. Read projections expose on-hand, held and available physical stock; expired holds do not count as reserved. Service products require no physical allocation. Stock is consumed at fulfillment, not at draft creation.

Acquisition costs live in durable FIFO layers within each stock movement transaction. Positive counted additions can carry an optional unit acquisition cost with `valuation.write`; blank means unknown. Historic opening stock is migrated as unknown, never zero. Fulfillment consumes original lot intervals; restocked returns retain the original consumed basis and discarded returns do not increase inventory. Integer allocation preserves rounding through partial returns and later consumption.

`valuation.read` authorizes cost projections; aggregate inventory value additionally requires `stock.read` and all-record scope. API money aggregates use decimal strings, so browser formatting preserves amounts beyond JavaScript's exact integer range. Unknown cost is shown separately from known zero. Fulfillment cost is inventory basis, not revenue, profit or confirmed payment. Services have no physical inventory cost. See [valuation projections](../../crates/adapters/src/erp/inventory_valuation.rs) and [costing/rollback/concurrency integration tests](../../crates/host/tests/workspace_flows.rs).

[Migration 006](../../migrations/commerce-pg/006_private_cost_writers.sql) limits valuation writes to ledger-owned triggers. Runtime can read cost tables but cannot write them or attach the cost writer functions to its own temporary triggers. Existing broad grants are revoked during upgrade; legitimate physical movement and return triggers retain forced tenant RLS and a fixed `pg_catalog` search path. This closes a reproduced database-role privilege path; it is not evidence of an unauthenticated HTTP exploit.

[Migration 010](../../migrations/commerce-pg/010_return_layer_provenance.sql)
adds the exact `return_cost_id` to each new returned stock layer. Its composite
tenant FK and unique binding identify the actual immutable return portion and
original use, including equal-cost portions from different original accounts.
The link cannot be rewritten. Older ambiguous returned layers keep NULL and
remain operationally consumable; their financial origin is not guessed from
price, quantity or timing. Unknown original cost remains unknown.

The current mounted-profile startup checks the exact provenance trigger,
validated FK, unique index and private cost-writer privileges. A disabled trigger
or exposed writer refuses startup. Earlier declared schema profiles retain their
explicit prefix behavior. Six focused Commerce HTTP/PostgreSQL tests pass,
including actual prior-009 return history upgrade/reapply, new return links,
later legacy consumption and startup refusal. Full current-source/native build
acceptance remains pending. This does not create or post COGS journals.

Sales can explicitly link an existing CRM customer with `order.create_from_contact`.
Its ID and reviewed revision persist through invoice conversion while historical
names stay frozen. Current CRM read and order grants are checked on creation/replay.
The picker supports search and paging; legacy named orders stay unlinked. See the
[customer identity contract](CUSTOMER_IDENTITY.md) for dependency and scope details.

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/COMMERCE_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
