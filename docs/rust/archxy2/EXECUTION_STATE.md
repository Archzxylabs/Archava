# Archxy 2 execution state — ERP migration (R04–R08)

**Owner:** Archxy 2 (ERP implementator). This file is Archxy 2-owned state; Archxy 1
reads it at checkpoints and must not write here (counterpart handoff rules,
ARCHXY_2_ERP_IMPLEMENTATION_TASK.md §3.3).

**Working copy:** `/home/haikaru/Archverse/Lab/Archava_Platform_archxy2_erp`
(isolated candidate copy, rsync of HEAD + dirty/untracked public source,
manifest-verified — see handoff `BASELINE.md`). Nothing committed anywhere.

## Where the work actually stands (as of first green ERP run)

- **R06 purchasing host surface is IMPLEMENTED and VERIFIED.** The full chain
  `supplier.create → offer.create → order.create → order.confirm → order.receive`
  runs end to end through the real router against the owned PostgreSQL fixture:
  `rust/crates/domain/src/erp/` (pure contracts, 27 tests green),
  `rust/crates/adapters/src/erp/` (transactional purchasing repository +
  commerce kernel port: product-create, stock reads/locks, cross-ledger
  purchase receipt), `rust/crates/host/src/erp/` (Axum routes +
  `/erp/purchasing/commands` + `/status`, mirroring the CRM gate).
- **`cargo test -p archava-host --test erp_purchasing` = 9/9 witnesses green**
  (~53 s). The nine and their required checks are frozen into the coverage
  manifest as `P-ERP-PURCHASING` (scope edit delivered in PACKET-000 for
  review).
- **Producer wired:** `tools/rust/erp/verify-purchasing.mjs` grades each manifest
  check against its named witness BY NAME and writes
  `dist/rust-erp-purchasing-proof.json`. It ran green end to end (9 passed,
  discovered 9, exit 0, 52.9 s).
- **Gate wiring:** `tools/rust/verify.mjs` gained the step `erp purchasing
  suite` (after `pg authority suite`) + the `P-ERP-PURCHASING` evidence
  mapping; `tools/rust/test/verify.test.mjs` gained the three-lists-equal drift
  test for the new proof. All 19 gate unit tests pass under `node --test`.
- **Pre-existing host suites stay green in the copy:** authority 18/18,
  crm 26/26, readiness 5/5 (host) + 11/11 (adapters tenant context),
  kdf_budget 3/3, domain 27/27, adapters/erp + core all green.

## Where the work actually stands (as of the foodservice kernel run, 2026-10-18)

- **R06's second writer — the foodservice kernel slice — is IMPLEMENTED and
  VERIFIED.** Menus (products in the commerce pool), recipes, and dining
  tickets run end to end through the real router against the owned
  PostgreSQL fixture: `rust/crates/domain/src/erp/foodservice.rs` (13-arm
  command grammar, 6 unit tests; domain 33/33 total), the order-side additions
  to `adapters/src/erp/commerce_kernel.rs` (lock/reserve/fulfill/cancel +
  menu-product writes), `rust/crates/adapters/src/erp/foodservice.rs`
  (context ladder + all 13 arms + recovery), and the host surface
  (`host/src/erp/foodservice.rs`, routes `POST /erp/foodservice/commands` +
  `/status`, `begin_foodservice` 10s/3s).
- **`cargo test -p archava-host --test erp_foodservice` = 9/9 witnesses
  green** (~96–108 s). The nine are frozen into the coverage manifest as
  `P-ERP-FOODSERVICE` (scope edit delivered in PACKET-001 for review):
  authorized flow consumes the shared pool exactly once (single −2 consume
  movement, receipts bound in BOTH ledgers under the same request id),
  over-reservation refusal without holds, non-tracked ingredient rollback,
  revoked grant on recovery (RN-ERP-01), key/fingerprint conflict (RN-ERP-02,
  no hijack row), cross-tenant refusal, lost-response replay (one held
  allocation, decision replayed), cancel releases untouched, missing grant.
- **Producer wired:** `tools/rust/erp/verify-foodservice.mjs` grades each
  manifest check against its named witness BY NAME and writes
  `dist/rust-erp-foodservice-proof.json`. Green end to end (9 passed,
  discovered 9, exit 0, 80.5 s); the purchasing producer re-run after the
  gate edits is unaffected (85.9 s, PASS).
