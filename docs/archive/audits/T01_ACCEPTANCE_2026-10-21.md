# T01 — A1 acceptance evidence (command recovery bound to initiator + current read grant)

Scope audited: A1 of `docs/audit/ARCHLAB_AUDIT_2026-10-21.md` (P1). Task `T01` of
`~/Archverse/Lab/Automation/codex-orchestrator/runs/20261021-162158-plan.json`.

Claim format below is `claim → file:line` plus the command output that measured it.
Line numbers are post-change (the audit report cites pre-change lines).

## 1. Rust HTTP status endpoints now consume the resolved access envelope

`let _ = &access;` no longer exists anywhere under `rust/crates/host/src/erp/`
(`grep -rn "let _ = &access" rust/crates/host/src/erp/ | wc -l` → `0`), and the
envelope is passed into the adapter:

- `rust/crates/host/src/erp/billing.rs:498` — `billing_command_status(connection, query.tenant.as_str(), &access, query.request_id)`
- `rust/crates/host/src/erp/subscriptions.rs:543` — `subscriptions_command_status(... &access ...)`
- `rust/crates/host/src/erp/attendance.rs:532` — `attendance_command_status(... &access ...)`

Enforcement inside the adapters (read permission + initiator binding in the query):

- `rust/crates/adapters/src/erp/billing.rs:996` `grant(access, "billing.read")?;`
  and `:999` `... WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`
- `rust/crates/adapters/src/erp/subscriptions.rs:870` `require(access, "subscriptions.read")?;`
  and `:873` `... WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`
- `rust/crates/adapters/src/erp/attendance.rs:821` `require(access, "attendance.read")?;`
  and `:824` `... WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`

Result: missing/revoked read grant → the domain's existing closed refusal (HTTP 403
`forbidden`); a grant holder that is not the recorded initiator → the plain
`{"status":"unknown"}` document, no `receipt` field; the initiator → `recorded`.

## 2. Seven TypeScript repositories, same contract

Every `getCommandStatus` calls its domain read grant and binds the recorded
initiator into the lookup key itself (`actor_id=$3` / `created_by=$3`, params
`[tenant, commandId, access.ownerId]`), so no foreign row ever leaves the
database:

| domain           | grant                                                                                   | binding                                                                      |
| ---------------- | --------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| billing          | `packages/billing-pg/src/repository.ts:706` `grant(access, 'billing.read')`             | `:709` `... AND actor_id=$3`, params `[tenant, commandId, access.ownerId]`   |
| subscriptions    | `packages/subscriptions-pg/src/repository.ts:621` `grant(access, 'subscriptions.read')` | `:624` `... AND actor_id=$3`, params `[tenant, commandId, access.ownerId]`   |
| attendance       | `packages/attendance-pg/src/repository.ts:537` `grant(access, 'attendance.read')`       | `:540` `... AND actor_id=$3`, params `[tenant, commandId, access.ownerId]`   |
| laundry          | `packages/laundry-pg/src/repository.ts:593` `grant(access, 'laundry.read')`             | `:596` `... AND actor_id=$3`, params `[tenant, commandId, access.ownerId]`   |
| fulfillment      | `packages/fulfillment-pg/src/repository.ts:618` `grant(access, 'fulfillment.read')`     | `:621` `... AND actor_id=$3`, params `[tenant, commandId, access.ownerId]`   |
| field-services   | `packages/field-services-pg/src/repository.ts:533` `grant(access, 'field.read')`        | `:542` `... AND created_by=$3`, params `[tenant, commandId, access.ownerId]` |
| formal-education | `packages/formal-education-pg/src/repository.ts:603` `grant(access, 'education.read')`  | `:612` `... AND created_by=$3`, params `[tenant, commandId, access.ownerId]` |

