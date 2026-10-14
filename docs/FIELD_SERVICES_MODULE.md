`@archava/field-services-pg` owns seven forced-RLS business tables: `memberships`, `jobs`,
`visits`, `work_lines`, `signoffs`, `commands`, and `audit`. Its checked migration ledger
`001_field_services_dispatch_visits.sql` is checksummed
(`094bf7d79c03ec16654d1adccb07984514d90524311822deaab51fc9683aa8d1`) and constitutes ledger
ke-22.

- **Optimistic revision locking**: every status change raises `revision` and the command
  ledger records the receipt that produced it; replayed command IDs return the recorded
  receipt instead of applying twice.
- **Tenant isolation**: every table carries forced row-level security on
  `archava.tenant_id`; the runtime role is neither superuser nor bypass-RLS nor schema
  owner, and cross-tenant job lookups return not found.

## What this domain owns

Only the dispatch handover and its evidence. Scheduling remains the authority that mints
the appointment this domain points at, and billing remains the authority for invoicing and
settlement. `field_services` records which technician was sent, what happened on site, what
labour and parts the work consumed, and whether the customer accepted it.

## Value object

`jobId` is the aggregate root. Every command carries `appointmentRef`, so a dispatch can
never exist without the appointment it belongs to. Only the customer signoff is stored as a
distinguishing acceptance record: "the work was done" and "the customer accepted the work"
are different claims.

## State machine

```
requested -> assigned -> visited -> evidenced -> signed_off -> approved
     |            |          |            |             |
     +------------+----------+------------+-------------+----> canceled
                                                   (signed_off <-> evidenced)
```

`canceled` and `approved` are terminal. A second `record_visit` on a visited-or-beyond job
is refused, and any `append_work_line` after approval is refused. The back edge
`signed_off -> evidenced` exists so revised evidence is signed again rather than approved on
stale proof.

## Weight evidence on two axes

Labour is hours, parts are pieces. A labour line with `unit: 'pieces'` is rejected at the
schema boundary; a part line with a fractional quantity is rejected; a labour line carrying a
`partRef` is rejected. The counters never share a column, so one number can never stand in
for both.

## Grants

| Grant            | What it allows                                     |
| ---------------- | -------------------------------------------------- |
| `field.read`     | list and read jobs, visits, lines, signoffs, audit |
| `field.write`    | append work lines (counter work at the machine)    |
| `field.dispatch` | assign technicians and record visits               |
| `field.signoff`  | record the customer's acceptance and approve       |

`field.dispatch` is deliberately separate from `field.write`: sending a technician is not the
same act as recording evidence at the machine, and a grant for one is not a grant for the
other. Approval requires a signoff that exists and is accepted, not merely permission.

## Proofs

- Domain proof (`field-services:verify`, 32 checks): forward chain to approval, post-approval
  evidence refusal, appointment requirement, rejected signoff, cross-tenant isolation,
  superuser/schema-owner refusal, service-principal and zero-grant refusal, idempotent
  replay, append-only audit triggers.
- Host proof (`field-services:verify-host`, 6 checks): compiled CRM + headless Chromium —
  disabled module 404s, module enablement never auto-enrolls an account, CSRF/origin/viewer
  write refusal, unknown tool 404, restart retention, dispatch-to-signoff workflow, and all
  five S1 recovery modes.

## S1 recovery semantics

A command whose outcome is unknown stays pending: requery reports
`Command status is still unknown on the server` when nothing was recorded, and a retry
replays the same command ID instead of issuing a second write.
