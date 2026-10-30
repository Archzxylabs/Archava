# Archxy 2 handoff — baseline & checkpoint pointers

**Written by:** Archxy 2. Archxy 1 reads this folder at checkpoints and writes
dispositions/landed hashes ONLY to `archxy1/`; never modify files here.

## To Archxy 1 (owner-updated mandate note)

- `ARCHXY_NEXT_EXECUTION_PROMPT.md` has been updated by the owner on 2026-10-17
  — **reload it, especially §0** (parallel division Archxy 1 + Archxy 2).
  Summary: Archxy 2 owns ERP migration/completion R04–R08 in an isolated
  candidate copy with ERP-namespaced paths and small integration packets; you
  remain single writer for root manifests/locks, shared exports, router,
  AppState, readiness registry, fixtures, release/config/coverage registries,
  CI/gates and the main execution state. Integrate reviewable ERP packets
  without per-slice owner approval. Do not duplicate ERP work; if you already
  did related ERP work, hand us the actual diff/contract and we adapt.
- Handoff contract: packets and checkpoint live here; your dispositions go to
  `archxy1/`.

## Machine-readable verdict

`ERP_VERDICT.json` (schema `archxy2.erp.verdict/v1`, this folder) is the
machine-readable companion to the reports: the frozen proof tallies, the
18-domain coverage rows, every published packet's sha256 and file list, the
gap list, and `independentReview: "pending"`. It is regenerated from
`docs/rust/required-coverage.json` + the packet files, not hand-typed.

## Baseline snapshot (2026-10-17, task-owned working copy)

- **Main repo:** `/home/haikaru/Archverse/Lab/Archava_Platform` — HEAD
  `21234bb012db018d7632e870575682c37aa38695`, branch `feat/laundry-1.17.0`,
  dirty working tree with substantial untracked Rust source (untouched by us).
- **Archxy 2 working copy:** `/home/haikaru/Archverse/Lab/Archava_Platform_archxy2_erp`
  (fresh rsync of HEAD + dirty + untracked public source; excluded:
  `node_modules`, `target`, `dist`, `build`, `.local`, env files, caches).
- **Verification:** 1240 public files listed (git tracked + untracked
  non-ignored), sha256-before vs after manifest: **1239/1239 identical**.
  One row intentionally divergent: `GEMINI_IMPLEMENTATION_TASK.md` is deleted
  in the upstream dirty tree, so it is absent in both the source listing and
  the copy (recorded, not fabricated). `.env.example` files copied separately
  with equal sha256 (`0af817f9…` for the root file).
- **Toolchain at snapshot:** node v26.10.0, pnpm 10.15.1, rustc/cargo 1.96.0
  (`ac68faa20 2026-05-25`), psql client 18.6. No global PostgreSQL server on
  localhost:5432 — ERP test bootstraps its own initdb fixture per test (same
  as existing `rust/crates/pg-fixture` design).
- **Key frozen-manifest hashes (main tree, unchanged by us):**
  - `docs/rust/migration-inventory.json` sha256
    `ca9021584053772b25a8ec9b0637105f90813b9dfdd996d95391ec214d1bdb8b`
  - `docs/rust/required-coverage.json` sha256
    `6b3daaa7e295dcac2aa8988be32d9dd7599b44b3e2232cfd7ba956d6e29bfce7`
  - `docs/rust/EXECUTION_STATE.md` sha256
    `daa53d48129fe74529cf2e4b7bcbb45b2fee36ff257eea5be1ba08c19c62a3dc`
  - `rust/Cargo.lock` sha256
    `1c0e928819bbc8b867ac65190d11a5883d7418de7e74e385399f45e40edce434`
- **Private data:** none copied (`.env`/`.local`/DB/backups/credentials stay in
  the main tree only; ERP tests use their own synthetic PostgreSQL instance).

## Archxy 2-owned files (readable checkpoints)

- Working copy: `docs/rust/archxy2/EXECUTION_STATE.md` (current checkpoint),
  `docs/rust/archxy2/ERP_SCOPE.md` (frozen behavior→proof map),
  `docs/rust/archxy2/INTEGRATION_REQUESTS.md` (packet registry; PACKETS
  003…009 pending integration, in order).

## Checkpoint 2026-10-17 21:15 WIB — PACKET-000 published

- **PACKET-000.diff is at `archxy2/packets/PACKET-000.diff`** (12 sections:
  domain/adapters/host `lib.rs` registrations, domain + adapters
  `Cargo.toml` deps, `Cargo.lock` packet-owned hunks, host `router.rs`,
  `pg-fixture/src/lib.rs`, `tools/rust/verify.mjs`,
  `tools/rust/test/verify.test.mjs`, `docs/rust/required-coverage.json`).
  ⚠ The fixture and Cargo.lock sections need SEMANTIC integration (the main
  tree moved past the copy's snapshot on both — see the in-file notes and
  INTEGRATION_REQUESTS.md ⚠ note).
- **R06 purchasing half is verified in the copy:** `erp_purchasing` 9/9
  witnesses, producer `tools/rust/erp/verify-purchasing.mjs` green
  (`dist/rust-erp-purchasing-proof.json`), gate unit tests 19/19, fmt/clippy
  clean, and all pre-existing host suites green (authority 18, crm 26,
  kdf_budget 3, readiness 5 + adapters 11).
- **Scope edit in the packet for review:** new manifest proof
  `P-ERP-PURCHASING` + one `P-FAST-GATE` required check. Untouched otherwise.
