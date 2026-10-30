# Archava Platform — Full Project Audit, 23 October 2026

**Verdict: 62/100 — risky, do not tag. Engineering quality is high; the 1.17 modules ship cross-tenant authorization holes and the release gate is red.**

This audit was produced independently of the 21–22 October packs. Every claim below
was reproduced on this tree; every BLOCKER was re-read in source after the reviewer
flagged it. Score is capped at 69 by missing authorization on sensitive data, and at 84
by a red gate; 62 reflects both.

| Item         | Value                                                                |
| ------------ | -------------------------------------------------------------------- |
| Repo         | `/home/haikaru/Archverse/Lab/Archava_Platform`                       |
| HEAD         | `412f763` (`feat/laundry-1.17.0`)                                    |
| Working tree | **203 modified + 75 untracked**, none gated                          |
| Branch       | 57 commits ahead of `main`, last commit 20 Oct; no CI run against it |
| Runtime      | node v22.23.2 (evidence host), pnpm 10.15.1, rustc/cargo 1.96.0      |

## 1. Gate results (reproduced this pass)

| Gate                                         |       Exit | Detail                                                                                                                     |
| -------------------------------------------- | ---------: | -------------------------------------------------------------------------------------------------------------------------- |
| `pnpm structure`                             |          0 | 70 workspace projects, all `@archava/*` imports declared                                                                   |
| `pnpm format:check`                          |      **1** | 1 file: `docs/audit/ARCHAVA_AUDIT_2026-10-23.md`                                                                           |
| `pnpm lint`                                  |      **1** | 46 errors: 17 source (`workspace.ts` ×16, `server-headers.test.ts` ×1) + 29 from `rust/target/site/pkg/*.js` (not ignored) |
| `pnpm typecheck`                             |          0 | all packages + `apps/web`                                                                                                  |
| `pnpm test`                                  |          0 | **159 files / 1,953 tests**, 5.8 s                                                                                         |
| `pnpm test:coverage`                         |          0 | lines 33.56%, branches 34.40%, functions 41.76%                                                                            |
| `cargo fmt --check` / `clippy --all-targets` |          0 | clean (reviewer run)                                                                                                       |
| `pnpm audit --audit-level=high`              |          0 | no known vulnerabilities                                                                                                   |
| `cargo audit`                                | **1 vuln** | `ctutils 0.4.3` RUSTSEC-2026-0385; `paste` unmaintained (warning)                                                          |

`pnpm verify` runs format before lint (`tools/scripts/verify.mjs:39-51`), so it stops at
gate two on this tree. `.github/workflows/ci.yml` has the same order. "All eight root gates
passed" in `docs/BUILD_STATUS.md:24` is not reproducible, and the test counts there
(1,786 / 134) are stale against today's 1,953 / 159.

## 2. Blockers

### B01 — CRITICAL — Field services: 5 of 7 tables have no tenant isolation

`packages/field-services-pg/migrations/001_field_services_dispatch_visits.sql`

`jobs` is the only table with `ENABLE`/`FORCE ROW LEVEL SECURITY` (`:177-181`).
`visits`, `work_lines`, `signoffs`, `audit` have no RLS. `visits`, `work_lines` and
`signoffs` have **no `tenant_id` column at all**, so RLS cannot be added without a
migration. `memberships` likewise has no RLS. The runtime role is granted
`SELECT,INSERT` on all of them (`src/migrate.ts:60-68`).

The repository happens to gate child reads behind a tenant-scoped job fetch, so nothing
leaks through today's handlers — but the database does not enforce anything. Any new
query, direct SQL, or bug that reads children by `job_id` returns every tenant's visit
notes, work lines and customer signoffs. `getJobDetail` already reads children by
`job_id` alone (`src/repository.ts:654-669`).

**Fix:** add `tenant_id` + composite `(tenant_id, job_id)` keys to the three child tables,
`ENABLE`/`FORCE` RLS with the standard policy, enable RLS on `memberships`, and add the
tenant predicate to every child query.

