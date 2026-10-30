# Archxy 2 — mandat lengkap ERP sampai integrated local candidate

**Versi aktif: 20 Oktober 2026.** Lu adalah **Archxy 2**, implementator domain
ERP Archava. Archxy 1 owns shared core/CRM/host/configuration dan integrasi akhir.
Codex melakukan audit independen setelah candidate dan bukti implementasi lengkap.

Bro, kerjakan seluruh required ERP scope di file ini sampai usable, integrated
dan verified lokal. Jangan berhenti pada contracts, kernel mutations, satu cohort,
packet terkirim atau suite hijau. Self-review dan tests berjalan selama pengerjaan;
tidak perlu meminta audit Codex/owner `continue` pada setiap milestone.

Canonical task berada di main repo:
`/home/haikaru/Archverse/Lab/Archava_Platform/ARCHXY_2_FULL_IMPLEMENTATION_TASK.md`.
**Resume existing working copy**:
`/home/haikaru/Archverse/Lab/Archava_Platform_archxy2_erp`.
Jangan membuat ulang/menimpa copy itu atau mengedit source main secara paralel.

File ini menggantikan urutan/status/scope shortcuts pada penugasan ERP 17 Oktober.
PRD/security/data contracts tetap berlaku. Instruksi sistem dan owner/AGENTS yang
lebih baru lebih tinggi; ADR implementator tidak boleh mengurangi requirement,
menukar Rust behavior dengan TS baseline atau mendefinisikan ulang arti PASS.

## 1. Produk dan batas selesai

ARCHZXY adalah company; Archava produk dan reusable module library. Website
ARCHZXY akan menjadi consumer terpisah di luar repo. Target ERP adalah kepingan
bisnis yang dapat dikomposisikan dengan Archava, sesuai configuration, dependency,
entitlement, permission dan resource scope, tanpa source fork per business model.

Tetap modular monolith dan existing pinned Rust/Axum/Tokio/PostgreSQL/SQLx/Leptos
SSR/WASM stack. Pure rules/contracts terpisah dari adapters dan serving/UI.
Gunakan cohesive internal modules dan transaction ports; tidak perlu satu crate
per npm package, microservices, universal EAV/workflow engine atau plugin ABI.

**ERP lengkap dalam tugas ini:** seluruh bounded ERP behavior frozen R04–R08,
corrected invariants, actual reusable Rust API/library/UI, configuration/tooling
contributions, real data/recovery/upgrade proofs dan integrasi ke candidate main.
Existing source yang sudah valid dipertahankan dan dilengkapi secara incremental.

Ini tidak otomatis menambah full payroll, manufacturing, tax/accounting, regulated
clinical system, full LMS atau 40 full operating kits. Jangan mengganti incomplete
existing capability dengan klaim “future scope”. Katalog kit, reusable module,
bounded workflow dan complete industry kit mempunyai acceptance berbeda.

## 2. Ownership, baseline dan koordinasi

### A2-00 — Baca actual state, bukan snapshot lama saja

1. [Master PRD](PRD.md), [Full Rust PRD](docs/PRD_FULL_RUST.md), module contracts,
   corrected authority, migration/cohort rules, R04–R08 dan G01–G10.
2. [Archxy 1 full mandate](ARCHXY_1_FULL_IMPLEMENTATION_TASK.md) dan
   [Owner configuration PRD](docs/OWNER_CONFIGURATION_PRD.md). Lu menyediakan
   domain configuration/readiness/API/tools/UI; generic owner plan/apply owns A1.
3. `docs/rust/MIGRATION_INVENTORY.md`, `migration-inventory.json`,
   `required-coverage.json`, `KNOWN_DIFFERENCES.md`, `DECISIONS.md` dan main
   `EXECUTION_STATE.md`; paths singkat juga berada di `docs/rust/`.
4. **Working-copy** `docs/rust/archxy2/ERP_SCOPE.md`, `EXECUTION_STATE.md`,
   `INTEGRATION_REQUESTS.md`, actual source/diffs/evidence dan shared handoff.
   Salinan checkpoint yang sudah landed di main dapat lebih tua dari copy lu.
5. [Audit A01–A09](docs/PROJECT_AUDIT_2026-10-14.md),
   [J findings](docs/ARCHXY_KIT09_REVIEW_2026-10-15.md),
   [RN findings](docs/ARCHXY_RUST_AUDIT_2026-10-17.md) dan
   [C01–C07](docs/ARCHXY_1_CRM_AUDIT_2026-10-19.md). Shared fixes owns A1;
   analogous domain defects/proof shortcuts tetap harus lu cegah/tutup.
6. Actual `packages/<domain>` / `<domain>-pg`: strict contracts, command unions,
   service/repository, migrations, APIs/tools/dashboards dan verifiers. Baca domain
   docs yang dirujuk pada §4–§8. Docs/test counts tidak menggantikan source.

Saat mandat ini dibuat, working-copy checkpoint mencatat PACKET-000…007,
purchasing/foodservice, R04 organizations/proposals/projects, R05 scheduling/
accommodation/assets dan first R07 billing work. Main disposition mencatat
000–002 landed, sementara packet selanjutnya tersedia. **Ini reading snapshot,
bukan fresh independent acceptance.** Reconcile actual source/evidence/handoff
saat resume; jangan mengulang pekerjaan valid atau menyebut packet absent tanpa
memeriksa file terbaru. Capacity operations, complete commerce, reads/UI dan Rust
consumer depth tetap harus diinventaris; jangan menganggap sudah selesai dari
label cohort/checkpoint atau test kernel.

### A2-01 — Scope matrix yang menutup seluruh behavior

