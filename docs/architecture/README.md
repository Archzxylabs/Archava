# Architecture

<a id="current-architecture"></a>

Archava is a modular Rust monolith: the root Cargo workspace contains 22 crates, and the application is assembled into the `archava-server` binary from `crates/slice`. Keeping the current code in these boundaries is more useful than splitting it into additional services for cosmetic reasons. Consult the [domain glossary](../GLOSSARY.md) for canonical definitions of Ava, Orbz, Cockpit, and the 7 Tier 1 pillars.

This document describes the source and entry points. It does not establish launch readiness, regulatory compliance, production latency, or completion of every domain's user workflow. The older aspirational blueprint is retained in [history](../archive/history/platform-2026-10-28/ARCHITECTURE.md).

## Dependency direction

Arrows below mean “depends on”.

```mermaid
flowchart TD
    UI["slice: SSR server + WASM UI"] -->|"native target only"| Host["host: Axum / session / HTTP"]
    UI -->|"native bootstrap"| Adapters["adapters: SQL repositories / transactions"]
    Host --> Adapters
    Adapters --> Domain["domain: business rules"]
    Domain --> Core["core: contracts / tenant types"]
    Adapters --> Core
    Host --> Goal["host::agent: goal / review / continuation"]
    Goal --> Providers["providers: bounded Gemini planner transport"]
    Goal --> Adapters
    Host --> Assistant["assistant / own: planning and configuration"]
    Host --> Agents["cockpit / orbz: task and memory components"]
    Agents --> Core
    Ops["ops: operator CLI"] --> Assembly["assembly: capability planning"]
```

`core` has no internal production dependency. `domain` depends on `core`; `adapters` depends on `core` and `domain`. HTTP and persistence must not leak into the portable business layers. `slice` imports the host and adapters only in its native target, keeping them out of the browser's WASM graph. The PostgreSQL fixture is a development dependency, not part of the production serving graph.

[tools/check-structure.mjs](../../tools/check-structure.mjs) enforces these key dependency rules, native-only UI/host binding, embedded-file paths, and active helper imports. It is a structural guard, not a complete semantic architecture audit.

## Ownership map

| Layer / purpose | Crates |
| --- | --- |
| Shared contracts | `core` |
| Business rules and portable planners | `domain`, `pricing`, `rates`, `assembly`, `capacity`, `assistant`, `own`, `context` |
| Database and provider infrastructure | `adapters`, `providers` |
| Task and memory components | `cockpit`, `orbz` |
| HTTP composition and authority | `host` |
| Product UI and server bootstrap | `slice` |
| Native bindings and operator CLI | `native`, `ops` |
| Protocols, evaluation, campaigns, browser automation | `mcp`, `evals`, `campaigns`, `rpa` |
| Test database lifecycle | `pg-fixture` |

A crate's existence does not mean the persistent host mounts all of its behavior. For example, the host directly composes `cockpit` and `orbz`, while the other protocol/automation crates have their own boundaries. Inspect [the router](../../crates/host/src/router.rs) and [server bootstrap](../../crates/slice/src/bin/archava_server.rs) for the actual mounted surface.

## Runtime and UI

The Leptos server renders the document and the browser hydrates the same view tree. The browser reads session-scoped HTTP APIs; it does not decide authorization. The host resolves session/grants and validates commands before tenant-bound transactions. All ERP and CRM persistence is unified inside `archava-adapters` under atomic `FORCE RLS` transactions (ADR-014, ADR-018). Existing receipt, idempotency, stale-revision, and uncertain-outcome contracts remain at those boundaries.

The current application includes sign-in, Home, Contacts, contact records, follow-up drafting, receipt recovery, Assistant, Settings, and native Operations for catalog/sales, purchasing, invoices, support and read-tool missions. The remaining business ledgers expose native operator forms and scoped collection-read HTTP routes. Home derives a bounded first-page contact overview; it does not fabricate workspace-wide financial totals. See [the appearance guide](../product/design/WORKSPACE_APPEARANCE.md) for composition and dated browser evidence; product completeness is separate from a screen existing.

Home and Desk expose Ava’s goal/review/result journey through `host::agent`. Native business forms and the legacy CRM shortcuts remain mounted; [the agent runtime](../modules/AGENT_RUNTIME.md) defines the current planning and continuation boundary.

