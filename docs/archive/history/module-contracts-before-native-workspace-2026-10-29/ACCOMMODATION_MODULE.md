# Accommodation module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/accommodation.rs), [persistence](../../crates/adapters/src/erp/accommodation.rs), [HTTP](../../crates/host/src/erp/accommodation.rs), [integration tests](../../crates/host/tests/erp_accommodation.rs).

Profile 1.6.0 adds `@archava/accommodation` and `@archava/accommodation-pg` 0.1.0,
with `scheduling` and `scheduling-pg` 0.2.0. Hotel and villa consumers install the
same compiled implementation. This accepts the bounded K01 `accommodation_stays`
scope; complete hotel operations remain blocked in the [programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Records and rules

The accommodation domain owns properties, room types, individual rooms, guest
stays, immutable nightly quotes, permissions, command receipts and audit. Its
PostgreSQL schema has eight forced tenant-RLS business tables and a checksummed
migration ledger. There is no mandatory CRM, AI or payment-provider dependency.
The local owner host retains its existing contacts/workspace prerequisite.

Properties have an immutable IANA timezone and scheduled check-in/check-out
times; checkout time cannot be later than check-in time. Room types define a
maximum guest count (1–16) and integer IDR nightly rate (0–1,000,000,000). Rooms
are explicitly assigned, capacity one, with unique codes within each property.
No personal identity documents, card details or regulated guest-reporting records
are collected by this slice.

A stay covers 1–31 **calendar nights**, with an exclusive departure date. Local
times become explicit UTC instants when booked. Nights are calendar dates, not
elapsed hours: a two-night DST stay can span 47 or 49 hours. Missing or ambiguous
local times are refused; the caller cannot supply an arbitrary offset to override
the property policy. Dates are bounded to 2000–2100; booking cannot begin before
the property's current date. Current-day walk-ins are supported within the stay's
time window. Property policies cannot yet be edited.

The server reads the room type's current rate and freezes its revision and a
quote row for every night. A rate update affects future bookings only. The total
is deterministic `nightlyRate × nights`; callers cannot supply a price, paid
status, expiry, owner or execution time. Quotes are estimates, without tax,
discounts, deposits, invoices or refunds. They do not modify ARCHZXY's pricebook.

| Command              | Rule                                                                                                      |
| -------------------- | --------------------------------------------------------------------------------------------------------- |
| `property.create`    | Human property-write grant; immutable policy and server-derived owner                                     |
| `room-type.create`   | Visible property; room-write grant; bounded capacity and nightly rate                                     |
| `room-type.set-rate` | Current type revision and reason; new bookings only                                                       |
| `room.create`        | Room type belongs to same tenant/property; creates shared capacity atomically                             |
| `room.set-active`    | Current room revision and reason; disabling refuses live reservations or any physical guest               |
| `room.housekeeping`  | Current revision and reason; dirty → clean → inspected, or return to dirty; occupied rooms refuse changes |
| `stay.hold`          | Active individual room, guest capacity and non-overlapping dates; TTL 1–1,800 seconds, default 900        |
| `stay.confirm`       | Current revision, held and unexpired; internal operator confirmation                                      |
| `stay.check-in`      | Confirmed stay, inspected active room, no physical guest, and scheduled start ≤ database time < end       |
| `stay.check-out`     | In-house stay; release capacity, mark room dirty, retain original quote                                   |
| `stay.cancel`        | Held/unexpired or confirmed; current revision and reason; terminal, with no payment implication           |

Check-in marks the room occupied and dirty. After checkout, cleaning and inspection
are required before the next guest can check in. Requiring `inspected` is this
module's explicit policy. The distinction between dirty, clean and inspected is
also documented by [Oracle's housekeeping board](https://docs.oracle.com/en/industries/hospitality/opera-cloud/25.1/ocsuh/t_housekeeping_using_the_housekeeping_board.htm).
An in-house guest remains physically present after the planned departure until an
actual checkout. Current-day holds, cleaning, disabling and another check-in
remain blocked. Future planned reservations can exist; the operator must resolve
an overdue guest before the next actual check-in.

## Shared capacity and transaction boundary

Scheduling migration `002_shared_occupancy` is additive; original migration 001 is
unchanged. It copies prior timed reservations into a shared occupancy table,
retaining cancelled/expired history and original identifiers. Timed reservations
continue to synchronize their capacity through a SQL trigger. No synthetic
business command or human permission is introduced by migration.

Accommodation rooms create managed scheduling resources through a bounded factory.
The fixed internal service identity has **no scheduling domain grants**. Generic
scheduling APIs hide and refuse those resources, including for a human with all
generic scheduling permissions. Thus generic commands cannot bypass stay or
housekeeping rules. The capacity table contains no guest name or quote.

Both domains lock the same resource row. Stay, occupancy, nightly quote, audit and
receipt commit in one database transaction; rejected room creation or booking
cannot leave orphan capacity. SQL guards protect overlap and domain binding;
the design follows PostgreSQL's [trigger semantics](https://www.postgresql.org/docs/current/trigger-definition.html).
Expiry uses `clock_timestamp()` **after** the lock, including transactions that
began before expiry. Explicit stored instants follow PostgreSQL's documented
[timestamp behavior](https://www.postgresql.org/docs/current/datatype-datetime.html).

Each command UUID is bound to tenant, actor and canonical payload with a stable
HMAC key. Duplicate requests return the original receipt after current grants and
scope are checked; they never extend holds or restore terminal stays. Revisions
reject stale edits. Committed receipts, nightly quotes and audit are immutable.
Runtime roles cannot change memberships or delete records. The trusted server
owns tenant context and database credentials; tenant RLS is not a sandbox against
a compromised private database role. Kernel exports require a caller-owned
transaction and prior domain authorization; they are not public action authority.

## API, tools and permissions

Host routes are `/api/accommodation/:tenant`:

| Route                                                                                | Result                                                                               |
| ------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| `GET /access`                                                                        | Current accommodation grants and scope                                               |
| `GET /properties`, `/properties/:id`                                                 | Bounded property page or authorized property                                         |
| `GET /room-types?propertyId=UUID`                                                    | Current room types/rates                                                             |
| `GET /rooms?propertyId=UUID`, `/rooms/:id`                                           | Readiness, active flag and physical occupancy                                        |
| `GET /stays?propertyId=UUID`, `/stays/:id`                                           | Stay page or detail with nightly quote and latest 50 audit entries                   |
| `GET /availability?propertyId=UUID&roomId=UUID&arrival=DATE&departure=DATE&guests=N` | Boolean snapshot and current estimated rate; successful hold alone reserves capacity |
| `POST /commands`                                                                     | Strict `{requestId, command}` and atomic receipt                                     |
| `GET /commands/:requestId`                                                           | Actor-bound receipt or current `not_committed` observation                           |
| `POST /tools`                                                                        | Strict private read-only `{name, input}`                                             |

Pages default to 20 and permit at most 100, with UUID cursors and database
observation time. Audit is `null` without its grant, with an explicit truncation
flag when permitted. Unknown/duplicate query fields and bodies over 16 KiB are
refused; errors never expose SQL or credentials.

Permissions: `properties.read/write`, `rooms.read/write`, `housekeeping.write`,
`rooms.inspect`, `stays.read/write`, `audit.read`. Inspection has a separate grant.
Assigned scope requires property ownership and additionally stay ownership for
stay detail, edits and receipt recovery. Per-property staff sharing is not yet
implemented. Service reads intersect service and current human grants/scope;
machine writes and machine receipt recovery are refused.

Private tools: `accommodation.properties`, `.room-types`, `.rooms`, `.stays`,
`.availability`, `.detail`. They do not yet route hotel language in the CRM
conversation. Public visitors never receive owner tools. A standalone consumer
supplies authenticated identity and cookie CSRF protections; the owner host adds
current session, CSRF, same origin, configured tenant and active workspace checks.

## Composition and local activation

Assembly 4.0.0 adds optional `accommodation`, requiring `scheduling` in the local
host. Earlier v1/v2/v3 contracts retain their original fields and old four-module
defaults remain unchanged. Enabling a module does not enroll any account.

```json
{
  "schemaVersion": "1.0.0",
  "catalogueVersion": "1.0.0",
  "profile": "1.6.0",
  "selections": [{ "id": "K01", "scope": "accommodation_stays" }]
}
```

Use `pnpm kits:plan REQUEST.json`, then `pnpm library:assemble REQUEST.json OUTPUT --kit`.
Assembly records the bounded scope; full K01 operations still refuse on missing
capabilities. Configuration is not account permission, runtime health or deployment.

For the existing owner host: `pnpm crm:build`, then `pnpm crm:setup` with the host
stopped. Pass a privately owned mode-0600 JSON containing desired existing flags
plus `scheduling: true` and `accommodation: true` to `pnpm crm:modules FILE.json`.
Separately pass this private grant file to `pnpm crm:user accommodation FILE.json`:

```json
{
  "username": "owner@archava.local",
  "tenants": ["studio-demo"],
  "access": "write",
  "scope": "all"
}
```

Use the actual enrolled account. `read` grants reads; `none` revokes hotel
membership. The operator changes only accommodation grants. Start via
`pnpm crm:start`. The panel handles property/type/room creation, readiness,
room toggles and stay lifecycle. Rate updates and full audit details use the API.
Disabling hides the panel/API while retaining records, grants and receipts.

Uncertain browser writes retain the same UUID/body **in memory** for status checks
or retry, even after a refused retry. Reload/logout/tenant changes discard this
memory; durable tab-reload recovery remains future work. Business text uses
`textContent`; disposal aborts requests and logout clears guest data. Restore goes
into a fresh target with explicit identity recovery and original private-state
protections.

## Verification and remaining scope

`pnpm accommodation:verify` installs two independent consumers and exercises real
PostgreSQL migration from non-empty 1.5, overlapping/duplicate races, rollback,
current permissions, parent scopes, DST, expiry after lock waits, frozen rates,
room readiness, overdue physical occupancy, restart and fresh restoration.
The overdue test seeds a historical fixture in a disposable database with an
owner-only transaction; API creation and all tested commands retain their guards.
`pnpm accommodation:verify-host` exercises the actual compiled owner host and
Chromium, including response loss, viewer refusal, lifecycle and backup restore.
Both use synthetic records and zero provider calls.

Not implemented: moving/extending/rescheduling stays, pooled room-type inventory,
multi-room/group bookings, rate calendars, taxes/discounts/folios, deposits/payment,
refunds, no-shows, housekeeping staff assignment, timed maintenance windows,
OTA/channel manager, guest identity reporting, public booking, customer agreement,
general hotel AI planning or a complete PMS. All forty full operational kit
families remain in the programme ledger. Next domain: orders/retail/food service.
