# Archxy — selesaikan migrasi Full Rust Archava

**Routing aktif 20 Oktober 2026:** Archxy 1 mulai dari
[ARCHXY_1_FULL_IMPLEMENTATION_TASK.md](ARCHXY_1_FULL_IMPLEMENTATION_TASK.md),
termasuk closure audit CRM C01–C07, seluruh own-lane scope dan integrasi akhir.
Archxy 2 memakai [full ERP task](ARCHXY_2_FULL_IMPLEMENTATION_TASK.md).
File ini menyimpan kontrak program dan riwayat instruksi; urutan/status dan
ownership historis di bawah mengikuti mandat aktif tersebut. PRD/security/data
requirements tetap berlaku; tidak perlu audit Codex antar milestone.

**Versi mandat:** 17 Oktober 2026, setelah audit RN01–RN08.  
**Target:** seluruh R00–R11 + OWN-V1 implemented dan verified lokal; kemudian
satu audit akhir Codex.  
**Mode kerja:** eksekusi berkelanjutan; checkpoint tidak mengakhiri tugas.

**Mulai dari [ARCHXY_NEXT_EXECUTION_PROMPT.md](ARCHXY_NEXT_EXECUTION_PROMPT.md):**
itu entry point aktif setelah audit 17 Oktober. §1.1–§1.4 di bawah menyimpan
konteks penugasan sebelumnya. Gunakan correction queue terbaru, pertahankan hasil
valid dan tuntaskan scope/completion contract pada §§2–11 tanpa menunggu audit
antar milestone.

## 1. Instruksi owner dan mandat eksekusi

Bro Archxy, owner menugaskan lu untuk **mengerjakan migrasi Archava sampai target
Full Rust selesai secara lokal**, termasuk reusable core/library, business
modules, backend, first-party web UI, SDK boundaries dan operational tooling.
Mandat kelanjutan juga mencakup **OWN-V1 owner self-service** dari
[Owner configuration PRD](docs/OWNER_CONFIGURATION_PRD.md) sebagai explicit product
scope delta, dengan acceptance matrix terpisah dari frozen migration inventory.
Codex akan melakukan audit independen setelah seluruh implementasi dan bukti
verifikasi siap. Baca file ini dan PRD sampai selesai, lalu langsung kerjakan.

Instruksi owner tetap: **kerjakan seluruh scope sampai implemented dan verified
lokal; kemudian Codex melakukan audit independen.** Angka progres dalam instruksi
historis bukan pengukuran kondisi sekarang.
Jangan mengakhiri giliran hanya dengan laporan satu milestone atau pertanyaan
“lanjut?”, “ready?”, “just say the word”, “mau commit dulu?”, atau “tunggu audit”.
Setelah checkpoint tersimpan, pilih remaining item berikutnya dan lanjut eksekusi.

**Ini tugas implementasi end-to-end, bukan tugas membuat rencana atau pilot saja.**
Jalankan R00–R11 pada [PRD Full Rust](docs/PRD_FULL_RUST.md). Setelah satu milestone
lulus self-verification, lanjut milestone berikutnya tanpa meminta owner mengetik
`continue`, `gas`, atau approval rutin. Milestone adalah checkpoint engineering;
jangan menjadikannya alasan berhenti menunggu audit Codex di tengah pekerjaan.

Owner sudah memilih target sampai UI Rust. Jangan meminta ulang pilihan
backend-only versus full UI, provider Gemini versus OpenAI, atau lokal versus
hosting. Tetap lokal dan mengikuti kebijakan gratis yang sudah dipilih.

Workspace produk:
`/home/haikaru/Archverse/Lab/Archava_Platform`.

Prototype pembanding existing:
`/home/haikaru/Archverse/Lab/Experiments/rust-pilot`.

File ini adalah mandat eksekusi terbaru untuk scope migrasi. Ia menggantikan
batas tugas historis “pilot saja”, “mulai R00/R01 lalu tunggu”, atau “jangan
integrasikan Rust ke produk” ketika batas tersebut khusus berasal dari penugasan
riset/pilot lama. Security, data integrity, ownership, commercial requirements dan
acceptance evidence existing tetap berlaku. Instruksi sistem dan instruksi owner
yang lebih baru tetap mempunyai prioritas.

### 1.1 Instruksi lanjutan setelah laporan R02 benchmark

Owner meminta pekerjaan **diteruskan sampai seluruh target selesai**, setelah
laporan pricing/rates, benchmark, formatting corrections dan manual proofs.
Laporan tersebut adalah checkpoint, bukan terminal deliverable atau permintaan
keputusan berikutnya. Baca current execution state, pertahankan hasil yang valid,
lalu langsung jalankan urutan di bawah.

1. **Tutup integration gap verification.** Sediakan repeatable `pnpm rust:verify`
   untuk Rust checks/parity/declared consumer regressions yang sesuai dan
   `pnpm rust:verify-release` untuk complete candidate verification, termasuk
   controlled benchmarks dan required end-to-end/upgrade/restore proofs. Hubungkan
   fast regression gate ke regular repo/CI verification dan full gate ke candidate
   release workflow; menambahkan scripts yang tetap hanya diingat untuk dijalankan
   manual belum cukup. Jaga dependency order agar clean checkout tanpa prior dist
   dapat bootstrap, tanpa recursive root/security/gate loops atau menimpa archives.
   Gunakan source/artifact/config/fixture identity yang sesuai; required FAIL,
   missing/stale proof dan actual child-process exit code wajib diteruskan. Full
   completion gate tidak boleh hijau selagi required inventory/proof belum lengkap.
2. **Bedakan scope dan verdict.** P1 `338 match + 12 error-class-only` bukan exact
   boundary parity atau approval Codex. Pulihkan supported error contract jika
   boundary dikirim ke consumer; jika hanya experimental bridge, catat explicit
   non-shipping/versioned disposition dan buktikan replacement contract. P2's five
   corrected expectations tetap dibedakan dari legacy matches. Pricing/rates proof
   tidak menutup deployment/assembly planning atau capacity math; tuntaskan required
   conformance/measurement coverage kedua slice itu sesuai PRD, bukan mengubah
   denominator R02 menjadi dua engine yang lebih mudah diukur.
3. **Kerjakan authority dan real persistence berikutnya.** Baca
   [interim review RP01–RP04](docs/ARCHXY_RUST_PROGRESS_REVIEW_2026-10-15.md).
   Request body tidak menjadi authority untuk tenant/role/grants/approval. Resolve
   verified identity/current memberships/resource scopes dan bound approval pada
   server; buktikan forged valid tenant/role/consent, revocation dan service/human
   intersection ditolak sebelum business operation. Identity lookup yang memang
   diperlukan boleh menggunakan DB. Tambahkan real non-owner PostgreSQL tests,
   bukan copied SQL string assertions; tutup relevant A/J/RP findings dan complete
   behavior inventory. Jangan menganggap malformed slug berarti tenant ownership.
4. **Selesaikan runnable vertical slice R01/R03.** Implement real login/session,
   contacts/pipeline read, reviewed command, authorized receipt recovery dan UI
   Leptos SSR/hydration. Pilih compatible stable Leptos/integration versions;
   latest search result yang RC bukan bukti tidak ada stable release. Jangan
   menunggu upstream 0.9/1.0 atau menghabiskan fase berikutnya pada framework
   comparison tanpa concrete incompatibility. Buktikan whole user flow pada real
   browser dengan synthetic records, termasuk denied/refresh/restart cases.
