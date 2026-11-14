# Eksekusi PRD Archava

Mulai 6 November 2026 atas instruksi owner. Acuan kebutuhan:
[PRD produk aktif](PRD.md). Tracker ini mencatat pekerjaan baru secara bertahap;
kelulusan kandidat sebelumnya tidak menerima requirement baru secara otomatis.
Website/deployment tetap mengikuti jadwal owner dan tidak dijalankan pada tahap ini.

## Pekerjaan berlanjut — 8 November 2026

Scope pekerjaan tetap seluruh PRD. [Register delivery](PRD_DELIVERY_REGISTER.v1.json)
mencatat 35 requirement aktif; [inventaris turunan](PRD_INHERITED_TRACEABILITY.v1.json)
mencatat 89 kewajiban bernama dan 96 bagian sumber. Semua bagian kini mempunyai
review semantik, termasuk [master produk bab 7–38](PRD_MASTER_PRODUCT_REVIEW_2026-11-08.md),
[kontrak Full Rust](PRD_RUST_CONTRACT_REVIEW_2026-11-08.md),
[master bab 39–51](PRD_MASTER_CONTRACT_REVIEW_2026-11-08.md) dan
[fondasi bab 1–6](PRD_MASTER_FOUNDATION_REVIEW_2026-11-08.md).
Inventaris ini tidak menyatakan requirement telah diterima.

Owner menegaskan target lintas model bisnis pada 8 November: belum ada pilihan
customer pertama; Archava harus dapat disusun untuk berbagai model dan hybrid.
Gue lanjut pengembangan/simulasi per cakupan katalog, dengan profil sebagai unit
konfigurasi dan acceptance. Memilih satu jenis usaha bukan prasyarat pekerjaan
lokal. Tiap klaim dukungan tetap memerlukan alur bisnis lengkap dan bukti sendiri.

Fondasi [Accounting native](../modules/ACCOUNTING.md) sudah dipasang: buku resmi
per company yang dikonfigurasi eksplisit, akun, periode, jurnal seimbang,
persetujuan posting, reversal dan laporan dengan nominal eksak. Sebelas skenario
database keuangan lulus, termasuk pengelolaan buku setelah company operasional
dinonaktifkan. Ava juga mendapatkan lookup berizin di luar halaman pertama.

Gate penuh terbaru lulus 1.261 tes Rust dalam 99 suite serta 29 tes helper,
Clippy dan pemeriksaan WASM, pada source
`fe4ec91c74f6d42886b662a55f028b88d5229f976f5617bae001f0a0481ba1f5`.
Build server, preview dan WASM untuk source itu telah selesai. Kandidat baru
lulus 143 pemeriksaan browser, 77 product probes dan 30 pemeriksaan Ava,
serta restore terenkripsi, upgrade dan rollback dari distribusi sebelumnya.
Paket native memuat 75 artefak terverifikasi. [Bukti fondasi GL](PRD_NATIVE_GL_ACCEPTANCE_2026-11-08.md)
membatasi penerimaan ini; bukti kandidat lama tidak menerima kode baru.

Rekomendasi kepada owner adalah accounting native terlebih dahulu, dengan
konektor eksternal opsional yang diuji sesuai kebutuhan dan satu sumber resmi
per company. Ini belum memilih buku, saldo awal, negara atau profil pelanggan
nyata. Integrasi transaksi ke pembukuan, AP/AR, bank, evaluasi Ava, konfigurasi
privasi dan penerimaan pengguna masih terbuka. Pekerjaan berlanjut setelah
fondasi GL; seluruh produk belum dinyatakan selesai.

Invoice source posting kemudian lulus pada source
`f88831b033be413d9adb80c1f6b33bff1a17e63b812eaf0e4b2ab4f3301bb9dd`:
1.269 tes Rust/99 suite, 29 tes helper, 154 pemeriksaan browser, 77 product
probes, 30 pemeriksaan Ava dan paket 77 artefak. [Bukti invoice](PRD_INVOICE_POSTING_ACCEPTANCE_2026-11-08.md)
mencakup issue, credit, verified collection/refund, void, sumber jurnal, close,
restart, upgrade dari GL sebelumnya dan restore sebelas tabel keuangan.
P95 Commerce 237,78 ms melewati target lokal 50 ms; ini tetap counterevidence.

Pekerjaan aktif setelah kandidat itu: query Commerce, portabilitas paket Domain
dan form review kebijakan invoice. [Temuan cold package](PRD_COLD_PACKAGE_FINDING_2026-11-08.md)
menyimpan kegagalan arsip lama. Snapshot katalog publik di dalam crate dan
pemeriksaan byte equality sudah dipasang; probe arsip baru berhasil dikompilasi
secara terisolasi. Probe kecil itu belum membuktikan dua consumer bisnis penuh.
Kode terbaru belum diterima oleh bukti kandidat invoice sebelumnya.

