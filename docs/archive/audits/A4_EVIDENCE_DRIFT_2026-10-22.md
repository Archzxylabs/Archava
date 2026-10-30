# A4 — Evidence drift Rust: klaim dokumen vs tree (2026-10-22)

- **Task:** `t_9d33766b` (P2, audit `ARCHLAB_AUDIT_2026-10-21.md` §A4), induk `t_be45b375`
- **Repo:** `/home/haikaru/Archverse/Lab/Archava_Platform`
- **HEAD verifikasi:** `412f763e17f3a68a457fd76c7b929184c50c916a` (`feat/laundry-1.17.0`, 2026-10-20 20:46 +07)
- **Manifest:** `docs/rust/required-coverage.json` — 93 item, 56 proof, 273 `requiredChecks`
- **Output gate:** `node tools/rust/coverage.mjs` → **exit 1**, `complete:false`, **294 diagnostic**, stderr kosong
- **Metode:** setiap angka di task body diperiksa ulang dari sumber otoritatif (manifest, `git rev-parse`/`merge-base`, isi `archxy2/packets/`, `dist/`, eksekusi gate). Tidak ada angka dokumen yang dipakai sebagai bukti. Raw: `docs/audit/evidence/a4/`.

## 1. Verdict

| #   | Temuan                                           | Status                                                                                                                                                                        |
| --- | ------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1   | 5 drift di task body                             | **terkonfirmasi semua**; sumber otoritatif benar, dokumen yang salah                                                                                                          |
| 2   | 4 drift tambahan di luar task body               | terkonfirmasi dan diselaraskan (`ARCHXY1_ACCEPTANCE_MATRIX.md` baris 45, 67, 244, 250)                                                                                        |
| 3   | `coverage.mjs` exit 1 / `complete:false`         | **by design**: 294 diagnostic = 294 klausa kontrak bukti yang belum dipenuhi, bukan 294 bug                                                                                   |
| 4   | "`rust:verify` gagal menulis `P-FAST-GATE.json`" | **tidak akurat**: file ada, ditulis `2026-10-21 14:48:29`, `result:"fail"`. Yang gagal gate-nya, bukan penulisannya                                                           |
| 5   | Penyebab gate gagal                              | socket PostgreSQL > 107 byte karena `TMPDIR` 52 byte. Sudah diperbaiki A5/A5-FU di tree; diuji ulang live di sesi ini                                                         |
| 6   | Gate akhir                                       | 6/6 exit 0 (§6); tidak ada source Rust/TS yang disentuh A4 — satu-satunya file non-dokumen yang disentuh adalah `.mjs` evidence milik lane A6 yang memblokir `pnpm lint` (§5) |

## 2. Tabel klaim → kenyataan → tindakan

Baris 1–5 adalah drift yang disebut task body; baris 6–13 yang ditemukan saat verifikasi ulang.

