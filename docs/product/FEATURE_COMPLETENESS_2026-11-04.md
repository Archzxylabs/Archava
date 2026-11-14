# Kelengkapan fitur Archava — 4 November 2026

**Verdict: belum lengkap sebagai keseluruhan produk.** Audit mencakup 18 domain
bisnis dan 141 operasi command yang dideklarasikan. Sebanyak 58
pemeriksaan requirement dijalankan pada Chromium, HTTP native dan PostgreSQL:
**30 terpenuhi, 28 menunjukkan gap**. Pemeriksaan yang berkaitan dapat
menunjuk gap yang sama; angka ini bukan jumlah bug unik atau persentase kesiapan.

Scope berasal dari [launch readiness](LAUNCH_READINESS.md), kontrak [modul](../modules/README.md),
[owner configuration](OWNER_CONFIGURATION_PRD.md) dan batas pada [PRD Rust](PRD_FULL_RUST.md).
Modul yang sejak awal API-only tetap dicatat sebagai API-only; keberadaannya tidak
menjadi janji bahwa semua layar industri wajib selesai untuk pilot Tier 1.
Future backlog seperti 40 kit penuh, semua kebutuhan ERP dan native mobile tidak
menjadi denominator produk yang telah dikirim.

## Metode dan bukti

- Build/source yang diuji: `ec008efbdca4bc1297d642144d77dc71b22700000d943491d72eba4e4b4aae11`,
  398 input. Hash binary/WASM cocok dengan source dan kandidat rilis lokal.
- Browser terbaru: `target/e2e/product-audit-20261104-05/result.json`. **38 pemeriksaan baseline
  tetap lulus**, ditambah 58 pemeriksaan requirement di `product.requirements`.
  Nol page error dan fixture cleanup 0. `status: passed` pada runner berarti
  eksekusi audit selesai; `product.allObservedRequirementsMet: false` menyatakan
  kelengkapan produk gagal. Join evidence keluar dengan **exit 2**.
- Bukti API digunakan ulang dari full Rust gate yang sudah lulus: **188 tes dalam
  18 suite domain**, bagian dari 1.109 tes Rust. Bukti ini tidak diklaim sebagai
  eksekusi baru pada audit ini. Business/build input masih cocok dengan baseline;
  pengelompokan akhir metrics pada baseline telah diperiksa dengan library gate.
- Seluruh API read probe menggunakan human Alice dengan grant nyata pada ledger
  disposable masing-masing; 403 akibat fixture tanpa grant tidak diklasifikasikan
  sebagai fitur hilang. Role negatif/revocation tetap punya bukti suite baseline.
- Mutasi tambahan hanya mengenai data sintetis milik runner. Tidak ada database
  pengguna, domain publik, provider berbayar atau secret live disentuh.
- Inventory 141 operasi berasal dari kontrak canonical. Referensi string pada
  source UI/tes dilaporkan sebagai petunjuk wiring, **bukan branch/line coverage**.
  Passing suite tidak membuktikan seluruh cabang 141 operasi pernah dieksekusi.

## Temuan yang harus memengaruhi klaim produk

### PC-01 — P1: Assistant mengakui receipt yang salah

Human-confirmed completion biasa berhasil: UI menunjukkan receipt dan SQL
membuktikan follow-up berstatus `completed`. Pada tes berikutnya, request yang
sah tetap mencapai server dan commit; runner mengganti acknowledgement yang
sudah kembali menjadi receipt dengan `requestId=wrong-request` dan
`id=wrong-resource`. UI justru menampilkan:

> Recorded. Receipt: wrong-resource (revision 1)

Request asli `7b8eff5a-cdc3-4048-b43a-b57336962479` tetap mempunyai status durable
`committed` dan receipt resource `1bf83a10-5d85-49c0-afbc-5e91d89d4a0c`,
revision 2. Acknowledgement palsu tidak cocok
terhadap key/resource tersebut. Ini fault-injection pada fixture, bukan bukti
bahwa penyerang luar dapat memalsukan respons jaringan.

