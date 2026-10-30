# Formal education: enrollment, attendance, and results

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/formal_education.rs), [persistence](../../crates/adapters/src/erp/formal_education.rs), [HTTP](../../crates/host/src/erp/formal_education.rs), [integration tests](../../crates/host/tests/erp_formal_education.rs).

`@archava/formal-education-pg` owns seven forced-RLS tables: `memberships`, `enrollments`,
`sessions`, `results`, `reviews`, `commands`, and `audit`. Its checked migration ledger
`001_formal_education_enrollment.sql` is checksummed
(`8cbe38d058d38e461507aca92f211cb5aeedf168b3349f69e91841e95b38b4c5`) and constitutes ledger
ke-23, the last of the twenty-three database ledgers in `config/library-release.v1.json`.

- **Append-only evidence**: recorded attendance, recorded results, reviews and audit entries
  cannot be updated or deleted at all, by any role, through triggers rather than by convention.
- **Tenant isolation**: every table carries forced row-level security on `archava.tenant_id`,
  applied to the table owner too, so a runtime role that owns the schema cannot read across
  tenants either; a session without the tenant setting sees nothing rather than everything.
- **Frozen status chain** in SQL as well as in the contract, so a caller that bypasses the
  service still cannot skip review and still cannot reach `retained` from anywhere but
  `reviewed`.

## What this domain owns

Admission, term enrollment, recorded attendance, recorded results, domain review and retention.

It does not own teaching. Scheduling remains the authority that mints the class meeting this
domain points at (`sessions.appointment_ref`), and billing remains the authority for the fee
(`enrollments.invoice_ref`). Both references survive here so an academic record can always name
the appointment it was taken at and the invoice raised against it; neither is a foreign key into
another schema, and neither grants that domain any authority over the academic record.

What it does own is the boundary between a record under review and a published one, and that is
the point of the whole domain.

## Value object

`enrollmentId` is the aggregate root: one student, one class, one term. Attendance, results and
reviews all hang off it, and none of them can exist without it.

The student is identified by `student_ref`, a reference, not a person record. This domain holds no
name, no date of birth and no contact detail — a guardian is `guardian_ref`, the same kind of
opaque reference. An academic record therefore cannot be used as a directory, and no part of this
domain is a place a personal-data collector would look.

## State machine

```
applied -> admitted -> enrolled -> attending -> reported -> reviewed -> retained
   |          |           |            |            |                      |
   |          |           |            |            |                      +-> graduated
   |          |           |            |            |                      |
   +----------+-----------+------------+------------+----------------------+----> withdrawn
                                        (reported <-> attending)
```

`withdrawn` is reachable from `applied`, `admitted`, `enrolled`, `attending`, `reported` and
`reviewed`; `retained`, `graduated` and `withdrawn` are terminal. The single back edge
`reported -> attending` exists so a result set recorded by mistake is withdrawn and replaced rather
than silently kept: a mis-keyed score moves the record back to attendance, never forward to
review. `revision` advances by exactly one on every move and the audit entry records who moved it,
so every position on this chain is attributable.

Withdrawal is a status, not an absence. An enrollment is never deleted — a guardian asking what
happened to last year's record gets the record back rather than a 404.

## The academic publishing boundary

`reviewed` is the boundary, and it is enforced twice: `PUBLISHED_STATUSES` in the contract and
`archava_edu.results_published()` in SQL, both naming exactly `reviewed`, `retained` and
`graduated` as published.

Everything before `reviewed` is a record under review. A guardian or a student reading it is told
that explicitly through the `resultsPublished` flag rather than by being handed an empty list —
"none recorded yet" and "recorded and not published" are different answers, and the caller is
given the one that is true.

`review` is its own grant, not an implication of `education.record_result`. The teacher who records
a score and the reviewer who publishes it are two acts, and a grant for one is not a grant for the
other. `retain` is likewise its own grant, is reachable only from `reviewed`, and is terminal.

## Score evidence on the assessment's own scale

A result carries `score` and `outOf`, and the bound is `outOf`, never a fixed ceiling: a school
that marks out of 20 must not be able to record 85, and one that marks out of 100 must not be able
to record 0.85. `assert_score` fires on insert in SQL and the same refinement applies to the command
envelope, because those two schemas differ — the server stamps `recordedBy`, the caller does not —
and a rule living on only one of them would let an out-of-range score become legal by choosing
which schema it was submitted through.

