# AUDIT GATE — final A1/A10/gate battery (2026-10-22, third run)

**This document is evidence; the verdict it feeds lives in the orchestrator run below.** Nothing here
claims A1, A10 or the gates are closed on its own authority.

> **VERDICT (2026-10-22T08:12:30): `approve`, 0 findings** —
> `~/Archverse/Lab/Automation/codex-orchestrator/runs/20261022-081230-audit.json`
> (sha256 `d0f65ebd7f71795b1a23eebc3bb9b97ce8eff695ae85665b80e1dbc81710cea1`). See §9.

Card: `t_be45b375` — _AUDIT — Codex verdict untuk A1/A10/gate (gate terakhir)_.
Predecessor pack (superseded): `docs/audit/AUDIT_GATE_A1FU_2026-10-22.md`
(verdict `changes_requested`, `runs/20261022-045512-audit.json`, 1×P1 + 1×P2).

## 1. Tree under audit (one frozen snapshot, hash-pinned)

| item                                                         | value                                                                                 |
| ------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| snapshot taken                                               | `2026-10-22T07:10:46+07:00`                                                           |
| `git rev-parse HEAD`                                         | `412f763e17f3a68a457fd76c7b929184c50c916a`                                            |
| dirty entries (`git status --porcelain \| wc -l`)            | **223**                                                                               |
| `git diff --stat`                                            | **170 files changed, 5575 insertions(+), 2864 deletions(-)**                          |
| `SCOPE` (sha256 of status listing + diff)                    | `0f98919ce5a3448c7f7d4f7896252ec04b254b97b9a2cac5abd3466ebd3fc43a`                    |
| `RUST_SCOPE` (same, restricted to `rust/`)                   | `b325d301b89ce9cb3c48b61ce15004d813b6ab5c56fedc857dc43d93c58e9b99`                    |
| `CONTENT_SCOPE_BEFORE` (worktree bytes of every scoped file) | `991b38fdb0b7997a0d38c97e8f8b8e6e93b7e9a48380fae6acf92f6ccf31808f` over **191** files |

The worktree is deliberately dirty: HEAD predates the whole A1 remediation, so `git diff` cannot
attribute authorship. Admissibility is proven by byte identity — the trio wrapper recomputes
`SCOPE_HASH` / `RUST_SCOPE` before and after the run, and `scope-hash.sh` hashes the worktree bytes
of every modified **and untracked non-ignored** scoped file plus a `git diff --binary` digest.

Scope paths: `rust`, `packages`, `apps`, `tools`, `package.json`, `pnpm-lock.yaml`,
`eslint.config.mjs`, `.prettierignore`, `vitest.config.ts`.

## 2. Gate battery (all run on the snapshot above, `TMPDIR=/tmp`)

| #   | gate                   | command                                                                                                                        | exit       | result                                                                |
| --- | ---------------------- | ------------------------------------------------------------------------------------------------------------------------------ | ---------- | --------------------------------------------------------------------- |
| 1   | format                 | `pnpm format:check`                                                                                                            | **0**      | `All matched files use Prettier code style!` (25.6 s)                 |
| 2   | structure              | `pnpm structure`                                                                                                               | **0**      | `structure: 70 workspace projects, every @archava/* import declared.` |
| 3   | typecheck              | `pnpm typecheck`                                                                                                               | **0**      | clean (66.6 s)                                                        |
| 4   | unit tests (all)       | `pnpm test`                                                                                                                    | **0**      | **153 files / 1931 tests passed** (14.5 s)                            |
| 5   | ERP vitest (7 domains) | `pnpm exec vitest run packages/{billing,subscriptions,attendance,laundry,field-services,formal-education,fulfillment}-pg/test` | **0**      | **23 files / 239 tests passed**                                       |
| 6   | lint                   | `pnpm lint` (`eslint . --max-warnings=0`, heap 6144 MB in script)                                                              | **0**      | 0 errors / 0 warnings, 0 output lines (72.2 s)                        |
| 7   | library pack           | `pnpm library:pack`                                                                                                            | **0**      | `Packed 62 compiled internal packages in dist/library/tarballs.`      |
| 8   | library verify         | `node tools/library/verify-{billing,subscriptions,attendance,laundry,field-services,formal-education,fulfillment}.mjs`         | **0** each | **178 PASS** (30/19/18/30/32/22/27), real PostgreSQL per harness      |
| 9   | rust fmt               | `cd rust && cargo fmt --all --check`                                                                                           | **0**      | empty output (0 bytes)                                                |
| 10  | rust clippy            | `cargo clippy --all-targets --locked --offline -- -D warnings`                                                                 | **0**      | 0 warnings; warm fingerprint, no recompile needed (0.46 s)            |
| 11  | rust trio              | `bash .audit-trio.sh` under `systemd-run --user --collect`                                                                     | _see §5_   | _see §5_                                                              |

