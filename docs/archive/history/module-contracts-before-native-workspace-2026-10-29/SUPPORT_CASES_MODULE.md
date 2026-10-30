# Internal support cases and investigation history

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/support.rs), [persistence](../../crates/adapters/src/erp/support.rs), [HTTP](../../crates/host/src/erp/support.rs), [integration tests](../../crates/host/tests/erp_support.rs).

Profile 1.9.0 introduces `@archava/support` and `support-pg` 0.1.0. Professional
service and property-operations consumers share one native case workflow without
CRM or AI dependencies. This accepts bounded internal support, not a full legal,
clinical, recruitment, property-management or repair-workshop system.

## Domain ownership

The pure package owns strict contracts, services, API, private read tools and a
shared owner dashboard. The PostgreSQL adapter owns six forced-RLS business tables:
memberships, cases, commands, internal notes, immutable transitions and audit;
its seventh table is the checksummed migration ledger. No existing domain schema
or stored record is migrated. Runtime SQL privileges do not enroll accounts.

Cases own immutable requester/subject/description and creator identity, current
assignee, priority, an optional internal due target, investigation state, resolution
cycle and revisions. Active assignment targets are human case readers in the same
domain/tenant. Friendly labels are private operator metadata; they grant no access.
The global assignment directory is separately permission-gated and paginated.
Detail/recovery reads hold the authorized case row until the entire projection
completes; concurrent reassignment/lifecycle writes wait rather than exposing
new private state under an earlier resource relationship.

Assignment never transfers creator identity. Assigned scope permits current
creator or assignee; global scope permits all tenant cases. Reassignment immediately
changes resource access, including receipt recovery for former assignees. Revoking
an assignee does not prevent the still-authorized owner from recording work.

## Investigation lifecycle and evidence

```text
open → in_progress ⇄ waiting → resolved → closed
  ↘          ↘          ↘
             cancelled

resolved / closed → explicit reopen → open in the next cycle
```

Start/resume/wait/close/cancel require reasons. Resolve requires an explicit
1–4,000-character solution. Close is an operator's internal investigation action;
it does not assert customer acceptance, contractual discharge or a sent message.
Cancellation is terminal; reopening resolved/closed work starts a new cycle and
clears current resolution timestamps while retaining prior evidence.

Every revision has immutable before/after state, assignment, priority, due target,
cycle and resolution evidence, plus actor, reason, request UUID and audit. Deferred
integrity checks require complete history/receipt binding before commit. Old
resolutions are never overwritten when the current case is reopened.

Internal notes are append-only findings, separately protected by note permissions.
They do not record an email, customer reply, notification or first-response SLA.
Adding a note increments the case revision even on closed/cancelled records;
it does not reopen them. Notes and transitions/audit expose only the latest 100
items with explicit additional-history flags; the underlying database retains all
immutable records. Archive/history export is separate work.

Priorities are low/normal/high. Due targets are optional explicit instants with
timezone offsets, supported from 2000 through 2099. The backend normalizes time
and evaluates overdue state from the database clock. Only open/in-progress/waiting
cases with an elapsed target are overdue. Waiting never silently pauses a target;
missing targets do not imply an SLA or lateness. Reopening receives its own explicit
new target or none. Target/priority changes need reasons and current revisions;
closed/cancelled metadata cannot silently change.

