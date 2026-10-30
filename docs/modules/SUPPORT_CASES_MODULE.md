# Support cases

Tier 1. Native operator workspace implemented; acceptance is tracked against current execution evidence.

The working journey is **Open → in progress → waiting → resolved → closed / reopened, with notes and history**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain | [crates/domain/src/erp/support.rs](../../crates/domain/src/erp/support.rs) |
| PostgreSQL adapter | [crates/adapters/src/erp/support.rs](../../crates/adapters/src/erp/support.rs) |
| HTTP boundary | [crates/host/src/erp/support.rs](../../crates/host/src/erp/support.rs) |
| Integration tests | [crates/host/tests/erp_support.rs](../../crates/host/tests/erp_support.rs) |

The native UI is `/app/erp?module=support`. It reads current database projections, submits authorized commands, and shows a saved result only after a matching receipt. The server resolves tenant membership and permissions; browser controls do not grant authority. Unknown transport outcomes retain their request reference for status recovery.

The worklist and detail show current status and bounded histories. Operator controls create cases, start/wait/resume work, resolve/close/reopen them and add notes, using the current revision. Additional assignment and target controls are API capabilities; their existence does not mean an operator screen has been implemented.

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/SUPPORT_CASES_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