Logs: `/tmp/g3-fmt.log`, `/tmp/g3-structure.log`, `/tmp/g3-typecheck.log`, `/tmp/g3-jstest.log`,
`/tmp/g3-vitest7.log`, `/tmp/g3-lint.log`, `/tmp/g3-pack.log`, `/tmp/g3-verify-*.log`,
`/tmp/g3-cargofmt.log`, `/tmp/g3-clippy.log`.
Trio wrapper log: `docs/audit/evidence/audit-trio-gate3.log`.

## 3. A1 — replay authorization (the P1 class that failed the last two runs)

### 3.1 TypeScript `execute()` — grant precedes the ledger read, ledger lookup binds the initiator

| package               | grant (line)                                             | ledger read (line)    | initiator binding   | occupancy probe |
| --------------------- | -------------------------------------------------------- | --------------------- | ------------------- | --------------- |
| `billing-pg`          | `:267` `COMMAND_PERMISSIONS[mutation.type]`              | `:270`                | `AND actor_id=$3`   | `:292`          |
| `subscriptions-pg`    | `:229` `COMMAND_PERMISSIONS[mutation.type]`              | `:232` (`FOR UPDATE`) | `AND actor_id=$3`   | `:250`          |
| `attendance-pg`       | `:167` `COMMAND_PERMISSIONS[mutation.type]`              | `:170`                | `AND actor_id=$3`   | `:192`          |
| `laundry-pg`          | `:233` (conditional `laundry.process` / `laundry.write`) | `:241`                | `AND actor_id=$3`   | `:263`          |
| `field-services-pg`   | `:231` `COMMAND_PERMISSIONS[guarded.action]`             | `:234`                | `AND created_by=$3` | `:255`          |
| `formal-education-pg` | `:270` `COMMAND_PERMISSIONS[guarded.action]`             | `:273`                | `AND created_by=$3` | `:295`          |
| `fulfillment-pg`      | `:222` write, `:224`/`:227` dispatch, `:236` produce     | `:240`                | `AND actor_id=$3`   | `:251`          |

`fulfillment-pg` is this round's fix (`t_0b7608c5`): authorization now runs entirely **above** the
ledger read — `fulfillment.write` plus the two dispatch grants from the payload, and
`fulfillment.produce` only after a `SELECT kind FROM archava_fulfillment.jobs` lookup when the named
job is production (kind only, no lifecycle state). The old post-ledger arm guard was removed, leaving
one decision site. Regression tests: `packages/fulfillment-pg/test/replay.test.ts:210` (produce
withdrawn → refused, no receipt, no ledger write), `:250` (initiator still holding produce → replay),
`:263` (dispatch-only line → write grant suffices).

Foreign keys never reach `INSERT`/unique-violation: the actor-bound read hides the row, the occupancy
probe answers the pre-existing `conflict` fingerprint-mismatch shape, so no new error/refusal shape
and no existence oracle was introduced.

Sweep of the same `execute()` in all `packages/*-pg` with a `*.commands` ledger: the seven above are
the complete set — every one authorizes before the read (see §4).

### 3.2 Rust ERP adapters — grant hoisted above `claim_command`

| adapter            | hoisted gate                                                             | claim → replay                                       | initiator check                                 |
| ------------------ | ------------------------------------------------------------------------ | ---------------------------------------------------- | ----------------------------------------------- |
| `attendance.rs`    | `:326` `require(access, mutation.command.required_permission())?`        | `:328` claim → `:337` `Claim::Replay(...) => return` | `recorded_actor != actor_id` in `claim_command` |
| `subscriptions.rs` | `:321` `require(access, mutation.command.required_permission())?`        | `:323` claim → `:332` `Claim::Replay(...) => return` | `recorded_actor != actor_id` in `claim_command` |
| `billing.rs`       | `:438` `grant(access, command.required_permission())?` (already correct) | `:461` `claim_billing_command`                       | `row_actor != actor_id` → `conflict()`          |

