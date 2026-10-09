# Follow-up worklist — local owner operation

Checkpoint: 9 October 2026, baseline `af9ae4b`. The existing reusable CRM gains
a bounded read projection across contacts. CRM, PostgreSQL and conversation
artifacts are 0.5.0; the native approval adapter is 0.1.1 with the updated CRM
dependency. This adds no package, database migration, grant or external provider.

## Use

Open `http://127.0.0.1:4175`, sign in and choose a workspace. **Daftar kerja
follow-up** lists open tasks, earliest due first. Choose **Terlambat** or
**Mendatang**, search by task text/contact name, and use **Buka kontak** to reach
the existing contact details. Complete tasks through the existing human dashboard.
**Perbarui daftar** reads current data; the list does not claim to update itself.

Archava understands these local commands:

- `tampilkan follow-up`
- `tampilkan follow-up terlambat`
- `tampilkan follow-up mendatang`

Results contain the same authorized tasks as the dashboard. Selecting a contact
from a conversation result prepares the existing `ringkas` command; it does not
execute a write. Counts refer to the displayed page, not all tasks in the business.
Gemini keeps its exact synthetic-only Free pilot; these new commands run locally.

## Reusable contract

`CrmRepository.listFollowups` is optional. An older adapter can continue its
existing operations; requesting this unsupported projection returns unavailable.
`CrmService.listFollowups` validates both query and bounded result. The public
`@archava/crm/worklist-contracts` export is browser-safe; the standalone
`@archava/crm/worklist-dashboard` component accepts the host transport and contact
navigation callback. Business-specific templates do not fork this implementation.

```http
GET /api/crm/:tenant/followups?bucket=overdue&limit=20
Cache-Control: no-store
```

Query fields are `bucket` (`open`, `overdue`, `upcoming`), `search` (up to 120
characters), `limit` (1–100, default 20), optional `asOf` and optional paired
`afterDueAt`/`afterId`. The response contains `asOf`, projected items and an
optional `next` cursor. Clients carry the returned time and cursor into the next
page. Contact name and task summary search treats SQL wildcard characters as
literal text. Dates are UTC on the API and local time in the browser.

All buckets exclude completed tasks. Overdue means `dueAt < asOf`; upcoming
includes the exact boundary `dueAt >= asOf`. The initial timestamp comes from
the server; conversation uses the host turn timestamp. Due-time/UUID ordering
retains database microseconds in the cursor, so adjacent times and tied times
can be paged without losing a task. A fixed boundary is not a database snapshot:
completion, reassignment and new data can change later pages. Refresh restarts
from current data and current server time.

## Authority and recovery

Both `contacts.read` and `followups.read` are required inside the existing
membership transaction. Assigned staff can read only tasks linked to their
currently assigned contacts. A delegated service uses the intersection of its
current grants and the initiating human's grants/scope. Tenant RLS and composite
contact joins apply before pagination. Revocation and reassignment are checked
again on every request; no stale browser permission grants access.

The `crm.followups.list` tool is read-only. It creates no command, task or
mutation audit, and does not add native action authority. Owner approval for
`crm.followup.create`, command receipt recovery and restored execution holds are
unchanged. Queue refresh clears previous displayed rows before a new request;
failure does not leave those rows presented as fresh results. Logout/workspace
change aborts and destroys the component; late replies cannot repopulate it.

This is a CRM task worklist. Delivery capacity/project scheduling, automated
messages and a cross-device action-recovery inbox remain separately scoped work.

## Verification

Run the existing root gates, `pnpm crm:verify`, `pnpm crm:verify-runtime -- --browser`
and `pnpm e2e:crm-runtime`. Unit checks exercise strict input/output and old-adapter
compatibility. Two cold-installed consumers exercise actual PostgreSQL due buckets,
microsecond/tied-time pagination, permissions, delegated scope, hostile searches
and zero command/audit/task writes. Authenticated HTTP and Chromium cover the
installed widget, filters, navigation, literal record text and owner/staff isolation.
The [release validation](audit/2026-10-09-worklist-validation.json) records all
eight root gates, 1,426 tests in 96 files, and 24 installed artifact versions/hashes.

- [CRM database proof](audit/2026-10-09-worklist-crm-proof.json): 33 checks,
  including both installed consumers and the new worklist boundary.
- [Act regression](audit/2026-10-09-worklist-act-proof.json): 14 checks,
  preserving native one-time approval, recovery and restoration hold.
- [Runtime proof](audit/2026-10-09-worklist-runtime-proof.json): 31 checks,
  including authenticated worklist/conversation and actual backup/restore.
- [Chromium proof](audit/2026-10-09-worklist-browser-proof.json): 54 checks,
  including task buckets, literal text, navigation, staff scope and mobile use.
- [Original installation](audit/2026-10-09-worklist-installed-proof.json):
  protected domain/identity/approval/provider rows and private configuration
  preserved; old credentials, both workspace reads and local conversation work.

Installed host artifacts match the artifacts checked by both consumers. All test
fixtures are separate and synthetic. The original installation smoke creates zero
domain commands and makes zero model or external action calls.
