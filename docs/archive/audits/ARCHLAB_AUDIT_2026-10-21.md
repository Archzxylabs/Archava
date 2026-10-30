# Audit Archava Platform — 21 Oktober 2026

**Verdict: platform-nya nyata dan jalan, tapi klaim release-nya belum sah.**
Runtime CRM lulus 44/44 pemeriksaan, unit suite 1.821 test hijau, dan typecheck/build
bersih. Di sisi lain **2 dari 8 root gate merah** (`format`, `lint`), gate Rust gagal
di lingkungan dengan `TMPDIR` panjang, satu jalur otorisasi lintas-actor masih
terbuka di 3 endpoint ERP, dan tiga temuan owner-configuration dari audit 20 Oktober
(F04–F06) **tidak diperbaiki sama sekali** — file owner tidak berubah sejak commit
`9b5c997`.

| Item         | Nilai                                                                                                           |
| ------------ | --------------------------------------------------------------------------------------------------------------- |
| Repo         | `/home/haikaru/Archverse/Lab/Archava_Platform`                                                                  |
| HEAD         | `412f763e17f3a68a457fd76c7b929184c50c916a` (sama dengan HEAD audit 20 Okt)                                      |
| Branch       | `feat/laundry-1.17.0`                                                                                           |
| Working tree | kotor: 27 file tracked berubah (`+1516 / −1160`) + 16 untracked                                                 |
| Skala        | 70 workspace project, 220.176 LOC TS+Rust (640 file), 1,5 GB di disk                                            |
| Metode       | eksekusi gate nyata, `pnpm audit`/`cargo audit`, probe Rust, pembacaan kode per-lensa (authz, injection, owner) |

Laporan pendukung di direktori yang sama:
`archlab-2026-10-21-authz.md`, `archlab-2026-10-21-injection.md`,
`archlab-2026-10-21-owner-claims.md`, `archlab-2026-10-21-supplychain.md`.
Artefak mentah: `~/.hermes/profiles/archlab/cache/scratch/archava-audit/`.

---

## 1. Gate matrix — hasil eksekusi, bukan kutipan dokumen

| Gate                           | Hasil          | Bukti                                                                                                                     |
| ------------------------------ | -------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `pnpm structure`               | **PASS**       | "70 workspace projects, every @archava/\* import declared"                                                                |
| `pnpm typecheck`               | **PASS**       | exit 0, seluruh package + `typecheck:web`                                                                                 |
| `pnpm test`                    | **PASS**       | **1.821 test / 137 file, 0 gagal, 0 skip**, 7,24 s                                                                        |
| `pnpm build`                   | **PASS**       | exit 0                                                                                                                    |
| `pnpm crm:verify-runtime`      | **PASS**       | **44/44** persistent runtime check                                                                                        |
| `pnpm reuse:verify`            | **PASS**       | dua consumer independen, versi+hash artefak core identik                                                                  |
| `pnpm format:check`            | **FAIL**       | exit 1 — **36 file** melanggar Prettier                                                                                   |
| `pnpm lint`                    | **FAIL**       | OOM heap default (exit 134); dengan heap 8 GB selesai dengan **49 error**                                                 |
| `pnpm rust:verify`             | **FAIL** (env) | `cargo test --workspace` exit 101: 11/11 test `archava-adapters --test tenant_context` gagal karena socket path >107 byte |
| `node tools/rust/coverage.mjs` | **FAIL**       | exit 1, `complete:false`, 93 item / 56 proof, **263 diagnostic**                                                          |

Klaim `docs/BUILD_STATUS.md` "all eight root gates passed: 1,786 tests in 134 files"
**tidak berlaku untuk tree ini**: jumlah test sudah 1.821/137, dan `pnpm verify` akan
berhenti di gate `format` sebelum sampai `test`.

### 1.1 Format — 36 file, sudah ter-commit

Mayoritas `tools/rust/*.mjs` dan `docs/rust/*`. Ini bukan efek working tree kotor:
`git status` tidak menyentuh file-file tersebut, jadi gate ini merah juga di HEAD.

