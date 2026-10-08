# Architecture

What the pipeline is, and where the boundaries are. This document describes the
tree as it is; for what the tree does not yet do, see
[`BUILD_STATUS.md`](BUILD_STATUS.md).

The future reusable business-module architecture is specified in [PRD v2
§§40–49](../PRD.md) and explained in [the research](REUSABLE_PLATFORM_RESEARCH.md).
This document continues to describe the implementation, including its separate
realtime template boundary. A PRD upgrade does not add auth, module resolution or
CRM/ERP persistence to the code.

## The shape

A turn is a sequence of boundaries. Each boundary holds a **port** — an
interface, in TypeScript, with implementations in the tree — and each
implementation is an honest in-tree stand-in that says what it is standing in
for.

```
browser (SDK)                 packages/assistant                packages/acl
─────────────                 ─────────────────                 ────────────
context graph ──────▶ mask / project ──▶ BrainProvider.plan ──▶ ActionPolicy
(ids, names, flags,              (§16 masking)        │            decide
 never form values)                                   ▼
                              KnowledgePort.retrieve ─┐
                              StructuredTruthPort ────┤ answer + components
                                                      ▼
                              validateActionInputs (§9) ──▶ EntityResolver
                                                      ▼
                              ActionExecutor.execute (§18) ──▶ side effect
                                                      ▼
                                        TurnOutcome ──▶ chat renderers
```

Every port returns a promise. That is a deliberate contract
(`packages/assistant/src/turn.ts:85-107`): a port that may return synchronously
is a port whose result can be consumed without anyone noticing it had not
arrived, and a caller that could tell the difference from the outside is a caller
that could skip an `await`.

## The ports

| Port                  | Declared                                  | In-tree implementation                                                                                                                       | What it would be in production |
| --------------------- | ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------ |
| `BrainProvider`       | `packages/assistant/src/turn.ts`          | `ScriptedBrain`, `ReferenceBrain` — deterministic fixtures (`packages/adapters/src/brain.ts`)                                                | a model client                 |
| `KnowledgePort`       | `turn.ts:85`                              | `@archava/knowledge` — `retrieveContext(request, store)`, wired by `buildKnowledgeStore(config)` (`packages/reference/src/knowledge.ts:224`) | a hosted index                 |
| `StructuredTruthPort` | `turn.ts:103`                             | `referenceTruth` — date-stated fixtures (`packages/reference/src/rates.ts:421`)                                                              | an inventory / pricing service |
| `EntityResolver`      | `packages/assistant/src/validation.ts:48` | `pageEntityResolver` (`apps/web/src/surface.ts:142`) — page-scoped                                                                           | a tenant catalog               |
| `ActionExecutor`      | `packages/assistant/src/execution.ts`     | `pageExecutor` (`apps/web/src/surface.ts:217`) — `ui.*` only                                                                                 | the tenant's real systems      |

The offline reference does not contact a model, a PMS, or a payment provider. `pageExecutor`
is the clearest example of the rule: it will highlight a room and it will not
book one. It answers `not_a_page_action` — _"This page can only draw on itself;
a browser may not perform that action."_ — for anything a browser is not entitled
to do on a tenant's behalf. Where a real system would sit, the code returns a
labelled failure rather than a plausible-looking success.

## What a turn actually does

`runTurn` (`packages/assistant/src/turn.ts:914`) takes a `TurnRequest` and
returns a `TurnOutcome`. The order matters, and each step fails closed:

1. **Mask, then project.** `projectContextGraph` (`turn.ts:221`) emits route,
   page kind, section, locale, and entity `{id, name}` — never a form value,
   because the graph carries field _names_ and not field values. A value cannot
   be projected because it was never carried. Masking strategies are
   `preserve | redact | partial | tokenise` (`packages/acl/src/mask.ts:15`).

2. **Ask the brain.** The provider returns an answer, optional generative
   components, and optional action requests. A brain that cannot support a
   claim is recorded as a `knowledgeGap`, not glossed.

3. **Answer from knowledge or structured truth.** `basis` is one of
   `structured_truth | retrieval | none` — the outcome states which, so a reader
   can tell a live value from a corpus answer from a refusal.

