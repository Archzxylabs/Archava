# Attendance, session rosters and event check-in module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/attendance.rs), [persistence](../../crates/adapters/src/erp/attendance.rs), [HTTP](../../crates/host/src/erp/attendance.rs), [integration tests](../../crates/host/tests/erp_attendance.rs).

This contract covers session/roster lifecycles, participant registration, capacity limits, verified check-in events with idempotency keys, and immutable attendance audit.
Candidate release 1.14.0 adds bounded course and event attendance without altering existing CRM, scheduling or commerce authority.
Full course/cohort and event/venue operating kits retain their other requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/attendance` owns strict contracts, human service commands, private read tools, HTTP API and reusable DOM dashboard with a hardened state machine receipt recovery (S1). It depends only on pinned Zod and needs no external CRM or AI provider.
`@archava/attendance-pg` owns six forced-RLS business tables: `memberships`, `sessions`, `rosters`, `check_in_events`, `commands`, and `audit`. Its checked migration ledger `001_attendance_sessions_rosters.sql` is checksummed (`f71bab0e95d33c253bac9eae573675e5fe321c33f8d81f7124561351f7484883`) and constitutes ledger ke-18.

Two independent business consumers demonstrate generalization:

1. **Consumer 1: Courses, bootcamp and training cohorts (`K13: courses-training`)**:
   Registers learners onto bounded cohort sessions (`role: 'attendee'` / `'instructor'`), enforces seat capacity at registration, records attendance per session and reports enrolled versus attended counts for completion review.
2. **Consumer 2: Events, ticketing and venues (`K15: events-venues`)**:
   Registers ticket holders against event sessions with unique ticket codes, performs scanner check-in (`method: 'qr_scan'`) with idempotency keys, refuses duplicate scans and keeps a per-session check-in ledger for reconciliation.

## Lifecycle and Invariants

```text
Session lifecycle:
draft ──[change_session_status]──> scheduled ──> in_progress ──> completed
                                    │                              ▲
                                    └──[canceled]──────────────────┘

Roster lifecycle:
registered ──> confirmed ──> attended (record_check_in)
     │              ├──> waitlisted
     ├──> no_show   └──> canceled
```

- **Human-only mutations**: `create_session`, `register_participant`, `record_check_in`, `update_roster_status` and `change_session_status` are refused for service/automation principals at the service boundary.
- **Capacity limits**: `capacity` is a positive seat count or `null` (unbounded). The `session_capacity_limit` CHECK keeps `enrolled_count <= capacity`, so registration past the limit is refused atomically rather than overbooking.
- **Roster identity constraints**: one roster row per `(tenant, session, contact)` and one ticket code per tenant; duplicate registration or duplicate ticket issuance is a conflict, not a second record.
- **Verified check-in and idempotency**: a check-in event records `method` (`qr_scan`, `manual_operator`, `kiosk`), optional `evidence_ref`, `recorded_by` and an `idempotency_key` unique per roster. A second scan with a new key on an already-checked-in roster is refused; replaying the same key returns the original receipt with `idempotencyReplay: true`.
- **Attendance counting**: `attended_count` on the session advances with recorded check-ins, and session detail reports enrolled versus attended totals derived from roster state.
- **Optimistic revision locking**: roster and session status updates require the exact `expectedRevision`; stale requests fail with `conflict`.
- **S1 resilient receipt state machine**: the DOM dashboard assigns a client-generated command UUID, marks the outcome uncertain _before_ dispatch, blocks new writes while an earlier outcome is unknown, and offers **Re-query status** (authoritative receipt reconciliation, including detection of an invalid or mismatched receipt) and **Retry same request** (same UUID, same body).
- **Immutable audit and command history**: triggers on `audit` and `commands` prevent update, delete and truncation, and receipts are only written once.
- **Tenant isolation**: every table carries forced row-level security on `archava.tenant_id`; cross-tenant session, roster and check-in lookups return not found.

## Security & Verification Matrix

- **18 Synthetic Verification Checks (`pnpm attendance:verify`)**: two independent cold consumers, migration idempotency and checksum refusal, superuser/schema-owner refusal, service-principal refusal, inactive/zero-grant access refusal, capacity boundary refusal, cross-tenant isolation, duplicate scan refusal, idempotent replay, optimistic locking, session counters, read-only tool boundaries, S1 command recovery and immutable audit.
- **4 Headless Browser E2E Checks (`pnpm attendance:verify-host`)**: authenticated routes and grants, CSRF/origin/viewer/tool boundaries on the compiled host, restart with idempotent receipt recovery, and a full Chromium CDP workflow covering session creation, participant registration and scanner check-in, plus five S1 receipt-recovery regressions (truncated JSON, body-stream read failure on retry, invalid receipt schema on re-query, mismatched recovery command ID, and successful re-query resolution) each asserting zero duplicate sessions, with blocked new writes while an earlier outcome is uncertain.
