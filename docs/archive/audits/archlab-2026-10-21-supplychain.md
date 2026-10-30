# Supply Chain & Dependency Audit — Archava_Platform

- **Tanggal**: 2026-10-21 14:45 WIB (+0700)
- **Auditor**: Archlab (automated, Hermes)
- **Repo**: `/home/haikaru/Archverse/Lab/Archava_Platform`
- **Commit**: `412f763` + 27 file uncommitted. Audit mencerminkan **working tree**, bukan HEAD. Tiga file terkait dependency berstatus modified: `package.json` (hanya penambahan script `erp:*`/`formal-education:*`), `apps/crm/package.json` (+`workspace:*` untuk `formal-education`, `formal-education-pg`), dan `pnpm-lock.yaml` (+28 baris, menambahkan importer `packages/formal-education{,-pg}`). Tidak ada perubahan versi dependency eksternal di working tree.
- **Tooling**: pnpm 10.15.1, Node 22.23.2, cargo 1.96.0, cargo-audit 0.22.2, cargo-deny 0.20.2
- **Advisory DB (RustSec)**: 1348 advisory, `last-updated 2026-10-21T08:20:38+02:00`
- **Artefak mentah**: `/home/haikaru/.hermes/profiles/archlab/cache/scratch/archava-audit/`
  (`pnpm-audit-full.json`, `pnpm-audit-prod.json`, `pnpm-outdated.json`, `pnpm-outdated-recursive.json`,
  `cargo-audit.json`, `cargo-metadata.json`, `pnpm-licenses.json`, `pnpm-lock.before.yaml`)
- **Tidak ada perubahan** pada `pnpm-lock.yaml`, `Cargo.lock`, maupun `node_modules` — diverifikasi via hash sebelum/sesudah.

---

## 0. Ringkasan Eksekutif

| #   | Temuan                                                                               | Severity | Dampak                                          | Fix tersedia                     |
| --- | ------------------------------------------------------------------------------------ | -------- | ----------------------------------------------- | -------------------------------- |
| 1   | `vite@7.3.6` — 1 high + 2 moderate (launch-editor, Windows-only)                     | **P2**   | Dev tooling saja; `pnpm audit --prod` = 0       | Ya — `vite 7.3.7`                |
| 2   | Tidak ada vulnerability gate otomatis (CI tanpa `cargo audit`/SCA, tanpa Dependabot) | **P2**   | Advisory baru masuk tanpa terdeteksi            | Ya — workflow + `dependabot.yml` |
| 3   | `paste@1.0.15` unmaintained (RUSTSEC-2024-0436)                                      | P3       | Build-time proc-macro, tanpa CVE                | Belum (hulu: Leptos)             |
| 4   | 16 crate punya >1 versi (dua stack RustCrypto paralel)                               | P3       | Permukaan advisory & ukuran binary naik         | Sebagian (tunggu sqlx/leptos)    |
| 5   | GitHub Actions di-pin ke tag/branch mutable (`@v4`, `@stable`)                       | P3       | Vektor supply-chain klasik                      | Ya — pin ke commit SHA           |
| 6   | 16 crate Rust + semua package TS tanpa field `license`                               | P3       | Blocker publishing, bukan risiko legal saat ini | Ya — tambah metadata             |
| 7   | 12 devDependency outdated (TypeScript 5→7, ESLint 9→10, Vitest 4→5)                  | P3       | Drift mayor, tanpa auto-drift (semua exact pin) | Ya — upgrade terencana           |

**Tidak ada temuan P1.** Tidak ada critical vulnerability, tidak ada dependency non-registry di manifest yang di-track, dan lockfile terverifikasi 100% berintegritas.

Angka kunci:

- `pnpm audit` full: **334** dependency, **0 critical / 1 high / 2 moderate / 0 low**
- `pnpm audit --prod`: **89** dependency produksi, **0 advisory**
- `cargo audit`: **327** crate, **0 vulnerability**, **1 warning** (unmaintained)
- Lockfile: **263** package entry, **263/263** punya `integrity: sha512-…`, **0** resolusi `tarball:`/`git:`