Update `docs/rust/archxy2/ERP_SCOPE.md` dan buat acceptance matrix
`docs/rust/archxy2/ERP_ACCEPTANCE_MATRIX.md`. Untuk **setiap domain**:

- Inventory setiap existing command variant, validation/error, read projection,
  access/list/detail/history/status route, tool, dashboard action, CLI/worker seam,
  data owner/ledger dan cross-domain reference/cohort.
- Map baseline behavior → corrected target → Rust implementation → actual API/UI
  entrypoint → named positive/negative effect assertions → producing proof ID.
- Catat baseline-supported, corrected compatibility delta, required new PRD
  contract dan explicit future backlog secara terpisah. No denominator shrink.
- Status berbeda: implemented, locally verified in copy, packet delivered,
  integrated verified, independent review pending. Tidak ada blanket accepted
  karena crate/command grammar atau nine happy-path witnesses tersedia.
- Pertahankan existing frozen IDs dan needed additive proofs. Jangan mengganti
  `P-R04-DOMAIN`…`P-R08-DOMAIN` dengan narrow producer yang scope-nya lebih kecil.

Minimum domain set berikut mencakup package murni **dan** pasangan `-pg` serta
actual host/UI/tool surfaces. Actual frozen inventory tetap authoritative jika
menyertakan required extension lain; tidak ada domain yang dihapus dari scope
hanya karena implementasinya belum tersedia di Rust.

| Milestone | Domain IDs baseline yang wajib dicakup                             |
| --------- | ------------------------------------------------------------------ |
| R04       | `organizations`, `proposals`, `projects`, `capacity`               |
| R05       | `scheduling`, `accommodation`, `assets`                            |
| R06       | `commerce`, `purchasing`, `foodservice`                            |
| R07       | `billing`, `subscriptions`, `attendance`, `support`, `fulfillment` |
| R08       | `laundry`, `field-services`, `formal-education`                    |

### A2-02 — Single-writer boundaries

| Area                                                                                                                  | Owner / writer                  |
| --------------------------------------------------------------------------------------------------------------------- | ------------------------------- |
| ERP pure domain rules, repositories, domain handlers/views, domain tests/migration proposals/proofs                   | Archxy 2                        |
| Core/identity/session/logout, generic authority/approval/receipt/idempotency, tenant transaction/fixture primitives   | Archxy 1                        |
| CRM, main Leptos shell, assistant/providers/SDK/media, generic pricing/assembly/capacity math, OWN-V1 engine          | Archxy 1                        |
| Business capacity pools/allocations/queue persistence dan domain capability/config adapters                           | Archxy 2                        |
| Root manifests/locks, common exports/router/AppState/readiness/fixtures, release/config/coverage registries, CI/gates | Archxy 1 integration writer     |
| ERP packets/copy checkpoint/domain docs and evidence                                                                  | Archxy 2                        |
| Shared main candidate, aggregate proofs, global release report                                                        | Archxy 1 with ERP contributions |
| Final independent review                                                                                              | Codex; no self-sign-off         |

Default owned namespaces:

```text
rust/crates/domain/src/erp/**
rust/crates/adapters/src/erp/**
rust/crates/host/src/erp/**
rust/crates/slice/src/erp/**
rust/crates/{domain,adapters,host}/tests/erp_*.rs
tools/rust/erp/**
examples/rust-consumers/erp/**
docs/rust/archxy2/**
```

Adapt existing equivalent paths via explicit packet; do not create duplicates.
`erp` namespace is ownership, not a requirement to enable all domains at once.
In isolated copy, temporary shared integration edits are allowed to build/test;
keep exact diff and submit to A1. They are not landed main changes. Do not
independently rewrite shared policy, manifests or root migrations to bypass a seam.

### A2-03 — Actual candidate isolation dan incremental packets

Preserve existing working copy and own uncommitted work. Record HEAD/branch,
dirty/untracked public source hashes, current main baseline, config/locks/migrations,
tools/artifacts and task-owned processes. If a new copy is genuinely needed,
snapshot actual public dirty/untracked candidate consistently, not HEAD alone;
exclude `.env`, `.local`, DB/backups, keys, caches and build outputs. No writable
hardlinks or wholesale rsync over active work. Never commit copied A1 work as lu.

Handoff root: `/home/haikaru/Archverse/Lab/Archava_ERP_Handoff/`.
Lu writes `archxy2/`; A1 writes `archxy1/`. Own state/docs live in the copy and
are published through handoff; do not edit A1 main execution state. Read counterpart
dispositions at checkpoints. Sessions do not automatically receive each other's
changes. No external messaging is authorized by this file.

Each packet has immutable ID/base hashes, owned files, exact shared changes,
dependency/version/SQL/DTO impact, migration/cohort ownership, actual commands/
exits/logs/proofs and next integration action. Publish successors rather than
silently replacing earlier evidence. Explicitly distinguish a shared-file patch
from the namespaced source bundle; packet text alone must not omit required code.

Integrate in small steps. A1 reviews current semantics and lands the shared/root
parts; lu refreshes baseline without overwriting own pending work, resolves owned
conflicts and rechecks affected ERP behavior on the integrated candidate. No huge
detached fork/merge surprise at the end. If A1 changes a shared interface, use its
current contract, not a copied authentication/transaction engine.

## 3. A2-SECURITY — applies to every domain

### A2-SEC-01 — Tenant, current grants and relationships

Authenticated principal comes from shared host, not body fields. Recheck active
domain/workspace memberships, granular permissions, module/entitlement availability,
assigned/resource scope and child/reference ownership inside the correct transaction.
Service on-behalf-of permission **and scope** intersect the represented human's
current grants. No numeric rank that widens guardian/self/assigned scopes.

