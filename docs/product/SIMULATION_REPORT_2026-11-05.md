# Simulasi menyeluruh Archava — 5 November 2026

**Verdict: fungsi operasional dan batas eksekusi yang diuji bekerja; pengalaman
agentic AI for business belum diterima.** Ini pengujian lokal synthetic, bukan
penerimaan provider asli atau public deployment.

Pengujian memakai serving source yang sama:
`70d46de105d563b25d32b028f00801d759f04a74dce8e8e57c76c2d10134aba3`.
Implementasi serving tidak diubah. Harness simulasi ditambah agar bahasa customer,
perubahan tujuan, keselamatan dan lifecycle dapat diuji bersama pada kandidat
native `dist/native-candidate-20261105-feature-complete-02`.

## Hasil simulasi aplikasi

| Kelompok | Hasil | Batas bukti |
| --- | --- | --- |
| Browser/operasional | 42 checks selesai, 0 page error, cleanup fixture 0 | Runner selesai tidak berarti seluruh target latency atau agentic acceptance lulus. |
| Requirement operasional | 75 terpenuhi, 0 gap | Mencakup workspace, form, lifecycle, Owner, import, Feed dan recovery; bukan seluruh kombinasi state semua operasi. |
| Diagnostik agentic | 6 terpenuhi, 8 gap | Membuktikan perbedaan executor yang bekerja dan percakapan yang belum mengendalikan pekerjaan. |
| Bahasa customer | 36 tujuan: 18 domain × Indonesia/Inggris | 34 mendapat bantuan CRM, 2 salah diarahkan ke pencarian kontak; 0 tujuan terpenuhi. |
| Tujuan lintas domain | 6 skenario | Semua mendapat bantuan CRM. |
| Ambiguitas/perubahan tujuan/memori/status | 8 skenario | Semua mendapat bantuan CRM. |
| Kontrol perintah yang didukung | 5 lulus | Overview Indonesia/Inggris, kontak, follow-up dan pipeline memakai data CRM asli. |
| Boundary runtime | 7 lulus | Ledger tetap sama di sekitar goal prompts; missing CSRF, foreign tenant dan no session ditolak. Tiga instruksi adversarial tidak menimbulkan write pada router deterministik ini. |
| Lifecycle misi eksplisit | 2 lulus | Cancel sebelum efek memberi nol produk; pause dan wrong resume signal menahan efek; sinyal benar + eksekusi eksplisit menghasilkan satu produk. |

Harness tambahan menghasilkan 64 observasi: 14 `met`, 48 `gap` dan dua
`needs-journey-verification`. Review terhadap kedua respons terakhir menemukan
salah domain, sehingga **50 dari 50 tujuan customer tidak terpenuhi**: 48
unsupported/help dan dua misclassification. Ini variasi reproduksi atas beberapa
akar masalah; bukan 50 bug independen atau persentase kesiapan.

Contoh salah domain:

> Cari jadwal resource yang bentrok dan ajukan pilihan pemindahan reservasi.

Versi Inggrisnya juga diuji. Keduanya memberi `source=crm`, `data.kind=contacts`,
daftar kosong dan pesan bahwa tidak ada kontak yang cocok. Respons bukan rencana
atau klarifikasi untuk Scheduling. Ini memperjelas temuan planner pada
[audit agentic sebelumnya](AGENTIC_AUDIT_2026-11-05.md): prefix pencarian generik
dapat salah mengklasifikasikan tujuan bisnis sebagai pencarian kontak.

## Cakupan operasional yang dieksekusi

- Seluruh 18 workspace dibuka/dibaca. Tiga belas domain tambahan menyimpan satu
  record melalui native review/confirm dengan receipt dan pembuktian SQL.
  Seluruh 120 pilihan aksi pada 15 shared-form modules dirender. Kontrol tambahan
  Purchasing dan Support diuji sampai perubahan durable.
- CRM, inventory/order, purchasing/receiving, invoice/payment/refund, email dan
  support menjalankan alur asli. Owner preview/confirm dan business discovery,
  reviewed import atomic serta proactive reorder diuji pada database tenant.
- Fault injection menguji respons hilang, acknowledgement salah, replay dengan
  key asli, reload, revocation, concurrency dan checkpoint failure setelah commit.
  Misi eksplisit juga membuktikan approval, dependency dan current domain grants.
- Media benar-benar mentransmisikan audio/video RTP ke LiveKit dan PCM/JPEG ke
  fixture model, menerima audio/transcript dan menutup tracks/peer connections.
  WSS dengan verifikasi sertifikat/revocation diuji oleh suite Rust.
- HTTPS/proxy/login isolation, writer startup refusal/takeover, backup terenkripsi,
  isolated restore, digest data dan login/read sesudah restore diuji. External
  effects tetap held sesudah restore; tidak ada plaintext staging tertinggal.

## Workspace dan simulasi library

