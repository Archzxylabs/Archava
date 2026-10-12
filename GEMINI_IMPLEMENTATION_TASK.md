# Gemini — lanjut implementasi Archava

Bro, lu jadi implementator. Codex sekarang dipakai buat orchestration dan review
saat owner memintanya. Lanjutkan pekerjaan di repository yang sudah ada, tuntaskan
implementasi beserta bukti pengujiannya, dan jaga data lokal owner. Baca file ini
sampai habis sebelum mengubah kode.

## 0. Tugas aktif — perbaiki hasil review `f1c0a89`

**Review Codex, 11 Oktober 2026: tiga domain baru BELUM diterima sebagai release.**
Baseline purchasing ada pada `faabcc1`, dengan koreksi dokumentasi `345d039`.
Commit `f1c0a89` menambah organizations, proposals dan capacity. Implementasinya
sudah ada, tetapi review menemukan blocker yang bisa direproduksi. Jangan lanjut
modul berikutnya atau mengklaim end-to-end, zero bugs, maupun zero leaking sebelum
blocker di bawah ditutup.

Bagian ini adalah tugas terbaru dan mengalahkan instruksi snapshot lama di §§2/4.
Bagian lama dipertahankan sebagai konteks; bukan instruksi mengulang purchasing
atau mengembalikan owner ke release 1.9.0.

### Temuan yang sudah dibuktikan

Pengujian akses dan kapasitas memakai installed packages dari `dist/crm-runtime`,
PostgreSQL terisolasi, runtime role non-owner/non-bypass-RLS, dan data sintetis.
Owner database, credentials dan settings tidak diubah oleh review ini.

| ID                      | Temuan                                                                          | Reproduksi / rujukan                                                                                                                                                                                                                                                                                                                                                                                    |
| ----------------------- | ------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R1 — P1                 | Staff dengan scope `assigned` dapat mengubah proposal milik owner lain.         | Staff memiliki proposal permissions, tetapi tidak memiliki proposal tersebut. `update_proposal` tetap berhasil dan revision owner berubah 1 → 2. `packages/proposals-pg/src/repository.ts:443` hanya memeriksa permission; lookup record tidak membatasi owner/scope.                                                                                                                                   |
| R2 — P1                 | Receipt recovery dan replay melewati current authorization pada ketiga domain.  | Viewer aktif, scope `assigned`, permissions kosong, mengetahui UUID command owner: `commandStatus` mengembalikan `committed` beserta receipt untuk organizations, proposals dan capacity. Setelah seluruh operation/read grants actor asli dicabut, replay input identik juga mengembalikan cached receipt. Ini akses hasil tanpa izin; review tidak membuktikan efek bisnis baru dari replay tersebut. |
| R3 — P1                 | Fresh compiled host tidak dapat menjalankan profile baru.                       | Pada home sintetis baru, `operator.mjs setup` exit 0, lalu `doctor` exit 1 dengan PostgreSQL `42P01`. `apps/crm/src/operator.ts:188` hanya memigrasikan modul sampai purchasing; profile sudah mewajibkan tiga ledger tambahan. Host/routes/UI/operator grants untuk tiga domain belum terintegrasi.                                                                                                    |
| R4 — P1                 | Capacity mengalokasikan throughput pool berulang untuk tiap job.                | Pool 200 hours/week, pembagi 5 hari, maxConcurrentJobs 2. Dua alokasi masing-masing 40 jam dengan desired start `2030-01-07T00:00:00Z` sama-sama dijadwalkan selesai `2030-01-08T00:00:00Z`: 80 jam pada pool 40 jam/hari. `packages/capacity-pg/src/repository.ts:281` dan cabang `commit_allocation` memakai seluruh burn rate untuk setiap job.                                                      |
| R5 — P1                 | Release identity berubah tanpa versi baru dan kontrak lama menerima flags baru. | Current library/runtime menyebut 1.10.0 dengan 46 artifacts/15 ledgers; archive accepted `dist/releases/1.10.0/library` menyebut 1.10.0 dengan 40 artifacts/12 ledgers. Host/config/kits/assembly versions belum berubah. Installed `assemblyRequestSchema` menerima flags organizations/proposals/capacity pada contract 2.0.0 dan 7.0.0 ketika baseline fields valid.                                 |
| R6 — acceptance blocker | Verification belum memenuhi gate release.                                       | ESLint enam package baru gagal: **60 errors**. Domain scripts baru hanya mencetak hasil; tidak menyimpan acceptance `results.json`. Belum ada actual host/Chromium proofs untuk tiga domain. Security runner belum memasukkan ketiga domain. `dist/capacity-proof/results.json` yang lama adalah load benchmark, bukan delivery-capacity domain proof.                                                  |

