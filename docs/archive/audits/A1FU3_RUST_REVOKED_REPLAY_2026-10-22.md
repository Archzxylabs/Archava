# A1-FU3 (Rust) — grant check before claim/replay (revoked-permission replay bypass)

Card `t_57b870a5` (P1) · origin gate `t_be45b375` · origin run `runs/20261022-045512-audit.json`
Tree: `HEAD 412f763e` + dirty working tree (fix uncommitted; no checkout, no copy, no revert) · 2026-10-22
Origin content scope `9195ba46…`, origin `RUST_SCOPE 75db1305…`.

## Finding

`record_attendance_command` and `record_subscriptions_command` authorized **after** the command claim, so
`Claim::Replay(receipt) => return Ok(*receipt)` answered the stored receipt before any per-command grant was
checked. A principal whose seat is still `active` but whose permission was withdrawn could replay the original
payload byte-for-byte and still receive the stored receipt — a revocation bypass that also skipped the audit
trail (`claim_command` was never even reached on the replay path, so no new row, no new audit).

`billing.rs` was already correct (`grant(access, command.required_permission())?` at `:438`, before the claim at
`:461`) and is untouched.

## Fix

One hoisted gate per adapter, the same shape the billing adapter uses: the enum's own `required_permission()`
gates the claim, not just the arm body.

| adapter                                         | before (`git show HEAD:` line numbers)                                                                                                                                               | after (current tree)                                                                                                                               |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| `rust/crates/adapters/src/erp/attendance.rs`    | `claim_command` `:320` → replay return `:329`; per-arm `require(access, …)` at `:351`, `:413`, `:660`, `:737` (`attendance.write`), `:512` (`attendance.checkin`)                    | `:326` `require(access, mutation.command.required_permission())?;` → claim `:328` → replay return `:337`; per-arm `require` calls removed (0 left) |
| `rust/crates/adapters/src/erp/subscriptions.rs` | `claim_command` `:315` → replay return `:324`; per-arm `require` at `:348`, `:653`, `:769` (`subscriptions.write`), `:428` (`subscriptions.payment`), `:498` (`subscriptions.usage`) | `:321` hoisted `require` → claim `:323` → replay return `:332`; per-arm `require` calls removed (0 left)                                           |

Line-number note: this document quotes the numbers measured on the frozen `HEAD` commit, which sit ~4 lines
earlier than the numbers in the audit packet (`:324`/`:333`, `:355`/`:417`/`:516`/`:664`/`:741` and `:319`/`:328`,
`:352`/`:432`/`:502`/`:657`/`:773`). Same code, same defect: the packet measured a working tree that already
carried an earlier card's edits. The `after` column is the tree this card finishes with.

The arm-level `require(access, …)` calls are gone rather than left in place: the hoisted gate already covers every
command variant through `required_permission()` (attendance: 5 arms — `attendance.write` ×4, `attendance.checkin`
×1; subscriptions: 5 arms — `subscriptions.write` ×3, `subscriptions.payment`, `subscriptions.usage`), and a
duplicated per-arm string is exactly the drift that produced this bug. No wire/JSON shape changed, no new refusal
shape — the existing `403 {"reason":"forbidden"}` is what a withdrawn grant now gets, on the fresh _and_ the replay
path.

## Sweep — every `record_*` command handler in `rust/crates/adapters/src/erp/`

Order is `permission check → claim/replay` in all twelve; the two adapters above were the only leaks. The table was
produced by reading each handler body and comparing the first authorization call against the first claim call
(`claim_command` / `claim_*`), not by trusting the call sites named in the finding.

| adapter              | handler                        | authorization (line, expression)                                                        | claim (line) | order          |
| -------------------- | ------------------------------ | --------------------------------------------------------------------------------------- | ------------ | -------------- |
| `attendance.rs`      | `record_attendance_command`    | `326` `require(access, mutation.command.required_permission())` **(hoisted this card)** | `328`        | ok             |
| `subscriptions.rs`   | `record_subscriptions_command` | `321` `require(access, mutation.command.required_permission())` **(hoisted this card)** | `323`        | ok             |
| `billing.rs`         | `record_billing_command`       | `438` `grant(access, command.required_permission())`                                    | `461`        | ok (untouched) |
| `accommodation.rs`   | `record_accommodation_command` | `907` `access.holds(&[command.required_permission()])`                                  | `923`        | ok (untouched) |
| `assets.rs`          | `record_assets_command`        | `381` `access.holds(&[command.required_permission()])`                                  | `388`        | ok (untouched) |
| `scheduling.rs`      | `record_scheduling_command`    | `500` `access.holds(&[command.required_permission()])`                                  | `517`        | ok (untouched) |
| `organizations.rs`   | `record_organization_command`  | `268` `access.holds(&[mutation.data.kind().permission()])`                              | `275`        | ok (untouched) |
| `projects.rs`        | `record_project_command`       | `323` `access.holds(&[mutation.command.kind().permission()])`                           | `335`        | ok (untouched) |
| `proposals.rs`       | `record_proposal_command`      | `432` `access.holds(&[mutation.data.kind().permission()])`                              | `445`        | ok (untouched) |
| `foodservice.rs`     | `record_food_command`          | `459` `authorize_operation(context, &mutation.command)`                                 | `464`        | ok (untouched) |
| `purchasing.rs`      | `record_purchase_command`      | `781` `authorize_operation(&context.context(), mutation.command.operation())`           | `785`        | ok (untouched) |
| `commerce_kernel.rs` | `record_product_command`       | `817` `context.access.holds(&["catalog.write"])` (+ `scope == All`, `catalog.read`)     | `830`        | ok (untouched) |