Full workspace gate `verify.mjs --offline --wasm`: **1.124 tes Rust lulus**,
0 gagal dan 0 diabaikan, terdiri dari 563 unit tests dan 561 integration tests
pada 52 integration suites. Struktur, docs, format, Clippy semua target dan
portable/WASM boundary lulus. Sebanyak 21 doc-test targets diperiksa, dengan
nol executable doctest cases.
Seluruh helper Node: **24 lulus**. Audit dependency terbaru: **0 vulnerability**,
dengan warning transitive `paste` unmaintained yang sudah diizinkan proyek.

Library MCP, campaigns, RPA, evals, context, capacity dan komponen lain ikut dalam
gate seluruh 22 crate. Cakupannya harus dibedakan dari serving integration:

| Komponen | Simulasi yang ada | Yang tidak dibuktikan |
| --- | --- | --- |
| Evals | Lima vector standar melalui oracle lokal, journal invariant dan kontrol target cacat | Oracle prompt/role/temporal menggunakan aturan string; bukan evaluasi reasoning LLM live, keamanan umum atau sertifikasi compliance. |
| Campaigns | Rencana trigger AR/reorder dan registrasi misi pada in-memory CockpitEngine | Scheduler durable pada serving host atau pengiriman WhatsApp/penagihan nyata. |
| MCP | JSON-RPC/tool/authority/resource contracts dan registry gateway | Stock/journal handler mengembalikan label `SIMULATED_READ_MODEL`/`SIMULATED_STAGING`; bukan transaksi database bisnis atau koneksi server eksternal. |
| RPA | Recipe DJP/bank, allowlist dan credential masking | Browser login/eksekusi portal bank/pajak; source yang diuji menghasilkan recipe. |
| Orbz/context | Storage/scope/retrieval dan memory-related regression | Pengingatan otomatis dari percakapan utama; goal memory probes masih gap. |

Tidak ada langkah library yang dipromosikan menjadi klaim integrasi aplikasi hanya
karena nama modulnya menyebut autonomous atau nilai oracle-nya 100%.

## Beban dan pengukuran ulang

Dataset 10.000 kontak + 2.000 produk; concurrency 8, 500 read request per endpoint
serta 200 mutasi unik. Pada run gabungan yang berlangsung bersamaan dengan suite
Rust, semua request berhasil dan 200 mutasi/receipt cocok. p95:

| Fase | Run gabungan | Run terisolasi | Target |
| --- | --- | --- | --- |
| Contacts | 4,91 ms | 3,43 ms | <50 ms |
| Commerce workspace | **54,81 ms** | **30,44 ms** | <50 ms |
| Contact create | 8,94 ms | 3,61 ms | <250 ms |

Run gabungan mempunyai `load.latencyTargetsMet=false`. Sesudah Rust selesai,
run terisolasi mengulang dataset/concurrency yang sama dan memperoleh
`load.latencyTargetsMet=true`. Seluruh request dan 200 mutasi/receipt berhasil pada
kedua run. Run terisolasi menyelesaikan 39 browser/operational checks, 0 page error,
cleanup fixture 0; ini pengulangan subset, bukan tambahan 39 checks unik.

Kedua evidence dipertahankan. Perbedaan dicatat bersama konteks eksekusinya;
audit ini tidak mengisolasi seluruh variabel hardware untuk membuktikan penyebab
kausal tunggal. Hasil lokal tidak mengesahkan sizing atau latency produksi.

## Evidence dan batas

- Combined: `target/e2e/all-simulations-20261105-01/result.json`,
  `product-observations.json`, `agentic-observations.json`,
  `simulation-observations.json` serta screenshots dan log provider-owned fixtures.
- `target/simulations-20261105/`: `browser.log`, `full-gate.log`, `helpers.log`,
  `dependency-audit.log`, `semantic-review.json`, `rust-suite-results.json`,
  `load-isolated.log` dan `summary.json`.
- Isolated load: `target/e2e/all-simulations-load-isolated-20261105-01/result.json`.
- MCP/campaign/RPA library unit results: masing-masing 2/2/4 tes lulus. Evals
  mempunyai 3 unit tests yang mencakup kelima vector standar, empty-suite dan
  defective-target controls. Semuanya termasuk dalam total 1.124, tidak dijumlah
  sebagai tes tambahan.
- [Scenario catalogue](../../tools/product/simulation-scenarios.mjs),
  [runtime simulation harness](../../tools/e2e/simulations.mjs) dan
  [browser runner](../../tools/e2e/verify.mjs).

Skenario goal tidak menguji model live karena planner model belum terpasang ke
chat utama. Keamanan pada prompt negatif hanya dibuktikan untuk jalur deterministik
yang berjalan. Voice-to-business tools, replanning, output-to-input dataflow dan
integrasi eksternal yang belum terpasang tetap belum diterima. Tidak ada claim
seluruh kemungkinan input/state telah diuji. Tidak ada fitur agentic yang
diimplementasikan atau diperbaiki oleh pekerjaan pengujian ini.
