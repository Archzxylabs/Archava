# T02 follow-up (Rust) — `read_entitlements` guessed its target kind, and its contact branch had no owner predicate

Task: `t_23799344` (Rust follow-up of `t_84230d16`, itself the follow-up of T02 / A10).
Date: 2026-10-21. Scope: `rust/crates/adapters`, `rust/crates/host` (subscriptions ERP surface only).

## Defect

`rust/crates/adapters/src/erp/subscriptions.rs` `read_entitlements(connection, tenant, access, target_id)`
carried the same two defects the TS port had already fixed in `t_84230d16`, plus one the TS never
had:

1. **Value-shape routing.** Line 1022 chose the branch with
   `archava_domain::erp::valid_uuid(target_id)`. `entitlements.contact_id` is `text`, but every
   contact id the schema admits is uuid-shaped (`entitlementSchema.contactId` is `z.string().uuid()`
   in TS), so every contact read entered the _subscription_ branch, the parent probe found no
   subscription with that id, and the read answered `not_found`. The contact branch was unreachable
   through any public path — a functional defect, not dead code.
2. **No owner predicate on the contact branch.** The contact statement was plain
   `WHERE tenant_id=$1 AND contact_id=$2`. Under `access.scope == "assigned"` any grant holder could
   read another owner's entitlements by knowing the contact reference — an authorization leak.
   `read_subscription_detail` already enforced `assigned` ownership; this read did not.
3. **No owner predicate on the subscription probe.** The probe was
   `SELECT id FROM ...subscriptions WHERE tenant_id=$1 AND id=$2`, so an `assigned` caller could
   read any subscription's entitlements whenever it knew the subscription id. Again, the detail read
   already refused exactly that.

Host documentation promised the impossible branch: `rust/crates/host/src/erp/subscriptions.rs`
lines 31-34 said "a uuid target reads one subscription's entitlements … any other target reads a
contact's entitlements".

## Decision

Identical to the TS contract fixed in `t_84230d16` — the discriminator is the **function**, not the
value and not a fallback chain:

| Entry point                                                                               | Kind         | Change                                                                                                                                                   |
| ----------------------------------------------------------------------------------------- | ------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `adapters::read_entitlements(conn, tenant, access, subscription_id)`                      | subscription | `valid_uuid` inference and the contact branch removed; parameter renamed `target_id` → `subscription_id`; probe gains `AND owner_id=$3` under `assigned` |
| `adapters::read_contact_entitlements(conn, tenant, access, contact_id)`                   | contact      | new; `WHERE e.tenant_id=$1 AND e.contact_id=$2`, ownership via `EXISTS` on the tenant-scoped parent, `ORDER BY e.created_at DESC`                        |
| `host::subscription_entitlements` (`GET /erp/subscriptions/entitlements`)                 | subscription | unchanged wire (`targetId` kept for consumers), doc narrowed to the subscription read                                                                    |
| `host::subscription_contact_entitlements` (`GET /erp/subscriptions/contact-entitlements`) | contact      | new route, `tenant` + `contactId`                                                                                                                        |

Rejected: inferring kind from the value (that _is_ the bug), a sequential subscription-then-contact
fallback (two round-trips, and it blurs the `not_found` indistinguishability contract), and deleting
the contact read (the capability is promised and the TS tool consumes it).

Ownership is `EXISTS (SELECT 1 FROM archava_subscriptions.subscriptions p WHERE p.tenant_id = e.tenant_id AND p.id = e.subscription_id AND p.owner_id = $3)`,
never an inner join: `entitlements.subscription_id` is nullable, so a subscription-less entitlement
has no owner to inherit — it must stay visible under `all` scope and be hidden under `assigned`,
and an inner join would silently drop it from `all` too.

Foreign equals missing: a foreign subscription answers `not_found` (probe with the owner predicate,
so no child row is read at all), and a foreign or unknown contact answers `[]`. No `403`, no
existence oracle.

Unchanged on purpose: `require(access, "subscriptions.read")` still runs before any statement in
both reads; subscription ordering is still `created_at ASC`; contact ordering is `created_at DESC`;
`read_subscription_detail`, `read_usage_events` and the command-status binding were not touched in
round 1. Round 2 below closes the usage read's missing owner predicate — the first audit's P2-1.

## Verification