Reproducer lokal review tersedia sementara di `/tmp/archava-f1c-review.mjs`.
Pelajari fixture/hasilnya, lalu buat regression tests permanen di repo. Jangan
menganggap file `/tmp` sebagai release evidence, dan jangan mengubah expectation
agar perilaku tanpa izin menjadi PASS. Log focused lint:
`/tmp/archava-f1c-review-lint.log`.

### Perbaikan wajib, dalam urutan ini

1. **Authorization dan command recovery.** Audit setiap read, mutation, version/
   amendment lookup, pool/allocation lookup, status dan replay di ketiga domain.
   Terapkan current membership, operation permission, resource scope/assignment
   dan human boundary pada server/repository. Tenant RLS tetap wajib, tetapi tenant
   yang sama bukan berarti semua record boleh diakses. Cek scope sesudah mengunci
   record yang akan diubah; race revocation/reassignment harus memiliki kebijakan
   transaksi yang jelas. Pertahankan pola authorization existing, jangan membuat
   bypass atau menaikkan scope menjadi `all` agar proof lulus.
2. **Receipt bukan bearer token.** Recovery wajib memeriksa actor serta current
   grants dan resource authority yang relevan. UUID saja tidak memberi hak melihat
   hasil. Replay cached receipt harus tetap menjalankan current authorization;
   pencabutan izin tidak boleh membuka receipt melalui jalur retry. Simpan binding
   command ke tenant, actor, operation dan input tervalidasi, dengan fingerprint
   deterministik. Organizations sekarang hanya meng-hash `mutation.data`: tambahkan
   operation binding. Pastikan same UUID/body/actor menghasilkan satu efek saja;
   different actor/body/operation ditolak. Authorized lost-response recovery tetap
   bekerja setelah restart, dengan refusal yang tidak membocorkan hasil kepada
   unauthorized caller.
3. **Capacity correctness.** Definisikan arti weeklyCapacity sebagai throughput
   agregat pool, serta arti hours/days/slots dan kalender kerja yang didukung.
   Concurrent jobs berbagi budget pool; slot limit tidak menambah throughput.
   Gunakan satu allocator deterministik untuk estimate dan commit, lalu revalidate
   secara atomik saat commit. Perhitungkan remaining work, alokasi mendatang,
   batas overlap, completion/cancellation dan queue handover. Jangan menghitung
   lima business days lalu menambah 24 jam calendar tanpa kebijakan weekend.
   Kebijakan paling sederhana yang benar boleh dipilih; tulis limitations daripada
   menambah scheduler enterprise. Test fixed clock, weekday/weekend boundary,
   > 1 concurrent job, future desired start, progress, release dan competing commits.
4. **Release/version compatibility.** Buat candidate release baru, misalnya 1.11.0,
   dengan bump host/config/kits dan kontrak assembly yang berubah. Bump package
   versions yang artifact accepted-nya berubah; jangan mengganti bytes di bawah
   identity accepted yang sama. Perbarui pins, lockfile, release contracts, planner
   dan verifier secara konsisten. Kontrak lama menolak field modul yang belum
   diperkenalkan pada versi itu, termasuk field bernilai `false`; old valid plans
   tetap didukung sesuai kebijakan existing. Pertahankan accepted 1.10.0 archive
   beserta checksum. Jangan overwrite archive untuk menyembunyikan mismatch.
5. **Actual host integration.** Tuntaskan operator setup/migrate/grant/user/doctor,
   verified installed ledgers, module flags, server services/routes dan browser
   panels untuk organizations, proposals dan capacity sesuai pola existing.
   Feature enabled tidak otomatis memberikan grants. Public/customer tools tidak
   memperoleh internal access. Browser bundle bebas PG code/secrets. Tunjukkan
   workflow owner lewat actual compiled host dengan staff/viewer refusal,
   CSRF/session checks, tenant switch/logout cleanup, reviewed mutation dan
   idempotent lost-response recovery. Jangan berhenti pada service-level demo.
6. **Evidence dan root gates.** Perbaiki 60 lint errors tanpa blanket suppressions
   atau menurunkan lint policy. Tambahkan scripts root yang jelas untuk tiga domain
   dan host proofs; simpan machine-readable results pada path unik. Gunakan
   `dist/delivery-capacity-proof/` dan `dist/delivery-capacity-host-proof/` agar tidak
   bentrok dengan benchmark `dist/capacity-proof/`. Organizations/proposals juga
   memiliki domain dan host report masing-masing. Reports merekam profile,
   artifact/catalog hashes, checks aktual, dua consumer, limitations dan
   providerCalls 0. Security matrix/runner wajib mensyaratkan proofs baru dan
   menolak report stale; kesamaan string `1.10.0` tidak cukup membuktikan freshness.

### Acceptance sebelum lanjut programme

