# Archava — agentic AI for business

**Acuan kebutuhan produk aktif · revisi 6 November 2026.** Dokumen ini memperbarui
arah produk berdasarkan [riset ERP global](ERP_GLOBAL_BENCHMARK_2026-11-06.md).
Requirement dan target di sini **bukan pernyataan bahwa implementasi sudah lulus**.
Versi dokumen tidak mengubah versi aplikasi, harga, lisensi atau kontrak pelanggan.

## 1. Produk yang ingin dibangun

**Archava adalah AI untuk bisnis yang memahami kebutuhan pengguna, membantu
merencanakan pekerjaan, menjalankan tindakan sesuai izin, dan menunjukkan hasil
yang bisa diperiksa. Sistemnya dapat disusun dari modul bisnis yang kompatibel.**

Customer bisa bertanya dan melakukan transaksi yang didukung. Karyawan bisa
menuntaskan pekerjaan operasional. Owner bisa mengawasi hasil, menangani
pengecualian dan mengatur sistem dalam bahasa bisnis. Ketiganya mendapat data,
tindakan dan batas kewenangan yang berbeda.

Contoh target perjalanan: “Stok barang ini hampir habis. Siapkan pembelian yang
sesuai kebutuhan dan budget, minta persetujuan, lalu pantau sampai penerimaan dan
pembayarannya cocok.” Archava harus menjelaskan apa yang dapat dikerjakan,
meminta informasi yang kurang, menjaga approval dan melaporkan setiap hasil.
**Perjalanan lengkap contoh ini belum diterima pada kandidat sekarang.**

Modul bisnis tetap dapat digunakan lewat UI/API tanpa LLM. Ava memakai kontrak
bisnis yang sama; keputusan model tidak menggantikan aturan uang, stok, hak akses
atau bukti transaksi. CRM adalah satu domain dalam sistem, dengan beberapa
capability dan operasi; nama modul tidak menyatakan seluruh kedalaman CRM pasar.

Visi jangka panjang tetap lintas industri. Kit merupakan titik awal komposisi,
bukan batas permanen bisnis. Katalog industri adalah roadmap; hanya kombinasi
yang terimplementasi dan teruji yang ditawarkan sebagai capability tersedia.

## 2. Hubungan dengan dokumen lain

| Acuan | Tanggung jawab |
| --- | --- |
| Dokumen ini | Arah produk aktif, kebutuhan, prioritas dan ukuran keberhasilan |
| [Owner configuration PRD](OWNER_CONFIGURATION_PRD.md) | Rincian self-service, komposisi, preview, apply dan recovery; OWN-T01–OWN-T10 tetap diperlukan |
| [Full Rust PRD](PRD_FULL_RUST.md) | Requirement migrasi dan reuse; konteks versi serta penerimaan migrasi tetap bertanggal |
| [Riset global](ERP_GLOBAL_BENCHMARK_2026-11-06.md), [matriks 32 capability](ERP_CAPABILITY_MATRIX_2026-11-06.csv), [45 sumber primer](ERP_RESEARCH_SOURCES_2026-11-06.json) | Dasar perbandingan pasar dan gap pada snapshot 6 November |
| [Protokol B01–B07](ERP_PARITY_ACCEPTANCE_2026-11-06.md) | Skenario pembuktian yang diusulkan; belum merupakan hasil eksekusi |
| [Launch readiness](LAUNCH_READINESS.md) dan catatan pengujian bertanggal | Pekerjaan rilis berjalan, hasil aktual dan penerimaan yang masih terbuka |
| [Arsitektur](../architecture/README.md), [spesifikasi modul](../modules/README.md), source yang terpasang | Batas teknis, kontrak dan permukaan runtime aktual |
| [Master PRD historis](../archive/history/PRD.md) | Asal kebutuhan dan baseline komersial yang tetap berlaku jika tidak diubah secara eksplisit |

Dokumen ini menjadi pintu masuk kebutuhan produk sekarang. Kebutuhan lama yang
tidak diganti tetap berlaku; riwayat dan nomor requirement lama tidak dihapus.
Jika implementasi berbeda dari kebutuhan, perbedaan dicatat sebagai gap, bukan
diatasi dengan mengubah definisi “selesai”.

