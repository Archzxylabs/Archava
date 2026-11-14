# Audit pengalaman agentic Archava — 5 November 2026

**Verdict: fondasi eksekusi bisnis terverifikasi; MVP agentic AI for business belum diterima.**

Positioning produk yang ditegaskan owner adalah **agentic AI for business**.
Acceptance harus membuktikan perjalanan customer: menyampaikan tujuan → agen
memahami konteks → merencanakan pekerjaan → memperoleh approval sesuai policy →
mengeksekusi → memeriksa hasil → melaporkan dan melanjutkan pekerjaan.
Kelulusan workspace/form/API tidak dengan sendirinya membuktikan perjalanan ini.

Audit ini menjalankan **14 probe agentic: 6 terpenuhi, 8 menunjukkan gap**,
dengan tiga area tambahan belum terverifikasi. Angka tersebut bukan persentase
kesiapan atau delapan bug unik. Runner baseline menyelesaikan 38 checks, 0 page
error dan cleanup fixture 0; `agentic.agenticReady` adalah **false**. Status runner
`passed` berarti diagnostik selesai, bukan penerimaan produk agentic.

## Reproduksi utama

Di Chromium, customer memasukkan:

> Ava, urus kebutuhan stok minggu depan. Periksa stok dan pemasok, buat rencana pembelian, lalu minta persetujuan saya sebelum menjalankannya.

Server menjawab 200, tetapi `data.kind=help`, `source=none`,
`planner.providerId=archava-crm-local` dan `model=deterministic-commands-v1`.
UI mengarahkan pengguna ke contoh perintah CRM. Permintaan tentang invoice jatuh
tempo juga mendapat bantuan yang sama. SQL sebelum/sesudah membuktikan jumlah
misi tetap satu, PO tetap satu, command CRM tetap tujuh, serta memory nodes dan
episodes tetap nol. Tidak ada klaim bahwa tujuan tersebut dieksekusi.

Sebagai pembanding, harness menyusun misi dua langkah melalui API yang didukung.
Keduanya membuat produk dengan SKU synthetic yang unik. Sebelum approval,
eksekusi memberi 409 dengan nol record. Langkah kedua sebelum prerequisite
selesai juga memberi 409. Langkah pertama setelah approval membuat satu record;
replay tetap satu. Mencabut `catalog.write` menolak langkah kedua dengan 403.
Setelah grant asli dipulihkan, eksekusi eksplisit menyelesaikan misi ke
`touchdown`, dengan dua record dan receipt terverifikasi untuk setiap langkah.
Pertanyaan kepada chat tentang hasil misi itu tetap mendapat `help/source=none`.

## Temuan

| ID / prioritas | Bukti dan dampak | Penerimaan yang masih dibutuhkan |
| --- | --- | --- |
| A01 · P1 · Goal planning dan konteks bisnis belum menyatu | AG-03/04: tujuan stok dan pertanyaan Billing mendapat CRM help. Host mengikat `LocalCrmPlanner`; tidak ada model planner lintas domain pada chat utama. | Goal menghasilkan klarifikasi atau rencana yang memakai data bisnis terotorisasi, dengan scope, batas dan tindakan yang dapat ditinjau. |
| A02 · P1 · Conversation belum membentuk pekerjaan durable | AG-06/07: tidak ada misi baru setelah goal; UI hanya menampilkan bantuan CRM. Misi yang berhasil berasal dari langkah eksplisit yang disediakan harness. | Dari percakapan, customer dapat meninjau lalu mengaktifkan misi yang terikat tujuan, rencana, actor, tenant dan versi approval. |
| A03 · P1 · Memori percakapan belum terhubung | AG-05: menyampaikan lalu menanyakan kembali preferensi pembelian mendapat help; tidak ada node/episode yang tercatat. Input conversation tidak memuat riwayat/session; host tidak memakai Orbz pada alur ini. | Preferensi/context relevan dapat diingat lintas turn secara scoped dan bersumber. Teks memori tidak boleh memberi grant atau mengubah policy tanpa jalur approval yang sah. |
| A04 · P1 · Kelanjutan misi masih memerlukan pemanggilan eksplisit | AG-11: setelah kedua langkah disetujui dan langkah pertama dijalankan, langkah kedua masih pending pada observasi 1,5 detik. Source serving yang diperiksa hanya menjalankan satu `stepIndex` per `run-next`; tidak ditemukan loop worker yang menghubungkannya ke goal. | Executor melanjutkan langkah eligible sesuai autonomy policy; claim/lease, pause/cancel, current grants dan recovery setelah restart harus diuji. Observasi pendek ini bukan pengujian SLA scheduler. |
| A05 · P1 · Laporan hasil belum kembali ke percakapan | AG-14: misi sudah touchdown dengan dua record nyata, tetapi chat tidak dapat membaca dan melaporkan hasilnya. | Agen melaporkan hasil dan kegagalan dari ledger/receipt asli, mengaitkannya ke tujuan, serta tidak mengakui tindakan dari teks model semata. |
| A06 · P2 · Pintu masuk belum mengutamakan agen | AG-01: Home menampilkan directory CRM dan tidak mempunyai input goal; Assistant adalah tautan terpisah. | Pengalaman utama memperlihatkan tujuan, rencana, approval yang menunggu, progres agen dan hasil. Modul operasional tetap menjadi tempat inspeksi/kontrol. |

