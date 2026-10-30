# T01-FU — A1 acceptance evidence (capacity + proposals command recovery)

Scope: out-of-scope A1 finding reported by T01 (`t_8e364497`, worker `t_daf3380e`), executed
as card `t_26f873d5`. Same bug class as A1 in `docs/audit/ARCHLAB_AUDIT_2026-10-21.md`: a
receipt lookup that was keyed on the command only, so any holder of the domain read grant
could recover another initiator's receipt.

Contract applied (decided in T01, no new variant): the resolved access envelope is consumed
by `commandStatus` (no second permission resolution), the domain read grant is enforced
_before_ the query, the recorded actor is a predicate of the statement
(`... AND actor_id=$3`, params `[tenant, commandId, access.ownerId]`), `scope === 'all'`
never bypasses that predicate, and the wire answers stay canonical: foreign/missing key →
`unknown` with no `receipt` field, missing/revoked read grant → domain `forbidden` with no
`receipt` field.

## 1. Sources

| domain    | read grant enforced first                                                        | lookup bound to the recorded actor                                                                                                                           |
| --------- | -------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| capacity  | `packages/capacity-pg/src/repository.ts:910` `grant(access, 'capacity.read')`    | `:921` `SELECT receipt FROM archava_capacity.commands WHERE tenant_id=$1 AND command_id=$2 AND actor_id=$3` — params `[tenant, commandId, access.ownerId]`   |
| proposals | `packages/proposals-pg/src/repository.ts:1405` `grant(access, 'proposals.read')` | `:1416` `SELECT receipt FROM archava_proposals.commands WHERE tenant_id=$1 AND command_id=$2 AND actor_id=$3` — params `[tenant, commandId, access.ownerId]` |

Both methods are the existing `commandStatus(tenant, principal, commandId)` entry points; the
envelope comes from the repository's single `transaction()` resolution, so no permission is
resolved twice inside the read.

Removed behaviour, and why:

- `SELECT receipt, actor_id ... WHERE tenant_id=$1 AND command_id=$2` — the actor column is no
  longer selected, so a foreign row can never reach the process at all.
