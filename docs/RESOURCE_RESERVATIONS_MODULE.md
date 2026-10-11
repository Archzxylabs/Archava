# Resource reservations module

Historical profile 1.5.0 added `@archava/scheduling` and `@archava/scheduling-pg` version
0.1.0. This is the shared foundation for timed appointments and individual asset
reservations. It is not acceptance of a full salon, rental company or hotel kit.
The [programme ledger](STARTER_KIT_IMPLEMENTATION.md) retains those domain gaps.

## Implemented contract

Each tenant owns resources of type staff, asset, room or space. Each resource has
capacity **one**, an IANA display timezone, an immutable owner and a revision.
The type is a label, not a connection to an employee, hotel room or asset register.
Resources have no weekly opening hours yet: they are available unless disabled or
occupied by a live reservation/block.

All input times are explicit ISO instants including seconds and UTC offset, such
as `2026-10-12T10:00:00+07:00`. They normalize to UTC at millisecond precision.
Offset-free wall-clock input is refused, including ambiguous DST input. Intervals
are positive, at most 31 elapsed days, and half-open: an end at 11:00 allows the
next reservation to start at 11:00. The resource timezone controls display; it does
not reinterpret an explicitly supplied offset or define hotel calendar nights.

Human commands:

| Command               | Preconditions and effect                                                                                                                                          |
| --------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `resource.create`     | Name, resource type and supported timezone; immutable owner supplied by the server; capacity one                                                                  |
| `resource.set-active` | Current resource revision and reason; disabling refuses unexpired holds or confirmed reservations/blocks whose end is in the future                               |
| `reservation.hold`    | Active visible resource, future non-overlapping slot; database-generated expiry with TTL 1–1,800 seconds, default 900                                             |
| `reservation.confirm` | Current reservation revision; held, unexpired and not yet started; internal operator confirmation only                                                            |
| `reservation.move`    | Current revision and reason; not cancelled/expired/started; same resource, future non-overlapping interval; hold expiry is never extended                         |
| `reservation.cancel`  | Current revision and reason; not already cancelled/expired; immediately releases occupancy; no financial/refund implication                                       |
| `reservation.block`   | Additional `resources.write` grant; confirmed maintenance/unavailability interval; blocks booking, and editing/cancelling also requires resource-write permission |

A stored `held` row becomes effectively `expired` when its expiry is at or before
the database clock. Read results include `effectiveState` and `evaluatedAt`;
expiry releases occupancy without deleting history or requiring a background job.
It produces no synthetic expiry command/audit event. Retrying the original hold
returns its original receipt and never resurrects it. A new hold requires a new
request UUID. Confirmed and cancelled rows retain their original expiry if they
began as a hold; it no longer controls confirmed occupancy.

Every command uses a UUID bound to tenant, actor and canonical payload via the
stable fingerprint key. Mutation, audit and receipt commit in one transaction.
Current authorization is rechecked on duplicates and receipt recovery. A stale
revision, overlapping move or unsupported transition rolls back entirely.
Different requests on the same resource serialize. SQL triggers additionally
guard overlap, identity changes and disabling occupied resources. Memberships,
resources, reservations, commands and audit have forced tenant RLS. Runtime roles
cannot grant memberships or delete records; committed receipts and audits are
immutable. The trusted server owns the connection and tenant context: RLS is not
a sandbox for a compromised database runtime credential.

