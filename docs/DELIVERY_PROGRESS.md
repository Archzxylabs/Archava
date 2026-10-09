# Archava delivery progress

**Baseline:** 27 September 2026, before a1–a3 Decision Intelligence results are audited.

**Target:** Archava v1 commercial readiness in `PRD.md` §34, including the platform needed to operate it.

**PRD v2 scope, evidence updated 9 October 2026:** §34 remains the broader commercial gate. The
new module-library milestones in [PRD §49](../PRD.md) are assessed separately:

| Milestone                  | Current evidence                                                                                                                                                                                                 |
| -------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| M0 specification alignment | Research, PRD and supporting operating/status documents updated; documentation only.                                                                                                                             |
| M1 independent core reuse  | Two independent cold-installed core consumers use matching compiled artifacts; see the M1 entry below.                                                                                                           |
| M2 business-module reuse   | One persistent contacts/activity/follow-up module serves two contexts with the same API/dashboard; see the M2 entry below.                                                                                       |
| M3 controlled operations   | Supported local owner pilot adds verified native follow-up execution, receipt recovery, restore hold and diagnostics. Public hosting, general AI planning and external/presence providers remain separate gates. |
| M4 library expansion       | Small CRM sales-pipeline extension locally verified across two independent inquiry consumers; broader kits, support and commercial scope remain open.                                                            |

The historical weighted percentage below is not recalculated or reused as v2
progress. M1/M2 evidence is assessed separately from commercial readiness.
The bounded [internal owner pilot](INTERNAL_OWNER_PROGRESS.md) has its own
explicit six-capability planning denominator: 50% at source `83b65a8`, before
the next durable action work. It does not score every future CRM/ERP capability.

**9 October pipeline checkpoint:** contact-linked opportunities, guarded stages,
immutable history, separate grants, v1 schema upgrade and recovery now have local
evidence. See [pipeline scope](CRM_PIPELINE.md) and
[verification](CRM_PIPELINE_2026-10-09.md). This does not complete M4 or change the
historical commercial score; no contract, payment or project execution is inferred
from Won.

**Current estimate after Act pilot audit:** **about 30% weighted delivery coverage. Release readiness: not met.**

**Source consolidation, 7 October 2026:** Decision Intelligence, Act host/storage/
confirmation/Postmark packages, the website and local CRM/ERP frontend, and the
portable voice/avatar template now share one source tree. The 30% figure remains
the last audited commercial score; the historical scorecard below does not yet
score the standalone template or new frontend. Source consolidation does not
establish shared Chat/Voice/Human intelligence, live Act/Transact, authentication,
CRM/ERP integration, or production deployment. The operational-readiness task
results were completed in the 9 October durable library checkpoint; see
`delegation/act-operational-readiness/` and the subsequent
[local owner pilot](LOCAL_OWNER_PILOT.md).

The audited Decision Intelligence line moves from 10% to **65%**: contracts, Rule provider, orchestrator, three modes, five tasks, assistant/reference integration, offline evaluation, metadata observability, and mocked Jev adapter are verified. Live Jev behavior, provider data terms, production telemetry and service hosting remain open. Its 8-point weighted contribution moves from 0.80 to 5.20, lifting the total from **24.15** to **28.55/100 ≈ 29%**. Other lines keep their baseline scores because this phase did not deliver voice, avatar, real Act/Transact workflows, tenant operations, or production deployment. This estimate is independent of the Rule provider's eval accuracy.

The next Act pilot adds tenant-scoped booking and email executor ports plus mocked
`runTurn` integration. Its estimated line coverage moves from 10% to **25%**,
adding 1.50 weighted points: **30.05/100 ≈ 30%**. No external booking or
email was performed, so the PRD §34 real Act gate remains open. The optional
Jev SHADOW eval command is ready but has not made a live request; Decision
Intelligence stays at 65%.

The later replay/outbox and evidence-evaluation hardening improves offline safety coverage but does not add a real provider, durable store, or live evaluation. The weighted estimate therefore remains about 30% until those external and operational claims can be verified.

This is a planning estimate, not a measurement of code volume, test count, developer productivity, revenue, or runtime performance. The weights express how much each line contributes to the target. A passing unit test raises a line only when it proves the relevant user or operator capability. Mocked integrations and reference fixtures are labelled as such.

## Baseline scorecard

