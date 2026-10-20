# ARCHXY 1 — Acceptance Matrix (traceability index)

**Status:** live index, updated as work lands. This is a TRACEABILITY index,
not a substitute denominator: the frozen denominator stays
`docs/rust/required-coverage.json` (93 items / 53 proofs), and this file never
shrinks it. Independent (Codex) review is a separate fact from local
verification and is marked `pending` everywhere below.

**Mandate:** `ARCHXY_1_FULL_IMPLEMENTATION_TASK.md` (20 Oktober 2026).
**Audit baseline:** `docs/ARCHXY_1_CRM_AUDIT_2026-10-19.md`
([machine evidence](audit/2026-10-19-archxy1-crm-audit.json)).
**Candidate at matrix creation:** HEAD `409eab9` (`feat/laundry-1.17.0`),
profile `1.18.0`, rustc/cargo 1.96.0, node 26.10.0, pnpm 10.15.1.

Status vocabulary: `implemented` → `locally verified` → `integrated verified`.
Marker headings, file/LOC counts, check names and a JSON `pass` are NOT proof
of behaviour.

## C-finding closure queue (the audit's seven)

| ID | Task | Required closure | Current evidence (real, not a marker) | Status | Next action |
| --- | --- | --- | --- | --- | --- |
| C01 | A1-CRM | All seven baseline writes, existing reads and usable CRM screens/action flow — not `followup.create` plus a read-only assistant | **Locally verified (2026-10-20).** Commands: all seven contract operations (`contact.create/update`, `activity.add`, `followup.create/complete`, `opportunity.create/move`) with the TS wire names, tri-state patch, boundary validation before database work, the operation-family write grant (+`contacts.read` for the pipeline lane), scope containment, `PIPELINE_TRANSITIONS` as its own state machine with immutable history, and one transaction per command carrying effect + audit + receipt. Reads: contacts list (literal search, stable cursor pages), record with contact/activities/follow-ups/opportunities AND each opportunity's transition history (one scoped query). Product UI: contacts search/next-page/selection/detail + a real `contact.create` form (C05 marker written before the send, storage refusal blocks, unresolved marker blocks a fresh attempt with a recovery link, host refusal renders its reason); `/app/contact` edit/activity/follow-up complete/opportunity create/move/history; `/app/desk` visible assistant panel over the real conversation route (overview, worklist, pipeline, contacts, summary, planner identity) where a prepared completion carries `requiresApproval`, the task stays `open:1` until the human confirms, and the confirmation executes the typed `followup.complete`; plus draft/recover and login/logout. Proof: `cargo test -p archava-host --test crm -- --test-threads=1` → 36/36; assistant suite 13/13, assistant lib 25/25; `pnpm rust:browser-slice` → 32/32 witnesses, exit 0 (record effects asserted in PostgreSQL, desk `open:1` before / `completed:2` + `alice:followup.complete` after, create form psql row at revision 1 owned by alice, pipeline history rendered, planner identity shown). Scope note: the TS panel's optional `approvalBase` native-approval SERVICE is not part of this slice; the consent surface here is prepare → explicit human confirmation → typed command (A1-CRM-03's separate intent/consent contracts), and the service-on-behalf lane is proven at the core level with both actors recorded. | **locally verified** | Final Codex audit; `integrated verified` only after it |
| C02 | A1-AUTH | UI logout revokes the server session and closes cookie/bearer/reconnect authority | **Closed locally (2026-10-20).** `Identity::revoke` (`rust/crates/host/src/identity.rs`) deletes the session row and appends the `logout` identity event in one transaction; `POST /logout` (`rust/crates/host/src/router.rs`) runs the CSRF gate BEFORE revocation, deletes the row, then expires the cookie (`Max-Age=0`); the slice's `close_server_session` closes the server session first and clears local facts only after the server says so (a 401 "no session" also clears, a 403/503 keeps state and reports the refusal). Proof: `cargo test -p archava-host --test session --offline` → 6 passed, 0 failed (read/mutate/recover refused after logout, missing/forged CSRF with a LIVE session refused without revoking, other sessions untouched, restart does not resurrect, no-session logout = 401); `pnpm rust:browser-slice` → 20/20 witnesses, EXIT=0, including `logout_revokes_server_session` (psql: the presented row is gone) and `logout_blocks_old_credential` (same browser reads /contacts → 401). | **locally verified** | Re-run the full gate after C01 lands |
| C03 | A1-OWN | Actual owner configuration lifecycle and semantic OWN-T01–T10 proofs | **Partially verified (2026-10-20).** Engine 13/13; persisted `archava_owner` lifecycle + routes + `/app/owner` UI + CRM `crm-pipeline` gate proven by `cargo test -p archava-host --test own -- --test-threads=1` → 8/8 (disable→`module_disabled`→enable, replay, stale/foreign confirmation, missing/revoked grant, restore-as-new-revision with intact history, racing applies = one revision, rebuilt host replays, mandatory blocked, assistant tool gate). Still open: T01 two packaged consumers, T02 chat=dashboard parity, T10 browser journey, OWN evidence producer re-binding, final gate | **open** (partially verified) | Bind T01/T02/T10 and the OWN evidence producer; run the browser owner journey and the final gate |
| C04 | A1-REUSE | Two packaged independent consumers execute real CRM business behaviour on isolated persisted data | `tools/rust/k05-k04-consumers.mjs` bundles `archava-core`/`archava-domain`; consumer witnesses cover tenant IDs, role table, action registry, `CommandAuthorizer` — no CRM repository/operation runs | **open** | Extend both consumers to run contact/activity/follow-up/opportunity read-write-recovery against their own persisted store, from identical released CRM artifacts |
| C05 | A1-RECOVERY | App-owned pending identity survives reload/unknown outcome and blocks accidental new intent | **Closed locally (2026-10-20).** New `rust/crates/slice/src/pending.rs` stores an account/workspace-bound marker (`requestId` + `tenant` + `principal` + `createdAt` only — no business payload, no secret) BEFORE the send; a storage refusal blocks the submission instead of sending unrecoverably; `draft.rs` restores the ORIGINAL key on mount (session-bound effect), re-submits under it, refuses a fresh key while unresolved, and clears the marker on any known outcome (200/409/refusal) and on committed recovery; logout clears every marker. Proof: `node tools/rust/browser-slice.mjs` → **32/32 witnesses, EXIT=0** at HEAD `902fb9d`, including `pending_marker_survives_reload` (reload returns the original key; fresh key refused), `pending_marker_cleared_after_resolution` (committed receipt clears it), `storage_refusal_blocks_submission` (host never saw the command — psql count 0) and `record_pending_marker_blocks_new_intent` (the exact stored marker shape, including principal `alice@archava.test`, made a new activity write refuse `pending_unresolved` with the psql count unchanged; clearing it unblocked the record). | **locally verified** | Re-run the full gate after C01 lands |
| C06 | A1-UI | Literal query encoding, functional cursor pagination and reset/error/race semantics in the browser | **Closed locally (2026-10-20).** `contacts.rs` now percent-encodes tenant, search and cursor (`query_encode`); the page renders real `Next page`/`First page` actions driven by the server's cursor, resets the cursor when the filter changes, and names "this page is the last" when the server returns none. Host keyset (`ORDER BY id`, `id > after`) was already correct. Proof: `pnpm rust:browser-slice` → 20/20, EXIT=0, including `literal_search_is_encoded` (`Ada &` finds only "Ada & Lovelace Ltd"; the audit's `Ada&limit=1` returns 0 rows) and `cursor_pagination_loads_later_rows` (26 rows: page 1 = 20 → next = 6 → first page returns the same first row). | **locally verified** | Re-run the full gate after C01 lands |
| C07 | A1-EVIDENCE | Final source/artifact/proof identity consistent; coverage reevaluated; no hand-editing old proof HEAD/hash | Audit measured 32 diagnostics (26 proof-identity mismatches, 1 source digest mismatch, 5 missing domain proofs) | **open** (bookkeeping + identity policy) | Regenerate affected evidence by producing runs after the code lands; never edit a stored proof's HEAD/hash/timestamps |

