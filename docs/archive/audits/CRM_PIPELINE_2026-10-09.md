# CRM sales pipeline verification — 9 October 2026

Baseline: local `main` checkpoint `bddb4fe`, preserving the contacts, identity and
read/draft conversation slices. This checkpoint adds a small optional CRM domain
extension through public subpaths, with no client fork or third-party dependency.
See [scope, API and upgrade procedure](CRM_PIPELINE.md).

## Evidence

| Check                                  | Result                                                                                                                                                        |
| -------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm verify`                          | All eight gates passed; 1,339 Vitest tests across 87 files.                                                                                                   |
| Independent core reuse, included above | Cold-installed hospitality/retail consumers use matching core artifact versions and implementation hashes.                                                    |
| `pnpm crm:verify`                      | 19 real PostgreSQL groups; two independently installed hospitality/professional-service consumers use matching CRM/PG/conversation artifacts.                 |
| `pnpm crm:verify-runtime -- --browser` | 18 credential/session/CSRF/operator/restart/backup/restore groups passed before holding the private browser fixture.                                          |
| `pnpm e2e:crm-runtime`                 | 31 browser checks; pipeline contact selection, stages, history, unknown-response retry, labels, tenant/logout cleanup and assigned staff scope.               |
| Protected bytes                        | Original CRM migration, original PRD archive, pricing/template config, config module registry, standalone template and Pending Act readiness files unchanged. |
| Runtime                                | Node 22.22.3; real private PostgreSQL, with TCP disabled.                                                                                                     |
| Model/provider calls                   | Zero. No message send, booking, payment or project execution.                                                                                                 |

Machine-readable checks and installed artifact versions/hashes are in
[pipeline proof](audit/2026-10-09-crm-pipeline-proof.json). The artifact closure
contains 13 internal packages. CRM/PG are 0.2.0; the conversation adapter remains
0.1.0 with its existing bounded contact/follow-up intents. Builders ran sequentially
because they share `dist/library`. No remote CI run, package publication or public
deployment was performed. The standalone template suite was not rerun; its bytes
were checked unchanged.

## What the tests establish

The same pipeline service works for a group-stay inquiry and a consulting engagement.
Separate tenant data and configured labels use identical code. This establishes a
shared opportunity workflow, not a complete hospitality or project-management kit.

Concurrent retries produce one opportunity, initial history entry and audit event.
Competing stage moves have one winner. Invalid stage jumps, missing loss reasons,
terminal reopening, stale revisions, changed retry intent and cross-actor replay
are refused. Separate grants, contact authorization, assigned scope, machine-write
refusal and service/human grant intersection remain enforced. Losing an HTTP
response after commit retains the browser's request UUID; retry adds no second event.
Concurrent detail reads return a stage and latest history revision that agree.

Audit failure rolls back the opportunity, history and command identity. Tenant RLS
holds without a scope GUC, history refuses update/delete, and lists are bounded with
escaped hostile search. A real 001-only database upgrades to 002 without changing
existing memberships, contacts, activities, follow-ups, receipts or audit. Modified
migration history is refused. Restart and actual `pg_dump`/`pg_restore` retain the
opportunity, history and retry receipt. Runtime setup preserves a deliberate grant
revocation until the operator explicitly changes it.

## Existing local installation

The operator's old installation was backed up before its host stopped. A private
pre-upgrade snapshot captured record counts and hashes, account hashes and key
fingerprints without printing records or credentials. After migration, comparison
confirmed every existing CRM record, membership, account and secret unchanged.
Only then was pipeline access explicitly granted to the existing initial owner;
other accounts were not automatically expanded by migration/setup.

The persistent host was restarted at `http://127.0.0.1:4175`; its health endpoint
returned 200 and its served page includes the pipeline surface. No test opportunity
was created in the operator's database. Regression fixtures used disposable private
clusters and were removed after verification. Backup and fingerprint evidence stay
in ignored private `.local/crm` state.

## Remaining boundaries

Opportunity reassignment, custom stage graphs, forecasting, amounts, native
pipeline conversation intents and terminal reopening are outside this slice.
Live planner evaluation, approved durable Act writes, proposal/signature/payment,
project capacity, public identity/deployment, telemetry and production support
remain separate gates. The three Act operational-readiness tasks remain Pending.
M3/M4 completion and the historical commercial score are not inferred from these
test counts.
