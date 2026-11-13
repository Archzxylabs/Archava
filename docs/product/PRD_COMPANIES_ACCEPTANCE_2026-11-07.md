# Penerimaan M02c2c1 — managed-company identity

Tanggal: 7 November 2026 (Asia/Jakarta). Scope: kelanjutan
[eksekusi PRD](PRD_EXECUTION_WORK.md) setelah
[party-linked transactions](PRD_PARTY_TRANSACTIONS_ACCEPTANCE_2026-11-07.md).
Status: **verified local untuk declared company master**. Accounting R0 tetap
belum ditentukan; increment ini tidak menerima seluruh ERP atau public production.

## Perilaku yang diterima

- Workspace dapat secara eksplisit mendaftarkan perusahaan yang dikelolanya dari
  Organizations record yang direview. Company ID, code, owner dan source identity
  tetap; official name dan tax/reference string merupakan snapshot source/revisi.
- Company registration adalah deklarasi operator. Nama resmi/reference yang
  tersimpan tidak memverifikasi status legal, registrasi pajak atau kepemilikan.
- Dua perusahaan dengan official name sama tetap punya ID/code dan source yang
  berbeda. Unique source/code berlaku per tenant; tidak ada auto-match berdasarkan nama.
- Source changes tidak mengubah company secara otomatis. Refresh harus mengikuti
  revisi source baru, tidak mengubah activity, dan menyimpan versi sebelumnya.
- Deactivation tetap tersedia ketika source tidak lagi terlihat/aktif. Reactivation
  memerlukan source aktif yang masih cocok dengan snapshot; source berubah perlu
  direview/refresh terlebih dahulu. Arsip source menolak reactivation baru.
- Actor/tenant/grants/scope berasal dari session/database. Write/status memerlukan
  companies.read + companies.write; history juga memerlukan audit.read. Source
  creation/refresh/reactivation memerlukan current organizations.read/source scope.
- Original replay memeriksa current visibility dan mempertahankan receipt awal
  meskipun source berubah/diarsip. Status recovery tidak membaca source baru.
  Reuse request ID lintas company/party/legacy organization ditolak.
- Native registration memakai paginated source picker, exact selected UUID/version,
  official-name/reference review, company code dan reason. Enter tidak membuat
  company. Profile panel menyediakan reviewed refresh dan activity controls.
  Ketiga tool baru tetap memerlukan approval mission yang berlaku.

Kontrak: [managed companies](../modules/MANAGED_COMPANIES.md).

## Validasi yang dijalankan

| Pemeriksaan | Hasil |
| --- | --- |
| Full workspace gate | 1220 Rust tests, 98 suites, 0 failed/ignored |
| Source final | Full gate dimulai sesudah source dibekukan: structure/docs, 25 helper tests, fmt, Clippy -D warnings, WASM dan seluruh Rust tests |
| Tambahan terarah | 9 router/DB scenarios dan 2 grammar tests |
| Real Chromium | 113 checks; 0 JS runtime errors; fixture cleanup exit 0 |
| Existing bounded product probes | 75 met / 0 gaps |
| Agent runtime | 30 owned-fixture checks, source cocok |
| Audit helper | 3 tests; inventory/source/browser binding lulus |
| Native candidate | 69 artifacts terverifikasi |

Tests mencakup explicit identity/independent party, stale/absent/archived source,
missing official name, forged field/authority/accounting claims, current grants,
assigned scope, service-kind refusal, private history, forced tenant RLS, duplicate
registration race, source-update race, competing activity/refresh CAS, immutable
identity/history, missing audit/receipt, exact status dan approved mission.

Genuine prior-profile upgrade memakai Organizations 003 dengan party-linked
transaction ledgers yang sudah berjalan. Existing organization, party, audit dan
command snapshots dipertahankan. Upgrade diulang dan tidak membuat company atau
memberi company permissions kepada membership lama. Native bootstrap baru memakai
explicit installed-catalogue permissions; migration tidak mengubah existing grants.

Browser memilih source melalui pagination/search, menolak stale registration,
melakukan refresh review lalu mendaftarkan dua company dengan official name sama.
Company pertama dinonaktifkan, stale profile refresh ditolak, current review
menghasilkan versi berikutnya, lalu direactivate. Source diarsip: deactivation
berhasil, reactivation ditolak dan original receipt tetap recoverable.
Independent SQL membuktikan 2 company, company pertama revisi 5 dengan captured
source revisi 4 sementara source revisi 5 archived, immutable history [1,2,3,4,5],
activity [true,false,false,true,false], total 6 audit events dan 102 parties yang
tidak otomatis berubah menjadi company. Tidak ada country/currency/verified/
accounting-mode/book fields yang ditetapkan diam-diam. Tiga screenshot registration,
refresh review dan company profile diperiksa.

Owned media transport dan existing product probes ikut lulus; ini bukan pengujian
ulang provider produksi atau persentase kelengkapan ERP di market.

## Provenance

- Source SHA-256: `240014b9f20cb96b42e2b3d08c07c0f686cda5ee9de4dfc2de6566f003da9403` (465 inputs, 22 crates).
- Candidate: `dist/native-candidate-20261107-prd-companies-01`.
- Inventory: 18 business domains, 157 canonical operations, 155 native forms,
  2 provider-only operations; 57 SQL files, 50 mounted migrations/24 namespaces.
- Organizations 004, checksum/embedded profile, fixture/runtime ACL dan write fences
  terdaftar bersama. Applied SQL lama, prior acceptance reports dan source/working
  tree di luar scope dipertahankan.
- Full gate (termasuk WASM): `target/prd-companies-20261107/verify-03.log`; browser: `target/e2e/prd-companies-20261107-01/result.json`.
- Build/package/audit/hash checks: `target/prd-companies-20261107/final-verification.json`.
  Log iterasi yang gagal tetap disimpan. Gate pertama menemukan harness upgrade
transaksi lama mencoba menyalakan current host pada schema Organizations 003.
Harness kini menegaskan startup refusal tersebut dan menyiapkan legacy documents
melalui production adapters dalam tenant transactions sebelum migrasi, tanpa
melonggarkan startup/schema checks.

## Batas berikutnya

Existing documents belum diberi company mapping otomatis. Transaction visibility
belum diisolasi per company oleh increment ini. Company/account/book mapping dan
consolidation merupakan scope M02c2c2 berikutnya dengan kontrak serta pengujian
tersendiri. Jurisdiction/currency, legal verification, credit account dan pilihan
sumber accounting R0 masih terbuka. AP/AR/GL, bank, posting, close, tax compliance,
comparative benchmark, human-work reduction, production load/HA, live provider,
public deployment dan customer pilot belum diterima. Website/deployment tetap
mengikuti jadwal owner.
