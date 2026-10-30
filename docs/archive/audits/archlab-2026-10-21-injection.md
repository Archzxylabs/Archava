# Archava Platform — Audit Injection / XSS / SSRF / Secrets / Crypto

- **Tanggal:** 2026-10-21 (WIB)
- **Auditor:** archlab (Hermes subagent)
- **Repo:** `/home/haikaru/Archverse/Lab/Archava_Platform`
- **Commit:** `412f763` — branch `feat/laundry-1.17.0` (working tree dirty; nomor baris mengacu ke isi file di working tree saat audit)
- **Mode:** READ-ONLY. Tidak ada file produk yang diubah. Satu-satunya artefak tulis adalah laporan ini.
- **Cakupan:** `apps/`, `packages/`, `tools/`, `rust/crates/`, `Archava Template/` (721 file TS/JS termasuk `.mjs` + 158 file Rust, exclude `node_modules`, `dist`, `target`).

## Ringkasan eksekutif

**0 P1, 0 P2, 7 P3.** Tidak ditemukan SQL injection, command injection, XSS yang dapat dieksploitasi, SSRF, atau kebocoran secret aktif.

Alasan utamanya struktural, bukan kebetulan:

1. **sqlx 0.9 memaksa `AssertSqlSafe`** untuk SQL runtime. 54 pemakaian diperiksa satu per satu; setiap SQL dinamis hanya menginterpolasi placeholder index (`$n`), nama tabel dari `matches!`/union type, atau `namespace` konstanta.
2. **Semua 24 fungsi `grant*Runtime`** memvalidasi nama role dengan regex ber-anchor `/^[a-z][a-z0-9_]{0,62}$/` sebelum identifier masuk ke `GRANT` (identifier memang tidak bisa jadi parameter).
3. **Tidak ada HTTP client sama sekali** di Rust (`reqwest` = 0, `hyper` client = 0) dan tidak ada `http.request`/`axios` di TS product. Permukaan SSRF praktis nol.
4. **Tidak ada `dangerouslySetInnerHTML`/`insertAdjacentHTML`/`eval`/`new Function`** (0 hit). Dashboard modul memakai `createElement` + `textContent`.
5. **Identitas & kripto sudah benar**: scrypt (32768/8/3, maxmem 48 MiB), `randomBytes(32)`, `timingSafeEqual`, CSRF = HMAC-derived dan tidak disimpan, session token hanya disimpan sebagai HMAC digest.

Temuan yang tersisa adalah **hardening**: satu cookie tanpa `Secure` di host Rust, header keamanan yang belum dipasang di tiga entrypoint, empat pemakaian `Math.random()` pada identifier, satu helper HTML yang tidak meng-escape (belum ada caller yang injectable), dan fixture password yang dicetak saat runtime.

---

## 1. Temuan

### F-01 — Cookie sesi host Rust tanpa `Secure` dan tanpa prefix `__Host-`

| Field      | Nilai                                                                                                                                                                |
| ---------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Severity   | **P3**                                                                                                                                                               |
| Kategori   | Crypto / cookie flags                                                                                                                                                |
| Lokasi     | `rust/crates/host/src/router.rs:1751` (set saat login), `rust/crates/host/src/router.rs:1879` (expire saat logout), nama cookie `rust/crates/host/src/router.rs:111` |
| Reachable  | Ya — setiap browser yang login ke host Rust                                                                                                                          |
| Confidence | **High** (kode eksplisit; tidak ada middleware yang menambahkan `Secure`)                                                                                            |

**Bukti**

```rust
// router.rs:1751
format!("{SESSION_COOKIE}={token}; HttpOnly; Path=/; SameSite=Strict")
```

`SESSION_COOKIE = "archava_session"` (baris 111). Tidak ada `Secure`, tidak ada prefix `__Host-`. Komentar di `router.rs:105-110` menyatakan ini disengaja karena "fixture runs plain HTTP — a TLS deployment fronts this host with its own cookie policy", tetapi **reverse proxy tidak bisa menambahkan `Secure` ke `Set-Cookie` yang sudah dikirim aplikasi**. Jadi bila host ini di-deploy di belakang TLS (skenario yang didokumentasikan), cookie tetap terkirim tanpa `Secure`.

Bandingkan dengan implementasi TS yang sudah benar — `apps/crm/src/host.ts:55` + `:92`:

```ts
return `${name}=${token}; HttpOnly; SameSite=Strict; Path=/${secure ? '; Secure' : ''}${clear ? '; Max-Age=0' : ''}`
const cookieName = secure ? '__Host-archava_session' : 'archava_local_session'
```

