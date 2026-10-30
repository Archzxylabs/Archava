# Archxy — eksperimen pembanding TypeScript dan Rust

**Scope update:** Owner kini menugaskan migrasi Full Rust end-to-end melalui
[execution task](ARCHXY_FULL_RUST_EXECUTION_TASK.md). Batas “pilot saja” di dokumen
ini tetap menjelaskan original experiment dan isolation-nya; ia tidak membatasi
mandat implementasi produk yang lebih baru. Pertahankan fixtures/benchmark rules,
lanjut seluruh R00–R11, dan jangan berhenti menunggu keputusan migrasi yang sudah
diberikan. Audit independen Codex dilakukan setelah candidate lengkap.

Bro Archxy, tugas lu kali ini **membuat dan menjalankan percobaan lokal pada tiga bagian
modul Archava**, lalu membawa bukti buat keputusan teknologi. Owner ingin mencoba
sebelum memutuskan. Codex tetap orchestrator/reviewer; lu implementator.

Selesaikan ketiga prototype, correctness checks, benchmark, cold-install proof dan
laporan dalam satu rangkaian kerja. Jangan berhenti setelah membuat rencana atau
setelah satu microbenchmark. Hasil Rust lebih lambat juga merupakan hasil yang sah.

**Ini izin membuat prototype terisolasi, bukan izin memigrasikan platform.**
Jangan mengaktifkan Rust pada CRM host, mengubah kontrak produk atau menyebut tiga
prototype ini sebagai tiga business modules yang sudah dipindahkan ke Rust.

## 1. Baca konteks dan bekukan baseline

Workspace: `/home/haikaru/Archverse/Lab/Archava_Platform`.

Baca:

- `docs/RUST_MIGRATION_RESEARCH_2026-10-14.md`;
- `docs/PROJECT_AUDIT_2026-10-14.md`;
- bagian aktif `ARCHXY_IMPLEMENTATION_TASK.md`, `agent.md`, dan bagian PRD yang
  relevan untuk pricing, composition dan capacity;
- source dan tests yang disebut di bagian 3 di bawah.

Snapshot saat prompt disusun: `a462591`, candidate profile 1.17.0. Audit menemukan
authorization/integrity blockers dan release identity yang belum konsisten.
Jangan menganggap snapshot itu accepted hanya karena root tests lulus. Bila HEAD
sudah berubah, catat HEAD aktual, working-tree changes dan status audit aktual.

Percobaan pure computation ini boleh berjalan sekarang secara terisolasi. Temuan
audit tetap pekerjaan terpisah dan wajib ditutup sebelum integrasi ke production.
Jangan memasukkan pekerjaan fix seluruh audit ke scope benchmark ini.

Rekam SHA-256 seluruh source, tests, catalogs dan build config baseline yang
dipakai. Working tree bisa berisi pekerjaan owner/Codex; jangan reset, stash atau
memasukkan perubahan orang lain ke commit. Gunakan Node 22.22.3/pnpm 10.15.1 sesuai
repo bila tersedia. Rekam versi Rust, Cargo, compiler target dan dependencies
aktual; pin toolchain serta lockfiles eksperimen.

## 2. Batas perubahan

- Lokasi eksperimen sekarang `/home/haikaru/Archverse/Lab/Experiments/rust-pilot/`,
  mengikuti struktur `/home/haikaru/Archverse/Lab/CLAUDE.md`. Ini menggantikan arahan
  semula `experiments/rust-pilot/` di dalam product repo. Product source tetap pada
  workspace di bagian 1; jangan pindahkan platform.
- Sebelum relocation, buat backup dan inventory/checksums source, fixtures,
  lockfiles serta evidence existing. Backup di `~/Archverse/Backups/`; build caches
  boleh dikecualikan bila regenerable. Pastikan target tidak mempunyai data yang
  akan tertimpa. Salin, verifikasi exact files, baru hapus source experiment lama
  setelah transfer terverifikasi. Jangan reset/recreate prototype yang sudah ada.
- Perbaiki 15 lint errors actual pada Node harness (`URL`/`process`, dua unused
  constants). Sediakan standalone lint/format/test scripts untuk eksperimen di
  lokasi baru. Memindahkan source keluar product lint tidak menyelesaikan errors.
- Setelah relocation, jangan menurunkan recommended lint menjadi dua rules saja.
  Review 15 Oktober membuktikan `createRequire` yang di-anchor ke product
  `package.json` dapat memuat `@eslint/js`; 61 recommended rules sudah lulus pada
  tujuh files harness/config. Restore rules lengkap dengan dependency resolution
  yang explicit atau standalone pinned dependencies; revisi klaim imports
  impossible pada README. Ini berbeda dari enrolling JS ke TS projectService.
