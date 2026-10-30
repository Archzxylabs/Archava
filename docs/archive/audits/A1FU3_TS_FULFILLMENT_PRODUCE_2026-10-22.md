# A1-FU3 — `fulfillment.produce` was decided after the replay short-circuit

Task: `t_0b7608c5` (follow-up of audit gate `t_be45b375`, finding A1-FU3 (TS), P2, `runs/20261022-045512-audit.json`).
Date: 2026-10-22. Scope: `packages/fulfillment-pg` only — `src/repository.ts`, `test/replay.test.ts`, `test/proof.test.ts`.

## Defect

`PostgresFulfillmentRepository.execute()` decided `record_line` in two places, and the narrower half of the
decision sat **below** the idempotency ledger read:

| Order | Pre-fix site      | Statement                                                                                                   |
| ----- | ----------------- | ----------------------------------------------------------------------------------------------------------- |
| 1     | `:220`            | `grant(access, 'fulfillment.write')`                                                                        |
| 2     | `:222` / `:225`   | `fulfillment.dispatch` for `record_finding`+`delivery` and for `record_custody` with a dispatch-stage event |
| 3     | `:239`            | replay return — `fulfillmentReceiptSchema.parse({ ...existingCmd.receipt, idempotencyReplay: true })`       |
| 4     | `:333` (line arm) | `if (job.kind === 'production') grant(access, 'fulfillment.produce')`                                       |

So a principal that still held `fulfillment.write` but had lost `fulfillment.produce` was answered with the
recorded production receipt on replay — `idempotencyReplay: true`, the `lineId`, the recorded status. The
command it could no longer execute came back as a success. The gate's probe reproduced it exactly: replay
`idempotencyReplay=true`, fresh execution `Missing required permission: fulfillment.produce`.

The pre-existing withdrawal test (`refuses a replay whose grant was withdrawn before it reads the ledger`, the case
the gate cited as `:146`) sets `permissions: []` on a `create_job` command, so it never reaches `record_line` and
never exercises "coarse grant kept, narrow grant withdrawn".

## Decision

One authorization point, above the ledger read, computed from the mutation payload plus exactly one non-lifecycle
column (`archava_fulfillment.jobs.kind`):

```ts
// packages/fulfillment-pg/src/repository.ts:216-237
// Authorization precedes the ledger read: a replay must never be answered
// for a principal that could not have executed the command it replays.
// The payload decides the dispatch grants; the production grant follows
// from the job the payload names, so its kind is read here too. Only the
// kind is read: a legitimate retry must not be held to lifecycle state
// that has moved on since the command was recorded.
grant(access, 'fulfillment.write')
if (mutation.type === 'record_finding' && mutation.category === 'delivery') {
  grant(access, 'fulfillment.dispatch')
}
if (mutation.type === 'record_custody' && DISPATCH_STAGE_EVENTS.has(mutation.event)) {
  grant(access, 'fulfillment.dispatch')
}
if (mutation.type === 'record_line') {
  const jobKind = (
    await client.query<Row>(
      'SELECT kind FROM archava_fulfillment.jobs WHERE tenant_id=$1 AND id=$2',
      [tenant, mutation.jobId],
    )
  ).rows[0]
  if (jobKind?.kind === 'production') grant(access, 'fulfillment.produce')
}
```

The line arm no longer authorizes (`:341-344` is `case`, `loadJob`, `assertOpen` only), so exactly one site decides
this command's permissions.

| Rejected alternative                                      | Why not                                                                                                                                                                          |
| --------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| keep the arm guard and add a hoisted copy as well         | two sources of truth for one decision; the arm copy is dead on the replay path and drifts on the next edit                                                                       |
| hoist `loadJob` + `assertOpen` and reuse the full job row | `assertOpen` reads lifecycle state; a legitimate retry may arrive after the job left `production`, and the replay answer must not depend on state that moved on since the record |
| demand `fulfillment.produce` for every `record_line`      | `produce` guards production only; dispatch lines are recorded under `fulfillment.write` (pinned by test 3)                                                                       |
| derive the permission from `mutation.type` alone          | the type does not identify the permission class; `jobId → kind` is the discriminator the payload already carries, so no wire/JSON change is needed                               |

