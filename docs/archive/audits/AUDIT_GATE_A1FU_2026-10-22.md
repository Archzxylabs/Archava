# FINAL GATE EVIDENCE PACK — A1 (command **replay** authorization) + A10 (assigned scope) + root gates

> **STATUS: EVIDENCE, NOT A VERDICT.** The verdict belongs to the Codex audit of card
> `t_be45b375`. This pack supersedes `docs/audit/AUDIT_GATE_FINAL_2026-10-22.md` for the
> replay half of A1: that pack's §2 covered receipt _recovery_ (status reads), while the
> round-1 verdict (`runs/20261022-025930-audit.json`) found the leak on the _replay_ path
> (`POST <domain>/commands`), which remediation cards `t_ce9ad928` (TS) and
> `t_cf46760e` (Rust) then closed. Everything below was produced on the frozen tree of §1;
> raw logs are quoted or hash-bound, not summarised from memory.

- Date: 2026-10-22 (Asia/Jakarta), gate window 04:08 – 04:52
- Repo: `/home/haikaru/Archverse/Lab/Archava_Platform`
- Base commit: `412f763e17f3a68a457fd76c7b929184c50c916a` (all remediation is uncommitted — dirty-tree policy applies)
- Gate card: `t_be45b375` — last gate before A1/A10 may be called closed

## 1. Tree under audit (frozen, hash-bound)

```
git rev-parse HEAD         → 412f763e17f3a68a457fd76c7b929184c50c916a
git status --porcelain     → 212 entries at 04:08 (213 at 04:53; the two additions are this
                             pack and docs/audit/evidence/audit-trio-gate.log)
git diff --stat            → 168 files changed, 4621 insertions(+), 2608 deletions(-)
```

Three hash bindings, taken before and after the cargo trio (they did not move):

| binding                                                                                                                                                                                                      | value                                                              | before/after                                          |
| ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------ | ----------------------------------------------------- |
| wrapper scope hash (`git status + git diff`, trio wrapper)                                                                                                                                                   | `4058da370afbbb41816dbacd7e6e8e9145c9a970612ea4226fda9b58cd91c27f` | identical                                             |
| rust-only scope (`git status -- rust + git diff -- rust`)                                                                                                                                                    | `75db13053d48f9efd1067abb6aad99c7c551825e4fa9b3c1cfe6a2e98cbf2d96` | identical — the same tree card `t_cf46760e` certified |
| **content** scope hash (sha256 of every dirty/untracked file inside `rust/ packages/ apps/ tools/ package.json pnpm-lock.yaml eslint.config.mjs .prettierignore vitest.config.ts`, plus `git diff --binary`) | `9195ba468685e79ef0f76f69f247049da8eba9d80e6b6c9e7bd6b4d9e9a70f89` | identical (04:15 vs 04:51)                            |

The third hash matters because the wrapper hash binds only _names_ of untracked files;
`packages/formal-education-pg/**` and the seven `test/replay.test.ts` files are untracked,
and their contents are bound by it. `formal-education-pg` is an untracked package in this
tree — its diff cannot appear in `git diff`; it is hash-bound and its fixes are exercised by
its own tests (`test/replay.test.ts`, 4 tests) and two harnesses.

## 2. A1 — a replay is authorised before the ledger is read, and never replays another actor's receipt

Round-1 verdict (P1): `packages/billing-pg/src/repository.ts:253` and its six siblings read
`receipt, fingerprint … WHERE tenant_id=$1 AND request_id=$2` with **no actor predicate** and
returned a foreign receipt with `idempotencyReplay: true` **before** any `grant(...)` ran.

### 2.1 TypeScript — all seven `-pg` domains

