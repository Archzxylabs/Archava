# Riset tradeoff Rust untuk Archava — 14 Oktober 2026

Riset memakai dokumentasi dan laporan engineering primer, serta source snapshot
`a462591`. Ini bahan keputusan, bukan persetujuan migrasi atau perubahan PRD.
Tidak ada source produk, database owner, dependency atau toolchain yang diubah.
Tidak ada benchmark Rust versus TypeScript Archava yang dijalankan dalam riset ini.

**Rekomendasi saat ini: pertahankan TypeScript sebagai platform bisnis utama;
evaluasi Rust untuk workload terukur yang mendapatkan keuntungan CPU/memory yang
cukup besar untuk menutup biaya maintenance. Full backend Rust tetap opsi yang
valid bila profiling dan pengalaman pilot menunjukkan keuntungan menyeluruh.**

## Posisi awal yang benar-benar ada

- Monorepo pnpm dengan 68 workspace projects; library profile 1.17.0 mempunyai
  60 internal artifacts dan 22 database ledgers. Bukan semua artifacts adalah
  business modules; inventory mencakup core, SDK, adapters dan infrastructure.
- `packages/<domain>` memiliki TypeScript/Zod contracts, service/API/tools dan
  browser dashboard. `<domain>-pg` memakai PostgreSQL, raw SQL dan custom ledgers.
- `apps/crm` mengonsumsi compiled Node packages; `apps/web` memakai TypeScript
  web server/esbuild. Standalone realtime template mempunyai jalur lain.
- Browser SDK/page awareness dan domain dashboards berinteraksi langsung dengan
  JavaScript/DOM. Moving backend tidak mengharuskan frontend ikut ditulis ulang.
- Purchasing/commerce mempertahankan stock receipt, audit dan command claims
  dalam satu transaksi PostgreSQL. Billing memakai signed events; fingerprint
  beberapa commands mengandalkan exact JSON serialization.
- Audit terbaru menemukan authorization/integrity blockers. Rust tidak otomatis
  menutupnya. Lihat [audit source](PROJECT_AUDIT_2026-10-14.md).

Fakta di atas berasal dari repository, bukan generalisasi internet. Penilaian
biaya dan rekomendasi di bawah adalah inferensi engineering terhadap konteks ini.

## Fakta riset dan implikasinya

### 1. Runtime performance dan memory

Ownership memeriksa aturan memory saat compile tanpa garbage collector. Ini
membuka peluang penggunaan resource dan latency yang lebih terkendali; hasil
aplikasi tetap perlu diukur. [Rust Book](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html).