5. **Lanjut R04–R11 sampai habis.** Implementasikan tiap domain/cohort, UI/API/tool,
   provider/realtime contract, CLI, packaging, upgrade/recovery/restore dan legacy
   replacement yang masuk frozen inventory. Local self-verification membuka tahap
   berikutnya; pending independent audit bukan blocker untuk development. Capability
   yang dinyatakan locally_proven harus usable, bukan health endpoint/acknowledgement/
   empty handler yang hanya selalu menolak.
6. **Selesaikan OWN-V1 dalam rangkaian ini.** Owner memakai chat atau dashboard
   untuk inspect → plan → dependency/readiness/cost preview → confirm → actual
   supported configuration apply → status/recovery/history/compatible restore.
   Buktikan dua business-model consumers tanpa fork dan satu optional installed
   module enable/disable tanpa owner edit code/config files/terminal. Jalankan
   OWN-T01–OWN-T10; catat product delta terpisah dari migration parity. OWN-V2,
   arbitrary app generation dan 40 full kits tetap di luar tugas ini.
7. **Akhiri hanya dengan completed candidate dan audit packet**, atau genuine
   external blocker setelah independent work habis. Task-tracking tool yang gagal
   menyimpan status bukan blocker: gunakan filesystem checkpoint dan actual exit
   evidence. Jangan berhenti pada penjelasan bahwa sesuatu belum guarded;
   implementasikan guard-nya, verifikasi, lalu lanjut. Tidak perlu mengulang seluruh
   benchmark/security suite yang sudah valid tanpa affected-source/gate reason.

Scope tetap lokal/gratis dan owner-data preservation berlaku. Tidak ada izin baru
untuk live financial/email effects, paid provider calls, deployment/push, archive
overwrite atau active-owner database cutover. Self-verification dan final Codex
acceptance adalah dua facts berbeda; jangan menulis independent sign-off sendiri.

### 1.2 Mandat aktif setelah audit 16 Oktober: tuntaskan sampai candidate lengkap

Baca [audit RA01–RA07](docs/ARCHXY_RUST_AUDIT_2026-10-16.md) dan
[evidence audit](docs/audit/2026-10-16-rust-audit.json) terlebih dahulu. Bagian ini
menentukan next actions terbaru; §1.1 tetap berlaku untuk scope yang belum selesai.
Resume actual checkpoint dan pertahankan hasil valid; jangan restart dari nol.

Audit menemukan 228 Rust tests lulus serta parity dan dua cold consumers untuk
pricing/rates. Root 9/9 baru lulus sesudah benchmark dan consumer artifacts dibuat
manual; clean unit suite dan standalone Rust gate gagal. Hitung ulang bila source
berubah; angka snapshot bukan target test count yang harus dipaksakan.

**Arti “100%”: seluruh required behavior pada frozen migration inventory R00–R11
beserta OWN-V1 implemented, usable lokal dan lulus proof yang diwajibkan pada
exact candidate yang sama.** Independent review tetap pending. Jangan mengganti
completion menjadi jumlah file/crate, semua current tests hijau, backend-only,
atau 40 kit mempunyai folder. Jangan menurunkan scope PRD untuk mengejar angka.

#### A. Tutup tujuh temuan dengan regression proof

| ID   | Pekerjaan wajib                                                                                                    | Bukti penutupan minimum                                                                                                                                                                     |
| ---- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| RA01 | Resolve identity, tenant membership, current service/human grants/scope dan bound approval pada server             | Tanpa session atau forged valid tenant/role/approval ditolak; authorized operation bekerja; revoked grant ditolak pada execute/status/replay/recovery melalui actual HTTP + PostgreSQL      |
| RA02 | Hubungkan startup/readiness ke non-owner runtime, exact migration/checksum, required tables dan enabled/forced RLS | Host menolak serving saat tabel/policy/ledger/runtime salah; pooled tenant context tidak bocor setelah commit/rollback; actual query/DB tests, bukan copied SQL literal                     |
| RA03 | Hilangkan ketergantungan regular unit tests pada benchmark output yang belum dibuat                                | Fresh unit suite berjalan tanpa prior dist/target/owner evidence; required assertions benar-benar dijalankan, tidak silent early return                                                     |
| RA04 | Betulkan fast-gate dependency order dan release-report unit fixtures                                               | Standalone `pnpm rust:verify` berjalan setelah documented clean bootstrap, tanpa membuat consumer/profile output manual sebelum gate                                                        |
| RA05 | Sediakan cold CI bootstrap locked Cargo dependencies dan selaraskan compiler pin                                   | Registry/target kosong bisa bootstrap lalu offline verification; cache miss tetap bekerja; benchmark dan consumers memakai declared toolchain                                               |
| RA06 | Buat item-per-behavior/proof inventory dan actual candidate identity binding                                       | Coverage dihitung dari required IDs/proofs; current dirty source, locks/config/migrations/artifact hashes dan runtime terikat; missing/stale/wrong identity membuat gate FAIL               |
| RA07 | Perbaiki capacity numeric/date validation dan loop bounds                                                          | NaN/infinite/out-of-domain input ditolak sesuai versioned contract; extreme overflow dan horizon/allocation bounds menghasilkan error terkontrol; valid cases tetap sesuai corrected parity |

Simpan closure tiap finding: reproducer before/after, relevant source/test paths,
command/actual exit, candidate identity dan regular gate yang menjaganya. Prioritas:
RA03–RA05 untuk cold feedback loop, RA01–RA02 sebelum business operations,
RA06 untuk traceability, RA07 sebelum capacity reusable release. Independent slices
boleh disusun sesuai dependency; jangan menunggu owner memilih urutan.

Applicable A01–A09, J01–J04 dan RP01–RP04 juga memerlukan current disposition/proof.
Jangan mengikuti known authorization/financial/integrity bugs demi parity, atau
mengulang fix yang sudah terbukti hanya karena ID lama ada pada dokumen historis.

#### B. Sesudah temuan ditutup, lanjut seluruh produk

RA01–RA07 adalah prerequisite, bukan seluruh remaining backlog. Tuntaskan semua
R01–R11 pada §6: runnable Rust host, real identity/CRM/action/assistant, seluruh
frozen domain/cohort, Leptos UI, SDK/embed/page awareness, provider/voice/avatar,
first-party worker replacement, Rust CLI, upgrades, backup/restore, local
handover/rollback, packaging dan retirement. Planning/capacity tetap memerlukan
required parity/measurement/cold-consumer proof; rates tidak menggantikannya.

Selesaikan juga OWN-V1 dan OWN-T01–OWN-T10: real owner chat/dashboard journey
sampai actual apply, recovery/history dan compatible restore; actual optional
module enable/disable mengubah UI/API/tools. Developer CLI atau JSON preview
tidak menggantikan owner flow. Gunakan K05 agency dan K04 wholesale untuk core/CRM
proof sesuai PRD; pilih pasangan domain lain berdasarkan inventory/kontrak.

Jangan meminta owner memilih ulang scope, provider, framework, bisnis pertama
atau hosting. Stable dependency compatibility diperiksa melalui spike konkret;
latest release RC bukan alasan menghentikan UI atau mengganti framework diam-diam.

#### C. Loop kerja wajib sampai required backlog habis

```text
read checkpoint + actual working tree
→ pilih next unblocked inventory/proof item
→ implement behavior + UI/API/data boundary yang terkait
→ self-review + meaningful scoped verification
→ perbaiki failure sampai valid
→ simpan checkpoint/evidence/coherent changeset
→ langsung kerjakan required item berikutnya
```

