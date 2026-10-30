# Audit Archava Platform — 23 Oktober 2026

**Verdict: kode-nya keras, tapi kandidat rilis di tree ini belum tersertifikasi.**
Tidak ada kerentanan kritis yang bisa gue buktikan hari ini. Yang merah adalah
**gate**: `pnpm lint` gagal di working tree sekarang, dan itu terjadi karena
hardening XSS A8/F-04 masuk **setelah** gate final 22 Oktober direkam. Jadi
"all eight root gates passed" yang jadi dasar acceptance **tidak lagi mengikat
tree yang lo pegang**.

| Item         | Nilai                                                                                            |
| ------------ | ------------------------------------------------------------------------------------------------ |
| Repo         | `/home/haikaru/Archverse/Lab/Archava_Platform`                                                   |
| HEAD         | `412f763e17f3a68a457fd76c7b929184c50c916a` (sama dengan audit 21–22 Okt)                         |
| Branch       | `feat/laundry-1.17.0`                                                                            |
| Working tree | **202 modified + 74 untracked** — seluruh remediasi masih uncommitted                            |
| Scanner      | `pnpm audit` (platform) 0 advisory; `cargo-audit 0.22.2` terpasang, tidak dijalankan di pass ini |
| Rust gate    | `pnpm rust:verify` dijalankan; hasil final dicatat di §6                                         |

---

## 1. Metode

Audit ini **tidak** mengutip laporan 21–22 Oktober. Semua angka di bawah
diproduksi ulang di tree sekarang, lalu kode dibaca independen di jalur yang
memutuskan keamanan: sesi & login, CSRF/origin, isolasi tenant & RLS, SQL,
binding presence, module gating, dan supply chain.

Yang dijalankan:

```text
pnpm structure        → exit 0    (70 workspace projects, semua import @archava/* dideklarasikan)
pnpm typecheck        → exit 0    (semua package + apps/web)
pnpm format:check     → exit 0    ("All matched files use Prettier code style!")
pnpm test             → exit 0    (159 files, 1.953 test, 7,34 s)
pnpm lint             → exit ≠ 0  (46 error, lihat F-01)
pnpm audit --json     → 0 low/moderate/high/critical
```

`pnpm verify` menjalankan `format` lalu `lint` (`tools/scripts/verify.mjs:40-41`),
dan CI menjalankan `pnpm lint` di `.github/workflows/ci.yml:87`. Artinya
`pnpm verify` **berhenti di langkah kedua** di tree ini, dan PR dari tree ini
merah sebelum sampai ke typecheck/test.

---

## 2. Temuan

### F-01 — P1 — Gate `lint` merah; sertifikasi 22 Okt tidak mencakup tree sekarang

**Bukti.** `eslint . --max-warnings=0` → **46 error di 3 file**:

| File                                                            | Error | Rule                                               |
| --------------------------------------------------------------- | ----: | -------------------------------------------------- |
| `apps/web/src/frontend/workspace.ts`                            |    16 | `@typescript-eslint/restrict-template-expressions` |
| `apps/web/test/server-headers.test.ts:55:26`                    |     1 | `@typescript-eslint/no-unnecessary-type-assertion` |
| `rust/target/site/pkg/archava-slice.js` (generated, gitignored) |    29 | `no-undef` ×19, `no-unused-vars` ×10               |

Lokasi 16 error sumber: `workspace.ts:213:612, 388:185, 396:433, 422:595,
447:412, 561:1687, 569:914, 574:889, 581:757, 588:885, 602:243, 602:284,
749:388, 755:561, 971:302, 971:346`.

**Kenapa ini P1.** Bukan karena stylistik, tapi karena kontrak repo ini sendiri:
`docs/audit/AUDIT_GATE_FINAL_2026-10-22.md:105` merekam `pnpm lint` →
**0 error, 0 warning, 45 s**, terikat ke `SCOPE_HASH` `0b28a999…` (baris 33).
Timeline file membuktikan tree sudah berubah setelah itu:

| Waktu (22 Okt, +07) | Kejadian                                                   |
| ------------------- | ---------------------------------------------------------- |
| 02:13–03:00         | gate final dijalankan; lint 0 error; scope hash direkam    |
| 14:24:24            | `server-headers.test.ts` diubah                            |
| 14:29:37            | `workspace.ts` + `frontend-escape.test.ts` diubah          |
| 18:28:25            | `apps/web/src/frontend/html.ts` dibuat (hardening A8/F-04) |

