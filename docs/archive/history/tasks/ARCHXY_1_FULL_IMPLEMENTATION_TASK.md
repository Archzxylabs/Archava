# Archxy 1 — mandat lengkap sampai final local candidate

**Versi aktif: 20 Oktober 2026, setelah audit CRM C01–C07.**

Bro, lu **Archxy 1**, implementator core/CRM, interface Archava, owner
configuration dan integrasi akhir. Kerjakan seluruh scope yang didefinisikan
di sini sampai working local candidate, self-reviewed dan verified. **Codex
melakukan audit independen setelah candidate lengkap**, bukan menjadi gate
lanjut setiap milestone. Jangan berhenti setelah audit fixes atau satu slice.

Workspace utama: `/home/haikaru/Archverse/Lab/Archava_Platform`.
Archxy 2 mengerjakan domain ERP di working copy terpisah. Platform tetap satu
modular monolith dan reusable library; dua implementator bukan dua produk.

**File ini entry point dan execution checklist aktif Archxy 1.** Ia menggantikan
urutan/status/pembatasan scope dalam prompt historis. Product/security/data
contracts di PRD tetap berlaku. Instruksi sistem dan instruksi owner/AGENTS yang
lebih baru tetap di atas file ini. Jangan memakai ADR buatan implementator untuk
menurunkan requirement owner, memindahkan whole feature ke TS atau mendefinisikan
ulang arti PASS.

## 1. Target yang harus terasa sebagai produk

Owner dapat menjalankan Archava lokal, login/logout, memilih workspace yang sah,
mengelola CRM, bertanya kepada Archava, meninjau dan mengonfirmasi supported
actions, serta memulihkan outcome setelah gangguan. Owner dapat mengubah supported
configuration melalui chat/dashboard: inspect → plan → preview → confirm →
actual apply → recovery/history → compatible configuration restore.

Core/CRM yang sama dipakai K05 agency dan K04 wholesale tanpa source fork.
Optional installed capabilities bisa diaktifkan/dinonaktifkan dengan dependency,
entitlement, permission, resource scope dan data retention yang benar. ERP dari
Archxy 2 dipasang ke shared host/UI/authority dan release yang sama.

Target bahasa: **first-party core, business runtime, backend, operational tooling
dan web views/interactions memakai Rust**, sesuai frozen inventory R00–R11 dan
OWN-V1. Axum/Tokio, PostgreSQL/SQLx, Leptos SSR/WASM serta pinned versions existing
dipertahankan. HTML/CSS/SQL/JSON, generated JS bootstrap dan thin vendor/browser
compatibility bridges mengikuti explicit PRD boundaries. Build/test tooling boleh
menggunakan Node; shipped business engine atau full first-party Python worker
tidak berubah menjadi bridge hanya dengan mengganti label.

Website company ARCHZXY dibuat terpisah dan tidak masuk tugas ini. Existing
Archava application/demo/widget routes yang masuk frozen inventory tetap harus
punya disposition; jangan menghapusnya dengan alasan company website terpisah.

## 2. Baseline, sumber wajib dan scope yang dibekukan

Baca actual source dan dokumen berikut sekali, lalu buka detail yang relevan
ketika mengerjakan itemnya:

1. [Audit terbaru C01–C07](docs/ARCHXY_1_CRM_AUDIT_2026-10-19.md) dan
   [machine evidence](docs/audit/2026-10-19-archxy1-crm-audit.json).
2. [Master PRD](PRD.md) dan [PRD Full Rust](docs/PRD_FULL_RUST.md): seluruh required contracts,
   migration/cohort/vendor boundaries, roadmap R00–R11 dan gates G01–G10.
3. [Owner configuration PRD](docs/OWNER_CONFIGURATION_PRD.md): OWN-01–OWN-08,
   bounded OWN-V1 dan OWN-T01–OWN-T10.
4. `docs/rust/MIGRATION_INVENTORY.md`, `migration-inventory.json`,
   `required-coverage.json`, `EXECUTION_STATE.md`, `KNOWN_DIFFERENCES.md` dan
   `DECISIONS.md`; paths selain yang lengkap juga di bawah `docs/rust/`.
5. [Project audit A01–A09](docs/PROJECT_AUDIT_2026-10-14.md),
   [KIT09 J findings](docs/ARCHXY_KIT09_REVIEW_2026-10-15.md),
   [RN audit](docs/ARCHXY_RUST_AUDIT_2026-10-17.md),
   [late reaudit](docs/ARCHXY_RUST_REAUDIT_2026-10-16_LATE.md) dan relevant
   RP/RF/RA history yang dirujuk dokumen tersebut.
6. [CRM module](docs/CRM_MODULE.md), [pipeline](docs/CRM_PIPELINE.md),
   [overview](docs/CRM_OVERVIEW.md), [worklist](docs/CRM_WORKLIST.md),
   [completion](docs/CRM_COMPLETION.md), [recovery](docs/CRM_COMMAND_RECOVERY.md),
   local runtime/provider/native-action docs dan source/test implementations.
7. [Archxy 2 full task](ARCHXY_2_FULL_IMPLEMENTATION_TASK.md), current ERP checkpoint,
   integration requests dan latest shared handoff. Existing full execution task
   menyimpan historical contracts; file ini yang menentukan active ownership.

Snapshot audit 19 Oktober: HEAD `409eab9…`, profile `1.18.0`, 27 CRM + 13 assistant
tests PASS, tetapi incomplete CRM/OWN-V1 dan seven C findings. **Itu snapshot,
bukan status wajib saat lu mulai.** Verifikasi current diffs dan existing fixes;
jangan mengulang pekerjaan valid atau mengasumsikan angka test/artifact lama.

