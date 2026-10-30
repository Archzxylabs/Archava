# Full Rust audit packet

The re-auditor's entry point: what to run, in what order, what each command
must produce, and what is already known to be imperfect. Nothing here
requires trusting a table — every claim is bound to a command whose exit and
artifact are recomputed by the coverage validator.

## Verification commands

Fast gate (the working-tree spine; ~15–20 min):

```
pnpm rust:bootstrap          # cold-cache fetch; exit 0 required
pnpm rust:verify             # 18 steps; RUST_VERIFY_EXIT=0 required
```

Release gate (the complete candidate; ~45–75 min):

```
pnpm rust:verify-release     # ver-ver steps; exit 0 required
node tools/rust/coverage.mjs # the scoreboard; complete=true required
```

Per-producer commands (each also runs inside the gates; run standalone for a
focused re-check — every one writes its details file even on failure):

```
node tools/rust/pg-authority.mjs        # P-AUTHORITY-PG  (real PostgreSQL)
node tools/rust/pg-readiness.mjs        # P-READINESS-PG  (real PostgreSQL)
node tools/rust/release-ops.mjs         # P-CLI-OPERATIONS + P-UPGRADE-RESTORE-RUST + P-RELEASE-IDENTITY
node tools/rust/legacy-replacement.mjs  # P-LEGACY-REPLACEMENT
node tools/rust/sdk-widget.mjs          # P-SDK-WIDGET (packs the library; ~5 min)
node tools/rust/realtime-voice.mjs      # P-REALTIME-VOICE
node tools/own/verify.mjs               # P-OWN-T01 … P-OWN-T09 (~15 min)
node tools/own/verify.mjs --browser-only # P-OWN-T10 (real headless Chromium)
node tools/rust/k05-k04-consumers.mjs   # P-K05-K04 (kit consumers from packed artifacts)
node tools/rust/browser-slice.mjs       # P-RUST-BROWSER (Leptos slice in a real browser)
pnpm erp:verify-consumers               # P-MODULE-CONSUMERS
pnpm rust:benchmark                     # P-BENCHMARK (+ the artifact-proofs step that follows it)
pnpm security:verify                    # P-SECURITY-CHAIN (46 commands, ~30 min)
```

Test pins (drift sentinels for the gates; no PG, no browser):

```
node --test tools/rust/test/verify-release.test.mjs   # 24/24: steps↔mapping↔manifest↔pins
node --test tools/rust/test/verify.test.mjs           # fast-gate drift + negative replays
node --test tools/rust/test/bootstrap.test.mjs        # consumer closure pin
node --test tools/rust/test/upgrades.test.mjs         # upgrade-flow drift pins
```

Evidence lands in: `dist/rust-evidence/*.json` (validator-checked proofs),
`dist/own-evidence/P-OWN-T*.json` (the OWN family), `dist/rust-ops-proof.json`

- `dist/rust-legacy-scan.json` (the R10/R11 producers), and the step logs'
  uppercase `PASS`/`FAIL` lines.

## Run instructions

Prerequisites:

- PostgreSQL server tools on PATH (`initdb`, `pg_ctl`, `pg_dump`,
  `pg_restore`) — the fixture-backed suites provision real clusters.
- The Rust toolchain pinned by `rust/rust-toolchain.toml` (cargo 1.96.x) and
  `wasm32-unknown-unknown` installed (`rustup target add
wasm32-unknown-unknown`) — the wasm boundary step refuses without it.
- Node (pnpm store warm) — the TS-side witness suites run via pnpm exec.
- A headless Chromium-family browser for the two real-browser proofs
  (`E2E_BROWSER` overrides the default `/usr/bin/brave`).

Order (fresh checkout):

1. `pnpm install` (workspace deps) and `pnpm rust:bootstrap` (cold cargo).
2. `pnpm rust:verify` — the fast gate; fix anything red before continuing.
3. `pnpm rust:verify-release` — the release gate. NOTE: the step that packs
   the library (`sdk-widget.mjs` runs `tools/library/verify-consumers.mjs`)
   rewrites `dist/library/`; the gate's own ops step recomputes identity
   AFTER that pack, and if you run kits/CRM verification MANUALY afterwards,
   re-run `pnpm crm:build` first (the recorded RN03 instruction) so the CRM
   runtime's recorded profile hash matches the fresh pack.