Edge considered: if the job row were missing, the kind lookup yields nothing and the produce bar would silently
disappear. `archava_fulfillment.jobs` is never deleted on any command path
(`grep -rn 'DELETE FROM archava_fulfillment.jobs' packages/fulfillment-pg/src` → no match), so the lookup cannot
drop the bar for an existing production job, and `test/proof.test.ts` seeds the job row for every mutation that
reaches an arm.

## Behaviour

- Production line replay, `fulfillment.write` kept, `fulfillment.produce` withdrawn → `forbidden`; no receipt field
  in the refusal; no statement against `archava_fulfillment.commands`; the stub throws on any writer, so reaching a
  ledger write would have failed the run.
- Production line replay with both grants → unchanged: recorded receipt with `idempotencyReplay: true`.
- Dispatch line replay with `fulfillment.write` only → unchanged replay.
- Untouched: fresh execution, the two dispatch grants, `create_job`, `record_finding`, `record_custody`,
  `change_job_status`, the `actor_id` predicate on the ledger read. The only added statement is one unlocked
  `SELECT kind FROM archava_fulfillment.jobs WHERE tenant_id=$1 AND id=$2`, on `record_line` only.
- No new refusal shape (`forbidden` reused), no wire change.

## Sweep — the other six `-pg` packages

Each `packages/<domain>-pg/src/repository.ts` `execute()` body, replay return vs. every authorization call
(`grant` / `requireGrant` / `ensureGrant` / `checkGrant`):

| Package               | `execute()` | authorization                                        | replay return | calls after the replay return |
| --------------------- | ----------- | ---------------------------------------------------- | ------------- | ----------------------------- |
| `billing-pg`          | 255-740     | `:267` `COMMAND_PERMISSIONS[mutation.type]`          | `:282`        | 0                             |
| `subscriptions-pg`    | 217-638     | `:229` `COMMAND_PERMISSIONS[mutation.type]`          | `:241`        | 0                             |
| `attendance-pg`       | 154-571     | `:167` `COMMAND_PERMISSIONS[mutation.type]`          | `:182`        | 0                             |
| `laundry-pg`          | 220-620     | `:233` ternary `laundry.process`/`laundry.write`     | `:253`        | 0                             |
| `field-services-pg`   | 201-550     | `:231` `COMMAND_PERMISSIONS[guarded.action]`         | `:247`        | 0                             |
| `formal-education-pg` | 233-620     | `:270` `COMMAND_PERMISSIONS[guarded.action]`         | `:287`        | 0                             |
| `fulfillment-pg`      | 206-638     | `:222` write, `:224`/`:227` dispatch, `:236` produce | `:250-253`    | 0                             |

`field-services` / `formal-education` return through `return <domain>ReceiptSchema.parse({ ...existingCmd.receipt })`,
which carries no literal `idempotencyReplay` on that line; the line numbers above are the return statements
themselves (`repository.ts:247`, `:287`).

Verdict: **fulfillment was the only outlier.** On the pre-fix tree the same sweep reported exactly one
`AFTER-REPLAY` authorization call in the whole tree — fulfillment's `produce` guard (the gate reported it at `:333`;
`:335` in the reconstructed pre-fix ordering, because the A1-FU actor predicate shifted the arm by two lines). The six
packages above
were already hoisted by A1-FU (`t_ce9ad928`), each with a single grant call before its ledger read and none after it.
Their per-command maps are keyed by the command itself, so no payload-dependent narrowing exists to fall below the
replay return.

Sweep tooling: `scan_all_pkgs.py` in the worker scratch cache — regex over `async execute(` … next member, locating
the replay return and any grant-family call after it. Boundary: it judges packages whose `execute()` authorizes
through `grant()`. `foodservice-pg`, `purchasing-pg` and `support-pg` gate through other helpers (no `grant()` inside
their `execute()` bodies) and are outside A1-FU3's named scope, so they are not judged here.

