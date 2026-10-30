# Riset model bisnis dan reusable modules — 8 Oktober 2026

<!-- documentation-status -->
> **Status: dated research; external facts not revalidated in this audit.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

**Company:** ARCHZXY. **Product:** Archava. **Baseline repo:** `1fad26a`.
**Hasil:** [peta kapabilitas](BUSINESS_CAPABILITY_MATRIX.md), dengan pola operasi,
contoh industri, kandidat modul, batas reuse, dan prioritas.

## Keputusan utama

Buat satu katalog kapabilitas bersama dan profil komposisi berdasarkan kebutuhan
bisnis. Satu bisnis bisa memakai beberapa pola. Katalog ini menjadi bahan discovery
dan roadmap; implementasi mengikuti kebutuhan nyata serta batas kesiapan repo.

Klasifikasi berikut adalah **sintesis untuk ARCHZXY**. Dokumentasi vendor membantu
mengidentifikasi transaksi dan batas domain. Dokumentasi tersebut tidak membuktikan
permintaan pasar, persentase reuse, biaya operasional kita, atau kesiapan Archava.

## Metode dan cakupan

Riset menggunakan 32 rujukan primer: dokumentasi Odoo, Medusa, ERPNext/Frappe,
Stripe, Oracle Hospitality, Shopify, Midtrans, HL7, dan Strategyzer. Daftar sumber
beserta klaim terbatas yang didukung tersedia di
[ledger sumber](evidence/2026-10-08-business-model-sources.json).

29 halaman diperiksa melalui isi dokumen; tiga halaman Medusa menggunakan kutipan
hasil pencarian resmi karena fetch langsung timeout. Halaman Odoo yang timeout
diverifikasi melalui sumber dokumentasi resmi pada branch `19.0`. Versi tersebut
menjadi referensi yang dipilih, bukan klaim versi terbaru. Halaman rolling dicatat
sebagai snapshot tanggal riset. Halaman kosong/gagal tidak dipakai sebagai bukti.

Ini riset arsitektur dan workflow. Tidak ada instalasi framework, benchmark vendor,
deployment, panggilan model, penggunaan data pelanggan, atau perubahan pricebook.
Interview pelanggan dan validasi aturan tiap bisnis tetap diperlukan. Cakupan ini
tidak mengklaim seluruh internet atau seluruh jenis usaha sudah terwakili.

## Temuan yang memengaruhi desain

### 1. Model bisnis lebih luas daripada nama industri

