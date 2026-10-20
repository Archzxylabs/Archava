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
| C01 | A1-CRM | All seven baseline writes, existing reads and usable CRM screens/action flow — not `followup.create` plus a read-only assistant | `rust/crates/host/src/crm.rs` `CrmCommand` has ONE variant (`followup.create`); slice route table has login/contacts/draft/recover only; `tests/crm.rs` even asserts `contact.create` is refused (422) | **open** — implemented for 1/7 writes | Implement `contact.create`, `contact.update`, `activity.add`, `followup.complete`, `opportunity.create`, `opportunity.move` with the TS wire contracts, current authority, atomic audit/receipt; then the product UI and assistant execution |
| C02 | A1-AUTH | UI logout revokes the server session and closes cookie/bearer/reconnect authority | `rust/crates/slice/src/session.rs` sign-out calls `forget_session()`/`clear_session_facts()` only; no logout route in `router.rs`; audit repro: post-sign-out `GET /contacts` → HTTP 200 with 20 rows | **open** | Server logout endpoint (CSRF/origin policy), session revocation + cookie expiry, UI wiring, and HTTP/UI/restart regressions proving the old cookie cannot read/mutate/recover |
| C03 | A1-OWN | Actual owner configuration lifecycle and semantic OWN-T01–T10 proofs | `tools/own/verify.mjs` derives T01 from a policy-consumer `outcome.ok`, T06 from writer-fence/rollback rehearsals, T07 from deployment-plan unit tests, T10 from the TypeScript CRM browser battery — none exercise an owner config apply/restore | **open** (audit says OWN-V1 "delivered" is not supported) | Build the real config lifecycle (inspect→plan→preview→confirm→apply→recovery→history→restore) as one serving engine for chat+dashboard, then re-bind each OWN-Txx to the operation it names |
| C04 | A1-REUSE | Two packaged independent consumers execute real CRM business behaviour on isolated persisted data | `tools/rust/k05-k04-consumers.mjs` bundles `archava-core`/`archava-domain`; consumer witnesses cover tenant IDs, role table, action registry, `CommandAuthorizer` — no CRM repository/operation runs | **open** | Extend both consumers to run contact/activity/follow-up/opportunity read-write-recovery against their own persisted store, from identical released CRM artifacts |
| C05 | A1-RECOVERY | App-owned pending identity survives reload/unknown outcome and blocks accidental new intent | `rust/crates/slice/src/draft.rs` holds the request UUID in reactive memory only and mints a new one per mount; audit repro: request ID changes across reload, original marker absent from tab storage/URL | **open** | Persist an account/workspace-bound opaque pending marker BEFORE the submission, restore it after reload, gate new intents while unresolved, distinguish absent/unconfirmed from failed |
| C06 | A1-UI | Literal query encoding, functional cursor pagination and reset/error/race semantics in the browser | `rust/crates/slice/src/contacts.rs` concatenates `search_term` raw (audit: `Ada&limit=1` becomes another query parameter; UI shows `Ada Lovelace` while the encoded API search returns 0 rows); 29 contacts → UI shows 20 with "a further page exists" text and no next-page action | **open** | URL-encode query values, add real cursor navigation with reset-on-search/workspace-change and stable ordering, separate loading/empty/refused/unknown states |
| C07 | A1-EVIDENCE | Final source/artifact/proof identity consistent; coverage reevaluated; no hand-editing old proof HEAD/hash | Audit measured 32 diagnostics (26 proof-identity mismatches, 1 source digest mismatch, 5 missing domain proofs) | **open** (bookkeeping + identity policy) | Regenerate affected evidence by producing runs after the code lands; never edit a stored proof's HEAD/hash/timestamps |

## Task families (mandate sections)

| Task | Frozen IDs | Scope | Status | Next action |
| --- | --- | --- | --- | --- |
| A1-00 | R00-FREEZE-INVENTORY, R00-FINDINGS-RECHECK | Candidate snapshot + authoritative worklist + this matrix; correct superseded claims | **in progress** | Finish this file + supersede the OWN-V1 "delivered" claims in the D-docs/ADRs |
| A1-AUTH-01 | R01-AUTHORITY-POLICY-SEAM | Real operator identity, logout revocation, cookie flags, CSRF | implemented (host) / **open** (UI logout, revocation) | C02 closure |
| A1-AUTH-02 | R01-NONOWNER-RLS-COHORT, R01-AXUM-HOST | Authorization on every real entry point; service-human intersection; read-grant receipt reads | locally verified (host) | Re-verify after C01/C02 land |
| A1-AUTH-03 | R01-AUTHORITY-POLICY-SEAM | Confirmed, typed, durable business execution; approval consumed once | partially (followup.create) | Extend with the six new commands |
| A1-CRM-01/02/03 | R03-CONTACTS-PIPELINE, R03-IDENTITY-SESSIONS-CSRF, R03-CONFIRMATION-RECEIPTS, R03-NEGATIVE-EVIDENCE | Seven writes, projections/list usability, operational UI + assistant | **open** (1/7 writes; read-only UI) | C01 closure |
| A1-RECOVERY | R03-CONFIRMATION-RECEIPTS | Unknown outcomes, app-owned pending marker, UI state safety | **open** | C05 closure |
| A1-UI | R01-LEPTOS-UI, R09-SDK-WIDGET-PAGE-AWARENESS | Query encoding, pagination, states, a11y, SSR/hydration, asset release | **open** | C06 closure |
| A1-OWN-01/02/03 | OWN-T01…OWN-T10 | Owner configuration lifecycle, module availability puzzle, exact OWN-T semantics | **open** | C03 closure |
| A1-REUSE | R02-PARITY-BOUNDARY, R05-TWO-CONSUMERS, R06-TWO-CONSUMERS | Module contract, two-consumer CRM proof, assembly refusal | implemented (core reuse) / **open** (CRM behaviour) | C04 closure |
| A1-ENGINES | R02-PRICING-PORTED, R02-RATES-PORTED, R02-PARITY-BOUNDARY | Pricing/rates/planning/capacity as separate measured surfaces | locally verified | Re-verify only if C01 touches them |
| A1-INTERFACE-01/02/03 | R09-CONTEXT-KNOWLEDGE-DECISION, R09-PROVIDER-CONTRACTS, R09-SDK-WIDGET-PAGE-AWARENESS | Context/knowledge/decision, provider wiring, SDK/widget/media | implemented (contracts, widget, synthetic voice) | Re-verify after UI changes; keep vendor-bridge limits explicit |
| A1-ERP-INTEGRATION | R04…R08 items, G04-DOMAIN-INTEGRITY | Integrate Archxy 2 packets onto real shared files; no duplicate domain work | **open** (8 packets on hand) | Apply PACKET-003→007 in order onto current main, run affected proofs |
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
checkpoint). On-hand Archxy 2 packets: PACKET-000…PACKET-007 (last written
19 Oktober 05:46), a shared-file patch sequence whose `a/` sides were cut
against `c96abf0` — integration must be semantic, not a blind `patch -p1`.

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