> **field-services / formal-education keying (archlab, task t_34fb0e3b).** Both
> packages previously read on `WHERE tenant_id=$1 AND command_id=$2` and rejected a
> foreign creator in JavaScript (`row.created_by !== access.ownerId`) — a
> membership-visible read that leaves the row in the process. The creator is now part
> of the statement's key (`created_by=$3`, params `[tenant, commandId, access.ownerId]`),
> and `created_by` is no longer selected.
>
> Test evidence — `packages/{field-services,formal-education}-pg/test/proof.test.ts`
> stub guards assert the predicate itself (`created_by=$3`) and bind the row on
> `params[2]`, and each package carries a six-arm matrix: own recovery (whole
> document), foreign creator that holds the read grant, cross-tenant key, key never
> recorded, active member without the grant (forbidden), and grant revoked after a
> successful recovery (forbidden). Mutation-checked: dropping the predicate, or
> dropping the `grant(...)` call, each fails the suite.

> **Correction (archlab, audit round 3 P3).** The `billing`/`subscriptions`/
> `attendance`/`laundry`/`fulfillment` right-hand cells above previously read
> `row.actor_id !== access.ownerId`, i.e. a JavaScript post-filter over a row the
> statement had already fetched. That was wrong: the shipped code never selects
> `actor_id`, and the initiator is part of the SQL predicate
> (`WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`, params
> `[tenant, commandId, access.ownerId]`). The cells now cite the predicate line,
> matching `grep -n "actor_id=\$3" packages/*-pg/src/repository.ts`.

Five use `request_id + actor_id`; field-services and formal-education use
`command_id + created_by`; the subs/billing/attendance `actor_id` is the principal
that recorded the command (`access.ownerId`, so delegated-human writes stay owned by
the delegating principal).

## 3. Tests that previously encoded membership-only / cross-actor recovery — replaced

- `rust/crates/host/tests/erp_attendance.rs` — `narrow_attendance_grants_refuse_exactly_the_arms_they_lack`
  asserted an audit-only principal gets `200` on the status read; now asserts `403`,
  and `revoked_attendance_grant_refused_on_recovery` proves full-grant non-initiator
  `ax` → `{"status":"unknown"}` while initiator `axv` → `recorded`, then walks three
  closure arms on the same key: (a) only `attendance.read` withdrawn while the seat
  stays `active` (read back from the row and asserted) → `403 forbidden`, while the
  byte-identical write replay still answers `idempotencyReplay: true` under the
  untouched write permission; (b) grant restored → `recorded` again; (c) the
  membership itself deactivated (`active=false`) → `403 forbidden`, then restored.
  The read path therefore re-answers the ladder under CURRENT grants, and a dead
  seat keeps its own closure.
- `rust/crates/host/tests/erp_billing.rs` — `insufficient_grants_refuse_before_any_statement`
  now asserts a read+audit principal that is not the initiator (`bxg`) receives
  `unknown` with no receipt fields, while `bx` recovers `recorded` (the old
  "billing viewer recovers the manager's receipt" proof is gone).
- `rust/crates/host/tests/erp_subscriptions.rs` — `narrow_grants_refuse_exactly_the_arms_they_lack`
  carries the same proof (`sxg` → `unknown`, `sx` → `recorded`).
- `tools/library/verify-{billing,laundry,fulfillment}.mjs` — dropped the cross-actor
  recovery assertion (`viewer` reading `clerk`/`counter`/`owner` command → `recorded`);
  now asserts non-initiator grant holder → `unknown` with no `receipt`, initiator → `recorded`.

New TS matrix files: `packages/{billing,laundry,field-services,formal-education,fulfillment}-pg/test/proof.test.ts`
(updated) and `packages/{subscriptions,attendance}-pg/test/status.test.ts` (new). Each
matrix covers: no grant → closed refusal, revoked grant → refusal, active member
without grant → refusal, foreign actor with grant → `unknown` without receipt fields,
cross-tenant key → `unknown`/refusal, missing key → `unknown`, own recovery → `recorded`.

## 4. Measured acceptance output

### #1 vitest, 7 domain packages — exit 0 (`/tmp/t01-gate1-r4.log`)

```
START=2026-10-21T21:53:38+07:00
 Test Files  12 passed (12)
      Tests  168 passed (168)
   Start at  21:53:40
   Duration  1.10s (transform 5.53s, setup 0ms, import 8.00s, tests 482ms, environment 3ms)
VITEST_EXIT=0
END=2026-10-21T21:53:41+07:00
```