---

## 1. `pnpm audit --json`

### 1.1 Perintah & angka asli

```bash
pnpm audit --json > pnpm-audit-full.json     # exit 1 (ada advisory)
pnpm audit --prod --json > pnpm-audit-prod.json  # exit 0
```

```json
// full
"vulnerabilities": { "info": 0, "low": 0, "moderate": 2, "high": 1, "critical": 0 },
"dependencies": 334, "devDependencies": 0, "optionalDependencies": 0, "totalDependencies": 334

// --prod
"vulnerabilities": { "info": 0, "low": 0, "moderate": 0, "high": 0, "critical": 0 },
"dependencies": 89, "totalDependencies": 89
```

Catatan: `dependencies: 334` pada run full adalah seluruh graph (pnpm menghitung semua sebagai dependencies), sedangkan `--prod` memfilter ke **89** paket runtime nyata. **Tidak ada satu pun advisory yang menyentuh jalur produksi.**

### 1.2 Daftar paket rentan

Semua tiga advisory menunjuk **satu paket dan satu versi yang sama**.

| Paket  | Versi terpasang | Rentan            | Patched   | Severity | Advisory                                                                                               | CWE    |
| ------ | --------------- | ----------------- | --------- | -------- | ------------------------------------------------------------------------------------------------------ | ------ |
| `vite` | 7.3.6           | `>=7.0.0 <=7.3.6` | `>=7.3.7` | **high** | GHSA-587h-7wmj-h5pm — `fs.existsSync()` existence check can be bypassed on Windows                     | CWE-77 |
| `vite` | 7.3.6           | `>=7.0.0 <=7.3.6` | `>=7.3.7` | moderate | GHSA-r825-w8mm-5pxw — Editor CLI flag misinterpretation → arbitrary command execution for some editors | CWE-88 |
| `vite` | 7.3.6           | `>=7.0.0 <=7.3.6` | `>=7.3.7` | moderate | GHSA-698v-4r3x-3vcv — `escapeCmdArgs()` omits `%` escaping on Windows                                  | CWE-78 |

### 1.3 Jalur dependensi

```
. (root, devDependencies)
└─ vitest@4.1.11
   └─ vite@7.3.6   ← satu-satunya instance di lockfile
```

- `pnpm audit` melaporkan path `'.>vitest>vite'` — yaitu root importer → `vitest` (devDependency) → `vite`.
- `vite` **tidak** dipakai langsung oleh `apps/web` atau package lain; ia murni mesin test di bawah `vitest`.
- Seluruh **71 workspace project** me-resolve instance `vite@7.3.6` yang sama (`vitest@4.1.11(...)(vite@7.3.6(...))`), jadi tidak ada duplikasi versi vite.

### 1.4 Apakah fix tersedia

**Ya.** Diverifikasi langsung ke registry:

```
GET https://registry.npmjs.org/vite/7.3.7
  version: 7.3.7 | deprecated: null
  integrity: sha512-AHEzTORMbi7/7aXp7w7KZ5+DCUdSeLsJz0grY8xHpc9DHdYb0vQu6DqJfm2tOw3gDExNgPgnJTXxM6syYFHaCA==
GET https://registry.npmjs.org/vitest/4.1.11
  dependencies.vite: ^6.0.0 || ^7.0.0 || ^8.0.0
```

Karena `vitest@4.1.11` menerima `vite ^7.x`, **tidak perlu menaikkan vitest** — cukup override vite. `vite` terbaru saat audit adalah **8.3.5** (major bump, jangan dipaksa tanpa uji).

### 1.5 Penilaian dampak (kenapa P2, bukan P1)

Ketiga advisory adalah isu **`launch-editor`**, komponen yang hanya aktif saat dev server dijalankan dan dipicu untuk membuka editor (`--open`, atau tombol `e`). Eksploitasinya **Windows-only** dan memerlukan input yang dikendalikan penyerang pada argumen editor. Tidak ada jalur dari ini ke runtime produksi: `pnpm audit --prod` bersih (0/89). Jadi:

- Bukan P1 (tidak reachable di produksi, tidak ada data/tenant terpapar).
- Tetap P2 karena (a) severity "high" akan memicu gate keamanan mana pun, (b) fix-nya satu baris dan sudah tersedia, (c) mesin developer Windows adalah tempat kredensial repo berada.

### 1.6 Rekomendasi

Prioritas 1 — pin melalui `overrides` yang sudah ada di `pnpm-workspace.yaml`:

```yaml
overrides:
  source-map-js: '1.2.2'
  vite: '7.3.7' # GHSA-587h-7wmj-h5pm, GHSA-r825-w8mm-5pxw, GHSA-698v-4r3x-3vcv
```

lalu `pnpm install` (lockfile berubah — memang harus) dan jalankan `pnpm test` + `pnpm build`.

Alternatif jika ingin menghindari override: naikkan `vitest` ke `5.0.5` (sudah tersedia, lihat §3) yang menarik vite lebih baru. Ini perubahan mayor pada test runner — lebih mahal, jadi override lebih dulu.

---

## 2. `cargo audit`

### 2.1 Perintah & hasil

```bash
cd rust && cargo audit --json
# exit 0
```

```
Fetching advisory database from https://github.com/RustSec/advisory-db.git
Loaded 1348 security advisories
Scanning Cargo.lock for vulnerabilities (327 crate dependencies)

Crate:     paste
Version:   1.0.15
Warning:   unmaintained
Title:     paste - no longer maintained
Date:      2024-10-07
ID:        RUSTSEC-2024-0436
URL:       https://rustsec.org/advisories/RUSTSEC-2024-0436

warning: 1 allowed warning found
```

| Metrik                | Nilai       |
| --------------------- | ----------- |
| Dependency dipindai   | 327         |
| Vulnerabilities       | **0**       |
| Warnings unmaintained | 1 (`paste`) |
| Warnings unsound      | 0           |
| Warnings notice       | 0           |
| Yanked crates         | 0           |

### 2.2 Lockfile sekunder (di luar workspace root)

Repo punya dua `Cargo.lock` lain yang tidak ikut scan root:

| Lockfile                                   | Dependency | Vuln | Warning |
| ------------------------------------------ | ---------- | ---- | ------- |
| `tools/rust/benchmark/rust/Cargo.lock`     | 25         | 0    | 0       |
| `examples/rust-consumers/crate/Cargo.lock` | 19         | 0    | 0       |

Keduanya bersih.

### 2.3 `paste` — jalur dependensi

```
paste v1.0.15 (proc-macro)
├── either_of v0.1.9
│   └── leptos v0.8.22
├── leptos v0.8.22
│   └── archava-slice v0.1.0 (crates/slice)
├── reactive_graph v0.2.16
│   ├── leptos v0.8.22
│   └── reactive_stores v0.4.4
└── tachys v0.2.19
```

Semua jalur berujung di stack render **Leptos** (`crates/slice`, UI SSR/WASM). `paste` adalah proc-macro: ia berjalan saat kompilasi, **tidak ada di binary hasil build** dan tidak menerima input runtime. RUSTSEC-2024-0436 adalah status "no longer maintained", **bukan** CVE/kerentanan.

### 2.4 Rekomendasi

1. **Jangan panic-fix.** Tidak ada eksposur runtime.
2. Catat sebagai accepted risk dengan justifikasi, supaya `cargo audit` di CI tidak berisik:
   ```toml
   # rust/audit.toml
   [advisories]
   ignore = ["RUSTSEC-2024-0436"]  # paste: unmaintained, proc-macro build-time only, via leptos 0.8
   ```
3. Track kenaikan `leptos` (0.8.22 → rilis berikutnya) yang mengganti `paste` dengan `pastey`; saat itu tiba, hapus ignore-nya.

---

## 3. `pnpm outdated`

```bash
pnpm outdated --format json              # exit 1, 12 paket
pnpm -r outdated --format json           # set unik sama: 12 paket
```

Semua **devDependencies** dengan **exact pin** (tidak ada `^`/`~`), jadi tidak ada auto-drift — upgrade sepenuhnya keputusan manusia.

