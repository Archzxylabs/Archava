# Archava repository re-audit — 10 October 2026

**Source baseline:** `main`, `80ea845`. **Decision:** audit and reconcile the
architecture/status now, then continue bounded milestones. Preserve the working
modular monolith. No wholesale rewrite, folder flattening or new ERP framework is
justified by this review. ARCHZXY's company website remains outside this repository.

## Audit method and limits

Inventory covers all 682 tracked files at the baseline, including 400
TypeScript/JavaScript/Python source/test/tool files. All 32 workspace manifests,
their declared dependency graph, packaging paths, folder roles, PRD §§40–49,
operating rules, existing audits and verification entry points were reviewed.
The architecture review follows representative end-to-end paths through assistant
policy, trusted owner identity, CRM services/repository, native approval/recovery,
provider admission, backups and the standalone realtime worker.

This is a repository/architecture and local verification audit, not a claim to
have manually reviewed every source line or performed a production penetration
test. Test count is not code coverage or product completion. The credential scan
checks several high-confidence patterns in tracked text only; it is not a Git
history/secret-manager audit. Original private credentials and business records
are not exported into the report. Tests use isolated synthetic databases and
accounts. No live model, email, booking, payment or public deployment was invoked.

The [machine-readable evidence](audit/2026-10-10-project-reaudit.json) records fresh
results and the package graph. Historical provider evidence remains explicitly
historical. [Current architecture](ARCHITECTURE.md) is the implementation map.

## A. Current state

The reusable foundation and first business module now exist beyond workspace
folders. The selected library is compiled into 24 private artifacts, consumed by
independently installed examples and the persistent local owner host. CRM covers
contacts, activities, follow-ups, opportunities/stage history, worklists and
authorized overview totals. Dashboard/API access does not require an LLM.

Local owner identity, tenant/resource authorization, two approved follow-up actions,
atomic audit/receipts, read-only recovery and backup restoration are exercised.
Gemini remains an optional synthetic-only planner; it does not receive private
CRM data or arbitrary owner prompts. Voice/avatar code exists in the standalone
template, using a separate Python intelligence and knowledge path.

| PRD milestone                | Audited status                                                                                             | Limit                                                                                              |
| ---------------------------- | ---------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| M0 specification alignment   | Company/product/library boundaries established; current implementation documents reconciled in this audit  | A specification is not runtime delivery                                                            |
| M1 independent core reuse    | Passed locally: hospitality and retail consume identical compiled implementations                          | Fictional brains/knowledge; no deployed customer host inferred                                     |
| M2 persistent business reuse | Passed locally: hospitality and professional services share CRM/API/dashboard/conversation implementations | Two bounded inquiry contexts, not every industry or a complete ERP                                 |
| M3 controlled operations     | Supported local owner acceptance complete; broader M3 remains partial                                      | Shared presence, external provider actions and public production operation are separate open gates |
| M4 demand-backed expansion   | Small pipeline extension verified; general module/profile/kit expansion remains open                       | Candidate industry/module catalog does not count as implemented scope                              |

The earlier approximately 30% score is a historical v1 commercial score, not a
fresh v2 total. The approximately 60% owner figure remains a separately scoped
planning estimate, not 60% of all Archava. This audit reports milestone acceptance
instead of silently changing either denominator or inferring progress from tests.
An overall numeric target needs an explicit finite release scope and weights;
"all future CRM/ERP industries" cannot serve as a completion denominator.

## B. Target state

One reusable assistant intelligence/action authority serves independently
installable chat, voice and human-presence consumers. Deterministic business
modules remain usable without the assistant. Each consumer supplies branding,
supported configuration, knowledge and verified identity; domain rules and
permissions are shared implementations. Existing client systems can remain
authoritative through explicit adapters.

Keep one supported local release combination first. Add production hosting,
providers and business modules through separate acceptance. Maximizing reuse for
the current small team does not require a universal plugin engine or every ERP
capability before the first supported release.

## C. Gap analysis and architectural conflicts

