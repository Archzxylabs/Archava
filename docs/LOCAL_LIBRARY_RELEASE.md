# Local reusable library release

## Current extension — profile 1.5.0, 11 October 2026

The reusable scheduling domain and PostgreSQL adapter extend profile 1.4.0:
30 internal artifacts, 18 unchanged external pins, seven owned database ledgers,
host 0.8.0, config 0.7.0 and kits 0.2.0. New assembly v3 supports optional
`scheduling`; existing v1/v2 compositions and four-module defaults remain supported.
The pure module needs no CRM or LLM. The existing host composes it with current
identity/workspace membership plus independent scheduling grants.

| Extension                    | Accepted evidence                                                                                                                                                                                                                                                   | Limit                                                                     |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Exclusive timed reservations | Two independently installed consumers use the same implementation for staff appointments and individual asset rental; PostgreSQL current-grant/RLS checks, duplicate/overlap races, revision rollback, expiry during a proven lock wait, restart/restore; 14 checks | Capacity one; explicit instants; no complete appointment/rental/hotel kit |
| Local owner composition      | Auth/CSRF/current grants, real Chromium resource/hold/confirm/move/cancel flow, response-loss recovery, viewer refusal, disable/re-enable and fresh backup restore; 9 checks                                                                                        | Human writes; in-memory browser recovery; blocks/audit details via API    |
| Trusted composition          | K07/K10 narrow `resource_reservations` scope composes through v3; updated 8-check kit proof and retained v1/v2 scopes                                                                                                                                               | All 40 full operating kits remain blocked on mandatory gaps               |
| Preserved behavior           | Eight root gates, 1,485 tests/105 files; fresh CRM 38, Act 18, runtime 44, Chromium 3/70, project 15/11 and upgrade 4                                                                                                                                               | Local/synthetic scope; no new live-provider acceptance                    |
| Security and owner migration | 27 controls, zero supported root/runtime advisories; original 30 tables, keys/settings/grants retained; verified doctor and backups before/after                                                                                                                    | Template 13 moderate findings remains separate; no security certification |

[Sanitized release evidence](audit/2026-10-11-resource-reservations-release.json)
records current artifact hashes and runnable reports. See
[the module contract and activation](RESOURCE_RESERVATIONS_MODULE.md) and
[the complete programme ledger](STARTER_KIT_IMPLEMENTATION.md). No existing account
receives scheduling access automatically and no pricing/provider policy is changed.

`pnpm security:verify` also runs scheduling domain/host proofs; acceptance rejects
stale profile evidence. Availability is a database observation: only a successfully
committed hold reserves a slot. Confirmation is an internal operator action and
has no customer agreement, financial or external-provider meaning. Next build the
accommodation domain's room-night/stay/housekeeping rules.

## Previous extension — profile 1.4.0, 11 October 2026

The accepted 1.3.0 baseline below is extended by three private artifacts: `kits`,
`projects` and `projects-pg`. There are 28 internal artifacts, 18 unchanged external
pins, host 0.7.0, config 0.6.0 and six owned database ledgers. The project schema is
additive and does not migrate CRM records or automatically grant account access.

| Extension                                 | Accepted evidence                                                                                                                                                       | Limit                                                            |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Trusted catalogue and requirement planner | 40 immutable families; exact installed-profile checks; positive/narrow and missing/forged-authority refusals                                                            | No complete operating kit accepted                               |
| Independent consumption and assembly      | Two cold-installed kit consumers, browser-safe contracts and scoped v1/v2 assembly; 7 checks                                                                            | Full kit requirements stay blockers; assembly is configuration   |
| Persistent project domain                 | Two independent consumers; real PostgreSQL grants/RLS, 20-request idempotency race, parent revisions, delivery invariants, SQL immutability, restart/restore; 15 checks | Internal scope/task/milestone tracking only                      |
| Owner composition                         | Explicit grants, authenticated APIs/read tools, Chromium workflow and response-loss recovery, disable/re-enable and backup/account recovery; 11 checks                  | No project machine writes, client contract, invoice or payment   |
| Existing behaviour                        | Eight root gates; 1,475 tests/104 files; CRM 38, Act 18, host 44, Chromium 3/70, prior-profile upgrade 4                                                                | Local/synthetic acceptance                                       |
| Security extension                        | 22 applicable controls; fresh root/runtime zero findings; exact dependency inventory                                                                                    | Template 13 moderate findings remains its separate realtime gate |

[Sanitized current evidence](audit/2026-10-11-kit-project-release.json) links the
executed reports. The original owner state was backed up before and after additive
migration; all eight CRM tables, existing grants and identity/fingerprint keys are
unchanged. Doctor verifies the new profile. Project activation remains explicit.