### B02 — CRITICAL — `assigned` scope is not enforced in fulfillment or field services

`packages/fulfillment-pg/src/repository.ts:671-696, 707-756, 757-790`
`packages/field-services-pg/src/repository.ts:637-676`

Fulfillment reads `scope` from the membership (`:192`) and never uses it:
`listJobs` filters only `tenant_id`/status/kind; `getJobDetail`, `listLines`,
`listFindings`, `listCustody` have no owner predicate. Field services enforces scope in
`listJobs` only (`:612-620`); its detail and child reads do not.

A principal with `scope='assigned'` can list and open every job in the tenant. This is
the same class that was fixed for billing/attendance/laundry; it was never applied to the
two newest modules. No test passes `scope:'assigned'` for fulfillment.

**Fix:** apply the `(owner_id = principal OR created_by = principal)` predicate to every
read and mutation path, and add an assigned-scope test per module (seed two owners, assert
the second sees nothing and cannot mutate).

### B03 — HIGH — Tenant GUC is self-certified in the ACT host ledger

`packages/act-host-pg/migrations/001_host_intents.sql:90-171`,
`packages/act-host-pg/src/migration.ts:26-30`

Every ledger function (`open_attempt`, `lookup_attempt`, `find_head`, `claim_attempt`,
`settle_attempt`, `cancel_attempt`, `reconcile_attempt`) takes `tenant_id` and
`session_id` as arguments and calls `set_config` itself before running. The RLS policy
then compares the row to the value the caller just supplied. The runtime role additionally
holds direct `SELECT,INSERT,UPDATE` on `heads` and `intents`. Any holder of the app role
can read or write another tenant's intents by passing that tenant's id.
(`REVOKE ALL … FROM PUBLIC` at `:168-169` does not revoke the app-role grant.)

**Fix:** bind tenant/session server-side (per-tenant role or `SECURITY DEFINER` resolving
from an authenticated principal); stop accepting them as arguments; revoke direct table
access.

### B04 — HIGH — `memberships` tables without RLS are granted to the runtime role

`packages/field-services-pg/migrations/001…:7-15`,
`packages/formal-education-pg/migrations/001…:17-28`

Both schemas grant `SELECT ON memberships` (`field-services-pg/src/migrate.ts:60-62`,
`formal-education-pg/src/migrate.ts:73-75`) while neither table has RLS. The app role can
enumerate every tenant's principals, permissions and scopes (and, in education, `classes`
and `guardian_ref`).

**Fix:** enable RLS on both; add the missing
`wanted_tenant = current_setting('archava.tenant_id', true)` predicate to
`lock_membership` (omitted in these two plus projects/scheduling/crm).

### B05 — HIGH — Formal education children can attach to another tenant's enrollment

`packages/formal-education-pg/migrations/001…:30-32, 52-87`

`enrollments` PK is `enrollment_id` alone (global). `sessions`, `results` and `reviews`
carry their own `tenant_id` but their FKs reference `enrollments(enrollment_id)` only.
A row in tenant A can reference tenant B's enrollment while satisfying RLS (row tenant
matches) and the FK (id exists) — silently attaching academic history across tenants.

**Fix:** `PRIMARY KEY (tenant_id, enrollment_id)` and composite FKs in all three children
plus `audit`.

### B06 — HIGH — Release gate is red and never ran on this tree

Format fails, lint fails, and the last CI run is from 7 October on `main`. Fifty-seven
commits and 278 uncommitted paths have no gated evidence. Per this project's own rule
(a verdict binds a scope hash), the 22 October certification does not cover the current
tree.

**Fix:** wrap the `Html` interpolations in `workspace.ts` with `renderValue(...)` (do not
relax the rule), remove the redundant assertion in `server-headers.test.ts:55`, add
`'**/rust/target/**'` to `eslint.config.mjs` ignores, run `prettier --write` on the audit
docs, then re-run `pnpm verify` and record a new gate pack with a fresh scope hash.

