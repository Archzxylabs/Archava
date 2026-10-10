# Internal owner pilot — scope and planning progress

**10 October re-audit:** [repository review](PROJECT_REAUDIT_2026-10-10.md) confirms
the delivered local owner scope and refreshes the separate verification evidence.
The 50% baseline and broader 60% planning estimate below retain their declared
denominator; this audit does not recalculate them as whole-product progress.
Current next work is a supported local release profile and a shared presence
boundary, with production/general AI and new business modules scoped separately.

Baseline: 9 October 2026, source `83b65a8`. Estimated delivery coverage for the
bounded internal owner pilot is **50%**. This is a planning estimate, not measured
productivity, revenue, percentage of tests, or a claim of release readiness.

The denominator is reusable core, the existing CRM domain, trusted identity,
approved actions, bounded AI operation and reliable hosting. It excludes the
separate ARCHZXY company website/sales-delivery application, every future ERP
vertical, and the broader customer Voice/Human/commercial release. The historical
30% v1 score in `DELIVERY_PROGRESS.md` uses a different denominator and is unchanged.

| Internal capability                                 | Weight | Coverage | Weighted points | Evidence and remaining gap                                                                                                                                        |
| --------------------------------------------------- | -----: | -------: | --------------: | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Reusable core and configuration                     |     20 |       75 |              15 | Independent cold-installed consumers, explicit public packages and configuration; supported production profiles/upgrade matrix remain open.                       |
| Contacts, activities, follow-ups and sales pipeline |     20 |       70 |              14 | Shared persistent domain/API/dashboard, two business contexts, migrations/audit/receipt recovery; broader domain and release evidence remain open.                |
| Trusted identity, tenant/resource permissions       |     20 |       60 |              12 | Local owner/staff/viewer sessions, RLS, revocation, CSRF and account recovery; production identity/TLS/enrollment remain open.                                    |
| Approved action execution and recovery              |     20 |       15 |               3 | Policy/confirmation/executor ports and durable provider-specific stores; atomic host intent, deployed approval flow and authoritative reconciliation remain open. |
| AI operational interface                            |     10 |       30 |               3 | Local read/draft commands, thirteen synthetic Gemini cases, persisted budget; general planning quality and approved private-data policy remain open.              |
| Reliable operation and diagnostics                  |     10 |       30 |               3 | Local health, backup restoration and safe provider metadata; deployment, automated monitoring/backup drills and incident workflow remain open.                    |
| Total                                               |    100 |          |    **50 / 100** | Internal pilot remains partial.                                                                                                                                   |

Calculation: weight × coverage / 100. Update a line only after its operator
capability is implemented and verified; a passing isolated mock does not establish
live integration. Preserve the baseline and add dated checkpoints with evidence.

Current work order follows PRD §49: durable atomic intent/confirmation/recovery,
public composition proof, a bounded native approved action, supported host
configuration and operational checks. Keep existing behavior/tests. New business
modules require concrete domain scope and support ownership; the library is not
an infinite checklist of industries to implement at once.

9 October follow-up: atomic durable Act intents, positive read recovery and
installed full-path PostgreSQL composition are implemented. See
[the operational evidence and remaining owner integration](DURABLE_ACT_OPERATIONS.md).
The 50% baseline stays unchanged: isolated infrastructure evidence does not yet
complete the owner-facing approval capability.

## Local release checkpoint — 9 October 2026

The owner selected **local first, free** as the completion target. The six local
acceptance capabilities are delivered: shared core/CRM consumers, CRM CRUD and
pipeline, trusted identity, a bounded native approval action, bounded AI/read-draft
operation, diagnostics and tested backup/restore. See [the exact supported local
pilot](LOCAL_OWNER_PILOT.md). This completes that local acceptance, not the full
commercial/presence/ERP roadmap.

Preserve the original 50% planning baseline above. Broader owner roadmap coverage
is now estimated at **60%** on the same weights: actions advance to 60% (12 points)
after native UI/HTTP/current-grants/recovery/restore proof; operations advance to
40% (4 points) after diagnostics and local release checks. The other weighted
contributions stay 15 + 14 + 12 + 3. Total = 15 + 14 + 12 + 12 + 3 + 4 = 60.
This still includes open production/support/general-planning work, so it is a
different assessment from the completed local release acceptance.

9 October local follow-up: the [0.5.0 CRM worklist](CRM_WORKLIST.md) exposes open,
overdue and upcoming tasks across authorized contacts in both the dashboard and
local Archava. It reuses existing permissions/data and adds no action authority
or infrastructure. The broader 60% estimate is retained; this is a useful local
CRM increment rather than evidence for production/general AI readiness.

9 October follow-up: [reviewed task completion](CRM_COMPLETION.md) adds a second
bounded native action through the existing approved execution path. Exact task
reads and selection remain read-only; final completion uses current delegated
grants and a separate action-bound challenge. The broader 60% estimate remains
unchanged pending production/general-planning evidence.

9 October follow-up: the [0.7.0 authorized CRM overview](CRM_OVERVIEW.md) gives
dashboard and local Archava the same complete contact/task/pipeline aggregates.
Unavailable sections remain distinct from permitted zero counts; current tenant,
assigned and delegated read grants govern every refresh. The broader 60% planning
estimate remains unchanged.

9 October follow-up: [automatic local backups](CRM_BACKUPS.md) add an opt-in
per-database host schedule, integrity diagnostics and real restoration of an
automatic archive. Existing library/action behavior is unchanged. Unattended OS
operation, off-machine storage, retention and production reliability remain open,
so the broader 60% planning estimate is retained.
