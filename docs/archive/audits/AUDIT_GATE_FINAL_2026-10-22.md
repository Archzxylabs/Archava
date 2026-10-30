# FINAL GATE EVIDENCE PACK — A1 (command-recovery authorization) + A10 (assigned scope) + root gates

> **STATUS: EVIDENCE, NOT A VERDICT.** This pack is the input to the Codex audit of
> card `t_be45b375`. It supersedes `docs/audit/AUDIT_GATE_2026-10-22.md`, whose §3 trio
> evidence was stale by construction. Every number below was produced on the frozen tree
> described in §1; raw logs are quoted, not summarised from memory.

- Date: 2026-10-22 (Asia/Jakarta), gate run window 02:13 – 03:00
- Repo: `/home/haikaru/Archverse/Lab/Archava_Platform`
- Base commit: `412f763e17f3a68a457fd76c7b929184c50c916a` (all remediation is uncommitted — dirty-tree policy applies)
- Gate card: `t_be45b375` — last gate before A1/A10 may be called closed
- Objective given to the executor: close **A1** and **A10** from
  `docs/audit/ARCHLAB_AUDIT_2026-10-21.md`, then restore passing `format:check` and `lint`.

## 1. Tree under audit (frozen, hash-bound)

```
git rev-parse HEAD        → 412f763e17f3a68a457fd76c7b929184c50c916a
git status --porcelain | wc -l → 206 entries
git diff --stat           → 168 files changed, 4240 insertions(+), 2544 deletions(-)
```

The verdict binds to a **scope hash**, not to a commit, because the whole remediation
program is uncommitted. The hashes below were recorded by the trio wrapper _inside_ the
run (`/tmp/audit-trio-final.log`, copied to `docs/audit/evidence/audit-trio-final.log`):

| hash                                                                                                                                                                                                     | value                                                                           |
| -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| `SCOPE_HASH_BEFORE` (02:13:16)                                                                                                                                                                           | `1ae4d421f8b539a1e726e6219cb404e5860e2690d99d9601b270d9dbc23b76bd`              |
| `SCOPE_HASH_AFTER` (02:47:43)                                                                                                                                                                            | `1ae4d421f8b539a1e726e6219cb404e5860e2690d99d9601b270d9dbc23b76bd`              |
| `RUST_SCOPE_BEFORE`                                                                                                                                                                                      | `dab512f680e037b5f100ff19e7968c39e268d5bc19ab3531af27d48d49f685c0`              |
| `RUST_SCOPE_AFTER`                                                                                                                                                                                       | `dab512f680e037b5f100ff19e7968c39e268d5bc19ab3531af27d48d49f685c0`              |
| source scope, independently measured before/after (`git status` + `git diff --binary` + `git ls-files -s` over `rust packages apps tools package.json pnpm-lock.yaml eslint.config.mjs .prettierignore`) | `0b28a99954e999e01d4203b8f6c106f978414ba972c578bddf5e68099f8c0248` (both sides) |

Both wrapper hashes and the independent source hash are **identical before and after** the
34.6-minute run, so the trio certifies exactly the tree an auditor inspects. The only files
added after the run are this pack and the raw trio log (`docs/audit/evidence/`) — declared
in §6; no source file changed.

Rust changes in the tree are exactly the ten ERP files (nothing else):

```
 M rust/crates/adapters/src/erp/{attendance,billing,subscriptions}.rs
 M rust/crates/host/src/erp/{attendance,billing,subscriptions}.rs
 M rust/crates/host/src/router.rs
 M rust/crates/host/tests/erp_{attendance,billing,subscriptions}.rs
```

## 2. A1 — receipt recovery is now bound to the initiating principal

Rust: the three handlers that previously resolved access and discarded it
(`billing.rs:499`, `subscriptions.rs:543`, `attendance.rs:532` in the audit's `file:line`
citations) no longer contain `let _ = &access;` — `grep -rn 'let _ = &access' rust/crates/host/src/erp/`
returns nothing. The lookup keys on the recorded actor **inside SQL**, and the read grant is
still enforced before the query, so a foreign actor gets the same `unknown` document as a
missing row:

| adapter                                             | statement                                              |
| --------------------------------------------------- | ------------------------------------------------------ |
| `rust/crates/adapters/src/erp/attendance.rs:824`    | `WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3` |
| `rust/crates/adapters/src/erp/billing.rs:999`       | `WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3` |
| `rust/crates/adapters/src/erp/subscriptions.rs:873` | `WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3` |

TypeScript (7 domains, per the owner's option-A decision: grant + actor binding + tests, no
new abstraction). Every receipt-recovery read in the seven packages now carries the actor
predicate — `grep -rn "SELECT receipt FROM" packages/{subscriptions,attendance,billing,laundry,field-services,formal-education,fulfillment}-pg/src/repository.ts | grep -v actor_id`
returns nothing:

| package             | line                | statement                                                  |
| ------------------- | ------------------- | ---------------------------------------------------------- |
| billing-pg          | `repository.ts:726` | `... WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3` |
| subscriptions-pg    | `repository.ts:624` | idem                                                       |
| attendance-pg       | `repository.ts:557` | idem                                                       |
| laundry-pg          | `repository.ts:613` | idem                                                       |
| fulfillment-pg      | `repository.ts:621` | idem                                                       |
| field-services-pg   | —                   | `... AND command_id=$2 AND created_by=$3`                  |
| formal-education-pg | —                   | idem                                                       |

The `FOR UPDATE` fingerprint lookups (`subscriptions:216`, `attendance:154`, `billing:253`,
`laundry:232`, `fulfillment:218`) are the write path of a command already in flight, not a
recovery read; they are unchanged by design.

## 3. A10 — `assigned` scope is enforced on parent and child reads

Owner predicate is spliced **inside** the SQL and applied **before** pagination, for both
the parent read and every child reader (entitlements, roster/check-in, lines/process/findings,
usage events). Representative anchors:

| package          | anchor                                         | shape                                                                                                    |
| ---------------- | ---------------------------------------------- | -------------------------------------------------------------------------------------------------------- |
| billing-pg       | `src/repository.ts:76`, `:81`, `:768`          | `access.scope === 'assigned' ? ' AND p.owner_id=$3' : ''` + owner bound into params                      |
| attendance-pg    | `src/repository.ts` (sessions/roster/check-in) | same conditional, owner param appended before `LIMIT/OFFSET`                                             |
| laundry-pg       | `src/repository.ts`                            | same, plus line/process/finding readers                                                                  |
| subscriptions-pg | `src/repository.ts`                            | entitlements + usage events; usage history reachable only through an owned subscription under `assigned` |

## 4. Gate evidence — exact commands, real exit codes, raw logs

All commands were executed in this worktree at 02:14 – 02:59 on the frozen tree of §1.

| #   | command                                                                                                                               | exit | log                                        | result                                                                                                                      |
| --- | ------------------------------------------------------------------------------------------------------------------------------------- | ---- | ------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------- |
| 1   | `pnpm format:check`                                                                                                                   | 0    | `/tmp/be45final-fmt.log`                   | `All matched files use Prettier code style!`                                                                                |
| 2   | `pnpm structure`                                                                                                                      | 0    | `/tmp/be45final-structure.log`             | 70 workspace projects, every `@archava/*` import declared                                                                   |
| 3   | `pnpm typecheck`                                                                                                                      | 0    | `/tmp/be45final-typecheck.log`             | all workspaces clean                                                                                                        |
| 4   | `pnpm lint` (checked-in 6144 MB heap bound)                                                                                           | 0    | `/tmp/be45final-lint.log`                  | 0 errors, 0 warnings, 45 s                                                                                                  |
| 5   | `pnpm test`                                                                                                                           | 0    | `/tmp/be45final-jstest.log`                | 145 files / 1896 tests passed, 5 s                                                                                          |
| 6   | `pnpm exec vitest run` on the seven ERP `test` dirs                                                                                   | 0    | `/tmp/be45final-vitest.log`                | 16 files / 208 tests passed                                                                                                 |
| 7   | `cargo test --offline --locked -p archava-host --test erp_billing --test erp_subscriptions --test erp_attendance -- --test-threads=1` | 0    | `docs/audit/evidence/audit-trio-final.log` | 44 passed / 0 failed (§5)                                                                                                   |
| 8   | `pnpm library:pack`                                                                                                                   | 0    | `/tmp/be45final-gate3b.log`                | 62 compiled internal packages packed, 20 s                                                                                  |
| 9   | `TMPDIR=/tmp node tools/library/verify-<domain>.mjs` × 7                                                                              | 0    | `/tmp/be45final-gate3b.log`                | 178 checks: subscriptions 19, attendance 18, billing 30, laundry 30, field-services 32, formal-education 22, fulfillment 27 |

`cargo fmt --all --check` is green as of card `t_077c2d73` (exit 0, empty output) and is
re-run live by the auditor as evidence item `cd rust && cargo fmt --all --check`.

## 5. The trio run (single, authoritative, detached)

Wrapper `.audit-trio.sh` records `START`, `GIT_HEAD`, both scope hashes before and after,
the cargo exit code, and `END`. It was launched as
`systemd-run --user --unit=audit-trio-be45b375-final --collect` so no worker budget could
truncate it:

```
START=2026-10-22T02:13:16+07:00
running 14 tests   → test result: ok. 14 passed; 0 failed … finished in 732.75s   (erp_attendance)
running 13 tests   → test result: ok. 13 passed; 0 failed … finished in 552.15s   (erp_billing)
running 17 tests   → test result: ok. 17 passed; 0 failed … finished in 775.77s   (erp_subscriptions)
EXIT=0
END=2026-10-22T02:47:43+07:00
```