### A1-00 — Establish current candidate dan authoritative worklist

- Catat HEAD/branch, tracked + dirty + untracked public source, config/locks/
  migration checksums, artifact identities, tool versions dan active processes.
  HEAD saja belum tentu mencakup seluruh pekerjaan valid. Jangan reset/stash/
  clean/amend/relocate produk atau menimpa baseline orang lain.
- Existing frozen behavior inventory adalah baseline; OWN-V1 adalah explicit
  product delta. Map existing source/API/UI/worker/CLI/SDK surfaces ke target,
  consumer, data owner/cohort, required behavior tests dan proof IDs.
- Buat/update `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md` dengan task IDs file ini,
  links ke frozen IDs, behavior, API/UI/library location, negative cases,
  producing command/check, candidate identity, actual status dan next action.
  Ini traceability index, bukan denominator pengganti yang lebih kecil.
- Koreksi current report/checkpoint/ADR yang menyatakan C findings atau OWN-V1
  selesai tanpa bukti. Preserve history; tandai superseded/disputed claims.
  Tidak perlu menulis ulang audit Codex atau menghapus counterexample.
- Status terpisah: implemented → locally verified → integrated verified;
  independent review tetap pending. Marker heading, jumlah files/LOC, check name
  atau JSON `pass` tidak menjadi bukti behavior.

## 3. Ownership dan kontrak kerja dengan Archxy 2

| Area                                                                                                                                                                                                              | Owner                                             |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------- |
| Shared core/contracts, identity/session, authority/approval/receipt/idempotency infrastructure, tenant transaction/fixture                                                                                        | Archxy 1                                          |
| CRM all existing behavior + Leptos CRM UI + owner assistant execution integration                                                                                                                                 | Archxy 1                                          |
| Pricing/rates, assembly/deployment planning, capacity math, context/knowledge/decision, SDK/widget/providers/media                                                                                                | Archxy 1                                          |
| OWN-V1 plan/configuration engine, owner dashboard/chat flow, runtime capability enforcement                                                                                                                       | Archxy 1                                          |
| Organizations/proposals/projects/business capacity persistence, booking/occupancy/assets, commerce/purchasing/foodservice, billing/subscriptions/attendance/support/fulfillment, laundry/field-services/education | Archxy 2 domain lane R04–R08                      |
| Root manifests/locks, shared exports/router/AppState/readiness/fixtures, main UI shell, config/release/coverage registries, CI/gates and final integration                                                        | Archxy 1, single integration writer               |
| ERP namespaced implementations/domain tests/docs                                                                                                                                                                  | Archxy 2; Archxy 1 reviews and integrates packets |
| Final local candidate/package/operations/proofs/audit handoff                                                                                                                                                     | Archxy 1, consuming actual ERP evidence           |
| Independent final audit/acceptance                                                                                                                                                                                | Codex; neither implementator self-signs           |

Shared handoff: `/home/haikaru/Archverse/Lab/Archava_ERP_Handoff/`.
Archxy 2 writes `archxy2/`; lu writes only `archxy1/` and main checkpoint.
Read counterpart packets at each relevant checkpoint. Do not assume another
session automatically reads messages/files. Record packet ID, baseline/current
hashes, requested interfaces, landed diff, conflicts, commands/exits and status.

**A1-01 — Continuous semantic integration:** inspect packet content against
current source, integrate registrations/dependencies/migrations/handlers/views
onto actual main implementations, then run affected proofs. Do not copy stale
root locks/router/fixture wholesale. Existing packets can supersede earlier ones;
do not report “packets absent” without checking latest handoff. Mark component
proof, integrated host proof, domain completion and reviewer acceptance separately.

Do not duplicate ERP domain work, silently take over owned paths or create a
second stock/auth engine. Shared-contract corrections remain lu punya; send
domain counterexamples to Archxy 2 via handoff and proceed with independent work.
Business capacity uses the same math engine but its pools/allocations/queue data
belong to ERP. Domain-specific data invariants remain intact during integration.

## 4. Correction queue C01–C07 — wajib ditutup secara nyata

| Finding / task    | Closure wajib                                                                                                                                                  |
| ----------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C01 / A1-CRM      | All seven baseline writes, existing reads and usable CRM screens/action flow; not just `followup.create` plus read-only assistant                              |
| C02 / A1-AUTH     | UI logout revokes server session and closes cookie/bearer/reconnect authority; old credentials cannot continue reading/mutating/recovering                     |
| C03 / A1-OWN      | Actual owner configuration lifecycle and semantic OWN-T01–T10 proofs; remove the interpretation that unrelated component tests deliver the feature             |
| C04 / A1-REUSE    | Two packaged independent consumers execute real CRM business behavior on isolated persisted data from identical reusable artifacts                             |
| C05 / A1-RECOVERY | App-owned pending identity survives reload/unknown outcome and blocks accidental new intent until resolution; driver variables do not substitute for app state |
| C06 / A1-UI       | Literal query encoding, functional cursor pagination and reset/error/race semantics in the actual browser                                                      |
| C07 / A1-EVIDENCE | Final source/artifact/proof identity consistent; actual coverage reevaluated, no hand-editing old proof HEAD/hash to make it current                           |

Keep proven RN01–RN08/RF/RA corrections: current-grant status/recovery, actor/input
fingerprint under races, non-owner checked ledgers/readiness, default KDF budget,
bounded capacity arithmetic, real fixture execution and format/Clippy. Reopen only
when current evidence fails. Do not weaken a shipped limit to make tests pass.

