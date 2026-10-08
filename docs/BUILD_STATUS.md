# Build status

The [7 October project audit](PROJECT_AUDIT_2026-10-07.md) reruns the platform,
template, browser and local PostgreSQL checks and records dependency findings.
It also reproduces the Chat browser-export and config/readiness gaps. Passing
reference tests does not establish independently consumable or live modules.

**8 October cleanup and offline M1:** internal compiled package consumption is
now exercised by two independent hospitality/retail consumers. A thin module
registry and V2 deployment plan separate template suggestions from quoted scope
and tenant-bound host connection evidence. The V1 resolver remains an explicitly
labelled compatibility path. See [library consumption](LIBRARY_CONSUMPTION.md).
Trusted production authentication, durable host attempt recovery, shared realtime
presence remain separate release gaps. The persistent contacts/activity/follow-up
CRM slice is now exercised across two contexts; see [CRM module](CRM_MODULE.md). The company website
is still a separate future consumer.

The consolidated source also includes the [website and CRM/ERP frontend](FRONTEND.md)
and the [portable realtime template](<../Archava Template/README.md>). Frontend
records and approvals are browser-local demos. The template is a standalone npm
project, has provider SDK dependencies, and is not connected to the platform's
shared intelligence core. The workspace dependency and runtime classifications
below refer to the pnpm platform unless explicitly stated otherwise. Neither
addition closes the commercial-readiness gates.

An honest classification of every area of `PRD.md` against this tree. The four
labels mean:

| Label                     | Means                                                                                                                       |
| ------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| **Implemented**           | works today, in this tree, offline, and a gate would fail if it stopped working                                             |
| **Partially Implemented** | the mechanism exists and is exercised, but only through an in-tree stand-in, or only for one tenant, or only on one surface |
| **Planned**               | named in the PRD's intended baseline, and _absent_ from the tree — no code, no dependency, no configuration                 |
| **Not Implemented**       | neither present nor scheduled; stopping at this boundary is a deliberate position                                           |

Where a claim below rests on evidence, the evidence is a file path. Where it
rests on the absence of something, that absence was verified rather than
assumed: no `package.json` in the workspace declares an external service SDK.
The foundation runtime dependencies are `@archava/*` and `zod`. The new
server-only `@archava/crm-pg` adapter additionally uses pinned `pg` and its
type package; browser CRM code imports only the shared dashboard contract.
Other tooling dependencies in the manifests
(eslint, prettier, typescript, typescript-eslint, vitest, tsx, esbuild,
@vitest/coverage-v8, eslint-plugin-react-hooks, @eslint/js) remain
devDependencies. See _External baselines_ below.

## The short version

The assistant slice runs end to end in a browser, offline, for one reference
tenant. The persistent CRM slice separately serves two business contexts with
shared compiled artifacts. The assistant pipeline's decisions are real and fully visible: retrieval,
structured truth, the capability gate, §9 input validation, entity resolution,
and execution each happen in code with a test beside them. Production commercial
workflows remain unverified. There is no payment integration, no PMS integration,
and no production-connected executor for any action that changes a tenant's data.

The PRD's acceptance criterion is `PRD.md` §34: _Act_ can complete one
booking/CRM/email workflow; _Transact_ can complete one full payment →
confirmation → receipt workflow. This tree completes neither. It reaches
`payment.initiate` and stops, for two independent reasons that are worth
stating plainly:

1. `payment.initiate` has no registered input contract, so §9 refuses it —
   `No input contract` — before anything else is consulted.
2. There is no payment `ActionExecutor` in the tree. `pageExecutor`
   (`apps/web/src/surface.ts`) handles `ui.*`; the separate server-side Act
   executors handle only booking and email, not payment.

Production release gates remain open for the pipeline and product.

## Implemented

