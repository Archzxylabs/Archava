# Audit Archava — 14 Oktober 2026

Audit source commit `a462591`, branch `main`, dengan working tree bersih saat mulai.
Baseline pembanding: review terakhir `d06f6e9` / profile 1.11.0. Codex melakukan
review dan verifikasi; product implementation tetap tugas implementator.

**Kesimpulan: reusable library berkembang nyata, tetapi checkpoint 1.17.0 belum
diterima oleh review ini. Perbaiki authorization dan integritas domain sebelum
menambah domain atau mengandalkan kit assembly untuk penggunaan nyata.** Tidak
diperlukan rewrite framework atau pemindahan seluruh folder.

## Cakupan dan batas audit

Ditinjau: inventory workspace/package, perubahan sejak 1.11.0, PRD/architecture/
status/kit ledger, host/operator composition, release manifests/archives, security
runner dan tujuh domain baru beserta service, contracts, persistence, migrations,
tools dan proof patterns. Pendalaman perilaku memakai installed artifacts dan
PostgreSQL sungguhan dengan runtime role non-owner/non-bypass-RLS.

Ini bukan klaim membaca setiap baris 36 ribu baris baru, menguji setiap kombinasi
kit, atau mengulang semua browser/provider/template/upgrade proofs. Owner database,
credentials, environment keys, settings dan backups tidak diubah. Tidak ada paid
provider calls, push, deployment atau pengiriman data owner.

## Current state

| Area                | Keadaan yang diamati                                                                                                                                   |
| ------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Inventory           | 68 workspace projects; 60 internal compiled artifacts; 22 database ledgers; 18 external pins                                                           |
| Current profile     | 1.17.0; host 0.20.0; assembly contract 16.0.0                                                                                                          |
| Pertumbuhan         | Assets, subscriptions, attendance, fulfillment, billing, laundry, field-services; tujuh domain/persistence pairs sejak 1.11.0                          |
| Struktur            | TypeScript modular monolith, package per domain dan adapter PostgreSQL, local owner host sebagai consumer; standalone realtime template tetap terpisah |
| Reuse               | Tujuh domain mempunyai dua cold consumers berbasis artifact; minimal installation masih punya blind spot dependency (A06)                              |
| Full kit acceptance | 0 dari 40; narrow domain capabilities tidak menyelesaikan seluruh hotel, restoran, ERP atau bisnis lainnya                                             |
| Security evidence   | Current report 83 controls; build-status text masih 80. Matriks tidak membuktikan seluruh permission semantics (A01–A03)                               |
| Archive             | 1.17.0 archived library mempunyai 58 artifacts/assembly 15, sementara current mempunyai 60/assembly 16 (A07)                                           |

Separation antara deterministic domain dan LLM, private data, forced tenant RLS,
checksummed migrations, compiled consumers, append-only business evidence dan
human mutation boundaries adalah fondasi yang layak dipertahankan. Arsitektur
yang diterapkan masih sesuai modular monolith untuk tim kecil. Masalah terbesar
adalah kontrak security yang tidak konsisten antar repository, integritas relasi
domain, dan acceptance yang terlalu mengandalkan happy-path proofs.

## Fresh verification

| Pemeriksaan yang dijalankan ulang             | Hasil                                                                               |
| --------------------------------------------- | ----------------------------------------------------------------------------------- |
| `pnpm verify` via Node 22.22.3 / pnpm 10.15.1 | Seluruh 8 gates lulus; 1.697 unit tests / 130 files                                 |
| Assets domain proof                           | 18 checks lulus                                                                     |
| Subscriptions domain proof                    | 18 checks lulus                                                                     |
| Attendance domain proof                       | 18 checks lulus                                                                     |
| Fulfillment domain proof                      | 27 checks lulus                                                                     |
| Billing domain proof                          | 30 checks lulus                                                                     |
| Laundry domain proof                          | 30 checks lulus                                                                     |
| Field-services domain proof                   | 32 checks lulus                                                                     |
| Field-services compiled host/browser proof    | 6 checks lulus                                                                      |
| Additional adverse PostgreSQL scenarios       | Menghasilkan pelanggaran yang dirinci di A01–A05/A08, walaupun suite existing lulus |
| Minimal field-services artifact import        | Gagal `ERR_MODULE_NOT_FOUND` untuk Zod; A06                                         |