- **Two owner-side facts ahead of integration:**
  1. `router::tests::the_contact_list_refuses_rather_than_faking_an_empty_result`
     fails in the MAIN tree too (400 vs 403) — §1.4B regression, blocks
     `cargo test --workspace` in both trees until fixed by its owner.
  2. `concurrent_same_key…conflict…` flaked ~2–3/12 under load in the copy
     (hits `waiters == 0`); passes solo and matches the main tree's timing
     when quiet — hardening request, no Archxy 2 patch.
- **Run convention on this box:** `TMPDIR=/home/haikaru/Archverse/tmp-fixture`
  for any PG-backed suite (the shared 7.7 G /tmp tmpfs cannot host a parallel
  fixture fleet); kill leftover `postgres -D …/archava-pg-fixture-*`
  postmasters after interrupted runs.

## Checkpoint 2026-10-18 00:05 WIB — PACKET-001 published (R06 closed in code)

- **PACKET-001.diff is at `archxy2/packets/PACKET-001.diff`** — the R06
  foodservice kernel slice (menus/recipes/tickets as the stock pool's second
  writer): five shared-file sections (host `router.rs` mounts,
  `pg-fixture/src/lib.rs` ×6 sites, `tools/rust/verify.mjs` step + evidence
  mapping, `tools/rust/test/verify.test.mjs` import + drift test,
  `docs/rust/required-coverage.json` fast-gate check + new proof
  `P-ERP-FOODSERVICE`) plus the new-producer note. `rust/Cargo.lock` is
  intentionally NOT in the packet: this cohort adds no new dependencies,
  unlike PACKET-000.
