# PRD M01 — penerimaan lokal 6 November 2026

**Lulus untuk scope M01:** fulfilled native sales order → draft invoice dengan
frozen values, source document/line references, current authority dan replay.
Ini bagian DAT-01/DAT-02/FIN-03 dari [PRD aktif](PRD.md), bukan penerimaan seluruh
PRD, financial-close parity, keunggulan pasar atau public production.

## Perilaku yang diterima

Operator mencari customer dan memilih order fulfilled melalui form invoice.
Customer, currency, quantity dan selling price disalin dari snapshot order;
operator tidak memasukkan ulang line sheet. Invoice dan setiap baris mempunyai
referensi sumber yang dijaga database. Perubahan harga katalog setelah fulfillment
tidak mengubah harga invoice. Pilihan source dikosongkan setelah konversi.

Current `billing.write`, Commerce `orders.read`, human membership, tenant,
ownership scope dan module availability tetap diperiksa server. Order belum
fulfilled, stale, managed atau sudah mengalami retur ditolak. Source lock,
tenant-bound FK, unique reference dan deferred completeness guards bekerja dalam
transaksi yang sama dengan invoice, audit dan command receipt.

Original request replay mengembalikan invoice yang sama. Request baru untuk order
yang sudah dikonversi ditolak. Operasi juga tersedia melalui finite business tool
`billing.create_invoice_from_order`; integration test membuktikan mission tidak
mengeksekusinya sebelum approval dan tidak menggandakan hasil saat dijalankan ulang.

## Bukti yang dijalankan

| Pemeriksaan | Hasil aktual |
| --- | --- |
| Full repository gate revisi final | **1.155 tes Rust**, 98 suites, 0 gagal/diabaikan; structure, docs, fmt, workspace Clippy dan WASM boundary lulus |
| Node helpers | 25 gate tests dan 3 product-audit helper tests lulus |
| Focused integration tests baru | 8 lulus: frozen values/SQL/replay/status; invalid source states; scope/grant/revoke; race; later return/correction; DB guards; approved mission; cross-tenant source |
| Parser regression baru | Menolak forged customer/amount/authority fields, UUID tidak valid serta revision nol, negatif, pecahan, string atau di luar int32 |
| Chromium/native final | **75 pemeriksaan lulus**, termasuk 3 pemeriksaan order-invoice dan 30 pemeriksaan agent runtime |
| Product journey probes | **75 terpenuhi, 0 gap** pada suite yang dijalankan; ini bukan 75 requirement baru atau seluruh requirement PRD |
| Media | Native mic/camera, RTP serta model media exchange lulus dengan owned LiveKit/Gemini-compatible transports; bukan provider produksi |
| Browser errors dan cleanup | 0 JavaScript/WASM errors; fixture cleanup exit 0 |
| Serving build | Native production binary, disposable preview dan SSR/WASM payload dibangun ulang dengan provenance yang cocok |
| Native package | **60 artifact** dihitung ulang dan diverifikasi Ops; tidak dideploy |

Browser membuat source berisi tiga unit dengan harga 12.000 IDR, kemudian
mengubah harga katalog menjadi 25.000 sebelum konversi. Form tetap memperlihatkan
36.000 IDR dan menyimpan harga asli. Independent SQL setelah replay serta duplicate
attempt membuktikan satu invoice, source revision 3, quantity 3, unit price 12.000,
gross 36.000 dan source line yang cocok. Reload mempertahankan provenance.

## Identitas kandidat dan reproduksi

Source SHA-256:
`e0390795e6794443071d7860cb953cce529529c5a4a865682655e47d8f43afda`.

Native candidate: `dist/native-candidate-20261106-prd-m01-01`.
Serving provenance: `target/site/build-provenance.json`.
Snapshot memuat 18 business domains, 142 canonical operations, 140 native review
forms dan 2 provider-only operations. Penambahan satu operasi bukan modul baru.

Log dan evidence:

- `target/prd-execution-20261106/gate-full-02.log`
- `target/prd-execution-20261106/order-invoice-tests-02.log`
- `target/prd-execution-20261106/product-helper-tests.log`
- `target/prd-execution-20261106/build-02.log`
- `target/prd-execution-20261106/browser-03.log`
- `target/prd-execution-20261106/package-01.log`
- `target/prd-execution-20261106/final-verification.json`
- `target/e2e/prd-m01-20261106-03/result.json`
- `target/e2e/prd-m01-20261106-03/product-observations.json`
- `target/e2e/prd-m01-20261106-03/operations-order-invoice.png`

Perintah utama mengikuti [developer guide](../DEVELOPER_GUIDE.md). Full gate
memakai `--offline --wasm` tanpa `--lib`. Browser memakai
`ARCHAVA_E2E_MEDIA=1`, `ARCHAVA_E2E_PRODUCT=1`, `ARCHAVA_E2E_AGENT_RUNTIME=1`,
installed Chromium dan `E2E_LIVEKIT_BIN` yang sudah tersedia. Profil/database milik
tes, no-sandbox fallback yang eksplisit, dan synthetic records dicatat pada hasil.

## Temuan dan run sebelumnya

Run pertama lulus tiga pemeriksaan order-invoice, tetapi review screenshot
menemukan pemilih perlu dikosongkan setelah sumber ditagih. Revisi berikutnya
menambahkan reset, customer search dan keterangan batas daftar.

Run luas pertama juga berhenti pada lookup kontak Subscriptions: helper menunggu
target di daftar awal. Helper sekarang menggunakan authorized contact detail dan
pencarian UI, lalu final run menyelesaikan seluruh native module probes. Media
belum diaktifkan pada run pertama. Run kedua berhenti di preflight karena lokasi
binary LiveKit belum diberikan; run ketiga memakai binary yang sudah terpasang.
Semua log/run dipertahankan; tidak ada kegagalan yang diganti menjadi hasil lulus.

## Batas penerimaan dan pekerjaan berikutnya

- Source order dan line IDs sudah stabil; customer name/reference belum menjadi
  master-party identity lintas CRM, Commerce, Purchasing dan Billing.
- Satu source memiliki satu linked invoice, termasuk invoice yang kemudian void.
  Reissue/replacement policy dan credit-note document linkage belum ditambahkan.
- Manual invoices lama tetap valid dengan provenance null. Free-text reference
  tidak dibackfill atau dihitung sebagai hubungan yang telah diverifikasi.
- Retur sebelum konversi ditolak. Retur setelahnya memerlukan correction/refund
  review terpisah; snapshot asli tidak diubah dan credit/refund tidak otomatis.
- Picker menampilkan maksimal 100 recent matching orders dengan customer search;
  tidak ada klaim seluruh histori dapat dibaca dalam satu halaman.
- GL/AP/AR penuh, bank reconciliation, localization, close dan B01–B07 secara
  menyeluruh belum diterima. Pilihan ledger native/adapter tetap keputusan R0.
- Agent runtime menggunakan owned planning fixtures pada run ini; tidak ada
  penerimaan baru atas perencanaan model Google nyata untuk operasi invoice baru.
- Tidak ada comparative customer benchmark, bukti pengurangan waktu 30%, production
  load/SLA atau deployment baru. Target Commerce 50 ms yang sebelumnya terbuka
  belum ditutup oleh pekerjaan ini.

Lanjutkan melalui [tracker eksekusi PRD](PRD_EXECUTION_WORK.md); M02 menyiapkan
master data dan hubungan lintas domain, sementara scope accounting ditetapkan
sebelum implementasi keuangan penuh.