**Dampak.** Satu request `http://` ke host yang sama (mis. redirect downgrade, link http, HSTS belum aktif) mengirim cookie sesi dalam cleartext. `__Host-` juga mencegah cookie di-set oleh subdomain.

**Fix.** Turunkan `secure` dari konfigurasi public origin seperti TS (`new URL(origin).protocol === "https:"`), lalu emit `Secure` + nama `__Host-archava_session`; pertahankan nama polos hanya untuk mode fixture. Terapkan juga pada `expire_session_cookie()`.

**Kelas yang sama (dev-only, bukan produk):** `tools/crm-reference/src/server.ts:50` — `crm_reference=${token}; HttpOnly; SameSite=Strict; Path=/`, tanpa `Secure`. Alat referensi lokal, dicatat untuk kelengkapan.

---

### F-02 — Header keamanan (CSP / HSTS / nosniff / frame-ancestors) tidak dipasang di host Rust, `apps/web`, dan template

| Field      | Nilai                                                                                                                                                                                                                                                            |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Severity   | **P3**                                                                                                                                                                                                                                                           |
| Kategori   | CORS/header — defense in depth untuk XSS                                                                                                                                                                                                                         |
| Lokasi     | `rust/crates/host/src/router.rs:1744-1757` (satu-satunya header yang di-set adalah `Set-Cookie`); `apps/web/src/server/server.ts:239` (hanya `content-type`); `Archava Template/server/app.mjs:18-28` (hanya `content-type`, `cache-control`, `nosniff`, `vary`) |
| Reachable  | Ya — semua response HTML/SSR                                                                                                                                                                                                                                     |
| Confidence | **High**                                                                                                                                                                                                                                                         |

**Bukti.** `rg -i 'content-security-policy|strict-transport|x-content-type-options|referrer-policy'` pada `apps/web/src/server`, `rust/crates/host/src`, `rust/crates/slice`, dan `Archava Template/server` → **0 hit**. Implementasi TS CRM host justru sudah punya (`apps/crm/src/host.ts:687-695`): `nosniff`, `referrer-policy: no-referrer`, `cache-control: no-store`, CSP penuh dengan `frame-ancestors 'none'`, dan HSTS saat `publicOrigin` di-set.

**Dampak.** Tidak ada layer kedua bila suatu saat ada sink HTML yang lolos; tidak ada anti-framing; MIME sniffing terbuka di dua server.

**Fix.** Pindahkan header block dari `apps/crm/src/host.ts:687-695` menjadi helper bersama dan pakai di `apps/web/src/server/server.ts` serta response HTML host Rust. CSP untuk host Rust dapat memakai baseline yang sama (`script-src 'self'`; hydration Leptos butuh `'wasm-unsafe-eval'` bila WASM dipakai — verifikasi saat implementasi).

---

### F-03 — `Math.random()` sebagai fallback session id yang membatasi approval/handoff antar-visitor

| Field      | Nilai                                                                                  |
| ---------- | -------------------------------------------------------------------------------------- |
| Severity   | **P3**                                                                                 |
| Kategori   | Crypto — randomness                                                                    |
| Lokasi     | `apps/web/src/page.ts:207-213` (fallback), dipakai di `:227`                           |
| Reachable  | Hanya bila `globalThis.crypto.randomUUID` tidak tersedia (browser lama / harness test) |
| Confidence | **High** untuk kode; **Low** untuk dampak praktis                                      |

**Bukti**

```ts
function newSessionId(): string {
  const generator = globalThis.crypto
  if (generator !== undefined && typeof generator.randomUUID === 'function')
    return generator.randomUUID()
  return `anon-${Math.random().toString(36).slice(2)}`
}
```

Komentar di atas fungsi menjelaskan konsekuensi bila id bertabrakan: dua visitor berbagi approvals dan handoffs ("cross-visitant leak §16 exists to prevent"). `Math.random()` menghasilkan ±10 karakter base36 dengan state PRNG internal ~52 bit (bukan CSPRNG), sehingga dapat diprediksi/di-enumerasi.

**Dampak.** Di browser modern tidak tercapai. Di lingkungan tanpa `crypto.randomUUID`, dua visitor dapat berbagi state otorisasi — persis kegagalan yang ingin dicegah komentar tersebut.

**Fix.** Hapus fallback: `throw` bila `crypto.randomUUID` tidak ada, atau gunakan `crypto.getRandomValues(new Uint8Array(16))` (tetap CSPRNG) dan hex-encode. Jangan pernah `Math.random()` untuk identifier yang menyentuh otorisasi.

---

### F-04 — Helper HTML di frontend demo tidak meng-escape (latent sink, belum ada caller injectable)

