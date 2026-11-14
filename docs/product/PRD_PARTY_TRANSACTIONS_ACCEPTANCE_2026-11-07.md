# Penerimaan M02c2b — party-linked transactions

Tanggal: 7 November 2026 (Asia/Jakarta). Scope: kelanjutan
[PRD execution](PRD_EXECUTION_WORK.md) setelah [registry master](PRD_PARTIES_ACCEPTANCE_2026-11-06.md).
Status: **verified local untuk scope di bawah**. Ini tidak menerima seluruh ERP,
finance parity, public production atau customer pilot.

## Perilaku yang diterima

- Sales dapat membuat order dari party ID dan versi yang direview. Peran customer
  harus aktif dan nama diambil dari master, bukan klaim request.
- Purchasing dapat membuat supplier dari party yang sama dan merefresh profilnya
  secara eksplisit. Supplier baru, offer baru, PO baru dan reactivation memerlukan
  peran supplier aktif; profil supplier harus mengikuti versi master terkini.
- Order, invoice, supplier dan PO menyimpan party ID/versi. Invoice menyalin
  identitas historis order. Perubahan nama/peran master tidak menulis ulang dokumen.
- Order lama tetap bisa direserve, fulfilled dan diinvoicing; PO lama tetap bisa
  dikonfirmasi dan diterima setelah peran dimatikan atau akses baca party dicabut,
  selama izin transaksi yang berlaku mengizinkan penyelesaian tersebut.
- Replay memeriksa izin/scope terkini, lalu mempertahankan receipt awal walaupun
  peran atau versi sudah berubah. Status recovery tidak membuka data master baru.
- Published-party read cukup untuk konsumsi transaksi; party-write maupun raw
  source CRM read tidak diwajibkan. Grant tetap eksplisit dan session/database
  yang menentukan authority. Legacy entry paths tetap memiliki kontraknya sendiri.
- Native UI menyediakan scoped party picker, pagination, pencarian tanpa submit
  saat Enter, refresh pilihan yang eksplisit, supplier profile review dan ID/versi
  historis di detail transaksi. Tool baru mengikuti approval mission yang berlaku.

Kontrak rinci: [party transactions](../modules/PARTY_TRANSACTIONS.md).

## Bukti yang dijalankan

| Pemeriksaan | Hasil |
| --- | --- |
| Full workspace gate | 1209 Rust tests dalam 98 suites; 0 failed/ignored |
| Source final | Structure/docs, 25 helper tests, fmt, Clippy -D warnings, WASM dan library gate lulus lagi setelah source dibekukan |
| Tambahan terarah | 10 router/DB scenarios dan 2 grammar tests |
| Real Chromium | 107 checks; 0 browser runtime errors; owned fixture cleanup exit 0 |
| Existing product probes | 75 met / 0 gaps dalam kontrak bounded yang sama |
| Agent runtime | 30 owned-fixture checks; source cocok |
| Product-audit helper | 3 tests; inventory/source/browser binding lulus |
| Native package | 68 artifacts terverifikasi; build/provenance/package source cocok |

Grammar, router dan DB tests mencakup stale/missing/forged/foreign sources,
current permissions/scope, service-seat refusal, stable replay/status, supplier
refresh/reactivation, SQL immutable-version guards, role/order race,
supplier-refresh/PO race dan approved mission. Real prior-profile upgrade memakai
Organizations 003, Commerce 007, Purchasing 003 dan Billing 006; transaksi legacy
dibuat lewat HTTP, kemudian upgrade diulang tanpa kehilangan data atau auto-link.

Browser memublikasikan 102 party pada database disposable, menguji pagination
dan pencarian, menolak sale dengan versi lama, mereview ulang lalu membuat sale,
supplier dan PO native dari satu party. Source/name master berubah; PO baru ditolak
sampai supplier refresh direview dan diterapkan. Kedua role kemudian dimatikan;
sale lama tetap diselesaikan menjadi invoice dan PO lama menerima barang.
Independent SQL membuktikan sale/invoice versi 7, supplier/PO baru versi 8,
master versi 9 nonaktif, PO lama versi 7 received, satu receipt, dan stock 6.
Tiga screenshot sales review, supplier review dan invoice history diperiksa.

Owned media transport juga lulus; ini bukan pengujian ulang model/provider
produksi. Existing 75 product probes mengukur kontrak terbatas, bukan persentase
kelengkapan semua ERP di market.

## Provenance dan migrasi

- Source SHA-256: `08bddf9c436158d7878f56ca5a13ec7529d170d8c80497478c46c109ec4653bf` (459 inputs, 22 crates).
- Candidate lokal: `dist/native-candidate-20261107-prd-party-transactions-01`.
- Inventory: 18 business domains, 154 canonical operations, 152 native forms,
  2 provider-only operations; 56 SQL files, 49 mounted migrations, 24 namespaces.
- Commerce 008, Purchasing 004 dan Billing 007 menambah referensi versi historis.
  Installer menerapkan Identity → CRM → Organizations → Commerce → dependent ledgers.
  Semua migration ID/checksum lama, SQL bytes, runtime grants, acceptance reports
  terdahulu dan source di luar scope dipertahankan.
- Full gate: `target/prd-party-transactions-20261106/verify-01.log`; final library/WASM gate:
  `target/prd-party-transactions-20261106/verify-02.log`; browser: `target/e2e/prd-party-transactions-20261107-01/result.json`.
- Build/package/audit dan final hash checks:
  `target/prd-party-transactions-20261106/final-verification.json`. Log iterasi yang gagal tetap disimpan.

## Batas yang tetap terbuka

Registration tidak otomatis menghubungkan data legacy berdasarkan nama.
Legal identity verification, credit account dan company/book mapping ada pada
M02c2c dan R0. Native GL vs adapter/external accounting, jurisdiction/currency
serta proses wajib customer masih memerlukan keputusan profil rilis.
AP/AR, bank, posting, close, comparative market benchmark, human-work reduction,
production load/HA, live provider acceptance, public deployment dan customer pilot
belum diterima oleh increment ini. Website/deployment tetap mengikuti jadwal owner.