Presence **Chat / Voice / Human**, Capability **Assist / Act / Transact /
Enterprise**, lingkungan digital, integrasi dan consumption tetap dimensi
komersial yang terpisah. Avatar bukan syarat accounting, dan modul bukan satuan
tarif baru secara otomatis. PRD ini tidak mengubah pricebook atau menjanjikan
semua fitur termasuk dalam setiap paket. Website ARCHZXY tetap proyek terpisah.

## 3. Fakta awal dan batas penerimaan

Snapshot source aplikasi:
`c299a8e192bcdbbe68a270ea7ada189cd6a856790c6a4a6c8e940c7c16e2a8dd`.
Perubahan PRD ini tidak mengubah source tersebut atau menjalankan tes produk baru.

| Area | Bukti yang tersedia | Batas kesimpulan |
| --- | --- | --- |
| Permukaan bisnis | 18 domain terpasang, 141 operasi kanonis: 139 form native dan 2 operasi billing khusus provider | Jumlah ini tidak menyatakan 18 ERP lengkap atau accounting parity |
| Pengujian kandidat 5 November | 1.146 tes Rust, 49 pemeriksaan browser termasuk 11 pemeriksaan live-agent | Hasil berlaku pada kandidat, fixture, konfigurasi dan scope yang direkam |
| Ava dengan provider nyata | Enam planning turns Gemini melalui browser, approval, efek PostgreSQL dan receipt | Belum evaluasi luas atas tujuan bisnis, voice atau seluruh kombinasi domain |
| Integritas eksekusi | Typed operations, current authority, approval, replay dan recovery sudah mempunyai bukti lokal | Belum bukti semua provider, seluruh external delivery atau operasi produksi |
| Keuangan | Billing, settlement dan costing inventory mempunyai perilaku terpasang | GL operasional, AP penuh, bank reconciliation dan financial close belum tersedia sebagai satu proses terpasang |
| Enterprise dan industri | Sejumlah lifecycle domain tersedia secara terbatas | Payroll/MRP library, Organizations dan Attendance tidak membuktikan payroll, consolidation atau HR penuh |
| Performa | Commerce p95 terakhir 74,28 ms gabungan dan 54,89 ms terisolasi, tanpa request error | Target lokal 50 ms masih terbuka; belum perbandingan dengan pesaing |

Rincian hasil dan kegagalan provider ada pada
[penerimaan live Gemini](AGENT_LIVE_ACCEPTANCE_2026-11-05.md); pengukuran Commerce
ada pada [catatan penyelesaian agentic](AGENTIC_COMPLETION_2026-11-05.md).
Model, reasoning profile, quota dan latensi adalah bagian dari bukti; hasil satu
konfigurasi tidak otomatis berlaku pada model atau akun lain.

## 4. Apa arti “di atas rata-rata pasar”

Riset publik memberi arah dan menunjukkan gap. Ia belum memberi ukuran rata-rata
pasar atau peringkat produk berdasarkan penggunaan langsung. Klaim yang dapat
dibuktikan harus berbentuk: **untuk pelanggan, pekerjaan dan edisi pembanding
tertentu, Archava mencapai hasil yang sama benarnya dengan usaha yang lebih kecil**.
AI sudah dimiliki sejumlah pesaing; diferensiasi harus terlihat pada hasil kerja.

Urutan keberhasilan:

1. **Kelengkapan wajib:** seluruh proses yang ditawarkan untuk profil pelanggan
   dapat diselesaikan, dikoreksi dan direkonsiliasi. Kelebihan AI tidak menutupi
   kekurangan accounting atau workflow wajib.
2. **Keunggulan yang terukur:** tugas prioritas memerlukan waktu atau intervensi
   lebih sedikit daripada baseline manusia/form dan produk pembanding.
3. **Kelayakan penggunaan:** onboarding, biaya, konektivitas, pengoperasian dan
   bantuan pengguna sesuai kondisi pelanggan nyata.

### Scorecard pembuktian

Semua target berikut adalah **target usulan**, bukan hasil sekarang atau SLA.
Profil pelanggan, dataset, baseline dan batas biaya ditetapkan sebelum eksekusi.