- Repro R1/R2 sekarang ditolak dengan database/audit/revisions unchanged. Tambahkan
  negative tests untuk unrelated assigned staff, missing operation/read grant,
  revoked membership, current assignment change, wrong actor/body/operation,
  machine principal dan cross-tenant lookup pada seluruh jalur terkait.
- Genuine two-consumer cold-install proofs per domain tetap lulus memakai artifact
  identik tanpa source imports. Tambahkan real PostgreSQL concurrency/rollback
  checks; jangan hanya menambah mocks yang meniru implementation.
- Fresh home: setup → doctor → compiled host → browser flow lulus. Module disabled
  dan missing dependencies ditolak dengan benar. Tiga domain memiliki actual
  host/browser evidence, bukan counting stdout dari cold consumers sebagai UI test.
- Capacity proof membuktikan aggregate budget dan overlap/slot invariants. Fixture
  R4 tidak boleh lagi menerima 80 jam di satu hari berkapasitas 40 jam.
- Jalankan seluruh delapan root gates melalui `pnpm verify`, fresh regression
  proofs semua domain existing, CRM/Act/runtime/browser/assembly/kits, upgrade dan
  security setelah source/artifacts final. Serialkan repack/build yang berbagi
  output; parallel hanya jobs terisolasi yang aman. Catat exit status tiap gate.
- Upgrade non-empty 1.10.0 dari archive ke candidate baru dan backup/restore
  dibuktikan di database sintetis terlebih dahulu. Sebelum owner migration ambil
  baseline dan backup privat. Bandingkan rows dengan timestamp precision asli,
  memberships/settings/keys dan existing audit/receipts. Modul baru tidak
  auto-enroll owner atau menciptakan business records. Jangan menganggap laporan
  purchasing sebelumnya sebagai bukti preservation upgrade baru.
- README, PRD, architecture, build status, capability matrix, starter-kit ledger,
  module docs, release/handoff dan sanitized audit manifest menyebut profile dan
  limitations yang sama. Financial pricing/client signature/project linkage hanya
  boleh diklaim bila benar-benar terintegrasi dan diuji; internal proposal acceptance
  tidak otomatis membuktikan signed customer agreement. Semua 40 full kits tetap
  memiliki domain/adapter gaps sampai acceptance masing-masing terpenuhi.
- Archive baru dibuat setelah gates lulus dan hash library/runtime cocok; original
  archive tetap utuh. Commit lokal tanpa private data dan tanpa push. Laporkan
  actual checks/report paths, remaining limits dan next executable task. Jangan
  menyebut candidate sebagai accepted hanya karena sudah di-commit.

**Mulai dari R1/R2, lalu R4, kemudian release dan host integration. Tuntaskan
acceptance ini sebelum mengimplementasikan domain berikutnya.** Codex sedang
orchestrate/review; perbaikan product code tetap tugas Gemini.

## 1. Tujuan yang harus lu pegang

**ARCHZXY adalah company. Archava adalah produk unggulan dan library reusable.**
Website ARCHZXY nanti dibuat di proyek terpisah yang mengonsumsi library ini.
Sekarang fokusnya membangun modul bisnis dan kit yang bisa dirakit tanpa fork,
dengan Archava sebagai interface AI di atas sistem bisnis yang deterministik.

Hotel perlu operasi hotel; restoran perlu operasi restoran. Shared CRM, stok,
scheduling atau project tracking membantu kedua bisnis, tetapi tidak menggantikan
aturan domainnya. Satu modul yang sudah jalan bukan bukti seluruh kit selesai.

Arah kerja:

- Reuse kode dan kontrak existing; konfigurasi/composition sebelum membuat fork.
- Modular monolith TypeScript dan PostgreSQL, mengikuti struktur repo sekarang.
- Modul bisnis memiliki data dan aturan transaksi; LLM hanya meminta aksi.
- Backend memutuskan izin, validasi, idempotency, approval dan hasil eksekusi.
- Public/customer dan owner/employee memiliki identitas, data dan tools berbeda.
- Maksimalkan hasil untuk tim kecil. Jangan mengganti framework atau menambah
  microservices, Kubernetes, workflow engine atau dependency tanpa kebutuhan nyata.

Owner sudah menyerahkan urutan implementasi dan mengizinkan pekerjaan lokal yang
reversible. Lanjutkan pekerjaan mandiri yang jelas; tanyakan hanya input yang benar-benar
menentukan pekerjaan berikutnya. Paid calls, publikasi, deployment online dan
pengiriman data privat ke provider tetap membutuhkan otorisasi/input tersendiri.

## 2. Snapshot awal handoff — historis, 11 Oktober 2026

Status di bagian ini merekam handoff awal sebelum `faabcc1`/`345d039`/`f1c0a89`.
Untuk tugas dan acceptance terbaru, ikuti §0.