[Desk acknowledgement handling](../../crates/slice/src/desk.rs#L201) hanya
memeriksa `decision == receipt`, lalu menghapus pending state dan menampilkan
resource yang diterimanya. Jalur ini harus mencocokkan key, resource dan revision
serta mempertahankan recovery ketika acknowledgement salah/tidak pasti.
Screenshot: `target/e2e/product-audit-20261104-05/product-assistant-receipt.png`.
**Bug direproduksi; source produk belum diperbaiki pada audit kelengkapan ini.**
Sembilan temuan audit sebelumnya tetap terpisah; kandidat yang lolos 41 checks
sebelumnya belum menguji jalur ini.

### PC-02 — Owner self-service baru mencakup konfigurasi CRM yang terbatas

`/own/chat` dan `/own/plan` benar-benar menghasilkan plan hash yang sama untuk
`disable crm-pipeline`; fitur tersebut bukan sekadar proposal. Preview, confirm,
history/restore dan grant/replay juga punya bukti baseline.

Namun catalogue yang terpasang hanya `crm-contacts` dan `crm-pipeline`. Instruksi
“Bisnis saya hotel, tambahkan laundry tanpa restaurant” menghasilkan 400
`unrecognized_request`, dan Settings tidak menyediakan input percakapan.
Discovery bahasa bisnis, composable modules lintas domain dan owner journey yang
lebih luas pada PRD belum terpenuhi. Source: [catalogue dan chat](../../crates/host/src/own.rs),
[Settings](../../crates/slice/src/owner.rs).

### PC-03 — 13 domain belum punya layar operator

Organizations, Proposals/change requests, Projects, Foodservice, Resource reservations, Accommodation, Assets, Subscriptions, Attendance, Laundry, Field services, Fulfillment, Formal education.

Browser memeriksa URL/module untuk setiap domain. Navigasi module yang tidak
terdaftar jatuh ke Catalog; tab yang tersedia hanya Catalog, Sales orders,
Purchasing, Invoices, Support dan Missions. HTTP 200/hydration tidak diperlakukan
sebagai bukti bahwa layar domain tersedia. Source: [Operations dispatch](../../crates/slice/src/operations/mod.rs#L407).
Lima domain bisnis yang memiliki layar adalah CRM, Commerce, Purchasing, Billing
serta Support; Missions adalah komponen lintas domain.

### PC-04 — Tujuh domain hanya mengekspos mutation/status, tanpa collection read

Organizations, Proposals/change requests, Projects, Foodservice, Resource reservations, Accommodation, Assets.

Command/status route memang terpasang dan tes API-nya lulus. Tidak ada route
collection/list bisnis untuk ketujuh domain pada [route registry](../../crates/host/src/routes/erp.rs).
Status receipt berdasarkan request ID tidak menggantikan daftar record bisnis.
Ini batas kelengkapan host yang dikirim, bukan kegagalan otorisasi pada probe.

### PC-05 — Mission executor belum mengeksekusi mutasi bisnis

Mission dengan tool `crm.followup.complete` berhasil dibuat (201), tetapi
`run-next` menjawab 400: “This mission tool is not registered for operator
execution.” Tiga tool terpasang hanya membaca workspace Commerce, Purchasing
dan Billing. Flow DAG/read tools, persistence dan supervisor APIs yang ada tetap
dibedakan dari worker/scheduler atau eksekusi mutasi nyata.
Source: [run_next allowlist](../../crates/host/src/cockpit.rs#L1020).

### PC-06 — Feed aksi proaktif belum mempunyai dispatch worker

Feed terbaca 200; eksekusi `generate_reorder_po` memberi 501
`action_execution_not_implemented`. Feed menghasilkan data demonstrasi dan tidak
membuktikan reminder dikirim atau PO dibuat. Source: [feed](../../crates/host/src/feed.rs).

### PC-07 — Impor data hanya sampai preview

Reconcile tabel kontak valid berhasil 200. Commit preview yang sama menjawab
501 `migration_commit_not_implemented`. Tidak ada klaim import durable yang
lulus. Source: [ingestion](../../crates/host/src/ingestion.rs).

### PC-08 — Voice/media produksi belum tersedia

Native binary tanpa konfigurasi menjawab 503 `presence_unconfigured`. Ini sendiri
membuktikan jalur tidak tersedia pada konfigurasi yang diuji. Inspeksi source
menjelaskan batas yang lebih jauh: [transport_available](../../crates/providers/src/voice.rs#L77)
hanya benar untuk `loopback_mode`; transport live tetap mengembalikan
`TransportUnavailable`. Jadi memasukkan credential saja tidak menyelesaikan bridge.
Tes loopback presence yang lulus bukan percakapan audio/video LiveKit/Gemini nyata.

## Aksi Tier 1 yang masih terbatas pada API

Inspeksi source UI juga menemukan batas yang lebih kecil daripada satu modul:

- Purchasing tidak menyediakan kontrol untuk `supplier.set-active`, `offer.set-cost`,
  `offer.set-active` dan `order.replace-lines` pada layar native sekarang.
- Support tidak menyediakan kontrol untuk `case.cancel`, `case.assign`,
  `case.set-priority` dan `case.set-target` pada layar native sekarang.

Referensi: [Purchasing UI](../../crates/slice/src/operations/purchasing.rs) dan
[Support UI](../../crates/slice/src/operations/support.rs). Ini hasil penelusuran
wiring source; tidak diklaim sebagai delapan percobaan UI baru. Sebaliknya,
`record_provider_event` Billing memang merupakan jalur service/provider, sehingga
ketiadaan tombol operator untuk memaksakan pembayaran adalah batas yang benar.

## Matriks domain bisnis

Kolom layar berarti create control domain ditemukan di browser, bukan seluruh
aksi/exception path domain sudah lengkap. Kolom read berarti collection endpoint
yang relevan terpasang dan menghasilkan 200 untuk human yang berwenang.

| Domain | Operasi | Suite API lulus (digunakan ulang) | Layar native | Collection read |
| --- | ---: | ---: | --- | --- |
| CRM | 7 | 36 | Ada; journey inti diuji | 200 — `/contacts` |
| Commerce | 8 | 4 | Ada; journey inti diuji | 200 — `/erp/commerce/workspace` |
| Purchasing | 11 | 10 | Ada; journey inti diuji | 200 — `/erp/purchasing/workspace` |
| Billing | 6 | 14 | Ada; journey inti diuji | 200 — `/erp/billing/workspace` |
| Support | 12 | 12 | Ada; journey inti diuji | 200 — `/erp/support/cases` |
| Organizations | 6 | 10 | Belum tersedia | Belum terpasang |
| Proposals/change requests | 14 | 10 | Belum tersedia | Belum terpasang |
| Projects | 6 | 10 | Belum tersedia | Belum terpasang |
| Foodservice | 13 | 9 | Belum tersedia | Belum terpasang |
| Resource reservations | 7 | 10 | Belum tersedia | Belum terpasang |
| Accommodation | 11 | 10 | Belum tersedia | Belum terpasang |
| Assets | 5 | 10 | Belum tersedia | Belum terpasang |
| Subscriptions | 5 | 19 | Belum tersedia | 200 — `/erp/subscriptions/subscriptions` |
| Attendance | 5 | 16 | Belum tersedia | 200 — `/erp/attendance/sessions` |
| Laundry | 4 | 2 | Belum tersedia | 200 — `/erp/laundry/orders` |
| Field services | 7 | 2 | Belum tersedia | 200 — `/erp/field/jobs` |
| Fulfillment | 5 | 2 | Belum tersedia | 200 — `/erp/fulfillment/jobs` |
| Formal education | 9 | 2 | Belum tersedia | 200 — `/erp/education/enrollments` |

## Hasil tambahan yang benar-benar berfungsi

Selain baseline create/follow-up, stock/order, purchasing/receiving, invoice issue,
support resolve, payment/email loopback, owner confirm dan restart/recovery:

- Contact edit, activity add, opportunity create dan stage move berhasil lewat UI
  dan dikonfirmasi langsung pada ledger.
- Retur parsial restock mengembalikan satu unit beserta acquisition basis asli;
  retur discard unit berikutnya menutup order tanpa menambah stok.
- Koreksi invoice 1.000 memperbarui outstanding menjadi 31.000 tanpa settlement;
  void eligible invoice berhasil dan settled tetap 0.
- Support close/reopen, wait/resume serta internal note berhasil; note tercatat
  tepat sekali pada SQL dan history UI.
- Assistant SQL overview dan satu completion dengan konfirmasi manusia berhasil.
  Jalur acknowledgement yang salah tetap gagal seperti PC-01.

## Batas lain pada scope produk

| Area | Ada dan terbukti terbatas | Belum menjadi bukti produk lengkap |
| --- | --- | --- |
| Email | Resend loader, snapshot/outbox/retry dan UI lokal | Live delivery; klaim SMTP/Postmark tidak dibuktikan oleh registry yang hanya memasang Resend |
| Pembayaran | Midtrans correlation, verified loopback settlement/refund dan recovery | Merchant/provider-live acceptance yang owner tunda |
| Assistant | Deterministic CRM planner, data SQL, preparation dan human-confirmed command | Gemini/model live dalam serving composition, autonomous multi-domain worker |
| Orbz | Tenant SQL, scope/authority, cold restart dan temporal history pada baseline hardening | UI memory workflow, retrieval quality/reflection end-to-end dan semantic model integration |
| MCP | Struktur protocol/dispatch library | Authenticated deployed transport dan outbound provider nyata |
| Campaigns | Trigger/plan dan in-memory mission registration | Trigger worker kontinu, dispatch, delivery dan reconciliation |
| RPA | Action recipe/planning library | Browser executor produksi dan eksekusi portal nyata |
| Evals | Local/synthetic checks dan replaceable target | Hasil terhadap model dan HTTP deployment nyata |
| Pricing/rates/assembly/capacity/context | Library/planner/unit-integration contracts yang ikut full gate | Seluruh proposal/contract/payment/capacity customer journey yang terpasang |
| Enterprise/hardware/messaging | Journal/tax calculations, protocol bytes/parsers dan payload models | Authorized ledger posting/UI, printer/device transport atau WhatsApp live delivery |
| Embed SDK | Guardrails/allowlist dan explicit verified callbacks pada source | Complete external-origin consumer acceptance pada release ini |
| Operasi | Native package, install/bootstrap, encrypted restore, writer fencing dan TLS/load lokal pada baseline | Alert delivery/on-call, offsite/WAL recovery dan actual-user acceptance |

Batas library di atas adalah source/evidence review, bukan runtime probe baru.
Referensi: [frontier components](../architecture/FRONTIER_AGENTIC_CRATES.md),
[enterprise engines](../architecture/ENTERPRISE_ENGINES.md), [Cockpit](../architecture/COCKPIT_TASK_ARCHITECTURE.md),
[release candidate](RELEASE_CANDIDATE_2026-11-04.md). Provider live tetap ditunda;
audit ini tidak mengubah keputusan tersebut atau mencoba staging publik.

## Cara mengulang

Gunakan build server/preview/WASM dan bukti full Rust yang cocok dengan business
source saat ini. Contoh path berikut mengacu pada output lokal sesi ini:

```bash
ARCHAVA_E2E_PRODUCT=1 node tools/e2e/verify.mjs --output target/e2e/product-audit-next
node tools/product/audit.mjs \
  --browser target/e2e/product-audit-next/result.json \
  --rust-log target/remediation-20261104/verify-01.log \
  --baseline target/remediation-20261104/delivery.json \
  --output target/product-audit-next.json
```

Exit 2 berarti gap requirement ditemukan; exit 1 berarti audit gagal selesai.
Exit 0 pada join hanya berarti requirement yang diamati terpenuhi, bukan jaminan
seluruh roadmap atau semua cabang kode telah diterima.

Bukti sesi: `target/product-audit-20261104/inventory.json`,
`requirements.csv`, `operations.csv`, `helper-tests.log` dan
`target/e2e/product-audit-20261104-05/`. Runner 01 gagal karena harness mengasumsikan
semua ledger punya SQL permission CHECK. Runner 02 memakai alias kolom impor yang
tidak didukung. Keduanya diperbaiki pada harness dan tidak dihitung sebagai bug
produk. Runner 03/04 lulus dengan scope lebih kecil; 05 menambahkan journey CRM dan
mission action. Semua log awal dipertahankan. Tidak ada source produk Rust, SQL,
asset atau package rilis diubah untuk menutupi kegagalan.

## Seluruh requirement yang dieksekusi

`met` adalah perilaku spesifik yang berhasil dibuktikan. `gap` adalah requirement
kelengkapan yang belum terpenuhi; bisa berupa fitur belum tersedia atau bug.

| ID | Hasil | Bukti ringkas |
| --- | --- | --- |
| MODULE-crm-operator | **met** | `{"heading":"Contacts","tabs":[],"control":true,"catalogFallback":false}` |
| MODULE-crm-read | **met** | `{"path":"/contacts","status":200}` |
| MODULE-commerce-operator | **met** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":true,"catalogFallback":true}` |
| MODULE-commerce-read | **met** | `{"path":"/erp/commerce/workspace","status":200}` |
| MODULE-purchasing-operator | **met** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":true,"catalogFallback":false}` |
| MODULE-purchasing-read | **met** | `{"path":"/erp/purchasing/workspace","status":200}` |
| MODULE-billing-operator | **met** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":true,"catalogFallback":false}` |
| MODULE-billing-read | **met** | `{"path":"/erp/billing/workspace","status":200}` |
| MODULE-support-operator | **met** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":true,"catalogFallback":false}` |
| MODULE-support-read | **met** | `{"path":"/erp/support/cases","status":200}` |
| MODULE-organizations-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-organizations-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-proposals-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-proposals-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-projects-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-projects-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-foodservice-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-foodservice-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-scheduling-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-scheduling-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-accommodation-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-accommodation-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-assets-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-assets-read | **gap** | `{"path":null,"reason":"No collection read route mounted; command status is not a business list"}` |
| MODULE-subscriptions-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-subscriptions-read | **met** | `{"path":"/erp/subscriptions/subscriptions","status":200}` |
| MODULE-attendance-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-attendance-read | **met** | `{"path":"/erp/attendance/sessions","status":200}` |
| MODULE-laundry-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-laundry-read | **met** | `{"path":"/erp/laundry/orders","status":200}` |
| MODULE-field-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-field-read | **met** | `{"path":"/erp/field/jobs","status":200}` |
| MODULE-fulfillment-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-fulfillment-read | **met** | `{"path":"/erp/fulfillment/jobs","status":200}` |
| MODULE-education-operator | **gap** | `{"heading":"Operations","tabs":["Catalog","Sales orders","Purchasing","Invoices","Support","Missions"],"control":false,"catalogFallback":true}` |
| MODULE-education-read | **met** | `{"path":"/erp/education/enrollments","status":200}` |
| COMMERCE-partial-restock | **met** | `{"quantity":11,"inventoryValueMinor":"110000","restockedCostMinor":"10000"}` |
| COMMERCE-discard-return | **met** | `{"quantity":11,"inventoryValueMinor":"110000","state":"returned"}` |
| BILLING-correction | **met** | `{"correction":1000,"outstanding":31000,"settled":0}` |
| BILLING-void | **met** | `{"status":"void","settled":0}` |
| SUPPORT-close-reopen | **met** | `{"state":"open","cycle":"2"}` |
| SUPPORT-wait-resume | **met** | `{"state":"in_progress"}` |
| SUPPORT-note | **met** | `{"notes":1}` |
| CRM-contact-edit | **met** | `{"organization":"Product audit f47a4f87-d5e3-4e9a-bf16-bf9aa113d259"}` |
| CRM-activity | **met** | `{"activities":1}` |
| CRM-opportunity-create | **met** | `{"id":"83f381ed-ed1e-46bb-9fd4-424074a8c5ba"}` |
| CRM-opportunity-move | **met** | `{"stage":"qualified"}` |
| OWN-bounded-chat-parity | **met** | `{"chatStatus":200,"dashboardStatus":200,"chatHash":"84d3ed695fc999d8","dashboardHash":"84d3ed695fc999d8"}` |
| OWN-business-discovery | **gap** | `{"status":400,"body":{"message":"I can enable or disable a module, or rename a pipeline stage.","reason":"unrecognized_request"}}` |
| OWN-business-modules | **gap** | `{"modules":["crm-contacts","crm-pipeline"]}` |
| OWN-chat-ui | **gap** | `{"preview":true,"restoreControls":1,"chatInput":false}` |
| COCKPIT-business-write | **gap** | `{"missionStatus":201,"executionStatus":400,"body":{"error":{"message":"This mission tool is not registered for operator execution."}}}` |
| FEED-dispatch | **gap** | `{"feedStatus":200,"executeStatus":501,"body":{"action_type":"generate_reorder_po","card_id":"product-audit","error":{"code":"action_execution_not_implemented","message":"Purchase-order drafting is not wired through this route."},"is_simulated":false}}` |
| IMPORT-durable-commit | **gap** | `{"previewStatus":200,"commitStatus":501,"body":{"detail":"No tenant-scoped commit writer is mounted on this host.","error":"migration_commit_not_implemented"}}` |
| VOICE-production-media | **gap** | `{"status":503,"body":{"error":"presence_unconfigured","message":"Realtime presence provider is unconfigured"}}` |
| ASSISTANT-sql-read | **met** | `{"source":"crm","planner":"deterministic-commands-v1 / archava-crm-local","contacts":"7"}` |
| ASSISTANT-confirmed-write | **met** | `{"action":"Recorded. Receipt: ce0d4227-5130-49e5-b661-9f722d6a373f (revision 2)","followupId":"ce0d4227-5130-49e5-b661-9f722d6a373f","databaseStatus":"completed"}` |
| ASSISTANT-exact-receipt | **gap** | `{"display":"Recorded. Receipt: wrong-resource (revision 1)","submittedRequestId":"7b8eff5a-cdc3-4048-b43a-b57336962479","serverStatus":"committed","serverReceipt":{"id":"1bf83a10-5d85-49c0-afbc-5e91d89d4a0c","kind":"followup","revision":2},"acknowledgement":{"requestId":"wrong-request","resourceId":"wrong-resource"}}` |

## Inventaris operasi command

Daftar ini menyatakan kontrak yang ditemukan, bukan 141 sertifikasi readiness.
Detail referensi source dan test names ada di JSON inventory/CSV.

- **CRM**: `contact.create`, `contact.update`, `activity.add`, `followup.create`, `followup.complete`, `opportunity.create`, `opportunity.move`.
- **Commerce**: `product.create`, `product.set`, `stock.adjust`, `order.create`, `order.reserve`, `order.fulfill`, `order.cancel`, `order.return`.
- **Purchasing**: `supplier.create`, `supplier.set-active`, `offer.create`, `offer.set-cost`, `offer.set-active`, `order.create`, `order.replace-lines`, `order.confirm`, `order.cancel`, `order.short-close`, `order.receive`.
- **Billing**: `create_invoice`, `issue_invoice`, `append_correction`, `void_invoice`, `record_payment_attempt`, `record_provider_event`.
- **Support**: `case.create`, `case.start`, `case.wait`, `case.resume`, `case.close`, `case.cancel`, `case.resolve`, `case.reopen`, `case.assign`, `case.set-priority`, `case.set-target`, `note.add`.
- **Organizations**: `create_organization`, `update_organization`, `archive_organization`, `create_affiliation`, `update_affiliation`, `remove_affiliation`.
- **Proposals/change requests**: `create_proposal`, `update_proposal`, `set_scope_items`, `submit_proposal`, `accept_proposal`, `reject_proposal`, `withdraw_proposal`, `link_project`, `create_change_request`, `update_change_request`, `submit_change_request`, `approve_change_request`, `reject_change_request`, `withdraw_change_request`.
- **Projects**: `project.create`, `project.transition`, `task.create`, `task.transition`, `milestone.create`, `milestone.complete`.
- **Foodservice**: `table.create`, `table.set-active`, `menu.create`, `menu.set-price`, `menu.set-active`, `menu.replace-recipe`, `ticket.create`, `ticket.queue`, `ticket.prepare`, `ticket.ready`, `ticket.serve`, `ticket.close`, `ticket.cancel`.
- **Resource reservations**: `resource.create`, `resource.set-active`, `reservation.hold`, `reservation.block`, `reservation.confirm`, `reservation.move`, `reservation.cancel`.
- **Accommodation**: `property.create`, `room-type.create`, `room-type.set-rate`, `room.create`, `room.set-active`, `room.housekeeping`, `stay.hold`, `stay.confirm`, `stay.check-in`, `stay.check-out`, `stay.cancel`.
- **Assets**: `create_asset`, `update_asset`, `handover_asset`, `record_maintenance`, `retire_asset`.
- **Subscriptions**: `create_subscription`, `record_payment`, `consume_usage`, `change_status`, `grant_entitlement`.
- **Attendance**: `create_session`, `register_participant`, `record_check_in`, `update_roster_status`, `change_session_status`.
- **Laundry**: `create_order`, `record_process`, `record_finding`, `change_order_status`.
- **Field services**: `create_job`, `assign_job`, `record_visit`, `append_work_line`, `record_signoff`, `approve_job`, `cancel_job`.
- **Fulfillment**: `create_job`, `record_line`, `record_finding`, `record_custody`, `change_job_status`.
- **Formal education**: `admit_applicant`, `confirm_enrollment`, `enroll_student`, `record_session`, `record_result`, `review_record`, `record_retention`, `graduate_enrollment`, `withdraw_student`.