- Gunakan explicit `--source-root` atau task-specific `ARCHAVA_PILOT_SOURCE_ROOT`
  untuk baseline build inputs. Relative source imports seperti
  `../../../packages/pricing/src/engine.ts` akan rusak setelah relocation; gantikan
  dengan compiled baseline packaging sesuai bagian 4/5. Runtime cold consumers
  tetap tidak boleh bergantung source imports/workspace links/`NODE_PATH`.
- Eksperimen punya manifests, scripts, lockfiles dan output sendiri. Jangan pasang
  symlink pilot kembali ke product repo, menambah blanket ignores atau mengubah
  product manifests untuk membuat relocation tampak selesai.
- Production packages, apps, SQL migrations, release catalogs, accepted archives,
  runtime profiles, pricebook figures dan PRD tidak diubah oleh pilot.
- Tidak membaca/mengubah `.local`, `.env`, keys, owner DB, grants, settings atau
  backups. Fixtures sintetis saja. Tidak memanggil Gemini/payment/email/provider.
- Tidak push, publish, deploy, memasang database baru untuk owner, atau menjalankan
  root pack/release commands yang menimpa evidence existing. Tidak ada biaya SaaS.
- Build/install dependencies boleh dari registry resmi. Pilih dependencies minimal,
  pin dan review manifests/build scripts/advisories. Jangan menonaktifkan scanning
  atau menjalankan remote shell script sembarang agar instalasi cepat selesai.
- Bila Rust belum tersedia, pasang toolchain resmi secara lokal untuk eksperimen;
  jangan mengubah shell startup atau default toolchain milik pekerjaan lain.
- Native build/cache/binaries diabaikan Git melalui aturan lokal eksperimen.
  Source, fixtures dan lockfiles tetap reviewable. Jangan mengabaikan evidence
  failures dengan mengubah root verification rules.

## 3. Tiga slice yang benar-benar dicoba

### P1 — Pricing: satu quote deterministik

Baseline utama:

- `packages/pricing/src/engine.ts`: `PricingEngine.quote()`;
- `packages/pricing/src/request.ts`, `rounding.ts`;
- `packages/config/src/pricing/`, `config/pricing.v1.json`;
- seluruh `packages/pricing/test/`.

Port kalkulasi quote lengkap beserta output `Quote`, bukan hanya penjumlahan atau
rounding. Catalog dimuat dari fixture host yang sama. Tidak ada angka harga baru.
Uji ID/GLOBAL, seluruh tier, included integrations, bundle discounts, support,
design add-ons, usage/overage, annual option, defaults dan invalid region/tier.

Pertahankan `oneTimeTotal: null` untuk scope custom/Enterprise/from, subtotal,
minimum, unresolved drivers serta urutan lines. Rounding mengikuti perilaku aktual
TypeScript, termasuk floating-point/half boundaries; jangan mengganti semua uang
dengan integer lalu mengklaim output otomatis setara. Catat limitation bila
baseline memperlihatkan numeric edge yang bermasalah.

Dua independent consumers: contoh kebutuhan hospitality dan retail, memakai
catalog dan implementation artifacts identik dengan konfigurasi berbeda.

### P2 — Config: deployment planning dan dependency resolution

Baseline utama:

- `packages/config/src/client/plan.ts`: `buildDeploymentPlan()`;
- `packages/config/src/client/modules.ts`: `ModuleRegistry`;
- config/template schemas dan pure exports `@archava/config/pure`;
- `packages/config/test/deployment-plan.test.ts` dan client-config tests.

Port planning operation lengkap: config validation/defaults, host-owned registry,
dependency graph validation, requested versus dependency modules, supported/planned,
host integration evidence, blockers, suggestions dan `ready`.

Uji missing dependency, cycles bahkan pada module yang tidak dipilih, duplicates,
unknown disabled module, explicit dependency opt-out, declared versions, foreign
tenant evidence, impossible health chain dan forged client health fields.

Template suggestions tidak otomatis menjadi enabled scope. Planning readiness dan
permissions metadata tidak memberikan execution authority. Trusted registry dan
integration evidence berasal dari fixture host, bukan input visitor. Ikuti unknown
field policy aktual tiap schema: ada yang strict, ada yang strip; jangan mengubah
semantics agar Rust terasa lebih aman. Jangan menciptakan semver resolver baru.

Dua independent consumers: hospitality dan retail dengan konfigurasi dan catalog
template existing. Large synthetic graphs hanya stress fixtures, bukan klaim
semua node adalah capabilities Archava yang sudah operasional.