| ID  | Finding and source evidence                                                                                                                                                           | Impact / priority                                                                                       | Disposition                                                                                                                 |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| F1  | Architecture/status prose retained outdated package counts, absent-CRM/identity/action claims and a label calling the old 30% estimate current                                        | High for planning: can direct work toward already delivered capabilities or inflate release claims      | Current architecture rewritten; current status summary and historical-score labels updated                                  |
| F2  | `apps/web/src/frontend/data.ts` and `workspace.ts` maintain fictional browser-storage business UI, separate from `apps/crm` and `packages/crm`                                        | Medium: two screens can be mistaken for one authoritative CRM                                           | Preserve and label the web surface as a reference/demo; use shared CRM components for any future real workspace             |
| F3  | `Archava Template/backend/agent.py` owns its own model session, product facts and tools; it does not use `runTurn` or CRM service authority                                           | High before unified presence: shared source is not shared intelligence                                  | Design a typed server boundary; retain standalone installation and existing session/page protocols                          |
| F4  | `moduleManifestSchema` has dependency IDs but not the complete PRD version/conflict/environment/health contract; packaging proves exact artifacts, not a supported upgrade matrix     | High before broader consumer releases                                                                   | Define and validate one supported local profile before adding version-range or plugin complexity                            |
| F5  | Template npm audit reports 13 moderate package entries from the OpenTelemetry/Spatius chain; Python transitive dependency lock and local SDK-test venv are absent                     | Gate before live template release; does not establish a reachable exploit in the owner host             | Track compatible vendor upgrade/triage; do not force an automatic SDK downgrade or claim full supply-chain assurance        |
| F6  | CI runs platform, CRM/runtime and template offline checks, but not authenticated Chromium or Python SDK integration tests; root verify alone excludes real PostgreSQL/template checks | Medium: green root checks have a narrower denominator than whole-repository verification                | Keep explicit separate acceptance commands; add reproducible browser/SDK CI when preparing their supported release          |
| F7  | Backup scheduling lives in the owner host process; local HTTP host was not listening at audit start, while `crm:doctor` reported the installation ready                               | Operations remain bounded: stopped hosts do not keep scheduling snapshots                               | Document the lifecycle; unattended process supervision, off-machine backups and alerts are a separate requirement           |
| F8  | `crm-act/src/native-host.ts` constructs an internal hospitality/commerce context; some orchestration/UI/verifier files are large                                                      | Low current debt; synthetic metadata should not become future consumer configuration or price authority | Use neutral/trusted host context when extending supported profiles; split large files only when behavior changes justify it |
| F9  | `pricing` and `reference` are not in the 24-artifact closure; `tools/configurator` exports code consumed by `apps/web`                                                                | Distribution scope is narrower than all folders; tool/library naming can confuse consumers              | Preserve working placement; package quote primitives separately when an external consumer actually needs them               |

No declared workspace dependency cycle or structure-gate import escape was found.
No production security clearance is inferred from those checks. No framework or
fundamental folder replacement was demonstrated necessary.

## D. Proposed architecture and folder disposition

Keep current paths and ownership. `packages/` holds shared implementation;
`apps/` hosts/composes it; `tools/` builds/evaluates/verifies it; `examples/` proves
independent consumption. The standalone template remains a presence consumer.
`docs/` holds current guides plus dated evidence; `delegation/` is historical.

| Folder / material                                         | Action                                                                   | Reason                                                                                                                   |
| --------------------------------------------------------- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------ |
| Existing assistant/Act/business packages                  | Keep                                                                     | Separate contracts, domain ownership, persistence and providers are useful boundaries, not accidental duplicate products |
| `apps/crm`                                                | Keep as the supported local owner host                                   | Already consumes shared compiled business/assistant modules                                                              |
| `apps/web`                                                | Keep as reference tooling; do not expand it into ARCHZXY's company app   | Preserves existing demos/tests without creating a second authoritative CRM                                               |
| `Archava Template`                                        | Keep standalone; connect through a future installed-package/API contract | Parent imports would violate its installation/export contract                                                            |
| `examples`                                                | Keep                                                                     | Two-consumer evidence is a product milestone, not redundant client copies                                                |
| `delegation` and dated audit files                        | Retain as history; current guides take precedence                        | Old Pending/branch instructions are not the active roadmap                                                               |
| Empty ignored `clients/generic-business`                  | Optional later housekeeping; no functional blocker                       | Contains no consumer source                                                                                              |
| `dist`, `node_modules`, cache folders                     | Regenerable output, leave during this audit                              | Their existence is normal and not evidence of unmerged source                                                            |
| `.local`, database files, credentials and backup archives | Preserve                                                                 | Private operational state; deleting them is not source cleanup                                                           |