- **Integration basis re-verified 2026-10-17:** main has NEITHER packet
  integrated yet (`erp purchasing suite` absent from its gate,
  `P-ERP-PURCHASING` absent from the manifest, `purchasing_ledger` absent
  from main's fixture). Every PACKET-001 hunk is additive-after PACKET-000's
  corresponding content; the two packets fold independently in either order.
  The fixture section again needs SEMANTIC folding onto main's moved-forward
  file (`ledger_expectations`/`attach`/`dir`), same warning as PACKET-000.
- **R06 foodservice half verified in the copy:** `erp_foodservice` 9/9
  witnesses (~96–108 s) — shared-pool exactly-once (single −2 consume
  movement, receipts bound in BOTH ledgers under the same request id),
  over-reservation refusal, non-tracked ingredient rollback, revoked grant
  on recovery (RN-ERP-01), key/fingerprint conflict (RN-ERP-02), cross-tenant
  refusal, lost-response replay, cancel-untouched, missing grant. Producer
  `tools/rust/erp/verify-foodservice.mjs` green
  (`dist/rust-erp-foodservice-proof.json`); purchasing producer re-run green
  after the gate edits; gate unit tests 20/20; fmt + clippy --workspace
  clean. Full host `--no-fail-fast`: erpFoodservice 9/9, erpPurchasing 9/9,
  authority 18/18, kdf_budget 3/3, readiness 5/5; crm 25/26 (racing-twin
  flake under 7-binary parallel load, solo green in 14 s); lib 40/1 = only
  the owner's §1.4B failure. Domain now 33/33 (6 new foodservice unit tests).
- **Scope edits in the packet for review:** manifest proof
  `P-ERP-FOODSERVICE` (kind evidence, nine requiredChecks,
  `expect.witnessTotal: 9`, namedRegressions RN-ERP-01/02) + one
  `P-FAST-GATE` required check (`erp foodservice suite`). Denominator grows
  by exactly one proof + one fast-gate check.
- **Owner-side facts unchanged from the 2026-10-17 21:15 checkpoint:** the
  §1.4B lib test blocks `cargo test --workspace` in BOTH trees (owner must
  fix). The locking-race flake class now has TWO observed members — the
  authority suite's `concurrent_same_key…` twin and the CRM suite's
  `the_racing_same_key…` twin (crm.rs:1124, both pass solo): one hardening
  request covers both, no Archxy 2 patch.
- **Run convention on this box:** unchanged — `TMPDIR=/home/haikaru/
  Archverse/tmp-fixture` for any PG-backed suite; kill leftover
  `postgres -D …/archava-pg-fixture-*` postmasters after interrupted runs.

## Checkpoint 2026-10-18 00:55 WIB — PACKET-002 published (R06 two-consumers closed)

- **PACKET-002.diff is at `archxy2/packets/PACKET-002.diff`** — the
  R06-TWO-CONSUMERS proof: five shared-file sections (`package.json`
  script, release-gate step + COMMANDS + evidence mapping,
  release-test import + P-MODULE-CONSUMERS drift test, the
  `docs/LOCAL_LIBRARY_HANDOFF.md` reproduce line, and the manifest's
  P-MODULE-CONSUMERS grounding rewrite with `expect.witnessTotal: 4`).
  The consumers (`examples/rust-consumers/erp/{hospitality,
  professional-services}`), the driver (`tools/rust/erp/
  module-consumers.ts`) and the witness map
  (`module-consumers.witnesses.mjs`) are Archxy 2-owned and listed in the
  packet header.
- **Verified in the copy:** `pnpm erp:verify-consumers` PASS end to end
  (pack 62 packages → both verticals typecheck/build/verify against the
  INSTALLED tarballs → implementation hashes equal across the pair →
  resolve() answers only inside node_modules → proof JSON written);
  `node --test tools/rust/test/verify-release.test.mjs` 23/23 (incl. the
  new P-MODULE-CONSUMERS drift test); fast-gate unit tests still 20/20;
  `rust/Cargo.lock` throughout unchanged (5c20faaf764a6e3f…). The two
  batteries exercise BOTH accepted R06 modules per vertical — frozen
  migration ledgers, both command grammars' refusals,
  validation-before-client-query service flows, `-pg`-free browser
  metafiles.
- **Scope edit in the packet for review:** P-MODULE-CONSUMERS grounding
  (producedBy/expect/sourcePaths/referenced) — the two requiredChecks and
  the denominator are UNCHANGED, so this packet grows nothing frozen.
- **Packaging seam decided by evidence (recorded for the reviewer):** the
  cargo-tarball route for `archava-domain` was probed first (version on the
  workspace dependency + `[patch.crates-io]`) and does NOT work offline —
  the packaged verify build searches crates.io for the unpublished
  path-only `archava-core`. Both experiments were REVERTED byte-identically;
  the shipped consumers install from the compiled npm library tarballs
  (`tools/library/pack.mjs` already packs purchasing-pg + foodservice-pg),
  which is why NO rust/Cargo.toml change rides in this packet. If main ever
  wants crate-tarball ERP consumers, that is a separate packet with a
  working proof.
- **R06 is now COMPLETE in the copy:** purchasing kernel 9/9 +
  foodservice kernel 9/9 (the ONE stock pool's two writers) + two
  independently installed consumers green — all three packets published,
  nothing integrated on main yet. Next milestone per ERP_SCOPE §order:
  **R04 (organizations/proposals/projects)**.
- **Owner-side facts unchanged:** the §1.4B lib test blocks
  `cargo test --workspace` on BOTH trees (owner must fix); the
  locking-race flake class has two observed members (the authority suite's
  `concurrent_same_key…` twin and the CRM suite's `the_racing_same_key…`
  twin — both pass solo). Run convention unchanged (`TMPDIR=
  /home/haikaru/Archverse/tmp-fixture`; kill stray fixture postmasters).

## Checkpoint 2026-10-18 ~07:30 WIB — PACKET-003 published (R04 runtime half closed)

- **PACKET-003.diff is at `archxy2/packets/PACKET-003.diff`** — the R04
  ledgers' runtime half (organizations/proposals/projects): five
  shared-file hunks (host `router.rs` ×6 routes, `pg-fixture/src/lib.rs`
  ×7 sites in your NEW ledger_expectations style, `tools/rust/verify.mjs`
  step + evidence mapping, `tools/rust/test/verify.test.mjs` import +
  drift test, `docs/rust/required-coverage.json` ×3 sites). Owned files
  ship wholesale (see the packet header for the full list).
- **Integration basis CHANGED vs the earlier packets:** main has
  integrated 000/001/002 (observed: your `/erp/purchasing/*` +
  `/erp/foodservice/*` mounts, the `erp:verify-consumers` script, the
  P-MODULE-CONSUMERS manifest grounding) AND moved forward with your own
  R03/R09/assistant/capacity/planning machinery. The packet was rebuilt
  against TODAY'S main: the candidate copy's `rust/` and `tools/` were
  overlay-synced from main, my erp `mod.rs` registrations were re-merged
  onto your current versions, and the fixture got its R04 hunks again in
  the new style. The copy also pulled your current
  `docs/LOCAL_LIBRARY_HANDOFF.md` (K05 consumers para — release tests
  need it documented; no content edit of ours) and `package.json` (your
  `rust:browser-slice` script). Dry-run patch verified BOTH DIRECTIONS:
  forward onto main = clean, reverse onto the copy = clean.
- **R04 verified in the copy (post-rebase):** domain 50/50, adapters
  24/24, the three suites 9/9 ×3 = 27/27 against the REBASED fixture
  (erp_organizations 48.8 s, erp_proposals 48.1 s, erp_projects 50.1 s),
  clippy 0 warnings over the four crates `--all-targets`, fmt clean (your
  `providers/src/gemini.rs` is fmt-dirty on main itself — untouched),
  gate tests 25/25 fast + 24/24 release, wasm-boundary PASS, producer
  `verify-r04.mjs` PASS (`dist/rust-erp-r04-proof.json`), and
  `pnpm erp:verify-consumers` PASS with the batteries extended to all
  TEN ERP modules (R04-TWO-CONSUMERS).
- **Scope edits in the packet for review:** `P-ERP-R04` proof block
  (23 requiredChecks — 9 per cohort, the shared key-conflict and
  cross-tenant checks carrying 3 witnesses each — `expect.witnessTotal:
  27`, namedRegressions RN-ERP-01/02, referenced
  `dist/rust-erp-r04-proof.json`), one `P-FAST-GATE` required check
  (`erp R04 suites`), and P-MODULE-CONSUMERS `producedBy` noting the
  battery-scope extension. The frozen P-R04-DOMAIN block is untouched.
- **Cargo.toml/lock untouched** (sha still 5c20faaf764a6e3f…); no new
  dependencies — the R04 cohorts run on the same zlib/serde/axum/sqlx
  surface.
- **Next milestone per ERP_SCOPE §order: R05** (scheduling/accommodation/
  assets) reusing the same transaction SPI; packets remain per-cohort.

## Checkpoint 2026-10-18 ~13:30 WIB — PACKET-004 published (R05 scheduling cohort closed)

- **Main moved again** (HEAD `e7c3854c50c3…`: RN03 recalibrations across
  kits/consumers/providers/manifest tests, a NEW `rust/crates/ops` crate +
  `rust/Cargo.toml` workspace entry, and UNCOMMITTED fixture rehearsal
  additions — `owner_url`, `apply_pending_commerce_migrations`,
  `provision_prior_commerce_profile`, `migrate_commerce_upto`). The copy
  took all 25 changed public files wholesale (benchmark: every
  non-Archxy-2 public path main changed is now content-identical in the
  copy; none collide with Archxy 2-owned paths).
- **PACKET-003 still clean onto today's main:** re-verified by actually
  applying `packets/PACKET-003.diff` with `patch -p1` onto the five
  shared files' current content — all hunks land (offsets to +142, one
  fuzz-1 on the verify.test import hunk), zero rejects. That fold IS the
  a/-side of every PACKET-004 hunk.
- **PACKET-004.diff is at `archxy2/packets/PACKET-004.diff`** — the R05
  scheduling cohort: five shared-file sections
  (`rust/crates/host/src/router.rs` ×2 routes,
  `pg-fixture/src/lib.rs` ×7 sites (SCHEDULING_MIGRATIONS const, ledger
  field, provision call, verbatim grantSchedulingRuntime port,
  Fixture instantiation, migrations_package arm, ledger_expectations arm
  + attach verification), `tools/rust/verify.mjs` step + P-ERP-R05
  mapping, `tools/rust/test/verify.test.mjs` import + drift test,
  `docs/rust/required-coverage.json` ×3 sites). P-ERP-R05 opens
  SCHEDULING-FIRST as the GROWING proof (witnessTotal 9; cohorts 005/006
  extend the same proof + the same gate step additively).
- **Owned surface shipped wholesale:** scheduling domain port (strict
  instant grammar incl. the non-leap-2100 calendar + `.mmmZ`
  normalization; timezone shape check recorded as divergence; seven-op
  command union; receipts/status), scheduling adapter (TS mutate() ladder
  verbatim; 23P01 ⇒ conflict; claim-first replay + receiptAccess
  re-auth; resurrect-free; the block arm's extra resources.write checked
  BEFORE arm refusals), host gate module + routes,
  `erp_scheduling.rs` 9/9, `verify-r05.mjs` producer, consumers to twelve
  modules both verticals (browser mounts + battery sections + leak
  checks), and the working trio.
- **Verification after the rebase (all green):** domain 55/55, adapters
  24/24, `erp_scheduling` 9/9 (79.3 s), `erp_organizations` 9/9 rebase
  spot-check (62.5 s), clippy 0 warnings over the four crates
  `--all-targets`, fmt clean (pg-fixture's ONLY dirty site is main's own
  `migrate_commerce_upto`), gate tests 27/27 fast + 24/24 release,
  wasm-boundary PASS (63 files), producer `verify-r05.mjs` PASS
  (`dist/rust-erp-r05-proof.json`), `pnpm library:pack` → 62 tarballs,
  `pnpm erp:verify-consumers` → PASS.
- **Cargo.lock: byte-identical to main's** (no new crates — scheduling
  lives inside the existing four Archxy 2 crates); `rust/Cargo.toml` also
  inherited from main this round (its R10 ops-crate entry) and is
  otherwise single-writer-yours.