A/J findings need explicit source-specific disposition. A01 replay permissions,
A02 assigned scope, A03 service/human intersection, A04 roster relationships,
A05 paid entitlement, A06 minimal closure, A07 release identity, A08 reference
authority and A09 docs cannot all be dismissed as “TS-only” when the target
reproduces those same behaviors. Archxy 2 fixes domain-owned rules; lu owns shared
enforcement, packaging/reporting/gates and integrated regressions. J01 clean unit
bootstrap, J02 complete failure reasons, J03 truthful current docs and J04 lint
coverage remain applicable. Historical bugs are corrected, not golden parity.

## 5. A1-AUTH — identity, tenant, grants dan action authority

### A1-AUTH-01 — Real operator identity dan logout

- Local operator can provision/manage actual identities/workspace grants through
  supported Rust tooling. Synthetic fixture users/passwords/HMAC constants must
  stay fixture-only, never become production bootstrap defaults.
- Verify password encoding/parameters, session mint/expiry/revocation, tenant
  membership, cookie flags/path/origin and CSRF. Credentials/secrets never travel
  in browser DTOs, logs, URLs or provider prompts. Private config remains private.
- Implement server logout and UI wiring. After logout, old cookie and bearer
  cannot access contacts, assistant, commands, receipt recovery or reconnect.
  Clear browser state/pending private projections, ignore late responses, handle
  multiple tabs/restart correctly. Missing/expired session is distinct from an
  empty dataset. Preserve valid current sessions according to migration policy.
- Update old browser CSRF negative: test a missing/forged CSRF with a valid
  session. Do not preserve insecure sign-out behavior to keep that witness green.
- Keep password work bounded and overload/rate-limit outcomes explicit. No public
  production setter or hidden test override that changes the shipped KDF limit.

### A1-AUTH-02 — Authorization on every real entry point

Resolve identity server-side; check current membership, granular operation grants,
module/capability entitlement, assigned/resource relationships and tenant ownership.
Valid slugs, broad role, authenticated session or a visible button are insufficient.
Service on-behalf-of authority intersects current service and human permissions
**and** scopes. Browser/model identity/role/approved flags cannot grant anything.

Check list/detail/child/audit/tools, mutation, confirmation, replay/status/recovery,
CLI/workers/webhooks as applicable. Membership/resource changes while waiting on
locks cannot widen authority. Non-owner PostgreSQL and forced RLS are required;
application pools cannot bypass tenant transactions. No server-function bypass.

Read-only receipt recovery uses current required **read** grants and resource
scope according to its contract; do not blanket-require write permission where
the baseline explicitly permits a former writer to read a receipt. Mutation replay
rechecks current write authority. Bind actor/initiator and tenant on both paths.

### A1-AUTH-03 — Confirmed, typed, durable business execution

Prepare/plan is read-only. Server-issued approval binds principal/initiator,
tenant, action, canonical input, target/resource revision, expiry and policy.
Current approver/delegation revocation policy is explicit and tested. Consume
approval/intent once in the correct transaction boundary. Revalidate on execute.

Every declared supported action runs its real operation before returning an
authoritative receipt. A stored opaque payload/acknowledgement in a generic
commands table is not `product.read`, `admin.config.update` or CRM execution.
Reject unavailable actions explicitly; do not register them as operational.
Audit/claim/domain effect/receipt are atomic. External deliveries use bounded
outbox/idempotency/reconciliation, not a universal exactly-once promise.

## 6. A1-CRM — complete existing reusable CRM

### A1-CRM-01 — All seven baseline commands

Preserve source wire contracts/errors, supported roles and transaction semantics.
If compatible routes change, provide and test the public compatibility boundary;
silently dropping fields/endpoints or retaining a TS runtime is not a port.

| Command              | Required implementation and proof                                                                                                                                               |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `contact.create`     | Validated full name/email/organization, server-owned tenant/owner/IDs/default revision, actual contact/audit/receipt; forbidden ownership overrides refused                     |
| `contact.update`     | Supported field allowlist, expected revision, current resource scope, atomic revision/audit update; stale edit cannot overwrite another writer                                  |
| `activity.add`       | Existing note/call/meeting vocabulary, authorized contact/revision, immutable activity and coherent parent revision/audit                                                       |
| `followup.create`    | Existing validation/due-time semantics, current contact revision, atomic task/contact/audit/receipt; preserve working behavior                                                  |
| `followup.complete`  | Exact task/contact/tenant association, open state and current revision, current permissions/assigned scope, one completion/audit; creation approval cannot authorize completion |
| `opportunity.create` | Authorized contact/revision, inherited owner, valid title/canonical initial stage, initial immutable transition/history and atomic receipt                                      |
| `opportunity.move`   | Expected revision, allowed transition graph, required lost reason, immutable stage history; won/lost terminal rules preserved, races have one winner                            |

Preserve lifecycle new → qualified → proposal → won/lost with the exact supported
backward transitions from source; do not infer the table above replaces the full
graph. No invented finance/forecast fields, stage reopening or machine pipeline
writes without a verified new authority contract. No hardcoded client names.

### A1-CRM-02 — Reads, projections dan list usability

- Contacts: literal search, stable keyset cursor/limit validation, detail,
  activities and follow-ups with explicit bounded-history semantics.
- Exact follow-up lookup must reach an authorized task beyond the first detail
  page. Worklist supports existing open/overdue/upcoming filters, task/contact
  literal search, server `asOf`, due/UUID cursors including microseconds/ties.
