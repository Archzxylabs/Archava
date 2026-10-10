# Reusable project delivery module

`@archava/projects` and `@archava/projects-pg` add internal project, task and
milestone tracking. The PostgreSQL module owns `archava_projects`; it has no CRM,
LLM, payment or signature dependency. The local owner host composes it with its
existing identity and CRM workspace membership.

This is a bounded delivery slice, not a complete agency or construction system.
Proposal negotiation, client signature, deposit verification, capacity planning,
change-order pricing, invoicing and warranty/maintenance remain separate work.
An internal owner confirming scope is not a client's contractual acceptance.

## Behaviour

- Projects move from draft to active, review, delivered and closed. Review can
  return to active. Draft/active/review can be cancelled with a recorded reason.
- Activation records who confirmed the internal scope. The original scope,
  owner and confirmation cannot be rewritten through this API or the runtime's
  SQL privileges. Additional scope needs a future explicit change-order contract.
- Tasks use todo, doing, done and cancelled. A cancelled required task still
  blocks delivery. Milestones require recorded evidence and at least one linked
  task; every linked required task must be done before completion.
- Delivery requires at least one required task, every required task done and
  every required milestone reached. Reached milestone work cannot reopen;
  delivered, closed and cancelled projects refuse additional child changes.
- Every child mutation checks and advances the parent project revision. Competing
  edits fail with a conflict rather than silently overwriting work.
- Each project supports at most 100 tasks and 100 milestones. Detail responses
  include these bounded collections and the latest 50 audit entries, with an
  explicit `auditHasMore` flag. All earlier audit entries remain stored.

Due dates are accepted as ISO timestamps by project/task/milestone creation APIs.
The first dashboard provides the basic creation and state workflow; deadline
editing, task assignment and risk/capacity reporting are not implemented.

## Authority and persistence

The host authenticates the person and binds the tenant. Request input cannot
supply permissions, an owner identity or a tenant override. Project-domain
membership is separate from CRM membership: enabling the module grants nobody
access. Owner-host requests require both current workspace and project access.

Permissions are `projects.read/write`, `tasks.read/write`,
`milestones.read/write` and `audit.read`. Assigned scope follows project ownership;
tasks and milestones inherit that scope. Service reads intersect the service's
grants with its human initiator's current grants. Machine writes are refused.

The repository locks current memberships, uses tenant-local transactions and
forces RLS on all six domain tables. Runtime roles cannot own the tables, have
superuser/BYPASSRLS, edit memberships or remove audits. The database credential
belongs to the trusted server; it is not an end-user credential.

A UUID command, actor-bound HMAC fingerprint, business mutation, audit entry and
receipt commit atomically. Repeating the same UUID and payload returns the same
receipt after current authorization. Different actors or payloads cannot reuse
it. Committed receipts and audit entries are immutable.

`GET /api/projects/:tenant/commands/:requestId` checks the current person's original
receipt without repeating a write. `not_committed` is an observation, not proof
that an in-flight request failed. Database restart and backup restoration retain
receipts, grants and revisions when the fingerprint key is retained.

The dashboard holds an uncertain write and offers Check pending result / Retry
same request. It retains the body only in memory. Reload recovery is not yet a
durable dashboard feature; callers needing it must retain the original request
ID and use the read-only receipt endpoint. A reload must not be treated as proof
that a write failed. The older CRM follow-up panel has its separate tab recovery.

## Public integration surface

The pure package exports `ProjectService`, strict schemas, `createProjectApi`,
`createProjectTools` and `@archava/projects/dashboard`. The server package exports
`PostgresProjectRepository`, explicit owner migrations and runtime grants.

Routes under `/api/projects/:tenant`:

| Route                  | Method | Result                                                    |
| ---------------------- | ------ | --------------------------------------------------------- |
| `/access`              | GET    | Current effective permissions and ownership scope         |
| `/records`             | GET    | Bounded project page, search/state filter and UUID cursor |
| `/records/:id`         | GET    | Project and permitted tasks/milestones/audit              |
| `/commands`            | POST   | Strict human mutation and authoritative receipt           |
| `/commands/:requestId` | GET    | Original command outcome, with current access             |
| `/tools`               | POST   | Scoped read tool invocation                               |

Read tools are `projects.list` and `projects.detail`. They are ready for a host to
register with Archava; the existing CRM conversation planner does not yet route
project-language requests to them. There is no public visitor project tool or
registered project write tool. Cookie deployments must add trusted session/CSRF
transport; the module API also checks origins and bounds body size to 16 KiB.

## Enable in the local owner host

Use Node 22.22.3 and pnpm 10.15.1. Stop the host before rebuilding, upgrading or
changing composition. Keep a verified backup before applying additive migrations:

```bash
pnpm crm:backup
pnpm crm:build
pnpm crm:setup
```

Setup adds the owned project schema and runtime privileges. It does not grant
existing accounts project access. Preserve existing private settings and keys.

Prepare a privately owned mode-0600 JSON file under the ignored `.local` directory:

```json
{
  "username": "owner@archava.local",
  "tenants": ["studio-demo"],
  "access": "write",
  "scope": "all"
}
```

Run `pnpm crm:user projects .local/crm/project-grant.json`. Supported access is
`read`, `write` or `none`; scope must explicitly be `all` or `assigned`. Enrollment
requires an active identity and workspace membership. It changes only the named
project-domain grants.

Use a separate private module-selection JSON with the existing four choices plus
`"projects": true`, then run `pnpm crm:modules .local/crm/modules.json` and restart
with `pnpm crm:start`. Disabling projects removes its host routes/dashboard while
retaining the data, grants, audit and receipts.

Assembly contract `2.0.0` supports the additional flag. Contract `1.0.0` remains
valid for previous four-module assemblies and refuses the new flag. The original
four-module runtime defaults remain unchanged.

## Kit scope and evidence

`project_tracking` is a deliberately narrow assembly scope for K05 project
services and K31 construction/EPC. It means this shared internal tracking module;
it does not implement either family's full operating domain. Both full
`operations` requests still report missing capabilities and refuse assembly.

`pnpm projects:verify` installs two independent consumers, uses an isolated real
PostgreSQL cluster, exercises authorization, races, state invariants, SQL protection,
restart and restoration, and writes `dist/project-proof/results.json`.
`pnpm projects:verify-host` tests actual operator enrollment, authenticated APIs,
Chromium workflow and response-loss recovery, module disable/re-enable and owner
backup restoration. It writes `dist/project-host-proof/results.json`.

Both checks use synthetic records and private disposable state, with no provider
calls or changes to the user's owner database. These are local acceptance checks,
not production performance, live-client or complete-kit certification.