| #   | file:line                                          | Klaim                                                               | Kenyataan (terverifikasi)                                                                                                                                                                                                                                                                                     | Tindakan                                                                                    |
| --- | -------------------------------------------------- | ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------- |
| 1   | `docs/rust/EXECUTION_STATE.md:23`                  | `93 items / 53 proofs`                                              | `93 items / 56 proofs`, 273 `requiredChecks`                                                                                                                                                                                                                                                                  | diedit → `93 items / 56 proofs`                                                             |
| 2   | `docs/rust/EXECUTION_STATE.md:33`                  | baseline HEAD `409eab9`                                             | HEAD `412f763`; `409eab9` adalah ancestor, **24 commit** di belakang; bukti tersimpan masih di `94307ed`/`3754217` (C07), bukan baseline kedua                                                                                                                                                                | diedit: baseline `412f763`, jarak 24 commit, runtime evidence 20-10 vs node host `v22.23.2` |
| 3   | `docs/rust/EXECUTION_STATE.md:44-48`               | 8 packet `PACKET-000…007`, last 19-10 05:46, cut `c96abf0`          | **11 packet** `PACKET-000…010`; `010` support 20-10 **18:51**; sisi `a/` bergerak: `006`@`c96abf0`, `007`@`409eab9`, `008`@`e1e83c6`, `009`@`6c06998`, `010`@`0179c2a`                                                                                                                                        | diedit: 11 packet + cut per packet                                                          |
| 4   | `docs/rust/EXECUTION_STATE.md:962`                 | "acceptance matrix records C03/C04 locally verified"                | C04 benar; C03 hanya **partial** (T06/T08/T09 belum)                                                                                                                                                                                                                                                          | diedit: C04 verified, C03 partial + alasan                                                  |
| 5   | `docs/rust/EXECUTION_STATE.md:966`                 | "Still open" tanpa T06/T08/T09                                      | T06/T08/T09 masih open (A2/A3, F04–F06)                                                                                                                                                                                                                                                                       | diedit: ditambahkan ke daftar open                                                          |
| 6   | `docs/rust/FULL_RUST_COMPLETION_REPORT.md:4`       | `93 scope items, 53 proofs`                                         | 56 proof                                                                                                                                                                                                                                                                                                      | diedit → 56                                                                                 |
| 7   | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:28`        | C03/OWN-T01–T10 "locally verified"                                  | engine 13/13, host 11/11, browser pass **tidak membuktikan semantik** T06/T08/T09: F04 (readiness/cost tidak masuk plan identity; konfirmasi stale diterima), F05 (request id baru per klik, tanpa pending marker → lost-response replay unreachable), F06 (restore merge label, `restore(1)` → `changes=[]`) | diedit: verdict partial, T06/T08/T09 ditandai belum, bukti F04–F06 + status/next-action sel |
| 8   | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:45`        | A1-OWN baris `OWN-T01…T10` "locally verified"                       | sama dengan #7                                                                                                                                                                                                                                                                                                | diedit → partial + alasan                                                                   |
| 9   | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:67`        | packet `000…009`, "latest 009 attendance", semua `a/` cut `c96abf0` | 11 packet, `010` support 20-10 18:51, cut per packet (bukan satu cut)                                                                                                                                                                                                                                         | diedit                                                                                      |
| 10  | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:244`       | "C03 is **locally verified**"                                       | partial                                                                                                                                                                                                                                                                                                       | diedit                                                                                      |
| 11  | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md` baris C07 | "32 diagnostics" (audit 20-10)                                      | 294 hari ini; rincian 7 kelas di §3; 5 domain proof tetap hilang, sisanya artefak stale                                                                                                                                                                                                                       | diedit: angka baru + rincian + link dokumen ini                                             |
| 12  | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:250`       | "`53 proofs` note ... corrected in `3754217`"                       | hanya matrix yang pernah dikoreksi; `EXECUTION_STATE.md:23` dan `FULL_RUST_COMPLETION_REPORT.md:4` masih 53 sampai A4                                                                                                                                                                                         | diedit: "re-synced across the Rust docs by A4 on 2026-10-22"                                |
| 13  | `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md:12`        | "Candidate at matrix creation: HEAD `409eab9`"                      | **benar** — pernyataan historis, bukan baseline hidup                                                                                                                                                                                                                                                         | tidak diubah                                                                                |
| 14  | `docs/rust/required-coverage.json`                 | 56 proof                                                            | benar; salinan worktree hanya reformat prettier, **semantik identik** dengan `HEAD` (dibandingkan `json.dumps(sort_keys=True)`)                                                                                                                                                                               | tidak diubah                                                                                |

Catatan yang bukan bagian finding: `dist/library/artifacts.json` berisi 62 artefak vs archive 1.17.0 = 58; klaim `BUILD_STATUS.md` soal archive akurat dan A07 sengaja dibiarkan terbuka.

## 3. Anatomi 294 diagnostic

Gate: `node tools/rust/coverage.mjs` → `EXIT=1`, `complete:false`, 294 entri `errors`, stderr 0 byte. Sebaran (disjoint, total tepat 294):