- Overview and pipeline counts reflect authorized current facts. A missing grant
  is unavailable/omitted per contract, not fabricated zero. Opportunity list,
  detail and transition history preserve filters, consistency and scope.
- Read/draft tools use existing allowlists, minimal projections and current
  grants. Sensitive email/organization/owner fields do not enter public/model
  outputs merely because a human dashboard can read them. No implicit KB ingest.
- Include current access/readiness and actual command status for every supported
  command kind. Wrong actor/tenant/resource/malformed receipt never discloses data.

### A1-CRM-03 — Operational CRM UI and assistant

Implement visible Leptos login/logout/workspace states, contact search/next-page/
selection/detail/create/edit, activities, follow-up create/worklist/complete,
pipeline create/move/history, overview and assistant panel. Use actual APIs/data;
no placeholders, developer-only SQL seed requirement or hardcoded lists.

Configured business terminology/pipeline display labels may differ between K05
and K04; canonical stage IDs/outcomes/history stay stable. Custom pipeline stage
graphs/typed custom fields are OWN-V2 unless already in frozen supported behavior.

Assistant asks/reads/prepares through the same authorized services and displays
planner identity, draft/review/confirmation and authoritative outcome. Native
follow-up creation **and completion** use their separate action/consent/intent
contracts. Other machine writes remain refused unless explicitly supported.
Typing/selecting a contact or asking for completion never executes a mutation.

### A1-RECOVERY — unknown outcomes and safe UI state

Persist the minimal account/workspace-bound opaque pending marker **before** a
submission that can commit; storage failure prevents unsafe submission. Restore
pending state after reload and query authoritative status with original identity.
Do not persist secrets or unnecessary business payloads. Preserve current grants.

While outcome is unknown, prevent a fresh intent/key from silently repeating the
operation. Distinguish absent/unconfirmed from failed/not-committed. Resolve same
key/same input safely; changed input/actor conflicts. Logout/workspace switch clears
private views and ignores late replies; pending recovery policy remains explicit.
Cross-device inbox is not added as a new requirement where baseline is tab-scoped.

Test browser-side response loss after actual commit, reload **before clicking a
recovery link**, refresh/two tabs, backend restart and allowed restore recovery.
The application must retain/recover identity itself; an ID saved only in test
driver memory proves backend lookup, not product recovery.

## 7. A1-UI — general browser acceptance

Encode query values and paths with correct URL semantics. Test `&`, `+`, `%`,
`#`, Unicode and literal SQL wildcard characters. Cursor navigation actually loads
later rows, resets on search/workspace changes and respects stable ordering.

Handle loading/empty/refused/offline/unknown outcomes separately. Prevent stale
async responses from repopulating logged-out/changed tenant screens. Keyboard,
focus, labels/error association, announcements, mobile layout/local time and
safe text rendering must meet declared PRD usability targets. SSR/hydration must
not leak secrets, full hidden records or privileged tool registries. Provide
usable basic SSR/form/read failure behavior if hydration is delayed/unavailable.

Build/cache server and hashed browser assets as one compatible release. Verify
supported native/wasm feature boundaries; browser bundles do not include SQLx,
server credentials, privileged repositories or test fixture code. A screenshot
or successful WASM compile does not prove an end-to-end user flow.

## 8. A1-OWN — implement owner configuration as product behavior

### A1-OWN-01 — Shared engine and persisted configuration lifecycle

1. **Inspect:** authenticated owner sees current business profile, configuration
   revision, installed/enabled modules, dependencies, entitlement, own allowed
   operations and supported options with actual readiness/gaps.
2. **Discover:** chat asks for missing business intent and converts it to typed
   request. Dashboard submits the same contract. Neither gets privileged access
   or writes configuration directly from LLM output.
3. **Plan:** deterministic backend creates immutable proposed config/diff from
   current catalogue/profile/installed artifacts/readiness/pricebook. Bind source
   config revision and relevant catalogue/pricing/target resource identities.
4. **Preview:** show actual changes, dependency expansion/refusals, data retention,
   affected workflows, implementation/recurring/usage cost classifications and
   risk/approval requirements from authoritative data. No invented price/SLA.
5. **Confirm:** server-bound approval for the exact plan/actor/tenant/versions/
   expiry. Changed targets/config/catalogue/pricing require replan/reconfirmation.
6. **Apply:** recheck current permission, entitlement, dependencies, readiness and
   base revision; apply actual supported config once, with config version, audit
   and durable receipt. Failure/unknown outcome is not labelled applied.
7. **Recover:** same plan/intent survives duplicate/racing submits, browser reload,
   transport loss and host restart; current authorized status is authoritative.
8. **History/restore:** show prior config versions/diffs/actor/time/outcome. Restore
   is a new authorized compatible config transition; revalidate current bounds.
   It does not rewrite stock, posted finance, business records or audit history.

This must be one serving engine for chat and dashboard. A unit test comparing
`LocalCrmPlanner` to itself, a writer-fence rehearsal or a read-only assembly
plan does not implement the config management lifecycle.

### A1-OWN-02 — Actual module availability and bounded puzzle

Pick an already installed optional capability; **CRM pipeline** is the default
candidate if its actual contracts support the required boundary. Demonstrate
enable/disable and dependency refusal without waiting for unfinished ERP. If a
different installed supported module is required, record concrete reason and
same acceptance; do not ask owner to choose the first module again.

Config apply changes actual UI/navigation, available tools and backend API
behavior for that tenant. Direct disabled module operations refuse even when
caller skips the UI. Installed/enabled/paid entitlement/user grants/resource
scope are distinct facts. Enable never auto-enrolls users or grants authority.
Dependent disable refuses with an understandable dependency explanation.

