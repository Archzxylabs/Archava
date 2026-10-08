# Reusable platform research — ARCHZXY / Archava

**Reviewed:** 7 October 2026. **Repository baseline:** `main`, `63b221b`.
**Purpose:** inform [PRD v2](../PRD.md), preserving the existing assistant/action boundaries and commercial pricebook.

**8 October follow-up:** [Business-model research](BUSINESS_MODEL_RESEARCH_2026-10-08.md)
adds an operating-pattern/industry capability map against checkpoint `1fad26a`.
The earlier findings and repository snapshot below remain dated historical evidence.

## Decision

Build an internal ARCHZXY module library incrementally. Archava remains the flagship AI interface product. The library can contain deterministic CRM/ERP business modules as well as assistant, presence, SDK and integration modules. Business modules must also work through dashboards/APIs without an LLM. The ARCHZXY company website remains a separate consumer project.

Start with existing core contracts and independent consumption, then one small CRM slice demonstrated in two business contexts. Expand ERP capabilities from repeated demand. Maintain adapters for established client systems. This is our architectural recommendation from the evidence below, not a vendor certification or evidence that the proposed modules already exist.

## Method and limits

The review covers 34 primary documentation, specification and license sources across modular business systems, domain boundaries, distribution, tenancy, action security, workflow recovery, observability and supply chain. Documentation shows supported designs and contracts; vendor claims do not prove their performance or suitability for our workload. No competing framework was installed, benchmarked, deployed or called with real customer data.

The scope is broad but bounded by product decisions. It does not claim to cover the entire internet, validate every industry workflow, establish legal compliance, or refresh every provider price. Existing pricing remains authoritative. Older market anchors in PRD §36 are historical context and require separate commercial validation.

Some rendered documentation URLs timed out or moved. Odoo's manifest documentation was verified through its official documentation repository; Twenty's current data-model and app pages were located through its official documentation index. Failed fetches are not evidence of missing capabilities. Branch-based license sources are dated observations; adoption requires checking the exact release, package and enterprise exclusions.

The prior PRD is preserved at [PRD.v1.0.0.md](history/PRD.v1.0.0.md). This research informs scope and future implementation; it does not change runtime behavior, package versions or signed commitments.

## What the repository establishes

| Area             | Evidence                                                                                                                                                    | Implication for v2                                                                                                                                           |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Core contracts   | [assistant exports](../packages/assistant/src/index.ts), [policy](../packages/acl/src/policy.ts), [SDK](../packages/sdk/src/index.ts)                       | Extend existing ports, gated execution and context contracts. Preserve the working reference behavior.                                                       |
| Distribution     | [SDK manifest](../packages/sdk/package.json), [core manifest](../packages/core/package.json)                                                                | Private workspace packages export TypeScript source. Independent installation/build and browser/server separation are not established by workspace tests.    |
| Configuration    | [schema](../packages/config/src/client/schema.ts), [resolver](../packages/config/src/client/resolver.ts)                                                    | Module names/toggles do not resolve dependency/version graphs. Template integrations can be merged as enabled before an implementation or connection exists. |
| Business records | [frontend demo](../apps/web/src/frontend/data.ts), [frontend status](FRONTEND.md)                                                                           | CRM/ERP screens use browser-local fictional data. They are reference UI, not a reusable authoritative CRM/ERP backend.                                       |
| Act recovery     | [attempt ledger](../packages/act-host/src/attempts.ts), [host](../packages/act-host/src/host.ts), [Postmark status](../packages/act-postmark/src/status.ts) | Host ledger creation is not atomic/durable; provider status remains unknown. These must be resolved before corresponding live writes.                        |
| Voice/avatar     | [portable starter](<../Archava Template/README.md>), [validation](<../Archava Template/docs/VALIDATION.md>)                                                 | A standalone surface exists but is not joined to shared intelligence. Its dependency advisory and live-validation gaps remain.                               |

