# Protokol pembuktian kesetaraan ERP Archava

Disusun 6 November 2026 berdasarkan
[riset ERP global](ERP_GLOBAL_BENCHMARK_2026-11-06.md).

**Semua skenario dan kriteria di bawah ini adalah rancangan pengujian dan backlog.
Dokumen ini tidak menyatakan bahwa pengujian tersebut sudah dijalankan atau lulus.**
Angka dan batas penerimaan adalah usulan untuk disepakati terhadap proses pelanggan,
bukan SLA produksi yang sudah ditawarkan.

## 1. Pilih scope yang dapat dibandingkan

Sebelum mengukur, tulis sebuah profil acceptance dengan:

- Segmen, negara, ukuran dan jenis usaha, serta proses yang wajib diselesaikan.
- Daftar entity, cabang, gudang, mata uang dan periode yang dipakai.
- Produk pesaing, versi, lisensi, add-on dan konfigurasi yang menjadi baseline.
- Pilihan sumber accounting yang berwenang: native Archava atau ERP/accounting eksternal.
- Owner data dan dokumen serta sistem yang berhak mengubah setiap saldo.
- Workflow approval, role, kewenangan, audit dan cara menangani pengecualian.
- Dataset, opening balance, dokumen awal, expected outcome dan kasus negatif.
- Infrastruktur, latensi jaringan, jumlah pengguna dan workload.
- Model AI, reasoning profile, prompt/tool version, quota dan cost accounting.
- Definisi tugas selesai, gagal, belum pasti, perlu manusia, dibatalkan dan diperbaiki.

Scope harus mencakup pekerjaan yang benar-benar hendak digantikan. Pembanding
invoice sederhana tidak dapat mengesahkan manufacturing ERP. Perbandingan dengan
produk berbayar harus menghitung fitur/add-on yang diperlukan, sementara biaya
operasional Archava harus memasukkan hosting, database, AI, integrasi dan support.

Simpan source SHA, release manifest, konfigurasi yang sudah disanitasi, fixture
digest, test command, waktu, log dan output database. Jangan memasukkan credential
atau data pribadi pelanggan ke paket benchmark.

## 2. Prioritas backlog dengan hasil yang bisa diperiksa

| Prioritas | Deliverable | Ketergantungan | Bukti untuk menyatakan selesai |
| --- | --- | --- | --- |
| P0 | Register kemampuan yang membedakan library, API, UI, integrasi dan penerimaan | Inventaris source/manifest/router | Klaim produk dan paket uji mengacu ke register yang sama; drift dokumen teridentifikasi |
| P0 | Strategi accounting dan master data | Scope pelanggan | Owner customer/item/vendor/entity/account ditetapkan; mapping antarsistem dan dokumen eksplisit |
| P0 | GL native atau adaptor accounting sungguhan | Strategi accounting | Posting, reversal, periode, saldo, audit dan pemulihan dapat dipakai melalui produk |
| P0 | Order-to-cash lengkap | Customer/item/AR/GL, transaksi operasi | Order, shipment, invoice, receipt, reconciliation dan posting memiliki keterlacakan yang tervalidasi |
| P0 | Source-to-pay lengkap | Supplier/item/AP/GL, transaksi operasi | PO/receipt/bill/matching/payment/reconciliation selesai dengan jumlah dan nilai tepat |
| P0 | Tutup satu periode dan koreksi | GL, AR/AP, bank, persediaan | Trial balance dan subledger cocok; periode tertutup menolak perubahan tidak sah |
| P1 | Laporan penuh dan query tools berizin | Model data dan ledger | Filter periode/entity benar; aggregate tidak dihitung dari first page; drill-down cocok ke dokumen |
| P1 | IAM dan operasi sasaran | Profil pelanggan/hosting | Identity, akses, audit, backup, restore, upgrade dan alert diuji di lingkungan sasaran |
| P1 | Migrasi opening balance dan histori | Strategi data/ledger | Mapping, total kontrol, rollback dan source-to-target reconciliation menghasilkan bukti |
| P1 | Benchmark Ava pada tujuan bisnis yang lebih luas | Workflow deterministik sudah benar | Correctness database, approval, recovery, context completeness dan biaya tugas dilaporkan |
| P1 per segmen | Multiwarehouse/UOM/lot/serial atau project accounting | Kebutuhan pelanggan | Workflow industri yang dipilih mencapai hasil stok/biaya/keuangan yang benar |
| P2 per segmen | MRP, HR/payroll, consolidation atau vertical mendalam | Fondasi dan discovery sektor | Satu lifecycle industri lengkap, role negatif, koreksi dan integrasi finance diuji |
| P2 | Ekosistem connector dan konfigurasi lanjutan | Kontrak authority dan versi | OAuth/secret lifecycle, webhook, replay, mapping dan delivery reconciliation terbukti |

