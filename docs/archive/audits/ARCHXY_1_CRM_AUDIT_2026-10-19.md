# Audit Archxy 1 — CRM Rust, 19 Oktober 2026

**Verdict: belum selesai seluruhnya; belum diterima sebagai CRM Rust lengkap.**
Ada working vertical slice dan kemajuan backend nyata. Akan tetapi, enam operasi
tulis baseline belum dimigrasikan, owner configuration belum dibuktikan sesuai
requirement, dan audit browser menemukan defect logout, pending recovery,
literal search serta pagination. Sisa ini bukan seluruhnya menunggu ERP Archxy 2.

Snapshot dimulai **19 Oktober 2026, 03:34 WIB**, HEAD
`409eab9a3e366c40b05dbc56e3c850cefa2c7c9b`, branch `feat/laundry-1.17.0`,
profile `1.18.0`. Audit mencakup actual dirty/untracked public source: **1.349
files** di baseline. Seluruh baseline file hashes dan HEAD tetap sama pada
integrity check akhir. Tidak ada perubahan kode aplikasi atau owner DB.

Evidence ringkas: [machine-readable audit](audit/2026-10-19-archxy1-crm-audit.json).
Raw logs, browser reproducer dan standalone fixture-server harness berada di:

```text
/home/haikaru/.cache/archava-codex-audits/crm-20261019-7m0dxhzw/
```

## Yang sudah terbukti bekerja

Fresh command, memakai owned synthetic PostgreSQL dan runtime non-owner:

```sh
# Dari rust/; AUDIT_TMP adalah task-owned short directory di /tmp.
TMPDIR="$AUDIT_TMP" cargo test --release --locked --offline \
  -p archava-host --test crm --test assistant -- --test-threads=1
```

- CRM: **27/27 PASS**. Includes actual follow-up persistence, assigned scope,
  current grants/revocation, CSRF, stale revisions, idempotent replay, conflicting
  requests/races, receipt recovery dan service/human scope intersection.
- Assistant HTTP/PostgreSQL: **13/13 PASS**. Includes real overview, contact
  summary/search, follow-up worklist, read-only preparation termasuk completion,
  opportunity reads dan current membership/grant checks.
- Independent headless browser: Rust SSR/WASM login, scoped contact list dan
  actual follow-up creation berjalan. Audit sengaja kehilangan response setelah
  commit dan memeriksa satu actual stored follow-up di PostgreSQL.

Passing tests membuktikan behavior yang mereka uji. Salah satu CRM test justru
memastikan `contact.create` ditolak karena belum diimplementasikan; suite hijau
tidak membuktikan seluruh CRM sudah dimigrasikan.

## Temuan

### C01 — P1: enam dari tujuh operasi tulis CRM baseline belum tersedia

**Evidence: source review + executed refusal test.**
[Rust command enum](../rust/crates/host/src/crm.rs) hanya berisi
`followup.create`. Baseline [contacts contracts](../packages/crm/src/contracts.ts)
dan [pipeline contracts](../packages/crm/src/pipeline-contracts.ts) memiliki tujuh
operasi berikut:

| Operasi              | Rust host saat audit                                                 |
| -------------------- | -------------------------------------------------------------------- |
| `contact.create`     | Belum diimplementasikan; executed existing test membuktikan HTTP 422 |
| `contact.update`     | Tidak ada command variant/handler yang menjalankan operasi ini       |
| `activity.add`       | Tidak ada command variant/handler yang menjalankan operasi ini       |
| `followup.create`    | Implemented; fresh DB tests dan browser effect terverifikasi         |
| `followup.complete`  | Assistant bisa menyiapkan draft, tetapi execution belum tersedia     |
| `opportunity.create` | Tidak ada command variant/handler yang menjalankan operasi ini       |
| `opportunity.move`   | Tidak ada command variant/handler yang menjalankan operasi ini       |