| Field      | Nilai                                                                                                     |
| ---------- | --------------------------------------------------------------------------------------------------------- |
| Severity   | **P3**                                                                                                    |
| Kategori   | XSS — latent sink                                                                                         |
| Lokasi     | `apps/web/src/frontend/workspace.ts:128-130` (`button`), `:131-133` (`heading`), `:545-547` (`detailRow`) |
| Reachable  | **Tidak saat ini** — semua caller mengirim literal atau nilai yang sudah `escape()`                       |
| Confidence | **High**                                                                                                  |

**Bukti**

```ts
function detailRow(label: string, value: string): string {
  return `<div class="detail-row"><span>${label}</span><span>${value}</span></div>`
}
```

`detailRow` dipanggil dengan `escape(lead.email)`, `escape(order.customer)`, dsb. (mis. `:554`, `:569`, `:581`) dan dengan literal (`:602`), jadi tidak ada injeksi hari ini. Tetapi kontrak fungsinya "value adalah HTML", sehingga satu caller baru yang meneruskan `lead.name` mentah langsung menjadi stored XSS (data lead berasal dari form `lead-form`, `:828-862`, dan dari `localStorage`).

`heading`/`button` menerima `description`/`label` mentah; `:221` mengirim `escape(industries[industry].name)` — aman karena pre-escaped, bukan karena helper-nya aman.

**Dampak.** Nol saat ini; risiko regresi jangka panjang pada satu-satunya area frontend yang benar-benar menyusun string HTML.

**Fix.** Ubah ketiga helper agar meng-escape parameter kecuali yang eksplisit `Html` (branded type), atau ganti ke `textContent`/`createElement` seperti `packages/*/src/dashboard.ts`. Tambahkan test yang memanggil `detailRow('<img onerror=...>')` dan menuntut output ter-escape.

---

### F-05 — `Math.random()` pada identifier yang dikirim/di-idempotensikan

| Field      | Nilai                                                                                                                                      |
| ---------- | ------------------------------------------------------------------------------------------------------------------------------------------ |
| Severity   | **P3**                                                                                                                                     |
| Kategori   | Crypto — randomness pada identifier non-auth                                                                                               |
| Lokasi     | `packages/subscriptions/src/dashboard.ts:393` (idempotency key), `packages/attendance/src/dashboard.ts:386` dan `:405` (ticket/badge code) |
| Reachable  | Ya — operator memakai dashboard modul; nilai dikirim ke command API                                                                        |
| Confidence | **High**                                                                                                                                   |

**Bukti**

```ts
idempotencyKey: `dash-use-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`
regTicket.value = `TCK-${Math.floor(100000 + Math.random() * 900000)}`
```

**Dampak.** Bukan token otorisasi, tetapi:

- **Idempotency key** yang bertabrakan membuat command kedua mengembalikan receipt command pertama (`existingCmd.receipt`), yaitu receipt untuk record yang salah. `Math.random()` 5 karakter + `Date.now()` membuat tabrakan antar-tab/retry lebih mungkin daripada UUID.
- **Ticket/badge code** adalah field yang disubmit (`ticketCode`, `packages/attendance/src/contracts.ts:98`) dan disimpan sebagai bukti check-in/registrasi. 6 digit dari PRNG non-kripto mudah ditebak/di-enumerasi; nilai ini bukan kontrol akses, tapi melemahkan nilai bukti.

**Fix.** `crypto.randomUUID()` untuk idempotency key; untuk ticket code gunakan `crypto.getRandomValues` dengan panjang cukup (mis. 8 byte → base32) atau biarkan server yang menerbitkan.

---

### F-06 — Password fixture hardcoded dan dicetak di UI runtime

| Field      | Nilai                                                                                                                                                                                                                                                             |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Severity   | **P3**                                                                                                                                                                                                                                                            |
| Kategori   | Secrets — kredensial fixture                                                                                                                                                                                                                                      |
| Lokasi     | `rust/crates/slice/src/login.rs:150` (dicetak di halaman login), `rust/crates/slice/examples/slice_server.rs:61-65` (hash scrypt precomputed), `examples/rust-kit-consumers/kit-template/tests/crm_witness.rs:58`, `rust/crates/host/tests/*.rs` (konstanta test) |
| Reachable  | Ya, bila example server dijalankan (`cargo run --example slice_server`, bind `127.0.0.1`)                                                                                                                                                                         |
| Confidence | **High** (fakta); **Low** (risiko, karena fixture)                                                                                                                                                                                                                |

**Bukti**

```rust
"Fixture accounts: alice, bob, carol, dave — password: correct horse battery staple"
```

**Dampak.** Bukan kredensial produksi — tidak ada user yang di-seed oleh migration produk (`packages/identity-pg/migrations/001_identity.sql` hanya membuat tabel; user hanya dibuat di test/example). Risiko muncul bila seseorang menjalankan example server di host bersama: login terbuka dengan password yang terdokumentasi di UI.