| Ukuran | Target atau cara menetapkan batas | Bukti |
| --- | --- | --- |
| Proses wajib | Semua skenario wajib profil lulus, termasuk koreksi dan kegagalan; tidak ada selisih uang/stok yang tidak dijelaskan | B01 dan benchmark industri yang relevan |
| Keselamatan | Nol kejadian akses/aksi tanpa izin, salah target kritis, duplikasi efek bisnis atau klaim sukses palsu dalam suite penerimaan | B05/B06; satu kejadian menggagalkan kandidat untuk scope itu |
| Keberhasilan tujuan Ava | Persentase tujuan didukung yang mencapai expected state; threshold per profil ditetapkan dari baseline dan risiko pekerjaan | B05, suite terpisah dari tuning, semua percobaan dihitung |
| Usaha pengguna | Sasaran awal penurunan median waktu kerja aktif sedikitnya 30% pada tugas prioritas terhadap masing-masing baseline terpilih | B07; review, koreksi dan recovery ikut dihitung; bukan syarat universal semua tugas |
| Biaya dan kecepatan | Biaya per tugas benar serta p50/p95 end-to-end memenuhi budget/SLO profil; penurunan waktu tidak boleh dibeli dengan error atau biaya tak disepakati | B05/B07; biaya provider, retry, infra dan tenaga review dilaporkan |
| Owner self-service | Journey konfigurasi didukung selesai tanpa mengedit source/config file atau meminta engineer; blocked requests dijelaskan dengan benar | OWN-T01–OWN-T10 dan uji pengguna |
| Penggunaan nyata | Satu siklus bisnis dan periode pelaporan profil diselesaikan; owner/finance/operator dapat merekonsiliasi hasil | Pilot; bukan pengganti tes regresi atau bukti seluruh tahun operasi |

Waktu kerja aktif mencakup input, review dan perbaikan manusia; elapsed time
mencakup menunggu model, sistem dan provider. Laporkan keduanya. Biaya per tugas
benar membagi **seluruh biaya percobaan termasuk kegagalan** dengan jumlah tugas
yang selesai benar. Laporkan denominator, variasi dan confidence interval yang
sesuai; sampel kecil tidak menjadi klaim reliability produksi.

Pilih edisi, add-on, negara dan konfigurasi pesaing yang benar. Bandingkan hasil
akhir bisnis yang sama, meskipun cara interaksinya berbeda. Laporkan pula tugas
yang lebih lambat atau kalah; jangan membuat skor gabungan yang menyembunyikan
kegagalan kritis. Perbandingan rata-rata pasar hanya boleh dibuat bila populasi,
sampling dan metodenya benar-benar tersedia.

## 5. Profil rilis dan keputusan yang belum ditetapkan

**Penegasan owner, 8 November 2026:** target Archava adalah siap untuk berbagai
model bisnis, termasuk kombinasi/hybrid; owner belum memilih satu calon customer
dan tidak meminta target dibatasi ke satu jenis usaha. Implementasi dan simulasi
lokal dilanjutkan terhadap cakupan model bisnis dalam PRD/katalog, tanpa menunggu
pilihan customer pertama. Profil di bawah adalah unit konfigurasi dan pembuktian
per model, bukan pembatas target platform. Setiap model tetap membutuhkan alur,
invariant, persona dan acceptance sendiri; dukungan satu profil tidak menerima
profil lain secara otomatis. Negara, aturan dan konektor riil ditetapkan eksplisit
ketika konfigurasi/pemakaian riil terkait dibuat.

Satu profil rilis mendefinisikan jenis pelanggan, persona, negara, legal entity,
currency, proses wajib, volume data, concurrency, integrasi, SLO dan lingkungan.
Requirement P0 adalah wajib untuk profil yang menawarkan proses terkait. P1
menambah kedalaman atau keunggulan setelah fondasinya benar; P2 bersyarat pada
segmen. Kebutuhan pelanggan dapat menaikkan P1/P2 menjadi wajib sebelum dijual.

**Profil benchmark awal yang disarankan untuk discovery:** perdagangan/distribusi
dengan barang dan purchasing, serta jasa/proyek dengan scheduling dan billing.
Ini hipotesis untuk memanfaatkan fondasi yang ada, belum keputusan owner tentang
ICP, negara peluncuran atau batas bisnis jangka panjang.