Preserve human-only mutations/recovery where specified. Provider-signed billing
events are a separate explicit service path, not blanket permission for machine
writes. Module enable never creates memberships/grants. Public/customer personas
cannot reach owner ERP tools just because they share an Archava interface.

Runtime PostgreSQL role is non-owner, no superuser/BYPASSRLS, forced tenant RLS
and transaction-local context on each path. Missing context/readiness is a clear
error, not fabricated empty success. Caller cannot override tenant/owner/prices/
state/paid flags/clock/permission fields through unvalidated payloads.

### A2-SEC-02 — Retry, receipt dan approval integrity

Command claim binds tenant, actor/initiator, action and canonical validated input
under stable private fingerprint keys; revision/expiry/approval binding follows
the actual contract. Same-key/same-input retries produce one effect; changed body
or unauthorized actor conflicts without leaking another receipt. Lock-wait expiry,
grant revocation and reassignment are checked at the committing boundary.

Replay/status/recovery checks current required read/operation grants and resource
scope before exposing cached results. Read-only recovery must not acquire a new
write authority; conversely a cached receipt cannot bypass a revoked read grant.
Different actor/tenant/private child/audit references do not become visible.
External provider event IDs have their own signature/claim/dedup contract; do not
confuse them with ordinary human intent IDs or reuse one domain's receipt in another.

Use shared one-time approval/intent machinery for any explicitly supported risky
action. Planning/draft metadata, browser `confirmed: true` or a valid model output
are not server authority. Do not add machine mutations just to make a tool demo.

### A2-SEC-03 — Atomic cohorts dan trusted reference status

Domain write, child state, audit, command receipt and relevant stock/occupancy/
financial effect commit or roll back together. Deterministic lock order and explicit
transaction SPI prevent oversell/overlap/deadlocks; no dual commits via TS↔Rust HTTP.
One owning runtime writes each tenant/cohort, including tools/CLI/jobs/recovery.

Internal references relied upon for an operation must exist and be authorized in
the correct tenant/resource scope. Use composite constraints or trusted typed
ports as appropriate. An arbitrary UUID/string is not verified authority.
External opaque references can remain explicitly unverified metadata when the
supported contract allows that; do not invent cross-domain FK or claim them bound.
No reference grants permission to read the target domain automatically.

### A2-SEC-04 — Corrected contracts and boundary discipline

Port strict schemas/error vocabulary, number/safe-integer/money/quantity/time
bounds, null/unknown fields, Unicode and canonical fingerprint bytes explicitly.
Rust Serde defaults are not automatically equivalent to Zod. Preserve keys,
IDs/revisions/microseconds and audit semantics. Use vetted crypto, not own schemes.

A01–A05/A08 and analogous defects require concrete negative PostgreSQL tests.
Known-broken TS output is not golden correctness: record old counterexample,
correct target behavior, version/compatibility impact and proof. A06 minimal
dependency closure and A07/A09 release/docs truth are part of domain delivery.
Shared bug → reproducer/request to A1; continue independent work instead of
bypassing authorization or widening fixture permissions.

## 4. A2-R04 — organizations, scope, delivery and capacity

### A2-ORG — Organizations and affiliations

Port all `organizations`/`organizations-pg` commands, access/list/detail/tools and
dashboard behavior: account/legal/billing profile fields, state/revision changes,
contact affiliations/roles, primary link and deactivation/history. Bound field
validation and current grants/scope must hold for parent and child resources.

At most one active primary affiliation per organization under concurrent changes;
multiple non-primary links remain possible. Replacing/deactivating a link retains
history. Archive/revision policy and parent immutability follow source. A CRM
contact's free-text organization is not automatically a verified organization
account/primary affiliation. Any internal contact binding relied upon is explicit.

### A2-PROPOSALS — Proposals, agreed scope, amendments

Port all `proposals`/`proposals-pg` lifecycle, line/deliverable editing, submission/
review/decision, immutable acceptance snapshot and change request transitions.
Acceptance freezes scope v1; direct editing of accepted scope refuses. An approved
amendment creates next version with recorded price/time impact, decision identity
and immutable previous scope. Competing approvals/active CRs follow source rules.

Scope/prices/revisions come from validated backend facts; proposal draft/internal
operator confirmation is not a client signature or payment. Decision grants are
distinct from draft-write grants. Expose real scope/version/amendment history and
retain rejected/cancelled outcomes; no silent feature absorption or history rewrite.

### A2-PROJECTS — Actual delivery workflow

Port all `projects`/`projects-pg` project/task/milestone operations, bounded detail,
audit/recovery and UI. Draft/active/review/delivered/closed/cancelled transitions,
required task/milestone gates, evidence and parent revision synchronization follow
the supported graph. Cancelling a required task must not count as completing it.
Reached/delivered/closed terminal work cannot be silently reopened/edited.

Link accepted scope/opportunity through explicit supported references/ports, not
a copied CRM or guessed approval. Preserve distinction between internal project
activation and client's contractual acceptance. Future assignment/risk/forecast
features not in current contract remain declared gaps unless explicitly required
by Full Rust PRD; do not claim them operational from a due date field.

### A2-CAPACITY — Business capacity operations, not just pure math

Port all `capacity`/`capacity-pg` pool/window/allocation/demand/progress/complete/
cancel/list/detail/estimate workflows. A1 owns reusable pure estimator; consume
that implementation and versioned contract, do not duplicate it in a repository.