| Area                                                                        | Evidence                                                                                                                |
| --------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------- |
| Action registry with levels, confirmation modes, L5 never-autonomous        | `packages/acl/src/actions.ts` — 24 baseline actions, `ACTION_LEVELS` at :18, `NEVER_AUTONOMOUS_LEVEL` in `policy.ts:46` |
| Role model with baseline role definitions                                   | `packages/acl/src/roles.ts` — `ROLE_NAMES` at :44                                                                       |
| Capability gate with closed denial-reason vocabulary                        | `packages/acl/src/policy.ts` — `DENIAL_REASONS` at :48, `PolicyResult` at :90                                           |
| Field masking with four strategies                                          | `packages/acl/src/mask.ts` — `MASK_STRATEGIES` at :15                                                                   |
| Turn assembly: mask → plan → answer → gate → validate → execute             | `packages/assistant/src/turn.ts` — `runTurn` at :914                                                                    |
| Context-graph projection that cannot leak a form value                      | `turn.ts:221` `projectContextGraph` — the graph carries field _names_, not field values                                 |
| Declarative §9 input contracts                                              | `packages/assistant/src/validation.ts` — `INPUT_SCHEMAS` at :181, 12 of 24 actions                                      |
| Entity resolution with fail-closed semantics                                | `validation.ts:48` `EntityResolver`; tests in `packages/assistant/test/validation.test.ts`                              |
| Three-field action outcome (`policy` / `execution` / `output`)              | `packages/assistant/src/execution.ts`                                                                                   |
| Zod-parsed context graph and event reducer                                  | `packages/core/src/context-graph.ts`                                                                                    |
| Retrieval store with ranking                                                | `packages/knowledge/src/{store,retrieval}.ts`                                                                           |
| Deterministic quote engine with rounding rules                              | `packages/pricing/src/{engine,rounding}.ts`                                                                             |
| Currency contract with explicit minor-unit exponents                        | `packages/config/src/currency.ts`; `packages/chat/src/money.ts`                                                         |
| Chat DOM renderers including money                                          | `packages/chat/src/{dom,view,money,styles}.ts`                                                                          |
| SDK that builds a page awareness graph                                      | `packages/sdk/src/{page,session,consent}.ts`                                                                            |
| Reference tenant with fixtures, rates, knowledge, brain                     | `packages/reference/src/{tenant,rates,knowledge,brain}.ts`                                                              |
| Runnable web app: chat surface and Studio route                             | `apps/web/src/server/server.ts` — both serve 200; `/` and `/studio` checked                                             |
| One stateless HTTP endpoint                                                 | `POST /api/quote` (`apps/web/src/server/api.ts`)                                                                        |
| Configurator: intake → normalised → variants/recommendation/budget → config | `tools/configurator/src/*.ts`, `pnpm configurator`                                                                      |
| Structural honesty gate                                                     | `tools/scripts/structure.mjs` — app exists, `@archava/*` imports are declared, no filesystem escapes                    |
| Full gate chain, same order as CI                                           | `tools/scripts/verify.mjs`                                                                                              |
| Browser E2E against the served page                                         | `tools/e2e/home.mjs` — 31 assertions, passing                                                                           |
| GitHub Actions CI                                                           | `.github/workflows/ci.yml`                                                                                              |
| Persistent contacts, activities and follow-up module                        | `packages/crm`, `packages/crm-pg`; real database and two independent consumers in `tools/crm-reference`                 |
| Shared CRM dashboard and authorized read/draft tools                        | `packages/crm/src/{dashboard,service,api}.ts`; browser and SQL evidence in `docs/CRM_M2_2026-10-08.md`                  |

## Partially Implemented

The mechanism is real and tested; what is behind it is a stand-in.