| Kelas                               | Jumlah | Arti                                                                                                                                                          | Bug produk?                          |
| ----------------------------------- | -----: | ------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| `required check` belum `pass`       |    169 | 16 step proof menyimpan `fail: step "X" did not pass in this invocation` dari satu invokasi gate yang gagal                                                   | **tidak** — echo satu kegagalan step |
| identity mismatch                   |     52 | 26 proof × (`head` + `node`): 25 proof di `94307ed`, 1 di `3754217`; semuanya mencatat node `v26.10.0`, host sekarang `v22.23.2`                              | tidak — bukti lama, C07              |
| `sourceDigest` mismatch             |     40 | proof menyimpan sha256 source yang berubah setelah remediasi A1-FU3/A5/A5-FU/A9                                                                               | tidak                                |
| source list tidak menutupi manifest |     23 | proof merekam subset `sourcePaths`; contoh `P-FAST-GATE` belum memuat `rust/crates/pg-fixture/tests/socket_dir.rs` + `socket_dir_forced.rs` (file baru A5-FU) | tidak                                |
| evidence document hilang            |      5 | `P-R04-DOMAIN`…`P-R08-DOMAIN` — proof domain ERP, packet Archxy 2 belum diintegrasikan                                                                        | tidak — scope tertunda               |
| frozen tally hilang                 |      3 | `P-ERP-R04/R05/R07` tidak punya `details.namedRegressions`; ditulis ulang run gagal dengan skema baru                                                         | tidak                                |
| artifact hilang                     |      2 | `P-ARTIFACT-PROOFS`, `P-BENCHMARK` menunjuk `tools/rust/benchmark/rust/target/release/archava-bench` yang sudah dibersihkan dari disk                         | tidak                                |

**263 vs 294.** Audit 21-10 mencatat 263 setelah run gagal; hari ini satu run penuh atas evidence yang sama (tidak ada file `dist/rust-evidence`/`dist/own-evidence` yang lebih baru dari `2026-10-21 14:48`) menghasilkan 294 deterministik. Selisihnya berasal dari perubahan tree setelah audit: A1-FU3/A5/A5-FU/A9 mengubah source yang tercatat di `sourcePaths`, sehingga diagnostic `sourceDigest`/source-list bertambah. Angka historis tidak bisa direproduksi tanpa me-restore file-file itu; komposisi di atas yang otoritatif.

## 4. `coverage.mjs`: exit 1, `complete:false`, dan kenapa `P-FAST-GATE.json` fail

Kontrak yang dibaca gate (per proof): dokumen evidence ada dan parseable; `identity.{head,branch,rustc,cargo,node,pnpm}` cocok dengan kandidat live; `sourceDigest` atas `sourcePaths` cocok; setiap `requiredChecks` bernilai `pass`; `result` tidak kontradiksi dengan checks; `details` memuat tally beku; artifact ada di disk; source yang direkam menutupi `sourcePaths` manifest. Satu pelanggaran = satu entri `errors`; jika `errors` tidak kosong maka `complete:false` dan exit 1. Jadi hitungan diagnostic adalah **jumlah klausa kontrak yang gagal**, bukan jumlah cacat produk.

`dist/rust-evidence/P-FAST-GATE.json` **ditulis** (mtime `2026-10-21 14:48:29`) dengan `result:"fail"`, 21 check: 4 `pass` (`build release artifacts`, `cargo fmt --all --check`, `cargo clippy --all-targets -- -D warnings`, `wasm portable boundary`) dan 17 `fail` dengan teks identik:

```text
fail: step "cargo test --workspace" did not pass in this invocation
(cargo test --workspace: cargo test --workspace failed with exit 101
    ran for 124.7s)
```

Satu step gagal → 16 proof ditulis ulang dengan `fail` → 169 diagnostic turunan. Penyebab step: `TMPDIR` worker 52 byte mendorong socket PostgreSQL melewati batas `sun_path` 107 byte; audit §A5 mencatat `Unix-domain socket path ".../.s.PGSQL.37969" is too long (maximum 107 bytes)` dan 11/11 test `archava-adapters --test tenant_context` gagal; rerun `TMPDIR=/tmp/arv` membuat 11/11 hijau dan gate lanjut.

Perbaikan sudah di tree (`choose_socket_base` di `rust/crates/pg-fixture/src/lib.rs`, A5/A5-FU), diverifikasi live pada sesi ini dengan `TMPDIR` 52 byte:

```text
cargo test -p archava-pg-fixture --offline --locked   → EXIT=0
  tests/socket_dir.rs  a_cluster_binds_its_socket_whatever_tmpdir_is ... ok  (1.17s)
  tests/socket_dir_forced.rs  a_forced_long_socket_dir_names_the_sun_path_limit - should panic ... ok
  lib 16 passed; integration 1 + 1 passed; 0 failed
```