This round's fix (`t_57b870a5`) hoisted the per-command grant in `attendance.rs` / `subscriptions.rs`
above `claim_command` and deleted the five now-redundant per-arm `require` calls in each, so a seat
that is still active but has lost the permission answers `403 forbidden` on the replay path instead
of returning the stored receipt. Wire shape unchanged (`{"reason":"forbidden"}`).

Regression tests: `rust/crates/host/tests/erp_attendance.rs:1690`
`revoked_attendance_grants_refuse_the_stored_writes`, `rust/crates/host/tests/erp_subscriptions.rs:1075`
`revoked_subscription_grants_refuse_the_stored_writes` — success → permission withdrawn while the
seat stays active → byte-identical replay refused with no receipt and no writes; the still-authorized
initiator keeps `idempotencyReplay: true`.

Mutation controls from the remediation cards (recorded, not re-run here): dropping the hoisted gate
reproduces `200` where the test expects `403` in both suites; dropping the actor predicate or the
hoisted grant in `billing-pg` / `field-services-pg` turns exactly one new TS test red.

## 4. Independent adversarial sweep (read-only, this run)

Static ordering check — for every command path, does an authorization call appear lexically before
the first ledger read? Scripts: `replay-order-sweep.py` / `replay-order-sweep-rust.py`.

- **TS**: 7/7 packages with an `execute()` that reads `*.commands` → `authorize_before_ledger=True`;
  every ledger SELECT carries `AND actor_id=$3` (5 packages) or `AND created_by=$3` (2 packages).
- **Rust replay handlers**: `record_attendance_command` and `record_subscriptions_command` →
  `gate_before_claim=True`; that is the complete set of replay-returning claim sites in
  `crates/adapters/src/erp`.
- **Kernel claim sites without a local `require`/`grant`** (`commerce_kernel::receive_purchase_stock`,
  `record_product_command`, `prepare_food_commerce_order`): not replay-receipt paths. `record_product_command`
  gates explicitly on `context.access.holds(&["catalog.write"])`; the other two call
  `claim_commerce_command`, and a lost claim is answered `PurchaseError::Domain(ErpError::Conflict)` —
  no stored receipt is ever returned, so a revoked principal cannot recover output.
- Result: `PROBLEMS: none`.

## 5. Rust trio (the long pole)

Runner: `.audit-trio.sh` via `systemd-run --user --collect --unit=audit-trio-g3` (started
`07:10:55`), so a tool/session timeout cannot kill a multi-minute cargo build+test.
It prints `SCOPE_HASH_BEFORE` / `RUST_SCOPE_BEFORE` and re-measures both after the last binary.

```
START=2026-10-22T07:10:55+07:00   END=2026-10-22T08:02:04+07:00   EXIT=0
test result: ok. 16 passed; 0 failed ... finished in 1165.35s   (erp_attendance)
test result: ok. 14 passed; 0 failed ... finished in  814.97s   (erp_billing)
test result: ok. 19 passed; 0 failed ... finished in 1083.14s   (erp_subscriptions)
```

**49 passed / 0 failed**, `--test-threads=1`, `--offline --locked`.
Log: `docs/audit/evidence/g3/trio-gate3.log`, sha256
`dd24e872a71f129321b0b6d5578b582bf7ba2428025f74a209c7e54187f51bfa`.

### 5.1 Scope reconciliation (why `SCOPE_HASH_AFTER` differs from `SCOPE_HASH_BEFORE`)

The wrapper hashes `git status --porcelain; git diff` before and after, so the audit document this
run authored mid-flight (`docs/audit/AUDIT_GATE_G3_2026-10-22.md`) enters the after-hash. Removing
that one `??` line and re-hashing reproduces the before value exactly:

| hash                                                                   | value                                                                |
| ---------------------------------------------------------------------- | -------------------------------------------------------------------- |
| `SCOPE_HASH_BEFORE`                                                    | `0f98919ce5a3448c7f7d4f7896252ec04b254b97b9a2cac5abd3466ebd3fc43a`   |
| `SCOPE_HASH_AFTER`                                                     | `426bc67109dbc667830592babe24a7dc5d633e181fd7f98296a7bbe1118c46f4`   |
| after-hash minus the one new audit doc                                 | `0f98919ce5a3448c7f7d4f7896252ec04b254b97b9a2cac5abd3466ebd3fc43a` ✓ |
| `RUST_SCOPE` before / after                                            | `b325d301…` / `b325d301…` (identical)                                |
| `CONTENT_SCOPE` before / after (191 scoped code files, worktree bytes) | `991b38fd…30808f` / `991b38fd…30808f` (identical)                    |

