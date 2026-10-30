# A11 — residual authz hardening (Rust + TS)

Date: 2026-10-22 · Branch: `feat/laundry-1.17.0` · Card: `t_897bdb30`
Source findings: `docs/audit/ARCHLAB_AUDIT_2026-10-21.md:266-277` (A11),
`docs/audit/archlab-2026-10-21-authz.md` F-05…F-10.

Scope: `rust/crates/adapters/src/erp/**`, `rust/crates/host/src/router.rs` (+ related host
tests), `rust/crates/host/src/identity.rs` (the per-source budget lives there),
`apps/crm/src/host.ts` + `apps/crm/test/host.test.ts` for the tenant oracle.
`rust/crates/own/**` and `rust/crates/slice/**` were not touched by this card (their dirty
state predates it: mtimes 08:39/08:42/09:29 vs. this card's first edit after 10:30) and the
pre-existing dirty tree was preserved (no `git add/commit/stash/revert`).

**Status: all three items landed and proven red→green.** Evidence logs under
`docs/audit/evidence/a11/`.

| item           | defect                                                                                | state                                                                                                 |
| -------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| P2 / F-05…F-08 | status read organizations/projects/proposals/scheduling ignored the domain READ grant | fixed, 4 domains; RED+GREEN for orgs/projects/proposals, scheduling pinned by a green actor test (§1) |
| P3 / F-09      | one global login bucket ⇒ 60 failures from anywhere closed sign-in for everyone       | fixed, per-source bucket with a bounded registry, RED+GREEN                                           |
| P3 / F-10      | 404 (unknown tenant) vs 403 (known, non-member) tenant-existence oracle               | fixed, RED+GREEN                                                                                      |

---

## 1. P2 — status read now answers to the domain READ grant (DONE)

### Defect (verified in tree, not just in the audit)

| adapter fn                                                       | before                                                                                                                           |
| ---------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| `adapters/src/erp/organizations.rs::organization_command_status` | only `principal.kind` gate, then `fetch_command_row`, then `access.sees(owner)`. Actor discarded (`_`), no `organizations.read`. |
| `adapters/src/erp/projects.rs::project_command_status`           | actor discarded (`_`), no `projects.read`, **no actor gate at all**.                                                             |
| `adapters/src/erp/proposals.rs::proposal_command_status`         | actor discarded (`_`), no `proposals.read`, only `access.sees(owner)`.                                                           |
| `adapters/src/erp/scheduling.rs::scheduling_command_status`      | actor gate present, but no `resources.read` / `reservations.read`.                                                               |

Consequence: a member holding only `audit.read` (or any membership at all) recovered
another actor's committed receipt — project id, organization id, proposal id, revisions,
committed-at — i.e. the full ledger row, not just `access.sees(owner)`.

### Fix — the A1 order: grant FIRST, then the ledger, then the actor binding

Applied to all four domains, using only helpers already in the crate
(`access.holds(&[...])`, `forbidden()`, `fetch_command_row`, the existing per-domain
`*Access` types from `crates/domain`). No new abstraction, no signature change.

```rust
if principal.kind.as_str() != "human" { return Err(forbidden()); }
// The status transaction's own grant (TS twin: grant(access, '<domain>.read')).
if !access.holds(&["<domain>.read"]) { return Err(forbidden()); }
let Some((row_actor, _, row_receipt)) = fetch_command_row(...).await? else { /* not committed */ };
if row_actor != principal.id { /* see table */ }
```

Landed at: `organizations.rs:749`, `projects.rs:398`, `proposals.rs:532`,
`scheduling.rs:850` (plus `scheduling.rs:869` for the reservation arm).

| domain        | grant (checked before the ledger read)                                                                       | foreign actor result                                                                                        |
| ------------- | ------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------- |
| organizations | `organizations.read`                                                                                         | `403 forbidden`                                                                                             |
| projects      | `projects.read`                                                                                              | `403 forbidden` (TS `projectCommandStatus` parity)                                                          |
| proposals     | `proposals.read`                                                                                             | `200 {"decision":"unknown"}` — identical to an id never written (TS binds `actor_id` in the key; no oracle) |
| scheduling    | `resources.read`, plus `reservations.read` when the stored receipt is a reservation (`receiptAccess` parity) | `403 forbidden` (gate already existed)                                                                      |

Reference implementations in-crate used as the pattern: `assets.rs::assets_command_status`
(grant always, actor gate) and `accommodation.rs` (grant + actor gate). TS twins:
`packages/{organizations,projects,proposals,scheduling}-pg/src/repository.ts`
(`commandStatus`) — all four already `grant(access, '<domain>.read')` plus an actor binding;
the Rust ports had dropped both halves.

### RED-BY-DESIGN evidence (before the fix)

```
crates/host/tests/erp_organizations.rs:1378  assertion `left == right` failed:
  audit.read is not organizations.read: {"decision":"committed","receipt":{... "revision":1,"type":"create_organization"}}
  left: 200   right: 403
crates/host/tests/erp_projects.rs   audit.read is not projects.read:
  {"decision":"committed","receipt":{"id":"67c6e9b5-…","projectRevision":1,…}}  left: 200 right: 403
crates/host/tests/erp_proposals.rs  audit.read is not proposals.read:
  {"commandId":"a11d…","decision":"committed","receipt":{…"status":"committed"}}  left: 200 right: 403
```

Raw logs: `docs/audit/evidence/a11/red-erp_projects.log`, `red-erp_proposals.log`
(the organizations run is quoted verbatim above; `red-erp_scheduling.log` is the actor-gate
case discussed below).

### GREEN after the fix

```
test status_read_requires_the_organizations_read_grant ... ok      (1 passed; 26.62s)
test status_read_requires_the_projects_read_grant     ... ok      (1 passed; 28.55s)
test status_read_requires_the_proposals_read_grant    ... ok      (1 passed; 28.19s)
```

Logs: `docs/audit/evidence/a11/green-erp_{organizations,projects,proposals}.log`.
Each test asserts, in order: the author's commit is `OK`; an `audit.read`-only member is
`403 forbidden` (the grant); a same-tenant full-grant peer is refused (the actor binding;
proposals: `unknown`); the author still recovers `decision: committed`.

### Honest note on scheduling

`scheduling_command_status` already had `row_actor != principal.id ⇒ 403`, so the new
scheduling test (`status_read_requires_the_scheduling_read_grant`) was **green before the
fix too** — it pins the actor binding, it is not a red proof of the grant. The grant gap
that was real there: an author whose `resources.read` is later revoked (membership still
active) could still read their own receipt, and a `resources.read`-only member could read a
**reservation** receipt without `reservations.read`. Both are closed by the added lines
(`scheduling.rs:850`, `:869`); neither has a dedicated red run yet — a red proof needs a
partial `permissions` UPDATE on `archava_scheduling.memberships`, which is listed as
follow-up below rather than claimed as proven.

---

## 2. P3 / F-09 — login lockout is per source, bounded (DONE)

### Defect

`host/src/router.rs` used to carry `const LOGIN_SOURCE: &str = "host-router";` and pass it
into `Identity::login(&username, &password, LOGIN_SOURCE)` — one string for every request.
`Identity::rate`
(`identity.rs:461`) keys its `source` bucket on that string, so **every** request shared one
bucket: 60 failed attempts from anywhere returned `429` to every user for five minutes —
an unauthenticated, essentially free global denial of service. The TS host buckets on
`remoteAddress`; the Rust port had replaced that with a constant and documented the
simplification instead of closing it.

### Fix

1. **Transport peer, not a constant.** `router.rs` now has `struct LoginSource(String)` with
   `impl FromRequestParts<AppState>` reading `ConnectInfo<SocketAddr>` and keeping only
   `peer.ip()` (the port is attacker-chosen, so it is not part of the key). The login handler
   takes `source: LoginSource` and passes `&source.0`. A deployment that serves the router
   without `into_make_service_with_connect_info` falls back to
   `NO_CONNECT_INFO_SOURCE = "no-connect-info"` — the old shared bucket, named in
   `login_buckets` instead of silently turning every login into a 500.
2. **Bounded registry** (`identity.rs:75-110`): `MAX_LOGIN_SOURCES = 1024`, a process-wide
   `OnceLock<Mutex<HashSet<String>>>` of sources that own a bucket, and `bucket_for()`:
   the first 1024 distinct sources get `peer:<ip>`; every source past the cap shares
   `peer:overflow`. An attacker rotating addresses grows neither the set nor the number of
   distinct `archava_identity.login_buckets` rows beyond `1024 + 1 + accounts-in-window`;
   the bucket rows themselves are still reaped by the existing daily `DELETE`.
   The registry is process-wide on purpose: a per-`AppState` registry would hand a restarted
   router a fresh 1024 slots while the DB buckets (keyed by a digest of the source string)
   kept the old counts — the two would disagree.
3. `Identity::login` calls `self.rate(&username, &source_bucket(source))`.

Per-account limiting (5 failures / 15 min) is unchanged, and a successful login still clears
the account bucket only — one account's lockout never affects another's, as before.

### Tests

- `crates/host/src/identity.rs::every_source_gets_its_own_bucket_and_the_registry_stops_growing`
  — distinct sources ⇒ distinct buckets; the registry stops at `MAX_LOGIN_SOURCES`; source
  `1025` gets `peer:overflow` and is not stored.
- `crates/host/tests/session.rs::exhausting_one_source_does_not_lock_another_source_out`
  — drives the real `POST /login` route through `oneshot` with the `ConnectInfo` extension
  axum installs from the transport. Two refusals from `203.0.113.7` (different accounts) name
  that peer's bucket row, the row is moved to the limit where the limiter reads it, the
  noisy peer is then refused (`429`) even with the right password, and `198.51.100.9` still
  signs in with a token. `login_from()` installs the peer and `login_buckets()` reads the
  rows; the route itself is untouched by the test.

### RED-BY-DESIGN → GREEN

RED is produced by a two-line shim that reinstates the pre-fix behaviour in `router.rs`
(constant `LOGIN_SOURCE`, the extractor field left unused, the call passing the constant),
then the fixed file is restored and the sha256 re-checked. Full transcript
(`docs/audit/evidence/a11/redgreen-session.log`):

```
FIXED_SHA256=7f36fa9ee7e9eaa2dd12e07a7379a9adb380a116f489283916f64acde7b6b4c0
=== 1. SHIM: reinstate the single global source (pre-fix behaviour) ===
SHIM_APPLIED
107:const LOGIN_SOURCE: &str = "host-router";
1773:        .login(&submission.username, &submission.password, LOGIN_SOURCE)
=== 2. RED run (expect the isolation assertion to fail: quiet peer 429) ===
RED_EXIT=101
=== 3. RESTORE fixed router ===
RESTORED_SHA256=7f36fa9e… MATCHES_FIXED=yes
LOGIN_SOURCE_OCCURRENCES=0
=== 4. GREEN run (fixed router, same test) ===
GREEN_EXIT=0
```

RED (`docs/audit/evidence/a11/red-session.log`, real route, real limiter, real rows):

```
thread 'exhausting_one_source_does_not_lock_another_source_out' panicked at …/session.rs:604:
assertion `left == right` failed: a quiet source still signs in while another source is throttled:
  {"decision":"refused","message":"Too many login attempts; try again later.","reason":"rate_limited"}
  left: 429
 right: 200
test result: FAILED. 0 passed; 1 failed; … finished in 17.76s
```

i.e. with one shared source the quiet peer is refused exactly like the noisy one — the global
DoS, observed rather than argued. GREEN (`docs/audit/evidence/a11/green-session.log`):
`1 passed; 0 failed; … finished in 26.62s`, the quiet peer receives a token.

### The first RED run failed for the wrong reason (kept as evidence of a bad test design)

The first version of this test replayed 61 real refusals (61 different accounts, one peer) and
was **vacuous**: each refusal runs a key derivation (~9 s in a debug build), so 61 of them
took 561 s — longer than the limiter's 300 s window — and the window rolled over before the
61st attempt. The RED run therefore failed on the _noisy_ assertion instead:
`the noisy source has spent its own budget: left 200, right 429` (`red-session.log`, first
run, 560.89 s). The test now spends the budget by moving the noisy peer's own bucket row to
the limit (two cheap probes identify which row belongs to the peer: `rate` writes one row for
the source and one for the account, and the account changes between probes). Same route, same
limiter, same rows; the counter is seeded instead of accumulated. Runtime 17.8 s, and the RED
failure now lands on the isolation assertion itself.

---

## 3. P3 / F-10 — tenant-existence oracle (DONE)

### Defect

`apps/crm/src/host.ts:186-194` (and the same pre-filter on the other domain mounts — 17
occurrences of `!settings.tenants.some((item) => item.id === tenant)` in `HEAD`) answered
`404 {"error":"not_found"}` for a tenant absent from `settings.tenants`, while a _known_
tenant with no membership row surfaced `CrmError('forbidden', 'CRM membership required.')`
from `packages/crm-pg/src/repository.ts:202` and left through the outer `CrmError` arm as
`403 {"error":{"message":"CRM request refused."}}`. Status **and** body differed, so
membership of a known tenant was observable from an unauthenticated-shaped probe (tenant ids
are documented non-secret, hence P3).

### Fix

Deleted the tenant-existence pre-filter from **all 17** domain mounts that had it (`git diff`
removes 17 such lines; `grep -c "settings.tenants.some" apps/crm/src/host.ts` → `0`;
`settings.tenants` uses fell from 23 to 6 — the remainder resolve the tenant id and feed the
native/pipeline routes). A request for an unknown tenant now
falls through to exactly the same `service.access(tenant, principal)` call a known tenant
makes; `archava_crm.lock_membership(text, text)` (`packages/crm-pg/migrations/001_crm_contacts.sql:84`
— `text`, so a non-uuid tenant cannot raise) returns no row and `authorize` throws the same
`CrmError('forbidden', 'CRM membership required.')` an existing non-member gets. Both cases
leave through the same outer arm: `403` + `{"error":{"message":"CRM request refused."}}` +
`cache-control: no-store` + `content-type: application/json`.

The mount's own 404s remain, and they are _configuration_ facts, not tenant-existence facts:
`!tenant` (path did not parse), `!modules.<domain>` (module disabled), `!<service>` (not
wired). None of them depends on whether the tenant id exists.

### Test

`apps/crm/test/host.test.ts::answers an unknown tenant exactly like a known tenant the actor
cannot access (A11/F-10)` — drives the real handler with `settings.tenants = ['known-tenant']`
and a stub `access` that always rejects `forbidden`, then compares `known-tenant` against
`ghost-tenant`: same status, byte-identical body, same `content-type`.

- RED (`red-crm-host-oracle.log`): `AssertionError: expected 404 to be 403` — known `403`,
  ghost `404`.
- GREEN (`green-crm-host-oracle.log`): `Test Files 1 passed`, `Tests 6 passed (6)`, `EXIT=0`
  (the file now carries 7 tests — the native-route witness below was added after that run).

### Residual (documented, not silently skipped)

- A **malformed** tenant id (e.g. an empty or slash-bearing segment) now answers `400
{"error":{"message":"Invalid request."}}` from the Zod path instead of `404`; a malformed
  id cannot name a real tenant, so this distinguishes nothing about existence.
- The **pipeline/native-action** routes (five sites: `cancelNative`, `native`,
  `completeNative`, `receiptRoute`, `conversation`) resolve the tenant before delegating and
  used to answer `403 {"error":{"message":"Workspace access refused."}}` for an unknown
  tenant, while a known tenant + non-member left through the `CrmError` arm as
  `403 {"error":{"message":"CRM request refused."}}` — same status, different body, same
  oracle one layer down. **Closed in this card**: `if (!tenant) throw new
CrmError('forbidden', 'CRM membership required.')`, so both cases leave through the single
  `CrmError` arm (`grep -c 'Workspace access refused' apps/crm/src/host.ts` → `0`).
  Witness: `answers an unknown tenant on the native action routes like a forbidden member
(A11/F-10)` — real handler, `nativeFor` stub rejecting `forbidden`, compares
  `known-tenant` (reaches the stub) with `ghost-tenant` (throws locally) on status, body and
  content-type. RED `red-crm-native-oracle.log`:
  `Expected "…CRM request refused." / Received "…Workspace access refused."` (1 failed | 6
  passed); GREEN `green-crm-native-oracle.log` / `gate-crm.log`: 7 passed, `EXIT=0`.
- Tenant ids themselves remain non-secret by documentation; this fix removes the _membership_
  oracle, not the knowledge that a tenant id exists in a deployment's settings.

---

## 4. Not done, and why

- **Scheduling grant red proof** (§1): needs a partial `permissions` UPDATE against
  `archava_scheduling.memberships` in the fixture; the green behaviour is asserted, the red
  proof is not claimed.
- **Malformed `tenant` segments on the CRM mounts**: see §3 residual (Zod 400), not an
  existence oracle.
- `erp_billing`, `erp_attendance`, `erp_subscriptions` suites were **not** run, per the card
  (≈52 min, cargo-lock contention); the dirty state of `erp/{attendance,billing,subscriptions}.rs`
  belongs to the A1-FU3 card and was not touched or reverted here.

---

## 5. Gates

Run from `rust/` with `TMPDIR=/tmp`, serial, one block (`redgreen-session.log` records the
RED/GREEN phase; every gate below has its own raw log in `docs/audit/evidence/a11/`).

| gate                     | command                                                                                            | result                                     |
| ------------------------ | -------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| adapters                 | `cargo test --offline --locked -p archava-adapters`                                                | **EXIT 0** — 13 + 11 = 24 passed, 0 failed |
| host `erp_organizations` | `cargo test … -p archava-host --test erp_organizations`                                            | **EXIT 0** — 10 passed, 0 failed (65.02 s) |
| host `erp_projects`      | `… --test erp_projects`                                                                            | **EXIT 0** — 10 passed, 0 failed (68.82 s) |
| host `erp_proposals`     | `… --test erp_proposals`                                                                           | **EXIT 0** — 10 passed, 0 failed (66.69 s) |
| host `erp_scheduling`    | `… --test erp_scheduling`                                                                          | **EXIT 0** — 10 passed, 0 failed (66.83 s) |
| host `session`           | `… --test session`                                                                                 | **EXIT 0** — 7 passed, 0 failed (50.08 s)  |
| fmt                      | `cargo fmt --all --check`                                                                          | **EXIT 0** — no diffs                      |
| clippy                   | `cargo clippy --offline --locked -p archava-adapters -p archava-host --all-targets -- -D warnings` | **EXIT 0** — 0 warnings                    |
| CRM (TS)                 | `pnpm exec vitest run apps/crm/test/host.test.ts`                                                  | **EXIT 0** — 1 file, 7 tests passed        |

Verbatim tail of `redgreen-session.log`:

```
=== 5. GATES ===
ADAPTERS_EXIT=0
HOST_EXIT=0
FMT_EXIT=0
CLIPPY_EXIT=0
CRM_EXIT=0
=== DONE ===
```

`gate-crm.log`:

```
 ✓ apps/crm/test/host.test.ts (7 tests) 14ms
 Test Files  1 passed (1)
      Tests  7 passed (7)
```

Prettier check on the two TS files touched (`pnpm exec prettier --check apps/crm/src/host.ts
apps/crm/test/host.test.ts`): `All matched files use Prettier code style!`

Deliberately not run: `erp_billing`, `erp_attendance`, `erp_subscriptions` (per the card —
≈52 min, cargo-lock contention); the CRM oracle RED/GREEN from earlier runs are kept at
`red-crm-host-oracle.log` / `green-crm-host-oracle.log` because they predate this block.

### Frozen artifacts

```
router.rs            7f36fa9ee7e9eaa2dd12e07a7379a9adb380a116f489283916f64acde7b6b4c0
A11 doc (this file)  a6eed4dfa803d848bdeeea2c9cadb6b3f3d2175fbca4df7151ed12abe9e98926   (pre-gate copy)
```

The pre-gate sha of this document is recorded so the gate output appended to §5 can be
distinguished from the reviewed text; the doc hash will differ after this section is written.

Tree: dirty tree preserved (no `git add/commit/stash/revert`). Files touched by this card:
`rust/crates/adapters/src/erp/{organizations,projects,proposals,scheduling}.rs`,
`rust/crates/host/src/{router,identity}.rs`, `rust/crates/host/tests/{session,erp_organizations,
erp_projects,erp_proposals,erp_scheduling}.rs`, `apps/crm/src/host.ts`,
`apps/crm/test/host.test.ts`, this document. `rust/crates/own/**`, `rust/crates/slice/**`
and `rust/Cargo.lock` were not modified by this card (`git status` lists own/slice as dirty
from the earlier A2/A3 cards; their mtimes are older than this card's first edit).