## Task families (mandate sections)

| Task | Frozen IDs | Scope | Status | Next action |
| --- | --- | --- | --- | --- |
| A1-00 | R00-FREEZE-INVENTORY, R00-FINDINGS-RECHECK | Candidate snapshot + authoritative worklist + this matrix; correct superseded claims | **in progress** | Finish this file + supersede the OWN-V1 "delivered" claims in the D-docs/ADRs |
| A1-AUTH-01 | R01-AUTHORITY-POLICY-SEAM | Real operator identity, logout revocation, cookie flags, CSRF | **locally verified** (C02 closed) | Re-run the full gate after C01 lands |
| A1-AUTH-02 | R01-NONOWNER-RLS-COHORT, R01-AXUM-HOST | Authorization on every real entry point; service-human intersection; read-grant receipt reads | locally verified (host) | Re-verify after C01/C02 land |
| A1-AUTH-03 | R01-AUTHORITY-POLICY-SEAM | Confirmed, typed, durable business execution; approval consumed once | partially (followup.create) | Extend with the six new commands |
| A1-CRM-01/02/03 | R03-CONTACTS-PIPELINE, R03-IDENTITY-SESSIONS-CSRF, R03-CONFIRMATION-RECEIPTS, R03-NEGATIVE-EVIDENCE | Seven writes, projections/list usability, operational UI + assistant | **open** (1/7 writes; read-only UI) | C01 closure |
| A1-RECOVERY | R03-CONFIRMATION-RECEIPTS | Unknown outcomes, app-owned pending marker, UI state safety | **locally verified** (C05 closed) | Re-run the full gate after C01 lands |
| A1-UI | R01-LEPTOS-UI, R09-SDK-WIDGET-PAGE-AWARENESS | Query encoding, pagination, states, a11y, SSR/hydration, asset release | **locally verified** for encoding/pagination (C06); a11y/asset release ride the existing proofs | Re-run the full gate after C01 lands |
| A1-OWN-01/02/03 | OWN-T01…OWN-T10 | Owner configuration lifecycle, module availability puzzle, exact OWN-T semantics | **open** | C03 closure |
| A1-REUSE | R02-PARITY-BOUNDARY, R05-TWO-CONSUMERS, R06-TWO-CONSUMERS | Module contract, two-consumer CRM proof, assembly refusal | implemented (core reuse) / **open** (CRM behaviour) | C04 closure |
| A1-ENGINES | R02-PRICING-PORTED, R02-RATES-PORTED, R02-PARITY-BOUNDARY | Pricing/rates/planning/capacity as separate measured surfaces | locally verified | Re-verify only if C01 touches them |
| A1-INTERFACE-01/02/03 | R09-CONTEXT-KNOWLEDGE-DECISION, R09-PROVIDER-CONTRACTS, R09-SDK-WIDGET-PAGE-AWARENESS | Context/knowledge/decision, provider wiring, SDK/widget/media | implemented (contracts, widget, synthetic voice) | Re-verify after UI changes; keep vendor-bridge limits explicit |
| A1-ERP-INTEGRATION | R04…R08 items, G04-DOMAIN-INTEGRITY | Integrate Archxy 2 packets onto real shared files; no duplicate domain work | **in progress — PACKET-003 locally verified (27/27 host witnesses, P-ERP-R04 producer green); 004–009 open** (9 packets on hand) | Fold PACKET-004→009 in order, then C04/T01, OWN evidence and the final gate |
| A1-OPS-01/02/03/04 | R10-* , R11-* | Operator tooling, migration compatibility, writer fencing/cutover rehearsal, observability | implemented (CLI, upgrade/restore rehearsal) | Re-verify after changes |
| A1-EVIDENCE-01/02/03 | G01…G10 | Traceability, required verification families, gate order + identity seal | **open** | C07 closure after code lands |