| Area                                     | What exists                                                                                                                             | What is missing                                                                                                          |
| ---------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------ |
| Conversation (`BrainProvider`)           | `ScriptedBrain` and `ReferenceBrain` — deterministic, keyword-matched, with an explicit fallback sentence                               | a generative model client. The Jev decision adapter does not implement conversation                                      |
| Retrieval (`KnowledgePort`)              | `buildKnowledgeStore` (`packages/reference/src/knowledge.ts:224`) + `retrieveContext`                                                   | a hosted index, embeddings, incremental ingest. The store is in memory and built at request time                         |
| Live facts (`StructuredTruthPort`)       | `referenceTruth` (`packages/reference/src/rates.ts:421`) — date-stated fixtures                                                         | a PMS or inventory service. Prices and availability are fixtures, not live                                               |
| "Does this id exist?" (`EntityResolver`) | `pageEntityResolver` (`apps/web/src/surface.ts:142`) — answers from the rendered page, kind `pageEntity`                                | a tenant catalog. The resolver can only confirm what the page is already showing                                         |
| Execution (`ActionExecutor`)             | `pageExecutor` for UI; `packages/act-booking`, `act-email`, and `act` for server-side booking/email ports with mocked integration tests | a connected booking/email service, durable host recovery, gated CRM machine writes, payment and production tenant wiring |
| Confirmation and human approval          | `confirmedActionIds`, `declinedActionIds`, `humanApprovedActionIds`, `handoffRequested` on `TurnRequest`; gate reasons emit them        | an operator console. In the reference slice these are supplied by the caller, so no real person ever approves anything   |
| Multi-tenancy                            | Assistant tenant checks; CRM memberships, assigned-record scope, forced tenant RLS and two-context database verification                | production provisioning/session management and tenant operations across all runtime surfaces                             |
| Analytics                                | `AnalyticsEvent`s on every `TurnOutcome` (`packages/assistant/src/analytics.ts`)                                                        | anywhere to send them. No PostHog, OTel, Phoenix or Sentry                                                               |
| SDK integration                          | Graph building, consent, session and two independent compiled core consumers                                                            | a production host page shipped by a tenant                                                                               |
| Configurator                             | a config set and a quote                                                                                                                | persistence and tenant provisioning. "Built" means the file exists                                                       |
| Handoff to a human                       | `HandoffContext` and `packages/assistant/src/handoff.ts`                                                                                | a channel to a human. The request is modelled; nothing is delivered                                                      |
| Auth and authorisation                   | Core roles/tiers; CRM server-side memberships/resource permissions and fictional local reference sessions                               | production authentication, enrollment, session expiry/revocation and secure host deployment                              |

## Planned

Everything in this section is named in the PRD's intended technical baseline
(`PRD.md` §22) and is **absent from the tree**: no code, no dependency, no
configuration, no feature flag. This section exists because a missing
integration that nobody has written down keeps getting assumed to exist.

The PRD itself sets the ground rule for how to talk about these
(`PRD.md` §68): _the client does not buy "Spatius integration", "LiveKit
integration", "Gemini integration", or a bundle of APIs. Provider names are
implementation details unless the client asks for technical/procurement
disclosure._ So none of the rows below is a commitment, and none of them is
done.

**AI and model providers**

- A real `BrainProvider` backed by a Gemini Live-class model, behind the adapter
  the PRD names (`PRD.md` §22, :1054)
- Embeddings from an embedding-class provider for retrieval (:965)
- A realtime transport: LiveKit, with client-side rendering and a LiveKit Agents
  integration (:1053, :1104)
- Human rendering: a Spatius-class avatar provider behind a provider adapter
  (:1055, :1443) — the PRD is explicit that the provider must not be hardcoded
  outside the adapter
- Voice, avatar and realtime session orchestration

**Data and infrastructure**

- Neon PostgreSQL, pgvector, Drizzle — an Act attempt-store migration and SQL
  adapter exist, but no database driver, deployed database, or general
  persistence layer is connected
- Upstash Redis, Cloudflare R2
- Payload CMS
- Nx, Next.js, Tailwind + shadcn, Storybook (the web app is a hand-written
  esbuild bundle and two HTML pages)
- Infisical for secrets

**Workflows, messaging and commerce**

- Trigger.dev for durable background workflows
- Email delivery (Postmark or equivalent)
- Payment providers: Midtrans, Xendit, Stripe
- Booking and commerce systems: Cal.com, Shopify, WooCommerce, Medusa
- Broader CRM and ERP: a full CRM, ERPNext

**Observability and security tooling**

- PostHog, OpenTelemetry, Arize Phoenix, Sentry
- Playwright, k6, Semgrep, Trivy, OWASP ZAP

## Not Implemented

Deliberately absent, and worth naming so nobody reads a silence as a TODO.

