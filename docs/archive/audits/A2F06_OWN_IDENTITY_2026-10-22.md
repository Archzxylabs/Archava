# A2/F04 + A3/F06 — plan identity mengikat effects & katalog, restore menghapus label

- **Card**: `t_708ef38a` (A2 + A3/F06, sisi Rust)
- **Sumber temuan**: `docs/audit/ARCHLAB_AUDIT_2026-10-21.md:152-190`, `docs/audit/archlab-2026-10-21-owner-claims.md`
- **Tanggal**: 2026-10-22 (Asia/Jakarta)
- **Branch / HEAD**: `feat/laundry-1.17.0` @ `412f763e17f3a68a457fd76c7b929184c50c916a`
- **Dirty tree bawaan**: dipertahankan. Tidak ada `git add`, `git commit`, `git stash`, atau revert.
- **Scope yang disentuh (hanya 4 file)**: `rust/crates/own/src/lib.rs`, `rust/crates/own/tests/engine.rs`, `rust/crates/host/src/own.rs`, `rust/crates/host/tests/own.rs`
- **Tidak disentuh**: `rust/crates/adapters/**`, `rust/crates/slice/**`, `packages/**` (read-only grep saja), `rust/Cargo.lock` (bersih)

## Verdict

Ketiga cacat ditutup dan dibuktikan dengan test merah→hijau. Plan identity sekarang adalah fungsi dari **jawaban** yang dilihat owner — termasuk klasifikasi readiness/cost tiap modul dan identitas katalog/harga — bukan hanya daftar modul dan label; konfirmasi basi ditolak eksplisit (409 `catalogue_changed` / `pricing_changed`) tanpa menulis apa pun; `restore` mengembalikan set label **persis** ke snapshot target lewat varian `Change::RemoveStage` yang ikut masuk identity.

| Gate (dari `rust/`, `TMPDIR=/tmp`)                                                            | Exit | Output mentah                   |
| --------------------------------------------------------------------------------------------- | ---- | ------------------------------- |
| `cargo test --offline --locked -p archava-own`                                                | 0    | `17 passed; 0 failed`           |
| `cargo test --offline --locked -p archava-host --test own`                                    | 0    | `13 passed; 0 failed` — 166.44s |
| `cargo fmt --all --check`                                                                     | 0    | 0 baris                         |
| `cargo clippy --offline --locked -p archava-own -p archava-host --all-targets -- -D warnings` | 0    | `Finished` — 0 warning          |

Trio ERP (`erp_billing`/`erp_attendance`/`erp_subscriptions`) **tidak dijalankan** sesuai instruksi card (di luar scope, ~52 menit, rebutan cargo lock).

Laporan ini sendiri ikut gate formatter repo: `pnpm format:check` → exit 0, `All matched files use Prettier code style!` (`evidence/a2f06/gate5-repo-format.log`).

## Snapshot pembuktian (hash-pinned)

```
HEAD=412f763e17f3a68a457fd76c7b929184c50c916a
branch=feat/laundry-1.17.0
b4f91305da0cfdc3e86489e1d834fb7faabf687cbc51f4b65e00e400c78bf71b  crates/own/src/lib.rs
834f4527edfbec3941d396056b2ed5c333a89a9a1a2cfe6d2e2a733c553fbf1c  crates/own/tests/engine.rs
08616162df02f9888f635c8042f83e6aca4eecc652dd70eeaeba5f905b12a532  crates/host/src/own.rs
a57f6486d5bc259b0f0c402662a267dee0263b11129407d88400a853142aac1c  crates/host/tests/own.rs
```

Evidence mentah: `docs/audit/evidence/a2f06/` — `red-engine.log`, `red-host.log`, `gate1-engine.log`, `gate2-host-own.log`, `gate3-fmt.log`, `gate4-clippy.log`, `gate5-repo-format.log`, `frozen-tree.txt`, `scope-proof.txt`. Log iterasi sebelum tree di-freeze ulang (pasca-`cargo fmt` + dua test engine terakhir) dipindah ke `evidence/a2f06/superseded/` dengan `NOTE.md`; sertifikat card ini hanya `gate*` yang terikat ke `frozen-tree.txt`.

## Cacat 1 — A2/F04: `canonical()` tidak memuat `effects`

### Klaim audit