## Ownership boundary in force

| Area | Owner |
| --- | --- |
| Shared core/contracts, identity/session, authority/approval/receipt/idempotency, tenant transactions/fixtures | Archxy 1 (this lane) |
| CRM all existing behaviour + Leptos CRM UI + owner assistant execution integration | Archxy 1 |
| Pricing/rates, assembly/planning, capacity, context/knowledge/decision, SDK/widget/providers/media | Archxy 1 |
| OWN-V1 plan/configuration engine, owner dashboard/chat, runtime capability enforcement | Archxy 1 |
| Organizations/proposals/projects/capacity persistence, scheduling/accommodation/assets, commerce/purchasing/foodservice, billing/subscriptions/attendance/support/fulfillment, laundry/field/education | Archxy 2 (domain lane R04–R08) |
| Root manifests/locks, shared exports/router/AppState/readiness/fixtures, main UI shell, config/release/coverage registries, CI/gates, final integration | Archxy 1 (single integration writer) |
| Independent final audit/acceptance | Codex — neither implementator self-signs |

Handoff directory: `/home/haikaru/Archverse/Lab/Archava_ERP_Handoff/`
(Archxy 2 writes `archxy2/`, this lane writes `archxy1/` plus the main
checkpoint). On-hand Archxy 2 packets: PACKET-000…PACKET-009 (latest 009 attendance,
20 Oktober), a shared-file patch sequence whose `a/` sides were cut against
`c96abf0` — integration must be semantic, not a blind `patch -p1`.

