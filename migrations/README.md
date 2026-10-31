# SQL migration ownership

All 44 checked-in SQL migrations currently live under this directory, grouped into 30 ledger directories. The owner-configuration migration was relocated from `crates/host/migrations/owner` to `owner-pg/`; its migration ID, schema namespace, SQL bytes, and checksum are unchanged.

The persistent server embeds the migrations it mounts in [archava_server.rs](../crates/slice/src/bin/archava_server.rs). The disposable PostgreSQL fixture resolves the same canonical files through [pg-fixture](../crates/pg-fixture/src/lib.rs). A file's presence here does not mean a given runtime mounts it or its UI is complete.

## Applied versus unmounted ledgers

`manifest.v1.json` is the installer's contract: 37 of the 44 files, across 24 ledgers, carry an `id`, `path` and SHA-256 that the Ops installer verifies and applies. The remaining seven files are **unmounted**: no manifest entry, no embedded registration, and no runtime readiness check.

| Unmounted ledger | Files |
| --- | --- |
| `act-confirmation-pg` | `001_confirmation_challenges.sql` |
| `act-host-pg` | `001_host_intents.sql`, `002_host_scope_binding.sql` |
| `act-storage` | `001_act_attempts.sql` |
| `capacity-pg` | `001_delivery_capacity_pools.sql` |
| `crm-act` | `001_native_confirmations.sql` |
| `universal-pg` | `001_universal_enterprise_ledgers.sql` |

They are kept in place rather than deleted (the bytes may be the only record of an earlier design) and rather than promoted into the manifest (that would change what an installer verifies and applies, and none of the seven has a mounted route or readiness registration). The decision is enforced, not remembered: [`tools/check-structure.mjs`](../tools/check-structure.mjs) fails when a SQL file is in neither the manifest nor its explicit `UNMOUNTED_MIGRATIONS` list, when a listed checksum stops matching, or when a listed file disappears. Mounting one of these ledgers therefore requires the full four-step registration described in `AGENTS.md`, plus removing its row from that list.

Applied migrations are append-only. Add a numbered file and its ledger/readiness registration instead of editing historical SQL. Apply migrations with the owner role; serve requests through a non-owner runtime role. Do not rename schemas or migration IDs as part of directory cleanup.
