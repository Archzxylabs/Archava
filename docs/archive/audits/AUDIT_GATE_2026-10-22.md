# PRE-GATE EVIDENCE PACK — A1 (command-recovery authorization) + A10 (assigned scope) + root gates

> **STATUS: NOT A VERDICT.** The Codex audit for card `t_be45b375` has **not** been run
> yet, and the Rust trio evidence in §3 is **stale by construction**: card `t_23799344`
> edited three Rust files (`crates/adapters/src/erp/subscriptions.rs`,
> `crates/host/src/erp/subscriptions.rs`, `crates/host/tests/erp_subscriptions.rs`) at
> 00:39–00:40 and again at ~00:49, i.e. after the trio binaries were compiled (00:06).
> `t_be45b375` is therefore parked as a **dependency** of `t_23799344`; when that card
> closes, the trio is re-run on the frozen final tree and the audit is invoked once.
> Do not read this file as acceptance of A1/A10.

- Date: 2026-10-22 (Asia/Jakarta)
- Repo: `/home/haikaru/Archverse/Lab/Archava_Platform`
- Base commit: `412f763e17f3a68a457fd76c7b929184c50c916a` (all remediation work is uncommitted — dirty-tree policy applies)
- Gate card: `t_be45b375` — final gate before A1/A10 are called closed
- Cards covered: `t_84230d16` (T02-FU), `t_2aac1772` (T02), `t_26f873d5` (T01-FU), `t_555a90c3` (T03), `t_23799344` (Rust ERP port — still **running**, parent of this gate card)

## 1. Tree under audit

```
git status --porcelain=v1 | wc -l → 203 dirty entries
git diff --stat                    → 168 files changed, 3987 insertions(+), 2530 deletions(-)
```

The tree is the accumulated, uncommitted output of the whole remediation program (T01,
T01-FU, T02, T02-FU, T03 plus the in-flight Rust port card). Per the dirty-tree policy
the verdict binds to a **scope hash** of this exact tree, not to a commit.

| hash                                               | value                                                              |
| -------------------------------------------------- | ------------------------------------------------------------------ |
| `HEAD`                                             | `412f763e17f3a68a457fd76c7b929184c50c916a`                         |
| scope hash (worktree status+diff, 00:04:58)        | `05d87110798513e95936b0cb8e372d1acc56c8939f6868af0faf5654e06b4720` |
| rust-only scope hash (00:04:58)                    | `8ad390c38f8d67ccc331a3a0d286d5a2d606c231e407ead24172b7b274426565` |
| scope hash (00:41:09, after trio)                  | `d041028f304e09464f1b1e172a11097fa87c352fd1c7606ebae22fe986bfce9a` |
| rust-only scope hash (00:41:09)                    | `1687a7d6647b632087f2e455e5c3b64789422a25d7f2ac5adf257ed524659672` |
| rust-only scope hash (00:49:17, at trio re-launch) | `b66268867cb72899a3fba7454d3e59027c2ee2f6f39e61e5283722c91c42afc8` |

The worktree-hash delta between 00:04:58 and 00:41:09 is explained entirely by files this
gate card itself wrote (this document, `.audit-trio.sh`); the **rust** hash delta is not —
it is card `t_23799344` editing the ERP subscriptions surface while the trio ran. That is
why the trio result below cannot certify the current tree.

## 2. What A1/A10 actually changed (semantic surface)

### A1 — command recovery must be bound to the initiating principal

Receipt recovery SQL now keys the lookup on the recorded actor inside the query
(`actor_id = $3`, `created_by = $3` for field-services), so a read grant alone cannot
recover another principal's receipt:

| package           | statement                                                                                                                       |
| ----------------- | ------------------------------------------------------------------------------------------------------------------------------- |
| billing-pg        | `SELECT receipt FROM archava_billing.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`                             |
| subscriptions-pg  | `SELECT receipt FROM archava_subscriptions.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`                       |
| attendance-pg     | `SELECT receipt FROM archava_attendance.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`                          |
| laundry-pg        | `SELECT receipt FROM archava_laundry.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`                             |
| fulfillment-pg    | `SELECT receipt FROM archava_fulfillment.commands WHERE tenant_id=$1 AND request_id=$2 AND actor_id=$3`                         |
| capacity-pg       | `SELECT receipt FROM archava_capacity.commands WHERE tenant_id=$1 AND command_id=$2 AND actor_id=$3`                            |
| proposals-pg      | `SELECT receipt FROM archava_proposals.commands WHERE tenant_id=$1 AND command_id=$2 AND actor_id=$3`                           |
| field-services-pg | `SELECT command_id, action, job_id, receipt FROM archava_field.commands WHERE tenant_id=$1 AND command_id=$2 AND created_by=$3` |

