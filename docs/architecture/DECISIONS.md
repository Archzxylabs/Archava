# Architecture Decisions & Evolution Ledger

Living chronicle of architectural refinements, module deepenings, and structural decisions for the Archava modular monolith.

This document succeeds the [Historical Rust Decisions](../archive/rust/DECISIONS.md) (ADR-R01 through ADR-R13). It is maintained as a single continuous ledger to eliminate file sprawl and provide a unified source of truth for the codebase's structural evolution.

---

## Navigation & Index

- [ADR-014: Deepen CRM Seam & Extract `archava-adapters::crm`](#adr-014-deepen-crm-seam--extract-archava-adapterscrm)
- [ADR-015: Decompose Host Router God-Module](#adr-015-decompose-host-router-god-module)
- [ADR-016: Formalize Cockpit L4/L5 Autonomous Mission Supervisor Gate](#adr-016-formalize-cockpit-l4l5-autonomous-mission-supervisor-gate)
- [ADR-017: Deepen Assistant Conversation Flow & Session Extraction](#adr-017-deepen-assistant-conversation-flow--session-extraction)
- [ADR-018: Normalize CRM as First-Class ERP Business Domain & Harmonize Seams](#adr-018-normalize-crm-as-first-class-erp-business-domain--harmonize-seams)
- [ADR-019: Documentation Harmonization, Canonical Domain Glossary & Zero-Redundancy Tooling Gate](#adr-019-documentation-harmonization-canonical-domain-glossary--zero-redundancy-tooling-gate)

---

## [ADR-014] Deepen CRM Seam & Extract `archava-adapters::crm`

- **Status**: Completed
- **Date**: 2026-10-31
- **Domain**: CRM & Core Persistence Layer
- **Impacted Files**:
  - [`crates/host/src/crm.rs`](../../crates/host/src/erp/crm.rs)
  - [`crates/adapters/src/lib.rs`](../../crates/adapters/src/lib.rs)
  - [`crates/adapters/src/crm.rs`](../../crates/adapters/src/erp/crm.rs)
  - Documentation: [`docs/modules/CRM_MODULE.md`](../modules/CRM_MODULE.md) and [`docs/architecture/README.md`](README.md)

### Context & Friction

In Archava, all 20+ ERP ledgers (commerce, purchasing, foodservice, laundry, accommodation, billing, etc.) have their PostgreSQL persistence cleanly sequestered inside `crates/adapters/src/erp/*` behind atomic `FORCE RLS` transactions.

CRM has remained the solitary architectural exception: [`crates/host/src/crm.rs`](../../crates/host/src/erp/crm.rs) (2,612 lines) couples Axum HTTP request handling, tenant auth, state machine pipeline transitions, mutation claims, and raw SQL queries directly in the host crate. This represents a shallow, leaky seam:
1. **Asymmetry**: The database layer is split — ERP queries live in `archava-adapters`, but CRM queries live in `archava-host`.
2. **Lack of Locality**: Schema changes, transaction timeouts, and SQL adjustments for CRM require modifying the HTTP layer.
3. **Low Leverage**: Other consumers (CLI tools, background maintenance jobs, Ops routines) cannot execute CRM mutations without depending on the Axum HTTP router stack.

### Decision

Extract the complete CRM persistence engine from [`crates/host/src/crm.rs`](../../crates/host/src/erp/crm.rs) into a dedicated, deep repository module in `crates/adapters/src/crm.rs`.

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

2. **[`crates/host/src/crm.rs`](../../crates/host/src/erp/crm.rs) (The HTTP Coordinator)**:
   - Shrinks from ~2,600 lines to ~400 lines.
   - Responsible strictly for HTTP boundary concerns: parsing JSON request bodies, extracting authenticated session claims, verifying CSRF headers, delegating to `adapters::crm`, and mapping domain receipts or refusals to HTTP status codes.

### Invariants & Safety

- **Layer Direction**: `archava-adapters` depends only on `core` and `domain`. It must NEVER depend on `host`.
- **Extractor Safety**: [`crates/host/src/extractors.rs`](../../crates/host/src/extractors.rs) compile-time assertions remain satisfied.
- **Append-only Migrations**: No database migrations are altered; existing table schemas and indexes remain identical.
- **Zero Test Regressions**: All 28+ CRM integration test cases in `crates/host/tests/crm.rs` must pass without alteration to their behavioral expectations.

### Execution Checklist

- [x] **Phase 1: Adapter Module Scaffolding**
  - Create `crates/adapters/src/crm.rs` and register `pub mod crm;` in [`crates/adapters/src/lib.rs`](../../crates/adapters/src/lib.rs).
  - Define `CrmPersistenceError`, `CrmCommand`, and domain receipt structures in `adapters::crm`.
- [x] **Phase 2: SQL & Transaction Extraction**
  - Migrate contact creation, update, activity logging, follow-up tracking, and pipeline opportunity transition queries.
  - Implement atomic idempotency claim and audit logging within transaction scope.
- [x] **Phase 3: Host Coordinator Refactor**
  - Rewire [`crates/host/src/crm.rs`](../../crates/host/src/erp/crm.rs) to call `archava_adapters::crm`.
  - Strip redundant SQL strings and direct pool operations from `crates/host`.
- [x] **Phase 4: Verification & Docs Alignment**
  - Run `cargo test -p archava-host --test crm --locked -- --test-threads=1`.
  - Update [`docs/modules/CRM_MODULE.md`](../modules/CRM_MODULE.md) boundary table to list `crates/adapters/src/crm.rs`.
  - Run full gate: `node tools/rust/verify.mjs`.

---

## [ADR-015] Decompose Host Router God-Module

- **Status**: Completed
- **Date**: 2026-10-31
- **Domain**: HTTP Host Routing & Boundary Enforcement
- **Impacted Files**:
  - [`crates/host/src/router.rs`](../../crates/host/src/router.rs) (reduced from 3,913 LOC to 1,738 LOC, handler logic extracted)
  - [`crates/host/src/lib.rs`](../../crates/host/src/lib.rs) (registered `pub mod routes;`)
  - [`crates/host/src/extractors.rs`](../../crates/host/src/extractors.rs) (registered all route files in compile-time tripwire)
  - Created subtrees under `crates/host/src/routes/`:
    - [`crates/host/src/routes/mod.rs`](../../crates/host/src/routes/mod.rs) (`mount_routes`)
    - [`crates/host/src/routes/auth.rs`](../../crates/host/src/routes/auth.rs) (`/login`, `/logout`, cookie lifecycle)
    - [`crates/host/src/routes/commands.rs`](../../crates/host/src/routes/commands.rs) (`/commands`, `/commands/status`, `/approvals`, `/approvals/approve`)
    - [`crates/host/src/routes/crm.rs`](../../crates/host/src/routes/crm.rs) (`/contacts`, `/crm/records`, `/crm/commands`, `/crm/commands/status`, `/own/*`, `/api/crm/{tenant}/conversation`)
    - [`crates/host/src/routes/erp.rs`](../../crates/host/src/routes/erp.rs) (`/erp/*` command and read endpoints across 17 ERP ledgers)
    - [`crates/host/src/routes/cockpit.rs`](../../crates/host/src/routes/cockpit.rs) (`/api/v1/cockpit/*`, `/api/v1/orbz/*`, feeds, presets, presence, preview, ingestion)
    - [`crates/host/src/routes/webhooks.rs`](../../crates/host/src/routes/webhooks.rs) (`/api/webhooks/privy`, `/api/webhooks/payment`)
    - [`crates/host/src/routes/system.rs`](../../crates/host/src/routes/system.rs) (`/healthz`, `/readyz`, `/metrics`, static assets, embed checkout)

### Context & Friction

[`crates/host/src/router.rs`](../../crates/host/src/router.rs) was the largest single god-module in the workspace (3,913 LOC). It mounted 10 distinct route families, handled extractor rejection enforcement, implemented hundreds of lines of request handler functions, and contained ~1,000 lines of test fixtures. This created significant cognitive friction and context exhaustion for both human maintainers and AI contexts.

### Decision

Decompose `router.rs` into modular route subtrees under `crates/host/src/routes/`:
- `routes/auth.rs`: `/login`, `/logout`, session cookie expiration.
- `routes/commands.rs`: `/commands`, `/commands/status`, `/approvals`, `/approvals/approve`.
- `routes/crm.rs`: `/contacts`, `/crm/records`, `/crm/commands`, `/crm/commands/status`, `/own/*`, CRM conversation.
- `routes/erp.rs`: `/erp/*/commands`, `/erp/*/*` workspace and read routes across all 17 ERP ledgers.
- `routes/cockpit.rs`: `/api/v1/cockpit/*`, `/api/v1/orbz/*`, feeds, presets, presence, preview, ingestion.
- `routes/webhooks.rs`: `/api/webhooks/privy`, `/api/webhooks/payment`.
- `routes/system.rs`: `/healthz`, `/readyz`, `/metrics`, static assets (`archava.js`, `archava.css`, `workspace.css`), embed checkout.

The root `router.rs` retains the thin composition root (`pub fn router(state: AppState) -> Router`), shared authorization & error response helpers (`authenticate`, `authority_refusal`, `forbidden`, `unavailable`, `query_rejection`, etc.), and 100% backward-compatible re-exports.

### Execution Checklist

- [x] **Phase 1: Route Subtree Structure & Module Registration**
  - Create `crates/host/src/routes/` and declare `pub mod routes;` in `crates/host/src/lib.rs`.
  - Register all new route files in `crates/host/src/extractors.rs` `MODULES` compile-time tripwire.
- [x] **Phase 2: Handlers & Sub-routers Migration**
  - Extract auth endpoints to `routes/auth.rs`.
  - Extract generic command & approval endpoints to `routes/commands.rs`.
  - Extract CRM & owner endpoints to `routes/crm.rs`.
  - Extract ERP command & workspace endpoints across all 17 ledgers to `routes/erp.rs`.
  - Extract Cockpit missions, Orbz, feed, preset, presence, preview, and ingestion to `routes/cockpit.rs`.
  - Extract webhook handlers to `routes/webhooks.rs`.
  - Extract system health, metrics, assets, and embed checkout to `routes/system.rs`.
- [x] **Phase 3: Root Composition & Backward Compatibility**
  - Simplify `router.rs::router()` to mount sub-routers via `routes::mount_routes(state.clone())`.
  - Re-export all moved public types, constants, and helper functions from `router.rs`.
- [x] **Phase 4: Verification & Link Validation**
  - Validate structure: `node tools/check-structure.mjs`.
  - Validate doc links: `node tools/check-docs.mjs`.
  - Verify workspace clippy with `-D warnings`.
  - Pass all unit and integration test suites with `--test-threads=1`.

---

## [ADR-016] Formalize Cockpit L4/L5 Autonomous Mission Supervisor Gate

- **Status**: Completed
- **Date**: 2026-10-31
- **Domain**: Cockpit Agentic Orchestration & Safety
- **Impacted Files**:
  - [`crates/cockpit/src/mission.rs`](../../crates/cockpit/src/mission.rs) (`ApprovalGate`, `GateStatus`, `SupervisorStamp`, `ApprovalGateError`, `StepStatus::RequiresApproval`)
  - [`crates/cockpit/src/lib.rs`](../../crates/cockpit/src/lib.rs) (`approve_step_gate`, engine gate enforcement)
  - [`crates/core/src/roles.rs`](../../crates/core/src/roles.rs) (`CapabilityTier::as_str`)
  - [`crates/host/src/cockpit.rs`](../../crates/host/src/cockpit.rs) (step creation gate configuration, `approve_step` handler)
  - [`crates/host/src/routes/cockpit.rs`](../../crates/host/src/routes/cockpit.rs) (`/api/v1/cockpit/missions/{id}/steps/{step_index}/approve`)
  - [`crates/slice/src/operations/missions.rs`](../../crates/slice/src/operations/missions.rs) (`ui.approve_step`, step approval UI badge)
  - [`crates/host/tests/cockpit_hold_resume.rs`](../../crates/host/tests/cockpit_hold_resume.rs) (`test_cockpit_step_approval_gate_lifecycle_and_non_self_approval`)

### Context & Friction

Autonomous subagents executing L4/L5 missions in Ava Cockpit can perform high-impact side effects (e.g. invoice emission, large stock adjustments, external webhook dispatches). Previously, human supervisor approvals were coordinated partially through client-side UI signals (`MissionUi`) and unverified JSON states.

### Decision

Deepen `crates/cockpit/src/mission.rs` by introducing a first-class `ApprovalGate` state machine within `FlightStep`. High-impact steps cannot transition from `RequiresApproval` to `Running` without a cryptographically verified supervisor approval token and role capability check enforced directly by the domain engine.

### Invariants & Guarantees

1. **Non-Self-Approval**: Initiators cannot self-approve steps or missions requiring L4 or L5 authority. Independent supervisor signoff is mandatory.
2. **Cryptographic Token Verification**: Approval tokens are generated via HMAC-SHA256 bound to `(tenant, mission_id, step_index, supervisor_id, role, capability, timestamp)` and verified using constant-time comparison (`subtle::ConstantTimeEq`).
3. **Execution Block**: Handlers `run-next` and `complete` strictly refuse unapproved steps with 409 Conflict.
4. **Zero Migration Footprint**: Approval gates serialize cleanly within `flight_steps.payload_input` JSONB and map to `status = 'requires_approval'` without schema changes.

### Execution Checklist

- [x] **Phase 1: Domain State Machine**
  - Implement `ApprovalGate`, `GateStatus`, `SupervisorStamp`, and `ApprovalGateError` in `crates/cockpit/src/mission.rs`.
  - Add `StepStatus::RequiresApproval`.
  - Implement token generation and constant-time verification.
- [x] **Phase 2: Host Router & Persistence Gate**
  - Add `POST /api/v1/cockpit/missions/{id}/steps/{step_index}/approve` in `crates/host/src/routes/cockpit.rs` and `cockpit.rs`.
  - Enforce non-self-approval and supervisor role checks in `cockpit.rs::approve`.
  - Block execution of `RequiresApproval` steps in `run_next` and `complete`.
- [x] **Phase 3: Leptos UI Operations**
  - Add `approve_step` action and supervisor approval button in `crates/slice/src/operations/missions.rs`.
- [x] **Phase 4: Verification & Link Validation**
  - Add integration test `test_cockpit_step_approval_gate_lifecycle_and_non_self_approval`.
  - Verify workspace Clippy, WASM target, docs links, and all tests.

---

## [ADR-017] Deepen Assistant Conversation Flow & Session Extraction

- **Status**: Completed
- **Date**: 2026-10-31
- **Domain**: CRM Assistant & Host Integration Seam
- **Impacted Files**:
  - [`crates/host/src/assistant/mod.rs`](../../crates/host/src/assistant/mod.rs)
  - [`crates/host/src/assistant/sql_host.rs`](../../crates/host/src/assistant/sql_host.rs)
  - [`crates/host/src/assistant/bridge.rs`](../../crates/host/src/assistant/bridge.rs)
  - [`crates/host/src/extractors.rs`](../../crates/host/src/extractors.rs)

### Context & Friction

The legacy `crates/host/src/assistant.rs` (1,107 lines) combined three distinct architectural concerns into a single god-module:
1. **HTTP Extraction & Wire Protocol**: Tenant path validation, session/CSRF authentication, utterance intake, System 1 shadow decision logging, and masked house JSON envelope mapping.
2. **Type & Vocabulary Translation Bridge**: Mapping CRM domain vocabulary (`Scope`, `PipelineStage`, `ActivityKind`, `FollowUpStatus`) and database errors into typed `AssistantError` envelopes without leaking sensitive query text or internal identifiers.
3. **`SqlHost` Persistence Engine**: 800 lines implementing `HostCrm` over real PostgreSQL transactions, running scoped queries with `FORCE RLS` for contacts, follow-up queues, completion drafts, and opportunity pipelines with `FOR SHARE` concurrency control.

### Decision

Decompose `crates/host/src/assistant.rs` into modular subtrees under [`crates/host/src/assistant/`](../../crates/host/src/assistant/):
- **[`mod.rs`](../../crates/host/src/assistant/mod.rs)**: Houses the Axum HTTP handler `POST /api/crm/{tenant}/conversation`, session extraction, System 1 shadow pass, and wire error response masking. Re-exports `bridge` and `sql_host`.
- **[`bridge.rs`](../../crates/host/src/assistant/bridge.rs)**: Pure translation layer bridging authority errors, database errors, and CRM storage strings into `archava_assistant` vocabulary (`bridged_stage`, `bridged_access`, `bridged_scope`, `as_uuid`, `escaped_search`).
- **[`sql_host.rs`](../../crates/host/src/assistant/sql_host.rs)**: Dedicated `SqlHost<'c>` engine implementing `HostCrm` against the `archava_crm` schema with transaction-level isolation and RLS executor re-checks.

Updated compile-time tripwire [`crates/host/src/extractors.rs`](../../crates/host/src/extractors.rs) to audit all 3 modular files in `MODULES`. All 14 integration tests in `tests/assistant.rs` and 81 host unit tests remain 100% green.

---

## [ADR-018] Normalize CRM as First-Class ERP Business Domain & Harmonize Seams

- **Status**: Completed
- **Date**: 2026-10-31
- **Domain**: Enterprise Architecture & Domain Normalization
- **Impacted Files**:
  - [`crates/domain/src/erp/crm.rs`](../../crates/domain/src/erp/crm.rs)
  - [`crates/domain/src/erp/mod.rs`](../../crates/domain/src/erp/mod.rs)
  - [`crates/adapters/src/erp/mod.rs`](../../crates/adapters/src/erp/mod.rs)
  - [`crates/adapters/src/crm.rs`](../../crates/adapters/src/erp/crm.rs)
  - [`crates/host/src/routes/erp.rs`](../../crates/host/src/routes/erp.rs)
  - [`crates/host/src/routes/crm.rs`](../../crates/host/src/routes/crm.rs)
  - [`docs/architecture/README.md`](README.md)
  - [`docs/modules/CRM_MODULE.md`](../modules/CRM_MODULE.md)

### Context & Friction

Historical evolution generated a false conceptual dichotomy ("ERP vs CRM") across codebase terminology and placement:
1. **Isolated Domain Vocabulary**: Pure domain contracts for all other 15 business capabilities lived cleanly under `crates/domain/src/erp/*`, but CRM types (`ActivityKind`, `PipelineStage`, `ContactFields`, `ContactPatch`) were stranded directly in `archava-adapters`.
2. **Missing Adapter Hierarchy**: All 16 ERP repositories were organized under `archava_adapters::erp::*`, whereas CRM sat alone as `archava_adapters::crm`.
3. **Route Inconsistency**: ERP exposed unified command and query endpoints under `/erp/{module}/*`, while CRM only had `/crm/*` and `/contacts`.
4. **Architectural Confusion**: Documentation and code comments frequently referred to "ERP and CRM" as if they were peer architectures, rather than recognizing CRM as the front-office Customer Relationship Management pillar of the Archava ERP Monolith.

### Decision

Normalize CRM as a first-class business module of the Archava ERP suite across all layers while preserving 100% backwards compatibility:

1. **Domain Layer (`crates/domain/src/erp/crm.rs`)**:
   Created `archava_domain::erp::crm` containing pure, WASM32-safe domain types, permission vocabularies, pipeline transition machines, contact fields/patches, and validation predicates. Registered `pub mod crm;` in `crates/domain/src/erp/mod.rs`.
2. **Adapter Layer (`crates/adapters/src/erp/mod.rs`)**:
   Exposed `pub use crate::crm;` under `archava_adapters::erp::crm`, formally grouping CRM persistence alongside the rest of the ERP cohorts while preserving `archava_adapters::crm` for backward compatibility.
3. **Persistence Refactor (`crates/adapters/src/crm.rs`)**:
   Refactored `archava_adapters::crm` to consume and re-export domain types directly from `archava_domain::erp::crm::*`, eliminating redundant duplicate definitions.
4. **Host Route Alignment (`crates/host/src/routes/erp.rs`)**:
   Mounted canonical `/erp/crm/commands`, `/erp/crm/commands/status`, `/erp/crm/records`, and `/erp/crm/contacts` routes under the ERP router tree, while retaining `/crm/*` and `/contacts` in `routes/crm.rs` as front-office client aliases.
5. **Documentation Harmony**:
   Updated `docs/architecture/README.md` and `docs/modules/CRM_MODULE.md` to define Archava unequivocally as a Modular ERP Monolith with CRM as its native front-office pillar.

### Addendum (2026-11-12): physical relocation

The compatibility re-export is gone. The adapter now lives at `crates/adapters/src/erp/crm.rs` and the HTTP boundary at `crates/host/src/erp/crm.rs`; the only module paths are `archava_adapters::erp::crm` and `archava_host::erp::crm`. `crates/host/src/routes/crm.rs` stays where it is: it is the front-office route family (`/contacts`, `/crm/*`, agent, owner configuration, assistant), not a CRM module. URLs, the `crm-pg` ledger, migration IDs and the `tests/crm.rs` suite name are unchanged. File names in the ADR-014 and ADR-018 text above describe the tree at the time of those decisions.

---

## [ADR-019] Documentation Harmonization, Canonical Domain Glossary & Zero-Redundancy Tooling Gate

- **Status**: Completed
- **Date**: 2026-10-31
- **Domain**: Documentation, Static Assets & Tooling Quality Gate
- **Impacted Files**:
  - [`docs/GLOSSARY.md`](../GLOSSARY.md)
  - [`docs/README.md`](../README.md)
  - [`AGENTS.md`](../../AGENTS.md)
  - [`docs/architecture/README.md`](README.md)
  - [`docs/architecture/LAYA_AGENTIC_SYSTEM.md`](LAYA_AGENTIC_SYSTEM.md)
  - [`docs/DEVELOPER_GUIDE.md`](../DEVELOPER_GUIDE.md)
  - `static/v1/archava.js` (Deleted)
  - [`crates/slice/src/server.rs`](../../crates/slice/src/server.rs)
  - [`tools/rust/verify.mjs`](../../tools/rust/verify.mjs)
  - [`tools/test/verify.test.mjs`](../../tools/test/verify.test.mjs)

### Context & Friction

1. **Cognitive Load & Domain Ambiguity**: Key domain terms (Ava, Orbz, Cockpit, FORCE RLS, L0/L1/L2 memory disclosure, flight plan DAG, Tier 1 pillars) were dispersed across multiple design docs, PRDs, and ADRs without a single authoritative glossary.
2. **Static Asset Redundancy & Missing Font**: A duplicate clone directory `static/v1/archava.js` (609 lines) existed alongside `static/archava.js` while the server already served the root file directly. Meanwhile, `static/archava.css` imported `/assets/fonts/space-grotesk.woff2`, but the SSR slice router only mounted DM Sans and JetBrains Mono, returning a 404 for Space Grotesk.
3. **Factual Metric Drift**: `docs/architecture/README.md` recorded "38 SQL files", out-of-date compared to the 44 checked-in SQL files across 30 ledgers in `migrations/README.md`.
4. **Unwired Test in Tooling**: `tools/test/native-provenance.test.mjs` was fully functional but omitted from `tools/rust/verify.mjs`.

### Decision

1. **Canonical Domain Glossary (`docs/GLOSSARY.md`)**:
   Created a single source of truth for all domain concepts: Ava Sovereign AI, Orbz agentic memory, Cockpit durable flight execution, Modular Monolith layer boundaries, FORCE RLS multi-tenancy, and Tier 1 pillars. Linked across `docs/README.md`, `AGENTS.md`, and `docs/architecture/README.md`.
2. **Static Asset Cleanliness & Font Mount (`crates/slice/src/server.rs`)**:
   Deleted the redundant `static/v1/` clone. Mounted `/assets/fonts/space-grotesk.woff2` into the Axum slice router alongside DM Sans and JetBrains Mono, completing the offline font trinity and adding test coverage in `the_font_routes_serve_woff2_bytes`.
3. **Documentation Factual Alignment**:
   Synchronized SQL migration counts to 44 files across 30 ledgers (37 mounted, 7 unmounted) in `docs/architecture/README.md`. Clarified Ava as the sovereign persona in `docs/architecture/LAYA_AGENTIC_SYSTEM.md`.
4. **Tooling Gate Completion (`tools/rust/verify.mjs`)**:
   Wired `native-provenance.test.mjs` into `verify.mjs` and guarded it with an assertion in `tools/test/verify.test.mjs`, elevating the internal Node test suite from 16 to 17 green tests.