Existing verification from this session's source consolidation: six `pnpm verify` gates, 1,295 tests in 80 files, 31 reference-browser checks and 16 frontend checks passed. Template verification passed 55 JavaScript tests, four offline Python guardrail tests and build. Seven Python SDK tests were not rerun during consolidation. Disposable PostgreSQL checks covered attempt invariants/races and 27 confirmation checks. These are historical offline/local verification results, not new tests run for this research and not production evidence.

## Findings and their consequences

### 1. Reuse needs ownership and contracts

Odoo declares dependencies and installation resources in module manifests. Frappe exposes extension hooks and a migration process. Medusa explicitly isolates module resources and connects models through module links. These are concrete examples of reuse beyond folders. [Odoo manifest documentation](https://raw.githubusercontent.com/odoo/documentation/19.0/content/developer/reference/backend/module.rst), [Frappe hooks](https://docs.frappe.io/framework/user/en/python-api/hooks), [Frappe migration](https://docs.frappe.io/framework/user/en/bench/reference/migrate), [Medusa isolation](https://docs.medusajs.com/learn/fundamentals/modules/isolation), [Medusa links](https://docs.medusajs.com/learn/fundamentals/module-links).

**Our decision:** business modules own their records, validation and state transitions. Other modules consume public services/events. Begin with a build-time registry and thin manifest, then add persistence/UI metadata when the first module needs them. Do not copy a framework's entire plugin, dependency-injection or metadata engine.

### 2. Cross-industry reuse has a domain boundary

Twenty documents extensible CRM objects/fields and app packages containing business logic and UI. Fowler describes explicit boundaries between domain models whose vocabulary differs. ERPNext's ledger documentation shows why operational records also carry posting/reversal invariants. [Twenty data model](https://docs.twenty.com/getting-started/core-concepts/data-model), [Twenty apps](https://docs.twenty.com/getting-started/core-concepts/apps), [Bounded Context](https://martinfowler.com/bliki/BoundedContext.html), [ERPNext immutable ledger](https://docs.frappe.io/erpnext/immutable-ledger-in-erpnext).

**Our decision:** reuse contacts, activities, supported pipeline configuration, billing primitives and relevant operational modules. Put material domain differences into vertical extensions. Room availability is time/resource scheduling; product stock is quantity movement; an invoice is not a full accounting ledger. Patients and students do not automatically inherit sales-lead semantics or contact visibility. Shared identity does not imply shared access to domain-sensitive records.

### 3. Build our differentiation; integrate mature breadth

| Approach                                  | Where it fits                                                  | Main cost or uncertainty                                                            | Position                                                                 |
| ----------------------------------------- | -------------------------------------------------------------- | ----------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| Existing client CRM/ERP via adapter       | Client already has authoritative systems                       | API scopes, mapping, sync/reconciliation and provider changes                       | Supported path; preserve one source of truth per resource.               |
| Small first-party CRM/operational modules | Repeated needs that benefit from shared code and Archava tools | Data lifecycle, auth, migrations, UI and support become our responsibility          | Initial library direction, one bounded slice at a time.                  |
| Frappe/ERPNext or Odoo backend            | Requirements need broad mature ERP capabilities                | Additional runtime/operations, upgrades, edition/license constraints and domain fit | Evaluate for a concrete client; no wholesale platform migration.         |
| Twenty as CRM backend/app host            | Extensive CRM customization is needed                          | Role mapping, tenancy/deployment, app/runtime coupling and license boundaries       | Candidate for a focused adapter/extension spike.                         |
| Medusa for commerce capability            | Commerce is the client's actual requirement                    | Commerce-specific semantics and integration work                                    | Candidate adapter/backend; architecture reference for module boundaries. |
| New universal CRM/ERP framework           | Every future business requirement                              | Very large compatibility and operational burden for two people                      | Deferred.                                                                |

This comparison is our fit assessment, not a measured benchmark. Before adopting a framework, use a fixed scenario to test auth/tenant enforcement, data ownership, deployment cost, exports, migration and failure recovery. The source evidence supports extension mechanisms, not a conclusion that one framework is the best fit for Archava.

### 4. Same library version is simpler than arbitrary combinations

Semantic Versioning requires a declared public API; version numbers alone do not demonstrate compatibility. Node supports explicit package exports. pnpm 10 can pack artifacts, although options added after our pinned pnpm version must not be assumed available. [SemVer](https://semver.org/), [Node 22.18 package contracts](https://nodejs.org/download/release/v22.18.0/docs/api/packages.html), [pnpm 10 pack](https://pnpm.io/10.x/cli/pack).

**Our decision:** use a small set of tested release profiles with exact resolved module versions. Test packed artifacts in a consumer outside the monorepo, with no source aliases or parent imports. Initially support ESM and documented runtimes; add other packaging only for actual consumers. Resolve dependency ranges at build/configuration time, not by downloading tenant-selected code during a request.

Code version and shared-database schema version require a coordinated rollout. Two tenants sharing tables cannot independently install incompatible schema migrations. Dedicated deployments can remain pinned with an explicit support policy.

### 5. Reuse is code sharing; tenants retain isolated data

OWASP describes server-verified tenant context, isolation across caches/storage/async work and connection reuse. PostgreSQL documents that superusers and `BYPASSRLS` roles bypass RLS; table owners normally do too. Azure documents shared/dedicated storage trade-offs and warns against per-tenant table/column sprawl. [OWASP multi-tenant security](https://cheatsheetseries.owasp.org/cheatsheets/Multi_Tenant_Security_Cheat_Sheet.html), [PostgreSQL RLS](https://www.postgresql.org/docs/current/ddl-rowsecurity.html), [Azure storage/data approaches](https://learn.microsoft.com/en-us/azure/architecture/guide/multitenant/approaches/storage-data).

**Our decision:** shared PostgreSQL tables with tenant-scoped keys and RLS are the proposed default for ordinary hosted deployments, after auth/persistence exists. Runtime credentials must not bypass RLS. Establish tenant scope per transaction; test reused connections. Dedicated databases/deployments remain an isolation option for requirements that justify their operational cost. Bound caches, file paths, retrieval filters, exports, attempts and worker messages to the authorized tenant.

### 6. The business API must authorize every entry point

OWASP identifies tool privilege escalation, injection and approval manipulation. Frappe's API uses user-associated credentials and checks that user's roles. [OWASP AI agent security](https://cheatsheetseries.owasp.org/cheatsheets/AI_Agent_Security_Cheat_Sheet.html), [Frappe REST API](https://docs.frappe.io/framework/user/en/api/rest).

**Our decision:** distinguish initiating human/customer identity from the Archava service actor. Business modules enforce resource access and invariants for dashboard, API, agent and worker callers. Module activation, feature visibility, commercial entitlement and action authorization are separate checks. Retain existing L0–L5 rules and server-derived confirmations. A safe assistant gate does not compensate for an unsafe direct module API.

### 7. Recovery is a business property

Stripe describes duplicate and unordered webhook delivery plus provider-side idempotency behavior. BullMQ requires idempotent jobs; Temporal provides persistent workflow recovery/replay. [Stripe webhooks](https://docs.stripe.com/webhooks), [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests), [BullMQ idempotent jobs](https://docs.bullmq.io/patterns/idempotent-jobs), [Temporal executions](https://docs.temporal.io/workflow-execution).

**Our decision:** persist canonical action identity, payload binding, attempts, approval and outcome. Authenticate notifications, deduplicate durable inboxes and reconcile unknown results. Repeated retries cannot manufacture a fresh intent. Do not promise exactly-once external effects universally. Adopt one queue/workflow tool only when a real resumable flow justifies it; neither a broker nor workflow replay replaces provider-specific effect handling.

### 8. Operability belongs to the module contract

CloudEvents defines event identity and context; it does not itself provide reliable delivery. OpenTelemetry places responsibility for sensitive attributes on the implementer. PostgreSQL describes multiple backup approaches. [CloudEvents](https://github.com/cloudevents/spec/blob/main/cloudevents/spec.md), [OpenTelemetry sensitive data](https://opentelemetry.io/docs/security/handling-sensitive-data/), [PostgreSQL backups](https://www.postgresql.org/docs/current/backup.html).

**Our decision:** use versioned events, correlation and safe tenant metadata without replacing existing chat-event vocabulary. Add durable audit separately from sampled telemetry. Define health/readiness for required integrations. Before hosted persistent modules go live, demonstrate backup restoration and record observed recovery; set contractual RPO/RTO/SLA only after measurement. Select one initial telemetry/error path rather than operating every vendor named in the older PRD.

### 9. Private library ownership has third-party boundaries

Licenses observed at review time: Frappe Framework is MIT, ERPNext GPLv3, Frappe CRM AGPLv3 and Odoo community core LGPLv3. Medusa's root license excludes enterprise materials from its MIT grant. Twenty includes AGPLv3, commercial files, MIT packages and an application exception. The exact package/release matters. [Frappe license](https://github.com/frappe/frappe/blob/develop/LICENSE), [ERPNext license](https://github.com/frappe/erpnext/blob/develop/license.txt), [Frappe CRM license](https://github.com/frappe/crm/blob/develop/LICENSE), [Odoo license](https://github.com/odoo/odoo/blob/19.0/LICENSE), [Medusa license](https://github.com/medusajs/medusa/blob/develop/LICENSE), [Twenty license](https://github.com/twentyhq/twenty/blob/main/LICENSE).

**Our decision:** ARCHZXY owns its first-party library subject to contracts; dependency incorporation does not transfer third-party ownership. Track exact artifact licenses and distribution obligations before an adoption/release. An API boundary is an architectural choice, not a blanket legal exemption. Do not copy vendor internals into our proprietary core as an assumed shortcut.

### 10. Distribution needs provenance without exposing private source

SLSA describes incremental supply-chain guarantees. npm provenance links builds to source/build instructions and explicitly does not prove code is non-malicious. [SLSA 1.2](https://slsa.dev/spec/v1.2/), [npm provenance](https://docs.npmjs.com/generating-provenance-statements/).

**Our decision:** start with private, access-controlled artifacts, controlled dependencies, lockfiles, advisory/license review and recorded build/source hashes. Verify the chosen registry's provenance support and disclosure properties before enabling public attestations. No SLSA level, audit certification or secure distribution is claimed by this documentation change.

## Priority and evidence gates

| Priority                          | Work                                                                                                                                                         | Evidence required                                                                                       |
| --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------- |
| MUST BUILD NOW                    | Public contract inventory, browser/server split, packed install proof, thin module registry, validated config                                                | Two independently built consumers use the same core artifact without copied implementation.             |
| MUST BUILD NOW                    | Design a bounded CRM slice and map shared versus domain-specific rules                                                                                       | Reviewable schema/API/UI/action contract and two business scenarios; no giant ERP schema.               |
| MUST BUILD NOW before live writes | Trusted principals, atomic durable attempt lifecycle, confirmation/recovery, audit                                                                           | Race/restart/forged approval/unknown outcome tests and a controlled external workflow.                  |
| SHOULD BUILD NEXT                 | Contacts + activities/follow-up CRM slice; shared Voice/Human core contract; one actual system adapter                                                       | Dashboard and Archava operate on the same authorized records; two compositions pass the same contracts. |
| BUILD LATER                       | Demand-backed ERP modules, vertical kits, additional providers, release profiles and provisioning                                                            | Repeated requirements plus migration/support capacity; per-module operational evidence.                 |
| DO NOT BUILD YET                  | Company website in this repo, universal ERP, metadata platform, arbitrary tenant code, unconstrained version solver, stack rewrite, microservices/Kubernetes | A concrete future requirement and evidence that the simpler implementation is insufficient.             |

## Open implementation decisions

| Question                        | Default direction                                                                                     | Evidence that would change it                                                                                                               |
| ------------------------------- | ----------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| First two contexts?             | Hospitality inquiry and professional-service inquiry, sharing contacts/activities; fixtures initially | Actual prospect requirements demonstrate a better reusable slice. These are reference consumers, not the company website.                   |
| Registry/distribution?          | Packed private artifacts first; private registry when multiple consumers need routine releases        | Measured release/access burden or client deployment requirements. No package is published by this work.                                     |
| Native CRM or existing backend? | Own a small bounded slice; adapters remain supported                                                  | A spike proves an existing backend reduces total integration, license and operation cost.                                                   |
| Backend framework/ORM?          | Preserve current TypeScript ports and Node host; explicit PostgreSQL migrations                       | Growing real domain/UI requirements justify an ADR and incremental adoption.                                                                |
| Queue/workflow engine?          | Keep execution recovery contracts independent of the tool                                             | A concrete long-running workflow, failure model and operator burden justify one option.                                                     |
| Inventory/billing scope?        | Operational slice only after CRM proof and customer demand                                            | Requirements establish reservation, valuation, tax or accounting obligations; broad statutory accounting remains outside the first release. |

## Source register

All sources were inspected on 7 October 2026. The entries below identify the evidence used above; access dates are not publication dates. Mutable branch/current-documentation URLs are not a pinned adoption bill of materials.

| ID  | Primary source                                                                                                                             | Evidence used                                                             |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------- |
| R01 | [Odoo 19 manifest documentation](https://raw.githubusercontent.com/odoo/documentation/19.0/content/developer/reference/backend/module.rst) | Module dependencies, metadata and install resources.                      |
| R02 | [Odoo community license](https://github.com/odoo/odoo/blob/19.0/LICENSE)                                                                   | LGPLv3 core; bundled license boundaries.                                  |
| R03 | [Frappe hooks](https://docs.frappe.io/framework/user/en/python-api/hooks)                                                                  | Extension hooks; DocType extension availability is version-specific.      |
| R04 | [Frappe migrate](https://docs.frappe.io/framework/user/en/bench/reference/migrate)                                                         | Hooks, patches, schema sync and app-version update.                       |
| R05 | [Frappe REST API](https://docs.frappe.io/framework/user/en/api/rest)                                                                       | User-associated API identity and role checks.                             |
| R06 | [Frappe Framework license](https://github.com/frappe/frappe/blob/develop/LICENSE)                                                          | MIT framework grant.                                                      |
| R07 | [ERPNext license](https://github.com/frappe/erpnext/blob/develop/license.txt)                                                              | GPLv3 product license.                                                    |
| R08 | [Frappe CRM license](https://github.com/frappe/crm/blob/develop/LICENSE)                                                                   | AGPLv3 product license.                                                   |
| R09 | [ERPNext immutable ledger](https://docs.frappe.io/erpnext/immutable-ledger-in-erpnext)                                                     | Posting history and traceable correction/reversal.                        |
| R10 | [Medusa modules](https://docs.medusajs.com/learn/fundamentals/modules)                                                                     | Reusable domain/integration packages with models/services.                |
| R11 | [Medusa isolation](https://docs.medusajs.com/learn/fundamentals/modules/isolation)                                                         | Module-owned resources and workflow composition.                          |
| R12 | [Medusa module links](https://docs.medusajs.com/learn/fundamentals/module-links)                                                           | Cross-module associations without internal-model access.                  |
| R13 | [Medusa license](https://github.com/medusajs/medusa/blob/develop/LICENSE)                                                                  | MIT baseline with enterprise exclusions.                                  |
| R14 | [Twenty data model](https://docs.twenty.com/getting-started/core-concepts/data-model)                                                      | Standard/custom objects, fields, relations.                               |
| R15 | [Twenty apps](https://docs.twenty.com/getting-started/core-concepts/apps)                                                                  | Packaged logic/UI/data extensions and scoped API permissions.             |
| R16 | [Twenty license](https://github.com/twentyhq/twenty/blob/main/LICENSE)                                                                     | AGPL/commercial/MIT distinctions and application exception.               |
| R17 | [Monolith First](https://martinfowler.com/bliki/MonolithFirst.html)                                                                        | Architectural reasoning about discovering boundaries before distribution. |
| R18 | [Bounded Context](https://martinfowler.com/bliki/BoundedContext.html)                                                                      | Explicit domain models and relationships.                                 |
| R19 | [Node 22.18 packages](https://nodejs.org/download/release/v22.18.0/docs/api/packages.html)                                                 | Package exports and public entry points.                                  |
| R20 | [pnpm 10 pack](https://pnpm.io/10.x/cli/pack)                                                                                              | Artifact packaging and option version requirements.                       |
| R21 | [Semantic Versioning](https://semver.org/)                                                                                                 | Public API and compatibility/version rules.                               |
| R22 | [PostgreSQL RLS](https://www.postgresql.org/docs/current/ddl-rowsecurity.html)                                                             | Default-deny policy and privileged-role bypass.                           |
| R23 | [OWASP multi-tenant security](https://cheatsheetseries.owasp.org/cheatsheets/Multi_Tenant_Security_Cheat_Sheet.html)                       | Trusted tenant context and isolation across request/data/worker paths.    |
| R24 | [OWASP AI agent security](https://cheatsheetseries.owasp.org/cheatsheets/AI_Agent_Security_Cheat_Sheet.html)                               | Tool privilege, independent authorization and high-impact controls.       |
| R25 | [Azure storage/data approaches](https://learn.microsoft.com/en-us/azure/architecture/guide/multitenant/approaches/storage-data)            | Shared/dedicated storage trade-offs and customization antipatterns.       |
| R26 | [BullMQ idempotent jobs](https://docs.bullmq.io/patterns/idempotent-jobs)                                                                  | Application-owned retry/effect correctness.                               |
| R27 | [Temporal workflow execution](https://docs.temporal.io/workflow-execution)                                                                 | Persistent recovery and replay semantics.                                 |
| R28 | [Stripe webhooks](https://docs.stripe.com/webhooks)                                                                                        | Duplicate/unordered delivery and verification.                            |
| R29 | [Stripe idempotent requests](https://docs.stripe.com/api/idempotent_requests)                                                              | Parameter binding, retention and provider-specific replay behavior.       |
| R30 | [CloudEvents specification](https://github.com/cloudevents/spec/blob/main/cloudevents/spec.md)                                             | Event source/ID/context contract.                                         |
| R31 | [OpenTelemetry sensitive data](https://opentelemetry.io/docs/security/handling-sensitive-data/)                                            | Minimization and instrumentation attribute review.                        |
| R32 | [PostgreSQL backup/restore](https://www.postgresql.org/docs/current/backup.html)                                                           | Backup approaches and recovery planning.                                  |
| R33 | [SLSA 1.2](https://slsa.dev/spec/v1.2/)                                                                                                    | Incremental supply-chain guarantees and provenance.                       |
| R34 | [npm provenance](https://docs.npmjs.com/generating-provenance-statements/)                                                                 | Build/source evidence and its limits.                                     |

## PRD change map

PRD v2 retains numbered §§1–39 so existing source references remain usable. It clarifies company/product ownership, replaces the mandatory stack wishlist with the audited implementation baseline and decision gates, and adds §§40–49 for reusable business modules, composition, tenancy, release/migration, packaging, operations and staged acceptance. The v1 pricebook, tier names, quote math and existing safety contracts remain authoritative in their respective domains.

The earlier [architecture proposal](ARCHAVA_REUSABLE_PLATFORM_PROPOSAL.md) remains supporting audit material. Where future requirements differ, PRD v2 is the product specification; implementation status remains in [BUILD_STATUS](BUILD_STATUS.md). The historical 30% v1 coverage is not a score for v2 or the module library.