A bounded native workflow follows the distinction in
[official Frappe ticket documentation](https://docs.frappe.io/helpdesk/ticket)
between investigation, assignment, resolution/reopening and internal findings.
Archava's exact transitions, authority and target semantics above are its own
policy, not that product's business-calendar or customer-communication engine.

## Authority, commands and recovery

Permissions: `cases.read`, `cases.write`, `cases.assign`, `notes.read`, `notes.write`,
`audit.read`. Writes/recovery are human-only; the case reader's current membership,
kind, active state, scope and resource relationship are checked inside every
transaction. Service reads intersect initiating human/service grants and scopes.
Global assignment additionally requires case-write and assignment permission.
The assignment directory is human-only and requires global case-read/assign access.

Each UUID is bound by HMAC to tenant, actor and canonical validated command. Same
UUID/intent commits/replays once; changed payload or another actor is refused.
Recovery rechecks current permission and resource access. Header, notes, full
before/after history, audit and receipt commit atomically. A failure after header
or note insertion rolls them all back and permits safe retry of the same intent.
Caller-supplied ownership, grants, state, SLA-success or delivery flags are refused.

The shared browser keeps an unresolved command in memory, blocks a fresh effect,
and checks or retries the original UUID after response loss. A denied retry after
unknown outcome retains it. Logout/disposal clears private state. Tab-reload
persistence remains separate work. Refresh clears a case the current scope can no
longer read. Reusable dashboard/API rules do not grant new authority to Archava.

Forced RLS supplies transaction tenant isolation, with bounded membership locking
and immutable SQL histories. This is not security against a compromised database
role or host trust root. Private read tools are `support.cases` and `support.detail`;
there is no public visitor, SQL, note-write, send-message or assignment tool.

## Local composition and operation

The owner host additionally checks existing verified identity/CSRF, configured tenant
and active CRM workspace access. Assembly v7 adds optional `support` requiring
contacts in that host; v1–v6 and the existing four-module defaults remain valid.
The planner accepts narrow `support_cases` for K05/K06/K12/K18/K21 only. No complete
operating kit is accepted by generic investigation/assignment capabilities.

With the host stopped, use private operator inputs:

```sh
pnpm crm:build
pnpm crm:setup
pnpm crm:doctor
pnpm crm:modules /absolute/private/modules.json
pnpm crm:user support /absolute/private/support-account.json
pnpm crm:start
```

Modules JSON retains required original flags and adds `support: true`. Account JSON
uses current `username`, configured `tenants`, `access: read|write|none` and
`scope: all|assigned`. The operator records a display label without expanding
identity or workspace access. Flags and migration create no membership automatically.

Base `/api/support/:tenant`: GET `access`, `assignees`, `cases`, `cases/:id`,
`commands/:uuid`; POST `commands`, `tools`. Directory and cases use bounded
pagination; cases support explicit state, priority and overdue filters. Unknown or
repeated query fields, ambiguous local timestamps and streamed bodies over 16 KiB
are refused. Cookie POST needs existing same-origin/CSRF identity. Errors hide
private dependency details.

The shared panel supports intake, filtering, assignment, internal notes, priority/
target changes, investigation transitions, resolution, close/reopen and receipt
reconciliation. The assignee selector shows the first 100 eligible accounts;
larger directories use the paginated API. Membership configuration stays private.

This release also makes owned host/operator/backup PostgreSQL shutdown wait for
actual client removal, including idle sockets already closing after `pool.end()`.
The unit reproduces pg-pool's early end resolution; actual host/restart/backup proofs
retain existing behavior under the new shutdown boundary.

## Verification and remaining scope

`pnpm support:verify` cold-installs identical code into two independent businesses
and exercises real PostgreSQL grants/scopes, assignment/revocation, 20-request
idempotency/revision races, note/history privacy, resolution cycles, database-clock
targets, injected rollback, SQL boundaries, restart/restore and modified-ledger refusal.
`pnpm support:verify-host` uses the actual compiled host/operator and Chromium for
intake, friendly assignment, investigation/notes, response loss/denied retry,
close/reopen, staff/viewer refusal, feature toggles and backup/identity recovery.
All records are fictional and no provider or customer communication is invoked.

Customer portal/email/WhatsApp, attachments, team routing, contractual SLA/business
calendars, automatic escalation, broader historical export, warranty/maintenance
entitlements and sector-specific case rules remain additional work. Cases are not
legal claims, clinical records, repair asset jobs or recruitment applicants. The
[programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md) retains all unfinished requirements.
