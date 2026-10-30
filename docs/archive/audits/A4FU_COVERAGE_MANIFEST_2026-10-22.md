# A4-FU — Manifest coverage RF01/RA06: freeze-supersede + koreksi dokumen

Tanggal: 2026-10-22 · Kartu: `t_09d08e54` (A4-FU) · Induk: `t_9d33766b` (A4 evidence drift)
Scope: **dokumen saja** — nol build, nol `cargo`, nol perubahan `rust/**`, `packages/**`, `apps/**`, `tools/**` (kecuali satu komentar baris di `tools/rust/coverage.mjs`).

## 0. Ringkasan

Klaim **"RF01/RA06 coverage CLOSED"** di `docs/rust/EXECUTION_STATE.md` dan
`docs/rust/FULL_RUST_COMPLETION_REPORT.md` **dicabut**. Manifest
`docs/rust/required-coverage.json` ditandai **historis/superseded** (bukan
di-regenerate): `frozen: 2026-10-16` dipertahankan apa adanya, dan validator
`node tools/rust/coverage.mjs` sekarang menyebut dirinya sebagai _scoreboard
program pra-remediasi_, bukan gate yang lulus.

Alasan keputusan (tidak dibuka ulang di kartu ini):

1. **40 `sourceDigest` basi bersifat permanen** — A1-FU3, A5, A5-FU dan A9
   sengaja mengubah sumber-sumber itu. Re-freeze = mencuci remediasi jadi
   seolah-olah bukti sudah dideklarasikan sebelum perubahan.
2. **5 evidence document `P-R04-DOMAIN`…`P-R08-DOMAIN` tidak pernah ada** —
   Archxy 2 belum diintegrasikan. Regenerate = memalsukan bukti, atau program
   berhari-hari di luar scope remediasi.

## 1. Fakta terukur (re-run, bukan dikutip dari body kartu)

Perintah: `node tools/rust/coverage.mjs` dari root repo. Output stdout adalah
JSON `{ requiredCoverage, errors, complete }`; exit code mengikuti
`errors.length === 0`.

| Sumber                                                                     | `complete` | `errors` | exit |
| -------------------------------------------------------------------------- | ---------- | -------- | ---- |
| Arsip A4 `docs/audit/evidence/a4/coverage-at-412f763.log` (HEAD `412f763`) | `false`    | **294**  | 1    |
| Re-run 2026-10-22 11:33Z (`/tmp/a4fu-run3.log`)                            | `false`    | **299**  | 1    |
| Capture final 11:40Z (`docs/audit/evidence/a4fu/coverage-final.json`)      | `false`    | **302**  | 1    |

Angka **294** di body kartu terkonfirmasi untuk tree `412f763`. Pada tree
sekarang angkanya sudah **299 → 302** dan masih bergerak (lihat §1.1) — selisih
294 → 299 dijelaskan per-kelas di §3, 299 → 302 per-elemen di §1.1; bukan angka
yang "diganti" supaya cocok.

`stderr` (satu baris, advisori):

```
note: docs/rust/required-coverage.json is a HISTORICAL manifest (frozen 2026-10-16, superseded 2026-10-22) — this validator is a scoreboard of the pre-remediation program, not a passing gate. See docs/audit/A4FU_COVERAGE_MANIFEST_2026-10-22.md
```

### 1.1 Angka ini bergerak — dan itu bagian dari temuannya

Validator dijalankan berkali-kali selama kartu ini, dan tiap run menangkap tree
yang berbeda karena remediasi kartu lain jalan paralel. Deret observasi:

| Waktu (UTC) | Tree      | `errors` | Catatan                                                          |
| ----------- | --------- | -------- | ---------------------------------------------------------------- |
| arsip A4    | `412f763` | 294      | baseline yang dikutip body kartu — terkonfirmasi                 |
| 11:33       | kerja     | 299      | +7 `source-list`, −2 identity (`P-RUST-BROWSER` membaik)         |
| 11:36       | kerja     | 302      | +10 / −7: `P-SECURITY-CHAIN` masuk mismatch, source list berubah |