| Area                                                | Position                                                                                                                                                                                            |
| --------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Accepting a payment                                 | The §34 acceptance criterion. No code in this tree does it, and nothing in the tree is on a path toward it beyond the gate and the contract slot where it would plug in                             |
| Refunds and cancellations that move money           | `order.cancel_refund` is in the action registry with no input contract, so §9 refuses it                                                                                                            |
| Autonomous high-risk actions                        | `PRD.md` §5 lists this as an explicit non-goal: no autonomous high-risk payments or refunds without an explicit authorization policy. `L5` (`NEVER_AUTONOMOUS_LEVEL`) is the code form of that rule |
| Cross-tenant data access                            | Enforced by construction — the tenant comes from the turn, never from a payload — but only because there is no shared store yet                                                                     |
| Offline business workflows decoupled from a session | Not started                                                                                                                                                                                         |

## What "Implemented" is worth here

Two things this tree does that a scaffold does not, both of which a gate
enforces:

1. **Ports return promises by contract**, not by accident
   (`packages/assistant/src/turn.ts:85-107`). A caller that could treat a result
   as an already-present value is a caller that could skip an `await` and quote a
   cached price as live.

2. **Missing machinery refuses rather than permits.** A missing entity resolver
   is a refusal. A missing input contract is a refusal. A throwing resolver is a
   refusal. A missing executor leaves the action `not_attempted`. In each case
   the code returns a labelled failure with a reason, because a check that can be
   skipped while still reporting `ok` is worse than no check at all.

The other direction matters too: a stored price's scale comes from the currency
contract, never from `Intl` — `formatMoney(1_438_000, 'IDR', …)` is Rp 1.438.000
on every machine. That rule exists because CI and a laptop disagreed about it
once, and the hundred-fold under-quote that followed is why
`packages/config` declares a minor-unit exponent for every currency.

## How to verify this document

```
pnpm typecheck     # every package + the web app
pnpm lint          # eslint --max-warnings=0
pnpm test          # vitest
pnpm verify        # install + structure + lint + typecheck + test + build
pnpm build && pnpm start
pnpm e2e:home      # 31 assertions against the served page
```

Nothing mechanically gates the prose in this file. `pnpm structure` gates the
tree — the runnable app must exist, imports must be declared, nothing may
escape the workspace — but the classifications above are maintained by hand. If
a row here stops matching the tree, the row is wrong, and the fix is to correct
the row.

## Decision Intelligence phase status (PRD §39)

**Partially Implemented.** The provider-neutral contract, five-task catalogue, Rule provider, off/shadow/assist orchestrator, tenant schema, turn integration, metadata events, optional reference inspector, offline eval and server-side Jev adapter exist and have offline tests. The Rule provider and reference app remain network-free. Jev transport is mocked in tests; no live request, calibrated Indonesian Jev evaluation, production decision service, retention agreement, or production telemetry sink is present. Laya and all future task seams remain Planned.

`pnpm decision:eval` scores 51 committed utterance cases (22 English, 17 Indonesian, 12 mixed) across four tasks. The Rule provider answered 35/204 expected judgments correctly, with 157 unanswered; effective routing is 50/51 correct and has **0 critical structured-truth downgrades among 25 protected cases**. Evidence sufficiency is excluded from this utterance-only score and evaluated separately on 28 synthetic evidence cases. These are Rule/offline measurements, not a Jev quality claim. The evaluator also reports per-label counts and confusion. Ordinary safety tests prove provider confidence cannot bypass capability, confirmation, validation, entity resolution or executor absence. See [`DECISION_INTELLIGENCE.md`](DECISION_INTELLIGENCE.md) and [`JEV_PROVIDER_RESEARCH.md`](JEV_PROVIDER_RESEARCH.md).

## Act pilot after delegation audit

**Partially Implemented.** `packages/act-booking` and `packages/act-email` implement narrow server-side `ActionExecutor` ports; `packages/act` dispatches them. A focused `runTurn` test reaches mocked booking and email gateways only after Act capability, user confirmation, input validation, and tenant-scoped entity resolution. Booking success requires an authoritative confirmed reference; email success means accepted for delivery, not delivered. Unknown outcomes fail without an automatic retry. No PMS, mail provider, connected production database, trusted production template catalog, real confirmation UI, or production tenant wiring is connected. This does **not** meet the PRD §34 real Act workflow criterion.