| Gate                       | Command                                                                                                | Result                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| -------------------------- | ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Focused suite              | `TMPDIR=/tmp cargo test -p archava-host --offline --test erp_subscriptions`                            | **15 passed, 0 failed** (446.57s), including both new witnesses                                                                                                                                                                                                                                                                                                                                                                                            |
| Both crates, card command  | `TMPDIR=/tmp cargo test -p archava-adapters -p archava-host --offline` (default thread count)          | every binary green — `archava-adapters` lib 13, `tenant_context` 11; `archava-host` lib 41, `assistant` 14, `authority` 18, `crm` 36, `erp_accommodation` 10, `erp_assets` 10, `erp_attendance` 14 (473s), `erp_billing` 13 (339s), `erp_foodservice` 9, `erp_organizations` 9, `erp_projects` 9, `erp_proposals` 9, `erp_purchasing` 9, `erp_scheduling` 9, `erp_subscriptions` 15 (447s), `kdf_budget` 3 — **except** `tests/own.rs` 3 passed / 8 failed |
| `tests/own.rs`, serialized | `TMPDIR=/tmp cargo test -p archava-host --offline --test own -- --test-threads=1`                      | **11 passed, 0 failed, exit 0** (135.49s)                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Remaining binaries         | `TMPDIR=/tmp cargo test -p archava-host --offline --test readiness --test session -- --test-threads=1` | `readiness` 5 passed, `session` 6 passed, **exit 0** (cargo stops after a failing binary, so these two — alphabetically after `own` — had to be run explicitly)                                                                                                                                                                                                                                                                                            |
| Lints                      | `cargo clippy --workspace --all-targets --offline -- -D warnings`                                      | **exit 0**                                                                                                                                                                                                                                                                                                                                                                                                                                                 |

The 8 `own.rs` failures under the default thread count are all `503 busy` at _login_, raised before
any business code runs. That is this repo's shipped KDF derivation budget — `crates/host/src/identity.rs`
lines 54-56: "Concurrent scrypt derivations before the baseline's `busy` refusal. Two", a process-wide
counter honoured by `tests/kdf_budget.rs` ("the third concurrent login is refused busy", 3 passed in
the same run). `own.rs` has eight tests that log in at once, so the default 16-thread runner is over
budget by construction; with `--test-threads=1` the same binary is 11/11. The crate's documented
command is `-- --test-threads=1` (`.audit-trio.sh`, `docs/rust/EXECUTION_STATE.md:931`,
`docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md`), and this card's diff contains no login, identity or KDF
change at all.

### Behaviour-level witnesses (real router, real PostgreSQL)

`rust/crates/host/tests/erp_subscriptions.rs`:

- `assigned_scope_binds_both_entitlement_reads_to_the_owner` — two subscriptions for the same
  contact, owned by different principals; the `assigned` caller reads its own subscription (200), its
  own contact entitlement **only** (1 row, its own `subscriptionId`), a foreign subscription as
  `404` (indistinguishable from missing), the `all`-scope manager sees both rows on that contact, and
  the subscription route answers `404` for the uuid-shaped contact reference (no silent fallthrough
  into the contact read).
- `contact_reads_keep_subscription_less_rows_for_all_scope_only` — two entitlements with
  `subscription_id IS NULL` and explicit instants: `all` scope returns all three rows in
  `created_at DESC` order (the `EXISTS` shape, not a join), `assigned` returns `[]` (nothing to
  inherit), and the subscription read is unchanged.

### Mutation controls

Each mutation was applied to the working tree, the witness re-run, and the tree restored from a
backup copy (`grep` count re-checked afterwards).

| Control | Mutation                                                              | Observed                                                                                                                                                                                                                    |
| ------- | --------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| A       | contact `EXISTS … AND p.owner_id=$3` → `($3 IS NOT NULL)`             | **FAILED** `assigned_scope_binds…`: `left: 2, right: 1` — the assigned caller's contact read returned the manager's entitlement too                                                                                         |
| B       | probe `… AND id=$2 AND owner_id=$3` → `($3 IS NOT NULL)`              | **FAILED** `assigned_scope_binds…`: `left: 200, right: 404` — the foreign subscription read answered with the other owner's rows                                                                                            |
| C       | host contact read `read_contact_entitlements(` → `read_entitlements(` | **FAILED** `assigned_scope_binds…`: `left: 404, right: 200`, `{"decision":"denied","reason":"unknown_resource"}` — the exact `isUuid`-gate answer, reproduced by routing the contact read back into the subscription branch |
| —       | all three restored                                                    | focused suite green again (15/15 above)                                                                                                                                                                                     |