Checkpoint, status message, satu commit atau satu green milestone tidak mengakhiri
tugas. Jangan mengirim final response “R03 done, ready for next” ketika pekerjaan
wajib lain masih bisa dilakukan. Codex tidak perlu mengaudit per milestone dan
bukan prerequisite untuk development berikutnya. Self-review tetap wajib.

Tool tracker tidak persisten, stale monitor, buffered output, test/compiler/format
failure, keputusan crate atau kebutuhan commit bukan external blocker. Gunakan
actual process exit dan filesystem checkpoint; perbaiki lalu lanjut. Jika sebuah
item perlu akses eksternal, checkpoint blocked pada affected proof dan kerjakan
semua independent work terlebih dahulu. Local synthetic/protocol suite tidak
memerlukan optional paid/live credentials; jangan memalsukan status live.

Prompt tidak dapat menghapus hard limit context/quota/session dari provider.
Simpan exact next action dan pending process handles sebelum batas itu; gunakan
available continuation untuk resume sendiri. Jika hard stop tidak bisa dihindari,
catat incomplete dan cara resume yang tepat. Jangan mengklaim masih bekerja di
background atau sudah complete ketika eksekusi sebenarnya berhenti.

#### D. Completion gate harus menilai seluruh mandat

Full gate memeriksa **frozen inventory, OWN-V1 coverage, seluruh required proofs
dan exact candidate identity**. Merangkai tujuh command existing saja tidak cukup.
Node-host browser/upgrade proof adalah baseline regression; tambahkan actual
Rust-host browser/upgrade/recovery proof. Jangan hardcode availability/completion
flags, invent proof IDs, memakai manual pre-warming sebagai hidden prerequisite,
atau mengganti error scanner dengan advisory tolerance.

Required missing/blocked/stale/failed item membuat completion FAIL. Retire legacy
code hanya bila actual supported behavior memiliki verified replacement; jangan
menghapus fitur agar remaining turun. Bind actual source/runtime/artifact bytes,
dan invalidate/rerun affected proof setelah perubahan. Jangan memakai baseline
inventory identity lama untuk menyatakan current run sudah diverifikasi.

#### E. Jangan close-out sebelum checklist ini lengkap

- [ ] RA01–RA07 dan applicable A/J/RP findings mempunyai closure/proof atau
      genuinely out-of-scope disposition yang tidak menghilangkan required behavior.
- [ ] Seluruh required R00–R11 inventory items implemented/replaced; zero required
      pending/placeholder/unexplained exclusion; required proof IDs terikat candidate.
- [ ] OWN-T01–OWN-T10 PASS dengan real browser dan verified backend effects.
- [ ] Full Rust application usable lokal tanpa hidden first-party Node/Python
      business runtime; supported vendor/build/test bridges tercatat eksplisit.
- [ ] Cold bootstrap, root regressions selama legacy supported, Rust checks,
      PostgreSQL/browser/SDK/media, two-consumer, performance, upgrade/restore/rollback
      dan full candidate gate PASS pada final candidate identity.
- [ ] Performance inconclusive/slower, advisory policy dan live-provider limits
      dilaporkan sesuai fakta; tidak ada toleransi untuk required integrity failures.
- [ ] Release/completion report, audit packet, machine-readable coverage/verdict
      dan operator commands pada §10–§11 lengkap, konsisten dan reproducible.
- [ ] Owner data/private config/prior archives aman; tidak ada unsolicited paid
      effects, deploy, remote push atau active-owner cutover.

Baru sesudah seluruh checklist terpenuhi, kirim **“Full Rust implementation
complete — ready for Codex audit”** dengan links laporan/audit packet, candidate
identity dan commands run/verify. Jangan menulis independent acceptance sendiri.
Jika checklist belum lengkap dan masih ada independent work, lanjut kerjakan.

### 1.3 Instruksi terbaru setelah reaudit: perbaiki hasil, lalu tuntaskan produk

Owner meminta mandat lengkap ini dijalankan lagi sampai selesai. Baca
[reaudit 16 Oktober](docs/ARCHXY_RUST_REAUDIT_2026-10-16.md) dan
[evidence reaudit](docs/audit/2026-10-16-rust-reaudit.json) sebelum memulai.
Bagian ini menentukan next actions terbaru; seluruh R00–R11, OWN-V1 dan
completion criteria pada file ini tetap wajib. Ini bukan tugas audit ulang,
menulis rencana, atau hanya menutup RF01–RF03 lalu berhenti.

**Resume actual source dan checkpoint.** Pertahankan finite-value validation,
library/HTTP tests dan hasil valid yang sudah dibuat. Reproduce findings pada
source sekarang; jangan menganggap status lama otomatis masih benar atau sudah
closed. Estimasi 10–15% adalah konteks audit, bukan angka yang boleh dinaikkan
melalui perubahan denominator atau status dokumen.

Kerjakan urutan berikut tanpa menunggu keputusan owner di antaranya:

1. **Pulihkan feedback loop yang benar.** Bereskan RA03, RA05 dan RF03: regular
   unit suite tanpa prior benchmark artifact, locked Cargo bootstrap yang bekerja
   pada registry cache miss, scoped formatting dan Clippy dengan rules tetap.
   Ukur actual exit; compile success tidak menggantikan test/format/lint success.
   Jangan memakai silent return, skip required tests atau blanket ignore.
2. **Tutup RF01 dan RA06 dengan actual evidence validation.** Required coverage
   berasal dari frozen behavior/proof manifest seluruh scope, bukan lima crate
   labels atau array apa pun yang kebetulan tidak kosong. Read/parse producing
   files, periksa actual result/required checks, recompute hashes dan bind current
   source/locks/config/migrations/runtime/artifacts. Tolak missing, stale, failing,
   invented, duplicate atau wrong-identity proof. Cached `exists: true` dan
   non-empty SHA bukan bukti file ada/valid. Reference Node CRM benchmark tidak
   membuktikan Rust capacity; npm kit proof tidak membuktikan Rust CRM/UI.
   Tambahkan negative tests ketika dist tidak ada dan inventory hanya mempunyai
   satu invented proof. Fast regression gate harus dapat PASS pada partial
   implementation yang valid; full completion gate harus tetap incomplete sampai
   seluruh required scope memang selesai. Jangan memalsukan proof untuk membuat
   full gate hijau lebih awal.
3. **Selesaikan actual authority/readiness dan owned DB fixture.** Tutup RA01,
   RA02 dan RF02 pada real host path. Resolve session/principal/current membership,
   service/human scope intersection dan bound approval di server. Valid tenant
   slug/role/body approval bukan authority. Hubungkan async startup assertion
   sebelum serving, termasuk required ledger/checksum dan enabled/forced policies.
   Fixture harus membuat dan menjaga instance/roles/datanya sendiri; runtime dan
   seed connections menuju owned instance yang sama. Hapus fixed seeding target
   `/tmp/pgdata` dan kebutuhan setup manual yang tidak disediakan gate. Buktikan
   context tidak bocor pada physical pooled connection sebelum begin berikutnya
   menimpa setting. Jangan menyamakan `declined` dengan grant revocation.
4. **Buktikan satu real product flow sekarang.** Runnable Rust host dan Leptos
   SSR/hydration: login → contacts/pipeline → reviewed business command → actual
   persisted effect → durable receipt → authorized recovery setelah refresh,
   lost response dan restart. Jalankan API/browser/PG negative tests untuk
   unauthenticated/forged tenant/approval, revocation, CSRF dan stale revisions.
   Static screen, acknowledgement, router-only tests atau endpoint open/rollback
   saja tidak menutup slice. Jangan menghabiskan fase berikutnya pada reporting
   atau benchmark yang source-nya tidak berubah sementara flow ini belum ada.