Workspace: `/home/haikaru/Archverse/Lab/Archava_Platform`.
Branch saat handoff: `main`. Commit implementasi terakhir yang diterima:
`a34a836` — support cases dengan assignment dan resolution cycles.

**Release diterima: 1.9.0. Working tree sedang mengerjakan 1.10.0.**
Angka di release config atau build output tidak membuktikan release diterima.
Working tree memang dirty karena purchasing sedang dikerjakan. Pertahankan dan
review perubahan tracked maupun file untracked tersebut. Jangan `git reset --hard`,
`git clean`, menghapus folder atau menimpa pekerjaan untuk mulai dari nol.

| Bagian                          | Bukti yang sudah ada                                                                           | Batas                                                                                 |
| ------------------------------- | ---------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Core / identity / reusable host | Cold install, tenant/resource authorization, native action consent/recovery, backup/restore    | Local/free; production dan live provider punya gate sendiri                           |
| CRM                             | Contacts/activity/follow-up, pipeline, worklist, overview, dua business consumer               | Bukan seluruh CRM/ERP universal                                                       |
| Projects                        | Task/milestone/internal delivery                                                               | Belum full proposal, capacity, billing dan change-order operations                    |
| Scheduling                      | Exclusive resource holds/confirm/move/cancel                                                   | Belum pooled capacity, recurring calendars atau seluruh rental                        |
| Accommodation                   | Room-night quotes, guest stays, check-in/out, housekeeping                                     | Belum full PMS, OTA, public booking dan financial flows                               |
| Commerce                        | Backend catalog price, frozen orders, stock reservations, fulfillment dan returns              | Satu stock pool; belum warehouse/valuation/invoice/payment                            |
| Food service                    | Physical tables, sealed recipes, whole-ticket kitchen/service, ingredient consumption/waste    | Belum full POS, per-item prep, hardware, split bills/public ordering                  |
| Support                         | Current assignment, internal notes, investigation/wait/resolve/close/reopen, immutable history | Internal cases; belum customer replies, contractual SLA atau specialized case systems |
| Purchasing                      | Implementasi baru di working tree; belum diterima                                              | Harus dituntaskan lebih dulu                                                          |

Release 1.9.0 mempunyai 38 artifacts, 18 external pins, 11 schema ledgers;
host 0.12.0, config 0.11.0, kits 0.6.0, assembly v7. Bukti diterima:
1.523 unit tests/111 files, delapan root gates, support 19 domain checks dan
9 host/browser checks, 47 security controls. Root/runtime advisory scans saat
checkpoint nol temuan; realtime template terpisah masih 13 moderate findings.

Seluruh **40 full operating kits masih belum diterima**. Ada narrow scopes yang
terbukti usable. Status ini harus tetap jujur sampai masing-masing kit punya bukti.

### Fakta working tree 1.10.0

Sudah dibuat/diubah, tetapi belum ditutup sebagai release:

- `packages/purchasing/`: contracts, service, API, private read tools, dashboard,
  boundary tests; versi calon 0.1.0.
- `packages/purchasing-pg/`: repository dan
  `001_suppliers_orders_receipts.sql`; versi calon 0.1.0.
- Commerce calon 0.3.0: additive `003_purchase_receipts.sql`, private
  `receivePurchaseStock`, inbound provenance, refusal pada generic command recovery.
- Optional purchasing composition di host/operator/browser, packaging, kit planner,
  assembly v8, release config dan lockfile.
- Calon host 0.13.0, config 0.12.0, kits 0.7.0; 40 artifacts, 18 pins, 12 ledgers.
- `tools/library/verify-purchasing.mjs` dan `verify-purchasing-host.mjs` sudah ada.
- `docs/PURCHASING_MODULE.md` adalah kontrak implementasi yang **masih pending**.

Verifikasi yang benar-benar sudah dijalankan pada working tree:

- `pnpm verify`: seluruh 8 gates lulus; **1.531 tests/112 files**.
- Fresh CRM 38, Act 18, persistent runtime 44, Chromium minimal/full 3/70 lulus.
- Lint proof scripts dan typechecks yang disebut di log terakhir lulus.
- Purchasing domain debug melewati 12 checkpoints awal, kemudian gagal pada
  helper pengujian machine refusal: service melempar error sinkron sebelum masuk
  `assert.rejects`. Pemanggilan sudah diubah ke callback, tetapi **belum direrun
  sampai akhir**. Kegagalan berikutnya masih mungkin ada; jangan menganggap fix itu
  sebagai acceptance.
- Purchasing host/browser proof **belum dijalankan**.
- `dist/purchasing-proof/results.json` dan purchasing host report belum ada.
- Security report masih profile 1.9.0/47 controls. Script calon menambah enam
  purchasing controls, tetapi report 1.10.0 belum dibuat/diverifikasi.