Resource units, throughput/calendar, windows, max concurrency and active/queued
demand are actual stored inputs. Estimate carries frozen clock/snapshot/revision,
assumptions, earliest start/finish and queue position; estimate is not reservation.
Reservation/commit uses concurrency control; saturation/slot handover, partial
progress, release/cancel/completion and rescheduling cannot double-allocate/free
capacity or produce negative load. Unknown capacity yields unknown/blocked, not
a confident date from LLM/defaults. Hours/days/slots require explicit conversion.

R04 is not complete because organizations/proposals/projects kernels passed;
capacity persistence, usable reads/UI and cross-module delivery facts also count.

### A2-R04-FLOW — Contribution to commercial/delivery chain

Provide typed facts/ports so A1 can compose confirmed requirement → deterministic
quote → versioned proposal/scope → acceptance/signature evidence → deposit request
→ authoritative settlement → capacity queue → delivery/review → warranty versus
maintenance, per BUS/FLOW/CAP contracts. Sales, agreement, payment and delivery
states remain separate. Changed scope requires explicit approved amendment.
Deposit percentage is configuration, not a newly hardcoded 50% rule. No live
signing/payment, legal/SLA guarantee or company website is added by this task.

## 5. A2-R05 — scheduling, accommodation and assets

### A2-SCHEDULING — Resource reservations

Port all [scheduling](docs/RESOURCE_RESERVATIONS_MODULE.md) commands and read/tool/
UI paths: resource setup/availability policy, bounded reservation hold/confirm/
cancel/expire and supported history. Check actual current capacity and transaction
clock after locks. Concurrent last-slot/overlap races have a valid single result;
stale/expired holds cannot revive or extend through replay. Calendar/time bounds,
pagination and same-resource locking remain consistent with source.

### A2-ACCOMMODATION — Room nights and physical occupancy

Port all [accommodation](docs/ACCOMMODATION_MODULE.md) property/room-type/rate/room/
active/housekeeping/stay hold-confirm-check-in-check-out-cancel operations and
bounded reads. Reuse managed shared scheduling capacity, not another occupancy
pool or public scheduling bypass. Generic APIs must hide/refuse managed resources.

Local calendar nights/timezone/DST semantics, bounded arrival/departure, guest
capacity, backend frozen nightly quote/rate revision and after-lock TTL are real
rules. Occupied rooms cannot be cleaned/disabled or host another physical guest;
overdue guests remain physically present until checkout. Checkout dirties/releases
appropriately; required inspection is enforced. Rate changes affect future stays,
not old quotes. No invented payment/refund status from stay confirmation/checkout.
Stay/room/occupancy/quote/audit/receipt share the correct transaction.

### A2-ASSETS — Serialized custody and maintenance

Port all [asset](docs/ASSETS_MODULE.md) registration, business/customer ownership,
custody/dispatch/return, condition/repair/maintenance/retirement operations and
history/list/detail/tools/UI. Customer-owned and business-owned lifecycle/required
references differ according to source. Preserve immutable serialized identity,
revisions, custody evidence and append-only maintenance/audit records.

Scheduling/field-service asset references cannot claim verified availability or
ownership from untrusted strings. Custody, repair completion and financial
settlement are separate facts. Where stock/occupancy effects compose, use the
owning transaction SPI and current grants; no parallel asset inventory ledger.

## 6. A2-R06 — complete shared stock cohort

### A2-COMMERCE — Public catalog/order/stock module, not only private kernel

Port all [commerce](docs/COMMERCE_MODULE.md) product/catalog/price/active/stock
administration, draft order/quote/reserve/fulfill/cancel/received-return operations,
list/detail/access/stock/movement/status APIs, allowed tools and dashboard.
Existing private `commerce_kernel` helpers alone do not satisfy public module scope.

Commerce is sole owner of product identity/base unit/physical stock/movements.
Backend quotes freeze price/revision/lines; supply costs remain separate. Quantity/
totals checked, reservations/expiry/last-unit races and multi-product rollback
enforced; available-stock read is an observation, not a guaranteed allocation.
Cancellation/return/restock/discard preserve actual movement history. Managed
food products/orders cannot bypass domain-specific rules through generic APIs.

### A2-PURCHASING — Purchase orders and physical inspection

Port the complete [purchasing](docs/PURCHASING_MODULE.md) supplier/offer/draft/
confirmed order/accepted-rejected receipt/cancel/short-close lifecycle, current
access/list/detail/audit/stock projections, tools and owner UI. Preserve backend
offer/product validation, immutable confirmed lines/quantity and supplier-cost
versus selling-price separation.

Cumulative accepted + rejected cannot exceed ordered. Only accepted units enter
usable commerce stock through a caller-owned tenant transaction SPI. Partial and
all-rejected findings remain auditable. Overflow/invalid line/revision conflict
rolls back **every** finding/claim/movement/audit in that command. Short closure
retains prior physical findings; terminal order cannot receive again. Return/
replacement/payables/valuation extensions remain explicit bounded gaps where
not supported; no duplicate physical stock balance or implied verified payment.

### A2-FOODSERVICE — Tables, recipes and tickets

Port all [foodservice](docs/FOODSERVICE_MODULE.md) tables/menus/versioned recipes,
ticket creation/queue/preparation/kitchen/serve/close/cancel, allowed reads/tools
and usable UI. Frozen recipe/price/station facts govern old tickets; later recipe
or price edits do not rewrite them. One open ticket per physical table and current
seat/capacity/permission constraints remain enforced.

Queue reserves aggregated shared ingredients atomically; preparation consumes
once, with ticket/shared order/stock/audits/receipt committed together. Insufficient
ingredient, expired hold and downstream failure leave no partial effects. Before
prep cancel releases reservation; after prep cancellation records waste without
restoring cooked ingredients as raw stock. Close frees table, not marks invoice
paid. Managed generic commerce routes cannot forge restaurant transitions.