| Paket                       | Current | Latest  | Lonjakan     |
| --------------------------- | ------- | ------- | ------------ |
| `typescript`                | 5.9.3   | 7.0.2   | **major ×2** |
| `eslint`                    | 9.37.0  | 10.13.0 | major        |
| `@eslint/js`                | 9.37.0  | 10.0.1  | major        |
| `vitest`                    | 4.1.11  | 5.0.5   | major        |
| `@vitest/coverage-v8`       | 4.1.11  | 5.0.5   | major        |
| `eslint-plugin-react-hooks` | 5.2.0   | 7.1.1   | major ×2     |
| `@types/node`               | 24.9.1  | 26.6.5  | major ×2     |
| `typescript-eslint`         | 8.70.1  | 8.72.0  | minor        |
| `prettier`                  | 3.6.2   | 3.9.10  | minor        |
| `tsx`                       | 4.20.6  | 4.23.15 | minor        |
| `zod`                       | 4.1.12  | 4.6.7   | minor        |
| `esbuild`                   | 0.25.12 | 0.28.2  | minor        |

Tidak ada dependency **runtime** yang outdated — 89 paket produksi semuanya pada versi terbaru yang kompatibel.

### Rekomendasi

- Kelompokkan upgrade: (a) minor/patch (`zod`, `prettier`, `tsx`, `typescript-eslint`, `esbuild`) → satu PR; (b) major `eslint 10 + @eslint/js 10 + eslint-plugin-react-hooks 7` → satu PR terpisah (flat-config migration); (c) `typescript 7` dan `vitest 5` → PR masing-masing, setelah (a)/(b) hijau.
- Khusus `esbuild`: root mem-pin `0.25.12`, sementara `vite@7.3.6` menuntut `^0.28.2` → dua salinan terpasang (`0.25.12` dan `0.28.2`). Samakan root ke `0.28.2` agar satu binary saja.

---

## 4. Install / postinstall scripts (potensi supply-chain RCE)

### 4.1 Manifest workspace

```bash
git ls-files '*package.json' | wc -l   # 76 manifest di-track
```

Hasil scan untuk `preinstall`, `install`, `postinstall`, `prepare`, `prepublish`, `prepublishOnly`:

```
install-script hooks: 0
```

**Nol.** Tidak ada satu pun package.json workspace yang mengeksekusi script saat install. Tidak ada `.npmrc` di repo (0 file), jadi tidak ada `ignore-scripts=false`, `enable-pre-post-scripts`, atau override registry.

### 4.2 Dependency tree terpasang

Scan 263 package.json di `node_modules/.pnpm/**`:

```
installed package.json scanned: 263
packages WITH install hooks: 19
```

Rincian 19 temuan itu:

| Jenis hook    | Jumlah                                                                                                                                                                                                     | Dieksekusi saat install?                                                                       |
| ------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------- |
| `postinstall` | 1 paket (`esbuild@0.25.12` **dan** `esbuild@0.28.2` — dua versi)                                                                                                                                           | **Tidak** — diblokir pnpm 10                                                                   |
| `prepare`     | 17 paket (`@eslint/eslintrc`, `@humanfs/*`, `acorn`, `balanced-match`, `brace-expansion`, `eslint-visitor-keys`, `globals`, `istanbul-reports`, `keyv`, `minimatch`, `rollup`, `tinyexec`, `ts-api-utils`) | **Tidak** — `prepare` hanya jalan untuk git dependency / package lokal, bukan tarball registry |

Verifikasi blokir:

```bash
pnpm ignored-builds
# Automatically ignored builds during installation:
#   esbuild
# hint: To allow the execution of build scripts for a package, add its name to
#       "pnpm.onlyBuiltDependencies" in your "package.json", then run "pnpm rebuild".
```

Ini adalah postur yang **baik**: pnpm 10 memblokir seluruh lifecycle script secara default, dan satu-satunya yang punya `postinstall` nyata (`esbuild`) memang sengaja tidak di-approve. Permukaan RCE install-time saat ini **nol**.

### 4.3 Catatan & rekomendasi

