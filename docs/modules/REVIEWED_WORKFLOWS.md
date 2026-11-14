# Reviewed imports, proactive cards and missions

Server-bound plans live in the tenant- and actor-scoped
[reviewed workflows ledger](../../migrations/owner-pg/002_reviewed_workflows.sql).
The owner ledger is registered in the manifest, embedded server profile and
PostgreSQL fixture. Plans and hashes are immutable; only the first terminal
receipt can be appended. PostgreSQL FORCE RLS and runtime grants apply. Plans
expire after 30 minutes; status and committed-effect recovery remain available.

## Imports

`POST /api/migration/reconcile` reviews CSV/TSV data for CRM contacts, catalogue
products or opening inventory. The native Import screen parses quoted cells and
shows normalized commands and omitted columns before confirmation. Batches are
bounded to 500 rows, 50 columns and 2,000 characters per cell. Amounts are whole
IDR integers, including formatted `Rp 150.000` values.

`POST /api/migration/commit` accepts tenant, batch ID, the reviewed plan hash and
explicit confirmation. It uses the stored plan, checks the current actor and
grants, and commits every row and the receipt in one transaction. A later failure
rolls back the entire batch. Retrying returns the same receipt. Opening inventory
requires explicit acquisition cost and a zero current balance/revision; it cannot
silently add to existing stock.

`GET /api/workflows/status?tenant=...&kind=import|feed&id=...` recovers the original
review and receipt. `POST /api/workflows/cancel` cancels only after verifying that
no effect committed, or recovers the existing effect instead. The native UI retains
a principal/tenant-scoped recovery marker and blocks replacement of uncertain work.

## Proactive cards

`GET /api/v1/feed/action-cards` derives bounded low-stock cards from real active
stock and supplier offers. It requires current Commerce and Purchasing access,
respects purchase ownership scope, and suppresses an existing visible open order.
It proposes replenishment to ten units for balances at or below five, choosing an
active supplier offer at its current cost. At most 20 cards are returned.

The supported action is `generate_reorder_po`. After review and confirmation,
`POST /api/v1/feed/action-cards/{id}/execute` creates a draft purchase order through
the existing domain command. Purchase confirmation remains an explicit operation.
Stock revision and visible open orders are rechecked. A product advisory lock
serializes these feed actions; domain keys remain stable if a business commit
succeeds before the workflow receipt is recorded. Recovery checks the actual
command ledger before retrying. Other former sample actions are explicitly
unavailable and produce no synthetic success.

## Missions

The same reviewed native form can create a mission using the finite tool name
`<module>.<canonical action>`. Business steps require approval before execution;
stronger independent-supervisor policy remains in force when selected. The server
uses a stable key derived from mission and step identity, invokes the existing
authenticated handler and checkpoints only a verified receipt. A failed checkpoint
can be retried without duplicating the domain effect. Caller-supplied completion
cannot claim success for a registered business step.

The implementation does not make a general autonomous business planner or mount
future MCP/RPA integrations. Missions execute explicitly configured, approved
steps. [Product workflow tests](../../crates/host/tests/product_workflows.rs)
exercise actor/hash binding, atomic rollback, current revocation and injected
failures after a business commit.