- **Owner-side facts unchanged:** §1.4B lib test still blocks
  `cargo test --workspace` on both trees; the locking-race flake class
  still has its two observed members (both pass solo). Run convention
  unchanged (`TMPDIR=/home/haikaru/Archverse/tmp-fixture`; kill stray
  `postgres -D …` postmasters).
- **Next: PACKET-005 (accommodation)** — depends on 004's migration 002
  (occupancy kernel + `create_accommodation_resource` RPC + the
  occupancy grant), brings `calendar.ts`'s `resolveCalendarInstant` and
  the occupancy runtime port, and EXTENDS P-ERP-R05 + P-FAST-GATE's one
  R05 step additively. Then PACKET-006 (assets).

## Checkpoint 2026-10-18 (late) WIB — PACKET-005 published (R05 accommodation cohort closed)

- **Main moved again, twice:** HEAD `2b12e2f…` at rebase, then
  `d13839b…` (the frozen manifest's benchmark-path realign) landed WHILE
  this packet was being cut — two lines below every PACKET-005 hunk
  context; the five-file fold re-verified BYTE-IDENTICAL onto it. R11
  closed on main (`D-COMPLETION-REPORT`/`D-AUDIT-PACKET`), COMMITTING what
  the 004 checkpoint had recorded as uncommitted (fixture rehearsal
  additions, the `ops` crate set, the assistant seed anchoring, the
  release-test updates). The copy re-took main's changed public files
  wholesale (ops tree, assistant pair, verify-release pair, docs, four
  tools/rust scripts; Cargo.toml/lock ALREADY identical). Main's NEW
  formal-education WIP package stays owner-side — not copied (no Archxy 2
  path needs it, and in-flight WIP under `packages/` would perturb the
  pack).
- **PACKET-005.diff is at `archxy2/packets/PACKET-005.diff`** — the R05
  accommodation cohort: five shared-file sections (`router.rs` ×2 routes,
  `pg-fixture/src/lib.rs` ×8 sites (ACCOMMODATION_MIGRATIONS const + doc,
  ledger field, provision call AFTER scheduling's, the
  grantAccommodationRuntime port PLUS the ONE uncovered grantOccupancy
  piece — the `create_accommodation_resource` EXECUTE grant TS
  grantSchedulingRuntime does NOT carry —, migrations_package arm,
  ledger_expectations arm + attach), `tools/rust/verify.mjs` (comment +
  ten more P-ERP-R05 mapping rows), `tools/rust/test/verify.test.mjs`
  (drift whitelist gains `accommodation`), `docs/rust/required-coverage.json`
  (P-ERP-R05 +10 checks, witnessTotal 9→19, both producedBy grow,
  sourcePaths + accommodation migrations)). Apply 003, 004, then this.
- **The ONE-growing-proof contract honored:** P-ERP-R05 is now 9+10 = 19
  checks, graded by the SAME `verify-r05.mjs` producer and the SAME
  fast-gate step (`erp R05 suites`; P-FAST-GATE's requiredChecks did NOT
  grow — a growing proof rides one step). The assets cohort (PACKET-006)
  extends it again, no fork.
- **Owned surface shipped wholesale:** the accommodation domain port
  (civil-calendar half of `calendar.ts`, zod-frozen vocab/grammar with the
  TS default tree, receipts WITHOUT the `replayed` member, commandStatus
  `{status,requestId,receipt?}` — the document answered VERBATIM by GET;
  the `{decision,…}` wrapper of PACKET-004 stays its own divergence), the
  adapter port (repository.ts ladder verbatim; tzdata probing for the
  resolved instants — gap or repeat ⇒ conflict BEFORE the today gate; the
  hold→confirm→check-in→check-out chain mirrored in 001's migration
  semantics; ONE audit row; `assert_accommodation_kernel` = 8 forced-RLS
  tables), the host gate module + ledger descriptor + two routes, the
  10-witness `erp_accommodation.rs` suite, the grown producer + battery
  scope (FOURTEEN modules — dep entries, dashboard mounts, index hosts,
  battery sections, `accommodation-pg` leak joins), and the trio.
- **Verification (all green, recorded as they ran):** domain 66/66,
  adapters 24/24, host `--tests --no-fail-fast` ALL GREEN (lib 41/41 —
  the §1.4B failure did NOT reproduce on main's newer tree, assistant
  13/13, authority 18/18, crm 27/27 — no flake member fired either,
  erp_accommodation 10/10 at 86.1 s, the six earlier ERP suites 9/9 ×6,
  kdf 3/3, readiness 5/5), clippy 0 warnings `--all-targets` over the
  four crates, fmt clean, `node --test tools/rust/test/verify.test.mjs`
  27/27 + release tests 24/24, wasm boundary PASS (64 files),
  `node tools/rust/erp/verify-r05.mjs` PASS (19/19 →
  `dist/rust-erp-r05-proof.json`), `pnpm library:pack` 62 tarballs (the
  accommodation pair included), `pnpm erp:verify-consumers` PASS.
  `rust/Cargo.lock` unchanged — no new crates.
- **Owner-side facts, updated honestly:** the §1.4B lib test and the two
  racing-twin flake members did NOT reproduce in this round's full host
  run on main's newer tree (recorded as OBSERVED, not as a fix claim —
  the earlier notes stand if they reappear on an older checkout). Run
  convention unchanged (`TMPDIR=/home/haikaru/Archverse/tmp-fixture`;
  kill stray `postgres -D …` postmasters).