P0 berlaku untuk target menggantikan sumber ERP/keuangan pada scope tersebut.
Jika Archava hanya menjadi lapisan agen untuk ERP existing, P0 accounting berarti
integrasi yang benar dengan sistem itu, bukan kewajiban membangun GL kedua.

## 3. Benchmark B01 — transaksi sampai laporan keuangan

Tujuan: membuktikan bahwa operasi dan accounting menghasilkan angka yang sama.
Skenario ini sengaja menyederhanakan pajak, ongkir dan diskon menjadi nol.
Ini ilustrasi kontrol, bukan kebijakan pajak atau panduan pembukuan untuk usaha nyata.

### Dataset dan asumsi

- Satu entity, satu currency dan satu periode terbuka.
- Opening bank 1.000.000 dan opening equity 1.000.000.
- Tidak ada stok, utang, piutang atau transaksi lama.
- Satu supplier, satu customer dan satu SKU stok.
- Pembelian 10 unit dengan biaya 100.000 per unit.
- Penjualan 4 unit dengan harga 150.000 per unit.
- Semua transaksi dalam satu periode; tidak ada biaya lain.
- Policy pengakuan receipt, vendor bill dan kemungkinan akun perantara ditetapkan
  sebelum tes. Akhir proses receipt/bill harus memberi hasil stok/AP yang konsisten.

### Tahapan dan expected outcome

| Tahap | Expected outcome independen |
| --- | --- |
| Opening balance | Bank 1.000.000; equity 1.000.000; trial balance seimbang |
| PO, inspected receipt dan vendor bill | Stok 10 unit bernilai 1.000.000; AP 1.000.000 setelah seluruh tahap pengakuan selesai |
| Sales order dan fulfillment 4 unit | Stok tinggal 6; biaya persediaan keluar 400.000; dokumen fulfillment dapat ditelusuri |
| Customer invoice 600.000 | AR 600.000 dan revenue 600.000; COGS total 400.000 sesuai policy yang ditetapkan |
| Customer payment dan bank reconciliation | AR nol; bank 1.600.000; settlement dan mutasi bank terhubung tanpa pengakuan ganda |
| Supplier payment dan bank reconciliation | AP nol; bank 600.000 |
| Laporan sebelum retur | Inventory 600.000; revenue 600.000; COGS 400.000; profit 200.000; total assets 1.200.000; equity 1.200.000 |
| Retur satu unit, credit/refund 150.000 | Stok 7 unit bernilai 700.000; bank 450.000; net revenue 450.000; net COGS 300.000; profit 150.000 |
| Tutup periode | AR/AP nol; assets 1.150.000 sama dengan equity 1.150.000; seluruh laporan dan dokumen saling cocok |

Sistem harus membuktikan hasil akhir tersebut dari posting dan dokumen yang
disimpan, bukan meminta LLM menuliskan angka yang diharapkan. Tanggung jawab
debit/kredit dan timing posting harus diterjemahkan ke konfigurasi accounting
yang disepakati. Bila memakai ERP eksternal, expected outcome diverifikasi di
ledger sistem itu dan referensinya dikorelasikan dengan Archava.

### Kasus kegagalan yang wajib masuk

1. Respons hilang setelah business commit; pengiriman ulang mempertahankan efek
   dan key asli. Nominal, stok dan jurnal tidak menjadi dua kali lipat.
2. Webhook pembayaran diterima berulang atau datang tidak berurutan.
3. Supplier receipt sebagian, rejected unit dan selisih tagihan membuat matching
   exception, bukan diam-diam menyeimbangkan nilai yang salah.
4. Harga atau landed cost berubah setelah dokumen dibekukan; basis yang digunakan
   mengikuti kontrak dan jejak koreksi, bukan data saat ini yang kebetulan berbeda.
