# Support-case implementation plan

KIT-07 follows accepted KIT-06 scopes while preserving all full-kit gaps. The next
shared capability is `support_cases`: real service issues rather than CRM follow-up
tasks. Independent property-operations and professional-service consumers can share
case identity, internal notes, assignment, investigation, resolution and reopening.
Their sector extensions remain separate. A case does not establish tenancy/property
accounting, legal case management, clinical treatment or a customer communication.

[Official Frappe ticket documentation](https://docs.frappe.io/helpdesk/ticket)
describes assignment, investigation, resolution/reopening and internal findings.
It distinguishes optional response/resolution targets from tickets with no SLA.
The following Archava policy is our own bounded implementation proposal, not a
claim to implement that product or its business-calendar SLA engine.

The pure module should own strict commands/API/tools and a reusable owner panel;
its PostgreSQL adapter should own independent memberships, case headers, immutable
notes/transitions, receipts and audit under forced tenant RLS. No mandatory CRM/AI
or external helpdesk service is needed for the native local workflow.

States: open → in_progress → waiting/resolved → closed. Explicit reasons are
required for reopening, reassignment and cancellation; resolved and closed must be
distinct. Reopening starts another investigation cycle without erasing the prior
resolution. A case note records an internal finding, never a sent customer reply.
No timestamp or model flag proves external delivery or a contractual SLA response.

Owner/assignee access must follow current independent domain membership and scope;
assignment targets must be active human case readers in the same tenant. Global
assignment/configuration needs explicit permissions. Machine reads intersect the
initiating human and service grants; writes/recovery remain human-only until a
separate approved action contract exists. Unknown role, tenant, actor, grant and
state overrides must fail before persistence.

Internal due targets may be explicit instants with timezone offsets. No default
paid SLA, business-calendar pause or automatic support entitlement should be
invented. A due report distinguishes an outstanding target from completed work and
from a missing target. Warranty, recurring support and scope changes retain their
own commercial contracts.

Acceptance requires two cold-installed business consumers, real PostgreSQL
assignment/grant and revision races, duplicate-command recovery, immutable history,
reopening, scope refusal and restart/fresh restore; actual compiled host/Chromium
proof, disable/re-enable and original owner preservation. The full operating kit
catalogue keeps outstanding sector, financial, communication and adapter evidence.
