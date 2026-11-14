# Deployment

This directory is the reference deployment for the persistent Rust runtime. It
does not provision a development database and it does not run migrations: the
runtime verifies the release profile's exact migration history at startup and
refuses to serve on drift. Migration is a deliberate operator step.

Current build and startup instructions are maintained here and in the
[developer guide](../docs/DEVELOPER_GUIDE.md). The older
[cutover runtime record](../docs/archive/rust/PERSISTENT_RUNTIME.md) preserves migration-era context.

## 1. Build the artifacts

Use the root `rust-toolchain.toml`, Node from `.tool-versions`, the WASM target,
and the pinned `wasm-bindgen-cli` described in the developer guide:

```bash
node tools/rust/verify.mjs
node tools/rust/build-slice.mjs
```

The build produces `target/release/archava-server` and `target/site/pkg`.
There is no root pnpm installation or separate browser app build step. Produce a new
native distribution from fresh artifacts:

```bash
node tools/rust/package-native.mjs --out dist/native-candidate-001
```

The package includes server, Ops CLI, site assets, exact migration manifest,
public configuration, source/artifact provenance and a profile accepted by
`archava-ops`, plus systemd/Caddy and monitoring templates. Existing output directories are refused. Follow the
[native install and recovery procedure](NATIVE_INSTALL.md) in the package.
The owner-specific [app.archzxy.com plan](ARCHZXY.md) preserves the existing
Netlify site and waits for a selected persistent application server.

Do not build a release image from an unverified working tree. The image copies
these two artifacts verbatim.

## 2. Prepare PostgreSQL

Create an owner role for migrations and a **non-owner** runtime role. The
runtime role must not be the database owner, a superuser, or have `BYPASSRLS`;
the host checks this at startup and refuses otherwise.

Run `archava-ops migrate --runtime-role archava_runtime` from a verified native
distribution with `ARCHAVA_MIGRATION_DATABASE_URL` naming the schema-owner
connection. The CLI applies the checked manifest and runtime grants; the serving
process only verifies history. Bootstrap the first tenant owner with
`archava-ops bootstrap-owner` as described in the native procedure. The disposable
fixture is for tests and is never the production installer.

For the compose stack, prepare the roles first:

```bash
cp deploy/postgres.env.example deploy/postgres.env
cp deploy/archava.env.example  deploy/archava.env
# edit both; generate the identity key with: openssl rand -hex 32
docker compose -f deploy/compose.yaml up -d postgres
```

The init script creates `archava_runtime` with `CONNECT` only. It does **not**
apply migrations. Keep the password inside `DATABASE_URL` identical to
`ARCHAVA_RUNTIME_PASSWORD` in `postgres.env`. Run the explicit Ops migration command against
`postgres:5432` as `archava_owner` before starting `archava`; a boot without the
exact ledger history is a refusal, by design.

## 3. Configure

`deploy/archava.env.example` documents every required value:

| Variable                   | Purpose                                          |
| -------------------------- | ------------------------------------------------ |
| `DATABASE_URL`             | Non-owner runtime URL; exact migrated database   |
| `ARCHAVA_IDENTITY_KEY_HEX` | ≥32 bytes, hex; session/CSRF HMAC key            |
| `ARCHAVA_BIND_ADDR`        | Listen address (`127.0.0.1:8347` behind a proxy) |
| `ARCHAVA_SITE_ROOT`        | Directory containing the `pkg` browser payload   |
| `RUST_LOG`                 | `info` by default; `archava=debug` for diagnosis |
| `ARCHAVA_BACKUP_KEY`       | 32-byte base64 key for encrypted CRM backups     |
| `ARCHAVA_TRUSTED_PROXIES` | Exact proxy peers/CIDRs allowed to supply the login client address; empty trusts no forwarding headers |
| `ARCHAVA_METRICS_TOKEN` | Required metrics header token for a non-loopback scraper |
| `LIVEKIT_URL`, `LIVEKIT_API_KEY`, `LIVEKIT_API_SECRET` | Optional native media room configuration |
| `GEMINI_LIVE_MODEL`, `GEMINI_LIVE_API_KEY` | Explicit live model/key; key may fall back to `GEMINI_API_KEY` |

See [live media setup and acceptance limits](../docs/modules/LIVE_MEDIA.md) before enabling voice/camera.

Keep `archava.env` mode `0600`, owned by root — systemd reads the environment
file before dropping to the service user, and the runtime never needs to read
it directly. Inject secrets from your secret manager rather than committing
them. The identity key is not rotatable without invalidating every session;
treat it as long-lived.

## 4. Run

### systemd

Create `/etc/archava/archava.env` from `deploy/archava.env.example`. For a
systemd deployment on the same host, change the database host in
`DATABASE_URL` from `postgres` to `127.0.0.1`.