- **Jangan** menambahkan `pnpm.onlyBuiltDependencies: ["*"]` atau `dangerouslyAllowAllBuilds: true`. Kalau `esbuild` perlu binary-nya, tambahkan **hanya** `"esbuild"` ke allowlist — jangan pernah wildcard, karena wildcard akan menghidupkan `prepare` milik paket lain juga di masa depan.
- Binary platform esbuild sudah tersedia sebagai optional dependency (`@esbuild/linux-x64` dll.) sehingga build tetap jalan tanpa `postinstall`; blokir ini tidak mematahkan build.
- Tambahkan guardrail di CI: `pnpm install --frozen-lockfile --ignore-scripts` (saat ini CI memakai `pnpm install --frozen-lockfile` tanpa `--ignore-scripts`, jadi bila kelak ada paket baru ber-`postinstall` yang di-approve, ia langsung jalan di runner dengan akses ke secrets).

---

## 5. Dependency non-registry (git / file / link)

### 5.1 package.json (76 manifest di-track)

```
non-registry (excluding workspace:): 0
workspace:* references: 892
```

- **0** dependency `git+`, `github:`, `gitlab:`, `bitbucket:`, `http(s)://`, `file:`, `link:`, atau alias `npm:`.
- 178 referensi `workspace:*` di 76 manifest yang di-track — semuanya internal monorepo, bukan sumber eksternal. (Angka 892 muncul bila direktori `examples/` yang tidak di-track ikut dipindai.) Semua 76 manifest juga `private: true`, jadi tidak ada jalur publish tak sengaja.
- **0** `peerDependencies` di seluruh manifest, jadi tidak ada range peer yang bisa memaksa resolusi liar.

Catatan artefak (bukan temuan): `dist/crm-runtime/package.json` dan `dist/releases/*/runtime/package.json` berisi dependency `file:/home/haikaru/.../tarballs/*.tgz` dengan **path absolut**. Ini murni hasil generate packaging, dan `dist/` **gitignored** (`git ls-files dist` = 0 file), jadi tidak masuk repo. Hanya perlu diingat: artefak `dist/` tidak reproducible di mesin lain karena path absolut tersebut.

### 5.2 Cargo.toml (20 manifest di-track)

```
GIT DEPS: 0
PATH DEPS: 31 | absolute: 0
path deps resolving OUTSIDE repo: 0
```

- **0** dependency `git =`.
- 31 dependency `path =` — semuanya menunjuk ke dalam repo: 8 ke workspace member (`crates/*`), sisanya ke crate hasil vendoring di `examples/*/deps/`. Tidak ada path absolut, tidak ada yang keluar dari repo.
- `Cargo.lock`: **0** package ber-`source = "git+…"`, **311** package registry semuanya punya `checksum`, **16** package `path` (workspace member) tanpa `source` — sesuai harapan.

---

## 6. Lockfile pinning & integritas

### 6.1 `pnpm-lock.yaml`

Konsistensi manifest ↔ lockfile diuji tanpa instalasi penuh:

```bash
sha256sum pnpm-lock.yaml > lock-before.sha256
pnpm install --frozen-lockfile --lockfile-only --ignore-scripts
# Scope: all 71 workspace projects
# Done in 482ms using pnpm v10.15.1
# exit 0
sha256sum -c lock-before.sha256
# pnpm-lock.yaml: OK
```

**Konsisten.** Lockfile tidak berubah satu byte selama audit; tidak ada manifest yang menuntut resolusi baru. (Lockfile memang berbeda dari HEAD — +28 baris untuk importer `packages/formal-education{,-pg}` — tapi itu perubahan working tree yang sudah benar dan self-consistent, terbukti oleh `--frozen-lockfile` yang lolos.)

Struktur lockfile (`lockfileVersion: '9.0'`):