Jadi 17 error itu **dibawa oleh remediasi yang belum pernah lewat gate**.
Pack itu sendiri menulis aturannya: verdict mengikat hash, bukan commit —
dan hash-nya sudah tidak cocok.

**Bukan XSS.** Gue trace 16 situs itu: `Html` yang diinterpolasi berasal dari
`button()`, `status()`, `detailRow()`, yang semuanya meng-escape input di dalam
`apps/web/src/frontend/html.ts`. `render()` meng-escape `string` dan
melewatkan `Html` — perilaku yang benar. Aturan lint menangkap **batas tipe
yang dilewati lewat `toString()` implisit**, bukan sink yang tidak aman.
Tidak ada stored XSS yang bisa gue buktikan di demo browser ini.

**Dampak.** Klaim "all eight root gates passed" di
`docs/BUILD_STATUS.md:24` tidak reproducible di tree ini; kandidat 1.17.0
belum punya gate yang sah. Remediasinga juga belum di-commit, jadi tidak ada
commit yang bisa di-review sebagai kandidat.

**Fix (mekanis, 3 langkah).**

1. Bungkus 16 interpolasi `Html` dengan `renderValue(...)` (sudah di-import
   sebagai `render`) — mis. `${contacts ? escape(...) : status(...)}` menjadi
   `${renderValue(contacts ? escape(...) : status(...))}`. Jangan longgarkan
   rule-nya; itu justru yang menjaga batas `Html`.
2. Hapus assertion di `apps/web/test/server-headers.test.ts:55`.
3. Tambahkan `'rust/target/**'` ke `ignores` (lihat F-02), lalu jalankan ulang
   `pnpm verify` dan rekam gate pack baru dengan scope hash baru.

---

### F-02 — P2 — ESLint tidak mengabaikan `rust/target`, lokal dan CI jadi beda

**Bukti.** `eslint.config.mjs:72-86` mengabaikan `dist/`, `build/`, `node_modules/`,
`coverage/`, tapi **tidak** `rust/target/`. Direktori itu gitignored
(`git ls-files rust/target` → 0 file) dan Prettier mengabaikannya lewat
`.gitignore`, tapi ESLint 9 tidak membaca `.gitignore`. Hasilnya 29 "error"
dari bundle WASM generated yang tidak pernah masuk repo.

**Dampak.** Developer lokal melihat 46 error, CI (fresh clone) hanya 17. Gate
yang berbeda antara lokal dan CI adalah cara paling halus untuk membuat orang
mengabaikan gate.

**Fix.** Tambah `'rust/target/**'` ke array `ignores`. Verifikasi dengan
`pnpm lint` → harus turun ke 17 error, lalu 0 setelah F-01.

---

### F-03 — P2 — Default identitas fail-open di 6 API factory library

**Bukti.** Di `packages/{attendance,billing,field-services,formal-education,
fulfillment,laundry}/src/api.ts`, parameter `identity` opsional. Kalau tidak
dipasang:

```ts
principal = { kind: 'human', id: request.headers.get('x-user-id') ?? 'anonymous' }
```

Artinya konsumen library yang memanggil `createBillingApi(service)` — bentuk
paling natural — menerima principal dari header yang dikontrol klien. Di repo
ini **tidak live**: `apps/crm/src/host.ts:191-333` selalu mengirim callback
identitas, dan satu-satunya pemanggil lain sudah eksplisit. Tapi ini default
yang salah untuk produk yang menjual reusable library; satu consumer lupa
argumen = impersonasi principal.

**Fix.** Jadikan `identity` wajib di signature (hapus `?`), hapus cabang
`x-user-id`, dan untuk harness bukti yang belum mengirimnya, kirim
`async () => human('viewer')` eksplisit. Fail-closed > fail-open.

---

### F-04 — P3 — Bucket rate-limit login kolaps di belakang reverse proxy

**Bukti.** `apps/crm/src/host.ts:627` mengirim `incoming.socket.remoteAddress`
sebagai `source`. `packages/identity-pg/src/sessions.ts:68-70` memakai
`['source', source, 300, 60]` dan `['account', username, 900, 5]`.
Kalau `settings.publicOrigin` di-set (mode TLS proxy, yang juga mengaktifkan
HSTS dan cookie `__Host-`), semua request masuk dari proxy → **satu bucket
`127.0.0.1` untuk seluruh pengguna**. 61 login gagal dalam 5 menit memblokir
semua orang; login serentak di kantor bisa memicunya tanpa penyerang.

