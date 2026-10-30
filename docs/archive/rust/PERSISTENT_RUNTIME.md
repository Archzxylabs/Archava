# Persistent Rust runtime

The default `pnpm dev` and `pnpm start` commands run the persistent Rust
launcher (`archava-server`). The launcher never provisions PostgreSQL and never
falls back to the disposable fixture.

## Required configuration

The database must already be migrated with the release profile's exact ledger
history and the URL must authenticate as the non-owner runtime role:

```bash
export DATABASE_URL='postgres://<runtime-user>:<password>@<host>:5432/<database>'
export ARCHAVA_IDENTITY_KEY_HEX='<at-least-32-byte-hex-secret>'
```

Optional settings:

```bash
export ARCHAVA_BIND_ADDR='127.0.0.1:8347'
export ARCHAVA_SITE_ROOT="$PWD/rust/target/site"
```

Start the server:

```bash
pnpm build
pnpm start
```

Startup derives the migration checksums embedded in the binary, verifies
commerce, identity, CRM, owner, purchasing, and every mounted ERP ledger, and
only then binds the HTTP listener. A mismatch, owner/superuser runtime, missing
forced RLS policy, short identity key, or missing browser payload is a startup
refusal.

## Disposable preview

The fixture-backed browser proof remains explicit:

```bash
pnpm rust:preview
pnpm rust:browser-slice
```

That path provisions its own PostgreSQL instance and is for local proof data;
it is not the persistent production launcher.

## Lifecycle proof

The Rust fast gate runs the focused lifecycle test. It provisions a disposable
database for the test, starts two separate persistent-server processes against
the same runtime URL, kills the first process, starts the second, and proves a
database identity remains usable after restart:

```bash
cargo test --locked --offline -p archava-slice --test persistent_launcher -- --test-threads=1
```