Dua katalog yang berbeda **hanya** pada readiness/cost menghasilkan `planHash` identik; konfirmasi lama tetap diterima. `canonical()` hanya merender `base`, `revision`, himpunan modul, label, dan `Debug` dari `changes`/`blocked`.

### Bukti MERAH (sebelum patch)

`red-engine.log` (15 test, 2 merah):

```
---- a2_a_catalogue_change_is_a_plan_identity_change stdout ----
thread 'a2_a_catalogue_change_is_a_plan_identity_change' (1344008) panicked at crates/own/tests/engine.rs:454:5:
assertion `left != right` failed: the readiness/cost of the enabled set must be part of the plan identity
  left: "2677dfd8ac469c8a"
 right: "2677dfd8ac469c8a"

---- a3_restore_removes_the_labels_the_target_never_had stdout ----
thread 'a3_restore_removes_the_labels_the_target_never_had' (1344009) panicked at crates/own/tests/engine.rs:506:5:
assertion `left == right` failed: restore must land the target snapshot's labels, not a merge
  left: {"new": "New", "qualified": "Bid", "won": "Won"}
 right: {"new": "New", "won": "Won"}

test result: FAILED. 13 passed; 2 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
```

Hash `2677dfd8ac469c8a` berasal dari fixture crate `archava-own`; probe audit melaporkan `1dab7835fe45184a` pada fixture-nya sendiri. Fenomena yang dibuktikan sama — dua katalog berbeda menghasilkan satu identity — angka hash-nya berbeda karena fixture berbeda.

### Perbaikan

`canonical()` sekarang memuat, dalam urutan deterministik:

```
catalogue={catalogue_version}
pricing={pricing_version}
base={base_revision}
revision={revision}
module={id}={readiness}/{cost}      # per modul, dari BTreeSet enabled
label={stage}={label}               # dari BTreeMap
change={kind}:{subject}[={detail}]  # di-sort, TANPA Debug
effect={module}={readiness}/{cost}  # di-sort per module
blocked={reason}:{module}:{detail}
```

Determinisme eksplisit: `enabled` adalah `BTreeSet`, `pipeline_labels` adalah `BTreeMap`, `changes`/`effects` di-sort sebelum dirender, dan `canonical()` tidak pernah menyentuh `HashMap`. `Debug` formatting diganti fungsi `change_line()`/token eksplisit supaya identity tidak bergantung pada representasi Rust.

Konsekuensi tambahan yang penting: `Store::apply` **tidak lagi mempercayai** `plan.plan_hash` yang dibawa objek `Plan`. Identity dihitung ulang dari field plan itu sendiri di bawah katalog yang hidup:

```rust
let identity = hash(&canonical(
    &self.catalogue,
    plan.base_revision,
    &plan.config,
    plan.changes.as_slice(),
    plan.effects.as_slice(),
    plan.blocked.as_ref(),
));
if identity != plan.plan_hash {
    return Err(ApplyError::PlanMismatch);
}
```

Jadi plan yang di-preview di bawah katalog lain tidak bisa di-apply walaupun string hash-nya "cocok" dengan dirinya sendiri.

### Bukti HIJAU

- `a2_a_catalogue_change_is_a_plan_identity_change` — dua katalog beda readiness/cost → `planHash` **berbeda**, dan konfirmasi ber-hash lama ditolak (`ApplyError::PlanMismatch`).
- `a2_plan_identity_does_not_depend_on_request_order` — `["restaurant","laundry"]` vs `["laundry","restaurant"]` menghasilkan `plan_hash`, `changes`, `effects`, dan `Plan` utuh yang identik; instance `Store` baru mereproduksi `Plan` byte-for-byte.
- `a2_a_moved_catalogue_identity_refuses_the_stale_plan` — plan dari `test-catalogue@1` di-apply ke store `test-catalogue@2` → `Err(CatalogueChanged { held, live })`.

## Cacat 2 — A2/F04: konfirmasi tidak mengikat versi katalog/harga

### Klaim audit

`Confirmation { plan_hash, base_revision }` dan host compare hanya membandingkan dua field. `CATALOGUE_VERSION`/`PRICING_VERSION` ditulis ke receipt host tapi tidak pernah dibandingkan dengan yang dipegang klien, sehingga layar basi tetap dieksekusi.

### Bukti MERAH (sebelum patch)

