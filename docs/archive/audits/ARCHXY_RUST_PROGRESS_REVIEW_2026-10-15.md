# Archxy Rust — progress review, 15 Oktober 2026

**Observed:** sekitar 18:40–18:45 WIB, pada working tree yang masih aktif berubah.  
**HEAD:** `242549cc292cd53bbc66eb9cf119d57443aa2a37`.  
**Branch:** `feat/laundry-1.17.0`.  
**Verdict:** Ada implementasi nyata dan reusable pure-engine work. Full Rust masih
di fondasi R01/R02; belum aplikasi bisnis end-to-end atau release accepted.

Ini interim review yang diminta owner saat Archxy masih bekerja. Codex tidak
menghentikan process, mengubah application source/checkpoints, menjalankan ulang
pack/release tools, memakai owner DB atau meminta implementator berhenti.
Temuan mengacu snapshot yang diperiksa, bukan final verdict atas pekerjaan yang
belum selesai.

## Progress yang terlihat

- Workspace Rust sudah mempunyai core, domain policy, PostgreSQL adapter seam,
  Axum router, pricing, reference rates, native bridge dan assembly planner.
- Saat review berlangsung, capacity bertambah dari source yang belum terdaftar
  menjadi workspace member dengan `src/lib.rs`. File masih diperbarui pada jam
  review. Ini bukti perubahan aktual; process agent yang hidup saja bukan bukti
  bahwa seluruh waktu sebelumnya digunakan efektif.
- Ada execution state, decisions, differences, aggregate inventory, parity harness
  dan dua narrow artifact consumers di `examples/rust-consumers`.
- Existing consumer proof mencatat Rust-crate consumer dan Node-addon consumer
  untuk pricing/rates. Ini bukan dua complete business consumers atau CRM proof.
- Belum ditemukan Leptos implementation, real login/session issuance, usable CRM
  contact read, recovery workflow atau runnable host CLI/main pada snapshot ini.
  `/login` memberi acknowledgement; `/contacts` selalu menolak. `archava-host`
  sekarang library/router, belum aplikasi pengganti owner host.

Checkpoint `EXECUTION_STATE.md` diperbarui sekitar 17:13, sehingga sebagian angka
dan daftar crates tertinggal dari source terbaru. Tabel awal menyebut R02 complete,
sedangkan detail §R02 menyebut started. Jangan memakai tabel awal sebagai bukti
milestone selesai. LOC ratio juga bukan persentase migration behavior.

## Verifikasi ringan yang dijalankan Codex

Untuk menghindari Cargo lock/build contention, Codex menyalin existing compiled
test executables ke direktori `/tmp` miliknya dan menjalankannya di sana. Tidak
ada fresh workspace build atau full regression suite dalam review ini.

| Existing compiled test artifact  | Passed  | Failed |
| -------------------------------- | ------- | ------ |
| Core                             | 28      | 0      |
| PostgreSQL adapter helper tests  | 11      | 0      |
| Domain policy                    | 13      | 0      |
| Pricing                          | 56      | 0      |
| Reference-rate integration tests | 11      | 0      |
| Native bridge unit tests         | 16      | 0      |
| Assembly integration tests       | 31      | 0      |
| **Total**                        | **166** | **0**  |

Ini smoke verification terhadap binaries existing, bukan klaim current source
seluruh workspace sudah dibangun ulang atau seluruh authorization benar.
Capacity yang sedang ditulis tidak diuji oleh angka ini.

Stored parity evidence saat dibaca:

- Pricing: 350 fixtures, 338 match, 12 error-class differences, `failed: 12`.
  Belum exact boundary parity bila compatibility contract memerlukan class itu.
- Rates: 753 fixtures, 748 match + 5 corrected expectations, `failed: 0`.
  Corrected cases terpisah dari exact legacy matches; tidak menjadi platform parity.

## RP01 — P1 sebelum integrasi bisnis: HTTP authority masih self-asserted

[CommandSubmission](../rust/crates/host/src/router.rs) menerima `tenant`, `role`,
`capability`, `confirmed`, `human_approved` dan `enabled_action_ids` dari JSON.
Handler membentuk `ActionRequest` dari field tersebut, mengevaluasi static policy,
lalu membuka tenant transaction. Belum ada authenticated session/principal,
membership resolution, current operation grants/resource scope atau bound approval
verification pada router ini.