5. **Tutup remaining R02 termasuk RA07, lalu R03–R11.** Calendar overflow harus
   mengembalikan error terkontrol, loop harus maju dan input/horizon/allocation
   bounds harus dibuktikan. Lengkapi planning/capacity conformance/measurement/
   cold-consumer requirements. Selanjutnya tuntaskan seluruh required domain,
   UI/API/tools/jobs, SDK/provider/realtime, operational CLI, packaging,
   fresh/prior upgrade, restore, local handover/rollback dan verified legacy
   replacement sesuai inventory. Ikuti atomic cohort dan dependency boundaries.
6. **Tuntaskan OWN-V1 dan final verification.** Dua model bisnis memakai identical
   core/CRM artifacts tanpa fork. Owner melalui chat/dashboard melakukan inspect,
   plan, dependency/readiness/cost preview, bound confirmation, actual supported
   apply, status/recovery/history/compatible restore. Jalankan OWN-T01–OWN-T10.
   Uji optional installed module enable/disable dan actual UI/API/tool effect.
   Setelah final source siap, jalankan seluruh required gates pada identity yang
   sama dan siapkan release serta audit packet pada §11.

Setiap langkah menghasilkan **working behavior + meaningful tests + checkpoint**,
lalu lu langsung lanjut langkah berikutnya. Affected-source changes membatalkan
affected proof; unchanged valid work tidak perlu dibuang atau diukur berulang
tanpa alasan. Context penuh diatasi dengan durable continuation state; task
tracker yang rusak tidak menggantikan pekerjaan yang masih bisa dilakukan.

**Jangan mengirim close-out “done” untuk koreksi audit saja.** Final response baru
dikirim setelah seluruh required R00–R11 + OWN-V1 implemented, working local
candidate reproducible, required gates PASS dan audit packet lengkap. Gunakan
`implementation complete — ready for Codex audit`; independent acceptance tetap
pending. Batas lokal/gratis, owner-data preservation dan external-effect/Git
policy di file ini tetap berlaku. Genuine external blocker mengikuti §4;
routine test/compiler/framework/format problems dikerjakan, bukan dilempar ke owner.

### 1.4 Mandat aktif setelah audit malam: selesaikan seluruh candidate

Bro Archxy, lanjut dari actual working tree sampai **seluruh Full Rust R00–R11
dan OWN-V1 selesai diimplementasikan, usable lokal dan verified**. Codex audit
setelah candidate lengkap siap. Ini mandat implementasi sampai tuntas; menutup
temuan audit adalah awal rangkaian, bukan hasil akhir tugas.

Baca [audit malam 16 Oktober](docs/ARCHXY_RUST_AUDIT_2026-10-16_EVENING.md),
[evidence JSON](docs/audit/2026-10-16-rust-evening-audit.json), checkpoint,
required-coverage manifest, PRD Rust dan owner PRD. Cocokkan dengan source sekarang;
jangan menganggap cached status lebih benar dari current behavior.

**Baseline yang sudah terbukti:** Rust gate 10/10, 254 workspace tests, 1.821 Node
unit tests/137 files; readiness sudah terhubung ke public host constructor; cold
bootstrap bekerja; artifact assertions terpisah dan benar-benar FAIL tanpa build,
lalu 4/4 PASS sesudah benchmark. Evidence validator sudah memeriksa actual files,
hashes dan identity. Pertahankan hasil ini dan regression-nya. Root verification
masih 8/9 karena format. Angka tersebut snapshot audit, bukan target jumlah tests
atau ukuran persentase completion.

#### A. Kerjakan koreksi terbaru terlebih dahulu

1. **RF04 — fixture cleanup.** `pg-fixture` membaca `<fixture>/postmaster.pid`,
   padahal server memakai `<fixture>/data/postmaster.pid`. Correct PID/data-dir
   lookup, ownership dan live-instance protection wajib sebelum stale directory
   dihapus. Missing/unreadable PID bukan bukti instance aman dihapus. Lindungi
   provisioning yang masih berjalan dan cleanup kegagalan setelah server start
   dengan lifecycle guard yang dibuat sebelum resource itu dapat bocor. Uji live
   fixture berusia lebih dari 300 detik, dead fixture, concurrent/in-flight runs
   dan failure-path teardown. Gunakan controlled metadata/clock atau canary
   sendiri; jangan menunggu lima menit atau merusak fixture orang lain sebagai
   test. Jangan menghapus massal directory hanya berdasarkan prefix/age.
2. **Format dan instruksi reproduksi.** Perbaiki enam file Prettier yang disebut
   audit dengan formatting terarah; jangan menjalankan global formatter atas
   pekerjaan yang kepemilikannya belum jelas. Selaraskan komentar artifact tests
   dengan actual release gate: benchmark dahulu, artifact assertions sesudahnya.
   Pertahankan default unit suite yang cold-safe dan hard assertions yang tidak
   silent skip. Jaga declared toolchain dan cold dependency closure; selesaikan
   reproducibility gap consumer closure yang masih melakukan unlocked fetch.
3. **RA01 — authority dari server.** Saat ini unauthenticated request dengan
   tenant/role/approval buatan caller bisa mendapat 200 `allowed` pada actual
   router setelah readiness PASS. Resolve verified principal/session, current
   membership, service/human grants dan resource scope pada server. Bound approval
   mengikat actor, tenant, command/input hash, revision, expiry dan single-use
   semantics sesuai kontrak. Body boleh membawa requested target; ia tidak
   memberikan kewenangan. Human approval tidak dibuktikan boolean dari client.
   Tolak unauthenticated, forged valid tenant/role/capability/approval, missing
   scope dan revoked grant pada execute/status/replay/recovery. Buktikan juga
   positive authorized path, CSRF refusal dan unchanged data pada refusal dengan
   actual HTTP + non-owner PostgreSQL. Tetap jalankan startup/readiness sebelum
   serving; jangan membuat constructor bypass untuk mempercepat tests.
4. **RA07 — numeric/calendar safety.** Retain finite-value validation; ganti
   overflow yang mengembalikan Instant lama dengan controlled error/contract.
   `count_working_days(i64::MAX-1, i64::MAX)` tidak boleh hang. Tetapkan supported
   timestamp/horizon/allocation/resource bounds dan termination guarantee pada
   semua walkers/callers, lalu uji near-boundary/overflow/wide-range serta valid
   corrected-parity cases. Jangan hanya memberi timeout pada caller atau
   mengabaikan fixture ekstrem agar suite hijau.

Tiap closure harus punya actual source/test paths, meaningful regression, command
dan exit, serta evidence yang sesuai candidate. Positive suite hijau tidak
menggantikan reproducer negatif. RF02 isolation membaik tetapi lifecycle belum
closed sebelum RF04 selesai. Jangan mengulang audit atau benchmark yang tidak
terpengaruh sebagai pengganti menyelesaikan behavior berikutnya.

#### B. Wujudkan produk yang bisa dipakai, lalu selesaikan seluruh scope

Setelah authority aman, buktikan **satu executable Rust host + Leptos SSR/WASM**:
login → contacts/pipeline → reviewed command → persisted business effect →
durable receipt → authorized recovery setelah refresh, lost response dan restart.
Gunakan real browser dan periksa stored PostgreSQL state. Handler yang hanya
evaluate/open/rollback, tampilan statis dan placeholder contacts tidak menutup
slice. Pilih compatible stable framework release berdasarkan primary docs dan
concrete spike; latest search result yang RC bukan alasan menunggu upstream.

