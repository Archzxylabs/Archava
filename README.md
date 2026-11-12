# Archava

Archava is agentic AI for business: customers describe an outcome, Ava prepares a
plan from authorized business context, humans review business changes, and the
agent executes through existing business tools and reports verified results.

The implementation is a Rust modular monolith with an Axum HTTP host,
PostgreSQL persistence and a Leptos server-rendered/WebAssembly UI. Its 18 business
domains provide native action forms and scoped reads. The [agent runtime](docs/modules/AGENT_RUNTIME.md)
connects durable conversations, preferences, reviewed goals, mission execution,
continuation and receipt-backed reports. General model-backed planning requires
a configured provider; the local fallback has explicit limits. Build/test and
owned-fixture results do not certify live-provider quality or public deployment.

## Start here

- [Developer setup and commands](docs/DEVELOPER_GUIDE.md)
- [Current architecture and ownership](docs/architecture/README.md)
- [Documentation index](docs/README.md)
- [Verification records](docs/README.md#verification-and-document-ownership)
- [Deployment](deploy/README.md)
- [Workspace design](docs/product/design/WORKSPACE_APPEARANCE.md)
- [Contributing](CONTRIBUTING.md) and [security reporting](SECURITY.md)

## Repository map

| Path | Purpose |
| --- | --- |
| `crates/` | The 22 Cargo workspace members: contracts, business engines, adapters, host, UI, and operator tools |
| `migrations/` | Canonical SQL ledgers, including the owner-configuration ledger |
| `static/` | Served browser assets, embed script, stylesheet, and licensed fonts |
| `config/` | Shared release/tooling catalogue; crate-owned fixtures and historical data live elsewhere |
| `deploy/` | Reference runtime packaging, service, proxy, and PostgreSQL role setup |
| `tools/` | Current checks, builds, launchers, and dependency inventory |
| `docs/` | Current entry documents, specifications, plans, research, and dated evidence |
| `target/`, `.local/` | Ignored generated artifacts and local state; outside the source architecture |

## Verify

Use the compiler pinned by [rust-toolchain.toml](rust-toolchain.toml). Node is only needed for repository helpers, not for the Rust libraries.

```bash
node tools/rust/verify.mjs             # structure, tool tests, fmt, Clippy, all Rust tests
node tools/rust/verify.mjs --lib       # smaller library-only scope
```

Add `--offline` when the Rust dependencies are already cached. Integration tests need PostgreSQL tools and provision disposable databases; they do not use your application database. See [the developer guide](docs/DEVELOPER_GUIDE.md) for direct Cargo commands and troubleshooting.

## Build and run

```bash
node tools/rust/build-slice.mjs
```

This builds `target/release/archava-server`, the fixture preview example, and `target/site/pkg`. Install the pinned `wasm-bindgen-cli` version listed in the developer guide before building the browser bundle.

For an existing migrated database, set `DATABASE_URL` to a non-owner runtime role and configure `ARCHAVA_IDENTITY_KEY_HEX` using [deploy/archava.env.example](deploy/archava.env.example), then run:

```bash
node tools/rust/run-persistent.mjs
```

For an isolated preview instead:

```bash
node tools/rust/dev-slice.mjs run
```

The preview uses disposable fixture accounts and data. Start at `/app`, then use Home or Contacts. See [deployment instructions](deploy/README.md) before operating a persistent database.

## Historical material

The former platform overview, architecture blueprint, and build log are preserved in [docs/archive/history/platform-2026-10-28](docs/archive/history/platform-2026-10-28/). Historical test counts, hashes, release profiles, and path names describe their original snapshots. They are not current verification results. The [cleanup report](docs/archive/audits/ARCHAVA_REPOSITORY_CLEANUP_2026-10-29.md) records relocations, checks, and remaining architectural work.
