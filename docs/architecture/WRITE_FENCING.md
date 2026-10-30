# Serving writer fencing

The native runtime carries one fixed writer epoch and process token in every PostgreSQL connection, including reconnects. Database statement triggers check that identity before runtime writes to registered product tables. Tenant membership, permissions and forced tenant RLS remain separate requirements.

The implementation is [the adapter factory](../../crates/adapters/src/writer.rs), [Control migration](../../migrations/control-pg/001_serving_writer.sql), [native launcher](../../crates/slice/src/bin/archava_server.rs) and [readiness boundary](../../crates/host/src/router.rs). The canonical execution status is [launch readiness](../product/LAUNCH_READINESS.md).

## Write and takeover ordering

1. The server reserves its listening socket, validates Control history/non-owner access, and acquires an epoch before accepting application traffic. The default refuses an already active writer.
2. Pool connection options retain that process's epoch/token. A replacement connection never fetches another process's identity automatically.
3. Before each INSERT, UPDATE, DELETE or TRUNCATE, a statement trigger takes a shared lock on the serving lease row and checks epoch, token and drain state. The lock lasts through the transaction commit or rollback. Valid transactions can share it.
4. A takeover updates the lease under an exclusive row lock. It waits for existing valid transactions; a bounded lock timeout refuses a blocked takeover. Once the new epoch commits, old writers fail at their next write, including raw Identity and worker SQL.
5. Startup checks guard coverage, function identity, enabled state and complete before-statement event coverage. An incomplete or disabled guard refuses readiness. A stale running process answers `/readyz` with 503.

The lock ordering follows PostgreSQL's [row lock rules](https://www.postgresql.org/docs/18/explicit-locking.html). Fixed connection parameters use the pinned SQLx [PgConnectOptions options API](https://docs.rs/sqlx/0.9.0/sqlx/postgres/struct.PgConnectOptions.html#method.options).

## Ownership and schema changes

Control is global operational metadata, not business data belonging to a tenant. Its forced policy allows operational rows; role grants protect access. Runtime receives schema/history access and four controlled functions, with no direct lease-table grant. The guard function is private, so runtime cannot attach it to its own temporary trigger.

Business tables retain their tenant policies. A human owner in the application still uses the non-owner database runtime. Dedicated schema-owner/cluster-admin sessions may run installation, bootstrap and restore; these are explicit operator paths. Readiness rejects serving roles that own tables or inherit/hold membership in an owning role, as well as SUPERUSER/BYPASSRLS.

Control migration runs after product migrations and registers guards only on the product schemas it names. The installer refreshes missing guards after future migrations; it refuses replaced or disabled guards. Adding a new product schema requires updating the Control protocol and its coverage tests. Migration bytes that have shipped are preserved.

## Drain, crash recovery and restart

Use the owner connection through `ARCHAVA_MIGRATION_DATABASE_URL` to run:

```bash
./bin/archava-ops drain-writer
```

Drain first waits for in-flight writes. Stop/drain traffic before this operator action; subsequent writes on the old process are refused. The command returns an epoch and drain state, without the token. Runtime credentials cannot perform this administrative command.

A normal native shutdown drains only its own epoch/token after requests finish. It cannot drain a newer holder. The next startup can keep `ARCHAVA_WRITER_TAKEOVER=false`.

After a crash or an intentional handover, set `ARCHAVA_WRITER_TAKEOVER=true` for the replacement startup when an old active lease remains. This is an explicit takeover decision. A duplicate bind address fails before acquiring a new epoch. A failed startup never silently adopts the previous holder's token.

## Restore

The native CLI excludes ordinary target connections during restore. `pg_restore` runs in a single transaction; a failed replay rolls back preceding target schema/data changes. After a successful replay, the shared restore function advances and drains the copied serving lease before reopening the target. A process token from the source must not revive on the restored database.

If replay does not return a successful acknowledgement, or lease invalidation fails afterwards, the CLI leaves the target connection limit at zero and returns a refusal. A process exit alone cannot prove that COMMIT did not happen; raw replay stderr is not included in the public report because it can contain business values. Inspect and repair the target through an administrator before reopening it. See [native installation and recovery](../../deploy/NATIVE_INSTALL.md).

## Evidence and limits

[Adapter tests](../../crates/adapters/tests/writer.rs) cover missing context, explicit takeover, stale pooled and fresh connections, old-holder drain refusal, concurrent valid writes, commit ordering, disabled/partial guards and inherited owner roles. [HTTP tests](../../crates/host/tests/workspace_flows.rs) cover stale CRM, Commerce, Cockpit and login effects plus readiness. [Ops tests](../../crates/ops/tests/upgrade_restore.rs) cover copied-token invalidation and failure during restore; [native installation](../../crates/ops/tests/native_install.rs) exercises the real CLI and postrestore login.

These persistence checks do not establish exactly-once external network effects. Provider workers still need original request keys, durable correlation and outcome verification. Live provider acceptance remains deferred by the owner.