Business Model Canvas mencakup pelanggan, nilai yang ditawarkan, channel, aktivitas,
resource, pendapatan, dan biaya. Menyebut “hotel” saja belum menjelaskan bagaimana
usaha tersebut beroperasi atau mendapatkan pendapatan.
[Strategyzer — Business Model Canvas](https://www.strategyzer.com/library/the-business-model-canvas).

**Sintesis kita:** discovery menyimpan empat sumbu: pola operasi, pendapatan,
channel, dan domain industri. Contohnya, gym dapat menggabungkan membership,
appointment personal trainer, dan penjualan produk. Profil komposisi harus dapat
mewakili kombinasi tersebut tanpa membuat tiga aplikasi klien hasil fork.

### 2. Komposisi modul mempunyai contoh nyata

Medusa menyusun commerce dari domain seperti produk, order, customer, dan
fulfillment. Workflow add-to-cart menggabungkan beberapa layanan. Dokumentasinya
juga membatasi akses antar-module dan menyediakan links untuk relasi data.
[Commerce modules](https://docs.medusajs.com/learn/fundamentals/modules/commerce-modules),
[module isolation](https://docs.medusajs.com/learn/fundamentals/modules/isolation),
[module links](https://docs.medusajs.com/learn/fundamentals/module-links).

**Sintesis kita:** gunakan kontrak domain yang sudah ada, dengan pemilik resource
yang jelas. Orkestrasi aplikasi menghubungkan layanan. Tidak perlu menyalin
container, workflow engine, atau seluruh framework vendor. Katalog perencanaan
juga tidak berarti setiap baris langsung menjadi package baru.

### 3. Booking berbagi mekanik, tetapi aturan bisnis tetap berbeda

Odoo Appointments mempunyai availability untuk pengguna/resource, kapasitas,
dan timezone. Rental mempunyai proses quotation, pickup, return, dan invoice;
konfigurasi produknya juga mencakup ruang yang tidak berpindah secara fisik.
[Appointments](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/productivity/appointments.rst),
[Rental](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/sales/rental.rst),
[rental product types](https://raw.githubusercontent.com/odoo/documentation/19.0/content/applications/sales/rental/configure_products/product_type.rst).

Oracle memodelkan reservasi hotel dan housekeeping dengan status kamar,
out-of-order/out-of-service, serta perbedaan status front office dan housekeeping.
[Bookings](https://docs.oracle.com/en/industries/hospitality/opera-cloud/26.4/ocsuh/ch_bookings.htm),
[room management](https://docs.oracle.com/en/industries/hospitality/opera-cloud/25.3/ocsuh/c_housekeeping_room_management.htm).

**Sintesis kita:** reuse interval waktu, resource reference, dan mekanik konflik
yang terbukti sama. Pertahankan aturan room-night, asset handover/return, dan
appointment sebagai kontrak yang eksplisit. Reservasi kuantitas barang juga
berbeda dari interval waktu.
[Medusa inventory reservations](https://docs.medusajs.com/resources/commerce-modules/inventory/reservations-lifecycle).

### 4. Order, invoice, payment, dan accounting mempunyai tanggung jawab berbeda

Medusa menghubungkan transaksi order ke payment/refund, sementara payment
processing dimiliki Payment Module. ERPNext memperlihatkan alur quotation,
sales order, delivery, invoice, dan payment entry sebagai langkah terpisah.
[Order transactions](https://docs.medusajs.com/resources/commerce-modules/order/transactions),
[payment flow](https://docs.medusajs.com/resources/commerce-modules/payment/payment-flow),
[ERPNext Sales Order](https://docs.frappe.io/erpnext/sales-order).

ERPNext menjaga riwayat posting dan reversal untuk koreksi. Midtrans menyediakan
verifikasi signature/status dan meminta penanganan notifikasi yang idempotent.
[Immutable ledger](https://docs.frappe.io/erpnext/immutable-ledger-in-erpnext),
[Midtrans notifications](https://docs.midtrans.com/reference/handle-notifications).

**Sintesis kita:** calon modul invoicing tidak otomatis menjadi accounting lengkap.
Status paid harus berasal dari sumber terverifikasi dan rekonsiliasi. Audit metadata
CRM yang sekarang ada juga tidak menggantikan ledger finansial. Pricing untuk
implementasi ARCHZXY dan harga barang/jasa milik klien mempunyai sumber berbeda.

### 5. Langganan dan marketplace menambah lifecycle

Stripe menjelaskan lifecycle subscription dengan invoice dan perubahan status;
usage-based billing mengukur pemakaian. Connect memodelkan beberapa pihak dan
perpindahan dana ke seller/provider.
[Subscriptions](https://docs.stripe.com/billing/subscriptions/overview),
[usage billing](https://docs.stripe.com/billing/subscriptions/usage-based),
[Connect](https://docs.stripe.com/connect).

**Sintesis kita:** recurring fee bukan sekadar invoice dengan cron. Perlu aturan
renewal/cancellation, penggunaan, dan hak akses. Marketplace membutuhkan ownership
seller, komisi, settlement, refund/dispute, dan rekonsiliasi. Referensi Stripe tidak
menetapkan provider untuk Indonesia atau ARCHZXY; pilihan tersebut belum dibuat.

### 6. Industri mempunyai data dan lifecycle khusus

Frappe Education menghubungkan student dengan program/course/academic term dan
menyediakan attendance, grades, serta fee history. HL7 FHIR memodelkan Patient
sebagai resource terkait pelayanan kesehatan. ERPNext Manufacturing memakai BOM,
work order, job card, dan workstation.
[Program enrollment](https://docs.frappe.io/education/program-enrollment),
[student portal](https://docs.frappe.io/education/student-portal),
[FHIR R5 Patient](https://hl7.org/fhir/R5/patient.html),
[manufacturing](https://docs.frappe.io/erpnext/manufacturing),
[BOM](https://docs.frappe.io/erpnext/bill-of-materials).

**Sintesis kita:** contact CRM dapat menjadi reference yang dihubungkan lewat
layanan berizin. Record student, patient, dan production order tetap memiliki
makna serta akses sendiri. Pengaturan label atau custom field tidak cukup untuk
semua perbedaan tersebut. Rujukan FHIR bukan sertifikasi compliance atau keputusan
untuk mengimplementasikan rekam medis.

### 7. API adapter tetap menjadi jalur reuse

Shopify membatasi operasi lewat API access scopes. Ini contoh bahwa integrasi
membutuhkan akses per resource, bukan akses bebas ke seluruh backend.
[Shopify access scopes](https://shopify.dev/docs/api/usage/access-scopes).

**Sintesis kita:** pilih satu pemilik data per resource. Jika klien sudah memiliki
ERP/commerce/pembukuan yang sesuai, hubungkan Archava dengan adapter berizin.
Module first-party digunakan untuk capability kecil yang dibutuhkan berulang.
Dual-write memerlukan desain rekonsiliasi tersendiri.

## Implikasi terhadap Archava yang sekarang

Di `1fad26a`, repo mempunyai core reuse yang terbukti, CRM contacts/activities/
follow-up persisten pada dua konteks, host dengan identitas/sesi, backup/restore,
dan adapter percakapan read/draft menggunakan parser lokal. Sales opportunity,
project management, booking persisten, inventory, billing, dan ERP vertikal belum
dibuktikan sebagai modul bisnis native yang siap dipakai.

Matriks baru mempertahankan status tersebut. Package source, module manifest,
commercial entitlement, koneksi provider, dan permission pengguna tetap dibedakan.
Demo booking atau quote configurator tidak menjadi bukti settlement pembayaran.
Lihat [status implementasi](../archive/audits/BUILD_STATUS_2026-10-29.md) dan
[hasil CRM conversation](../archive/audits/CRM_CONVERSATION_2026-10-08.md).

## Prioritas hasil riset

| Waktu                 | Pekerjaan                                                                                                                                           | Bukti yang dibutuhkan                                                                           |
| --------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| **MUST BUILD NOW**    | Pakai discovery/catalog ini; lanjutkan gate M3 pada slice yang sudah ada.                                                                           | Sumber data, identitas, permission, provider/recovery, telemetry, dan batas dukungan jelas.     |
| **SHOULD BUILD NEXT** | Validasi sales pipeline kecil; pilih satu workflow project delivery jika kebutuhan ARCHZXY/klien terkonfirmasi.                                     | Dua skenario konkret, ownership/state transitions, operasi dashboard/API, dan reuse tanpa fork. |
| **BUILD LATER**       | Invoicing + payment adapter, atau scheduling/catalog/orders sesuai klien pertama; perluas profile teruji.                                           | Provider yang dipilih, aturan domain, migrasi/export/restore, serta failure tests.              |
| **DO NOT BUILD YET**  | ERP universal, seluruh 25 kandidat sekaligus, clinical EHR, broad payroll/accounting, manufacturing/MRP penuh, atau marketplace settlement sendiri. | Kebutuhan, pemilik operasi, dan evaluasi backend khusus belum ada.                              |

Sales pipeline menjadi kandidat awal karena memperluas slice CRM yang nyata.
Project delivery relevan untuk ARCHZXY, tetapi tetap kandidat yang harus divalidasi.
Pilihan ini merupakan penilaian kesesuaian terhadap tim kecil dan repo saat ini;
riset vendor tidak menentukan urutan backlog secara otomatis. Workstream LLM live,
Act approval/execution/recovery, dan operasi produksi tetap memiliki gate masing-masing.

## Pertanyaan discovery sebelum memilih modul

1. Bagaimana bisnis memperoleh pendapatan, dan siapa pelanggan/aktor operasionalnya?
2. Apa tiga proses paling mahal atau paling sering gagal hari ini?
3. Sistem mana yang sekarang menjadi pemilik data, dan API apa yang tersedia?
4. Berapa lokasi, transaksi, resource, dan pengguna yang harus ditangani?
5. Apa aturan pembatalan, retur, revisi, payment, dan otorisasi yang wajib dipertahankan?
6. Apa yang boleh Archava baca, siapkan, dan jalankan dengan persetujuan siapa?
7. Apakah capability ini berulang di skenario kedua, dan siapa yang memeliharanya?

Riset ini menghasilkan peta untuk keputusan berikutnya. Riset tidak mengaktifkan
modul, mengubah kontrak klien, atau menaikkan klaim kesiapan komersial.