| Properti                                         | Nilai                  |
| ------------------------------------------------ | ---------------------- |
| Package entry (`packages:`)                      | 263                    |
| Entry dengan `resolution: {integrity: sha512-…}` | **263 / 263 (100%)**   |
| Entry tanpa integrity                            | **0**                  |
| Resolusi `tarball:` (non-registry)               | **0**                  |
| Snapshot (`snapshots:`)                          | 172                    |
| Importers (workspace project)                    | 71                     |
| Overrides                                        | `source-map-js: 1.2.2` |
| `excludeLinksFromLockfile`                       | `false`                |

Tidak ada `registries:` custom dan tidak ada URL registry non-default — semua mengarah ke registry default pnpm/npm, dengan hash SRI penuh. Ini adalah postur integritas yang kuat: paket yang di-swap di registry akan gagal verifikasi.

### 6.2 `Cargo.lock`

```bash
cargo metadata --locked --format-version 1   # exit 0
# packages: 327 | workspace members: 16
```

`--locked` gagal bila lockfile butuh update; exit 0 berarti **konsisten** dengan seluruh `Cargo.toml`.

| Properti                         | Nilai     |
| -------------------------------- | --------- |
| Package                          | 327       |
| Workspace member                 | 16        |
| Registry package dengan checksum | 311 / 311 |
| Package `git+`                   | 0         |
| Package `path` (tanpa source)    | 16        |
| Wildcard `"*"` di Cargo.toml     | 0         |

### 6.3 Range liar di runtime deps

```
RANGE/WILDCARD DEPS (^ ~ * latest) di 76 manifest: 0
```

**Nol.** Seluruh dependency eksternal dipin ke versi eksak (`"eslint": "9.37.0"`, `"vitest": "4.1.11"`, dst.), termasuk devDependencies. Ini praktik terbaik untuk reproducibility — tidak ada `^` yang bisa menarik patch berbahaya tanpa review, dan tidak ada `latest`.

### 6.4 Rekomendasi

- Pertahankan exact pin + `--frozen-lockfile` di CI (sudah dilakukan).
- Tambahkan `--ignore-scripts` pada langkah install CI (lihat §4.3).
- Pertimbangkan `pnpm audit --prod --audit-level high` sebagai step CI wajib; saat ini tidak ada.
- Untuk Rust, tambahkan `cargo metadata --locked` (murah, offline-friendly, exit 0/1) ke gate CI sebagai pengganti `cargo audit` yang butuh jaringan.

---

## 7. Lisensi

### 7.1 JavaScript (`pnpm licenses list --json`)

`pnpm licenses list --json` → 181 paket unik (union prod+dev; 263 entri terpasang termasuk duplikasi versi & varian peer). Subset produksi (`--prod`): **18 paket → 16 MIT + 2 ISC**. Tujuh grup lisensi, semuanya permisif:

| Lisensi       | Jumlah paket    |
| ------------- | --------------- |
| MIT           | 145             |
| Apache-2.0    | 13              |
| ISC           | 10              |
| BSD-2-Clause  | 6               |
| BSD-3-Clause  | 5               |
| Python-2.0    | 1 (`argparse`)  |
| BlueOak-1.0.0 | 1 (`minimatch`) |

**Tidak ada** GPL/AGPL/LGPL/SSPL/CC-BY-NC di graph JS. `Python-2.0` dan `BlueOak-1.0.0` keduanya permisif dan kompatibel dengan distribusi komersial.

### 7.2 Rust (`cargo metadata`, 327 package)

Distribusi ekspresi lisensi:

| Lisensi                                               | Jumlah |
| ----------------------------------------------------- | ------ |
| `MIT OR Apache-2.0`                                   | 154    |
| `MIT`                                                 | 78     |
| `Apache-2.0 OR MIT`                                   | 20     |
| `Unicode-3.0`                                         | 18     |
| `MIT/Apache-2.0`                                      | 10     |
| `Zlib`                                                | 6      |
| `Unlicense OR MIT`                                    | 4      |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 3      |
| lainnya (BSD, ISC, CC0, BSL-1.0, Zlib OR …)           | 17     |

Satu-satunya yang mengandung istilah copyleft:

```
r-efi 5.3.0  : MIT OR Apache-2.0 OR LGPL-2.1-or-later
r-efi 6.0.0  : MIT OR Apache-2.0 OR LGPL-2.1-or-later
```

