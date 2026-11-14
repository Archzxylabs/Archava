# Archava Domain Glossary

Canonical vocabulary, domain boundaries, and conceptual definitions for the Archava Rust modular monolith.

This glossary acts as the single source of truth for architectural and business terminology across all 21 workspace crates, database schemas, and documentation.

---

Product terms such as autonomous operator describe the intended capability.
Current runtime acceptance is recorded separately: the [5 November agentic audit](product/AGENTIC_AUDIT_2026-11-05.md)
recorded gaps in the earlier goal-to-plan-to-execution journey. The subsequent
[agent runtime](modules/AGENT_RUNTIME.md) connects that journey; its
[implementation record](product/AGENTIC_IMPLEMENTATION_WORK.md) distinguishes
verified local behavior from provider-live and public-deployment acceptance.

## 1. Sovereign AI & Agentic Core

### Ava
The sovereign AI persona and unified multi-modal interface of Archava. Ava acts as an autonomous digital concierge and enterprise operator. Ava is not a standalone binary or monolithic crate, but an emergent capability composed of:
- **Sensory Perception**: Presentation modalities (`chat_only`, `voice_only`, `avatar_voice`, `full`, `avatar_chat`), live media streaming via LiveKit, and the universal embed SDK ([`static/archava.js`](../static/archava.js)).
- **Authority & Deliberation**: Deterministic L1–L5 autonomy ladder, intent parsing, structured JSON schema enforcement, and assistant flow ([`crates/assistant`](../crates/assistant), [`crates/host/src/assistant`](../crates/host/src/assistant), [`crates/host/src/agent`](../crates/host/src/agent)).
- **Sovereign Memory**: Bi-temporal knowledge graph and progressive disclosure via [Orbz](#orbz) ([`crates/orbz`](../crates/orbz)).
- **Autonomous Execution**: Flight DAG state machine and durable task completion via [Cockpit](#cockpit) ([`crates/cockpit`](../crates/cockpit)).

*(Historical note: "Laya" was the early research codename for Ava's cognitive architecture).*

### Orbz
The sovereign agentic memory engine of Archava ([`crates/orbz`](../crates/orbz)), backed by the `archava_orbz` PostgreSQL schema. Implements OpenViking, Graphiti, and CoALA paradigms:
- **Semantic Memory Nodes**: Knowledge nodes addressable via `orbz://` virtual URIs.
- **Progressive Disclosure (L0 / L1 / L2)**:
  - **L0 (Summary)**: Ultra-compact intent digest (~50 tokens) for prompt budget conservation.
  - **L1 (Overview)**: Working context knowledge map (~500 tokens).
  - **L2 (Detail)**: Full raw JSON payload loaded on demand.
- **Bi-Temporal Validity**: Tracks transaction time (`tx_timestamp`) versus real-world truth (`valid_from` to `valid_to`), enabling fact invalidation and historical knowledge travel without mutating past audit trails.
- **Episodic Memory**: Append-only log of customer and operator interaction turns.

### Cockpit
The autonomous execution and durable flight engine of Archava ([`crates/cockpit`](../crates/cockpit)), backed by the `archava_cockpit` PostgreSQL schema:
- **Flight Plan DAG**: Directed Acyclic Graph of execution steps with explicit dependency management.
- **Aviation State Machine**: Rigid mission progression (`PreFlight` → `InFlight` → `Holding` → `Touchdown` / `Aborted`).
- **L4 / L5 Supervisor Gate**: Autonomous missions reaching high authority tiers (L4 autonomous execution or L5 sovereign authority) require explicit cryptographic human sign-off before committing financial or high-risk mutations.
- **Temporal Checkpoints**: Resumable state markers saved at every step boundary to guarantee recovery across server restarts.

---

## 2. Architecture & Monolith Foundations

### Modular Monolith
The architectural design of Archava: a single root Cargo workspace containing 21 focused crates. Rather than adopting distributed microservices that incur network latency and serialization overhead, modules communicate via in-memory Rust function calls while strictly enforcing compilation boundaries:
- `core`: Portable contracts, identity, and shared types (zero internal production dependencies).
- `domain`: Pure business rules and state machines (depends only on `core`, compiles to `wasm32`).
- `adapters`: Persistence implementations under PostgreSQL atomic transactions (depends on `core` and `domain`).
- `host`: Axum HTTP server, session resolution, route trees, and authority gates.
- `slice`: Server-side rendering (Leptos SSR), static assets, and WebAssembly browser hydration payload.

### Ledger
An isolated database schema namespace in PostgreSQL (e.g. `archava_commerce`, `archava_orbz`, `archava_crm`, `archava_billing`) representing an independent domain ledger. Each ledger owns its tables, sequential migrations, and runtime privilege grants.

### Mounted vs. Unmounted Ledgers
- **Mounted Ledgers**: 87 SQL migrations across 25 ledger directories verified by [`migrations/manifest.v1.json`](../migrations/manifest.v1.json) and applied to the active production database. The server binary embeds the 84 it verifies at startup (every ledger except `archava_provider` and `archava_control`), generated from that manifest at build time.
- **Unmounted Ledgers**: 7 SQL files across 6 reserved directories kept in the repository as authoritative future architectural designs. Strictly allowlisted in [`tools/check-structure.mjs`](../tools/check-structure.mjs) to prevent stray SQL drift.

### FORCE RLS (Row-Level Security)
PostgreSQL row-level isolation enforced via `ALTER TABLE ... FORCE ROW LEVEL SECURITY`. Every multi-tenant table carries a `tenant_id` column. The runtime role (`archava_runtime`) is a non-owner role that sets transaction-local tenant context (`SET LOCAL archava.current_tenant = ...`), enforcing tenant row isolation when the runtime role and transaction context are correct. Per-user membership, scope and operation permissions must still be checked by the host; RLS alone does not establish complete authorization.

### Privilege Separation (Owner vs. Runtime)
- **`archava_owner`**: DDL role used exclusively by operator CLI tooling ([`archava-ops`](../crates/ops)) to execute migrations and schema updates.
- **`archava_runtime`**: Least-privilege non-owner role used by the web serving host. Lacks DDL privileges; the server binary actively refuses to boot if configured with superuser or owner credentials.

---

## 3. Core Enterprise Pillars (Tier 1)

Archava formalizes 7 foundational Tier 1 operational surfaces:

1. **CRM**: Customer relationship management, contact lifecycles, activity feeds, sales pipeline stages, and follow-up review ([`crates/domain/src/erp/crm.rs`](../crates/domain/src/erp/crm.rs), [`crates/adapters/src/erp/crm.rs`](../crates/adapters/src/erp/crm.rs)).
2. **Commerce**: Product catalog, order lifecycles, inventory stock tracking, and landed cost layers ([`crates/adapters/src/erp/commerce.rs`](../crates/adapters/src/erp/commerce.rs)).
3. **Purchasing**: Supplier management, purchase orders (POs), and warehouse receipt vouchers ([`crates/adapters/src/erp/purchasing.rs`](../crates/adapters/src/erp/purchasing.rs)).
4. **Billing**: Invoices, payment gateway settlements, webhook processing, and email outbox dispatch ([`crates/adapters/src/erp/billing.rs`](../crates/adapters/src/erp/billing.rs)).
5. **Support Cases**: Customer support tickets, internal investigation notes, and dispute resolution ([`crates/adapters/src/erp/support.rs`](../crates/adapters/src/erp/support.rs)).
6. **Cockpit**: Flight plans, autonomous mission management, and supervisor approval gates ([`crates/cockpit`](../crates/cockpit)).
7. **Ops**: Operator CLI for cryptographic release verification, database backup/restore, and owner bootstrap ([`crates/ops`](../crates/ops)).

---

## 4. Reliability & Transaction Invariants

### Idempotency Key
A unique client-provided UUID attached to every mutating command (`requestId`). Handled by database-level command deduplication to guarantee that network timeouts, retries, or browser refreshes never execute duplicate business mutations.

### Durable Receipt
The cryptographic receipt emitted by the server upon successfully committing an atomic transaction. Contains the committed revision, resource identifier, idempotency key, and status. The frontend never assumes a mutation succeeded until it verifies the durable receipt.

### Single-Writer Lease (Write Fencing)
A distributed database lease managed in `archava_control` that guarantees only one server instance acts as the authoritative primary writer for scheduled jobs and background orchestrations at any time, preventing split-brain execution in multi-instance clusters.