**Status.** Profil yang didokumentasikan adalah host lokal `127.0.0.1`
(`docs/LOCAL_LIBRARY_HANDOFF.md:54`), jadi tidak aktif di mode supported.
Catat sebagai risiko saat `publicOrigin` mulai dipakai.

**Fix.** Kalau `publicOrigin` di-set, ambil hop pertama `x-forwarded-for`
**hanya** kalau peer socket ada di allowlist proxy tepercaya (setting baru),
selain itu tetap `remoteAddress`. Alternatif: buang bucket `source`, pertahankan
bucket akun + cap global yang longgar.

---

### F-05 — P2 — Evidence drift: "all eight gates" tidak cocok dengan tree

**Bukti.** `docs/BUILD_STATUS.md:24` (checkpoint "Current", profile 1.17.0)
menyatakan **1.786 test / 134 file** dan semua gate hijau. Tree sekarang:
**1.953 test / 159 file** dan lint merah. Dokumen itu juga sudah
di-diff untuk mengubah "seven of eight" menjadi "all eight" — jadi klaimnya
pernah benar, lalu basi.

**Kenapa penting.** Platform ini menjual auditabilitas. Dokumen acceptance yang
angkanya tidak bisa direproduksi adalah temuan produk, bukan typo. Ini
kekambuhan A4 dari audit 21 Okt.

**Fix.** Satu baris yang di-generate dari run terakhir (test count + gate
status + scope hash), bukan prosa yang di-edit tangan. Kalau gate merah,
dokumen harus bilang merah.

---

### F-06 — P3 — Coverage gate mengunci floor rendah di permukaan keamanan

**Bukti.** `vitest.config.ts:77-82` memasang floor global lines 33,2%.
Per-scope: `apps/crm` **7,18%** lines, `apps/web` **10,52%**. Ratchet-nya benar
(naik, tidak boleh turun), tapi angka awalnya diambil dari kondisi sekarang.
`apps/crm/src/host.ts` — tempat auth, CSRF, module gating, dan tenant check
berada — nyaris tidak tersentuh unit suite; keamanannya diuji lewat proof
harness PostgreSQL, bukan unit test.

**Fix (bukan blocker).** Tambah threshold per-file untuk `apps/crm/src/host.ts`,
`packages/identity-pg/src/sessions.ts`, dan `packages/acl/src/policy.ts`,
atau pindahkan minimal test keputusan (bukan I/O) ke unit suite supaya
regresi logika otorisasi tertangkap tanpa cluster.

---

## 3. Yang gue verifikasi aman (dengan bukti)

Bukan lip service — ini titik yang gue serang dan tidak tembus:

- **Isolasi tenant (RLS).** 8 tabel CRM `FORCE ROW LEVEL SECURITY`
  (`packages/crm-pg/migrations/001:57-67`, `002:37-39`), policy memakai
  `current_setting('archava.tenant_id', true)`; `set_config(..., true)` per
  transaksi (`packages/crm-pg/src/repository.ts:177`). Runtime menolak role
  owner/superuser/BYPASSRLS saat start (`repository.ts:138-144`).
- **Scope `assigned` sudah benar.** Billing memfilter parent (`invoices`) dulu,
  baru child rows — jadi child tidak bisa bocor sebelum gate
  (`packages/billing-pg/src/repository.ts:813-880`). Attendance
  (`attendance-pg/src/repository.ts:66,618`) dan laundry
  (`laundry-pg/src/repository.ts:60,673`) punya filter `owner_id` yang sama.
  Ini A10 dari audit lama: **fixed**.
- **Sesi.** Absolut 8 jam + idle 30 menit
  (`packages/identity-pg/migrations/001_identity.sql:14-15`,
  `sessions.ts:125`), token di-hash, CSRF `timingSafeEqual`, cookie
  `__Host-` saat secure + `HttpOnly; SameSite=Strict`
  (`apps/crm/src/host.ts:54-55,92`).
- **CSRF/origin.** Semua POST butuh `x-archava-csrf` (`host.ts:161-166`);
  request absolut-form dan POST lintas-origin ditolak sebelum routing
  (`host.ts:99-101`), jadi login CSRF tertutup. Body dibatasi 16 KB
  (`host.ts:606-617`).