- `if (access.scope === 'assigned') { if (row.actor_id !== principal.id) throw forbidden ... }`
  — the JavaScript post-fetch identity check is gone, and with it the `scope === 'all'` hole
  that skipped it entirely (any tenant-wide reader could recover the initiator's receipt).
- The follow-on `pools.owner_id` / `proposals.owner_id` re-check inside the recovery path is
  gone with it: the actor predicate already limits recovery to the initiator's own command,
  and the canonical contract (`rust/crates/host/src/router.rs status_receipt()` and the 10 T01
  implementations) authorizes recovery on read grant + initiator only.

> **Design decision (contract item 4).** No tenant-wide "read someone else's receipt" path is
> enabled here. `scope === 'all'` widens list/detail visibility, not command recovery. If a
> domain later needs an auditor to read another initiator's receipt, that is a separate
> design decision with its own grant and its own card; it is deliberately not switched on by
> this change and is therefore not silently reachable through `capacity.read` /
> `proposals.read`.

## 2. Unit matrix — gate #1

New suites (`packages/{capacity,proposals}-pg/test/` did not exist before this card):

| arm                                                                                        | capacity                                                                         | proposals                                       |
| ------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------- | ----------------------------------------------- |
| own recovery returns the whole recorded document                                           | `packages/capacity-pg/test/status.test.ts:157`                                   | `packages/proposals-pg/test/status.test.ts:168` |
| foreign actor **with** the read grant (tenant-wide `scope: 'all'`) → `unknown`, no receipt | `:164`                                                                           | `:176`                                          |
| foreign actor with the grant (`assigned` peer, write arms) → `unknown`                     | `:169`                                                                           | `:181`                                          |
| cross-tenant key → `unknown`                                                               | `:173`                                                                           | `:185`                                          |
| key never recorded → `unknown`                                                             | `:177`                                                                           | `:189`                                          |
| actor predicate is in the statement, not a post-fetch filter                               | `:195` `toContain('actor_id=$3')`, `:196` params `[tenant, COMMAND_ID, 'owner']` | `:206`, `:207`                                  |
| active member without the read grant → domain `forbidden`, no receipt field                | `:226`                                                                           | `:237`                                          |
| read grant withdrawn after a successful recovery → `forbidden`, no receipt field           | `:216`                                                                           | `:227`                                          |

The stub pool answers by emulating the database rather than the query shape: a lookup that
carries the actor as a parameter can only find that actor's row
(`packages/*-pg/test/stub-pool.ts`, `status.test.ts` `FROM archava_*.commands` branch).

Red-first evidence (pre-fix wide select restored, then reverted):
`Tests 4 failed | 2 passed (6)`, the failing assertion being
`expected { status: 'committed', receipt: {...} } to deeply equal { status: 'unknown' }` plus
`expected 'SELECT receipt, actor_id FROM archava…' to contain 'actor_id=$3'`.

Green, final tree (`/tmp/t01fu-gate1-vitest.log`):

```
✓ packages/proposals-pg/test/status.test.ts (3 tests)
✓ packages/capacity-pg/test/status.test.ts (3 tests)
Test Files  2 passed (2)
     Tests  6 passed (6)
GATE1_EXIT=0
```

Note: the matrix is six behavioural arms per domain across three `it` blocks (the last block
carries both `forbidden` arms: never-granted and revoked-after-success).

Re-run on the final tree after this doc was written (`/tmp/t26-*.log`, 22:26-22:28):
`Test Files 2 passed (2) / Tests 6 passed (6)` exit 0; pack exit 0; `verify-capacity`
31/31 checks exit 0; `verify-proposals` 32/32 checks exit 0; `pnpm typecheck` exit 0 with
zero `error TS` lines.

## 3. Library verification — gate #2

`tools/library/verify-capacity.mjs` and `tools/library/verify-proposals.mjs` previously proved
only own-recovery, missing-key and zero-grant recovery. They now also prove the A1 binding
against a real PostgreSQL cluster:

- membership seed gains a tenant-wide reader with only the read arm —
  `verify-capacity.mjs:175` `['agency', 'auditor', 'human', ['capacity.read'], 'all']`,
  `verify-proposals.mjs:177` `['agency', 'auditor', 'human', ['proposals.read'], 'all']`;
- `verify-capacity.mjs:303-338`: auditor (`all` scope) → `unknown` with no `receipt` key,
  `planner` (assigned peer holding `capacity.read`) → `unknown`, `epc` owner with the same
  command id → `unknown`, then the initiator's own key after withdrawing **only** the read arm
  → `forbidden` with no `receipt` key, then the grant is restored;
- `verify-proposals.mjs:337-383`: same arms via `client-reviewer` (`proposals.read`, `all`)
  and `lead-pm` (assigned peer), `epc` cross-tenant key, and the initiator after withdrawing
  only `proposals.read`.

Red/green proof that the new verifier arms are not vacuous (`/tmp/t01fu-redgreen/`):

| phase | sources                                                                                                               | pack   | capacity verify | proposals verify |
| ----- | --------------------------------------------------------------------------------------------------------------------- | ------ | --------------- | ---------------- |
| red   | `git show HEAD:packages/{capacity,proposals}-pg/src/repository.ts` (pre-fix wide select, `AND actor_id=$3` count = 0) | exit 0 | **exit 1**      | **exit 1**       |
| green | hardened working tree (`AND actor_id=$3` count = 1 per file)                                                          | exit 0 | exit 0          | exit 0           |

The red failure is exactly the A1 bug: `actual: { status: 'committed', receipt: { … poolId /
proposalId / committedAt … } }` where the verifier requires `{ status: 'unknown' }`
(failing check: `receipt recovery is bound to the initiating actor …`). After the run the
sources were restored byte-identically (`cmp` against the hardened copies: identical) and the
pack was rebuilt before the final gate.

Final literal gate — `pnpm library:pack && for d in capacity proposals; do
TMPDIR=/tmp npm_config_offline=true node tools/library/verify-$d.mjs || exit 1; done`
(`/tmp/t01fu-gate2-final.log`):

```
PASS receipt recovery is bound to the initiating actor: tenant-wide, assigned-peer and cross-tenant keys answer unknown without a receipt
PASS receipt recovery re-checks the read grant on every call: a withdrawn read arm is forbidden with no receipt
ALL 31 CAPACITY ESTIMATION & DELIVERY CHECKS PASSED PERFECTLY!
ALL 32 PROPOSALS, AGREED SCOPE & AMENDMENT CHECKS PASSED PERFECTLY!
GATE2_FINAL_EXIT=0
```

## 4. Typecheck — gate #3

`pnpm typecheck` (`/tmp/t01fu-gate3-typecheck.log`) → `TYPECHECK_EXIT=0`, including
`packages/capacity-pg typecheck: Done`, `packages/proposals-pg typecheck: Done` and the
`@archava/web` `tsc --noEmit` pass. No `error TS` line in the log.

## 5. Audit — gate #4

`orchestrate.sh --audit` on this tree:
`~/Archverse/Lab/Automation/codex-orchestrator/runs/20261021-222023-audit.json`
(run stamp 22:20:23 - after the 22:11 source edits and the green gate logs):

```json
{ "verdict": "approve", "findings": [] }
```

Auditor summary: both repositories enforce the read grant before querying and bind
tenant + command + actor without a scope bypass (`capacity-pg/src/repository.ts:910-922`,
`proposals-pg/src/repository.ts:1405-1417`); the Vitest matrices and the PostgreSQL verifier
assertions were inspected; red failures, green pack/verifiers and full typecheck were
substantiated from the saved logs. Zero findings, nothing to change.

## 6. Working tree

85 dirty entries measured at completion (78 pre-existing from before T01 plus T01's own changes and this
card's two new `test/` directories and two edited verifiers). Zero reverts, zero cleans; the
single pre-existing deletion (`GEMINI_IMPLEMENTATION_TASK.md`) is untouched. No `cargo test`
suite was re-run for this card (irrelevant to the TS-only change, per card instructions).
