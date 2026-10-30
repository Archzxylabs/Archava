# Review handoff KIT-09 — 14 Oktober 2026

**Keputusan: KIT-09 masih candidate. Perbaiki lint dan evidence binding sebelum
commit slice ini. Commit tooling yang sudah diperbaiki boleh dipisahkan dari
penutupan audit produk, dengan status candidate dan blockers ditulis jujur.**

**Follow-up `242549c`:** implementator sudah commit 11 files tetapi H01/H03 belum
tertutup; tiga identity-negative cases dijalankan ulang dan tetap diterima.
Existing commit dipertahankan, perbaikan melalui follow-up commit dengan hooks
aktif. Docs accepted pada commit ini belum diterima review.

Arahan lokasi eksperimen direvisi setelah membaca
[aturan Lab](../../CLAUDE.md): “Eksperimen non-produksi → Experiments/.” Struktur
file tersebut menempatkan Experiments di dalam Lab. Target actual:
`/home/haikaru/Archverse/Lab/Experiments/rust-pilot/`, bukan
`/home/haikaru/Archverse/Experiments/`. Perpindahan harus menjaga files/checksums,
memperbaiki 15 lint errors, mengganti relative source resolution dan mempunyai
standalone checks. Lokasi external tidak menghilangkan kewajiban quality checks.

Archxy implementator; Codex orchestrator/reviewer. Review ini tidak mengubah product
code, experiment code, database owner, release archives atau manifest generated.
Tidak commit/push dan tidak menghidupkan ulang monitor yang disebut implementator.

## Yang terkonfirmasi

- HEAD masih `a462591`. Tujuh tracked files KIT-09 berubah, ditambah generator,
  declaration, test dan generated acceptance manifest. Source tujuh domain audit
  sebelumnya tidak mempunyai perubahan tracked.
- `dist/security-proof/results.json`, checkedAt `2026-10-14T14:42:51.978Z`, mencatat
  **83/83 controls**, 46 commands, 1.707 unit tests dan 0 failed unit tests.
  Ini pembacaan report; Codex tidak menjalankan ulang security battery dalam review
  ini. `template-audit` exit 1 dicatat sesuai policy temuan template terpisah, bukan
  otomatis berarti security runner gagal. Status monitor lama tidak dipakai.
- Scoped manifest suite dijalankan ulang: **10/10 tests PASS**.
- Root ESLint dijalankan ulang: **15 errors**, semuanya pada empat `.mjs` files
  eksperimen. Sebanyak 13 `no-undef` untuk `URL`/`process`, dan dua unused constants
  `QUOTE_A`/`QUOTE_B` di `compare.mjs`. `.prettierignore` tidak memperbaiki ESLint.
- Review tidak memvalidasi klaim semua 191 perubahan formatting hanya whitespace;
  eksperimen masih untracked sehingga comparison terhadap baseline harus membawa
  snapshot/hash asalnya. Jangan mencampurkannya ke KIT-09 commit.

Log lint: `/tmp/archava-archxy-handoff-lint.json`. Exit 1 dari lint nyata, bukan
status monitor. Suite Vitest scoped tidak menjalankan packing/release build.

## H01 — P1: generated manifest belum mengikat artifact identity

`tools/library/manifest.mjs` memeriksa beberapa version strings, tetapi menyalin
`security.catalogSha256`/`artifactSha256` tanpa membandingkannya dengan loaded
catalog dan artifact inventory. `releaseProfile.catalog.version` juga tidak
divalidasi terhadap trusted catalog; artifact inventory dan profile dapat berbeda.

Additional tests memakai `buildManifest()` yang asli dan copies in-memory dari
evidence aktual. Semua skenario berikut masih menghasilkan manifest tanpa refusal:

| Perubahan input sintetis                                          | Hasil generator sekarang                                          |
| ----------------------------------------------------------------- | ----------------------------------------------------------------- |
| Security catalog/artifact hashes diganti menjadi unrelated hashes | Tetap mengeluarkan profile 1.17.0 dan hashes itu                  |
| Loaded release catalog diganti 1.16.0 / assembly 15               | Tetap mengeluarkan manifest 1.17.0                                |
| Satu artifact inventory dihapus                                   | Tetap mengeluarkan 59 artifacts dengan hash evidence 60 artifacts |
| Seluruh command/control/check arrays dikosongkan                  | Tetap mengeluarkan manifest tanpa refusal                         |

