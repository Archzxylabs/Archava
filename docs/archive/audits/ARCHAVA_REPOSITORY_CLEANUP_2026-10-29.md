# Archava — repository cleanup dan penilaian arsitektur

Tanggal: 29 Oktober 2026. Lingkup: seluruh struktur folder aktif Archava, dokumentasi, helper build/verifikasi, contoh lama, dan lokasi migrasi SQL. Ini bukan pentest ulang atau sertifikasi siap launching.

## Penilaian

**Fondasi arsitekturnya sudah cukup terstruktur. Bagian yang berantakan terutama sisa cutover TypeScript → Rust, jalur tooling yang tidak lagi cocok, dan dokumentasi yang mencampur instruksi aktif dengan sejarah/proposal.** Setelah cleanup, jalur kerja aktif lebih jelas. Masih ada utang pemisahan tanggung jawab dan kontrak release yang perlu dikerjakan terpisah.

Dasarnya adalah 22 crate dalam satu workspace Cargo. `domain` bergantung pada `core`; `adapters` bergantung pada `core` dan `domain`; HTTP dan UI berada di lapisan atas. Ini landasan modular monolith yang masuk akal. Menambah microservice atau memecah crate lagi belum menjadi prioritas dari temuan ini.

## Perubahan yang selesai

| Area | Hasil |
| --- | --- |
| Tool lama | 212 file direlokasi ke `tools/legacy/`; gate lama disalin utuh sebelum entry point verifikasi aktif diperbarui |
| Contoh lama | 90 file dipisahkan ke `examples/legacy/`, dengan penjelasan kebutuhan package atau bundle `deps/` yang tidak tersedia di root |
| Dokumen | 83 file root `docs/` dipindah sesuai fungsi; isi root turun dari 87 menjadi 4 dokumen pintu masuk |
| Migrasi | Satu file owner dipusatkan ke `migrations/owner-pg/`; seluruh 38 checksum SQL tetap identik |
| Launcher | Build, preview, dan persistent launcher memakai workspace root; `--help` tidak menyalakan layanan |
| Verifikasi | Satu entry point aktif menjalankan struktur, helper tests, format, Clippy, dan tes Rust; `--lib`, `--offline`, dan `--plan` punya lingkup eksplisit |
| Dependency inventory | SBOM diperbaiki untuk graph Cargo saat ini; tidak memerlukan root pnpm/package.json yang sudah dihapus |
| Benchmark | Path dependency standalone consumer diarahkan ke `crates/pricing` dan `crates/rates`; kompilasi diverifikasi |
| CI / ownership | Struktur dan helper regression tests ditambahkan ke CI yang ada; CODEOWNERS menunjuk folder aktif |
| Direktori kosong | Direktori migrasi host yang sudah kosong dan `static/archava` kosong dihapus |

Total **386 file direlokasi**. Map lengkap: [moves.json](evidence/repository-cleanup-2026-10-29/moves.json). Hash `sha256` merekam isi saat relokasi; `sha256AfterCleanup` menunjukkan isi tujuan setelah penyesuaian link dokumen. Sebanyak 323 file tooling, contoh, dan laporan audit yang dipindah diverifikasi tetap identik byte demi byte. Dokumen aktif boleh mengalami pembaruan link atau instruksi; bukti audit historis tidak ditulis ulang menjadi klaim hijau baru.

Isi semantik Cargo.toml root tetap identik setelah komentar dan daftar workspace diringkas. Tidak ada perubahan dependency, versi, atau profile build dari perapihan manifest tersebut. Perubahan Rust untuk SQL terbatas pada path embed di bootstrap server dan resolusi direktori di fixture.

## Struktur aktif

```text
Archava/
├── Cargo.toml, Cargo.lock, rust-toolchain.toml
├── crates/                 # 22 crate produk dan pendukung
├── migrations/             # 38 SQL / 29 direktori ledger
├── static/                 # asset yang disajikan host
├── config/                 # katalog release, pricing, template
├── deploy/                 # packaging dan referensi operasi
├── tools/
│   ├── check-structure.mjs
│   ├── rust/               # verify, build, preview, persistent, benchmark Rust
│   ├── release/            # inventory Cargo
│   ├── test/               # regresi helper dan batas arsitektur
│   └── legacy/             # arsip migrasi, bukan gate aktif
├── examples/legacy/        # contoh/bundle konsumen historis
└── docs/
    ├── README.md
    ├── ARCHITECTURE.md
    ├── DEVELOPER_GUIDE.md
    ├── BUILD_STATUS.md
    ├── architecture/, modules/, integration/, reference/
    └── plans/, research/, design/, audit/, rust/, history/
```

Peta tanggung jawab dan diagram dependency terkini ada di [ARCHITECTURE.md](../ARCHITECTURE.md). Dokumen desain subsystem, kontrak lama, proposal, dan laporan audit tetap tersedia, dengan status yang dibedakan dari instruksi operasi aktif.

## PR arsitektur yang masih nyata

