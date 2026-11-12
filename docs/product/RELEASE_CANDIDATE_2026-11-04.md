# Kandidat rilis Archava — 4 November 2026

Status: **kandidat lokal terverifikasi untuk pemasangan/pilot; public production belum disahkan**.
Sembilan temuan audit keseluruhan ditangani. Ini tidak menutup fitur MVP/provider
lain yang masih terbuka pada [launch readiness](LAUNCH_READINESS.md).

## Perubahan dan batas authority

| Audit | Temuan | Hasil remediasi |
| --- | --- | --- |
| F01 | Email replay setelah grant dicabut | Grant, actor dan scope invoice diperiksa sebelum outbox dan sebelum disclosure hasil provider. |
| F02 | Department dari request membuka memori | Department hanya filter; akses department/entity dibatasi Owner All sampai grant yang durable tersedia. |
| F03 | Editor membuat memori Sovereign/L5 | Write memerlukan authority yang sesuai; Sovereign memerlukan Enterprise Owner All. |
| F04 | Semua pengguna proxy berbagi lockout | Trust proxy eksplisit, right-to-left forwarded chain; direct client tidak dapat memilih bucket. |
| F05 | Cold host kehilangan memori durable | Read selalu dalam tenant transaction PostgreSQL; SQL gagal memberi 503. |
| F06 | Entity memory gagal disimpan | Persist scope canonical dan entity ID; validasi ukuran URI/payload. |
| F07 | Email identik/new key menggantung UI | 409 eksplisit mengikat submitted key dan invoice ke duplicateOf; UI menyelesaikan penolakan tanpa invocation kedua. |
| F08 | SDK JavaScript tidak mengubah build identity | Provenance mencakup static/archava.js; paket menolak source/artifact stale. |
| F09 | Format gate gagal | Format diperbaiki; gate penuh dan final library gate lulus. |

Regresi permanen berada di [production_hardening.rs](../../crates/host/tests/production_hardening.rs)
(10 tes), [invoice_email.rs](../../crates/host/tests/invoice_email.rs), unit proxy dan
receipt UI, serta [native-provenance.test.mjs](../../tools/test/native-provenance.test.mjs).
History memori mempertahankan interval asli; episode memakai principal/role dari
membership, bukan klaim body. Callback email setelah navigasi diuji dalam dokumen
WASM yang sama. Grant department/entity yang lebih rinci belum tersedia; pembatasan
Owner All merupakan kontrak saat ini.

## Verifikasi yang dijalankan

- Gate penuh `verify.mjs --offline --wasm`: **1.109 tes Rust**, 0 gagal/diabaikan,
  19 helper, format, Clippy `-D warnings`, WASM, struktur dan tautan docs lulus.
- Sesudah pengelompokan akhir histogram metrics: final `--lib --offline --wasm`
  lulus **555 tes library + 21 helper**, Clippy semua target, format dan WASM.
  Suite integrasi yang sudah lulus tidak dihitung ulang sebagai tes tambahan.
- Helper final: **21 lulus**. Struktur: 22 crate, 0 masalah.
- Fresh release server, preview dan WASM/bindgen: lulus pada Rust 1.96.0,
  Node 22.22.3 dan wasm-bindgen CLI 0.2.129.
- Gate Chromium diperluas: **41 pemeriksaan lulus**, 0 page error, cleanup fixture 0.
  Termasuk email duplicate recovery, late response sesudah SPA navigation,
  production restart, startup refusals dan writer fencing/drain.
- HTTPS menggunakan Caddy dan CA task yang diverifikasi curl: cookie Secure/HttpOnly
  dengan prefix `__Host-`, HSTS, CSP dan metrics publik 404. Lima password salah dari
  satu client memberi 429 pada percobaan berikutnya; client kedua tetap 200.
  CA tidak dipasang ke trust store. DNS publik/ACME tidak diuji oleh rehearsal ini.
- `promtool` resmi v3.15.1: scrape config valid dan **4 alert rule valid**.
  Caddy adapt dan Compose config juga valid; tidak ada service monitoring diaktifkan.