The read grant is still enforced **before** the lookup; a foreign actor gets the same
`unknown` document as a missing row, with no receipt fields.

### A10 — assigned scope on parent and child reads

Eight Postgres repositories gate list/detail and every child read on
`access.scope === 'assigned'`, binding `access.ownerId` **inside** SQL, with the owner
predicate applied **before** pagination:

```
packages/attendance-pg/src/repository.ts     |  59 ++++++++++---
packages/billing-pg/src/repository.ts        |  73 ++++++++++-----
packages/capacity-pg/src/repository.ts       |  31 +++----
packages/field-services-pg/src/repository.ts |  13 ++-
packages/fulfillment-pg/src/repository.ts    |  12 ++-
packages/laundry-pg/src/repository.ts        |  67 ++++++++++----
packages/proposals-pg/src/repository.ts      |  31 +++----
packages/subscriptions-pg/src/repository.ts  | 126 ++++++++++++++++++++-------
8 files changed, 282 insertions(+), 130 deletions(-)
```

Representative added predicates: `access.scope === 'assigned' ? ' AND owner_id=$3' : ''`
(parent list/detail) and the same conditional for every child reader
(entitlements, roster/check-in, lines/process/findings, usage events).

### Rust port of A1/A10 surfaces (in-tree, sibling card `t_23799344`)

`rust/crates/adapters/src/erp/{billing,subscriptions,attendance}.rs`,
`rust/crates/host/src/erp/{billing,subscriptions,attendance}.rs`,
`rust/crates/host/src/router.rs`, and the three integration suites under
`rust/crates/host/tests/` — included in the cargo trio evidence in §3, but owned and
accepted by card `t_23799344`, not by this gate. That card's audit round 1 returned
`changes_requested` with two P2 findings (`runs/20261022-003035-audit.json`):
`read_usage_events` ignoring `assigned` ownership, and a panic on a malformed `targetId`.
Both are being fixed there; neither is closed by this pack.

### Gate restoration (T03)

`eslint.config.mjs`, `package.json`, `.prettierignore`, `tools/**`, plus Prettier-only
reformatting carried by the same tree. Restored gates are re-run in §3.

## 3. Gate evidence (exact commands, real exit codes, raw logs)

All commands were run in this worktree from a detached `systemd-run --user` unit for the
long Rust job so no worker budget could truncate it. Raw logs are on disk and are passed
to the auditor verbatim.

| #   | command                                                                                                                                                                           | exit | log                          | result                                                                                                                                                                                                                                 |
| --- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---- | ---------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | `pnpm exec vitest run packages/{billing,subscriptions,attendance,laundry,field-services,formal-education,fulfillment}-pg/test`                                                    | 0    | `/tmp/audit-vitest.log`      | 16 files / 208 tests passed                                                                                                                                                                                                            |
| 2   | `pnpm exec vitest run packages/capacity-pg/test packages/proposals-pg/test`                                                                                                       | 0    | `/tmp/audit-vitest-a1fu.log` | 2 files / 6 tests passed                                                                                                                                                                                                               |
| 3   | `TMPDIR=/tmp cargo test --offline --locked --manifest-path rust/Cargo.toml -p archava-host --test erp_billing --test erp_subscriptions --test erp_attendance -- --test-threads=1` | 0    | `/tmp/audit-trio-run1.log`   | 42 passed / 0 failed — erp_attendance 14 (818.96 s), erp_billing 13 (617.96 s), erp_subscriptions 15 (723.37 s); `START=00:04:58`, `END=00:41:09`. **Valid only for the 00:04:58 tree** (binaries compiled 00:06); superseded — see §1 |
| 4   | `pnpm format:check`                                                                                                                                                               | 0    | `/tmp/audit-fmt.log`         | `All matched files use Prettier code style!`                                                                                                                                                                                           |
| 5   | `pnpm lint` (`node --max-old-space-size=6144 eslint . --max-warnings=0`)                                                                                                          | 0    | `/tmp/audit-lint.log`        | 0 errors, 0 warnings                                                                                                                                                                                                                   |
| 6   | `pnpm typecheck`                                                                                                                                                                  | 0    | `/tmp/audit-typecheck.log`   | all workspaces clean                                                                                                                                                                                                                   |
| 7   | `pnpm test`                                                                                                                                                                       | 0    | `/tmp/audit-jstest.log`      | 145 files / 1896 tests passed                                                                                                                                                                                                          |
| 8   | `pnpm library:pack`                                                                                                                                                               | 0    | `/tmp/audit-gate3.log`       | 62 compiled internal packages packed                                                                                                                                                                                                   |
| 9   | `node tools/library/verify-<domain>.mjs` × 7 (`billing subscriptions attendance laundry field-services formal-education fulfillment`)                                             | 0    | `/tmp/audit-gate3.log`       | all domain checks passed (19 + 18 + 22 + … in the log)                                                                                                                                                                                 |