Ini bukan bukti actual security report dipalsukan; ini membuktikan guard generator
belum menolak evidence campuran. Tidak ada file report/source yang dimutasi untuk
reproducer tersebut.

Perbaikan: validasi loaded trusted catalog, release catalog dan inventory sebagai
satu identity; hitung hashes memakai convention verifier existing dan cocokkan
dengan evidence. Tolak missing/empty required proof sections, duplicate/missing
artifacts dan mismatch profile/contract/hash. Tambahkan negative regression tests
yang gagal sebelum fix. Jangan hardcode inventory 60 atau controls 83 selamanya.

Ten tests sekarang belum mencakup skenario identity di atas. Declaration file juga
perlu mencerminkan actual output: `contracts` berupa object, beberapa evidence
fields absent/null, dan `main()` mengembalikan manifest, bukan `void`.

## H02 — P1: archive yang disebut dalam handoff berbeda dari current release

Actual files dibaca langsung:

| Identity  | Current `dist/library` | Archive `dist/releases/1.17.0/library` |
| --------- | ---------------------- | -------------------------------------- |
| Profile   | 1.17.0                 | 1.17.0                                 |
| Artifacts | 60                     | 58                                     |
| Assembly  | 16.0.0                 | 15.0.0                                 |

`docs/BUILD_STATUS.md` masih mengatakan archive matching 60 artifacts/assembly16.
Klaim itu tidak didukung archive yang ada. Dua repeated pack runs yang identik
tidak membuktikan archive lama sama dengan current artifacts. Ini kelanjutan A07
dari audit menyeluruh.

Preserve archive lama. Penutupan release memerlukan identity baru yang konsisten
dan exact archive comparison sebagaimana tugas A07; jangan overwrite archive
1.17.0 atau mengedit archived metadata agar kelihatan matching. Sebelum closure,
KIT-09 docs boleh menyebut tooling candidate dan actual archive mismatch.

## H03 — P2: pisahkan evidence summary dari acceptance decision

Generator secara sengaja menyalin failed-control counts. Saat supplied evidence
memuat satu failed control dan satu failed unit test, output memang melaporkan
82/83 controls dan unit failed 1 secara jujur, tetapi CLI tetap dapat menghasilkan
file bernama acceptance tanpa verdict yang membedakannya dari accepted release.

Evidence collector boleh menyimpan failed observations. Namun mode acceptance
harus menolak failed/incomplete gates, atau membawa explicit `not_accepted` verdict
dengan reasons yang dipakai downstream/docs. Tidak semua command exit 1 otomatis
fatal: pertahankan explicit security verifier policy untuk template findings dan
jangan membuat blanket exit-code rule yang menghapus batas release scope existing.

Generated manifest tidak memberikan authority untuk menyatakan source aman, suatu
scope accepted atau KIT-09 selesai. Klaim accepted pada programme/release docs
harus dikoreksi selama review blockers terbuka.

## Audit produk sebelumnya masih berlaku

Isolated PostgreSQL adverse scenarios dijalankan ulang memakai installed runtime
artifacts saat ini. Semua tujuh synthetic owner creations berhasil. Pelanggaran
yang diamati sebelumnya masih terulang:

- Enam domain mengembalikan owner receipt ke zero-grant viewer, menerima replay
  oleh actor lain dan replay setelah grants dicabut. Assets tetap menolak.
- Lima domain melayani service reads untuk represented human tanpa grants.
- Assigned staff membaca/mengubah field job dan invoice owner lain.
- Attendance menerima roster sesi A sebagai check-in sesi B.
- Paid-plan draft tanpa payment reference mengonsumsi entitlement.

Output baru: `/tmp/archava-archxy-handoff-domain-recheck.jsonl`. Reproducer membuat
cluster/socket/data sintetis sendiri dan membersihkannya sesudah pengujian. Exit
0 reproducer berarti script selesai; hasil `blocked: false` di cases unauthorized
adalah pelanggaran, bukan acceptance PASS. Tidak ada owner DB/provider calls.

Dengan demikian 83/83 controls tidak menutup A01–A05. A06/A08 dan audit lengkap
tetap mengikuti [audit produk](PROJECT_AUDIT_2026-10-14.md); dua item itu tidak
dijalankan ulang sebagai minimal-install/live-reference proof pada review ini.