Validasi lowercase tenant slug hanya memvalidasi bentuk input. User tanpa hak
tenant B tetap dapat menulis slug tenant B yang formatnya valid. PostgreSQL RLS
yang bergantung pada tenant setting tidak dapat menentukan authenticated tenant
asal apabila aplikasi memasang setting yang dipilih caller.

### Probe yang benar-benar dijalankan

Codex membuat small Rust program di `/tmp`, dikompilasi terhadap compiled host
library existing dan memanggil `router.oneshot`. Request tidak mempunyai cookie,
Authorization header, session ataupun CSRF proof. Pool sengaja menunjuk
`127.0.0.1:1` dengan acquire timeout 150 ms: tidak ada real database yang disentuh.

```json
{
  "tenant": "other-tenant",
  "role": "archava_assistant",
  "capability": "act",
  "action_id": "crm.followup.create",
  "confirmed": true,
  "human_approved": true
}
```

Hasil:

```text
unknown-action-control status=404 Not Found
  {"decision":"denied","reason":"unknown_action",...}

no-auth-forged-role-grant-and-approval status=503 Service Unavailable
  {"decision":"refused","reason":"tenant_context"}
```

Control ditolak oleh policy. Forged request melewati policy sampai database
connection attempt; ia gagal karena database sintetis sengaja unavailable.
Protected mutation seharusnya ditolak pada identity/authority boundary sebelum
mencoba operation transaction. Ini tidak membuktikan data production bocor atau
mutation terjadi: handler saat ini hanya membuka lalu rollback transaksi, belum
menjalankan business effect. Ini membuktikan G03/authenticated boundary belum siap.

Perbaikan: identity dari verified session/server context; resolve current tenant
membership, grants dan resource scope; capability/module availability dari trusted
config; approval dari bound server-verifiable challenge/receipt. Jangan menerima
claim authority/approval dari body sebagai evidence. Tambahkan valid-tenant-A/B,
forged valid role/capability, consent-boolean spoof dan revocation scenarios.

`declined: true` berarti consent refusal, bukan pencabutan membership/grants.
Enum role rejection dan malformed-tenant rejection juga bukan bukti resource scope
atau tenant ownership. Documentation pada host yang menyebut seluruh authority
seam proven perlu dibatasi sampai proof tersebut ada.

## RP02 — P2: beberapa adapter tests tidak menguji implementasi SQL

[rls.rs](../rust/crates/adapters/src/rls.rs) mempunyai tests yang memeriksa string
literal SQL yang dibuat ulang di test sendiri, misalnya:

```rust
assert!("SELECT set_config($1, $2, true)".ends_with(", true)"));
```

Test itu tetap hijau bila actual `begin()` berubah memakai transaction-local
flag yang salah. Test `relrowsecurity/relforcerowsecurity` dan bound namespace
juga memeriksa duplicated literals, bukan menjalankan adapter terhadap PostgreSQL.
Ini explains mengapa helper PASS tidak membuktikan pool isolation/RLS.

Selain itu, `assert_forced_policies` tersedia, tetapi `TenantTransaction::begin`
yang diperiksa hanya memanggil non-owner check, set tenant dan readback. Router
belum memakai `Ledger.tables` untuk menjalankan forced-policy startup gate.
Header host sendiri mengakui migration/startup assertion belum selesai.

Tambahkan meaningful tests pada isolated real PostgreSQL dengan non-owner role:
forced-policy gate, legal tenant A/B, missing/unforced RLS, pooled connection reuse,
commit/rollback/cancellation, service/human scopes dan actual membership revocation.
Jangan menyatakan G03 lulus dari test literals atau expected compile failure WASM.

## RP03 — P2: latest Leptos RC bukan alasan menahan seluruh UI

