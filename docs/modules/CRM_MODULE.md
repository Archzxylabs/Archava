# CRM

Tier 1. Native operator workspace implemented; acceptance is tracked against current execution evidence.

The working journey is **Contacts → activity → follow-up → durable receipt**. Current launch status belongs to [the canonical readiness tracker](../product/LAUNCH_READINESS.md), rather than historical acceptance labels.

| Boundary | Current source |
| --- | --- |
| Domain contracts | [crates/domain/src/erp/crm.rs](../../crates/domain/src/erp/crm.rs) |
| Transactional adapter & RLS | [crates/adapters/src/erp/crm.rs](../../crates/adapters/src/erp/crm.rs) (`archava_adapters::erp::crm`) |
| HTTP boundary & dispatch | [crates/host/src/erp/crm.rs](../../crates/host/src/erp/crm.rs) |
| Host router | [crates/host/src/routes/crm.rs](../../crates/host/src/routes/crm.rs) & [crates/host/src/routes/erp.rs](../../crates/host/src/routes/erp.rs) |
| Integration tests | [crates/host/tests/crm.rs](../../crates/host/tests/crm.rs) |

The native UI is `/app/contacts`. It reads current database projections, submits authorized commands, and shows a saved result only after a matching receipt. The server resolves tenant membership and permissions; browser controls do not grant authority. Unknown transport outcomes retain their request reference for status recovery. Each pending request has a separate browser storage key scoped to tenant, account and workflow; acknowledging one request removes only that reference. Legacy markers are still recognized. Signing out removes session facts while retaining unresolved request metadata, which contains no business payload or credential and is read only for the original account.

Contact creation, contact actions and follow-up review support an explicit “review and retry original action” path when the server has no visible receipt. Operators reconstruct or check the original details and reuse the original request key. Opening review sends nothing. A conflicting retry does not clear the pending original reference; server idempotency remains authoritative. ERP forms share the same rule. This does not automatically reconstruct a business payload after a reload.

Run the module's actual tests from the root workspace (see the source table), plus the [repository verification commands](../DEVELOPER_GUIDE.md). For Tier 1 cross-module/browser acceptance, see [workspace flow tests](../../crates/host/tests/workspace_flows.rs) and [browser acceptance](../../tools/e2e/verify.mjs). Test availability is distinct from an executed passing result.

The [previous module contract](../archive/history/module-contracts-before-native-workspace-2026-10-29/CRM_MODULE.md) is frozen for migration provenance. Its old package commands and acceptance statements are historical context.