The receipt-returning lookup now carries the initiator inside SQL and runs _after_ the
per-command-class grant. An occupied key that the actor predicate did not surface (the same
key held by another actor) is answered with the pre-existing _conflict_ ("Command payload
fingerprint mismatch on replay") via a probe outside the actor predicate — no new refusal
shape, no existence oracle beyond the message the fingerprint path already returned, and
never the unique-violation 500.

| package             | grant (hoisted, precedes SELECT)               | replay lookup (initiator bound)                | occupancy probe | test file                       |
| ------------------- | ---------------------------------------------- | ---------------------------------------------- | --------------- | ------------------------------- |
| billing-pg          | `repository.ts:267`                            | `:270` `… AND request_id=$2 AND actor_id=$3`   | `:292`          | `test/replay.test.ts` (4 tests) |
| subscriptions-pg    | `:229`                                         | `:232` `… AND actor_id=$3 FOR UPDATE`          | `:250`          | 4 tests                         |
| attendance-pg       | `:167`                                         | `:170` `… AND actor_id=$3`                     | `:192`          | 4 tests                         |
| laundry-pg          | `:233`                                         | `:241` `… AND actor_id=$3`                     | `:263`          | 4 tests                         |
| fulfillment-pg      | `:220` (+ `:222`/`:225` dispatch conditionals) | `:229` `… AND actor_id=$3`                     | `:251`          | 4 tests                         |
| field-services-pg   | `:231`                                         | `:234` `… AND command_id=$2 AND created_by=$3` | `:255`          | 4 tests                         |
| formal-education-pg | `:270`                                         | `:273` `… AND command_id=$2 AND created_by=$3` | `:295`          | 4 tests                         |

Test design (each of the seven files): a payload-driven stub answers statements by
their SQL, so a leak cannot be hidden by stubbing — `billing-pg/test/replay.test.ts:78`
(`actorBound`, foreign actor → rowless), `:87` (any write attempt after a refusal throws),
`:139` (`expect(lookup?.text).toContain('actor_id=$3')`). Four cases per package:
foreign actor with the exact original payload → `conflict` and no write; grant withdrawn →
`forbidden` _before any ledger read_; the initiator's own replay → recorded receipt with
`idempotencyReplay: true`; initiator with a changed payload → `conflict`.

The five _recovery/status_ reads (billing `:750`, subscriptions `:648`, attendance `:581`,
laundry `:630`, fulfillment `:638`) and field-services `:566` / formal-education `:636`
already bound the initiator from T01 and remain unchanged.

### 2.2 Rust — billing / attendance / subscriptions replay now compares the recorded initiator

`grant(...)` still precedes the lookup (`billing.rs:438`), the row read selects
`actor_id` beside `fingerprint`/`receipt`, and the comparison rejects the same-grant rival:

| adapter       | row read                                                                                              | initiator comparison                                                                      | receipt replay |
| ------------- | ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- | -------------- |
| billing       | `rust/crates/adapters/src/erp/billing.rs:291` `SELECT actor_id,fingerprint,receipt`                   | `:448` `row_fingerprint != fingerprint \|\| row_actor != actor_id` → `conflict()`         | `:457`         |
| attendance    | `rust/crates/adapters/src/erp/attendance.rs:193` `SELECT receipt, fingerprint, actor_id … FOR UPDATE` | `:207` `stored_fingerprint != fingerprint \|\| recorded_actor != actor_id` → `conflict()` | `:212`         |
| subscriptions | `rust/crates/adapters/src/erp/subscriptions.rs:245` (idem)                                            | `:259` (idem)                                                                             | `:264`         |

Three new negative tests (raw trio log): `a_same_grant_rival_never_replays_another_actors_{billing,attendance,subscription}_receipt`;
the legitimate replay remains green (`lost_response_replays_the_recorded_receipt` for billing
and subscriptions, `lost_response_replays_the_recorded_attendance_receipt` for attendance).

### 2.3 What A1-replay deliberately does **not** change

- The four _status reads_ in `organizations.rs:739`, `projects.rs:388`, `proposals.rs:522`,
  `purchasing.rs:886` and `assets.rs:824` (under `Assigned` only) still gate on the resource
  owner, not the recorded initiator — the read-side analogue of this finding, at lower
  privilege, recorded in `docs/audit/A1FU_RUST_REPLAY_INITIATOR_2026-10-22.md` and **not**
  fixed (out of this card's acceptance; tracked with the A11 residuals).
- No wire/JSON shape changed, no new refusal shape (`conflict` is the fingerprint-mismatch
  message; TS `unknown_effect` paths for recorded-without-receipt in field-services /
  formal-education are preserved).

## 3. A10 — `assigned` scope (unchanged by the remediation delta; previously approved)

The round-1 audit already reported "A10 appears correctly enforced"; the A1-FU delta is
confined to the replay functions and contains no change to scope/`owner_id` logic. The
anchors remain (spot-checked at 04:52 on this tree):

- billing-pg `repository.ts:90/:95/:825` — `pin(' AND p.owner_id=$3')` under `assigned`;
- attendance-pg `:66/:71`, laundry-pg `:60/:65` — same shape for sessions/orders;
- subscriptions-pg `:726/:764` — entitlements/usage owner predicate;
- Rust `subscriptions.rs:1042/:1094/:1131` — parent probe and `read_contact_entitlements` /
  `read_usage_events` carry `p.owner_id=$3` under `assigned` (card `t_23799344`, audit
  `runs/20261022-014232-audit.json`: verdict approve, zero findings).
- TS: `packages/billing-pg/test/scope.test.ts:265/:325`, attendance `:226/:275`,
  laundry `:240/:290`, subscriptions `scope.test.ts:432` — the child-reader oracle battery,
  all green in the focused run (§4).

## 4. Gate battery — exact commands, real exit codes, raw logs

Executed serially on the frozen tree, 04:08:31 – 04:11:25 (logs under `/tmp/be45g/`):

| #   | command                                                                                                          | exit | result                                                                                                                           |
| --- | ---------------------------------------------------------------------------------------------------------------- | ---- | -------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `pnpm format:check`                                                                                              | 0    | "All matched files use Prettier code style!"                                                                                     |
| 2   | `pnpm structure`                                                                                                 | 0    | 70 workspace projects, every `@archava/*` import declared                                                                        |
| 3   | `pnpm typecheck`                                                                                                 | 0    | all workspaces clean (49 s)                                                                                                      |
| 4   | `pnpm lint` (checked-in 6144 MB heap bound, `--max-warnings=0`)                                                  | 0    | 0 errors / 0 warnings (52 s)                                                                                                     |
| 5   | `pnpm test`                                                                                                      | 0    | **152 files / 1924 tests passed** (147 → 1920+); the +28 replay tests are the delta over T03's 1896                              |
| 6   | `pnpm exec vitest run` on the seven ERP `test` dirs                                                              | 0    | **23 files / 236 tests passed** (incl. 28 new replay tests)                                                                      |
| 7   | `pnpm library:pack`                                                                                              | 0    | 62 compiled internal packages packed                                                                                             |
| 8   | `TMPDIR=/tmp node tools/library/verify-<domain>.mjs` × 7                                                         | 0    | **178 checks** — billing 30, subscriptions 19, attendance 18, laundry 30, field-services 32, formal-education 22, fulfillment 27 |
| 9   | `cd rust && cargo fmt --all --check`                                                                             | 0    | 0 bytes output                                                                                                                   |
| 10  | `cd rust && cargo clippy --all-targets --locked --offline -- -D warnings`                                        | 0    | after the trio compile, 0 warnings                                                                                               |
| 11  | trio `cargo test` (§5) — `--test erp_billing --test erp_subscriptions --test erp_attendance -- --test-threads=1` | 0    | **47 passed / 0 failed**                                                                                                         |

The trio is **not** reproduced live by the audit (§6.1); its log
`docs/audit/evidence/audit-trio-gate.log` (sha256 `e2c50545ef0360c0edd7b2ae67365d2d4c6ebc3dd2fac507e4549ff1c32d6a7b`, 80 lines)
carries the wrapper's own `START/GIT_HEAD/SCOPE_HASH_*/EXIT/END` boundaries and every
passes/failures line, intact.

## 5. The trio run (single, authoritative, detached)

`.audit-trio.sh`, launched `systemd-run --user --collect --unit=be45-trio-gate`, 04:12:11 → 04:49:18 (37 min 07 s):

```
START=2026-10-22T04:12:11+07:00
GIT_HEAD=412f763e17f3a68a457fd76c7b929184c50c916a
SCOPE_HASH_BEFORE=4058da370afbbb41816dbacd7e6e8e9145c9a970612ea4226fda9b58cd91c27f
RUST_SCOPE_BEFORE=75db13053d48f9efd1067abb6aad99c7c551825e4fa9b3c1cfe6a2e98cbf2d96
test result: ok. 15 passed; 0 failed … finished in 771.59s   (erp_attendance)
test result: ok. 14 passed; 0 failed … finished in 625.35s   (erp_billing)
test result: ok. 18 passed; 0 failed … finished in 826.98s   (erp_subscriptions)
EXIT=0
END=2026-10-22T04:49:18+07:00
SCOPE_HASH_AFTER  = 4058da370afbbb41816dbacd7e6e8e9145c9a970612ea4226fda9b58cd91c27f  (identical)
RUST_SCOPE_AFTER  = 75db13053d48f9efd1067abb6aad99c7c551825e4fa9b3c1cfe6a2e98cbf2d96  (identical)
```

## 6. Declared deviations

1. **Trio as evidence file, not live command.** The harness runs evidence commands under
   `timeout 900`; the trio needs ~37 minutes, and a concurrent duplicate is a documented
   failure mode. `git diff --stat`, `pnpm format:check` and `cd rust &&
cargo fmt --all --check` are reproduced live by the auditor.
2. **`verify-*.mjs` needs `TMPDIR=/tmp`** (finding **A5**, still open): with the worker's
   long `TMPDIR` the fixture cluster dies on `sun_path` and `verify-subscriptions.mjs`
   masks it with a `TypeError` raised from its `finally`. All 7 harnesses were run with
   `TMPDIR=/tmp` and passed; A5 stays open either way.
3. **One evidence doc was Prettier-dirty at entry** (`A1FU_RUST_REPLAY_INITIATOR…md`); it
   was normalised with `prettier --write` — the delta is table padding plus the
   `*term* → _term_` emphasis form only, proven by normalized equality
   (whitespace-stripped, asterisks folded to underscores: tables' dash rules aside, no
   literal moved). Everything else entered and stayed clean.
4. **24 "extra" dirty entries** exist outside the source scope (docs audit packs, this
   pack, evidence tree, previous round formats). The content scope hash of §1 excludes
   them deliberately: the verdict binds the **source** tree.

## 7. Findings this gate does **not** close (do not read as fixed)

- **A2 (P1), A3 (P1)** — untouched; outside this card's objective, no card claims them.
- **A4** (Rust evidence drift), **A5** (fixture PG vs long `TMPDIR` — see §6.2), **A6**
  (coverage without threshold), **A7** (stale systemd units), **A8** (headers/cookie), **A9**
  (supply chain: vite pin, scheduled audit), **A11** (residual authz incl. the §2.3 status
  reads, global login throttle bucket, tenant-existence oracle) — all open.
- A green battery here certifies **A1 (both recovery and replay) + A10 + the two root
  gates** (`format:check` + `lint`). It says nothing about the findings above.