| Keputusan | Pilihan dan aturan kerja sementara | Kapan harus diputuskan |
| --- | --- | --- |
| Cakupan model bisnis dan proses | Lanjutkan reusable core, cakupan katalog dan kombinasi model yang tervalidasi. Perdagangan/jasa menjadi dua fixture awal, lalu acceptance diperluas per model; tidak menunggu owner memilih customer pertama atau menganggap semua domain wajib pada tiap perusahaan | Setiap klaim dukungan model memerlukan alur wajib dan benchmark sendiri |
| Sumber kebenaran accounting | GL native; adapter ke ERP/accounting existing; atau operasi native dengan accounting eksternal. Satu sumber posting resmi per entity/book; tidak ada dua ledger yang menulis transaksi yang sama secara independen | Sebelum desain FIN dan migrasi saldo |
| Negara, pajak dan mata uang | Tetapkan jurisdiction, tanggal berlaku, currency dan aturan yang memang dibutuhkan; formula library belum berarti conformance | Sebelum transaksi riil dan klaim kepatuhan |
| Integrasi prioritas | Pilih ERP/payment/bank/channel yang digunakan pelanggan, berikut API, sandbox, lisensi dan biaya | Sebelum menjanjikan konektor live |
| Profil operasional dan paket | Tetapkan kapasitas, support, retensi, data processing, recovery dan biaya berdasarkan kebutuhan terukur | Sebelum kontrak/SLA dan pilot produksi |

Pekerjaan inventaris capability, perbaikan kontrak, dataset sintetis dan spesifikasi
benchmark dapat berjalan sambil keputusan ini diselesaikan. Dokumen ini tidak
memutuskan arsitektur accounting atau paket komersial secara diam-diam.

## 6. Requirement fondasi data dan proses bisnis

Status dasar setiap requirement mengikuti matriks riset dan bukti di bagian 3.
Tabel berikut menetapkan perilaku target; **tidak ada baris yang otomatis accepted**.

| ID | Prioritas | Perilaku dan acceptance minimum | Pemetaan |
| --- | --- | --- | --- |
| DAT-01 | P0 | Customer, supplier, item, entity, dokumen dan external reference memiliki identitas stabil serta hubungan yang tervalidasi. Dari order dapat ditelusuri fulfillment, invoice, payment dan accounting; nama bebas tidak menjadi satu-satunya penghubung | F03/F08/F11/F12; B01/B05 |
| DAT-02 | P0 | Uang, quantity, currency, rounding, waktu dan revisi memakai aturan eksplisit. Transisi menjaga audit, idempotency dan concurrency; koreksi tidak menghapus histori atau mengubah original cost secara diam-diam | F01/F08/F26; B01/B05 |
| FIN-01 | P0 untuk proses keuangan | Ledger resmi persisten menerima posting seimbang, source document, periode, pembalikan dan referensi. Jika eksternal, mapping dan status sinkronisasinya dapat direkonsiliasi; library GL saja tidak lulus | F01; B01 |
| FIN-02 | P0 untuk purchasing lengkap | Requisition/PO sesuai scope, partial/rejected receipt, vendor bill, toleransi three-way match, AP dan supplier payment terhubung; mismatch ditahan untuk review sesuai policy | F02/F11; B01 |
| FIN-03 | P0 untuk penjualan lengkap | Order, fulfillment, invoice/credit note, AR, terms/aging, payment/refund dan inventory cost terhubung. Retry atau webhook berulang tidak menggandakan revenue, stok atau pembayaran | F03/F08; B01 |
| FIN-04 | P0 untuk keuangan | Bank/cash records, statement import, fee, settlement, matching dan exception dapat direkonsiliasi terhadap saldo serta ledger resmi. Provider payment status saja tidak memenuhi requirement | F04; B01 |
| FIN-05 | P0 untuk penggantian ERP keuangan | Trial balance, laba rugi, neraca, arus kas, ledger detail, subledger dan period close cocok ke sumber; closed period menolak perubahan tidak sah dan membuka kembali melalui policy berizin | F01/F23; B01 diperluas sesuai laporan profil |
| FIN-06 | P0/P1 sesuai profil | Tax, localization, opening balances dan effective dating mengikuti aturan scope yang ditelaah pihak kompeten. Export/filing serta integrasi resmi diuji bila dijanjikan; komentar PSAK/IFRS tidak menjadi sertifikasi | F07/F29; B01/B06 |