Artinya: penyebab kegagalan gate **sudah diperbaiki**; yang tersisa adalah artefak `fail` yang beku sampai release gate final (C07) menulis ulang 16 proof tersebut. `complete:false` hari ini juga struktural: 5 proof domain ERP memang belum ada sampai packet Archxy 2 diintegrasikan.

## 5. Perubahan di luar dokumentasi (dan alasannya)

- `docs/audit/evidence/a6/per-package-coverage.mjs` — 2 baris: `KEYS.map((k) => '-total'...)` / `'-hit'` menjadi `KEYS.map(() => ...)`. File ini peninggalan lane A6 (`t_91636400`, status `done`) dan membuat `pnpm lint` merah (`'k' is defined but never used`, 2 error), sehingga gate acceptance #4 tidak bisa hijau. Perbaikan semantik-netral (parameter tak terpakai dihapus); eslint + prettier file itu hijau. Ini bukan source Rust/TS, jadi kewajiban "jalankan ulang trio" tidak terpicu; seluruh gate di §6 dijalankan setelah perbaikan ini.
- Tidak ada source Rust/TS yang disentuh; tidak ada proof HEAD/hash yang diedit tangan; `docs/rust/required-coverage.json` tidak diubah.
- Tidak ada commit (`git add` tidak dijalankan); tree tetap dirty (232 entri saat gate).
- File dokumentasi yang berubah: `docs/rust/EXECUTION_STATE.md`, `docs/rust/FULL_RUST_COMPLETION_REPORT.md`, `docs/rust/ARCHXY1_ACCEPTANCE_MATRIX.md`, `docs/audit/A4_EVIDENCE_DRIFT_2026-10-22.md`, plus evidence `docs/audit/evidence/a4/`.

## 6. Gate akhir

Dijalankan `2026-10-22 08:51:51 → 08:55:15` pada HEAD `412f763`, 232 entri dirty, setelah lane A2 (`t_708ef38a`) dan A6 (`t_91636400`) selesai. Log: `docs/audit/evidence/a4/gates-final.log`.

| Gate                                                           | Exit | Catatan                                                                                              |
| -------------------------------------------------------------- | ---: | ---------------------------------------------------------------------------------------------------- |
| `cargo fmt --all --check`                                      |    0 | output 0 byte                                                                                        |
| `cargo clippy --all-targets --locked --offline -- -D warnings` |    0 | 0 warning                                                                                            |
| `pnpm format:check`                                            |    0 | termasuk 4 dokumen A4; rerun 09:05 setelah fix `.mjs` → 0                                            |
| `pnpm lint`                                                    |    0 | run 08:53 merah karena 2 unused `k` di file A6; setelah fix 2 baris, rerun 09:04 → 0 error/0 warning |
| `pnpm typecheck`                                               |    0 | 66 s (log gate G3: 66.6 s)                                                                           |
| `pnpm test`                                                    |    0 | 153 file / 1931 test lulus                                                                           |

Catatan konkurensi: `t_d9b90638` (A3/F05) masih `running` selama gate ini, tetapi tidak menulis file apa pun antara `08:51:51` dan `09:08` (tulisan Rust terakhirnya `08:42:57`), jadi hasil di atas terikat pada tree yang koheren. Run gate pertama (08:24–08:29) memang merah, tapi penyebabnya tulisan lane lain yang belum selesai: `vitest.config.ts` setengah tertulis dan `per-package-coverage.mjs` unused `k` — bukan perubahan A4 (yang hanya Markdown).

## 7. Follow-up (bukan A4)

1. **C07:** jalankan release gate final di HEAD final supaya 16 step proof + identity ditulis ulang; target diagnostic tinggal 5 domain proof (+ artefak benchmark bila perlu dibangun ulang).
2. **A2/A3:** tutup F04–F06; setelah itu matrix C03 boleh kembali `locally verified` dengan bukti baru.
3. **Archxy 2:** integrasikan `PACKET-000…010` secara semantik; itu yang menghapus 5 diagnostic `DOMAIN`.