## Instruksi eksekusi dan commit

1. Ikuti updated relocation instructions di `ARCHXY_RUST_PILOT_TASK.md` dan target
   Lab/Experiments di follow-up atas. Perbaiki Node imports/declarations untuk
   `URL`/`process` dan hapus dua unused constants. Jangan blanket-ignore source
   Rust pilot atau mematikan rules agar gate hijau. Generated output/cache boleh
   mempunyai ignore yang spesifik dan dijelaskan.
2. Selesaikan H01/H03 beserta negative tests dan declaration yang benar. Rapikan
   docs KIT-09 menjadi candidate, dengan unresolved archive/product findings.
   Jangan mengklaim archive matching sampai H02/A07 benar-benar ditutup.
3. Jalankan scoped manifest tests, lint, typecheck/format yang terpengaruh. Jika
   tests/evidence berubah, refresh producing commands yang diperlukan, lalu generate
   manifest dari final evidence. Jangan mengarang counts atau checkedAt.
4. Sesudah checks relevan lulus, boleh commit lokal slice KIT-09 saja. Stage exact
   paths: `package.json`, `apps/crm/scripts/upgrade.mjs`,
   `tools/library/verify-accommodation.mjs`, `tools/library/manifest.mjs`,
   `tools/library/manifest-module.d.ts`, `tools/library/test/manifest.test.ts`,
   `docs/BUILD_STATUS.md`, `docs/LOCAL_LIBRARY_HANDOFF.md`,
   `docs/LOCAL_LIBRARY_RELEASE.md`, `docs/STARTER_KIT_IMPLEMENTATION.md` dan generated
   manifest dengan status sebenarnya. Review `git diff --cached` sebelum commit.
5. Eksperimen/formatting, task-file rename, Codex research/review docs dan product
   security fixes tetap changesets terpisah. Jangan `git add .`, stage unowned
   changes, commit private files, push atau publish. Commit tooling bukan acceptance
   release. Tutup audit produk A01–A09 sebelum menyatakan platform accepted.

Tidak perlu re-arm monitor atau meminta pilihan yang sama lagi. Lanjutkan sesuai
keputusan ini dan laporkan actual results, commit scope serta blockers tersisa.

## Status closure per 15 Oktober 2026

Bagian di atas adalah snapshot 14 Oktober dan dibiarkan apa adanya. Yang berikut
adalah hasil mesin sesudah follow-up dijalankan. Tidak ada bagian di atas yang
disunting agar terlihat sudah benar.

