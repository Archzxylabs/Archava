# Local owner pilot — supported release scope

Target confirmed by the owner: **local first, free**, 9 October 2026. This release
is the reusable CRM owner pilot. Its acceptance is distinct from the full Archava
commercial/customer-presence roadmap and the separate ARCHZXY company website.

The subsequent [0.5.0 worklist release](CRM_WORKLIST.md) adds authorized due tasks
across contacts, using the same local host. The 0.4.0 acceptance evidence below
remains the historical foundation; its native action/recovery boundary is preserved.

The [0.6.0 release](CRM_COMPLETION.md) registers separate completion consent and
adds exact task reads/preparation. Creation approval never authorizes completion;
the existing permission, audit, recovery and restore boundaries apply to both.

## Supported capability

The same compiled library serves Hospitality and Professional Services without
forks. An authenticated owner can manage contacts, notes, follow-up tasks and
contact-linked sales opportunities, and use Archava to read records and prepare
follow-up drafts. Staff retain assigned-resource scope; viewers retain read-only
grants. Sessions, CSRF, current memberships and forced tenant RLS remain enforced.

The new `@archava/crm-act` module adds one bounded native action:
`crm.followup.create`. The owner reviews the exact text, due date and contact
revision, then clicks **Konfirmasi penyimpanan**. Review creates no task. Execution
requires a consumed one-time confirmation, a valid server MAC, the active intent
claim, current service and human grants, and the current resource revision.

The CRM transaction locks the service and human membership intersection and
checks the approval on the same database transaction before committing command,
task, contact revision and audit. Audit records both the service actor and the
human initiator. Ordinary machine mutation and model tools still cannot write.
The approved API dispatches only this follow-up command, not arbitrary CRM writes.

A response lost after commit is resolved through its original UUID receipt.
Reload and recovery never POST again. Unknown outcomes remain unresolved until
authoritative evidence exists. A pending action can be explicitly cancelled;
an executing or completed action cannot be cancelled into permission to repeat.
Only the opaque UUID is stored in account/workspace-scoped tab storage; draft
content and tokens remain transient.

Gemini retains the previously verified Free-project profile and the synthetic-only
pilot. Private/free-form owner text and records stay local. The existing thirteen
live synthetic cases are historical evidence, not rerun or represented as general
private-data planning quality. No paid fallback or external booking/mail/payment
adapter is configured in this host.

## Use and operation

Use the running host at `http://127.0.0.1:4175`. The existing private credential
file is `.local/crm/initial-owner.json`; setup preserves enrolled accounts and
existing credentials. Select a workspace and a contact. Fill the message and
schedule in **Detail draft follow-up**, then ask `siapkan follow-up`.
Review and confirm separately.
After an uncertain response, use **Cek hasil penyimpanan**.

```sh
pnpm crm:doctor
pnpm crm:start
pnpm crm:backup
```

The supported runtime is Node 22.22.3, pnpm 10.15.1 and local PostgreSQL tooling,
under the same non-root POSIX account. The database uses a private Unix socket;
the application listens on loopback. No cloud account or paid dependency is needed.
Diagnostics validate the owned schemas and runtime role, and report action hold
state without printing credentials. Existing public health checks report sanitized
database outages; backups and tested restoration are available to the operator.

Restore uses a **new database** and a separate private execution-control file.
It invalidates restored identity sessions/accounts and holds provider allowance
and native actions. A new approval epoch prevents old confirmation from running
against the restored copy. Recover selected accounts explicitly, then:

```sh
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:doctor
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:resume-actions
ARCHAVA_CRM_PROFILE=restored-runtime.json pnpm crm:start
```

Resume changes native approval epoch; it does not clear the Gemini/provider
restore hold or enable external effects. Retain pending recovery markers when a
receipt is absent after restore: absence never establishes that an old effect
failed. Backup retention, unattended schedules and production RPO/RTO remain
deployment-specific work rather than promises of this local pilot.

## Version and migration boundary

CRM/domain/assistant are 0.4.0; the CRM database adds migration
`003_native_approval` while preserving historical audit/command data. Existing
human rows gain a nullable initiator column; approved service rows populate both
identities. Native confirmations have their own checksummed schema and forced
tenant/owner-scope RLS. The shared durable intent schema remains separately owned.

The runtime cold-installs 24 compiled internal artifacts. The supported combination
is recorded in the artifact/proof manifests. Migration checks reject modified
history and incompatible old binaries. The pre-upgrade private backup and Git
revision provide rollback inputs; reverting only the binary does not downgrade
the database. Source pricebooks, archived PRD and the portable template are unchanged.

## Acceptance evidence

Root gates, two independent installed consumers, actual PostgreSQL races/restart,
authenticated HTTP, real backup/restore and Chromium owner/staff flows are checked
separately. The [release validation](audit/2026-10-09-owner-validation.json)
records all eight root gates, 1,422 tests in 95 files and the exact 24 artifact
versions/hashes. Installed artifacts match those exercised by both consumers.

- [Act composition](audit/2026-10-09-owner-act-proof.json): 14 checks, including
  native approval, concurrent execution, real restart and restored execution hold.
- [CRM regression](audit/2026-10-09-owner-crm-proof.json): 31 checks, including
  permissions, migrations, pipeline and authoritative receipt recovery.
- [Persistent runtime](audit/2026-10-09-owner-runtime-proof.json): 30 checks,
  including actual backup/restore, explicit resume and authenticated native HTTP.
- [Chromium browser](audit/2026-10-09-owner-browser-proof.json): 48 checks,
  including review before execution, response loss, reload, logout and staff scope.
- [Original installation](audit/2026-10-09-owner-installed-proof.json): original
  domain/identity rows and private files preserved; native feature enabled and
  the installed host healthy. The smoke submitted zero domain commands.

Tests use isolated databases and synthetic resources. This release made zero
actual model or external action calls; earlier Gemini synthetic live evidence
remains historical. No live provider send, payment, PMS booking or public
deployment is claimed.

Completed local acceptance comprises core/module reuse, CRM CRUD/pipeline,
credential-backed identity/resource grants, reviewed native execution/audit,
receipt recovery, bounded AI selection, diagnostics and backup restoration. Broader
ERP modules, general-purpose AI write planning, public identity/TLS/SSO, automated
monitoring and customer voice/avatar are future releases with separate acceptance.

The [0.7.0 authorized overview](CRM_OVERVIEW.md) adds a read-only owner/staff CRM
summary to both dashboard and local Archava. The original acceptance and its dated
counts above remain historical; overview evidence is recorded separately.