`authorize_operation` for foodservice/purchasing is not a rubber stamp: it maps the command to its own permission
(`crates/domain/src/erp/foodservice.rs:796`, `crates/domain/src/erp/purchasing.rs:839`) and refuses before the
handler reaches the claim.

Two further `claim_commerce_command` call sites are intra-transaction kernel helpers, not HTTP entry points:
`receive_purchase_stock` (`:430`) and `prepare_food_commerce_order` (`:1624`) are only called from the
foodservice/purchasing handlers **after** those handlers authorized. `claim_commerce_command` itself is a bare
`INSERT … ON CONFLICT DO NOTHING` returning a bool — a lost claim yields `conflict` and it never reads a stored
receipt, so there is no replay surface to gate.

## Regression tests (HTTP level, pg fixture)

`rust/crates/host/tests/erp_attendance.rs:1690` — `revoked_attendance_grants_refuse_the_stored_writes`
`rust/crates/host/tests/erp_subscriptions.rs:1075` — `revoked_subscription_grants_refuse_the_stored_writes`

Both follow the card's (a)/(b)/(c): execute the writes for real, withdraw the grant with
`UPDATE …memberships SET permissions = array_remove(permissions, $3) WHERE … AND active`, assert the seat is still
`active`, then replay the **byte-identical** payload.

- attendance: `session.create`, `attendance.register`, `attendance.checkin` all succeed first
  (`idempotencyReplay: false`, 3 command rows, 1 check-in event). With `attendance.write` withdrawn: the create
  replay and the register replay answer `403 {"reason":"forbidden"}`, no `sessionId`/`rosterId` fields, no
  `idempotencyReplay`, and the counters stay at 1 session / 1 roster / 3 commands / 1 check-in — "the claim never
  ran". Cross-arm control: the check-in replay is a different command that needs `attendance.checkin` (still
  held), so it keeps answering `idempotencyReplay: true` with the stored `rosterId`. Restoring the grant restores
  the stored-receipt replay. Then `attendance.checkin` is withdrawn: the check-in replay is `forbidden` with no
  `rosterId` and still 1 check-in event.
- subscriptions: `subscription.create` (allocation), `subscription.pay`, `subscription.use` succeed first (3
  commands, 1 usage event, revision read from the table). With `subscriptions.usage` withdrawn the use replay is
  `forbidden`, carries no `entitlementId`, meters nothing (`usage_events` stays 1, `commands` stays 3); the create
  replay still answers its own stored receipt. Restore → the use replay answers again. With
  `subscriptions.payment` withdrawn the pay replay is `forbidden` with no `subscriptionId`, the
  `archava_subscriptions.subscriptions.revision` re-read equals the committed revision, and `commands` stays 3;
  restore → replay with that same revision. With `subscriptions.write` withdrawn the create replay is `forbidden`
  with no `subscriptionId`, the subscription count stays 1 and `commands` stays 3; restore → replay with the stored
  `subscriptionId`.

## Negative control (the tests are witnesses, not decoration)

Both hoisted gates were deleted (one line each), the two new tests re-run, then the files restored. Log
`/tmp/a1fu3-negative-control-rust.log`; the reproduction is exactly the finding:

```text
attendance.rs: dropped 1 hoisted gate line(s)   subscriptions.rs: dropped 1 hoisted gate line(s)
thread 'revoked_attendance_grants_refuse_the_stored_writes' panicked at crates/host/tests/erp_attendance.rs:1733:9:
  left: 200  right: 403
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 15 filtered out; finished in 70.14s
thread 'revoked_subscription_grants_refuse_the_stored_writes' panicked at crates/host/tests/erp_subscriptions.rs:1114:5:
  left: 200  right: 403
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 18 filtered out; finished in 57.25s
SHA_AFTER_RESTORE:
6b6e22891a53a8881c94a5a58e94dae3d64f67063ebc02c71634ae22a6be7793  rust/crates/adapters/src/erp/attendance.rs
a30d04e1979aae52ab5c7ac28f0be2f3f0bd29805afe5216d3163994fbf2ec75  rust/crates/adapters/src/erp/subscriptions.rs
```