Assembly 2.0.0 adds optional `projects`; assembly 1.0.0 and original four-module
settings still work. Narrow `crm_inquiry` (K01/K05) and `project_tracking` (K05/K31)
are accepted configuration scopes. Both full services and EPC requests remain
blocked on missing domains/capabilities. The approved pricebook is unchanged.
See [project integration and limitations](PROJECT_DELIVERY_MODULE.md) and
[the full work ledger](STARTER_KIT_IMPLEMENTATION.md).

`pnpm security:verify` now also invokes kit, project and project-host proofs. The
runtime report records its actual verified profile; security verification rejects
stale runtime/kit/project proof profiles before accepting the matrix.

## Historical baseline — profile 1.3.0

The finite acceptance scope is PRD 2.1 §50. Company website, live voice/avatar,
public deployment and the broader ERP catalogue remain separate milestones.
Evidence below describes executed checks, not product/security percentages.

| Item                               | Status   | Evidence                                                                                                                                                                             |
| ---------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| EXEC-01 supported profile          | Accepted | 8 profile tests; identical cold consumer implementations; exact installed hashes/dependencies/schema and changed-package startup refusal; final host proof 44 checks.                |
| EXEC-02 composition/lifecycle      | Accepted | Typed planner/assembly; independent minimal/full installs; disabled routes/writes/tools; retained receipts and unresolved-effect refusal; minimal Chromium DOM proof.                |
| EXEC-03 presence boundary          | Accepted | Six gateway tests; installed chat and standalone Python transcript share server policy; forged identity, binding, revision, replay, expiry, private-tool and timeout refusal.        |
| EXEC-04 security/dependency matrix | Accepted | 15 source/control evidence rows; 38 real PostgreSQL CRM checks, 18 Act checks, 44 host checks; exact external inventory; root/runtime scans clean; separate template gates recorded. |
| EXEC-05 isolated capacity baseline | Accepted | Two tenants, 200 additional contacts each plus tasks/opportunities; 32 warm-up reads; 1/5/20 concurrency for 15s each, zero failures; 20 duplicate writes commit/recover once.       |
| EXEC-06 upgrade/recovery/handoff   | Accepted | Cold prior-source upgrade and fresh-target restore; Chromium minimal/full 3/70; eight root gates, 1,458 tests/102 files; template JS 55/guardrails 4/build; handoff and manifest.    |

The host embeds its trusted resolved profile at build time. The generated
`dist/library/release-profile.json` is an inventory; replacing its sidecar does
not change the host's supported implementation. Package content, internal
dependency versions, migration files and authoritative schema ledgers are
validated before CRM startup. This detects incompatible/tampered installations;
it is not a signature/provenance system against an attacker able to replace
the host binary and its trust root.

Package hashes cover implementation, declarations, migrations and semantic
package metadata. npm normalizes JSON whitespace, so JSON formatting is excluded
from the package metadata digest. Tarball byte hashes remain recorded separately.

Reproduce the profile checks with `pnpm test -- packages/config/test/release.test.ts`,
`pnpm reuse:verify`, `pnpm crm:build` and `pnpm crm:verify-runtime` on the pinned
Node/pnpm versions. Runtime verification creates and removes an isolated private
PostgreSQL installation; it does not replace the owner's data or call providers.

The historical baseline profile is `1.3.0` (host `0.6.0`, config `0.5.0`). Initial modules
are mandatory `contacts` (including activities/follow-ups), optional `pipeline`,
optional `assistant`, and `nativeActions` requiring `assistant`. No database data
is dropped. This is a logical composition within a modular monolith: the supported
installation keeps the common artifact closure; selection controls public surfaces
and execution, not uninstalling database ownership. Module metadata and the typed
`planAssembly` API ship in `@archava/config/assembly`. Legacy planning names in
the plan remain unresolved, never implicit installed capabilities.

After `pnpm crm:build`, use `pnpm library:assemble selection.json NEW_DIRECTORY
--dry-run`, then the same command without `--dry-run`. Selection has
`version: "1.0.0"`, the exact `profile`, and `modules` with all four boolean keys
(`contacts` must be true). Generated entries reference installed package exports;
no CRM implementation is copied. Run `npm install --ignore-scripts --no-audit
--no-fund` inside the new private consumer. `pnpm assembly:verify` reproduces the
independent-install, hash, dry-run, dependency and overwrite checks.