`red-host.log`, test `a_confirmation_from_a_previous_catalogue_or_pricing_refuses`:

```
thread 'a_confirmation_from_a_previous_catalogue_or_pricing_refuses' (1345298) panicked at crates/host/tests/own.rs:959:5:
assertion `left == right` failed: the plan the owner sees must name its catalogue: {"baseConfig":{"enabled":["crm-contacts","crm-pipeline"],"revision":0,"terminology":{"pipelineLabels":{}}},"catalogueVersion":"owner-catalogue@1","plan":{"baseRevision":0,"blocked":null,"changes":[],"config":{"enabled":["crm-contacts","crm-pipeline"],"revision":1,"terminology":{"pipelineLabels":{}}},"effects":[],"planHash":"3e7fd7f3999f04cd"},"pricingVersion":"included@1"}
  left: Null
 right: "owner-catalogue@1"
test result: FAILED. 0 passed; 2 failed; 0 ignored; 0 measured; 11 filtered out; finished in 15.77s
```

`plan` tidak menyebut katalognya sama sekali (`Null`) — jadi tidak ada yang bisa diikat, apalagi dibandingkan.

### Perbaikan

1. `Catalogue` membawa identitasnya sendiri: `catalogue_version`, `pricing_version`. `Plan` memuat keduanya, jadi setiap plan yang dilihat owner menyebut katalog/harga yang dipakai menghitungnya.
2. `Confirmation` menerima echo opsional `catalogue_version` / `pricing_version`. Opsional bukan berarti lemah: hash sudah mencakup katalog, jadi klien yang hanya memegang hash tetap terikat; echo membuat penolakan punya **alasan presisi** alih-alih "hash mismatch".
3. Engine: `Store::apply` menolak (`ApplyError::CatalogueChanged` / `PricingChanged`) ketika plan atau echo konfirmasi menunjuk identitas lain.
4. Host: `check_confirmation` mengembalikan `409 CONFLICT` dengan `reason` = `catalogue_changed` / `pricing_changed` dan `detail` = `{held, live}` — **sebelum** ada tulisan. Receipt tetap menuliskan versi yang dipakai.

### Bukti HIJAU

`a_confirmation_from_a_previous_catalogue_or_pricing_refuses` (host, `gate2-host-own.log`):

- `confirmation.catalogueVersion = "owner-catalogue@0"` → `409`, `reason = catalogue_changed`, `detail.held/live` benar, `config_revisions = 0`.
- `confirmation.pricingVersion = "included@0"` → `409`, `reason = pricing_changed`, `config_revisions = 0`.
- Konfirmasi yang sama dengan identitas hidup (`owner-catalogue@1` / `included@1`) → `200`, `receipt.revision = 1`, dan receipt mencatat kedua versi.

Plus di engine: `a2_a_moved_catalogue_identity_refuses_the_stale_plan` menutup tiga jalur (plan vs store, echo katalog, echo harga) sekaligus membuktikan echo yang jujur tetap lolos.

## Cacat 3 — A3/F06: `restore` menambah label, tidak pernah menghapus

### Klaim audit

Planner restore mulai dari `current.terminology.clone()` lalu hanya menambah; `Change` tidak punya varian hapus. Restore ke revision 1 (label `new` saja) menghasilkan `changes=[]` dan `won` tetap nyangkut.

### Bukti MERAH (sebelum patch)

Engine (`red-engine.log`):

```
assertion `left == right` failed: restore must land the target snapshot's labels, not a merge
  left: {"new": "New", "qualified": "Bid", "won": "Won"}
 right: {"new": "New", "won": "Won"}
```

Host (`red-host.log`) — bentuk probe audit, `changes` kosong sementara label target hanya `won`:

```
thread 'restore_removes_the_labels_the_target_snapshot_never_had' (1345299) panicked at crates/host/tests/own.rs:1078:5:
assertion `left == right` failed: the restore plan must land the snapshot's labels, not a merge: {"baseConfig":{"enabled":["crm-contacts","crm-pipeline"],"revision":2,"terminology":{"pipelineLabels":{"new":"Inquiry","won":"Closed deal"}}},"catalogueVersion":"owner-catalogue@1","plan":{"baseRevision":2,"blocked":null,"changes":[],"config":{"enabled":["crm-contacts","crm-pipeline"],"revision":3,"terminology":{"pipelineLabels":{"new":"Inquiry","won":"Closed deal"}}},"effects":[],"planHash":"1e812f0f3c4f5b42"},"pricingVersion":"included@1","targetRevision":1}
  left: Object {"new": String("Inquiry"), "won": String("Closed deal")}
 right: Object {"won": String("Closed deal")}
```

