# Rust-first cutover status

## Decision

The product direction is **all-in Rust**. Rust is the default local product
entrypoint; the TypeScript runtime remains available only under explicitly
named `legacy:*` scripts while the persistent Rust launcher and remaining
cutover proofs are completed.

## Current default commands

| Command                    | Current path                                         |
| -------------------------- | ---------------------------------------------------- |
| `pnpm build`               | Rust persistent server + SSR/WASM build              |
| `pnpm dev`                 | Rust persistent runtime                              |
| `pnpm start`               | Rust persistent runtime                              |
| `pnpm verify`              | Rust fast gate                                       |
| `pnpm rust:verify`         | Rust fast gate                                       |
| `pnpm rust:verify-release` | Rust candidate gate plus legacy compatibility proofs |

The default runtime requires an operator-provided `DATABASE_URL` and
`ARCHAVA_IDENTITY_KEY_HEX`, verifies the embedded migration identity for every
mounted ledger before binding, and serves the same SSR/WASM payload as the
fixture preview. The disposable PostgreSQL fixture remains available for local
browser proofs as `pnpm rust:preview`; it is not the production runtime.
Operational setup and the lifecycle proof are documented in
`docs/rust/PERSISTENT_RUNTIME.md`.

The old TypeScript entrypoints remain available as:

- `pnpm legacy:build:web`
- `pnpm legacy:dev`
- `pnpm legacy:start`
- `pnpm legacy:verify`

## Remaining cutover work

1. Rebind backup/restore and operational proofs to the persistent Rust launcher.
   The Rust browser proof now provisions/seeds only through the fixture helper,
   then serves all browser interactions through `archava-server` and restarts a
   second persistent process against the same database.
2. Finish the final release gate against one Rust-enabled candidate identity.
   `pnpm rust:verify` is green at 22/22 steps and `pnpm security:verify` is green
   for its 46-command chain. The first full release run reached 17/17 local
   steps but remained `incomplete` until the five historical aggregate proof IDs
   were rebound to current integrated cohort evidence. The current shell lacks
   `cargo`, so the post-rebinding release rerun is still open.
3. Delete the TypeScript runtime/apps only after those proofs pass against the
   same candidate identity.

No TypeScript source was deleted in this phase. Deleting it before the
persistent Rust proofs pass would turn compatibility gaps into a false
production cutover.