Lanjut otomatis sampai seluruh checklist §6 dan PRD terpenuhi:

- **R02:** pricing, assembly/deployment planning dan capacity conformance,
  corrected wire/error semantics, compiled benchmarks dan cold consumers.
  Pricing/rates yang sudah terbukti tidak menggantikan planning/capacity proof.
- **R03–R04:** identity/CRM/action/assistant dan operational UI; organizations,
  proposals/scope/amendments, projects, capacity pools/reservations/delivery queue.
- **R05–R08:** scheduling/accommodation/assets, stock cohort commerce/purchasing/
  foodservice, billing/subscriptions/entitlements, attendance/support/fulfillment,
  laundry/field-services/education dan seluruh extension pada frozen inventory.
  Pertahankan atomic cohorts, trusted references dan current authorization.
- **R09:** remaining knowledge/context/decision/assistant behavior, reusable
  SDK/widget/page awareness, provider budgets/recovery, voice/avatar/realtime
  boundaries sesuai supported baseline. Public dan owner tools tetap terpisah.
- **R10–R11:** Rust operating CLI, package/assembly/release tooling, minimal cold
  consumers, fresh/prior upgrades, backups dengan tested restore, fenced writer
  handover/rollback pada fixture, documented run/stop dan verified legacy
  replacement. Tidak ada hidden first-party Node/Python business runtime dalam
  target candidate. Keep supported test/build/vendor bridges explicit.
- **OWN-V1:** K05 agency dan K04 wholesale memakai identical core/CRM artifacts
  tanpa fork. Chat/dashboard memakai service/plan contract yang sama untuk inspect
  → plan → dependency/readiness/cost preview → bound confirm → actual supported
  apply → status/recovery/history/compatible config restore. Satu optional
  installed module enable/disable benar-benar mengubah UI/API/tools; disabled
  direct API dan dependent disable ditolak. Owner tidak mengedit source/config
  files atau memakai terminal. Buktikan **OWN-T01–OWN-T10** secara terpisah dari
  migration parity. Config restore tidak menulis ulang financial/business history.

Tidak menambah OWN-V2, arbitrary app generator atau 40 full operating kits ke
mandat ini. Tidak mengurangi supported baseline agar pekerjaan terlihat selesai.
Applicable A/J/RP findings tetap membutuhkan closure atau justified disposition
yang mempertahankan required behavior. Source library yang bisa dirakit adalah
hasil yang dituju; puluhan folder/crate tanpa workflow usable tidak cukup.

#### C. Kerja terus, dengan checkpoint yang bisa dilanjutkan

Pilih next unblocked required item → implement → scoped checks → fix → simpan
proof/checkpoint → langsung kerjakan item berikutnya. Jangan mengakhiri giliran
dengan “RF04 done”, “milestone done, lanjut?”, atau menunggu Codex per slice.
Progress update tetap diberikan; milestone atau commit tidak mengakhiri mandat.

Checkpoint mencatat remaining **behavior dan proof IDs**, next executable action,
source identity dan pending process/session/log/exit. Context penuh atau tracker
tidak persisten ditangani dengan durable checkpoint dan continuation yang
tersedia. Tidak ada prompt yang menghapus hard quota/session limit; jika terpaksa
berhenti, tulis incomplete dan exact resume action, jangan mengaku masih bekerja
di background. Genuine external blocker hanya menghentikan affected work; kerjakan
seluruh independent work terlebih dahulu dan jangan memalsukan blocked proof.

Tetap lokal/gratis, Gemini sesuai pilihan existing, no unsolicited paid/provider
effects, email/payment nyata, deploy, push atau active-owner cutover. `.env`,
`.local`, owner DB dan historical archives dijaga. Synthetic/protocol tests tidak
diganti klaim live. Jalankan service hanya saat dibutuhkan, cleanup milik task
sendiri dan pertahankan user/agent services lain. Browser QA mengikuti applicable
Playwright skill dengan unique session dan session close setelah selesai.
Local commits mengikuti §10 dengan hooks aktif; jangan stage pekerjaan orang lain.

#### D. Hanya tutup sebagai complete bila seluruh syarat ini terpenuhi

- Seluruh required R00–R11 behavior dan OWN-T01–OWN-T10 implemented, usable dan
  terbukti; RF04/RA01/RA07 serta applicable findings closed. Tidak ada required
  missing/placeholder/blocked path yang disamarkan sebagai PASS atau N/A.
- Frozen coverage manifest tetap lengkap. Tambahkan finer-grained behavior
  traceability bila diperlukan; jangan menghapus required IDs, mengecilkan scope,
  menulis self-declared PASS atau mengubah tests agar validator tampak complete.
  Missing/stale/wrong-hash/wrong-identity/failed proof tetap membuat full gate FAIL.
- Final `pnpm verify`, `pnpm rust:verify`, `pnpm rust:verify-release` dan semua
  required Rust/API/PG/browser/SDK/consumer/upgrade/restore/rollback proofs PASS
  pada candidate yang sama melalui documented clean bootstrap. Gunakan existing
  aggregate ordering agar tidak mengulang tiap expensive proof tanpa alasan.
  Node-host regression adalah baseline; ia tidak menggantikan Rust-host proof.
- Runtime/artifact/config/lock/migration/source identity terikat actual bytes.
  Source berubah sesudah proof → invalidate/rerun affected proof. Version/release
  identity baru konsisten; existing 1.17.0 archive tidak ditimpa. Performance yang
  slower/inconclusive dan advisory/live limitations dilaporkan sesuai fakta.
- `docs/rust/FULL_RUST_COMPLETION_REPORT.md`,
  `docs/rust/FULL_RUST_AUDIT_PACKET.md`, current execution state dan machine-readable
  coverage/verdict lengkap. Candidate dapat dijalankan dan dihentikan dari
  documented clean local profile. Independent review tetap **pending**.

Kalau seluruhnya terpenuhi, kirim **“Full Rust implementation complete — ready
for Codex audit”** beserta report/audit packet, candidate identity, actual commands
dan evidence. Kalau masih ada required work yang dapat dikerjakan, **lanjut kerja**.
100% berarti scope implementasi dan local proofs terpenuhi; audit independen
Codex tetap tahap berikutnya, bukan sign-off yang lu isi sendiri.

## 2. Konteks produk yang harus tetap benar

- **ARCHZXY adalah company; Archava adalah product.** Website company dibangun
  sebagai consumer terpisah di luar repository ini.
- Targetnya reusable modules yang dapat dirakit lintas model bisnis tanpa fork.
  Rust adalah implementasi; composition/configuration/versioning tetap kontrak.
- Pertahankan modular monolith. PostgreSQL tetap source of truth. LLM meminta
  tindakan; server menentukan kewenangan dan melakukan validasi/eksekusi.
- Customer/public dan owner/employee mempunyai identity, grants, scopes, data dan
  available tools yang berbeda.
- Product scope/harga tetap mengikuti [master PRD](PRD.md), [agent rules](agent.md)
  dan `config/pricing.v1.json`. Jangan mengubah harga atau package tiers karena
  backend pindah bahasa.
- “Semuanya selesai” pada tugas ini berarti **seluruh migration inventory yang
  dibekukan pada R00 ditambah explicit OWN-V1 scope delta**. Track keduanya terpisah.
  Ini bukan membangun semua ide masa depan, OWN-V2 atau 40 full business kits.
  Kit readiness tetap dihitung berdasarkan evidence tersendiri.