### A2-STOCK-COMPOSITION — Cross-domain evidence

Race purchase receipt against sell/restaurant consumption; prove one authoritative
on-hand/reserved/movement accounting and current cross-domain grants. Use sorted
locks, actual command rows and rollback failure points. Map fulfillment/asset
effects explicitly. R06 complete requires commerce public behavior, purchasing,
foodservice and applicable read/UI/recovery/upgrade/reuse, not two kernel suites.

## 7. A2-R07 — financial and operating modules

### A2-BILLING — Immutable invoice intent and provider settlement

Port all [billing](docs/BILLING_MODULE.md) invoice create/issue/correction/void,
attempt/event/recovery, list/detail/history/tools/UI contracts. Preserve backend
priced snapshots, currency/integer bounds, append-only correction/settlement
chains and current invoice/grant/revision checks. No “mark paid” shortcut.

Payment attempt records intent, not successful outcome. Only exact authenticated
provider claim/signature/reconciliation policy authorizes settlement; invalid
signature is refused before business mutation. Correct service caller still
needs current required billing authority. Test deduped/changed claims, cross-tenant,
wrong amount/currency, over-settlement, failed notification, out-of-order and
refund/correction behavior. A failed event may be history per contract but cannot
silently change invoice balance/revision. Human manual reference is not provider
verification. Use synthetic signed provider endpoints/events only.

### A2-SUBSCRIPTIONS — Lifecycle, entitlement and usage

Port all [subscription](docs/SUBSCRIPTIONS_MODULE.md) plan/subscription lifecycle,
entitlement activation/suspension/cancellation, usage/meter/idempotency and current
access/list/detail/status/tool/UI behavior. Keep period/time/quantity/money bounds,
allowed state transitions and immutable history/revisions.

Correct A05: paid draft/unpaid subscription cannot activate or consume paid
entitlement. Link authoritative billing/settlement facts through explicit policy.
Free trial/grace or approved non-provider arrangements, if supported, need
separate explicit audited authority; do not mislabel them provider-settled.
Concurrent usage/cancellation/replay cannot exceed quotas or revive entitlement.

### A2-ATTENDANCE — Session/roster/check-in relationships

Port all [attendance](docs/ATTENDANCE_MODULE.md) session setup/status, participant
registration/roster transitions, ticket/check-in events, counts, read/tool/UI and
recovery paths. Enforce unique tenant/session/contact and supported ticket keys,
capacity/state/time constraints, revisions and immutable attendance history.

Correct A04 in persistence/application: roster belongs to the supplied session,
contact and tenant; a valid UUID alone is insufficient. Concurrent registration/
check-in and same/different idempotency keys produce supported unique outcomes,
consistent counters and atomic rollback. Wrong session/roster cannot check in or
expose another record. Unenrolled/inactive/revoked caller receives no authority.

### A2-SUPPORT — Cases, assignment, internal notes and resolution

Port all [support](docs/SUPPORT_CASES_MODULE.md) create/assign/priority/target,
investigation/wait/resolve/close/cancel/reopen, append-only notes and immutable
transition/history/access/list/detail/directory/status/tool/UI behavior.

Current creator/assignee scope and separately gated human assignment directory
matter; reassignment changes old actor's detail/note/receipt access immediately.
Projection holds the necessary row/membership locks. Resolution cycles preserve
old solution evidence; reopening is explicit. Internal notes stay separately
permissioned and are not customer messages or proof of SLA/customer acceptance.
Missing due target is unknown, not on-time success. Bounded histories show truncation.

### A2-FULFILLMENT — Custody, dispatch and evidence

Port all [fulfillment](docs/FULFILLMENT_MODULE.md) package/job intake/inspection/
dispatch/handover/delivery/exception lifecycle, evidence/counters/revisions,
access/list/detail/status/tools and dashboard from actual source.

Validate contact/order/asset/appointment links when relied upon as authoritative;
restrict assigned/service scope correctly. Logistics state does not invent stock
movement, billing settlement or recipient signature. Relevant commerce effects
use the owning transaction port once; no second stock pool. Preserve monotonic
terminal rules, explicit exceptions and immutable chain/evidence history.

## 8. A2-R08 — vertical extensions

### A2-LAUNDRY — Weight/item orders and physical chain

Port all [laundry](docs/LAUNDRY_MODULE.md) tariff/order/intake/processing/chain/
quality/reconciliation/ready/return operations and read/tool/UI/recovery behavior.
Freeze authoritative price/weight/item units/totals; piece and gram counters
remain distinct. Preserve dedicated grants for machine/counter operations.

Chain order is monotonic. Quality findings/unreconciled counters require explicit
valid reconciliation once; a cancelled/backward/repeated step cannot bypass them.
Ready needs required quality/evidence and no unresolved finding. Returned orders
freeze. Physical status and priced quote do not assert payment or customer message.

### A2-FIELD — Field jobs, technician dispatch and sign-off

Port all [field service](docs/FIELD_SERVICES_MODULE.md) job/site/dispatch/visit/
evidence/approval/sign-off lifecycle, references, scoped lists/detail/history,
tools and UI. `field.dispatch`, recording and approval/signoff grants remain
distinct. Preserve both evidence axes, accepted sign-off prerequisites and actual
terminal/revision rules; permission to record is not permission to accept work.

Appointment/asset/contact/job relationships are checked through authorized ports
where required; unverified external references remain labelled as such. A visited
site is not a completed/paid project. Do not duplicate scheduling/assets/billing.

