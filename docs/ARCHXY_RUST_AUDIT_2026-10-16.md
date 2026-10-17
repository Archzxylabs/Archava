# Audit migrasi Rust — 16 Oktober 2026

**Verdict: fondasi dan beberapa pure engine terbukti lokal; belum siap sebagai
full Rust candidate. Estimasi progres seluruh mandat: sekitar 10–15%.**

Audit ini menghitung migrasi pada [PRD Full Rust](PRD_FULL_RUST.md), termasuk UI,
runtime, domain, recovery dan retirement, serta mandat tambahan
[OWN-V1](OWNER_CONFIGURATION_PRD.md). Angka tersebut bukan progres Archava
keseluruhan: implementasi TypeScript yang sudah ada tetap merupakan pekerjaan
produk yang bernilai. Jumlah crate, test, baris kode dan 40 calon kit tidak
dipakai sebagai persentase migrasi.

## Identitas dan metode

- Snapshot source: **16 Oktober 2026, 08:39:24 WIB**; HEAD
  `242549cc292cd53bbc66eb9cf119d57443aa2a37`, branch `feat/laundry-1.17.0`.
- Snapshot mencakup 1.201 file publik dari tracked dan untracked working tree.
  HEAD saja tidak mengidentifikasi pekerjaan Rust yang belum di-commit.
- Review mencakup sembilan crate Rust, pure-engine contracts, HTTP/policy/SQLx
  boundary, packaging/consumers, parity, benchmark, CI/gates, inventory dan
  kesesuaiannya dengan roadmap serta baseline audit sebelumnya.
- Semua build dan pengujian baru dijalankan dalam salinan independen di
  `/tmp/archava-rust-audit-20261016-lpx42oil/source`, dengan node_modules sendiri.
  Tidak menyalin private `.env`, `.local`, owner DB, dist atau target lama.
- Runtime uji: **Node 22.22.3, pnpm 10.15.1, Rust 1.96.0**. Registry Cargo
  mesin sudah terisi pada build biasa; kasus registry kosong diuji terpisah.
- PostgreSQL tambahan memakai cluster milik audit, Unix socket tersendiri,
  role non-owner dan dua baris sintetis. Cluster sudah dihentikan setelah uji.
- Tidak mengubah source implementator, menjalankan cutover, mengubah owner data,
  menghentikan implementator, melakukan commit atau push.

Observasi, hash source penting dan hasil uji disimpan dalam
[evidence JSON](audit/2026-10-16-rust-audit.json). Seluruh log dan probe berada di
direktori `/tmp` di atas; direktori tersebut bukan release archive.

## Hasil verifikasi baru