| Temuan                       | Status                                          | Evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| ---------------------------- | ----------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| H01 identity binding         | **Diklaim tertutup** — masih perlu owner review | 4 skenario sintetis dari review ini kini punya test yang menolak: hashes diganti, loaded catalog lebih tua, satu artifact inventory hilang, section kosong. Suite manifest 10 → 32 tests, 32/32 lulus.                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| H02 archive mismatch         | **Ditutupi, bukan diperbaiki**                  | Archive dijaga: 58 vs 60 artifacts (`@archava/field-services`, `@archava/field-services-pg` hanya di current), assembly 15.0.0 vs 16.0.0, 21 vs 22 ledgers — terverifikasi langsung dari kedua `artifacts.json`. Docs sekarang menyebut mismatch; archive tidak di-overwrite.                                                                                                                                                                                                                                                                                                                                                                                           |
| H03 evidence vs acceptance   | **Diklaim tertutup** — masih perlu owner review | `accepted` tetap `null`, tiga alasan selalu dicatat, failed control/unit test tetap dilaporkan, bukan dibulatkan.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| Lint 15 errors (eksperimen)  | **Tertutup**                                    | Eksperimen pindah ke `Experiments/rust-pilot/` dengan `lint.config.mjs` sendiri; `eslint . --max-warnings=0` di sana exit 0.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| Format eksperimen            | **Dua cacat nyata ditemukan dan diperbaiki**    | `pnpm-lock.yaml` membawa trailing blank line sehingga `prettier --check` menolaknya; `.prettierignore` menyebut `Cargo.lock` dan `pnpm-lock.yaml` di comment tapi tidak mendaftarkannya, jadi perilaku terdokumentasi itu terjadi secara tidak sengaja dari extension walk. Keduanya diperbaiki; `pnpm install --frozen-lockfile --ignore-scripts` tetap resolve.                                                                                                                                                                                                                                                                                                       |
| Write idempotent (H04, baru) | **Tertutup**                                    | `sameEvidence()` bandingkan by value, `generatedAt` dipertahankan; ditambah tujuh case (array-vs-object, order-sensitivity, key-order-insensitivity, dsb.) karena mutation testing menemukan tiga comparison yang terlalu longgar. 6 mutants menjadi 7 yang semua terkill.                                                                                                                                                                                                                                                                                                                                                                                              |
| Prose counts                 | **Diperbaiki**                                  | "80 controls" → 83 di dua docs; `1707 tests` → 1729. Keduanya diverifikasi terhadap `dist/security-proof/results.json` dan `dist/security-proof/unit.json` (vitest JSON reporter), bukan diketik dari memori.                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| Reproduce list               | **Diperbaiki, lalu dikoreksi lagi**             | Handoff mendaftar 6 command; generator mendaftar 9 (termasuk `accommodation:verify --cold-prior`, `kits:verify`, `capacity:verify` yang jadi sumber evidence). Handoff sekarang cocok persis dengan generator. **Koreksi 15 Oktober:** catatan pertama yang menyertai list ini terlalu kuat. Dari 46 command `security:verify`, `kits`, `capacity`, `capacity-host`, `accommodation` dan `accommodation-host` memang ikut berjalan, jadi 6 dari 9 file evidence benar-benar diperbarui oleh chain itu. Hanya 3 file yang tidak: dua `browser-proof` dan `upgrade-proof`. `crm:benchmark` menulis `dist/capacity-proof/` — path berbeda yang manifest tidak pernah baca. |
| Klaim `checkedAt` di handoff | **Dikoreksi**                                   | Dokumen handoff semula menulis bahwa manifest "mencatat setiap `checkedAt`". Salah: proof lain tidak mempunyai field timestamp, jadi 6 dari 7 entry `evidence` memuat `checkedAt: null` dan hanya `dist/security-proof/results.json` yang punya. Sekarang ditulis sebagai itu.                                                                                                                                                                                                                                                                                                                                                                                          |
| `1719 tests` di dua docs     | **Dikoreksi**                                   | `dist/security-proof/unit.json` melaporkan `numPassedTests: 1729`; angka 1719 yang saya tulis sebelumnya salah. HEAD = 1729 − 22 (32 vs 10 `it(` di manifest test) = 1707, cocok dengan baseline lama. Owner task file `ARCHXY_IMPLEMENTATION_TASK.md:174` masih memuat "80/80 controls" sebagai "bukti fresh" — file itu untracked dan milik owner, jadi tidak disentuh.                                                                                                                                                                                                                                                                                               |
| Evidence freshness           | **Diperbarui lalu divergenerate**               | `browser-proof` (23 h), `upgrade-proof` (5 h) dan `kit-proof`/capacity semuanya direfresh: `crm:verify-browser` 44/44 + 70/70 Chromium, `crm:verify-upgrade --cold-prior` 4/4 check, keduanya profile 1.17.0 dengan `providerCalls: 0`. Manifest diregenerate tiga kali dari evidence fresh → byte-identical (`bd6aae96…`). `pnpm lint` dan `format:check` keduanya exit 0.                                                                                                                                                                                                                                                                                             |

**Yang masih terbuka, tanpa perubahan:** A01–A09 audit produk; semua pelanggaran
unauthorized yang tercatat di bagian audit produk di atas (83/83 controls tidak
menutup A01–A05); `operationalKitsAccepted` tetap 0 dari 40 full operating kits;
13 moderate OpenTelemetry-chain template findings; gate certification dan
penetration-test tetap `false`; KIT-08 specialist kits tetap `Pending`; narrow scope
formal-education (K14) dan nonprofit-associations (K22) masih unstarted — keduanya
tercatalogue sebagai requirement tanpa domain implementation.

**Belum commit.** Tidak ada `git add .`, tidak ada stage perubahan yang bukan milik
slice ini, tidak ada push/publish. `git diff --cached` wajib direview sebelum commit.
`GEMINI_IMPLEMENTATION_TASK.md` → `ARCHXY_IMPLEMENTATION_TASK.md` adalah rename milik
owner, bukan dari pekerjaan ini, dan dibiarkan untracked.
