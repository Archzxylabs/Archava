# Full Rust completion report

This report closes the R00–R11 + OWN-V1 program measured by the frozen
manifest `docs/rust/required-coverage.json` (93 scope items, 56 proofs).
Completeness is not asserted here — it is recomputed by
`node tools/rust/coverage.mjs` against the live candidate identity, fresh
source digests, and the per-proof artifacts the two program gates write. What
this document adds on top of the validator is the _reading_: which findings
closed, where the inventory went, and what the owner-configuration fistula
delivered. If any table below and the validator disagree, the validator wins
and this report is corrected — not the manifest.

> **Superseded (2026-10-22):** `required-coverage.json` is a **historical
> manifest frozen 2026-10-16**. The `RF01/RA06 coverage` row below is
> **SUPERSEDED** — its earlier closure claim is withdrawn. Re-running
> `node tools/rust/coverage.mjs` against
> the live tree returns `complete=false` with **299 errors** (169 required
> checks never ran, 50 identity mismatches, 40 stale source digests, 30
> incomplete source lists, 5 missing `P-R04-DOMAIN`…`P-R08-DOMAIN` evidence
> documents, 3 frozen tallies, 2 missing artifacts). It is not regenerated:
> the stale digests are the deliberate edits of A1-FU3/A5/A5-FU/A9, and the five
> domain evidence documents never existed. The gates that apply now are the A6
> coverage thresholds in `vitest.config.ts`, `pnpm test`, and the Rust gates
> (`archava-slice`, `archava-host --test own -- --test-threads=1`, `cargo fmt
--all --check`, `cargo clippy … -D warnings`). Evidence:
> `docs/audit/A4FU_COVERAGE_MANIFEST_2026-10-22.md`.

**Release identity:** profile `1.18.0`, `@archava/commerce-pg` `0.4.0`
(migration `004_bound_approvals` folded into the TS ledger; FK-verified sync
in the RN03 round). `dist/releases/1.17.0` untouched (the RN03 guard).

## Audit finding closures

Every finding below carries its proof pointer in
`docs/rust/EXECUTION_STATE.md` (the §1.3, §1.4 and RN01–RN08 rounds) and is
re-provable per release-gate run through the named producers.