| Pemeriksaan                                                         | Hasil aktual                                                 | Makna dan batasnya                                                                                                 |
| ------------------------------------------------------------------- | ------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------ |
| Install snapshot, offline/frozen, tanpa install scripts             | PASS                                                         | 71 workspace projects termasuk root; tidak menggunakan node_modules produk                                         |
| `pnpm test` sebelum benchmark dibuat                                | **FAIL**, 1.822/1.823 tests, 136/137 files                   | Satu test membutuhkan ignored build artifact; RA03                                                                 |
| `pnpm rust:verify` sebelum consumer proof dibuat                    | **FAIL**, 4/8 steps selesai                                  | Rust format, Clippy dan 228 tests lulus; runner Node 48/52, empat gagal membaca artifact yang belum dibuat; RA04   |
| Pricing parity, build baru                                          | 350 fixtures: 338 match, 12 error-class-only, 0 diverge      | Dua belas beda class error tetap dicatat; bukan 350 exact matches                                                  |
| Reference-rate parity, build baru                                   | 753 fixtures: 748 match, 5 corrected expectations, 0 diverge | Scope reference rates, bukan capacity atau planning                                                                |
| Dua independent Rust consumers                                      | PASS                                                         | Consumer crate: 4 tests; consumer NAPI: 8 checks. Pricing/rates saja; bukan dua model bisnis CRM                   |
| `pnpm rust:benchmark`, Node 22                                      | PASS                                                         | Pricing steady-state median 8,10× dan setup+quote 2,53× pada workload ini; rates within-noise                      |
| Benchmark tests sesudah build eksplisit                             | 37/37 PASS                                                   | Menunjukkan efek bootstrap artifact, tidak menghapus kegagalan fresh checkout                                      |
| `pnpm verify` sesudah benchmark dan consumer proof dibuat eksplisit | **9/9 PASS**                                                 | Install, structure, format, lint, typecheck, 1.823 tests/137 files, build, reuse, Rust. Ini hasil warmed workspace |
| Rust gate dalam run warmed tersebut                                 | 8/8 PASS                                                     | 228 Rust tests, 52 Node runner tests, kedua parity harness dan consumer proof                                      |
| Cargo resolution dengan CARGO_HOME kosong dan offline               | **FAIL**, exit 101                                           | `no matching package named serde found`; CI belum menyediakan bootstrap dependency; RA05                           |
| HTTP command tanpa session terhadap PostgreSQL sintetis             | **200 allowed** untuk tenant yang dipilih caller             | Authority boundary belum mengikat identity/membership/approval; RA01                                               |
| RLS dimatikan pada tabel fixture                                    | Host tetap **200 allowed**                                   | Forced-policy checker menolak bila dipanggil langsung, tetapi host tidak memanggilnya; RA02                        |
| Capacity adverse inputs                                             | NaN diterima; calendar overflow tidak selesai dalam 1 detik  | Public library boundary belum aman untuk semua input yang bisa dikonstruksi; RA07                                  |
| Release-report composition dengan tujuh langkah hipotetis hijau     | `incomplete`                                                 | Bukan menjalankan full release gate; binding inventory masih hardcoded unavailable; RA06                           |
| `cargo audit --no-fetch --format json`                              | 182 dependencies, 0 findings pada DB cache                   | Timestamp DB tidak tersedia dalam output. Bukan bukti fresh advisory scan atau produk bebas vulnerability          |

Angka benchmark adalah durasi batch microbenchmark, bukan latency HTTP/UI
produksi. Rust dan TypeScript sama-sama compiled pada uji ini. RSS kedua sisi
tidak memiliki scope pengukuran yang sebanding; tidak ada klaim penghematan
memori keseluruhan. Evidence benchmark yang ada sebelum audit merekam Node 26;
audit ini menghasilkan pengukuran baru pada Node 22 di salinan terisolasi.

## RA01 — P1 sebelum business integration: caller menentukan authority sendiri

Pada [router.rs](../rust/crates/host/src/router.rs), `CommandSubmission` menerima
`tenant`, `role`, `capability`, `confirmed`, `human_approved` dan enabled actions.
Handler membentuk request policy dari nilai tersebut tanpa authenticated session,
server-resolved membership/grant atau approval receipt yang mengikat command.

Probe mengirim request JSON melalui router aktual, tanpa Authorization, cookie
atau CSRF token. Database tersambung sebagai non-owner:

```json
{
  "tenant": "tenant-b",
  "role": "archava_assistant",
  "capability": "act",
  "action_id": "crm.followup.create",
  "confirmed": true,
  "human_approved": true
}
```

Respons aktual: `200 {"decision":"allowed","tenant":"tenant-b"}`. Control
unknown action menghasilkan 404, sehingga probe benar-benar melewati policy.

Validasi kebab-case membatasi format tenant ID; ia tidak membuktikan hak caller
atas tenant. `declined` juga bukan pencabutan membership/grant. Komentar router
yang menyebut cross-tenant request tertolak saat deserialization tidak sesuai
dengan hasil ini.

**Batas dampak:** command saat ini hanya membuka lalu rollback transaction.
Belum ada business write/read pada endpoint tersebut. Audit ini tidak menemukan
kebocoran data owner. Temuan menjadi blocker sebelum menambahkan operasi bisnis.