Domain proofs di atas berjumlah 173 checks. Pengujian tambahan menggunakan
principal owner, unrelated assigned staff, active zero-grant viewer dan service
yang bertindak untuk viewer. Semua data pengujian sintetis di cluster/socket
sementara; cluster ditutup dan dibersihkan sesudah pengujian.

Security verifier tanpa `--run` juga dijalankan dan **exit 1** dengan
`Mismatched artifact hash: organizations`. Ini terjadi sesudah audit menjalankan
root reuse gate yang meregenerasi `dist/library`; bukan klaim security runner
awal Gemini sudah gagal pada snapshot sebelum repack. Guard stale evidence
bekerja, tetapi current review outputs belum menjadi satu accepted release bundle.
Current host artifacts tidak direbuild dalam audit ini. Implementator wajib
membekukan final pack/build lalu meregenerasi seluruh dependent proofs sebelum
release acceptance; jangan mengedit hashes/report metadata untuk memaksa gate lulus.

Logs lokal review: `/tmp/archava-audit-20261014-root.log`,
`/tmp/archava-audit-<domain>.log`,
`/tmp/archava-audit-field-services-host.log`,
`/tmp/archava-audit-20261014-domains.jsonl`. Reproducer sementara:
`/tmp/archava-audit-20261014-domains.mjs`. Logs `/tmp` bukan release archive.

## Temuan terverifikasi

### A01 — P1: command recovery/replay melewati current authorization

**Enam domain terdampak:** subscriptions, attendance, fulfillment, billing, laundry
dan field-services. Assets menolak ketiga skenario dan menjadi positive control.

Pada masing-masing enam domain, memakai UUID command owner yang sudah committed:

1. Active viewer dengan permissions kosong memperoleh status recorded/accepted
   dari recovery command owner.
2. Unrelated assigned staff dapat replay exact owner command dan memperoleh hasil.
3. Owner dapat replay cached receipt setelah seluruh grants owner dicabut.

Ini pelanggaran akses hasil dan actor/current-permission binding. Audit ini tidak
membuktikan efek bisnis kedua dari replay UUID identik tersebut. UUID yang diketahui
tidak boleh menjadi pengganti authorization, walaupun record masih dalam tenant
yang sama dan tenant RLS tetap bekerja.

Contoh source: [subscriptions replay](../packages/subscriptions-pg/src/repository.ts),
`execute` line 204 / recovery 615; attendance 124/531; fulfillment 206/612;
billing 224/700; laundry 203/587; field-services 186/527. Fingerprint di repository
terdampak tidak mengikat actor, dan cached return terjadi sebelum operation grant.
Recovery lookup hanya mengikat tenant dan command identifier.

### A02 — P1: assigned resource scope diabaikan

Terbukti di public services menggunakan runtime PostgreSQL:

- Staff `scope: assigned` membaca field job owner lain melalui `detail`, lalu
  `assign_job` mengubah job itu menjadi assigned dengan revision 1 → 2.
- Staff `scope: assigned` membaca invoice owner lain, lalu `issue_invoice`
  mengubahnya menjadi issued dengan revision 1 → 2.

Staff memiliki operation permission, tetapi tidak mempunyai authority untuk
resource tersebut. Field list mempunyai filter scope, sedangkan detail/mutation
tidak mengikuti filter itu. Menyembunyikan row dari list tidak membatasi API ID.

