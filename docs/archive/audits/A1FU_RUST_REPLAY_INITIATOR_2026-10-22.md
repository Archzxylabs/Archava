# A1-FU (Rust) — the replay is bound to the recorded initiator

Scope: the Rust residual of the A1 finding (P1) that Codex recorded at
`runs/20261022-025930-audit.json` (2026-10-22 03:02) against gate `t_be45b375`:

> Rust billing also returns a replay without checking the recorded initiator at
> `rust/crates/adapters/src/erp/billing.rs:443`.

Card: `t_cf46760e` (`A1-FU (Rust)`). Tree: `412f763e17f3a68a457fd76c7b929184c50c916a`
plus the uncommitted A1 remediation. Claim format is `claim → file:line` plus the
command output that measured it; line numbers are post-change.

The TS sibling of this finding (`billing`/`attendance`/`subscriptions` repositories) carries
the same fix in its own card; this document is the Rust half only. **This card does not
claim A1 closed** — gate `t_be45b375` decides that through the Codex verdict.

## 1. The defect

> **Provenance note (read before judging attribution).** Codex cited `billing.rs:443`, which is
> a comment line ("`// absent, and fall through on a receipt-less row …`"), not the branch; the
> branch it describes sits at `billing.rs:444-457`. The tree is dirty end-to-end (`HEAD`
> `412f763e` predates the whole A1 remediation), so `git diff` cannot separate this card's edits
> from the earlier uncommitted remediation: the pre-image in a diff is `HEAD`, not the working
> tree as the gate audited it. What is verifiable and what this document asserts is the **end
> state**: grep shows exactly one actor comparison per claim path, and the three new negative
> tests drive that comparison directly (a same-grant rival gets `conflict`, not a receipt). An in-run comment at 03:07 argued the premise was
> already satisfied — it was written against hash `dab512f6`; the trio gate then ran against
> `75db1305`, i.e. the tree changed after that comment. The audit's own read-only snapshot
> (03:05) records `SELECT fingerprint,receipt` with no `actor_id` at `billing.rs:283` and the same
> shape at `attendance.rs:193` / `subscriptions.rs:245`.

The replay branch read the command row by `(tenant_id, request_id)` only, so
`fingerprint` was the sole comparison. A rival holding the _same_ write grant (a legitimate
grant holder that is simply not the actor who claimed the command) replayed the stored
receipt of somebody else. In `billing` the `grant(...)` gate did run before the lookup, so
the leak was never a zero-grant actor — it was a same-grant non-initiator, which is exactly
what A1's `actor_id=$3` binding forbids.

The claim now reads the recorded initiator with the row and compares it beside the
fingerprint, in the same refusal shape:

| adapter         | the row read (post-change)                                                                      | the comparison                                                                            |
| --------------- | ----------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `billing`       | `rust/crates/adapters/src/erp/billing.rs:291` — `SELECT actor_id,fingerprint,receipt …`         | `billing.rs:448` — `row_fingerprint != fingerprint \|\| row_actor != actor_id`            |
| `attendance`    | `rust/crates/adapters/src/erp/attendance.rs:193` — `SELECT receipt, fingerprint, actor_id …`    | `attendance.rs:207` — `stored_fingerprint != fingerprint \|\| recorded_actor != actor_id` |
| `subscriptions` | `rust/crates/adapters/src/erp/subscriptions.rs:245` — `SELECT receipt, fingerprint, actor_id …` | `subscriptions.rs:259` — same shape                                                       |

`attendance`/`subscriptions` return the row's `actor_id` through `try_get`, i.e. the recorded
initiator column of the same `SELECT … FOR UPDATE` that already serialised the claim — no
extra statement, no new lock. `billing` widens its existing `fetch_command_row` tuple from
`(fingerprint, receipt)` to `(actor_id, fingerprint, receipt)`; it is the only caller.

## 2. Invariants the change deliberately keeps

- **Grant before lookup.** `record_billing_command` still calls
  `grant(access, …required_permission())` before the claim (`billing.rs:438`), and `attendance`/`subscriptions` still resolve the
  membership envelope in the host before `claim_command` runs. Nothing was moved below the
  replay.
- **One refusal shape.** A foreign initiator refuses with the same `conflict()` the fingerprint
  mismatch uses — HTTP 409 `{"decision":"refused","reason":"conflict"}`. No new refusal class,
  no new wording, no new existence oracle: the caller learns "conflict", exactly what a
  mismatched payload already told it.