## Round 2 — the second audit's two P2 findings (`runs/20261022-003035-audit.json`)

The first audit returned `changes_requested` with two P2 findings. Both are closed here; nothing from
round 1 was reverted. The first audit's own note is kept: the usage-history gap exists in `HEAD`, it
is T02's contract not yet ported — but it is inside this card's surface, so it is closed here rather
than deferred a fourth time.

### P2-1 — the usage history ignored `assigned` ownership

`adapters::read_usage_events` checked `subscriptions.read` and then filtered on
`tenant_id`/`entitlement_id` only, and `host::subscription_usage` forwarded straight through, so an
`assigned` caller who knew another owner's entitlement id could read its usage history.

The read now mirrors the TS `listUsageEvents()`: under `assigned` the owner predicate is an `EXISTS`
that walks the whole path — `usage_events u → entitlements e → subscriptions p`, `p.owner_id=$3` —
and `all` is unchanged. The join inside the `EXISTS` is deliberate: `entitlements.subscription_id` is
nullable, so a usage event on a subscription-less entitlement inherits no owner — `all` keeps it,
`assigned` hides it. Owned, foreign, missing and subscription-less all answer with an array
(`[]` for everything but the owned case), so the read stays silent about which ids exist.

The host module doc (`src/erp/subscriptions.rs:40-43`) now states the same rule instead of calling
the route plain "the `listUsageEvents()`".

### P2-2 — malformed `targetId` panicked after the inference was removed

Correct, and it was introduced by round 1: `targetId` is an unrestricted query string
(`src/erp/subscriptions.rs:296`) and `read_entitlements` handed it straight to `uuid_of`
(`Uuid::parse_str(..).expect(..)`).

`read_entitlements` now parses fallibly and returns the same structured `unknown_resource` refusal a
missing subscription gets — no new refusal shape, no existence oracle, `targetId` kept, routing still
by route. The contact read is untouched: its argument is `text` and never parsed.

Round 1's regression test for this (`malformed_target_ids_answer_not_found_instead_of_panicking`) had
been written **nested inside the previous test function**, so it compiled as an unnameable item and
never ran. The round-2 tree closes the missing brace; `clippy … -D warnings` rejects
`unnameable_test_items`, which is why the lint gate is part of this round's evidence rather than a
formality.

### Verification after round 2

| Gate                       | Command                                                                                                | Result                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| -------------------------- | ------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Focused suite              | `TMPDIR=/tmp cargo test -p archava-host --offline --test erp_subscriptions`                            | **17 passed, 0 failed** (477.63s) — round 1's 15 plus the usage-history witness and the malformed-`targetId` regressions                                                                                                                                                                                                                                                                                                                                                                              |
| Full card command          | `TMPDIR=/tmp cargo test -p archava-adapters -p archava-host --offline` (default threads)               | every binary green — adapters lib 13, `tenant_context` 11; host lib 41, `assistant` 14, `authority` 18, `crm` 36, `erp_accommodation` 10, `erp_assets` 10, `erp_attendance` 14 (408.47s), `erp_billing` 13 (329.59s), `erp_foodservice` 9, `erp_organizations` 9, `erp_projects` 9, `erp_proposals` 9, `erp_purchasing` 9, `erp_scheduling` 9, `erp_subscriptions` **17 (468.79s)**, `kdf_budget` 3 — **except** `tests/own.rs` 3 passed / 8 failed, the documented KDF-budget artifact (see round 1) |
| `tests/own.rs`, serialized | `TMPDIR=/tmp cargo test -p archava-host --offline --test own -- --test-threads=1`                      | **11 passed, 0 failed, exit 0** (98.52s)                                                                                                                                                                                                                                                                                                                                                                                                                                                              |
| Remaining binaries         | `TMPDIR=/tmp cargo test -p archava-host --offline --test readiness --test session -- --test-threads=1` | `readiness` 5 passed, `session` 6 passed, **exit 0**                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| Lints                      | `cargo clippy --workspace --all-targets --offline -- -D warnings`                                      | **exit 0**                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |

### Behaviour-level witness (new)