Source: field-services `loadJob` / `getJobDetail` di
[repository](../packages/field-services-pg/src/repository.ts), termasuk line 606;
billing `loadInvoice` / `getInvoiceDetail` di
[repository](../packages/billing-pg/src/repository.ts), termasuk line 762.
Semua read, child records, mutations, audit dan recovery keenam domain perlu
conformance review; jangan hanya memperbaiki dua route yang direproduksi.

### A03 — P1: service/human permission intersection hilang

Service memiliki domain grants dan `scope: all`, tetapi `onBehalfOf` menunjuk
human aktif tanpa grants. Attendance, fulfillment, billing, laundry dan
field-services tetap mengembalikan satu record dari list masing-masing.
Assets dan subscriptions menolak skenario dengan `forbidden`.

Transaction pada lima domain hanya membaca membership `principal.id`, tidak
memvalidasi membership/kind/permissions/scope human yang diwakili. Perbaikan wajib
mengikuti trusted identity dan service/human intersection existing. Jangan
menambah automation mutation authority sebagai jalan keluar.

### A04 — P1: attendance check-in tidak mengikat roster ke session

Roster dibuat untuk sesi A. `record_check_in` dengan roster itu dan `sessionId`
sesi B tetap committed; row roster berubah attended, sedangkan appended check-in
event menunjuk sesi B. Query SQL setelah commit membuktikan
`roster.session_id != check_in_event.session_id`.

[Repository](../packages/attendance-pg/src/repository.ts) line 288 mengunci session
dan roster secara terpisah tanpa memeriksa hubungan keduanya. Migration memiliki
FK roster tetapi tidak mengikat event ke pasangan roster/session. Perbaiki binding
di application dan database; periksa counters serta concurrency/rollback. Jangan
menulis ulang migration accepted atau diam-diam mengubah historical evidence.

### A05 — P1: entitlement berbayar aktif ketika subscription belum dibayar

Create subscription `unitPrice: 100000` dengan initial metered entitlement,
tanpa `record_payment`. Subscription tetap `draft`, paymentReference `null`,
tetapi `consume_usage` committed dan consumedQuantity menjadi 1.

[Repository](../packages/subscriptions-pg/src/repository.ts) line 374 memeriksa
beberapa state negatif tetapi tidak menolak draft. Initial entitlements sudah
aktif. Ini melanggar advertised activation-before-use lifecycle. Tetapkan
policy paid/free/trial yang deterministik dan eksplisit; default paid draft tidak
memberi hak konsumsi. Manual payment reference juga jangan diklaim sebagai
provider-verified settlement tanpa adapter/verifikasi yang benar-benar ada.

### A06 — P1: field-services artifact tidak bisa dipakai independen

[Source manifest](../packages/field-services/package.json) mempunyai
`dependencies: {}`, sementara source dan compiled contracts mengimpor `zod`.
Tarball diekstrak sebagai satu installed module dalam consumer sementara di luar
workspace; import `@archava/field-services` exit 1 dengan `ERR_MODULE_NOT_FOUND`.

Cold consumers existing menginstall seluruh release/profile dan external pins,
sehingga Zod tersedia secara kebetulan. Deklarasikan pinned dependency existing
dan verifikasi minimal install dependency closure setiap artifact. Jangan
mengatasi error dengan menambahkan semua platform packages pada consumer minimal.

### A07 — P1: profile identity dan archive tidak konsisten

Laundry commit `9b05321` dan field-services commit `a462591` sama-sama memakai
profile 1.17.0 / host 0.20.0, padahal field-services menambah artifact dan assembly.

| Output                                              | Artifacts | Assembly |
| --------------------------------------------------- | --------- | -------- |
| Current config/library/runtime                      | 60        | 16.0.0   |
| `dist/releases/1.17.0/library/release-profile.json` | 58        | 15.0.0   |
| Sanitized release manifest yang sekarang diklaim    | 60        | 16.0.0   |

