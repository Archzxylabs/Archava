# AGENTS.md

Archava is a Rust modular monolith: 22 Cargo workspace members, an Axum/PostgreSQL host, and a Leptos UI rendered on the server and hydrated with WebAssembly. The production binary is `crates/slice/src/bin/archava_server.rs`, the HTTP/authority surface is `crates/host/src/router.rs` (with modular route trees under `crates/host/src/routes/`), and `crates/slice/src/server.rs` joins the host router to the SSR shell. Read `docs/architecture/README.md` before moving code across layers.

## Commands

- `node tools/rust/verify.mjs` — the real gate: structure, docs links, helper tests, `cargo fmt --check`, workspace Clippy with `-D warnings`, then all tests with `--test-threads=1`.
- `node tools/rust/verify.mjs --lib --offline --wasm` — fast complete gate (structure, docs, tool tests, fmt, Clippy, WASM boundary, and library tests). Add `--plan` to inspect commands.
- `node tools/check-structure.mjs` — run after changing dependencies, migrations, or repository tooling. CI enforces it.
- `node tools/check-docs.mjs` — link gate for maintained Markdown.
- One suite or test: `cargo test -p archava-host --test crm the_contact_list_reads_real_rows_for_a_granted_reader --locked -- --test-threads=1`.
- `node tools/rust/build-slice.mjs` — release server, fixture preview, WASM/bindgen payload, and provenance manifest.
- `node tools/rust/dev-slice.mjs run` — build and launch a disposable-PostgreSQL preview at `/app`; `SLICE_PORT` overrides the port and `ARCHAVA_SLICE_PIDFILE` isolates its pidfile.
- `node tools/rust/run-persistent.mjs` — build and launch against an existing migrated database; requires `DATABASE_URL` and `ARCHAVA_IDENTITY_KEY_HEX`.
- `node tools/e2e/verify.mjs --output target/e2e/latest` — real-Chromium gate; run only after `build-slice.mjs`.
- `node tools/rust/package-native.mjs --out dist/<new-name>` — packages the native distribution; refuses an existing output directory or stale build artifacts.

Toolchain is pinned: Rust 1.96.0 (`rust-toolchain.toml`), Node 22.22.3 (`.tool-versions`), the `wasm32-unknown-unknown` target, and **wasm-bindgen-cli 0.2.129**. `build-slice.mjs` looks for `$WASM_BINDGEN` or `~/.cargo/bin/wasm-bindgen`; a different CLI version is a build failure.

`.cargo/config.toml` forces `sccache` and `clang` with `mold`. If `sccache` is unavailable, prefix Cargo/helper commands with `RUSTC_WRAPPER=`; install the configured linker or supply a target override. Do not read a wrapper or linker failure as a test failure. Release builds use LTO, `codegen-units = 1`, and `panic = "abort"`, so use `cargo check` or `--lib` while iterating.

`Cargo.lock` is committed and the gate runs with `--locked`; adding a dependency requires updating the lock first. Declare shared dependencies in `[workspace.dependencies]` and consume them as `workspace = true`.

## Boundaries that are enforced

`tools/check-structure.mjs` fails on these, so don't discover them by trial:

- `core` has no internal production dependency; `domain` → `core`; `adapters` → `core` + `domain`; `context` → `core`; `ops` → `assembly`.
- `archava-pg-fixture` is a development dependency only; it must never enter a production dependency graph.
- `archava-slice` may depend on `archava-host` only under `cfg(not(target_arch = "wasm32"))`.
- Workspace members must live under `crates/`; SQL files may live only under `migrations/`.
- `include_bytes!` and `include_str!` paths must exist.
- `tools/*.mjs` must not import the removed `@archava/*` TypeScript workspace.

Pass `--wasm` to `verify.mjs` to include the WASM boundary gate, or run the checks directly when touching `core`, `domain`, or `slice`:

```bash
cargo check --target wasm32-unknown-unknown -p archava-core -p archava-domain
cargo check --target wasm32-unknown-unknown -p archava-slice --lib --features hydrate
```