The native server embeds migrations, shared styles, and selected assets. `target/site/pkg` holds the separately generated JS/WASM browser package. Changes to embedded assets or SQL require a native rebuild; changes to view markup require rebuilding both native and WASM targets.

## SQL ownership

All 79 checked-in SQL files are under `migrations/`, grouped into 31 ledger directories (72 mounted migrations registered in `migrations/manifest.v1.json` for native installation, and 7 unmounted reserved specs). The owner migration is now `migrations/owner-pg/001_owner_config.sql`. Its migration ID, namespace, bytes, and checksum were preserved when its path changed.

Customer-linked orders add a real schema dependency: the native installer and
fixture apply Identity, CRM, Organizations, then Commerce before the other dependent ledgers.
Historical per-ledger migration identities and bytes are preserved. See
[customer identity](../modules/CUSTOMER_IDENTITY.md).

The persistent bootstrap embeds its mounted files and the test fixture reads the same canonical directories. Migration files are append-only. Folder cleanup must not rename ledger identities, rewrite applied SQL, or imply that every ledger is mounted and every domain has a completed UI.

## Repository lifecycle

- Current code: root Cargo files, `crates/`, `migrations/`, `static/`, `config/`, and `deploy/`.
- Configuration ownership: `config/` holds the shared release/tooling catalogue; the pricing reference JSON lives in `crates/pricing/tests/fixtures/`; the retired template catalogue is in `docs/archive/history/config/`. The pricing engine accepts caller-supplied data rather than loading product prices from the repository.
- Current helpers: `tools/check-structure.mjs`, `tools/check-modules.mjs`, `tools/rust/{verify,build-slice,dev-slice,run-persistent}.mjs`, and `tools/release/sbom.mjs`.
- Removed TypeScript-era tooling and consumers: recoverable from git history, not part of the current gates.
- Current documentation entry points: `docs/README.md` and `docs/DEVELOPER_GUIDE.md`. Technical architecture (including host/embed [integration](INTEGRATION.md)), modules, product planning & design, and archived records have four dedicated directories.
- Local database state/backups: ignored `.local/`. Generated compilation artifacts: ignored `target/`. These are not duplicate product layers and are not deleted as source cleanup.

## Maintenance priorities

Historical large coordinator files have been systematically decomposed into clean modular subtrees: `crates/host/src/router.rs` is decomposed into modular route trees under `crates/host/src/routes/` (ADR-015), `crates/host/src/erp/crm.rs` persistence is extracted into `crates/adapters/src/erp/crm.rs` (ADR-014), and CRM is normalized as a first-class domain of the ERP Monolith (ADR-018). The compile-time tripwire in `crates/host/src/extractors.rs` ensures all route extractions maintain strict extractor guarding.

The server bootstrap and fixture also carry parallel ledger-registration knowledge. Canonical file placement removes one path exception; it does not remove that registration duplication. A future shared typed ledger catalogue should be tested against migration IDs/checksums before replacing either reader.

The operator CLI selects a native `release-profile.json` when present and otherwise supports the legacy `dist/library` profile. The native packager recomputes source/artifact hashes and invokes that verifier; changing an embedded asset, including the JavaScript SDK, invalidates serving-build provenance. Local installation/restore and public deployment acceptance remain separate evidence.

See [the cleanup assessment](../archive/audits/ARCHAVA_REPOSITORY_CLEANUP_2026-10-29.md) for measured changes and the remaining work. The [build status](../archive/audits/BUILD_STATUS_2026-10-29.md) records the checks actually run for this pass.

## Subsystem references

<a id="subsystem-source-references"></a>

These source maps distinguish implemented library behavior, host wiring and remaining proof.