4. **Gate the actions** (`packages/acl/src/policy.ts`). Levels `L0`–`L5`
   (`packages/acl/src/actions.ts:18`), roles `archava_assistant | owner | admin
| editor | viewer`, capability tiers `assist | act | transact | enterprise`
   (`packages/config/src/pricing/types.ts:8`). `L5` is `NEVER_AUTONOMOUS`. The
   result is a decision plus a reason — `DENIAL_REASONS` is a closed list, so a
   refusal is never a bare boolean.

5. **Validate the inputs** `validateActionInputs` (`validation.ts:289`). A
   declarative contract per action id checks types, required fields, and that no
   `tenantId` travelled inside the payload. The registry names 24 actions
   (`packages/acl/src/actions.ts`); 12 of them carry a contract
   (`INPUT_SCHEMAS`, `validation.ts:181`). The other 12 are refused outright —
   `No input contract` — and they are not a random sample: `payment.initiate`,
   `payment.status.read`, `checkout.start`, `order.confirm` and
   `order.cancel_refund` are all in that half. A commercial action cannot run
   here because there is no contract for it, which is a stronger statement than
   "we have not wired it up yet".

6. **Resolve the entity ids.** Every contract field that names a tenant-catalog
   entity — at any depth, including `customer.customerRef` nested inside
   `booking.create` — is put to the `EntityResolver`, which is asked with the
   _turn's_ tenant.

7. **Execute** (`packages/assistant/src/execution.ts`). Only if an executor was
   supplied, and only for actions the gate allowed and §9 confirmed.

## Three fields, not one

`GatedAction` separates what a gate permitted from what an executor confirmed:

```ts
policy:     'allowed' | 'denied' | 'confirmation_required' | 'requires_human_approval'
execution:  'not_attempted' | 'succeeded' | 'failed'
output:     what the executor returned, when it ran
```

An action can be `allowed` and still `not_attempted`. The reference slice leaves
the executor out of the turn entirely, so every allowed action on the page is
`not_attempted` — which is the truth. Collapsing these into one flag is how a
booking gets reported as existing because it _would have_ been booked.

## The rules the tests enforce

These are not style preferences. Each one is a failure mode that looks like a
pass, and each has a test file that exists to stop it returning.

**The absence of a resolver is a refusal, not a permission.**
`packages/assistant/test/validation.test.ts` states the contract as a table
rather than enumerating the module, so a drifted action id fails loudly. A
validator that returns `ok` when no resolver was supplied passes every shape
test it has — the inputs are the right types, the required fields are present,
the tenant id is not in there — and an action executes with an id nobody ever
checked. `ui.highlight` / `ui.compare` are held to the same rule against the
page's own resolver, with the kind name observed rather than assumed, because a
kind typed differently from the one the resolver checks is the same hole wearing
a different hat.

**A stored price's scale comes from the currency contract, never from `Intl`.**
`packages/chat/test/money.test.ts` fixes `amountMinor: 1_438_000` and asserts the
digit count is identical in every locale `Intl` will accept, including one it
does not know (`xx-YY`) and one that renders non-Latin numerals (`ar-EG`, pinned
by `\p{Nd}` rather than `\d`). The divisor used to be read from
`Intl.NumberFormat(...).formatToParts(1)`, which made a _stored_ price depend on
the ICU dataset of the machine printing it: CI proved it — IDR gained two
fraction digits on the runner and lost them on the laptop that wrote the code,
so the same amount meant Rp 1.438.000 in one place and Rp 14.380,00 in the other.
Same input, two prices, and no single-machine gate could see it.

**An unreachable catalog is an answer of no.**
A throw from the resolver resolves to `ok: false` pointing at the id that went
unanswered, not an uncaught exception and not a yes. The refusal is _about the
id_, because that is the question that went unanswered; field `*` would suggest
the whole action was refused for a generic reason.

## The browser side

```
apps/web/src/
  server/server.ts    HOST/PORT (default 127.0.0.1:4173), three modes
  server/api.ts       POST /api/quote — the only HTTP endpoint
  slice.ts            createSlice(): wires every port for the reference tenant
  page.ts             bootstraps the SDK against the served page
  dom-bridge.ts       SDK events → context graph
  surface.ts          pageEntityResolver, pageExecutor
  main.ts / studio.ts the two entry points (chat surface, Studio)
```

The server holds no conversation state. `pnpm dev` compiles both bundles into
memory; `pnpm build` writes `apps/web/dist/{app.js,studio.js}`; `pnpm start`
serves those. Everything else — the awareness graph, the turn, the rendering —
runs in the visitor's browser against a graph the page produced.