## Tests

`packages/fulfillment-pg/test/replay.test.ts`, same in-memory harness (real repository class, stub pool):

| Test (line)                                                                  | Asserts                                                                                                                                                                                                                                                                    |
| ---------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `refuses a production line replay once the produce grant is withdrawn` (210) | `write` kept, `produce` dropped, job kind `production`, identical payload → refusal `forbidden`, `commandId`/`lineId` absent, kind read is `SELECT kind FROM` with no `status` and no `FOR UPDATE`, no statement against `commands`, no `INSERT`/`UPDATE` reaches the stub |
| `replays a production line for the initiator that still holds produce` (250) | both grants → recorded receipt with `idempotencyReplay: true`                                                                                                                                                                                                              |
| `replays a dispatch line on the write grant…` (263)                          | job kind `dispatch`, `write` only → replay still served, so the new bar is not applied where it does not belong                                                                                                                                                            |

Harness additions: `State.jobKind` (`:72`) and the `SELECT kind FROM archava_fulfillment.jobs` branch (`:98-99`) in
`replay.test.ts`; the same branch in `test/proof.test.ts:135-136`, because `proof.test.ts` drives the real repository
through the full command path and every `record_line` there now performs the kind read.

## Evidence

**Negative control** — the guard was temporarily reverted to the pre-fix ordering (arm guard restored, hoisted block
removed), the new test alone failed and printed the leaked receipt; the fixed file was then restored
(`sha256 ade4042587ffc306ca7d049825d67b034aa10850530fb36334ab74ffc9c84295`):

```
CONTROL_EXIT=1
 ✓ refuses to replay another initiator's receipt, even with the original payload
 ✓ refuses a replay whose grant was withdrawn before it reads the ledger
 ✓ replays the initiator's own command with the recorded receipt
 × refuses a production line replay once the produce grant is withdrawn
 ✓ replays a production line for the initiator that still holds produce
 ✓ replays a dispatch line on the write grant, since produce only guards production
 ✓ refuses a changed payload from the initiator as a fingerprint conflict
 Test Files  1 failed (1)
      Tests  1 failed | 6 passed (7)

AssertionError: expected { …(9) } to be null
+ Received:
{
  "action": "record_line",
  "commandId": "550e8400-e29b-41d4-a716-4466554400c5",
  "executedAt": "2026-10-12T10:30:00.000Z",
  "idempotencyReplay": true,
  "jobId": "770e8400-e29b-41d4-a716-4466554400b5",
  "lineId": "880e8400-e29b-41d4-a716-4466554400d5",
  "revision": 1,
  "status": "consume/normal",
  "tenantId": "tenant-a",
}
```

The two other new tests pass under both orderings, so the regression test bites the defect and nothing else.

**Gate runs on the final tree**

| Gate                                                                                        | Result                                                         |
| ------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| `pnpm exec vitest run packages/billing-pg/test … packages/fulfillment-pg/test` (7 packages) | `GATE1_EXIT=0`, 23 files / **239 tests** (3 added here)        |
| `pnpm format:check`                                                                         | `FORMAT_EXIT=0` — "All matched files use Prettier code style!" |
| `pnpm lint` (`--max-warnings=0`, heap bound inside the script)                              | `LINT_EXIT=0`                                                  |
| `pnpm typecheck`                                                                            | `TYPECHECK_EXIT=0`, 70 `Done` lines                            |
| `TMPDIR=/tmp node tools/library/verify-fulfillment.mjs`                                     | `VERIFY_EXIT=0` — "Fulfillment domain verified with 27 checks" |

## Tree state

Dirty tree, no commit, no `git add`: `src/repository.ts` and `test/proof.test.ts` are modified, and
`test/replay.test.ts` is untracked (created by `t_ce9ad928`), so it does not appear in `git diff`.

## Not claimed

A1's verdict stays with gate `t_be45b375`. This document closes finding A1-FU3 only: the production guard for
`record_line` is now evaluated from the payload before the idempotency ledger read, and both the denying and the
allowing replay paths are covered by tests.