Baseline accounting B01 sengaja sintetis dan bebas pajak. Kelulusannya merupakan
fondasi, bukan sertifikasi akuntansi atau pajak semua negara. Bila Archava hanya
melayani proses non-keuangan terbatas, ketiadaan FIN harus terlihat dalam scope;
produk tidak dipasarkan sebagai pengganti accounting penuh.

## 7. Requirement Ava dan penyelesaian pekerjaan

| ID | Prioritas | Perilaku dan acceptance minimum | Pemetaan |
| --- | --- | --- | --- |
| AGT-01 | P0 untuk klaim agentic | Tujuan natural-language menjadi rencana typed yang didukung, atau klarifikasi/gap yang jujur. Follow-up mempertahankan tujuan dan preferensi yang relevan; dependency memakai hasil terverifikasi. Perubahan scope setelah partial completion menampilkan efek yang sudah terjadi | F24/F25; B05 |
| AGT-02 | P0 | Preview menampilkan target, kuantitas, uang, dependency dan dampak penting. Approval terikat rencana/revisi; authority dicek kembali saat eksekusi. Stale plan, revoke, salah entity dan semantic mismatch tidak menghasilkan write | F26; B05/B06 |
| AGT-03 | P0 | Status membedakan proposed, awaiting approval, running, partial, committed, reconciled, failed dan cancelled sesuai kontrak. Laporan sukses membutuhkan receipt/effect nyata. Retry/restart mempertahankan correlation; cancel tidak berpura-pura membatalkan efek yang sudah commit | F25/F26/F27; B01/B05/B06 |
| AGT-04 | P0 untuk jawaban analitis | Knowledge dan data bisnis mempunyai sumber, timestamp, cakupan dan izin. Query/aggregate mencakup filter yang diminta atau mengungkap batasnya; data di luar halaman pertama tetap ditemukan. Catatan/dokumen tidak menjadi instruksi yang melampaui authority | F23/F25/F31; B05 |
| AGT-05 | P1 | Pemantauan stok, tugas jatuh tempo, invoice atau exception dapat menghasilkan rekomendasi proaktif dalam scope terkonfigurasi. Trigger, deduplication, budget, notifikasi dan berhenti/resume dapat diaudit; tidak ada autonomous external spending tanpa policy/authority yang diperlukan | F24/F26/F27; B05/B06 |
| AGT-06 | P0 untuk jalur AI yang dijual | Model/profile, budget, quota, timeout dan retry eksplisit. Provider down, output rusak atau referensi salah menghasilkan recovery/klarifikasi yang aman dan manual fallback. Model diganti hanya melalui evaluasi konfigurasi baru; semua kegagalan masuk laporan biaya dan availability | F24/F32; B05/B07 |

Evaluasi awal B05 yang diusulkan: sedikitnya 50 tujuan didukung yang dipisahkan
dari tuning, 30 kasus negatif/unsupported/authority dan 20 titik kegagalan/recovery
pada profil yang dipilih. Satu kasus dapat memiliki beberapa variasi; laporkan
jumlah skenario unik dan jumlah percobaan secara terpisah. Ulangi tujuan didukung
setidaknya tiga kali untuk melihat variasi model; jangan hanya menyimpan run
terbaik. Angka ini ukuran awal dataset, bukan bukti statistical reliability.

Tujuan didukung dinilai dari expected state independen, bukan narasi agen.
Unsupported goal yang ditolak dengan benar dinilai pada suite batas kemampuan,
bukan dihitung sebagai tujuan bisnis yang berhasil dituntaskan. Suite wajib
mencakup Bahasa Indonesia/Inggris, lintas domain, record bernama sama, konteks
di luar halaman awal, stale data, revoke dan prompt injection melalui data.

## 8. Requirement owner, pengalaman pengguna dan reuse