- **The legitimate replay is untouched.** The initiating actor still gets the stored receipt
  with `idempotencyReplay: true`; the receipt document and the wire/JSON shape are unchanged.
- **The partial-command fall-through survives.** A receipt-less row for the _same_ actor still
  re-executes inside the same transaction; a receipt-less row of a _foreign_ actor now refuses
  instead of re-executing somebody else's key.
- **Status reads were already correct** and were not touched: `billing.rs:1003`,
  `attendance.rs:828`, `subscriptions.rs:877` all bind the actor in the WHERE clause
  (`… AND actor_id=$3`), so a foreign key reads as `unknown`/not-committed.

## 3. Scan of every `.commands` row read in the adapters crate (task item 4)

`grep -rn "FROM archava_[a-z_]*\.commands" rust/crates/adapters/src` → 15 statements (12 row reads
on a write/claim path, 3 status reads). Every replay-path statement outside the trio already
selected **and** compared the recorded initiator; the trio was the only gap:

| adapter              | row read | actor compared         |
| -------------------- | -------- | ---------------------- |
| `purchasing.rs`      | `:720`   | `:801`                 |
| `commerce_kernel.rs` | `:727`   | `:837`                 |
| `foodservice.rs`     | `:299`   | `:470` (helper `:351`) |
| `organizations.rs`   | `:193`   | `:281`                 |
| `proposals.rs`       | `:198`   | `:448`                 |
| `projects.rs`        | `:299`   | `:341`                 |
| `scheduling.rs`      | `:429`   | `:523`                 |
| `accommodation.rs`   | `:783`   | `:929`                 |
| `assets.rs`          | `:312`   | `:394`                 |
| `billing.rs`         | `:291`   | `:448` ← fixed         |
| `attendance.rs`      | `:193`   | `:207` ← fixed         |
| `subscriptions.rs`   | `:245`   | `:259` ← fixed         |

The remaining statements are the domains' status reads. Their codes differ, and the difference
is a **read-side** matter, not a replay matter:

| status read                                              | how it treats the recorded initiator                                                          |
| -------------------------------------------------------- | --------------------------------------------------------------------------------------------- |
| `billing_command_status` (`billing.rs:990`)              | binds `actor_id=$3` in the WHERE — foreign key → not committed                                |
| `attendance_command_status` (`attendance.rs:817`)        | same (`attendance.rs:828`)                                                                    |
| `subscriptions_command_status` (`subscriptions.rs:865`)  | same (`subscriptions.rs:877`)                                                                 |
| `accommodation_command_status` (`accommodation.rs:1492`) | `row_actor != principal.id` → `forbidden` (`:1513`)                                           |
| `scheduling_command_status` (`scheduling.rs:840`)        | `row_actor != principal.id` → `forbidden` (`:857`)                                            |
| `food_command_status` (`foodservice.rs:413`)             | `row_actor != principal.id` → `forbidden` (helper `:351`)                                     |
| `assets_command_status` (`assets.rs:824`)                | only under `ErpScope::Assigned` (`:847`) — an `All`-scope reader reads a foreign receipt      |
| `organization_command_status` (`organizations.rs:739`)   | **no initiator check**: gates on the resource owner (`access.sees(&row_owner)`)               |
| `project_command_status` (`projects.rs:388`)             | **no initiator check**: gates on `owned_project(...)`                                         |
| `proposal_command_status` (`proposals.rs:522`)           | **no initiator check**: gates on `access.sees(&owner_id)` when the receipt carries a proposal |
| `command_status` (`purchasing.rs:886`)                   | **no initiator check**: gates on `recover_from_row(...)`                                      |

**Findings outside this card's scope — recorded only, not changed:**

- No module outside `rust/crates/adapters/src/erp/` touches a `*.commands` table
  (`grep -rn "\.commands" --include=*.rs rust/crates/adapters/src | grep -v "/erp/"` → empty), and
  no replay/recovery helper exists outside `erp/`
  (`grep -rn "idempotency_replay\|command_recovery\|replay_command" … | grep -v "/erp/"` → empty).
  So there is **no residual non-ERP adapter** with the same replay defect in this crate.
- The **write** path is now uniform: all 12 `.commands` row reads pair the fingerprint with the
  recorded initiator.
- The **status** path is not: `organizations`, `projects`, `proposals`, `purchasing` never look at
  `row_actor`, and `assets` only under `Assigned`. Whoever holds the _read_ grant (and, where
  present, the resource-owner scope) can therefore read the receipt of another actor's command —
  the read-side analogue of this finding, at a lower privilege. This card's scope is the replay
  path only, so it is recorded here for the `t_be45b375` verdict rather than fixed.
