# Reaudit progres Rust Archxy — 16 Oktober 2026, 23:26 WIB

**Ada kemajuan nyata sejak audit 19:40 WIB, terutama pada fixture, kalender,
dependency pinning dan server-side session/approval/receipt. Full Rust belum
selesai; authority masih parsial dan candidate belum hijau.**

Audit memeriksa snapshot public source **23:26:20 WIB** pada HEAD
`242549cc292cd53bbc66eb9cf119d57443aa2a37`, branch `feat/laundry-1.17.0`.
Source dirty/untracked ikut disalin; belum ada commit baru. Ini pemeriksaan
pekerjaan yang sedang berjalan, bukan audit completed handoff yang sudah diajukan.
Bandingkan dengan [audit sebelumnya](ARCHXY_RUST_AUDIT_2026-10-16_EVENING.md).
Hashes, commands, logs dan probes ada di
[evidence JSON](audit/2026-10-16-rust-late-reaudit.json).

## Perubahan yang benar-benar bekerja

| Area                    | Sebelumnya                                                                            | Hasil baru                                                                                                                                                                                      |
| ----------------------- | ------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RF04 stale fixture      | Salah lokasi PID; live directory bisa terhapus                                        | Exact helper canary mempertahankan live creator maupun live postmaster dan tetap menghapus dead fixture; **9/9 fixture unit tests PASS**                                                        |
| RA07 overflow hang      | `count_working_days(i64::MAX-1, i64::MAX)` macet                                      | Direct probe mendapat controlled error; **35 calendar unit + 13 capacity integration tests PASS**                                                                                               |
| Consumer closure        | Temporary closure memakai unlocked fetch; vendor configuration belum dibuktikan penuh | Consumer lockfile, locked closure derivation, `.cargo/config.toml` dan empty-cache vendored consumer proof tersedia; **10/10 bootstrap tests**, **4 Rust consumer tests + 8 addon checks PASS** |
| Request authority dasar | Tanpa session mendapat 200 `allowed`                                                  | Valid request tanpa session kini **401**; valid fixture-issued sessions melewati real identity lookup/CSRF                                                                                      |
| Durable receipt         | Hanya open/rollback tenant transaction                                                | Command receipt kini disimpan dan bisa direplay/recover melalui actual router dan PostgreSQL                                                                                                    |
| Root Prettier           | Enam files gagal                                                                      | Fresh root format gate **PASS**                                                                                                                                                                 |

Ini kemajuan backend. `POST /commands` masih mencatat receipt dari opaque input;
belum ada dispatcher yang melakukan `product.read`, CRM mutation atau actual
configuration apply. Receipt yang tersimpan tidak membuktikan business operation
yang namanya ada di receipt sudah dilaksanakan.

## Masalah yang masih terkonfirmasi

### RN01 — P1: status receipt tidak mengevaluasi ulang current operation authority