### P3 — Capacity: kalkulasi jadwal, tanpa database writes

Baseline utama:

- `packages/capacity-pg/src/repository.ts`: exported `calculateSchedule()`;
- `packages/capacity/src/contracts.ts`;
- cases schedule di `tools/library/verify-capacity.mjs`, khususnya N1/N4.

Port scheduling calculation saja: daily burn dari weekly capacity, concurrency
limits, occupied intervals, remaining units, earliest start/finish, queue position,
calendar days versus working days, weekends dan insufficient capacity.

Uji future allocation di tengah multi-day job, partial progress, full slots, empty
pool, overlap boundaries, Friday ke Monday dan timezone offsets yang mereferensi
instant sama. Uji guard iteration/horizon dan invalid input secara bounded agar
generator/benchmark tidak membuat process hang atau allocation memory tak terbatas.

Gunakan explicit `desiredStartAt` pada workload terukur. Untuk default `now`, pakai
clock terkontrol pada parity tests dan perkenalkan seam hanya di adapter eksperimen;
jangan mengubah production function. Sesuaikan validation boundary dengan existing
contracts, lalu bedakan refusal adapter dengan behavior kernel.

Dua independent consumers: agency/professional services K05 dengan hours dan
construction/EPC K31 dengan days. Buktikan satu implementation hash, bukan fork.

Tidak port SQL repositories, membership checks, allocation writes atau audit.
Hasilnya **schedule snapshot calculation**, bukan proof transaction safety, real
reservation, pembayaran atau permission enforcement pada database.

## 4. Implementasi dan jalur pembanding

Buat satu Rust workspace kecil berisi ketiga computation slices, dengan
`serde`/`serde_json` dan dependency waktu bila perlu. Untuk pemanggilan Node, default
gunakan NAPI-RS. Pilih versi dari dokumentasi/registry resmi saat implementasi dan
lock secara eksplisit; jangan mengandalkan versi yang ditebak dari prompt.

Rust library tidak boleh memanggil TypeScript atau mengembalikan precomputed fixture
results sebagai implementasinya. Input tak valid tidak boleh menyebabkan process
abort/panic. Hindari handwritten `unsafe`; ungkapkan native/generated boundary dan
dependencies yang memerlukannya. Jangan mengklaim seluruh dependency tree tanpa unsafe.

Sediakan tiga pengukuran yang dilabeli terpisah:

1. **Compute-only diagnostic.** TS compiled JavaScript/JIT warm versus Rust
   release optimized, menggunakan typed/prevalidated input dan output yang benar.
   Native timing instrumentation mengecualikan bridge hanya pada angka diagnostic.
   Jangan membandingkan process spawn per call dengan in-process function.
2. **Node consumer operation.** Existing TS operation versus Node → Rust addon →
   Node result. Ukur seluruh conversion/serialization, validation dan output
   construction. Rust tidak boleh menerima keuntungan dari menghapus validations.
3. **Local request path.** Dua instance host benchmark Node yang sama, satu memilih
   TS dan satu memilih Rust. Same route/limits/JSON/errors/config/workload; loopback
   dan synthetic data. Ukur client-visible latency termasuk transport dan semua
   adapter overhead. Ini request benchmark slice, bukan benchmark full CRM/Archava.

Runtime selector hanya di host/adapter eksperimen, default TS. Rust missing/failure
harus terlihat; jangan silent fallback ke TS saat sedang mengukur Rust. Bila NAPI
tidak dapat dibangun, laporkan blocker dan lanjut correctness/compute yang bisa
dijalankan. CLI/subprocess boleh sebagai diagnostic tetapi tidak menggantikan
hybrid integration proof dan tidak menghasilkan verdict GO untuk hybrid.

Compile/package baseline dari source yang dibekukan ke output eksperimen. Gunakan
actual functions/contracts, bukan reimplementasi TS yang sengaja dibuat lebih
lambat. Tidak benchmark `tsx`, Vitest/dev server atau debug Rust sebagai runtime.
Catalog loading, schema parsing dan registry construction harus punya lifecycle
yang setara. Ukur initialization/cold start terpisah dari steady-state.

Bila profiling menemukan optimasi TS yang nyata, buat varian tambahan hanya dalam
eksperimen. Bandingkan existing TS, optimized TS dan Rust secara jujur; pertahankan
baseline asli dan parity tests. Jangan sengaja mengurangi efisiensi lawan.

## 5. Correctness dulu, baru timing

- Bekukan contract input/output/error untuk setiap operation. Definisikan normalisasi
  error secara sempit dan konsisten; jangan menyamakan semua refusal menjadi `error`.
  Bandingkan field, codes, messages yang merupakan contract, defaults dan ordering.
