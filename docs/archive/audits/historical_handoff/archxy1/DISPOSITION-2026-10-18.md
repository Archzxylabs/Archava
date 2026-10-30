# Archxy 1 disposition — ERP packet integration, 2026-10-18

Written by Archxy 1 (main tree writer). Read-only against `archxy2/`.

## Landed

- **Commit:** `fa594b0` on `feat/laundry-1.17.0` ("integrate ERP packets
  000-002"), parent `b43598f` (P-K05-K04), grandparent `1cea4e4` (R03). All
  three packets integrated in ONE integration commit so the manifests/locks
  and the two suites land together; nothing was pushed.
- **Owned files:** copied verbatim from
  `/home/haikaru/Archverse/Lab/Archava_Platform_archxy2_erp` — domain
  `src/erp/**` (5), adapters `src/erp/**` (4), host `src/erp/**` (3),
  host `tests/erp_{purchasing,foodservice}.rs`, `tools/rust/erp/**` (4),
  `examples/rust-consumers/erp/**` (16 files), `docs/rust/archxy2/**` (the
  three readables as of 2026-10-18; future packets may publish newer
  versions — overwrite those paths). `packages/{purchasing,foodservice}-pg`
  were already tracked in main and byte-identical to your copy.

## Shared-file folds

- **pg-fixture/src/lib.rs — SEMANTIC, as requested:** main's
  `ledger_expectations()`/`attach()`/`Fixture::dir()` kept; purchasing +
  foodservice arms added to the frozen-list mapping and to `migrations_package`;
  both grant ratchets appended statement-for-statement; the 4-parameter
  `migrate_ledger` from your snapshot-era design NOT applied (main's 3-arg +
  dispatch stood); fixture construction (provision and attach) gained both
  ledger fields.
- **host/src/router.rs — SEMANTIC:** `try_new_erp` + the four mounts folded
  after main's R03 assistant mounts. Your diff's SESSION_COOKIE doc and
  `login()` HeaderMap rewinds were REJECTED — those were main's own R03
  changes your copy predates, not your edits.
- **verify.mjs / verify.test.mjs / required-coverage.json — SEMANTIC:** both
  erp steps land after `pg authority suite`, both mapping entries before
  P-COLD-CONSUMERS, both drift tests after P-AUTHORITY-PG's, both proof
  blocks after P-AUTHORITY-PG's; main's R03 step/proof/lines preserved
  around them. Manifest net change is exactly: P-FAST-GATE + 2 checks,
  + P-ERP-PURCHASING, + P-ERP-FOODSERVICE, P-MODULE-CONSUMERS grounding
  rewrite (denominator +2 proofs +2 checks, exactly as your packets state).
- **verify-release.mjs:** `erp module consumers` placed FIRST AFTER the
  K05/K04 step rather than directly after the fast gate — main consumed that
  slot at b43598f before your packet landed; both new steps are
  fixtureless/independent and the drift tests do not pin their order.
  COMMANDS + P-MODULE-CONSUMERS mapping folded per your packet.
- **verify-release.test.mjs:** import + drift test folded; the pinned
  `notInHandoffList` stays at the recorded three because
  `pnpm erp:verify-consumers` is documented in the handoff list (your note
  was correct).
- **package.json / docs/LOCAL_LIBRARY_HANDOFF.md:** line-level per-hunk
  staging (both files carry another lane's dirty lines on main; only the
  `erp:verify-consumers` script line and the reproduce-list line were
  committed — your lines landed, their in-flight lines stayed dirty).

## Verification on main (recorded exits)

- `cargo test -p archava-host --test erp_purchasing` — 9/9 (71.21 s)
- `cargo test -p archava-host --test erp_foodservice` — 9/9 (66.43 s)
- `node tools/rust/erp/verify-purchasing.mjs` / `verify-foodservice.mjs` —
  both PASS, proofs written
- `pnpm erp:verify-consumers` — PASS end to end
- `node --test tools/rust/test/verify.test.mjs` — 23/23
- `node --test tools/rust/test/verify-release.test.mjs` — 24/24
- Full fast gate `pnpm rust:verify` (TMPDIR=/home/haikaru/Archverse/tmp-fixture)
  — **16 steps, exit 0**; fmt + `cargo clippy --all-targets --locked
  --offline` clean. Cargo.lock regeneration matches your packets' predicted
  net effect exactly (adapters += archava-domain/serde/serde_json/uuid;
  domain += serde_json).

## Corrections to owner-side facts recorded in your checkpoints

1. **§1.4B lib failure — NOT present on main at integration time.**
   `router::tests::the_contact_list_refuses_rather_than_faking_an_empty_result`
   passes (solo run and inside the full lib run: **41/41**) on main at
   fa594b0^ and fa594b0. Your observation predates main's R03 commit
   (1cea4e4); your copy is at 21234bb. A first local re-run DID reproduce
   your exact error — stale `target` state from an earlier interrupted
   `cargo package` verification; `cargo clean -p archava-core` + rebuild
   resolved it and it has not returned across a full 16-step gate and
   multiple `--workspace`-scoped runs.
2. **Crate-tarball offline conclusion — confirmed identical to ours.** Main's
   P-K05-K04 (b43598f) shipped the same conclusion independently tonight:
   packed source bundles are the working offline artifact form for rust
   crates with internal workspace deps; `cargo package` cannot emit one for
   archava-domain. Your two probes did not need reverting on the main tree —
   main never carried that experiment.
3. **Locking-race hardening request — accepted, watch-listed.** Main's
   `8cc7ed4` already hardened the CRM twin ("the CRM racing witness awaits
   the holder's lock confirmation too"). The authority-suite
   `concurrent_same_key…` twin stays on the main watch list; no failure has
   occurred across main's gate runs so far. We will re-probe under load when
   we next run the full release gate.

## Left dirty on main (other lanes' in-flight lines)

`package.json` (formal-education script lines), `.github/workflows/ci.yml`,
`docs/LOCAL_LIBRARY_{HANDOFF,RELEASE}.md` (their module-list/prose lines),
`tools/library/` + `config/library-release.v1.json` (their surface), the
cross-lane release-gate blocker in
`tools/library/test/manifest.test.ts` (PROFILE 1.17.0 vs config 1.18.0)
remains with its owner — the full release gate still cannot run end to end
until that file's expectation updates; every individually-runnable release
step is green.

## Next from this side

R09 (assistant/context/SDK, per the remaining manifest
proofs), R10/R11, OWN-V1, and the full release gate the moment the
cross-lane manifest test unblocks. Integration of future packets continues
with the same fold-then-record loop.

## Landed shared-file hashes (as committed in fa594b0)

```
  rust/crates/pg-fixture/src/lib.rs a3df110b0efedd83d8ae3e4a2e53ca67774fa6f96e31fca3e319ec7edc2eec78
  rust/crates/host/src/router.rs 3f547f41b66d038e0bca7362fe4aeea572296909ffaa1690fcaffac61aaeb5f2
  tools/rust/verify.mjs 15bf6af24d99c781a3da24a15dd6dec236a2f8cbda1446f0f4523ea1f466ce02
  docs/rust/required-coverage.json f5a9070eea0680e2d5cf577ed0c322d7f7e8074b74065c610605bdfff0365259
  rust/Cargo.lock 7c521453ef7edb78d158a53201f43908f781869e246d14bf4d1b7397095b3750
```
