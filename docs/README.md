# Archava Documentation Hub

Comprehensive engineering, architectural, and operational documentation for the Archava Rust modular monolith (22 Cargo workspace crates, Axum host, PostgreSQL `FORCE RLS` multi-tenancy, and Leptos SSR/WASM UI).

Start with the [active product PRD](product/PRD.md) for Archava's agentic AI for
business direction, requirements and measurable outcomes. Requirements and roadmap
targets are separate from implementation and release acceptance.

---

## 🧭 Curated Reading Paths

Navigate directly to what you need based on your role or objective:

### 1. 🚀 New Developer Onboarding (Get Started in 5 Minutes)
1. **[Developer Guide](DEVELOPER_GUIDE.md)**: Toolchain requirements (Rust 1.96, Node 22, wasm-bindgen 0.2.129), fast gate commands (`verify.mjs`), build scripts, and tests.
2. **[Domain Glossary](GLOSSARY.md)**: Master definitions for Ava, Orbz, Cockpit, FORCE RLS, Idempotency, and the 7 Tier 1 Core Pillars.
3. **[Contributing Guide](../CONTRIBUTING.md)**: Workspace rules, commit hygiene, and pull request requirements.

### 2. 🧠 Sovereign AI & Cognitive Architecture
1. **[Architecture Master Overview](architecture/README.md)**: Workspace dependency hierarchy, layer isolation rules, and runtime composition.
2. **[Ava Cognitive Architecture](architecture/LAYA_AGENTIC_SYSTEM.md)**: The 4-pillar agentic architecture (Sensory Modalities, Authority Ladder, Memory, and Execution).
3. **[Orbz Agentic Memory](architecture/ORBZ_MEMORY_ARCHITECTURE.md)**: Bi-temporal knowledge graph, semantic nodes, and progressive context disclosure (L0/L1/L2).
4. **[Cockpit Flight Engine](architecture/COCKPIT_TASK_ARCHITECTURE.md)**: Flight Plan DAG, durable execution state machines, and L4/L5 Human Supervisor Gate.
5. **[Architecture Decisions (ADR Ledger)](architecture/DECISIONS.md)**: Living ledger of architectural decisions (ADR-014 through ADR-020).

### 3. 💼 ERP & CRM Business Modules
1. **[Business Modules Overview](modules/README.md)**: Domain contracts and evidence for the 18 mounted business domains.
2. **[CRM Module](modules/CRM_MODULE.md)**: Contacts, sales pipeline stages, follow-up drafting, and durable receipt recovery.
3. **[Commerce Module](modules/COMMERCE_MODULE.md)**: Catalog items, customer orders, inventory stock, and landed cost layers.
4. **[Billing Module](modules/BILLING_MODULE.md)**: Invoicing, gateway payment attempts, webhooks, and email outbox dispatch.
5. **[Purchasing Module](modules/PURCHASING_MODULE.md)**: Supplier relationships, purchase orders (POs), and warehouse receipt vouchers.

### 4. 🔌 External Integration & UI Design
1. **[Host & Browser Integration](architecture/INTEGRATION.md)**: Session-scoped HTTP API contracts and zero-NPM 3rd-party embedding (`/v1/archava.js`).
2. **[Workspace Appearance](product/design/WORKSPACE_APPEARANCE.md)**: Charcoal & Amber visual design system, token palette, light/dark behavior, and layout evidence.

### 5. 🚢 Production Deployment & Release Gates
1. **[Launch Readiness Plan](product/LAUNCH_READINESS.md)**: Canonical 13-gate release tracker (G01–G13), acceptance criteria, and audit evidence.
2. **[Deployment SOP](../deploy/README.md)**: Production reference deployment, non-owner runtime roles, and hardened Docker/Systemd configurations.

---

## 📁 Repository Documentation Layout

| Directory / File | Description | Status |
| :--- | :--- | :--- |
| [`DEVELOPER_GUIDE.md`](DEVELOPER_GUIDE.md) | Local development setup, CLI commands, and test suites | Living Reference |
| [`GLOSSARY.md`](GLOSSARY.md) | Authoritative domain dictionary and invariant terminology | Living Reference |
| [`architecture/`](architecture/README.md) | Technical system design, crate boundaries, ADR ledger, and integration contracts | Living Reference |
| [`modules/`](modules/README.md) | Business domain contracts and mounted capability specifications | Living Reference |
| [`product/`](product/README.md) | Product PRDs, roadmap, launch readiness, and Charcoal & Amber design system | Living Reference |
| [`archive/`](archive/README.md) | Immutable audit trails, historical research, and pre-Rust records | Frozen Archive |

---

<a id="verification-and-document-ownership"></a>
## 🛡️ Documentation Integrity & Verification

All maintained documentation is governed by automated verification gates:
- Run `node tools/check-docs.mjs` to verify that every internal Markdown link and heading anchor resolves.
- Run `node tools/check-structure.mjs` to ensure module boundaries and migration manifests remain intact.
- Historical evidence in `archive/` is frozen; never edit historical pass/fail records or retroactively rewrite past audit findings.

Current Ava business-agent behavior is defined by the [agent runtime](modules/AGENT_RUNTIME.md).
The [5 November completion record](product/AGENTIC_COMPLETION_2026-11-05.md) separates
functional acceptance, observed performance and external acceptance still outstanding.
The [live Gemini acceptance](product/AGENT_LIVE_ACCEPTANCE_2026-11-05.md) adds six
natural-language browser goals against the actual Google service with a tested
model/reasoning profile, independent SQL effects and the failures found along the way.

The [6 November global ERP comparison](product/ERP_GLOBAL_BENCHMARK_2026-11-06.md)
maps 18 product families to Archava's mounted capabilities and separates local
test success from ERP depth. Its [proposed acceptance protocol](product/ERP_PARITY_ACCEPTANCE_2026-11-06.md)
defines the work and evidence needed before claiming parity or superiority.
