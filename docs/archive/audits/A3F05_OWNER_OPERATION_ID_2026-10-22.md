# A3/F05 — operation identity owner `apply`: id persisten, replay route, response hilang

- **Card**: `t_d9b90638` (A3/F05, sisi Rust + browser)
- **Sumber temuan**: `docs/audit/ARCHLAB_AUDIT_2026-10-21.md:176-190`, `docs/audit/archlab-2026-10-21-owner-claims.md`
- **Tanggal**: 2026-10-22 (Asia/Jakarta)
- **Branch / HEAD**: `feat/laundry-1.17.0` @ `412f763e17f3a68a457fd76c7b929184c50c916a`
- **Dirty tree bawaan**: dipertahankan. Tidak ada `git add`, `git commit`, `git stash`, atau revert.
- **Scope yang disentuh (5 file)**:
  `rust/crates/slice/src/owner.rs`, `rust/crates/host/src/own.rs`,
  `rust/crates/host/src/router.rs`, `rust/crates/host/tests/own.rs`,
  `tools/rust/browser-slice.mjs`
- **Tidak disentuh**: `rust/crates/own/**` (card A2+F06), `rust/crates/adapters/**` (card A11),
  `packages/**`, `rust/Cargo.lock` — dibuktikan di bagian _Scope proof_.
  `rust/crates/slice/src/pending.rs` juga **tidak berubah**: modul marker `localStorage` itu
  sudah ada di HEAD (dipakai layar kontak); cacatnya adalah `owner.rs` tidak mengimpornya.

## Verdict

Tiga cacat F05 ditutup dan dibuktikan merah→hijau, termasuk di browser sungguhan:

1. **Satu operation id per intent.** `owner.rs` memanggil `reuse_or_mint()` (baris 86) yang
   memakai ulang marker `pending` milik `(tenant, principal)`; marker ditulis ke `localStorage`
   **sebelum** request dikirim (baris 275-283). Kalau storage tidak bisa ditulis, apply
   dibatalkan (`Action::Offline`) — tidak ada operasi yang identitasnya tidak bisa direcover.
2. **Route status/replay.** `GET /own/status?tenant=&requestId=` (router baris 377,
   handler `own::status` baris 942) mengembalikan receipt tersimpan dengan binding aktor
   (`receipt.actor == principal`) dan grant `config.read`; operasi yang tidak pernah commit —
   atau milik principal lain — dijawab `unknown`, bukan dieksekusi. `POST /own/apply` dengan
   `request_id` yang sudah ada mengembalikan receipt yang sama (`replayed: true`) tanpa menulis
   ulang, dan menolak id milik aktor lain (`request_id_reused` 409).
3. **Test response hilang yang nyata.** Harness browser memblok respons `POST /own/apply` di
   CDP `Fetch` **setelah** host commit (`Fetch.failRequest Aborted`, `requestStage: 'Response'`),
   lalu reload: halaman harus melaporkan outcome tersimpan dari id yang sama, dan psql
   membuktikan tidak ada revisi/receipt kedua.

## Gate (dari `rust/`, `TMPDIR=/tmp`)

| Gate                                                                                            | Exit | Output mentah                                              |
| ----------------------------------------------------------------------------------------------- | ---- | ---------------------------------------------------------- |
| `cargo test --offline --locked -p archava-slice`                                                | 0    | `2 passed; 0 failed`                                       |
| `cargo test --offline --locked -p archava-host --test own -- --test-threads=1`                  | 0    | `16 passed; 0 failed` — 197.93s                            |
| `cargo fmt --all --check`                                                                       | 0    | 0 baris                                                    |
| `cargo clippy --offline --locked -p archava-slice -p archava-host --all-targets -- -D warnings` | 0    | `Finished` — 0 warning                                     |
| `node tools/rust/browser-slice.mjs`                                                             | 0    | `evidence → dist/rust-evidence/P-RUST-BROWSER.json (pass)` |

Test host dijalankan serial (`--test-threads=1`): tiap test menyalakan instance PostgreSQL
sendiri dan 16 instance paralel saling rebutan (temuan yang sama dengan card A2+F06).

Trio ERP (`erp_billing`/`erp_attendance`/`erp_subscriptions`) **tidak dijalankan** sesuai
instruksi card (di luar scope, ~52 menit).

## Snapshot pembuktian (hash-pinned)