| Document | Purpose |
| --- | --- |
| [Module registry](MODULE_REGISTRY.md) | One descriptor per business module, its consumers, the registration gate and what is not yet covered |
| [Agent runtime](../modules/AGENT_RUNTIME.md) | Durable conversations, reviewed goals, current-authority continuation and receipt reports |
| [Native operations](../modules/NATIVE_OPERATIONS.md) | Installed form contracts, scoped reads, Owner configuration and receipt recovery |
| [Reviewed workflows](../modules/REVIEWED_WORKFLOWS.md) | Atomic imports, real proactive actions and approved mission execution |
| [Live media](../modules/LIVE_MEDIA.md) | Native LiveKit/Gemini transport, setup and owned-fixture acceptance limits |
| [COCKPIT_TASK_ARCHITECTURE.md](COCKPIT_TASK_ARCHITECTURE.md) | Mission state, host persistence, recovery and checkpoint limits |
| [DECISION_INTELLIGENCE.md](DECISION_INTELLIGENCE.md) | Decision policy, confidence floors, provider and host boundaries |
| [DURABLE_ACT_OPERATIONS.md](DURABLE_ACT_OPERATIONS.md) | Command receipts, pending state, recovery and provider composition |
| [ENTERPRISE_ENGINES.md](ENTERPRISE_ENGINES.md) | Ledger, tax, inventory, hardware, telematics and messaging components |
| [FRONTIER_AGENTIC_CRATES.md](FRONTIER_AGENTIC_CRATES.md) | MCP, evaluation, campaigns and RPA implementation boundaries |
| [INTEGRATION.md](INTEGRATION.md) | Host API authentication contracts, CSRF headers, and browser embedding |
| [ORBZ_MEMORY_ARCHITECTURE.md](ORBZ_MEMORY_ARCHITECTURE.md) | Memory tiers, access policy, reflection and persistence limits |

Earlier drafts are preserved byte for byte in [subsystem draft history](../archive/history/subsystem-drafts-2026-10-29/README.md). Current wording must follow source and dated evidence, rather than guarantees inferred from a crate or type name.

Serving persistence uses [fixed writer fencing](WRITE_FENCING.md) across product schemas; [native installation](../../deploy/NATIVE_INSTALL.md) owns operator drain, takeover and restore procedures.

Supplier registration and explicit refresh share the same CRM dependency;
[Purchasing identity](../modules/SUPPLIER_IDENTITY.md) keeps PO/receipt history
frozen while allowing a reviewed supplier master update.

Organizations 002 enforces new CRM affiliation references and preserves legacy
rows as explicitly unverified. [Organization identity](../modules/ORGANIZATION_IDENTITY.md)
describes reviewed creation, legacy verification and the NOT VALID FK boundary.

Organizations 003 adds a source-backed business-party registry and immutable
version history. [Business-party contracts](../modules/BUSINESS_PARTIES.md)
define explicit grants and independent roles. [Party transaction contracts](../modules/PARTY_TRANSACTIONS.md) cover the mounted sales/purchase/invoice references and historical versions.

Organizations 004 adds explicitly reviewed managed-company identities and immutable
version history. [Managed-company contracts](../modules/MANAGED_COMPANIES.md) distinguish this master from transaction/company/book mapping.

[Company trade](../modules/COMPANY_TRADE.md) partitions stock/cost through immutable company-owned product IDs and adds historical company document snapshots, without selecting an accounting engine.

Purchasing006 adds [operational supplier documents](../modules/SUPPLIER_DOCUMENTS.md), version snapshots and accepted-receipt allocations inside the existing atomic Purchasing boundary.

Accounting001 adds [explicit company books and native journals](../modules/ACCOUNTING.md).
Control002 extends writer fencing to those tables. Operational source posting,
AP/AR and bank reconciliation remain separate delivery requirements; installing
the ledger does not create books, opening balances or financial grants for
existing users. [Money units](../modules/MONEY_UNITS.md) records the distinction
between legacy operating values and exact book currency units.

Accounting002 captures reviewed future invoice sources and therefore follows Billing in installation order. Accounting003 binds source journals to their immutable command/audit and checks capture triggers at startup. See [invoice posting](../modules/INVOICE_POSTING.md); AP/COGS and bank reconciliation remain separate work.

Accounting004 adds [future confirmed-PO purchase sources](../modules/PURCHASE_POSTING.md): receipt/GRNI, goods claim/AP and void effects with separate source review/posting. This new integration is under validation; prior native acceptance does not cover it. Supplier payment, bank/COGS and full subledger close remain separate requirements.


Commerce010 adds exact future return-layer/cost-use provenance and conditional
startup checks while preserving ambiguous legacy layers as unbound. Financial
COGS capture/posting remains separate integration work.

Accounting009 adds [source-backed supplier bank payments and returns](../modules/SUPPLIER_PAYMENTS.md),
with live draft reservations, immutable bank bindings and separate posting.
Its local transaction tests are passing; native installation, browser and wider
financial-profile acceptance remain separate validation.