- **Gate wiring:** `tools/rust/verify.mjs` gained the step `erp foodservice
  suite` (after `erp purchasing suite`) + the `P-ERP-FOODSERVICE` evidence
  mapping; `tools/rust/test/verify.test.mjs` gained the foodservice drift
  test. All 20 gate unit tests pass under `node --test`.
- **PACKET-001 delivered:** handoff `archxy2/packets/PACKET-001.diff` — the
  five shared-file deltas (router mounts, fixture ×6 sites, gate step +
  mapping, gate drift test, manifest scope edit), additive to PACKET-000
  (main still has neither packet integrated; re-verified 2026-10-17).
- **R06's two-consumer requirement is IMPLEMENTED and VERIFIED.**
  `examples/rust-consumers/erp/{hospitality,professional-services}` are two
  independently installed product verticals consuming BOTH accepted R06
  modules (`@archava/purchasing` + `@archava/foodservice` and their `-pg`
  ledgers) from the COMPILED library tarballs — the crm-consumers precedent's
  packaging mechanics. The driver `tools/rust/erp/module-consumers.ts` packs
  the library (62 packages), installs both consumers from tarballs in scratch
  trees, typechecks/builds/verifies each against the INSTALLED modules, hashes
  the five module implementations across the pair (must match), and refuses
  any dependency that resolves outside node_modules. The batteries are real
  consumption, not typeof checks: frozen migration ledgers read back from the
  installed `migrations/` trees, both command grammars' refusals, service
  calls whose validation provably runs before any client query, browser
  metafiles with no `-pg` leakage. The witness map for the manifest drift
  test lives in `tools/rust/erp/module-consumers.witnesses.mjs`.
- **PACKET-002 delivered:** handoff `archxy2/packets/PACKET-002.diff` — five
  shared-file sections (package.json script, release-gate step + COMMANDS +
  evidence mapping, release-test import + P-MODULE-CONSUMERS drift test,
  LOCAL_LIBRARY_HANDOFF reproduce line, manifest P-MODULE-CONSUMERS
  grounding rewrite: `expect.witnessTotal: 4` + sourcePaths + referenced
  proof; the two requiredChecks themselves are unchanged). The cargo-tarball
  route for `archava-domain` was probed first and does NOT work offline (the
  packaged verify build searches crates.io for the unpublished path-only
  `archava-core`); both Cargo.toml experiments were reverted — the shipped
  consumers need NO workspace change.

## Known external facts (recorded, NOT Archxy 2 defects)

1. `router::tests::the_contact_list_refuses_rather_than_faking_an_empty_result`
   fails identically in the MAIN tree (`/home/haikaru/Archverse/Lab/Archava_Platform`,
   reproduced 2026-10-17): GET `/contacts` without a session returns
   `400 {"decision":"rejected","reason":"malformed_submission"}` instead of
   `403 role_not_allowed`. It is a §1.4B (Archxy 1) regression —
   `Query<ContactsPageQuery>` now rejects before the role gate runs.
   **It makes `cargo test --workspace` fail in both trees**, so the fast gate
   stops at that step until the owner fixes it; 4/12 upstream steps
   (build artifacts, fmt, clippy, wasm boundary) were green in the copy.
2. **Coverage-manifest drift vs the copy's snapshot:** the main tree's
   `rust/crates/pg-fixture/src/lib.rs` gained `ledger_expectations`,
   `attach()` and `Fixture::dir()` AFTER this copy's snapshot was taken
   (main-tree file mtime 2026-10-17 14:27 vs copy creation 19:27; `rust/` is
   untracked on both sides so git cannot arbitrate). PACKET-000's fixture
   patch must therefore be integrated SEMANTICALLY onto the current main file,
   not applied literally. Recorded in INTEGRATION_REQUESTS.md.
