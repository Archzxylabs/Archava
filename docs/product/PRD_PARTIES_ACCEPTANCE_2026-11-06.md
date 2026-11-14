# Penerimaan lokal M02c2a — business-party master registry

6 November 2026. Increment DAT-01/DAT-02: source-backed business identity,
independent customer/supplier roles, reviewed profile refresh dan immutable
version history. **Sales/purchasing/billing belum menggunakan party ID atau
menegakkan role flags baru ini.** Integrasi transaksi tetap M02c2b.

## Source dan hasil

- Baseline: `0569914ecf6079e787e4e88edbc405e9a468a7eecf3d05d3ae3c0f45db3bde38`.
- Source final: `437257f63ecad550906825f32cdaf4a7c3264fe351e0edba83ebc659c6a659cc`, 454 input.
- Full gate `verify.mjs --offline --wasm`: **1197 Rust tests/98 suite,
  0 failed, 0 ignored**, structure/docs, 25 helper tests, fmt, Clippy dengan
  `-D warnings` dan kedua WASM boundary checks lulus. Tiga product-audit helper
  tests juga lulus terpisah.
- Sembilan party integration tests lulus secara focused. Full gate mencakup 55
  workspace flows dan delapan Ops upgrade/restore tests; prior affiliation-profile
  fixture sekarang melanjutkan sampai Organizations 003.
- Fresh release server/fixture preview/WASM dari source sama. Chromium lulus
  **101 checks**, 75 existing product probes/0 gaps,
  30 owned agent-runtime checks, 0 JS/WASM page errors dan cleanup 0.
- Native candidate `dist/native-candidate-20261106-prd-parties-01`: **65 artifact**, Ops verifier lulus.
  Inventaris: 18 business domains, 151 canonical operations, 149 native forms,
  2 provider-only operations; 53 SQL files, 46 mounted migrations/24 namespaces.

## Perilaku yang dibuktikan

1. Registrasi memakai satu kontak CRM atau organisasi yang dipilih dan direview.
   Picker memakai authorized search/pagination, exact UUID/revision dan explicit
   selected-source refresh. Enter mencari tanpa men-submit registrasi.
2. Nama source yang berubah sebelum submit menghasilkan 409 tanpa master parsial.
   Refresh pilihan dan review baru menghasilkan satu party dengan peran customer
   dan supplier. Caller tidak dapat menyisipkan nama, owner atau authority.
3. Profile refresh yang stale ditolak tanpa mengubah master. Source revision baru
   direview secara eksplisit; ID dan old name/version history dipertahankan.
   Original create/refresh replay tidak menggandakan party atau audit.
4. Customer dan supplier adalah dua flag independen. Keduanya dapat dinonaktifkan
   lalu diaktifkan kembali pada party ID yang sama, dengan reason dan revision CAS.
   Ini tidak menonaktifkan transaksi legacy yang belum terintegrasi dengan registry.
5. Kontak dan organisasi dengan nama sama tetap memiliki typed source dan party
   UUID berbeda. Satu typed source tidak dapat didaftarkan sebagai party kedua.
   Tidak ada automatic matching, merge atau histori transaksi yang di-backfill.
6. Org-source refresh, original replay dan receipt status mengembalikan party yang
   benar. Reusing request ID untuk operasi Organizations lama/party baru konflik
   karena keduanya memakai command-key namespace yang sama.
7. SQL oracle memeriksa dua parties: contact party revision 5/source 4, organization
   party revision 2/source 3, current roles aktif dan tujuh immutable version events.
   Riwayat nama contact party berisi nama awal lalu empat versi nama master baru.
8. New explicit party grants dan assigned scope diuji terpisah dari raw-source
   grants. Create/refresh membutuhkan source access terkini; business-role change
   serta captured-master reads memakai party grant sendiri. Audit-read atau
   organization-read saja tidak mengekspos party snapshots/history.
9. Database guards menolak false source/name, source repointing, delete, history
   edit dan perubahan tanpa audit/receipt lengkap. Dua concurrent creations untuk
   source sama serta dua role CAS menerima satu penulis. Source archive menjaga
   master/history dan replay, sementara new refresh ditolak.
10. Approved mission benar-benar mendaftarkan party melalui same authority path;
    sebelum approval tidak ada party, replay run mengembalikan hasil sama.
    Unicode 160 karakter dipertahankan pada source/name snapshot.

## Bukti dan temuan

- [Kontrak master bisnis](../modules/BUSINESS_PARTIES.md).
- [Router/database tests](../../crates/host/tests/workspace_flows/parties.rs).
- [Native form dan profile](../../crates/slice/src/operations/parties.rs).
- [Browser dan SQL oracle](../../tools/e2e/parties.mjs).
- Work log: `target/prd-parties-20261106/`; final gate/build/package: `verify-03.log`,
  `build-03.log`, `package-01.log`.
- Browser final: `target/e2e/prd-parties-20261106-01/result.json`; source-review,
  role-lifecycle dan final master screenshots diperiksa secara visual.
- Integrity record: `target/prd-parties-20261106/final-verification.json`.

Gate struktur pertama menolak host file baru yang belum terdaftar di pemeriksa
extractor. File didaftarkan. Gate 02 kemudian menemukan missing write guards pada
fixture upgrade lama saat tabel party baru ditambahkan. Continuation helper kini
menginstal guard seperti installer Ops; focused prior-profile test dan gate 03
lulus. Build awal dihentikan dan build 03
memakai source final. Iterasi compiler/fixture sebelumnya juga dipertahankan di
folder kerja; kelulusan hanya memakai log final yang terikat source sama.

New source inventory memeriksa empat PartyCommand operations selain existing
OrganizationKind, sehingga angka catalogue bukan dihitung dari satu enum saja.
Applied migration bytes, unrelated serving inputs dan laporan penerimaan M01,
M02a/M02b/M02c1 dipertahankan. Tidak ada commit/deployment. Task-owned fixtures
berakhir setelah verifikasi.

## Batas penerimaan

Registry ini bukan seluruh universal-party integration. Legacy contact/supplier
links tetap seperti sebelumnya dan tidak memperoleh party provenance berdasarkan
tebakan. M02c2b masih perlu menghubungkan order, supplier, PO dan invoice ke party
serta menegakkan current business roles pada transaksi baru tanpa menulis ulang
history. Peran yang dinonaktifkan di registry belum memblokir legacy transactions.

Grant aplikasi baru tidak diberikan otomatis ke existing users. Current party
read/write diperlukan; published business-master snapshots sengaja terpisah dari
akses raw CRM/organization. Source refresh tetap memerlukan source visibility.

Company-book, legal-entity verification, credit/account, R0 ledger decision,
AP/AR/GL, bank/close, customer pilot, comparative ERP parity, new load/SLA dan
public/provider production belum diterima. Owned agent/media fixtures tidak
menjadi bukti Gemini live atau klaim seluruh produk production-ready.