| Delivery line                                         |   Weight | Coverage |       Weighted points | Evidence and principal gap                                                                                                                     |
| ----------------------------------------------------- | -------: | -------: | --------------------: | ---------------------------------------------------------------------------------------------------------------------------------------------- |
| Product specification and architecture                |       5% |      75% |                  3.75 | `PRD.md`, `agent.md`, `docs/ARCHITECTURE.md`; Decision Intelligence addendum and production architecture remain open.                          |
| Core context, knowledge, policy, and turn pipeline    |      12% |      50% |                  6.00 | `packages/core`, `knowledge`, `acl`, `assistant`; offline turn works, authoritative production systems do not.                                 |
| Decision Intelligence                                 |       8% |      10% |                  0.80 | Uncommitted `DecisionProvider` port and tests only; orchestration, runtime integration, Jev adapter, and eval results are pending.             |
| Chat, web SDK, and reference browser                  |       8% |      50% |                  4.00 | `packages/chat`, `sdk`, `apps/web`, `tools/e2e`; one offline tenant/reference slice, no deployed multi-tenant product.                         |
| Voice and realtime conversation                       |      10% |       0% |                  0.00 | No LiveKit/realtime voice runtime.                                                                                                             |
| Human avatar presence                                 |       8% |       0% |                  0.00 | No live avatar rendering or Human preflight/runtime fallback.                                                                                  |
| Act workflows and external integrations               |      10% |      10% |                  1.00 | Policy/validation/executor boundaries exist; no real booking, CRM write, or email workflow completes.                                          |
| Transact, payment, and receipts                       |      12% |       5% |                  0.60 | Action vocabulary and safety boundaries exist; no payment or receipt integration.                                                              |
| Tenant persistence, admin, and isolation in operation |      10% |      10% |                  1.00 | Tenant-scoped shapes and reference checks exist; no shared database, provisioning, auth, or client admin.                                      |
| Studio and commercial configurator                    |       7% |      50% |                  3.50 | Deterministic pricebook/configurator and browser Studio slice exist; persistence and full proposal workflow remain.                            |
| Observability, security, and reliability              |       6% |      25% |                  1.50 | Event shapes, masking and gates exist; no production telemetry sinks, retention controls, operational hardening, or live reliability evidence. |
| Verification and delivery operations                  |       4% |      50% |                  2.00 | CI, structure/type/lint/test/build gates and reference browser E2E exist; production deploy and integration/load/security verification remain. |
| **Total**                                             | **100%** |          | **24.15 / 100 ≈ 24%** |                                                                                                                                                |

## How to update it

`weighted points = weight × coverage / 100`; sum the points for the total. Coverage is an evidence-backed estimate, rounded to the nearest practical milestone rather than inferred from lines of code:

- **0%:** absent.
- **10–25%:** contract, scaffold, or isolated tested behavior.
- **50%:** runnable and tested end-to-end reference behavior, with mocks/fixtures clearly identified.
- **75%:** real provider or business system integrated and tested in a controlled environment.
- **100%:** the applicable PRD acceptance criteria are met, documented, and operationally verified.

The lead updates a row only after auditing source, tests, and the relevant runnable result. Record the evidence and remaining gap in the same row. Work in progress, agent reports without source audit, and live provider claims without a live test do not count as completed delivery.

The percentage does not replace release gates. In particular, the current PRD §34 criteria for real Act booking/CRM/email and Transact payment → confirmation → receipt remain unmet. Voice and Human Presence are also absent. A high aggregate percentage must not be used to claim commercial readiness while a required gate is open.

## Next review point

The next Act review needs a selected booking system, approved tenant template
catalog and mail provider, durable idempotency/outbox behavior, server-side
tenant configuration, a real user confirmation surface, and a controlled
external transaction. The next Jev review needs the optional live synthetic
evaluation and a data-term decision. Keep both claims separate from offline
tests and mocked gateways.

## 8 October 2026 — reusable-core cleanup and offline M1

Nine compiled internal artifacts are consumed by two independently installed
hospitality/retail examples with identical implementation hashes. Knowledge,
branding and allowed page actions differ; cross-tenant and unsafe model action
requests are refused. V2 deployment planning keeps template suggestions separate
from declared scope, with explicit V1 compatibility. See
[library consumption](LIBRARY_CONSUMPTION.md).

This advances the PRD v2 reuse milestone; it does not change the historical V1
commercial score or close the three Pending Act operational tasks. Persistent
CRM reuse, production identity/recovery and shared realtime presence remain next
work. The company website remains outside this repo.

## 8 October 2026 — persistent CRM slice / offline M2