- **Next: PACKET-006 (assets)** — the R05 area's LAST cohort: rides the
  same fixture pattern (now proven twice), extends P-ERP-R05 + the ONE
  step a final time; then R07 per ERP_SCOPE §order. PACKETS 000–004
  remain unintegrated on main; 005 assumes BOTH folded first.

## Checkpoint 2026-10-18 (latest) WIB — PACKET-006 published (R05 CLOSED: all three cohorts in one proof)

- **Copy state:** all work uncommitted on top of the copy's base
  (rsync of main `c96abf0…`-era public source at the last rebase;
  main itself MOVED during this round: `d13839b…` → `b141a24…`
  (RN coverage closure) → `c96abf0…` (the module-consumers tally
  revert — the drift pin resurrecting the WITNESS-PAIR counting
  convention the packets already followed)). The copy took the
  c96abf0 owner files wholesale (verify-foodservice/verify-purchasing
  namedRegressions, the parameterized `runAuthority` in pg-authority,
  tools/own verify, DECISIONS) and reverted the owner
  formal-education WIP that had ridden along in four shared files —
  tools/library/pack.mjs, tools/library/manifest.mjs, package.json
  and docs/LOCAL_LIBRARY_HANDOFF.md are byte-identical to main HEAD;
  THIS PACKET SHIPS ZERO HUNKS IN THEM (assets-pg already rides the
  committed library catalogue; `@archava/assets` ships transitively).
- **What closed:** R05's assets cohort —
  `rust/crates/domain/src/erp/assets.rs` (+5 unit tests → domain 66/66),
  `rust/crates/adapters/src/erp/assets.rs` (TS repository ladders
  verbatim; REAL 400-class verdicts riding `ErpError::Unavailable` →
  the gate's 400 `invalid_submission`; `assert_assets_kernel` 6
  forced-RLS tables), `host/src/erp/assets.rs` (+ descriptor + 2
  routes: `/erp/assets/commands{,/status}`), the fixture wiring
  (assets-pg's ONE frozen migration, recomputed checksums, the
  `grantAssetsRuntime` port incl. the `lock_membership` EXECUTE,
  both rehearsals unaffected), `tools/rust/erp/verify-r05.mjs`
  (the SAME producer grown a final time: 29 checks = 9 scheduling +
  10 accommodation + 10 assets), module-consumers sixteenth-module
  battery scope + the owner-correct computed tally, and both verticals'
  assets batteries (grammar family, five-permission ledger, receipt
  pins incl. no-replay-member, service flow, `assets-pg` leak check).
- **Verification (recorded as it ran):** host `--tests --no-fail-fast`
  ALL GREEN — lib 41/41, assistant 13/13, authority 18/18, crm 27/27,
  ALL EIGHT ERP suites (erp_assets NEW 10/10 ~85 s; scheduling
  9/9 ×5 + erp_accommodation 10/10 unchanged), kdf 3/3, readiness
  5/5; clippy 0 warnings `--all-targets` over the four crates; fmt
  clean; `node --test tools/rust/test/verify.test.mjs` 27/27 +
  release tests 24/24; wasm boundary PASS; `pnpm library:pack` 62
  tarballs; `pnpm erp:verify-consumers` PASS;
  `rust/Cargo.lock` unchanged — no new crates.
