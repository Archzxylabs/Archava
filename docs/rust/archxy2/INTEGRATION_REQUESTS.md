# Archxy 2 integration requests — small reviewable packets

Format per packet: ID, base hashes, owned paths, public interfaces, needed
shared edits/dependency versions, migration/transaction ownership, tests and
commands with exits, proof paths, compatibility effects, exact next
integration action. Successor packets reference prior IDs; no packet is
overwritten.

## PACKET-000 — ERP namespace foundation (purchasing stock cohort)

- **Base hashes:** see handoff `BASELINE.md` (working copy HEAD 21234bb…, dirty
  overlay as of 2026-10-17; copy created 19:27 WIB). Packet diff published at
  handoff `archxy2/packets/PACKET-000.diff`.
- **Owned paths:** `rust/crates/domain/src/erp/**`, `rust/crates/adapters/src/erp/**`,
  `rust/crates/host/src/erp/**`, `rust/crates/host/tests/erp_purchasing.rs`,
  `tools/rust/erp/**`, `docs/rust/archxy2/**` (handoff-only writes to
  `Archava_ERP_Handoff/archxy2/`).
- **Public interfaces requested (new, additive):**
  - `archava_domain::erp` — the pure ERP contracts (purchasing order/offer/
    supplier values, `PurchaseError` domain mapping, stock pool invariants;
    wasm-clean, `serde` only).
  - `archava_adapters::erp` — transactional ports: `erp::purchasing` (quote →
    claim → execute → recovery against `archava_purchasing` +
    `archava_commerce` in ONE tenant transaction) and `erp::PurchaseError`.
  - `archava_host::erp` — `PURCHASING_LEDGER`, `ErpGateError`,
    `erp::begin()`; routes `POST /erp/purchasing/commands` (CSRF true) and
    `GET /erp/purchasing/commands/status` (CSRF false), mirroring the CRM
    gate's decision/receipt documents.
