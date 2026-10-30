# A8-FU — Hardening TS/web/tools (t_0d834d2e)

Sumber: `docs/audit/ARCHLAB_AUDIT_2026-10-21.md` §A8 (ringkasan 7 item) +
`docs/audit/archlab-2026-10-21-injection.md` (detail F-01…F-07). Card ini hanya
menangani sisi TS/web/tools. Sisi Rust host (F-01 cookie, F-02 headers) ditutup
oleh card A8 (`t_849628ec`, `docs/audit/A8_HARDENING_2026-10-22.md`).

Branch `feat/laundry-1.17.0`, dirty tree bawaan dipertahankan, 0 commit, 0 stash.
`rust/**` tidak disentuh, `apps/web/src/server/security.ts` tidak disentuh.

## Status per item

| Item                                                                         | Status                            | Bukti                                                                                                                                                                                                                                                                                                              |
| ---------------------------------------------------------------------------- | --------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| F-03 `Math.random()` fallback session id (`apps/web/src/page.ts`)            | ✅ sudah fix sebelumnya           | Regresi terpasang: `apps/web/test/session-id.test.ts` (3 case: randomUUID / fallback `getRandomValues` 16-byte hex / refuse tanpa sumber). Negative control RED_EXIT=1, restore sha256 identik (`evidence/a8fu/f03-sessionid-*.log`).                                                                              |
| F-04 helper HTML tanpa escape (`apps/web/src/frontend/html.ts`)              | ✅ sudah fix sebelumnya           | `html.ts`: `Html` branded + `render()` + `button()`/`detailRow()` escape-by-default; regresi `apps/web/test/frontend-escape.test.ts`. Sisa audit sink diperiksa semua aman (Item 2). Negative control RED_EXIT=1, restore sha256 identik (`evidence/a8fu/f04-*.log`).                                              |
| F-05 `Math.random()` identifier (audit: `packages/*`)                        | ✅ sudah fix sebelumnya           | `packages/subscriptions/src/dashboard.ts` idempotency key = `crypto.randomUUID()`; `packages/attendance/src/ids.ts` = `crypto.getRandomValues` + test. **Residual outside this card:** `packages/capacity/src/dashboard.ts:339` masih Math.random → perlu card packages.                                           |
| F-05 residual `Math.random()` di `apps/crm` + `tools/library` (port pickers) | ✅ fixed di card ini              | 19 file → `node:crypto randomInt` + alasan tertulis (Item 1). `node --check` 18/18 OK, prettier OK, grep residu 0 (`evidence/a8fu/ports-randomint-*.log`).                                                                                                                                                         |
| F-06 password fixture dicetak ke log                                         | ✅ tidak ada di scope; guard baru | Grep `apps/**`, `tools/e2e/**`, `tools/library/**`: tidak ada pencetakan nilai password (Item 4). Lokasi audit F-06 ada di `rust/**` (`login.rs:150`, example) — di luar scope card ini (terlarang karena bentrok card A8). Guard test baru: `apps/crm/test/password-fixture.test.ts` dengan negative control RED. |
| F-07 token sesi mentah di body login                                         | ✅ sudah benar; verifikasi baru   | `apps/crm/src/host.ts:113-115` — body `{authenticated: true}`, token hanya lewat `sessionCookie()`: `HttpOnly; SameSite=Strict; Path=/` (+`; Secure` saat https, nama `__Host-archava_session`). Test baru di `apps/crm/test/host.test.ts` + negative control RED_EXIT=1 (Item 5).                                 |
| F-01 cookie `Secure`/`__Host-`, F-02 CSP/HSTS                                | ➡ card A8                        | `docs/audit/A8_HARDENING_2026-10-22.md` (`t_849628ec`).                                                                                                                                                                                                                                                            |

## Item 1 — F-03/F-05: sisa `Math.random()` di `apps/crm` + `tools/library`