- Pakai independent expected fixtures dari spesifikasi/tests existing, differential
  TS/Rust tests dan seeded generated cases. TS output bukan oracle sempurna bila
  bertentangan dengan business rule. Laporkan bug baseline; jangan mem-port bug
  diam-diam atau mengubah expected outcome agar keduanya terlihat lulus.
- Minimal 300 distinct valid/invalid fixtures per slice, termasuk kedua consumers
  dan boundary cases. Jumlah fixtures tidak menggantikan branch/invariant coverage.
- Exact money outputs, null/absent distinctions, enum states, timestamps dan array
  order harus sama. JSON object-key ordering boleh dinormalisasi untuk comparison
  structural biasa; raw bytes jangan dinormalisasi jika contract memang memakai
  signature/fingerprint. Pilot ini tidak mengubah command signing/idempotency.
- Assertion dijalankan sebelum timing; output tetap dikonsumsi/checksummed saat
  pengukuran untuk mencegah penghapusan work. Jangan menghabiskan timing loop pada
  deep assertions atau console logging.
- Setiap consumer diinstal ke directory berbeda di luar source workspace, memakai
  hanya declared dependency closure dan compiled artifacts. Matikan source aliases,
  `NODE_PATH` dan workspace resolution. Jangan install seluruh 60 artifacts untuk
  menutupi dependency yang hilang. Buktikan artifact SHA-256 identik antar consumers.
- Node consumer tidak memerlukan Rust compiler bila memakai local prebuilt artifact
  untuk target yang diuji. Wajib proof cold-install current Linux target. Target
  lain cukup matrix planned/untested; tidak perlu mengklaim cross-platform PASS.

Correctness mismatch membuat slice gagal acceptance meskipun Rust lebih cepat.
Jangan membuat cache khusus Rust atau memoized fixture lookup yang tidak ada pada TS.

## 6. Metode benchmark yang adil dan dapat diulang

Rekam CPU/cores, RAM, OS/kernel/libc, power/resource limits bila diketahui, Node,
Rust/toolchain target, build flags, dependency hashes, source/fixture hashes dan
seed. Jangan benchmark sambil build atau menjalankan full regression suite.

Workload harus memisahkan penggunaan wajar dari synthetic stress:

| Slice    | Penggunaan wajar                                        | Stress yang dilabeli jelas                                     |
| -------- | ------------------------------------------------------- | -------------------------------------------------------------- |
| Pricing  | Single quote dengan pricebook existing dan variasi tier | Batch 32/1.000 independent quotes; bukan quote raksasa fiktif  |
| Config   | Registry/template existing dan client scope nyata       | Host fixture DAG 64/512 modules dengan explicit fan-out/depth  |
| Capacity | 0/25 active allocations dan multi-day cases             | 250/1.000 allocations dengan occupied intervals yang realistis |

Pricing batch merupakan API eksperimen tambahan: bandingkan batch identik di kedua
runtime dan laporkan terpisah dari single quote. Jangan menggunakan graph/queue
stress yang selalu cepat menolak sebagai bukti throughput successful operation.

- Warm-up sampai relatif stabil; catat caranya. Minimum lima independent measured
  repetitions per headline comparison, bergantian urutan TS/Rust untuk mengurangi
  efek cache/thermal/order. Pilih durasi/sample count melalui calibration, kemudian
  gunakan budget pengukuran yang sama. Laporkan variation/confidence, bukan best run.
- Local request test memakai external load driver dan concurrency 1/8/32. Pakai
  scheduled arrival/load dengan actual versus target rate, queue delay dan timeout
  accounting; jangan menyembunyikan overload lewat closed-loop averages. Bila ada
  workload yang hanya cocok concurrency 1, jelaskan dan tandai sisanya not run.
- Ukur p50/p95/p99 dalam ms, successful throughput, failures/timeouts, CPU time
  per successful operation, process RSS/peak RSS dan event-loop lag pada Node.
  Pisahkan server resources dari load-driver resources. Native addon termasuk
  memory/CPU Node host; Rust kernel-only RSS bukan total memory hybrid.
- Jangan melaporkan p99 dari sample count yang terlalu kecil. Rekam jumlah samples,
  percentile method dan measurement duration. Ukuran sub-millisecond membutuhkan
  calibrated batching/high-resolution timer; tampilkan absolute savings juga.
- Sama-sama fresh process untuk cold start. Rekam time sampai ready, artifact size,
  clean build, incremental rebuild setelah perubahan kecil pada source eksperimen
  dan packaging/cold-install time. Pisahkan download/cache time dari compile time.
  Jangan menjalankan `cargo clean` pada projects lain atau cache global owner.