The first native business module now covers contacts, activities and follow-up
tasks. Hospitality and professional-service references consume identical compiled
CRM artifacts and the same dashboard/API/service. PostgreSQL verification proves
retry identity, revision races, tenant/resource permissions, audit atomicity,
revocation and restart durability. See [CRM module](CRM_MODULE.md).

Reference authentication uses fictional local accounts; production identity,
backup/restore, live Act writes/provider recovery and broader CRM/ERP remain
release work. No historical commercial-readiness score or Pending Act task is
marked complete by this slice. The full check results are in
[offline M2 evidence](CRM_M2_2026-10-08.md).

## 8 October 2026 — persistent authenticated CRM host / partial M3

`apps/crm` now uses the compiled CRM packages and an optional identity adapter.
Owner/staff/viewer credentials replace fictional identity selection on this host.
Sessions expire and are revoked server-side. Data survives host/database shutdown;
private backup archives restore into a fresh database with migration/grant checks,
preserved retry receipts and explicit account recovery. See
[CRM runtime](CRM_RUNTIME.md).

This is local operational evidence. Public TLS/deployment, MFA/SSO, automated
backup/alerts, provider correlation, durable Act attempts and shared realtime
presence remain open. The historical readiness estimate and Pending Act tasks
are unchanged. Checkpoint `4f24164` preserves the M1/M2 baseline.

## 8 October 2026 — reusable CRM read/draft conversation / partial M3

The optional conversation adapter and shared panel connect existing CRM tools to
`runTurn`. Two independent compiled consumers read tenant-scoped PostgreSQL data
using identical adapter hashes. The authenticated host verifies membership,
staff assignment and draft permissions; conversation requests do not change
contacts, activities, follow-ups, command receipts or mutation audit counts.

The shipped planner recognizes local commands and makes zero model requests.
Drafts require explicit content/date and remain transient. Live planner quality,
durable Act confirmation/execution, public hosting and telemetry remain open.
See [CRM conversation](CRM_CONVERSATION.md). Historical commercial percentages
and the three Pending Act operational tasks remain unchanged.

## 9 October 2026 — bounded Gemini planning and human-reviewed follow-up / partial M3

The optional adapter now supports live intent planning for a committed synthetic
corpus and pipeline read projections across the same two business consumers.
Arbitrary private text remains local under the unpaid provider data boundary.
An explicit human Save uses existing authorization, revision and durable
idempotency/audit mechanics; conversation turns still perform no mutation.

See [Gemini pilot](CRM_GEMINI_PILOT.md) for its data, pricing and evaluation
limits. This does not establish general conversational quality, autonomous Act
approval/execution/recovery or production deployment. The historical commercial
score and three Pending Act tasks remain unchanged.

## 9 October 2026 — durable provider admission and diagnostics / partial M3

The optional provider-runtime primitive shares a persisted UTC-day allowance and
lease across independent consumers. The host records metadata-only outcomes and
exposes a private status command. Restart retains usage; restore conservatively
holds provider calls until the next UTC day while authorized local CRM remains
available. See [provider runtime](PROVIDER_RUNTIME.md).

This adds controlled operational evidence without completing M3, production
telemetry/alerts, project-wide billing limits or durable autonomous Act execution.
Historical commercial percentages and Pending Act tasks are unchanged.

## 9 October 2026 — read-only reviewed Save recovery / partial M3

The existing CRM command receipt can now be queried by its original human with
current tenant/read/resource authorization. An optional shared panel recovery
store preserves only a request UUID in the tab. A lost response followed by
reload is checked without resubmission; an unavailable receipt stays unconfirmed
and prevents a new Save. Both independent business consumers use the same 0.3.0
CRM artifacts. See [recovery scope](CRM_COMMAND_RECOVERY.md).

This is human operation recovery. Autonomous Act approval/lifecycle, broader
reconciliation, public hosting and telemetry remain open. M3 stays partial; the
historical commercial percentage and Pending Act tasks are unchanged.

## 9 October 2026 — supported local owner approval release

The owner chose local/free completion. `@archava/crm-act` adds exactly one native
owner-approved follow-up command, backed by current delegated grants, signed
consent, active claim, version binding, atomic domain/audit receipt and read-only
recovery. Restored copies hold execution under a new private approval epoch until
explicit operator resume. Diagnostics, real restoration and Chromium flows are
verified; the existing private local account/configuration are preserved.

This completes [the supported local owner acceptance](LOCAL_OWNER_PILOT.md).
Broader M3/presence/commercial release and general ERP remain separate. The
historical commercial percentage is unchanged; the broader owner planning
checkpoint and the completed local acceptance use distinct denominators.
