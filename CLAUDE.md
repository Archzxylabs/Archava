# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

`AGENTS.md` is the maintained agent guide (commands, enforced boundaries, migration registration, test rules, repo conventions). It is imported here rather than duplicated — update it there, not in this file.

@AGENTS.md

## Command quick reference

```bash
node tools/rust/verify.mjs --lib --offline --wasm   # fast gate while iterating
node tools/rust/verify.mjs                          # full gate (serial integration tests, slow)
node tools/rust/verify.mjs --plan                   # print the commands without running them

# one integration suite / one test (always --locked and --test-threads=1)
cargo test -p archava-host --test erp_billing --locked -- --test-threads=1
cargo test -p archava-host --test crm <test_name> --locked -- --test-threads=1

cargo check -p archava-host                         # cheapest feedback; release builds use LTO and are slow
node tools/rust/domain-assets.mjs --write           # after editing config/{modules,operator-actions,accounting-currencies}.v1.json
node tools/check-modules.mjs --explain laundry      # registration points for one business module
node tools/rust/dev-slice.mjs run                   # disposable-PostgreSQL preview at /app
```

If a Cargo command dies in `sccache`/`clang`/`mold` before compiling, that is the wrapper/linker from `.cargo/config.toml`, not the code: retry with `RUSTC_WRAPPER=` prefixed.

`.config/nextest.toml` defines a `serial-pg` group for the PostgreSQL-backed packages, but the gate (`verify.mjs`, CI) uses plain `cargo test`.

## Big picture

Modular monolith, one binary. Dependency direction is one-way and machine-checked by `tools/check-structure.mjs`:

```
slice (Leptos SSR + WASM hydrate, server bootstrap)
  └─ host (Axum router, session, authority, agent/assistant flow)      ← native target only
       └─ adapters (all SQL: repositories + transactions, FORCE RLS)
            └─ domain (pure business rules, compiles to wasm32)
                 └─ core (contracts, tenant types; no internal deps)
```

Side crates: `pricing`, `rates`, `assembly`, `capacity`, `assistant`, `own`, `context` are portable rule/planner crates; `providers` is the bounded model transport; `cockpit` (mission/flight state) and `orbz` (memory) are composed by the host; `ops` → `assembly` is the operator CLI (migrations, install, restore); `mcp`, `campaigns`, `rpa` have their own boundaries and are not all mounted; `pg-fixture` is dev-only.

How one feature spans the layers (e.g. an ERP domain such as billing):

| Concern | Location |
| --- | --- |
| Invariants / state machine | `crates/domain/src/erp/<domain>.rs` |
| SQL + tenant-bound transaction | `crates/adapters/src/erp/<domain>.rs` |
| HTTP handler, authority checks | `crates/host/src/erp/<domain>.rs`, wired in `crates/host/src/routes/` |
| UI | `crates/slice/src/operations/`, `static/workspace.css` |
| Declaration | one descriptor in `config/modules.v1.json` (the module registry) |
| Schema | `migrations/<domain>-pg/NNN_*.sql` + its entry in `migrations/manifest.v1.json` (see `AGENTS.md`) |
| Integration test | `crates/host/tests/erp_<domain>.rs` (owns a disposable PostgreSQL cluster) |
| Spec | `docs/modules/<DOMAIN>_MODULE.md` |

Things that are not obvious from any single file:

- **Module registry.** `config/modules.v1.json` declares each business module once; agent routing keywords, the provider-readable collections and the product catalogue read it, and `node tools/check-modules.mjs` checks every Rust registration point against it. When adding or moving a module, run `node tools/check-modules.mjs --explain <id>` — it lists what is missing and where. `docs/architecture/MODULE_REGISTRY.md` has the procedure and the per-module tables that are still hand-maintained.

- **Mounted ≠ present.** The actual surface is `crates/host/src/router.rs` + `routes/` and the ledgers in `migrations/manifest.v1.json`, which the server embeds and verifies. Seven SQL files are deliberately unmounted and allowlisted in `check-structure.mjs`.
- **Two roles, two phases.** `archava_owner` applies migrations (via `archava-ops`); the server runs as non-owner `archava_runtime`, sets a transaction-local tenant, and refuses to boot on an owner/superuser/`BYPASSRLS` URL or when DB migration history differs from the embedded checksums. RLS gives tenant isolation only — grants/scope/role are still checked in the host from session + database, never from request bodies.
- **Write contracts are part of the boundary:** command receipts, idempotency keys, stale-revision (optimistic concurrency) checks, uncertain-outcome recovery, and writer fencing (`docs/architecture/WRITE_FENCING.md`). Preserve them when touching adapters or handlers.
- **Embedding.** The native server `include_bytes!`s migrations, `static/archava.{js,css}`, `static/workspace.css`, and fonts. SQL/asset change → rebuild native; view markup change → rebuild native *and* WASM (`build-slice.mjs` does both). Changing an embedded asset also invalidates native package provenance.
- **Migrations have one list.** The server's embedded migrations are generated from `migrations/manifest.v1.json` by `crates/slice/build.rs`, and the fixture reads the same manifest. Only a new ledger needs code: the fixture must apply it and grant the runtime role.
- **Agent runtime ("Ava").** Not a crate: goal → review → execution → receipt-backed report lives in `crates/host/src/agent/` and `crates/host/src/assistant/`, using `assistant`/`context` for planning, `providers` for model calls, `cockpit` for durable missions, `orbz` for memory. Vocabulary is defined in `docs/GLOSSARY.md`; see `docs/modules/AGENT_RUNTIME.md`.

## Working in this checkout

- The working tree is usually dirty with in-progress work across many crates. Don't revert, reformat, or "clean up" files unrelated to the task, and don't stage `target/`, `dist/`, or `.local/`.
- `docs/archive/` is frozen evidence; dated pass/fail records and old counts there are not current results. Maintained docs quote counts that nothing verifies (crates in `README.md`/`AGENTS.md`/`docs/README.md`; migrations and ledgers in `docs/GLOSSARY.md`, `docs/architecture/README.md`, `migrations/README.md`) — update them by hand when the underlying number changes, and run `node tools/check-docs.mjs` after touching Markdown links.
- Report which gate scope actually ran (`--lib` vs full vs browser); a library pass does not certify integration or release readiness.