[Kandidat portabilitas/Commerce](PRD_PORTABLE_COMMERCE_ACCEPTANCE_2026-11-08.md)
lulus full gate 1.270 tes Rust/99 suite dan 35 helper, 159 pemeriksaan browser,
77 product probes dan 30 Ava checks; Commerce p95 31,11/33,21/17,31 ms pada
pengukuran lokal terpisah, di bawah target 50 ms tanpa error. Kandidat fcd94e…
menyimpan counterevidence label yang belum dirender dan grant pihak bisnis yang
hilang pada bootstrap owner. Kedua perbaikan sekarang sedang diuji pada source
baru, bersama restore tanpa perubahan transaksi. Bukti lama tidak menerima kode
baru. AP/bank/COGS dan requirement lain tetap dilanjutkan setelah validasi ini.

Perbaikan owner kemudian lulus full gate 1.272 Rust/99 suite dan 35 helper pada
source c231af…. Browser membuktikan label `Discovery <&>` dirender sebagai teks;
probe paket native membuktikan bootstrap owner baru dapat membuat party tanpa
grant tambahan. [Temuan berikutnya](PRD_OWNER_PRESENTATION_FINDING_2026-11-08.md)
menyimpan kegagalan Ava `agent_context_too_large` setelah izin party yang benar
membuka konteks Organizations lebih besar. Batas 32 KB dipertahankan; seleksi
baris berdasarkan byte dan coverage yang jujur sekarang sedang divalidasi.

[Recheck purchase/contact](PRD_PURCHASE_CONTACT_RECHECK_2026-11-08.md) kemudian
membuktikan 1.288 tes Rust, 35 helper, 167 pemeriksaan browser dan tujuh alur
purchase posting pada source 7b9ea4…. Respons Contacts yang terlambat setelah
pindah halaman tidak lagi membaca owner yang sudah dibuang. Review visual
menemukan panel jurnal desktop terlalu sempit; perbaikan layout dan teks
coverage laporan sedang diuji pada source berikutnya. Ini belum menerima
AP aging/payment, bank/COGS, full close atau seluruh produk.

[Dua consumer persisten](PRD_PERSISTENT_CONSUMERS_2026-11-08.md) sekarang juga
lulus: client Rust dibangun terisolasi dari arsip publik yang sama, kemudian
memakai konfigurasi Owner, CRM tersimpan dan extension Projects/Commerce pada
dua server/database native berbeda. Sebelas skenario membuktikan replay,
revocation, stage/revisi palsu, restart dan isolasi. Ini menutup sebagian celah
bukti komposisi persisten, belum menerima seluruh kit K04/K05 atau CFG-03.

Bridge biaya stok kini sedang divalidasi lewat router/PostgreSQL: biaya
fulfillment memakai origin pembelian asli, restock membalik COGS asal meskipun
policy berganti, dan discard tidak menciptakan persediaan. Migration baru
Accounting 005/Commerce 010 sudah terdaftar; tidak ada buku, saldo atau jurnal
yang dibuat otomatis. [Kontrak biaya stok](../modules/STOCK_COST_POSTING.md)
menjelaskan batas profil dan pekerjaan lanjutan. Preview purchase kini juga
menempatkan nominal dan debit/credit sebelum referensi teknis. Kode baru ini
belum mendapat penerimaan native/browser; bukti e638 sebelumnya tetap terbatas
pada source saat pengujiannya. Validasi source
`03021cfedccbb1afe302609d2f41bc3b8a46e5fa9524e40b1b81396a0de22390`
kemudian lulus 28 tes integrasi Accounting dan gate penuh 1.302 Rust/99 suite,
36 helper, Clippy serta kedua boundary WASM. Build native untuk source ini
sedang disiapkan sebelum acceptance browser/upgrade. Tes lokal ini tidak
menerima pengiriman data Accounting ke provider Ava; pengaturan disclosure
masih perlu diselesaikan.

[Kandidat stock source](PRD_STOCK_SOURCE_FINDING_2026-11-08.md) kemudian lulus
175 pemeriksaan browser, 77 scoped product probes dan 30 Ava checks, termasuk
upgrade riwayat pembelian/cost dari binary lama, restore 19 tabel keuangan dan
restart sumber stok. Review screenshot tetap menemukan tabel jurnal HP terlalu
padat dan nominal desktop terpisah dari mata uang. Perbaikan penyajian mobile
serta pemeriksaan overflow internal sekarang sedang dikerjakan; kelulusan fungsi
tidak menutup temuan visual ini atau pekerjaan whole-PRD lainnya.