- `assets.rs:403`/`:847` and `scheduling.rs:857` are the same shape as the fix (actor compared
  beside scope) and were left untouched.

## 4. Tests added (one negative per binary, plus the positive regression)

`rust/crates/host/tests/erp_billing.rs`
`a_same_grant_rival_never_replays_another_actors_billing_receipt` — `bx` claims the create
(`idempotencyReplay: false`); `bxr` (BILLING_ALL, active, same tenant, **byte-identical** body)
gets 409 `conflict`, no `receipt`/`invoiceId` in the refusal, still one command row and one
invoice; the row's `actor_id` is `bx`; `bx`'s own replay returns the same `invoiceId` with
`idempotencyReplay: true`.

`rust/crates/host/tests/erp_attendance.rs`
`a_same_grant_rival_never_replays_another_actors_attendance_receipt` — `ax` claims;
`axv` (ATTENDANCE_ALL, active, scope `all`, same body) gets 409 `conflict`, no receipt, still
one command row / one session / one audit row; `actor_id` is `ax`; `ax`'s replay is unchanged.

`rust/crates/host/tests/erp_subscriptions.rs`
`a_same_grant_rival_never_replays_another_actors_subscription_receipt` — `sx` claims; `sxv`
(SUBSCRIPTION_ALL, active, same body) gets 409 `conflict`, no receipt, one command row / one
subscription / one entitlement; `actor_id` is `sx`; `sx`'s replay is unchanged.

Neighbouring regressions that ran green in the same binary:

- **billing** — `same_key_different_command_conflicts` (fingerprint mismatch → `conflict`),
  `lost_response_replays_the_recorded_receipt` (initiator replay positive),
  `revoked_billing_grant_refused_on_recovery` (revoked grant → `forbidden`),
  `insufficient_grants_refuse_before_any_statement` (grant before write).
- **attendance** — `the_same_attendance_key_a_different_command_conflicts`,
  `the_same_check_in_key_a_different_command_conflicts`,
  `lost_response_replays_the_recorded_attendance_receipt`,
  `revoked_attendance_grant_refused_on_recovery`.
- **subscriptions** — `the_same_key_a_different_command_conflicts`,
  `lost_response_replays_the_recorded_receipt`,
  `narrow_grants_refuse_exactly_the_arms_they_lack` (this binary carries no
  `revoked_*_grant_refused_on_recovery` test; the grant arm is the one above).

`revoked_billing_grant_refused_on_recovery` and `revoked_attendance_grant_refused_on_recovery`
are the T01 pair named in the card, unchanged and green.

`cargo test … -- --list` proves the three new names are top-level test functions:

```
a_same_grant_rival_never_replays_another_actors_attendance_receipt: test
a_same_grant_rival_never_replays_another_actors_billing_receipt: test
a_same_grant_rival_never_replays_another_actors_subscription_receipt: test
```

## 5. Gate output (measured, this tree)

```
cd rust && cargo fmt --all --check
FMT_CHECK_EXIT=0                       # no diff

cd rust && cargo clippy --all-targets --locked --offline -- -D warnings
Finished `dev` profile … in 3.46s
CLIPPY_EXIT=0                          # every erp_* integration target is in the artifact set

bash .audit-trio.sh   (LOG=/tmp/a1fu-trio.log, --test-threads=1)
test result: ok. 15 passed; 0 failed; 0 ignored; finished in 823.57s   # erp_attendance
test result: ok. 14 passed; 0 failed; 0 ignored; finished in 623.09s   # erp_billing
test result: ok. 18 passed; 0 failed; 0 ignored; finished in 841.96s   # erp_subscriptions
EXIT=0
AUDIT_TRIO_DONE=0
```

47 tests, 0 failed, `START=2026-10-22T03:09:03+07:00 → END=2026-10-22T03:47:20+07:00`
(38m17s). The wrapper's scope hashes are identical before and after the run
(`RUST_SCOPE_BEFORE == RUST_SCOPE_AFTER == 75db1305…`), i.e. the suite mutated no tracked
file. Raw log kept at `~/Archverse/Lab/_research/a1fu-rust-trio-2026-10-22.log`
(sha256 `3d87733044777094ba3fdc19de532deba646c3d455d650bda9d054237a7110fd`, 80 lines) and
attached to card `t_cf46760e`.