### #2 cargo, 3 ERP HTTP suites, `--test-threads=1` — exit 0 (`/tmp/t01-gate2-final.log`)

```
GIT_HEAD=412f763e17f3a68a457fd76c7b929184c50c916a
SCOPE_HASH_BEFORE=32eead580947276244892f1c7bce0a8a4b38601628ebf09bd1492e2162678c5f
     Running tests/erp_attendance.rs
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 991.91s
     Running tests/erp_billing.rs
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 685.36s
     Running tests/erp_subscriptions.rs
test result: ok. 13 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 743.52s
EXIT=0
SCOPE_HASH_AFTER=32eead580947276244892f1c7bce0a8a4b38601628ebf09bd1492e2162678c5f
```

Run window 21:08:11 → 21:48:33, taken from `stat -c '%y'` on
`/tmp/t01-gate2-final.scope-before` and on the log itself: that wrapper did not stamp `START`/`END`
into the log, so no timestamp is quoted that the command did not print.
`SCOPE_HASH_BEFORE`/`AFTER` are `{ git status --porcelain; git diff; } | sha256sum`. An earlier
round-3 run (`/tmp/t01-gate2.log`, `e22be7ed…`, 893.52 / 733.97 / 674.39s) was measured before the
round-4 edits and is superseded by the numbers above.

40 tests total (14 attendance + 13 billing + 13 subscriptions), all green, `EXIT=0`. The source scope hash is
identical before and after the run, so the suites were executed against exactly the tree this document
describes. Cargo runs the three test binaries in alphabetical target order
(`erp_attendance` → `erp_billing` → `erp_subscriptions`), not in the order the `--test` flags are written.

One `cargo test` invocation, one process, 40 tests, `EXIT=0`: counts are attributed to the binary whose
`Running tests/...` line precedes them (attendance 14, billing 13, subscriptions 13); an earlier revision
of this file printed those three counts against the wrong suite names.

### #3 `pnpm library:pack` + 7 × `verify-<domain>.mjs` — exit 0 (`/tmp/t01-gate3-r4.log`)

```
START=2026-10-21T21:53:41+07:00
PACK_EXIT=0
Billing domain verified with 30 checks
All 19 synthetic verification checks passed successfully.        # subscriptions
All 18 attendance proof checks passed.
Laundry domain verified with 30 checks
Field services domain verified with 32 checks
22 formal education checks passed
Fulfillment domain verified with 27 checks
LOOP_EXIT=0
END=2026-10-21T21:54:55+07:00
```

178 `PASS` lines total (`grep -cE "^PASS"`): 30 + 19 + 18 + 30 + 32 + 22 + 27 — subscriptions gained the
`command status recovery requires the current read grant and the initiating actor` check
(own recovery, foreign initiator with the grant, missing key, cross-tenant key, missing
grant, revoked grant with the seat still active).

### Round-4 re-run — all three gates, final tree

