# Review koreksi KIT-09 — 15 Oktober 2026

**Status: perbaikan identity H01 yang diuji sudah bekerja; evidence-only verdict
H03 sudah ada. Tahan commit koreksi sampai test checkout baru, reporting failures
dan dokumentasi diperbaiki. A01–A09/release acceptance tetap terbuka.**

HEAD masih `242549c`; delapan files koreksi KIT-09 belum staged/committed. Rename
task file dan dokumen Codex tetap terpisah. Tidak ada push, perubahan product
domain, owner database atau archived release dari review ini. Monitor implementator
tidak di-re-arm; review memakai direct commands dan evidence yang ada.

## Kemajuan yang terverifikasi

- Security report `2026-10-14T18:15:22.462Z`: 83/83 controls, 46 commands, 1.729 unit
  tests dan 0 failed. Ini pembacaan report, bukan rerun seluruh security battery.
- Manifest actual SHA-256
  `bd6aae96e9125b5e3633db8c663510598aba141b464c20bda08ca661e7062ae0`;
  profile 1.17.0, 60 artifacts, `accepted: null`, 0 accepted operational kits.
  Hash tetap sama setelah scoped test CLI melakukan regeneration.
- Hash asing, loaded catalog 1.16.0, inventory kehilangan artifact dan required
  proof arrays kosong sekarang ditolak pada additional tests memakai builder actual.
  Failed unit/control evidence tetap dicatat; generator tidak memberi acceptance.
- Scoped manifest suite di working repo: **32/32 PASS**.
- Root ESLint direct rerun: **exit 0**. Root `prettier --check .`: **exit 0**.
- Pilot berada di `/home/haikaru/Archverse/Lab/Experiments/rust-pilot/`; lokasi lama
  dalam product repo sudah tidak ada. Ini konfirmasi lokasi, bukan acceptance
  lengkap benchmark/cold consumers/Rust migration.
- Archive 1.17.0 tetap 58 artifacts/assembly15, current tetap 60/assembly16.
  Mismatch sudah dijelaskan pada docs; archive tidak dipromosikan sebagai matching.

Logs direct lint/format: `/tmp/archava-review-20261015-lint.json` dan
`/tmp/archava-review-20261015-format.log`. Background IDs yang dikirim implementator
tidak dikoleksi lewat tool task manager; report dibaca dari filesystem.

## J01 — P1: unit test memerlukan ignored release evidence

Test `the manifest on disk` di `tools/library/test/manifest.test.ts` menjalankan
CLI generator terhadap root repository asli. CLI memerlukan
`dist/security-proof/results.json` serta delapan source reports lain.

Pada fixture checkout baru dengan test/generator/config actual dan dependencies
yang sama, tetapi **tanpa dist**, hasilnya **31/32 PASS, satu FAIL**, exit 1:

```text
Missing acceptance evidence dist/security-proof/results.json;
run its producing command first
```

Root verification menjalankan unit tests sebelum build/reuse. `dist/` ignored dan
security runner sendiri menjalankan unit suite. Karena itu checkout baru tidak
bisa mem-bootstrap gate lewat sequence yang didokumentasikan. Working repo yang
sudah punya reports menutupi dependency ini. Test juga berpotensi menulis generated
manifest tracked hanya karena `pnpm test` dijalankan.

Perbaiki menjadi filesystem/CLI integration test memakai temporary owned root dan
synthetic coherent evidence. Export writer seam atau gunakan fixture tree untuk
CLI; regular unit suite harus tidak bergantung release artifacts existing. Jangan
membuatnya PASS dengan skip test, membaca owner evidence atau menjalankan seluruh
security battery sebagai unit-test setup.

Test harus membuktikan fresh write, repeated write, formatter result dan changed
evidence benar-benar bekerja pada temp output. Root manifest asli tidak boleh
berubah. Reproducer yang bisa dijalankan:

```sh
fnm exec --using 22.22.3 node /tmp/archava-manifest-cold-review-20261015.mjs
```

Reproducer membuat/menghapus temp tree sendiri; tidak menghapus actual `dist` atau
mengubah owner state. Pada uji ini manifest owner tetap byte-identical. `/tmp`
helper bukan release evidence dan fixture paths perlu disesuaikan jika test helper
baru ditambahkan; permanenkan behavior tersebut dalam tests repo yang benar.

## J02 — P2: beberapa failures hilang dari refusal reasons

Additional cases memakai in-memory copy reports aktual:

1. `browser.result = 'failed'` tetap menghasilkan counts browser tetapi tidak
   membawa failed result atau reason browser failure dalam output.
2. `template-audit.exitCode = 2` dicatat dalam commandFailures tetapi dikecualikan
   dari reasons karena semua exit codes dari advisory command names diabaikan.
   Verifier sebenarnya mengizinkan **0 atau 1**, bukan 2/error lainnya.

`accepted` tetap null; review tidak mengklaim failed data diterima sebagai
`accepted: true` atau actual browser run gagal. Ini kelemahan evidence reporting:
failure yang known tidak boleh hilang ketika manifest menyimpulkan reasons.

Preserve explicit standalone result ketika source memang menyediakan field itu,
dan tambahkan reason bila result failed. Source tanpa result jangan diberi PASS
buatan. Advisory exception hanya untuk allowed code 1; codes lain tetap error.
Tambahkan negative tests tersebut. Certification/penetration-test flags tetap
limits scope; jangan menambah requirement enterprise berbayar untuk menyimpan
perbaikan tooling candidate lokal.

## J03 — P2: docs current masih bertentangan

- `docs/BUILD_STATUS.md` dan `docs/LOCAL_LIBRARY_RELEASE.md` masih 1.719 tests;
  latest security report dan generated manifest sudah 1.729.
- Release docs masih mengatakan `Operator handoff (KIT-09) is accepted`, kemudian
  menyebut scope yang sama candidate. Starter-kit ledger row sudah candidate,
  tetapi paragraph `KIT-09 is accepted` masih ada tanpa label historical.
- Link `../PROJECT_AUDIT_2026-10-14.md` di starter-kit doc menunjuk root file yang
  tidak ada; audit sebenarnya berada di folder docs yang sama.

Koreksi paragraphs current ke candidate atau labeli historical claim secara jelas.
Counts final mengikuti producing reports setelah tests berubah; jangan hardcode
1.729 jika suite berikutnya menambah cases lagi. Jangan menyebut semua A01–A09
closed hanya karena lint/format/test existing berhasil.

## J04 — P2: standalone pilot menurunkan lint coverage

Pilot `lint.config.mjs` mengaktifkan hanya `no-undef` dan `no-unused-vars`. README
secara eksplisit mengakui recommended rules lain dihilangkan. Ini tidak memenuhi
arahan mempertahankan quality checks; pemindahan folder tidak memberi alasan
menurunkan rule coverage.

Alasan bahwa recommended config tidak bisa di-load dari luar product tree diuji
langsung dan terbantah. `createRequire()` yang di-anchor ke product `package.json`
berhasil memuat `@eslint/js`. Dengan full config tersebut:

**61 recommended rules, tujuh files harness/config, 0 errors dan 0 warnings.**

Restore full recommended rules dengan anchored dependency resolution atau declared
standalone pinned dependencies. Tidak perlu mematikan guards, mengganti nama config
untuk menutupi pengurangan checks, atau memasang package sembarang. Revisi README
yang menyebut imports impossible/about twenty rules. Current `tsx` source adapter
masih harus diganti compiled baseline saat menyelesaikan benchmark/cold consumers;
pilot lengkap tetap mengikuti [prompt Rust](../ARCHXY_RUST_PILOT_TASK.md).

## Next executable task

1. Archxy menutup J01/J02 dan regression coverage, lalu J03.
2. Jalankan scoped suite normal serta temp checkout tanpa dist. Jalankan
   lint/typecheck/format yang terpengaruh; refresh unit/security evidence bila test
   count berubah, kemudian regenerate manifest final dari producing reports.
3. Setelah checks ini lulus, boleh commit lokal **delapan files koreksi KIT-09**:
   generator, declaration, test, generated manifest, BUILD_STATUS, release,
   handoff dan starter-kit ledger. Hooks aktif, existing `242549c` dipertahankan,
   tidak stage Codex/task/Rust changes, tidak push.
4. J04 adalah perubahan pilot terpisah. Report perubahan tooling candidate tidak
   boleh mengklaim full pilot accepted sebelum benchmark/reuse gates dijalankan.
5. Sesudah koreksi tooling tersimpan, lanjut product audit **A01–A03** terlebih
   dahulu, kemudian A04–A06 dan A07–A09/release identity sesuai task utama.
   K14/K22 dan full operating kits tidak dipromosikan oleh pekerjaan metadata ini.