### 1.2 Lint — dua masalah terpisah

1. **OOM**: `pnpm lint` mati di ~2 GB heap walau mesin punya 15 GB dan limit Node
   default 4.144 MB. Tanpa `NODE_OPTIONS=--max-old-space-size=8192`, gate tidak bisa
   selesai sama sekali.
2. **49 error** setelah heap dinaikkan: 15 `no-unsafe-assignment`, 9 `no-unsafe-call`,
   7 `no-undef`, 6 `no-unused-vars`, 5 `no-unsafe-return`, 4 `no-unsafe-argument`,
   1 `restrict-plus-operands`, 1 `no-unsafe-member-access`, 1 `no-unused-labels`.

**Akar masalah config**, bukan kode produk:

- Flat config memakai `js.configs.recommended` untuk `.mjs` tetapi tidak pernah
  mendefinisikan Node globals → `URL`, `process`, `Buffer` dianggap undefined
  (7 `no-undef`).
- `examples/**` tidak di-ignore dan tidak ada di `tsconfig` project, tetapi kena
  `recommendedTypeChecked` dengan `projectService: true` → semua `no-unsafe-*`
  muncul karena type information tidak tersedia.

### 1.3 Rust gate — gagal karena environment, bukan domain

`pg-fixture` menaruh Unix socket di `$TMPDIR`. Di sesi ini `TMPDIR` menunjuk path
panjang (`.../cache/scratch/...`), PostgreSQL menolak:

```
Unix-domain socket path ".../.s.PGSQL.37969" is too long (maximum 107 bytes)
FATAL: could not create any Unix-domain sockets
```

Rerun dengan `TMPDIR=/tmp/arv` membuat 11/11 test RLS itu hijau dan gate lanjut:
**24 suite / 555 test lulus, 0 gagal**, lalu proses berhenti mendadak di suite
`erp_scheduling` (7 dari 9 test tampil) tanpa baris `EXIT` — pola kill resource
(14 cluster PostgreSQL paralel + scrypt 32 MB per login), bukan test failure. **Fixture-nya tidak salah secara
domain, tapi tidak defensif**: komentar menyebut "short path", kodenya tidak
memvalidasi atau fallback ke `/tmp`, jadi satu variabel environment bisa memerahkan
seluruh gate.

Catatan biaya waktu: suite `erp_attendance` sendiri butuh **470 detik** untuk 14 test
(semuanya lulus). Setiap test membuat cluster PostgreSQL sendiri, jadi gate Rust
memang lambat secara struktural — bukan hang, tapi durasi ini perlu dianggarkan
sebelum dipakai sebagai gate PR.

### 1.4 Coverage

`pnpm test:coverage` (unit saja, bukan suite host/proof): **30,32% statement,
32,29% branch, 37,37% function, 31,05% line** dari 23.548 statement.
Tidak ada `thresholds` di `vitest.config.ts`, jadi angka ini tidak menjaga apa pun —
regresi coverage tidak akan menggagalkan gate.

---

## 2. Temuan

Severity: **P1** = dampak langsung ke uang/otorisasi/lintas tenant; **P2** = eksploitasi
butuh konteks terbatas atau merusak jaminan release; **P3** = hardening / drift / operasional.

### A1 — P1, confidence High: receipt command lintas-actor bisa dibaca lintas-grant (3 endpoint Rust + 7 domain TS)

Pola F03 audit 20 Oktober **belum ditutup di main**. Tiga handler meresolusi akses
lalu membuangnya:

- `rust/crates/host/src/erp/billing.rs:499` — `let _ = &access;`
- `rust/crates/host/src/erp/subscriptions.rs:543` — `let _ = &access;`
- `rust/crates/host/src/erp/attendance.rs:532` — `let _ = &access;`

Query receipt hanya memfilter tenant + request id, tanpa actor:

- `rust/crates/adapters/src/erp/subscriptions.rs:861-886`
  (`WHERE tenant_id=$1 AND request_id=$2`)
- `rust/crates/adapters/src/erp/attendance.rs:813-825` (idem)
- `rust/crates/adapters/src/erp/billing.rs:986-998` via `fetch_command_row(tenant, request_id)`

**Dampak.** Anggota tenant mana pun yang aktif — tanpa grant domain apa pun — bisa
membaca receipt command milik principal lain di tenant yang sama (input command,
ID subscription/attendance/billing) asal mengetahui UUID request id. Bukan akses
anonim dan bukan lintas tenant (RLS tetap bekerja), tetapi ini perbedaan otorisasi
antara read normal (butuh `subscriptions.read` dan sejenisnya) dan recovery
(cukup membership) — persis yang dicatat F03.

**Nuansa yang sudah diverifikasi:** implementasi TS-nya juga membaca receipt dengan
`tenant_id + request_id` di dalam `transaction(tenant, principal, ...)`
(`packages/attendance-pg/src/repository.ts:531-543`), jadi ini pola warisan dari
lane TypeScript — bukan regresi port Rust. Yang membuatnya tetap bug adalah
perbandingan dengan kontrol di domain lain dan dengan `/commands/status` CRM.

**Kontras yang membuktikan ini bug, bukan desain:** 9 domain ERP lain
(accommodation, assets, organizations, proposals, projects, scheduling, purchasing,
foodservice) meneruskan `&access`/principal ke fungsi status, dan
`/commands/status` CRM sudah dikeraskan (RN01: recovery menjawab ulang policy gate
dengan grant saat ini).

**Skala penuh (laporan authz):** dari **250 route entry** yang diperiksa (200 TS +
50 Rust), **30** adalah command-status read; hanya **TS 12/19** dan **Rust 2/11**
yang punya read-grant eksplisit dan/atau initiator binding. Tujuh domain TS mengulang
bentuk yang sama — subscriptions, attendance, billing, laundry, field-services,
formal-education, fulfillment (`packages/*-pg/src/repository.ts`, F-04 di
`archlab-2026-10-21-authz.md`).

**Fix.** Bind receipt ke `actor_id` dan jawab ulang gate baca saat recovery, seperti
`status_receipt()` di `rust/crates/host/src/router.rs:1346-1400`. Tambah test negatif:
actor B (tanpa grant baca) menanyakan request id milik actor A → harus 403/unknown.

### A2 — P1, confidence High: konfirmasi owner tidak mengikat biaya/readiness (F04 masih terbuka)

`canonical()` di `rust/crates/own/src/lib.rs:489-513` hanya memuat base revision,
revision, enabled set, label, changes, dan blocked result — **`effects` tidak masuk
identity**. `Confirmation` hanya `{planHash, baseRevision}` (`lib.rs:170-175`), dan
host hanya membandingkan dua field itu (`rust/crates/host/src/own.rs:406-410`).

Probe terhadap crate engine publik membuktikan dua catalogue yang berbeda hanya pada
`readiness`/`cost` menghasilkan hash identik dan konfirmasi lama tetap diterima:

```
cheap effects = Ready/Included   dear effects = NeedsSetup/Recurring
cheap hash = 1dab7835fe45184a   dear hash = 1dab7835fe45184a
old confirmation accepted = true
```

`CATALOGUE_VERSION`/`PRICING_VERSION` ditulis ke receipt tetapi tidak pernah
dibandingkan dengan yang dipegang klien. Tidak ada satu pun test yang memvariasikan
catalogue sambil mempertahankan request/base yang sama.

**Dampak.** Owner bisa meng-apply plan yang menampilkan biaya/readiness A sementara
katalog yang dieksekusi adalah B — persetujuan biaya berulang tidak benar-benar
mengikat apa yang dijalankan setelah catalogue berubah.

### A3 — P1, confidence High: owner apply tanpa operation identity (F05) dan restore tidak menghapus label (F06)

