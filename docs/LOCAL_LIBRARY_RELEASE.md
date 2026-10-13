# Local reusable library release

## Current extension — profile 1.17.0, 13 October 2026

Laundry and dry cleaning processing adds 2 compiled artifacts (`laundry` and
`laundry-pg` 0.1.0), 18 unchanged external pins, twenty-one owned ledgers, host 0.20.0, config 0.19.0, and kits 0.14.0.
Laundry runs without CRM or an LLM. Assembly v15 adds terminal laundry composition alongside billing,
fulfillment, attendance and subscriptions while preserving previous contracts and four-module defaults.
State machine receipt recovery assigns a client-generated command UUID and marks the outcome uncertain
_before_ mutation dispatch, blocks new writes while an earlier outcome is unknown, and requires explicit human
re-query or retry across transport errors, body read rejections, truncated JSON payloads, and receipt schema/command-ID mismatches.

| Extension                                 | Accepted evidence                                                                                                                                                                                                                                                                            | Limit                                                                                                             |
| ----------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| Frozen priced laundry order intent        | Two cold-installed kilo (`K19` weight) and dryclean (`K19` item) consumers; server-computed totals from validated integer lines with split piece/gram counters; order code, kind, customer, currency and total immutable at the database boundary; undeletable orders; 30 checks             | Prices are frozen minor-unit integers only; no pricing engine, tax calculation, discounts, or currency conversion |
| Monotonic processing chain & quality gate | intake→tag→wash→dry→press→quality_check→ready→handover with repeat/backwards refusal; `ready` blocked while a quality finding is unreconciled and until a quality check exists; one-clear-per-reconciliation; dedicated `laundry.process` machine grant versus `laundry.write` counter grant | No barcode/RFID hardware, chemical inventory, route dispatch, or per-garment tracking                             |
| Quality-gated terminal handover           | returned/canceled terminal states refuse further evidence; optimistic revision locking; idempotent replay with fingerprint-mismatch refusal; forced tenant RLS; superuser/schema-owner/service/inactive refusals; read-only tools; append-only audit                                         | No invoicing or payment behaviour (billing owns settlement); no customer-facing portal                            |

All eight root gates passed: 1,653 tests/127 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6, assets 18/5, subscriptions 18/5, attendance 18/4, fulfillment 27/6, billing 30/10,
laundry 30/6 and upgrade 4.
The security matrix has 80 controls; root/runtime scans have zero advisories. The separate realtime template
retains thirteen moderate findings and independent SDK/live gates. These checks do not establish production load
or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.17.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.17.0 are archived in `dist/releases/1.17.0/library` and `dist/releases/1.17.0/crm-runtime`.