```
HEAD=412f763e17f3a68a457fd76c7b929184c50c916a
branch=feat/laundry-1.17.0
f645d6cc026804edc34a371aee0cc3f205ad470aa7d8e3a64b175a8d56c0b8e3  crates/slice/src/owner.rs
bd5b7a8ec4e1a7bae6a94446ca4af2aa4f90e1ff9a71e3c0b23a73157281caa3  crates/slice/src/pending.rs   (tidak berubah)
b021fac708bfe45540a729c1d4c9510fa5f3911b057056f23933224bc01c82be  crates/host/src/own.rs
1fce54f5d9f40a526fcd6e92c291f832097a192737e032349d1f7249a28cf746  crates/host/src/router.rs
45edf88b2f8599acc788e91a5da59e0ebd417093524b94d3c83497b889cbeac1  crates/host/tests/own.rs
1a9f0064dbc5acc650987195b94d46d158b890134db4ae004ed04f641d1a7a66  tools/rust/browser-slice.mjs
```

Evidence mentah: `docs/audit/evidence/a3f05/` — `f05-red-host.log`, `f05-green-full-host.log`,
`f05-slice-final.log`, `f05-fmt-clippy-final.log`, dan empat run harness
(`f05-browser.log`, `f05-browser2.log`, `f05-browser3.log`, `f05-browser4.log`, terakhir = hijau).

## Cacat 1 — id baru pada setiap klik, tidak pernah dipersist

Sebelum: `uuid_string()` dipanggil di dalam handler confirm, jadi setiap klik/retry adalah
operasi baru. Marker `pending` (yang sudah menulis `localStorage` untuk layar kontak) tidak
diimpor sama sekali oleh `owner.rs`. Efeknya persis yang ditulis audit: response yang hilang
membuat owner kehilangan identitas operasi, dan satu-satunya jalan adalah intent baru —
berisiko double-apply.

Sesudah (`crates/slice/src/owner.rs`):

- `reuse_or_mint(held, principal, tenant)` (baris 86) memakai ulang `request_id` hanya bila
  marker milik principal+tenant yang sama; selain itu mint baru.
- `pending::write(&marker)` dipanggil **sebelum** `spawn_local` yang mengirim request
  (baris 275-283). Kalau `localStorage` menolak, apply tidak dikirim.
- Saat halaman dimuat, `recover()` (baris 150-183) membaca marker dan menanyakan
  `GET /own/status` dengan key asli:
  - `decision: "receipt"` → key dianggap lunas (`pending::clear`), halaman menampilkan
    `recovered {outcome} committed revision {n}` lalu refresh;
  - `decision: "unknown"` → key tetap disimpan dan status menunjukkan operasi masih pending,
    supaya retry memakai key yang sama.

## Cacat 2 — tidak ada route status/replay

`crates/host/src/own.rs`:

- `status_inner()` (baris 957) memvalidasi `requestId`, mewajibkan sesi + grant
  `config.read`, membaca `apply_receipts` dalam satu transaksi, dan hanya mengembalikan
  receipt bila `receipt.actor == principal`. Selain itu `{decision:"unknown", replayed:false}`
  — absen bukan bukti gagal, jadi klien tidak boleh membuang key.
- Jalur `POST /own/apply` yang sudah ada tetap menjadi replay: request id yang punya receipt
  mengembalikan receipt itu dengan `replayed: true` dan tidak menulis apa pun; id milik aktor
  lain ditolak `409 request_id_reused`. Route didaftarkan di `crates/host/src/router.rs:377`.

Bentuk ini mengikuti pola replay+binding A1 (grant dulu, baru baca ledger dengan binding
aktor) — bukan menyalin `packages/billing-pg/src/repository.ts:245-275`, yang hanya jadi acuan
bentuk.

## Cacat 3 — `refreshRecovery` hanya reload setelah apply sukses

`tools/rust/browser-slice.mjs` blok `--- F05: an owner apply whose answer died on the wire ---`
(baris 1599 dst):

1. Halaman `/owner` pada revisi 4; hitung `config_revisions` + `apply_receipts` via psql.
2. `Fetch.enable` dengan pattern `${ORIGIN}/own/apply*`, `requestStage: 'Response'` — jadi
   **server sudah menulis** dan hanya responsnya yang dibunuh (`Fetch.failRequest Aborted`).
3. UI harus menampilkan `operation … is still pending`, elemen `owner-pending-id` memuat key,
   dan `localStorage['archava.pending.bulk-demo']` memuat key yang sama.
4. psql: `apply_receipts` terbaru = key itu; jumlah revisi naik **tepat 1**.
5. Reload `/owner`: status `recovered apply committed revision 5`, jumlah revisi **tidak**
   berubah, jumlah receipt tidak berubah, dan marker terhapus.

