# Architecture Decisions & Evolution Ledger

Living chronicle of architectural refinements, module deepenings, and structural decisions for the Archava modular monolith.

This document succeeds the [Historical Rust Decisions](../archive/rust/DECISIONS.md) (ADR-R01 through ADR-R13). It is maintained as a single continuous ledger to eliminate file sprawl and provide a unified source of truth for the codebase's structural evolution.

---

## Navigation & Index

- [ADR-014: Deepen CRM Seam & Extract `archava-adapters::crm`](#adr-014-deepen-crm-seam--extract-archava-adapterscrm)
- [ADR-015: Decompose Host Router God-Module](#adr-015-decompose-host-router-god-module)
- [ADR-016: Formalize Cockpit L4/L5 Autonomous Mission Supervisor Gate](#adr-016-formalize-cockpit-l4l5-autonomous-mission-supervisor-gate)

---

## [ADR-014] Deepen CRM Seam & Extract `archava-adapters::crm`

- **Status**: Accepted / Active
- **Date**: 2026-10-31
- **Domain**: CRM & Core Persistence Layer
- **Impacted Files**:
  - [`crates/host/src/crm.rs`](../../crates/host/src/crm.rs)
  - [`crates/adapters/src/lib.rs`](../../crates/adapters/src/lib.rs)
  - New target: `crates/adapters/src/crm.rs`
  - Documentation: [`docs/modules/CRM_MODULE.md`](../modules/CRM_MODULE.md) and [`docs/architecture/README.md`](README.md)

### Context & Friction

In Archava, all 20+ ERP ledgers (commerce, purchasing, foodservice, laundry, accommodation, billing, etc.) have their PostgreSQL persistence cleanly sequestered inside `crates/adapters/src/erp/*` behind atomic `FORCE RLS` transactions.

CRM has remained the solitary architectural exception: [`crates/host/src/crm.rs`](../../crates/host/src/crm.rs) (2,612 lines) couples Axum HTTP request handling, tenant auth, state machine pipeline transitions, mutation claims, and raw SQL queries directly in the host crate. This represents a shallow, leaky seam:
1. **Asymmetry**: The database layer is split — ERP queries live in `archava-adapters`, but CRM queries live in `archava-host`.
2. **Lack of Locality**: Schema changes, transaction timeouts, and SQL adjustments for CRM require modifying the HTTP layer.
3. **Low Leverage**: Other consumers (CLI tools, background maintenance jobs, Ops routines) cannot execute CRM mutations without depending on the Axum HTTP router stack.

### Decision

Extract the complete CRM persistence engine from [`crates/host/src/crm.rs`](../../crates/host/src/crm.rs) into a dedicated, deep repository module in `crates/adapters/src/crm.rs`.

#### Seam Division of Responsibilities

1. **`crates/adapters/src/crm.rs` (The Deep Module)**:
   - Owns all SQL statements for `contacts`, `contact_activities`, `followups`, `opportunities`, and `opportunity_transitions`.
   - Manages transaction lifecycle: `SET LOCAL lock_timeout`, `SET LOCAL statement_timeout`, and RLS tenant isolation (`SET LOCAL archava.current_tenant`).
   - Owns idempotency claim handling on `commands` table and audit recording on `audit` table.
   - Exposes a minimal, deep interface to callers:
     ```rust
     pub async fn execute_command(
         pool: &PgPool,
         tenant: &TenantId,
         actor: &Principal,
         command: CrmCommand,
     ) -> Result<CommandReceipt, CrmPersistenceError>;

     pub async fn read_status(
         pool: &PgPool,
         tenant: &TenantId,
         actor: &Principal,
         request_id: &str,
     ) -> Result<Option<CommandReceipt>, CrmPersistenceError>;
     ```

2. **[`crates/host/src/crm.rs`](../../crates/host/src/crm.rs) (The HTTP Coordinator)**:
   - Shrinks from ~2,600 lines to ~400 lines.
   - Responsible strictly for HTTP boundary concerns: parsing JSON request bodies, extracting authenticated session claims, verifying CSRF headers, delegating to `adapters::crm`, and mapping domain receipts or refusals to HTTP status codes.

### Invariants & Safety

- **Layer Direction**: `archava-adapters` depends only on `core` and `domain`. It must NEVER depend on `host`.
- **Extractor Safety**: [`crates/host/src/extractors.rs`](../../crates/host/src/extractors.rs) compile-time assertions remain satisfied.
- **Append-only Migrations**: No database migrations are altered; existing table schemas and indexes remain identical.
- **Zero Test Regressions**: All 28+ CRM integration test cases in `crates/host/tests/crm.rs` must pass without alteration to their behavioral expectations.

### Execution Checklist

- [ ] **Phase 1: Adapter Module Scaffolding**
  - Create `crates/adapters/src/crm.rs` and register `pub mod crm;` in [`crates/adapters/src/lib.rs`](../../crates/adapters/src/lib.rs).
  - Define `CrmPersistenceError`, `CrmCommand`, and domain receipt structures in `adapters::crm`.
- [ ] **Phase 2: SQL & Transaction Extraction**
  - Migrate contact creation, update, activity logging, follow-up tracking, and pipeline opportunity transition queries.
  - Implement atomic idempotency claim and audit logging within transaction scope.
- [ ] **Phase 3: Host Coordinator Refactor**
  - Rewire [`crates/host/src/crm.rs`](../../crates/host/src/crm.rs) to call `archava_adapters::crm`.
  - Strip redundant SQL strings and direct pool operations from `crates/host`.
- [ ] **Phase 4: Verification & Docs Alignment**
  - Run `cargo test -p archava-host --test crm --locked -- --test-threads=1`.
  - Update [`docs/modules/CRM_MODULE.md`](../modules/CRM_MODULE.md) boundary table to list `crates/adapters/src/crm.rs`.
  - Run full gate: `node tools/rust/verify.mjs`.

---

## [ADR-015] Decompose Host Router God-Module

- **Status**: Proposed / Queued
- **Date**: 2026-10-31
- **Domain**: HTTP Host Routing & Boundary Enforcement
- **Impacted Files**:
  - [`crates/host/src/router.rs`](../../crates/host/src/router.rs) (3,913 lines)
  - Target: `crates/host/src/routes/` (`auth.rs`, `crm.rs`, `erp.rs`, `cockpit.rs`, `webhooks.rs`, `system.rs`)

### Context & Friction

[`crates/host/src/router.rs`](../../crates/host/src/router.rs) is the largest single file in the workspace (3,913 LOC). It mounts 10 distinct route families, handles extractor rejection enforcement, implements route handlers, and contains ~1,500 lines of test fixtures. This creates significant cognitive friction for both human maintainers and AI contexts.

### Decision

Decompose `router.rs` into modular route subtrees under `crates/host/src/routes/`:
- `routes/auth.rs`: `/login`, `/logout`, session lifecycle.
- `routes/crm.rs`: `/commands`, `/approvals`, `/crm/*`.
- `routes/erp.rs`: `/erp/*/commands`, `/erp/*/*` reads across all cohorts.
- `routes/cockpit.rs`: `/api/v1/cockpit/*`, `/api/v1/orbz/*`.
- `routes/webhooks.rs`: `/api/webhooks/privy`, `/api/webhooks/payment/*`.
- `routes/system.rs`: `/healthz`, `/readyz`, `/metrics`.

The root `router.rs` becomes a thin composition root (~150 LOC) that combines these routers.

---

## [ADR-016] Formalize Cockpit L4/L5 Autonomous Mission Supervisor Gate

- **Status**: Proposed / Queued
- **Date**: 2026-10-31
- **Domain**: Cockpit Agentic Orchestration & Safety
- **Impacted Files**:
  - `crates/cockpit/src/mission.rs`
  - `crates/cockpit/src/flight_status.rs`
  - `crates/slice/src/operations/missions.rs`

### Context & Friction

Autonomous subagents executing L4/L5 missions in Ava Cockpit can perform high-impact side effects (e.g. invoice emission, large stock adjustments, external webhook dispatches). Currently, human supervisor approvals are coordinated partially through client-side UI signals (`MissionUi`) and unverified JSON states.

### Decision

Deepen `crates/cockpit/src/mission.rs` by introducing a first-class `ApprovalGate` state machine within `FlightStep`. High-impact steps cannot transition from `RequiresApproval` to `Running` without a cryptographically verified supervisor approval token and role capability check enforced directly by the domain engine.