Mekanismenya bukan "validator flaky": `sourcePaths` di manifest menunjuk
**direktori**, bukan file — `P-SECURITY-CHAIN` → `["tools/security", "packages",
"apps/crm"]`, `P-LEGACY-REPLACEMENT` → `["rust", "apps", "packages", "tools"]`.
`digestSources()` menyusuri direktori itu, jadi setiap file baru dari kartu lain
(contoh nyata: `apps/crm/test/password-fixture.test.ts` dari kartu A8) menambah
(a) sumber yang wajib dicakup evidence beku dan (b) nilai digest yang dihitung
ulang. Karena regenerate ditolak, **angka ini hanya bisa naik** — sebagian
kenaikan justru bukti remediasi lain maju, bukan bukti validator rusak.

Jadi angka §2 dibaca sebagai **snapshot 299 @11:33Z**, bukan konstanta. Yang
stabil di setiap run: `complete=false` dan `exit 1`.

## 2. Komposisi 299 error (snapshot 11:33Z, per kelas, `coverage.mjs` apa adanya)

| Kelas error (literal dari validator)                       | Jumlah  | Arti                                                                                                     |
| ---------------------------------------------------------- | ------- | -------------------------------------------------------------------------------------------------------- |
| required check `... did not pass in this invocation`       | 169     | 16 step proof menyimpan `fail` dari release-gate run 21 Oktober (akar: PG fixture socket path; A5/A5-FU) |
| `identity.head` mismatch                                   | 25      | evidence ditulis di HEAD lama (mayoritas `94307ed`, satu `3754217`)                                      |
| `identity.node` mismatch                                   | 25      | evidence ditulis di Node `v26.10.0`, tree sekarang Node lain                                             |
| `sourceDigest mismatch`                                    | 40      | sumber berubah setelah freeze (remediasi A1-FU3/A5/A5-FU/A9) → digest tidak akan pernah cocok lagi       |
| `recorded sources do not cover the manifest's sourcePaths` | 30      | source list di evidence tidak memuat file yang diminta manifest (7 proof baru masuk daftar ini)          |
| `no evidence document`                                     | 5       | `P-R04-DOMAIN`, `P-R05-DOMAIN`, `P-R06-DOMAIN`, `P-R07-DOMAIN`, `P-R08-DOMAIN`                           |
| `details missing frozen tally "namedRegressions"`          | 3       | `P-ERP-R04`, `P-ERP-R05`, `P-ERP-R07`                                                                    |
| `artifact ... no longer exists on disk`                    | 2       | `archava-bench` untuk `P-ARTIFACT-PROOFS` dan `P-BENCHMARK`                                              |
| **Total**                                                  | **299** | `169+25+25+40+30+5+3+2`                                                                                  |