### Perbaikan

1. `Request` mendapat `replace_terminology: Option<Terminology>` — semantik **ganti set**, berbeda dari `terminology` yang bersifat patch (menambah/relabel).
2. `Change` mendapat varian `RemoveStage { stage }`; perubahan di-sort dengan `change_line()` sehingga urutan proposal stabil.
3. `Store::restore` mengirim snapshot target sebagai `replace_terminology`, bukan `terminology`.
4. `canonical()` merender `remove:{stage}`, jadi penghapusan ikut mengubah identity — restore yang hanya menghapus label tidak bisa berbagi hash dengan no-op.

### Bukti HIJAU

- Engine `a3_restore_removes_the_labels_the_target_never_had`: setelah relabel revision 2, restore ke revision 1 menghasilkan `changes` yang memuat `{"kind":"removeStage","stage":"qualified"}`, `config.terminology.pipeline_labels == target.terminology.pipeline_labels`, `plan_hash` berbeda dari no-op di base yang sama, dan apply-nya mendaratkan set label yang sama.
- Host `restore_removes_the_labels_the_target_snapshot_never_had`: restore ke revision 1 (snapshot hanya `won`) menghasilkan plan yang menghapus `new`, dan set label akhir **sama persis** dengan snapshot target.

## Catatan determinisme (eksplisit)

- Tidak ada iterasi `HashMap` di jalur identity. `enabled: BTreeSet`, `pipeline_labels: BTreeMap`, `changes`/`effects` di-sort sebelum hashing dan sebelum dikirim ke klien.
- `effects` di-sort juga di `Plan` (bukan hanya di `canonical()`) supaya dua request dengan urutan modul berbeda men-serialisasi JSON yang identik — ini yang ditangkap `a2_plan_identity_does_not_depend_on_request_order` dan diperbaiki dengan `effects.sort_by(...)` di `build_plan`.
- Identity tidak lagi bergantung pada `Debug`: `change_line()` dan token `readiness_token`/`cost_token`/`blocked_reason_token` adalah kontrak eksplisit.

## Catatan jujur & batasan

1. **`cargo test -p archava-host --test own` harus serial.** Dengan default thread (= jumlah core), 13 test mem-provision 13 instance PostgreSQL sekaligus dan gagal `503` di helper `login`. Dijalankan dengan `-- --test-threads=1` (konvensi yang sama dengan gate G3) → 13/13 hijau. Ini keterbatasan fixture, bukan regresi dari perubahan ini; test yang sama lolos sendiri-sendiri.
2. **Receipt engine tidak punya field `catalogue_version`.** Yang menulis versi ke receipt adalah host (`rust/crates/host/src/own.rs`). Di engine, binding versi diekspresikan lewat `Plan.catalogue_version` + `ApplyError::{CatalogueChanged,PricingChanged}`; di host lewat `check_confirmation` + receipt. Keduanya dibuktikan test masing-masing.
3. **Perubahan wire bersifat aditif**: field `catalogueVersion`/`pricingVersion` pada `plan` dan `confirmation` opsional, varian `removeStage` adalah nilai baru pada enum `changes`. Grep `relabelStage|removeStage` di `packages/**` dan `tools/**` tidak menemukan konsumer TypeScript yang meng-enumerasi kind change, jadi tidak ada card lain yang pecah. Dashboard (card A3-F05/A11) tetap perlu menampilkan `removeStage` agar owner melihat penghapusan label sebagai perubahan.
4. **Regression lain tidak dijalankan**: `pnpm test`, `verify-*`, dan trio ERP berada di luar scope card ini; gate G3 (`docs/audit/AUDIT_GATE_G3_2026-10-22.md`) sudah menutup snapshot HEAD yang sama.
5. Tidak ada hardcode/stub yang membuat test lolos: setiap assertion merah di atas gagal karena perilaku produksi, dan setiap assertion hijau melewati jalur produksi (`canonical`, `Store::apply`, `check_confirmation`, `Store::restore`).