## 3. High-value fixes

### H01 — Login rate limit is an account-lockout DoS

`packages/identity-pg/src/sessions.ts:64-86,115`; Rust twin
`rust/crates/host/src/identity.rs:466`

Every attempt increments an `account` bucket (5 per 900 s); only a successful login
clears it. Five wrong passwords every 15 minutes lock the owner out permanently.
Fix: make the account bucket failure-only and non-blocking; block per (account, source)
with exponential delay, and alert on saturation.

### H02 — Rust serving entrypoint has no peer address, so login limiting is one global bucket

`rust/crates/slice/examples/slice_server.rs:353`

`into_make_service()` omits `ConnectInfo`; every request reports
`NO_CONNECT_INFO_SOURCE`. A global 60-attempt budget locks out all users. Fix: use
`into_make_service_with_connect_info::<SocketAddr>()` and configure the trusted proxy hop
explicitly.

### H03 — scrypt runs on Tokio worker threads

`rust/crates/host/src/identity.rs:539-550, 604`

`derive()` calls `scrypt` (~32 MiB, tens–hundreds of ms) inside the async login future,
bounded only by a 2-permit atomic. Two concurrent bad logins pin both workers and stall
`/healthz` and every session-authenticated read. Fix: `tokio::task::spawn_blocking` (or a
dedicated KDF pool), releasing the permit inside the blocking task.

### H04 — ERP readiness gate covers 1 of 12 ledgers

`rust/crates/host/src/router.rs:316-341, 405-558`

`try_new_erp` calls `assert_startup_readiness` only for `PURCHASING_LEDGER`, yet mounts
organizations/proposals/projects/scheduling/accommodation/assets/billing/subscriptions/
attendance/foodservice. Their `assert_*_kernel` gates are invoked only from tests. A
namespace whose RLS is enabled-but-not-forced (or a drifted ledger checksum) serves
traffic with nothing catching it.

### H05 — Receipt recovery diverges for delegated service principals

`packages/capacity-pg/src/repository.ts:921`, `packages/proposals-pg/src/repository.ts:1416`

The command row stores `principal.id` (`capacity:521`, `proposals:463`) but the status read
filters `actor_id = access.ownerId`, which is rewritten to the human behind a service
principal (`capacity:321-327`). A delegated service that loses a response can never
recover its receipt (`unknown` forever) while the same key replays through `mutate`.
The new status tests cannot catch it: the stub returns the row without applying
`query.params[2]` (`capacity-pg/test/status.test.ts:104`).
Fix: one identity on all three paths, and filter on the parameter in the stub.

### H06 — Coverage and PostgreSQL proofs are not in the PR gate

`package.json:18-20`, `tools/scripts/verify.mjs:43`, `.github/workflows/ci.yml:92-99`

- `pnpm test` omits `--coverage`, so the A6 ratchet can regress without red CI.
- `billing:verify`, `laundry:verify`, `subscriptions:verify`, `fulfillment:verify`,
  `field-services:verify`, `capacity:verify`, `attendance:verify` are invoked only from
  `tools/security/verify.mjs`, which no workflow calls. Migration and RLS regressions
  pass CI as long as the stub pool agrees.

### H07 — Module APIs fail open on a client-supplied `x-user-id`

`packages/{attendance,billing,laundry,fulfillment,field-services,formal-education}/src/api.ts:53-56`

`create*Api(service, identity?)` falls back to
`{kind:'human', id: request.headers.get('x-user-id') ?? 'anonymous'}`. The CRM host always
passes a real closure, so it is latent here — but this is a library product; one consumer
that forgets the argument hands impersonation to any remote caller. Make `identity`
required; delete the header branch.

### H08 — `cargo audit` is not gated and currently finds a vulnerability

`rust/Cargo.lock:654-657`