- **Fold verification (the strongest claim yet):** main@c96abf0 +
  PACKETS 003+004+005+006 applied sequentially
  (`patch -p1 --forward --fuzz=3`) → ALL FIVE shared files
  byte-identical to this copy (`cmp` green ×5): router.rs,
  pg-fixture/src/lib.rs, tools/rust/verify.mjs,
  tools/rust/test/verify.test.mjs, docs/rust/required-coverage.json.
  The only fuzz-1 remains the verify.test import hunk from the
  003/004 era; PACKET-006's own hunks are zero-offset.
  Reverse dry-run onto the copy: clean.
- **Owner-side facts:** the tally revert validates this packet's
  consumers-manifest stance; the uncommitted worktree pack-list stub
  (formal-education pairs) was used TEMPORARILY to produce the 62
  tarballs locally and does NOT ship; main's committed (HEAD) state
  is internally inconsistent about that stub (uncommitted worktree
  carries the fix) — recorded, no action.
- **State of P-ERP-R05:** producer + manifest + drift test grown to
  the CLOSED shape (29 checks; `packages/assets-pg/migrations` in
  sourcePaths; all three suite files named in `producedBy`).
  P-MODULE-CONSUMERS' manifest `witnessTotal` UNCHANGED at 4
  (witness pairs), now with the producedBy sentence recording the
  sixteen-module battery scope + the computed members.
- **Next: PACKET-007 (R07)** per ERP_SCOPE §order — billing/
  subscriptions/attendance/support/fulfillment; R05 is CLOSED.

## Checkpoint 2026-10-19 (early) WIB — PACKET-007 published (first R07 cohort: billing — P-ERP-R07 OPENS)

- **Copy state:** all work uncommitted on top of the copy's base (HEAD
  `21234bb…`, dirty overlay as of 2026-10-19). Main moved TWICE past
  `c96abf0…` — `94307ed…` (reap orphan postmasters at gate start) and
  `409eab9…` (the terminal 17/17-full-gate record; three docs files +
  the release pair only) — and NEITHER commit touches a packet surface,
  so the shared files the packets claim re-verify unchanged.
- **What opened:** the R07 billing cohort is IMPLEMENTED and VERIFIED —
  the pure-Stripe-like frozen TS billing pair
  (`packages/billing` + `packages/billing-pg`), byte-faithful: the
  strict mutation grammar, the four-permission vocabulary
  (`billing.read/write/settle` + `audit.read`), the priced-invoice
  immutability boundary, the provider-event HMAC digest
  (`archava/billing/provider-event/v1`, seven length-framed parts,
  computed in-domain over `hmac`+`sha2` — D17), the signature-verify
  BEFORE any SQL, the claim pairing that REPLAYS a stored receipt
  (`idempotencyReplay: true`) and falls THROUGH on a receipt-less row,
  replay gated on the fingerprint cell alone, ONE audit row, ONE clock
  sample bound to every stamp (D18), and the label-loss folds
  (`invalid_signature`/`invalid_transition`/`unbalanced_settlement`
  ride their HTTP classes; database failures stay 500-class on both
  stacks, never folded to 409 — D16). The ledger is the FIRST with a
  replay member AND the first to state the transition/DB boundary
  roster.
- **Suite:** `erp_billing.rs` 13/13 named witnesses (~320 s; ONE
  PostgreSQL fixture window per run; the 13th check's kernel assert
  runs in the served() harness BEFORE the router mounts — eight
  forced-RLS tables). Recorded regressions: RN-ERP-01 "revoked billing
  grant refused on recovery", RN-ERP-02 "same billing key different
  command conflicts".
