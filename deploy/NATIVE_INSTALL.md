# Install and recover a native Archava distribution

This directory is a native distribution with `bin/archava-server`,
`bin/archava-ops`, browser assets in `site/pkg`, an exact SQL migration manifest,
public release configuration, build provenance and `release-profile.json`.
Keep the distribution immutable. Secrets and database volumes belong elsewhere.

From the distribution root:

```bash
export ARCHAVA_OPS_ROOT="$PWD"
./bin/archava-ops verify-release
```

The verifier recomputes every listed file digest and checks catalogue identity.
A profile from a different source/build is not fresh evidence for this package.
Verify the downloaded archive's published digest before extracting it.

## Database and first owner

Provision PostgreSQL and two separate login roles before installation. The
migration role owns the database/schemas. The runtime role has no SUPERUSER,
BYPASSRLS, CREATEDB, CREATEROLE or membership in the owner role. Provision roles
through your PostgreSQL administration process; Archava never creates roles or
passwords implicitly. Keep PostgreSQL off the public network.

Load `ARCHAVA_MIGRATION_DATABASE_URL` from your secret manager/environment as
the migration owner connection. Then:

```bash
./bin/archava-ops migrate --runtime-role archava_runtime
```

The CLI verifies the distribution, checks the manifest's SQL digests, serializes
migration history, refuses a modified/unknown prefix, applies missing migrations
in dependency order and grants the reviewed runtime privileges. The whole apply
runs in one database transaction. A second successful run applies zero SQL.

Set `ARCHAVA_OWNER_PASSWORD` through the same private environment mechanism
(15–128 bytes). Do not put the password in shell arguments, source or chat.

```bash
./bin/archava-ops bootstrap-owner \
  --tenant my-company --principal owner --username owner@example.com
unset ARCHAVA_OWNER_PASSWORD ARCHAVA_MIGRATION_DATABASE_URL
```

Bootstrap creates a new human identity, Tier 1 domain memberships and owner
configuration authority atomically. It refuses existing users and previously
provisioned tenant owners; it never overwrites a password or silently grants
more authority to an existing account. Later account administration needs an
explicit procedure. Tier 2 modules do not gain grants from this bootstrap.

## Serve

Load `DATABASE_URL` for the **runtime** role and a private random
`ARCHAVA_IDENTITY_KEY_HEX` of at least 32 bytes (64 hex characters). Configure
`ARCHAVA_BIND_ADDR` for the reverse proxy and use this distribution's site:

```bash
export ARCHAVA_SITE_ROOT="$PWD/site"
./bin/archava-server
```

The server reserves its socket before acquiring a serving writer epoch, then verifies exact mounted migration history, non-owner RLS, fixed writer context and complete write-guard coverage before accepting application traffic. Use `/readyz` to check database and writer readiness. Configure TLS, service
supervision, logs and external monitoring through the deployment environment.
The server does not migrate or create an owner automatically.

Keep `ARCHAVA_WRITER_TAKEOVER=false` for initial startup and after a normal graceful shutdown. An active writer refuses a second startup. For a planned stop, use the owner connection to run `./bin/archava-ops drain-writer`, then stop the old process. After a crash or intentional takeover, explicitly set `ARCHAVA_WRITER_TAKEOVER=true` for the replacement. Old processes keep their old epoch even on reconnect and cannot write after takeover. A stale process cannot drain its replacement. The epoch check lasts through each transaction; an intentional takeover waits for preceding valid writes to finish.

## Encrypted backup and isolated restore

The CLI uses local PostgreSQL client tools. Inject `PGPASSWORD` for the explicit
backup/restore cluster administrator and `ARCHAVA_BACKUP_KEY` as a private
32-byte hex/base64 key, kept off the backup volume. Use a new output directory
for every capture:

```bash
./bin/archava-ops backup --port 5432 --db archava --user postgres \
  --out /private/backups/capture-001
```

Archives use AES-256-GCM. Plaintext staging and the archive are created with
mode `0600` on Unix; staging is removed when the command exits normally. A
killed process can leave staging behind, so use a private backup directory and
include its cleanup in the operator procedure. Retain the manifest beside the
archive and keep key recovery separate from archive recovery.

Create a separate target database, with the **same owner/runtime role names
already provisioned**. Restore preserves their archived object ownership and
grants. A role mismatch is a restore failure. Stop/drain writers for that target;
restoring to the source database, a target with any other client connection, or an active fenced target is refused. During restore the CLI holds a target lock and connection limit zero, blocking ordinary runtime connections. It restores the previous limit on success and pre-replay validation failure. An unconfirmed replay or failure to invalidate the restored writer leaves the target closed for administrator inspection. If the process crashes, the limit stays zero: an administrator must inspect the target and explicitly restore its intended connection limit before starting services.

```bash
./bin/archava-ops restore \
  --manifest /private/backups/capture-001/archava-backup.manifest.json \
  --port 5432 --db archava_recovery --user postgres
```

After restore, start the candidate server against the target runtime connection
and verify readiness, owner login, tenant access and representative records
before traffic cutover. A successful `pg_restore` alone is not application
acceptance. The CLI runs pg_restore in a single transaction and preserves object ownership/grants. A replay error rolls back preceding target changes. Successful restore advances and drains the source writer identity before reopening the target; copied process tokens cannot revive there. The tests exercise this login boundary locally; they do not prove
production-sized recovery time or zero-loss disaster recovery. Plan RPO from
available backups/WAL and measure RTO on representative data.

## Provider configuration

Live email, payment and realtime voice acceptance is tracked separately. The
native package or a passing local rehearsal does not prove those integrations
are connected. Keep any unimplemented/unconfigured provider feature unavailable
until its transport, authorization and live acceptance have been completed.

## Optional invoice payments

See `docs/architecture/PAYMENTS.md` in the source repository for payment configuration and restore recovery. The distribution includes `config/payment-providers.example.json` with secret environment variable references. Provision a dedicated Billing service with `archava-ops bootstrap-payment-service --tenant T --principal P` using the explicit owner connection, then set `ARCHAVA_PAYMENT_CONFIG` for the serving service. Do not put key values in the JSON. After restore, payment creation stays held until merchant reconciliation and `archava-ops resume-payment-creation --confirm-reconciled`.

## Optional invoice email

The distribution includes `config/email-providers.example.json`. Set
`ARCHAVA_EMAIL_CONFIG` to that administrator-managed file (normally
`/etc/archava/email-providers.json`). The `apiKeyEnv` field names the
environment variable that holds the Resend key; supply that variable through
the service secret environment and never put the key in the JSON. An absent
registry leaves email unconfigured and send requests fail closed with
`email_provider_unconfigured`; an explicitly invalid registry refuses startup.
Configuration does not prove delivery, and live email acceptance is tracked
separately.

## Optional realtime presence

Set `LIVEKIT_URL`, `LIVEKIT_API_KEY` and `LIVEKIT_API_SECRET` to enable
realtime presence. If any value is absent, presence is unconfigured and
session creation fails closed with `presence_unconfigured`; no provider is
contacted at startup. The loopback transport used by local tests does not
prove live voice or video.