**Perbaikan:** resolve principal, tenant membership, service/human grant
intersection dan scope di server. Verifikasi approval yang terikat pada actor,
tenant, action, payload digest, revision dan expiry. Tolak forged authority;
recheck grant pada execute/replay/recovery. Tambahkan real PostgreSQL/router
negative tests termasuk revoked grant dan cross-tenant slug yang valid.

## RA02 — P1 sebelum business integration: forced-RLS guard tidak terhubung

[rls.rs](../rust/crates/adapters/src/rls.rs) memiliki `assert_forced_policies`,
tetapi `TenantTransaction::begin` hanya memanggil non-owner check dan tenant
setting/readback. `AppState::new` tidak memverifikasi daftar `Ledger.tables`.

Pada fixture dengan forced RLS aktif, adapter membaca hanya baris tenant yang
sesuai dan tenant setting kembali kosong setelah rollback: ini bukti positif
untuk transaction-local context. Setelah RLS dimatikan pada tabel fixture:

```text
forced_policy_check=Err(UnsatisfiedPolicyGate { expected: 1, found: 0, ... })
forged_tenant_and_approval: status=200 OK, body={"decision":"allowed","tenant":"tenant-b"}
adapter_rows_tenant-a=["synthetic-a", "synthetic-b"]
adapter_rows_tenant-b=["synthetic-a", "synthetic-b"]
```

Dua baris terakhir berasal dari direct adapter probe, bukan endpoint HTTP yang
mengembalikan records. Mereka membuktikan efek missing guard pada fixture.

**Perbaikan:** buat async startup/readiness assertion yang memeriksa migration
identity/checksums, required tables serta enabled/forced policies. Host harus
refuse readiness/serving ketika assertion gagal. Tes dengan policy yang benar,
disabled policy, missing table dan runtime owner. Test yang memeriksa suffix
literal SQL terpisah tidak membuktikan query implementasi dijalankan.

## RA03 — P1: unit suite memerlukan hasil benchmark yang belum dibuat

[pricing-coverage.test.ts](../tools/rust/benchmark/test/pricing-coverage.test.ts)
baris 169 mewajibkan keberadaan compiled baseline dan menyuruh menjalankan
benchmark terlebih dahulu. Root verify dan CI menjalankan unit tests sebelum
build/benchmark. Checkout bersih gagal; setelah benchmark dibuat, 37 benchmark
tests dan kemudian seluruh 1.823 unit tests lulus tanpa perubahan source.

Baris 262 dan 354 juga memakai early `return` jika artifact tidak ada. Test
tersebut dilaporkan PASS tanpa menjalankan assertion yang membutuhkan artifact.

**Perbaikan:** unit tests memakai temporary fixture/build setup yang mereka
miliki, atau pindahkan compiled-consumer integration tests ke gate yang secara
eksplisit membuild dependency-nya. Jangan mensyaratkan menjalankan benchmark
timing sebelum unit suite, menyembunyikan test dengan early return, atau memakai
artifact owner sebagai setup. Tambahkan proof fresh checkout tanpa dist/target.

## RA04 — P1: standalone fast gate membaca output langkah yang belum dijalankan

[verify.mjs](../tools/rust/verify.mjs) menjalankan release-report unit tests pada
step kelima. Empat test memanggil `inventoryBinding()`, yang membaca
`dist/rust-consumer-proof.json` dan `dist/library/release-profile.json` langsung
dari repository. Rust consumer proof baru diproduksi pada step kedelapan;
library profile diproduksi oleh jalur npm packaging lain.

Fresh `pnpm rust:verify` gagal empat test dengan ENOENT sebelum parity/consumer
steps. Setelah kedua artifact tersedia, runner yang sama lulus 52/52 dan gate
lulus 8/8. Menghubungkan gate ke CI merupakan kemajuan, tetapi urutan ini masih
mengandalkan riwayat penggunaan folder lokal.