## Dirty-tree discipline (in force)

Owner-dirty paths are not swept into any commit: `.github/workflows/ci.yml`,
`PRD.md`, `package.json`, `pnpm-lock.yaml`, `apps/crm/src/*`,
`apps/crm/package.json`, `apps/crm/public/index.html`, `packages/config/*`,
`tools/library/*`, `tools/scripts/verify.mjs`, `tools/security/verify.mjs`,
`vitest.config.ts`, `docs/BUILD_STATUS.md`, `docs/LOCAL_LIBRARY_*.md`,
`docs/STARTER_KIT_IMPLEMENTATION.md`, `docs/audit/local-library/*`,
`tools/rust/benchmark/evidence/benchmark.json`, and the untracked
`ARCHXY_*` task files, `docs/ARCHXY_1_CRM_AUDIT_2026-10-19.md`,
`docs/audit/2026-10-19-archxy1-crm-audit.json`, `packages/formal-education*`,
`tools/library/verify-formal-education*.mjs`. This lane commits only exact
owned paths: `rust/**`, `tools/rust/**`, `tools/own/**`, `docs/rust/**` and new
`docs/rust/ARCHXY1_*` files.

## Evidence notes (2026-10-20)

- **Fixture destruction by a concurrent lane, not a product failure.** A
  35-test serial CRM run reported 4 failures
  (`a_contact_create_over_http_persists_the_row_and_its_audit`,
  `a_principal_without_a_crm_membership_is_refused`,
  `a_reviewed_followup_create_persists_the_real_effect`,
  `a_search_is_escaped_so_the_caller_percent_stays_a_literal`), each a
  `login … 503`/vanished fixture directory; all four pass alone and the other
  31 passed. The cause is now named exactly: the Archxy 2 lane runs
  `pkill -x postgres; sleep 3; pkill -9 -x postgres; rm -rf
  /tmp/archava-pg-fixture-* /home/haikaru/Archverse/tmp-fixture/archava-pg-fixture-*`
  before its own ERP battery (observed in its shell command line), which kills
  every PostgreSQL and deletes every fixture on the machine — including a
  browser proof in flight (`Refused: identity` mid-journey at step 11 with the
  fixture's data directory gone). A gate run must not share the machine with
  that lane; a failed run under it proves nothing about this code.

- **PACKET-003 runtime half folded locally (2026-10-20).** The R04 ledgers
  (organizations, proposals, projects) are copied from the ERP handoff and the
  shared router/fixture/proof files are folded semantically: six routes, three
  migration/ledger descriptors + grants, nine module files, three host suites.
  Recorded runs: `cargo test -p archava-domain --offline` → 50 passed;
  `cargo test -p archava-adapters --offline` → 11 passed;
  `erp_organizations` 9/9 (168.82s), `erp_proposals` 9/9 (171.99s),
  `erp_projects` 9/9 (167.18s); `node --test
  tools/rust/test/verify.test.mjs` → 26/26; `cargo fmt --all --check` → 0.
  `P-ERP-R04` is wired (producer + 23 checks + P-FAST-GATE step) and the
  quiet-window producer run passed at 18:23:13: organizations 9/9 (81.7s),
  proposals 9/9 (69.2s), projects 9/9 (66.8s), 27/27 witnesses, 23/23 checks,
  `dist/rust-erp-r04-proof.json` `allPassed: true`. The earlier failures were
  the ERP lane's `pkill -x postgres` (the failed projects witness passes
  alone 1/1, 26.98s), not the suite.