### A2-EDUCATION — Enrollment, evidence, review and retention

Port corrected frozen [formal education](docs/FORMAL_EDUCATION_MODULE.md)
enrollment/admission/session/attendance/result/review/retention/withdrawal/graduation
operations, scoped read projections/tools/UI and recovery. Existing candidate
does not become a complete school/LMS simply by compiling.

Guardian/self/assigned-class/all are separate predicates, not ordered ranks.
Check exact child/enrollment/session/class relationships and delegated grants;
student cannot read sibling, guardian another family, teacher another class.
Unreviewed results remain protected with truthful publication state; review and
retain permissions are separate from score recording. Enforce score against
`outOf`, append-only reviews/results/evidence, terminal transitions and revisions.

Protect minimal student/guardian references and academic facts; do not add personal
directories or send child records to provider prompts. Scheduling/invoice refs do
not themselves authorize target-domain access or prove attendance/payment.
Retention/withdrawal is not destructive history deletion. Unsupported specialist
policy/legal/regulated guarantees remain declared gaps, not fabricated acceptance.

## 9. A2-SURFACES — every domain must be usable

### A2-API — Complete reads/writes and packaged public contract

For every scope item, implement strict browser-safe DTOs/errors, actual authorized
access/list/detail/filter/pagination/history/command/status routes, tools and
service/library entrypoints. Preserve optional/truncation/unavailable semantics;
unsupported is distinct from empty data or not-found. Return trusted projections,
not full private records filtered client-side. No SQL/credential/internal details
in public errors. API bodies/queries/lists/resources have bounded limits.

Tools are allowlisted by identity/module/capability; preserve read-only or prepare
scope unless authorized execution is explicitly supported. Tool metadata alone is
not execution. Model/customer cannot call privileged transaction SPI directly.
All registered commands dispatch actual domain effects before a committed receipt.

### A2-UI — Real Leptos domain panels

Provide namespaced Leptos components, routes/contracts and integration packet to
the A1 shell. Actual stored lists/detail/forms/review/results/history must be
reachable in main; a kernel suite or HTML heading is not domain UI completeness.
Reuse shared login/session/workspace/CSRF/approval/recovery interfaces, not copied
auth code or a second ERP app. Choose actual suitable synthetic business consumers.

Literal URL encoding, functional cursor navigation, filters/date/time/money/units,
safe text rendering and visible required transition reasons must work. Test
Unicode/`&`/`+`/`%`, loading/empty/denied/offline, keyboard/focus/labels/mobile,
logout/workspace disposal and stale async response containment. Read-only user
must not become writer because UI showed an enabled button.

### A2-RECOVERY — pending identity and unknown outcomes

Domain mutation retains the original account/tenant-bound opaque request identity
before it can commit, using shared recovery contract. Storage failure prevents
unsafe submission where durable tab recovery is required. On network loss,
HTTP 500, truncated/non-JSON/malformed/mismatched receipt, denied retry or recovery
outage, keep unknown pending and prevent a new key/new effect until reconciled.

Status uses current authority and actual receipt/resource binding. Absence is
unconfirmed, not automatically safe-to-repeat. Same key/input may retry according
to supported policy, never change input under that key or blindly re-execute an
unknown provider outcome. Reload/refresh/restart recovery must use app-owned state;
test-driver remembered UUID is not app persistence. Follow current product recovery
requirements and retain existing narrower behavior only with explicit supported
disposition. No cross-device inbox is invented as a new universal requirement.

Logout/workspace change clears private views and ignores late replies. A1 owns
actual server logout; lu proves ERP endpoints stop accepting revoked sessions
after integration. Do not preserve insecure shared behavior just for a green test.

## 10. A2-MODULES — configuration, reuse and genuine Rust consumers

Versioned module contract includes ID/SemVer/dependencies/incompatibilities,
permissions/API/events, config schema/UI/tools, required env **references**,
SQL/ledger compatibility, readiness/health and migration/upgrade proof requirements.
Installed, enabled, commercial entitlement, user permission and resource scope are
five different facts; one does not imply another.

Expose actual ERP capability/config/readiness adapters for A1's OWN-V1 planner
and apply service. Module disable affects serving UI/API/tools, direct disabled
operation refuses, dependent disable refuses and data/history remains retained.
Never auto-grant during enable, fabricate readiness or edit pricing source to make
unsupported selection look available. Owner configuration is separate from developer
CLI/JSON files and installation of a not-yet-installed module.

Two independent consumers for each declared reusable domain/cohort use identical
released **Rust** business implementation and required SQL artifacts under different
supported business configuration, with actual isolated PostgreSQL effects/denials.
Extract library/packaged host boundary if current code only works inside a proof
fixture. Minimal dependency closure only; no product source aliases/private owner
env, all-ERP installation to hide imports or client-specific copies.

Existing `module-consumers.ts` installs compiled TypeScript `@archava/*` tarballs.
That is valuable **TS baseline distribution evidence**, not sufficient evidence
that Rust ERP operations are independently consumable. Keep it under its truthful
scope; add genuine Rust packaged consumers and actual persisted workflows. Merely
checking command parsing/pre-SQL refusal, hashes, typechecking or nine policy tests
does not exercise database/business behavior.

Choose appropriate model pairs from existing contracts/verifiers: purchasing
wholesale/restaurant, accommodation hotel/villa, proposals/capacity agency/EPC,
organizations wholesale/recruitment, assets rental/repair, laundry kiloan/dryclean,
and source-supported pairs for the rest. Verify current mappings rather than
inventing kit labels. Different config/terms are allowed; source must remain same.
Narrow module reuse is not full industry-kit acceptance.

