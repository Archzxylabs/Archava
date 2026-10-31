# Repository tools

Current helpers use Node built-ins and the root Cargo workspace; no root npm/pnpm install is required.

| Command | Purpose |
| --- | --- |
| `node tools/check-structure.mjs` | Layer boundaries, embedded paths, and active tool imports |
| `node tools/check-docs.mjs` | Local file/heading links in maintained docs; frozen records are excluded |
| `node tools/e2e/verify.mjs --output DIRECTORY` | Real Chromium CRM flow, lost-response recovery, role/tenant/CSRF negatives, and production-process restart on an owned disposable database |
| `node tools/rust/verify.mjs` | Structure, docs links, helper tests, formatting, Clippy, and all Rust tests |
| `node tools/rust/verify.mjs --lib --offline` | Explicitly smaller library-only verification |
| `node tools/rust/build-slice.mjs` | Release server, preview fixture, WASM/bindgen and source/artifact provenance |
| `node tools/rust/package-native.mjs --out NEW_DIRECTORY` | Native server/Ops/site/migrations package; refuses stale inputs and verifies the resulting profile |
| `node tools/rust/dev-slice.mjs run` | Build and launch a disposable PostgreSQL preview |
| `node tools/rust/run-persistent.mjs` | Build and launch against an existing migrated database |
| `node tools/rust/load-benchmark.mjs` | Concurrency & p50/p95/p99 latency benchmark under concurrent loads |
| `node tools/release/sbom.mjs` | Inventory the Cargo dependency graph |

The standalone Rust benchmark is in `tools/rust/benchmark/rust`. Removed TypeScript-era baseline comparisons, package builders, security matrices, and proof producers are recoverable from git history; their evidence is historical and is not replaced by Cargo test success.

The browser gate requires the current release build (`node tools/rust/build-slice.mjs`), installed Chromium and PostgreSQL tools. It creates and closes its own server, database and browser profile. `E2E_BROWSER` selects an installed executable; `E2E_BROWSER_NO_SANDBOX=1` is an explicit fallback for environments that cannot launch Chromium sandboxing. It never attaches to a user browser or shared application database. This gate covers CRM, native Tier 1 operator journeys and read-tool missions, including deliberate lost responses and a production-binary restart. It does not certify live providers or public deployment.
