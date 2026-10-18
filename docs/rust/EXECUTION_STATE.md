# Rust migration — execution state

Checkpoint file. Read this first when resuming. Every status below uses the PRD
§23 vocabulary; nothing is marked `accepted` or `release_candidate`, because
nothing here has earned it.

**Updated:** 2026-10-17 (RN01–RN08 correction round closed — see below; §1.4B vertical slice next)
**Baseline HEAD:** `242549cc292cd53bbc66eb9cf119d57443aa2a37` (work uncommitted on top)
**Branch:** `feat/laundry-1.17.0`
**Release identity:** profile `1.18.0`, `@archava/commerce-pg` `0.4.0` (+ migration `004_bound_approvals`), Rust crates unchanged at their existing versions. `dist/releases/1.17.0` untouched.

## RN01–RN08 correction round, 2026-10-17 — closed this session

Source audit: [ARCHXY_RUST_AUDIT_2026-10-17.md](../ARCHXY_RUST_AUDIT_2026-10-17.md). Every RN finding from that
audit is closed below with its reproducer and the actual command + exit that
proves it. The §1.4 table's `RA01 CLOSED` row above stays true for what it
proved and is now corrected *in scope* by this round: two of its authority
witnesses had holes (RN01/RN02), and the manifest was widened to name them —
nothing from the prior round was deleted.