- Original owner database tetap **1.9.0**, belum dimigrasikan ke purchasing.

Build output `dist/library` dan `dist/crm-runtime` sekarang calon 1.10.0.
Arsip rollback `dist/releases/1.9.0/{library,crm-runtime}` tersedia lokal.
Semua proses pemeriksaan yang terakhir diamati sudah selesai; tetap cek proses/lock
sebelum menjalankan build atau host baru.

## 3. Baca sumber yang relevan sebelum coding

Ikuti `AGENTS.md` yang berlaku untuk folder yang disentuh. Root `agent.md` adalah
kontrak produk/pricing; bukan pemberi runtime authentication/grants. Template punya
`Archava Template/AGENTS.md` sendiri dan harus tetap standalone.

Urutan baca:

1. `PRD.md`, terutama §§40–50 dan §46.6.
2. `docs/ARCHITECTURE.md`, `docs/BUILD_STATUS.md`.
3. `docs/LOCAL_LIBRARY_RELEASE.md`, `docs/LOCAL_LIBRARY_HANDOFF.md`.
4. `docs/STARTER_KIT_IMPLEMENTATION.md` — ledger programme aktif.
5. `docs/STARTER_KIT_COVERAGE_RESEARCH_2026-10-10.md`,
   `docs/BUSINESS_CAPABILITY_MATRIX.md`.
6. `packages/kits/src/catalogue.ts`, `contracts.ts`, `index.ts`;
   `packages/config/src/assembly.ts`, release contracts dan release verifier.
7. `docs/PURCHASING_MODULE_PLAN.md`, `docs/PURCHASING_MODULE.md`, seluruh diff
   purchasing/commerce/host beserta scripts pengujiannya.
8. Kontrak modul existing yang akan dikomposisikan, berikut implementation/tests.

Research audit tanggal 10 Oktober adalah history, bukan status implementasi hari ini.
Status terbaru mengikuti accepted release evidence dan kode yang benar-benar diuji.
File handoff ini menyampaikan arahan owner terbaru; Codex tidak sedang meneruskan
implementasi paralel dengan lu.

## 4. Tugas purchasing pada handoff awal — referensi historis

Review kode existing, perbaiki masalah yang ditemukan, lalu buktikan alur:

`Supplier → backend offer → reviewed draft → confirmed PO → partial inspection → stock`.

Invariants yang wajib dipertahankan:

1. Supplier unit cost berbeda dari selling price dan pricebook ARCHZXY.
   Order menerima offer ID/quantity; tidak menerima harga/total/paid dari LLM/caller.
2. Draft menyimpan quote backend. Offer berubah membuat confirm ditolak;
   operator refresh/review draft. Confirmed quote/units/lines tidak berubah lagi.
3. Hanya accepted goods menambah stock commerce; rejected goods hanya findings.
   Kebijakan awal: cumulative **accepted + rejected ≤ ordered quantity**.
   `received` berarti semua unit memiliki findings, bukan semua accepted/paid.
   Pengganti rejected goods memakai order baru; tulis batas ini dengan jelas.
4. Partial receipt, cancellation sebelum findings, short-close setelah partial,
   over-receipt refusal dan stock-capacity overflow tidak menghapus physical truth.
5. Purchasing tidak punya stock balance kedua dan tidak menulis tabel commerce
   langsung. Private commerce transaction SPI memiliki stock/claim/audit writes.
6. Purchase findings, shared movement, kedua audit dan kedua UUID claims commit
   bersama atau semuanya rollback. Same UUID/body/actor menambah stock sekali.
7. Current purchasing dan commerce grants, human/service intersection, resource
   scope dan forced tenant RLS berlaku pada read/write/recovery.
   Machine mutation/recovery tetap ditolak.
8. Generic commerce command API tidak mengambil alih purchase receipt authority.
9. Public browser output tidak membawa PG/server code atau secrets. UI memakai
   text rendering, membersihkan data saat logout/tenant switch, dan menahan UUID
   unknown/lost-response/denied retry sebelum efek baru.
10. Migration 001/002 commerce dan migration modul lama tidak ditulis ulang.
    Additive 003 mempertahankan non-empty prior data, quotes, grants, audit/receipts.

Periksa khusus source binding di deferred SQL checks, NULL/malformed JSON,
receipt-line/header linkage, sorted product locks, concurrent receipts dari order
berbeda, actor/body collisions, grant revocation dan atomic rollback multi-SKU.
Uji perilakunya di PostgreSQL sungguhan; passing mocks tidak menutup gate ini.

Urutan praktis:

- Jalankan ulang purchasing domain proof dan perbaiki sampai seluruh checks lulus.
- Jalankan actual compiled host/Chromium proof. Perbaiki selector/state/recovery
  atau backend apabila gagal. Jangan mengubah test agar menerima perilaku salah.
- Perkuat bukti concurrency/source-binding yang belum tercakup.
- Rerun gates yang terpengaruh setelah code berubah; final release memerlukan
  fresh proofs dengan profile/hashes yang sama dan regresi semua modul lama.
- Tutup owner migration, security matrix, release docs/manifest/archive/commit
  hanya setelah bukti yang diwajibkan lulus.

### Data owner yang wajib dijaga

`.local/`, `.env`, credentials, identity/fingerprint keys, provider/native control,
settings/grants, database dan backup adalah data privat penting. Jangan dibersihkan,
dibaca ke context chat, dicetak, direset atau dimasukkan Git/release artifacts.
Program backend boleh membacanya secara lokal tanpa mengeluarkan nilainya.
Gemini credential sudah ada di environment; jangan meminta key di chat atau
mengaktifkan live calls untuk menyelesaikan workflow lokal.

Backup pra-purchasing sudah tersedia:
`.local/crm/backups/pre-purchasing-1.10.0-20261011.dump`.
Baseline SQL JSON hashes privat:
`.local/crm/purchasing-upgrade-baseline.json`.
Snapshot meliputi **74 pre-existing tables**, delapan CRM business tables,
original columns dengan microsecond timestamps, settings dan key hashes.

Helper sementara `/tmp/archava-purchasing-owner.mts` mempunyai mode `capture` dan
`verify`. Inspeksi dahulu; jika hilang, buat verifier ekuivalen. **Jangan capture
ulang setelah migrasi**, karena itu bisa menutupi perubahan data yang salah.
Verifier mengizinkan tepat satu additive commerce ledger entry 003 sambil menjaga
semua original ledger rows. Sembilan purchasing business tables harus kosong dan
account tidak auto-enrolled. Tidak perlu mengaktifkan module/grants di owner
asli untuk proof; gunakan isolated hosts untuk itu.

Setelah seluruh isolated proofs lulus: host asli harus berhenti, setup/migrate
local owner, doctor, verifikasi baseline, buat backup sesudah upgrade. Pertahankan
backup lama dan arsip release. Jika kondisi lokal berbeda dari snapshot, selesaikan
penyebabnya; jangan menonaktifkan preservation assertions.

## 5. Tugas berikutnya: lanjutkan programme, bukan hanya purchasing

Setelah purchasing diterima, lanjutkan KIT-07 general families, KIT-08 specialist
adapters dan KIT-09 handoff. Daftar requirement setiap keluarga sudah ada di typed
catalogue/research. Jangan mengganti seluruhnya dengan generic CRUD atau menamai
folder sebagai kit yang selesai.

Prioritas berikut yang masuk akal, dengan dependency/domain review:

| Urutan | Pekerjaan                                              | Hasil nyata yang dibutuhkan                                                                                                        |
| ------ | ------------------------------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------- |
| 1      | Organizations, proposal/scope/change request, capacity | Account affiliation; versioned agreed scope/amendments; queue/start/finish estimates dari kapasitas nyata                          |
| 2      | Asset register, rental/maintenance/field-service       | Serialized identity, condition/custody, handover/return, work/parts evidence; scheduling saja tidak cukup                          |
| 3      | Subscriptions/usage/entitlements                       | Recurring lifecycle, explicit quota/access/revocation; payment truth dari authoritative backend                                    |
| 4      | Attendance, course/event capacity/check-in             | Enrollment/participant/resource access dan duplicate/check-in rules; academic domain terpisah                                      |
| 5      | Fulfillment/dispatch/production                        | Custody/pick-pack/delivery findings; bahan/output/scrap/BOM untuk produksi sederhana                                               |
| 6      | Invoice/payment boundaries                             | Immutable invoice intent/snapshot/corrections dan provider adapter; payment success hanya dari verified authority                  |
| 7      | Remaining general-domain extensions                    | Tutup requirement hotel, restaurant, repair, laundry, property, recruitment, nonprofit, content dan keluarga lain secara eksplisit |
| 8      | Archava assembly/use experience                        | Deterministic dependency/version/readiness planning dan private domain tools; NL routing tidak mendapat write authority gratis     |
| 9      | Specialist adapters + operability                      | Selected backend contracts, tenant/data projection, audit/recovery, domain/privacy review, genuine integration evidence            |
| 10     | Whole-programme handoff                                | Honest supported catalog, reproducible releases, upgrade/recovery/operator guide dan semua residual blockers                       |