The original Act pilot gate run passed 898 tests across 58 files. After replay
and evaluation hardening, the final offline run passed **987 tests across 64
files**, `pnpm verify` passed all six stages, `pnpm format:check` passed, and the
served reference browser passed 31/31 E2E checks.

`pnpm decision:jev-eval -- --live --max-cases 5` is a separate optional synthetic SHADOW evaluation command. It has offline mocked tests, but no live Jev result or provider data agreement. Evidence sufficiency is scored on a separate 28-case synthetic evidence set; the 51-case utterance corpus still carries no evidence payload. See [`JEV_EVAL_READINESS.md`](JEV_EVAL_READINESS.md).

The new booking replay and email outbox boundaries require an injected durable
atomic attempt store, stable server-side HMAC key, and read-only authoritative
reconciliation. Tests use fakes and controlled races. No production server host
constructs these boundaries yet, and no production database or external booking or
email provider is connected. A confirmed booking replay currently depends on
underlying PMS health even when a stored reference exists; this is an
availability limitation. PRD §34 remains unmet.

## Act infrastructure seam after production-readiness delegation

**Implemented as server-side contracts, not connected to a production host.**
`@archava/act-storage` adds PostgreSQL attempt stores for booking and email,
with a unique tenant/action/key claim, conditional settlement, a server-held
digest of the upstream idempotency key, and an optional disposable local
PostgreSQL verifier. `@archava/act` now exposes a tenant-bound composition
factory that requires replay and outbox wrappers. `@archava/act-confirmation`
issues a tenant/session/action/input-bound challenge and delegates one-time
consumption to an atomic store port. Existing reference behavior remains
offline. The new packages introduce no external runtime dependency.

The confirmation service now has a PostgreSQL store adapter and a server-side
host call site, but neither is connected to a deployed database or HTTP route.
`runTurn` still accepts caller-supplied `confirmedActionIds` at its port; the
new host discards browser flags and derives confirmation from a verified
receipt. A deployment must preserve the same turn identity and timestamp on
retry, retain HMAC keys for the replay lifetime, keep raw upstream idempotency
keys out of provider requests/logs, and connect tenant-scoped credentials.
No PMS, live mail provider, or real user confirmation flow is connected. PRD
§34 remains unmet. See [`ACT_PILOT.md`](ACT_PILOT.md).

The reconciliation audit passed `pnpm verify` (1,131 tests across 72 files),
`pnpm format:check`, and the served browser E2E (31/31). The optional local
PostgreSQL verifier applied the migration, checked 14 invariants, and confirmed
one winner under separate-process claim and settlement races. These results
verify the infrastructure seams; they do not certify a production Act workflow.

## Act host, confirmation store, and Postmark adapter

**Partially Implemented.** `@archava/act-host` accepts a bounded visitor
envelope, checks policy and validated inputs before minting a challenge, and
turns a verified receipt into one confirmed action in `runTurn`. Tenant,
session, capability, role, executor, and keys come from server-owned
construction. Host tests run through the real confirmation service with an
in-memory challenge store and fake execution ports. `@archava/act-confirmation-pg`
implements atomic challenge issue and consume through an injected SQL client;
its optional local PostgreSQL verifier is separate from ordinary CI.
`@archava/act-postmark` sends an approved template through an injected
transport and returns accepted only for a validated Postmark message ID on an
HTTP 200 success. Its status port returns unknown until a trustworthy positive
reconciliation path exists. No live provider send has been run.

There is still no HTTP host route, production tenant/session binding, deployed
challenge database, durable host-attempt ledger, live Postmark credential, or
PMS integration. The host ledger contract does not yet make concurrent reviews
choose one canonical `occurredAt`, and identical action/input submissions in
one session do not yet have an explicit new-intent lifecycle. These are
deployment blockers for reliable replay protection. The browser remains
offline, and PRD §34 is still unmet. See [`ACT_PILOT.md`](ACT_PILOT.md).

The integration audit passed all six `pnpm verify` stages: **1,288 tests in
79 files**. `pnpm format:check` passed; the served reference browser passed
31/31 E2E checks. The optional disposable PostgreSQL verifier passed 27/27
checks, including separate-process consume races. These checks use mocked
provider transport and no live Postmark or PMS credential.