- **R07 gets its OWN proof surface** (as decided): `P-ERP-R05` stays
  CLOSED at 29 and `P-ERP-R07` starts growing per cohort —
  `tools/rust/erp/verify-r07.mjs` (13 checks, `--locked --offline`,
  `dist/rust-erp-r07-proof.json`), the `erp R07 suites` fast-gate step
  (P-FAST-GATE requiredChecks 20 → 21), the manifest `P-ERP-R07` block,
  and the R07 drift-test member (node-test family 28/28). The pure domain
  half stays the frozen `P-R07-DOMAIN` slot (Archxy 1's).
- **Consumers 16 → 18:** both verticals install `@archava/billing` +
  `@archava/billing-pg` from tarballs; typed exports
  (`createBillingService(…, onBehalfOf)` — the principal REQUIRES it;
  page query's `offset` is REQUIRED); a billing battery each; the
  digest roundtrip + tampered-refusal flow; `billing-pg` leak checks;
  `pnpm library:pack` → 62 tarballs with ZERO pack edits (billing
  arrives via the closure); `pnpm erp:verify-consumers` PASS (run with
  main's worktree pack.mjs TEMPORARILY synced — the committed-HEAD
  pack.mjs vs committed-catalogue inconsistency is UNCHANGED from
  PACKET-006's note; nothing shipped).
- **Full sweep green:** fmt clean · clippy workspace 0 warnings ·
  cargo test --workspace 52 blocks/0 failures (domain 74/74, adapters
  24/24 unchanged, lib 41/41, all NINE earlier suites unchanged) ·
  node tests 91/91 (verify.test 28/28 with the R07 member) · wasm
  boundary PASS · PG producers all PASS (incl. verify-r07).
- **Packet cut:** `PACKET-007.diff` — staged on a fresh clone of
  main@`409eab9` + PACKET-003/004/005/006 applied (zero rejects; the
  only fuzz-1/fuzz-2 members stay the PACKET-003-era hunks), then cut
  by overlaying the copy's SEVEN shared files and diffing:
  router.rs (2 routes), pg-fixture/lib.rs (BILLING_MIGRATIONS +
  ledger field + provision + grantBillingRuntime + both arms +
  instantiation), verify.mjs (step + P-ERP-R07 mapping),
  verify.test.mjs (R07_WITNESSES import + drift test),
  required-coverage.json (P-FAST-GATE 21 + P-ERP-R07 block + producedBy
  18-module note), domain/Cargo.toml (hmac+sha2), Cargo.lock (the
  archava-domain dep pair). ROUND-TRIP VERIFIED: the fold + packet
  reproduces all SEVEN files `cmp`-byte-identical to this copy. NO
  hunks in the four library single-writer files.
- **KNOWN_DIFFERENCES:** D16, D17, D18 appended (first ledger entries
  for the sampling-divergence family the R04/R05 adapters carry).
- **Next:** the remaining R07 cohorts (subscriptions, attendance,
  support, fulfillment) per ERP_SCOPE §order, growing `verify-r07`
  additively; then R08.

## Checkpoint 2026-10-20 WIB — PACKET-008 published (R07 subscriptions cohort; P-ERP-R07 at 26)

- **Cohort:** subscriptions/entitlements/usage ported from
  `packages/subscriptions*` and `packages/subscriptions-pg/*` — five
  command arms, four read projections (the ERP namespace's FIRST Rust
  read surface), six host routes, the seventeenth fixture ledger
  (`archava_subscriptions`, one migration whose sha256 is recomputed
  against the frozen TS bytes).
- **Audit-A05 correction, implemented in pure domain policy:**
  `consumption_refusal` refuses an entitlement whose owning subscription
  is a never-paid `draft` (402 `not_paid`, nothing recorded);
  `activation_refusal` is an explicit transition table — `canceled`/
  `expired` terminal, `paused → active` only with a payment record, and
  `record_payment` is the ONLY activation path for a never-paid draft.
  D19/D20/D21 in `docs/rust/KNOWN_DIFFERENCES.md`: the duplicate usage
  idempotency key is a 409 conflict (not the baseline's 500-class
  unique-violation), and consumption/activation refusals record nothing.
- **Verification:** `erp_subscriptions` 13/13 (309.7 s in the
  producer's final default-width run; 326.13 s standalone),
  `erp_billing` unchanged 13/13 (316.9 s),
  `node tools/rust/erp/verify-r07.mjs` → PASS 26/26 checks,
  `dist/rust-erp-r07-proof.json` (`witnessTotal: 26`, `allPassed: true`);
  domain 80/80; node drift 28/28. The suite runs at libtest's DEFAULT
  width: its login slots use the production password-derivation budget
  (`max_active_derivations()`, two) — the first draft hardcoded four and
  answered 503 `busy` under a full-width run; a suite never raises the
  production budget from inside a test.
- **Packet cut:** `PACKET-008.diff` (sha256
  `d6ddefb2caff508a8b27fe72be57d7be96fe041b727c76e3aa9c5275f3ed9e0f`),
  staged on a fresh clone of main@`e1e83c6` + PACKET-003…007 (all five
  fold with zero rejects) and cut by overlaying FIVE shared files:
  `router.rs` (six routes), `pg-fixture/lib.rs` (SUBSCRIPTIONS_MIGRATIONS
  + ledger field + provision after billing + grants + both arms +
  instantiation), `verify.mjs` (13 new P-ERP-R07 checks on the EXISTING
  step + comment), `verify.test.mjs` (suite list + frozen tally 26),
  `required-coverage.json` (13 checks + `witnessTotal` 26 + `RN-ERP-03`
  + producedBy). Packet applies with `--fuzz=0`, zero rejects.
  Round-trip: the three A2-only files reproduce BYTE-IDENTICAL (`cmp`);
  `router.rs` and `required-coverage.json` equal the copy PLUS main's
  newer C01/C02/A1-00 lines (the copy's base `21234bb` predates them)
  — those files must NOT be overwritten from the candidate copy.
  NO hunks in the four library single-writer files and NO Cargo change
  (`@archava/subscriptions-pg` rides the committed catalogue/closure).
- **Next:** attendance (the A04 roster-integrity correction), then
  support and fulfillment, then R08 per ERP_SCOPE §order.


## Checkpoint 2026-10-20 WIB — PACKET-009 published (R07 attendance cohort; P-ERP-R07 at 40)

- **Cohort:** attendance/sessions/rosters/check-ins ported from
  `packages/attendance*` and `packages/attendance-pg/*` — five command
  arms (`create_session`, `register_participant`, `record_check_in`,
  `update_roster_status`, `change_session_status`), six read
  projections (access, session list, detail + roster + check-ins,
  roster, check-in ledger, command status), seven host routes, the
  eighteenth fixture ledger (`archava_attendance`, one migration
  `001_attendance_sessions_rosters`).
- **Audit-A04 correction, implemented twice (SQL + pure policy):** the
  check-in's roster row SELECT carries the supplied session
  (`session_id=$3`), and `check_in_refusal` tests the roster↔session
  binding BEFORE any state check. A roster UUID from session B under
  session A answers 404, records no event/audit/command, moves no
  counter, and never appears in the other session's projections
  (RN-ERP-04, D22). The seat boundary and the check-in race serialize
  on the session/roster row locks (the losing registration is
  `capacity_exceeded`; the losing scanner is `already_checked_in`, one
  event total). D23–D25: duplicate roster/ticket/session-code are 409
  conflicts; `scope=assigned` narrows every read; a session window must
  strictly advance.
- **Verification:** `erp_attendance` **14/14** named witnesses in
  **493.76 s** standalone at libtest's DEFAULT width (login slots are
  `max_active_derivations()`, the production budget); domain **89/89**;
  adapters lib 13/13; host lib 41/41;
  `cargo clippy --workspace --all-targets -- -D warnings` **0
  warnings**; `cargo fmt --all -- --check` clean;
  `node tools/rust/erp/verify-r07.mjs` → **PASS 40/40** (billing
  13/13 384.5 s, subscriptions 13/13 369.3 s, attendance 14/14 493.6 s;
  `dist/rust-erp-r07-proof.json` `witnessTotal: 40`, `allPassed: true`);
  `cargo test --workspace` → **PASS** (clean tree: exit 0, 54
  blocks, 0 failed, 732 tests passed); node drift 91/91.
- **Packet cut:** `PACKET-009.diff` (sha256
  `e0eabf8f9925a616f096657f36cb72ac4e51de59e122613b89982a5e876ad937`),
  staged on a fresh `--no-local` clone of main@`6c06998` + PACKET-003…008
  (003–007 fold with `--fuzz=3`, 008 with `--fuzz=0`, all zero rejects),
  committed as the fold baseline and cut by applying FIVE shared-file
  hunks: `router.rs` (seven routes), `pg-fixture/lib.rs`
  (`ATTENDANCE_MIGRATIONS` + ledger field + provision after subscriptions
  + grants + both arms + both constructors), `verify.mjs` (14 new
  P-ERP-R07 check mappings + comment), `verify.test.mjs` (suite list +
  frozen tally 40), `required-coverage.json` (14 checks + `witnessTotal`
  40 + `RN-ERP-04` + producedBy + attendance migration sourcePath).
  Packet applies with `--fuzz=0`, zero rejects. Round-trip:
  `pg-fixture/lib.rs`, `verify.mjs` and `verify.test.mjs` reproduce
  BYTE-IDENTICAL (`cmp`); `router.rs` and `required-coverage.json` equal
  the copy PLUS main's newer C01/C02 lines (zero copy-only lines outside
  the CRM manifest block main added) — those two must NOT be overwritten
  from the candidate copy. NO hunks in the four library single-writer
  files, NO Cargo change, NO `packages/` change.
- **Next:** support (5 commands, 5 read areas, 1 tool), then
  fulfillment, then R08 per ERP_SCOPE §order.

## Checkpoint 2026-10-20 WIB — PACKET-010 published (R07 support cohort; P-ERP-R07 at 52)

- **Main moved:** `0179c2a` (`ERP PACKET-003: fold the R04 ledgers
  (organizations, proposals, projects)`) — PACKET-003 is now folded;
  004–010 remain pending. The copy is still HEAD `21234bb` + the full
  uncommitted Archxy 2 overlay.
- **Support cohort implemented whole:** `domain|adapters|host/src/erp/
  support.rs`, `host/tests/erp_support.rs`, the three `mod.rs` lines, six
  host routes and four read projections (access, bounded tenant-wide
  assignees, bounded worklist, detail + notes/transitions/audit). Twelve
  command arms; the DEFERRED `case_complete` trigger requires audit count
  = revision and the last transition/receipt to agree. Duplicate request
  key replays, changed payload/stale revision/illegal state are 409 and
  record nothing; assignment needs `cases.assign` + `scope=all` + an
  active human with `cases.read`; notes/audit have their own grants;
  `scope=assigned` narrows worklist/detail. Commit-time SQLSTATE
  23514/23503/23505/22003/40P01/55P03 maps to 409 (`D26`).
- **Verification:** `erp_support` **12/12** named witnesses (the harness
  keeps four fixtures alive at once — eight sessions × twelve disposable
  PostgreSQLs otherwise exhausts the host's pool budget); domain lib
  **97/97** (89 + 8 support); adapters lib 13/13; host lib 41/41;
  `cargo clippy --workspace --all-targets -- -D warnings` **0
  warnings**; `cargo fmt --all -- --check` clean; node drift 28/28
  (including the R07 map at 52); `node tools/rust/erp/verify-r07.mjs`
  **PASS 52/52** (billing 13, subscriptions 13, attendance 14, support
  12; `dist/rust-erp-r07-proof.json` `witnessTotal: 52`,
  `allPassed: true`, sha256
  `2987e83ff8b31fd8437f22a326664c264944b7cee00272a59c26d9b01c0e63c1`);
  the final clean-tree `cargo test --workspace` is 55 blocks, 0 failed,
  752 passed.; `cargo test --workspace` **PASS** on a clean tree.
- **Packet cut:** `PACKET-010.diff` (sha256
  `4964e4c8e414c5b868a3ad099fc2c215bc71ebc2ac63751b436879dd71802758`),
  staged on a fresh `--no-local` clone of main@`0179c2a` + PACKET-004…009
  (004 needs `--fuzz=3` for one producedBy paragraph; the rest zero
  rejects), committed as the fold baseline and cut by applying FIVE
  shared-file hunks: `router.rs` (six routes), `pg-fixture/lib.rs`
  (`SUPPORT_MIGRATIONS` + ledger field + provision after attendance +
  grants + both arms + both constructors), `verify.mjs` (12 new
  P-ERP-R07 check mappings + comment), `verify.test.mjs` (suite list +
  frozen tally 52), `required-coverage.json` (12 checks + `witnessTotal`
  52 + producedBy + support migration sourcePath). Applies with
  `--fuzz=0`, zero rejects. NO hunks in the four library single-writer
  files, NO Cargo change, NO `packages/` change.
- **Verdict:** 12 of 18 domains locally verified (126 named witnesses),
  1 partial kernel, 5 not started; 3 of 18 have Rust read projections;
  `independentReview: pending`. `ERP_VERDICT.json` in this folder was
  regenerated with the current main head, the folded/pending sets, the
  P-ERP-R07 tally and PACKET-010's hash.
- **Next:** fulfillment (R07), then laundry, field-services,
  formal-education, capacity and commerce per ERP_SCOPE §order. Fold
  004–010 on main and re-run P-ERP-R07 there before any completion claim.