`POST /api/quote` is the only HTTP endpoint, and it is stateless: a POST in, a
response out, nothing kept. The body is `{ intake, branding?, updatedAt?,
allowedOrigins? }`. A valid intake returns a variant set — `recommended`,
`lean` and `full` quotes, a recommendation, a budget verdict and the
`selectionReason` that produced it — and, when `branding` and `updatedAt` are
supplied, an emitted `ClientConfig` (`schema_version`, `tenantId`,
`environment`, `presence`, `capability`, `region`, `template`, …). Two
identical posts return byte-identical variant sets, which is what makes a quote
reproducible from a saved intake. It refuses in the other directions too: a
`GET` is `405` ("A quote is a submission, not a fetch."), a body that is not
JSON or not an object is `400`, and an invalid intake is `400` carrying the
per-field Zod issues — the intake schema is `.strict()` and camelCase.

The SDK (`packages/sdk`) reduces host-page events into a `ContextGraph`:
`seedContextGraph` → `reduceContextGraph` / `foldContextEvents` →
`actionableContext` → `parseContextGraph` (`packages/core/src/context-graph.ts`).
The graph is Zod-parsed on the way in, so a host page cannot hand the pipeline
an arbitrary object.

## The tools

- `tools/configurator` (`pnpm configurator`) — intake → normalised → variants /
  recommendation / budget → built client config. `pnpm configurator` is the CLI
  entry at `tools/configurator/src/cli.ts`.
- `tools/scripts/structure.mjs` (`pnpm structure`) — the honesty gate on the
  tree: the runnable web app must exist, every `@archava/*` specifier in source
  and tests must be a declared dependency, and no file may reach outside the
  workspace.
- `tools/scripts/verify.mjs` (`pnpm verify`) — install + structure + lint +
  typecheck + test + build, the same list in the same order as CI, so "CI
  passed" and "`pnpm verify` passed" are one fact rather than two. It adds no
  checks of its own.
- `tools/e2e/home.mjs` (`pnpm e2e:home`) — drives the served page in a browser
  and asserts against `BASE` (default `http://127.0.0.1:4173`).

## Package graph

```
core      ChatEvent vocabulary, ContextGraph schemas, tenant shape
config    pricebook / template / currency contracts — all Zod-parsed
pricing   deterministic quote engine (+ rounding rules)
acl       action registry, roles, capability gate, masking
knowledge retrieval store, ranking, structured-truth port shape
assistant turn assembly: mask, gate, §9 validation, execution, outcome
adapters  BrainProvider and presence/renderer adapters
decision  bounded task catalogue, Rule provider, orchestrator and ranking primitive
act-booking authoritative booking gateway and booking.create executor port
act-email approved-template email gateway and email.send executor port
act       tenant-bound composition and dispatch of the two Act executors
act-storage PostgreSQL attempt-store adapters and migration
act-confirmation server-side one-time confirmation challenge contract
act-confirmation-pg PostgreSQL challenge store and migration
act-host  server-owned Act review and presentation boundary
act-postmark injected Postmark template-mail gateway
chat      DOM renderers: shells, money, knowledge cards
reference the reference tenant: fixtures, rates, knowledge, brain
sdk       page awareness graph for a host page
+ apps/web, tools/configurator
```

Dependencies point inward: `reference` may depend on everything; `core` depends
on nothing but `zod`. `pnpm structure` enforces this, so a boundary violation is
a gate failure rather than a code-review opinion.

## Decision Intelligence extension (PRD §39)

The turn now has an optional bounded judgment seam between deterministic classification and the Brain. `packages/adapters/src/decision.ts` declares a vendor-neutral `DecisionProvider`; `packages/decision` owns five task definitions, per-task confidence/direction policy, the offline Rule provider, and the orchestrator. The assistant supplies Foundation baselines and projected/redacted context. The orchestrator's `off` mode makes no call, `shadow` records disagreements without effects, and `assist` may apply an approved answer. Routing can only escalate to a trusted structured-truth subject. ActionPolicy, input validation, entity resolution and ActionExecutor remain separate and authoritative.

Jev lives in `@archava/adapters/server`; no Jev type enters core product logic or the browser. The reference browser can run Rule decisions offline and renders trace metadata under `?inspect`. A server host may inject Jev through the same port. No live Jev verification has occurred. The full contract, privacy boundaries and eval command are documented in [`DECISION_INTELLIGENCE.md`](DECISION_INTELLIGENCE.md).

