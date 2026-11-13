# Penerimaan lokal company-owned trade — 7 November 2026

**M02c2c2 diterima untuk scope lokal di bawah.** Ini penerimaan produk/stok/PO/sales/invoice milik perusahaan, bukan penerimaan seluruh ERP, pembukuan resmi atau produksi publik. Pekerjaan berjalan atas instruksi owner untuk meneruskan lintas bagian dengan pemeriksaan berlapis; tidak ada deploy.

Source: `60b4b5b01b67d923d23b7e4e0f2567e647d08c1be8b4a8fd5d185eb1c76d9118`, 471 input. Kandidat native: `dist/native-candidate-20261107-prd-company-trade-05`, 72 artifact. Permukaan saat ini: 18 domain, 161 operasi kanonis, 159 form native dan dua operasi billing khusus provider. Jumlah operasi tidak mengukur kedalaman ERP atau rata-rata pasar.

## Hasil yang diterima

- Produk mempunyai satu pemilik company atau scope legacy tanpa company; identitas ini tidak dapat dipindahkan. Product ID memisahkan stok fisik dan FIFO. SKU tetap unik satu workspace.
- Pembuatan produk, counted stock, sales dan PO memakai company aktif pada versi yang direview. Sales juga memakai published customer party; supplier/offer menjaga aturan source party yang ada.
- PO/receipt, reserve/fulfill/cancel/return dan invoice dari fulfillment memakai company yang sama. Dokumen menyimpan ID/revisi/code/official name historis dan FK ke company history. Komitmen lama dapat diselesaikan setelah company tidak aktif, dengan izin terkini.
- Workspace Commerce/Purchasing/Billing secara eksplisit dipilih company; tanpa pilihan hanya mengembalikan data legacy. Human company grant/scope diperiksa lagi untuk read, mutation, replay, status, signed checkout, email dan payment history. Callback provider tetap memakai signature/service authority dan invoice immutable.
- Native picker memiliki search/paging, review revision dan refresh terpisah. Pergantian scope membersihkan records/selection/cart. Respons terlambat tidak menimpa scope baru; refresh gagal membersihkan detail lama tanpa beralih diam-diam ke legacy. Identitas historis terlihat pada dokumen.
- Ava menggunakan company pada context dan source guard reads. Follow-up mempertahankan company goal; Organizations membership ikut access fingerprint. Rencana memakai action company dan observed/verified references. Owned-model dua langkah create product → counted stock lolos, termasuk replay approval dan pencabutan izin sebelum kerja berikutnya.
- Email menampilkan issuer company historis di text dan escaped HTML, memakai immutable outbox. Owned gateway menyelesaikan invoice yang sama dari hasil provider terverifikasi, tanpa client mark-paid.

[Kontrak dan batas lengkap](../modules/COMPANY_TRADE.md) membedakan declared identity, current authority, historical completion dan pembukuan.

## Tiga lapis verifikasi

| Lapisan | Hasil |
| --- | --- |
| Aturan dan persistence | Delapan router/PostgreSQL tests company trade: alur PO→stock→sales→invoice/FIFO; lintas company/legacy; inactive/current review; grant/scope/revoke/replay/status/checkout; DB identity/line guards; stok bersamaan; upgrade profile lama dua kali tanpa backfill atau perubahan receipt |
| Regresi repositori | 1229 Rust tests, 0 gagal, 0 diabaikan, 98 suites; 25 helper Node; struktur, docs, fmt, Clippy semua target dengan warnings ditolak, serta WASM boundary |
| Native dan Chromium | 123 pemeriksaan browser, termasuk sepuluh company-trade, 75 product probes/0 gap, 30 owned agent-runtime checks dan media transport; 0 JS page errors; fixture cleanup exit0; native package dan SHA/provenance diverifikasi |

Regresi penuh di atas dijalankan pada source `3d70a43090dccf750f07da73a85cc3e7664d49cd256355af2ec8ec45697e895c`. Setelah itu hanya pesan akses UI dan stylesheet berubah. Gate library/Clippy/WASM final (`verify-08-ui.log`) serta build dan browser baru menguji delta dua file tersebut; bukti full gate tidak diklaim berasal dari stylesheet yang lebih baru.

Fixture browser menjalankan native preview dengan Host/SSR/WASM dan PostgreSQL runtime non-owner untuk alur bisnis serta owned payment/email. Setelah itu executable produksi direstart pada database/session yang sama dan membuktikan stok, PO/sale, settled invoice beserta original receipt bertahan. Owned test endpoints tidak ditambahkan ke konfigurasi produksi; parser HTTPS kanonis tetap berlaku. Chromium, model dan media memakai fixture lokal yang dimiliki task. Bukti ini tidak menambah penerimaan Gemini reasoning kualitas umum atau provider produksi.

Oracle SQL independen untuk journey native: purchase receipt company A; stok A enam, stok B nol; fulfillment FIFO 10.000 dan inventory acquisition value 36.000; source invoice gross/settled 30.000, outstanding nol dan state settled; company/revision konsisten; tiga movement beratribusi company serta issuer email sesuai snapshot. Tampilan produk/sales/invoice direview dari screenshot hasil run yang sama.

## Kegagalan yang diperbaiki