Assistant sudah dapat membaca pipeline; jangan menyebut pipeline seluruhnya
tidak tersentuh. Yang belum lengkap adalah mutation lifecycle dan product UI.
[Leptos route table](../rust/crates/slice/src/lib.rs) hanya memuat login,
contacts, draft dan recovery. Belum ada equivalent contact editing/detail,
activity/worklist/completion, sales pipeline atau visible assistant panel.

Requirement: Full Rust PRD §2.2/§2.4, BUS-01, UI-02, frozen existing inventory dan
mandat Archxy 1. Narrow R03 read checks tidak menggantikan existing behavior.
Tuntaskan supported operations dan UI, termasuk current authority, typed
approval/native-action integration, durable effects dan meaningful regressions.
Angka **1/7 adalah command coverage**, bukan persentase progres proyek.

### C02 — P1: sign-out tidak mengakhiri sesi server

**Evidence: independently reproduced melalui actual Rust UI + cookie API.**
[Sign-out handler](../rust/crates/slice/src/session.rs) hanya memanggil
`forget_session()` dan `clear_session_facts()`. Router belum mempunyai logout
endpoint yang mencabut sesi/menutup cookie. Komentar source sendiri menyebut
server cookie revocation sebagai pekerjaan berikutnya.

Reproducer memakai fixture account dan browser profile baru:

1. Login melalui UI; contact list membaca PostgreSQL.
2. Klik `sign out`; UI marker menjadi `signed out`, stored CSRF menjadi null.
3. Dari browser yang sama, `fetch('/contacts?tenant=acme-retail')`.
4. **Actual: HTTP 200, 20 authorized contact rows masih terbaca.**

Ini belum membuktikan akses actor lain atau cross-tenant bypass. Defect-nya adalah
sesi yang seharusnya ditutup tetap memberikan akses. Clearing UI state tidak
mencabut authority cookie. Ini melanggar tenancy/session requirement PRD §8.1.
Tambahkan authenticated logout dengan CSRF/origin policy yang sesuai, revoke
server session dan expire cookie; setelah logout, old cookie/bearer tidak boleh
membaca/menulis/recover. Tambahkan UI, HTTP dan restart regression yang nyata.

### C03 — P1: klaim OWN-V1 delivered tidak ditopang acceptance behavior

**Evidence: producer logic, route inventory dan requirement comparison.**
[Owner PRD](OWNER_CONFIGURATION_PRD.md) meminta inspect → plan → preview →
confirm → actual configuration apply → recovery/history/restore. Akan tetapi,
[OWN producer](../tools/own/verify.mjs) mengisi PASS memakai bukti yang berbeda:

- T01: `owner config changes via UI only` diturunkan dari `outcome.ok` pada
  policy consumer runner; tidak menjalankan owner configuration UI.
- T06: racing config apply/recovery dinilai dari writer-fence/rollback/backup
  rehearsals dan release identity recomputation; bukan config apply race.
- T07: `disabled direct API rejected` dinilai dari deployment-plan unit tests,
  tanpa men-disable runtime module lalu memanggil actual disabled endpoint.
- T10: full owner configuration browser journey dinilai dari keberhasilan
  **TypeScript CRM** browser battery dan keberadaan Chromium/CDP source markers.
  Itu tidak membuktikan owner module configuration/apply/restore di Rust UI.

ADR-R13 mengubah interpretasi OWN-V1 menjadi acceptance family atas existing
surfaces, tetapi requirement owner dan active mandate tetap meminta actual
product behavior. ADR implementator tidak menutup delta tersebut. Ini masalah
semantic coverage: check names, hashes dan actual exit 0 dapat benar sambil
behavior yang diberi label PASS belum diuji.

Pertahankan existing tests sebagai component evidence; bangun missing product
flow dan tautkan setiap OWN proof ke actual operation yang dimaksud. Sampai itu
terbukti, status OWN-V1 delivered harus diturunkan menjadi incomplete.

