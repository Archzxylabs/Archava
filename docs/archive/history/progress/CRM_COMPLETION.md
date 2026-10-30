# Reviewed follow-up completion — local owner operation

Checkpoint: 9 October 2026, baseline `b869cc9`. CRM/domain/assistant are 0.6.0;
the native adapter is 0.2.0. Action registry 1.2.0 adds a separate bounded
`crm.followup.complete` action. Existing creation, human dashboard operations,
receipt recovery and private restoration controls remain supported.

## User flow

At `http://127.0.0.1:4175`, select **Tinjau penyelesaian di Archava** on a task
in the worklist. Alternatively ask `tampilkan follow-up`, then select **Pilih
tugas untuk Archava**. Selection fills `tinjau penyelesaian follow-up`; it does
not call the execution route. Submit that request to read the selected task.

The draft shows the contact, task text and schedule. **Tinjau penyelesaian**
rechecks the current task and issues one-time consent. **Konfirmasi selesai**
marks exactly that task completed. Only its authoritative receipt makes the card
show **Selesai**. The worklist refreshes after confirmed completion. An already
completed task cannot be prepared again; another selection is required.

`selesaikan follow-up` is also a local preparation command. It requires an
explicit selected task; a selected contact alone never identifies a task.
Conversation/selection does not execute a command or mark work completed.
Gemini retains the exact synthetic-only Free pilot. No task content is sent to it.

## Contracts and authority

`CrmRepository.followupDetail` is an optional read capability. The public
`CrmService.followup` and `GET /api/crm/:tenant/followups/:id` validate the exact
task/tenant/contact association. This does not depend on the twenty tasks shown
in a contact's detail page. Older adapters remain compatible with their existing
operations; unsupported exact-task reads refuse as unavailable.

The read requires `contacts.read` and `followups.read`, current membership and
the contact's assigned scope. `crm.followup.complete.prepare` also requires the
current `followups.write` grant and an open task. It returns an approval draft;
it does not mint consent, update a task or create command/audit rows.

```http
POST /api/crm/:tenant/actions/followup/complete/review
POST /api/crm/:tenant/actions/followup/complete/present
GET  /api/crm/:tenant/commands/:intentId
```

Review inputs are exactly `{ followupId, expectedRevision }`. Execution requires
Act capability, the registered role, exact input/entity validation, verified
one-time consent, the active durable intent claim and the current private approval
epoch. A task-creation challenge cannot authorize completion. The native adapter
verifies the actual receipt MAC/action/input binding and consumed scoped database
challenge inside the domain transaction.

Both service and initiating human grants remain locked through commit. The task
and contact are locked before completion, so a resource reassignment cannot slip
between its authorization check and the write. Current task revision and open
status are checked again in that transaction. Status, revision, command receipt
and the audit with service actor/human initiator commit together.

Only two native operations are accepted: follow-up creation and completion.
Ordinary machine `mutate`, model confidence, browser approval/role fields and
other CRM operations confer no authority. This adds no permission grant,
database migration, dependency, provider or infrastructure.

## Response loss and restore

Before presenting consent, the panel persists only the opaque intent UUID in its
existing account/workspace-scoped tab recovery store. Failure to persist it stops
execution. Creation and completion share that unresolved-operation gate.

Once presented, a lost response is checked with GET under the original UUID.
There is no blind POST retry. A displayed completion card additionally matches
the receipt's task ID and resulting revision. After reload, the backend matches
the original service actor/human initiator and current resource read grants.
An absent/malformed receipt stays unconfirmed and does not permit another action.

Logout/workspace change destroys the UI and ignores late responses. Closing or
clearing the tab still loses its marker; a cross-device recovery inbox is separate
work. Restored copies retain the private execution hold and fresh approval epoch.
Explicit operator resume does not revive old completion consent or release the
provider/Gemini restoration hold. See [local operation](LOCAL_OWNER_PILOT.md).

## Verification

Root gates and the existing CRM/Act/runtime/browser verifiers remain acceptance
checks. Tests use separate synthetic databases. New cases exercise exact task
reads beyond the contact page, read-only preparation, distinct action binding,
concurrent completion, stale/closed tasks, revocation, assigned-resource changes,
receipt recovery after real restart and restoration controls. Authenticated HTTP
and Chromium exercise the actual installed routes and shared owner/staff UI.

- [Root validation](audit/2026-10-09-completion-validation.json): all eight gates,
  1,431 tests in 97 files, and 24 compiled package artifacts.
- [CRM proof](audit/2026-10-09-completion-crm-proof.json): 34 checks, including
  matching package artifacts installed by two independent consumers.
- [Native action proof](audit/2026-10-09-completion-act-proof.json): 18 checks
  against real PostgreSQL, including concurrency, restart and restore.
- [Runtime proof](audit/2026-10-09-completion-runtime-proof.json): 32 checks
  against the authenticated host and restored database.
- [Browser proof](audit/2026-10-09-completion-browser-proof.json): 61 Chromium
  checks, including owner/staff completion and GET-only recovery.
- [Installed pilot proof](audit/2026-10-09-completion-installed-proof.json):
  original domain, identity, approval and provider rows and private settings
  preserved; authenticated read-only smoke passed with no domain command or
  actual model/external action call.