Catatan: satu error `P-COLD-CONSUMERS` ("required check `artifact freshness
enforced` is fail: step `two cold consumers` … exit 101") ikut terhitung di
kolom 169, bukan di kolom artifact — itu kegagalan step time-ordered, bukan
artefak hilang.

## 3. Delta 294 → 299 (set-diff string exact, bukan selisih kasar)

Membandingkan dua log per-elemen (`Set` difference atas string error utuh):
**+47 ditambahkan, −42 dihapus**. Setelah diklasifikasi:

| Kelas                       | Ditambahkan | Dihapus | Net    |
| --------------------------- | ----------- | ------- | ------ |
| `sourceDigest mismatch`     | 33          | 33      | 0      |
| `do not cover the manifest` | 14          | 7       | +7     |
| `identity.head`             | 0           | 1       | −1     |
| `identity.node`             | 0           | 1       | −1     |
| kelas lain (169/40/5/3/2)   | 0           | 0       | 0      |
| **Total**                   | **47**      | **42**  | **+5** |

Bacaan:

- **+33/−33 `sourceDigest`**: hash yang sama-sama mismatch, tapi nilai
  `recomputed` berubah karena remediasi melanjutkan edit sumber setelah audit
  A4 pagi hari. Jumlah proof yang digest-nya basi tetap **40**.
- **+7 `do not cover`**: 7 proof yang sebelumnya lolos cek source-list kini
  gagal — `P-AUTHORITY-PG`, `P-ERP-PURCHASING`, `P-ERP-FOODSERVICE`,
  `P-ERP-R04`, `P-ERP-R05`, `P-ERP-R07`, `P-READINESS-PG`. Penyebabnya file
  test baru (mis. `rust/crates/pg-fixture/tests/socket_dir.rs`) tidak ada di
  `sourcePaths` manifest 2026-10-16.
- **−2 identity**: `P-RUST-BROWSER` satu-satunya proof yang **keluar** dari
  daftar mismatch (identity-nya kini cocok). Sisa 25 proof tetap mismatch.

Jadi `294 + 7 − 2 = 299`. Tidak ada error yang dihapus dari manifest untuk
memperkecil angka.

## 4. Freeze-supersede pada manifest

`docs/rust/required-coverage.json` (skema yang sudah ada, tanpa skema baru):

- `frozen: "2026-10-16"` — **tidak diubah**.
- `frozenBy: "…"` — **tidak diubah**.
- Ditambah satu blok `superseded`:

```json
"superseded": {
  "at": "2026-10-22",
  "status": "historical",
  "reason": "Frozen pre-remediation (2026-10-16). Remediation A1-FU3/A5/A5-FU/A9 deliberately changed 40 sources, so their recorded sourceDigest can never match again; the five P-R04-DOMAIN…P-R08-DOMAIN evidence documents were never produced (Archxy 2 was never integrated). Regenerating would launder remediation and fabricate proofs, so the freeze is retained as a historical scoreboard instead.",
  "liveValidator": {
    "command": "node tools/rust/coverage.mjs",
    "observedAt": "2026-10-22T11:36Z",
    "complete": false,
    "errors": 302,
    "note": "Snapshot, not a constant: 294 @412f763 -> 299 @11:33Z -> 302 @11:36Z. …"
  },
  "unrecoverableProofs": ["P-R04-DOMAIN", "P-R05-DOMAIN", "P-R06-DOMAIN", "P-R07-DOMAIN", "P-R08-DOMAIN"],
  "supersededByGates": [
    "vitest.config.ts A6 coverage thresholds",
    "pnpm test",
    "archava-slice",
    "cargo test -p archava-host --test own -- --test-threads=1",
    "cargo fmt --all --check",
    "cargo clippy --workspace --all-targets -- -D warnings"
  ],
  "evidence": "docs/audit/A4FU_COVERAGE_MANIFEST_2026-10-22.md"
}
```

`loadManifest()` hanya memvalidasi `items` dan `proofs`, jadi key tambahan ini
tidak mengubah jalur validasi. Manifest sha256 berubah
`d44add01…` (HEAD) → `fa5631e9…` (sekarang); pointer turunan di
`docs/rust/migration-inventory.json` (`requiredCoverage.manifestSha256`) sudah
di-refresh ke nilai nyata itu, dan `note`-nya menyatakan manifest historis.

## 5. Validator: satu baris advisori, logika tidak berubah

`git diff tools/rust/coverage.mjs` hanya berisi:

1. `process.stderr.write(...)` satu baris di blok `if (import.meta.main)` —
   dicetak **sebelum** JSON verdict, ke **stderr**, supaya stdout tetap JSON
   valid yang bisa diparse auditor.
2. **Tidak ada restorasi direktif.** Direktif `/* global process, URL */` di
   HEAD ternyata redundan: `eslint` menolaknya dengan `no-redeclare`
   (`process`/`URL` sudah builtin global), jadi state akhir membiarkannya
   terhapus — `pnpm exec eslint tools/rust/coverage.mjs` → `ESLINT_EXIT=0`.

Tidak ada perubahan pada `validateCoverage()`, pada himpunan error, atau pada
`process.exitCode` (tetap `1` bila `errors.length > 0`).

## 6. Koreksi dokumen + negative proof

Sebelum (tree `HEAD`, apa adanya):

```
docs/rust/EXECUTION_STATE.md:104:| RF01/RA06 coverage           | **CLOSED** | Frozen manifest `docs/rust/required-coverage.json` (…
docs/rust/FULL_RUST_COMPLETION_REPORT.md:30:| RF01/RA06 coverage | CLOSED | `required-coverage.json` + `tools/rust/coverage.mjs` + …
```

Sesudah:

```
$ grep -rn "RF01/RA06" docs/rust/ | grep "CLOSED"
$ echo $?
1
```

Kosong — tidak ada satu pun baris di `docs/rust/` yang memuat `RF01/RA06`
**dan** `CLOSED`. Baris yang tersisa:

```
docs/rust/EXECUTION_STATE.md:111:| RF01/RA06 coverage | **SUPERSEDED** (historis; pra-remediasi 2026-10-21) | …
docs/rust/EXECUTION_STATE.md:114:## Superseded: RF01/RA06 coverage manifest (2026-10-22)
docs/rust/EXECUTION_STATE.md:116:`docs/rust/required-coverage.json` is a **historical manifest, frozen 2026-10-16**, and the `RF01/RA06 coverage` row above is **SUPERSEDED** — its earlier closure claim is withdrawn. …
docs/rust/FULL_RUST_COMPLETION_REPORT.md:14:> manifest frozen 2026-10-16**. The `RF01/RA06 coverage` row below is
docs/rust/FULL_RUST_COMPLETION_REPORT.md:45:| RF01/RA06 coverage | SUPERSEDED (historis; pra-remediasi 2026-10-21) | …
```

Angka yang ikut dikoreksi (sebelumnya drift terhadap manifest/tree):

- `51 proofs: 42 evidence + 7 document + 2 json` → **`56 proofs: 48 evidence +
6 document + 2 json; 273 required checks`** (dihitung dari manifest).
- `→ live: complete=false, 44 errors` → **`at freeze: 44 errors; re-run
2026-10-22: 299 errors, exit 1`**.
- Next actions butir 3 (`coverage.mjs` = "running scoreboard, 44 → 0 errors is
  the completion curve") → **historical scoreboard**, 299 error pada tree
  sekarang, exit 1.
- Terminal record `FULL_RUST_COMPLETION_REPORT.md` ("reports the same five and
  nothing else") diberi anotasi tanggal + angka 299, karena kalimat itu benar
  untuk run 2026-10-17 dan salah untuk tree sekarang.

Gate yang berlaku sekarang (ditanam di kedua dokumen):

- (a) threshold coverage A6 di `vitest.config.ts` (floor line/function/branch/
  statement, ratchet naik saja);
- (b) `pnpm test`;
- (c) gate Rust: `archava-slice`, `cargo test -p archava-host --test own --
--test-threads=1`, `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`.

## 7. File yang berubah

| File                                              | Perubahan                                                                                            |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `docs/rust/required-coverage.json`                | blok `superseded` (§4); `frozen`/`frozenBy` tidak disentuh                                           |
| `docs/rust/EXECUTION_STATE.md`                    | status baris RF01/RA06 → SUPERSEDED, paragraf gate berlaku, tally 56 proof, next-actions             |
| `docs/rust/FULL_RUST_COMPLETION_REPORT.md`        | banner superseded, status baris → SUPERSEDED, anotasi terminal record                                |
| `docs/rust/migration-inventory.json`              | `requiredCoverage.note` + `manifestSha256` nyata (`fa5631e9…`)                                       |
| `tools/rust/coverage.mjs`                         | satu baris `stderr` advisori; logika validasi tidak berubah                                          |
| `docs/audit/A8FU_TS_HARDENING_2026-10-22.md`      | **milik kartu lain** (`t_0d834d2e`): hanya padding tabel Prettier, konten tidak disentuh — lihat §10 |
| `docs/audit/A4FU_COVERAGE_MANIFEST_2026-10-22.md` | dokumen ini                                                                                          |

Tidak ada perubahan di `rust/**`, `packages/**`, `apps/**`. Tidak ada commit,
stash, atau revert; dirty tree milik kartu lain dibiarkan apa adanya.

Catatan soal diff `required-coverage.json` yang besar (697 baris): versi di `HEAD`
**tidak** Prettier-clean (`git show HEAD:docs/rust/required-coverage.json` →
`[warn] Code style issues found`), jadi `pnpm format:check` sudah merah di file
itu sebelum kartu ini. Sekarang file tersebut Prettier-clean dan gate hijau.
**Diff semantik vs `HEAD` = tepat satu key**, `/superseded` — dibuktikan dengan
walk JSON rekursif (`93 items / 56 proofs / 273 requiredChecks` dan `frozen`
identik di kedua versi).

## 8. Yang tetap tidak bisa dibuktikan

Kartu ini **tidak** memulihkan bukti, dan itu disengaja:

- **5 evidence document `P-R04-DOMAIN`…`P-R08-DOMAIN` tidak pernah ada.**
  Archxy 2 belum diintegrasikan, jadi tidak ada run yang bisa menghasilkan
  dokumen itu. Selama kelimanya absen, `G04-DOMAIN-INTEGRITY` dan lane
  R04–R08 tetap tidak bisa dinyatakan tertutup oleh manifest ini — atau oleh
  manifest mana pun yang jujur.
- **40 `sourceDigest` tidak akan pernah cocok lagi** dengan manifest 2026-10-16.
  Yang bisa dibuktikan hanyalah bahwa sumber _sekarang_ berbeda dari yang
  direkam _dahulu_ — bukan bahwa perilakunya setara. Kesetaraan perilaku
  dibuktikan gate lain: `pnpm test`, threshold A6, dan suite Rust di §6(c).
- **169 required check "did not pass"** adalah jejak release-gate run 21
  Oktober yang gagal karena PG fixture socket path. Bukti bahwa kegagalan itu
  sudah diperbaiki ada di kartu A5/A5-FU (log gate sendiri), bukan di manifest
  beku ini.
- **50 identity mismatch** (HEAD + Node) hanya bisa hilang dengan menulis
  ulang evidence dari tree sekarang — persis pekerjaan yang ditolak kartu ini.

Konsekuensinya: `complete: true` **tidak** bisa dicapai dari manifest ini tanpa
mengarang bukti. Angka 294 → 299 → 302 adalah rekor historis yang jujur
(naik ketika remediasi lain mendarat), bukan target yang harus dinolkan.

## 9. Reproduksi

```bash
# verdict + exit code (exit 1, complete=false; capture final 11:40Z = 302 errors)
node tools/rust/coverage.mjs > /tmp/coverage.json 2>/tmp/coverage.err; echo "EXIT=$?"

# komposisi per kelas
node -e 'const fs=require("fs");const r=fs.readFileSync("/tmp/coverage.json","utf8");
const d=JSON.parse(r.slice(r.indexOf("{")));console.log(d.errors.length)'

# negative proof
grep -rn "RF01/RA06" docs/rust/ | grep "CLOSED"; echo "EXIT=$?"

# format gate
pnpm format:check
```

## 10. Gate — exit code apa adanya (final run, 2026-10-22 11:36–11:42Z)

| #   | Perintah                                          | Exit  | Output ringkas                                                          |
| --- | ------------------------------------------------- | ----- | ----------------------------------------------------------------------- |
| 1   | `node tools/rust/coverage.mjs`                    | **1** | `complete=false`, `errors=302` (memang tidak lulus — manifest historis) |
| 2   | `pnpm format:check`                               | **0** | `All matched files use Prettier code style!`                            |
| 3   | `pnpm exec prettier --check docs/rust docs/audit` | **0** | `All matched files use Prettier code style!`                            |
| 4   | grep RF01/RA06 + CLOSED di `docs/rust/`           | **1** | kosong (inilah negative proof-nya)                                      |
| 5   | `pnpm exec eslint tools/rust/coverage.mjs`        | **0** | bersih                                                                  |
| 6   | parse `docs/rust/migration-inventory.json`        | **0** | valid JSON; `manifestSha256` == sha256 manifest nyata (`fa5631e9…`)     |

Log mentah (captured, bukan ditulis ulang) ada di
`docs/audit/evidence/a4fu/`:

| File                                                                                   | Isi                                                                           |
| -------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------- |
| `coverage-final.json`                                                                  | stdout JSON validator pada capture final (`complete=false`, 302 error)        |
| `coverage-final.stderr.txt`                                                            | baris advisori "HISTORICAL manifest" yang dicetak validator                   |
| `gate-final.log`                                                                       | semua gate di §10 dengan exit code apa adanya, satu blok per gate             |
| `coverage-before.log`                                                                  | stdout JSON run awal kartu ini (299 error)                                    |
| `coverage-after.json`                                                                  | stdout JSON run paralel setelah koreksi dokumen (manifest `fa5631e9…`)        |
| `negproof-rf01-ra06-closed.log`                                                        | **0 byte** — grep RF01/RA06 + CLOSED tidak menemukan apa pun                  |
| `format-check.log`                                                                     | `pnpm format:check` hijau                                                     |
| `coverage-stdout.json`, `coverage-stderr.txt`, `gate.log`, `coverage-after.stderr.log` | capture antara sebelum capture final — disimpan apa adanya, bukan bukti utama |

Gate #2 sempat merah **bukan** karena file kartu ini: satu-satunya file yang
gagal adalah `docs/audit/A8FU_TS_HARDENING_2026-10-22.md` (untracked, ditulis
kartu A8-FU `t_0d834d2e` pada 18:28 lokal, setelah run gate pertama kartu ini).
File itu diformat dengan `prettier --write` — hanya padding tabel yang berubah,
teks tidak — supaya gate repo hijau untuk semua kartu. Kalau pemiliknya menulis
ulang file tersebut tanpa memformat, gate #2 akan merah lagi dan itu tanggung
jawab kartu A8-FU, bukan kartu ini.
