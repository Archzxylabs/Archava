# CRM conversation verification — 8 October 2026

Baseline: local main checkpoint `78171b3`, preserving the persistent identity/CRM
host. This change adds the optional `@archava/crm-assistant` adapter, shared panel
and a trusted live-routing input to the existing assistant pipeline. No company
website, universal ERP, vendor SDK or machine-write path was added.

## Verified behavior

- List/search authorized contacts, resolve an explicit contact and show permitted
  activity/follow-up data through `StructuredTruthPort` and `runTurn`.
- Reject caller identity/role/tenant injection, malformed provider plans,
  anonymous/CSRF-less requests and forbidden tenant/resource access.
- Ask for contact selection when search is ambiguous/truncated; require explicit
  draft content/time and detect a contact revision change during preparation.
- Keep stored HTML/prompt injection as inert text. The planner receives only the
  bounded user utterance; private CRM records never return to planning.
- Keep drafts transient. SQL snapshots show unchanged contacts, activities,
  follow-ups, command receipts and mutation audit counts across conversation
  read/prepare/unsupported-send requests.
- Clear conversation, selection and draft fields on business switch/logout;
  abort in-flight work and discard late results from the former business.

## Evidence

| Check                                     | Result                                                                                                              |
| ----------------------------------------- | ------------------------------------------------------------------------------------------------------------------- |
| `pnpm verify`                             | All eight gates pass; 1,337 Vitest tests across 86 files.                                                           |
| `pnpm reuse:verify`, included above       | Two independent core consumers; identical compiled artifacts.                                                       |
| `pnpm crm:verify`                         | 13 PostgreSQL checks; hospitality/professional-service consumers cold-install matching CRM/adapter implementations. |
| `pnpm crm:verify-runtime`                 | 14 authenticated persistent-host checks, including actual backup/restore and business-table non-mutation.           |
| `pnpm e2e:crm-runtime`                    | 25 browser checks; includes stored HTML and delayed-response business switching.                                    |
| Root dependency audit                     | Zero vulnerabilities at every severity; no external dependency added.                                               |
| Node / database                           | Node 22.22.3 / PostgreSQL 18.6, private disposable local clusters.                                                  |
| Language-model or business-provider calls | Zero.                                                                                                               |

Machine-readable results and artifact hashes are preserved in
[conversation proof](audit/2026-10-08-crm-conversation-proof.json). The artifact set
contains 13 compiled internal packages. Existing pricing/template bytes and the
original PRD archive are unchanged. The final browser run deliberately aborts one
old-business conversation request; only that exact endpoint/error is accepted by
its otherwise strict resource-error check. CI definitions include the CRM checks,
but no remote CI run or deployment was performed.

Two verifiers initially collided because both rebuild `dist/library`. The final
independent-consumer and runtime checks were run sequentially and passed. This
local tooling directory must have one builder at a time.

Browser screenshots are local diagnostics under
`/tmp/archava-crm-runtime-browser/shots/`; the shared panel was inspected at 360px.
Tests use their private fixture, not the operator's `.local/crm` records.

## Remaining gates

The shipped planner is a deterministic local command parser. Live model choice,
data terms, prompt/tool evaluation and failure budgets remain work before a live
planner is enabled. Draft persistence/sending require the verified Act approval,
idempotency, durable execution and recovery boundary. Public hosting, telemetry,
automatic backup operations and shared voice/avatar presence remain open. The
three Act operational-readiness tasks stay Pending, and the historical commercial
readiness estimate is not recalculated from this slice.

See [CRM conversation](CRM_CONVERSATION.md) for usage and boundary contracts.