[Penerimaan stock posting](PRD_STOCK_POSTING_ACCEPTANCE_2026-11-08.md) kemudian
membuktikan perbaikan mobile pada source a5389f…: 611 tes library dan 36 helper,
Clippy/WASM, rebuild native 80 artefak, 181 pemeriksaan browser, 77 scoped
product probes dan 30 Ava checks. Penerimaan penuh backend 1.302 tes tetap
terikat source 03021…; perubahan berikutnya hanya markup/CSS review. Simulasi
satu perusahaan/satu buku kini menghubungkan delapan sumber pembelian, stok,
invoice, verified collection/refund dan koreksi sampai laporan serta restart.
Dua cold consumers lulus 10 tes; dua consumer persisten baru lulus 11 skenario
pada database berbeda. Bank, supplier payment, full close, pembukaan saldo,
privacy dan financial Ava belum diterima. Pekerjaan seluruh PRD berlanjut.

Fondasi review izin data AI berikutnya sudah memiliki konfigurasi berversi,
pilihan kategori/modul/company, pencabutan eksplisit dan persetujuan HMAC
yang terikat tenant, pemilik, seluruh rencana, penyedia/model/account dan
masa berlaku. Tes Owner HTTP/PostgreSQL lulus 21 skenario (empat baru untuk
disclosure), engine Owner lulus 21 tes dan transport planner lulus tiga tes.
Gate library source `2c4c0f…` lulus 613 tes Rust/22 suite, 36 helper,
format, Clippy seluruh target dan kedua boundary WASM.
Restore tetap membuat revisi baru dan tidak dapat memulihkan izin ke penyedia
yang berubah tanpa review. Kunci API maupun path endpoint privat tidak masuk
profil publik. Bukti ini **belum** menerima fitur privasi end-to-end: pembatasan
seluruh payload/history/lookup ke provider, editor izin native dan financial
Ava masih dalam pengerjaan. Belum ada rebuild/penerimaan native untuk kode
baru ini. Kelulusan tes bukan persentase penyelesaian seluruh produk.

## Urutan dan status

Implementasi berikutnya menambahkan [batas data AI](../modules/AI_DATA_SHARING.md)
dan editor izin native, pemeriksaan izin sebelum/setelah setiap respons model,
serta review Ava untuk jurnal dengan nominal dan nama akun asli. Suite Ava
HTTP/PostgreSQL terbaru lulus 29 skenario; Accounting 28 dan Owner 22.
Temuan alias organisasi milik company yang belum dipilih sudah direproduksi,
disimpan dan diperbaiki dengan scope company pada alias sumber. Temuan effect
invoice `credit` yang keliru juga ditangkap tes lama dan diperbaiki menjadi
kontrak asli `correction`; migration yang sudah diterapkan tidak diubah.
Source `d67999…` kemudian lulus gate penuh 1.331 Rust/99 suite dan paket 81
artefak, tetapi native browser menemukan dispatcher Accounting belum terhubung
untuk eksekusi Ava. [Temuan dan reproduksi](PRD_AI_FINANCIAL_FINDING_2026-11-09.md)
menyimpan kegagalan itu. Tiga pemeriksaan privasi native lulus; eksekusi jurnal
ditahan sebelum handler keuangan. Perbaikan dispatcher lulus 30 Ava dan 28
Accounting HTTP/PostgreSQL, termasuk reviewer terpisah serta replay. Source
baru `2afdf2…`, dengan perbaikan tampilan fakta pada ponsel, kemudian lulus
gate penuh 1.332 Rust/99 suite, 36 helper, 191 pemeriksaan browser, 77 product
probes, 30 Ava checks dan paket 81 artefak. [Penerimaan scoped native](PRD_AI_NATIVE_BOUNDARY_ACCEPTANCE_2026-11-09.md)
mengikat kedua upgrade/rollback, restore 19 tabel keuangan, lima pengukuran load,
dua consumer persisten dan tampilan desktop/ponsel. Uji Gemini asli terpisah
gagal pada turn pertama; hasil itu tetap tersimpan sebagai kegagalan.

[Investigasi Gemini berikutnya](PRD_GEMINI_SCHEMA_FINDING_2026-11-09.md)
menemukan penolakan combined schema dan binding hasil yang dikodekan sebagai
scalar. Diagnosis sekarang hanya membawa kategori/struktur yang aman. Format
shared-field typed dan satu koreksi binding dalam budget lama sudah dipasang;
probe developer Gemini asli menghasilkan rencana supplier/offer tanpa eksekusi.
Source `d4f032…` kemudian lulus gate penuh 1.336 Rust/99 suite, 36 helper,
191 pemeriksaan browser, 77 observed product probes, 30 Ava checks dan paket
81 artefak. Uji Gemini terpisah berhasil sampai supplier/offer yang disetujui,
hasil SQL, replay, laporan receipt dan preferensi, kemudian gagal pada follow-up
kedua dengan field yang tidak diizinkan. Run itu tetap gagal; field spesifiknya
tidak ditebak dari pesan generik.