5. Retur parsial berulang tidak mengembalikan unit atau cost lebih dari konsumsi asli.
6. Perubahan data setelah rencana direview memicu pemeriksaan revisi dan review baru.
7. Actor tanpa grant, lintas tenant/entity atau actor yang approver-nya tidak
   memenuhi pemisahan tugas ditolak sebelum efek yang tidak sah.
8. Posting ke periode tertutup, tanggal invalid, akun tidak berlaku atau currency
   yang tidak sesuai ditolak atau masuk proses koreksi/reopen yang sah.
9. Restore dan upgrade mempertahankan saldo, referensi, histori dan larangan
   menjalankan ulang efek eksternal yang sudah terjadi.
10. Seluruh kontrol tetap bekerja ketika AI tidak dikonfigurasi atau provider gagal.

Penerimaan B01 memerlukan hasil operasi, subledger, bank, GL dan laporan cocok.
Lulus membuat invoice saja tidak mencukupi. Struktur pembandingnya terlihat pada
[record to report](https://learn.microsoft.com/en-us/dynamics365/guidance/business-processes/record-to-report-overview)
dan [alur pengadaan](https://learn.microsoft.com/en-us/dynamics365/guidance/business-processes/source-to-pay-overview).

## 4. Benchmark B02 — multiwarehouse dan traceability

Gunakan SKU yang sama pada dua gudang, dengan stok awal, cost dan lot berbeda.
Uji penerimaan, transfer antar lokasi, reservation, picking, dispatch, retur,
stock count dan koreksi. Total harus benar di setiap lokasi dan setelah roll-up.

Uji setidaknya:

- Transfer mengurangi lokasi sumber dan menambah tujuan sesuai status transit
  serta aturan pengakuan yang disepakati.
- Dua operator mengambil unit terakhir bersamaan tanpa oversell.
- UOM pembelian, stocking dan penjualan mempunyai konversi serta rounding yang jelas.
- Batch/serial, expiry, quality hold dan lokasi quarantine diperiksa bila wajib
  bagi bisnis tersebut; serial asset tidak menggantikan serial inventory.
- Retur mengacu pada pengiriman dan basis biaya yang benar, bukan cost rata-rata
  yang dihitung ulang tanpa kontrak.
- Satu serial/batch dapat ditelusuri ke supplier/receipt dan customer/shipment.
- Nilai persediaan dan cost movement direkonsiliasi dengan accounting.

Pool stok Archava saat ini belum membuktikan acceptance ini. Fitur dapat dibangun
native atau disediakan warehouse system yang diintegrasikan secara nyata.

## 5. Benchmark B03 — produksi yang terhubung dengan finansial

Untuk target manufacturing, pilih satu produk dengan BOM bertingkat, setidaknya
dua work center dan satu komponen yang kekurangan stok. Uji demand, perhitungan
kebutuhan, procurement, kapasitas, work order, pemakaian, scrap, quality, hasil,
costing dan posting.

Kriteria hasil:

- Gross/net material requirements dan lead time memengaruhi rencana pembelian/produksi.
- Bottleneck kapasitas benar-benar mengubah jadwal yang dapat dilaksanakan.
- Consumption, WIP dan finished goods mempertahankan kuantitas dan nilai.
- Yield/scrap dan varians dapat dijelaskan; perbedaan biaya tidak menghilang.
- Subcontracting dapat menelusuri material milik sendiri dan hasil yang diterima.
- Versi BOM/routing pada order tetap dapat diketahui setelah engineering change.
- Hasil quality membatasi operasi berikutnya sesuai policy.
- Jurnal dan saldo finance cocok dengan cost produksi setelah close.

Ini merupakan contoh kedalaman yang perlu dibandingkan dengan
[Work Order ERPNext](https://docs.frappe.io/erpnext/work-order) dan
[manufacturing Business Central](https://learn.microsoft.com/en-us/dynamics365/business-central/production-manage-manufacturing),
dengan memperhatikan paket Premium dan konfigurasi yang berlaku.
Library BOM dua test case tidak mengesahkan seluruh proses tersebut.

## 6. Benchmark B04 — organisasi, currency dan penutupan grup

Untuk target grup perusahaan, bedakan customer aplikasi, badan hukum, cabang,
gudang, cost center dan ledger. Buat dua entity dengan functional currency berbeda,
akun pemetaan yang jelas dan transaksi intercompany.

Uji:

- Buku dan periode tiap entity, serta pembatasan akses lintas entity.
- Invoice/payment foreign currency dan realized/unrealized FX sesuai policy.
- Transaksi intercompany yang berpasangan dan exception bila nilainya berbeda.
- Translasi, eliminasi, consolidation dan drill-down ke dokumen sumber.
- Perubahan kurs setelah posting tidak diam-diam mengubah histori.
- Report per entity dan report grup bisa direkonsiliasi.

Bandingkan terhadap paket yang benar. Contohnya, OneWorld merupakan add-on dan
mendokumentasikan subsidiary, currency serta konsolidasi.
[NetSuite OneWorld](https://www.netsuite.com/portal/products/global-business-management.shtml).
Keberhasilan tenant isolation Archava tidak dihitung sebagai hasil B04.

## 7. Benchmark B05 — tujuan bisnis dan agen AI

Pilih kumpulan tujuan berdasarkan pekerjaan nyata segmen sasaran. Mulai dengan
tujuan yang sudah ada dalam katalog simulasi, lalu tambahkan variasi yang belum
pernah dipakai untuk menyetel prompt. Pisahkan suite pengembangan dari suite
evaluasi agar hasil tidak mengukur hafalan terhadap test case.

### Variasi yang perlu dicakup

- Bahasa Indonesia/Inggris, parafrase, typo, tanggal dan angka yang ambigu.
- Data lengkap, hilang, bertentangan, record dengan nama sama dan revisi berubah.
- Customer/product yang terletak di luar halaman pertama atau koleksi pertama.
- Tujuan beberapa langkah dan beberapa domain, serta tujuan yang memang belum didukung.
- Follow-up, pembatalan, perubahan scope setelah sebagian langkah berhasil.
- Current-grant/session revoke, batas entity, tenant berbeda dan approver berbeda.
- Instruksi berbahaya di chat, catatan bisnis, dokumen atau hasil integrasi.
- Provider timeout/quota, response hilang, JSON valid tetapi referensi semantik salah.
- Kegagalan checkpoint setelah efek commit dan delivery webhook yang berulang.

### Ukur hasilnya pada data dan pengalaman pengguna

| Metrik | Cara pengukuran |
| --- | --- |
| Penyelesaian benar | Expected business state dan receipt diverifikasi independen dari teks model |
| Aksi tidak sah | Setiap perubahan yang melewati current authority/approval ditandai critical failure |
| Salah record/angka | Salah entity, barang, supplier, jumlah, harga atau periode tidak disembunyikan di skor rata-rata |
| Klarifikasi | Agen meminta data yang memang kurang dan mempertahankan tujuan, bukan membuat identifier |
| Recovery | Original effect diketahui; retry tidak menggandakan operasi atau mengabaikan partial completion |
| Kelengkapan konteks | Klaim total/seluruh bisnis sesuai data yang benar-benar dicakup dan dapat ditelusuri |
| Usaha pengguna | Langkah manual, koreksi, waktu review dan intervensi untuk menuntaskan tugas |
| Latensi | Pisahkan waktu model, aplikasi, integrasi, approval manusia dan end-to-end task |
| Biaya | Token, media, provider, retry, infra dan tenaga review per tugas yang benar-benar selesai |
| Ketersediaan | Hasil saat model/provider tidak tersedia, termasuk batas kuota akun |

Usulan batas keselamatan: tidak ada unauthorized mutation yang diterima dan tidak
ada klaim selesai tanpa bukti transaksi. Target throughput, keberhasilan, latensi
dan biaya harus dipilih untuk scope pelanggan; dokumen ini tidak menetapkan
angka universal 95%, 99% atau SLA yang belum diukur.

Jalankan baseline manusia/form dan baseline competitor dengan tugas yang sama.
Simpan semua percobaan, termasuk prompt yang gagal dan kasus yang membutuhkan
manusia. Jangan mengganti model atau mengubah suite sesudah gagal lalu menyebut
hasilnya seolah berasal dari konfigurasi awal. Versi model/profile adalah bagian
dari kandidat yang dievaluasi.

Enam tujuan provider langsung pada 5 November merupakan bukti awal, bukan
hasil pelaksanaan benchmark B05 yang lebih luas.
[Scope penerimaan terdahulu](AGENT_LIVE_ACCEPTANCE_2026-11-05.md).

## 8. Benchmark B06 — IAM, integrasi dan pengoperasian

### Identitas dan kontrol

Uji login, MFA/SSO bila wajib, onboarding/offboarding, perubahan role, batas
record/entity, approval independen, akses service account dan audit akses.
Pisahkan actor manusia, agent, integrasi dan operator infrastruktur agar tindakan
dapat diatribusikan. Referensi kemampuan identitas pesaing:
[Business Central dan Entra MFA](https://learn.microsoft.com/en-us/dynamics365/business-central/dev-itpro/security/multifactor-authentication).

### Integrasi

Setiap adaptor wajib mempunyai sumber truth, mapping, identifier, key lifecycle,
permission scope, webhook verification, idempotency, timeout, retry dan cara
rekonsiliasi. Tentukan kompensasi ketika layanan eksternal sudah mengeksekusi
aksi tetapi responsnya hilang. Jangan menjanjikan exactly-once efek jaringan
hanya karena transaksi PostgreSQL lokal atomik.

### Migrasi, operasi dan disaster recovery

- Migrasi master data, opening balance dan histori dengan total kontrol.
- Upgrade profile dan migrasi database tanpa merusak histori dan hak akses.
- Backup terenkripsi, restore penuh, verifikasi saldo/dokumen dan kembali melayani.
- Drain/takeover writer, crash saat request berlangsung dan penolakan stale writer.
- Secret rotation, alert yang benar-benar diterima dan diagnosis dari log yang disanitasi.
- Restore dari lokasi cadangan dan bukti RTO/RPO pada ukuran data sasaran.
- Dukungan operasional: siapa merespons gangguan, jadwal, runbook dan batas layanan.

Paket lokal Archava sudah memiliki sebagian bukti fondasi ini; acceptance
lingkungan pelanggan tetap harus mengukur kegunaan prosedur dan hasil sebenarnya.
[Kontrak writer](../architecture/WRITE_FENCING.md),
[instalasi/pemulihan native](../../deploy/NATIVE_INSTALL.md).

## 9. Benchmark B07 — performa, TCO dan pilot

Bandingkan pada workload, ukuran data, region dan aturan bisnis yang sama.
Uji beban campuran; operasi baca ringan saja tidak mewakili reservation, invoice,
posting, report, integration worker dan pekerjaan agent pada saat bersamaan.

Laporkan p50/p95/p99, request errors, kesalahan bisnis, throughput, queue lag,
utilisasi DB/CPU/memory dan biaya. Bedakan warm/cold cache, single-instance,
handover serta konfigurasi node/DB. Ukur AI task latency dan user approval time
secara terpisah dari endpoint latency.

TCO perlu memasukkan lisensi/add-on, setup, migrasi, kustomisasi, konektor, AI,
hosting, database, backup, observability, support, upgrade dan waktu pengguna.
Tidak ada quotation atau studi TCO sebanding yang tersedia dalam riset ini.

Sebagai usulan awal, pilot harus mencakup satu siklus operasi dan penutupan buku
untuk segmen yang memang membutuhkan accounting. Jumlah pelanggan, lama pilot dan
target KPI ditetapkan sebelum pengujian. Kumpulkan baseline sebelum penggunaan
dan outcome setelahnya, bukan hanya testimoni atau jumlah akun yang dibuat.

Klaim "lebih unggul" baru layak untuk scope yang sama ketika hasil bisnis dan
kontrol setara, serta perbaikan waktu/biaya/usaha pengguna terukur dan dapat
diulang. Tidak ada dasar dalam riset ini untuk klaim menang pada seluruh ERP dunia.

## 10. Artefak untuk keputusan berikutnya

Untuk setiap kandidat, hasilkan:

1. Profil scope, edisi/licensing dan source release yang dipakai.
2. Diagram alur dokumen dan kepemilikan data/accounting.
3. Fixture/dataset dan expected outcome yang disetujui pemilik proses.
4. Test log, query/verifikasi independen dan seluruh failure/recovery traces.
5. Matriks role/entity/tenant, approval dan hasil kasus negatif.
6. Latensi, task completion, usaha pengguna dan biaya dengan denominator jelas.
7. Hasil migrasi/restore/upgrade, alert dan runbook operator.
8. Keputusan: diterima pada scope, diterima bersyarat pada batas tertentu, atau
   belum diterima; setiap pengecualian ditulis bersama buktinya.

Ini membuat keputusan produk konkret tanpa menyamakan library yang tersedia,
jumlah modul dan kualitas implementasi dengan kemampuan ERP yang telah diterima.