No source file moved while the suite ran.

## 6. Recovery/status reads (A1 second half) and A10 — unchanged this round, re-anchored

Recovery/status paths — permission first, then an actor-bound read, `unknown` for a foreign key:

| adapter             | gate                                     | SELECT                                                         |
| ------------------- | ---------------------------------------- | -------------------------------------------------------------- |
| `billing.rs:1000`   | `grant(access, "billing.read")?`         | `:1003` `WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3` |
| `attendance.rs:826` | `require(access, "attendance.read")?`    | `:827` same shape                                              |
| `subscriptions.rs`  | `require(access, "subscriptions.read")?` | `:876` same shape                                              |

These were accepted by the previous two runs; they are listed here only so the auditor can
re-check them on this exact snapshot. The TS recovery reads (`billing:747-750`,
`subscriptions:645-648`, `attendance:578-581`, `laundry:627-630`, `fulfillment`, `field-services:557/566`,
`formal-education:627/636`) still grant a read permission and bind the initiator before returning a
stored receipt.

A10 (`assigned` scope) predicates still in place: `billing-pg/src/repository.ts:90`,
`attendance-pg:66`, `laundry-pg:60`, `subscriptions-pg:806` and `:839` (`EXISTS … AND p.owner_id=$3`),
with their focused suites green in §2.

## 7. Explicitly NOT claimed by this pack

A1 and A10 closure is the Codex verdict's call. Still open and untouched here: **A2, A3, A4, A5, A6,
A7, A8, A9, A11** (A5/A7/A9 have their own cards closed separately; their code is in this tree but is
not part of this acceptance). No commit, no `git add`, no revert, no checkout was performed.

## 8. Reproduction

```bash
export TMPDIR=/tmp
cd /home/haikaru/Archverse/Lab/Archava_Platform
pnpm format:check && pnpm structure && pnpm typecheck && pnpm test && pnpm lint
pnpm library:pack
for d in billing subscriptions attendance laundry field-services formal-education fulfillment; do
  node tools/library/verify-$d.mjs || break
done
cd rust && cargo fmt --all --check && cargo clippy --all-targets --locked --offline -- -D warnings
bash .audit-trio.sh   # clippy -D warnings + fmt + erp_attendance/erp_billing/erp_subscriptions
```

## 9. Verdict

Orchestrator run `20261022-081230-audit.json` (mode `audit`, effort `high`, timeout 2400 s,
harness exit 0, read-only sandbox over this repo):

| field             | value                                                                                                                                              |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| verdict           | **`approve`**                                                                                                                                      |
| findings          | **0**                                                                                                                                              |
| evidence supplied | `git diff --stat`, this pack, `docs/audit/evidence/g3/GATES.txt`, `docs/audit/evidence/g3/trio-gate3.log`, `docs/audit/evidence/g3/g3-vitest7.log` |
| sha256            | `d0f65ebd7f71795b1a23eebc3bb9b97ce8eff695ae85665b80e1dbc81710cea1`                                                                                 |

Verbatim summary:

> Approve A1/A10 and format/lint closure. Reviewed the actual repositories, adapters and regression
> tests. Replay authorization precedes receipt retrieval (`fulfillment-pg/src/repository.ts:222–240`;
> Rust `attendance.rs:326`; `subscriptions.rs:321`), and recovery binds the initiator after checking
> read permission. Assigned-scope predicates cover parent and child reads, including
> `subscriptions-pg/src/repository.ts:806` and `:839`. Independent direct Node runs of Prettier and
> ESLint both exited 0. Supplied evidence records 239 ERP tests and 49 Rust tests passing; the Rust
> log SHA-256 matches. Test reruns were prevented by sandbox temporary-file restrictions. Approval
> covers only the requested scope.

So, on this exact snapshot: **A1 closed** (replay and recovery), **A10 closed**, and the
**format:check / lint gates restored**. The other P2/P3 items in `ARCHLAB_AUDIT_2026-10-21.md`
(A2, A3, A4, A6, A8, A11 — A5, A7 and A9 have since closed on their own cards) stay **open** and are
not covered by this approval.