[status_receipt](../rust/crates/host/src/router.rs#L982) hanya memastikan active
membership dan receipt actor cocok. Nilai role/capability/scope yang diperoleh
tidak dipakai untuk mengizinkan pembacaan receipt. Replay command menggunakan
policy gate, sehingga kedua entrypoint berbeda.

Probe memakai valid synthetic service session, real non-owner runtime dan public
readiness constructor. Setelah satu receipt dibuat, membership tetap active tetapi
role diturunkan dari `archava_assistant` menjadi `viewer`:

- `GET /commands/status` tetap **200**, termasuk original input di receipt.
- `POST /commands` untuk replay yang sama ditolak **403 `role_not_allowed`**.

Active membership saja belum memenuhi mandat current read/operation grants pada
status/recovery. Terapkan authority yang sesuai stored operation/resource sebelum
payload dikembalikan; uji downgrade permission/role/capability/scope, bukan hanya
`active=false`. Lindungi historical receipt tanpa menulis ulang history.

### RN02 — P2: race idempotency mengembalikan receipt untuk input berbeda

Sequential replay memeriksa fingerprint, tetapi
[cabang kalah INSERT race](../rust/crates/host/src/router.rs#L939) hanya memeriksa
actor sebelum mengembalikan winning receipt.

Reproduksi memakai dua HTTP requests melalui actual Axum router, actor dan UUID
sama, input `race-A` dan `race-B`. Audit menahan SHARE lock pada commands table
supaya keduanya melewati initial lookup; `pg_stat_activity` membuktikan **dua
INSERT waiters**, lalu lock dilepas.

Keduanya mendapat **200**. Request `race-A` mendapat `replayed` dengan receipt
berisi **`race-B`**. Database tetap memiliki satu receipt untuk UUID itu; audit
tidak menemukan duplicate business effect. Bug-nya adalah input/command binding
yang hilang pada conflict recovery.

Validasi actor dan fingerprint/action/revision pada setiap jalur conflict/reload.
Tambahkan deterministic concurrent same-key/same-input dan same-key/different-input
tests; yang berbeda wajib mendapat conflict sesuai supported contract.

### RN03 — P2: migration 004 belum masuk supported release/migrator

`packages/commerce-pg/migrations/004_bound_approvals.sql` sudah ditambahkan dan
dipakai Rust fixture, tetapi [TypeScript migrator](../packages/commerce-pg/src/migrate.ts#L5)
dan `config/library-release.v1.json` masih menyatakan tiga migrations. Packager
menyalin empat SQL files dan release validator menolaknya:

`ReleaseProfileError: Unsupported local release: migration_set_mismatch`.

Fresh `pnpm reuse:verify` gagal. Existing active runtime tidak boleh dipaksa memakai
database ledger baru tanpa compatibility path. Selesaikan migration ownership,
kernel/table requirements, versions/checksums dan prior-upgrade proof; gunakan
new consistent candidate identity, jangan overwrite archive 1.17.0.

### RN04 — P2: default authority integration suite belum stabil

Fresh workspace tests mencapai `host/tests/authority.rs`, lalu **3/15 PASS,
12 FAIL**. Semua 12 berhenti saat helper login mendapat **401**, sebelum negative
authority assertions yang dimaksud berjalan. Full workspace run exit 101 dan
remaining test targets tidak dijalankan.

Control test `an_authorized_operation_succeeds_with_a_receipt` dijalankan sendiri
dengan satu test thread dan **PASS**. Source membatasi dua concurrent KDFs;
`verify_password` mengubah kegagalan derivation menjadi false. Ini konsisten
dengan parallel login saturation; hasil control tidak berarti seluruh 12 tests
sudah lulus atau seluruh failure setelah login sudah diketahui.

Perbaiki owned auth fixture/suite scheduling dan overload semantics, lalu buktikan
default workspace/regular gate benar-benar PASS. Jangan menaikkan production
security limits, menghapus tests atau mengubah expected auth menjadi success
untuk menutupi masalah. `P-AUTHORITY-PG` named-proof producer juga belum terhubung.

### RN05 — P2 library boundary: kalender dapat mengembalikan tanggal di luar domain

Overflow hang lama sudah diperbaiki. Namun
[next_business_day](../rust/crates/capacity/src/calendar.rs#L326) hanya memvalidasi
input; hasil weekend stepping tidak divalidasi lagi.

`next_business_day(Instant(MAX_SUPPORTED_MILLIS))` mengembalikan
**`Ok(Instant(8640000172800000))`**, sementara batasnya
`8640000000000000`. `result.in_supported_domain()` adalah **false**.

Ini bounded library correctness defect, bukan remote exploit yang dibuktikan.
Tolak result yang melampaui supported domain dan uji public primitive serta
schedule callers. RA07 belum bisa ditutup sebagai seluruh numeric/date safety.

### RN06 — P3: Rust formatting dan Clippy kembali merah pada perubahan baru

Root Prettier sudah PASS, tetapi Rustfmt menemukan diffs pada tujuh files di
host/fixture. Independent Clippy warnings-as-errors exit 101: dua
`too_many_arguments` di `load_bound_approval`/`consume_approval` dan satu
`manual_is_multiple_of` di identity. Pertahankan checks dan rapikan affected code.

## Authority yang masih belum tercakup

Session, membership lookup dan bound approval sudah ada. Namun
[resolve_membership](../rust/crates/host/src/authority.rs#L145) tidak membaca
existing `permissions`, dan service command belum mengikat on-behalf-of human
grant/scope intersection. Probe `product.read` dari service dengan `permissions=[]`
tanpa human delegation tetap **200**. Karena handler belum melakukan actual product
read, ini bukti coarse policy seam belum lengkap, bukan bukti data produk bocor.
Baseline commerce repository mempunyai granular permissions dan service/human
intersection; jangan melewatkannya ketika business handlers dipasang.

Probe tambahan menunjukkan pending approval yang sudah distamp tetap dapat dipakai
sesudah membership approver dicabut. Tetapkan dan uji current-approver/delegation
revocation policy secara eksplisit. Audit tidak menyamakan observasi ini dengan
penutupan atau pelanggaran kebijakan approval yang belum didefinisikan lengkap.

## Ringkasan verifikasi

| Pemeriksaan                          | Hasil snapshot                                                                        |
| ------------------------------------ | ------------------------------------------------------------------------------------- |
| Fresh root `pnpm verify`             | **7/9 PASS**; reuse dan Rust FAIL                                                     |
| Node unit suite                      | **1.821/1.821**, 137 files PASS, tanpa prior benchmark artifacts                      |
| Rust fast gate                       | Build artifact PASS; berhenti pada Rustfmt, belum menjalankan seluruh sepuluh langkah |
| Independent Rust workspace tests     | Exit 101 pada authority **3 PASS/12 FAIL**; bukan full workspace PASS                 |
| Host unit tests                      | **37 PASS**                                                                           |
| Calendar/capacity tests              | **35 + 13 PASS**                                                                      |
| Fixture unit tests                   | **9/9 PASS**                                                                          |
| Bootstrap derivation tests           | **10/10 PASS**                                                                        |
| Packaged Rust consumers              | **4 tests + 8 checks PASS**, offline, empty registry untuk vendored consumer          |
| Positive authority control, isolated | **1/1 PASS**; tidak menggantikan default suite                                        |
| Required coverage                    | **complete=false**, 47 diagnostics dalam audit snapshot                               |

Coverage diagnostics mengandung missing/failed/stale proofs dari partial run dan
required future work. Itu bukan 47 bug atau ukuran kemunduran/progres. Fast gate
yang berhenti di format juga gagal menulis beberapa aggregate evidence karena
referenced outputs belum tersedia. Full release/security/browser/upgrade/restore
acceptance tidak dijalankan; pricing/rates parity tidak diulang karena engine dan
harness-nya tidak berubah. Historical PASS tidak dipromosikan menjadi hasil baru.

## Posisi perjalanan dan tindak lanjut

Masih sepuluh crate, belum ada product binary, Leptos SSR/hydration atau satu
owner journey Rust lengkap. `GET /contacts` masih placeholder refusal. Domain
business migration, public SDK/media/worker/operational Rust replacement dan
OWN-T01–OWN-T10 tetap belum terbukti selesai. Kedua final completion/audit-packet
documents belum ada.

Prioritas: stabilkan RN03/RN04/RN06 dan authority RN01/RN02, lengkapi granular
grants/delegation, tutup boundary RN05, lalu buktikan runnable vertical slice
login → CRM → reviewed actual business effect → receipt → recovery di browser.
Setelah itu lanjut seluruh R00–R11 + OWN-V1 sesuai mandat. Audit ini tidak
mengotorisasi pengurangan scope atau mengganti target menjadi receipt scaffolding.

Tidak ada persentase baru: crate/test/LOC atau manifest row count tidak mengukur
full behavior completion. Penilaian yang bisa dibuktikan adalah **fondasi membaik,
authority sedang dibangun, alur produk lengkap belum tersedia**.

## Metode dan batas snapshot

1.227 public source/config/doc files disalin ke local audit-only clone, tanpa
`.env`, `.local`, owner DB, previous dist atau product build output. Frozen/offline
pnpm install, audit-owned Cargo build cache, unique temporary root dan disposable
PostgreSQL digunakan. Tidak ada product source fix, commit, push, live provider,
browser, container atau REA yang dimulai.

HTTP probes memakai real Axum router `oneshot`, public readiness constructor dan
real non-owner PostgreSQL. Authorization probes menggunakan fixture-issued valid
HMAC sessions/CSRF untuk memisahkan authz dari expensive password hashing; login
sendiri diuji oleh integration suite dan isolated positive control. Race memakai
controlled DB lock dan observed waiters. Cleanup memakai exact extracted helper
dan owned canaries, bukan live user server destruction.

Archxy masih mengedit identity, authority, tests dan coverage wiring selama audit;
dua Clippy allowances juga sudah ditambahkan setelah snapshot. Hasil di atas
terikat source 23:26; perubahan setelahnya belum diklaim verified. Main handler
receipt/status/race, kalender, migrator dan release profile tidak berubah pada
pemeriksaan penutupan. Seluruh fixture/PG milik audit sudah berhenti; service dan
pekerjaan milik user/implementator dipertahankan.