### C04 — P2: dua consumer membuktikan policy reuse, belum reusable CRM behavior

**Evidence: package list dan executed witness definitions, bukan fresh consumer rerun.**
[K05/K04 producer](../tools/rust/k05-k04-consumers.mjs) hanya membundel
`archava-core` dan `archava-domain`.
[Consumer witnesses](../examples/rust-kit-consumers/kit-template/tests/kit_witness.rs)
menguji tenant IDs, role table, action registry dan `CommandAuthorizer`.
Actual CRM repository/operations ada pada `archava-host::crm` dan assistant
adapter; keduanya tidak dijalankan oleh consumer proof tersebut.

Identical artifacts dan offline installation adalah kemajuan valid. Akan
tetapi, melabelinya reusable CRM complete terlalu luas. Buktikan dua independent
consumers menjalankan contacts/pipeline/follow-up behavior dari identical
released CRM artifacts dengan actual isolated persisted data dan minimal closure.
Business differences tetap configuration, tanpa copied source atau client forks.

### C05 — P2: reload setelah response loss kehilangan pending request identity

**Evidence: independently reproduced melalui actual Rust UI + PostgreSQL.**

1. Submit follow-up melalui UI. Audit transport menerima server response lalu
   melempar error sebelum aplikasi membacanya; actual DB effect count = **1**.
2. UI menampilkan unknown response dengan original request ID dan recovery link.
3. Reload halaman draft yang sama sebelum mengikuti link recovery.
4. Request ID berubah dari `ec757ce8-6da6-4925-a6a1-293fa0484b0c` menjadi
   `36183da8-2953-4c8d-a29d-a9c6d6083ab1`. Original ID tidak ada di tab/local
   storage atau URL halaman draft; outcome panel kembali kosong.

[Draft state](../rust/crates/slice/src/draft.rs) menyimpan UUID hanya pada
reactive memory dan membuat UUID baru setiap mount. Backend recovery tetap
berfungsi bila original key masih diketahui; masalahnya produk kehilangan
penanda untuk menggunakannya. Existing browser proof menyimpan key di variabel
driver dan membawanya kembali setelah reload; itu tidak membuktikan pending
marker milik aplikasi bertahan.

Preserve account/tenant-bound opaque pending marker sebelum execution, recover
status setelah reload dan pertahankan unresolved-operation gate. Jangan memulai
intent baru hanya karena transport error/reload. Audit ini tidak mengklaim sudah
membuktikan duplicate business writes; yang terbukti adalah hilangnya identity.

### C06 — P2: literal search dan pagination belum usable sesuai baseline

**Evidence: independently reproduced melalui actual Rust browser dan API.**

- [Search URL construction](../rust/crates/slice/src/contacts.rs) menggabungkan
  `search_term` mentah. Input literal `Ada&limit=1` menjadi query parameter lain.
  Encoded API search mengembalikan **0** rows, tetapi UI menampilkan **Ada
  Lovelace**. Gunakan URL query encoding; ini query-construction defect, bukan
  bukti SQL injection atau auth bypass.
- Dengan 29 synthetic contacts, UI menampilkan **20** dan pesan `a further page
exists after ...`, tetapi tidak menyediakan next-page action. Backend cursor
  tersedia; view hanya menampilkannya sebagai teks. Tambahkan actual cursor
  navigation, search-reset semantics dan stable page-state handling.

### C07 — P2: live coverage bukan lagi hanya lima ERP blockers

**Evidence: fresh read-only `node tools/rust/coverage.mjs`, exit 1.**

Snapshot menghasilkan `complete=false`, **32 diagnostics**:

- 26 proof identity mismatches: proof HEAD `94307ed…`, current HEAD `409eab9…`.
- 1 source digest mismatch pada legacy-replacement proof.
- 5 missing domain proofs R04–R08, yang memang masih berada di ERP lane.