- **F05:** `rust/crates/slice/src/owner.rs:188-200` membuat `uuid_string()` baru pada
  setiap klik dan hanya menyimpan outcome di signal; `pending` module (yang menulis
  `localStorage`) tidak diimpor owner. Tidak ada route status/replay owner.
  Test `refreshRecovery` di `tools/rust/browser-slice.mjs:1460-1473` hanya reload
  **setelah apply sukses** — bukan response hilang.
- **F06:** planner restore mulai dari `current.terminology.clone()` lalu hanya
  menambahkan label (`rust/crates/own/src/lib.rs:448-461`); `Change` tidak punya
  varian hapus label. Probe: restore ke revision 1 (label `new` saja) menghasilkan
  `changes = []` dan label `won` tetap ada.

**Dampak.** Setelah network/500, user kehilangan identitas operasi yang sama
(tidak bisa recover, hanya bisa mengulang intent baru), dan restore konfigurasi
tidak mengembalikan terminology ke snapshot target.

### A4 — P2: evidence Rust tidak konsisten dan memburuk (F10)

`node tools/rust/coverage.mjs` → exit 1, `complete:false`. Audit 20 Okt: **177**
diagnostic. Hari ini: **223** (run subagent), lalu **263** setelah run
`rust:verify` saya yang gagal menulis `dist/rust-evidence/P-FAST-GATE.json`.
Angka ini pesan validator, bukan 263 bug.

Drift dokumen yang masih hidup:

- `docs/rust/EXECUTION_STATE.md:23` menyebut 53 proof; manifest berisi **56**.
- `:33` baseline HEAD `409eab9`; aktual `412f763`.
- `:38` menyebut `PACKET-000…007`; file yang sama di `:958` sudah membahas `PACKET-010`.
- `docs/rust/FULL_RUST_COMPLETION_REPORT.md:4` juga masih 53.
- `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:28` menyatakan C03/OWN-T01–T10
  "locally verified", padahal A2/A3 membuktikan OWN-T06/T08/T09 belum.

Catatan: `dist/library/artifacts.json` = **62** artefak vs archive 1.17.0 = **58** —
klaim `BUILD_STATUS.md` soal archive yang tidak match itu **akurat** (A07 memang
sengaja dibiarkan terbuka, bukan salah hitung).

### A5 — P3: fixture PostgreSQL tidak tahan `TMPDIR` panjang

Lihat §1.3. Fix: pilih socket dir pendek (`/tmp`) atau bind TCP saja, dengan error
yang menyebut penyebabnya. Ini akan menghemat satu sesi debugging bagi siapa pun
yang menjalankan gate dari sandbox/CI dengan `TMPDIR` panjang.

### A6 — P3: coverage tanpa threshold

30% line coverage bukan angka yang bisa dipertahankan untuk platform yang menjual
auditabilitas. Tidak ada `thresholds`, jadi tidak ada gate. Minimal: threshold per
package untuk `packages/*/src` dengan ratchet, atau tandai suite host/proof sebagai
sumber coverage utama agar angkanya tidak menyesatkan.

### A7 — P3: unit systemd stale dan service mati

`archava-public-api.service` dan `archava-voice-worker.service` **inactive +
disabled**. `WorkingDirectory` unit public API menunjuk
`/home/haikaru/Archverse/Lab/Archava_onchain` yang **sudah tidak ada** (project
pindah ke `Lab/Hackathon/Archava_onchain`). Kalau di-enable, service langsung gagal.
Tidak ada deployment publik yang bisa diaudit dari mesin ini.

### A8 — P3 (dari laporan injection): 7 hardening item, 0 P1/P2

Ringkas: cookie host Rust tanpa `Secure`/`__Host-` (F-01), header CSP/HSTS/nosniff
tidak dipasang di host Rust + `apps/web` + template padahal CRM host TS sudah punya
(F-02), `Math.random()` untuk identifier otorisasi/demo (F-03/F-05), helper HTML
tanpa escape (F-04, latent), password fixture dicetak (F-06), token sesi mentah di
body login (F-07). Detail dan bukti ada di `archlab-2026-10-21-injection.md`.