Keep existing records/history and a separately authorized export/retention path
according to supported policy; disable is not uninstall/delete. Do not build a
universal workflow/EAV engine, dynamic native plugin loader or arbitrary app
generator. OWN-V1 configures existing supported capabilities only.

### A1-OWN-03 — Exact OWN-T acceptance, with producing evidence

| ID      | Required actual scenario; unrelated checks do not substitute                                                                                                                    |
| ------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| OWN-T01 | K05/K04 independently installed, same released core/CRM; owner changes supported labels/config through UI/chat without editing source/config files                              |
| OWN-T02 | Same configuration request through real chat/dashboard entrypoints produces equivalent validated plan/diff with matching authoritative base/catalogue/config/pricing identities |
| OWN-T03 | Actual preview shows changes/dependencies/readiness/cost; inspect/plan/preview produces no hidden activation/provider/payment side effect                                       |
| OWN-T04 | Missing management grant/public session/forged valid tenant-role-approval/revoked confirm-status-replay fail at actual config endpoints                                         |
| OWN-T05 | Missing/incompatible/unavailable dependency or unsupported workflow returns explicit blocked/gap; no runtime apply effect or success classification                             |
| OWN-T06 | Duplicate/racing apply and two-tab stale configuration yield one config transition; refresh/lost response/host restart recover the same outcome                                 |
| OWN-T07 | Optional module enable/disable changes actual UI/API/tools; disabled direct request and dependent disable are refused                                                           |
| OWN-T08 | Config changes/restoration preserve existing business rows/audit/receipts/grants and historical meaning; restore creates a compatible new config version                        |
| OWN-T09 | Stale config/catalogue/price/target snapshots force replan/reconfirmation; no invented price/readiness                                                                          |
| OWN-T10 | Real browser owner journey inspect → plan → preview → confirm → applied → recovery/history/restore, without owner code/file/terminal steps                                      |

Repair `tools/own/verify.mjs`, actual witness mapping, gate integration and related
ADRs/report claims. Preserve useful component tests under their true scope. A
passing TS CRM browser battery, Chromium source marker or two policy consumers
cannot be mapped to an unexercised configuration scenario. Add missing required
behavior/checks transparently; do not narrow the owner PRD to fit existing tests.

## 9. A1-REUSE — modules, assembly, two consumers

Module contract exposes stable ID/version, dependency/incompatibility constraints,
permissions, browser-safe DTO/API/events, configuration schema, UI/tool extension
points, env references, migration/schema/ledger compatibility, health/readiness
and upgrade proof requirements. Availability, readiness and commercial selection
are real backend facts, not package-name presence.

Use existing Rust module/crate structure and narrow transaction ports. Keep
domain behavior reusable outside the one demo host. Split crates only for real
dependency/reuse/build boundaries; no mechanical npm-to-crate rewrite, client
fork, dozens of services or wholesale folder renaming to match a proposal.

**Two-consumer CRM proof:** independently install K05 and K04 outside the product
workspace, with identical shipped core/CRM artifacts, migrations and compatible
web assets. Run actual contact/activity/follow-up/opportunity read/write/recovery
scenarios and business-model configuration on isolated persisted data. Minimal
declared dependency closure only, no private env/owner DB/source workspace import.
Tenant/role/action policy tests remain useful but are not a CRM behavior proof.

Assembly/config resolver validates dependencies/version bounds/migration readiness
and refuses incompatible combinations or disabled prerequisites. Two optional
modules must not require all unrelated ERP modules merely to import/build CRM.
New capability is composed/configured; no `if customerName` source branches.
Forty catalogue candidates retain exact gaps; two module consumers do not accept
two whole industry kits or all possible module combinations.

## 10. A1-ENGINES — preserve and complete pure engines R02

Keep pricing, reference rates, assembly/deployment planning and capacity math as
separate measured surfaces. Complete required conformance, error/numeric/rounding/
canonicalization boundaries, extreme time/overflow/horizon cases and cold consumer
closure. Corrected security/integrity expectation wins over a broken TS baseline.

Keep original fixtures/corrections counts traceable, not hardcoded as current
progress. A changed divergence/error class needs explicit compatible/versioned
disposition and consumer proof. Run fair compiled TS versus optimized Rust on
identical workload bytes; separate setup/steady state and record absolute
latency/CPU/RSS/throughput/build/browser costs under registered policies. Slower
or inconclusive is reported honestly; not every module needs a new benchmark.

## 11. A1-INTERFACE — remaining Archava capabilities R09

### A1-INTERFACE-01 — Context, knowledge, decision and public boundary

Preserve required page awareness, consent, graph/context projection, knowledge
retrieval, navigation/handoff/decision modes and no-fabrication rules. Public
visitor and owner/employee identities/tools/data stay separate. Hostile page/
retrieved content cannot change authorization, tool allowlists or backend truth.
No automatic ingestion of all CRM/customer/internal conversations into a KB.

### A1-INTERFACE-02 — Assistant/provider behavior actually wired

Rust serving orchestration consumes the actual planner/provider adapter contract,
validates output, applies current authority and projects trusted tool results.
Local deterministic planner remains usable when supported. A provider Rust type
or canned fake without serving integration does not close an inventory entry.

Preserve Gemini/free pilot choice, existing synthetic-only/privacy limits,
admission/budgets/rate limits, attempt ledger, quota/timeouts/cancellation and
unknown-outcome reconciliation. No silent paid/model/provider fallback. Secret
references/config stay server-side. Live availability/tier/model terms must be
verified from official sources before any already-authorized live invocation;
local acceptance uses owned protocol fixtures, not new paid calls.