```bash
sudo useradd --system --home /var/lib/archava --shell /usr/sbin/nologin archava
sudo install -d -o archava -g archava -m 0750 /var/lib/archava
sudo install -d -o root -g archava -m 0750 /etc/archava
sudo install -m 0644 deploy/archava.service /etc/systemd/system/archava.service
sudo install -m 0600 -o root -g archava deploy/archava.env /etc/archava/archava.env
sudo install -d -o root -g root -m 0755 /opt/archava/bin /opt/archava/site
sudo install -m 0755 target/release/archava-server /opt/archava/bin/archava-server
sudo cp -a target/site/. /opt/archava/site/
sudo systemctl daemon-reload
sudo systemctl enable --now archava
```

Check:

```bash
curl -fsS http://127.0.0.1:8347/healthz   # liveness: "ok"
curl -fsS http://127.0.0.1:8347/readyz    # readiness: database probe
curl -fsS http://127.0.0.1:8347/metrics   # Prometheus text
journalctl -u archava -f                  # JSON logs
```

### Docker / compose

After the database is migrated and `deploy/archava.env` + `deploy/postgres.env`
exist:

```bash
docker build -f deploy/Dockerfile -t archava-runtime .
docker compose -f deploy/compose.yaml up -d archava caddy
```

The image is read-only, drops all capabilities, runs as UID 10001, and uses
`/readyz` as its healthcheck. Caddy obtains and renews TLS certificates; set
`ARCHAVA_DOMAIN` in the environment or a `.env` file next to the compose file.
The dedicated ingress network fixes Caddy at `172.30.84.2`; only that peer is
trusted for forwarded login addresses. If the subnet conflicts with a host
route, change the subnet, Caddy address and trusted proxy setting together.

## 5. TLS and proxy

`deploy/Caddyfile` forwards to the runtime and leaves the security headers to
the application (duplicating HSTS/CSP at the proxy produces conflicting
headers). If you use another proxy, preserve:

- the original `Host` and `X-Forwarded-Proto` only from a trusted hop;
- request body limits;
- no caching of authenticated responses;
- `/metrics` stays on the monitoring network: the reference Caddyfile blocks
  it publicly and a local scraper reads `127.0.0.1:8347/metrics` (or replaces
  the block with an explicit allow-list). Non-loopback scrapers must send
  `x-archava-metrics-token` matching `ARCHAVA_METRICS_TOKEN`; without it the
  application refuses before probing the database.

`ARCHAVA_TRUSTED_PROXIES=127.0.0.1,::1` fits host-local Caddy. The runtime
walks `X-Forwarded-For` from the trusted transport toward the first untrusted
hop. Never add public client networks to this list. A direct client cannot
change its login budget by submitting a forwarded header.

Scrape and alert templates are in [monitoring/README.md](monitoring/README.md).
Configure and test an actual receiver/on-call route before operational acceptance.

## 6. Backups

CRM backups are AES-256-GCM encrypted with `ARCHAVA_BACKUP_KEY`. A backup
without the key refuses rather than writing plaintext. Store the key in a
secret manager, **not** on the backup volume. Test restore into a disposable
database before relying on the archive:

```bash
# Inspect the native operator contract before choosing connection/role values.
cargo run --release -p archava-ops -- help
# Store the key securely in the environment; these examples target local PostgreSQL.
ARCHAVA_BACKUP_KEY=... cargo run --release -p archava-ops -- backup \
  --port 5432 --db archava --user archava_backup --out /secure/backup-check
ARCHAVA_BACKUP_KEY=... cargo run --release -p archava-ops -- restore \
  --manifest /secure/backup-check/archava-backup.manifest.json --port 5432 \
  --db archava_restore_check --user archava_restore
```

## 7. Upgrade and rollback

1. Take and verify a backup.
2. Stop the runtime (`systemctl stop archava` or `docker compose stop archava`).
3. Apply the new profile's migrations with the owner role. Migrations are
   append-only; never edit an applied file.
4. Replace the binary and `site` payload, then start the runtime.
5. Confirm `/readyz` is 200 and the startup log says every mounted ledger
   passed readiness.
6. If startup refuses, restore the previous binary **and** the previous
   database snapshot; a schema newer than the binary is not forward-compatible.

The runtime logs a refusal reason on startup (migration checksum, owner role,
missing RLS, short key, missing payload). Do not bypass it by changing the
role or the ledger.

## Business goal planner

The native server starts and joins its durable Ava worker. Configure
`GEMINI_AGENT_MODEL` and `GEMINI_AGENT_API_KEY` (fallback `GEMINI_API_KEY`) for
general model-backed planning. Without them, bounded context, clarification,
stock replenishment and native action detail forms remain available. No model
is guessed and Gemini Live uses separate settings. See the
[agent runtime contract](../docs/modules/AGENT_RUNTIME.md) for approval, current
session expiry, recovery, budgets and owned-fixture acceptance limits.