`ctutils 0.4.3` (RUSTSEC-2026-0385, published the day of this audit) arrives via
`sqlx 0.9 → hkdf → digest 0.11 → ctutils`. `cargo audit` is deliberately absent from the
PR gate and only runs in the daily cron. Fix: `cargo update -p ctutils` (≥ 0.4.4), and
consider a lockfile-only audit in CI (no network advisory DB needed for `--offline`
deny-list pinning).

### H09 — Documentation evidence drift

`docs/BUILD_STATUS.md:24-31` — test counts, "all eight gates", and "83 controls" are not
reproducible from the current tree. Generate these from the last gate run (counts + scope
hash + exit codes) instead of hand-editing prose.

### H10 — Smaller but real

| Item                                                                                                 | File                                                                                                                                                                                                           | Fix                                                                                               |
| ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------- |
| Command idempotency is read-then-INSERT in 10 repositories (concurrent duplicate surfaces raw 23505) | `attendance-pg:168`, `billing-pg:270`, `laundry-pg:250`, `field-services-pg:234`, `assets-pg:432`, `fulfillment-pg:270`, `organizations-pg:376`, `capacity-pg:521`, `subscriptions-pg:258`, `proposals-pg:463` | `INSERT … ON CONFLICT DO NOTHING RETURNING` + re-read (pattern already in `accommodation-pg:586`) |
| Provider credential structs derive `Debug`                                                           | `rust/crates/providers/src/gemini.rs:128`, `postmark.rs:245`                                                                                                                                                   | Hand-implement redacting `Debug`; private fields                                                  |
| RLS read-back on every statement (3–4 extra round-trips per request)                                 | `rust/crates/adapters/src/rls.rs:449-494`                                                                                                                                                                      | Verify once at `begin`; cache role check per pool                                                 |
| Silent numeric clamping (`unwrap_or(0)` / `i32::MAX`)                                                | `rust/crates/adapters/src/erp/commerce_kernel.rs:1145,1531`; `host/src/crm.rs:2313`                                                                                                                            | Use `int4()` which refuses with `Conflict`                                                        |
| SSR `/app` document bypasses security headers                                                        | `rust/crates/slice/examples/slice_server.rs:318-324`                                                                                                                                                           | Apply `security::apply` as outer layer                                                            |
| Dead same-origin check                                                                               | `apps/crm/src/host.ts:99`                                                                                                                                                                                      | Compare `Origin`/`Host` directly, or pass the untrusted base URL                                  |
| Cookie scheme from client-controlled `X-Forwarded-Proto`                                             | `rust/crates/host/src/security.rs:61-82`                                                                                                                                                                       | Deployment-configured scheme / trusted hop                                                        |
| Formal-education permission contract names wrong grants                                              | `packages/config/src/assembly.ts:297-303`                                                                                                                                                                      | Mirror `EDUCATION_PERMISSIONS`                                                                    |
| Unindexed education children / field signoffs / subscription listing                                 | `formal-education-pg:52-87`, `field-services-pg:70-77`, `subscriptions-pg:700`                                                                                                                                 | Add `(tenant_id, …)` composite indexes                                                            |
| `OFFSET` pagination + unbounded child reads                                                          | `attendance-pg:623`, `billing-pg:805`, `subscriptions-pg:702`, `fulfillment-pg:699`                                                                                                                            | Keyset cursors; bound child collections                                                           |
| Superuser operator path (backup/upgrade/lifecycle)                                                   | `apps/crm/src/local-postgres.ts:19`, `backups.ts:244`, `operator.ts:528`                                                                                                                                       | Dedicated `NOSUPERUSER` role                                                                      |
| No down-migrations; destructive DDL apply-only                                                       | `commerce-pg/migrations/002,003`, `scheduling-pg/002`, `field-services-pg/001`                                                                                                                                 | Paired down migrations, expand/contract                                                           |
| Unqualified legacy tables in `public`                                                                | `act-confirmation-pg/migrations/001:100`, `act-storage/migrations/001:63`                                                                                                                                      | Dedicated schema or explicit fixture-only                                                         |
| `allowedOrigins: ["*"]` accepted; no enforcer anywhere                                               | `packages/config/src/client/schema.ts:93-98`                                                                                                                                                                   | Reject wildcard except reference configs                                                          |
| Repository internals returned in 500 bodies                                                          | `apps/web/src/server/server.ts:138-141`, `api.ts:142,169`                                                                                                                                                      | Fixed error body; log detail server-side                                                          |
| `Box::leak` of migration SQL                                                                         | `rust/crates/providers/src/store.rs:816-823`                                                                                                                                                                   | `OnceLock<&'static str>`                                                                          |

