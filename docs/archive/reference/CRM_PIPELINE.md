# Reusable sales pipeline — controlled local slice

<!-- documentation-status -->
> **Status: legacy platform contract reference.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../ARCHITECTURE.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

`crm_sales_pipeline` extends `crm_contacts` in the existing CRM domain. It ships as
public subpaths of `@archava/crm` and uses `@archava/crm-pg`; both internal artifacts
are version **0.2.0**. One repository owns contacts, opportunities, authorization,
command receipts and audit transactions. No new service, framework, queue or
dependency was introduced. The ARCHZXY company website remains a separate consumer.

The two independent consumers represent a hospitality group-stay inquiry and a
professional-service engagement. They install identical compiled artifacts outside
workspace aliases, build the same dashboard, and operate on separate tenant data.
This proves reuse of an inquiry/opportunity workflow. It does not establish support
for room availability, booking, project delivery, invoicing or every industry rule.

## Domain and configuration

An opportunity references one authorized contact, inherits its owner, and records
title, canonical stage, revision and timestamps. A contact can have multiple
opportunities. Creating an opportunity checks the contact's current revision and
does not change the contact itself. There is no amount, probability, forecast,
automatic follow-up, source assignment, contract or payment field in this slice.

| Current stage | Allowed next stages        |
| ------------- | -------------------------- |
| `new`         | `qualified`, `lost`        |
| `qualified`   | `new`, `proposal`, `lost`  |
| `proposal`    | `qualified`, `won`, `lost` |
| `won`         | Terminal                   |
| `lost`        | Terminal                   |

`lost` requires a reason. Reopening terminal opportunities is unsupported. Every
creation and move appends a history entry with actor, request id, from/to stage,
reason, revision and time. Detail returns the latest 20 entries; the database
retains the full history. Lists use bounded keyset pages and optional contact,
stage and title filters. UI pagination and stage filtering use the same API.
Detail reads hold a short shared opportunity lock so a concurrent stage move cannot
produce an older stage alongside a newer history entry in the same response.

Tenant configuration can supply a strict `pipelineLabels` map containing exactly
the five canonical keys. Hospitality can display “Offer” where professional
services displays “Proposal”; both keep the same transition rules. Labels grant
no permission and cannot redefine an outcome. **Won records a sales outcome.**
Booking, contract, payment and project statuses remain owned by their systems.

## Public boundaries

| Export / route                           | Purpose                                                   |
| ---------------------------------------- | --------------------------------------------------------- |
| `@archava/crm/pipeline`                  | Service, module metadata, strict contracts and read tools |
| `@archava/crm/pipeline-contracts`        | Browser-safe schemas, labels and stage rules              |
| `@archava/crm/pipeline-dashboard`        | Shared human-operated DOM component                       |
| `GET /api/crm/:tenant/opportunities`     | Authorized paginated list                                 |
| `GET /api/crm/:tenant/opportunities/:id` | Opportunity and recent history                            |
| `POST /api/crm/:tenant/commands`         | `opportunity.create` / `opportunity.move` intents         |
| `POST /api/crm/:tenant/tools`            | `crm.opportunities.list` / `crm.opportunity.summary`      |

Hosts supply trusted principals and optionally pass `CrmPipelineService` as the
third argument of `createCrmApi`. The dashboard accepts an injected session/CSRF
transport and receives an explicitly selected contact from the CRM dashboard.
Destroying it aborts requests and clears records, selections and pending UUIDs.
All record and label text renders through DOM text properties.

Pipeline reads require both `pipeline.read` and `contacts.read`. Mutations require
`pipeline.write` and authorized contact access. Membership kind, tenant, assigned
scope, delegation intersection and active status remain server-side checks.
Clients cannot supply ownership or grants. Runtime PostgreSQL uses a non-owner
role without superuser/BYPASSRLS and forced tenant RLS on both new tables.

Mutations atomically claim the shared CRM request UUID, validate current resource
access and revision, write the opportunity/history/audit, and save the receipt.
An identical retry returns the original receipt; changed intent or actor conflicts.
Replay rechecks current grants and resource access. A failed audit rolls back the
entire transaction. History rejects updates/deletes. Financial truth is untouched.

The two pipeline tools are **read-only** and omit contact email, organization and
owner fields. Their free-text output still requires untrusted-data handling by any
model adapter. The existing local conversation planner continues to support its
documented contact/follow-up intents; natural-language pipeline intents and live
planner evaluation have not shipped. Machine pipeline writes remain refused until
the corresponding Act boundary has verified approval, durable execution and recovery.

## Upgrade, access and rollback

Migration `001_crm_contacts.sql` remains byte-for-byte unchanged. Additive migration
`002_sales_pipeline.sql` introduces permission names, tables, tenant/contact/owner
foreign keys, indexes and immutable history. Migration tooling validates an ordered
checksum prefix and applies missing migrations in one transaction. The runtime
requires the complete known two-migration ledger and eight forced tenant tables.

Before upgrading, back up with the old running installation, then stop the host:

```sh
pnpm crm:backup
# Stop the running host gracefully before rebuilding its generated artifacts.
pnpm crm:build
pnpm crm:setup
```

Setup preserves existing account grants. Fresh owner/staff/viewer presets include
appropriate pipeline permissions. Existing accounts require an explicit operator
change using an owned mode-0600 JSON file:

```json
{
  "username": "owner@archava.local",
  "tenants": ["hotel-demo", "studio-demo"],
  "access": "write"
}
```

```sh
pnpm crm:user pipeline /absolute/private/pipeline-access.json
pnpm crm:start
```

`access` is `read`, `write` or `none`. The command requires an active human account
with existing active contact membership in each configured tenant, preserves all
other grants and scopes, and commits the selected tenant changes atomically. This
is operator-only; public API callers cannot grant themselves access. Older profiles
without `pipelineLabels` remain valid and use the default stage labels.

The 0.1.0 repository rejects a two-migration database. **Code rollback alone is not
supported.** Preserve the pre-upgrade backup plus its private runtime metadata and
use an appropriate restore procedure for the target code/schema. Existing restore
commands create a new database, rotate identity keys and require explicit account
recovery. Never discard the original database or fingerprint key to make a downgrade
appear to work. Production upgrade scheduling and retention remain deployment work.

## Verification

The [9 October checkpoint](../audit/CRM_PIPELINE_2026-10-09.md) records the completed local
gates and preservation checks for the existing installation.

```sh
pnpm verify
pnpm crm:verify
pnpm crm:verify-runtime -- --browser
pnpm e2e:crm-runtime
```

Run artifact builders sequentially: they share generated `dist/library` output.
The CRM verifier uses a disposable real PostgreSQL cluster and two cold-installed
consumers. It exercises stage races, retries, invalid transitions, missing reasons,
cross-tenant/assigned access, separate grants, delegation, machine-write refusal,
atomic rollback, immutable history, pagination, legacy upgrade and restart. Runtime
checks exercise credential/CSRF boundaries, explicit grants, restart and actual
`pg_dump`/`pg_restore`. Browser checks cover contact selection, configured labels,
stage history, lost-response retry, tenant/logout cleanup and staff isolation.

Local evidence does not close broader M3, public deployment, telemetry/alerting,
provider integration, support readiness or commercial pricing gates. No runtime
pricebook/catalog values or Pending Act readiness files are changed by this slice.
