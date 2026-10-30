# A8 — Hardening host Rust: cookie `__Host-`/`Secure` + CSP/HSTS

**Card:** `t_849628ec` · **Tanggal:** 2026-10-22 · **Repo:** `/home/haikaru/Archverse/Lab/Archava_Platform`
**Branch:** `feat/laundry-1.17.0` · **HEAD:** `412f763` · **Tree:** dirty pra-aksi dipertahankan, nol commit, nol revert.
**Verdict card ini:** **F-01 ✅ dan F-02 ✅ terverifikasi empiris.** Sisa F-03…F-07 diserahkan ke card `t_0d834d2e` (A8-FU).

---

## 1. Item dan status

| Item | Isi                                                                                       | Status di card ini | Bukti                                                                                                                                             |
| ---- | ----------------------------------------------------------------------------------------- | ------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| F-01 | Session cookie: prefix `__Host-` + `Secure` hanya di jalur TLS, logout menghapus dua nama | **DONE**           | `security.rs:35,84,100,119`; `router.rs:1783,1876,1889,1914`; 4 unit test                                                                         |
| F-02 | CSP + HSTS + `nosniff` + `no-referrer` di response host Rust dan server web               | **DONE**           | `security.rs:47,52,144`; `router.rs:561`; `apps/web/src/server/security.ts:33,47,59,83`; 4 unit test (policy, API policy, TLS detect, body token) |
| F-03 | `Math.random()` residual → `crypto.randomUUID()`                                          | Dihandover         | `t_0d834d2e` (`apps/web/src/page.ts` sudah beres)                                                                                                 |
| F-04 | Escape HTML di surface/render                                                             | Dihandover         | `t_0d834d2e` (`surface.ts:44` sudah ada `escapeHtml`)                                                                                             |
| F-05 | `Math.random()` sebagai pemilih port di verify-\*.mjs + CRM operator                      | Dihandover         | `t_0d834d2e`                                                                                                                                      |
| F-06 | Password fixture bocor ke log                                                             | Dihandover         | `t_0d834d2e`                                                                                                                                      |
| F-07 | Token di body login                                                                       | Dihandover         | `t_0d834d2e`                                                                                                                                      |

---

## 2. F-01 — cookie

Kode (`rust/crates/host/src/security.rs`):

- `:35` `pub(crate) const SECURE_SESSION_COOKIE: &str = "__Host-archava_session";`
- `:84` `session_cookie(token, secure)` → nama plain kalau `!secure`, nama `__Host-` + `Secure` kalau `secure`
- `:100` `expired_session_cookies()` → meng-expire **dua** nama (`__Host-` + plain) supaya sesi yang dimint di skema lain tidak hidup melewati logout
- `:119` `cookie_token()` → resolver membaca **kedua** nama, menolak cookie yang cuma berbagi suffix (`not_archava_session`)

Wiring (`rust/crates/host/src/router.rs`):

- `:1783` `session_cookie(&token, is_secure(&headers))`
- `:1876` dan `:1889` `expire_session_cookie()` pada dua jalur logout
- `:1914` `fn expire_session_cookie()` membungkus `expired_session_cookies()`

Unit test: `security.rs:185` (plain HTTP → cookie plain, tanpa `Secure`), `:197` (TLS → `__Host-`, `Secure`, `Path=/`, tanpa `Domain`), `:217` (logout expire dua nama), `:228` (resolver dua nama + penolakan decoy).

## 3. F-02 — header keamanan

Host Rust (`security.rs`):

- `:47` `CONTENT_SECURITY_POLICY = "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'"`
- `:52` `STRICT_TRANSPORT_SECURITY = "max-age=31536000"`
- `:144` `apply(headers, secure)` → `nosniff`, `no-referrer`, `no-store`, CSP, dan HSTS **hanya** kalau `secure`
- `:61` `is_secure()` menerima `Forwarded`/`X-Forwarded-Proto` dan origin TLS

Wiring: `router.rs:561` `.layer(axum::middleware::from_fn(crate::security::security_headers))` — satu layer untuk seluruh router, bukan per-handler.

Sisi web TypeScript: `apps/web/src/server/security.ts` (3.900 B) — `CONTENT_SECURITY_POLICY` (`:33`), `STRICT_TRANSPORT_SECURITY` (`:47`), `securityHeaders()` (`:59`), `applySecurityHeaders()` (`:83`), dengan aturan yang sama: HSTS tidak dikirim ke plain HTTP.

---

## 4. Gate

Dijalankan dari `rust/` (bukan root), `TMPDIR=/tmp`, satu run otoritatif, tanpa worker paralel.