Tests in that run that carry the A1/A10 claims directly (names from the raw log):

- `revoked_billing_grant_refused_on_recovery` (`erp_billing.rs:1657`),
  `revoked_attendance_grant_refused_on_recovery` (`erp_attendance.rs:1521`)
- `cross_tenant_callers_refuse_before_any_write`, `narrow_grants_refuse_exactly_the_arms_they_lack`
- `assigned_scope_binds_the_usage_history_to_the_entitlement_owner`,
  `malformed_target_ids_answer_not_found_instead_of_panicking`
- `lost_response_replays_the_recorded_receipt`

## 6. Declared deviations (each with its reason)

1. **The trio is not an `--evidence` command of `orchestrate.sh`.** The harness wraps the
   whole audit in `timeout 900`; the trio needs ~35 minutes on this box, and running it a
   second time concurrently is a documented failure mode (an earlier same-tree concurrent
   run was SIGTERM'd). It is executed once, detached, and its raw log is handed to the
   auditor as a **file** (`docs/audit/evidence/audit-trio-final.log`), whose own
   `START`/`END`/`SCOPE_HASH_*` lines bind the run to the inspected tree. `git diff --stat`,
   `pnpm format:check` and `cd rust && cargo fmt --all --check` are still reproduced live by
   the auditor.
2. **The seven `verify-*.mjs` scripts need a short `TMPDIR`** (finding A5, still open). Run
   with the worker's long `TMPDIR` (`/home/haikaru/.hermes/profiles/archlab/cache/scratch`,
   52 chars), `verify-subscriptions.mjs` dies before the second check with
   `TypeError: Cannot read properties of undefined (reading 'totalCount')` raised from
   `tools/library/pg-proof.mjs:4` inside its `finally` block — the fixture cluster never
   starts because the unix socket path exceeds `sun_path`. With `TMPDIR=/tmp` the same
   script passes 19/19 (exit 0, `/tmp/be45final-verify-subs.log`). The first attempt
   (`/tmp/be45final-gate3.log`, `VERIFY_subscriptions_EXIT=1`) is kept in place, not
   overwritten, because the masking behaviour is itself worth recording: `endProofPool(app)`
   is called in `finally` before `app` can exist, so the real cause of any early failure in
   these proofs is hidden by a second exception.
3. **Evidence is passed as files plus three live commands** (the doc, the raw trio log,
   `git diff --stat`, `pnpm format:check`, `cd rust && cargo fmt --all --check`), because
   `orchestrate.sh` truncates command evidence to 200 lines and aborts under `set -e` when an
   evidence command exits non-zero.

## 7. Conformance oracle used by this gate

Every ERP proof suite keeps the T01 test matrix as the behavioural oracle for the later
Rust cutover (owner decision, option A) — a green `proof.test.ts` is what "the TS patch does
not invent its own authorization model" means in practice:

- `recovers a command only for its initiator, and only with the read grant`
  (`billing-pg/test/proof.test.ts:501`, `laundry-pg/test/proof.test.ts:678`)
- `refuses recovery for an active member that never held the read grant`
  (`billing-pg/test/proof.test.ts:540`, `laundry-pg/test/proof.test.ts:708`)
- `lists only the reader-owned invoices under assigned scope` (`billing-pg/test/scope.test.ts:265`),
  `guards every child reader independently under assigned scope` (`:325`)
- `lists only the reader-owned sessions under assigned scope` (`attendance-pg/test/scope.test.ts:226`),
  `guards roster and check-in reads independently under assigned scope` (`:275`)
- `lists only the reader-owned orders under assigned scope` (`laundry-pg/test/scope.test.ts:240`),
  `guards line, process and finding reads independently under assigned scope` (`:290`)
- `never reaches usage events through a subscription-less entitlement under assigned scope`
  (`subscriptions-pg/test/scope.test.ts:432`)

## 8. Findings this gate does **not** close (do not read as fixed)

- **A2 (P1) and A3 (P1)** are untouched by this tree: `canonical()` in
  `rust/crates/own/src/lib.rs` still carries no `effects`/`readiness`/`cost` component, and
  no operation identity exists on owner apply. Neither finding was in this card's objective;
  no card has claimed them.
- **A4** (Rust evidence drift), **A5** (fixture PG vs long `TMPDIR` — reproduced today, §6.2),
  **A6** (coverage without threshold), **A7** (stale systemd units), **A8** (headers/cookie
  hardening), **A9** (supply chain: vite pin, no scheduled audit), **A11** (residual authz:
  organizations/projects/proposals/scheduling status reads, global login throttle bucket,
  tenant-existence oracle) all remain open.
- A green trio and a green gate battery certify **A1 + A10 + the two root gates**. They say
  nothing about the findings above.