**Klasifikasi temuan.** Sisa satu-satunya di kode in-scope adalah _pemilih port_
(anti-tabrakan dev/proof listener), bukan identifier otorisasi:

- `apps/crm/src/operator.ts:191` — `databasePort: 20000 + Math.floor(Math.random()*25000)`
- 13 × `tools/library/verify-*.mjs` — `port = 20000 + Math.floor(Math.random()*25000)`
- `verify-{billing,fulfillment,laundry,field-services}.mjs` — `PORT = 5400 + Math.floor(Math.random()*1000)`
- `verify-formal-education.mjs` — `PORT = 5500 + Math.floor(Math.random()*500)`

Port acak bukan secret dan bukan kontrol akses: tabrakan port kalah di OS
(EADDRINUSE → retry), dan nilainya tidak pernah menyeberang batas trust.
Namun `Math.floor(Math.random()*N)` tetap ECMA-PRNG yang bisa diprediksi —
tidak ada biaya meninggalkan sumber `Math.random()` di source dari proof chain,
jadi **diganti `node:crypto randomInt(0, N)`** (uniform integers kriptografis,
modulo-bias-free) dengan alasan tertulis di sini, bukan cuma "biar grep bersih".

- 19 file diubah; `randomInt` digabung ke import `node:crypto` yang sudah ada
  (tidak ada import duplikat). Bukti edit + sha256 per file:
  `evidence/a8fu/ports-randomint-edit.log`, `/tmp/a8fu-ports-hashes.json`.
- Verifikasi: `node --check` 18/18 OK; probe distribusi ekspresi barunya
  (200k draw): min 20000, max 44999, allInteger, allInRange
  → `evidence/a8fu/ports-randomint-verify.log`; prettier 19 file OK.
- Grep residu in-scope setelah edit: `apps/crm/src` 0, `tools/library/*.mjs` 0.
- File `.mjs` ini tidak punya unit test vitest (script infrastruktur yang
  membutuhkan dist/library + PG penuh); bukti korektnya = parse check, probe
  range, prettier, dan grep. Tidak diterapkan red-first karena perubahannya
  hanya sumber integer (semantik sama, rentang setara) — bukan perilaku
  security yang dijaga assertion.

**Last resort terhadap review:** nilai port setelah edit tetap berada di
rentang yang sama (`[20000,45000)`, `[5400,6400)`, `[5500,6000)`), jadi
perilaku anti-tabrakan tidak berubah.

## Item 2 — F-04: helper HTML meng-escape by default (regresi + audit sink tersisa)

Fix utamanya sudah ada di tree: `apps/web/src/frontend/html.ts` mengekspor:

- `Html` — branded class; satu-satunya jalan ke markup adalah `html(s)` yang
  harus ditulis dengan sengaja;
- `render(value: string | Html)` — markup lolos, text di-escape;
- `button(label, action, …)` dan `detailRow(label, value: string | Html)` —
  keduanya meng-escape parameter teks (audit menandai keduanya sebagai raw).

**Audit sink tersisa, diperiksa satu per satu (semua aman):**

- `workspace.ts` interpolasi atribut yang TIDAK lewat `escape()`:
  `${target}`/`${stage}`/`${mark}`/`${name}`/`${sku}` (tuple konstanta demo),
  `${nextDay()}`, `${index}`/`${key}`, `${boardMode}`/`${aiVisible}` (boolean
  milik app), `${lead?.value ?? 0}` (tipe `number`, `data.ts:47` — angka tidak
  bisa menembus kutip atribut), `${href(...)}` (dibangun dari nama view statis).
  Tidak ada record field yang lolos mentah.
- `workspace.ts:569` `detailRow('Task', \`Follow up with ${lead.name}\`)`melempar kalimat berisi record field sebagai **plain string** → kini
di-escape di dalam`detailRow` — perilaku escape-by-default bekerja.
- `surface.ts` — `escapeHtml` + `escapeAttribute` dipakai di seluruh markup
  (`:97-98`, `:161`, atribut `="${escapeAttribute(...)}"`) ✓ verifikasi.