## 3. Baca sumber ini sebelum perubahan pertama

**Mulai dari audit terbaru:**
[audit 17 Oktober: RN01–RN08](docs/ARCHXY_RUST_AUDIT_2026-10-17.md),
[evidence JSON](docs/audit/2026-10-17-rust-audit.json), lalu
[prompt aktif](ARCHXY_NEXT_EXECUTION_PROMPT.md).
Referensi audit sebelumnya di bawah dipakai untuk traceability dan full scope.

**Product clarification terbaru:** Baca
[Owner configuration PRD](docs/OWNER_CONFIGURATION_PRD.md) dan master PRD §51.
Target reuse juga mencakup owner merakit supported modules/options lewat Archava
atau dashboard tanpa code/config-file/terminal steps. Bedakan internal Studio
dengan tenant-owner authority. Catat explicit scope delta dan map overlapping
config/plan/approval/history primitives ke roadmap; new owner-facing features
dilaporkan terpisah dari frozen migration parity. OWN-V1 sekarang wajib diselesaikan
dalam mandat kelanjutan §1.1; OWN-V2/later tetap backlog. Jangan mengklaim builder selesai
karena assembly CLI bekerja, memperluas denominator diam-diam, atau membuang
pekerjaan Rust yang sedang berjalan. Authority/data baseline tetap prioritas.

1. [Audit 16 Oktober, RA01–RA07](docs/ARCHXY_RUST_AUDIT_2026-10-16.md),
   [evidence JSON](docs/audit/2026-10-16-rust-audit.json) dan
   `docs/rust/EXECUTION_STATE.md`. Mulai dari remaining work aktual; jangan
   mengulang seluruh milestone yang sudah mempunyai proof valid.
2. [PRD Full Rust](docs/PRD_FULL_RUST.md), termasuk G01–G10, R00–R11 dan mapping
   seluruh package directories pada §26.3.
3. [Master PRD](PRD.md), [architecture](docs/ARCHITECTURE.md),
   [active/historical implementation task](ARCHXY_IMPLEMENTATION_TASK.md).
4. [Audit produk](docs/PROJECT_AUDIT_2026-10-14.md),
   [review KIT-09 pertama](docs/ARCHXY_KIT09_REVIEW_2026-10-14.md),
   [review koreksi](docs/ARCHXY_KIT09_REVIEW_2026-10-15.md).
5. [Riset Rust](docs/RUST_MIGRATION_RESEARCH_2026-10-14.md) dan
   [prompt pilot](ARCHXY_RUST_PILOT_TASK.md), untuk mempertahankan fixtures dan
   findings yang berguna tanpa mengikuti batas scope pilot yang sudah diganti.
6. Relevant source/tests, release catalogue, kit catalogue, SQL ledgers, scripts,
   operator commands dan semua consumers yang diinventaris pada R00.
7. Applicable `AGENTS.md`/workspace rules. Baca
   `Archava Template/AGENTS.md` sebelum mengubah subtree template.

Source aktual mengalahkan snapshot angka pada laporan lama untuk inventory.
Requirement authority dan corrected expected behavior mengalahkan source yang
terbukti mempunyai bug. Jangan menerima komentar “CLOSED” tanpa recheck proof.

## 4. Cara bekerja terus sampai selesai

- Langsung eksekusi, self-review, jalankan checks yang relevan, perbaiki failure,
  lalu lanjutkan slice berikutnya. Jangan berhenti setelah scaffolding, satu
  benchmark, satu domain, atau satu browser screen.
- Putuskan routine implementation details sendiri dalam batas PRD. Toolchain dan
  dependency choices dicatat dalam ADR; tidak perlu meminta owner memilih setiap
  crate, nama folder, urutan internal, atau keputusan reversible.
- Simpan status/checkpoint sebelum context penuh atau sesi terputus. Pada
  continuation baca checkpoint dan lanjut dari next action, bukan ulang riset.
  Ikuti mekanisme continuation yang tersedia; prompt ini tidak dapat menghapus
  hard limit context, quota, tool runtime atau sesi dari penyedia agent.
- Tetap kirim update singkat tentang hasil, blocker dan langkah aktif. Progress
  update tidak mengakhiri pekerjaan dan tidak meminta permission rutin.
- Gunakan commands langsung atau managed process/session dengan exit code yang
  dapat dikumpulkan. Catat PID/session/output path bila background diperlukan.
  Jangan membuat stale monitor baru atau menunggu pattern buffered sebagai
  pengganti hasil process yang authoritative.
- Jika suatu integrasi buntu, baca primary docs/source, buat reproducer, uji
  alternatif yang masih memenuhi PRD dan lanjut independent work. Jangan
  menurunkan tests, mengganti vendor diam-diam, atau membatasi scope agar bisa
  mengumumkan selesai.
- Berhenti karena blocker hanya bila memang perlu informasi/akses eksternal yang
  tidak dapat diperoleh dan tidak ada remaining independent work yang berguna.
  Laporkan exact blocker dan affected gate; jangan melabelinya completed.
- Audit independen Codex dilakukan setelah seluruh candidate siap. Lu wajib
  melakukan self-review dan checks sendiri selama implementasi; jangan memakai
  “nanti diaudit” untuk melewati gate.

## 5. R00 — Bekukan scope, data boundary dan baseline

Sebelum mengubah source, catat actual HEAD, branch, tracked/untracked changes,
source/build-input hashes, tool versions, dependency locks dan current evidence.
Working tree sudah mempunyai pekerjaan existing; jangan reset, stash, clean,
amend history, memindahkan folder produk, atau stage file orang lain secara massal.

Snapshot PRD menyebut 64 package directories, 62 release artifacts/23 ledgers,
assembly17 pada candidate 1.17.0, versus arsip 1.17.0 yang berbeda. **Hitung ulang**;
jangan memaksa hasil aktual sama dengan angka itu. Formal education candidate
termasuk scope bila berada pada frozen baseline; jangan menghapusnya agar port
terlihat selesai. New feature work setelah freeze menjadi explicit delta backlog,
tidak otomatis memperluas denominator tanpa batas.

Buat:

- `docs/rust/MIGRATION_INVENTORY.md` dan `docs/rust/migration-inventory.json`;
- `docs/rust/EXECUTION_STATE.md`;
- `docs/rust/DECISIONS.md`;
- `docs/rust/KNOWN_DIFFERENCES.md`;
- evidence directory dengan release-specific identity, mengikuti convention PRD.

Setiap inventory item mencakup ID, existing source/entrypoints, behavior, consumers,
contracts/errors, security/scope, data/ledger, transaction cohort, target/replacement,
required proof IDs, status dan blockers. Jangan membatasi inventory ke packages:
masukkan apps, browser panels/routes, SDK/page awareness, standalone template,
Python worker, provider adapters, jobs/webhooks, operational CLI dan release tools.

Recheck outstanding A01–A09 dan J01–J04 sesuai latest source. Perbaiki affected
baseline dan buat meaningful regression/conformance tests. Findings yang sudah
ditutup harus mempunyai reproducer/proof; angka unit/control PASS tidak cukup.
Jangan meniru security/financial bugs demi parity. Bekukan corrected expectations
dan explicit contract differences sebelum affected Rust slice diterima lokal.

Data owner tetap utuh. Mandatory tests/builds memakai synthetic data dan isolated
DB. Jangan memakai owner DB sebagai test target atau melakukan backup restore ke
DB aktif. Private backup/restore rehearsal, bila menggunakan data existing sesuai
otorisasi yang berlaku, hanya di target clone yang diverifikasi dan tidak masuk
logs, Git, public evidence atau provider requests.