`crates/host/src/extractors.rs` reads `router.rs` as a string at compile time and asserts that no `Path<`, `Form<`, or bare `Query<` extractor is wired. A new query extractor must be `Result<Query<T>, QueryRejection>` and map through `query_rejection()`; adding the bare form fails a test whose message contains the fix.

`sqlx` is intentionally built without `macros`: do not add `sqlx::query!`/`query_as!` or any build-time database connection. Use runtime `sqlx::query` and explicit transactions. Resolve actor, tenant, grant, and role from the session and database, never from request-body claims. All CRM and ERP persistence is unified inside `crates/adapters` under atomic `FORCE RLS` transactions (ADR-014, ADR-018). HTTP routes are modularized in `crates/host/src/routes/` (ADR-015), and assistant turn flow is in `crates/host/src/assistant/` (ADR-017).

A crate or migration existing does not mean the host mounts it. Check `crates/host/src/router.rs` and the ledger constants in `crates/slice/src/bin/archava_server.rs` for the actual surface.

## Migrations and embedded assets

Applied SQL is append-only: never edit its bytes, rename migration IDs, or rename schemas to tidy a directory. A new migration for a mounted ledger needs more than the `.sql` file:

1. `migrations/<ledger>/NNN_name.sql`
2. its SHA-256 entry in `migrations/manifest.v1.json` (the Ops installer verifies this)
3. `include_bytes!` plus ledger registration in `crates/slice/src/bin/archava_server.rs`
4. the corresponding list in `crates/pg-fixture/src/lib.rs` when the fixture applies that ledger

The server embeds migrations, `static/archava.js`, `static/archava.css`, `static/workspace.css`, and the fonts, and refuses to serve when the database history differs from the embedded profile. Rebuild the native server after changing SQL or an embedded asset; rebuild the WASM bundle too after changing view markup. `build-slice.mjs` rebuilds both.

Apply migrations with the owner role and serve with a non-owner role. The runtime refuses an owner, superuser, or `BYPASSRLS` database URL at startup.

## Tests

- Integration tests need PostgreSQL server binaries (`initdb`, `pg_ctl`) on `PATH`; the browser gate also needs `psql`. Each PostgreSQL-backed integration test provisions its own disposable cluster in a temp directory with synthetic roles. Missing tools fail loudly; they never skip.
- Never point tests at an application or shared database. There is no supported `ARCHAVA_TEST_DATABASE_URL` path.
- Keep `--test-threads=1`. The KDF budget is process-wide and integration fixtures are expensive; parallel runs produce false failures. ERP suites can take minutes each, so use `-p`/`--test` for focused work.
- `--lib` skips integration tests and does not certify release readiness.
- The browser gate requires a fresh release build, installed Chromium, and PostgreSQL. It owns its browser profile and database. `E2E_BROWSER` selects an installed browser; `E2E_BROWSER_NO_SANDBOX=1` is the explicit no-sandbox fallback. It never attaches to a user browser or shared database.

## Runtime and repository conventions

- Use `.env.example` (local dev) or `deploy/archava.env.example` (production/systemd) for the Rust runtime environment template.
- `DATABASE_URL` must name the migrated non-owner runtime role. `ARCHAVA_IDENTITY_KEY_HEX` must contain at least 32 hex-encoded bytes. Startup also verifies mounted ledger checksums.
- There is no root npm/pnpm install. `tools/` helpers use Node built-ins only.
- The TypeScript-era tools and consumers are removed from the tree; recover a specific file from git history if an old comparison is needed, and don't wire it back into the current gate.
- `docs/` is maintained; `docs/archive/` is frozen evidence. Don't rewrite dated pass/fail records. Keep source references current and put new module specifications under `docs/modules/`.
- `.local/`, `target/`, and `dist/` are local state/build output; don't delete another checkout's state or stage these paths.
- `config/` holds shared catalogues with named consumers; keep crate-specific fixtures under that crate's `tests/fixtures/`.

Start with `docs/DEVELOPER_GUIDE.md`, `docs/architecture/README.md`, `CONTRIBUTING.md`, `deploy/README.md`, and `migrations/README.md` for detail.