### A9 — P2 (dari laporan supply chain): vite 7.3.6 + tidak ada audit terjadwal

`pnpm audit`: 334 dependency → 1 high + 2 moderate, semuanya `vite@7.3.6` (dev-only,
Windows `launch-editor`); `--prod` 89 dependency → **0 advisory**. `cargo audit`:
327 crate → **0 vulnerability**, 1 unmaintained (`paste`, proc-macro via Leptos).
Lockfile 263/263 ber-integrity sha512, 0 dependency non-registry, 0 install hook
aktif. Yang kurang: **tidak ada workflow `schedule`/Dependabot** dan Actions masih
di tag mutable (`@v4`, `@stable`). Fix vite tersedia (7.3.7).

### A10 — P2: scope `assigned` tidak ditegakkan di billing/attendance/laundry (dan sub-resource subscriptions)

`scope='assigned'` disimpan di membership dan dipakai domain lain (subscriptions
detail, field-services, formal-education), tetapi:

- billing: `listInvoices`/`getInvoiceDetail`/`listAttempts`
  (`packages/billing-pg/src/repository.ts:726-864`) hanya filter `tenant_id`, bukan `owner_id`;
- attendance: empat read (`:555-766`) idem;
- laundry: dua read (`:613-700`) idem;
- subscriptions: `listEntitlements`/`listUsageEvents` (`:714-766`) melewati cek scope
  yang justru dipakai `getSubscriptionDetail` (`:696-698`).

Efek: operator dengan scope `assigned` bisa membaca invoice/sesi/order/entitlement
milik operator lain di tenant yang sama bila mengetahui id-nya. Confidence Medium
(semantik `assigned` disimpulkan dari kontrak dan dari domain yang menegakkannya).

### A11 — P2/P3: sisa temuan authz

- **P2** — status read organizations/projects/proposals/scheduling tidak memeriksa
  read grant; organizations bahkan membuang actor
  (`rust/crates/adapters/src/erp/organizations.rs:739-760` hanya `access.sees(owner)`).
- **P3** — throttle login host Rust memakai satu bucket global `LOGIN_SOURCE`
  (`router.rs:96`, `:1740`): 60 percobaan gagal dari mana pun bisa menutup sign-in
  semua user selama 5 menit. TS host memakai `remoteAddress`.
- **P3** — tenant-existence oracle: mount domain menjawab 404 untuk tenant tak dikenal
  vs 403 untuk tenant dikenal-tapi-bukan-member (`apps/crm/src/host.ts:186-194` × 19).
  Tenant id didokumentasikan non-rahasia, jadi dampaknya rendah.

Detail: `archlab-2026-10-21-authz.md` F-05…F-10.

---

## 3. Yang benar-benar terbukti kuat

- **Otorisasi terpusat di CRM host TS** (`apps/crm/src/host.ts`): origin check,
  session, CSRF untuk semua POST, tenant existence + `service.access()` per blok,
  dan security header lengkap (`nosniff`, `no-referrer`, `no-store`, CSP dengan
  `frame-ancestors 'none'`, HSTS saat origin HTTPS).
- **Kripto identitas**: scrypt 32768/8/3 dengan `maxmem`, `randomBytes(32)`,
  `timingSafeEqual`, CSRF HMAC-derived, token sesi disimpan sebagai digest.
- **Rust authority module** (`rust/crates/host/src/authority.rs`): request body tidak
  pernah menjadi sumber tenant/role/grant/approval; grant dibaca dari membership row;
  approval terikat ke command; error sengaja tidak membedakan tenant/principal.
- **Recovery CRM `/commands/status`** sudah menjawab ulang policy gate dengan grant
  saat ini (RN01) — inilah pola yang harus dipakai A1.
