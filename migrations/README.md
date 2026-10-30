# SQL migration ownership

All 38 checked-in SQL migrations currently live under this directory, grouped into 29 ledger directories. The owner-configuration migration was relocated from `crates/host/migrations/owner` to `owner-pg/`; its migration ID, schema namespace, SQL bytes, and checksum are unchanged.

The persistent server embeds the migrations it mounts in [archava_server.rs](../crates/slice/src/bin/archava_server.rs). The disposable PostgreSQL fixture resolves the same canonical files through [pg-fixture](../crates/pg-fixture/src/lib.rs). A file's presence here does not mean a given runtime mounts it or its UI is complete.

Applied migrations are append-only. Add a numbered file and its ledger/readiness registration instead of editing historical SQL. Apply migrations with the owner role; serve requests through a non-owner runtime role. Do not rename schemas or migration IDs as part of directory cleanup.