The implementation follows PostgreSQL's documented [row locks](https://www.postgresql.org/docs/current/explicit-locking.html#LOCKING-ROWS)
and [inclusive/exclusive interval semantics](https://www.postgresql.org/docs/current/rangetypes.html#RANGETYPES-INCLUSIVITY).
No new third-party dependency or database extension is needed. Expiry time is
sampled with `clock_timestamp()` after acquiring the resource lock, rather than
using the transaction's start timestamp.

## Ownership and API

The pure domain imports only Zod. The PostgreSQL adapter owns
`archava_scheduling`, its memberships and checksummed migration. It never reads or
writes CRM, payments, client signatures or another scheduling provider's records.
The two independent installed consumers use the identical compiled implementation
for staff appointments and individual camera rental, without a source fork.

Permissions are `resources.read`, `resources.write`, `reservations.read`,
`reservations.write`, and `audit.read`. Assigned scope requires resource ownership
and, for reservation detail/edit/receipt access, reservation ownership as well.
This does not yet support staff sharing a resource through per-resource grants.
Service reads intersect the service and human's current grants/scope. All machine
writes and machine receipt recovery are refused. No write tool is registered.

Host routes under `/api/scheduling/:tenant`:

| Route                                                       | Result                                                                                       |
| ----------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `GET /access`                                               | Current domain grants and scope                                                              |
| `GET /resources?limit=20&after=UUID&active=true`            | Bounded page, cursor, observation time; filters optional                                     |
| `GET /resources/:id`                                        | Visible resource                                                                             |
| `GET /reservations?resourceId=UUID&limit=20&after=UUID`     | Bounded visible reservation history, including cancelled/expired rows                        |
| `GET /reservations/:id`                                     | Record plus up to 50 audit entries; `null` audit without its grant, explicit truncation flag |
| `GET /availability?resourceId=UUID&startsAt=ISO&endsAt=ISO` | Boolean snapshot, including every occupant; no other occupant's private details              |
| `POST /commands`                                            | Strict `{requestId, command}`; atomic authoritative receipt                                  |
| `GET /commands/:requestId`                                  | Actor-bound committed receipt or current `not_committed` observation                         |
| `POST /tools`                                               | Strict `{name,input}`; bound read-only tools                                                 |

Availability is an observation, not a capacity promise. Only the successful hold
command reserves a slot. List pages default to 20, maximum 100. Unknown/duplicate
query keys, caller-supplied identity/capacity/state/expiry and oversized bodies are
refused. A standalone consumer supplies verified identity and cookie CSRF checks;
the existing owner host supplies session, CSRF, same-origin, configured tenant,
active CRM workspace membership and independent scheduling permission checks.
Errors expose codes without private SQL/configuration values.

Read tools are `scheduling.resources`, `scheduling.availability`,
`scheduling.reservations` and `scheduling.detail`. They are available through the
private API when composed. They do not yet route natural-language requests in the
existing CRM conversation or expose private owner data to public visitors.

## Composition and local operation

Assembly contract 3.0.0 adds optional `scheduling`; v1/v2 requests remain supported
for their original fields. Existing four-module defaults are unchanged. Domain
packages do not depend on CRM. The local owner-host composition retains contacts
as its workspace prerequisite.

For a new consumer's bounded scope:

```json
{
  "schemaVersion": "1.0.0",
  "catalogueVersion": "1.0.0",
  "profile": "1.5.0",
  "selections": [{ "id": "K07", "scope": "resource_reservations" }]
}
```

K10 supports the same narrow scope. Full K07/K10 operations and K01 hotel requests
still refuse assembly on missing domain/capability requirements. Use
`pnpm kits:plan REQUEST.json`, then `pnpm library:assemble REQUEST.json OUTPUT --kit`.
Assembly checks the trusted installed profile and records the narrow scope; it
does not grant account access, claim runtime health or change the pricebook.

For the existing local owner host, build/setup first. With the host stopped, pass
a privately owned mode-0600 file containing the existing module selection plus
`"scheduling": true` to `pnpm crm:modules FILE.json`. Preserve desired existing
flags. Separately pass a private file to `pnpm crm:user scheduling FILE.json`:

```json
{
  "username": "owner@archava.local",
  "tenants": ["studio-demo"],
  "access": "write",
  "scope": "all"
}
```

The actual account must have active identity/workspace membership. `access: read`
provides read permissions; `none` revokes domain membership. The operator changes
only scheduling grants. Start with `pnpm crm:start`. The composed panel supports
resource creation/toggle and hold/confirm/move/cancel; maintenance blocks and audit
details currently use the API. Disabling the module hides routes/panel while
retaining data, grants and receipts. Setup/restore migrations never auto-enroll
existing accounts in scheduling. Backups include the owned schema; restore remains
into a fresh target with the existing identity/consent recovery policy.

The panel preserves uncertain mutations in memory, retaining their UUID/body for
checking or retrying. A denied retry does not discard a prior uncertain result.
Reload, logout and tenant switching discard that in-memory recovery; do not claim
durable browser recovery yet. It renders business text with `textContent`, aborts
disposed requests and hides controls without current grants. Refresh reads the
database clock; a countdown is not authoritative.

## Evidence and remaining work

Run `pnpm scheduling:verify` for independent artifact installs, real PostgreSQL
overlap/idempotency races, current grants/RLS, expiry during lock wait, rollback,
restart and fresh-target restoration. Run `pnpm scheduling:verify-host` for the
compiled owner host and real Chromium, including lost-response recovery, viewer
refusal, lifecycle and host backup restoration. These proofs use private disposable
databases and synthetic records with zero provider calls.

Not implemented: recurring calendars/opening hours, group or pooled capacity,
multiple-resource reservations, changing the assigned resource, buffer periods,
waitlists, calendar sync, public customer booking, reminder delivery, deposits,
refunds, customer cancellation policies, rental handover/damage/return, hotel
room-night inventory/tariffs/stay lifecycle/housekeeping, durable browser recovery,
or a general scheduling AI planner. These remain explicit kit requirements.

Profile 1.6.0 adds a separate [accommodation domain](ACCOMMODATION_MODULE.md)
for room nights, stays and housekeeping. Scheduling 0.2.0 adds an additive shared
occupancy migration and pure calendar primitives. The five original scheduling
tables retain their contracts; occupancy is the sixth forced-RLS table. Generic
timed APIs hide/refuse accommodation-managed rooms, including with all generic
grants. The kernel contains no guest/quote data. Scheduling migration 001 remains
unchanged. Current kit requests use profile 1.6.0; the 1.5.0 example above is historical.