[Reproduksi historical context](PRD_PRIOR_CONTEXT_FINDING_2026-11-09.md) juga
membuktikan teks tiket lama masih ikut ke provider setelah assignment berubah,
meski read terbaru sudah menolak akses. Input historis sekarang ditahan seluruhnya;
hanya metadata aksi dan penyelesaian terverifikasi yang dapat dibagikan. Kasus
negatif asli kini lulus. Suite Ava baru lulus 34 skenario, unit host 21 dan
provider 5. Koreksi kontrak memakai schema aksi terpilih dalam budget dua putaran
yang sama. Probe developer Gemini dua turn berhasil membuat satu offer 3500 lalu
mengubahnya menjadi 4200 setelah persetujuan, dengan SQL independen.

Source `60e65f…` kemudian lulus gate penuh 1.341 Rust/99 suite, 36 helper,
191 pemeriksaan native, 77 observed product probes, 30 Ava checks dan paket
81 artefak. Gemini berhasil sampai enam live checks termasuk follow-up 4200,
kemudian turn CRM ketiga gagal pada budget lookup. Probe CRM terpisah menerima
HTTP 429 dari Google. Kedua kegagalan disimpan; uji enam turn belum diterima.

[Pekerjaan lookup berikutnya](PRD_CRM_READ_FINDING_2026-11-09.md) mempertahankan
batas total tiga panggilan termasuk routing, sambil mendukung pencarian/detail
bertahap ketika routing lokal sudah cukup. Simulasi 110 kontak dan seluruh 35
uji Ava lulus. Kasus pengalihan tiket di tengah satu turn juga direproduksi;
sumber kini dibaca ulang sebelum setiap putaran berikutnya dan kasus aslinya
lulus. Regresi gabungan serta pekerjaan lokal PRD berlanjut sementara panggilan
Google ditahan setelah 429. Kandidat lama tidak menerima perubahan baru ini.

[Kontrol bank manual](../modules/MANUAL_BANK_RECONCILIATION.md) menambahkan registrasi akun kas, impor mutasi dengan total kontrol persis, matching jurnal posted dan koreksi tanpa menghapus riwayat. Accounting008 kemudian menambah basis opening yang dipilih, cakupan seluruh periode, saldo bank versus GL, reviewer independen opsional, hash bukti yang direview, dan guard tutup/buka ulang termasuk posting kas mundur. Ketiga uji periode serta seluruh **36 tes integrasi Accounting** lulus lokal. Native, browser, consumer, upgrade/restore untuk source baru dan penyelesaian AP/AR/settlement tetap terbuka; kandidat native lama tidak menerima perubahan ini.