Ini urutan awal yang boleh lu sesuaikan dari dependency dan bukti, bukan alasan
membangun sepuluh sistem besar sekaligus. Reuse existing capability ketika tepat.
Dua business consumer independen harus membuktikan generalisasi sebelum dinyatakan
reusable. Tidak wajib satu package/service baru untuk setiap label capability.

Katalog mencakup 24 general families dan 16 specialist families berikut. Gunakan
requirement/invariants yang rinci di source; daftar ini hanya untuk mencegah scope
hilang saat context berganti.

| ID  | Keluarga                                     |
| --- | -------------------------------------------- |
| K01 | Hotel, villa, homestay                       |
| K02 | Restaurant, cafe, food service               |
| K03 | Retail dan e-commerce                        |
| K04 | Distributor dan wholesale B2B                |
| K05 | Agency, studio, jasa proyek                  |
| K06 | Layanan profesional berbasis kasus           |
| K07 | Salon, spa, appointment personal care        |
| K08 | Gym, club, membership dengan kelas           |
| K09 | SaaS dan produk digital berlangganan         |
| K10 | Sewa kendaraan, alat, dan barang             |
| K11 | Agency real estate dan penjualan properti    |
| K12 | Pengelolaan sewa properti dan coworking      |
| K13 | Kursus, bootcamp, training cohort            |
| K14 | Sekolah dan administrasi pendidikan formal   |
| K15 | Event, ticketing dan venue                   |
| K16 | Tour, travel, dan aktivitas wisata           |
| K17 | Teknisi, instalasi, layanan lapangan         |
| K18 | Bengkel dan servis barang                    |
| K19 | Laundry dan dry cleaning                     |
| K20 | Gudang, fulfillment dan logistik barang      |
| K21 | Recruitment dan staffing agency              |
| K22 | NGO, donasi dan asosiasi                     |
| K23 | Produksi sederhana dan made-to-order         |
| K24 | Media, content studio dan penerbit           |
| K25 | Klinik dan dental outpatient                 |
| K26 | Rumah sakit dan inpatient                    |
| K27 | Lab diagnostik dan testing                   |
| K28 | Apotek dan regulated health retail           |
| K29 | Pertanian tanaman dan kehutanan              |
| K30 | Peternakan dan perikanan budidaya            |
| K31 | Konstruksi, kontraktor dan EPC               |
| K32 | Manufaktur industri dan regulated production |
| K33 | Pertambangan dan industri ekstraktif         |
| K34 | Utilitas listrik, gas dan air                |
| K35 | Pengelolaan limbah dan jasa lingkungan       |
| K36 | Banking, lending dan layanan keuangan        |
| K37 | Asuransi dan claims                          |
| K38 | Marketplace dan transaksi multipihak         |
| K39 | Layanan publik dan administrasi pemerintahan |
| K40 | Residential care dan layanan sosial          |

Specialist integration yang belum dipilih backend-nya adalah **blocker nyata**.
Jangan memilih vendor berbayar/regulated scope secara diam-diam atau mengarang
live evidence. Siapkan boundary, contracts, synthetic contract tests dan pending
requirements; minta input backend/domain yang konkret sambil melanjutkan pekerjaan
independen. Fake gateways sah untuk failure tests, bukan bukti pembayaran, regulator,
live integration atau production support.

## 6. Quality, security dan scalability

- Preserve passing behavior, exact consent/action boundaries dan unresolved receipt
  recovery. No blanket grants, owner impersonation atau secrets di prompts.
- Tenant isolation dan resource authorization harus server-side dan diperiksa ulang
  saat efek/recovery; permission cache tidak boleh memperpanjang revoked access.
- Global catalog/stock authority tidak otomatis didapat dari assigned order access.
- Bounded inputs/pages/quantities, canonical UUID intent, optimistic revisions,
  atomic audit, rollback, database clocks dan explicit timezone/unit rules.
- Module ownership/API/SPI jelas; cross-domain writes memakai module-owned ports.
- Curated dependencies, existing pins/lockfiles, review install scripts. Prefer
  existing tested primitives. Jangan menjalankan audit fix/upgrade massal tanpa
  analisis compatibility dan regression evidence.
- Document actual compatibility matrix, additive migrations dan rollback limits.
  Jangan mengklaim ratusan kombinasi jika baru fixed profile yang diuji.
- Ukur scalability dengan declared workload/concurrency, bukan jumlah packages atau
  persen tebakan. Gunakan existing benchmark, lalu perluas untuk bottleneck nyata.
- Observability metadata-only untuk tool/webhook/queue/error/cost; backups dengan
  tested restore. RPO/RTO/SLA produksi butuh evidence dan operator policy tersendiri.
- Realtime template tetap standalone. Review 13 moderate advisory findings dan
  SDK/live gaps dengan instructions template; jangan menutupnya memakai test root.

## 7. Cara menjalankan checks tanpa merusak artifact evidence