For shared presence, the Python consumer should send only validated session/context
and bounded requests to server-owned intelligence/domain APIs. The server binds
tenant/actor identity and tools. Copying owner credentials into a guest worker or
trusting model/guest-provided permissions would break the architecture.

## E. Module contract design

Retain the thin build/startup-owned registry and separate requested/supported/
configured/connected/healthy integration states. Native CRM manifests declare
their actual tools and persistence adapter; legacy template names such as `crm`
remain planning vocabulary and are not aliases granting native CRM access.

The next small contract should record one supported profile ID, exact package
versions/hashes, public contract/config versions, required adapters, owned schema
versions and operator checks. Build/install/upgrade validation should reject an
unsupported combination. Use only trusted implementations already installed by
the host. General semver dependency ranges, conflicts, events/UI contracts and
optional module activation should grow from concrete second-module requirements.

## F. Tenancy strategy

Current CRM uses shared PostgreSQL tables with forced tenant RLS and per-operation
membership/resource scope checks. Transaction-local tenant context prevents pool
reuse from inheriting a previous tenant. Runtime roles cannot own/bypass the
protected tables. Staff assignment and service/human grant intersections remain
enforced. Local OS/operator access is a trusted administration boundary, not a
hostile shared multi-user deployment model.

Production provisioning, secure public identity/enrollment, other-surface tenant
binding and dedicated-client deployment remain separate acceptance. Voice guests
must receive public tools, never the owner's CRM principal or tool inventory.

## G. Configuration strategy

Preserve existing Zod schemas, explicit host settings and browser-safe exports.
V2 planning does not implicitly activate template suggestions. V1
`resolveLegacy()` is compatibility, not provisioning authority. Configuration,
commercial entitlement and runtime permission remain separate decisions.

Align the supported local profile with host tenancy, the actual CRM manifests and
owned migrations. Never accept adapter registration, secret values or arbitrary
module code from browser/model config. Keep provider keys in private runtime state.

## H. Versioning strategy

Current cold installs bind exact implementation versions and hashes. CRM/domain/
assistant are 0.7.0, the native CRM action adapter 0.2.0 and owner host 0.2.0.
Checksummed database histories protect schema identity. This is evidence for the
tested release combination only, not hundreds of interchangeable versions.

Start with a release profile/changelog and one previous-profile upgrade fixture.
Document compatible schemas and unsupported older binaries. Do not introduce an
arbitrary cross-client version solver before there is a real supported matrix.

## I. Migration strategy

Keep existing code behavior and migrations. New profile work should initially
describe/validate the current combination. Then prove upgrading an existing
installation preserves records, grants, audit and original receipts. Test recovery
into a new database, invalidate restored sessions, keep provider/native holds and
require a fresh approval epoch. Binary rollback alone is not schema rollback.

Move or retire demo UI only when a replacement consumer passes the same behavior
checks. Introduce presence behind an adapter; do not replace the working template
or copy its runtime secrets/knowledge into the core.

## J. Security model

Preserve server-side identity/CSRF, least privilege, typed tool allowlists, tenant
RLS, current grants, revision checks, atomic audit and secret-free diagnostics.
Untrusted prompts and browser state cannot register adapters or grant authority.
Existing local tests verify refusal and race/restart/reassignment paths; deployment
load/adversarial testing, alerting, policy retention and production identity remain
open. No claim of comprehensive security certification is made.