| Bagian | Scope | Status dan bukti |
| --- | --- | --- |
| M01 — hubungan sales order dan invoice | Bagian DAT-01/DAT-02/FIN-03: draft invoice dari fulfillment yang terverifikasi, frozen amounts, source line links, current authority dan replay | **Verified local**: full gate 1.155 Rust tests, 75 browser checks, 75 product probes dan paket native 60 artifact; [scope dan bukti final](PRD_M01_ACCEPTANCE_2026-11-06.md) |
| M02a — identitas customer CRM | Referensi contact UUID/revisi pada sales order dan invoice, name snapshot, scoped picker dan pagination | **Verified local**: 1.164 Rust tests, 81 browser checks, 75 product probes dan paket native 62 artifact; [scope dan bukti final](PRD_M02A_ACCEPTANCE_2026-11-06.md) |
| M02b — identitas supplier CRM | Register supplier dari CRM, reviewed profile refresh, shared contact reference dan frozen PO/receipt history | **Verified local**: 1.175 Rust tests, 87 browser checks, 75 product probes dan paket native 63 artifact; [scope dan bukti final](PRD_M02B_ACCEPTANCE_2026-11-06.md) |
| M02c1 — verified organization/contact mapping | Reviewed source revisions, enforced CRM references dan explicit legacy verification | **Verified local**: 1.186 Rust tests, 94 browser checks, 75 product probes dan paket native 64 artifact; [scope dan bukti final](PRD_M02C1_ACCEPTANCE_2026-11-06.md) |
| M02c2a — master pihak bisnis | Stable typed source, independent customer/supplier roles, reviewed refresh dan version history | **Verified local**: 1.197 Rust tests, 101 browser checks, 75 product probes dan paket native 65 artifact; [scope dan bukti final](PRD_PARTIES_ACCEPTANCE_2026-11-06.md) |
| M02c2b — integrasi party ke transaksi | Reviewed party references, role enforcement dan historical versions pada sales/purchasing/billing | **Verified local**: 1.209 Rust tests, 107 browser checks, 75 product probes dan native package 68 artifact; [scope dan bukti final](PRD_PARTY_TRANSACTIONS_ACCEPTANCE_2026-11-07.md) |
| M02c2c1 — managed-company master | Explicit organization source, official-name/reference snapshot, stable company identity dan version history | **Verified local**: 1.220 Rust tests, 113 browser checks, 75 product probes dan 69 native artifacts; [scope dan bukti final](PRD_COMPANIES_ACCEPTANCE_2026-11-07.md) |
| M02c2c2 — company-owned trade | Produk/stok/PO/sales/invoice terikat company, current access, UI dan Ava | **Verified local**: 1.229 Rust tests, 123 browser checks, 75 product probes dan 72 native artifacts; [scope dan bukti final](PRD_COMPANY_TRADE_ACCEPTANCE_2026-11-07.md) |
| M03a — supplier goods document review | Explicit unposted vendor claims, exact PO/accepted-receipt checks, held differences, correction and allocation history | **Verified local**: 1.238 Rust tests, 132 browser checks, 75 product probes dan 73 native artifacts; [scope dan bukti final](PRD_SUPPLIER_DOCUMENTS_ACCEPTANCE_2026-11-07.md) |
| M02c2c3 — legal/account/book mapping | Legal verification, account dan company-book mapping | Terbuka; mengikuti keputusan R0 tanpa otomatis mengubah transaksi legacy |
| R0 — profil rilis dan accounting | Kebutuhan pelanggan, proses wajib, jurisdiction, pilihan native ledger/adapter dan integrasi prioritas | Terbuka sesuai bagian 5 PRD; M01 tidak memutuskan strategi accounting |
| R1 — proses keuangan penuh | AP/AR, bank, posting dan close sesuai ledger resmi yang dipilih | Belum diterima; [B01](ERP_PARITY_ACCEPTANCE_2026-11-06.md) masih protokol usulan |
| R2–R5 — Ava, konfigurasi, operasi dan pilot | Kedalaman tujuan, owner self-service, sektor, integrasi/operasi dan perbandingan langsung | Tetap mengikuti requirement dan dependency PRD; belum diterima oleh pekerjaan M01 |

## M01 — kontrak dan batas

`billing.create_invoice_from_order` membuat satu draft dari sales order native
yang sudah fulfilled dan belum mengalami retur. Customer, currency, quantity dan
harga berasal dari snapshot order. Form menampilkan rincian sebelum submit;
invoice dan setiap barisnya menyimpan sumber yang tidak dapat dipindahkan.
Harga katalog terkini tidak dipakai untuk mengganti harga transaksi.
Picker menyediakan pencarian customer pada maksimal 100 recent matching orders;
pilihan yang sudah ditagih dikosongkan agar tampilan dan input tetap cocok.

Izin billing dan sumber Commerce diperiksa kembali dari database. Satu transaksi
menyimpan invoice, lines, audit dan command receipt. Source lock serta unique
constraint mencegah dua konversi untuk order yang sama. Original request replay
mengembalikan invoice yang sama; key baru untuk source yang sudah ditagih ditolak.
Invoice manual lama tetap didukung dan tidak diberi provenance hasil tebakan.

Delapan tes integrasi memakai router nyata, role runtime non-owner dan PostgreSQL
disposable: frozen values/SQL/replay/status; stale/unfulfilled/missing/returned
source; grant/scope/revoke; race; later return plus reviewed correction; database
guards; approved mission tool; dan cross-tenant source refusal. Unit parser
menolak amount/customer/authority tambahan serta revision atau UUID tidak valid.

Retur setelah invoice memerlukan correction/refund yang direview terpisah;
otomatisasi credit note dan reissue invoice belum termasuk. M01 tidak menerima
master-party, GL/AR, cash/bank atau financial-close parity.

## Bukti kerja

Pekerjaan M02a menggunakan `target/prd-m02-20261106/`. Delapan tes customer baru
berjalan bersama seluruh suite `workspace_flows` (28 lulus), dan delapan
upgrade/restore tests lulus. Scope, schema dependency dan pemulihan stale selection
ada pada [kontrak customer identity](../modules/CUSTOMER_IDENTITY.md). Gate 03,
build 03 dan browser final telah lulus pada source yang sama; bukti lengkap ada di
[penerimaan M02a](PRD_M02A_ACCEPTANCE_2026-11-06.md) dan
`target/prd-m02-20261106/final-verification.json`.

