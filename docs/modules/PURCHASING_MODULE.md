# Purchasing

Tier 1. Native operator workspace implemented; acceptance is tracked against current execution evidence.

The working journey is **Supplier → offer → purchase draft → confirm → inspected goods receipt**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain | [crates/domain/src/erp/purchasing.rs](../../crates/domain/src/erp/purchasing.rs) |
| PostgreSQL adapter | [crates/adapters/src/erp/purchasing.rs](../../crates/adapters/src/erp/purchasing.rs) |
| HTTP boundary | [crates/host/src/erp/purchasing.rs](../../crates/host/src/erp/purchasing.rs) |
| Integration tests | [crates/host/tests/erp_purchasing.rs](../../crates/host/tests/erp_purchasing.rs) |

The native UI is `/app/erp?module=purchasing`. It reads current database projections, submits authorized commands, and shows a saved result only after a matching receipt. The server resolves tenant membership and permissions; browser controls do not grant authority. Unknown transport outcomes retain their request reference for status recovery.

Goods receipts distinguish accepted and rejected units and add only accepted units to Commerce stock, in the same transaction. Purchase lines retain their offer price snapshot; receipt valuation uses that frozen price even if the current offer changes. Operators can enter a whole-Rupiah landed fee and choose allocation by accepted value or accepted quantity. Integer largest-remainder allocation conserves every Rupiah, with line IDs breaking ties. A positive fee with no allocation weight is refused; zero-priced goods require quantity allocation. Rejected units receive no inventory cost.

[Commerce migration 005](../../migrations/commerce-pg/005_inventory_cost_layers.sql) stores FIFO acquisition layers, consumption and original return costs. [Purchasing migration 002](../../migrations/purchasing-pg/002_landed_cost_receipts.sql) independently checks the receipt allocation at commit. A failed cost write rolls back the physical stock, receipt, audit and request claim together. Exact request replay adds no stock or cost. The [exact integer domain helper](../../crates/domain/src/erp/inventory_valuation.rs) is the native implementation; the older `inventory_costing` helper is retained for compatibility, outside this path.

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/PURCHASING_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