Karena berbentuk **OR**, lisensi MIT/Apache-2.0 dapat dipilih — **tidak ada kewajiban LGPL** yang menempel. `r-efi` hanya dipakai jalur UEFI (target non-Linux), jadi praktis tidak relevan untuk deployment ini.

**Kesimpulan: tidak ada lisensi bermasalah.**

### 7.3 Temuan metadata

**16 crate workspace tidak punya field `license`** di `Cargo.toml`:

`archava-adapters`, `archava-assembly`, `archava-assistant`, `archava-capacity`, `archava-context`, `archava-core`, `archava-domain`, `archava-host`, `archava-native`, `archava-ops`, `archava-own`, `archava-pg-fixture`, `archava-pricing`, `archava-providers`, `archava-rates`, `archava-slice`.

Package TS juga tidak punya `license` (contoh: `@archava/web`, `@archava/core` — `license: None`, `private: true`).

Ini **bukan risiko legal** selama artefak tidak dipublikasikan, tapi akan menggagalkan publish ke crates.io/npm dan membuat SBOM tidak lengkap. Rekomendasi: tambahkan `license = "UNLICENSED"` (proprietary) atau ekspresi SPDX yang disepakati, dan `publish = false` untuk crate yang tidak untuk dipublikasi.

---

## 8. Temuan tambahan: governance supply chain

### 8.1 Tidak ada gate keamanan otomatis di CI — **P2**

Satu-satunya workflow adalah `.github/workflows/ci.yml`, dan `cargo audit` sengaja **tidak** dipasang:

```yaml
# .github/workflows/ci.yml:46
# `cargo audit` is deliberately NOT installed here. It needs its advisory
# database from the network and takes no `--offline` flag ...
# so it belongs on a schedule against a warmed cache rather than in a PR gate
```

Alasannya valid untuk PR gate, **tetapi schedule-nya tidak ada**. Tidak ada workflow `schedule:`, dan tidak ada `.github/dependabot.yml`. Konsekuensinya: advisory baru (seperti tiga GHSA vite di atas) hanya terdeteksi kalau seseorang menjalankan audit manual — persis yang baru saja terjadi.

Rekomendasi:

1. Workflow terjadwal (harian) yang menjalankan `pnpm audit --prod --audit-level high` dan `cargo audit`, dengan cache `~/.cargo/advisory-db` supaya murah:
   ```yaml
   on:
     schedule: [{ cron: '0 2 * * *' }]
     workflow_dispatch:
   ```
2. Tambahkan `.github/dependabot.yml` untuk `npm`, `cargo`, dan `github-actions` (mingguan, grouped).
3. Di PR gate, tambahkan yang tidak butuh jaringan: `cargo metadata --locked`, `pnpm install --frozen-lockfile --ignore-scripts`, dan `pnpm audit --prod --audit-level critical` (gagal hanya pada critical, supaya tidak flaky).

### 8.2 GitHub Actions di-pin ke ref mutable — **P3**

```
3× actions/setup-node@v4
3× actions/checkout@v4
2× pnpm/action-setup@v4
1× dtolnay/rust-toolchain@stable
```

`@v4` adalah tag yang bisa digeser, dan `dtolnay/rust-toolchain@stable` adalah **branch** yang selalu bergerak. Ini vektor supply-chain klasik (compromised action → secrets runner). Mitigasi: pin ke commit SHA penuh + komentar versi, dan biarkan Dependabot yang menaikkan SHA.

Catatan positif: `rust-toolchain.toml` mem-pin `channel = "1.96.0"`, jadi walaupun action-nya floating, toolchain yang diinstal tetap deterministik.

### 8.3 Duplikasi versi crate Rust — **P3**

16 crate punya lebih dari satu versi:

