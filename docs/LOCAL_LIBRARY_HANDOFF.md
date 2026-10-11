# Local library 1.3.0 handoff

This remains the historical finite baseline. Current profile `1.6.0` preserves
these four-module defaults and adds optional project tracking/resource reservations
plus kit planning.
Use the exact current profile when generating a new assembly. See
[project activation and API](PROJECT_DELIVERY_MODULE.md),
[reservation activation and API](RESOURCE_RESERVATIONS_MODULE.md),
[current acceptance](LOCAL_LIBRARY_RELEASE.md) and
[kit programme status](STARTER_KIT_IMPLEMENTATION.md). Company website and full
operating kits remain separate work.

This is the finite PRD 2.1 §50 release: shared installed artifacts, safe logical
composition, offline presence, scoped controls, measured local capacity and
upgrade/recovery. ARCHZXY's website is a future independent consumer.

## Cold installation and owner operation

Use a non-root POSIX account, Node 22.22.3, pnpm 10.15.1 and PostgreSQL tooling
(`pg_config`, `initdb`, `pg_ctl`, `pg_dump`, `pg_restore`). Chromium/Brave is needed
only for the browser regression command. Do not copy another owner's `.local`,
`.env`, credentials or database directory into a client installation.

```sh
pnpm install --frozen-lockfile --ignore-scripts
pnpm crm:build
pnpm crm:setup
pnpm crm:doctor
pnpm crm:start
```

The host listens at `http://127.0.0.1:4175`. Setup writes an owned mode-0600
bootstrap credential file under the mode-0700 `.local/crm` directory. Read it
privately, sign in and change the password; remove the obsolete bootstrap file.
Repeated setup preserves existing records and keys. The host and operator check
their embedded profile and owned schema ledgers before serving supported work.
Stop the host before rebuilding or changing modules. Build refuses a live lock.

No provider key is required for this local release. The owner conversation uses
its existing local planner by default. Public presence and the synthetic voice
consumer use offline policy; they make no live audio/avatar/provider request.
The previously configured free Gemini pilot remains bounded by its original
synthetic-corpus policy. See [CRM runtime](CRM_RUNTIME.md) for accounts and
[pilot operation](LOCAL_OWNER_PILOT.md) for exact consent/recovery.

## Compose an independent consumer

Write a selection file with the exact supported profile:

```json
{
  "version": "1.0.0",
  "profile": "1.3.0",
  "modules": {
    "contacts": true,
    "pipeline": false,
    "assistant": false,
    "nativeActions": false
  }
}
```

```sh
pnpm library:assemble selection.json NEW_DIRECTORY --dry-run
pnpm library:assemble selection.json NEW_DIRECTORY
cd NEW_DIRECTORY
npm install --ignore-scripts --no-audit --no-fund
node entries.mjs
```

The plan validates module prerequisites, package availability and exact release
compatibility. Output overwrite is refused. Files contain configuration and
package entry references; no CRM/assistant implementation is forked. Local-file
tarball references are intended for the developer's machine. For another machine,
transfer the private generated library inventory and tarballs and rewrite file
references under deterministic validation; do not transfer private state. Public
registry publication and autonomous deployment are outside this release.

All supported modules remain installed in the shared monolith; configuration
controls exposure/authority while retaining schema ownership. For an existing
owner host, place only the four-key `modules` object in an owned mode-0600 file:

```sh
pnpm crm:modules /absolute/private/selection.json
pnpm crm:start
```

This command requires a stopped host. Native disable refuses unresolved effects.
Cancel/reconcile with the original authorized host first. Disabled features refuse
new routes, tools and writes; receipt lookup/cancellation do not grant execution.
Enable does not create new identity grants. No module data is purged.

The future company website can import typed `planAssembly` from
`@archava/config/assembly`. Model-generated input is untrusted and must pass the
same validation; a valid plan supplies no credentials, price or deployment grant.

## Backup and safe recovery

```sh
pnpm crm:backup /absolute/private/pre-upgrade.dump
pnpm crm:backup-status
pnpm crm:restore /absolute/private/pre-upgrade.dump archava_recovered
```

Keep the archive and its mode-0600 runtime metadata together. Restore requires a
new database and preserves the original target. It generates
`restored-runtime.json`, invalidates restored identity and sets provider/native
holds with a fresh approval epoch. Select that private profile for explicit recovery:

```sh
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:user recover /absolute/private/recovery.json
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:doctor
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:resume-actions
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:start
```

Recovery input contains only `username` and a new `password`. Owner/staff grants
remain stored and rechecked; recovery does not invent roles. Read the original UUID
receipt after an uncertain result before considering a new action. Old approval
tokens do not become valid after resume. Provider holds follow their separate UTC
quota policy; native resume does not resume a provider.

Optional scheduled backups are enabled explicitly with
`pnpm crm:backup-policy enable 24` and verified through `crm:backup-status`.
They run only while the host is active, retain previous snapshots and have no
automatic deletion. Restored databases begin with scheduling disabled. Off-machine
backup, stopped-host scheduling and production RPO/RTO remain separate work.

## Versions, upgrade and rollback

The catalogue is `config/library-release.v1.json`. The generated resolved profile
records 25 internal artifact versions/hashes, 18 pinned runtime external versions,
five host-owned migration ledgers and contract versions. Host `0.6.0`, config
`0.5.0`, presence `0.1.0`, CRM/domain/persistence/assistant `0.7.0` and native
CRM Act `0.2.0` are the supported combination. Browse the catalogue for all entries.

The tested prior installation is profile `1.0.0`, source commit `f5cb768`.
Stop, back up, preserve private state/keys, rebuild supported artifacts, run setup
for explicit migrations/grants, then doctor and start. Existing deployments with
older, unknown schema combinations must be reconciled; editing migration checksums
to silence a refusal is not an upgrade strategy.

This release adds no database migration over that prior profile. Binary rollback
still requires the prior artifact closure and prior-compatible configuration;
the older strict settings schema cannot parse new module fields. Restoring an
older binary never reverses a future database migration. Use a tested fresh-target
backup restore when schema compatibility cannot be proved, with identity recovery
and execution holds intact.

## Reproduce acceptance

```sh
pnpm verify
pnpm security:verify
pnpm crm:benchmark
pnpm crm:verify-browser
pnpm crm:verify-upgrade --cold-prior
```

The dedicated CRM, Act, runtime, reuse and assembly commands remain available.
The security runner invokes their real PostgreSQL and installed-artifact checks;
root `verify` alone does not cover browser/database/template acceptance. Upgrade
uses an isolated Git archive when no prior binary exists, installs its frozen
lock with scripts disabled, and builds/starts both releases against synthetic data.
Fixtures and owned browsers/database processes are cleaned up after checks.

The standalone template keeps its own `npm test`, `npm run test:agent` and
`npm run build` commands. Its seven SDK integration checks need its Python venv;
that missing evidence, 13 moderate OpenTelemetry-chain findings and live provider
validation remain realtime gates. No production deployment, pentest certification
or unrestricted general AI writes are claimed.

See [acceptance and design decisions](LOCAL_LIBRARY_RELEASE.md) and
[sanitized evidence](audit/local-library/security.json).