### Trio re-run protocol (executed when `t_23799344` closes)

1. Confirm the box is quiet: `pgrep -af 'cargo test'` empty, and the rust scope hash
   stable across 60 s.
2. `systemd-run --user --unit=audit-trio-be45b375-r3 --collect --setenv=LOG=/tmp/audit-trio.log .audit-trio.sh`
   (the wrapper records `GIT_HEAD`, `SCOPE_HASH_BEFORE/AFTER`, `RUST_SCOPE_BEFORE/AFTER`,
   `EXIT`, `END`).
3. Accept the run only if `RUST_SCOPE_BEFORE == RUST_SCOPE_AFTER` and `EXIT=0`.

## 4. Conformance oracle tests (the behaviors this gate certifies)

A1 — recovery binding (capacity-pg, proposals-pg):

- `recovers its own receipt and answers every foreign key as unknown`
- `keys the receipt lookup on the recorded actor inside the query`
- `refuses recovery when the read grant is missing or withdrawn`

A10 — assigned scope (billing-pg, subscriptions-pg, attendance-pg, laundry-pg):

- `lists only the reader-owned <resource> under assigned scope`
- `leaves all-scope readers with the whole tenant page`
- `applies the owner predicate before pagination, not after it`
- `hides a foreign <resource> before any child is read, exactly like a missing one`
- `guards every child reader independently under assigned scope`
- `keeps child reads unscoped for an all-scope reader`
- `keeps foreign-tenant <resources> out of both list and children`
- `still requires the read grant before any scope filter runs`
- subscriptions additionally: `never treats a contact id as a subscription id`,
  `resolves entitlements by contact reference for an owned contact`,
  `keeps a subscription-less entitlement visible only under all scope`,
  `never reaches usage events through a subscription-less entitlement under assigned scope`,
  `binds a delegated service principal to the human owner and narrows scope`

## 5. Deviation from the card's evidence plan (declared, with reason)

The card lists the cargo trio as an `--evidence` command inside `orchestrate.sh`. It is
instead executed once, directly, as a detached systemd unit and its raw log is handed to
the auditor (`--evidence "grep -E '...' /tmp/audit-trio.log"`). Reasons:

1. The trio takes ~21 minutes on this machine under load, and a second concurrent run is a
   known failure mode (an earlier same-tree concurrent run was SIGTERM'd with exit 143),
   so it is run exactly once, when no other card is driving cargo.
2. Running it inside the audit would double the wall clock of the gate without adding
   evidence: the surface hash and the log's own `START`/`END`/`SCOPE_HASH` lines bind the
   run to exactly the tree the auditor will inspect.

`pnpm format:check` is re-run live by the auditor, so at least one surface gate is
independently reproduced.

## 6. Findings explicitly NOT closed by this gate

From `docs/audit/ARCHLAB_AUDIT_2026-10-21.md`, these stay open and must not be read as
resolved: A4 (evidence drift), A6 (coverage threshold), A7 (systemd stale units),
A8 (headers/cookie), A9 (supply chain). Also open by construction: the Rust ERP card
`t_23799344` carries two open P2 findings of its own
(`runs/20261022-003035-audit.json`), so any acceptance of its Rust changes is its own
card's business, not this gate's.

## 7. Owner decision binding this audit (2026-10-21, option A)

A1 remediation stays two-language: the TypeScript patch is limited to
`grant(<domain>.read)` + binding the actor into the query + tests, with no new
abstractions and no refactor spillover; the TS surface doubles as the conformance oracle
for the later Rust cutover. Security remediation and the migration program are separate
workstreams. Any TS change in the diff beyond that shape is scope creep and should be
flagged in the verdict.

> **Measurement drift:** the counts above were taken at **00:04:58**. By **00:52** the same two commands returned `205` dirty entries and `168 files changed, +4199 / -2542`, because card `t_23799344` kept patching the Rust surface. Re-measure both at audit time and quote the fresh pair; the scope hash, not the count, is what the verdict binds to.