## 6. Implementasikan seluruh R01–R11

Gunakan urutan dan acceptance criteria PRD. Tabel ini adalah execution checklist,
bukan pengganti detail requirement di PRD.

| Tahap | Kerjakan sampai usable dan terbukti                                                                                                                                                                  |
| ----- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| R01   | Rust workspace/toolchain/dependencies, contracts/errors/policy, SQLx transaction dengan non-owner RLS, Axum host, Leptos SSR/hydration, login/contact view/one reviewed command/recovery feasibility |
| R02   | Pricing, deployment/assembly planning, capacity math, corrected parity, fair compiled TS baseline dan two-consumer proofs; reference-rate pilot dicatat terpisah                                     |
| R03   | Identity/sessions/CSRF, CRM contacts/pipeline, confirmations/receipts/recovery, basic assistant chat/planning dan operational UI; K05/K04 memakai identical core/CRM                                 |
| R04   | Organizations, proposals/agreed scopes/amendments, projects, capacity pool/reservations/queue beserta real UI/API/CLI paths                                                                          |
| R05   | Scheduling/accommodation/shared occupancy dan asset boundaries; booking overlaps, cancellation, concurrency/recovery                                                                                 |
| R06   | Commerce/purchasing/foodservice stock cohort dan related fulfillment/asset effects; satu physical stock pool, same-transaction receipts/audit/claims                                                 |
| R07   | Billing/subscriptions/entitlements, attendance, support dan fulfillment dengan correct financial/relationship/delegation semantics                                                                   |
| R08   | Laundry, field-services dan formal-education/other frozen vertical extensions; actual domain behavior, scoped authority dan trusted references                                                       |
| R09   | Remaining context/knowledge/decision/assistant behaviors, provider budgets/recovery, reusable public SDK/widget/page awareness, voice/avatar/realtime parity                                         |
| R10   | Rust operational CLI, assembly/pack/verify, cold consumers, fresh/prior upgrades, backup/restore, local release archive, single-writer handover dan rollback rehearsal                               |
| R11   | Close seluruh frozen inventory, remove required legacy-runtime dependencies dari Rust candidate, reconcile docs/evidence, prepare complete audit handoff dan supported legacy-retirement disposition |

Milestone yang membutuhkan beberapa commits tetap diselesaikan. Jangan membuat
puluhan placeholder crates terlebih dahulu. Ikuti dependency/transaction graph;
boleh mengubah urutan independent slices dengan alasan pada checkpoint. Tidak
boleh memecah atomic cohort hanya agar sesuai urutan tabel.

Local verified checkpoint cukup untuk melanjutkan tahap berikutnya; tidak perlu
menunggu reviewer eksternal di setiap tahap. Catat ADR/corrected-contract decisions
yang memerlukan final audit secara eksplisit. Final independent acceptance tetap
pending sampai Codex benar-benar mereview candidate yang sama.

## 7. Full Rust dan browser/vendor boundaries yang konkret

Target aplikasi first-party adalah Rust backend/library + Leptos web UI. HTML/CSS,
SQL, JSON, generated JS/WASM loader dan thin vendor/compatibility bridges sah.
Tidak ada hidden TypeScript engine untuk jalur yang disebut Rust.

Yang benar-benar ada pada realtime template:

- `@spatius/avatarkit` dan `@spatius/avatarkit-rtc` untuk avatar rendering/transport;
- `livekit-client` untuk browser realtime connection/media;
- `@livekit/components-react` untuk existing React controls/hooks;
- Node session server dan Python LiveKit agent/plugin orchestration.

Rust UI dapat memakai documented bridge ke vendor browser SDK. UI React controls
diganti dengan Leptos equivalents untuk full target. Jangan menulis ulang vendor
avatar renderer, WebRTC stack atau SDK engine demi memaksakan nol JavaScript.
LiveKit mempunyai Rust SDK; verifikasi supported targets dan exact behavior, bukan
menganggapnya drop-in pengganti React UI atau Python Agents framework.

Port first-party worker/orchestration behavior ke Rust melalui supported protocol/
SDK bila target membutuhkan replacement. Retaining entire first-party Python/Node
worker sebagai “adapter kecil” tidak menutup inventory item tersebut. Jika vendor
mempunyai hard constraint yang belum teratasi, catat gate blocked dan lanjutkan
pekerjaan lain; jangan menyatakan Full Rust complete dengan runtime lama tersembunyi.

Existing Gemini CRM dan Postmark adapters menggunakan HTTP seams; mereka tidak
memerlukan JavaScript runtime hanya karena source existing TypeScript. Pertahankan
provider contracts, free/local policy, idempotency/reconciliation dan privacy.

Website eksternal boleh tetap React/Next/Vue/plain HTML. Public embed/SDK harus
tetap mudah dipasang tanpa client harus ikut migrasi Rust. SDK compatibility
facade tidak membawa owner credentials atau internal owner tool catalogue.

## 8. Data, transactions, authority dan recovery: jangan kompromi

- Same PostgreSQL schemas/ledgers/migration bytes/checksums; perubahan integrity
  baru additive dan versioned. Jangan menjalankan fresh migrator terhadap existing
  DB lalu menganggap semua SQL belum diaplikasikan.
- Preserve IDs, relationships, revisions, timestamp microseconds, historical
  approvals/audit/receipts, password/session/key derivation dan fingerprint framing.
  Gunakan vetted crypto dan synthetic cross-language golden fixtures.
- Current tenant/actor/grants/scope dicek sebelum receipt/replay result dikembalikan.
  Intersect service dan represented-human constraints; scope bukan numeric rank.
- Preserve single transaction untuk stock/occupancy/financial invariants. One writer
  per tenant/domain cohort mencakup routes, tools, CLI, jobs, webhooks dan recovery.
- No dual writes untuk shadow comparison ke owner/provider. Compare synthetic data,
  captured requests atau allowed read-only snapshots.
- External outcomes dapat unknown; reconcile melalui authoritative status. Jangan
  membayar/mengirim/memutasi ulang dengan UUID baru agar recovery terlihat berhasil.
- Provider signed events/backend authoritative untuk settlement. Paid entitlement
  tidak aktif hanya karena draft/manual reference/chat/LLM menyebut sudah dibayar.
- UI/SSR/hydration/browser assets tidak membocorkan hidden fields, keys, prompts,
  tokens, owner records atau privileged server modules.

## 9. Verification yang wajib dikerjakan sampai tuntas

Semua G01–G10 harus dievaluasi sesuai frozen scope dan exact candidate identity.
Tidak boleh menghapus test lama, mengurangi lint/security coverage, mengecilkan
fixture set, silent fallback, blanket ignores, bypass hooks, atau mengganti
FAIL/divergence menjadi PASS tanpa approved corrected expectation.

Pertahankan existing pnpm regressions selama affected TypeScript paths masih
supported. Root menyatakan pnpm10.15.1 dan Node >=22.22.3; baseline comparisons
memakai selected supported runtime yang konsisten dan tercatat, bukan mengambil
Node berbeda diam-diam. Rust toolchain/Cargo.lock/build tools dipin.

Checks harus mencakup:

1. Scoped tests selama implementasi; root gates dan relevant domain regressions
   pada corrected baseline/final cross-language integration. Jangan menjalankan
   semua pack scripts berulang tanpa dependency/change reason.
2. Rust format, Clippy, compile/tests, selected native/browser feature matrix,
   dependency/license/advisory checks dan build-script review.