The subscription control body is the stored receipt a revoked actor used to receive: `200` with
`"action":"consume_usage"`, `"idempotencyReplay":true`, `subscriptionId`, `usageEventId`, `entitlementId`. With the
gate in place the same bytes answer `403 forbidden` and no receipt fields at all.

## Gates

Single authoritative run `a1fu3-trio2` (`systemd-run --user --collect`, detached, `TMPDIR=/tmp`, log
`/tmp/a1fu3-trio2.log`). Source-tree fingerprints are unchanged by the run:

| gate        | command                                                                        | result                                         |
| ----------- | ------------------------------------------------------------------------------ | ---------------------------------------------- |
| fmt         | `cd rust && cargo fmt --all --check`                                           | `exit 0`, empty log (`/tmp/a1fu3-fmt.log`)     |
| clippy      | `cd rust && cargo clippy --all-targets --locked --offline -- -D warnings`      | `exit 0`, 0 warnings (`/tmp/a1fu3-clippy.log`) |
| trio        | `bash .audit-trio.sh` (`erp_attendance` + `erp_billing` + `erp_subscriptions`) | `EXIT=0` — **49 passed / 0 failed**            |
| docs format | `pnpm format:check`                                                            | `exit 0`                                       |

```text
START=2026-10-22T06:03:15+07:00
GIT_HEAD=412f763e17f3a68a457fd76c7b929184c50c916a
SCOPE_HASH_BEFORE=8fe16dc21e449dcd3ecc8ed630aaae60bdb8cdfac533d762534b91a10a5c2726
RUST_SCOPE_BEFORE=b325d301b89ce9cb3c48b61ce15004d813b6ab5c56fedc857dc43d93c58e9b99
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1029.89s   # erp_attendance
test result: ok. 14 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 950.92s    # erp_billing
test result: ok. 19 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 1163.86s   # erp_subscriptions
EXIT=0
END=2026-10-22T06:55:53+07:00
SCOPE_HASH_AFTER=8fe16dc21e449dcd3ecc8ed630aaae60bdb8cdfac533d762534b91a10a5c2726
RUST_SCOPE_AFTER=b325d301b89ce9cb3c48b61ce15004d813b6ab5c56fedc857dc43d93c58e9b99
AUDIT_TRIO_DONE=0
```

`SCOPE_HASH_BEFORE == SCOPE_HASH_AFTER` and `RUST_SCOPE_BEFORE == RUST_SCOPE_AFTER`: no file moved while the
suites ran. Both new tests are inside the trio (`revoked_attendance_grants_refuse_the_stored_writes ... ok`,
`revoked_subscription_grants_refuse_the_stored_writes ... ok`) and the older replay tests stay green
(`lost_response_replays_the_recorded_attendance_receipt`, `lost_response_replays_the_recorded_receipt`,
`a_same_grant_rival_never_replays_another_actors_subscription_receipt`), so a replayed initiator who still holds
the grant keeps `idempotencyReplay: true`.

This document is written after the run, deliberately: adding it before the trio would have made the run's own
`SCOPE_HASH_AFTER` describe a tree the suites never saw. `RUST_SCOPE` still equals `b325d301…` afterwards, which is
the check that the Rust source under test did not change after the trio.

A first trio invocation (`a1fu3-trio`, `05:14:05`, `SCOPE a7c6649e…`, `RUST 97670dba…`) was discarded rather than
reported: sibling card `t_90dc34a7` (A5-FU Rust, pg-fixture socket path) was still writing `rust/crates/pg-fixture`
while it ran, so its before/after fingerprints described a moving tree. This run started from the settled tree.

## Scope

Changed: the two adapters (hoist only), the two host test files (helpers + one test each), this doc.
Not changed: `billing.rs`, host resolvers, wire/JSON shapes, refusal shapes, any other card's source files.
`A1` is **not** claimed closed — the verdict stays with gate `t_be45b375`.

One foreign file was touched, formatting only: `docs/audit/A5FU_RUST_PG_SOCKET_2026-10-22.md` (card `t_90dc34a7`)
was written after that card's own format check and left `pnpm format:check` red for the whole repo
(`*emphasis*` rewritten to `_emphasis_`, table cell padding). It was normalised with `prettier --write` so the
mandated `pnpm format:check → 0` is a property of the real tree and not of a subset. The token stream before and
after is identical (1 240 tokens both sides, `diff` of the stripped text empty), so no word of that document
changed — `/tmp/a5fu-format-diff.patch`, `/tmp/a5fu-pre-format.md`.
