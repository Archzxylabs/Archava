# Fulfillment, dispatch and light production module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/fulfillment.rs), [persistence](../../crates/adapters/src/erp/fulfillment.rs), [HTTP](../../crates/host/src/erp/fulfillment.rs), [integration tests](../../crates/host/tests/erp_fulfillment.rs).

This contract covers warehouse custody chains, pick/pack/dispatch evidence, delivery findings with reconciliation, and simple made-to-order work orders with material, output and scrap traceability.
Candidate release 1.15.0 adds bounded fulfillment and light production authority without altering existing CRM, commerce, attendance or scheduling authority.
Full logistics and made-to-order operating kits retain their other requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/fulfillment` owns strict contracts, human service commands, private read tools, HTTP API and a reusable DOM dashboard with a hardened state machine receipt recovery (S1). It depends only on pinned Zod and needs no external CRM or AI provider.
`@archava/fulfillment-pg` owns seven forced-RLS business tables: `memberships`, `jobs`, `job_lines`, `findings`, `custody_events`, `commands`, and `audit`. Its checked migration ledger `001_fulfillment_jobs_evidence.sql` is checksummed (`901ab569b84baa26008c2128144dff1d0bc0214d6cfd32f7567faeaae9fc9cb6`) and constitutes ledger ke-19.

The package deliberately composes **nothing** from commerce: commerce remains the only authority for stock and pricing, while fulfillment owns custody, dispatch and production evidence only.

Two independent business consumers demonstrate generalization:

1. **Consumer 1: Warehouse, fulfillment and logistics (`K20: logistics-fulfillment`)**:
   Receives goods, stores them, picks and packs them, dispatches them to a carrier and records proof of delivery, with a monotonic custody chain and delivery findings that must be reconciled before a job may be completed.
2. **Consumer 2: Simple production and made-to-order (`K23: light-production`)**:
   Plans materials against a work order, consumes input, produces output, records scrap as a separate axis, and keeps every quantity traceable through append-only job lines.

## Lifecycle and Invariants

```text
Job lifecycle:
draft ──[change_job_status]──> released ──> in_progress ──> completed
 │                                │             │
 └──[canceled]<───────────────────┴─────────────┘

Custody chain (K20):
received ──> stored ──> picked ──> packed ──> dispatched ──> delivered
                                          └──[returned]  (rollback, needs dispatched)

Reconciliation gate (K20):
finding(partial|rejected|lost|damaged) ──> unreconciled_count +1
finding(accepted, reconciles_id)       ──> unreconciled_count -1
completion refused while unreconciled_count > 0
```

- **Human-only mutations**: `create_job`, `record_line`, `record_finding`, `record_custody` and `change_job_status` are refused for service/automation principals at the service boundary.
- **Append-only evidence**: `job_lines`, `findings` and `custody_events` are immutable at the database boundary (triggers refuse update, delete and truncation), jobs are undeletable, and `audit` and `commands` carry the same guarantees. Material, output, scrap, findings and custody are proven by what was appended, never by a mutable counter alone.
- **Counter axes stay separate**: `consume` with `disposition: 'normal'` advances `consumed_count`, `produce` with `normal` advances `produced_count`, and a `scrap` disposition on either direction advances `scrap_count`. Advanced MRP remains deferred to `K32`.
- **Custody ordering**: the first custody event must be `received`; stages are monotonic by `received(0) < stored(1) < picked(2) < packed(3) < dispatched(4) < delivered(5)`; `returned` is a rollback that requires a prior `dispatched`. Any other order is `custody_out_of_order`, not a silent reordering.
- **K20 reconciliation invariant**: an exception disposition raises `unreconciled_count`; an `accepted` finding carrying `reconciles_id` clears exactly one, and only if the target belongs to the same job, is still an exception, and has not already been reconciled. Completion is refused with `unreconciled_exceptions` at the service boundary and by the `job_reconciliation_gate` CHECK in the database.
- **Optimistic revision locking**: `change_job_status` requires the exact `expectedRevision`; stale requests fail with `conflict`. Evidence recording is refused with `invalid_transition` once a job is `completed` or `canceled`.
- **Permission gating**: every mutation requires `fulfillment.write`; production material lines additionally require `fulfillment.produce`; custody stages `dispatched`, `delivered` and `returned` plus `delivery` findings require `fulfillment.dispatch`. Read paths require `fulfillment.read`, audit paths `audit.read`.
- **S1 resilient receipt state machine**: the DOM dashboard assigns a client-generated command UUID, marks the outcome uncertain _before_ dispatch, blocks new writes while an earlier outcome is unknown, and offers **Re-query status** (authoritative receipt reconciliation, including detection of an invalid or mismatched receipt) and **Retry same request** (same UUID, same body).
- **Tenant isolation**: every table carries forced row-level security on `archava.tenant_id`; the runtime role is neither superuser nor bypass-RLS nor schema owner, and cross-tenant job lookups return not found. `getAccess` parses through `fulfillmentAccessSchema`, so permissions outside the fulfillment vocabulary are rejected instead of trusted.

## Security & Verification Matrix

- **27 Synthetic Verification Checks (`pnpm fulfillment:verify`)**: two independently cold-installed logistics and production consumers sharing unchanged fulfillment code with a browser-safe dashboard, dispatch and production work-order registration, material/output/scrap traceability, per-job client ownership separation, custody order and monotonicity, return-before-dispatch refusal, dedicated dispatch and produce grant refusals, delivery findings raising the unreconciled counter, completion blocked while unreconciled, optimistic revision conflicts, one-clear-per-reconciliation, double-reconciliation refusal, completed-job freeze, idempotent replay, service-principal refusal, read-only tool boundaries, command status recovery, cross-tenant and inactive-principal refusal, append-only enforcement, runtime role privilege refusal, grant revocation, and migration checksum parity with the release catalog.
- **6 Headless Browser E2E Checks (`pnpm fulfillment:verify-host`)**: module-disabled 404, authentication and domain-grant boundaries (no auto-enrollment), compiled-host CSRF/origin/viewer/tool boundaries, restart with idempotent receipt recovery, a Chromium CDP receive-and-inspect workflow, and all five S1 receipt-recovery regressions (network interruption, HTTP 500 on command, non-JSON receipt, mismatched receipt command ID, and recovery-query outage plus authoritative `unknown` resolution) each asserting the recovery panel stays visible until a recorded receipt reconciles the pending command.
- **26 persistence tests and 7 contract tests** cover the repository against a stub pool plus the package's domain boundaries.