The round-4 edits — `tools/library/verify-subscriptions.mjs`,
`rust/crates/host/tests/erp_attendance.rs`, this document — were gated by re-executing all three
commands on the tree those edits produced. Full-tree scope
(`{ git status --porcelain; git diff; } | sha256sum`) was `32eead58…` before and after gate #2 and again
after gates #1/#3; the source-only scope (`git status --porcelain -- rust packages tools package.json
pnpm-lock.yaml vitest.config.ts` plus the matching `git diff`) was `69ee2d54…`, identical across those runs.

- **#1** `pnpm exec vitest run packages/{billing,subscriptions,attendance,laundry,field-services,formal-education,fulfillment}-pg/test`
  → `Test Files 12 passed (12)`, `Tests 168 passed (168)`, `VITEST_EXIT=0`,
  `START=21:53:38`, `END=21:53:41`, log `/tmp/t01-gate1-r4.log`.
- **#2** `TMPDIR=/tmp cargo test --offline --locked --manifest-path rust/Cargo.toml -p archava-host
--test erp_billing --test erp_subscriptions --test erp_attendance -- --test-threads=1`
  → one invocation, 40 tests green (`erp_attendance` 14 / 991.91s, `erp_billing` 13 / 685.36s,
  `erp_subscriptions` 13 / 743.52s), `EXIT=0`,
  `SCOPE_HASH_BEFORE == SCOPE_HASH_AFTER == 32eead58…`, log `/tmp/t01-gate2-final.log`, hashes in
  `/tmp/t01-gate2-final.scope-before|after`.
- **#3** `pnpm library:pack && for d in billing subscriptions attendance laundry field-services formal-education fulfillment; do TMPDIR=/tmp npm_config_offline=true node tools/library/verify-$d.mjs || exit 1; done`
  → `PACK_EXIT=0`, `LOOP_EXIT=0`, 178 `PASS` lines (30+19+18+30+32+22+27),
  `START=21:53:41`, `END=21:54:55`, log `/tmp/t01-gate3-r4.log`.

## 5. Notes for the auditor

- The repo working tree already contained ~45 unrelated dirty entries before T01; they
  were preserved untouched (`git status --porcelain` evidence). T01 touches only the
  files listed there under `rust/crates/host`, `rust/crates/adapters`, `packages/*-pg`
  and `tools/library`.
- `erp_*` suites run ≈1.4 min/test (scrypt + real Postgres fixture); acceptance #2 was
  executed as a detached systemd unit so it could not be killed by the worker's
  50-iteration budget, and its log is the artifact quoted above.
- No wire schema changed: `unknown` is the domain's pre-existing status document, and
  receipt fields are simply absent for a foreign initiator.

---

## Addendum 2 — post-audit hardening (A1 audit: 2 x P2, 2 x P3)

Fresh audit verdict before this round: `changes_requested` with P2 x2 + P3 x2. Each item and its
closing proof, measured this round.

### P2-1 — HTTP endpoints did not refuse an active member without a read grant, nor a revoked grant

`rust/crates/host/tests/erp_billing.rs:1901-1966` (in `insufficient_grants_refuse_before_any_statement`):
`bxn` — **active** membership, grants `audit.read` only — is refused `403 FORBIDDEN` on
`GET /erp/billing/commands/status` (`:1941` `"an audit-only member may not read a command"`), then
`bxg`'s read grant is revoked **while the membership stays active** (`UPDATE ... permissions=$2` where
`$2 = ["audit.read"]`) and the same route answers `403` (`:1964` `"a revoked read grant refuses
recovery"`). Same two proofs for subscriptions in
`rust/crates/host/tests/erp_subscriptions.rs:1240-1276` (`sxn` -> 403, `sxg` revoked -> 403),
inserted after the positive list read so the principal is not reused later.

Coverage now in all three directions at the HTTP layer: missing grant (403), revoked grant with a live
seat (403), foreign actor holding the grant (bare `{"status":"unknown"}`, no receipt).

### P2-2 — unit-test stubs ignored the tenant of the key; three verify scripts had no negative case

Unit stubs (the receipt lookup used to return the row no matter which ledger asked):

| package          | stub guard                                                               | foreign-actor assertion                  | cross-tenant assertion | revoked-read assertion                               |
| ---------------- | ------------------------------------------------------------------------ | ---------------------------------------- | ---------------------- | ---------------------------------------------------- |
| billing          | `proof.test.ts` — `String(query.params[0]) === tenant ? ... : undefined` | `:523-527` `toEqual({status:'unknown'})` | `:529-532`             | `state.permissions = ['billing.write']` -> forbidden |
| subscriptions    | `status.test.ts` `tenantId` in state                                     | `:78-82`                                 | `:84-87`               | `:92`                                                |
| attendance       | `status.test.ts` `tenantId` in state                                     | `:79-83`                                 | `:85-88`               | `:93`                                                |
| laundry          | `proof.test.ts` stub guard                                               | `:688-693`                               | `:696-699`             | `:703`                                               |
| fulfillment      | `proof.test.ts` stub guard                                               | `:772-777`                               | `:780-784`             | `:788`                                               |
| field-services   | `proof.test.ts` stub guard (`created_by` binding)                        | `:709-718` full document                 | `:719-726`             | `:730`                                               |
| formal-education | `proof.test.ts` stub guard (`created_by` binding)                        | `:712-722`                               | `:723-731`             | `:735`                                               |

The foreign-actor assertions compare the **whole document** (`toEqual`), so a single leaked `receipt`,
`subscriptionId`, `sessionId`, `jobId` or `enrollmentId` field fails the test rather than passing on a
`status` check alone. Every matrix also asserts `state.active === true` before the read grant is
withdrawn, so "revoked grant" cannot be satisfied by an inactive seat.

Verify scripts, now carrying the negative cases against the real Postgres adapter:

- `tools/library/verify-attendance.mjs` — a `attendance.read` holder that is not the initiator gets a
  bare `{ status: 'unknown' }`; then its read arm is withdrawn via `array_remove` (seat still active)
  -> `forbidden`; restored -> the route reopens.
- `tools/library/verify-field-services.mjs` — `viewer` (`field.read`) reading `owner`'s recorded
  command -> `status: 'unknown'`, `recordedAt: null`, no `receipt` key; `inactive` seat -> `forbidden`;
  `foreigner` (other tenant) -> `forbidden`.
- `tools/library/verify-formal-education.mjs` — `reviewer` (`education.read`) reading a key it did not
  record -> `status: 'unknown'`, no `receipt`; `inactive` and `foreigner` -> `forbidden`.
- `verify-{billing,laundry,fulfillment}.mjs` were rewritten earlier in this card: initiator ->
  `recorded`, grant-holder non-initiator -> `unknown` without receipt fields.

Revoked-grant recovery is asserted against the real adapters in all seven unit matrices and at the HTTP
layer in the two Rust suites above; the three verify scripts above assert foreign-reader, inactive-seat
and cross-tenant refusals.

### P3-1 / P3-2 — stale "membership-only" wording and unsupported evidence claims

Every test comment that described the old contract was replaced while writing the matrix above: the
docstrings now say recovery needs the **current** read grant **and** the recorded initiator
(`rust/crates/host/tests/erp_billing.rs:1852-1856`). No comment, docstring or verify-script message in
this change still describes membership-only or cross-actor recovery as permitted behaviour.

Evidence in this document is limited to commands run in this working tree; every claim above carries
the file:line where the assertion lives, and the raw logs are cited by path, so a reviewer can re-run
`grep -n` on any line number and land on the assertion.

## Commands and raw results (round 4, final tree)

```
pnpm exec vitest run packages/{billing,subscriptions,attendance,laundry,field-services,formal-education,fulfillment}-pg/test
  -> 12 files, 168 passed, VITEST_EXIT=0              (/tmp/t01-gate1-r4.log)     21:53:38 -> 21:53:41

pnpm library:pack                                     -> PACK_EXIT=0  (/tmp/t01-gate3-r4.log)
for d in billing subscriptions attendance laundry field-services formal-education fulfillment; do
  TMPDIR=/tmp npm_config_offline=true node tools/library/verify-$d.mjs || exit 1; done
  -> 178 PASS lines (30+19+18+30+32+22+27), LOOP_EXIT=0  (/tmp/t01-gate3-r4.log)  21:53:41 -> 21:54:55

TMPDIR=/tmp cargo test --offline --locked --manifest-path rust/Cargo.toml -p archava-host \
  --test erp_billing --test erp_subscriptions --test erp_attendance -- --test-threads=1
  -> EXIT=0, 40 tests: attendance 14, billing 13, subscriptions 13   (/tmp/t01-gate2-final.log)
     single invocation, SCOPE_HASH_BEFORE == SCOPE_HASH_AFTER = 32eead58…  21:08:11 -> 21:48:33
```

## Audit verdict (post-round-4)

`orchestrate.sh --audit` (Codex, read-only sandbox) returned **`approve`** with zero findings on the
tree above — `runs/20261021-215656-audit.json`, `AUDIT_RC=0`. Summary as returned:

> All three prior findings are closed in the inspected code, tests, and acceptance table. Recovery
> checks current read grants and recorded initiator identity across all ten implementations. Raw logs
> confirm 168 unit tests, 40 HTTP tests, and 178 packaged-proof checks passed; current scope hashes
> match the recorded values.

This section was appended after the verdict was issued; it changes no measured content above.