**Perbaikan:** inject fixture evidence pada unit tests report composition.
Integration gate harus membangun setiap prerequisite sebelum membacanya dan
memiliki proof standalone dari clean checkout. Jangan membaliknya dengan fake
PASS/default hashes ketika artifact tidak ada.

## RA05 — P1 untuk CI bersih: offline Cargo tanpa dependency bootstrap

[CI](../.github/workflows/ci.yml) memasang compiler, tetapi tidak menjalankan
Cargo fetch/vendor bootstrap atau restore dependency cache. Sementara itu
[build-artifacts.mjs](../tools/rust/build-artifacts.mjs) memulai dengan
`cargo build --offline --release --locked`.

Probe menggunakan CARGO_HOME kosong dan toolchain yang sudah terpasang:
`cargo metadata --offline --locked --manifest-path rust/Cargo.toml` gagal
exit 101 karena serde tidak ada. Lulus pada registry cache lokal tidak membuktikan
job CI bersih bisa membuild artifact.

**Perbaikan:** sediakan locked dependency fetch dalam bootstrap CI atau vendored
closure yang diverifikasi; sesudah itu baru enforce offline build. Cargo
[mendokumentasikan fetch sebelum offline commands](https://doc.rust-lang.org/cargo/commands/cargo-fetch.html).
Cache merupakan optimasi, bukan satu-satunya cara job bisa berjalan.

Komentar CI juga menyebut action membaca pin subdirectory, sedangkan
[`dtolnay/rust-toolchain`](https://github.com/dtolnay/rust-toolchain) memilih
toolchain dari revision/input. Action `@stable` dan `rust/rust-toolchain.toml`
perlu diselaraskan, termasuk benchmark/consumer yang berjalan di luar `rust/`.
Rustup dapat memilih pin ketika cwd berada di dalam `rust/`; audit tidak
menganggap semua command sekarang pasti memakai compiler yang salah.

## RA06 — P2: inventory belum dapat membuktikan completion atau candidate identity

[migration-inventory.json](rust/migration-inventory.json) masih berisi aggregate
counts dan nama file; belum ada per-behavior IDs, replacement, authority/data
cohort, required proof IDs atau status evidence. Inventory juga menyebut file
server/operator/settings sebagai browser panels dan menunjuk Gemini adapter
yang tidak ada di `apps/crm/src/gemini.ts`. Implementasinya berada di
`packages/crm-assistant/src/gemini-planner.ts`; dashboard domain tersebar di
`packages/*/src/dashboard.ts`.

[verify-release.mjs](../tools/rust/verify-release.mjs) secara jujur mengembalikan
`incomplete` karena `perItemProofIdsAvailable = false`. Tujuh langkah hijau
hipotetis pada pure composition probe juga tetap incomplete. Ini guard yang
baik, sekaligus membuktikan full completion gate belum selesai diimplementasikan.
Browser/upgrade proofs yang dirangkainya saat ini masih untuk host Node existing,
sehingga tidak menggantikan Rust UI/upgrade/recovery evidence.

Candidate identity disalin dari baseline inventory, bukan runtime/source snapshot
yang sedang diuji. Probe Node 22 menghasilkan `candidate.node = v26.10.0`.
Timestamp freshness artifact juga tidak menggantikan hash current dirty source,
lockfiles dan build configuration. Tidak ada false acceptance pada jalur sekarang
karena verdict masih incomplete; identity binding tetap harus diperbaiki sebelum
guard tersebut dapat menyatakan candidate-verified.

**Perbaikan:** satu ledger item-per-behavior dengan consumer, data owner,
transaction cohort, target/replacement, required proof IDs dan disposition.
Bind actual source/tree hashes, runtime, toolchain dan artifact bytes pada run
yang sama. R00 belum complete hanya karena keempat kelompok dokumennya ada.

## RA07 — P2 sebelum library dipakai luas: capacity input dapat menghasilkan NaN/hang

[schedule.rs](../rust/crates/capacity/src/schedule.rs) menerima public `f64`
capacity/required units tanpa finite-value validation. Probe direct Rust consumer
mendapat `Ok` untuk NaN required units dan NaN weekly capacity; kasus terakhir
menghasilkan `burn_rate_per_day: NaN`. Negative required units juga diterima.

[calendar.rs](../rust/crates/capacity/src/calendar.rs) mengizinkan seluruh i64
sebagai `Instant`. Overflow `plus_days` mengembalikan instant semula.
`count_working_days(Instant(i64::MAX - 1), Instant(i64::MAX))` tidak pernah maju;
probe dihentikan setelah timeout satu detik. Batas outer scheduling iterations
tidak membatasi inner calendar loop tersebut.

**Batas dampak:** belum ada route bisnis Rust yang mengekspos fungsi ini.
NaN tidak dapat dikirim sebagai angka JSON standar, tetapi dapat dibuat oleh
Rust caller dari public API. Ini library-readiness defect, bukan bukti remote DoS
pada owner host. Perilaku zero units dapat dipertahankan jika memang contractual.

**Perbaikan:** validated numeric/date types, finite and domain constraints,
bounded scheduling horizon/allocation size, overflow yang mengembalikan error,
dan invariant loop selalu maju. Catat intentional correction terhadap baseline
jika contract lama memang menerima invalid input; jangan sekadar mengganti
reference fixture agar test hijau.

## Status architecture dan roadmap

Modular monolith tetap cocok dengan tujuan reuse. Pembagian pure contracts,
engines, host dan adapter sudah mulai terlihat; transisi ke microservices tidak
menyelesaikan blocker di atas. Saat ini host berupa router library, belum runnable
product binary, authenticated application atau UI.

| Scope                                                    | Status audit                                                                                                                                               |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R00 baseline/inventory                                   | Partial: aggregate inventory dan docs ada, corrected baseline/coverage/identity belum selesai                                                              |
| R01 core + HTTP + DB + UI feasibility                    | Prototype: workspace/types/policy/transaction seam ada; authenticated flow, readiness, login/contacts/recovery UI belum ada                                |
| R02 pricing/planning/capacity                            | Partial: pricing/rates punya fresh parity, benchmark dan cold proof; assembly/capacity punya implementation/tests tetapi belum seluruh exit proof tersebut |
| R03 complete CRM/action/assistant slice                  | Belum terbukti sebagai Rust operational consumer                                                                                                           |
| R04 organizations/proposals/projects/capacity operations | Belum ada Rust domain persistence/UI port; pure capacity math bukan keseluruhan domain                                                                     |
| R05 scheduling/accommodation/assets                      | Belum ada Rust operational port                                                                                                                            |
| R06 commerce/purchasing/foodservice stock cohort         | Belum ada Rust operational port                                                                                                                            |
| R07 billing/subscriptions/attendance/support/fulfillment | Belum ada Rust operational port                                                                                                                            |
| R08 laundry/field services/formal education              | Belum ada Rust operational port                                                                                                                            |
| R09 SDK/knowledge/providers/voice/avatar completeness    | Mapping ada; Rust implementation/integration parity belum terbukti                                                                                         |
| R10 operations/release/cutover                           | Gate tooling mulai ada; Rust operator CLI, actual upgrade/restore/fencing/rollback belum terbukti                                                          |
| R11 acceptance dan legacy retirement                     | Belum dapat diterima; Node/TS masih first-party runtime existing                                                                                           |
| OWN-V1                                                   | Belum ada real owner inspect → preview → confirm → apply → recover/history/restore flow dalam Rust                                                         |

Tidak ditemukan Leptos dependency atau SSR/hydration UI dalam workspace Rust.
Alasan menunggu latest Leptos RC masih tercatat meskipun [stable 0.8.17 tersedia](https://docs.rs/leptos/0.8.17/leptos/).
Lakukan compatibility spike pada stable pins, bukan menunggu label latest berubah.
Keberhasilan compile pure crate ke WASM bukan bukti UI/hydration atau SDK berjalan.

Tidak ada additional R00–R11 milestone yang dapat dinyatakan independently
accepted dari audit ini. Scope pricing/rates yang berhasil tidak menutup seluruh
R02 yang secara eksplisit juga meminta deployment planning dan capacity.

## Dasar estimasi 10–15%

Berikut rubric pekerjaan untuk menjelaskan estimasi, bukan automated coverage
atau angka dari implementator. Poin adalah approximate engineering judgement;
dibulatkan karena inventory behavior belum lengkap.

| Kelompok pekerjaan                                        |   Bobot | Poin yang dapat dikreditkan sekarang |
| --------------------------------------------------------- | ------: | -----------------------------------: |
| Corrected baseline, requirements, inventory               |      10 |                                   ~3 |
| Kernel/contracts, authority, host/DB foundation dan gates |      15 |                                   ~2 |
| Pure engines beserta parity/packaging/performance proof   |      10 |                                   ~6 |
| Complete CRM/action/assistant foundation                  |      15 |                                    0 |
| Domain/cohort migrasi R04–R08                             |      25 |                                    0 |
| Rust UI, SDK dan realtime completeness                    |      10 |                                    0 |
| Operations, compatibility, cutover dan retirement         |      10 |                                    0 |
| OWN-V1 owner configuration                                |       5 |                                    0 |
| **Total**                                                 | **100** |                              **~11** |

Jadi jawaban yang layak sekarang adalah **sekitar 10–15% dari full Rust mandate**,
bukan 50–80%. Ini dapat direvisi setelah per-behavior inventory tersedia. Zero
poin pada suatu kelompok berarti belum ada operational Rust deliverable yang
dapat dikreditkan, bukan belum ada desain atau implementasi TypeScript sebelumnya.
Belum ada satu full Rust owner workflow yang memenuhi acceptance end-to-end.

## Urutan lanjut yang konkret

1. **MUST BUILD NOW:** tutup RA03–RA05 dengan cold-checkout proof; tutup RA01–RA02
   sebelum menambahkan business read/write. Reproduce remaining baseline A/J
   findings pada jalur yang akan diport, khususnya authorization saat replay.
2. Lengkapi inventory RA06 dan feasibility UI stable: authenticated login →
   contact read → reviewed command → receipt/recovery pada synthetic PostgreSQL.
   Tambahkan real revoked-scope/CSRF/replay/cross-tenant browser/API tests.
3. **SHOULD BUILD NEXT:** tutup capacity RA07 dan remaining R02 proof untuk
   planning/capacity; kemudian selesaikan R03 dengan K05 dan K04 menggunakan core
   dan CRM artifact identik. Jangan menganggap NAPI smoke consumer sebagai proof
   dua bisnis CRM.
4. **BUILD LATER, masih bagian mandat selesai:** R04–R11 menurut data/transaction
   cohorts; OWN-V1 sebagai explicit product delta. Pertahankan local checkpoints
   dan lanjut tanpa approval rutin, sesuai execution task existing.
5. **DO NOT BUILD YET:** 40 full kits, universal ERP, framework/ORM/auth sendiri,
   dynamic native plugins, microservice fleet atau owner-data cutover sebelum
   readiness dan recovery proof tersedia.

## Batas kesimpulan

Audit ini tidak menjalankan ulang seluruh 46-command security chain, provider
live, standalone template battery, archived upgrade/restore atau full release
gate. Hasil historisnya bukan fresh Rust acceptance. Audit sebelumnya A01–A09
dan J01–J04 tidak ditutup otomatis oleh 9/9 warmed gates; manifest bootstrap
lama tidak gagal pada unit run ini, tetapi itu tidak menutup temuan lain.

Full release verdict diuji melalui pure composition dengan steps sintetis dan
source inspection, bukan diklaim sebagai tujuh proof yang dieksekusi. Source
snapshot/hash diperiksa lagi pada penutupan; setiap perubahan implementator
setelah snapshot harus direview sebagai delta. Tidak ada klaim zero bugs,
zero vulnerabilities, atau bahwa semua file produk sudah direview manual baris
per baris.