- **C01 command coverage is 7/7 AND the product surface is now built.** The
  audit's sentence "angka 1/7 adalah command coverage" still stands as a
  warning, not as a status: the commands, the reads, the contact create form,
  the record screen with pipeline history, and the visible assistant desk are
  all driven by the browser proof, with their effects asserted in PostgreSQL.
  A standalone pipeline BOARD (a board view across contacts) does not exist;
  the pipeline read lives in the desk's pipeline answer and the record
  screen's per-contact moves/history, which is what A1-CRM-03 asks for
  ("pipeline create/move/history").

- **The desk found a real assistant bug (2026-10-20).** Driving the panel
  after the record journey renamed Ada to "Ada Lovelace (edited)" (21
  characters) made `POST /api/crm/{tenant}/conversation` answer `503`:
  `ConversationData::verified()` applied the schema's bounded-collection
  ceiling (`max(20)`) to a contact's NAME length and to a draft's summary
  length. The ceiling belongs to arrays only; a prepared completion must
  instead be an OPEN task with a literal `requiresApproval: true`. Fixed in
  `rust/crates/assistant/src/contracts.rs` and pinned by four unit tests; the
  assistant integration suite is 13/13 and the browser run then completed the
  desk journey. Without the product-surface proof the bug was invisible to
  every unit and route test.

- **C05 extended to the record screen (2026-10-20; browser proof pending).**
  `contact.rs` now writes the app-owned marker before EVERY record write,
  refuses a NEW intent while one is unresolved (`pending_unresolved`, with a
  recovery link), keeps the retry of the SAME key allowed, and clears the
  marker on known outcomes. The harness witness
  `record_pending_marker_blocks_new_intent` is added; the browser run is
  pending because the Archxy 2 lane is executing `pkill -x postgres` and
  workspace tests on the same machine. Not claimed as verified until that
  run is green.