- `page.ts:190` `host.innerHTML = surfaceMarkup(units)` — units di-escape di
  dalam komposisi markup ✓.
- `marketing.ts:110/175` — konten dari objek statis internal, bukan input user.

**Negative control (bukti assertion asli):** `detailRow` sementara dibongkar
ke nilai mentah (`${value}` tanpa `render`) → `frontend-escape.test.ts`
2 test gagal `RED_EXIT=1` (vitest — kode exit gagal runner ini adalah 1,
bukan 101) dengan `GREEN_BASELINE_EXIT=0`; restore byte-identik: sha256
`apps/web/src/frontend/html.ts` = `fd3677d222daff081838cfe6bb7d39c7ce48bfdcd4d8fab095bf076bce7ab5a8`
sebelum & sesudah. Log: `evidence/a8fu/f04-escape-green.log` +
`evidence/a8fu/f04-red.log`.

## Item 3 — F-03: negative control fallback session id

Fix sudah ada: `apps/web/src/page.ts:207-221` `newSessionId()` memakai
`randomUUID` → fallback `getRandomValues` 16-byte hex → `throw` bila dua-duanya
tidak ada (`Math.random()` hanya tersisa di komentar penjelasan). Test regresi
`apps/web/test/session-id.test.ts` membuktikan ketiga jalur dengan stub global.

**Negative control:** fallback `Math.random()` ditambahkan sementara sebelum
`throw` → test `refuses to mint a session when no cryptographic source exists`
gagal `RED_EXIT=1`; SHA256 `apps/web/src/page.ts` identik sebelum & sesudah
(`f8d4cbe3e9c7a4d25e39347c3cde9d3bff1b38508ab11cc60f955e774bf7357a`). Log:
`evidence/a8fu/f03-sessionid-{green,red}.log`.

## Item 4 — F-06: password fixture tidak melewati log TS/tools

Grep menyeluruh (`console.*password`, `password.*console`, literal fixture
`correct horse battery staple`) di `apps/**`, `tools/e2e/**`,
`tools/library/**`: **tidak ada cetakan nilai password**. Yang tampak hanya
pemakaian password sebagai payload login (mis. `tools/e2e/crm-minimal.mjs:15`
mengisi form login via DOM — tidak menge-print; nilai fixture pun acak
`randomBytes(24)` di `verify-formal-education-host.mjs:137`).

Residual sesuai audit ada di sisi **Rust** (`rust/crates/slice/src/login.rs:150`
mencetak daftar akun fixture di halaman login; hash precomputed di example) —
di luar scope card ini (terlarang menyentuh `rust/**`, bentrok card A8).
Direkomendasikan card Rust untuk gate pencetakan itu di belakang flag fixture.

**Guard baru** — `apps/crm/test/password-fixture.test.ts`: memindai
`apps/crm/{src,scripts}`, `apps/web/src`, `tools/e2e`, `tools/library` untuk
pola print `console.*` yang membawa `password` dan menolak file yang
meng-hardcode password jelas di sumber TS (assertion asli: gagal bila ada
file baru berisi print). Negative control: file probe sementara
`apps/crm/src/a8fu-red-probe.ts` berisi `console.log("password: red-probe")`
→ test merah; file dihapus → hijau.
Log: `evidence/a8fu/f06-*.log`.

## Item 5 — F-07: token sesi tidak pernah di body login (verifikasi + test)

**Fakta kode `apps/crm/src/host.ts` (dibaca, bukan diasumsikan):**

- `:107-115` `POST /api/login` → `json({ authenticated: true }, 200,
{ 'set-cookie': sessionCookie(cookieName, result.token, secure) })`. Body
  hanya `{authenticated: true}` — token mentah tidak pernah masuk body.
