# Penerimaan lokal M02c1 — verified organization/contact mapping

6 November 2026. Increment DAT-01/DAT-02: hubungan organisasi dengan kontak CRM
yang benar, reviewed creation dan explicit legacy verification. Ini tidak menerima
universal party, customer credit account, legal-entity verification atau finance.

## Source dan hasil

- Baseline M02b: `ddc6176a54fa4358466b08c23da549f23aa0c9f145c355ee8a171ddabedf3e3c`.
- Source final: `0569914ecf6079e787e4e88edbc405e9a468a7eecf3d05d3ae3c0f45db3bde38`, 448 input.
- Full gate `verify.mjs --offline --wasm`: **1186 Rust tests/98 suite,
  0 failed, 0 ignored**, structure/docs, 25 helper tests, fmt, Clippy
  `-D warnings` dan kedua WASM boundary checks lulus. Tiga product-audit helper
  tests terpisah juga lulus.
- Sepuluh existing Organizations integration tests dan sembilan identity tests
  baru lulus secara focused. Full gate mencakup 46 workspace flows dan delapan
  Ops upgrade/restore tests.
- Fresh release server/fixture preview/WASM dibangun dari source sama.
  Chromium lulus **94 checks**, 75 existing product probes/0 gaps,
  30 owned agent-runtime checks, 0 JS/WASM page errors dan cleanup 0.
- Native candidate `dist/native-candidate-20261106-prd-m02c1-01`: **64 artifact**, Ops verifier lulus.
  Inventaris: 18 business domains, 147 canonical operations, 145 native forms,
  2 provider-only operations; 52 SQL files, 45 mounted migrations/24 namespaces.

## Perilaku yang dibuktikan

1. Native form memilih organisasi dan kontak yang berbeda secara eksplisit dari
   halaman kedua masing-masing. Legal name/tax reference dan email membedakan
   nama yang sama. Enter mencari tanpa menyimpan relationship yang sudah siap.
2. Source organization yang berubah dan source contact yang berubah masing-masing
   ditolak HTTP 409, tanpa affiliation parsial. Refresh pilihan yang eksplisit
   mengizinkan review ulang dan submit terhadap dua revisi yang cocok.
3. New relationship menyimpan contact UUID/revision/name yang benar. Rename CRM
   selanjutnya tidak mengubah snapshot. Original request replay tidak membuat
   affiliation atau aggregate revision tambahan.
4. Existing `create_affiliation` tidak lagi menerima UUID kontak yang tidak ada
   atau tidak terlihat. Current CRM read wajib, termasuk pada replay. Read-only
   CRM grant cukup; CRM write tidak diperlukan. Activation juga mengecek source
   access; deactivation/removal tetap tersedia untuk melepaskan histori.
5. Real pre-002 fixture memiliki satu referensi lama yang benar dan satu opaque
   missing UUID. Owner migration continuation dan reapplication lulus; kedua
   record dipertahankan dengan snapshot null, bukan dianggap terverifikasi.
6. Legacy verification menggunakan stored UUID yang immutable, current source
   access, reason dan kedua reviewed revisions. Stale review ditolak; review
   ulang menyimpan snapshot sekali, lalu record keluar dari pending list.
   Missing source tetap unverified dan tidak dapat diam-diam diarahkan ke kontak lain.
7. SQL oracle browser memeriksa organization revision 4, CRM revision 4,
   affiliation baru pada CRM revision 2 dan legacy verified pada revision 4,
   satu opaque relationship masih unverified, total tiga relationships dan tiga
   business audits. Nama snapshot baru tetap berbeda dari nama CRM terakhir.
8. Negative router tests mencakup tenant, assigned scope di kedua ledger,
   permission revoke, missing/forged source, source fields yang disisipkan pada
   payload optional dan revision invalid. Constraint/trigger menolak snapshot
   palsu, penghapusan verification dan source repointing. Unicode 160 karakter
   dipertahankan sebagai contact snapshot.
9. Dua reviewed writes dengan aggregate revision sama menerima satu penulis;
   race rename menghasilkan pasangan snapshot yang konsisten atau refusal.
   Approved missions menjalankan kedua operasi; run sebelum approval ditolak,
   dan run replay mengembalikan hasil komit yang sama.

## Bukti kerja dan temuan

- [Kontrak dan batas migration](../modules/ORGANIZATION_IDENTITY.md).
- [Router/database tests](../../crates/host/tests/workspace_flows/organization_identity.rs).
- [Browser dan SQL oracle](../../tools/e2e/organization-identity.mjs).
- Work log: `target/prd-m02c-20261106/`; full gate/build/package di `verify-01.log`,
  `build-01.log`, `package-01.log`.
- Browser final: `target/e2e/prd-m02c1-20261106-02/result.json`. Screenshot pilihan source,
  legacy review dan pending record telah diperiksa secara visual.
- Integrity record: `target/prd-m02c-20261106/final-verification.json`.

Focused run 01 mencatat 4 pass/5 fail karena consumer baru membaca snake_case
sementara projection API yang sudah ada memakai camelCase. Consumer UI dan tests
disesuaikan dengan kontrak tersebut; run 02 sembilan tes lulus. Hasil gagal tetap
tersimpan. Parser juga diperketat: jumlah optional fields tidak boleh menjadi
celah untuk menyisipkan key di luar grammar. Gate penuh menerima perubahan final.

Browser run 01 berhenti saat opening-data fixture belum memiliki membership
Organizations untuk Alice; FK owner menolak seed sebelum affiliation scenario.
Fixture sintetis dilengkapi, run 02 lulus dengan runtime source/build yang sama.
Hasil gagal dan cleanup 0 dipertahankan.

SQL lama, unrelated source/working-tree state dan laporan M01/M02a/M02b tidak
diubah. Integrity record memeriksa source/build/package binding, artifact hash,
helper browser, migration checksum dan dokumentasi. Tidak ada commit/deployment.

## Batas dan kompatibilitas

Foreign key tetap `NOT VALID` untuk menjaga sejarah pre-002 yang sebelumnya
menerima arbitrary UUID. Ini menegakkan new references, **bukan** bukti bahwa semua
legacy relationships sudah bersih. Record lama tidak di-backfill berdasarkan
tebakan; inactive unverified record harus diverifikasi sebelum active/primary.
Opaque source memerlukan removal dan reviewed replacement. Old active state
bersama marker unverified tetap dipertahankan sampai direkonsiliasi.

Existing create/activation clients memerlukan current CRM read setelah upgrade.
Receipt-status recovery menggunakan policy Organizations yang sudah ada tanpa
memerlukan akses baru ke profil CRM. Module disable, revoke dan stale state tetap
menghasilkan refusal sesuai authority path.

M02c2 universal party/roles, company-book, legal entity/account dan R0 accounting
profile tetap terbuka. AP/AR/GL, bank/close, customer pilot, comparative ERP parity,
new load/SLA dan provider production tidak diterima oleh increment ini. Agent dan
media checks memakai owned fixtures; tidak ada bukti Gemini live/public deployment
baru atau klaim seluruh produk production-ready.