4. `node tools/rust/coverage.mjs` — the scoreboard must read
   `complete=true` with zero errors; any non-manifest movement is a refusal
   printed with the exact reason.

Environment notes carried forward: scrypt's debug-build cost (~10.9 s vs
0.207 s release) makes authority-suite timing a container-dependent property
— the suite is semaphore-serialized to the shipped budget and asserts CLOSED
outcome sets, not wall-clock luck (RN07 note). The PG fixture reaper keeps
live instances older than the age gate; concurrent fixture runs are
serialized by `PROVISION_SLOTS`.

## Known risks

- **R04–R08 domain-depth proofs are packet-blocked.** `P-R04-DOMAIN` …
  `P-R08-DOMAIN` are wired and REFUSED by the validator until the Archxy 2
  packets land in the handoff directory (`archxy2/` of the ERP handoff tree)
  — the modules themselves are integrated and proven by
  `P-MODULE-CONSUMERS`, but the domain-depth audits await packet delivery
  and a Codex sign-off the FK rule forbids self-filling.
- **D01/D02 record measured contract differences, not defects.** 12
  error-class-only pricing differences and 5 reference-rates divergences are
  the frozen, honest record (`docs/rust/KNOWN_DIFFERENCES.md`); the crate
  boundary preserves the TS contract where it matters and the split is
  measured, not assumed.
- **The D-documents bind by presence, not by prose.** `D-COMPLETION-REPORT`
  / `D-AUDIT-PACKET` / `D-KNOWN-DIFFS` are validated for existence and
  marker headings; their claims are auditable through the commands above,
  and any table here that disagrees with a producer output is corrected in
  the document, never in the manifest.
- **A02–A05, A08–A09, J01–J04, K22 remain open** on the TypeScript side or
  as unrelated tasks; the Rust program records them as not-owned rather
  than absorbing them silently (the completion report's closure table).
- **A07's prior archive stays untouched by design** (D06): `dist/releases/
1.17.0` is a PRIOR identity; `archava-ops verify-release` refuses any
  archive claiming the CURRENT identity and scans every slot each run.
- **The cutover flip is deferred, not failed:** R10 delivered the safety
  machinery (fence, rehearsals, identity verify, runbook executed); serving
  traffic flips only after the domain depths land.
- **Real-browser proofs are environment-dependent.** `P-RUST-BROWSER` and
  `P-OWN-T10` drive a headless Chromium-family binary; a missing browser is
  a loud step failure, not a silent pass.
- **Timing-sensitive denials are proven at counters, not clocks.** The
  login-budget and presence-lane witnesses pin deterministic refusal shapes
  in-crate; HTTP-level floods assert closed outcome sets (RN07's recorded
  diagnosis).

### Watchlist — security-chain environment (recorded against final6/final7/final8)

- **Orphan postmasters from aborted runs.** A full-gate run that aborts
  mid-chain can leave private postgres processes alive after their battery's
  tempdir was removed; their io workers keep burning CPU on a deleted data
  dir. Two consecutive chain failures had no code behind them
  (`fulfillment-host`: operator "database state" envelope; then `act`:
  live-but-unready login preceded the real catch) and both batteries
  reverified green standalone minutes later — the corpses' load made
  `pg_ctl -w` miss its 60s start deadline. The gate now reaps orphans at
  start (`reapOrphanPostmasters`, `94307ed`): kill only whose `-D` data
  directory does not exist, log what was killed. The per-command chain logs
  (`dist/security-proof/<name>.log`) are where the real inner error lives.
- **The chain batteries require the default TMPDIR.** The act integration's
  isolated-worker guard accepts only a fixture host under
  `/tmp/archava-act-proof-` by name; do not redirect TMPDIR around
  `pnpm rust:verify-release`. (Recorded because one full run was poisoned by
  exactly this operator action.)
- **Private-postgres start deadline.** `apps/crm/src/local-postgres.ts`
  boots with `pg_ctl -w` (60s default deadline). Under heavy load this can
  fail transiently; the reaper plus a quiet box is the mitigation recorded
  here. The deadline change itself lives in owner-dirty CRM source and is
  not part of this lane's commits.