- Fixed catalog, immutable snapshots dan same output. Tidak ada filesystem/network
  fetch/provider calls di satu timed path saja. Extra worker/offload/threads diuji
  sebagai varian tersendiri dengan resource budget setara, bukan keunggulan tersembunyi.

## 7. Deliverables yang harus ditinggal

Susunan berikut boleh disesuaikan selama satu-command workflow tetap jelas:

```text
/home/haikaru/Archverse/Lab/Experiments/rust-pilot/
  README.md
  rust-toolchain.toml
  Cargo.toml / Cargo.lock
  crates/                 # pricing, planning, schedule + native boundary
  node/                   # compiled baseline adapters, host, scripts, lockfile
  fixtures/               # synthetic, seeded, independent expected cases
  consumers/              # cold-install consumer recipes
  evidence/               # manifest, parity results, raw runs, summaries
docs/RUST_PILOT_RESULTS.md
```

Expose commands untuk setup/build, correctness, cold consumers, benchmark, dan
`all` yang menjalankan urutannya. Dokumentasikan exact invocation dari experiment
root maupun product repo root memakai explicit experiment path/source-root,
termasuk Node selection. Commands harus fail nonzero untuk failed checks, mismatch
atau missing Rust artifact. Partial evidence tidak boleh diberi status complete.

Evidence manifest memuat actual commit + dirty-source hashes, timestamp, toolchain,
fixtures/artifacts/dependency hashes, OS target, executed commands dan exit codes.
Simpan raw per-run measurements, bukan screenshot/table tulisan tangan saja.

`docs/RUST_PILOT_RESULTS.md` harus mencantumkan:

1. Scope aktual per slice, invariants covered, tests dan independent consumers.
2. Existing TS, optimized TS bila dibuat, Rust compute-only, Node hybrid dan local
   request path; p95/CPU/RSS/throughput, absolute delta, relative delta dan variation.
3. Cold/build/install/package costs, dependency/native target matrix, integration
   lines/effort observed serta maintenance tradeoffs. Jangan mengarang human-hours.
4. Mismatches, skipped/blocked checks, benchmark limitations dan rollback/removal.
5. Verdict per slice: KEEP TS / INVESTIGATE FURTHER / RUST CANDIDATE, dengan bukti.
6. Apa yang belum diuji: SQL latency, tenant auth, production transactions, live AI,
   browser/WASM, deployment dan full platform migration.

Sebelum mengukur, rekam decision thresholds. Default heuristic untuk menjadikan
slice **candidate riset lanjutan**: improvement repeatable sekitar 20% CPU/op atau
p95 request latency, atau 25% total host memory, pada workload wajar dengan hasil
identik. Throughput/error rate tidak memburuk; regression di metrik lain sekitar
5% atau lebih wajib dibahas bersama noise pengukuran. Angka ini policy pilot,
bukan fakta benchmark atau janji ROI. Stress-only wins tidak cukup.

Relative win tetap harus material secara absolut dan masuk akal terhadap perkiraan
frekuensi operation. Bila belum ada production workload/frequency yang diketahui,
katakan ROI belum terbukti. Hasil inconclusive lebih jujur daripada dipaksa GO.

## 8. Penutupan dan handoff ke reviewer

Jalankan experiment tests/build/format/static checks yang relevan. Verifikasi source
produk, catalogs dan existing archives tetap identik dengan baseline. Tidak perlu
root release/repack/security battery hanya untuk membuat pilot terisolasi ini;
jalankan checks tambahan bila ada unexpected shared changes, dan jelaskan penyebab.

Tidak mengubah acceptance/security status produk melalui benchmark. Tidak mengganti
official release archives dengan artifact eksperimen. Untuk menghapus eksperimen,
production TS path harus tetap berfungsi tanpa Rust/toolchain/dependency tambahan.

Jangan minta owner memilih lagi di tengah pekerjaan untuk keputusan rutin. Bila
ada blocker nyata, lanjut bagian independen yang bisa dikerjakan, lalu laporkan
command/error persis dan bagian yang belum selesai. Jangan menutupi blocker dengan
hardcoded output, relaxed assertions, grant changes atau fallback diam-diam.

Final reply ke owner: ringkasan comparison aktual per slice, lokasi evidence,
commands untuk rerun, recommendation berdasarkan data dan limitations. Hindari
klaim “zero bugs”, “Rust pasti lebih aman”, atau “platform selesai dipindahkan”.
**Integrasi atau migrasi berikutnya hanya keputusan setelah review hasil pilot.**