**Fix.** Gate pencetakan cast di belakang flag eksplisit (`ARCHAVA_FIXTURE=1` / `--fixture`) dan tolak start tanpa flag tersebut; pertahankan hash hanya di example/test.

---

### F-07 — Response login mengembalikan token sesi mentah di body meski jalur browser memakai cookie HttpOnly

| Field      | Nilai                                                                                                 |
| ---------- | ----------------------------------------------------------------------------------------------------- |
| Severity   | **P3**                                                                                                |
| Kategori   | Secrets / session handling                                                                            |
| Lokasi     | `rust/crates/host/src/router.rs:1759` (`token: Some(token)`), komentar klaim HttpOnly di `:1746-1749` |
| Reachable  | Ya — setiap `POST /login`                                                                             |
| Confidence | **High** (fakta kode); **Low** (dampak)                                                               |

**Bukti**

```rust
// The browser path holds the token in an HttpOnly cookie; the CSRF
// value stays in the body, where the slice's own script reads and
// echoes it as the mutation header.
...
Json(LoginOutcome { token: Some(token), csrf: Some(csrf), .. })
```

Klien slice hanya membaca `csrf` (`rust/crates/slice/src/login.rs:59-65`), jadi `token` di body tidak dibutuhkan oleh browser. Token itu ada untuk klien bearer (test/CLI).

**Dampak.** Klaim "HttpOnly keeps the token out of script reach" tidak berlaku pada response login: XSS yang aktif saat login dapat membaca token dan memakainya di luar browser (selama sesi hidup; logout menghapus row). Dampaknya kecil karena XSS yang sama sudah bisa melakukan aksi atas nama user, tetapi ini melemahkan alasan memakai cookie HttpOnly.

**Fix.** Kembalikan `token` hanya untuk pemanggil non-browser (mis. `Accept: application/json` tanpa cookie flow, atau header `x-archava-client: cli`), atau pisahkan endpoint login browser (form post → hanya `Set-Cookie`) dari login API.

---

## 2. Ringkasan tabel

| ID   | Sev | Kategori               | Lokasi                                                                                              | Reachable dari input eksternal  | Confidence  | Fix singkat                                    |
| ---- | --- | ---------------------- | --------------------------------------------------------------------------------------------------- | ------------------------------- | ----------- | ---------------------------------------------- |
| F-01 | P3  | Cookie flags           | `rust/crates/host/src/router.rs:1751,1879`                                                          | Ya (browser via HTTP downgrade) | High        | Turunkan `Secure` dari origin; pakai `__Host-` |
| F-02 | P3  | Security headers       | `router.rs:1744-1757`, `apps/web/src/server/server.ts:239`, `Archava Template/server/app.mjs:18-28` | Ya (semua response)             | High        | CSP/HSTS/nosniff/frame-ancestors bersama       |
| F-03 | P3  | Randomness             | `apps/web/src/page.ts:207-213`                                                                      | Hanya tanpa `crypto.randomUUID` | High (kode) | Hapus fallback `Math.random`                   |
| F-04 | P3  | XSS latent sink        | `apps/web/src/frontend/workspace.ts:128-133,545-547`                                                | Tidak (semua caller aman)       | High        | Escape-by-default / branded `Html`             |
| F-05 | P3  | Randomness             | `packages/subscriptions/src/dashboard.ts:393`, `packages/attendance/src/dashboard.ts:386,405`       | Ya (operator dashboard)         | High        | `crypto.randomUUID()`                          |
| F-06 | P3  | Fixture secret         | `rust/crates/slice/src/login.rs:150`, `slice_server.rs:61-65`                                       | Ya bila example dijalankan      | High        | Gate di belakang flag fixture                  |
| F-07 | P3  | Session token exposure | `rust/crates/host/src/router.rs:1759`                                                               | Ya (`POST /login`)              | High (kode) | Token di body hanya untuk klien non-browser    |

---

## 3. Yang diperiksa dan dinyatakan aman (dengan bukti)

### 3.1 SQL injection