Commit terbaru menambahkan terminal report setelah proving run, sehingga sebagian
ini adalah evidence bookkeeping drift, bukan bukti 26 regression aplikasi.
Namun final current-candidate PASS tidak bisa diklaim berdasarkan report lama.
Regenerasi affected evidence sesuai identity policy setelah corrections; jangan
mengganti hash/head pada proof lama tanpa producing run atau approved reuse rule.
Seluruh semantic gaps C01–C06 tetap perlu dibenahi meskipun validator nanti hijau.

## Scope matrix untuk keputusan owner

| Bagian                                                                     | Hasil audit                                                      |
| -------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Rust login, scoped contact reads, follow-up create, receipt/replay backend | Bekerja pada bounded slice; fresh tests PASS                     |
| Assistant overview/search/worklist/pipeline reads dan preparation          | Real PostgreSQL/HTTP behavior terbukti; bukan semua execution/UI |
| Full existing CRM mutations dan reusable dashboard parity                  | Belum selesai                                                    |
| Logout/revocation melalui UI                                               | Gagal menutup sesi; browser counterexample terbukti              |
| Pending write recovery setelah draft reload                                | Backend ada; browser pending marker hilang                       |
| Two-model reusable CRM proof                                               | Baru authority/policy reuse; CRM business reuse belum terbukti   |
| Owner puzzle configuration OWN-V1                                          | Belum memenuhi actual acceptance; green producer terlalu luas    |
| ERP R04–R08                                                                | Lane Archxy 2; tidak dijadikan alasan menutup incomplete CRM     |
| Full Rust release acceptance                                               | Tidak diberikan pada audit ini                                   |

## Urutan perbaikan Archxy 1

1. Tutup server logout/revocation, pending recovery dan literal URL encoding.
2. Lengkapi enam missing CRM writes, actual CRM UI dan assistant-to-authorized
   execution flow; preserve source baseline behavior dan corrected authority.
3. Buktikan CRM business reuse dari dua packaged independent consumers.
4. Implement actual OWN-V1 configuration lifecycle; perbaiki semantic proof
   mappings. Jangan mengganti required behavior dengan unrelated passing suites.
5. Jalankan affected tests/browser/consumer proofs, kemudian common-candidate
   gates dan refresh current evidence. Koordinasikan ERP integration dengan
   Archxy 2; Codex acceptance tetap external.

## Metode, batas dan lifecycle

Tidak menjalankan ulang seluruh 45–75 menit release/security chain, live Gemini,
external payments/email, owner migration atau ERP domain suite. Audit ini tidak
menyatakan tidak ada defect lain. Scope khusus Archxy 1/CRM + related authority,
reuse, owner configuration dan evidence integrity.

Independent browser server memakai current example server dengan **satu** source
adaptation: PID file dipindahkan ke task-owned cache agar tidak menimpa instance
Archxy. Host/UI libraries dan shipped JS/WASM assets tidak diedit. Browser profile,
listen port dan synthetic DB terpisah; database privileges/RLS memakai fixture
baseline yang sama. Raw harness/adaptation disimpan untuk reproduksi.

Initial audit run dengan TMPDIR panjang gagal karena Unix socket path limit;
itu setup failure milik audit, bukan 27 product failures. Diperbaiki dengan owned
short `/tmp` directory, lalu kedua fresh suites PASS. Dua initial browser harness
attempts mengalami hydration polling race dan stale DevTools port file; log tetap
disimpan. Final probe memakai hydrated UUID checks dan fresh profile, exit **0**.

Semua owned browser/server instances dihentikan; ketiga browser-run PostgreSQL
directories terhapus dan ports tertutup, termasuk failed attempts. Integrity
record tidak menemukan perubahan pada 1.349 baseline public files. Owner settings,
keys, `.local`, DB, historical archives dan unrelated running processes tidak
diubah. Tidak ada commit atau push dari audit ini.