Actual HTTP/SSE protocol adapters for required providers must work through local
fake endpoints with malformed/partial/racing/unknown-result cases. Do not retain
a whole first-party TS HTTP client as a required Rust business runtime merely
because no official vendor Rust SDK exists. Preserve existing restrictions on
real email/payment; never create external effects to fill evidence counts.

### A1-INTERFACE-03 — SDK/widget/media and supported host integrations

Implement first-party Rust view/interaction ownership and tested generated/thin
JS/TS-facing compatibility facade. Two independent host environments embed the
same shipped SDK/widget without requiring the client website to migrate Rust.
Test mount/unmount, CSS/router/focus isolation, trusted origins/postMessage,
page consent/context updates, stale sessions, disposal/reconnect and secret leaks.

Map each supported realtime voice/avatar/template flow and first-party worker
to actual serving replacement/allowed vendor bridge. Vendor JS renderers/WebRTC
SDKs may be explicit exceptions; whole React application or Python orchestration
is not automatically a thin bridge. Preserve session establishment, microphone
permission denial, mute/interrupt/disconnect, stream cancel/cleanup, timeout and
chat fallback. No own codec/avatar engine rewrite or paid live usage required.
Document unverified live limitations separately from protocol-local proof.

## 12. A1-ERP-INTEGRATION — accept actual domain packets, not labels

R04–R08 domain code remains Archxy 2's lane. Lu supplies shared extensions,
readiness, routes, sessions/CSRF, typed action/tool registration, main Leptos shell,
configuration availability, package/CLI/fixture/gate integration and same-candidate
proof collection. Domain UI component must be reachable and usable in the host.

Verify cohort completeness with its actual owner: organizations/scope/project/
capacity, scheduling/accommodation/assets occupancy, shared commerce/purchasing/
foodservice stock, billing/subscription/attendance/support/fulfillment and required
vertical extensions. Do not mark domain complete from a policy/readiness smoke.

One physical stock owner and same transaction for relevant receipts/allocations/
audit. Occupancy and financial/relationship cohorts follow the same principle.
No split TS↔Rust HTTP commits, dual writers or arbitrary cross-schema SQL bypass.
Runtime grants and capabilities require actual current upstream/related authority.

Wire required BUS/FLOW/CAP contracts across module boundaries: CRM opportunity/
confirmed requirement → deterministic quote/versioned agreed scope → explicit
acceptance/signature evidence → bound deposit request → authoritative settlement
→ capacity queue → delivery/review → warranty versus maintenance. Archxy 2 owns
the domain facts; lu owns shared orchestration/integration and actual UI/API flow.
Sales/agreement/payment/delivery states stay separate. Deposit policy is approved
configuration, not a hardcoded new 50% rule; estimates are not capacity promises.
Use local provider fixtures for duplicate/out-of-order/mismatched/failed/refund
events. Do not label browser/chat/manual references as verified provider payment
or claim legally executed contracts from drafts. Inventory each signature/payment
adapter honestly as existing/sandbox/manual/draft/not implemented per FLOW-06;
no real vendor effects or company website implementation are added here.

Domain local verification must run without requiring a prior Codex signature.
Independent review status is external metadata, not fake producing evidence or a
reason to skip tests/UI. If a current gate conflates local proof with reviewer
sign-off, repair the distinction transparently while preserving required scope
and proof strictness; do not self-fill an external sign-off.

## 13. A1-OPS — local operation, migration and recovery R10–R11

### A1-OPS-01 — Runnable product and actual operator tooling

Provide documented clean bootstrap/build/package/run/doctor/stop commands for an
isolated local profile. Support actual setup/users/grants/modules, assembly,
release verification, backup/status/policy/run, restore and action recovery that
exist in frozen inventory. A printed runbook/command mapping or exit-0 summary
does not replace the actual old CLI operation.

Operator administration remains separate from owner self-service UI. Users can
run the local release without disposable proof-only accounts or mandatory
first-party Node/Python daemons. Fixture examples stay separate from real setup.

### A1-OPS-02 — Data and migration compatibility

Preserve existing schema/ledger ownership, exact historical SQL/checksums, IDs,
FK relationships, revisions, key/fingerprint framing, timestamp microseconds,
sessions/approvals, business/audit/receipt history and pending outcomes. New SQL
is additive, ordered, versioned and registered across migrator/host/package.
Do not rewrite historical SQL or auto-enroll existing users on module enable.

Test fresh install **and** supported non-empty prior profiles, including old
command kinds/pending/approved/unknown states. Backup must restore into a fresh
explicitly checked target and preserve records/settings/key references without
printing secrets. Test wrong active target refusal, incompatible/partial ledger,
missing forced RLS and non-owner readiness. Owner DB is never a build/test target.

### A1-OPS-03 — Single writer, cutover and rollback rehearsal

Fence the actual persistence boundary per tenant/cohort across HTTP, CLI, tools,
workers/webhooks/jobs/recovery. Prove drain/handover and stale-writer denial with
actual domain writes, not just a lease-table demo. Reconcile pending outbox,
provider/approval/command outcomes before declaring safe switch.

Rehearse local cutover/restart/compatible rollback on synthetic non-empty profiles.
Do not downgrade DB or erase committed finance/stock/history. Active owner
runtime/data remains unchanged until separately authorized cutover. Preserve old
source/archives needed by that runtime or supported prior proof; remove obsolete
active candidate wiring only after verified replacement, not by deleting history.