| Finding                                       | Status                                          | Where it is proven                                                                                                                                                                                                                                                                                                                                                   |
| --------------------------------------------- | ----------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RF02 owned PG fixture                         | CLOSED                                          | `archava-pg-fixture` (per-test `initdb` + teardown + reap-with-liveness; the §1.4 round then hardened reaping and armed a panic guard). Runs inside every PostgreSQL-backed suite.                                                                                                                                                                                   |
| RA02 readiness gates serving                  | CLOSED                                          | `crates/host/tests/readiness.rs` + `tools/rust/pg-readiness.mjs` — `AppState::try_new` is the only constructor and refuses un-forced RLS / tampered checksums / owner connections.                                                                                                                                                                                   |
| RF03 fmt + Clippy                             | CLOSED                                          | `cargo fmt --all --check` + `cargo clippy --workspace --all-targets -- -D warnings`, both gates.                                                                                                                                                                                                                                                                     |
| RA03 fresh unit suite                         | CLOSED                                          | Cold `pnpm test` 137 files / 1821 tests; artifact-bound assertions split to `vitest.artifacts.config.ts`, run in the release gate after the build that produces them.                                                                                                                                                                                                |
| RA05 cold bootstrap                           | CLOSED                                          | `pnpm rust:bootstrap` from an empty `CARGO_HOME` (exit 0); consumer closure pinned by committed lock.                                                                                                                                                                                                                                                                |
| RF01/RA06 coverage                            | SUPERSEDED (historis; pra-remediasi 2026-10-21) | `required-coverage.json` (frozen 2026-10-16) + `tools/rust/coverage.mjs` + drift/negative tests (the gate's own test files refuse steps↔mapping↔manifest drift). Live validator: `complete=false`, 299 errors — scoreboard, not a passing gate.                                                                                                                    |
| RA07 calendar arithmetic                      | CLOSED                                          | Instant-domain validation + horizon-bounded walks, differential parity suite green (`crates/capacity`).                                                                                                                                                                                                                                                              |
| RA01 server-side authority                    | CLOSED (ADR-R11)                                | `crates/host/src/identity.rs` + migration `004_bound_approvals` + `tests/authority.rs` (18 tests, real HTTP + real PostgreSQL).                                                                                                                                                                                                                                      |
| RF04 stale fixture cleanup                    | CLOSED                                          | `reap_verdict` liveness matrix + `ProvisioningGuard` (`pg-fixture`).                                                                                                                                                                                                                                                                                                 |
| RN01 downgraded-grant receipt recovery        | CLOSED                                          | `a_downgraded_grant_cannot_recover_a_receipt_it_may_no_longer_act_on` + policy-gate re-derivation on status/replay/recovery; P-AUTHORITY-PG names it.                                                                                                                                                                                                                |
| RN02 idempotency-key reuse leak               | CLOSED                                          | Sequential + concurrent conflict witnesses (404 `unknown_request` / 409 `request_id_reused`, never the other receipt); P-AUTHORITY-PG names both.                                                                                                                                                                                                                    |
| RN03 release profile / package / ledger drift | CLOSED                                          | Profile 1.18.0 ↔ commerce-pg 0.4.0 ↔ `004_bound_approvals` (checksum verified on both languages); `dist/releases/1.17.0` verified untouched per pack.                                                                                                                                                                                                              |
| RN04/RN07 login-budget integrity              | CLOSED                                          | Shipped override deleted; deterministic `Err(Busy)` witness at the real budget; flood asserts a closed outcome set {200, 503 busy, 429 rate_limited} + recovery.                                                                                                                                                                                                     |
| RN05 capacity output domain                   | CLOSED                                          | Output-domain + underflow-floor regressions (`cargo test -p archava-capacity --lib` 38/38).                                                                                                                                                                                                                                                                          |
| RN06 fmt/clippy debt                          | CLOSED                                          | Workspace-wide `-D warnings` clean (both gates re-prove).                                                                                                                                                                                                                                                                                                            |
| RN08 migration-004 compile break              | CLOSED                                          | Workspace builds; readiness blocks re-verified against the real checksum.                                                                                                                                                                                                                                                                                            |
| A01 replay after revocation (D05)             | **CLOSED**                                      | The authority suite's named checks `revoked grant refused on replay` (+ execute/status/recovery) run over real PostgreSQL on every gate — the conformance test D05 demanded before the affected slice could be accepted. The frozen D05 text records the moment of its writing; this row is the status update, cross-referenced, not a rewrite of the frozen record. |
| A02–A05, A08–A09, J01–J04                     | **Recorded, not closed by this program**        | These are TypeScript-side or cross-cutting audit items outside the Rust program's ownership; they are not silently counted by the 83/83 controls and stay on the outstanding list.                                                                                                                                                                                   |
| A06 minimal artifact missing `zod` external   | Recorded                                        | Packaging-side item; the Rust program's release-identity machinery preserves the recorded artifact ledger without re-deciding it.                                                                                                                                                                                                                                    |
| A07 archive identity drift (D06)              | Recorded, unreconciled **by design**            | `dist/releases/1.17.0` is intentionally NOT rewritten; the old archive is a PRIOR identity, and `archava-ops verify-release` now scans every slot and refuses an archive claiming the current identity.                                                                                                                                                              |
| K22 nonprofit-associations narrow scope       | Not started                                     | Explicitly out of the Rust program's lane (unrelated task #8).                                                                                                                                                                                                                                                                                                       |
| Codex sign-off (R04–R08 domain proofs)        | Pending, external                               | The FK rule forbids self-filling the reviewed sign-off; the packets' absence blocks the four domain proofs (see below).                                                                                                                                                                                                                                              |

## Inventory disposition

The migration inventory is `docs/rust/migration-inventory.json`, bound by
`J-INVENTORY-JSON` in the frozen manifest and re-validated per coverage run
(`node tools/rust/coverage.mjs`; `tools/rust/inventory.mjs` feeds the same
tables). The dispositions this program recorded:

- **KEEP** — the standalone template (`Archava Template/`) with its Gemini
  realtime voice / LiveKit / Spatius renderer stack stays a separate,
  separately-gated surface (`tools/security/verify.mjs`'s `realtimeTemplate`
  limit restates it in every PASS line). P-REALTIME-VOICE quotes all four
  bridge documents byte-level.
- **MAP (documented bridge)** — the Gemini planner adapter: PRD §574's
  AI-04-wrapped vendor-bridge exception is honored by the live paths
  `packages/crm-assistant/src/gemini-planner.ts` (TS side) and
  `rust/crates/providers/src/gemini.rs` (Rust provider contract port). The
  legacy scan resolves the inventory's `apps/crm/src/gemini.ts` to these live
  paths because that inventory entry never existed in git — recorded in
  `dist/rust-legacy-scan.json` rather than silently re-pointed.
- **REPLACE (executed)** — the operational cutover table
  (`archava_ops::runbook::table()`) is executed by `tools/rust/release-ops.mjs`
  on every release-gate run: five CLI rows through the built binary + the
  rehearsal row through the cargo suite. The census is
  `{pg_ctl: 1, kill: 1}` — only PostgreSQL tooling and the process-signal
  wrapper remain on the runtime path; no JS engine is a dependency of any
  Rust crate.
- **Portal parity** — the WASM portable boundary (`tools/rust/
wasm-boundary.mjs`) holds six crates compilable to wasm32 and refuses the
  host crate (a PASS): what travels to the browser is portable, what cannot
  is named.
- The **SDK-side hosting** stays TypeScript-side per the recorded disposition
  in `crates/core/src/graph.rs`; P-SDK-WIDGET holds that surface to its
  contract instead of porting it into reached-but-unproven Rust code.

## OWN-V1

> **DISPUTED / NOT DELIVERED (superseded in part by the 19 Oktober 2026 CRM
> audit, finding C03).** The 19-10 audit reproduced that the ten `P-OWN-Txx`
> proofs are satisfied by evidence that does not exercise an owner
> configuration lifecycle: T01 from a policy-consumer `outcome.ok`, T06 from
> writer-fence/rollback rehearsals, T07 from deployment-plan unit tests, T10
> from the TypeScript CRM browser battery. The active mandate
> (`ARCHXY_1_FULL_IMPLEMENTATION_TASK.md` §4/A1-OWN-03) asks for the real
> inspect → plan → preview → confirm → apply → recovery → history/restore
> product behaviour with each proof bound to the operation it names. The
> paragraph below is kept as the history of what this lane believed on
> 19-10; it is **not** a claim that OWN-V1 is delivered. Current status:
> incomplete — see `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md` row C03.

**20-10 update — the semantic lifecycle now exists.** The 20 Oktober C03
closure supplies the behaviour the audit found missing: `rust/crates/own` +
`rust/crates/host/src/own.rs` + `/app/owner` implement inspect → plan →
preview → confirm → applied → recovery/history/restore over the persisted
`archava_owner` ledger; `/own/chat` parses into the SAME typed `Request` and
resolves through the same `catalogue().plan` as `/own/plan`
(`the_chat_plan_equals_the_dashboard_plan_and_applies`); `cargo test -p
archava-host --test own -- --test-threads=1` → 11/11; the browser owner
journey records revisions 1–4, refresh recovery, history, restore r1 as r3
and re-enable r4 with four `config_audit` rows by `alice`. The K05/K04
consumers now run the real CRM flow (6/6 witnesses each, identical
artifact/code/migration digests). `tools/own/verify.mjs` is re-bound to these
Rust/browser witnesses. This is **local** verification; independent Codex
sign-off and the final release gate remain pending.

The paragraph below is the 19-10 historical account. Its mapping (T01 from a
policy `outcome.ok`, T02 from the CRM assistant planner, T10 from the TS CRM
battery) is superseded by the 20-10 evidence above.

The bounded first-owner release was described as the acceptance family over
released surfaces (ADR-R13): `pnpm own:verify` (the `tools/own/verify.mjs`
driver) ran the ten `P-OWN-Txx` proofs fresh per invocation and the release
gate binds them as two steps. T01 = the K05/K04 two-business-model consumers
from one artifact bundle (config-only difference); T02 = the in-crate
witness proving the chat flow's consumed plan equals the dashboard path's
direct `LocalCrmPlanner` answer byte-for-byte JSON (21/21 lib tests + the TS
contract suites); T03/T07 = `buildDeploymentPlan`'s dependency/readiness/cost
refusals plus the two independently installed ERP module consumers; T04 = the
real-PostgreSQL authority suite mapped onto the four refusal families; T05 =
the ops binary's executed refusal classes AND the compatible request still
planning; T06/T08 = the fixture-backed rehearsal suite (single-writer fence,
rollback, backup/restore) plus the fresh-process identity recompute and
prior-archives-intact checks; T09 = presence/adapters staleness and
no-fabrication witnesses plus dual-engine pricing parity reruns; T10 = the
real headless Chromium journey via the CRM host battery, graded with the
harness's own no-owner-scope assertion. Full suite `OWN_SUITE_EXIT=0`; T10
green standalone. OWN-V2's custom-stage scope stays out per the PRD's own
boundary.

## Milestone state

| Milestone                                     | State                                                               | Notes                                                                                                                                                                                                                                                                                                                                                |
| --------------------------------------------- | ------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R00 inventory + freeze                        | **delivered**                                                       | `D-INVENTORY`, `J-INVENTORY-JSON`, `D-KNOWN-DIFFS`, `D-DECISIONS`, `D-EXECUTION-STATE`.                                                                                                                                                                                                                                                              |
| R01 foundation                                | **delivered**                                                       | workspace, axum host, contracts/errors, identity, readiness, RLS cohort, wasm boundary, perf budgets, browser slice.                                                                                                                                                                                                                                 |
| R02 pricing/rates/planning/capacity           | **delivered**                                                       | dual parity (P-P1/P-P2), planning + capacity conformance, benchmark, two cold consumers.                                                                                                                                                                                                                                                             |
| R03 CRM core                                  | **delivered**                                                       | contacts/pipeline, assistant contracts, receipts, identity/CSRF, K05/K04 consumers, negative evidence.                                                                                                                                                                                                                                               |
| R04–R07 (+ R08) domain depths                 | **blocked — Archxy 2 packets not delivered**                        | `P-R04-DOMAIN` … `P-R08-DOMAIN` are wired in the manifest and refused by the validator until the packets land in the handoff directory; the modules themselves (`@archava/purchasing`, `@archava/foodservice` + their `-pg` ledgers) are INTEGRATED and proven by `P-MODULE-CONSUMERS`, but the four domain-depth proofs await packet-side sign-off. |
| R08 other extensions (`R08-OTHER-EXTENSIONS`) | **partial**                                                         | Formal-education host steps are part of the 46-command security chain (owner-supplied); the R08 domain proof remains packet-blocked (above).                                                                                                                                                                                                         |
| R09 UI/assistant lanes                        | **delivered**                                                       | assistant context + provider contracts, SDK/widget/page-awareness, realtime/voice, browser slice.                                                                                                                                                                                                                                                    |
| R10 cutover safety                            | **delivered**                                                       | archava-ops CLI family, prior-window rehearsals, release-identity verify, ADR-R05/R12.                                                                                                                                                                                                                                                               |
| R11 legacy + kits + gates                     | **delivered** (documents bind here)                                 | legacy replacement census + bridge dispositions; kit readiness carried by the 46-command security chain; this report + the audit packet close R11-ALL-GATES.                                                                                                                                                                                         |
| OWN-V1                                        | **locally re-delivered (2026-10-20); independent sign-off pending** | Rust owner engine/host/UI lifecycle + `/own/chat` parity (host 11/11, engine 13/13) + real browser owner journey (revisions 1–4, refresh/history/restore, four `alice` audit rows) + K05/K04 6/6 witnesses each. The old acceptance-family mapping is retired. Tracking: `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md`.                                   |
| G01–G10 gate items                            | **delivered except G04**                                            | G04's five domain proofs are the packet-blocked set.                                                                                                                                                                                                                                                                                                 |

**Still open after this report:** (1) the four packet-blocked domain proofs
(P-R04…P-R08-DOMAIN) + their Codex sign-off; (2) A02–A05/A08–A09/J01–J04
(non-owned, recorded); (3) the cutover FLIP itself (serving traffic from the
Node runtime to the Rust host) — deferred by design until the domain depths
land; the safety machinery (R10) is complete. Everything the program OWNS is
proven by executed commands whose exits and artifacts the validator
recomputes.

**Terminal record (delivered):** the full gate over the corrected tree
(`pnpm rust:verify-release`, including the fast gate, the security chain,
both cold-prior paths, kits/ops/legacy/SDK/widget/realtime, the CRM
benchmark and every OWN producer) finished **17/17 proofs green** with
`RUST_VERIFY_EXIT=0`-equivalent all-green steps and **5 coverage errors**,
each one a packet-blocked domain proof (`P-R04…P-R08-DOMAIN` "no evidence
document"); the verdict's blocked set is exactly the R04–R08 lane items plus
`G04-DOMAIN-INTEGRITY` (25 items) — all downstream of those five producers.
`node tools/rust/coverage.mjs` reported the same five and nothing else on that day (re-run against the remediated tree, 2026-10-22: **299 errors**, exit 1 — see the superseded banner). Two
intermediate full runs failed inside the security chain for environmental
reasons (orphan postmasters after an aborted earlier run — see the audit
packet's watchlist); both affected batteries reverified green standalone and
the sixth run passed the chain with the orphans cleared. The program's local
milestone obligations (§50.3 EXEC-01…EXEC-06, §51.4 OWN-V1 with OWN-T01–T10,
and the R00–R03/R09–R11 porting lanes) are closed with executed-command
evidence; the release decision stays external (Archxy 2 packets + Codex
sign-off), and the cutover FLIP remains deferred by design until the domain
depths land.
