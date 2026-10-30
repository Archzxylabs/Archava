# Laundry and dry cleaning processing module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/laundry.rs), [persistence](../../crates/adapters/src/erp/laundry.rs), [HTTP](../../crates/host/src/erp/laundry.rs), [integration tests](../../crates/host/tests/erp_laundry.rs).

This contract covers laundry intake with counted garments, frozen priced order intent, a monotonic processing chain, quality findings with reconciliation, and quality-gated handover back to the customer.
Candidate release 1.17.0 adds bounded laundry processing authority without altering existing CRM, commerce, fulfillment, billing or scheduling authority.
Full laundry and dry cleaning operating kits retain their other requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/laundry` owns strict contracts, human service commands, private read tools, HTTP API and a reusable DOM dashboard with a hardened state machine receipt recovery (S1). It depends only on pinned Zod and needs no external CRM or AI provider.
`@archava/laundry-pg` owns seven forced-RLS business tables: `memberships`, `orders`, `order_lines`, `process_events`, `findings`, `commands`, and `audit`. Its checked migration ledger `001_laundry_orders_processing.sql` is checksummed (`0152962d7fe05b506c757c739657615334acbda62f04eb5c61ba3fc340ba08a6`) and constitutes ledger ke-21.

The package deliberately composes **nothing** from commerce or billing: commerce remains the authority for catalog stock, billing remains the authority for invoicing and settlement, while laundry owns garment custody, processing and quality evidence only. Prices are recorded as frozen minor-unit integers at intake; no pricing engine, tax calculation or payment behaviour exists here.

Two independent business consumers demonstrate generalization:

1. **Consumer 1: Kilo laundry (`K19: laundry-processing`, weight-based)**:
   Counts pieces at intake, prices by weight in integer grams with per-gram minor-unit rates, and moves the batch through washing, drying and pressing with machine-side operators under a dedicated grant.
2. **Consumer 2: Dry cleaning and steam pressing (`K19: laundry-processing`, item-based)**:
   Prices per garment piece (shirts, suits), keeps customer references separated, and returns pressed garments only after a recorded quality check.

## Lifecycle and Invariants

```text
Order lifecycle:
received ──[change_order_status]──> processing ──> ready ──> returned
 │                                     │
 └──[canceled]<────────────────────────┘

Processing chain:
intake ──> tag ──> wash ──> dry ──> press ──> quality_check ──> ready ──> handover

Quality gate:
finding(partial|rejected)              ──> unreconciled_count +1
finding(accepted, reconciles_id)       ──> unreconciled_count -1
ready refused while unreconciled_count > 0 or no quality_check exists
```

- **Human-only mutations**: `create_order`, `record_process`, `record_finding` and `change_order_status` are refused for service/automation principals at the service boundary.
- **Frozen priced intent**: `create_order` accepts validated lines only (`itemRef`, positive integer `quantity`, `unit` of `piece` or `gram`, non-negative `unitPriceMinor`); the server computes `line_total_minor` and `total_minor` and the client's payload never carries a total. `order_code`, `kind`, `customer_ref`, `currency`, `total_minor`, `created_at` and `owner_id` are immutable at the database boundary, and orders are undeletable, so a priced order intent can be neither rewritten nor erased.
- **Append-only evidence**: `order_lines`, `process_events` and `findings` refuse update and delete at the database boundary; `audit` and `commands` carry the same guarantees. Piece and gram quantities accumulate on separate counters (`item_count`, `weight_grams`).
- **Processing chain ordering**: the first process step must be `intake`; steps are monotonic by `intake(0) < tag(1) < wash(2) < dry(3) < press(4) < quality_check(5) < ready(6) < handover(7)`. Any repeated or backwards step is `process_out_of_order`, not a silent reordering.
- **Quality gate**: an exception disposition (`partial`, `rejected`) raises `unreconciled_count`; an `accepted` finding carrying `reconciles_id` clears exactly one, and only if the target belongs to the same order, is still an exception, and has not already been reconciled. `ready` is refused with `unreconciled_exceptions` while an exception is open, with `invalid_transition` until a `quality_check` exists, and by the `order_quality_gate` CHECK in the database. `returned` and `canceled` are terminal and refuse further evidence.
- **Optimistic revision locking**: `change_order_status` requires the exact `expectedRevision`; stale requests fail with `conflict`.
- **Permission gating**: every mutation is human-gated; machine-side steps `wash`, `dry` and `press` additionally require the dedicated `laundry.process` grant, while counter-side steps (`intake`, `tag`, `quality_check`, `ready`, `handover`), findings and lifecycle changes require `laundry.write`. Read paths require `laundry.read`, audit paths `audit.read`. Machine-only operators therefore cannot take in or hand back garments, and counter operators cannot record machine cycles.
- **S1 resilient receipt state machine**: the DOM dashboard assigns a client-generated command UUID, marks the outcome uncertain _before_ dispatch, blocks new writes while an earlier outcome is unknown, and offers **Re-query status** (authoritative receipt reconciliation, including detection of an invalid or mismatched receipt) and **Retry same request** (same UUID, same body).
- **Tenant isolation**: every table carries forced row-level security on `archava.tenant_id`; the runtime role is neither superuser nor bypass-RLS nor schema owner, and cross-tenant order lookups return not found. `getAccess` parses through `laundryAccessSchema`, so permissions outside the laundry vocabulary are rejected instead of trusted.

## Security & Verification Matrix

- **30 Synthetic Verification Checks (`pnpm laundry:verify`)**: two independently cold-installed kiloan and dryclean consumers sharing unchanged laundry code with a browser-safe dashboard, idempotent migration parity with the release catalog, priced weight and item order registration, server-computed frozen totals with split piece/gram counters, customer reference separation, chain refusal before intake, dedicated machine and counter grant refusals, strict monotonic chain order, repeat/backwards refusal, quality findings raising the unreconciled counter, ready blocked while unreconciled and until a quality check exists, optimistic revision conflicts, one-clear-per-reconciliation, double-reconciliation and cross-order reconciliation refusal, returned-order freeze, full received→processing→ready→returned lifecycle, idempotent replay, fingerprint-mismatch replay refusal, service-principal refusal, read-only tool boundaries with unknown-tool refusal, command status recovery, cross-tenant and inactive-principal refusal, append-only plus intent-immutability enforcement, runtime role privilege refusal, grant revocation, and migration checksum parity.
- **6 Headless Browser E2E Checks (`pnpm laundry:verify-host`)**: module-disabled 404, authentication and domain-grant boundaries (no auto-enrollment), compiled-host CSRF/origin/viewer/tool boundaries, restart with idempotent receipt recovery, a Chromium CDP intake workflow, and all five S1 receipt-recovery regressions (network interruption, HTTP 500 on command, non-JSON receipt, mismatched receipt command ID, and recovery-query outage plus authoritative `unknown` resolution) each asserting the recovery panel stays visible until a recorded receipt reconciles the pending command.
- **35 persistence and contract tests** cover the repository and service against a stub pool plus the package's domain boundaries.

Limits: no barcode hardware integration, no route/driver dispatch, no chemical inventory or recipe engine, no per-garment RFID tracking, no pricing beyond frozen integer minor units, and no invoicing or payment behaviour (billing owns that). Full `K19` acceptance also still requires commerce, invoicing and payment evidence from their owning modules. See [build status](../archive/audits/BUILD_STATUS_2026-10-29.md) for the honest classification of every PRD area.