See [laundry contract](LAUNDRY_MODULE.md), [sanitized evidence](audit/2026-10-13-release-1.17.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.

## Previous extension — profile 1.16.0, 13 October 2026

Invoicing and payment settlement adds 2 compiled artifacts (`billing` and
`billing-pg` 0.1.0), 18 unchanged external pins, twenty owned ledgers, host 0.19.0, config 0.18.0, and kits 0.13.0.
Billing runs without CRM or an LLM. Assembly v14 adds terminal billing composition alongside fulfillment,
attendance and subscriptions while preserving previous contracts and four-module defaults.
State machine receipt recovery assigns a client-generated command UUID and marks the outcome uncertain
_before_ mutation dispatch, blocks new writes while an earlier outcome is unknown, and requires explicit human
re-query or retry across transport errors, body read rejections, truncated JSON payloads, and receipt schema/command-ID mismatches.

| Extension                                     | Accepted evidence                                                                                                                                                                                                                                      | Limit                                                                                                  |
| --------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------ |
| Immutable invoice intent and priced snapshots | Priced draft creation with frozen `gross_minor` equal to the sum of its lines; issue-time snapshot freezing; issued-invoice re-issue refusal; append-only correction chains that change what is owed while the priced total stays intact.              | No line editing after issue, no partial-invoice dispatch, no automated pricing or tax engine           |
| Payment intent without an outcome field       | `record_payment_attempt` carries no currency field and no field capable of asserting a result; recording one settles nothing and does not even bump the invoice revision.                                                                              | Records intent only; never a settlement, a refund, or a charge                                         |
| Provider-authoritative settlement             | Only a correctly signed provider notification settles an issued invoice; tampered-amount, foreign-key, foreign-domain and unconfigured-provider refusals all fail closed; a correctly signed event still requires the dedicated `billing.write` grant. | No hosted checkout, no card capture, no stored payment credentials, no provider API client             |
| Balanced, reopenable lifecycle                | Over-settlement and over-refund refused as unbalanced; refund reopens `settled → issued` without erasing the settlement; a `failed` notification moves no balance, status or revision; `void` is terminal.                                             | No partial refunds below a settled amount already covered; no dunning, collections or ageing schedules |
| Signature as the single trust boundary        | Domain-separated digest under `archava/billing/provider-event/v1` over length-framed tenant, invoice, provider event id, outcome, amount and currency; verification lives in one code path.                                                            | One shared-secret scheme; no mTLS, no key rotation service, no per-processor key escrow                |

All eight root gates passed: 1,618 tests/124 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6, assets 18/5, subscriptions 18/5, attendance 18/4, fulfillment 27/6, billing 30/10 and upgrade 4.
The security matrix has 62 controls; root/runtime scans have zero advisories. The separate realtime template
retains thirteen moderate findings and independent SDK/live gates. These checks do not establish production load
or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.16.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.16.0 are archived in `dist/releases/1.16.0/library` and `dist/releases/1.16.0/crm-runtime`.

See [billing contract](BILLING_MODULE.md), [sanitized evidence](audit/2026-10-13-release-1.16.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.

## Previous extension — profile 1.15.0, 13 October 2026

Fulfillment dispatch and light production adds 6 compiled artifacts (`fulfillment` and
`fulfillment-pg` 0.1.0), 18 unchanged external pins, nineteen owned ledgers, host 0.18.0, config 0.17.0, and kits 0.12.0.
Fulfillment runs without CRM or an LLM. Assembly v13 adds terminal fulfillment composition alongside attendance
and subscriptions while preserving previous contracts and four-module defaults. State machine receipt recovery assigns a client-generated
command UUID and marks the outcome uncertain _before_ mutation dispatch, blocks new writes while an earlier outcome
is unknown, and requires explicit human re-query or retry across transport errors, body read rejections, truncated
JSON payloads, and receipt schema/command-ID mismatches.

| Extension                                     | Accepted evidence                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         | Limit                                                                                                                                                         |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Fulfillment dispatch & light production       | Two cold-installed warehouse/logistics (`K20`) and made-to-order production (`K23`) consumers; real PostgreSQL job, append-only line/finding/custody lifecycles with immutable audit, monotonic receive→deliver custody chain, returns gated on a prior dispatch, delivery findings under a one-clear-per-reconciliation gate that blocks completion, separate material/output/scrap counters, idempotent replay, cross-tenant RLS isolation, superuser/schema-owner refusal, service-principal and inactive/zero-grant refusal, optimistic revision locking, dedicated dispatch and produce grants, read-only tool boundaries; 27 checks | Custody, dispatch evidence and simple work orders only; no carrier APIs, stock ownership, pricing, MRP scheduling, label printing, or backup/restore evidence |
| Resilient receipt recovery & host composition | Compiled host and Chromium fulfillment dashboard, human-readable failure messages, blocked new writes while an outcome is uncertain, 5 receipt-recovery regressions (network interruption, HTTP 500 on command, non-JSON receipt, mismatched command UUID, recovery-query outage resolving to an authoritative `unknown`), full browser receive→inspect workflow; 6 host checks                                                                                                                                                                                                                                                           | In-memory pending browser recovery; human-only writes                                                                                                         |
| Trusted assembly                              | Narrow K20 `fulfillment_dispatch` and K23 `fulfillment_dispatch` under v13; kits 13 and assembly 2                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                        | No full operating kit accepted                                                                                                                                |

All eight root gates passed: 1,587 tests/121 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6, assets 18/5, subscriptions 18/5, attendance 18/4, fulfillment 27/6 and upgrade 4. The security matrix has 62 controls;
root/runtime scans have zero advisories. The separate realtime template retains thirteen moderate findings and independent
SDK/live gates. These checks do not establish production load or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.15.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.15.0 are archived in `dist/releases/1.15.0/library` and `dist/releases/1.15.0/crm-runtime`.

See [fulfillment contract](FULFILLMENT_MODULE.md), [sanitized evidence](audit/2026-10-13-release-1.15.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.

## Previous extension — profile 1.14.0, 13 October 2026

Attendance recording for in-person teaching and event admission adds 6 compiled artifacts (`attendance` and
`attendance-pg` 0.1.0), 18 unchanged external pins, eighteen owned ledgers, host 0.17.0, config 0.16.0, and kits 0.11.0.
Attendance runs without CRM or an LLM. Assembly v12 adds optional attendance composition alongside subscriptions while
preserving previous contracts and four-module defaults. State machine receipt recovery assigns a client-generated
command UUID and marks the outcome uncertain _before_ mutation dispatch, blocks new writes while an earlier outcome
is unknown, and requires explicit human re-query or retry across transport errors, body read rejections, truncated
JSON payloads, and receipt schema/command-ID mismatches.

| Extension                                     | Accepted evidence                                                                                                                                                                                                                                                                                                                                                                                                            | Limit                                                                                                                      |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| Attendance & event check-in                   | Two cold-installed training/course (`K13`) and event/meetup (`K15`) consumers; real PostgreSQL session, roster and verified check-in lifecycles with immutable audit, duplicate scan refusal, idempotent replay, capacity boundary refusal, cross-tenant RLS isolation, superuser/schema-owner refusal, service-principal and inactive/zero-grant refusal, optimistic revision locking, read-only tool boundaries; 18 checks | Sessions/rosters/check-ins only; no course instances, venues, passes, credential scanning apps, or backup/restore evidence |
| Resilient receipt recovery & host composition | Compiled host and Chromium attendance dashboard, human-readable failure messages, blocked new writes while an outcome is uncertain, 5 receipt-recovery regressions (truncated JSON `{`, body stream read rejection, invalid receipt schema, mismatched command UUID, successful re-query resolution), full browser register→check-in workflow; 4 host checks                                                                 | In-memory pending browser recovery; human-only writes                                                                      |
| Trusted assembly                              | Narrow K15 `community_events` and K13 `course_delivery` under v12; kits 13 and assembly 2                                                                                                                                                                                                                                                                                                                                    | No full operating kit accepted                                                                                             |

All eight root gates passed: 1,553 tests/118 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6, assets 18/5, subscriptions 18/5, attendance 18/4 and upgrade 4. The security matrix has 62 controls;
root/runtime scans have zero advisories. The separate realtime template retains thirteen moderate findings and independent
SDK/live gates. These checks do not establish production load or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.14.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.14.0 are archived in `dist/releases/1.14.0/library` and `dist/releases/1.14.0/crm-runtime`.

See [attendance contract](ATTENDANCE_MODULE.md), [sanitized evidence](audit/2026-10-13-release-1.14.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.

## Previous extension — profile 1.13.0, 12 October 2026

Shared recurring subscriptions, entitlements, and usage tracking add 50 compiled artifacts, 18 unchanged external pins,
seventeen owned ledgers, host 0.16.0, config 0.15.0, and kits 0.10.0. Subscriptions/subscriptions-pg 0.1.0
run without CRM or an LLM. Assembly v11 adds optional subscriptions composition while preserving previous contracts and four-module defaults.
State machine receipt recovery sets pending command UUID and uncertain status prior to mutation dispatch across all domains,
holding UI recovery controls open and blocking new writes on transport errors, body read rejections, truncated JSON payloads (`{`),
or receipt schema/ID mismatches.

| Extension                                     | Accepted evidence                                                                                                                                                                                                                                                                                                                                                                                          | Limit                                                          |
| --------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------- |
| Recurring subscriptions & entitlements        | Two cold-installed SaaS (`K09`) and gym (`K08`) consumers; real PostgreSQL recurring subscription lifecycles, explicit entitlements (`metered`, `seat_count`, `boolean_access`, `pass_bundle`), backend payment confirmation, quota exhaustion refusal, idempotent usage consumption, optimistic revision locking, forced tenant RLS, machine write refusal, restart/restore; 18 checks                    | Bounded quotas & periods; no external payment gateway webhooks |
| Resilient receipt recovery & host composition | Compiled host and Chromium subscription, entitlement, and usage consumption lifecycles; 5 failure mode regressions (truncated JSON `{`, body stream read rejection, invalid receipt schema, mismatched command UUID, grant revocation) asserting zero duplicate business records, uncertain write refusal, recovery UI assertion, same-UUID recovery, disable/re-enable, and backup/restore; 5 host checks | In-memory pending browser recovery; human-only writes          |
| Trusted assembly                              | Narrow K09 `digital_subscriptions` and K08 `membership_passes` under v11; kits 13 and assembly 2                                                                                                                                                                                                                                                                                                           | No full operating kit accepted                                 |

All eight root gates passed: 1,547 tests/117 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6, assets 18/5, subscriptions 18/5 and upgrade 4. The security matrix has 62 controls; root/runtime scans have
zero advisories. The separate realtime template retains thirteen moderate findings and independent
SDK/live gates. These checks do not establish production load or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.13.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.13.0 are archived in `dist/releases/1.13.0/library` and `dist/releases/1.13.0/crm-runtime`.

See [subscriptions contract](SUBSCRIPTIONS_MODULE.md), [sanitized evidence](audit/2026-10-12-release-1.13.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.
No company pricebook, external execution or private-data disclosure boundary changes.

## Previous extension — profile 1.12.0, 12 October 2026

Shared asset register, custody tracking, and maintenance logging add 48 compiled artifacts, 18 unchanged external pins,
sixteen owned ledgers (reaching full schema capacity of 16), host 0.15.0, config 0.14.0, and kits 0.9.0. Assets/assets-pg 0.1.0
run without CRM or an LLM. Assembly v10 adds optional assets composition while preserving previous contracts and four-module defaults.
State machine receipt recovery sets pending command UUID and uncertain status prior to mutation dispatch across all domains,
holding UI recovery controls open and blocking new writes on transport errors, body read rejections, truncated JSON payloads (`{`),
or receipt schema/ID mismatches.

| Extension                                     | Accepted evidence                                                                                                                                                                                                                                                                                                                                                                                                          | Limit                                                                                    |
| --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------- |
| Serialized asset register & custody tracking  | Two cold-installed rental (`K10`) and repair workshop (`K18`) consumers; real PostgreSQL serialized item identity, business-owned vs customer-owned custody, custody dispatch/return transitions with condition grading, contact linkage, optimistic revision locking, forced tenant RLS, machine write refusal, restart/restore; 18 checks                                                                                | Physical item custody & condition; no GPS telematics or fleet vehicle DMV title registry |
| Maintenance & service logging                 | Preventive and corrective work logging with replaced parts inventory audit, parts cost ledger, and post-service condition rating updates; forced tenant RLS, immutable audit triggers, restart/restore; 18 checks                                                                                                                                                                                                          | Workshop work evidence; no automated parts ordering or warranty adjudication             |
| Resilient receipt recovery & host composition | Compiled host and Chromium asset register, custody dispatch/return, and maintenance logging lifecycles; 5 failure mode regressions (truncated JSON `{`, body stream read rejection, invalid receipt schema, mismatched command UUID, grant revocation) asserting zero duplicate business records, uncertain write refusal, recovery UI assertion, same-UUID recovery, disable/re-enable, and backup/restore; 5 host checks | In-memory pending browser recovery; human-only writes                                    |
| Trusted assembly                              | Narrow K10 `asset_rental` and K18 `repair_workshops` under v10; kits 13 and assembly 2                                                                                                                                                                                                                                                                                                                                     | No full operating kit accepted                                                           |

All eight root gates passed: 1,543 tests/116 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6, assets 18/5 and upgrade 4. The security matrix has 62 controls; root/runtime scans have
zero advisories. The separate realtime template retains thirteen moderate findings and independent
SDK/live gates. These checks do not establish production load or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.12.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.12.0 are archived in `dist/releases/1.12.0/library` and `dist/releases/1.12.0/crm-runtime`.

See [assets contract](ASSETS_MODULE.md), [sanitized evidence](audit/2026-10-12-release-1.12.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.
No company pricebook, external execution or private-data disclosure boundary changes.

## Previous extension — profile 1.11.0, 12 October 2026

Shared organizations, proposals, and delivery capacity add 46 compiled artifacts, 18 unchanged external pins,
fifteen owned ledgers, host 0.14.0, config 0.13.0 and kits 0.8.0. Organizations/organizations-pg 0.1.0,
proposals/proposals-pg 0.1.0, and capacity/capacity-pg 0.1.0 run without CRM or an LLM.
Assembly v9 adds optional organizations, proposals, and capacity composition while preserving previous
contracts and four-module defaults.
State machine receipt recovery sets pending command UUID and uncertain status prior to mutation dispatch
across all domains, holding UI recovery controls open and blocking new writes on transport errors, body read
rejections, truncated JSON payloads (`{`), or receipt schema/ID mismatches.

| Extension                                     | Accepted evidence                                                                                                                                                                                                                                                                                                                                                                                                             | Limit                                                                            |
| --------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| Organizations and affiliations                | Two cold-installed wholesale/recruitment consumers; real PostgreSQL org/contact race conditions, primary affiliation uniqueness constraint (at most one active primary contact per organization), secondary contact links, revision locking, forced tenant RLS, machine write refusal, restart/restore; 24 checks                                                                                                             | Master org accounts; no automated external KYC or multi-tier corporate hierarchy |
| Agreed proposals and change requests          | Two cold-installed agency/EPC consumers; real PostgreSQL draft deliverable updates, client acceptance sealing baseline Scope Version 1 snapshot, direct-edit refusal on accepted proposal, versioned change request approval advancing scope to v2+, forced tenant RLS, machine/unrelated staff refusal, restart/restore; 30 checks                                                                                           | Contractual scope baseline; no automated legal dispute or multi-party e-sign     |
| Pooled capacity estimation                    | Two cold-installed agency/EPC consumers; real PostgreSQL business-day timeline estimation with weekend blackout (Fri-Mon = 3 cal / 1 work days), concurrency saturation queueing with dynamic handover, progress burn recalculation, completion/cancellation release, forced tenant RLS, restart/restore; 29 checks                                                                                                           | Discrete daily burn rate; no sub-hour shift roster or multi-timezone dispatch    |
| Resilient receipt recovery & host composition | Compiled host and Chromium organization/affiliation, proposal/CR, and capacity pool lifecycles; 5 failure mode regressions (truncated JSON `{`, body stream read rejection, invalid receipt schema, mismatched command UUID, grant revocation) asserting zero duplicate business records, uncertain write refusal, recovery UI assertion, same-UUID recovery, disable/re-enable, and backup/restore; 6 host checks per domain | In-memory pending browser recovery; human-only writes                            |
| Trusted assembly                              | Narrow K04/K21 `organization_accounts`, K05/K31 `proposals_scope`, K05/K31 `delivery_capacity` under v9; kits 13 and assembly 2                                                                                                                                                                                                                                                                                               | No full operating kit accepted                                                   |

All eight root gates passed: 1,540 tests/115 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9,
commerce 18/10, food-service 23/10, support 19/9, purchasing 19/11, organizations 24/6, proposals 30/6,
capacity 29/6 and upgrade 4. The security matrix has 62 controls; root/runtime scans have
zero advisories. The separate realtime template retains thirteen moderate findings and independent
SDK/live gates. These checks do not establish production load or security certification.

Synthetic upgrade from previous supported 1.10.0 to 1.11.0 passed all 4 checks on an
isolated PostgreSQL cluster without altering historical records, audit logs, or command UUIDs.
Release artifacts for candidate 1.11.0 are archived in `dist/releases/1.11.0/library` and `dist/releases/1.11.0/crm-runtime`.

See [organizations contract](ORGANIZATIONS_MODULE.md), [proposals contract](PROPOSALS_MODULE.md),
[capacity contract](CAPACITY_MODULE.md), [sanitized evidence](audit/2026-10-12-release-1.11.0.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). All forty full operating kits remain unaccepted.
No company pricebook, external execution or private-data disclosure boundary changes.

## Previous extension — profile 1.10.0, 11 October 2026

Shared purchasing adds 40 compiled artifacts, 18 unchanged external pins, twelve
owned ledgers, host 0.13.0, config 0.12.0 and kits 0.7.0. Purchasing/purchasing-pg 0.1.0
run without CRM or an LLM; commerce 0.3.0 additive 003 provides inbound stock provenance.
Assembly v8 adds optional purchasing while preserving previous contracts and four-module defaults.
Existing identities require explicit independent purchasing permissions; migration does not enroll accounts.

| Extension                  | Accepted evidence                                                                                                                                                                                                                                     | Limit                                                                                            |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Supplier quotes and orders | Two cold-installed wholesale/restaurant consumers; real PostgreSQL supplier/offer races, stale draft quote refusal, immutable confirmed quotes, 1–50 lines, bounded integer IDR totals, restart/restore; 19 checks                                    | Backend supplier unit cost distinct from sales price/pricebook; no landed cost or freight        |
| Physical stock receipts    | Partial goods receipt findings, accepted + rejected <= ordered, source-bound inbound movements via commerce transaction SPI, rejected findings separated from usable stock, over-receipt refusal, short closure, multi-SKU atomic rollback; 19 checks | Initial policy: received means all units accounted for; replacement uses new PO; no auto dispute |
| Owner composition          | Compiled host and Chromium supplier creation, offers, PO create/review/confirm, goods inspection receipt, lost-response/denied recovery, short-close, viewer controls, disable/enable and backup restore; 11 checks                                   | Human writes; pending browser recovery in memory; catalog/order selector paginated up to 1,000   |
| Trusted assembly           | K02/K04/K18/K23 narrow `purchase_receipts` under v8; kits 13 and assembly 2                                                                                                                                                                           | No full operating kit accepted                                                                   |

All eight root gates passed: 1,531 tests/112 files. Fresh regression evidence
retains CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9,
accommodation 17/9, commerce 18/10, food-service 23/10, support 19/9 and upgrade 4. The security
matrix has 53 controls; root/runtime scans have zero advisories. The separate
realtime template retains thirteen moderate findings and independent SDK/live
gates. These checks do not establish production load or security certification.

The original owner was backed up before/after (`pre-purchasing-1.10.0-20261011.dump`,
`post-purchasing-1.10.0-20261011.dump`) and upgraded to verified 1.10.0.
SQL JSON hashes preserve microsecond timestamps and original rows/columns across
all 74 pre-existing tables, including eight CRM business tables and all previous
schema ledgers. Settings, identity/fingerprint keys and existing domain grants are
unchanged. Nine new purchasing business tables remain empty. Activation is explicit.

See [purchasing contract and activation](PURCHASING_MODULE.md),
[sanitized evidence](audit/2026-10-11-purchasing-release.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). Continue KIT-07 with
organizations, proposals/scopes and capacity, remaining general/specialist requirements
and full handoff. All forty full operating kits remain unaccepted. No company
pricebook, external execution or private-data disclosure boundary changes.

## Previous extension — profile 1.9.0, 11 October 2026

Shared support adds 38 compiled artifacts, 18 unchanged external pins, eleven
owned ledgers, host 0.12.0, config 0.11.0 and kits 0.6.0. Support/support-pg 0.1.0
run without CRM or an LLM. Assembly v7 adds optional support while preserving
previous contracts and four-module defaults. Existing identities require explicit
independent support permissions; migration does not enroll accounts.

| Extension                   | Accepted evidence                                                                                                                                                                                                                                                | Limit                                                                                            |
| --------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Assigned cases and findings | Two cold-installed service/property consumers; real PostgreSQL duplicate/revision races, current assignment, projection/reassignment locking, atomic failure rollback, immutable history, internal targets, resolution/reopen cycles, restart/restore; 19 checks | Internal case management; no contractual SLA or customer message                                 |
| Owner composition           | Compiled host and Chromium intake, staff assignment, investigation/wait/resume, resolution/close/reopen, lost-response/denied recovery, viewers, toggles and backup restore; 9 checks                                                                            | Human writes; pending browser recovery in memory; assignee selector first 100 with paginated API |
| Trusted assembly            | K05/K06/K12/K18/K21 narrow `support_cases` under v7; kits 12 and assembly 2                                                                                                                                                                                      | No full operating kit accepted                                                                   |
| Database shutdown           | Actual runtime pool clients, including idle-expiry sockets already closing, drain before stopping an owned database                                                                                                                                              | Local process lifecycle; no production HA guarantee                                              |

All eight root gates passed: 1,523 tests/111 files. Fresh regression evidence
retains CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9,
accommodation 17/9, commerce 18/10, food-service 23/10 and upgrade 4. The security
matrix has 47 controls; root/runtime scans have zero advisories. The separate
realtime template retains thirteen moderate findings and independent SDK/live
gates. These checks do not establish production load or security certification.

The original owner was backed up before/after and upgraded to verified 1.9.0.
SQL JSON hashes preserve microsecond timestamps and original rows/columns across
all 67 pre-existing tables, including eight CRM business tables and all previous
schema ledgers. Settings, identity/fingerprint keys and existing domain grants are
unchanged. Six new support business tables remain empty. Activation is explicit.

See [support contract and activation](SUPPORT_CASES_MODULE.md),
[sanitized evidence](audit/2026-10-11-support-release.json) and
[the programme ledger](STARTER_KIT_IMPLEMENTATION.md). Continue KIT-07 with
[purchasing](PURCHASING_MODULE_PLAN.md), remaining general/specialist requirements
and full handoff. All forty full operating kits remain unaccepted. No company
pricebook, external execution or private-data disclosure boundary changes.

## Previous extension — profile 1.8.0, 11 October 2026

Restaurant/catering service extends commerce: 36 compiled artifacts, 18 unchanged
external pins, ten owned ledgers, host 0.11.0, config 0.10.0 and kits 0.5.0.
Food-service 0.1.0 owns tables, sealed recipes and whole-ticket kitchen/service;
commerce 0.2.0 owns every shared price/stock write through its private transaction
SPI. Additive migration 002 marks managed portion products without changing
commerce migration 001 or its public product contracts. Assembly v6 preserves
older contracts and four-module defaults.

| Extension                 | Accepted evidence                                                                                                                                                                                                                         | Limit                                                                                      |
| ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| Tables/recipes/tickets    | Two cold-installed restaurant/catering consumers; real PostgreSQL table/ingredient races, frozen price/recipe snapshots, duplicate preparation, waste, expiry after lock waits, current grants, SQL integrity, restart/restore; 23 checks | Whole-ticket service; no per-item POS, financial settlement, hardware or public self-order |
| Shared commerce ownership | Non-empty archived 1.7 migration retains historical quotes/stock/grants/receipts; generic APIs refuse managed portions; preparation commits both domains/audits atomically with rollback after injected failure                           | Trusted server SPI; no machine write grant                                                 |
| Owner composition         | Actual compiled host and Chromium create tables/menu recipes, queue/prepare/ready/serve/close, cancel cooked food, recover lost responses/denied retry, viewers, toggles and backup restore; 10 checks                                    | In-memory pending browser recovery; larger ingredient catalogs use API                     |
| Trusted assembly          | Narrow K02 `foodservice_tickets` under v6; existing scopes retained; kits 11 and assembly 2                                                                                                                                               | Full operating-kit acceptance remains zero of 40                                           |

All eight root gates passed: 1,514 tests/109 files. Fresh regression evidence retains
CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9,
accommodation 17/9, commerce 18/10 and upgrade 4. The security matrix has 41
controls; root/runtime scans have zero advisories. The separate realtime template
retains thirteen moderate findings and independent SDK/live gates. Local controls
and tests do not establish production capacity, certification or full-system support.

The original owner was backed up before/after and upgraded to verified 1.8.0.
SQL JSON hashes preserve microsecond timestamps and original rows/columns in all
57 pre-existing tables; only the expected commerce ledger entry is added. Eight
CRM business tables, settings, identity/fingerprint keys and existing domain grants
remain unchanged. Nine new food business tables remain empty. Migration and flags
enroll no account; activation and food/commerce permissions are explicit.

See [food-service contract and activation](FOODSERVICE_MODULE.md),
[sanitized release evidence](audit/2026-10-11-foodservice-release.json) and
[the complete programme ledger](STARTER_KIT_IMPLEMENTATION.md). KIT-06 is accepted
for bounded commerce/food-service scopes. Continue KIT-07 with
[support cases](SUPPORT_CASES_MODULE_PLAN.md), then remaining general/specialist
requirements and full-programme handoff. No existing pricebook or provider boundary
changes; closing a restaurant service is never reported as verified payment.

## Previous extension — profile 1.7.0, 11 October 2026

Shared commerce extends profile 1.6.0: 34 compiled artifacts, 18 unchanged
external pins, nine owned ledgers, host 0.10.0, config 0.9.0 and kits 0.4.0.
Assembly v5 adds optional commerce while preserving previous contracts/defaults.
Pure catalog/order/stock code and the PostgreSQL adapter run without CRM or an LLM.
The local host requires its existing workspace identity plus explicit commerce grants.

| Extension            | Accepted evidence                                                                                                                                                                                                             | Limit                                                                                |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| Catalog/orders/stock | Two cold-installed retail/wholesale consumers; 20-request retry and last-unit races, immutable price snapshots, multi-SKU rollback, lock-wait expiry, partial restock/discard returns, scopes/RLS, restart/restore; 18 checks | One stock pool; integer unit/gram/millilitre; IDR estimates; no financial settlement |
| Owner composition    | Actual compiled host and Chromium catalog/cart/reserve/fulfill/return flow; lost-response UUID recovery, viewers, module toggles and restored database; 10 checks                                                             | Human writes; in-memory pending browser command; no NL order routing                 |
| Trusted assembly     | K03/K04 bounded `commerce_orders` composes under v5; earlier narrow scopes retained; kits 10 and assembly 2                                                                                                                   | All forty full operating kits remain unaccepted                                      |
| Preserved behavior   | Eight root gates; 1,506 tests/108 files; fresh CRM 38, Act 18, runtime 44, Chromium 3/70, projects 15/11, scheduling 14/9, accommodation 17/9 and upgrade 4                                                                   | Local controls and regression evidence, not production/load certification            |

The security matrix has 36 controls. Root/runtime dependency scans have zero
advisories; the separate realtime template retains thirteen moderate findings and
its own SDK/live gates. Restart proofs now await completed pool-client removal
before stopping their isolated PostgreSQL clusters.

The original owner was backed up before/after and upgraded to verified 1.7.0.
SQL JSON snapshots preserve microsecond timestamps: all original rows/columns in
46 pre-existing tables, eight CRM business tables, settings, keys and domain grants
remain unchanged. Ten new commerce business tables remain empty. Enabling the
feature and granting accounts are explicit operations; migration enrolls no account.

See [commerce contract, activation and proofs](COMMERCE_MODULE.md),
[sanitized release evidence](audit/2026-10-11-commerce-release.json) and
[the complete work ledger](STARTER_KIT_IMPLEMENTATION.md). Continue with
[food-service ownership](FOODSERVICE_MODULE_PLAN.md) and the remaining programme;
this accepted slice does not complete restaurant, retail or wholesale full kits.
Pricing, tax, invoice and payment authority retain their existing boundaries.

## Previous extension — profile 1.6.0, 11 October 2026

The accommodation domain and PostgreSQL adapter extend profile 1.5.0: 32 compiled
artifacts, 18 unchanged external pins, eight owned ledgers, host 0.9.0, config
0.8.0 and kits 0.3.0. Scheduling 0.2.0 owns the shared capacity kernel through
additive migration 002; migration 001 remains unchanged. Assembly v4 adds optional
accommodation while preserving earlier compositions and four-module defaults.
Standalone hotel/villa consumers need neither CRM nor an LLM. Owner-host access
requires explicit accommodation grants plus current identity/workspace checks.

| Extension         | Accepted evidence                                                                                                                                                                                                | Limit                                                                     |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Room-night stays  | Two independently installed hotel/villa consumers; real PostgreSQL duplicate/overlap races, frozen rates, DST, expiry after lock waits, assigned scopes, readiness and overdue guest; restart/restore; 17 checks | Individual assigned rooms; internal confirmation; no full PMS/payment/OTA |
| Shared capacity   | Non-empty historical 1.5 migration preserves original records and receipts; generic APIs refuse managed hotel rooms; stay, quote, audit and capacity commit atomically                                           | Kernel is a private trusted-server boundary                               |
| Owner composition | Actual compiled host and Chromium create property/type/room, clean/inspect, hold/confirm/check-in/out; lost-response recovery, viewer refusal, disable/re-enable and restore; 9 checks                           | Human writes; memory-only browser recovery; rate updates via API          |
| Trusted assembly  | K01 bounded `accommodation_stays` composes under v4; earlier narrow scopes remain accepted                                                                                                                       | All forty full operating kits remain blocked                              |

See [accommodation contract, activation and proofs](ACCOMMODATION_MODULE.md),
[sanitized release evidence](audit/2026-10-11-accommodation-release.json) and
[the complete programme ledger](STARTER_KIT_IMPLEMENTATION.md). No existing
account is automatically enrolled in hotel permissions. Quotes do not change the
approved ARCHZXY pricebook or represent verified payment. Next: orders/retail/food
service. `pnpm security:verify` includes the new domain and host evidence and
rejects stale release profiles.

All eight root gates passed: 1,498 tests across 107 files. Fresh regression evidence
retains CRM 38, Act 18, runtime 44, Chromium 3/70, project 15/11, scheduling 14/9,
assembly 2 and upgrade 4 checks; kit proof now has 9 checks. The security matrix
has 32 controls, with zero root/runtime dependency advisories. The separate
realtime template retains thirteen moderate findings and its own live/SDK gates.
These are local controls and tests, not security certification or production load
acceptance.

The original owner was backed up before/after and upgraded to verified 1.6.0.
Hashes of original rows and columns in all 36 pre-existing tables were retained;
the scheduling ledger gained its expected additive entry and resources gained the
new management column. Eight CRM business tables, keys, settings and existing
domain grants stayed unchanged. The new hotel's eight business tables are empty,
with no automatic account enrollment. Activation remains explicit.

## Previous extension — profile 1.5.0, 11 October 2026

The reusable scheduling domain and PostgreSQL adapter extend profile 1.4.0:
30 internal artifacts, 18 unchanged external pins, seven owned database ledgers,
host 0.8.0, config 0.7.0 and kits 0.2.0. New assembly v3 supports optional
`scheduling`; existing v1/v2 compositions and four-module defaults remain supported.
The pure module needs no CRM or LLM. The existing host composes it with current
identity/workspace membership plus independent scheduling grants.

| Extension                    | Accepted evidence                                                                                                                                                                                                                                                   | Limit                                                                     |
| ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| Exclusive timed reservations | Two independently installed consumers use the same implementation for staff appointments and individual asset rental; PostgreSQL current-grant/RLS checks, duplicate/overlap races, revision rollback, expiry during a proven lock wait, restart/restore; 14 checks | Capacity one; explicit instants; no complete appointment/rental/hotel kit |
| Local owner composition      | Auth/CSRF/current grants, real Chromium resource/hold/confirm/move/cancel flow, response-loss recovery, viewer refusal, disable/re-enable and fresh backup restore; 9 checks                                                                                        | Human writes; in-memory browser recovery; blocks/audit details via API    |
| Trusted composition          | K07/K10 narrow `resource_reservations` scope composes through v3; updated 8-check kit proof and retained v1/v2 scopes                                                                                                                                               | All 40 full operating kits remain blocked on mandatory gaps               |
| Preserved behavior           | Eight root gates, 1,485 tests/105 files; fresh CRM 38, Act 18, runtime 44, Chromium 3/70, project 15/11 and upgrade 4                                                                                                                                               | Local/synthetic scope; no new live-provider acceptance                    |
| Security and owner migration | 27 controls, zero supported root/runtime advisories; original 30 tables, keys/settings/grants retained; verified doctor and backups before/after                                                                                                                    | Template 13 moderate findings remains separate; no security certification |

[Sanitized release evidence](audit/2026-10-11-resource-reservations-release.json)
records current artifact hashes and runnable reports. See
[the module contract and activation](RESOURCE_RESERVATIONS_MODULE.md) and
[the complete programme ledger](STARTER_KIT_IMPLEMENTATION.md). No existing account
receives scheduling access automatically and no pricing/provider policy is changed.

`pnpm security:verify` also runs scheduling domain/host proofs; acceptance rejects
stale profile evidence. Availability is a database observation: only a successfully
committed hold reserves a slot. Confirmation is an internal operator action and
has no customer agreement, financial or external-provider meaning. Next build the
accommodation domain's room-night/stay/housekeeping rules.

## Previous extension — profile 1.4.0, 11 October 2026

The accepted 1.3.0 baseline below is extended by three private artifacts: `kits`,
`projects` and `projects-pg`. There are 28 internal artifacts, 18 unchanged external
pins, host 0.7.0, config 0.6.0 and six owned database ledgers. The project schema is
additive and does not migrate CRM records or automatically grant account access.

| Extension                                 | Accepted evidence                                                                                                                                                       | Limit                                                            |
| ----------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| Trusted catalogue and requirement planner | 40 immutable families; exact installed-profile checks; positive/narrow and missing/forged-authority refusals                                                            | No complete operating kit accepted                               |
| Independent consumption and assembly      | Two cold-installed kit consumers, browser-safe contracts and scoped v1/v2 assembly; 7 checks                                                                            | Full kit requirements stay blockers; assembly is configuration   |
| Persistent project domain                 | Two independent consumers; real PostgreSQL grants/RLS, 20-request idempotency race, parent revisions, delivery invariants, SQL immutability, restart/restore; 15 checks | Internal scope/task/milestone tracking only                      |
| Owner composition                         | Explicit grants, authenticated APIs/read tools, Chromium workflow and response-loss recovery, disable/re-enable and backup/account recovery; 11 checks                  | No project machine writes, client contract, invoice or payment   |
| Existing behaviour                        | Eight root gates; 1,475 tests/104 files; CRM 38, Act 18, host 44, Chromium 3/70, prior-profile upgrade 4                                                                | Local/synthetic acceptance                                       |
| Security extension                        | 22 applicable controls; fresh root/runtime zero findings; exact dependency inventory                                                                                    | Template 13 moderate findings remains its separate realtime gate |

[Sanitized current evidence](audit/2026-10-11-kit-project-release.json) links the
executed reports. The original owner state was backed up before and after additive
migration; all eight CRM tables, existing grants and identity/fingerprint keys are
unchanged. Doctor verifies the new profile. Project activation remains explicit.

Assembly 2.0.0 adds optional `projects`; assembly 1.0.0 and original four-module
settings still work. Narrow `crm_inquiry` (K01/K05) and `project_tracking` (K05/K31)
are accepted configuration scopes. Both full services and EPC requests remain
blocked on missing domains/capabilities. The approved pricebook is unchanged.
See [project integration and limitations](PROJECT_DELIVERY_MODULE.md) and
[the full work ledger](STARTER_KIT_IMPLEMENTATION.md).

`pnpm security:verify` now also invokes kit, project and project-host proofs. The
runtime report records its actual verified profile; security verification rejects
stale runtime/kit/project proof profiles before accepting the matrix.

## Historical baseline — profile 1.3.0

The finite acceptance scope is PRD 2.1 §50. Company website, live voice/avatar,
public deployment and the broader ERP catalogue remain separate milestones.
Evidence below describes executed checks, not product/security percentages.

| Item                               | Status   | Evidence                                                                                                                                                                             |
| ---------------------------------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| EXEC-01 supported profile          | Accepted | 8 profile tests; identical cold consumer implementations; exact installed hashes/dependencies/schema and changed-package startup refusal; final host proof 44 checks.                |
| EXEC-02 composition/lifecycle      | Accepted | Typed planner/assembly; independent minimal/full installs; disabled routes/writes/tools; retained receipts and unresolved-effect refusal; minimal Chromium DOM proof.                |
| EXEC-03 presence boundary          | Accepted | Six gateway tests; installed chat and standalone Python transcript share server policy; forged identity, binding, revision, replay, expiry, private-tool and timeout refusal.        |
| EXEC-04 security/dependency matrix | Accepted | 15 source/control evidence rows; 38 real PostgreSQL CRM checks, 18 Act checks, 44 host checks; exact external inventory; root/runtime scans clean; separate template gates recorded. |
| EXEC-05 isolated capacity baseline | Accepted | Two tenants, 200 additional contacts each plus tasks/opportunities; 32 warm-up reads; 1/5/20 concurrency for 15s each, zero failures; 20 duplicate writes commit/recover once.       |
| EXEC-06 upgrade/recovery/handoff   | Accepted | Cold prior-source upgrade and fresh-target restore; Chromium minimal/full 3/70; eight root gates, 1,458 tests/102 files; template JS 55/guardrails 4/build; handoff and manifest.    |

The host embeds its trusted resolved profile at build time. The generated
`dist/library/release-profile.json` is an inventory; replacing its sidecar does
not change the host's supported implementation. Package content, internal
dependency versions, migration files and authoritative schema ledgers are
validated before CRM startup. This detects incompatible/tampered installations;
it is not a signature/provenance system against an attacker able to replace
the host binary and its trust root.

Package hashes cover implementation, declarations, migrations and semantic
package metadata. npm normalizes JSON whitespace, so JSON formatting is excluded
from the package metadata digest. Tarball byte hashes remain recorded separately.

Reproduce the profile checks with `pnpm test -- packages/config/test/release.test.ts`,
`pnpm reuse:verify`, `pnpm crm:build` and `pnpm crm:verify-runtime` on the pinned
Node/pnpm versions. Runtime verification creates and removes an isolated private
PostgreSQL installation; it does not replace the owner's data or call providers.

The historical baseline profile is `1.3.0` (host `0.6.0`, config `0.5.0`). Initial modules
are mandatory `contacts` (including activities/follow-ups), optional `pipeline`,
optional `assistant`, and `nativeActions` requiring `assistant`. No database data
is dropped. This is a logical composition within a modular monolith: the supported
installation keeps the common artifact closure; selection controls public surfaces
and execution, not uninstalling database ownership. Module metadata and the typed
`planAssembly` API ship in `@archava/config/assembly`. Legacy planning names in
the plan remain unresolved, never implicit installed capabilities.

After `pnpm crm:build`, use `pnpm library:assemble selection.json NEW_DIRECTORY
--dry-run`, then the same command without `--dry-run`. Selection has
`version: "1.0.0"`, the exact `profile`, and `modules` with all four boolean keys
(`contacts` must be true). Generated entries reference installed package exports;
no CRM implementation is copied. Run `npm install --ignore-scripts --no-audit
--no-fund` inside the new private consumer. `pnpm assembly:verify` reproduces the
independent-install, hash, dry-run, dependency and overwrite checks.

For an existing owner host, stop it, place the four-key module selection in a
privately owned mode-0600 file, and run `pnpm crm:modules /absolute/selection.json`.
Restart the host. Old profiles without module selection keep the previous full
behavior. Native disable refuses pending/executing/unknown intents; reconcile or
cancel through the original authorized host first. Startup independently checks
this policy even after manual settings edits. A transient operator connection
only checks unresolved-state existence; request execution still uses the existing
non-owner application role with RLS and current grants. Receipt GET recovery and
pending cancellation remain reconciliation operations, never new execution grants.

The shared `@archava/presence` boundary has strict versioned contracts and a
server-only session gateway. `POST /api/presence/session` accepts only a configured
`tenantId`; identity comes from a verified owner cookie or a server-issued guest
cookie. Owner presence uses current CRM authorization and the offline planner;
guests receive public help only. Neither transport accepts roles, grants, SQL,
approval tokens or execution tools. `/context` accepts an allowlisted section and
a strictly increasing revision; `/turn` binds the current revision, session and
request UUID. Sessions expire within 120 seconds (hard maximum 300); restart
invalidates these ephemeral scopes. Capacity and per-session requests are bounded.
Timeout aborts read work, reports a refusal and keeps the lane busy until late
work settles. Presence creates no new mutation/consent route.

Both public and owner replies use the existing `runTurn` intelligence/policy
implementation. The Python example is a standalone HTTP transport using only its
standard library and public guest scope. It runs from a copied isolated directory
with no owner/database environment. Synthetic transcript evidence is not a live
audio/avatar test. The existing Template LiveKit worker remains an independent
realtime consumer and retains its protocol; connecting its live provider path to
this gateway is a separate gated migration.

`pnpm security:verify` reruns unit, real PostgreSQL CRM/Act/host, reuse and
assembly proofs, then checks the control matrix against their executed labels.
It also scans both the root lock and the installed runtime's npm lock.
The [sanitized matrix](audit/local-library/security.json) records 15 applicable
controls, exact runtime dependencies, licenses/notices and both lock digests.
Each external runtime version is pinned in the host-owned catalogue, installed
using those pins and checked at startup. These pins capture the tested dependency
combination; they are not a vulnerability-driven forced downgrade.

The template's 13 moderate findings derive from
[GHSA-8988-4f7v-96qf](https://github.com/advisories/GHSA-8988-4f7v-96qf):
oversized inbound baggage can allocate excess memory; transport header limits
reduce the HTTP impact but do not prove safety of arbitrary transports. The
owner profile does not install this dependency chain. Template dependency/client
reachability has not been proved; vendor-compatible remediation and its missing
seven Python SDK integration checks remain open realtime release gates. No SDK
downgrade, override or claim of unexploitable code is inferred from scanner output.

External SPDX metadata and shipped notice files are inventoried; generated
`dist/security-proof/third-party-notices.txt` contains their included notices.
Internal artifacts remain private with no declared public license. Public
distribution and client IP terms need their own legal/contract review. This is
scoped engineering evidence, not a penetration test or certification.

`pnpm crm:benchmark` reruns the disposable persistent host fixture and then seeds
200 additional contacts per tenant, 20 tasks and 10 opportunities. It warms up
32 authenticated reads and runs a mixed contacts/worklist/pipeline/overview load
for 15 seconds at each concurrency. Login and fixture creation are outside the
timed workload. Only the fixture's loopback host and socket directory are accepted;
the original database and external providers cannot be benchmark targets.

The [capacity report](audit/local-library/capacity.json) includes runtime/database
versions, machine resources, fixture sizes, samples, throughput, p50/p95, failures
and the separate 20-request mutation/recovery race. The observed read envelope is
for this machine, fixture and eight-connection app pool. It is not a production
SLA or number of supported users.

The six finite items are accepted for local profile `1.3.0`.
[Handoff](LOCAL_LIBRARY_HANDOFF.md) covers cold setup, independent assembly,
account recovery, upgrade and binary/schema compatibility. The
[sanitized manifest](audit/local-library/manifest.json) links the generated
profile, identical-consumer proof, control matrix, capacity, upgrade and browser
results. The final eight root gates passed with 1,458 tests in 102 files; CRM
38, durable Act 18, persistent host 44 and Chromium minimal/full 3/70 passed.
Template regression passed 55 JavaScript tests, four Python guardrail checks
and build; its seven SDK integration checks and live gates remain open.

The existing owner's installation passed `crm:doctor` with the exact supported
profile and two workspaces. Its enabled backup was due while the HTTP host was
stopped; `crm:backup-run` published a new intact private snapshot, retaining all
previous archives. Doctor now reports fresh backup. Tests used synthetic isolated
data; the real owner database was only diagnosed and backed up through its
existing policy. No real record, grant, credential or pricebook was changed.
