# Fulfillment

Tier 2. API/domain module retained. No native operator screen is claimed for the first release.

The working journey is **Job → production / packing → dispatch → handover**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain | [crates/domain/src/erp/fulfillment.rs](../../crates/domain/src/erp/fulfillment.rs) |
| PostgreSQL adapter | [crates/adapters/src/erp/fulfillment.rs](../../crates/adapters/src/erp/fulfillment.rs) |
| HTTP boundary | [crates/host/src/erp/fulfillment.rs](../../crates/host/src/erp/fulfillment.rs) |
| Integration tests | [crates/host/tests/erp_fulfillment.rs](../../crates/host/tests/erp_fulfillment.rs) |

Changes must preserve the module's contract, authorized transaction and receipt path. API integration tests establish their tested boundary; they do not establish a complete operator screen or pilot acceptance.

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/FULFILLMENT_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