## 11. A2-DATA — migrations, upgrade, restore and writer ownership

Reuse historical SQL bytes/checksums/ledger ownership and same DB schemas until
explicit relocation has compatible packaging/upgrade proof. Corrective SQL is
additive/versioned and coordinated with A1 shared migrator/readiness/fixture/
release registry. An unknown ledger/checksum is a refusal, not permission to reset
the DB. Packaged Rust release must include SQL without TS workspace at runtime.

Verify each accepted domain on fresh synthetic DB and supported non-empty prior
profile: keys/fingerprints, IDs/FKs/revisions, timestamp microseconds, grants,
command/audit/history and pending/approved outcomes preserved. Tests cannot use
owner DB/keys/settings/backups. Restore goes to fresh explicit target; active
target overwrite is refused. No auto-enrollment or silent owner migration.

Backup/restore/restart must recover historical receipts with current authorization
without another physical/financial effect. Writer handover/fencing covers actual
domain mutations through routes/tools/CLI/jobs/webhooks and stale process denial;
lease-table success alone is not a domain write fence. Shared implementation owns
A1; lu provides domain enforcement ports and real write/recovery witnesses.

Rollback follows compatible schema/runtime window and retains committed stock,
financial facts/academic records/audit. No DB downgrade or history erasure to make
rollback green. Archive/profile identity follows A1's sealed candidate; don't choose
another release version independently or overwrite historical 1.17.0 artifacts.

## 12. A2-PROOFS — meaningful verification and gate integration

### A2-PROOFS-01 — Required layers, for actual required scope

| Layer            | Minimum evidence                                                                                                                                     |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Domain contracts | All supported command variants/read/error/boundary rules, corrected legacy cases, strict fields/number/time/Unicode/canonicalization                 |
| PostgreSQL       | Non-owner forced RLS, tenant/context/grant/resource/delegation, parent/child/reference integrity, revisions/history and actual persisted effects     |
| Atomic cohort    | Last unit/slot/capacity/entitlement races, cross-ledger rollback after partial work, lost outcome, expiry/revocation during proven lock wait         |
| Packaged host    | Current authenticated routes/read/tools/commands/status, CSRF/origin, role/module disable, malformed/mismatched receipt and restart/recovery         |
| Leptos browser   | Visible full supported lifecycle, read-only/assigned denials, literal queries/paging, app-owned pending recovery/reload and disposal                 |
| Rust reuse       | Two actual independent minimal packaged consumers, unchanged Rust artifacts/SQL and isolated business data/effects                                   |
| Data operations  | Fresh/non-empty prior upgrade, old receipt binding, isolated restore, compatible rollback and actual write fencing                                   |
| Gate integrity   | Each behavior backed by real producer/witness/current identity; missing/fail/stale/wrong profile/never-ran source-marker shortcut refuses completion |

Tests inspect actual business/child/stock/occupancy/financial rows and audit/receipt
binding, not only HTTP 200 or summary test count. Use deterministic race barriers/
observed waiters and bounded timeouts. Fixtures may not widen shipped authority/
KDF limits or swallow a skipped test as PASS. Record setup failures separately
from product counterexamples; short owned `/tmp` socket paths avoid Unix length
failures. Cleanup all task-owned fixtures, including failed provisioning.

### A2-PROOFS-02 — Semantic mapping and complete milestone proofs

Map every ERP_SCOPE row to named actual assertions and shared required proof IDs.
Narrow `P-ERP-*` progress proofs can grow additively; whole `P-R04-DOMAIN` through
`P-R08-DOMAIN` only pass when the full cohort's actual behavior/UI/consumer/data
requirements run. They are not A1's free slot to leave empty after domain delivery.
Lu supplies complete producing code/evidence mapping; A1 registers and reruns it.

No consumer whose effect stops at validation/mock pool can prove PostgreSQL flow.
No TS host/browser battery closes a Rust UI path. No static source marker or
`accepted: true` field is independent review. Preserve scope/count history;
intentional fixture/correction changes are documented, not silently relabelled.

Local proof must run **without prior Codex sign-off**. Independent review stays
pending metadata until actual final review. If a registry conflates reviewer
signature with ability to execute tests, send a precise correction packet; don't
self-sign, skip local proofs or stop domain work because the reviewer hasn't run.

### A2-PROOFS-03 — Identity, aggregation and safe final runs

Evidence records actual command/exit/tool versions, source/config/locks/catalogue/
migration/binary/web asset hashes, fixture class, checks/effects and raw log paths.
Copy-local HEAD/overlay identity and landed-main identity are separate; re-prove
affected behavior after integration. Do not hand-edit old proof HEAD/digests.

Add meaningful ERP regressions to normal gates/CI through A1 integration; not a
manual list forgotten outside the release path. Preserve existing root/Rust/TS
regressions where supported. Build/pack/proof writers share outputs: coordinate
ordering with A1, avoid parallel artifact regeneration and evidence clobbering.
Run scoped checks during coding, full affected cohorts after integration and
required final sequences on the sealed candidate; no expensive repeated all-suite
runs without new changes/failed checks/coverage reason.

Prepare final source/docs and own authorized local commits before last proof seal.
After source/artifact/covered-doc/HEAD changes, invalidate and rerun under identity
policy. Archive identity collision, stale green artifacts and dropped failure
reasons are real failures. A1 owns common aggregate `pnpm verify`,
`pnpm rust:verify`, `pnpm rust:verify-release` and final coverage; supply reproducible
ERP steps and collect actual dispositions, not guessed global PASS.

## 13. Execution loop, boundaries and continuation