| ID | Prioritas | Perilaku dan acceptance minimum | Pemetaan |
| --- | --- | --- | --- |
| CFG-01 | P0 untuk self-service | Owner menjelaskan bisnis dan mendapatkan susunan modul/options yang memang tersedia dan kompatibel. Preview memperlihatkan dependency, izin, biaya/readiness dan gap; extension yang belum tersedia tidak ditampilkan seolah sudah aktif | F28; OWN-T02/T03/T05/T09 |
| CFG-02 | P0 untuk self-service | Apply memakai current management grant dan versi konfigurasi. Dua tab, retry dan restart memberi satu perubahan yang sah. Disable memperbarui UI/API/tools sekaligus dan menjaga data/history; restore konfigurasi tidak memundurkan transaksi keuangan | F26/F28; OWN-T04/T06/T07/T08/T10 |
| CFG-03 | P0 untuk klaim reusable | Dua consumer/business model memakai kontrak dan rilis core/module yang sama tanpa source fork. Kombinasi mempunyai manifest, compatibility, permission dan upgrade evidence; keberadaan crate tidak cukup | F28; OWN-T01 dan bukti integration consumer |
| UX-01 | P1; wajib sebelum pilot | Owner/operator baru dapat menyiapkan bisnis, mengimpor data uji dan menyelesaikan tugas pertama lewat flow yang dimengerti. Ukur waktu onboarding, hambatan dan kebutuhan bantuan terhadap profil pengguna, termasuk migrasi existing system | F28/F29; B06/B07 dan uji pengguna |
| UX-02 | P0 untuk jalur yang dijual | UI/API menyediakan operasi manual yang setara ketika AI tidak tersedia. Status, error, approval dan recovery memakai bahasa bisnis, mendukung keyboard dan viewport sasaran; bukti dapat dibuka tanpa membaca log atau raw JSON | F24/F32; B05/B06 dan browser QA |
| UX-03 | P0 untuk Presence yang dijual | Customer/employee/owner dan embed/channel memakai identity serta data scope yang benar. Chat, live voice, interruption dan avatar diterima terpisah untuk paket yang menawarkan masing-masing mode; transcript saja tidak membuktikan voice quality | F24/F27; B05/B06 dan penerimaan media nyata |

Persona uji meliputi owner non-teknis, operator harian, finance/approver,
customer dan administrator integrasi. Masing-masing mempunyai izin serta
expected outcome berbeda. UI tidak boleh mengharuskan pengguna mengetahui
nomor modul, state machine internal atau arsitektur agar dapat mengambil keputusan.

## 9. Requirement integrasi dan pengoperasian

| ID | Prioritas | Perilaku dan acceptance minimum | Pemetaan |
| --- | --- | --- | --- |
| INT-01 | P0 untuk integrasi yang dijual | Connector memakai kontrak berversi, scope/secret lifecycle dan signature/correlation yang benar. External effect memiliki outbox, retry dan reconciliation; webhook berulang/terlambat, response hilang dan partial failure teruji. Tidak mengklaim exactly-once universal untuk jaringan eksternal | F27; B01/B06 |
| INT-02 | P0 sebelum migrasi data pelanggan | Import memvalidasi mapping, duplicate identity, opening balance dan control totals. Dry run, rollback/kompensasi yang sesuai serta rekonsiliasi source-to-target tersedia; data production tidak dipakai sebagai fixture uji | F29; B01/B06 |
| SEC-01 | P0; kedalaman sesuai profil | Actor, tenant, entity, role, service account, independent approver dan revoke ditegakkan server. MFA/SSO/provisioning/offboarding wajib bila profil mensyaratkan; RLS lokal tidak dihitung sebagai bukti seluruh IAM enterprise | F26/F30; B06 |
| SEC-02 | P0 | Owner memahami data apa yang dikirim ke provider. Knowledge/context field, retention, export/delete, audit dan residency mengikuti policy profil; secret tidak masuk prompt/log. Self-host backend tidak berarti inference selalu lokal; pilihan tanpa provider diuji bila ditawarkan | F25/F31; B05/B06 |
| OPS-01 | P0 sebelum pilot produksi | Backup terenkripsi, restore ke host bersih, upgrade/migration, audit/alert dan runbook diuji. Tetapkan serta ukur RTO/RPO; single-writer fencing bukan bukti HA, dan restore lokal bukan bukti disaster recovery produksi | F30/F32; B06/B07 |
| OPS-02 | P0 sebelum klaim kapasitas/SLA | Sizing, workload, concurrency, data volume dan konfigurasi dikunci. Laporkan p50/p95, errors, saturation, recovery dan biaya; pertahankan target Commerce 50 ms yang masih terbuka sampai ada perubahan requirement beralasan dan tercatat | F32; B07 |