- Audit dependency dari fase audit sebelumnya tetap berlaku karena Cargo.lock tidak
  berubah: 0 vulnerability; 1 warning transitive unmaintained `paste`
  (RUSTSEC-2024-0436). Warning tersebut tetap terbuka, bukan diperbaiki di sini.

## Beban dan pemulihan lokal

Dataset tambahan: 10.000 kontak dan 2.000 produk, concurrency 8; 500 request per
read route dan 200 `contact.create` dengan key unik. Seluruh request mendapat 200,
0 network/status error; SQL membuktikan tepat 200 contact dan 200 receipt baru.

| Jalur | p50 ms | p95 ms | p99 ms | Target lokal p95 ms |
| --- | ---: | ---: | ---: | ---: |
| contacts | 1.88 | 3.46 | 6.31 | 50 |
| commerce | 17.67 | 26.28 | 28.40 | 50 |
| contact-create | 2.05 | 4.18 | 6.03 | 250 |

Hardware: 11th Gen Intel(R) Core(TM) i7-11800H @ 2.30GHz, 16 logical CPU, sekitar 15.3 GiB RAM,
Linux x64. Angka ini merupakan pengukuran lokal singkat pada route yang disebutkan,
bukan penerimaan seluruh mutasi kompleks, soak test atau kapasitas server produksi.

Native Ops menghasilkan archive AES-256-GCM 1,338,695 byte, mode 0600;
backup 167.72 ms, restore 515.33 ms,
restore sampai ready/login 891.05 ms.
Jumlah baris dan digest kontak (10,234), produk (2,001),
CRM command (207) dan email (3) cocok setelah restore.
Login dan authenticated contacts read berhasil dengan role runtime non-owner.
External effects tetap held; tidak ada plaintext staging tersisa.
Ini rehearsal database sintetis terpisah dalam cluster disposable, bukan RTO/RPO
data produksi atau bukti pemulihan dari kehilangan seluruh server/volume.

## Paket dan bukti

- Native distribution: `dist/native-candidate-20261104-remediated-01`,
  **54 artifact** diverifikasi Ops, version/artifact/prior checks lulus.
- Archive: `dist/archava-native-20261104-remediated-01-linux-x86_64.tar.gz`; checksum: `dist/archava-native-20261104-remediated-01-linux-x86_64.tar.gz.sha256`.
- Source SHA-256: `ec008efbdca4bc1297d642144d77dc71b22700000d943491d72eba4e4b4aae11` (398 input, termasuk SDK JavaScript).
- Server SHA-256: `da97d439182352024c30e743244fc37740200b3dbeb909916d93ff91dd8d4917`.
- WASM SHA-256: `293fb73500a68b17b88e95dbfab1b24d1d02fb21851da7e06f0ad9ae091f5662`.
- Evidence lokal: `target/remediation-20261104/` dan
  `target/e2e/remediation-20261104-01/result.json` beserta log/screenshot.
  Direktori ini output lokal, tidak di-stage. Audit awal tetap di
  `target/audit-20261104/REPORT.md`; kegagalan awal tidak ditimpa.
- Paket memuat template systemd, Caddy, scraper dan alert. Template tidak berarti
  alert receiver, backup schedule atau monitoring produksi sudah berjalan.

## Pekerjaan yang memerlukan target produksi

Owner menyebut `archzxy.com`; pemeriksaan read-only menunjukkan Cloudflare DNS
serta respons HTTPS dari Netlify. Rencana [app.archzxy.com](../../deploy/ARCHZXY.md)
mempertahankan situs utama dan menempatkan server Rust/PostgreSQL secara terpisah.
Tidak ada DNS, hosting atau deployment publik diubah. Server target/akses belum
tersedia dalam sesi; Netlify yang disebut owner belum menyediakan target untuk
proses native Archava saat ini.

Public hostname/certificate, konfigurasi server/secrets, alert delivery/on-call,
backup offsite/WAL/retention dan penerimaan beban/RTO produksi belum disahkan.
Provider live tetap ditunda sesuai keputusan owner sebelumnya. Voice/media bridge,
shared presence, verified assistant action worker dan external-origin embed
acceptance masih mengikuti daftar terbuka di launch readiness; candidate ini
tidak mengklaim implementasi atau penerimaan fitur tersebut.