```
block-buffer   0.10.4, 0.12.1
convert_case   0.6.0, 0.11.0, 0.12.0
cpufeatures    0.2.17, 0.3.1
crypto-common  0.1.6, 0.2.2
digest         0.10.7, 0.11.3
getrandom      0.3.4, 0.4.3
hashbrown      0.16.1, 0.17.1
hmac           0.12.1, 0.13.0
r-efi          5.3.0, 6.0.0
rand           0.9.5, 0.10.3
rand_core      0.9.5, 0.10.1
sha1           0.10.7, 0.11.0
sha2           0.10.9, 0.11.1
syn            2.0.119, 3.0.9
thiserror      1.0.69, 2.0.21
thiserror-impl 1.0.69, 2.0.21
```

Akar masalah: `sqlx 0.9` sudah di RustCrypto 0.11/0.12, sementara `leptos`/`axum` masih di 0.10. Efeknya **dua stack kripto paralel** (`sha2` 0.10.9 + 0.11.1, `hmac` 0.12.1 + 0.13.0, `digest` 0.10.7 + 0.11.3). Bukan kerentanan, tapi:

- permukaan advisory jadi dua kali lipat (satu CVE di `sha2` bisa mengenai dua versi dengan status patch berbeda),
- ukuran binary dan waktu kompilasi naik,
- audit manual jadi lebih sulit.

Rekomendasi: jangan paksa dedup sekarang (bisa memecah `sqlx`/`leptos`). Catat sebagai technical debt, dan review ulang setiap kali `leptos` atau `axum` naik versi mayor. `convert_case` (3 versi) murni proc-macro build-time — paling tidak berisiko.

---

## 9. Batasan audit

- `pnpm audit` memakai advisory DB registry npm; tidak ada cross-check ke OSV, CISA KEV, atau GitHub Dependabot alerts untuk repo ini.
- `cargo audit` hanya memakai RustSec; CVE yang belum masuk RustSec tidak akan terdeteksi. DB diperbarui 2026-10-21 (segar).
- Severity mengikuti label vendor (GitHub Advisory). `cvss.score` untuk ketiga GHSA vite bernilai `0` di payload `pnpm audit` (field tidak terisi), jadi severity high/moderate diambil dari label GitHub, bukan skor numerik.
- Tidak ada SCA tingkat file/lockfile untuk image container atau artefak `dist/` (di luar scope).
- Audit dijalankan pada working tree dengan 27 file uncommitted; namun diff manifest hanya menambah `workspace:*` internal, dan `--frozen-lockfile` lolos, sehingga hasilnya setara HEAD untuk urusan dependency eksternal.
- Tidak ada SBOM (CycloneDX/SPDX) yang dihasilkan; bisa jadi langkah berikutnya bila dibutuhkan untuk compliance.

---

## 10. Action items

| Prioritas | Aksi                                                                                                                           | Estimasi |
| --------- | ------------------------------------------------------------------------------------------------------------------------------ | -------- |
| P2        | Tambah `vite: '7.3.7'` ke `overrides` di `pnpm-workspace.yaml`, jalankan `pnpm install`, verifikasi `pnpm test` + `pnpm build` | 15 menit |
| P2        | Workflow `schedule` harian: `pnpm audit --prod --audit-level high` + `cargo audit` (cache advisory-db)                         | 30 menit |
| P2        | `.github/dependabot.yml` untuk npm, cargo, github-actions                                                                      | 10 menit |
| P3        | `rust/audit.toml` dengan ignore RUSTSEC-2024-0436 + komentar justifikasi                                                       | 5 menit  |
| P3        | Pin 9 `uses:` ke commit SHA                                                                                                    | 20 menit |
| P3        | Tambah `license`/`publish = false` ke 16 crate + package TS                                                                    | 20 menit |
| P3        | Samakan `esbuild` root ke `0.28.2` (hapus duplikasi binary)                                                                    | 5 menit  |
| P3        | Upgrade devDeps bergelombang (minor dulu, lalu eslint 10, lalu TS 7 / vitest 5)                                                | 1–2 hari |
| P3        | Catat duplikasi crate Rust sebagai technical debt di docs/rust/DECISIONS.md                                                    | 10 menit |
| Nice      | Tambah `pnpm install --ignore-scripts` di CI                                                                                   | 5 menit  |
| Nice      | Generate SBOM CycloneDX di CI                                                                                                  | 1 jam    |