## Fondasi yang terbukti

| Probe | Hasil |
| --- | --- |
| AG-02 | Perintah overview CRM memakai SQL nyata; jumlah yang dilaporkan sama dengan database. |
| AG-08 | Approval menahan write dengan nol efek sebelum disetujui. |
| AG-09 | Dependency menahan langkah yang belum eligible. |
| AG-10 | Tool executor mencatat receipt terverifikasi dan replay tanpa produk ganda. |
| AG-12 | Approval yang sudah diberikan tidak melewati domain grant yang kemudian dicabut. |
| AG-13 | Dua langkah yang disusun eksplisit dapat selesai dengan dua efek SQL nyata. |

[Bukti operasional sebelumnya](FEATURE_COMPLETION_2026-11-05.md)—1.124 tes Rust,
24 helper, 42 browser/operasional checks dan 75 probe operasional—tetap valid untuk
cakupannya. Katalog 18 domain, 139 tool manusia, Owner configuration, proactive
reorder, atomic import, memory storage serta media transport merupakan fondasi.
Keberadaannya tidak menjadi bukti bahwa seluruhnya dipakai bersama oleh satu agen.

## Belum terverifikasi

- Kualitas reasoning/planning model live: model planner belum dipasang ke chat
  utama; mengisi credential saja tidak menghubungkan alur tersebut.
- Voice-to-business execution: transport audio/video telah diuji sebelumnya;
  source Live menyediakan percakapan dan mengarahkan mutasi ke native review,
  tanpa deklarasi business tools pada setup yang diperiksa.
- Replanning otomatis dan pengaliran output suatu langkah menjadi input langkah
  berikutnya belum mempunyai runtime proof di audit ini. Tidak ada syntax
  placeholder hipotetis yang dikirim sebagai seolah-olah kontrak API resmi.

## Source dan evidence

Serving source SHA-256:
`70d46de105d563b25d32b028f00801d759f04a74dce8e8e57c76c2d10134aba3`.
Audit tidak mengubah implementasi serving; hanya menambah harness dan dokumentasi.
Build/candidate yang sama dengan penerimaan operasional sebelumnya dipakai ulang.

- [Diagnostic harness](../../tools/e2e/agentic.mjs), dijalankan melalui
  `ARCHAVA_E2E_AGENTIC=1 node tools/e2e/verify.mjs --output <new-directory>`.
- Runtime evidence: `target/e2e/agentic-audit-20261105-01/result.json` dan
  `agentic-observations.json`; traces merekam input, status, body, snapshot SQL,
  mission state serta hash harness. Screenshot: `agentic-home.png` dan
  `agentic-customer-goal.png`. Log: `target/agentic-audit-20261105-01.log`.
- [Chat composition](../../crates/host/src/assistant/mod.rs),
  [deterministic planner](../../crates/assistant/src/planner.rs),
  [conversation contracts](../../crates/assistant/src/contracts.rs),
  [native desk](../../crates/slice/src/desk.rs).
- [Single-step execution](../../crates/host/src/cockpit.rs),
  [finite business dispatch](../../crates/host/src/business.rs),
  [Live model setup](../../crates/providers/src/voice/live.rs).

Seluruh data synthetic berada pada PostgreSQL dan browser profile milik gate.
Tidak ada akun provider asli, credential customer, database bersama atau public
deployment yang digunakan. Semua proses yang dimulai gate sudah ditutup.

## Urutan penutupan

1. Tetapkan satu vertical slice tujuan bisnis yang lengkap sebagai kontrak
   penerimaan—contohnya replenishment stok—dengan klarifikasi, rencana, approval,
   eksekusi dan laporan yang terhubung pada goal yang sama.
2. Hubungkan conversation planner ke registry read/action dan konteks bisnis,
   lalu ke mission executor yang sudah ada. Otorisasi tetap berada di domain.
3. Tambahkan state percakapan/memory scoped dan continuation/recovery worker;
   uji pause, perubahan rencana, revocation dan restart pada perjalanan yang sama.
4. Hubungkan pelaporan berbasis receipt dan tampilan progres agen ke pintu masuk
   utama. Uji variasi bahasa customer, ambiguitas, kegagalan parsial dan perubahan
   kondisi; lakukan acceptance model/provider live ketika environment tersedia.

Tidak ada perbaikan fitur agentic yang diklaim telah dibuat dalam audit ini.

## Pengujian lanjutan seluruh simulasi

[Run gabungan](SIMULATION_REPORT_2026-11-05.md) memperluas goal ke 18 domain dalam
dua bahasa, enam tujuan lintas domain dan delapan kasus percakapan. Dua permintaan
Scheduling malah diperlakukan sebagai pencarian kontak. Ini tambahan bukti
misclassification untuk temuan planner; enam fondasi dan delapan gap di audit
awal tetap merupakan hasil probe awal, bukan hasil yang ditimpa.