- **Tenant oracle (A11/F-10).** Mount domain tidak lagi mengecek
  `settings.tenants`; tenant tak dikenal dan non-member sama-sama 403 lewat
  `service.access` (`host.ts:185-333`). Tidak ada lagi 404-vs-403.
- **Presence binding.** `PresenceGateway` membandingkan
  `scope.identity.binding` dan tenant sebelum menjawab
  (`packages/presence/src/index.ts:106`), jadi session ID tidak bisa dipakai
  orang lain.
- **Module gating.** Jalur `/actions/*` diblok sebelum dispatch kalau
  `modules.nativeActions` mati (`host.ts:348-353`); `/api/education/*` dijaga
  `modules.formalEducation` + service (`host.ts:270-276`).
- **SQL.** Tidak ada interpolasi input user ke query. Satu-satunya
  `format!`/template di migrasi memakai nama role yang divalidasi regex
  (`packages/crm-pg/src/migrate.ts:65`). Semua query data terparameterisasi.
- **Supply chain.** `pnpm audit` 0 advisory. Semua `uses:` di
  `ci.yml` dan `security-audit.yml` dipin ke commit SHA, bukan tag.
- **Header web.** CSP/HSTS/nosniff/referrer sekarang ada di `apps/web` dan
  template (`apps/web/src/server/security.ts`, `Archava Template/server/app.mjs`),
  menutup A8/F-02.
- **Test suite.** 1.953 test hijau, termasuk test escape baru
  (`apps/web/test/frontend-escape.test.ts`) yang mengunci perilaku
  `render()`/`html()`.

---

## 4. Status temuan audit sebelumnya

| ID       | Temuan                                               | Status di tree ini                                         |
| -------- | ---------------------------------------------------- | ---------------------------------------------------------- |
| A10      | scope `assigned` bocor di billing/attendance/laundry | **Fixed** — diverifikasi ulang di kode                     |
| A11 F-10 | tenant oracle 404 vs 403                             | **Fixed** — cek `settings.tenants` dihapus                 |
| A6       | coverage tanpa threshold                             | **Fixed** — threshold + ratchet ada                        |
| A8 F-02  | `apps/web` tanpa header keamanan                     | **Fixed** — `security.ts`                                  |
| A8 F-04  | sink HTML demo browser                               | **Fixed secara perilaku**, tapi melanggar gate lint (F-01) |
| A1       | receipt recovery terikat initiator (Rust)            | Diklaim fixed; **belum diverifikasi ulang** di pass ini    |
| A4       | evidence/angka dokumen basi                          | **Terbuka** — F-05                                         |

---

## 5. Sisa ketidakpastian

1. **Rust gate.** `pnpm rust:verify` dijalankan di pass ini tapi masih
   meng-compile workspace saat laporan ditulis. Audit 21 Okt menemukan gate ini
   gagal **hanya** karena `TMPDIR` panjang; run ini memakai `/tmp/arv`.
   Hasil final masuk ke evidence run.
2. **`pnpm security:verify` tidak dijalankan.** Gate itu butuh armada proof
   PostgreSQL + artefak ter-pack (`tools/security/verify.mjs`), bukan hal yang
   bisa diselesaikan dalam satu pass review.
3. **Tidak ada pentest live host.** Semua temuan berasal dari kode + gate,
   bukan eksploitasi runtime. `crm:verify-runtime` (44 cek) belum dijalankan
   ulang.

---

## 6. Urutan perbaikan yang gue sarankan

1. **F-01 + F-02** — 16 `renderValue()` wrap, 1 assertion, 1 baris `ignores`.
   Setelah itu `pnpm verify` harus hijau; baru rekam gate pack baru.
2. **Commit remediasi.** 202 file modified + 74 untracked berarti tidak ada
   commit yang bisa dijadikan kandidat. Gate pack yang mengikat hash memang
   jujur, tapi hash atas tree kotor bukan artefak rilis.
3. **F-05** — generate baris status di `BUILD_STATUS.md` dari run terakhir.
4. **F-03** — jadikan `identity` wajib sebelum library di-publish ke consumer.
5. **F-04** — dokumentasikan atau perbaiki sebelum `publicOrigin` dipakai.
6. **F-06** — threshold per-file untuk tiga file keamanan inti.
