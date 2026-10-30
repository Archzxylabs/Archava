# Audit Rust Archxy — 16 Oktober 2026, malam

**Verdict: INCOMPLETE untuk mandat Full Rust R00–R11 + OWN-V1. Ada perbaikan
fondasi yang berhasil diverifikasi ulang. Belum tersedia satu alur owner lengkap
di runtime dan UI Rust.**

Snapshot diambil **19:40:37 WIB**, HEAD
`242549cc292cd53bbc66eb9cf119d57443aa2a37`, branch `feat/laundry-1.17.0`.
Source dirty dan untracked ikut diperiksa; HEAD yang belum berubah tidak berarti
tidak ada pekerjaan. Audit ini melanjutkan [audit siang](ARCHXY_RUST_REAUDIT_2026-10-16.md).
Detail identitas, hash source/log/probe dan hasil command ada di
[evidence JSON](audit/2026-10-16-rust-evening-audit.json).

## Temuan yang masih harus dikerjakan

### RA01 — P1: HTTP authority masih berasal dari request body

[CommandSubmission](../rust/crates/host/src/router.rs#L169) menerima tenant,
role, capability, confirmation dan human approval dari caller. Pada
[handler command](../rust/crates/host/src/router.rs#L304), nilai itu langsung
menjadi `ActionRequest`; tidak ada authenticated principal, membership atau
grant/approval yang diselesaikan dari server sebelum transaksi dibuka.
Validasi slug tenant tidak membuktikan hak akses ke tenant tersebut.

Probe independen memakai **constructor publik `AppState::try_new`**, readiness
yang benar-benar PASS, dan disposable PostgreSQL dengan role runtime non-owner.
Request HTTP dijalankan melalui actual Axum router memakai `oneshot`, tanpa
listener jaringan atau browser. Tidak ada cookie/session/authorization header.

| Request sintetis                                                                              | Hasil                                                |
| --------------------------------------------------------------------------------------------- | ---------------------------------------------------- |
| Unknown action, kontrol negatif                                                               | 404 `unknown_action`                                 |
| Tenant `tenant-b`, role `archava_assistant`, action `crm.followup.create`, approval dari body | **200 `{"decision":"allowed","tenant":"tenant-b"}`** |

Ini membuktikan keputusan authority yang salah dan pembukaan konteks tenant.
Handler saat ini hanya membuka lalu rollback transaksi; probe **tidak**
membuktikan follow-up tersimpan atau data pelanggan bocor. Prioritas P1 berlaku
sebagai blocker sebelum business reads/writes dipasang pada jalur tersebut.

Perbaikan: principal/session, membership, service/human grant intersection,
scope dan approval terikat command harus berasal dari server. Uji forged tenant,
role, capability, approval serta revoked grant pada execute/status/replay/recovery.
Checkpoint implementator sendiri mencatat RA01 sebagai pekerjaan berikutnya;
audit ini tidak memperlakukan checkpoint itu sebagai klaim selesai.

### RF04 — P2 baru: stale-fixture cleanup membaca lokasi PID yang salah

[clean_stale_dirs](../rust/crates/pg-fixture/src/lib.rs#L243) membaca
`<fixture>/postmaster.pid`, sedangkan
[provision](../rust/crates/pg-fixture/src/lib.rs#L288) menjalankan PostgreSQL
dengan data directory `<fixture>/data`. Lokasi PID sebenarnya adalah
`<fixture>/data/postmaster.pid`.

Sesudah usia direktori melewati 300 detik, PID yang tidak ditemukan dianggap
tidak hidup dan direktori dihapus. Test run lain yang memakai temporary root
yang sama dapat menghapus fixture yang masih aktif.

Reproduksi aman mengeksekusi **exact function body** dari snapshot dalam program
std-only. Dua direktori canary milik audit diberi usia 600 detik dan PID proses
probe yang masih hidup. Hasil:

| Canary                               | Sebelum        | Sesudah cleanup |
| ------------------------------------ | -------------- | --------------- |
| Layout aktual: `data/postmaster.pid` | Ada, PID hidup | **Terhapus**    |
| Kontrol: `postmaster.pid` di root    | Ada, PID hidup | Tetap ada       |

Ini simulasi layout/PID milik audit, bukan penghapusan server PostgreSQL milik
user. Semua integration tests audit memakai `TMPDIR` khusus supaya reaper tidak
menyentuh fixture di luar audit.

Perbaikan: periksa PID di data directory yang benar, verifikasi kepemilikan dan
liveness sebelum penghapusan, lindungi provisioning yang masih berjalan, serta
tambahkan regression untuk live fixture berusia lebih dari lima menit dan dead
fixture. Tambahkan guard teardown sejak provisioning dimulai agar kegagalan
sesudah server start juga membersihkan resource miliknya sendiri.

### RA07 — P2: kalender masih bisa masuk loop tanpa kemajuan

[Instant::plus_days](../rust/crates/capacity/src/calendar.rs#L110) mengembalikan
instant awal bila checked addition overflow. Sementara itu,
[count_working_days](../rust/crates/capacity/src/calendar.rs#L260) mengulang
sampai instant mencapai finish tanpa mendeteksi bahwa nilai tidak maju.

Direct probe pada crate snapshot memakai `start = i64::MAX - 1` dan
`finish = i64::MAX`. `plus_days_progresses=false`; subprocess dihentikan setelah
timeout satu detik. Ini konfirmasi ulang bug library, bukan demonstrasi endpoint
publik yang sudah bisa dieksploitasi. Belum ada HTTP capacity workflow Rust.

Perbaikan: batas rentang tanggal/horizon/allocation/resource yang eksplisit,
arithmetic fallible dan termination guarantee. Pertahankan koreksi NaN/Infinity
yang sudah ada; koreksi itu tidak menutup overflow kalender.

### Format — P3: root verification belum seluruhnya hijau

Fresh `pnpm verify` exit 1: **8/9 gates PASS**, hanya Prettier gagal pada:

- `docs/rust/EXECUTION_STATE.md`
- `docs/rust/migration-inventory.json`
- `tools/rust/bootstrap.mjs`
- `tools/rust/evidence.mjs`
- `tools/rust/pg-readiness.mjs`
- `tools/rust/test/verify.test.mjs`

Rustfmt dan Clippy sudah PASS. Audit tidak memformat source milik implementator.
Komentar dalam `vitest.artifacts.config.ts` dan artifact test juga masih menunjuk
fast gate/build-artifacts; actual wiring yang benar ada di release gate sesudah
benchmark. Sinkronkan penjelasannya agar instruksi reproduksi tidak menyesatkan.

## Perbaikan yang terbukti bekerja

| Area                          | Verifikasi baru                                                                                                                 | Status sempit                                                         |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------- |
| RA02 startup readiness        | Satu constructor publik menjalankan non-owner, exact migration/checksum ledger dan forced RLS; named fixture-backed suites PASS | Penyambungan readiness ke serving state sudah bekerja                 |
| RF02 fixed/global test DB     | Fixture membuat instance/role/migrations sendiri; workspace integration suites PASS                                             | Target DB terisolasi; lifecycle belum tuntas karena RF04              |
| RA03 cold unit suite          | Source tanpa benchmark build output: **1.821 tests / 137 files PASS**                                                           | Default unit suite tidak lagi bergantung pada output benchmark        |
| RA03 artifact assertions      | Tanpa benchmark output **4/4 FAIL**; setelah `pnpm rust:benchmark` **4/4 PASS**                                                 | Tidak lagi silent skip; release gate menjalankannya setelah benchmark |
| RA05 cold bootstrap           | Empty `CARGO_HOME` → bootstrap PASS → workspace dan benchmark locked/offline metadata PASS                                      | Cache kosong tidak lagi menyebabkan kegagalan resolve yang lama       |
| RF01/RA06 evidence validation | **23/23** release/coverage Node tests PASS, termasuk missing/stale/invented/wrong-identity proofs                               | Cached flags lama sudah diganti actual file/hash/identity validation  |
| RF03 Rust checks              | `cargo fmt --all --check` dan Clippy warnings-as-errors PASS                                                                    | Rust formatting/lint sudah diperbaiki                                 |

Bootstrap workspace dan benchmark memakai locked fetch. Temporary consumer
registry closure masih diselesaikan dengan `cargo fetch` tanpa lockfile;
keberhasilan bootstrap tidak berarti semua dependency graph sudah dipin dengan
cara yang identik.

## Hasil verifikasi independen

| Pemeriksaan                               | Hasil                                                                       |
| ----------------------------------------- | --------------------------------------------------------------------------- |
| Root `pnpm verify`                        | **8/9 gates**, exit 1 karena format                                         |
| Rust fast gate di dalam root verification | **10/10 steps**, exit 0                                                     |
| Rust workspace tests                      | **254 PASS**, sebelum named readiness rerun; tidak dihitung dua kali        |
| Named readiness rerun                     | Host **5/5**, adapters **11/11**, 15 required witness references            |
| Pricing parity                            | 350 fixtures: 338 match, 12 error-class-only, 0 diverge                     |
| Rates parity                              | 753 fixtures: 748 match, 5 corrected-expectation, 0 diverge                 |
| Cold packaged consumers                   | Rust consumer 4 tests; Node addon consumer 8 checks PASS                    |
| WASM boundary                             | Portable crates compile; host refusal diperiksa oleh gate                   |
| Release/coverage unit tests terpisah      | **23/23 PASS**                                                              |
| Benchmark command + artifact tests        | Exit 0; **4/4 artifact tests PASS**                                         |
| Required coverage sesudah fast gate       | **complete=false**, 39 errors atas required proofs/docs yang belum tersedia |

39 errors berasal dari audit source snapshot tanpa warisan dist/evidence;
sebagian proof memang hanya diproduksi full release gate yang tidak dijalankan
di audit ini. Angka itu **bukan** 39 bug, persentase pekerjaan tersisa, atau
perbandingan langsung terhadap cached evidence milik implementator. Benchmark
dijalankan untuk memverifikasi artifact flow; audit tidak menyimpulkan speedup
Rust dari timing pada mesin yang tidak dikontrol secara eksklusif.

## Batas progres dan langkah berikutnya

Workspace sekarang memiliki sepuluh crate, termasuk satu test fixture. Pricing
dan rates sudah punya parity serta dua cold consumer. Readiness, tenant
transaction primitives dan verification tooling membaik secara konkret.

Namun Rust host masih library router: belum ada executable produk atau Leptos
SSR/hydration, authenticated CRM workflow, durable business effect/receipt,
recovery flow maupun owner module assembly UI. Full baseline domain/adapter
migration, runtime/SDK/CLI/worker/realtime dan OWN-T01–T10 juga belum terbukti.
`FULL_RUST_COMPLETION_REPORT.md` dan `FULL_RUST_AUDIT_PACKET.md` masih tidak ada.

Urutan paling berguna: perbaiki RF04 dan format, selesaikan RA01, lalu buktikan
satu vertical slice login → kontak/pipeline → review command → persisted effect
→ receipt → recovery pada Rust host/UI. Tutup RA07 dan planning/capacity proof,
kemudian lanjutkan domain serta OWN-V1 sesuai mandat. Jumlah test, crate, LOC
dan coverage-manifest rows tidak bisa digunakan sebagai persentase Full Rust.

## Metode dan batas audit

Snapshot berisi 1.220 public source/config/doc files dalam local no-checkout
clone khusus audit. `.env`, `.local`, owner DB dan previous dist tidak disalin.
Dependency install frozen/offline; build memakai dependency/compile cache milik
audit sebelumnya dan temporary output milik audit. Toolchain: Node 22.22.3,
pnpm 10.15.1, Rust 1.96.0. Tidak ada perubahan source produk, commit atau push.

Tidak menjalankan full 46-command security chain, browser/upgrade/restore
acceptance, live provider atau deployment. Hasil historisnya tidak dipromosikan
menjadi hasil baru. Tidak ada browser, container, REA atau background monitor
yang dimulai. PostgreSQL task-owned sudah dihentikan oleh fixture; pemeriksaan
akhir tidak menemukan task-owned PostgreSQL yang tersisa. Source snapshot
dibandingkan lagi dengan source kerja setelah pengujian untuk mendeteksi drift.
