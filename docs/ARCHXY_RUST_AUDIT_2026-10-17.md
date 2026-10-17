# Audit progres Rust Archxy — 17 Oktober 2026

**Verdict: INCOMPLETE. Ada kemajuan pada tooling, formatting dan persiapan UI,
tetapi critical authority/recovery findings belum tertutup. Current test fixture
juga belum compile.**

Snapshot **11:09:14 WIB**, HEAD `242549cc292cd53bbc66eb9cf119d57443aa2a37`, branch
`feat/laundry-1.17.0`. Public dirty/untracked source ikut diperiksa. Audit
melanjutkan [reaudit 23:26](ARCHXY_RUST_REAUDIT_2026-10-16_LATE.md).
[Evidence JSON](audit/2026-10-17-rust-audit.json) mengikat source, logs dan probes.

## Kemajuan sejak audit sebelumnya

- `cargo fmt --all --check` sekarang **PASS**. Scoped Clippy pada host library
  normal juga **PASS**; all-target checks masih terblokir fixture compile error.
- `tools/rust/pg-authority.mjs` sudah terhubung ke fast gate, manifest dan named
  witness mapping. Runner menolak suite yang tidak bisa berjalan; ini perbaikan
  terhadap proof yang hanya mengandalkan jumlah tests.
- Selected gate/coverage Node suites **41/41 PASS** pada snapshot fresh.
- Stable Leptos dependencies dan crate `archava-slice` sudah ditambahkan. Namun
  `slice/src/lib.rs` berisi **empat baris komentar, 225 bytes**. Belum ada view,
  SSR route, hydration entry, executable produk atau browser flow.

Ini persiapan dan perbaikan fondasi. Belum ada bukti owner bisa menjalankan satu
workflow CRM/business lengkap pada Rust UI. Tidak ada completion report atau
final audit packet.

## Temuan prioritas

### RN08 — P1 verification blocker baru: Fixture tidak dapat dikompilasi

