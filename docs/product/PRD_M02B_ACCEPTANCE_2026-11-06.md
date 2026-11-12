# Penerimaan lokal PRD M02b — identitas supplier CRM

6 November 2026. Scope: supplier registration dari kontak CRM yang dipilih,
refresh profil supplier yang direview secara eksplisit, dan frozen identity pada
PO/receipt. Ini increment DAT-01/DAT-02, bukan penerimaan seluruh master-data PRD.

## Source dan hasil

- Baseline M02a: `0d4f6609d02dcde7bafea2276d3306ad66bd1d5a1a1cd13846f605a78daffe8d`.
- Source final: `ddc6176a54fa4358466b08c23da549f23aa0c9f145c355ee8a171ddabedf3e3c`, 445 input.
- Full gate `verify.mjs --offline --wasm`: **1175 Rust tests dalam 98 suite,
  0 failed, 0 ignored**; structure/docs, 25 helper tests, fmt, Clippy dengan
  `-D warnings` dan kedua WASM boundary checks lulus.
- Tiga product-audit helper tests lulus; 37 workspace flow tests termasuk 9
  supplier integration tests dan 8 upgrade/restore tests juga lulus secara focused.
- Fresh release server, disposable preview dan WASM bundle dibangun dari source
  yang sama. Browser Chromium: **87 checks**, 0 JS/WASM page errors,
  75 existing product probes terpenuhi/0 gaps dan 30 owned agent-runtime checks.
  Media memakai owned fixture transport; bukan provider produksi atau Gemini live.
- Native candidate `dist/native-candidate-20261106-prd-m02b-01`: **63 artifact**, Ops verifier lulus.
  Inventaris: 18 business domains, 145 canonical operations, 143 native forms dan
  2 provider-only operations; 51 SQL files, 44 mounted migrations/24 namespaces.

## Perilaku yang dibuktikan

1. Pilihan kontak dengan nama sama tetap memakai UUID yang tepat. Native picker
   mencari dan memuat halaman kedua; email/organization membantu operator memilih.
   Enter dalam pencarian tidak men-submit supplier yang sudah siap diisi.
2. Nama supplier berasal dari source CRM yang direview. Perubahan sebelum submit
   menghasilkan HTTP 409 tanpa supplier/receipt/audit parsial. Refresh pilihan
   yang eksplisit memungkinkan operator meninjau data terbaru dan mencoba lagi.
3. Panel supplier memuat source UUID yang tersimpan. Refresh membutuhkan supplier
   revision, newer CRM revision, current read authority dan reason; payload tidak
   dapat memindahkan source atau menyisipkan nama, owner dan role.
4. Profile berubah setelah preview juga ditolak 409. Load/review ulang memperbarui
   master supplier dari CRM revision 2 ke 4, tanpa mengubah PO lama revision 2.
5. PO baru memakai snapshot master terbaru. PO lama tetap dapat dikonfirmasi dan
   diterima setelah master diperbarui. Receipt tetap terhubung ke PO historis.
6. Satu kontak digunakan juga oleh customer order, dengan snapshot customer yang
   tetap historis. Ini referensi kontak bersama; supplier ID dan sales-order ID tetap
   terpisah dan tidak menyatakan kesamaan legal entity.
7. SQL oracle independen memeriksa CRM revision 4, supplier revision 2/source 4,
   old PO source 2, new PO source 4 dan customer order source 2 pada kontak sama.
   Receiving 4 accepted/1 rejected menghasilkan stock 4, acquisition value 24.000
   IDR dan tepat satu receipt. Replay tidak menggandakan perubahan supplier.
8. Router/runtime PostgreSQL tests memeriksa current CRM read-only permission,
   revoked permission, tenant/assigned scope, forged fields, missing/stale source,
   supplier revision, Unicode 160 karakter dan manual rows yang tetap unlinked.
9. Race refresh vs PO menghasilkan pasangan nama/source yang konsisten; race code
   sama menerima satu penulis dan menolak satu. Beberapa code untuk satu contact
   tetap diizinkan secara eksplisit. Database guards menolak source repointing,
   false supplier snapshots dan perubahan source pada PO.
10. Approved mission dapat menjalankan kedua operasi melalui authority path yang
    sama. Tanpa approval tidak ada efek; replay run mengembalikan hasil yang sama.

Generic supplier/order pickers juga mempertahankan pilihan DOM ketika projection
berubah, sehingga nama yang ditampilkan cocok dengan ID yang akan dikirim.

## Bukti dan temuan selama validasi

Log dan snapshot kerja berada di `target/prd-m02b-20261106/`. Run focused pertama
`supplier-tests-01.log` mencatat 7 pass/2 fail: satu test memakai URL status yang
salah; satu mengungkap parser manual supplier yang menghitung byte UTF-8.
URL test diperbaiki dan batas supplier disamakan dengan 160 karakter PostgreSQL;
focused rerun serta full gate lulus. Record gagal dipertahankan.

Browser run 01 lulus 87 checks, tetapi screenshot belum menampilkan panel yang
relevan secara penuh. Helper capture kemudian men-scroll panel ke layar. Run 02
lulus pada runtime source yang sama; ketiga screenshot final telah diperiksa
secara visual. Kedua hasil browser dipertahankan.

- [Kontrak supplier identity](../modules/SUPPLIER_IDENTITY.md).
- [Integration tests](../../crates/host/tests/workspace_flows/supplier_identity.rs).
- [Browser + SQL oracle](../../tools/e2e/supplier-identity.mjs).
- Browser final: `target/e2e/prd-m02b-20261106-02/result.json`.
- Screenshot: `supplier-contact-selection.png`, `supplier-profile-review.png`
  dan `supplier-purchase-history.png` di folder browser final.
- Gate/build/package: `verify-01.log`, `build-01.log`, `package-01.log`.
- Final integrity record: `target/prd-m02b-20261106/final-verification.json`.

SQL lama, unrelated source/working-tree state dan laporan M01/M02a dipertahankan;
integrity record memverifikasi checksum migration, source/build/package/browser
binding dan hash helper browser tersendiri. Tidak ada commit atau deployment.

## Batas penerimaan

Manual supplier lama tidak di-link berdasarkan tebakan nama dan tidak dapat
langsung di-retrofit oleh refresh. Tidak ada auto-merge, universal party registry,
legal entity, vendor bank/account, company-book atau organisasi lintas modul baru.
M02c tetap terbuka. Accounting profile R0 serta AP/AR/GL, bank dan close belum
selesai. Tidak ada klaim ERP parity/superiority, customer-pilot benefit, comparative
benchmark, new load/SLA, staging/public deployment atau production-provider baru.