- Baseline dan log baru: `target/prd-execution-20261106/`.
- Delapan focused integration tests: `order-invoice-tests-02.log`.
- Spesifikasi: [Billing](../modules/BILLING_MODULE.md).
- Implementasi: [invoice adapter](../../crates/adapters/src/erp/billing_invoice.rs),
  [migration 005](../../migrations/billing-pg/005_order_invoice_provenance.sql),
  [native form](../../crates/slice/src/operations/billing.rs).
- Browser scenario: [order-invoice gate](../../tools/e2e/order-invoice.mjs), memakai
  browser/database milik gate dan memeriksa SQL secara independen.

Hasil final dan fingerprint source berada pada
[penerimaan M01](PRD_M01_ACCEPTANCE_2026-11-06.md) dan
`target/prd-execution-20261106/final-verification.json`. Tidak ada klaim live-model
atau public production baru. M01 menutup scope increment ini, bukan seluruh
DAT-01/DAT-02/FIN-03 atau PRD.

## Temuan selama validasi

- Review screenshot run pertama menemukan pemilih source perlu dikosongkan
  setelah konversi; perbaikan ini dan pencarian customer masuk revisi UI berikutnya.
- Run browser luas pertama lulus tiga pemeriksaan order-invoice, kemudian berhenti
  pada pemilih kontak Subscriptions. Skenario tes sebelumnya menunggu target pada
  daftar awal; helper sekarang memakai authorized contact detail dan pencarian UI.
  Hasil gagal disimpan di `target/e2e/prd-m01-20261106-01/`.
- Pemeriksaan media pertama tidak diaktifkan, sehingga probe media dilaporkan gap;
  rerun menggunakan owned media fixture. Ini tetap bukan penerimaan provider produksi.

## M02b — supplier identity dan reviewed refresh

Supplier dapat didaftarkan dari kontak CRM yang dipilih, lalu profil master
di-refresh setelah revisi baru direview. PO dan receipt lama mempertahankan
snapshot; PO baru memakai master terbaru. Kontak yang sama dapat dipakai juga
sebagai customer tanpa menyatukan legal entity atau account. Current authority,
CAS/replay, tenant/scope dan dua race diuji. [Penerimaan lokal M02b](PRD_M02B_ACCEPTANCE_2026-11-06.md)
mencatat full gate/build/browser/package yang terikat ke source sama dan batas
M02c/finance/pilot/deployment yang tetap terbuka.

## M02c1 — verified organization/contact mapping

Reviewed affiliation creation memeriksa dua source revisions; legacy create juga
memerlukan kontak CRM yang benar dan terlihat. Legacy rows tetap unverified sampai
review yang eksplisit, dengan real prior-profile migration dan immutable source
identity. [Penerimaan M02c1](PRD_M02C1_ACCEPTANCE_2026-11-06.md) mencatat full gate,
UI/SQL, approved missions, source/package binding dan NOT VALID FK boundary.
Kelanjutan master-party dicatat pada M02c2a–c; R0/finance tetap terbuka.

## M02c2a — business-party registry

Satu reviewed CRM contact atau organization menjadi typed source bagi party ID
yang stabil. Customer/supplier roles independen, source refresh eksplisit dan
setiap versi memiliki immutable audited snapshot serta receipt. Current grants,
CAS, uniqueness, tenant/scope dan approved mission diuji.
[Penerimaan registry](PRD_PARTIES_ACCEPTANCE_2026-11-06.md) membatasi scope ini:
registry-only pada penerimaan sebelumnya. Integrasi transaksi dilanjutkan di
[contract M02c2b](../modules/PARTY_TRANSACTIONS.md); entity/account dan accounting R0 tetap terbuka.

## M02c2b — party-linked transactions

Sales, supplier registration, reviewed supplier refresh, PO/receipt dan source-order
invoice sekarang membawa party ID/versi historis. Current role/grant/scope berlaku
pada transaksi baru; penyelesaian commitment lama menjaga history.
[Penerimaan M02c2b](PRD_PARTY_TRANSACTIONS_ACCEPTANCE_2026-11-07.md) membuktikan
1.209 Rust tests, 107 browser checks, 75 product probes dan 68 native artifacts.
M02c2c (entity/account/company-book), R0 dan finance tetap terbuka; tidak ada
auto-backfill atau keputusan accounting diam-diam.

## M02c2c1 — identitas perusahaan yang dikelola

[Kontrak managed company](../modules/MANAGED_COMPANIES.md) mencatat deklarasi
perusahaan dari reviewed organization source dengan stable ID/code dan official-name
snapshot. Refresh dan perubahan activity eksplisit, scoped grants serta immutable
audit/receipt diuji. Pilihan accounting R0 tetap belum ditetapkan; increment ini
tidak memilih negara, currency, accounting mode atau mengaitkan transaksi legacy.