3. **Intermittent authority-suite flake (Archxy 1's test):**
   `concurrent_same_key_different_input_yields_conflict_not_the_other_receipt`
   fails intermittently under parallel-suite load on this box
   (~2/10 observed), always at the same point: the lock-holder's 30 s poll
   reports **0 parked waiters** (`pg_locks` shows none of the two submissions
   waiting on `archava_commerce.commands`). It passes when the suite runs
   without sibling load (solo runs, quiet-machine runs: 104.76 s matching the
   main tree's 104.03 s). Not load-injectable by Archxy 2 — recorded as an
   integration request to harden the holder's deadline/poll design.
   **2026-10-18 addendum:** the CRM suite's racing twin of the same holder
   design, `the_racing_same_key_yields_conflict_not_the_other_receipt`, flaked
   once in a 7-test-binary parallel run (crm.rs:1124 — "both same-key
   submissions must have parked on the write lock") and passes solo in 14 s.
   Same class, same hardening request — extend the holder fix to the CRM
   racing test as well; no Archxy 2 patch.
4. **`/tmp` tmpfs quota (7.7 G, shared with other tooling) cannot host a full
   parallel PG fleet** — repeated `initdb … No space left on device /
   Disk quota exceeded` under 26 concurrent instances. Zero-code workaround
   (the fixture honors `TMPDIR`):

   ```
   TMPDIR=/home/haikaru/Archverse/tmp-fixture <run>
   ```

   All green runs in this state file were executed that way; killed test runs
   leave `postgres -D /tmp/archava-pg-fixture-*` postmasters behind, which must
   be killed (`pkill -x postgres` on this box) and the dirs removed before
   re-running.

## Required ID ledger (done / remaining)

| Required ID (coverage)                 | Status         | Producing artifact                                        |
| -------------------------------------- | -------------- | --------------------------------------------------------- |
| R06-SINGLE-STOCK-POOL                  | verified (commerce + purchasing + foodservice write the ONE pool; 9+9 witnesses) | `erp_purchasing.rs` + `erp_foodservice.rs` + `adapters/src/erp/commerce_kernel.rs` |
| R06-PARTIAL-RECEIPTS                   | purchasing half verified (foodservice adds no receipts contract) | same purchasing suite (over-acceptance + rejected-only witnesses) |
| R06-RECEIPTS-AUDIT-CLAIMS              | verified (purchasing receipts; foodservice tickets bind receipts/audit/commands in BOTH ledgers under one request id) | both suites (receipts/receipt_lines/audit/commands read-backs) |
| R06-TENANT-SCOPE                       | verified (purchasing + foodservice: cross-tenant, missing-grant, revocation witnesses) | both suites |
| R06-SINGLE-WRITER                      | foodservice kernel slice verified (tickets claim/reserve/consume via ONE command ledger; fencing = claim + fingerprint) | `erp_foodservice.rs` + `host/src/erp/foodservice.rs` |
| R06-TWO-CONSUMERS                      | verified (two independently installed verticals consume BOTH -pg modules from compiled tarballs; batteries + hash-equality + node_modules-only resolution; PACKET-002) | `examples/rust-consumers/erp/*` + `tools/rust/erp/module-consumers.ts` |
| R04-Organizations/Proposals/Projects   | not_started    | depends on G02/G03 common contracts, no UI dependency      |
| R05-Scheduling/Occupancy/Assets        | not_started    | follows R06 patterns (same transaction SPI)                |
| R07-Billing/Subscriptions/Attendance/Support/Fulfillment | not_started | inside dependencies |
| R08-Laundry/FieldServices/FormalEducation/Other | not_started | follows corrected contracts (formal-education TS baseline) |
| P-ERP-PURCHASING                       | producer wired + green; gate step registered | `tools/rust/erp/verify-purchasing.mjs` → `dist/rust-erp-purchasing-proof.json` |
| P-ERP-FOODSERVICE                      | producer wired + green; gate step registered (PACKET-001) | `tools/rust/erp/verify-foodservice.mjs` → `dist/rust-erp-foodservice-proof.json` |
| P-MODULE-CONSUMERS                     | producer wired + green; release-gate step registered (PACKET-002) | `tools/rust/erp/module-consumers.ts` → `dist/rust-erp-module-consumers-proof.json` |
| P-R06-DOMAIN                           | untouched (Archxy 1 slot; the remaining-half work now lives in the cohort suites above) | — |

## Next steps

1. R04 → R05 → R07 → R08 in that order per ERP_SCOPE.md §order (R05 reuses
   the foodservice-class transaction SPI and can reuse the two-consumers
   packaging shape verbatim; R07 reuses receipts/approvals).
2. Delivery state: PACKET-000, PACKET-001 and PACKET-002 are all published
   to the handoff (see INTEGRATION_REQUESTS for the semantic-integration
   notes); Archxy 1 folds them in any order, records landed hashes, and
   runs the gates on main after repairing the §1.4B lib test.

## Blockers

- `pnpm rust:verify` cannot go all-green until the §1.4B lib test above is
  repaired by its owner (reproduces in BOTH trees). Everything downstream of
  `cargo test --workspace` in the gate is individually verified green in this
  copy (readiness/authority/erp producers ran; parity+consumers unaffected).