[Struct Fixture](../rust/crates/pg-fixture/src/lib.rs#L172) sudah mempunyai
`crm_ledger`, tetapi [initializer](../rust/crates/pg-fixture/src/lib.rs#L731) tidak
mengisi field itu. Actual compile result:

```text
error[E0063]: missing field `crm_ledger` in initializer of `Fixture`
```

`cargo clippy -p archava-host --all-targets` exit 101. Named authority producer
juga exit 1 karena child Cargo exit 101: **nol tests ditemukan/dijalankan**, semua
delapan required checks FAIL. Ini bukan 0/0 PASS dan bukan pengulangan hasil
12 failing logins sebelumnya.

Selesaikan provisioning CRM/migration ledger dan initializer secara utuh; empty
ledger hanya untuk membuat compile tidak cukup bila host/fixture menyatakan CRM
ready. Jalankan kembali meaningful default fixture/authority/readiness suites.
P1 di sini berarti blocker verifikasi, bukan klaim remote security exploit.

### RN01 — P1 sebelum business integration: status receipt lolos setelah downgrade

[status_receipt](../rust/crates/host/src/router.rs#L987) masih hanya memeriksa
active membership dan matching receipt actor. Probe baru pada actual router dan
owned non-owner PostgreSQL mengonfirmasi:

| Setelah role service diturunkan menjadi viewer, membership tetap active | Hasil                                |
| ----------------------------------------------------------------------- | ------------------------------------ |
| GET status receipt                                                      | **200**, original input dikembalikan |
| POST replay command yang sama                                           | **403 `role_not_allowed`**           |

Terapkan current read/operation/resource authority sebelum status/recovery
mengembalikan payload. `active=false` tests saja belum mencakup penurunan grants.

### RN02 — P2: idempotency race belum memeriksa fingerprint pada conflict recovery

[Conflict branch](../rust/crates/host/src/router.rs#L943) memeriksa actor tetapi
tidak fingerprint. Dua actual router requests, same actor/key, input `race-A` dan
`race-B`, ditahan pada DB lock sampai **dua INSERT waiters** teramati. Setelah
release, keduanya **200**; request A menerima receipt dengan input **race-B**.

Database hanya menyimpan satu receipt untuk race key. Tidak ada duplicate business
effect yang didemonstrasikan. Yang gagal adalah binding input/command pada hasil
retry. Periksa actor/fingerprint/action/revision pada semua sequential maupun
concurrent recovery paths; different-input collision wajib conflict.

### RN03 — P2: migration/release mismatch tetap mematahkan reuse

Migration `004_bound_approvals.sql` ada, tetapi TypeScript migrator dan current
release profile tetap mendeklarasikan tiga migrations. Fresh `pnpm reuse:verify`
kembali gagal **`migration_set_mismatch`**. Runtime Rust test fixture memakai
ledger yang berbeda dari supported baseline migrator/profile.

Selesaikan ownership, migration/kernel/table list, versions/checksums, new
candidate identity dan prior-upgrade compatibility. Jangan overwrite archive
1.17.0 atau langsung memigrasikan owner DB untuk memperbaiki verifikasi.

### RN07 — P2 test/runtime boundary baru: override KDF ikut diekspor normal build

[set_max_active_derivations](../rust/crates/host/src/identity.rs#L69) bukan lagi
`cfg(test)`: ia public dan diekspor host normal. Integration fixture memanggilnya
dengan limit 32, sementara production default 2.

Executable audit yang mengimpor host sebagai **normal non-test dependency** dapat
memanggil setter dengan 0, lalu `encode_password` mendapat **`Err(Busy)`**. State
direset ke 2 dalam proses audit yang sama. Tidak ada endpoint remote atau
production process yang dimodifikasi.

Ini bukti test seam ikut shipped API dan tests memakai budget berbeda; bukan
bukti attacker HTTP dapat memanggil setter. Pisahkan fixture/test scheduling dari
runtime resource policy. Buktikan production-default budget/overload semantics
secara terpisah dan jangan mengklaim default-path concurrency sudah diverifikasi
oleh suite yang menaikkan limit global.

### RN05 — P2 library boundary: output tanggal masih melampaui supported domain

Fresh probe `next_business_day(Instant(MAX_SUPPORTED_MILLIS))` tetap menghasilkan
**`Ok(Instant(8640000172800000))`**, di atas max `8640000000000000`;
`in_supported_domain=false`. Validasi hasil weekend stepping dan semua callers.
Old `i64::MAX` infinite loop sudah tetap memberi controlled error; jangan
menyamakan perbaikan sempit itu dengan seluruh calendar boundary tertutup.

## Batas authority yang belum selesai

`resolve_membership` masih tidak membaca granular `permissions` atau melakukan
service/human on-behalf-of intersection. Service dengan empty permissions tanpa
human delegation mendapat allowed receipt untuk `product.read`. Karena handler
hanya mencatat opaque input/receipt, ini tidak membuktikan actual product query
atau data produk bocor. Baseline authority tetap wajib ketika real handlers masuk.

Approver membership yang dicabut setelah stamping juga belum membatalkan approval
pada probe. Tetapkan current-approver/delegation revocation policy secara eksplisit;
audit tidak memberi severity terpisah tanpa policy yang lengkap.

RA01 tidak dapat disebut closed secara menyeluruh hanya dari no-session refusal,
membership deactivation dan delapan named witness labels. RN01/RN02 serta granular
authority masih membutuhkan behavior dan regression proof.

## Hasil verifikasi dan batasnya

| Pemeriksaan                                                    | Hasil baru                                            |
| -------------------------------------------------------------- | ----------------------------------------------------- |
| Frozen/offline pnpm install                                    | PASS                                                  |
| Rustfmt seluruh workspace                                      | PASS                                                  |
| Host Clippy `--lib`, warnings-as-errors                        | PASS                                                  |
| Host Clippy `--all-targets`                                    | FAIL, E0063 pada fixture                              |
| Selected Node gate/coverage suites                             | **41/41 PASS**                                        |
| Packaged library reuse                                         | FAIL, migration_set_mismatch                          |
| Named authority producer                                       | FAIL, child compile exit 101, nol witnesses berjalan  |
| Standalone host executable dengan independent owned PG harness | Compile PASS, fresh adverse probes di atas selesai    |
| Required coverage                                              | `complete=false`, 45 diagnostics dalam audit snapshot |

Tidak menjalankan ulang full root, full Rust/release/security gate, browser,
benchmarks atau upgrade/restore setelah deterministic fixture/reuse blockers
ditemukan. Tidak ada fresh klaim root 9/9 atau Rust gate 11/11. Angka 45 bukan
persentase pekerjaan tersisa atau jumlah vulnerabilities.

RN04 default parallel auth verification tetap **unverified setelah perubahan**:
setter baru sudah ditambahkan tetapi current suite tidak melewati compile.
RN06 Rustfmt dan normal host-library Clippy membaik; all-target verification tetap
blocked oleh RN08. Jangan membawa semua status FAIL lama sebagai current result,
dan jangan menyebut semua finding CLOSED berdasarkan checkpoint saja.

## Handoff yang diperbaiki

Prompt aktif sebelumnya masih mengarahkan Archxy ke audit 19:40 dan §1.4. Temuan
RN audit 23:26 belum dipromosikan menjadi instruksi utama. Prompt baru
[ARCHXY_NEXT_EXECUTION_PROMPT.md](../ARCHXY_NEXT_EXECUTION_PROMPT.md) menjadi
starting point tunggal untuk correction queue dan seluruh R00–R11 + OWN-V1.
Mandat completion tidak berubah; tidak ada penambahan OWN-V2 atau 40 full kits.

Prioritas: pulihkan fixture/release verification, tutup RN01/RN02 dan test/runtime
boundary, lengkapi granular authority, tutup RN05, lalu wujudkan real vertical
slice sebelum melanjutkan seluruh domain/SDK/operasional/owner configuration.

## Metode dan lifecycle

1.232 public source/config/doc files dibekukan pada audit-only local clone; tidak
menyalin `.env`, `.local`, owner DB, previous dist atau product target. Node 22.22.3,
pnpm 10.15.1, Rust 1.96.0 dan task-owned Cargo cache digunakan. Product source,
owner data, commits/history dan service milik implementator tidak diubah.

Repository fixture dibiarkan gagal apa adanya. Untuk menguji normal host tanpa
memalsukan hasil fixture, audit memakai **harness PostgreSQL independen**: instance
dan owner/runtime roles milik audit, exact current commerce/identity SQL beserta
checksums, runtime non-owner, forced tenant RLS, lalu public `AppState::try_new`.
Ini runtime probe host, bukan PASS untuk repository fixture/tests.

HTTP requests memakai Axum `oneshot` dan valid synthetic HMAC sessions/CSRF;
password login tidak diuji ulang oleh probe ini. Race memakai lock barrier dan
observed waiters. Tidak ada browser/REA/container/provider call. Owned PostgreSQL
stop exit 0 dan status exit 3; owned data directory kemudian dibersihkan. Logs,
reproducer dan hash tetap tersimpan di audit scratch yang direferensikan JSON.