- **Needed shared edits (single-writer: Archxy 1) — ACTUAL diffs in PACKET-000.diff:**
  1. `rust/crates/domain/src/lib.rs`: `pub mod erp;` registration.
  2. `rust/crates/adapters/src/lib.rs`: `pub mod erp;` registration.
  3. `rust/crates/domain/Cargo.toml`: workspace dep `serde_json` — the ERP
     fingerprint canonicalization traverses JSON values (in-file comment
     records the packet).
  4. `rust/crates/adapters/Cargo.toml`: workspace deps `archava-domain`,
     `serde`, `serde_json`, `uuid` (v4 + serde) — the receipts and command
     payloads ride JSONB columns; `Uuid::new_v4` mints the ids the INSERTs
     carry (TS side uses `randomUUID()`). `Cargo.lock` hunks attached for
     exactly those two packages — the third lock hunk on the main tree
     (js-sys/leptos restructuring) is Archxy 1's post-snapshot work and is
     excluded from the packet.
  5. `rust/crates/host/src/lib.rs`: `pub mod erp;` registration.
  6. `rust/crates/host/src/router.rs`: `try_new_erp` (readiness via
     `assert_startup_readiness` with `crate::erp::PURCHASING_LEDGER`, primary
     ledger = commerce) + two route mounts, unconditionally, immediately after
     the CRM mounts.
  7. `rust/crates/pg-fixture/src/lib.rs`: `PURCHASING_MIGRATIONS` const,
     `Fixture::purchasing_ledger` field (ledger summary for startup
     readiness), `migrate_ledger` threading the frozen list explicitly as a
     parameter, and the purchasing runtime-grant block (statement-for-statement
     port of `grantPurchasingRuntime` from
     `packages/purchasing-pg/src/migrate.ts`: history/memberships read-only,
     supplier/offer/order/line draft writing, receipts/history append-only,
     `lock_membership` executable).
  8. `tools/rust/verify.mjs`: gate step `erp purchasing suite`
     (`node tools/rust/erp/verify-purchasing.mjs`) after `pg authority suite`,
     plus the `P-ERP-PURCHASING` evidence-mapping entry (details read
     `dist/rust-erp-purchasing-proof.json`).
  9. `tools/rust/test/verify.test.mjs`: import
     `WITNESSES as PURCHASING_WITNESSES` from `../erp/verify-purchasing.mjs`
     and the new drift test `the P-ERP-PURCHASING witness map and the frozen
     manifest cannot drift apart` (three-lists-equal + `witnessTotal`, mirroring
     the P-READINESS-PG/P-AUTHORITY-PG tests).
  10. `docs/rust/required-coverage.json` — **explicit scope edit, submitted for
     review (editPolicy: "adding/removing an item or proof changes the
     completion denominator")**:
     - `P-FAST-GATE.requiredChecks` gains `"erp purchasing suite"` (the gate
       drift test requires steps ≡ requiredChecks, so the step registration
       and this line are one change).
     - NEW proof `P-ERP-PURCHASING` (kind evidence): nine requiredChecks
       grading the purchasing cohort's named witnesses, `expect.witnessTotal:
       9`, `namedRegressions` RN-ERP-01/02, sourcePaths binding
       `tools/rust/erp/verify-purchasing.mjs` + the host/adapters/domain/core
       crates + both packages' migrations, referenced
       `dist/rust-erp-purchasing-proof.json`.
     - **Correction vs the earlier plan:** the proof is NOT bound to
       `P-R06-DOMAIN` (that slot stays untouched for the stock cohort's
       remaining half — commerce orders/foodservice). The purchasing host
       suite gets its own proof, so R06's remainder stays honestly open and
       the purchasing slice is graded now. Denominator grows by exactly one
       proof + one fast-gate check.
- **⚠ SEMANTIC-INTEGRATION NOTE (fixture file):** the main tree's
  `rust/crates/pg-fixture/src/lib.rs` gained `ledger_expectations()`,
  `attach()` and `Fixture::dir()` AFTER this copy's snapshot (main-tree mtime
  2026-10-17 14:27 vs copy creation 19:27; `rust/` untracked on both sides, so
  git cannot arbitrate provenance). PACKET-000.diff's fixture hunk is computed
  against the copy's snapshot state — **integrate its semantics (the
  purchasing additions) onto the CURRENT main file; do not apply the hunk
  literally.** In the current main design, the purchasing migration list
  belongs in `ledger_expectations()`'s mapping, the grants go in the same
  ratchet block, and `Fixture` gains the `purchasing_ledger` field. The
  4-parameter `migrate_ledger` in the copy is the snapshot-era equivalent of
  `ledger_expectations()` — prefer the main tree's design.
- **Migration/transaction ownership:** purchasing SQL migrations reused
  verbatim from `packages/purchasing-pg/migrations/` (no historical rewrite).
  ERP domain functions run inside the shared `TenantTransaction` cohort with
  `lock_membership` FOR SHARE; `SET LOCAL statement_timeout='10s'` /
  `lock_timeout='3s'`; no shadow authorization.
- **Tests/commands/exits (recorded as they ran, 2026-10-17):**
  | Command | Result |
  | --- | --- |
  | `cargo test -p archava-host --test erp_purchasing` | **9/9 pass** (~53–57 s) — repeated across disk states |
  | `node tools/rust/erp/verify-purchasing.mjs` | PASS — 9 passed, discovered 9, exit 0 (52.9 s), proof written |
  | `node --test tools/rust/test/verify.test.mjs` | 19/19 pass (incl. the new drift tests) |
  | `cargo fmt --all --check` | clean |
  | `cargo clippy --all-targets --locked --offline` | clean after 9 lint fixes (3× manual_range_contains, 3× op_ref, 1× needless borrow, 2× too_many_arguments via documented allow) |
  | `pnpm rust:verify` (TMPDIR workaround) | 4/12 steps green; **fails at `cargo test --workspace` on the pre-existing §1.4B lib failure** `the_contact_list_refuses_rather_than_faking_an_empty_result` (400 vs 403) — reproduced in the MAIN tree too; not Archxy 2's. Downstream producers verified individually: `pg-readiness.mjs` PASS, `pg-authority.mjs` PASS (one flaky run documented below), erp purchasing PASS |
  | `cargo test -p archava-host --test authority` | 18/18 (matches main tree: 104 s vs 104.76 s) |
  | `cargo test -p archava-host --test crm` | 26/26 |
  | Full host `--no-fail-fast` | authority 18/18, crm 26/26, erp_purchasing 9/9, kdf_budget 3/3, readiness 5/5; lib 40/1 (the pre-existing failure only) |
  | Known flake (Archxy 1's test, not ERP) | `concurrent_same_key_different_input_yields_conflict_not_the_other_receipt`: ~2–3 fails in ~12 runs under parallel-suite load, always `waiters == 0` after the holder's 30 s poll; passes solo and on quiet runs; filed in EXECUTION_STATE as an integration request (harden the holder), no Archxy 2 patch |
- **Proof paths:** `dist/rust-erp-purchasing-proof.json` (producer
  `tools/rust/erp/verify-purchasing.mjs`, manifest binding `P-ERP-PURCHASING`).
- **Compatibility effects:** additive only; TS baseline packages untouched;
  no route/response contract of existing surfaces changed (CRM/wasm paths
  byte-stable; the only router diff is the erp mount).
- **Exact next integration action:** Archxy 1 reviews PACKET-000.diff +
  `INTEGRATION_REQUESTS.md`, integrates the registrations + the SEMANTIC
  fixture edits + the gate/manifest scope edit, records landed hashes in
  `archxy1/` handoff, then runs `pnpm rust:verify` on the (main) tree — which
  additionally requires fixing the pre-existing §1.4B lib test first.

## PACKET-001 — R06 foodservice kernel slice (second writer on the ONE stock pool)

- **Base hashes:** same working copy as PACKET-000 (HEAD 21234bb… + dirty
  overlay; nothing committed anywhere). Packet diff published at handoff
  `archxy2/packets/PACKET-001.diff`. Additive to PACKET-000: the main tree has
  NOT integrated PACKET-000 yet (re-verified 2026-10-17: no `erp purchasing
  suite` step, no `P-ERP-PURCHASING` proof, no `purchasing_ledger` in main's
  fixture), so every PACKET-001 hunk lands right after PACKET-000's
  corresponding content and the two packets fold independently in either
  order. `rust/Cargo.lock` is UNCHANGED by this cohort — no new dependencies.
- **Owned paths:** `rust/crates/domain/src/erp/**` (incl. `foodservice.rs`),
  `rust/crates/adapters/src/erp/**` (incl. `foodservice.rs` and the
  order-side additions to `commerce_kernel.rs`),
  `rust/crates/host/src/erp/**` (incl. `foodservice.rs`),
  `rust/crates/host/tests/erp_foodservice.rs`, `tools/rust/erp/**` (incl.
  `verify-foodservice.mjs`), `docs/rust/archxy2/**`; handoff-only writes to
  `Archava_ERP_Handoff/archxy2/`.
- **Public interfaces requested (new, additive):**
  - `archava_domain::erp::foodservice` — `FoodPrincipal` (alias of the
    purchasing principal: same grammar), the 13-arm `FoodCommand` serde
    grammar (`table.create/set-active`, `menu.create/set-price/set-active/
    replace-recipe`, `ticket.create/queue/prepare/cancel/ready/serve/close`),
    `FOOD_PERMISSIONS` split (tables.read / menus.write / commerce
    baseline), `FoodReceipt`/`FoodCommandStatus`, strict
    `parse_command`/`FoodMutation::parse` (dine-in ⇄ table cross-rule),
    `replayed_receipt`, `authorize_operation`. wasm-clean (serde only).
  - `archava_adapters::erp::foodservice` — the transactional port: context
    ladder (commerce GUC first, foodservice floor grants), `owned_ticket`
    advisory fencing (open ticket per table), `execute_food_command` for all
    13 arms, `food_command_status`/recovery re-authorization under CURRENT
    grants, `claim_food_command` on commerce's idempotency row.
  - `archava_adapters::erp::commerce_kernel` order-side additions — the
    stock pool's second writer: `lock_commerce_order`, `reserve` /
    `fulfill` / `cancel` order flows with recipe×line allocations, menu
    product create/update, all against the SAME `archava_commerce` pool the
    purchasing cohort already writes.
  - `archava_host::erp::foodservice` — `FOODSERVICE_LEDGER` descriptor +
    `begin_foodservice` (`SET LOCAL statement_timeout='10s'`,
    `lock_timeout='3s'`), routes `POST /erp/foodservice/commands` (CSRF
    true) and `GET /erp/foodservice/commands/status` (CSRF false), mirroring
    the purchasing/CRM gate.
- **Needed shared edits (single-writer: Archxy 1) — ACTUAL diffs in PACKET-001.diff:**
  1. `rust/crates/host/src/router.rs`: the two foodservice mounts, exactly
     after the purchasing mounts (continuing the one `erp` api scope).
  2. `rust/crates/pg-fixture/src/lib.rs` — six sites: `FOOD_MIGRATIONS`
     const (`001_tables_recipes_tickets`), `Fixture.foodservice_ledger`
     field (+ ledger summary for startup readiness), the foodservice
     `migrate_ledger` call after the purchasing one, the runtime-grant block
     (statement-for-statement port of `grantFoodServiceRuntime` from
     `packages/foodservice-pg/src/migrate.ts`: schema USAGE + read-only
     memberships + menus/tables/tickets/commands/recipe_versions draft
     writing + components/lines/audit append-only + `lock_membership`
     executable), the constructor return field, and the
     `migrations_package` arm `"archava_foodservice" => "foodservice-pg"`.
  3. `tools/rust/verify.mjs`: gate step `erp foodservice suite`
     (`node tools/rust/erp/verify-foodservice.mjs`) directly after the
     purchasing step, plus the `P-ERP-FOODSERVICE` evidence-mapping entry
     (details read `dist/rust-erp-foodservice-proof.json`).
  4. `tools/rust/test/verify.test.mjs`: import
     `WITNESSES as FOODSERVICE_WITNESSES` and the drift test `the
     P-ERP-FOODSERVICE witness map and the frozen manifest cannot drift
     apart` (three-lists-equal + `witnessTotal`, mirroring the purchasing
     one; the hunk alignment is shifted because the two bodies share shape —
     it applies cleanly).
  5. `docs/rust/required-coverage.json` — **explicit scope edit, submitted
     for review (same editPolicy as PACKET-000)**:
     - `P-FAST-GATE.requiredChecks` gains `"erp foodservice suite"`.
     - NEW proof `P-ERP-FOODSERVICE` (kind evidence): nine requiredChecks
       grading the foodservice cohort's named witnesses — authorized flow
       consumes the shared pool exactly once, over-reservation refuses
       without holding stock, recipe refuses a non-tracked ingredient,
       revoked grant (RN-ERP-01), key/fingerprint conflict (RN-ERP-02),
       cross-tenant refusal, lost-response replay, cancel releases the
       reservation, missing grant — `expect.witnessTotal: 9`, sourcePaths
       binding `tools/rust/erp/verify-foodservice.mjs` + the
       host/adapters/domain/core crates + commerce and foodservice package
       migrations, referenced `dist/rust-erp-foodservice-proof.json`.
     Every witness name in the producer points at a real test identifier in
     `erp_foodservice.rs`, and `node --test` proves the frozen manifest, the
     WITNESSES map and the gate's step list cannot drift apart.
- **⚠ SEMANTIC-INTEGRATION NOTE (fixture file):** same situation as
  PACKET-000 — main's fixture file has moved past this copy's snapshot
  (`ledger_expectations` / `attach` / `dir`). Fold the six foodservice
  semantics onto MAIN'S CURRENT file (const into the migration mapping near
  purchasing's, field, extra `migrate_ledger` leg, grants into the ratchet
  block, constructor field, `migrations_package` arm); do not apply by line
  offset.
- **Migration/transaction ownership:** SQL migrations reused verbatim from
  `packages/foodservice-pg/migrations/001_tables_recipes_tickets.sql`
  (memberships composite FK, ticket UNIQUE open-per-table, menu_items FK →
  commerce products, ticket_lines FK → orders, movements CHECK consume-needs-
  order / adjust-needs-reason, `guard_stock_balance` stock ≡ Σ movements + 1
  revision — so test seeding goes through `commands` + an `adjust` movement,
  the honest path). Same shared `TenantTransaction`, `lock_membership` FOR
  SHARE on the membership ladder, no shadow authorization. Tickets claim the
  commerce idempotency row under the SAME request id and fingerprint rules
  the purchasing cohort froze, so the ONE command ledger arbitrates both
  writers.
- **Tests/commands/exits (recorded as they ran, 2026-10-17/18):**
  | Command | Result |
  | --- | --- |
  | `cargo test -p archava-host --test erp_foodservice` | **9/9 pass** (~96–108 s) across repeated runs |
  | `node tools/rust/erp/verify-foodservice.mjs` | PASS — 9 passed, discovered 9, exit 0 (80.5 s), proof written |
  | `node tools/rust/erp/verify-purchasing.mjs` (re-run after gate edits) | PASS — 9 passed, exit 0 (85.9 s) |
  | `node --test tools/rust/test/verify.test.mjs` | **20/20 pass** (incl. the foodservice drift test and the steps ≡ checks ≡ mapping test) |
  | Full host `--no-fail-fast` (TMPDIR convention) | erp_foodservice 9/9, erp_purchasing 9/9, authority 18/18, kdf_budget 3/3, readiness 5/5; **crm 25/26** — `the_racing_same_key_yields_conflict_not_the_other_receipt` flaked once under 7-binary parallel load (locks-assert at crm.rs:1124) and passes solo in 14 s — see EXECUTION_STATE fact 3; **lib 40/1** = only the pre-existing §1.4B failure (owner's) |
  | `cargo fmt --all --check` | clean |
  | `cargo clippy --workspace --all-targets --locked --offline` | clean |
  | `pnpm rust:verify` | unchanged blocker: it stops at `cargo test --workspace` on the §1.4B lib test (owner must fix); every downstream step is individually verified green |
- **Proof paths:** `dist/rust-erp-foodservice-proof.json` (producer
  `tools/rust/erp/verify-foodservice.mjs`, manifest binding
  `P-ERP-FOODSERVICE`).
- **Compatibility effects:** additive only; TS baseline packages untouched
  (the port MIRRORS `packages/foodservice-pg` and reuses its migrations);
  no existing route/response contract changed; no lock/package changes.
- **Exact next integration action:** Archxy 1 folds PACKET-000 first (or the
  purchasing hunks of its choice — packets are order-independent), then
  PACKET-001's hunks with the fixture folded semantically, re-runs both erp
  producers + `node --test tools/rust/test`, records landed hashes in
  `archxy1/`, then runs `pnpm rust:verify` on main — which still requires the
  §1.4B lib fix first.

## PACKET-002 — R06-TWO-CONSUMERS: two independently installed ERP module consumers

- **Base hashes:** same working copy as PACKET-000/001 (HEAD 21234bb… + dirty
  overlay; nothing committed anywhere). Packet diff published at handoff
  `archxy2/packets/PACKET-002.diff`. Independent of the earlier packets: they
  touch none of this packet's five files except
  `docs/rust/required-coverage.json`, and there at a DIFFERENT proof
  (P-MODULE-CONSUMERS vs P-ERP-*), so all three fold in any order.
- **Scope note (probes recorded honestly):** the cargo-tarball route
  (`cargo package -p archava-domain`) was probed first and does NOT work
  offline — the packaged verify build resolves `archava-core` the way a
  registry consumer does, and the unpublished path-only workspace
  dependency defeats it even with a `version` on the workspace dependency
  and a `[patch.crates-io]` present. Both Cargo.toml experiments were
  reverted byte-identically (`rust/Cargo.lock` sha
  5c20faaf764a6e3f… unchanged); this packet therefore contains NO
  rust/Cargo.toml change. The delivered consumers use the compiled npm
  library route instead: `tools/library/pack.mjs` already packs
  `@archava/purchasing-pg` + `@archava/foodservice-pg` (62 packages) into
  `dist/library/tarballs/`.
- **Owned paths:** `rust/crates/domain/src/erp/**`,
  `rust/crates/adapters/src/erp/**`, `rust/crates/host/src/erp/**`,
  `rust/crates/host/tests/erp_*.rs`, `tools/rust/erp/**` (incl. the new
  `module-consumers.ts` driver + `module-consumers.witnesses.mjs`),
  `examples/rust-consumers/erp/**` (the two new verticals),
  `docs/rust/archxy2/**`; handoff-only writes to
  `Archava_ERP_Handoff/archxy2/`.
- **Public interfaces requested (new, additive):**
  - `pnpm erp:verify-consumers` — the R06 module-consumers proof: packs the
    compiled library, installs BOTH product verticals
    (`examples/rust-consumers/erp/hospitality` +
    `examples/rust-consumers/erp/professional-services`) into scratch trees
    from the tarballs, and typecheck/build/verifies each against the
    INSTALLED modules.
  - The driver export `runModuleConsumers()` (`tools/rust/erp/
    module-consumers.ts`, run under tsx): per-consumer implementation
    hashes must MATCH across the pair; every hashed module must resolve
    INSIDE node_modules (workspace-path leak = refusal); writes
    `dist/rust-erp-module-consumers-proof.json`.
  - `tools/rust/erp/module-consumers.witnesses.mjs` — the P-MODULE-CONSUMERS
    witness map (consumers + stages), importable by `node --test` without
    launching tsx/npm.
  - The batteries ARE real consumption (a typeof check would not be a
    consumer): each vertical reads its ledger through the installed
    `purchasingMigrationLedger()` / `foodserviceMigrationLedger()` (the
    migration checksums read back from the installed `migrations/` trees),
    exercises both command grammars' refusal surfaces through the installed
    zod schemas, and drives `PurchasingService` / `FoodService` against
    recording fake repositories so the contract's validation is proven to
    run BEFORE any client query; each browser bundle's metafile must contain
    no `*-pg` module and no `node:` import.
- **Needed shared edits (single-writer: Archxy 1) — ACTUAL diffs in PACKET-002.diff:**
  1. `package.json`: script `"erp:verify-consumers":
     "tsx tools/rust/erp/module-consumers.ts"`.
  2. `tools/rust/verify-release.mjs`: release step `erp module consumers`
     (`pnpm erp:verify-consumers`) directly after the fast gate + COMMANDS
     entry + `releaseEvidenceMapping` entry binding
     `P-MODULE-CONSUMERS → erp module consumers` with details reading the
     proof JSON.
  3. `tools/rust/test/verify-release.test.mjs`: import of the witness map +
     the new drift test `the P-MODULE-CONSUMERS witness map and the frozen
     manifest cannot drift apart` (keys ≡ requiredChecks, per-consumer
     stage counts, frozen `witnessTotal`). The "two recorded additions"
     lists and the `notInHandoffList` expectation are UNCHANGED — the new
     command is documented (next item), not an addition.
  4. `docs/LOCAL_LIBRARY_HANDOFF.md`: the Reproduce-acceptance list gains
     `pnpm erp:verify-consumers`.
  5. `docs/rust/required-coverage.json` — **explicit scope edit, submitted
     for review**: the P-MODULE-CONSUMERS block is rewritten in place with
     the SAME proof id and the SAME two requiredChecks; grounding is added
     (`producedBy` names the real producer, `expect.witnessTotal: 4`,
     sourcePaths binding the consumers + driver + pack, `referenced` the
     proof JSON). No denominator growth: the requiredChecks list itself is
     unchanged.
- **Migration/transaction ownership:** none new — no SQL, no
  transactions, no fixture changes; pure packaging + npm installs +
  pure-contract batteries.
- **Tests/commands/exits (recorded as they ran, 2026-10-18):**
  | Command | Result |
  | --- | --- |
  | `pnpm erp:verify-consumers` | **PASS** end to end (pack 62 ✓ → both consumers typecheck ✓ build ✓ verify ✓ → hashes equal → proof written) |
  | `node --test tools/rust/test/verify-release.test.mjs` | **23/23 pass** (incl. the new P-MODULE-CONSUMERS drift test + the pre-existing reconciliation suite) |
  | `node --test tools/rust/test/verify.test.mjs` | 20/20 pass (fast-gate suite untouched by this packet's wiring) |
  | `sha256sum rust/Cargo.lock` | unchanged throughout (5c20faaf764a6e3f…) |
  | Full `pnpm rust:verify-release` | not run in the copy (owner runs it after folding; the new step is individually green and every other release step is unchanged) |
- **Proof paths:** `dist/rust-erp-module-consumers-proof.json` (bound to
  P-MODULE-CONSUMERS; `packages/purchasing-pg` + `packages/foodservice-pg`
  are the TS truths the batteries consume through the COMPILED artifacts).
- **Compatibility effects:** one new release-gate step; one new root
  script; the fast gate, Rust workspace, and all SQL/fixture surfaces are
  untouched; no lock changes.
- **Exact next integration action:** Archxy 1 folds PACKET-002 (any order
  relative to PACKET-000/001), re-runs `pnpm erp:verify-consumers` +
  `node --test tools/rust/test/`, records landed hashes in `archxy1/`, then
  runs full `pnpm rust:verify-release` on main.

## Pending packets

(none beyond the three above — all are published and await integration)