Fresh pnpm audit reports zero advisory entries. Template npm audit reports thirteen
moderate package entries and no high/critical; they are not thirteen independent
exploits. The root issue is the [OpenTelemetry baggage advisory](https://github.com/advisories/GHSA-8988-4f7v-96qf).
Its upstream description depends on transport/header limits; the audit did not
establish exploit reachability in this app. Resolve/triage it against a compatible
SDK before live template delivery. No package or lockfile was changed here.

## K. Tool/action permission model

Read/prepare tools remain separate from execution. Two native follow-up actions
require independent exact-action review/consent. A consumed challenge, server MAC,
current epoch, durable claim, current service/human grants and matching resource
revision are verified before the domain transaction commits. Completion cannot
reuse creation consent. Recovery uses an authoritative original UUID receipt and
does not treat missing evidence as failure or permission to repeat.

Booking/email adapters exist, but real provider writes are unverified and are not
configured in the owner installation. Payment authority is not implemented. Every
new action needs domain ownership and its own execution/recovery acceptance.

## L. Pricing engine architecture

Retain `packages/pricing`, `tools/configurator` and the authoritative V1 pricebook.
The engine is deterministic; module readiness is not price authority. New native
modules absent from the active pricebook cannot acquire invented prices. Current
quote tooling is stateless and does not implement a contract/payment workflow.
The company website may consume these primitives outside this repository.

## M. CRM/project pipeline model

The implemented pipeline links opportunities to contacts and owns guarded stages
and immutable transition history. Config labels support two inquiry contexts
without changing the domain's rules. Won is not a signed contract, paid deposit or
delivered project. Project execution/capacity/billing states are not implemented
by this CRM slice. Generalize them only from a scoped module requirement.

## N. Delivery queue model

ARCHZXY's delivery capacity/queue belongs to its separate consumer application.
Archava may expose that application's authorized tools later. Current follow-up
worklists are CRM tasks, not project capacity forecasts. Do not introduce Redis,
BullMQ or Temporal merely because the long-term company flow includes a queue.

## O. Proposal → contract → payment

Quote primitives and opportunity stages exist; signature, agreement amendments,
deposit checkout, provider verification and the full project lifecycle do not.
Those company workflows stay in the separate app, using reusable modules/adapters
where justified. Payment success must come from an authoritative backend/provider,
never an LLM statement or a CRM stage transition.

## P. Observability strategy

Current turn events, sanitized request IDs, provider budgets/outcomes, health,
native audit, backup integrity and CLI diagnostics provide local evidence. There
is no selected production telemetry/alert sink or unattended incident workflow.
Choose one small error/availability path when an always-on deployment is required.
Runbooks, process supervision, off-machine backups and recovery targets need that
explicit operational scope. Stop/start behavior is already part of local acceptance.

## Q. Test strategy and fresh verification

Run root, real-database, authenticated browser and standalone checks separately.
Existing passing suites must remain intact. Two-consumer tests must cold-install
compiled artifacts and compare implementations, rather than inherit workspace
source aliases. PostgreSQL tests must keep actual races/restart/current grants;
unknown-outcome tests must prove no repeat execution.

Fresh results are recorded in the linked machine evidence. Root gates passed:
1,442 unit tests in 99 files, structure/type/lint/build and independent core reuse.
CRM passed 38 real-database checks; durable Act composition passed 18. Authenticated
runtime passed 40 checks, including actual backup/restore, followed by 70 Chromium
owner/staff checks. The runtime fixture was held for the browser and stopped
afterwards; its temporary database was removed. Template
passed 55 JavaScript tests, four offline Python guardrails and its production
build; large SDK/WASM chunks remain a performance warning. Seven Python SDK
integration tests were not rerun because the template's project venv is absent.
No cloud/provider result is inferred from offline checks.

## R. Implementation roadmap

| Bucket            | Next work                                                                                                                    | Acceptance / stop condition                                                                                                                                  |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| MUST BUILD NOW    | Reconcile current architecture/status; define and enforce one supported local profile with an upgrade/recovery contract      | Existing consumers/behavior remain green; profile rejects unsupported combinations and documents its schema/config/artifact ownership                        |
| SHOULD BUILD NEXT | A typed shared intelligence/presence boundary and one authoritative integration selected from real demand                    | Independent consumer uses the same server-owned truth/policy; public/owner tools remain separate; synthetic transport proof precedes any live-provider claim |
| BUILD LATER       | Always-on operation, broader AI with approved data policy, demand-backed business modules, production profiles/provisioning  | Concrete user/domain requirement, support owner and separate provider/security/recovery acceptance                                                           |
| DO NOT BUILD YET  | Company website in this repo, universal ERP, arbitrary plugins/version combinations, stack rewrite, microservices/Kubernetes | Revisit only if a simpler supported composition demonstrably fails a concrete requirement                                                                    |

The documentation reconciliation is delivered by this audit. Profile/presence work
is proposed next implementation, not silently marked built. No business feature,
database schema, dependency, pricebook or private runtime policy was changed.