`assigned_scope_binds_the_usage_history_to_the_entitlement_owner` — one subscription per owner on the
same contact, plus a usage event on a subscription-less entitlement. The `assigned` caller reads one
row for its own entitlement, `[]` for the manager's entitlement (same bytes as a missing id) and `[]`
for the owner-less entitlement; the `all`-scope manager reads one row for the manager's entitlement
and one for the owner-less one. The assertion compares whole rows and counts, never a rendered string.

### Mutation controls

Applied to the working tree, witness re-run, tree restored from the pre-mutation backup and verified
byte-for-byte (`sha256 873c8e4e8c236d93e71a9eaec8952feea7513d918d3992b364c28761ffd17cef`, the same
bytes that produced the 17/17 run above; the filtered witness is green again, 57.71s).

| Control | Mutation                                                                                        | Observed                                                                                                                                                                                                                                           |
| ------- | ----------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D       | usage `EXISTS … AND p.owner_id=$3` → `($3 IS NOT NULL)`                                         | **FAILED** `assigned_scope_binds_the_usage_history…`: `foreign == missing: [{… "idempotencyKey":"usage-manager" …}]`, `left: Some(1), right: Some(0)` — the assigned caller read the manager's history                                             |
| E       | an inner-join predicate leaked into the `all` branch (`AND EXISTS … JOIN …` without `owner_id`) | **FAILED** the same witness: `all scope keeps history whose entitlement has no subscription: []`, `left: Some(0), right: Some(1)` — the subscription-less row disappeared from `all`, which is exactly why ownership is an `EXISTS` and not a join |

Round 1's controls A/B/C remain as recorded above; D/E are added for the read this round touched.

## Notes for the auditor

- `rust/crates/adapters/src/erp/subscriptions.rs` also carries an **uncommitted, pre-existing**
  change from `t_9db93882` (`subscriptions_command_status` now takes `&SubscriptionAccess` and binds
  `actor_id=$3`). It is not part of this card and was not modified here; the diff hunk is adjacent.
- `rust/crates/adapters/src/erp/{billing,attendance}.rs` and the matching host/tests files are the
  same sibling card's uncommitted work. This card's mutation controls restored files byte-for-byte
  from backups taken before the first mutation.
- `read_usage_events` **is** part of this card as of round 2 (owner predicate under `assigned`), so its
  diff in `rust/crates/adapters/src/erp/subscriptions.rs` is this card's, not the sibling's.
- No wire/JSON contract changed for existing consumers: `GET /erp/subscriptions/entitlements` keeps
  its shape and its `targetId` parameter; the contact read is an additive route. No TypeScript
  changed.

## Audit verdict (round 2)

`runs/20261022-014232-audit.json` — **`approve`, zero findings.**

The auditor read the code and the tests rather than the summary, and checked the claims it could
check without a writable tree:

- subscription-only routing, fallible `targetId` parsing and the shared missing-resource refusal at
  `adapters/src/erp/subscriptions.rs:1023` and `host/src/erp/subscriptions.rs:203`;
- contact ownership as a tenant-correlated `EXISTS` only under `assigned`, `created_at DESC`
  (adapter `:1084`); usage ownership on the `usage_events → entitlements → subscriptions` path only
  under `assigned`, with `all` still keeping subscription-less history (`:1120`);
- the router-level assertions at `tests/erp_subscriptions.rs:1434`, `:1550`, `:1646`, `:1670`;
- `target/debug/deps/erp_subscriptions-3d94755ae2c132ac --list` → **17 tests**, explicitly including
  `malformed_target_ids_answer_not_found_instead_of_panicking` — the test round 1 wrote nested and
  never ran is now nameable and collected;
- the adapter's SHA-256 **independently matches** the recorded post-restore hash
  `873c8e4e8c236d93e71a9eaec8952feea7513d918d3992b364c28761ffd17cef`, so the tree the controls ran
  on is the tree being approved;
- `read_subscription_detail` (`:987`) and the command-status binding (`:870`) are still intact — no
  T02 owner-scoping was reverted.

Runtime results and controls A-E were taken from the recorded evidence by necessity: the audit
sandbox is read-only, so it cannot re-run the suite. Its approval is explicitly scoped to this card's
Rust entitlement/usage changes; the attendance, billing and pre-existing `subscriptions_command_status`
hunks in the shared working tree are the sibling card's and were excluded.