Host docs/checkpoint menunda SSR karena `cargo search leptos` menampilkan
`0.9.0-rc0`. Latest search result tidak membuktikan tidak ada stable release yang
bisa dipilih. Dokumentasi resmi [Leptos 0.8.17](https://docs.rs/leptos/0.8.17/leptos/)
tersedia. Evaluasi/pin stable release dan matching integration/build crates;
buktikan compatibility lewat small SSR/hydration/login/recovery slice.

Jangan mengganti framework atau menunggu upstream 0.9/1.0 hanya karena latest
merupakan RC. Failure yang relevan adalah concrete incompatibility pada required
flow, bukan label latest. PRD memang meminta feasibility lebih dulu.

Catatan SQLx terkait: menghilangkan query macros merupakan design choice yang
sah, tetapi alasan “compile-time query checking selalu memerlukan live owner DB”
tidak benar. SQLx mendukung checked offline metadata dari synthetic schema dan
`SQLX_OFFLINE=true`. [SQLx offline workflow](https://github.com/transact-rs/sqlx/blob/main/sqlx-cli/README.md).
Jika memilih runtime queries, catat tradeoff dan ganti proof schema/query
compatibility dengan integration tests yang memadai; jangan menyebut penghapusan
compile-time checking sebagai security win dengan sendirinya.

## RP04 — P2: R00 inventory masih aggregate, belum migration coverage ledger

`migration-inventory.json` memiliki counts, ledger filenames, browser filename list
dan provider/template summaries. Belum ada per-behavior item IDs, consumers,
contract/error semantics, security scope, transaction cohort, target replacement,
required proof IDs dan status sebagaimana diminta execution task.

Concrete inaccuracies:

- Menyebut 14 `apps/crm/src/*.ts` sebagai browser panels, padahal list mencakup
  `server.ts`, `settings.ts`, `local-postgres.ts`, `postgres-pool.ts`, `operator.ts`.
  Actual domain dashboards berada di `packages/*/src/dashboard.ts` dan related
  CRM panels. Scope UI tidak boleh diturunkan dari daftar server files.
- Provider path `apps/crm/src/gemini.ts` tidak ada pada snapshot; Gemini planner
  actual ada di `packages/crm-assistant/src/gemini-planner.ts`.
- Prose tools roots salah menyebut `library/security/scripts/e2e` sebagai empat
  package roots. Actual tools packages adalah `configurator`, `decision-eval`,
  `crm-reference`, `act-integration`; folder helper tanpa package.json berbeda.
- Perbandingan total 70 roots dengan 64 `packages/*` pada PRD mencampur denominator.
  Subcount `packages/*` sendiri masih 64; itu bukan bukti snapshot 64 sudah stale.

Lengkapi inventory actionable sebelum menghitung coverage/claim R00 complete.
Gunakan actual entrypoints dan mark unknowns; jangan menghabiskan pekerjaan pada
rasio LOC untuk menggantikan ledger behavior yang belum dibuat.

## Arah lanjut yang disarankan

1. Tetap lanjut independent pure-engine/capacity work yang sedang aktif; jangan
   mengulang semua fondasi atau membuang hasil yang valid.
2. Perbaiki RP01 dan buat real authority/RLS proof sebelum menghubungkan host ke
   business reads/writes. Last product audit A01–A09 tetap memerlukan closure;
   port static ACL belum menutup findings itu.
3. Selesaikan inventory per behavior dan pilih stable Leptos untuk runnable small
   login → contact list → reviewed command → recovery flow. Jangan menunggu latest
   RC berubah stable atau menganggap HTTP router saja sudah vertical slice.
4. Perbarui checkpoint setelah slice selesai, catat evidence sesuai exact source,
   lalu lanjut roadmap tanpa meminta owner mengetik continue per tahap.

## Evidence dan batas review

Artifacts Codex lokal:

- `/tmp/archava-rust-progress-20261015-ke8w3zm1/smoke-results.json` — binary hashes,
  commands scope dan actual test outputs.
- `/tmp/archava-rust-progress-20261015-ke8w3zm1/authority_probe.rs` dan
  `authority-probe-output.txt` — standalone HTTP-boundary probe.
- `/tmp/archava-rust-progress-20261015-ke8w3zm1/review-context.json` — HEAD/branch,
  observation timestamp dan relevant source/document hashes.

Tidak menjalankan ulang Cargo build/workspace tests, cold-consumer pack, full
pnpm/security suite atau database/integration tests. Tidak menguji capacity yang
masih aktif ditulis. Tidak mengubah checkpoint/task/source milik Archxy. Existing
test counts/proofs di laporan implementator tidak otomatis diverifikasi oleh
review ini. Full independent acceptance tetap menunggu completed audit packet.