Gate pertama menolak tiga pola lint pada markup UI; diperbaiki. Regresi penuh berikutnya menemukan replay produk legacy 503 pada whole-row SQL reference; query diberi alias eksplisit, termasuk query issuer email yang memakai pola sama, dan tes gagal lolos pada focused rerun. Audit jalur alternatif menambah company authorization pada mint signed checkout. Refresh UI yang ditolak kini menghapus data/detail lama. Pemeriksaan serialisasi request lama menemukan bahwa field company null baru dapat mengubah hash request Ava; field kosong sekarang dihilangkan dari serialisasi dan oracle wire/hash pra-company serta replay explicit null lulus. Doctest putaran ketiga juga gagal membaca dependency compiled (E0463) setelah build terarah bersamaan; gate final dijalankan berurutan dan seluruh doctest harus lolos. Finalisasi checkout pernah salah memeriksa service gateway sebagai human company reader. Cek tersebut dihapus dari trusted service response boundary; service binding/current grants dan invoice lock tetap wajib, lalu current human company access diperiksa sebelum metadata dikembalikan. Tes baru membuktikan service tanpa Org membership menyimpan hasil setelah grant human dicabut, sementara human tidak dapat membaca receipt sampai haknya dipulihkan. Full gate baru dijalankan untuk source final setelah perubahan backend ini.

Build awal yang menjadi stale dihentikan pada compiler milik task saat perbaikan ditemukan. Build08 ditolak oleh provenance guard karena format Rust selesai 251ms setelah snapshot awal source; build09 memakai hasil cache kompilasi yang sesuai dan mengulangi manifest/asset binding dari source stabil. Build akhir dibuat ulang setelah source dibekukan dan diverifikasi terhadap paket. Browser pertama kemudian menemukan cache pilihan company yang mengembalikan versi lama ketika operator pindah lalu kembali. Refresh terpilih sekarang memperbarui cache baris pada halaman tersebut, dan pesan refresh dibersihkan saat berpindah. Browser kedua juga menemukan keranjang yang tertinggal: effects hanya men-track memo yang belum pernah dievaluasi. Kelima reset effect sekarang membaca company-key memo, sehingga dependency terhubung dan perubahan ID membersihkan basket serta selected document. Native journey menguji reset sales cart, purchase cart dan unsaved manual invoice lines. Journey browser berikutnya menguji bahwa versi tiga tetap dipilih tanpa refresh ulang; counted stock harus berhasil. Penghentian compiler tersebut tidak ditafsirkan sebagai kegagalan tes bisnis; provenance helper tetap menolak input atau artifact yang berbeda dari paket.

Review screenshot juga menemukan kontrol legacy yang tetap terlihat karena CSS grid menimpa atribut HTML hidden. Aturan `[hidden]` sekarang menjaga semantik visibility, browser memeriksa kedua label tidak memiliki painted rectangle, dan screenshot final direview ulang. Penolakan akses dalam mode company memberi pesan yang sesuai tanpa menyebut modul lain.

## Bukti yang dapat diperiksa

- `target/prd-company-flows-20261107/verify-07.log`, `target/prd-company-flows-20261107/focused-04.log`, `target/prd-company-flows-20261107/agent-test-01.log`, `target/prd-company-flows-20261107/legacy-replay-01.log`.
- `target/prd-company-flows-20261107/build-09.log`, `target/prd-company-flows-20261107/package-05.log`, `target/prd-company-flows-20261107/rust-evidence.json`.
- `target/e2e/prd-company-trade-20261107-06/result.json`, `target/prd-company-flows-20261107/visual-review.json`, `target/prd-company-flows-20261107/product-audit.json` dan `target/prd-company-flows-20261107/final-verification.json`.
- [Router/database witnesses](../../crates/host/tests/workspace_flows/company_trade.rs), [native browser/SQL journey](../../tools/e2e/company-trade.mjs), [Ava witnesses](../../crates/host/tests/agent_runtime.rs).

Seluruh SQL historis, daftar/grants migrasi sebelumnya, action catalogue lama, source di luar scope, status working tree yang tidak terkait, tujuh penerimaan sebelumnya dan archive dipertahankan. Tiga SQL baru terdaftar bersama checksum/native/fixture: 60 file SQL, 53 mounted, tujuh unmounted dalam 24 namespaces/30 direktori ledger. Upgrade tidak membuat company atau mengatribusi legacy dari tebakan.

## Yang tetap terbuka

Company master masih declared identity, bukan legal verification. Cohort industri lain, invoice manual dan bounded local replenishment suggestions tetap legacy. Return fisik tidak otomatis menerbitkan kredit finansial. Company/account/book mapping, intercompany, consolidation, GL resmi, AP/AR lengkap, bank/close, negara/currency, integrasi prioritas, R0 release profile, live-provider produksi, SLA/load, pilot serta perbandingan pasar belum diterima. Tidak ada pilihan accounting yang dibuat atas nama owner.

Pekerjaan berikutnya dapat menyiapkan pemeriksaan dokumen tagihan supplier terhadap PO/accepted receipts sebagai dokumen operasional unposted. Itu harus mempunyai kontrak dan bukti tersendiri; penerimaan ini tidak mengklaim fitur tersebut sudah tersedia.