### A1-OPS-04 — Observability and failure handling

Structured redacted logs/traces correlate request/intent/receipt and distinguish
authorization, validation, conflict, transport unknown and internal failures.
Record serving readiness versus liveness, tool/provider failures, budget/quota,
DB pool/lock waits, queue/outbox state and backup/restore outcomes for supported
paths. No silent “empty result” when tenant context/ledger is absent. Graceful
shutdown drains/records work and cleans task-owned services. Keep local overhead
small; no speculative Kubernetes/Temporal/service mesh or invented SLA/RPO/RTO.

## 14. A1-EVIDENCE — verification that proves the named behavior

### A1-EVIDENCE-01 — Traceability and semantic coverage

Every task/frozen behavior → application entrypoint → actual tested effect or
denial → named producing witness → required proof → exact source/artifact/profile.
Inspect the assertion, not just the test name. Component unit tests, source-text
markers, default fixtures and real owner/browser workflows remain different
evidence types. Report their limits. No policy consumer as CRM business proof,
no backup test as configuration apply race, no TS browser as Rust owner flow.

Preserve existing valid tests; add meaningful missing regressions. Fix baseline
bugs with explicit corrected/versioned expectations. Do not remove fixtures,
weaken assertions, widen tolerance/grants, add blanket ignores or reduce lint/
security coverage just to close the task. Required check failures appear in
verdict **and reasons**, including standalone advisory/unknown exit paths.

Required-coverage schema may need additive behavior/proof corrections exposed by
C findings. Keep original IDs/history and disclose the delta; do not shrink scope
or disguise an implementation gap as N/A. Independently reviewed acceptance is
pending after local verification; do not use an external review placeholder as
the sole explanation for absent local implementation/proof.

### A1-EVIDENCE-02 — Required verification families

| Family               | What must actually be exercised                                                                                                                                      |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Contracts            | Supported strict DTO/errors, unknown fields, Unicode, numeric/time bounds, canonical fingerprint compatibility and corrected legacy cases                            |
| Authority            | Tenant/actor/child/resource/grant/assigned scope, service-human intersection, forged approval/CSRF, revocation/logout, lock-wait races, current receipt reads/replay |
| CRM                  | Seven real writes plus required projections/transitions/history, competing revisions, duplicate/conflicting intents and atomic rollback                              |
| Product browser      | Rust login/logout/workspace, actual CRM/assistant actions, literal queries/paging, unknown response + app-owned recovery, restart, accessibility/safe rendering      |
| OWN-V1               | Actual OWN-T01–T10 on same candidate, including configuration effect, runtime disable refusal, real restore/history and two-tab stale plans                          |
| Reuse/distribution   | Two real minimal independent CRM consumers, appropriate domain consumers, no product workspace source aliases/private owner dependencies                             |
| Data/operations      | Fresh/prior upgrade, pending receipts/approvals, microseconds/key references, isolated backup/restore, target refusal, writer fencing and compatible rollback        |
| SDK/provider/media   | Two host embeds, consent/origins/mount/cleanup, required local protocol streams/unknown outcomes and media fallback/permission denial                                |
| Quality/supply chain | Pin/lock closure, Rustfmt/Clippy/build/test/native+wasm features, dependency/advisory/license/install/build-script policy and registered benchmarks                  |
| Gate integrity       | Missing/empty/failed/stale/wrong-profile/wrong-hash/never-ran/falsely mapped proof refuses completion; clean bootstrap without preexisting evidence                  |

Use installed pinned tools and dependency policies; add dependencies only for an
actual requirement. Verify selected runtime and command really execute their
entrypoint. Finite results cannot establish zero bugs/vulnerabilities or universal
scalability. No new tests merely mirroring trivial implementation details.

### A1-EVIDENCE-03 — Gate order and final identity seal

Use scoped checks while coding. Run artifact-mutating pack/build/proof commands
sequentially in correct dependency order, isolated from Archxy 2 outputs. Prevent
recursive root/security/Rust gate loops. Do not rerun huge suites after every
small edit without affected-scope reason; preserve and invalidate evidence honestly.

Final full sequence must include actual supported clean bootstrap, root
`pnpm verify`, `pnpm rust:verify`, `pnpm rust:verify-release`, applicable security/
consumer/browser/OWN/data/upgrade proofs, and read-only coverage validation.
Use aggregate gate ordering where these already nest; avoid duplicate expensive
runs. Missing required Rust paths do not become proven via Node baseline suites.

Prepare final source/docs and coherent authorized local commits **before** the
last producing run. Fix candidate identity: HEAD plus declared dirty source
digest, toolchains, locks, config/profile/catalogue/pricing/migrations, binaries
and web assets. Generate final proofs after that seal. Editing source/artifact/
covered docs or committing afterwards requires affected identity validation and
reruns under existing policy; never hand-edit old proof HEAD/hash/timestamps.

Store raw commands/exits/tool versions/checks/effect observations and logs. Shared
gate writers derive success from actual producing outcomes. Final validator must
evaluate the live candidate, not a cached prior report. Historical archives keep
their original bytes/identity; no overwriting a different release into 1.17.0.
Use a new candidate version when required by the actual immutable release policy.

## 15. Working loop, autonomy and operational limits

```text
read current checkpoint + counterpart handoff
→ choose highest-risk unblocked required item
→ inspect existing implementation, implement incrementally
→ self-review + meaningful scoped verification + fix
→ integrate packet / update proofs and checkpoint
→ proceed to next remaining item
```