Public deployment mengikuti jadwal owner setelah website siap. Pekerjaan
requirement, implementasi, browser lokal, provider sandbox/live yang diotorisasi
dan benchmark sintetis tidak harus menunggu website. Domain tersedia saja tidak
membuktikan hosting aplikasi, recovery, SLA atau kesiapan produksi.

## 10. Kedalaman industri mengikuti profil, bukan nama modul

**EXT-01 — P1/P2, menjadi P0 bila dijanjikan kepada pelanggan:** setiap extension
memiliki lifecycle lengkap, koreksi, role negatif, kaitan finance dan bukti pengguna
untuk scope yang ditawarkan. Berikut peta kebutuhan, bukan daftar fitur accepted.

| Kelompok | Kedalaman yang perlu dipilih dari discovery | Bukti |
| --- | --- | --- |
| Commerce/warehouse | Lokasi/bin, UOM, transfer, lot/serial/expiry, stocktake dan inventory-to-finance sesuai operasi pelanggan | F08/F09; B01/B02 |
| Manufacturing | BOM/routing, capacity, WIP, scrap, quality, subcontracting dan biaya produksi | F10; B03 |
| Multi-company | Legal-entity books, functional currency, FX, intercompany, elimination dan consolidation | F05/F06; B04 |
| CRM/projects/field service | Customer master, forecast, budget/time/cost, contract billing, parts, signoff dan margin | F12/F13/F17; B01/B05 |
| People/assets/subscriptions | Employee lifecycle/payroll, depreciation/disposal atau proration/revenue schedule sesuai produk yang dijual | F14/F15/F16/F18; B01/B06 |
| Hospitality/foodservice | Booking/channel, night audit, POS/cash close, costing dan finance sesuai kebutuhan outlet/properti | F19/F20; B01/B05 |
| Scheduling/education dan domain lain | Capacity/recurrence, learner/session/results, tuition atau workflow khusus yang dibutuhkan; integrasi sistem existing bila lebih tepat | F21/F22/F27; B01/B05/B06 |

Tidak perlu membangun seluruh kelompok sebelum memberi manfaat pada satu profil.
Sebaliknya, sektor yang diiklankan sebagai siap harus memenuhi kebutuhan wajibnya;
adapter/partner hanya dihitung bila integrasi dan tanggung jawab operasinya terbukti.

## 11. Urutan kerja dan syarat keluar

Ini urutan produk berbasis dependency, bukan estimasi kalender atau klaim seluruh
fase telah dipesan pelanggan. Jalur yang independen dapat berjalan bersamaan.

| Fase | Hasil konkret | Syarat keluar dan traceability |
| --- | --- | --- |
| R0 — profil dan kebenaran capability | Release-profile terisi; peta implemented/gap; dataset pelanggan sintetis; keputusan ledger dan integrasi prioritas | Setiap janji profil punya requirement, owner peran dan oracle; DAT-01/02, F01–F32 dan keputusan bagian 5 tercatat |
| R1 — satu rantai bisnis lengkap | Order-to-cash dan source-to-pay yang terpilih mencapai ledger resmi, bank dan close, termasuk correction/retry | DAT/FIN/INT-02 lulus B01 untuk profil; tidak ada selisih yang tidak dijelaskan |
| R2 — Ava menyelesaikan pekerjaan | Konteks lengkap, follow-up, recovery, exception dan manual fallback pada proses yang benar | AGT/UX-02/03 lulus B05, target scorecard profil terukur, live-provider evidence disimpan |
| R3 — owner dan kedalaman sektor | Komposisi self-service, onboarding dan extension prioritas; core yang sama digunakan ulang | CFG/UX-01/EXT-01, OWN-T01–T10 dan B02/B03/B04 sesuai janji profil |
| R4 — operasi dan pembanding langsung | Connector nyata, IAM, migrasi, restore, capacity/TCO dan comparative task report | INT/SEC/OPS lulus B06/B07; target performa terbuka diselesaikan atau perubahan dibuktikan dan dicatat |
| R5 — pilot dan klaim terbatas | Pelanggan sasaran menuntaskan satu siklus bisnis dan periode pelaporan, support tersedia | Semua P0 profil accepted; tidak ada blocker kritis; hasil pengguna dan biaya lengkap; klaim hanya sebesar bukti |