For an existing owner host, stop it, place the four-key module selection in a
privately owned mode-0600 file, and run `pnpm crm:modules /absolute/selection.json`.
Restart the host. Old profiles without module selection keep the previous full
behavior. Native disable refuses pending/executing/unknown intents; reconcile or
cancel through the original authorized host first. Startup independently checks
this policy even after manual settings edits. A transient operator connection
only checks unresolved-state existence; request execution still uses the existing
non-owner application role with RLS and current grants. Receipt GET recovery and
pending cancellation remain reconciliation operations, never new execution grants.

The shared `@archava/presence` boundary has strict versioned contracts and a
server-only session gateway. `POST /api/presence/session` accepts only a configured
`tenantId`; identity comes from a verified owner cookie or a server-issued guest
cookie. Owner presence uses current CRM authorization and the offline planner;
guests receive public help only. Neither transport accepts roles, grants, SQL,
approval tokens or execution tools. `/context` accepts an allowlisted section and
a strictly increasing revision; `/turn` binds the current revision, session and
request UUID. Sessions expire within 120 seconds (hard maximum 300); restart
invalidates these ephemeral scopes. Capacity and per-session requests are bounded.
Timeout aborts read work, reports a refusal and keeps the lane busy until late
work settles. Presence creates no new mutation/consent route.

Both public and owner replies use the existing `runTurn` intelligence/policy
implementation. The Python example is a standalone HTTP transport using only its
standard library and public guest scope. It runs from a copied isolated directory
with no owner/database environment. Synthetic transcript evidence is not a live
audio/avatar test. The existing Template LiveKit worker remains an independent
realtime consumer and retains its protocol; connecting its live provider path to
this gateway is a separate gated migration.

`pnpm security:verify` reruns unit, real PostgreSQL CRM/Act/host, reuse and
assembly proofs, then checks the control matrix against their executed labels.
It also scans both the root lock and the installed runtime's npm lock.
The [sanitized matrix](audit/local-library/security.json) records 15 applicable
controls, exact runtime dependencies, licenses/notices and both lock digests.
Each external runtime version is pinned in the host-owned catalogue, installed
using those pins and checked at startup. These pins capture the tested dependency
combination; they are not a vulnerability-driven forced downgrade.

The template's 13 moderate findings derive from
[GHSA-8988-4f7v-96qf](https://github.com/advisories/GHSA-8988-4f7v-96qf):
oversized inbound baggage can allocate excess memory; transport header limits
reduce the HTTP impact but do not prove safety of arbitrary transports. The
owner profile does not install this dependency chain. Template dependency/client
reachability has not been proved; vendor-compatible remediation and its missing
seven Python SDK integration checks remain open realtime release gates. No SDK
downgrade, override or claim of unexploitable code is inferred from scanner output.

External SPDX metadata and shipped notice files are inventoried; generated
`dist/security-proof/third-party-notices.txt` contains their included notices.
Internal artifacts remain private with no declared public license. Public
distribution and client IP terms need their own legal/contract review. This is
scoped engineering evidence, not a penetration test or certification.

`pnpm crm:benchmark` reruns the disposable persistent host fixture and then seeds
200 additional contacts per tenant, 20 tasks and 10 opportunities. It warms up
32 authenticated reads and runs a mixed contacts/worklist/pipeline/overview load
for 15 seconds at each concurrency. Login and fixture creation are outside the
timed workload. Only the fixture's loopback host and socket directory are accepted;
the original database and external providers cannot be benchmark targets.

The [capacity report](audit/local-library/capacity.json) includes runtime/database
versions, machine resources, fixture sizes, samples, throughput, p50/p95, failures
and the separate 20-request mutation/recovery race. The observed read envelope is
for this machine, fixture and eight-connection app pool. It is not a production
SLA or number of supported users.

The six finite items are accepted for local profile `1.3.0`.
[Handoff](LOCAL_LIBRARY_HANDOFF.md) covers cold setup, independent assembly,
account recovery, upgrade and binary/schema compatibility. The
[sanitized manifest](audit/local-library/manifest.json) links the generated
profile, identical-consumer proof, control matrix, capacity, upgrade and browser
results. The final eight root gates passed with 1,458 tests in 102 files; CRM
38, durable Act 18, persistent host 44 and Chromium minimal/full 3/70 passed.
Template regression passed 55 JavaScript tests, four Python guardrail checks
and build; its seven SDK integration checks and live gates remain open.

The existing owner's installation passed `crm:doctor` with the exact supported
profile and two workspaces. Its enabled backup was due while the HTTP host was
stopped; `crm:backup-run` published a new intact private snapshot, retaining all
previous archives. Doctor now reports fresh backup. Tests used synthetic isolated
data; the real owner database was only diagnosed and backed up through its
existing policy. No real record, grant, credential or pricebook was changed.