Toolchain lokal: **Node 22.22.3, pnpm 10.15.1**.
Contoh prefix yang tersedia: `fnm exec --using 22.22.3 pnpm ...`.

**Jangan menjalankan packers bersamaan.** `pnpm verify` (reuse gate), `library:pack`,
`crm:verify`, `act:verify`, `crm:build` dan beberapa wrapper domain menyiapkan ulang
shared `dist/library`/runtime. Cold-installed consumers harus membaca artifact
set yang stabil. Jangan start host/browser proofs saat runtime proof sedang
menjalankan fase tamper installed artifacts.

Contoh pengujian mandiri purchasing, jalankan berurutan:

```bash
fnm exec --using 22.22.3 pnpm purchasing:verify
fnm exec --using 22.22.3 pnpm purchasing:verify-host
```

Untuk final regression, urutkan pack/build preparation:

```bash
fnm exec --using 22.22.3 pnpm verify
fnm exec --using 22.22.3 pnpm crm:verify
fnm exec --using 22.22.3 pnpm act:verify
fnm exec --using 22.22.3 pnpm crm:verify-browser
```

Setelah shared artifacts stabil, jalankan scripts domain/host langsung lewat Node
untuk menghindari wrapper yang repack. Scripts tersedia di `tools/library/` untuk
projects, scheduling, accommodation, commerce, foodservice, support, purchasing,
kits dan assembly. Upgrade proof: `node apps/crm/scripts/upgrade.mjs`.
Inspect setiap exit status dan report; parallel hanya independent isolated jobs.

Security full runner `pnpm security:verify` menjalankan ulang prep/proofs; jangan
bersamaan dengan cold consumers. Jika seluruh fresh reports sudah tersedia, gunakan
fresh unit JSON dan `node tools/security/verify.mjs` tanpa `--run`:

```bash
fnm exec --using 22.22.3 pnpm exec vitest run --reporter=json --outputFile=dist/security-proof/unit.json
fnm exec --using 22.22.3 node tools/security/verify.mjs
```

Sesuaikan commands dari `package.json` jika source berubah. Root verification saja
bukan pengganti PostgreSQL, Chromium, upgrade atau template/provider checks.
Logs debug terakhir ada di `/tmp/purchasing-*.log`; gunakan seperlunya dan jangan
memperlakukan log yang gagal/stale sebagai accepted evidence.

## 8. Definition of done setiap milestone

Sebelum menyebut milestone diterima:

1. Workflow runnable dengan domain invariants, current authorization dan recovery.
2. Dua independent installed consumers memakai artifact yang sama tanpa source
   workspace imports/forks; browser/server surfaces terpisah.
3. Meaningful domain/database/race/rollback/restart/restore checks dan actual owner
   host/browser flow lulus. Scope, workload dan limitations ditulis.
4. Current compatibility/config planner menolak missing/disabled/incompatible
   dependencies. Enabling feature tidak memberi grants.
5. Semua existing behavior yang terpengaruh punya fresh regression evidence;
   installed profiles/hashes/ledgers konsisten.
6. Original owner preservation verified, before/after backups tersedia ketika
   migration dilakukan; existing flags, grants dan keys tetap sesuai baseline.
7. Update README/PRD, architecture, build status, capability matrix, programme ledger,
   release/handoff, module guide dan sanitized audit manifest dengan hasil aktual.
8. Archive exact matching library/runtime sebagai release baru tanpa overwrite
   arsip lama. Commit lokal hanya milestone yang sudah terbukti, tanpa private data.
   Jangan push/publish kecuali owner memberi instruksi itu.
9. Sisakan outstanding requirements di ledger; jangan mengubah narrow scope menjadi
   full-kit acceptance atau menyebut seluruh Archava selesai dari jumlah tests.

## 9. Reporting untuk owner / Codex

Buat progress singkat pada batas milestone; jangan minta konfirmasi berulang untuk
reversible work yang sudah diotorisasi. Saat review dibutuhkan, laporkan:

- commit dan accepted profile;
- workflow/consumer yang sekarang usable;
- checks yang benar-benar dijalankan, hasil dan file evidence;
- perubahan authorization/schema dan preservation/recovery;
- remaining requirements, blockers konkret dan next executable task.

Kalau belum selesai, bilang belum selesai. Jangan memberi persentase seluruh
produk, universal business coverage atau security tanpa denominator/evidence.
Codex boleh dipanggil owner untuk audit/review, tetapi jangan menunggu Codex untuk
setiap keputusan implementasi rutin.

**Mulai sekarang:** ikuti tugas review terbaru di §0. Perbaiki authorization,
recovery dan capacity, tuntaskan release/host integration beserta acceptance baru,
lalu lanjut programme dari ledger. Preserve pekerjaan existing dan data owner.