- **CRM runtime** 44/44: restore, backup, CSRF, forged authority, payload drift,
  replay, cross-tenant, dan spoofed forwarding header semuanya ditolak/tertangani.
- **Dua consumer independen** (`pnpm reuse:verify`) lulus dengan hash artefak core
  identik — bukti reuse artefak, bukan sekadar import workspace.
- **Hygiene supply chain**: exact pin, `--frozen-lockfile`, 100% integrity, tidak ada
  lifecycle script aktif, `.env.example` hanya placeholder, tidak ada `.env` tracked.
- **Injection surface**: 0 `dangerouslySetInnerHTML`/`eval`/`new Function`, 0 HTTP
  client di Rust, 0 `axios`/`got`, 54 query SQL diperiksa tanpa string interpolation
  input, identifier `GRANT` divalidasi regex ber-anchor.

---

## 4. Urutan perbaikan yang saya rekomendasikan

| #   | Aksi                                                                                                                          | Alasan                                                                             |
| --- | ----------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------- |
| 1   | Tutup A1: bind receipt ke actor + read grant di 3 endpoint Rust **dan** 7 repository TS, plus test negatif                    | Celah otorisasi paling luas: 30 status endpoint, hanya 14 yang punya grant/binding |
| 2   | Terapkan predikat scope `assigned` di billing/attendance/laundry + entitlements/usage subscriptions (A10)                     | Operator `assigned` bisa membaca data operator lain                                |
| 3   | Perbaiki gate `format` (36 file) dan `lint` (Node globals + `examples/**` di luar project)                                    | Tanpa ini `pnpm verify`/CI merah dan klaim release tidak bisa dipakai              |
| 4   | F04: masukkan `effects` + catalogue/pricing digest ke `canonical()`/`Confirmation`                                            | Persetujuan biaya harus mengikat apa yang dieksekusi                               |
| 5   | F05/F06: pakai `pending::PendingMarker` untuk owner + route status/replay; pisahkan patch vs replace terminology saat restore | Recovery dan restore saat ini tidak memenuhi mandat                                |
| 6   | Regenerasi evidence Rust pada kandidat final; update `docs/rust/*` dari manifest 56 proof                                     | 263 diagnostic dan dokumen 53 proof saling bertentangan                            |
| 7   | `vite` override 7.3.7 + workflow audit terjadwal + Dependabot                                                                 | Menutup P2 supply chain                                                            |
| 8   | Read grant di status organizations/projects/proposals/scheduling + throttle login per-source di host Rust (A11)               | Sisa P2/P3 authz                                                                   |
| 9   | Header keamanan + cookie `Secure`/`__Host-` di host Rust & `apps/web`                                                         | P3 hardening, murah, menutup jalur cleartext                                       |
| 10  | Threshold coverage minimal per package                                                                                        | 30% line tanpa gate tidak menjaga apa pun                                          |
| 11  | Rapikan unit systemd stale atau hapus kalau sudah tidak dipakai                                                               | Operasional: jangan tinggalkan unit yang menunjuk path hilang                      |

---

## 5. Batasan audit

- `pnpm security:verify --run` (matriks penuh 20+ langkah) **tidak** dijalankan;
  yang dijalankan gate root, runtime CRM, audit dependency, dan probe terarah.
- Tidak ada live provider call (Gemini/email/payment/LiveKit).
- Tidak ada pengujian beban, fuzzing, atau serangan jaringan ke host eksternal.
- Lensa authz selesai secara **statis**: 250 route entry diperiksa (200 TS + 50
  Rust), 245 di antaranya butuh sesi. Verdict berasal dari kode, bukan reproduksi
  runtime. Laporan: `archlab-2026-10-21-authz.md`.
- Angka coverage berasal dari unit suite (`pnpm test:coverage`), bukan dari suite
  host/proof yang dijalankan terpisah; jangan membacanya sebagai coverage produk.
- `dist/` dan `node_modules/` tidak diaudit isinya; `dist/` gitignored dan bisa
  berubah setiap kali gate dijalankan.