[Penerimaan M02c2c1](PRD_COMPANIES_ACCEPTANCE_2026-11-07.md) mencatat
1.220 Rust tests, 113 browser checks, 75 bounded product probes dan 69 native
artifacts pada satu source. Company trade mendapat kontrak/penerimaan terpisah pada M02c2c2; registered company master sendiri tidak menerima transaction isolation atau company/book mapping. R0/account/book tetap terbuka.

## M02c2c2 — transaksi milik perusahaan

[Company-owned trade](../modules/COMPANY_TRADE.md) menerima explicit company pada produk/stok/PO/sales/source invoice, historical completion, current human access, native selection dan Ava scope. [Penerimaan bertanggal](PRD_COMPANY_TRADE_ACCEPTANCE_2026-11-07.md) mengikat source, regresi, Chromium, SQL oracle dan native artifact yang sama. Company/account/book mapping dan keputusan R0 tetap terbuka; tidak ada legacy backfill atau accounting posting implisit.

## M03a — dokumen operasional tagihan supplier

[Supplier goods documents](../modules/SUPPLIER_DOCUMENTS.md) menyiapkan actual vendor claim, exact PO/accepted-receipt check, durable held differences, correction/void dan history tanpa posting accounting. [Penerimaan lokal](PRD_SUPPLIER_DOCUMENTS_ACCEPTANCE_2026-11-07.md) mengikat source, PostgreSQL, approved Ava, Chromium/SQL/restart dan paket native. R0 dan proses pembukuan penuh tetap terbuka; dokumen matched tidak berarti supplier sudah dibayar atau jurnal telah diposting.


## Ava byte budget — current source finding

[Current bounded evidence](PRD_AGENT_CONTEXT_FINDING_2026-11-08.md) binds source
`f6b74cfe…`, 1,276 Rust tests, 35 helper tests, 160 browser checks, 30 owned Ava
checks and 77 native artifacts. Organizations now fits the existing per-source
byte limit with honest partial coverage; exact reviewed records and current
guards remain protected. Owner rendering/recovery and fresh native provisioning
complete their observed checks. Browser correctness succeeded, while Commerce
p95 **55.98 ms** exceeded the unchanged 50 ms target. Five registered repetitions
are a diagnostic; this failed result stays retained. The unmaintained transitive
`paste` advisory also stays a review item. Whole PRD remains incomplete.

The full Rust chapter contract review is now [mapped explicitly](PRD_RUST_CONTRACT_REVIEW_2026-11-08.md),
with all original source hashes and named criteria preserved. Fifty-one historical
master chapters still require semantic review; mapping a chapter is not delivery.

Purchase-source SQL/rules are being tested separately as an **unmounted prototype**
in `target/prd-completion-20261107/purchase-source-draft/`. Its owned native operating
API/SQL proof currently exercises receipt/GRNI, goods claim/AP, landed fee accrual,
void, policy history and real concurrent confirmation/setup fences. The prototype itself is not the product. The separate [mounted purchase bridge](../modules/PURCHASE_POSTING.md) now has policy/source-review commands, runtime guards, native forms, scoped reads and source-aware reports under integration validation. Its 23-test combined Accounting suite passed, including actual API setup/confirmation races and independent posting. The full gate, new native/browser/upgrade/restore proof and complete payment/aging/bank/close remain required; prior native acceptance does not cover this newer source.

## 2026-11-12: merged gateway bank settlement and complete acceptance in progress

Accounting010 is merged after the locally accepted supplier-payment profile.
The frozen combined source is
`037f10ec3d50554eb28cca6e7bd7cfbf9787561f9df54a3da08ec8116a75fdbd`.
Its complete gate finished with actual exit zero: 1399 Rust tests/99 suites,
zero ignored, 36 helper tests, Clippy without warnings and both WASM checks.
[Gateway clearing](../modules/GATEWAY_BANK_CLEARING.md) records the source-bound
contract and current acceptance limits. The sequential native build/package and
browser/live/upgrade/recovery/consumer pipeline is still running.

The unchanged one-company/book B01 browser journey now exercises original-cost
purchase/stock/invoice sources, verified owned gateway collection/refund, native
supplier payment and clearing drafts, separate posting and bank matching. It
checks the pre-return bank/inventory/profit/physical quantities and final
450000 bank/700000 inventory/1150000 assets and equity/150000 profit/zero AR and
AP whole-IDR oracle. A separate book checks explicit collection/refund fees.
Those new browser journeys have not yet run; prepared assertions are not proof.

The real CRM label-planner failure remains open. Public action descriptions are
now preserved in planner contracts and explain free-text contact labels before
selecting reads. The unchanged complete real-Gemini journey is queued for the
new release with the existing model and budget; no live repair is claimed yet.