```text
read copy/main baseline + latest handoff
→ select highest-risk unblocked required ERP behavior
→ preserve/complete implementation incrementally
→ self-review + scoped real verification + fix
→ publish small packet → integrated affected recheck
→ update own scope/checkpoint/evidence → continue remaining work
```

Resume existing active slice and already valid packets. Finish remaining R07/R08
by actual dependency graph while closing R04 capacity, complete R06 commerce,
read/UI/configuration and Rust reuse gaps. Integrate available packets promptly;
do not postpone all product surfaces until every kernel exists or restart earlier
cohorts from zero because a newer mandate was issued.

No routine questions about Rust choice, provider, first business model, hosting,
crate naming, local test setup or next module order. Keep owner updated briefly;
status update is not a close-out. No “mau lanjut?”, “ready?”, “next tinggal X” or
“tunggu Codex” while required independent work remains. Self-review and meaningful
tests still happen throughout; only independent audit waits until final candidate.

Use direct commands/managed sessions with actual exit/log paths. Record PID/session
and collect completion; no stale monitor loops waiting for buffered text. Checkpoint
before context/session/quota limits and resume exact next action through available
continuation. Prompt cannot remove provider hard limits; if interrupted, record
incomplete state, not a fictional still-running background process.

If a genuine shared/external dependency blocks work, publish exact reproducer,
requested interface/packet and affected proofs. Continue independent domain work.
When no useful independent item remains, report pending/incomplete honestly and
resume when dependency changes; do not silently take over A1 paths or busy-wait.

- Remain local/free, existing Gemini/privacy boundaries if needed; no paid calls,
  deploy/publish/push, real payments/email/signatures or owner cutover implied.
- Preserve `.env`, `.local`, owner DB/settings/keys, historical releases and
  unrelated dirty/user processes. No source/data uploads to external services.
- Latest AGENTS requires manual skill activation. Ordinary work directly; no
  automatic ECC/e2e/learning-loop/Playwright loading, extra agents, hooks,
  observers, analysis-daemon setup or installed-tool auto-start. A named skill is
  selected only by explicit owner invocation; preserve its task-owned lifecycle.
- Use unique owned PostgreSQL/ports/browser profiles/scratch/PID paths. Stop task
  services on success/failure, preserve A1/owner tasks. No blanket `pkill`,
  `close-all`, reaping live unrelated fixtures or new boot/login background jobs.
- Local coherent commits follow standing authorization, active hooks and exact
  owned changes. No `git add .`, imported-baseline ownership claims, reset/amend,
  bypassing hooks or duplicate staging of A1 shared work. No remote push.

## 14. Final ERP handoff and definition of done

Keep own current summary at top of checkpoint: module/behavior IDs done/remaining,
copy/current main identity, packet dispositions, source/artifact versions, actual
command/exits/proofs, failed/incomplete paths, pending processes and next action.
Old tables remain historical, not competing next-task instructions. Do not mark
zero gaps by counting modules/files or call narrow kernel proof full kit acceptance.

Deliver/update:

1. `docs/rust/archxy2/ERP_SCOPE.md` and `ERP_ACCEPTANCE_MATRIX.md`: entire frozen
   domain surface, corrected deltas, actual Rust/API/UI/consumer/data witness map.
2. `docs/rust/archxy2/ERP_COMPLETION_REPORT.md`: each domain/cohort before/after,
   current implementation/local/integrated status, A/C analogue closures,
   configuration/availability, actual consumer pairs and explicit remaining kit/
   future/live boundaries. No fabricated Codex or legal/production acceptance.
3. `docs/rust/archxy2/ERP_AUDIT_PACKET.md`: exact code range/dirty overlay and
   landed-main identity, locks/config/migration/binary/UI hashes, clean bootstrap/
   synthetic demo/run/stop commands, all producing commands/exits/raw logs,
   failure/race/recovery/upgrade/restore reproducers and integration dispositions.
4. Versioned handoff packets + machine-readable ERP verdict/coverage; A1 can
   reproduce whole R04–R08 proofs and include them in final global packet.
   Reconcile main docs via integration writer; do not overwrite A1 own state.

**ERP implementation complete requires:**

- All required commands, reads, tools, service/data and UI behavior for every
  frozen R04–R08 domain exists and is meaningful; no required placeholder,
  unreachable view, missing library adapter or fake receipt effect.
- Domain/security/reference/stock/occupancy/financial/capacity invariants, actual
  current grants and shared authorization/cohort boundaries proven on PostgreSQL.
- Complete packaged Rust consumer reuse, actual host/Leptos workflows, pending
  recovery, module configuration/disable and required data/upgrade/restore proofs
  run, with truthful producer mappings and current identities.
- All required owned packets landed, equivalent shared registrations/UI/readiness/
  command/CLI/gate contracts integrated, and affected ERP proofs PASS on the same
  main candidate. Private-copy PASS or “patch applies” alone is not completion.
- Report/coverage/audit packet exact and reproducible, task-owned services cleaned,
  owner data/private settings untouched, `independent_review: pending`.

If all ERP behavior is locally verified but shared integration is unavailable,
report **“ERP locally verified — integration pending”** with exact packets/proofs.
If ERP integration is verified but non-ERP global work remains, report the scoped
ERP result and outstanding A1 dependency; do not claim whole Full Rust release or
request a partial Codex acceptance. A1 owns final aggregate verification/closure.

When the criteria hold, hand off:

**“Archxy 2 ERP implementation complete — integrated ERP evidence ready for the
final candidate audit.”**

Include exact main candidate, scope/proof verdict and final report links. Codex
audits the completed combined candidate after A1 finishes its remaining lane;
do not call for audit after each module or sign off the implementation yourself.
