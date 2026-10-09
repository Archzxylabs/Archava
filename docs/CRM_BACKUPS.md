# Automatic local CRM backups — host operation

Checkpoint: 9 October 2026, baseline `bd955c8`. The local CRM host is 0.2.0.
The reusable library artifacts remain unchanged from CRM 0.7.0; this is a supported
host operation, not a new business module. No dependency, schema migration,
permission grant, cloud service or model request is added.

## Operator use

```sh
pnpm crm:backup-policy enable 24
pnpm crm:backup-run
pnpm crm:backup-status
pnpm crm:doctor
```

The policy applies separately to the configured database and persists in its
private `backup-policy-<database>.json`. Missing policy means disabled. Enabling
is explicit; setup and code upgrades preserve accounts, settings and existing
archives. The supported local installation enables a 24-hour policy.

While the host runs, it checks eligibility at startup and every minute. A valid
snapshot younger than the configured interval is reused; a missing, expired,
corrupt or failed snapshot triggers a new attempt. Downtime causes one catch-up
snapshot on the next start. This is an in-process schedule: the host/laptop must
be running. It is not an unattended OS timer or a production RPO/RTO guarantee.

Intervals are whole hours from 1 to 168, counted from successful completion.
Manual `crm:backup [private-output.dump]` remains supported and shares the same
publisher. A successful manual backup also advances the schedule. `backup-run`
checks the enabled policy and reports `succeeded`, `not_due`, `disabled` or `busy`.
`backup-policy disable` stops future scheduled work when the worker next checks;
it does not interrupt an already active dump or remove any archive.

Existing backups are retained. There is no automatic deletion, external upload,
paid storage or credential output. Off-machine storage/encryption, disk retention
and a schedule while the application is stopped remain operator/deployment work.

## Snapshot publication and diagnostics

The shared publisher uses PostgreSQL custom-format `pg_dump` and validates its
archive catalogue with `pg_restore --list`. It writes private staging files in
the destination directory, flushes archive/metadata files, and publishes them
using no-overwrite hard links. Archives and adjacent `.runtime.json` are owned
mode-0600 files inside an owned private directory. The supported filesystem must
provide POSIX hard links. A failed write does not turn an empty placeholder into
a completed backup; temporary files created by that attempt are cleaned up.

The original archive and metadata are never overwritten. Complete material stays
available even if updating the private status subsequently fails. Policy/status
replacement uses private staging and atomic rename; a missing or malformed status
cannot establish freshness. Public requests and Archava tools cannot set policy,
choose a destination or execute a backup.

A PostgreSQL advisory lock serializes manual/scheduled processes for one database.
The lock belongs to a database session and releases when that session closes,
including process failure; no stale filesystem lock authorizes a second writer.
Dump and catalogue validation are asynchronous and each bounded to 60 seconds.
Host shutdown stops polling, aborts an active child and waits for cleanup.
Backup or diagnostic failure does not terminate the CRM host.

Private `backup-state-<database>.json` records the last attempt and completed
archive binding. Status checks validate private ownership/mode, archive length and
SHA-256, adjacent runtime metadata and the current configuration digest. Changing
configuration or damaging a file makes it unfit for freshness. The policy's
enabled flag remains separate from the existence of a valid manual archive.

`backup-status` and the `backups` field in `doctor` expose only status, timing,
interval, integrity and size. They expose no archive path, settings, identities,
configuration digest, key or database contents. A catalogue/integrity check is
not a full restore drill for every individual snapshot.

## Restore and verification

Use the existing [restore procedure](CRM_RUNTIME.md#backup-and-recovery) with the
archive and its adjacent metadata. It creates a new target, rotates identity,
invalidates restored sessions/accounts and holds native actions under a fresh
epoch. The Gemini/provider restoration hold remains separate. Primary policy and
freshness are not inherited by the new database; enable scheduling explicitly
after recovering that target.

Real PostgreSQL verification restores an automatically published archive, then
rechecks CRM records, memberships, command receipts, pipeline history, provider
holds and native-action controls. It also proves startup catch-up without repeat
capture, concurrent-process exclusion, failed/corrupt archive handling, private
policy refusal and restoration-policy separation. Worker lifecycle tests exercise
overlap, stop-before-start, active shutdown and failing diagnostics. Browser CRM
flows remain regression checks. Tests use separate synthetic installations.

The original local installation receives read-only smoke and protected-data
preservation checks; enabling the policy changes only its new backup configuration
and snapshot/status files. The broader owner planning estimate stays 60%.

- [Root validation](audit/2026-10-09-backup-validation.json): all eight gates,
  1,442 tests in 99 files; all 24 library artifact versions/hashes unchanged.
- [Runtime proof](audit/2026-10-09-backup-runtime-proof.json): 40 checks,
  including actual restoration of an automatic snapshot and current native
  identity/provider controls.
- [Browser regression](audit/2026-10-09-backup-browser-proof.json): 70 checks
  preserving owner/staff CRM, overview, completion and receipt recovery.
- [Installed pilot proof](audit/2026-10-09-backup-installed-proof.json):
  24-hour policy enabled, first snapshot integrity verified, repeat run skipped;
  protected original rows/files and 14 prior archive/metadata files preserved.