| Finding | Status | Proof (reproducer → command → exit) |
| ------- | ------ | ----------------------------------- |
| RN01 — a downgraded/revoked grant can still read its own receipt via status; authorized recovery did not re-derive the policy gate | **CLOSED** | `rust/crates/host/src/router.rs` (status route + `receipt_read_refusal`, recovery route): status/replay/recovery now re-derive the SAME policy gate against CURRENT grants on the STORED action — the refusal returns `role_not_allowed` with no receipt payload, and authorized recovery re-reads the same receipt for the user the grants still admit. Reproducer: `rust/crates/host/tests/authority.rs::a_downgraded_grant_cannot_recover_a_receipt_it_may_no_longer_act_on` (downgrade `archava_assistant→viewer` in memberships, then GET status ⇒ 403 role_not_allowed + `receipt: null`; restore grants ⇒ 200 `decision:"receipt"`). `cargo test -p archava-host --test authority` → **18 passed; 0 failed, exit 0**. Manifest: P-AUTHORITY-PG `namedRegressions` **includes RN01**, `witnessTotal: 8 → 11`, runner extended not weakened (`tools/rust/pg-authority.mjs` grades the new named witnesses — verified green). |
| RN02 — idempotency-key reuse returned the other command's receipt; unknown actor had no distinct refusal shape | **CLOSED** | `rust/crates/host/src/router.rs` execute replay branch: owner mismatch ⇒ **404 `unknown_request`** (no leak); owned key with a *different fingerprint* ⇒ **409 `request_id_reused`**, carrying NO receipt (never the other input's). Two regressions: sequential — `a_key_reused_for_a_different_command_conflicts_on_execute_and_in_race` (same key, `productId:1` recorded, then `productId:2` ⇒ conflict, and the replay bump (`recorded_fingerprint`, taken from the stored receipt before it is consumed) lets a caller prove the effect it got back is ITS command's); concurrent — `concurrent_same_key_different_input_yields_conflict_not_the_other_receipt`: a fixture-administration `LOCK TABLE archava_commerce.commands IN EXCLUSIVE MODE` parks BOTH `POST /commands` submissions (waiter count read from `pg_locks` filtered to the locked table, deadline-bounded, counted **before** the gate opens), then exactly one 200 + one 409 whose `receipt` is null, and exactly one stored command row. `cargo test -p archava-host --test authority` → **18/18, exit 0**. Manifest: P-AUTHORITY-PG namedRegressions **includes RN02 twice** (sequential + concurrent witnesses). |
| RN03 — release profile / commerce-pg package / migration ledger drift (profile said 1.17.0, package 0.3.0, migration 004 missing from the TS ledger) | **CLOSED** | `packages/commerce-pg/src/migrate.ts` adds `004_bound_approvals` with its rationale; `packages/commerce-pg/package.json` **0.3.0 → 0.4.0**; `config/library-release.v1.json` profile **1.17.0 → 1.18.0** + migration-004 checksum (`33870679fb6c3d9a960d6b23dd63e5765814be56b2ea50729911293444e4c9b2`, sha256 of the file bytes, matching the migrator's `createHash('sha256')` scheme) + schema-list entry. Proofs: `pnpm reuse:verify` → **PASS** (62 packed consumers, hospitality + retail, identical core artifact hashes); `pnpm crm:verify-upgrade --cold-prior` → **PASS** (4/4: prior-install data creation, non-destructive upgrade, fresh-restore identity invalidation, native resume with fresh approval epoch). `dist/releases/1.17.0` verified untouched. |
| RN04 — the suite gated logins through a raised budget instead of the shipped one | **CLOSED** | Per RN07: the shipped `set_max_active_derivations` override is gone, `witnessTotal` tests now run **at the real default (2)**, and `tests/authority.rs` serializes its logins through a runtime-agnostic `tokio::sync::Semaphore` sized to `max_active_derivations()` (`LOGIN_SLOTS`) — the same 2 concurrent scrypt the production flood gets. The suite passes **inside** the budget: `cargo test -p archava-host --test authority` → 18/18. |
| RN05 — capacity output-domain/underflow unverified | **CLOSED** | `rust/crates/capacity`: output-domain bounds + underflow-floor (`Math.max(1, …)` parity) regressions added. `cargo test -p archava-capacity --lib` → **38 passed; 0 failed, exit 0**. |
| RN06 — fmt/clippy debt | **CLOSED** | `cargo fmt --all` applied (identity.rs, authority.rs, kdf_budget.rs, router.rs); `cargo clippy --workspace --all-targets -- -D warnings` → **exit 0** (one real fix: an unused JoinHandle binding in the new concurrent test is now `_holder` with the drop-detach rationale documented). |
| RN07(RN04) — shipped budget override + the flood's unaccountable outcomes | **CLOSED** | Three parts, all green: (1) `set_max_active_derivations` deleted from the shipped API (`MAX_ACTIVE_DERIVATIONS` is a `const`);(2) in-crate deterministic witness `an_overloaded_server_is_busy_never_an_invalid_password` pins `Err(Busy)` when the *production* counter reaches 2 — deterministic because it waits on `active_derivations()` via the `budget_observation()` mutex, never on timing; (3) the HTTP flood (`the_third_concurrent_login_is_refused_busy_and_the_budget_recovers`) now asserts a **closed outcome set** {200 allowed, 503 `busy`, 429 rate_limited} — never 401 `invalid_credentials`, never 503 `identity` — plus sequential recovery afterwards. **Honest scope note:** "≥1 refused" is no longer asserted over wall-clock luck: a harness run traced a `(503, "identity")` to `pool timed out while waiting for an open connection` and a second run showed all-three-200 with `landing spread 0 ms / wall 29.9 s` — a *debug-build* scrypt costs ~10.9 s (a release one is 0.207 s) and the container's effective CPU allowance (~1.5–2 cores) can serialize three burns, so overlap is a scheduler property this environment cannot promise. The counter-level budget proof is deterministic in-crate; the flood proves the closed set + recovery; `cargo test -p archava-host --test kdf_budget` → **3 passed; 0 failed, exit 0**, host lib → **41 passed; 0 failed** (three new unit tests pin `refused_login_outcome`: each identity failure gets its own status, Busy is never 401, Unavailable names no mechanism). Two harness-shape fixes that the diagnosis produced, kept because they are production-true: the flood and the RN02 concurrency test spawn tasks on ONE runtime (any real serving host is one axum runtime; one sqlx pool driven from several runtimes is a shape no deployment has, and it jams `test_before_acquire`'s cross-runtime socket polls); `identity.rs`'s `From<sqlx::Error>` now `tracing::error!`s the real error at the boundary (a production 503 `identity` was previously uninvestigable), and kdf_budget.rs carries a hand-rolled minimal `tracing::Subscriber` so failure output names the sqlx error. |
| RN08 — migration-004 broke the workspace compile (E0063) | **CLOSED** | `packages/commerce-pg/migrations/004_bound_approvals.sql` sequence fixed; `cargo build --workspace --all-targets` → **exit 0**; `cargo test -p archava-host --test readiness` → **5 passed; 0 failed** (the tampered-checksum readiness blocks still hold: the profile's checksum matches the real file bytes on both languages). |

Round totals: manifests unchanged in denominator (93 items / 51 proofs); P-AUTHORITY-PG `requiredChecks 8 → 11` and `witnessTotal 8 → 11` — the only allowed movement (adds are mandated). We ran: authority 18/18 (exit 0, ~100 s), readiness 5/5, kdf_budget 3/3, host lib 41/41, capacity lib 38/38, `pg-authority.mjs` PASS (`dist/rust-authority-proof.json`, 11 named checks, `exitCode 0`), `reuse:verify` PASS, `crm:verify-upgrade --cold-prior` 4/4. Pool/cop runtime note recorded under RN07 for P-BENCHMARK's conditions block.


## §1.4 round, 2026-10-16 — closed this session

| Finding                              | Status                               | Proof (actual exits)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| ------------------------------------ | ------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RF04 stale PG-fixture cleanup (P1)   | **CLOSED**                           | `clean_stale_dirs` (`rust/crates/pg-fixture/src/lib.rs`) rewritten around a pure `reap_verdict(age, creator_pid, postmaster_pid, min_age, proc_alive) → KeepTooYoung \| KeepLive \| KeepUnknown \| Remove`. Creator PID comes from the directory name (`archava-pg-fixture-<pid>-<nanos>` — the lifecycle guard), the postmaster PID from `data/postmaster.pid` (the old code read the wrong path), and removal requires **positive proof** the creator is dead: missing/unreadable PID ⇒ `KeepUnknown`, non-Linux `proc_alive` conservatively `true`. New `ProvisioningGuard` (armed before `initdb`, disarmed after pools connect) stops+removes an instance when a panic lands between `pg_ctl start` and pool connect — the failure-path teardown the audit named; `Fixture::drop` and the guard share one `stop_server()`. `cargo test -p archava-pg-fixture --lib` **9/9** incl. `the_postmaster_pid_is_read_from_the_data_directory`, `a_live_fixture_older_than_the_age_gate_is_preserved` (in-flight), verdict matrix incl. concurrent runs; clippy `-D warnings` clean.                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Format — P3 root verification        | **CLOSED**                           | Targeted `prettier --write` on exactly the six audit-named files (`docs/rust/EXECUTION_STATE.md`, `docs/rust/migration-inventory.json`, `tools/rust/{bootstrap,evidence,pg-readiness}.mjs`, `tools/rust/test/verify.test.mjs`) → `pnpm format:check` **exit 0**. Artifact-test comments realigned to the actual wiring (`vitest.artifacts.config.ts`, `pricing-coverage{,.artifact}.test.ts`, `vitest.config.ts`): the artifact proofs run as `rust artifact proofs (vitest)` in `pnpm rust:verify-release`, directly after `compiled-consumer benchmark` (`pnpm rust:benchmark` — the step that produces `build/pricing-engine.mjs`, the workloads and `archava-bench`), not after `rust:build-artifacts` in the fast gate; failure hints now name `pnpm rust:benchmark`. Cold-safety and hard assertions untouched; artifact config **4/4**, benchmark unit tests **35/35**.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| RA07 calendar arithmetic (P1)        | **CLOSED**                           | Instant domain = the JS `Date` range (±8_640_000_000_000_000 ms) validated at every entrypoint; `CalendarError { TimestampOutOfRange, Overflow, HorizonExceeded }` converts to the wire's `invalid_request`; `plus_days` is now checked (`try_plus_days`) — no more "returns the unchanged instant" on overflow; `next_business_day`/`count_working_days` return `Result` with domain + `MAX_HORIZON_DAYS = 365_000` horizon validation **before** walking (termination by construction; at-limit walks, limit+1 refused — boundary pinned on both sides). All 7 `schedule.rs` call sites propagate. `cargo test -p archava-capacity` **green incl. the differential parity suite** (corrected-parity preserved); 6 new regressions incl. the `i64::MAX` reproducer that used to hang forever now returning a controlled error, near-boundary and a valid 261-working-day wide-range case; clippy clean.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Consumer-closure reproducibility gap | **CLOSED** ([ADR-R10](DECISIONS.md)) | One pin: committed `examples/rust-consumers/crate/Cargo.lock` (dropped from the crate's `.gitignore` generated set). `pnpm rust:bootstrap` derives the throwaway closure from that pin — manifest = the consumer's registry dependencies verbatim (requirements + features), lock = registry blocks byte-for-byte with locals stripped and the root rewritten — and fetches `cargo fetch --locked`; three named refusals (declared dep missing from lock / stale unreachable entry / closure crate reachable only via a path dependency). Measured rejection: pinning locked transitives as direct deps pulls `hashbrown`'s default features → `allocator-api2` → lock refused. The proof asserts the same pin (`generate-lockfile --offline --locked`, `vendor --offline --locked`) and now **writes** the `.cargo/config.toml` its comment had always claimed — measured: without it, `vendor/` was never used (empty `CARGO_HOME` fails `no matching package named serde`); consumer A tests run with an **empty `CARGO_HOME`** ⇒ evidence `vendoredSourceOnly: true`. New `tools/rust/test/bootstrap.test.mjs` **10/10** wired into the fast gate's `node --test` step. Evidence: cold `CARGO_HOME` bootstrap **exit 0** (124 MB fetched), `pnpm rust:verify-consumers` **exit 0** (A: 4 tests from `vendor/` only; B: 8 checks).                                                                                                                                                                                        |
| RA01 server-side authority (P1)      | **CLOSED** ([ADR-R11](DECISIONS.md)) | Identity port (`rust/crates/host/src/identity.rs`): scrypt `N=32768,r=8,p=3` byte-compatible with the baseline's `scrypt$32768$8$3$…` form, HMAC-SHA256 session digests, CSRF derived not stored, login rate buckets (source 60/300s, account 5/900s). `CommandSubmission` drops `role`/`capability`/`human_approved`/`enabled_action_ids`; unknown fields are dropped silently so a legacy body parses and changes nothing. Migration `004_bound_approvals.sql`: `memberships` gains `role`/`capability` columns (defaults least-privileged), `archava_commerce.approvals` with FK+immutability trigger+forced RLS. Bound approval is a database row: created by `POST /approvals`, stamped by a distinct human via `POST /approvals/approve` (service principals refused `approver_ineligible`, self-approval refused `self_approval`), consumed exactly once (trigger refuses un-consume). Gate re-evaluates on CURRENT grants for replays. New `tests/authority.rs` (15 tests, fixture-backed, real HTTP + real PostgreSQL): 8 required checks (unauthenticated, forged body, revoked execute/status/replay/recovery, authorized+receipt, cross-tenant DB boundary) + CSRF, consumed-approval, service-approver, self-approval, unknown-action, scope-rule, declined-confirmation negatives. New `tools/rust/pg-authority.mjs` producer (named witnesses, `witnessTotal: 8`), gate step `pg authority suite`, `fastGateEvidenceMapping` P-AUTHORITY-PG entry, manifest sync. `pnpm rust:verify` **11/11 steps, exit 0**. |

Consolidated candidate: `pnpm rust:verify` **11/11 steps, exit 0** on this tree
(fmt, clippy `-D warnings`, wasm boundary, `cargo test --workspace` 10.3s, readiness
suites, the authority suite, the new bootstrap tests inside `node --test`, both
parity harnesses, both cold consumers).

## §1.3 round, 2026-10-16 — closed this session

| Finding                      | Status     | Proof (actual exits)                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| ---------------------------- | ---------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RF02 owned PG fixture        | **CLOSED** | New dev-only crate `archava-pg-fixture`: per-test `provision()` = `initdb` into a unique temp dir + free loopback port + real commerce migrations applied by a migration-owner role + roles/pools (maintenance/admin/owner/runtime/single) + teardown (stop + delete). Process-wide `PROVISION_SLOTS` semaphore + 300s stale-dir reap with `/proc/<pid>` liveness; fail-loud at `initdb` (no skip, no `ARCHAVA_TEST_DATABASE_URL`, no `/tmp/pgdata`). 11 integration tests in `adapters/tests/tenant_context.rs` green (~5.4s): non-owner cohort, forced RLS on all 10 tables, no leak across commit/rollback/begin, RLS scoping, owner/superuser refusal, tampered checksum, unforced policy, invalid namespace. Zero leaked fixture dirs. CI verify job installs PostgreSQL tooling.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| RA02 readiness gates serving | **CLOSED** | `AppState::try_new(pool, ledger, expected_migrations)` is now the ONLY public constructor and runs `assert_startup_readiness` (non-owner runtime → exact `(id, sha256)` migration ledger → forced RLS on every ledger table) before any state exists — no state means no router means nothing binds. `AppState::new` is `pub(crate)`; the old external test that could reach it moved in-crate (`src/extractors.rs`). Producer suite `host/tests/readiness.rs` (5 tests): broken fixture (dropped FORCE / tampered checksum / owner-role connection) each makes `try_new` return the named readiness `Err`, ready fixture constructs state and `/healthz` answers 200 over real HTTP. Evidence wiring: new gate step `pg readiness suites` (`tools/rust/pg-readiness.mjs`) grades the 5 P-READINESS-PG required checks against **named witness tests parsed from harness output** (a deleted proving test fails the step, not just "all tests passed"); drift test pins witness map == manifest requiredChecks + `expect.witnessTotal: 15`. Fast gate **10/10 steps, exit 0**; `P-READINESS-PG.json` written **pass**; coverage **44 → 39 errors**.                                                                                                                                                                         |
| RF03 fmt+Clippy              | **CLOSED** | `cargo fmt --all --check` exit 0; `cargo clippy --workspace --all-targets --offline --locked -- -D warnings` exit 0 after fixing `extractors.rs` (redundant import, 2× unused-mut) + formatting `tenant_context.rs`. Also fixed the `declined`≠revocation conflation in `router.rs`/`extractors.rs` wording (§1.3.3).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| RA03 fresh unit suite        | **CLOSED** | Artifact-bound tests split to `tools/rust/benchmark/test/pricing-coverage.artifact.test.ts` (hard assertions, no `if (!exists) return`), collected only by `vitest.artifacts.config.ts`; default `vitest.config.ts` excludes `**/*.artifact.test.ts`. Cold proof: benchmark `build/` renamed away → `pnpm test` **137/137 files, 1821 tests, exit 0** → restored. Artifact config runs 4/4 green in release gate (step `rust artifact proofs (vitest)` after `compiled-consumer benchmark` — the files are produced by `pnpm rust:benchmark`, not `build-artifacts`).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| RA05 cold bootstrap          | **CLOSED** | New `pnpm rust:bootstrap` (`tools/rust/bootstrap.mjs`): `cargo fetch --locked` workspace + benchmark + consumer registry closure (parsed from consumer manifest, fail-closed). Proof: `CARGO_HOME=/tmp/opencode/cold-cargo pnpm rust:bootstrap` exit 0 (empty cache → 124M fetched) → `cargo metadata --offline --locked` workspace exit 0 + benchmark exit 0. CI verify job now runs bootstrap before `pnpm rust:verify`. Declared toolchain pins added: `tools/rust/benchmark/rust/rust-toolchain.toml`, `examples/rust-consumers/crate/rust-toolchain.toml` (channel 1.96.0).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| RF01/RA06 coverage           | **CLOSED** | Frozen manifest `docs/rust/required-coverage.json` (93 items: R00–R11 + G01–G10 + OWN-T01–T10; 51 proofs: 42 evidence + 7 document + 2 json). Producer `tools/rust/evidence.mjs` (checks/identity/source+artifact+reference digests; `result` derived, never asserted). Validator `tools/rust/coverage.mjs` (`node tools/rust/coverage.mjs` → live: **complete=false, 44 errors** — every one a named missing producer, none invented). `inventory.mjs` five-row `proofs[]` ledger REMOVED (replaced by measured `requiredCoverage` pointer); `inventoryBinding()` now delegates to the validator. Fast gate + release gate write evidence from actual step exits (`writeStepProofs`), pass AND fail. New guarded step `wasm portable boundary` (`tools/rust/wasm-boundary.mjs`): live run **exit 0** — portable 6 crates compile wasm32 (0.3s cached), host crate refused exit 101 (a PASS), 34 sources scanned clean; CI installs `wasm32-unknown-unknown`. Negative tests replay every reaudit probe against fixture roots (empty dist / one invented proof / stale digest / wrong identity / contradicting result / never-ran check / failing+undeclared check / moved tally / structural manifest break) + drift tests (gate steps == mapping == manifest). `node --test` on the four Rust test files: **67/67 pass**. |
| RA03 unit count              | note       | pricing-coverage.test.ts: 13 tests before → 10 unit + 4 artifact tests; nothing dropped, one precondition split out.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |

## Next actions (exact order, resume here)

1. **§1.4B vertical slice** — runnable Rust host + Leptos SSR/hydration: login → contacts/pipeline → reviewed command → persisted effect → durable receipt → authorized recovery (refresh/lost response/restart) + negative tests (unauth/forged tenant/approval/revocation/CSRF/stale revision). Produces `P-RUST-BROWSER`. Backend plan settled this round: extend `AppState::try_new` with an optional CRM ledger, serve real `GET /contacts` from `archava_crm.contacts` (`packages/crm-pg/migrations/001_crm_contacts.sql`: memberships with `permissions text[]` CHECK + scope `all|assigned`), then a typed `contact`/`followup` business command with a bound approval + durable receipt + authorized recovery through the same authority code path the commerce routes use, then the Leptos slice crate (`rust/crates/slice` is still a shell) + `examples/slice_server.rs` + browser proof.
2. R02 remaining conformance proofs — planning/capacity conformance/measurement/cold-consumer (`P-CAPACITY-CONFORMANCE`, `P-PLANNING-CONFORMANCE`; the RA07 arithmetic fix is landed and green, the conformance proofs are still unrun); then R03–R11, OWN-V1 (`P-OWN-T01`–`T10`), final gates + `FULL_RUST_COMPLETION_REPORT.md` + `FULL_RUST_AUDIT_PACKET.md` (the two missing documents the validator names).
3. Every new producer writes its evidence through `tools/rust/evidence.mjs` against the manifest entry already frozen for it; `node tools/rust/coverage.mjs` is the running scoreboard (44 → 0 errors is the completion curve, and no error may be deleted from the manifest to shorten it).

## Where the work actually stands

Separate denominators, never one blended percentage:

| Denominator                         | Done                    | Total                              |
| ----------------------------------- | ----------------------- | ---------------------------------- |
| R00 deliverables                    | 4                       | 4                                  |
| Milestones (R00–R11)                | R01 partial, R02 4 of 4 | 12                                 |
| Rust production lines (`code` rule) | 4,324                   | 130,335 (TS production, same rule) |
| Same, counting every non-blank line | 7,355                   | 148,939                            |
| Rust proofs with an automated gate  | 6                       | 6                                  |

The last row is the one this checkpoint added, and it is the only one here whose
"Done" number means "a gate fails when the proof stops running" rather than "the
code exists". Six: `cargo fmt --all --check`, `cargo clippy --all-targets`,
`cargo test --workspace`, the two parity harnesses, and the two cold consumers —
each was either un-invoked or invoked by hand before `tools/rust/verify.mjs`. A
seventh, `compare.test.mjs`, was written for `node --test` and never run.

Both Rust figures above are the counter's own output for `rust/crates/` and
nothing else, which is the honest scope: the counts moved from 2,844 / 7,355's
predecessor set as the promoted crates were added, and — as of the benchmark
harness — `tools/rust/benchmark/rust/src/main.rs` (**491 lines**) is _not_ in
either denominator, because the counter walks `rust/crates/` and the benchmark is
deliberately not a workspace member (see the R02 row below for why). The harness's
own footprint is 491 lines of Rust plus 824 of runner and 636 of tests. Measured,
not estimated, and reported as a separate line rather than folded in to make the
denominator bigger.

Every figure in that table is produced by `node tools/rust/count-lines.mjs`
(`--rule code` for the first row, `--rule raw` for the second), which states its
own counting rule in its output. None of them is typed by hand.

### The line counts below are measured, and why the old ones were thrown away

This file previously carried `4,715 production | 7,563`, a prose paragraph
saying `4,649 / 2,279`, and a TypeScript denominator of `125,573`. All of those
are gone. They were not wrong arithmetic — they were **four numbers that no
counting rule reproduces**, and `4,649 + 2,779` does not even equal `7,563`, so
the two halves of one row contradicted each other while it was being read.

The TypeScript denominator is the instructive failure. I tried to reproduce
`125,573` with raw tracked lines, tracked-minus-tests, non-blank-non-comment, at
the working tree and at the baseline HEAD. The answers were 115,910, 114,388,
133,663, 120,439, 152,023, 155,948, 150,148 — **none of them 125,573**. It came
from a rule I could not identify, applied to a tree state I no longer have. It
stayed in three documents (`EXECUTION_STATE.md`, `KNOWN_DIFFERENCES.md`,
`MIGRATION_INVENTORY.md`) because a number in a denominator looks like a
measurement even when nothing can reproduce it, and every ratio and percentage
multiplied by it was decoration. `Hitung ulang; jangan memaksa hasil aktual sama
dengan angka itu` — the instruction that applies here.

So the denominator is now derived by the same tool that derives the numerator,
under one stated rule, on the same run. The `code` row and the `raw` row are
both printed because the choice between them moves Rust by 42% — 7,563 raw
against 5,050 code — which is larger than the entire Rust delta between two
checkpoints. A reader who cannot see which rule is in force cannot compare
anything, which is the failure mode the two rows exist to avoid.

Per-crate production / test, `code` rule:

| Crate              | Production | Test      |
| ------------------ | ---------- | --------- |
| `archava-adapters` | 180        | 98        |
| `archava-core`     | 889        | 330       |
| `archava-domain`   | 267        | 248       |
| `archava-host`     | 230        | 415       |
| `archava-native`   | 130        | 255       |
| `archava-pricing`  | 876        | 806       |
| `archava-rates`    | 272        | 139       |
| **Total**          | **2,844**  | **2,291** |

Two of those numbers differ from the previous table for a reason worth
recording, both of them bugs in this measurement rather than changes in the
code:

**`archava-rates` moved from `438 / 208` to `272 / 139`.** Its 208 test lines
live in `tests/rates.rs`, an integration file that carries no `#[cfg(test)]`
attribute, so a rule that looks only for `#[cfg(test)]` blocks calls it all
production code. A test file is test code wherever it sits. The new rule treats
a whole file under a `tests/` directory as test code, which is why its
production figure dropped rather than rose.

**`archava-native` moved from `274 / 361` to `130 / 255`.** The old figures were
raw lines; the new ones exclude blank lines and comments. Its shape is unchanged
and still the point: the bridge is a serialization boundary between two
languages, and a boundary's correctness is defined by what it refuses to accept.
It remains one of the few crates here that is more test than code, and the host
is the other.

The host having more test lines than production lines is the intended shape for a
host whose R01 job is mostly to _refuse_. The router is a boundary, and a
boundary's correctness is defined by what it rejects.

`archava-pricing`'s production line count is 876, not the 1,555 quoted earlier:
that 1,555 was a raw-line figure that included its own inline tests. The
`code`-rule split above is the one to compare against.

The jump from 2,448 to 2,844 production lines is **not** new work — it is
`archava-pricing` and `archava-rates` promoted from the pilot (ADR-R06), so the
number moved without a line being written. Stated that way deliberately: a
milestone total that grows by importing verified code is a different thing from
one that grows by doing the work, and conflating them would be the inflation
this file exists to prevent.

### R02 — all four exit requirements met and re-verified today; the milestone is still not closed

R02 asks for pricing, deployment/assembly planning and capacity math, with
approved parity, compiled-TS benchmarks and **two independent cold consumers**.
All four now stand on evidence produced in this session, and the reason that is
not the same thing as `accepted` is below the table rather than in it.

| R02 requirement                                 | State                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ----------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Pricing ported to Rust                          | **Done, re-verified in place.** `archava-pricing`, 56 tests, promoted from the pilot and re-run against the product tree.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| Reference rates ported                          | **Done, re-verified in place.** `archava-rates`, 11 tests.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Wasm boundary preserved                         | **Proven, and the count moved.** Six non-host crates are wasm-clean — core, domain, pricing, rates, assembly, capacity; the portable gate covers all six and the host still fails it with mio, exit 101. A seventh leaf has since arrived (`capacity`), and it is wasm-clean for the same reason the others are: it carries no date crate, so it brings neither a timezone database nor a locale surface.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        |
| napi bridge for the harness to call             | **Done, re-verified in place.** `archava-native`, 16 tests, promoted from the pilot; built to `libarchava_native.node` and exercised from Node (see below).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| Differential parity harness in the product tree | **Done, promoted and re-verified in place.** All seven modules now live at `tools/rust/parity/`, every pilot-relative path corrected, and both slices re-run green from the product tree (see below). Not outside every glob anymore, and not claimed to be.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| Two independent cold consumers                  | **Two built, both green offline.** `pnpm rust:verify-consumers` unpacks both `.crate` tarballs and copies the addon into a `mkdtemp` scratch tree, then runs them: consumer A (`examples/rust-consumers/crate`) compiles a Rust integration test against the packaged crates alone — **4/4 tests**, no `path =` into `rust/crates/`, nothing from the registry (`--offline`, vendored closure); consumer B (`examples/rust-consumers/addon`) requires a _copy_ of `libarchava_native.node` from a cold Node process — **8/8 checks**, 26 ordered quote keys across the FFI, a malformed request refused rather than panicking, `ratesJson` pricing `garden-twin` at 850,000 IDR. Evidence: `dist/rust-consumer-proof.json` (addon sha256, both tarball hashes, `pricebookSha256`, node + cargo versions). The scratch tree is deleted in a `finally`; the hashes are what let anyone re-check it. **Not covered:** the TS baseline, the other five crates, any production behaviour.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| Deployment/assembly planning ported             | **Done, verified in place.** `archava-assembly`, 31 integration tests over a deserialized release catalogue — pure set logic, no HTTP and no database surface. Two structural findings are recorded in `KNOWN_DIFFERENCES.md`: the baseline validates `contracts.assembly` and `host_settings` through **two independent gates** (one in the planner, one in the packager), so passing one is not passing both; and a plan-time prerequisite throw in `planAssembly` is **provably dead**, because every path that could reach it has already been rejected by a precedence check. Both are the baseline's own shape and are reproduced, not corrected.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          |
| Capacity scheduling math ported                 | **Done, verified in place, and heavily differential.** `archava-capacity`, 41 tests (30 unit + 11 integration). Covers `calculateSchedule`, `countWorkingDays` and `nextBusinessDay` from `packages/capacity-pg/src/repository.ts:96-231` — the 1000-iteration cap, the `insufficient_capacity` throws, the queue-position rule, the three `Math.max(1, ...)` floors and the `+1e-6` capacity tolerance. No date crate, by decision (see `crates/capacity/Cargo.toml`), so instants are signed `i64` millisecond counts and the three calendar primitives are hand-implemented. Fidelity evidence is `tests/reference.json` (37 cases, generated by `tools/rust/capacity-probe.mjs`) because `calculateSchedule` is tested nowhere in the product — the only capacity test file stubs its repository.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Compiled-TS benchmark harness                   | **Done, and it earned a row through two bugs of its own.** `tools/rust/benchmark/` — a real Rust binary (`tools/rust/benchmark/rust/`, path-depending on `archava-pricing` and `archava-rates` exactly as a cold consumer does, deliberately _not_ a workspace member — an empty `[workspace]` table in its manifest is what stops cargo adopting it) timed against the compiled esbuild baseline, `pnpm rust:benchmark`. Both sides are compiled, because PRD §3.4 rejected the earlier benchmark for using "TS source/tsx and Node26; bukan bukti full product, compiled consumer, p95/RSS, atau runtime baseline Node22". PRICE-05's separation is structural, not a note: `steadyState` (engine built once), `parseOnly` (pricebook parsed, nothing priced) and `setupPlusQuote` (re-parsed per call, the addon's own shape) are three separate measurements in `rust/src/main.rs`, so a Rust parse-per-call can never be silently compared against a TS construct-once. Reported separately, on one line each, with medians and ranges. **Four runs of this harness so far have given four different sets of numbers, so the figures below are given as ranges rather than as a claim, and the live evidence file is the only place a current number should be read from.** The verdicts are what is stable: pricing **steady-state `candidate-faster`** on all four runs (ratios observed between 7.479x and 9.961x — the spread is the baseline's own variance, whose coefficient of variation reached 60.5% on one run, not a code change), pricing **setup+quote `candidate-faster`** on all four (2.218x–2.448x), and rates **`within-noise`** on all four — with the _reason string_ changing between runs, from overlapping ranges to "the ranges are disjoint but the gap is too small to call" (an 8.66% margin where the rule requires more than 50%). That last detail is the reason no rates number is recorded here as a finding: PERF-02 names inconclusive as a valid result, the rates baseline's coefficient of variation moved between 11.9% and 19.7% across runs, and the harness reports whichever string the measurement actually produced rather than picking the favourable one. `verdict` demands a 50% _relative_ margin (`better.max * 1.5 < worse.min`), so a disjoint-but-narrow gap and an interleaved pair get different reason strings instead of a pass. Conditions are registered _before_ measuring per PERF-01: node, platform, CPU, cores, RAM, rustc version, warmup, reps, `networkAccess: "none"`, and sha256 of the pricebook and of both case files. Coverage is counted by the code that executes, never by the workload's length — `rates.casesRun 753` is incremented once per op that ran, and the pricing side asserts the candidate priced **exactly** what the baseline priced (338 of 350, with 12 deliberate error fixtures rejected; the pair must sum to the workload, which is why asserting `=== 350` would fail on a healthy tree and get loosened until it checked nothing). Two bugs found here and recorded because both produced plausible numbers: `casesRun` written as `ops.len()` ran 568 of 753 cases and still reported `candidate-faster 1.702x`, and folding the baseline's `null` answers into the consumer's "declined" state via `?` mis-filed 319 legitimate answers as refusals. Regression tests: `tools/rust/benchmark/test/` (37 tests, 3 files), including a coverage.test that pins the 568-of-753 shape and a pricing-coverage.test that re-measures the live 338/12 split whenever the compiled baseline exists. RSS is reported too, because PRD §3.4's rejection named it explicitly: the consumer reports its own `VmHWM` (pricing has read 3376–3424 KiB and rates 3796–3800 KiB across the same four runs — a peak measurement moves by a few KiB every run, so the number to trust is the one in the current evidence file, not any figure transcribed here) rather than a `VmRSS` snapshot, and `null` rather than `0` where the read fails — substituting a well-formed zero for an unreadable measurement is the coverage bug's own failure shape. Node 26 removed `resourceUsage` from both `execFileSync` and `spawnSync`, so the figure has to come from inside the process that did the work. The TypeScript baseline's peak is **_not_ reported**, and that is deliberate: it is timed in-process (which is also what keeps PRICE-05's construct-once comparison fair), so there is no process boundary to attribute a peak to, and the harness process's own high-water mark (102904–104024 KiB across the same four runs) covers the esbuild compile, both case files and the Node runtime on top of the engine. Reporting the two side by side would invite a 30x "memory" claim that is an artefact of scope, so `memory.scope` and `memory.notComparableBecause` say so in the evidence file itself and no ratio or verdict is recorded. |

| Cold-consumer residue, corrected | **Found during close-out, fixed.** The proof script itself has never written into the product tree — `root` is used only for reads (package dir, addon, pricebook, evidence) and every cargo/test invocation runs on a scratch path. But a **manual** run during gate development did: it left 126 MB of cargo build output (`target/`, `vendor/`, `deps/`, `Cargo.lock`, `.cargo/`) inside `examples/rust-consumers/crate/`, and a `.cargo/config.toml` at the **repo root** redirecting crates-io to a `vendor/` directory that does not exist there. That root config broke the product Rust workspace outright: `cargo metadata --offline --manifest-path rust/Cargo.toml` exited **101** (`failed to load source for dependency serde`). Removed, backed up to `/tmp/quarantine-cargo/config.toml` (sha `77e9219c…`), workspace re-verified (`cargo build --offline` → 0). The consumer tree now holds **3 source files / 10 KB** (`Cargo.toml` 3,051 B, `tests/consumer.rs` 6,650 B, `.gitignore` 543 B), a `.gitignore` covers the six generated paths, no root `.cargo/` exists, and a cold re-run reproduces from that state. |
| Second instance of the same defect, closed | **My own, introduced by the benchmark crate.** The existing ignore rule was `rust/target/`, which is _anchored_ to that path, and the benchmark consumer deliberately sits outside the workspace at `tools/rust/benchmark/rust/` — so its **133 MB** of build output was stageable, the same failure as the row above one milestone later. Generalised to `**/target/` and verified with `git check-ignore -v` on both trees; `git status --porcelain -uall` now reports **0** stageable `target/` entries repo-wide and no tracked file has `target` in its path, so nothing that should be committed got hidden. A stray `tools/rust/benchmark/rust/log` — 169 B of my own cargo redirect output, missed by `*.log` because the name has no extension — was removed the same way. |

**What the napi promotion actually proved, and what it did not.** Both exports
answer correctly through Node from the product tree: `quoteJson` returns a
26-key quote from a real pricebook, and `ratesJson`'s `nightlyRate` returns
`{"unitId":"garden-twin","night":"2026-10-05","amountMinor":850000,"currency":"IDR"}`
— the same 850,000 IDR the ported Rust test asserts, computed across two
languages and a serialization boundary. `cheapestBookable` resolves 1 unit.

That is a **compiled consumer**, not a cold consumer, and for a long time the
difference was the whole remaining row. It proves the bridge and the engines are
callable by a program that did not previously know about Rust. It does not prove a
_second, independent_ program works on a cold start, and it does not prove the two
engines agree with the TypeScript baseline. The second consumer now exists (the
row above); the two engines agreeing is still only the parity harness's claim,
and that is where it belongs — these consumers deliberately do not re-check it,
because a consumer that recomputed parity would be a second harness.

One honest loose end from that check, now closed: `quoteJson`'s monthly field
printed `undefined` because I guessed the field name instead of reading the
response type — it is `monthlyBase`. Inspecting it properly, the response has
**26 keys** and every one of them matches the TypeScript `Quote` interface
key-for-key (`region`, `currency`, `presence`, `capability`, `environment`,
`support`, `coreComponents`, `paidCoreComponentCount`, `bundleDiscountPct`,
`fixedCoreSetup`, `discountedCoreSetup`, `integrations`, `integrationSetupTotal`,
`designAddons`, `designAddonSetupTotal`, `usageAllowance`, `overage`,
`estimatedOverage`, `oneTimeTotal`, `monthlyBase`, `annualPlatformOption`,
`deterministicSubtotal`, `fromMinimumTotal`, `requiresCustomQuote`,
`unresolvedScopeDrivers`, `roundedWithIncrement`). 26 for 26, no orphan fields in
either direction.

**A spot parity check, run by hand against the baseline.** With the boundary now
working I fed 432 requests (2 regions × 3 presences × 4 capabilities × 3
environments × 6 option-sets) through both engines and diffed the full JSON:

- **96 compared, 0 mismatches** — byte-identical including every nested array.
- **336 both errored, and the error messages matched on all of them.** Notably
  these did _not_ reproduce D01's error-class divergence: `PricingError:
Unknown presence "none"` came back from TypeScript and `Error: Unknown
presence "none"` from Rust. Same message, and only the type-name prefix
  differs — which is exactly the boundary mapping D01 calls for.
- 0 outcome divergences (one engine quoting where the other refused).

This is **not** the R02 harness and does not close its row. It is 432 cases from
one ad-hoc script run in a shell, unversioned and discarded; the pilot's own 350-
fixture run is separate evidence over a different set. What it does establish is
that the promoted engines, reachable through the promoted bridge, agree with the
TypeScript baseline on the request families I could construct — the strongest
honest statement available without the checked-in harness.

Two bugs in my own harness, recorded because both produced a _plausible_
wrong answer rather than an obvious failure. `continue outer` in nested loops
skips to the outermost loop, so my first run reported 4 cases where the
cross-product was 480 — I nearly recorded "4 cases, 2 mismatches". And the
"mismatches" were a harness defect: I flattened `standardIntegrations` into
`standard`, and Rust's `deny_unknown_fields` — added to the port precisely
because serde's default silently dropped mismatched keys once, per the comment
in `crates/pricing/src/request.rs` — correctly rejected them while TypeScript
accepted them as a stray property. Same lesson as D09 and D10, third instance:
the measuring instrument is where these hide.

The last row is the honest core of R02 and it is the one thing that cannot be
asserted from the pilot's results: a parity run proves two engines agree, and a
benchmark proves one is faster. Neither proves a _consumer_ works — that the
port can be dropped into a real program that did not previously know about Rust
and still function on a cold start.

An earlier version of this paragraph ended "Nothing in this repository
demonstrates that yet." That sentence is now false and was corrected rather than
left to be quietly outvoted by the two rows above it: `dist/rust-consumer-proof.json`
holds two cold consumers, re-run green today (4/4 Rust integration tests against
the packaged `.crate` files alone, and 8/8 checks against a _copied_ addon from a
cold Node process), with `coldOffline: true` and `productTreeReached: false` on
both. What still stands is narrower and worth keeping: a consumer proves the port
works outside the workspace, and proves nothing about the TypeScript baseline —
the parity harness's claim, not a second copy of it.

**The parity harness is now in the product tree.** All seven modules were
promoted from `Experiments/rust-pilot/node/` to `tools/rust/parity/`, and every
path that was correct at the pilot's location was corrected for this one:

- `runtime.mjs` read `../target/...`, right when the Cargo workspace was the
  pilot's sibling and wrong now that it is `rust/` in the product. Now
  `../../../rust/target/release/libarchava_native.node`.
- `run-parity.mjs` resolved the workspace root with a relative climb and handed
  it to `tsx` as the parent URL. The climb's depth still happens to be three
  from here, which is exactly the situation D10 warns about: a path that
  resolves by coincidence is indistinguishable from one that is right. It is now
  guarded by an `isProductRoot` check that asserts `package.json` and
  `packages/pricing/package.json` exist and throws with a readable message
  otherwise.
- `run-parity-rates.mjs`'s root got the same guard against
  `packages/reference/package.json`.

Both slices re-run green from the product tree, with exit codes captured inside
the redirects:

| Slice      | Fixtures | match | corrected-expectation | diverge | Exit |
| ---------- | -------- | ----- | --------------------- | ------- | ---- |
| p1 pricing | 350      | 338   | 0                     | 0       | 0    |
| p2 rates   | 753      | 748   | 5                     | 0       | 0    |

p1's 12 non-matches are all `error-class-only` — D01's boundary mapping, reported
and not counted as failures.

**Why p2 exits 0 with five known disagreements.** This was the interesting
problem. p2 exits non-zero on any `diverge`, which is correct, but five
`__proto__` fixtures diverge where the _baseline_ is wrong and the port is right
(D02). The harness could therefore never be green without one of two moves:
delete the fixtures, or weaken the exit rule. Both would stop measuring parity,
so neither was made. Instead `compare.mjs` gained a fifth verdict,
`corrected-expectation`, and the five fixtures are recorded as **approved** with
the exact canonical string the port must still produce.

The approval is checked, not assumed. Each run re-verifies that the port emits
`null` for those five; if that behaviour ever changes, the verdict reverts to
`diverge` and the run fails. An approval that stopped being verified would be
indistinguishable from an approval that was never earned. A stale-approval guard
throws if an approved id names a fixture the case file no longer contains. And
`corrected-expectation` is counted in its own tally key, never merged into
`match`, so nobody can read 753 agreements where 748 are agreements and 5 are
recorded disagreements.

**Two defects found in the promoted evidence, both fixed.** The manifest field
`runtime` was `requestedRuntime()`, which reports the `RUST_PILOT_RUNTIME`
environment variable and defaults to `'ts'`. Nothing in either harness selects
an engine from that variable — `requireRust()` _throws_ when the addon is absent
rather than falling back — so a run that executed both engines wrote
`"runtime": "ts"` into the very file whose purpose is to prove Rust was
exercised. Both harnesses now write `runtime: "ts+rust"`. This is worth
recording as more than a typo: the field was inherited from the pilot, where it
was _accurate_, and it became false on promotion without any code that reads it
breaking. A field nobody consumes is a field nobody notices is lying.

Second, and deliberately left as-is rather than "fixed": p1's `failed` counts
every non-match while p2's `passed` includes its five approved corrections. The
word means different arithmetic in the two files. That is a real consequence of
p2 having approvals and p1 having none, not a defect — but it is now written
down at the `manifest` literal in `run-parity.mjs` so a reader comparing the two
pass rates does not read 12 unapproved differences as worse than 5 approved
ones.

**Determinism.** Both evidence files are regenerated by their harnesses, not
hand-maintained — `run-parity.mjs:294` exists because a hand-kept
`p1-parity.json` kept claiming `"fixtures": 346` after the case set had grown.
p2's `casesSha256` is byte-identical to the pilot's (`e90dbf7a…`), which is the
re-verification ADR-R06 asks for: the case file survived promotion unchanged.
p2's `resultsSha256` differs from the pilot's only because five verdict labels
changed from `diverge` to `corrected-expectation`, which is the intended effect
of this checkpoint and nothing else.

**R02 close-out — what is proven, and why the milestone is still not `accepted`.**
All four exit requirements were re-verified against live evidence today, not from
the checkpoint prose: both parity slices re-run green after the benchmark rebuild
(p1 350 fixtures / 338 match / 12 error-class-only / **0 diverge**, exit 0; p2 753 /
748 match / 5 corrected-expectation / **0 diverge**, exit 0), the benchmark ran
three separate verdicts, both cold consumers re-ran offline (exit 0), and the
scope is still pricing and rates only. The benchmark evidence's
`ratesCasesSha256` (`e90dbf7a…`) is byte-identical to p2's own `casesSha256`, which
is the check that both measured the same bytes rather than two similar ones.

The repo gates were also re-run and are undisturbed by this work:
`pnpm format:check` exit 0, the **benchmark crate's** `cargo fmt --check` and
`cargo clippy --all-targets -- -D warnings` both exit 0 (run from
`tools/rust/benchmark/rust` — the workspace-wide `cargo fmt --all --check` is a
separate row that was failing when re-checked and is now fixed, see the gate
table above), vitest 37/37, and
`cargo test --workspace --offline --manifest-path rust/Cargo.toml` exit 0.
`pnpm security:verify` completed exit 0 over **46 commands** — 45 of them exit 0,
and the single non-zero is `template-audit=1`, whose `[0, 1]` tolerance is
declared at `tools/security/verify.mjs:92-96` (the same band all three audits
carry, not one added for this) and corresponds to the 13 pre-existing moderate
findings in the OpenTelemetry chain (`audits.platform` and `audits.runtime` are
both zero across every severity). 83 controls, all passed.
That chain ran to completion with my changes already in the tree, so the
benchmark crate and the `.gitignore` generalisation are inside what it proved.

So the milestone's _requirements_ are met and its _scope_ is still narrow. The
one thing that kept it short of `accepted` was that nothing in `pnpm verify` or
`tools/security/verify.mjs` ran either parity harness, the benchmark, or the
consumer proof — all of them were by-hand invocations, exactly as in the pilot,
so a future regression was caught by whoever remembered to run them. **That
obstacle is now removed** (see the gate wiring below): both parity harnesses, the
two cold consumers, and the three checks no gate ran at all are now steps of
`pnpm rust:verify`, which is a gate of `pnpm verify` and of CI. R02's evidence is
therefore no longer merely reproducible on demand — it is regression-guarded.
What still keeps the milestone short of `accepted` is unchanged and structural
rather than a missing box: three of the ported crates' fidelity rests on
fixtures generated by their own ports, and the comparator's own tests were, until
this checkpoint, wired to nothing. The `[0, 1]` audit band is likewise recorded
rather than resolved: the 13 moderate template findings stay open and are not
counted as clean.

**Gate wiring, which closes the "run by hand" objection (this checkpoint).**
Two gates now exist, and the split is §1.1 item 1's own:

- `pnpm rust:verify` — the fast gate, `tools/rust/verify.mjs`. Eight steps, in a
  load-bearing order: the release artifacts (both parity harnesses and the cold
  consumers consume them, so building after them would leave two proofs
  reporting two different hashes for one build), `cargo fmt --all --check`,
  `cargo clippy --all-targets --locked --offline -- -D warnings`,
  `cargo test --workspace`, the two `node --test` files that existed with no
  invoker, both parity harnesses each followed by a frozen-expectation
  assertion, then the two cold consumers. It is a gate of `pnpm verify` and of
  CI's verify job, so a Rust regression now fails a PR.
- `pnpm rust:verify-release` — the full candidate gate,
  `tools/rust/verify-release.mjs`. The fast gate, then the compiled-consumer
  benchmark, the 46-command security chain, the browser end-to-end proof, the
  cold-prior upgrade and restore, the cold-prior accommodation recovery and the
  CRM benchmark. It writes `dist/rust-release-report.json`, which keeps
  `local_verification` and `independent_review` as separate fields on purpose: a
  green run on one machine is not an independent audit.

That first draft of the full gate carried the defect this whole checkpoint is
about, so it is worth naming plainly: it declared
`everyStepInHandoffList: true` as a literal. Nobody had checked it, and it was
false — the gate ran `pnpm rust:benchmark`, which is not in
`docs/LOCAL_LIBRARY_HANDOFF.md`'s reproduce list, and it omitted
`pnpm accommodation:verify --cold-prior`, which is. A field asserting an
uncomputed fact is worse than no field, because a reader treats it as coverage.
The gate now _parses_ the documented list out of the handoff document and
compares the two at run time (`reconcileWithHandoffList`), throwing on any
omission without a recorded reason and on any recorded reason that stopped being
true. The four omissions it does make are each recorded with a reason, and two
of them are removals of my own duplication: `kits:verify` and `capacity:verify`
are already steps inside the 46-command chain, so running them again cost
minutes and proved nothing.

The gate's verdict is `incomplete` on this tree even with every step green, and
the process exits non-zero when it is. The reason is in the report itself:
`docs/rust/migration-inventory.json` carries aggregate counts, not per-behavior
item IDs, so §11 item 3's required inventory binding cannot be satisfied from
the inventory that exists. The binding binds what is real — the frozen baseline's
commit, the release profile version, and the artifact digests
`dist/rust-consumer-proof.json` already records — and records
`requiredInventoryComplete: false` with a `blockedBy` reason rather than
inventing item IDs to look complete.

The `--all` in `cargo fmt --all --check` is not decoration. `rust/Cargo.toml` is
a virtual workspace manifest with no `[package]`, so the bare
`cargo fmt --check` prints `Failed to find targets` and exits 1 **without
reading a single file** — the same exit code as a formatting failure and the
opposite meaning. That is how a false "format passes" row reached a gate table
in this repository before; the row is true now because the command reads files.

`node --test tools/rust` would sweep in the three vitest
`tools/rust/benchmark/test/*.test.ts` files, which fail under node's runner with
`Cannot read properties of undefined (reading 'config')` — vitest owns those 37
tests through the `{packages,apps,tools}/**/test/**/*.test.ts` include. So the
step names the two `.mjs` files explicitly instead of passing a directory.

**A second, unreported instance of the same defect was closed here.**
`tools/rust/parity/compare.test.mjs` was written to be run by `node --test`,
documented in `KNOWN_DIFFERENCES.md` as passing, and wired to nothing. The gate
runs it. That is the third time in this file that a proof existed and was never
invoked, and the reason both gates are wired rather than remembered.

**What the wiring does not do.** It adds no checks of its own to the parity
harnesses, and it does not decide what a difference means. The frozen
expectations in `tools/rust/verify.mjs` pin the tallies each harness already
reports, so a change to either number fails the gate in both directions: 12
`error-class-only` for p1 cannot quietly become 11 (either the port started
matching, which needs the D01 disposition rewritten in the same review, or the
classifier stopped classifying) and it cannot become 100 (the harness would
still exit 0 for either). The gate asserts the number; the disposition in
`KNOWN_DIFFERENCES.md` still owns its meaning.

**What the promotion does not establish.** The two-cold-consumer row above is
now satisfied by `examples/rust-consumers/`, but nothing here wires either slice
into `pnpm verify` or `tools/security/verify.mjs`; both — and the consumer proof —
are run by hand, exactly as in the pilot, so a regression in either engine is
caught by whoever remembers to run them.

**The comparator's test, promoted — and why it had to be.** `compare.mjs`'s own
header cited `compare.test.mjs` as the reason its verdicts are trustworthy. That
citation was the pilot's and the file was never promoted, so in the product tree
the comparator had been running on nine fixtures' worth of _assumed_ correctness
with a comment pointing at proof that was not there. It is now at
`tools/rust/parity/compare.test.mjs`: the pilot's nine tests plus **four new ones**
for the `corrected-expectation` verdict the pilot never had, all 13 passing under
`node --test` (exit 0).

The four cover both directions, because only one direction is worth anything:
an approval whose behaviour still matches is a `corrected-expectation`; an
approval applied to a fixture that already agrees is ignored rather than
manufacturing a divergence; an approval whose behaviour _changed_ reverts to
`diverge` and fails the run; and an unapproved divergence is a plain `diverge`.
That third test is the one that makes D02's approval a check instead of an
excuse.

Two things learned promoting it, both the same lesson as D09 and D10:

1. **Its header's stated reason was false after promotion.** It claimed nothing
   in the product tree sees it because `vitest.config.ts` globs `tools/`. The
   real reason is narrower — the globs match `.test.ts` only, so a `.test.mjs`
   under `tools/` matches neither. That is a fact about the file extension, not
   a directory boundary, and it stops being true the day a glob is widened.
2. **I broke the file with that very comment.** Writing the literal glob
   `*.test.{ts,mjs}` into a `/** */` block closes the comment at the `*/` inside
   the brace expansion — `SyntaxError: Unexpected token '.'`, caught immediately
   because I ran the test rather than trusting the edit. `run-parity.mjs`'s header
   already documents this hazard in prose for exactly this reason; I reproduced
   it anyway, which is the fourth instance this checkpoint of the measuring
   instrument being where the bug lives.

## Completed this checkpoint

**R00 — Freeze baseline.** Status `locally_proven` for its own deliverables.

| Deliverable                | File                                       |
| -------------------------- | ------------------------------------------ |
| Inventory generator        | `tools/rust/inventory.mjs`                 |
| Machine-readable inventory | `docs/rust/migration-inventory.json`       |
| Inventory prose            | `docs/rust/MIGRATION_INVENTORY.md`         |
| Decision log               | `docs/rust/DECISIONS.md` (ADR-R01…R09)     |
| Known differences          | `docs/rust/KNOWN_DIFFERENCES.md` (D01…D10) |
| Root script                | `rust:inventory` in `package.json`         |

Measured counts: `workspaceDirs=75 packageRoots=70 ledgerOwners=25
(23 registered in the release ledger, 22 by `-pg` suffix) sql=30 browser=14
template=8`.

**Corrections applied while measuring** — each is a case of a hand-typed number
disagreeing with the machine:

1. `docs/audit/2026-10-15-release-1.17.0.json` claimed `workspaces: 69`; pnpm
   resolves **70** package roots. Corrected.
2. My own first inventory revision reported 22 ledgers / 27 migrations by
   filtering a `-pg` suffix. Wrong: 25 packages own migration files (30 total),
   and only 23 are registered in the release ledger. Both the true counts and
   the reason for the gap are now recorded in `MIGRATION_INVENTORY.md`.
3. D08 is a new finding, not a correction: the release catalogue validates
   migration ownership in one direction only.

## R00 remaining

- Evidence directory with release-specific identity. **Partially closed this
  checkpoint.** `dist/rust-release-report.json` is the first file written by a
  Rust gate, and it carries the candidate identity, the per-step results, the
  handoff reconciliation and the inventory binding — all produced by
  `pnpm rust:verify-release`, not copied from the TypeScript release. What is
  still missing is a _complete_ one: the record's own verdict is `incomplete`
  on this tree, because the inventory carries aggregate counts and no
  per-behavior proof IDs to bind (§11 item 3). Everything else that was R00's
  — the parity evidence files under `tools/rust/parity/evidence/`,
  `dist/rust-consumer-proof.json` and the benchmark's own evidence file — landed
  under their producing milestone rather than here, which is the right place for
  them. Copying the TypeScript `dist/*-proof` files into a Rust path is still
  correctly refused: it would assert evidence a Rust build never produced.

## R01 — status `prototype`

The workspace, contracts, RLS cohort and Axum host exist and pass every gate
below. R01 is **not** `locally_proven` and is not an owner cutover, because
items 5, 6 and 7 of its scope are genuinely unfinished — see the gaps table.

### Crates (7, in a one-directional dependency graph)

| Crate              | Lines (prod / test)               | May depend on                        |
| ------------------ | --------------------------------- | ------------------------------------ |
| `archava-core`     | contracts, roles, actions, tenant | nothing internal                     |
| `archava-adapters` | RLS cohort, raw SQL               | core                                 |
| `archava-domain`   | authority gate (pure policy)      | core                                 |
| `archava-host`     | Axum router, 4 routes             | core, adapters, domain               |
| `archava-pricing`  | pricing engine (R02)              | serde, serde_json — nothing internal |
| `archava-rates`    | reference rates (R02)             | serde — nothing internal             |
| `archava-native`   | napi FFI bridge (R02)             | pricing, rates, serde, serde_json    |

`pricing` and `rates` are leaves — nothing internal, which is what keeps them in
the wasm-clean set. `native` is deliberately **not** wasm-clean and never will
be: napi produces a `.node` addon for a native runtime, so it sits outside the
portable set for the same structural reason as `archava-host`. It also points
_inward_ at pricing and rates rather than upward, so adding it did not change
the dependency direction the wasm gate enforces — which is why the portable gate
still passes with it in the workspace.

### Gate results (all measured, exit codes captured inside the redirect)

| Gate            | Command                                                                                                                                                     | Result                                     |
| --------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| Format          | `cargo fmt --all --check` (from `rust/`)                                                                                                                    | pass — after a fix, see below              |
| Check           | `cargo check --workspace --all-targets`                                                                                                                     | pass                                       |
| Clippy          | `cargo clippy --workspace --all-targets -- -D warnings`                                                                                                     | pass, 0 warnings                           |
| Tests           | `cargo test --workspace`                                                                                                                                    | **228 passed, 0 failed**                   |
| Lockfile        | `cargo check --workspace --all-targets --locked`                                                                                                            | pass (lock in sync)                        |
| Advisory        | `cargo audit --file Cargo.lock`                                                                                                                             | pass, **182 crates**, 0 vulnerabilities    |
| Wasm (portable) | `cargo check -p archava-core -p archava-domain -p archava-pricing -p archava-rates -p archava-assembly -p archava-capacity --target wasm32-unknown-unknown` | pass                                       |
| Wasm (host)     | `cargo check -p archava-host --target wasm32-unknown-unknown`                                                                                               | **fails, exit 101** — by design, see below |

> **The Format row did not pass when it was first recorded, and the correction is
> kept visible rather than edited away.** Re-reading the gate table during the R02
> benchmark close-out, I re-ran every row instead of trusting it. Format was the
> one that failed: the recorded command, `cargo fmt --check`, exited **1** with 30
> rustfmt diffs across seven files — `crates/assembly/src/{contracts,plan}.rs`,
> `crates/assembly/tests/assembly.rs` and
> `crates/capacity/src/{calendar,error,schedule}.rs`, `crates/capacity/tests/capacity.rs`.
> Both are R02 deliverables (`assembly` from #36, `capacity` from #37), so the
> crates this milestone added were the unformatted ones.
>
> Two separate errors were involved, and only the first was the defect. The table
> row's command was not runnable as written — `rust/Cargo.toml` is a **virtual**
> workspace manifest with no `[package]` and no root target, so `cargo fmt
--manifest-path rust/Cargo.toml --check` prints `Failed to find targets` and
> exits 1 before looking at any file. The runnable form is `cargo fmt --all
--check` from `rust/`, and _that_ is the command that found the 30 diffs. A
> wrapper returning usage-1 and a formatter returning 30-diffs-1 are the same exit
> code and opposite meanings, which is why the row now names the command that was
> actually run.
>
> `cargo fmt --all` was applied and the tree now passes. It changed layout only,
> and that is evidenced rather than asserted: 228/228 tests still pass across the
> same 11 binaries, `cargo clippy --all-targets -- -D warnings` still exits 0,
> `archava-native` was rebuilt in release, and the p2 parity run afterward
> produced a **byte-identical `resultsSha256`** (`ae16a76f9dfaf62c…`) to the
> pre-format run. Formatting was never the risk to the benchmark evidence; an
> unchecked claim that it had already been applied was.

Test counts by crate: core 28, adapters 11, domain 13, host **19**, pricing **56**,
rates 11 (integration file), native **16**, host-extractors **2** (integration
file), assembly 31 (integration file), capacity **41** (30 unit + 11
integration). Eleven test binaries, 228 total.

The workspace grew by 16 tests when `archava-native` was promoted, and those 16
are FFI-boundary tests, not re-counts of the engines' own; by 31 when the
assembly planner arrived; and by 41 when the capacity math did. Rates' and
assembly's integration files and native's two are D09's R02 follow-up; see
`KNOWN_DIFFERENCES.md`.

`archava-capacity` is the only crate in the workspace whose tests are almost
entirely _differential_. `packages/capacity-pg/src/repository.ts` has no unit
test covering `calculateSchedule` — `packages/capacity/test/boundaries.test.ts`
exercises `CapacityService` against a stub repository whose `estimate` returns a
hard-coded object, so nothing in the product pins what the function returns. The
reference file was therefore produced by transcribing the function verbatim into
`tools/rust/capacity-probe.mjs` and running it, and the 37 recorded cases are
asserted as TypeScript's actual answers. One of the 37 (`no-desired-start`)
records `desired: null`, so it read `new Date()`; its timestamps move on every
regeneration and it is compared by its own test rather than in the loop.

Three separate crate-count movements in the audited lockfile, each with its
stated reason rather than left as unexplained drift:

- 171 → 162 when `sqlx`'s `macros` and `migrate` features were removed (R01).
- 162 → 164 when `pricing` and `rates` were promoted — `serde` and `serde_json`
  arriving as first-class workspace members rather than transitive-only.
- 164 → 180 when `archava-native` was promoted — napi, `napi-derive`,
  `napi-build` and their transitive tree (napi is a large FFI runtime).

The last one is by far the largest and the least obvious: 16 crates for a bridge
that itself contains 274 production lines. Worth stating plainly that a 5-line
`build.rs` plus a 630-line `lib.rs` can pull sixteen packages into an audited
lockfile, because that is exactly the trade an audit needs to see rather than
discover.

The host's 10 → 19 tests are the G03 negative cases, and they are new in kind,
not just in count. They drive a `Submission` through the real `axum::Router`
via `tower::ServiceExt::oneshot` against a `PgPoolOptions::connect_lazy` pool —
so the tenant, role, capability and action refusals are proven to fire during
extraction, at the HTTP boundary, before any database contact. The earlier 62
tests included policy-level checks of the same rules; those prove the policy,
not the boundary, and G03 names the boundary.

### A defect found by doing it at the entrypoint

One of those nine tests failed on its first run, and the failure was real:
axum's default `Json` rejection echoed the caller's rejected input verbatim, so
`"tenant": "../etc/passwd"` came straight back in a 422 body. An unauthenticated
route was reflecting attacker-controlled input and naming the field that
rejected it. Recorded in full as **D09**; fixed with a `rejection()` handler
that logs the detail server-side and returns a fixed message identical across a
bad tenant, role and tier, so the response cannot reveal which field was wrong.

This is worth stating plainly: the authority policy was correct the entire time.
The bug was in the extraction layer above it, and a test suite that calls the
authorizer directly — which is what the previous 62 were — structurally cannot
find it. That is the measurable argument for G03's "real entrypoints" wording,
arrived at by hitting the defect rather than by asserting the principle.

### Budgets registered at R01 (PRD §PERF-03: measured, not invented)

| Budget                                      | Measured                |
| ------------------------------------------- | ----------------------- |
| Release build (cold, LTO + codegen-units=1) | 17 s                    |
| `libarchava_host.rlib`                      | 2,074,024 bytes         |
| `libarchava_adapters.rlib`                  | 106,882 bytes           |
| `archava_core.wasm` (release)               | 363 bytes / 326 gzipped |

### The wasm boundary is now structural, not asserted

`archava-domain` declared `archava-adapters` while importing nothing from it.
That dependency dragged sqlx → mio into a pure-policy crate, and mio has no wasm
target. Removing it was the fix, and the result is provable: the host and adapter
crates now **fail** to compile for wasm with
`error: This wasm target is unsupported by mio`, while `core` and `domain`
compile clean. The browser/server split is enforced by the toolchain rather than
by a convention a later change could quietly break.

The R02 boundary this creates is recorded in `crates/domain/Cargo.toml`: when the
first transaction-taking domain function is ported it must go in a separate
host-side crate (`archava-domain-pg`), not into `archava-domain`, or the wasm
gate fails again.

### Secrets kept out of the build, not just out of the binary

`sqlx`'s `macros` and `migrate` features were removed from the workspace. Both
are compile-time-database features: `sqlx::query!` connects to a live database
_during the build_ to validate SQL. This workspace must never require one, since
the standing constraint forbids using the owner database as a target — a build
needing a DB would either fail on a clean checkout or invite pointing it at the
owner. Only the runtime `sqlx::query` path is used. Side effect: 171 → 162
crates in the lockfile.

`rust/target/` (~1.6 GB) is now gitignored. It previously had no ignore rule at
all, so `git status` reported the whole tree as `?? rust/` and one
`git add rust/` would have staged a gigabyte. `Cargo.lock` is deliberately left
visible, because R01's pin is part of the deliverable. Verified: 16 source files
visible, 0 `target/` paths, `Cargo.lock` included.

### R01 gaps — these are open, not implied away

| #   | Scope item               | State                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| --- | ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 5   | Leptos SSR/hydration     | **Not started.** `leptos` measures `0.9.0-rc0` — still a release candidate. Wiring an RC into a release-candidate host is recorded as a blocker, and per PRD §216 a framework change needs an ADR comparing alternatives.                                                                                                                                                                                                                                           |
| 6   | Negative _browser_ tests | **Partly done, and the split is worth being precise about.** G03's negative authority cases now run at the real HTTP entrypoint — 9 of the host's 19 tests, via `router.oneshot` — and they found a defect the policy-level tests could not see (D09). What is still missing is the _browser_ half: no test drives a rendered page in an actual browser, because there is no SSR output to drive (item 5). The HTTP-boundary gap is closed; the browser gap is not. |
| 7   | Secret check on WASM/SSR | **Partially proven, and currently near-vacuous.** The wasm artifact contains no secret-shaped string (18 strings total, all metadata) — but it also contains no policy code, so the clean scan proves little. It becomes meaningful when wasm-bindgen actually exports functions.                                                                                                                                                                                   |
| 4   | Recovery route           | **Boundary recorded, not built.** `/login` and `/contacts` state their caller-owned-identity boundary explicitly rather than faking data; the recovery path is not implemented.                                                                                                                                                                                                                                                                                     |

**Why item 5 blocks items 6 and 7:** without SSR there is no browser entrypoint
to test against, so "negative browser tests" has nothing to exercise. R01's
backend gates are green; its UI gates are not, which is exactly the situation
PRD §216 describes ("lanjutkan backend yang lulus bersama UI existing dan catat
UI migration tertunda") rather than a reason to stop.

**D09's R02 follow-up is closed separately from this table, and closed
vacuously.** It is not an R01 gap and was never scoped as one — D09 opened it
against R02's future tenant-scoped routes, which R02 did not need because the
tenant still arrives in a body and the router has no path parameter. Rather than
leave that as prose, `rust/crates/host/tests/extractors.rs` now guards it: one
test asserts behaviorally that unmatched paths are not reflected, one reads the
router's source and fails loudly if a `Path`, `Query` or `Form` extractor is
ever wired without a refusal handler. Details and the reasoning are in
`KNOWN_DIFFERENCES.md` under D09. Counted: 154 → 156 Rust tests.

## Standing constraints

- Working tree carries uncommitted work. Never reset, stash, clean, amend, move
  product folders, or mass-stage other people's files.
- No commit or push without a new explicit instruction.
- Owner DB, credentials, grants, and backups are never negative-test targets.
  Synthetic fixtures against isolated databases only.
- `apps/crm` must not import from `tools/library/` — the boundary is deliberate.
- Do not edit `docs/audit/local-library/manifest.json` (legitimate 1.3.0
  historical baseline) or `ARCHXY_IMPLEMENTATION_TASK.md` (owner's untracked
  file).
- Never fabricate an exit code. A backgrounded
  `cmd > log 2>&1; echo "EXIT=$?"` reports 0 from the trailing echo; put the
  capture inside the redirected command.

## Outstanding findings carried forward

A01 (command replay after grant revocation, uncovered — D05), A06 (minimal
artifact missing `zod` external), A07 (archive identity drift — D06), A02–A05,
A08–A09, J01–J04. The 83/83 controls do not close A01–A05.

Also outstanding and unrelated to Rust: task #8, nonprofit-associations narrow
scope (K22). R08 names "Laundry, field services, formal education candidate and
other R00-inventory extensions" — nonprofit-associations is one of those
extensions and is not started.

## 2026-10-18 — ERP packet integration (Archxy 2 → main)

PACKET-000 (erp purchasing), PACKET-001 (erp foodservice) and PACKET-002 (R06
two module consumers) integrated from
`/home/haikaru/Archverse/Lab/Archava_ERP_Handoff/archxy2/` after
reconciliation; landed hashes and per-packet dispositions are recorded in that
handoff's `archxy1/` (this tree never writes to their `archxy2/` side). The
fixture sections folded semantically onto main's moved-forward design
(`ledger_expectations`/`attach`/`dir` kept; the packets' snapshot-era
4-parameter `migrate_ledger` and their router/login reversion lines REJECTED —
only erp additions folded). Both erp suites green in the fast gate (16 steps,
exit 0); both erp producers PASS; `pnpm erp:verify-consumers` PASS;
fast-gate unit tests 23/23; release-gate unit tests 24/24; fmt/clippy clean
off-line; `Cargo.lock` regenerates to exactly the packets' predicted net
effect (adapters += archava-domain/serde/serde_json/uuid; domain += serde_json).

Stale-fact corrections vs the packets' owner notes, recorded for the reviewer:
- The §1.4B router test
  `the_contact_list_refuses_rather_than_faking_an_empty_result` and the whole
  host lib suite (41/41) PASS on main at integration time — the failure they
  observed does not exist on the current tree (re-verified twice).
- The crate-tarball offline conclusion matches this lane's own P-K05-K04
  finding (b43598f): packed source bundles are the working offline artifact
  form for rust crates with internal workspace deps.
- The locking-race hardening request (two twin witnesses awaiting the
  holder's confirmation) — main's 8cc7ed4 already addressed the CRM twin; the
  authority-suite twin stays on the watch list; no failure observed in the
  main-tree gate runs so far.

## 2026-10-18 — R09 leftovers, R10 cutover machinery, R11 legacy lane, OWN-V1 (this checkpoint)

Where things landed after the kits/lib maintainership commits (696beb9,
d45641b, e7c3854) and this checkpoint's two commits (d99dbf6, 2b53f0a):

- **R10 — archava-ops crate + pg-fixture prior-profile window (d99dbf6).**
  `crates/ops` ships `status/doctor/verify-release/plan-assembly/backup/
  restore/runbook` as one JSON-line CLI (exit 0 ok / 2 refusal / 1 internal).
  `pg-fixture` gained the prior-window primitives
  (`provision_prior_commerce_profile` through the frozen ledger minus the
  newest migration; the 004-bound approvals grant is skipped only inside the
  prior window). The five fixture-backed rehearsal witnesses
  (`crates/ops/tests/upgrade_restore.rs`) pass 5/5; fmt/clippy clean offline.
  ADR-R05 closed with ADR-R12's five decisions.

- **R11 — legacy lane proven (P-LEGACY-REPLACEMENT).**
  `tools/rust/legacy-replacement.mjs`: spawn census over the candidate's
  runtime source (only PostgreSQL tools + the process-signal wrapper are
  allowlisted; no JS engine in any Cargo.toml), the D07 bridge documentation
  quoted byte-level with dispositions resolving to live paths
  (`packages/crm-assistant/src/gemini-planner.ts` +
  `rust/crates/providers/src/gemini.rs`; the inventory's `apps/crm/src/
  gemini.ts` never existed in git), replacement registry verified as EXECUTED
  from the ops proof artifact. Census: `{pg_ctl: 1, kill: 1}` only.

- **R09 leftovers — the two browser-lane proofs.** The disposition recorded
  in `crates/core/src/graph.rs` keeps SDK-side hosting TypeScript-side, so
  both proofs hold the TS surfaces to their contracts instead of letting
  them be document-only:
  - `tools/rust/sdk-widget.mjs` (P-SDK-WIDGET): §24 source census over
    `packages/sdk` + `packages/chat` (import allowlist, no `node:`, no
    `process.env`), the two reference hosts built from PACKED TARBALLS via
    `tools/library/verify-consumers.mjs` (each host's own verify asserts
    browser-bundle isolation), the studio host (`apps/web`) and the
    page-awareness witnesses — 208 tests green.
  - `tools/rust/realtime-voice.mjs` (P-REALTIME-VOICE): the presence
    gateway's six realtime-lifecycle witnesses, the documented vendor bridge
    in four quoted places (PRD AI-04, the security gate's `realtimeTemplate`
    limit, the template's operator README, the migration inventory's Python
    disposition), and the chat-fallback witnesses — 90 tests green.

- **OWN-V1 — the acceptance family over released surfaces (this
  checkpoint's third commit).** `tools/own/verify.mjs` runs the ten
  P-OWN-Txx proofs fresh per invocation, grades each required check from
  recorded facts, and refuses manifest drift in both directions. T01 = the
  K05/K04 two-business-model consumers; T02 = the new in-crate witness
  `chat_flow_and_direct_planner_resolve_the_same_plan` (21/21 lib tests) plus
  the TS contract suites; T03/T07 = the deployment-plan refusals and the ERP
  module consumers; T04 = the real-PostgreSQL authority suite; T05 = the ops
  binary's executed refusal classes (exit 2, canonical texts, and the
  compatible request still planning); T06/T08 = the fixture-backed rehearsal
  suite + the fresh-process identity recompute; T09 = presence/adapters
  witnesses + dual-engine pricing parity reruns; T10 = the real headless
  Chromium journey via the CRM host battery. Full suite 9/9 green
  (`OWN_SUITE_EXIT=0`); T10 green standalone. ADR-R13 records the placement.

- **Kit-template lock realignment.** The foundation-crate tracking commit
  (5bde51c) changed the shipped-manifest set (`serde_json` in
  `archava-core`'s dependency list), leaving the committed kit-template
  `Cargo.lock` one line behind the pin `k05-k04-consumers.mjs` asserts.
  Regenerated (exactly the one-line realignment); T01 green after.

- **Release-gate wiring.** `tools/rust/verify-release.mjs` now runs the four
  new producers (ops, legacy, SDK/widget, realtime/voice) and the two OWN
  steps; `verify-release.test.mjs`'s pins grew to NINE deliberate additions
  beyond the platform handoff fence; 24/24 green. The fast gate
  (`pnpm rust:verify`) finished green (18 steps, `RUST_VERIFY_EXIT=0`) with
  the R10 wiring in place.

Still open after this checkpoint: the full `pnpm rust:verify-release` run with
all ten OWN evidence files fresh, the D-docs (D-COMPLETION-REPORT,
D-AUDIT-PACKET), the coverage scoreboard refresh, and the completion report.