Witness yang dicatat ke evidence:

```
✓ owner_lost_response_committed — the answer for 5a770fbe… died on the wire after the host committed revision 5
✓ owner_lost_response_recovered — the reload replayed 5a770fbe… and reported revision 5 with no second write
evidence → dist/rust-evidence/P-RUST-BROWSER.json (pass)
```

`dist/rust-evidence/P-RUST-BROWSER.json` → `result: pass`; check
`"authorized recovery after lost response": "pass"`; `details.owner`:
`lostResponseRevision: 5`, `lostResponseRecovered: true`, `configurationRestored: true`,
`appliedRevisions: [1,2,3,4]`, `finalRevision: 4`, `auditCount: 4`.

## Red-by-design

Test host ditulis dan dijalankan **sebelum** route/handler ada
(`docs/audit/evidence/a3f05/f05-red-host.log`, 2026-10-22 09:13, tree pra-fix):

```
test a_committed_request_id_reads_back_its_receipt_after_a_lost_response ... FAILED
test an_uncommitted_request_id_answers_unknown_and_writes_nothing ... FAILED
test another_principal_cannot_read_or_replay_a_foreign_request_id ... FAILED

thread 'a_committed_request_id_reads_back_its_receipt_after_a_lost_response' panicked at crates/host/tests/own.rs:222:69:
called `Result::unwrap()` on an `Err` value: Error("EOF while parsing a value", line: 1, column: 0)
thread 'another_principal_cannot_read_or_replay_a_foreign_request_id' panicked at crates/host/tests/own.rs:1257:5:
assertion `left == right` failed
  left: Null
 right: "78bebe66e65d6014"

test result: FAILED. 0 passed; 3 failed; 0 ignored; 0 measured; 13 filtered out; finished in 60.18s
```

`EOF while parsing a value` = route `/own/status` belum ada (body kosong, bukan JSON);
`Null vs "78be…"` = replay belum mengikat aktor. Di tree yang sama, suite lama tetap hijau —
kegagalan datang dari test baru, bukan dari test yang dibuat lolos karena stub.

Untuk slice, test unit `reuse_or_mint` baru bisa ada bersama fungsinya (fungsi itu sendiri
yang jadi perbaikan); bukti perilakunya yang menentukan ada di suite host + harness browser
di atas, bukan di test yang menguji implementasinya sendiri.

## Scope proof

Card ini hanya menulis lima file di atas. Tidak ada tulisan ke card tetangga setelah
implementasi dimulai (09:29 WIB):

```
$ find rust/crates/own rust/crates/adapters packages rust/Cargo.lock -newer rust/crates/slice/src/owner.rs -type f
(empty)
```

`git status --porcelain` untuk kelima file scope menunjukkan `M` seluruhnya; entri `M`/`??`
lain di tree adalah dirty bawaan card lain yang sudah ada sebelumnya dan tidak disentuh.
`rust/Cargo.lock` bersih.

## Catatan jujur

- **Dua witness owner dipetakan ke check yang sudah ada**, `authorized recovery after lost
response`. Menambah check baru berarti mengubah manifest check yang dipakai harness
  (di luar 5 file scope card ini); memetakan ke check yang sama justru memperkuatnya — check
  itu sekarang punya bukti dari alur draft dan dari alur owner.
- **Harness mengembalikan konfigurasi setelah uji F05.** Intent F05 sengaja mematikan
  `crm-pipeline` (revisi 5). Karena journey setelahnya membaca pipeline, harness melakukan
  satu apply normal untuk meng-enable kembali (revisi 6). Apply itu terjadi **setelah**
  fakta owner yang dinilai (`appliedRevisions: 1..4`, `finalRevision: 4`, `auditCount: 4`)
  ditangkap, jadi grader P-OWN-T06 tetap terikat ke journey aslinya; fakta tambahan dicatat
  sebagai `owner.configurationRestored = true`.
- Run harness ketiga gagal di matcher status (`enable committed revision` vs teks nyata
  `apply committed revision 6`); lognya disimpan, run keempat hijau. Build release + wasm
  memakan ~5-6 menit per run, jadi gate browser dijalankan sekali di akhir, bukan per iterasi.
- Tidak ada perubahan pada `pending.rs`: modul marker sudah ada di HEAD; yang cacat adalah
  `owner.rs` tidak memakainya, dan itu yang diperbaiki.
