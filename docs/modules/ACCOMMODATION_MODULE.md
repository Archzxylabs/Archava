# Accommodation

Tier 2. API/domain module retained. No native operator screen is claimed for the first release.

The working journey is **Room → night inventory → stay → checkout**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain | [crates/domain/src/erp/accommodation.rs](../../crates/domain/src/erp/accommodation.rs) |
| PostgreSQL adapter | [crates/adapters/src/erp/accommodation.rs](../../crates/adapters/src/erp/accommodation.rs) |
| HTTP boundary | [crates/host/src/erp/accommodation.rs](../../crates/host/src/erp/accommodation.rs) |
| Integration tests | [crates/host/tests/erp_accommodation.rs](../../crates/host/tests/erp_accommodation.rs) |

Changes must preserve the module's contract, authorized transaction and receipt path. API integration tests establish their tested boundary; they do not establish a complete operator screen or pilot acceptance.

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/ACCOMMODATION_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