| Bukti                                                   | Lokasi                                                                                                                                                                                                                                                | Kenapa aman                                                                                                                                     |
| ------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| SQL dinamis Rust                                        | `rust/crates/host/src/crm.rs:2466-2474`                                                                                                                                                                                                               | `table` dijaga `debug_assert!` **dan** `matches!(table, "followups" \| "opportunities" \| "activities")` sebelum `format!`; nilai tetap `$1/$2` |
| SQL dinamis Rust                                        | `rust/crates/host/src/crm.rs:928` (`format!("{sql} FOR UPDATE")`)                                                                                                                                                                                     | `{sql}` adalah literal konstan di fungsi yang sama; hanya suffix tetap                                                                          |
| Builder filter Rust                                     | `rust/crates/adapters/src/erp/attendance.rs:862-892`                                                                                                                                                                                                  | Kondisi `format!("status = ${}", params.len())` — yang diinterpolasi adalah **index placeholder**, nilai tetap di-bind                          |
| Builder filter TS                                       | `packages/capacity-pg/src/repository.ts:355-381`                                                                                                                                                                                                      | `sql += ' AND state=$' + pIdx++` — pola sama, nilai masuk `params[]`                                                                            |
| `LIMIT ${limitParam}`                                   | `packages/formal-education-pg/src/repository.ts:667-674`                                                                                                                                                                                              | `limitParam = '$' + params.length` (string placeholder, bukan angka user)                                                                       |
| `${role}` di GRANT                                      | 24 fungsi `grant*Runtime` (mis. `packages/crm-pg/src/migrate.ts:65`, `packages/scheduling-pg/src/occupancy.ts:59`, `packages/act-host-pg/src/migration.ts:23`, `packages/provider-runtime/src/postgres.ts:56`, `packages/crm-act/src/postgres.ts:71`) | Semua memakai `/^[a-z][a-z0-9_]{0,62}$/`; **0 fungsi tanpa guard** (diverifikasi loop atas 24 file)                                             |
| `archava_${domain}`                                     | `packages/foodservice-pg/src/repository.ts:214`                                                                                                                                                                                                       | `domain: 'foodservice' \| 'commerce'` — union type, bukan string bebas                                                                          |
| `archava_crm.${table}`                                  | `packages/crm-pg/src/repository.ts:311`                                                                                                                                                                                                               | `table = receipt.kind === 'activity' ? 'activities' : 'followups'` — ternary konstanta                                                          |
| `${table}` CRM/Projects                                 | `apps/crm/src/operator.ts:365`, `packages/projects-pg/src/repository.ts:400`                                                                                                                                                                          | `domain.schema` dari registry statis `apps/crm/src/operator.ts:100-109`; `command.operation` ternary                                            |
| LIKE wildcard                                           | `packages/crm-pg/src/repository.ts:388,535`; `rust/crates/host/src/crm.rs:1155-1160`                                                                                                                                                                  | `\`, `%`, `_` di-escape sebelum bind + `ESCAPE '\\'`                                                                                            |
| `sqlSync(\`... '${id}'\`)`                              | `packages/act-confirmation-pg/scripts/verify-against-postgres.ts:624,641,655,758`                                                                                                                                                                     | Hanya di script verifikasi; `id` = UUID yang dibuat script itu sendiri. Bukan produk                                                            |
| `${mutation.disposition}` / `${String(invoice.status)}` | `packages/fulfillment-pg/src/repository.ts:356`, `packages/billing-pg/src/repository.ts:368,406,487,592`                                                                                                                                              | Bukan SQL — template literal di pesan error/audit; nilai sebenarnya di-bind                                                                     |

Catatan positif: `sqlx` 0.9 mewajibkan `AssertSqlSafe` untuk SQL runtime (54 site), jadi setiap SQL non-literal terlihat dan sudah ditelusuri.

### 3.2 Command injection

- **220** call site `spawn`/`spawnSync`/`execFileSync`/`execFile`/`execSync` di repo; **2** di product source: `apps/crm/src/local-postgres.ts:6,8`. Keduanya `execFileSync` dengan **array arg** dan **tanpa `shell: true`**.
- Satu-satunya titik yang menyentuh shell: `pg_ctl -o "-k <socket> -p <port> ..."` (`apps/crm/src/local-postgres.ts:30-45`). Path di-quote dengan escape `'\''` yang benar, port berasal dari `settings.databasePort` (integer tervalidasi `z.number().int().min(20000).max(45000)`), dan `database`/`home` berasal dari env operator. Tidak ada input request.
- Rust: **6** `Command::new` (`rust/crates/ops/src/backup.rs:23,41`, `rust/crates/pg-fixture/src/lib.rs:375,393,607`, `rust/crates/slice/examples/slice_server.rs:246`) — nama binary dari konstanta/settings, arg array, tanpa shell.
- `rg 'shell:\s*true'` → **0 hit**.

### 3.3 XSS

- **21** pemakaian `innerHTML` (18 di non-test), **0** `dangerouslySetInnerHTML` / `insertAdjacentHTML` / `outerHTML` / `document.write`.
- Semua `innerHTML` berada di tiga kategori:
  1. `apps/web/src/frontend/workspace.ts` — renderer demo; setiap field dinamis lewat `escape()` (mis. `:554`, `:569`, `:581`, `:620`). `heading`/`button`/`detailRow` adalah satu-satunya sink tanpa encoding (F-04).
  2. `apps/web/src/surface.ts:315` — `panel.innerHTML = markup`, dan `markup` dibangun dengan `escapeHtml`/`escapeAttribute` untuk setiap nilai (`:96-101`, `:161`, `:263`).
  3. `apps/crm/test/page-fixture.ts` — harness test.
- Dashboard modul (`packages/*/src/dashboard.ts`) memakai `createElement` + `textContent` (mis. `packages/crm/src/dashboard.ts:17-18`), dan `packages/chat/src/dom.ts:17` mendokumentasikan larangan `innerHTML`.
- Server tidak merender HTML dari data user: `apps/crm/src/host.ts:728-745` hanya memetakan tiga path statis ke file; `apps/web/src/server/server.ts` hanya menyajikan halaman statis. Tidak ada `text/html` response yang menginterpolasi data.
- Leptos: **0** `inner_html`/`RawHtml` di `rust/crates/**`; shell SSR (`rust/crates/slice/examples/slice_server.rs:369-390`) memakai `view!` (auto-escape).

### 3.4 SSRF / open redirect

- `reqwest` **0**, HTTP client Rust **0** (semua crate leaf tanpa network layer).
- `fetch(` **25** di product source: 24 di `apps/crm/src/browser.ts` (transport dashboard, `input` selalu `'/api/<module>/' + tenantId`, same-origin, cookie `same-origin`) dan 1 di `apps/web/src/studio.ts:102` (`fetch('/api/quote')`). Tidak ada URL dari request.
- Field webhook / `callbackUrl` / `redirect_uri` / image proxy: **0 hit**.
- `Location:` header / redirect: **0 hit** — tidak ada open redirect.
- Static file serving (path traversal): tiga handler, semuanya dijaga —
  - `apps/crm/src/host.ts:728-745`: `Map` whitelist (`/`, `/app.js`, `/styles.css`), 404 untuk path lain.
  - `tools/crm-reference/src/server.ts:82-87`: `resolve()` + `startsWith(resolve(publicDirectory) + '/')`.
  - `Archava Template/server/app.mjs:48-54`: `decodeURIComponent` + prefix check dengan `sep`.
- Origin check: `apps/crm/src/host.ts:99` membandingkan `url.origin` dengan origin konfigurasi, dan `url` dibangun dengan **base origin konfigurasi**, bukan header `Host` (`:683`) — Host header injection tidak menggeser pemeriksaan.

### 3.5 Secrets

- `.env` **tidak pernah** masuk git: `git log --all --diff-filter=A --name-only` hanya menemukan `.env.example`, `Archava Template/.env.example`, `Archava Template/.env.client.example`. `.gitignore:31` menutup `.env` dan `.env.*`.
- `.env.example` hanya berisi placeholder kosong (`LIVEKIT_API_SECRET=`, `GOOGLE_API_KEY=`, `POSTMARK_API_KEY=`, dst.).
- Scan bentuk key nyata (`AIza…`, `sk-ant-…`, `xai-…`, `hf_…`, `npm_…`, `glpat-…`) di seluruh tracked file: **0 hit**.
- Private key material: **0** (satu-satunya hit adalah pola scanner di `tools/rust/wasm-boundary.mjs:105`).
- Git history `git log -S`: `sk_live` → 1 hit (canary di test `rust/crates/context/tests/context.rs:123`, string ter-mask), `AKIA` → 0, `BEGIN RSA PRIVATE KEY` → 0.
- 15 kandidat "hardcoded credential" ditelusuri: 14 false positive (marker test, `test-key`, `never-send-this`, kartu test `4111 1111 1111 1111`, `apiKey` fixture) → 1 temuan (F-06).
- Penyimpanan state runtime benar: `apps/crm/src/settings.ts:41-62` menolak direktori non-0700 / file non-0600 / symlink, dan `writePrivateJson` memakai `mode: 0o600, flag: 'wx'`. `Archava Template/scripts/env-profiles.mjs:73-74` menulis `.env` dengan `mode: 0o600` + `chmodSync`.
- Tidak ada `console.log` yang mencetak token/password (`rg` → 0).

### 3.6 Crypto

- `Math.random()`: **8** di source non-test → 4 relevan (F-03: 1, F-05: 3), 4 false positive (port database `operator.ts:191`, `tools/crm-reference/src/cluster.ts:20`, script verifikasi `:361`, work id demo `capacity/src/dashboard.ts:339`).
- `md5`/`sha1`: **0**. Hash yang dipakai: SHA-256 (checksum migration, fingerprint) dan HMAC-SHA256 (session, CSRF, idempotency fingerprint) — tepat untuk pemakaiannya.
- JWT: **0** library (`jsonwebtoken`/`jose`/`jwt` tidak ada di dependency), **0** decode tanpa verifikasi. Sesi memakai token opaque 256-bit, bukan JWT.
- Perbandingan secret non-constant-time: **0**. `timingSafeEqual` di `packages/identity-pg/src/sessions.ts:135` (CSRF) dan `packages/identity-pg/src/password.ts:43` (password), plus `ct_eq` XOR-fold di `rust/crates/host/src/identity.rs:637-645`.
- Password hashing: scrypt `N=32768, r=8, p=3`, `maxmem` 48 MiB, salt 16 byte acak, bounded concurrency 2 (`packages/identity-pg/src/password.ts:15-43`; Rust `rust/crates/host/src/identity.rs:49-67`).
- Token sesi: `randomBytes(32).toString('base64url')` (`packages/identity-pg/src/sessions.ts:104`; Rust `identity.rs` login), dan **hanya digest HMAC** yang disimpan (`token_hash`), sehingga pembaca database tidak bisa memakai token.
- Enkripsi: tidak ada enkripsi aplikasi (tidak ada `createCipheriv`/AES), jadi tidak ada mode non-AEAD untuk dinilai. Tidak ada secret yang dienkripsi di repo.
- Rate limit login: per-source 60/300 s dan per-account 5/900 s, setiap percobaan mengonsumsi budget (`packages/identity-pg/src/sessions.ts:64-86`).

### 3.7 CORS / header / cookie

- `Access-Control-Allow-Origin: *`: **0 hit**.
- Satu-satunya implementasi CORS: `Archava Template/server/app.mjs:18-28` — origin hanya di-echo bila ada di `allowedOrigins` (allowlist dari config), plus `vary: Origin`, dan request dengan Origin tak dikenal ditolak 403 (`:82`). Tidak ada refleksi bebas.
- Host CRM TS: `HttpOnly; SameSite=Strict; Secure` (kondisional) + prefix `__Host-` + CSP + HSTS + `nosniff` + `referrer-policy` (`apps/crm/src/host.ts:55,92,687-695`).
- Host Rust: `HttpOnly; SameSite=Strict` — **tanpa `Secure`** (F-01) dan tanpa header keamanan (F-02).
- CSRF: semua route mutasi memakai `authenticate(..., true)` (`router.rs:1006,1484,1601,1805`); satu-satunya `false` ada di route read-only (`:1308`). Nilai CSRF = `HMAC(key, "csrf\0" + token)`, dibandingkan constant-time, dan tidak pernah disimpan. Di TS, `validCsrf` dipanggil untuk semua POST ber-sesi (`apps/crm/src/host.ts:167`) dan untuk route presence (`:124`).

---

## 4. Hitungan grep (candidate → confirmed → false positive)

| Pattern                                                                     | File yang dicari                                | Kandidat  | Confirmed           | False positive | Catatan                                                                                                         |
| --------------------------------------------------------------------------- | ----------------------------------------------- | --------- | ------------------- | -------------- | --------------------------------------------------------------------------------------------------------------- |
| `sqlx::query*`                                                              | `rust/**/*.rs`                                  | 710       | 0                   | 54             | 54 SQL runtime (`AssertSqlSafe`) ditelusuri satu per satu → 54 FP; 656 literal tidak perlu penelusuran per-site |
| `format!` berisi SQL                                                        | `rust/**/*.rs`                                  | 29        | 0                   | 29             | namespace/table/role/suffix tetap; 1 di test `upgrade_restore.rs:74`                                            |
| SQL template literal dengan `${`                                            | `**/*.ts`                                       | 105 baris | 0                   | 105            | 124 `${role}` di 24 fungsi, semuanya ber-guard regex                                                            |
| `${role}` tanpa guard                                                       | 24 fungsi grant                                 | 0         | 0                   | 0              | Semua `/^[a-z][a-z0-9_]{0,62}$/`; 124 baris `${role}` seluruhnya berasal dari 24 fungsi ini                     |
| `spawn`/`spawnSync`/`execFile*`/`execSync`                                  | seluruh repo                                    | 220       | 0                   | 220            | 2 di product source, 0 `shell:true`, 0 input request                                                            |
| `Command::new`                                                              | `rust/**/*.rs`                                  | 6         | 0                   | 6              | konstanta/settings, arg array                                                                                   |
| `innerHTML`                                                                 | TS/TSX/MJS                                      | 21        | 0                   | 21             | 18 non-test; semua nilai via `escape()`/`escapeHtml`                                                            |
| `dangerouslySetInnerHTML`/`insertAdjacentHTML`/`outerHTML`/`document.write` | seluruh repo                                    | 0         | 0                   | 0              | —                                                                                                               |
| `fetch(`                                                                    | `apps/**`, `packages/**` (non-test)             | 25        | 0                   | 25             | semua path relatif same-origin                                                                                  |
| `reqwest` / HTTP client Rust                                                | `rust/**`                                       | 0         | 0                   | 0              | tidak ada network layer                                                                                         |
| `https?.request/get`, `axios`, `got`                                        | TS/MJS                                          | 0         | 0                   | 0              | —                                                                                                               |
| webhook / callback / redirect_uri / image proxy                             | seluruh repo                                    | 0         | 0                   | 0              | —                                                                                                               |
| `Location:` / redirect handler                                              | seluruh repo                                    | 0         | 0                   | 0              | —                                                                                                               |
| `.env` tracked                                                              | git index                                       | 0         | 0                   | 0              | 3 file `.env.example` (placeholder)                                                                             |
| Bentuk key nyata (`AIza`, `sk-ant`, `xai-`, `hf_`, `npm_`, `glpat`)         | tracked files                                   | 0         | 0                   | 0              | —                                                                                                               |
| Private key material                                                        | tracked files                                   | 1         | 0                   | 1              | hanya pola scanner `wasm-boundary.mjs:105`                                                                      |
| Assignment credential hardcoded                                             | TS/RS                                           | 15        | 1 (F-06)            | 14             | fixture/marker test                                                                                             |
| `git log -S` (`sk_live`, `AKIA`, `BEGIN RSA PRIVATE KEY`)                   | seluruh history                                 | 3 query   | 0                   | 1              | canary ter-mask di test context                                                                                 |
| `Math.random`                                                               | `apps/**`, `packages/**`, `tools/**` (non-test) | 8         | 4 (F-03/F-05)       | 4              | port/demo id                                                                                                    |
| `createHash('md5'\|'sha1')`                                                 | seluruh repo                                    | 0         | 0                   | 0              | —                                                                                                               |
| `jsonwebtoken` / `jose` / `jwt` dependency                                  | `package.json`, `Cargo.toml`                    | 0         | 0                   | 0              | sesi opaque, bukan JWT                                                                                          |
| `===` pada token/secret                                                     | TS non-test                                     | 0         | 0                   | 0              | hanya `event.key === 'Enter'` dsb.                                                                              |
| `Access-Control-Allow-Origin`                                               | seluruh repo                                    | 1         | 0                   | 1              | allowlist, bukan `*`                                                                                            |
| `Set-Cookie` / cookie flags                                                 | TS + Rust                                       | 5 site    | 1 (F-01)            | 4              | TS host & logout benar; 1 dev tool                                                                              |
| Security headers                                                            | 4 entrypoint server                             | 4         | 1 (F-02 mencakup 3) | 1              | hanya CRM host TS yang punya                                                                                    |

**Total: 493 kandidat diperiksa (di luar 710 query literal yang tidak perlu ditelusuri per-site). Hasil: 7 temuan, seluruhnya P3 — 0 P1, 0 P2. False positive setelah ditelusuri: 486 dari 493.**

---

## 5. Batasan audit

- Statis saja (grep + baca kode). Tidak ada request yang dikirim ke host eksternal; tidak ada database yang dijalankan; tidak ada fuzzing.
- `dist/` (build output, gitignored) dan `node_modules/` tidak diaudit isinya; yang diaudit adalah source yang menghasilkannya.
- Analisis `git log -S` dibatasi pada tiga pola ber-signal tinggi, bukan seluruh history (repo besar; `-S` penuh terlalu mahal untuk nilai tambahnya).
- Dependency CVE / supply chain tidak termasuk lingkup ini (lihat audit dependencies terpisah di `docs/audit/2026-10-07-dependencies.json`).
- Severity memakai definisi: P1 = exploitable dari luar tanpa auth dengan dampak langsung (RCE/data lintas tenant), P2 = exploitable dengan auth/konteks terbatas, P3 = hardening / defense-in-depth / membutuhkan prasyarat tidak realistis.

## 6. Rekomendasi urutan perbaikan

1. **F-01** (cookie `Secure`) — satu perubahan kecil, menutup satu-satunya jalur kebocoran sesi cleartext.
2. **F-02** (header keamanan) — ekstrak helper dari `apps/crm/src/host.ts:687-695`, pasang di `apps/web` dan host Rust.
3. **F-03 / F-05** (randomness) — sapu bersih `Math.random` untuk identifier; sisakan hanya untuk port/demo.
4. **F-04** (escape-by-default) — mengunci satu-satunya area yang rawan regresi XSS.
5. **F-06 / F-07** — gate fixture cast dan persempit token di body login.
