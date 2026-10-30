# Contributing

The root Cargo workspace is the current product. Start with [the developer guide](docs/DEVELOPER_GUIDE.md), [architecture](docs/architecture/README.md), and [security reporting](SECURITY.md).

## Setup and checks

Use the pinned Rust toolchain in `rust-toolchain.toml`. Repository helpers use the Node version in `.tool-versions`. There is no root pnpm workspace or root npm installation step. PostgreSQL tools are required for integration tests.

```bash
node tools/rust/verify.mjs
```

Use `--lib` for library-only checks or `--offline` with cached dependencies; neither changes what the full gate means. Browser and deployment verification are additional scopes, not consequences of a passing library suite.

## Boundaries

- Keep pure business rules in `crates/domain` and contracts in `crates/core`. Infrastructure depends on these layers; they must not import the host or database adapters.
- Keep database transactions and repositories in `crates/adapters`, HTTP/session handling in `crates/host`, and product presentation in `crates/slice`.
- `archava-pg-fixture` is a development dependency. Never introduce it into the production dependency graph.
- Keep all SQL migrations in `migrations/<ledger>/`. Applied SQL is append-only: moving a file does not authorize changing its bytes or checksum.
- Resolve actor, tenant, grant, and role from the authenticated session and database. Caller-supplied authority is not trusted.
- Preserve tenant predicates, forced RLS under a non-owner role, bounded work, optimistic concurrency, idempotency, and uncertain-outcome recovery.
- Keep secrets, runtime databases, and backups out of Git. Do not remove another developer's `.local` state or clear shared build caches as part of source cleanup.
- Keep historical tools and evidence labelled. Do not use an archived passing result as a current release verdict.

Run `node tools/check-structure.mjs` when changing dependencies, migration paths, or repository tooling. CI enforces the same structural rules.

## Adding a module

Define the domain invariant, transactional adapter, authorized HTTP contract, and UI behavior separately. Add migrations with the corresponding release/readiness registration, positive and negative integration coverage, and a module specification under `docs/modules/`. A usable end-to-end flow needs its own proof beyond domain tests.

## Reviews

Keep changes focused and record the checks that actually ran. Security-sensitive behavior needs a regression case and another review. Do not mix a version bump or deployment with an unrelated cleanup. Preserve user work already present in a dirty checkout.
