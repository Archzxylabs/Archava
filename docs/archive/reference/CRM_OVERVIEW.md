# Authorized CRM overview — shared local operation

<!-- documentation-status -->
> **Status: legacy platform contract reference.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../ARCHITECTURE.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Checkpoint: 9 October 2026, baseline `0c8d444`. CRM, PostgreSQL adapter and CRM
assistant artifacts are 0.7.0. The shared overview adds no dependency, database
migration, permission grant, financial calculation or external provider.

## Use it

At `http://127.0.0.1:4175`, **Kondisi CRM** shows the complete authorized contact
count, open/overdue/upcoming/completed follow-ups and opportunities by canonical
stage. **Perbarui ringkasan** fetches current totals after dashboard changes.
Confirmed Archava follow-up completion also refreshes the overview. Other
dashboard changes require refresh; the displayed time makes the read explicit.

Ask **ringkas kondisi CRM**, **ringkas CRM** or **CRM overview** for the same
structured totals in Archava. No contact/task selection is required. This command
runs locally even when the synthetic Gemini allowance is held or exhausted. CRM
records and summary data are not sent to Gemini.

The overview counts all permitted records, beyond list-page limits. It contains
no contact names, emails, task content, opportunity titles, owners or financial
values. Won/lost remain sales-stage counts; neither establishes a contract,
booking, payment or project status.

## Public contract and authority

- `@archava/crm/overview-contracts`: strict aggregate schemas and types.
- `@archava/crm/overview-dashboard`: shared renderer and optional host component.
- `CrmService.overview`: validated tenant/principal, host-owned due-time cutoff
  and verified aggregate response.
- `GET /api/crm/:tenant/overview`: authenticated, read-only, `no-store` response.
- `crm.overview`: read-only tool with an exact empty input object.

`CrmRepository.overview` is optional, preserving old adapter compatibility. An
adapter without the capability refuses the overview as unavailable while its
existing operations continue. The native Act adapter stays 0.2.0 with the same
two bounded creation/completion actions and consent/recovery controls.

Current active membership and `contacts.read` are required. Follow-up aggregates
require `followups.read`; pipeline aggregates require `pipeline.read`. A missing
section grant produces `null`, rendered as unavailable. A permitted empty section
contains zero counts. Neither dashboard nor Archava substitutes zero for a
restricted section.

Assigned scope filters contacts by the current human owner. Follow-ups inherit
that contact scope; opportunities must also satisfy their own owner scope. Service
reads intersect current service/human grants and scope. Resource reassignment or
revocation takes effect on the next read. Public identities/other tenants receive
no aggregate access. Query/body scope, permission, identity, time and SQL fields
are rejected; they cannot widen access.

## Deterministic read and UI lifecycle

PostgreSQL computes all counters in one parameterized SELECT under the existing
membership-lock transaction and forced tenant RLS. Its single statement snapshot
keeps contact/task/pipeline totals coherent. All task buckets use one host-provided
UTC cutoff: open due dates strictly before it are overdue, dates at or after it
are upcoming. PostgreSQL retains microseconds for that comparison.

`asOf` is the due-time classification boundary, not a historical database query.
The overview is a current read, not a forecast. Strict safe-integer/nonnegative
validation checks open = overdue + upcoming, active = new + qualified + proposal,
and total = active + won + lost. Output tenant and cutoff must match the request.

Dashboard refresh clears old counters before reading. An error or changed grant
leaves no previous numbers visible. Workspace change/logout destroys the component,
aborts its request and ignores delayed responses. The dashboard and conversation
reuse the same text-only renderer and configured canonical-stage labels. No
records, counts or credentials are persisted by this component.

## Verification

The standard root, CRM, native Act, runtime and browser verifiers remain acceptance
checks. Dedicated real PostgreSQL fixtures prove complete aggregates beyond page
limits, microsecond cutoff boundaries, stage totals, empty/restricted sections,
assigned/delegated reads, current revocation/reassignment and read-only behavior
through two independent installed business consumers. Browser cases exercise
shared rendering, manual/confirmed-completion refresh, failed refresh, delayed
response isolation, staff scope and logout cleanup. Synthetic verifier writes
stay in disposable databases; the installed local pilot receives read-only smoke.

- [Root validation](../audit/2026-10-09-overview-validation.json): all eight gates,
  1,438 tests in 98 files and 24 compiled artifacts.
- [CRM proof](../audit/2026-10-09-overview-crm-proof.json): 38 checks across two
  independent installed consumers with matching package versions/hashes.
- [Native Act regression](../audit/2026-10-09-overview-act-proof.json): 18 checks,
  preserving separately approved creation/completion and recovery.
- [Runtime proof](../audit/2026-10-09-overview-runtime-proof.json): 33 checks,
  including restored owner/staff overview and zero provider calls.
- [Browser proof](../audit/2026-10-09-overview-browser-proof.json): 70 Chromium
  checks, including failed/delayed reads, current counts and scope cleanup.
- [Installed pilot proof](../audit/2026-10-09-overview-installed-proof.json):
  original protected rows and private files preserved; authenticated read-only
  smoke passed in both workspaces using existing credentials.

See [existing completion controls](../history/progress/CRM_COMPLETION.md),
[local release scope](../history/progress/LOCAL_OWNER_PILOT.md) and
[the planning denominator](../history/progress/INTERNAL_OWNER_PROGRESS.md). The broader owner estimate
remains 60%; this addition does not establish production/general-AI readiness.