```
TMPDIR=/tmp cargo test --offline --locked -p archava-host --lib -- --test-threads=1
cargo fmt --all --check
cargo clippy --offline --locked -p archava-host --all-targets -- -D warnings
```

Hasil (log: `docs/audit/evidence/a8/gate-lib-fmt-clippy.log`, sha256 `fe42cbb4f845ce157aa9e712d9494916b26cc5717d7023b76cf287cf542253be`):

```
test result: ok. 50 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 110.00s
LIB_EXIT=0
FMT_EXIT=0
CLIPPY_EXIT=0
```

## 5. Negative control (merah-dulu)

Bukan klaim, tapi dibuktikan: konstanta dirusak → test harus merah → file direstore → sha256 harus identik.

**F-01** — `security.rs:35` diubah `__Host-archava_session` → `archava_session_nohost`:

```
RED_EXIT=101
thread 'security::tests::a_tls_session_mints_the_host_prefixed_cookie' panicked at crates/host/src/security.rs:199:9:
the TLS cookie must carry the __Host- prefix: archava_session_nohost=token-value; HttpOnly; Path=/; SameSite=Strict; Secure
test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 49 filtered out
```

Log: `docs/audit/evidence/a8/red-cookie-host-prefix.log`, sha256 `6b6f44a2176927a1584ae3858116bcf4f293bf1185d4ae8f25816686bacf01b3`

**F-02** — `security.rs:147` diubah `"nosniff"` → `"none"`:

```
RED_EXIT=101
thread 'security::tests::every_response_carries_the_shared_policy_and_hsts_only_over_tls' panicked at crates/host/src/security.rs:248:9:
assertion `left == right` failed
  left: "none"
 right: "nosniff"
```

Log: `docs/audit/evidence/a8/red-headers-nosniff.log`, sha256 `b663bff56f82482f00681f7322efa4cb5fd7169836543a00fb4f9dbea9cf97d7`

Restore diverifikasi: `sha256(security.rs)` sebelum dan sesudah kedua mutasi = `3fe89ee1660c2d1a65f0a8bf62991ac84076ca766086ae8a6f56fce3a7ebd3cc` (`RESTORE_OK` dua kali).

Catatan metodologi: percobaan merah pertama **gagal sebagai bukti** — `sed` global ikut mengubah literal assertion di test sehingga test tetap hijau (tautologi). Percobaan diulang dengan mutasi **hanya pada baris konstanta** (`35s`) dan baru itu menghasilkan `RED_EXIT=101`. Ini dicatat supaya tidak ada yang mengira "merah-dulu" bisa dilakukan asal-asalan.

## 6. Keterbatasan yang diakui

1. **Gate card ini `--lib` saja** (`archava-host --lib`), bukan seluruh suite host. Sesuai steering comment #85: hemat kuota dan hindari rebutan `cargo` lock. Suite integrasi host tidak dijalankan di card ini.
2. **Sisi TypeScript belum punya test yang dijalankan di card ini.** `apps/web/src/server/security.ts` sudah ada dan dipakai, tapi pembuktian perilakunya (unit/integration) adalah lingkup `t_0d834d2e`.
3. **F-03…F-07 tidak dikerjakan di sini** — sengaja dipisah supaya card ini bisa ditutup tanpa menunggu pekerjaan TS/web/tools.
4. **Penulis artefak:** kode F-01/F-02 ditulis worker kanban run #129; run 130–133 mati oleh kuota provider (`HTTP 402 entitlement is exhausted`), sehingga gate, negative control, dan dokumen ini dieksekusi orchestrator. Tidak ada perubahan logika produk oleh orchestrator — hanya mutasi sementara yang direstore (sha256 terbukti).
5. **Tidak ada commit.** Tree tetap dirty seperti pra-aksi.

## 7. Reproduksi

```bash
cd rust
TMPDIR=/tmp cargo test --offline --locked -p archava-host --lib -- --test-threads=1
cargo fmt --all --check
cargo clippy --offline --locked -p archava-host --all-targets -- -D warnings

# negative control (mengubah lalu merestore; verifikasi sha256 sebelum/sesudah)
sha256sum crates/host/src/security.rs
sed -i '35s|__Host-archava_session|archava_session_nohost|' crates/host/src/security.rs
TMPDIR=/tmp cargo test -p archava-host --lib a_tls_session_mints_the_host_prefixed_cookie -- --test-threads=1   # harus 101
git checkout -- crates/host/src/security.rs   # atau restore dari backup, lalu cek sha256 identik
```
