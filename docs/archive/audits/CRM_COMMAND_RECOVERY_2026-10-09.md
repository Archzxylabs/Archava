# Reviewed Save recovery — verification, 9 October 2026

Source baseline: `2523094` (durable provider runtime). This incremental change
extends the existing CRM/PG/assistant artifacts to 0.3.0. It introduces no
database migration or external dependency and preserves the original mutation
path, model/tool boundary and existing passing tests.

## Checks

| Command                   | Result                                                                                                             |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| `pnpm verify`             | All eight repository gates; 1,383 tests in 90 files; fourteen compiled internal artifacts                          |
| `pnpm crm:verify`         | 31/31 groups against disposable PostgreSQL and two independent cold-installed consumers                            |
| `pnpm crm:verify-runtime` | 28/28 groups: real credentials, host/database restart, pg_dump/pg_restore and account recovery                     |
| `pnpm e2e:crm-runtime`    | 47/47 browser checks, including response loss/reload, uncertainty, account/workspace isolation and blocked storage |
| Focused ESLint            | Changed implementation and verifier files; zero warnings/errors                                                    |

Node 22.22.3 and pnpm 10.15.1 were used. Artifact builders ran sequentially.
Runtime/browser tests used their own private database/credentials; a network
guard proved zero external provider calls. No new live Gemini evaluation is
claimed by this increment. The earlier thirteen-case result remains historical.

## What the evidence proves

Two separate installed CRM service/repository classes with matching compiled
package hashes retrieve their original human's receipts. Repeated lookup leaves
command, follow-up and mutation-audit counts unchanged. The read survives an
actual PostgreSQL restart and authenticated host restart. After pg_dump/restore
and explicit recovery of the original account, GET retrieves the original UUID
without replaying a mutation.

A command actually blocked by another connection's contact lock is unconfirmed
while running. After the lock releases, that original command commits and the
same GET confirms it, without resubmission. Revoked read grants/membership and
contact reassignment refuse access; another actor/workspace cannot retrieve the
receipt. Service principals and CRM model tools remain excluded.

The browser loses an actual successful Save response, preserves only its UUID,
blocks a different draft's Save and reloads. Receipt lookup then confirms one
task with zero command resubmissions. A missing receipt stays unconfirmed;
malformed/mismatched responses cannot clear its marker. Another workspace/account
does not render or consume the original marker. Storage denial stops the write
before any command request. Prior retry, stale-revision, logout/late-result,
mobile and browser security checks continue to pass.

Machine-readable evidence: [database](audit/2026-10-09-crm-recovery-db-proof.json),
[host](audit/2026-10-09-crm-recovery-host-proof.json),
[browser](audit/2026-10-09-crm-recovery-browser-proof.json),
[installed host](audit/2026-10-09-crm-recovery-installed-proof.json).

The existing loopback installation now serves these compiled 0.3.0 artifacts.
An authenticated read smoke verified absent-receipt behavior without creating
business records or calling a provider; its session was logged out. Before/after
fingerprints match for CRM records/receipts/audit/memberships, identity users,
provider attempts and private configuration. The original local CRM was empty
and remains empty; positive committed-receipt evidence comes from the isolated
real-database fixtures above. Private values were absent from changed tracked
candidates. Existing migrations, identity/provider modules, Gemini planner/data
corpus, archived PRD, pricing/templates, realtime starter and Pending Act files
are byte-unchanged from the source baseline.

## Scope and limits

The host opts into tab-scoped recovery. No transcript/draft is persisted in the
browser, no failed command is inferred from an absent row, and no background
write/retry or uncertainty expiry is added. Closed/cleared tabs and other devices
have no recovery inbox. Persistently unconfirmed outcomes still need authoritative
operator reconciliation; the product offers no automatic bypass.

This is human Save recovery, not machine approval or Act lifecycle delivery.
M3 remains partial. Historical commercial percentages, pricing/templates,
the standalone realtime template and all Pending Act tasks remain unchanged.
Public deployment, broader model quality and durable autonomous actions remain
separate work. See [recovery contract and integration](CRM_COMMAND_RECOVERY.md).