In an isolated public-source copy, exact aging buckets and effective-date due
terms have passed all 248 domain tests, Clippy and portable core/domain WASM.
A strict parsing test initially failed because an internally tagged unit variant
accepted an unexpected date; an empty struct variant preserves the wire shape
while rejecting it. That isolated foundation has no terms persistence, mounted
API, native report or source-to-GL controls and is not part of the frozen release.
All whole-product and selected-profile requirements remain subject to the
[delivery register](PRD_DELIVERY_REGISTER.v1.json).

## 2026-11-12: gateway acceptance completed locally; due-term work remains isolated

The later [clearing acceptance](PRD_GATEWAY_CLEARING_ACCEPTANCE_2026-11-12.md)
supersedes the pending pipeline above for source
`c7e4b8cff4aa4a3c440fec070cdaa23ba5d2cd58708434f5a79f043fe53113e1`.
Its actual full gate passes 1400 Rust tests in 99 suites, with zero ignored,
36 helper tests, Clippy and both WASM checks. The packaged source passes the
focused financial journey and 215 broad Chromium checks, production restart,
declared prior upgrades, encrypted recovery and independent cold/persistent
consumers. The real Gemini controller remains failed: eight checks passed,
including the unchanged English contact-label update, before upstream429 on
the fifth planning turn. The sixth turn was not attempted. No remote retry or
model/key substitution follows that quota failure.

The isolated `finance-terms-staging-01` copy now appends Accounting011/012,
registers 19 explicit permissions and 192 operator actions, and records immutable
original-source due conditions with current document/terms versions and exact
receipt/audit claims. The first SQL run exposed a missing audit-kind allowlist;
append012 fixes it without changing the already exercised011 bytes. All49
Accounting PostgreSQL tests then passed, including supplier write races and
explicit payables-read authority. Private original-invoice/supplier source
pickers and immutable history were added next; four focused HTTP/PostgreSQL
tests now pass after correcting the projection's timestamp column to
`recorded_at`. Earlier failed outputs remain retained.

The staged native editor requires an explicit due condition, removes obsolete
dates when switching to unknown, clears a proposal when its source changes, and
pins the current document and terms revisions. All44 Slice library tests pass,
including real catalogue-to-wire roundtrips and preservation of the accepted
optional gateway-fee fix. The Accounting navigation tab is present only in the
isolated copy. Browser acceptance, complete as-of source/GL reconciliation,
selective merge and a fresh combined release remain outstanding; these staged
tests do not extend the accepted shipping profile or complete FIN04/FIN05.


## 2026-11-12: original-document terms and as-of controls selectively merged

[Due terms and dated subledgers](../modules/DOCUMENT_DUE_TERMS.md) now have
original-source native pickers/reviews, exact receipt acknowledgement, explicit
known/unknown dates, private immutable history, current read grants and whole-book
AR/AP reporting through a chosen cutoff. Source-derived control balances retain
historical account mappings and expose generic GL adjustments as differences.
The complete isolated55-test Accounting suite and45-test Slice suite pass,
along with all-target scoped Clippy and both WASM checks. A101-document actual
HTTP/PostgreSQL test checks pagination and exact totals above JavaScript's safe
integer range. Missing claims, tenant isolation and disabled startup guards also
fail closed. The selectively merged source is
`dcf6d93393066aac010aa47f63b55da1b62c5310199540ebe0696e1e85df405e`;
22-crate root structure verification passes. The prior accepted fee repair is
retained. New browser helpers require an actual Accounting-tab click and prepare
positive invoice/supplier date review, source-change clearing, original replay,
native reports, restart and29-table recovery; those prepared browser assertions
have not yet run. A fresh complete root gate/build/package is next. Whole-product
requirements and FIN04/FIN05 remain incomplete.


## 2026-11-12: due terms and dated subledgers accepted locally

[The dated source-bound acceptance](PRD_DUE_TERMS_ACCEPTANCE_2026-11-12.md)
records the corrected sourcee1a36367,1425 Rust tests/99 suites,36 helpers,
Clippy/WASM,88 native artifacts and focused7/5 plus broad221 Chromium checks.
Accounting is actually discovered through its Operations tab. Original source
versions, explicit/unknown dates, source-change clearing, exact whole-book
reports and old receipt recovery are exercised. Production restart, ten/eight
migration prior upgrades,29-table encrypted recovery and12 cold/11 persistent
consumer checks pass. The four overdue-label serialization mismatches were
caught by a compiled probe and fixed with an eight-bucket contract test and
actual native overdue rendering. The predecessor full gate passes1424 tests,
but its source-change rejection is retained and not reused as final acceptance.
No real Gemini request follows the prior429; its full live suite stays incomplete.
Complete financial-profile close and other delivery-register requirements remain;
local acceptance of this increment does not complete the whole PRD.