The letter reference stays free-form. Grading alphabets are institution-specific and this package
does not get to invent one.

## Access scopes

The four scopes are not ranked, and no combination of them widens to another. The predicate is
chosen by the stored membership row, never by the request, so asking for "more" gets the same
predicate rather than a wider one.

| Scope            | Reaches                                                              |
| ---------------- | -------------------------------------------------------------------- |
| `all`            | every enrollment of the tenant — the registrar's administrative view |
| `guardian`       | rows naming this principal as the student's guardian                 |
| `self`           | rows naming this principal as the student                            |
| `assigned_class` | rows in a class this principal teaches (`memberships.classes`)       |

A guardian cannot reach a class, a student cannot reach a sibling, and a teacher cannot reach a
guardian's other child. `guardian`, `self` and `assigned_class` each reach a different set, and the
empty `classes` array of an `assigned_class` membership makes that scope reach nothing.

The predicate lives in `archava_edu.visible_enrollment()`, one `SECURITY DEFINER` function with a
pinned `search_path`, rather than in the application. Its identifiers are qualified and the
membership columns are aliased to `m.` deliberately: in a SQL function an unqualified name resolves
to an in-scope column before it resolves to a parameter, so `m.principal_id = principal_id` over a
table that also has a `principal_id` column parses as `m.principal_id = m.principal_id` — always
true. An earlier draft of that function was written that way, and the assigned-class guard silently
became "visible to any caller" while still reading as though it checked the principal.

## Grants

| Grant                      | What it allows                                           |
| -------------------------- | -------------------------------------------------------- |
| `education.read`           | list and read enrollments, sessions, results, reviews    |
| `education.enroll`         | admit, enroll and advance the record (the three actions) |
| `education.record_session` | record attendance at an appointment                      |
| `education.record_result`  | record an assessment result                              |
| `education.review`         | review recorded results and publish them                 |
| `education.retain`         | retain a reviewed record (terminal)                      |
| `audit.read`               | read the audit trail                                     |

`education.enroll`, `education.record_session`, `education.record_result`, `education.review` and
`education.retain` are separate because they are separate acts. `review` in particular is the grant
that crosses the publishing boundary, and giving it to whoever records scores would collapse the
review step the whole chain exists to create. A scope-wide write is refused for `guardian`, `self`
and `assigned_class` principals: those scopes read.

## Proofs

- Domain proof (`formal-education:verify`, 22 checks): shared consumers and browser-safe
  dashboard, idempotent migrations with a narrow runtime role, the three admission actions each
  with their own revision and grant, attendance requiring an enrolled student and naming its
  appointment and recorder, the score bound, results staying unpublished until review, review and
  retention as separate terminal-gated steps, the three non-overlapping scopes, a limit, page or
  cursor not widening a scope the caller lacks, withdrawal surviving and refusing further
  evidence, append-only tables, immutable identity columns with the chain enforced in SQL, replay,
  altered-payload and unknown-command handling, the guardian fill-once rule, service principals
  refused for writes, cross-tenant identifiers never disclosed, inactive principals refused, tool
  boundaries, grant revocation taking effect on the next command, catalog/ledger checksum match,
  runtime role neither superuser nor bypass-RLS nor schema owner, and a tenantless query seeing
  nothing.
- Host proof (`formal-education:verify-host`, 6 check calls): compiled CRM plus headless Chromium —
  routes absent while the module is disabled, authentication and grants required with module
  enablement never auto-enrolling an account, CSRF, origin, guardian write refusal and tool
  boundaries on the compiled host, the status chain held one action wide with fingerprint replays,
  results withheld until review, and the compiled browser bundle carrying the dashboard mount
  point.

Both proofs run at profile `1.17.0` with `providerCalls: 0` — no live payment, email or AI provider
is contacted. The host proof accepts no operational kit: its `operationalKitsAccepted` is `[]`, so
nothing here is a full operating kit.

## S1 recovery semantics

A command whose outcome is unknown stays pending: an identical `command_id` returns its original
recorded receipt, an altered payload under a replayed ID is refused rather than re-executed, and an
ID that was never recorded is reported unknown instead of being treated as absent. `revision` is
what makes this safe — an unknown write cannot be applied twice with a different revision, because
each move advances it by exactly one.