3. Real non-owner PostgreSQL authority/relationship/race/rollback/recovery cases,
   termasuk reproducer A01–A05/A08 dan analogous cases semua ported domains.
4. Real browser SSR/hydration/API/SDK, CSRF/direct server-function attacks,
   approval refresh/two tabs/lost response, revocation/logout/reconnect, accessibility
   dan voice/avatar lifecycle. Screenshot saja tidak membuktikan behavior.
5. Two independently installed consumers untuk reusable core/CRM dan tiap module
   sesuai required manifest proof; minimal declared closure, no workspace/path hack.
6. Cold checkout/build tanpa dist/target/private env/owner DB. Unit tests memakai
   owned synthetic evidence; verification tidak membutuhkan proof yang baru akan
   dibuat oleh verification itu sendiri.
7. Prior-profile upgrade, legacy command recovery, exact SQL/key/time semantics,
   synthetic backup/restore dan compatible rollback rehearsal.
8. Fair compiled TS vs release Rust benchmarks, setup/steady-state dipisah,
   CPU/RSS/latency/throughput/UI/build metrics, repeated samples dan absolute costs.
   No universal speed/security claim; slower/inconclusive wajib dilaporkan.
9. Missing/stale/empty/wrong-hash/profile artifacts dan failed standalone/advisory
   proof negative cases. Required failures wajib masuk verdict dan refusal reasons.

Kumpulkan evidence final setelah source/artifacts final. Jika source berubah,
invalidate dan rerun affected proofs; jangan mengemas hasil snapshot lama ke
release baru. Preserve prior archives; jangan menimpa 1.17.0 collision dengan
contents yang berbeda. Pilih new unique release identity dan compatibility path.

Mock/synthetic integrations membuktikan local protocol/behavior yang diuji.
Vendor live readiness tetap diberi status terpisah bila belum diuji. Jangan
memanggil paid providers, mengirim email/payment nyata, publish/deploy atau membuat
hosting untuk melengkapi angka PASS. Mandatory local suite tidak memerlukan itu.

## 10. Local completion, Git dan legacy retirement

Hasil akhirnya adalah **working Full Rust local release candidate yang siap
diaudit**, bukan repo berisi proposal, scaffolding atau pure-engine prototype.
Sediakan satu documented command untuk bootstrap/run candidate pada isolated local
profile, beserta doctor, verify, backup/restore/recovery dan stop/cleanup commands.

Implementasikan dan buktikan local cutover/rollback pada isolated fixture profile.
Active owner DB/runtime tetap terjaga selama development; sediakan reviewed-plan
dan runnable target-checked command untuk activation terhadap owner profile bila
belum tercakup instruksi owner yang berlaku. Jangan menyatakan owner host sudah
dipindah jika yang berjalan masih TypeScript. Online deployment bukan scope tugas.

Target candidate tidak membutuhkan Node/Python runtime untuk first-party business
application/orchestration. Browser tests/build tooling dan documented vendor SDK
bridge dikecualikan sesuai PRD. Old TypeScript/Python dapat dipertahankan sebagai
historical comparison/rollback fixture yang terisolasi dan diberi disposition;
tidak boleh menjadi dependency tersembunyi candidate.

Retirement membersihkan aktif wiring/dependencies yang sudah mempunyai verified
replacement, bukan menghapus arsip/history/data. Jika deletion prerequisites belum
terpenuhi, tulis exact reason/disposition; jangan menghapus source yang masih
dibutuhkan oleh owner runtime atau historical proof.

Buat coherent local commits untuk changes milik pekerjaan ini dengan hooks aktif
setelah relevant checks pass. Pisahkan prerequisite corrections, Rust foundations,
domain slices, UI/SDK dan release/docs bila membantu review. Jangan `git add .`,
memasukkan existing changes yang kepemilikannya belum jelas, reset/amend history,
merge/push/publish tanpa instruksi. Catat baseline dirty changes yang tetap ada;
“clean working tree” bukan tujuan bila ada pekerjaan lain yang harus dipertahankan.

## 11. Checkpoints dan final audit packet

`docs/rust/EXECUTION_STATE.md` minimal menyimpan:

- frozen inventory/release/source identity dan relevant baseline dirty changes;
- completed/current/remaining milestone dan item IDs;
- exact files/commits/artifacts/proof commands/results;
- failed checks, known differences, findings status dan remaining external blockers;
- pending processes/session IDs serta cara mengambil exit status;
- next concrete action, agar continuation tidak butuh owner menjelaskan ulang.

Setiap checkpoint adalah save point lalu lanjut kerja. Tidak perlu menunggu
owner/Codex membalas. Routine recoverable failure dikerjakan sampai beres.

Sebelum final response, buat:

1. `docs/rust/FULL_RUST_COMPLETION_REPORT.md` — before/after architecture, full
   inventory disposition, domain/UI/provider/CLI coverage, actual exceptions,
   audit finding closures dan honest local/live/kit readiness.
2. `docs/rust/FULL_RUST_AUDIT_PACKET.md` — entry point untuk Codex: commit range
   dan dirty snapshot, artifact/lock/config/migration hashes, run instructions,
   exact verification commands/evidence, two-consumer setups, negative reproducers,
   benchmark raw results, upgrade/restore/rollback proof dan known risks.
3. Machine-readable gate/coverage report yang mengikat inventory items, required
   proof IDs dan candidate artifact identity. Gunakan `local_verification` dan
   `independent_review: "pending"` sebagai facts terpisah; jangan mengisi status
   “Codex accepted” sendiri atau memalsukan independent sign-off.
4. OWN-V1 completion evidence untuk OWN-T01–OWN-T10, two-business-model setups dan
   real owner browser/chat configuration flow. Matrix product delta dipisahkan
   dari Rust migration inventory; UI/CLI developer configuration tidak menggantikan
   proof owner self-service.

Kriteria implementation complete:

- Seluruh frozen migration inventory implemented/replaced dengan local proof;
  no placeholders, hidden old runtime, untested required path atau dropped feature.
- OWN-V1 implemented dan OWN-T01–OWN-T10 PASS; owner dapat menyelesaikan supported
  configuration journey tanpa coding, file editing atau terminal.
- Seluruh required local gates PASS pada candidate identity yang sama, termasuk
  performance/boundary decisions yang sesuai registered policy dan tercatat.
- Candidate bisa dijalankan ulang dari documented clean setup; two-consumer,
  security/integrity, API/UI, upgrade/recovery/restore dan release identity terbukti.
- Remaining compatibility/vendor bridges dipetakan eksplisit dan benar-benar
  memenuhi scope; live-provider status dan 40-kit completeness tidak dipalsukan.
- Active owner data aman, artifacts dan audit packet lengkap, tidak ada remote push
  atau undocumented billable/external effects.

Jika semua terpenuhi, laporkan **“Full Rust implementation complete — ready for
Codex audit”** dengan links/commands/counts aktual. Independent release acceptance
tetap pending. Jika belum terpenuhi, statusnya **incomplete/blocked** dengan exact
remaining item/gate; jangan menyebut semua selesai hanya karena quota/context
hampir habis atau mayoritas tests hijau.

**Resume sekarang dari checkpoint aktual dan correction queue pada
[prompt aktif 17 Oktober](ARCHXY_NEXT_EXECUTION_PROMPT.md); tuntaskan remaining
R00–R11 + OWN-V1 sampai seluruh target di atas terpenuhi.
Pertahankan pekerjaan valid dan lanjut tanpa meminta owner mengetik continue lagi.**