| Prioritas | Temuan | Pekerjaan lanjutan |
| --- | --- | --- |
| Utama | `host/src/crm.rs` menggabungkan otorisasi, SQL, receipt/idempotency, dan handler HTTP | Ekstrak repository/application service secara bertahap, dengan kasus negatif tenant, grant, stale revision, dan recovery tetap dijaga |
| Utama | Verifier operator membaca `dist/library`, sedangkan release workflow mengemas binary + site tarball | Selaraskan kontrak artifact/profile, lalu buktikan release identity dari hasil build yang benar |
| Menengah | `host/src/router.rs` 3.404 baris dan `crm.rs` 2.612 baris pada snapshot yang diperiksa | Pecah kepemilikan route/read/command/test; jumlah baris adalah sinyal navigasi, bukan bukti bug dengan sendirinya |
| Menengah | Registrasi ledger diduplikasi antara bootstrap server dan fixture | Rancang katalog ledger bertipe yang dipakai bersama tanpa mengubah ID/checksum SQL |
| Menengah | Dokumen subsystem masih memuat aspirasi yang melebihi wiring runtime dan UI | Kaitkan klaim fitur ke route, adapter, UI, dan bukti acceptance aktual |
| Sebelum release | Cutover root dan banyak perubahan sebelumnya belum tersimpan dalam baseline commit yang direview | Review dan rekam baseline, lalu verifikasi checkout yang dapat direproduksi |

Perapihan folder tidak menghapus masalah ini secara otomatis. Tidak ada klaim bahwa seluruh fitur agentic, ERP, integrasi provider, atau operasi produksi sudah selesai hanya karena crate dan tesnya ada.

## Verifikasi yang dijalankan

- Struktur: 22 crate, nol masalah pada aturan yang diperiksa.
- Helper: 9 tes lulus pada Node **22.22.3** yang dipin.
- Clippy workspace/all-targets dengan warning ditolak: lulus.
- Format workspace: lulus.
- Tes library workspace: **511 lulus**, 22 hasil suite.
- Integrasi readiness database: **5 lulus**, memakai fixture PostgreSQL disposable.
- Check WASM untuk core/domain dan hydration UI: lulus.
- Check standalone benchmark Rust: lulus. Cache target lama tidak writable; pengujian memakai target directory milik tugas, tanpa mengubah ownership cache lama.
- Seluruh **38 hash SQL** dibandingkan dengan snapshot sebelum relokasi: identik.
- SBOM Cargo menghasilkan **352 komponen**. Ini inventaris dependency, bukan hasil vulnerability scan.
- Syntax YAML CI diparse lokal; remote CI tidak dipicu.

[Hasil terstruktur](evidence/repository-cleanup-2026-10-29/checks.json), [integritas SQL](evidence/repository-cleanup-2026-10-29/migration-integrity.json), [dependency graph](evidence/repository-cleanup-2026-10-29/dependency-graph.json), [log library checks](evidence/repository-cleanup-2026-10-29/verify-lib.txt), dan [log readiness](evidence/repository-cleanup-2026-10-29/readiness.txt).

Suite integrasi penuh, pentest, flow browser, dan deployment tidak diulang pada pekerjaan cleanup ini. Perubahan UI sebelumnya punya bukti tersendiri dalam [workspace appearance](../design/WORKSPACE_APPEARANCE.md).

## Yang dipertahankan dengan sengaja

- `target/` sekitar **84 GB** adalah cache/hasil build yang sudah di-ignore, bukan lapisan kode produk. Tidak dilakukan `cargo clean` massal pada checkout yang dipakai bersama. Bila perlu mengosongkan disk, lakukan cleanup build sebagai operasi tersendiri saat tidak ada build yang bergantung padanya.
- `.local/` sekitar **71 MB** berisi database/socket/backup lokal. Ini data operasional, bukan sampah source.
- Embed script/CSS kompatibilitas dan jalur versi di `static/` tetap dipertahankan karena router masih menyajikannya. File yang terlihat lama tidak otomatis aman dihapus.
- Laporan, screenshot, riwayat migration, dan baseline tooling lama diarsipkan; tidak dipakai sebagai bukti current-pass.

Sebelum tugas ini sudah ada 130 file modified, 947 deleted, dan 534 untracked menurut status Git lengkap. Cleanup tidak melakukan reset, staging seluruh repo, commit gabungan, atau deployment. Map relokasi dan evidence dibuat agar hasilnya dapat direview tanpa menimpa cutover maupun pekerjaan agent lain.

## Tindak lanjut: isi `config/`

Review semantik folder menemukan bahwa konfigurasi bersama, data acuan tes, dan katalog lama masih tercampur setelah perapihan struktur awal. Dua file tambahan direlokasi tanpa mengubah byte JSON:

- `config/pricing.v1.json` → `crates/pricing/tests/fixtures/pricing.v1.json`: data acuan regresi milik crate pricing, bukan harga komersial aktif.
- `config/templates.v1.json` → `docs/history/config/templates.v1.json`: katalog dari configurator lama, tanpa pemakaian pada jalur aktif yang ditemukan.

`config/library-release.v1.json` tetap di root karena dibaca CLI operator dan pembuat SBOM. Panduan di [config/README.md](../../config/README.md) menjelaskan fungsi dan pemiliknya. Model/engine pricing menerima input dari pemanggil; pemindahan fixture tidak menambahkan loader harga runtime.

Validasi lanjutan: 56 tes pricing, Clippy all-target untuk pricing/native, format workspace, structure check, dan kedua hash JSON lulus. Ini tambahan dua relokasi di luar map 386 file pada cleanup awal. [Map tambahan](evidence/config-ownership-2026-10-29/moves.json) dan [hasil pemeriksaan](evidence/config-ownership-2026-10-29/checks.json).