Jangan overwrite archive lama untuk membuat angka cocok. Tetapkan baseline
identity yang valid, lalu buat release baru dengan coherent package/host/config/
kits/assembly versions dan exact archived bytes. Hindari klaim prior release
yang berubah sebagai rollback/upgrade evidence baru.

### A08 — P2: appointment/asset references belum mempunyai authority binding

Field job dengan `appointmentRef: NONEXISTENT_APPOINTMENT` dan
`assetRef: NONEXISTENT_ASSET` berhasil dibuat. Host/service tidak memanggil owned
scheduling/assets resolver dan assembly hanya mewajibkan contacts.

Dokumen menyebut dispatch terikat appointment scheduler. Yang dibuktikan sekarang
adalah ledger dengan required string reference. Tentukan boundary secara jujur:
native appointment-bound scope membutuhkan trusted resolver/adapter, tenant/
resource checks dan lifecycle rules; bila tetap external reference-only, jangan
mengklaim appointment telah diverifikasi atau reservasi resource telah terjadi.

### A09 — P2: architecture/status documentation drift

`docs/ARCHITECTURE.md` masih profile 1.9.0 / 46 workspace projects; runtime sekarang
1.17.0 / 68. Business capability matrix masih mencatat asset/subscription/invoicing
sebagai Later/Planned. Build status mencatat 80 security controls, actual report 83.
Links ke `ORGANIZATIONS_MODULE.md`, `PROPOSALS_MODULE.md`, `CAPACITY_MODULE.md`
mengarah ke file yang belum ada. Field guide menyebut optimistic revision locking,
tetapi increment revision tanpa caller expectedRevision bukan stale-write refusal.

Status awal file implementator juga menyebut domain baru CLOSED sebelum semantic
authorization tests di atas diterapkan. Perbarui dokumen dari accepted evidence,
bedakan sejarah, implementator report, candidate dan reviewer acceptance.

## Proposed incremental architecture

Pertahankan `packages/<domain>` / `<domain>-pg`, `apps/crm`, existing core/assistant,
dan standalone template. Folder bukan blocker. Tambahkan contract conformance
harness yang benar-benar sama lintas domain: current-grant refusal, assigned scope,
actor-bound replay, service/human intersection, tenant separation, response recovery,
dependency closure dan migration compatibility. Domain masih memiliki aturan
bisnis dan SQL sendiri; shared security primitives boleh diekstrak secara kecil
sesudah behavior diuji, tanpa membuat universal ERP atau rewrite semua repository.

Minimum module contract saat ini adalah fixed tested release composition, bukan
arbitrary compatible plugin/version graph. Broad dependency ranges, conflicts,
upgrade combinations, uninstall lifecycle, universal deployment orchestration dan
full observability tetap PRD work; jangan menganggap boolean module flags telah
menutup semua kebutuhan itu.

## Urutan implementator

1. **MUST BUILD NOW:** A01–A06; shared meaningful conformance tests; additive data
   integrity migrations; release identity A07; fresh regression/upgrade/recovery
   dan truthful acceptance docs A09. Domain baru ditahan sampai ini selesai.
2. **SHOULD BUILD NEXT:** A08 trusted cross-domain references; compatibility fixture
   dari actual previous profiles yang pernah deployed; cleanup duplicated boundary
   logic melalui small shared helpers; module health/readiness evidence.
3. **BUILD LATER:** sisa general/specialist kits menurut demand; richer SLA,
   business observability, production operations dan supported version combinations.
4. **DO NOT BUILD YET:** rewrite framework, split microservices, universal ERP,
   Kubernetes, unrestricted LLM execution, atau claim seluruh kit siap dirakit.

Existing root/domain passes dipertahankan. Fix harus menambah negative evidence
yang menolak kasus di atas, bukan melemahkan tests agar tetap hijau. Owner baseline
dan archive lama dilindungi; perubahan data privat tidak diperlukan untuk
membuktikan perbaikan pada fixtures. Setelah synthetic acceptance, upgrade owner
mengikuti baseline/backup/preservation policy existing.