## 4. Verified sound (evidence, not vibes)

- **Tenant isolation, 13 DO-loop schemas** — every created table appears in the RLS array
  with both `ENABLE` and `FORCE` and a `USING`+`WITH CHECK` policy (diffed table names
  against arrays).
- **Explicit-RLS schemas** — accommodation, crm, projects, scheduling, owner, crm-act,
  provider-runtime assert forced-policy counts at startup.
- **Session model** — 32-byte base64url tokens, HMAC digests only, 8 h absolute / 30 min
  idle, password change bumps `user_version` and deletes sessions
  (`identity-pg/src/sessions.ts:104-178`, Rust parity).
- **CSRF** — token-derived HMAC, `timingSafeEqual`, enforced on every POST in both hosts.
- **Billing/payment events** — length-framed domain-separated HMAC, constant-time compare,
  `UNIQUE(tenant, provider_event_id)`, over-settlement and currency tests.
- **SQL** — all user values bound; dynamic fragments are fixed identifiers or
  `valid_namespace`-checked namespaces wrapped in `AssertSqlSafe`.
- **Rust authority** — server-resolved membership/scope, service ∩ human permission
  intersection, single-use approvals bound to actor/action/input/revision.
- **XSS** — `escape()`/`Html` boundary, `textContent` only in CRM client, covered by
  `frontend-escape.test.ts`.
- **Static serving containment** — `resolve` + `startsWith(publicDir + sep)`.
- **Secrets** — `.env.example` placeholders only; no hardcoded keys; Gemini key read from
  a 0600 file and excluded from the browser build.
- **Discipline** — 0 TODO/FIXME, 0 `.skip/.only/.todo`, 0 empty catches, all CI `uses:`
  pinned to commit SHAs, `clippy`/`fmt` clean, no `unsafe` in Rust.

## 5. Evidence checked

Commands: `git status`, `git diff --stat main...HEAD`, `pnpm structure|format:check|lint|
typecheck|test`, `pnpm exec vitest run --coverage`, `pnpm audit --audit-level=high`,
`cargo fmt --check`, `cargo clippy --workspace --all-targets`, `cargo audit`,
`gh run list`.

Files: the migration, repository, API, host, router, identity, security and CI files
cited above, plus `vitest.config.ts`, `eslint.config.mjs`, `package.json`,
`tools/scripts/verify.mjs`, `tools/security/verify.mjs`, `docs/BUILD_STATUS.md`,
`docs/PROJECT_AUDIT_2026-10-14.md`, `docs/rust/EXECUTION_STATE.md`.

## 6. Evidence missing

- CI history for `feat/laundry-1.17.0` (never ran; the runs listed are Oct 7 on `main`).
- `pnpm build` / `reuse:verify` / `crm:verify*` / `rust:verify` in this pass.
- A reproducer showing exploitation of B01–B05 (tenant-isolation findings are from code
  and schema; no live cross-tenant query was executed).

## 7. Next action

Fix B01 + B02 (field-services tenant columns + RLS and assigned-scope enforcement in both
modules), then B06 (green gate with a fresh scope hash). Those three convert the release
claim from unattested to reproducible. `cargo update -p ctutils` is a one-line
follow-up for H08.