## Act pilot boundary

`packages/act` dispatches only booking.create and email.send to their
server-side executors. They receive actions only after the existing turn
pipeline has applied ActionPolicy, user confirmation, input validation and
tenant-scoped entity resolution. Each executor then asks an injected
authoritative gateway for a confirmed booking or durable email acceptance.
The composition factory requires booking replay and email outbox boundaries;
the PostgreSQL attempt-store adapters provide their durable port implementations.
`@archava/act-host` can convert a verified challenge receipt into the turn's
confirmation input and bind one validated action to that turn. It is a
server-side library, not an HTTP route; the caller must supply a trusted tenant
session, a durable attempt ledger, and the composed executor. The ledger port
does not yet guarantee an atomic canonical timestamp under concurrent reviews
or distinguish a later new intent with identical inputs, so deployment replay
safety remains incomplete. `@archava/act-confirmation-pg` implements the
one-time challenge store through an injected SQL client, and
`@archava/act-postmark` implements a template-send gateway through an injected
HTTP transport and server token. Neither is wired to a production database or
provider. The reference browser has no access to these gateways. Current
integration tests use fakes; no live PMS or mail provider is connected. See
[`ACT_PILOT.md`](ACT_PILOT.md).

## Internal artifacts and deployment planning — 8 October 2026

`tools/library/pack.mjs` compiles the SDK/assistant/Chat dependency closure into
nine private ESM/declaration artifacts while retaining module identity.
`tools/library/verify-consumers.mjs` installs them in two projects outside the
workspace and checks distinct knowledge/branding/action sets with shared hashes.
The web browser build now needs no configuration alias. Explicit `/pure` exports
keep filesystem readers on server paths.

`packages/config/src/client/modules.ts` provides thin host-owned planning
metadata; `plan.ts` resolves requested modules/dependencies and separates
suggested integrations from host connection/health snapshots. Existing V1
resolution is preserved through `resolveLegacy()`; new quote responses add V2
`deploymentPlan`. Planning readiness never grants action authority. Details and
migration limitations are in [library consumption](LIBRARY_CONSUMPTION.md).

## First business module — CRM contacts/activity/follow-up

`@archava/crm` owns schemas, public service/API and explicit read/draft tool ports;
its `/dashboard` subpath shares one DOM component across two contexts.
`@archava/crm-pg` supplies the server-only PostgreSQL adapter with a migration
ledger, membership/resource checks, forced tenant RLS, revision preconditions,
command identity and atomic metadata audit. Its pinned `pg` dependency is an
intentional database-driver addition; the Archava core has no CRM dependency.

Compiled artifacts include the owned migration. The CRM verifier loads installed
exports with plain Node, cold-builds both consumers and exercises an isolated
real PostgreSQL cluster. Production host identity and live Act integration remain
separate. See [CRM module](CRM_MODULE.md) for contracts and release boundaries.

## Persistent CRM consumer and identity adapter

`apps/crm` is a host consumer of installed compiled CRM artifacts. It supplies
verified identities from the optional server-only `@archava/identity-pg` adapter.
Identity owns credential/session/rate-budget tables, migrations and lifecycle
events; it does not own business roles. CRM memberships/resources remain the
authority for record access. Core and CRM do not import this identity adapter.

The host uses private persistent PostgreSQL state, session-bound CSRF transport,
cookie login and the same dashboard component. Operator enrollment and membership
creation share one transaction. Backups include private runtime metadata;
restoration creates a fresh database, preserves mutation identity and requires
explicit account recovery. See [CRM runtime](CRM_RUNTIME.md). Public deployment,
shared presence and durable Act/provider execution retain separate release gates.

## Optional CRM conversation adapter

`@archava/crm-assistant` depends on CRM and the existing assistant/core contracts;
the core has no reverse CRM dependency. Its planner sees the user's bounded text
and produces a strict read/prepare request. Verified host identity scopes each
CRM tool. `StructuredTruthPort` carries the authorized projection into `runTurn`;
the host's optional authoritative routing can only add live-data requirements.
No executor or page action is available. The browser `/panel` uses the shared
contracts and renders records as text. See [CRM conversation](CRM_CONVERSATION.md)
for local commands, independent-consumer evidence and the live-provider/Act gaps.