Recommended order: A1-00/status corrections → logout/recovery/query/pagination
and shared authority → seven-command CRM/backend/UI/assistant → actual reusable
consumers → actual OWN-V1 → remaining interface/provider/SDK/media/ops → final
integrated candidate/proofs. Integrate ERP packets as they become usable. Change
order for concrete dependency reasons without asking owner to choose routine work.

Do not end with “ready?”, “continue?”, “audit fixes done”, “tunggu Codex” or
“tinggal implement X” while required independent work remains. Report concise
progress/findings/blockers during work; a status message is not close-out.
No interim Codex audit request is required. Self-review/tests still happen
throughout. If owner explicitly requests an interim look, follow that newer request.

Use direct authoritative process exits/managed sessions. Record PID/session/log
and collect completion; do not arm stale monitors waiting for buffered patterns.
Checkpoint before context exhaustion and resume next concrete action using available
continuation. Prompt cannot remove provider context/quota/session limits. If a
hard limit intervenes, save exact incomplete/resume state; do not claim work still
runs in background when the process/session ended.

Genuine external dependency blocks only affected work. Exhaust independent work
first. If all remaining work depends on unavailable ERP packet/access/input, state
exact blocker, evidence, requested interface and resume action; no false completion
or indefinite polling. Preserve distinction between own-lane verified and whole
candidate ready. Codex sign-off itself is not a prerequisite to implementation.

- Remain local/free with existing provider/privacy choices. No hosting/domain,
  paid calls, deploy/publish/push, real payment/email or owner cutover implied.
- Do not read/copy/log private owner data for tests. Preserve `.env`, `.local`,
  keys/settings, historical archives and unrelated dirty work/user processes.
- Follow latest AGENTS manual skill policy: ordinary work directly; no automatic
  skills, learning-loop, hooks, observers, tool setup or extra agent spawning.
  Use a named workflow only when explicitly selected by the owner.
- Start services only for the task; use unique DB/ports/browser profiles and
  always close task-owned instances including failure paths. Avoid shared fixed
  scratch/PID files, broad `pkill`, `close-all`, active-user fixture reaping or
  unsolicited shutdown of Archxy 2/owner services. Keep idle analysis tools off.
- Coherent local commits follow standing authorization, active hooks and exact
  owned paths. No `git add .`, foreign-work capture, reset/amend or hook bypass.
  Existing unrelated dirty work does not need to be cleaned to call own work done.

## 16. Final handoff — audit benar-benar di akhir

Update main `docs/rust/EXECUTION_STATE.md` and acceptance matrix with actual
completed/remaining IDs, relevant files/commits/artifacts, exact producing commands/
exits, source identity, known differences, dependency dispositions, active owned
processes and exact next action. Keep current summary at the top; old long tables
are historical, not a competing “next task” section.

Prepare final documents:

1. `docs/rust/ARCHXY1_COMPLETION_REPORT.md`: each task family/ C01–C07 closure,
   before/after behavior, core/CRM/OWN-V1/interface/ops status, actual runtime/bridge
   inventory, integration packets, measured limits and explicit kit/live gaps.
2. `docs/rust/ARCHXY1_AUDIT_PACKET.md`: candidate HEAD/dirty snapshot and hashes,
   exact clean bootstrap/run/stop steps, synthetic demo profiles, two-consumer
   setup, all producing commands/exits/raw logs, negative reproducers, browser
   recovery/logout/OWN scenarios and data/cutover/rollback/performance evidence.
3. `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md` plus machine-readable coverage/verdict:
   task/frozen behavior IDs mapped to actual assertions/proofs and current identity;
   local verification and independent review are separate facts.
4. Reconcile `FULL_RUST_COMPLETION_REPORT.md`, `FULL_RUST_AUDIT_PACKET.md`, active
   PRD/build/handoff docs and counterpart disposition. No obsolete statement that
   OWN-V1 is delivered by unrelated tests or every remaining blocker belongs to ERP.

**Implementation complete / final audit ready requires all of the following:**

- Every Archxy 1 task family is implemented, usable and locally verified; all
  seven C findings closed with current evidence, not just marked closed.
- Existing frozen behaviors are preserved/replaced with actual supported APIs,
  UI/actions/operations; no hidden first-party old runtime or required placeholder.
- Real CRM business reuse by two independent consumers and actual OWN-T01–T10
  PASS, including runtime module availability and history-safe config restore.
- Required ERP packets are integrated, domain invariants remain intact, and
  full candidate has current actual local proofs for R00–R11 + OWN-V1 / G01–G10.
  Review metadata may remain pending; required local evidence cannot be missing.
- Root/Rust/full coverage and required data/browser/consumer/security/performance
  checks PASS under registered policies on the same sealed candidate. Known
  advisories/live limitations are disclosed with their actual policy disposition.
- Candidate runs from documented clean setup. Reports/evidence/reproducer logs
  are complete, owner data untouched, task-owned services stopped as appropriate.
- `independent_review: pending`; no forged Codex acceptance or universal “zero bugs”.

If own lane is finished but required ERP integration is genuinely unavailable,
report **“Archxy 1 locally verified — full candidate incomplete, ERP dependency
pending”** with exact remaining packets/proofs. That is not final audit readiness.

Only after the full criteria above hold, close with:

**“Archxy 1 implementation complete — integrated Full Rust + OWN-V1 candidate
ready for final Codex audit.”**

Include links to final reports, exact candidate identity, actual gate results and
local run command. Owner then requests the independent audit; lu tidak meminta
Codex menutup tiap milestone di tengah implementasi.
