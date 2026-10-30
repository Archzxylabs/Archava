# Developer guide

## Requirements

- Rust **1.96.0**, pinned by the root `rust-toolchain.toml`, with Rustfmt and Clippy.
- PostgreSQL server/client tools on `PATH` for integration tests.
- Node **22.22.3** from `.tool-versions` for the small repository helpers. No root pnpm installation is required.
- The configured Linux linker is `clang` with `mold`; `.cargo/config.toml` also selects `sccache` as the compiler wrapper.
- The browser build needs `wasm32-unknown-unknown` and **wasm-bindgen-cli 0.2.129**.

If `sccache` is unavailable, prefix Cargo or helper commands with `RUSTC_WRAPPER=`. Install the configured linker or supply a suitable Cargo target override; do not treat a cache/linker failure as a test failure.

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
```

## Read the tree

[Architecture](architecture/README.md) describes layer boundaries. [The documentation index](README.md) distinguishes current instructions from specifications, proposals, and history. `crates/`, `migrations/`, `static/`, and the root Cargo files are the current product. The removed TypeScript-era tools and consumers are recoverable from git history.

## Verify

```bash
node tools/rust/verify.mjs
node tools/rust/verify.mjs --offline --lib
node tools/rust/verify.mjs --plan
```

The default checks structure, maintained documentation links, the helper regression tests, formatting, workspace Clippy with warnings denied, and all Rust tests with serial integration execution. `--lib` runs only library tests. `--plan` prints commands without running them. The helper stops on the first failed or interrupted child.

The underlying commands are also available directly:

```bash
node tools/check-structure.mjs
node tools/check-docs.mjs
node --test tools/test/structure.test.mjs tools/test/docs.test.mjs tools/test/verify.test.mjs
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked -- --test-threads=1
```

Use PostgreSQL tooling for full integration coverage. Tests provision owned disposable fixtures; never repoint them at a shared application database. Test counts vary with the source and scope, so record the current output rather than copying a count from an old report.

## Build the application

```bash
node tools/rust/build-slice.mjs
```

Outputs are `target/release/archava-server`, `target/release/examples/slice_server`, and `target/site/pkg/{archava-slice.js,archava-slice_bg.wasm}`. The helper builds the root workspace. The default release build includes LTO and can take time; no fixed incremental-build latency is promised.

## Run

For a disposable preview:

```bash
node tools/rust/dev-slice.mjs run
```

For a persistent database, follow [deployment setup](../deploy/README.md), use a non-owner runtime database role, set the environment in [deploy/archava.env.example](../deploy/archava.env.example), and run:

```bash
node tools/rust/run-persistent.mjs
```

Both launchers build before serving. They support `--help` without starting a service. The preview fixture's `stop` command uses its own pidfile; preserve unrelated servers when working on a shared checkout.

Start at `/app` to sign in, `/app/home` for the overview, and `/app/contacts` for the CRM workflow. Operations at `/app/erp` is the native Tier 1 workspace for catalog/orders, purchasing, invoices, support and read-tool missions. The release tracker records remaining costing/provider work. The server embeds the shared stylesheet and migrations, so rebuild the native server after changing those files; rebuild WASM after changing view markup.

## Browser acceptance

After building the release artifacts:

```bash
node tools/e2e/verify.mjs --output target/e2e/latest
```

This gate owns a fresh Chromium profile and disposable PostgreSQL fixture. It exercises CRM UI writes, receipt recovery and authorization negatives, then restarts the production executable on the same database. It does not use a shared database or a user-started browser. See [tool ownership](../tools/README.md) for installed-browser configuration. Preview processes can use `ARCHAVA_SLICE_PIDFILE` to isolate their pidfile; the gate sets a unique path.

## Supporting tools

```bash
node tools/release/sbom.mjs --offline --output dist/sbom.cdx.json
CARGO_TARGET_DIR=target/benchmark cargo check --manifest-path tools/rust/benchmark/rust/Cargo.toml --locked
cargo run -p archava-ops -- help
```

The SBOM inventories the resolved Cargo graph, including development and target dependencies; it is not a vulnerability scan. The standalone benchmark crate uses the current pricing/rates crates. The explicit target directory avoids reusing an old, potentially unwritable nested build cache. Its old TypeScript comparison producer is archived, so an old comparison result is not a fresh performance claim.

## Change ownership

Business policy → `core` / `domain`; persistence → `adapters` and `migrations`; API and authority → `host`; UI → `slice` and `static/workspace.css`; operator workflows → `ops`; shared release configuration → `config`. Keep crate-owned regression data under that crate’s `tests/fixtures`; historical configuration belongs under `docs/archive/history/config`.

Use [the contribution rules](../CONTRIBUTING.md). Do not rename migration IDs or namespaces for cosmetic consistency. Keep local database state in `.local` and generated artifacts in ignored `target`/`dist` paths.

## Native distribution and installation

After a fresh serving build, run `node tools/rust/package-native.mjs --out dist/native-candidate-001` with a new output path. The packager refuses stale source/output hashes, builds the Ops CLI, writes the native profile and recomputes it with the shipped verifier. The [native procedure](../deploy/NATIVE_INSTALL.md) covers explicit owner-role migration, first-owner bootstrap, runtime launch and encrypted isolated restore. Local rehearsal tests verify login after both fresh install and restore; production data size and live providers need separate evidence.