Authority, privacy dan aturan uang berlaku dari awal, tidak menunggu fase R4.
Jika scope pertama hanya lapisan agen pada ERP existing, R1 membuktikan proses
di ERP resmi tersebut; bukan alasan membangun ledger kedua.

## 12. Riset lanjutan yang benar-benar diperlukan

Riset luas 18 keluarga produk dan 45 sumber sudah cukup untuk mengubah arah PRD.
Pekerjaan berikut adalah menutup ketidakpastian keputusan dan menguji hipotesis,
bukan mengumpulkan lebih banyak nama ERP. Aktivitas berikut **belum dijalankan**
oleh pembaruan dokumen ini.

| Pertanyaan | Aktivitas dan deliverable | Keputusan yang dibantu |
| --- | --- | --- |
| Pekerjaan mana yang paling bernilai? | Usulan awal 5–8 wawancara/walkthrough bisnis pada calon profil; sertakan owner, operator dan finance. Kumpulkan alur, frekuensi, exception, dokumen anonim, waktu/biaya sekarang serta kebutuhan pindah sistem. Ini discovery kualitatif, bukan statistik pasar | ICP, must-have, willingness to pay dan shortlist tugas |
| Accounting native atau integrasi? | Petakan satu O2C, satu source-to-pay dan satu close; telaah policy, API/sandbox, rekonsiliasi, migration cost serta tanggung jawab bersama praktisi yang relevan | Pilihan FIN dan batas integrasi/localization |
| Apakah Archava benar-benar lebih mudah? | Jalankan tugas yang sama pada manual/form Archava dan 2–3 produk/edisi yang dibeli segmen; rekam waktu, bantuan, error dan biaya. Pilihan Odoo/ERPNext atau software accounting lokal bergantung ICP, bukan ranking generik | Target 30%, scope parity dan alasan pembelian |
| Apakah layak dipakai berulang? | Pilot dengan data dan izin yang disepakati; lihat onboarding, penggunaan, exception, close dan support selama siklus profil | Kemampuan mempertahankan pelanggan, TCO dan kesiapan penawaran |

Periksa ulang versi produk, paket, harga, API, quota dan aturan yang mudah berubah
saat dipakai untuk keputusan. Dokumentasi vendor memberi bukti capability yang
didokumentasikan, bukan hasil usability, reliability atau TCO langsung.

## 13. Aturan penerimaan dan perubahan

Setiap requirement profil mempunyai status terpisah: **planned → implemented →
verified local → verified live integration bila relevan → verified pilot**.
Tidak semua requirement memerlukan provider, tetapi tidak boleh melewati
penerimaan provider/pilot ketika klaim bergantung padanya. Priority bukan status.

Record penerimaan memuat requirement dan benchmark, source/config/model identity,
expected outcome independen, tanggal, input sintetis/diotorisasi, seluruh hasil,
failure, artifact serta batas kesimpulan. Hasil baru menambah catatan bertanggal;
hasil gagal lama tidak ditulis ulang. Launch tracker menautkan bukti terbaru
tanpa menjadikan kelulusan fase lama sebagai bukti requirement baru.

Perubahan scope, target atau policy harus menyebut alasan dan dampaknya pada
pelanggan serta acceptance. Implementasi fitur baru membutuhkan kontrak, migrasi
dan verifikasi yang sesuai repository; PRD tidak mengotorisasi deployment,
pengiriman pesan, pemindahan data pelanggan atau transaksi eksternal otomatis.

Satu profil boleh dinyatakan siap hanya setelah seluruh requirement wajibnya
diterima. Pernyataan “lebih unggul” membutuhkan laporan perbandingan langsung
untuk tugas dan edisi yang disebutkan. Roadmap lintas industri tetap berkembang
melalui kebutuhan nyata, kombinasi teruji dan bukti manfaat yang dapat diperiksa.