- `:54-56` `sessionCookie` = `name=token; HttpOnly; SameSite=Strict; Path=/`
  - `; Secure` saat https (`secure = origin https:`, `:91`), dan nama cookie
    `__Host-archava_session` saat https / `archava_local_session` saat http
    (`:92`) — konsisten dengan fix F-01 card A8.
- Penyisihan lain yang memakai token: logout `/api/logout` juga hanya
  set-cookie clear (`:171-174`); `/api/me` (`:144-158`) membalas `csrf` (nilai
  anti-CSRF, memang disengaja lewat body untuk header mutasi), bukan token.

**Test baru** (`apps/crm/test/host.test.ts`, identity stub yang mengembalikan
token 43-char):

1. `sends the session token only through Set-Cookie, never the login body
(A8/F-07)` — body harus persis `toEqual({ authenticated: true })`,
   `not.toContain` nilai token, dan set-cookie memuat token + `HttpOnly` +
   `SameSite=Strict` (origin http);
2. `on a secure origin the login cookie is __Host- named and Secure (A8/F-07)`
   — origin https: nama cookie `__Host-archava_session` + `; Secure` di
   set-cookie.

**Negative control:** body login sementara diubah jadi
`{ authenticated: true, token: result.token }` → test (1) gagal
`AssertionError: expected { authenticated: true, …(1) } to deeply equal
{ authenticated: true }`, `RED_EXIT=1` (vitest). Restore byte-identik: sha256
`apps/crm/src/host.ts` =
`ff24e3616839aa3cb1fda260fcb57280fc7fcfd123e887f303d02679abcb6b80` sebelum &
sesudah; re-green `RESTORE_EXIT=0`. Log:
`evidence/a8fu/f07-host-{green,red,restore}.log`.

Aplikasi `apps/web` tidak punya endpoint login/SSO yang membalas token di body
(grep `login|token|set-cookie` di `apps/web/src/server/*.ts` tanpa `security.ts`
→ nol hasil). Template server di luar scope card ini.

## Gate

| Command                                                                   | Exit                                                                                                                                                                                                       |
| ------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `TMPDIR=/tmp pnpm exec vitest run apps/web/test/session-id.test.ts`       | 0 (green, RED kontrol = 1)                                                                                                                                                                                 |
| `TMPDIR=/tmp pnpm exec vitest run apps/web/test/frontend-escape.test.ts`  | 0 (green, RED kontrol = 1)                                                                                                                                                                                 |
| `TMPDIR=/tmp pnpm exec vitest run apps/crm/test/host.test.ts`             | 0 — 9 passed (RED kontrol = 1; restore 0)                                                                                                                                                                  |
| `TMPDIR=/tmp pnpm exec vitest run apps/crm/test/password-fixture.test.ts` | 0 (RED kontrol = 1; restore 0)                                                                                                                                                                             |
| `TMPDIR=/tmp pnpm exec vitest run <4 file terkait digabung>`              | 0 — 16 passed (16)                                                                                                                                                                                         |
| `pnpm exec tsc -p apps/crm/tsconfig.json --noEmit`                        | 0                                                                                                                                                                                                          |
| `pnpm exec tsc -p apps/web/tsconfig.json --noEmit`                        | 0                                                                                                                                                                                                          |
| `pnpm format:check` (repo-wide)                                           | 1 — pre-existing 4 file (docs/audit/A11*AUTHZ*…, docs/audit/A8*HARDENING*…, docs/rust/…×2), tidak ada file yang disentuh card ini; baseline dicapture sebelum edit (`evidence/a8fu/prettier-baseline.log`) |
| `pnpm exec prettier --check` scoped ke 23 file sentuh + doc               | 0 (`evidence/a8fu/gate-prettier.log`)                                                                                                                                                                      |

Semua raw output ada di `docs/audit/evidence/a8fu/*.log`; manifest sha256 semua
artefak di `evidence/a8fu/file-manifest.log`.