- **C03 engine + persistence + API + UI landed (2026-10-20; lifecycle
  locally verified, T01/T02/T10 still open).** The leaf crate
  `archava-own` is now stateless-plan capable (`Catalogue::plan`, the host's
  transactional caller) and models `mandatory` modules: disabling one yields
  a `mandatory` blocked plan. It still provides typed catalogue/config/
  request, immutable plans with a stable identity, explicit blocked results,
  confirmation bound to plan hash + base revision, idempotent apply,
  append-only history and restore-as-a-new-revision revalidated against the
  current catalogue. `cargo test -p archava-own` → **13/13**.
  Persistence is real: Rust-owned ledger `archava_owner`
  (`rust/crates/host/migrations/owner/001_owner_config.sql`) carries separate
  `config.read`/`config.apply` grants, revision + receipt + audit rows,
  append-only grants (no UPDATE/DELETE to the runtime) and forced RLS keyed
  by `archava.tenant_id`; the fixture migrates/grants it and
  `AppState::try_new_owner` asserts its checksum-exact readiness.
  `rust/crates/host/src/own.rs` serves `GET /own/config`,
  `POST /own/plan`, `POST /own/apply`, `POST /own/restore/plan`,
  `POST /own/restore`, `GET /own/history`: one transaction per apply
  (grant → advisory tenant lock → duplicate-receipt replay/conflict → current
  revision → catalogue plan → blocked/confirmation checks → revision +
  receipt + audit), row/JSON revision cross-checks, and a fail-closed
  `owner_unavailable` when the ledger cannot be read.
  The CRM backend now refuses `opportunity.create`/`opportunity.move` with
  `module_disabled` when the persisted config disables `crm-pipeline`
  (before any claim/write), the record read omits transition history and
  reports `pipelineEnabled: false`, the assistant's overview omits its
  pipeline section and the `tampilkan peluang` tool answers 404 while the
  module is disabled (re-enabling restores both), and the shipped `/app/owner`
  page drives inspect → plan → preview → confirm → applied → history/restore
  against those routes. Proof: `cargo test -p archava-host --test own --offline --
  --test-threads=1` → **8 passed, 0 failed, 140.98s** (real HTTP + real
  PostgreSQL + real sessions/CSRF): disable → backend `module_disabled` →
  enable → `opportunity` receipt; duplicate request id replays one receipt
  with one effect; stale base and foreign confirmation refuse without
  writing; missing and revoked grants refuse; restore lands as revision 3
  while history `[3,2,1]` and business contacts stay intact; racing applies
  converge to one revision/receipt/audit; a rebuilt serving state replays the
  SAME receipt from PostgreSQL with no in-memory carry; a mandatory module is
  blocked with nothing written. The assistant gate has its own witness
  (`disabling_the_pipeline_module_removes_the_tool_and_zeroes_the_counts`,
  one real fixture + owner apply): enabled → the pipeline tool answers; the
  persisted disable → the tool answers 404 and the overview has no pipeline
  section; the persisted enable → both return. It passes alone (18.99s).
  The same browser proof executed the shipped `/app/owner` journey as a real
  smoke (not a frozen P-OWN witness): disable `crm-pipeline` → the record
  hides the pipeline → re-enable → restore revision 1 → final enable, with
  PostgreSQL holding exactly four revisions and the pipeline enabled at the
  end. This is UI evidence, not a claim on the P-OWN-T01/T02/T10 checks.
  Explicitly still open, and why C03 is not claimed closed: T01 (two
  packaged business-model consumers, shared CRM behaviour) and T02 (chat
  plan equals dashboard plan) are C04/assistant work not delivered here;
  T06's host-level restart/replay leg is now proven and the owner page's
  journey has been exercised in the browser proof, but the owner page has no
  lost-response UI witness and the frozen OWN evidence producer
  (`tools/own/verify.mjs`) has NOT been re-bound to this engine, so no
  P-OWN-Txx evidence is claimed. The final gate has not been run at this
  HEAD.

- **C07 reevaluated honestly (2026-10-20; still open).** The frozen
  manifest is untouched (93 items / 53 proofs). A read-only
  `validateCoverage` run against live HEAD `91341cd` reports **94 errors**
  (`VALIDATOR_EXIT=3`; 97 before the wasm producer re-ran — see below):
  - **5 missing evidence documents** — `P-R04-DOMAIN` … `P-R08-DOMAIN`,
    exactly the Archxy 2 packet-blocked domain proofs, not a coverage gap
    this lane can fill without the ERP packets;
  - **39 stale identity errors** across three earlier local heads
    (14 at `409eab9`, 25 at `94307ed`, 1 at `6610236`), i.e. evidence from
    progressive milestones — legal history, but not the final identity;
  - **25 proofs with source drift**, including every `P-OWN-Txx`: the new
    `archava-own` crate now falls inside the frozen `sourcePaths` glob, so
    the recorded digests no longer match. That is the validator working as
    designed; the answer is a regenerated gate at the final HEAD, never a
    hand-edited head/hash.
  Running the DB-free producer `node tools/rust/wasm-boundary.mjs` after
  that first measurement regenerated `P-WASM-PORTABLE` at the current HEAD
  (its three errors disappeared: 97 → 94) — the correct direction of repair.
  C07 cannot close until the full release gate runs at the final HEAD and
  the five ERP domain proofs are produced.