Discord melaporkan perbaikan pada Read States service setelah memindahkan workload
cache besar dengan latency spikes dari Go ke Rust. Kasusnya spesifik, tahun 2020,
dan bukan benchmark CRM/AI atau pembuktian semua aplikasi Rust lebih cepat.
[Laporan engineering Discord](https://discord.com/blog/why-discord-is-switching-from-go-to-rust).

Implikasi Archava: kandidat yang masuk akal adalah processing lokal yang CPU-heavy,
large transformations, media work yang benar-benar kita jalankan sendiri, atau
memory-bound components. Menunggu PostgreSQL, network atau inference provider
tidak otomatis menjadi lebih cepat. Node juga memakai native cryptographic
implementations; mengganti wrapper bahasa bukan jaminan crypto lebih cepat.
[Node crypto](https://nodejs.org/api/crypto.html).

Contoh aritmetika **hipotetis**, bukan hasil benchmark: satu request 100 ms terdiri
dari 95 ms menunggu dan 5 ms processing lokal. Jika bagian lokal menjadi 1 ms,
latency total 96 ms, turun 4%. CPU savings dan throughput harus diukur terpisah
dari latency request tersebut.

### 2. Productivity dan compiler cost

Ownership, borrowing dan lifetimes membutuhkan model pemrograman yang berbeda.
Compiler dapat mencegah kesalahan sebelum runtime, tetapi adaptasi dan debugging
jenis error baru mempunyai biaya bagi tim yang belum familiar.
[Rust Book](https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html).

Compiler Performance Survey 2025 menjelaskan variasi build experience serta
keluhan incremental rebuild/linking dalam workspace. State of Rust 2025, diterbitkan
Maret 2026, masih mencatat compile-time/resource usage sebagai hambatan. Itu
survey responden Rust, bukan eksperimen yang membuktikan tiap developer lebih
lambat dari TypeScript. [Compiler survey](https://blog.rust-lang.org/2025/09/10/rust-compiler-performance-survey-2025-results/),
[State of Rust 2025](https://blog.rust-lang.org/2026/03/02/2025-State-Of-Rust-Survey-results/).

Implikasi: karena banyak shared contracts, perubahan pada crate dasar dapat
memengaruhi rebuild banyak consumers. AI implementator membantu menghasilkan
kode, tetapi keberhasilan compile tidak membuktikan permission, transaksi atau
business behavior sama. Jangan mengasumsikan Rust selalu memperlambat development;
setelah tim terbiasa, compiler dan typed models bisa mengurangi debugging tertentu.

### 3. AI provider ecosystem

Google mencantumkan official GenAI SDK untuk Python, JavaScript/TypeScript, Go dan
Java. Dokumentasi integrasi menyebut Rust sebagai bahasa yang dapat memakai
Direct API lewat REST/gRPC. Jadi Gemini tetap bisa dipakai, dengan kebutuhan
adapter/types/helpers sendiri atau library komunitas yang harus dipilih dan
dirawat. [SDK resmi](https://ai.google.dev/gemini-api/docs/libraries),
[Direct API integration](https://ai.google.dev/gemini-api/docs/partner-integration).

Implikasi: feature mapping, streaming, Live API transport, tools, retries dan
error handling tetap pekerjaan kita. Bahasa backend tidak dengan sendirinya
mengurangi token, voice minutes atau avatar usage. Pertahankan AI adapter
TypeScript jika portingnya tidak menghasilkan keuntungan terukur.

### 4. Reuse dan packaging

Rust dapat dipanggil Node memakai native addon dan dapat dipakai browser melalui
WebAssembly. NAPI-RS menyediakan loader dan TypeScript declarations, tetapi native
artifacts tetap dibangun untuk kombinasi OS/CPU/libc yang didukung. WASM mempunyai
JavaScript glue dan kontrak conversion sendiri. [NAPI-RS release](https://napi.rs/docs/deep-dive/release),
[compatibility](https://napi.rs/docs/more/support-compatibility),
[wasm-bindgen](https://wasm-bindgen.github.io/wasm-bindgen/).

Implikasi: shared pure computation bisa reusable di beberapa host. Business
service bukan drop-in pengganti `@archava/<domain>` yang sekarang mengexport
service dan DOM dashboard. Packaging perlu frontend facade/API atau native/WASM
wrapper, bukan mengganti ekstensi `.ts` dengan `.rs`.

Rust tidak otomatis membuat puzzle kit lebih mudah. Dependency metadata, tenancy,
config, permission, compatible versions dan migrations tetap kontrak platform.

### 5. PostgreSQL dan transaksi

SQLx mendukung PostgreSQL dan compile-time query checking melalui macros; build
dapat memakai prepared offline metadata. Query checking membutuhkan schema yang
sesuai. Itu tidak membuktikan authorization atau validitas lifecycle bisnis.
[SQLx upstream](https://github.com/transact-rs/sqlx).

Implikasi: database dan SQL migrations existing dapat dipertahankan. Yang perlu
diport adalah repositories, runtime identity/grants, transaction boundaries,
locking, audit, idempotency dan adapters. Perubahan driver tidak memberi alasan
untuk memindahkan data owner atau menulis ulang migration ledgers accepted.

Satu koneksi/transaksi stock receipt purchasing-commerce tidak dapat dipecah
menjadi dua service commits tanpa mendesain ulang atomicity. Rust tidak mewajibkan
microservices; full Rust backend juga dapat tetap modular monolith.

### 6. Wire contracts dan compatibility

Shared TS/Zod contracts harus punya pengganti untuk Rust structures dan frontend
types/validation. Serde menyediakan attributes untuk naming/defaults/unknown-field
policy; strict behavior tidak boleh diasumsikan sama dengan Zod `.strict()`.
[Serde container attributes](https://serde.rs/container-attrs.html).

Wajib mempertahankan semantics UUID, tenant IDs, enum states, absent versus null,
defaults, currency minor units, rounding, timestamps dan HTTP error/status codes.
Untuk commands yang difingerprint dari JSON, perbedaan order/encoding/number/date
format dapat membuat replay valid dianggap conflict. Provider HMAC framing dan
claim fields harus menghasilkan bytes yang identik melalui golden fixtures.

Contract generation membantu menghindari dua sumber types, tetapi tidak otomatis
menghasilkan business validation atau compatibility yang benar. Rust unit tests
baru melengkapi existing API/PostgreSQL/browser acceptance, bukan menggantikannya.

### 7. Security dan supply chain

Safe Rust mencegah kategori memory/data-race bugs tertentu, dengan caveat code
`unsafe` dan external/native boundaries. Runtime bisnis tetap dapat mempunyai
permission bypass atau incorrect payment/session binding. JavaScript application
code juga mempunyai managed memory; migrasi bukan bukti semua vulnerabilities
existing hilang. [Rust Reference](https://doc.rust-lang.org/reference/behavior-considered-undefined.html).

Crates tetap dapat memiliki advisories; Cargo dapat menjalankan build scripts.
Curated dependencies, lockfile/provenance dan auditing tetap diperlukan. Ini
mengganti sebagian supply-chain surface, bukan menghapusnya.
[RustSec](https://rustsec.org/), [Cargo build scripts](https://doc.rust-lang.org/cargo/reference/build-scripts.html).

### 8. Deployment, biaya dan observability

Full Rust backend dapat didistribusikan sebagai native executable plus konfigurasi
dan frontend assets. Runtime backend tidak harus menginstall Node, tetapi build
frontend/tooling TypeScript dapat tetap memakai Node. Binary compatibility dan
native dependencies masih perlu cocok dengan target deployment.

Hybrid native addon tetap membutuhkan Node. Hybrid HTTP service menambah process,
network hop, deployment/restart, telemetry dan potential cross-service transaction
coordination. Itu biaya operational nyata untuk dua manusia. Native/WASM bridge
pun mempunyai conversion overhead; fungsi kecil yang sering melintas boundary
belum tentu mendapatkan keuntungan.

Infrastructure savings hanya ada bila bottleneck dan billing model memungkinkan
resource benar-benar dikurangi. Toolchain learning, code port, compatibility tests,
parallel maintenance dan opportunity cost delivery harus masuk perhitungan.
Tidak ada dasar untuk angka penghematan rupiah, persentase speedup atau tanggal
selesai migrasi tanpa pilot dan workload/hardware yang disepakati.

## Tiga opsi yang bisa dipilih

| Opsi                                            | Keuntungan yang ditargetkan                                                                   | Biaya / konsekuensi                                                                                                         |
| ----------------------------------------------- | --------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------- |
| TypeScript platform + optimasi terukur          | Mempertahankan shared contracts, packages, provider adapters dan delivery workflow            | Masih membutuhkan profiling, SQL/pool tuning dan CPU isolation bila relevan                                                 |
| TypeScript + satu Rust computation component    | Mengambil keuntungan pada bagian CPU/memory-heavy; API dan domain transactions tetap familiar | Dua toolchains; packaging supported targets; boundary conversion dan parity tests                                           |
| Full Rust modular backend + TypeScript frontend | Satu ownership model/backend runtime Rust; kendali memory/concurrency menyeluruh              | Port banyak services/adapters dan release tooling; regenerate frontend contracts; ulang semua acceptance dan upgrade proofs |

Opsi kedua paling layak dievaluasi **bila ditemukan kandidat**. Menambah Rust tanpa
workload yang tepat justru menambah maintenance. Node optimizations seperti
memisahkan expensive work dari event loop juga perlu menjadi baseline pembanding.
[Panduan resmi Node](https://nodejs.org/en/learn/asynchronous-work/dont-block-the-event-loop).

## Decision gate sebelum mengizinkan migrasi

1. Tutup semantic security/integrity findings existing terlebih dahulu supaya
   port tidak membawa behavior yang salah sebagai baseline.
2. Profile workload representatif yang sintetis: CPU time, resident memory,
   allocation/GC, database/network waiting, p50/p95/p99 dan concurrency saturation.
   Ukur time-to-first-token terpisah dari inference completion bila menguji AI.
3. Pilih satu hotspot yang punya contract stabil. Hindari billing/auth/purchasing
   sebagai latihan pertama; pilih pure computation tanpa side effect jika ada.
4. Bandingkan optimized Node baseline versus Rust release build pada hardware,
   input size, concurrency, output correctness dan error cases yang sama. Sertakan
   bridge/serialization cost, bukan hanya loop microbenchmark.
5. Jalankan minimal installed consumers dan exact behavior fixtures, lalu ukur
   clean/incremental build, feature-change effort dan target deployment matrix.
6. Tentukan go/no-go berdasarkan end-to-end improvement dan total maintenance
   cost. Full migration memerlukan bukti keuntungan melampaui satu hotspot.

Belum ada implementasi/pilot atau perubahan stack/PRD dalam riset ini. Keputusan
engineering yang didukung bukti saat ini adalah memperbaiki fondasi, mengukur
bottleneck, lalu mengadopsi Rust secara selektif bila hasilnya menguntungkan.
