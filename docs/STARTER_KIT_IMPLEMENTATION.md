# Starter-kit implementation work

The user authorized completing the researched kit programme on 10 October 2026.
The objective covers the 40-family catalogue and usable domain implementations,
not only metadata. Local/free constraints and existing action/privacy boundaries
continue to apply. ARCHZXY's website remains an independent consumer project.

On 11 October the user requested uninterrupted execution across milestones until
the authorized programme is complete. Continue after accepted slices; milestone
reports are progress, not reasons to stop. Identify required external access or
domain decisions concretely while independent work continues. Full-kit acceptance
still requires declared domain/capability evidence; paid services and fabricated
support are not authorized by this instruction.

## Work ledger

| Work                                                   | Acceptance                                                                                                                                                                             | Current state                                                                                                                                   |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| KIT-01 typed trusted catalogue                         | Forty families, explicit domain ownership/configuration/API/action/UI/operability requirements; immutable public contracts; no implicit support from research status                   | Accepted: local catalogue/planner scope                                                                                                         |
| KIT-02 deterministic requirement and readiness planner | Every selected requirement has a disposition; full kits with missing implementations refuse assembly; narrow accepted scopes remain explicitly narrow; typed API and local CLI         | Accepted: local catalogue/planner scope                                                                                                         |
| KIT-03 packaged consumption                            | Two cold-installed consumers use the same catalogue/planner without source imports; browser-safe contracts; existing profile/CRM behaviour preserved                                   | Accepted: verified independent consumers                                                                                                        |
| KIT-04 project services                                | A bounded persistent project/task/milestone workflow through authorized API/dashboard; scope/revision/audit/recovery; reuse without CRM forks                                          | Accepted: bounded local tracking                                                                                                                |
| KIT-05 scheduling and accommodation                    | Time/resource scheduling and domain-specific room-night/stay/housekeeping ownership; concurrency/cancellation/restore evidence                                                         | Accepted: bounded reservations and accommodation                                                                                                |
| KIT-06 orders, retail and food service                 | Catalogue/orders/stock and restaurant table/kitchen rules; validated composition and failure semantics                                                                                 | Accepted: bounded commerce and food service                                                                                                     |
| KIT-07 remaining general kits                          | Build the 24 general families' explicit scopes from shared services and separate extensions; each needs its own evidence                                                               | In progress: bounded support, purchasing, organizations, proposals/scope, capacity, assets, subscriptions, attendance, and fulfillment accepted |
| KIT-08 specialist kits                                 | Domain review plus selected authoritative-system adapters, supported configuration, privacy and live integration evidence; 16 research families are not native full-system commitments | Pending                                                                                                                                         |
| KIT-09 operator handoff                                | Reproducible releases, migration/recovery, acceptance manifest and honest support catalogue                                                                                            | Pending                                                                                                                                         |

No candidate is promoted to accepted from a file count, generated manifest, UI
visibility, mocked provider response or a generic CRUD engine. Specialist adapter
implementation depends on the selected backend's contract and domain scope;
unselected/unconfigured integrations remain blockers. Existing accepted library
profile 1.3.0 is historical evidence, not automatic acceptance of new packages.

Scope can be made narrower only when recorded explicitly; this ledger retains
outstanding work until it has real acceptance. No company website, pricebook change,
paid call, private-data AI disclosure or public publishing follows from kit selection.

## First implementation slice

The operator selected project delivery first because ARCHZXY can use it internally.
The user explicitly delegated ordering while retaining the full catalogue target.
Profile 1.4.0 adds `@archava/kits`, `@archava/projects` and `@archava/projects-pg`
to the prior 25-artifact release: 28 internal artifacts, unchanged 18 external pins,
one additive project migration and six owned database ledgers. Host 0.7.0 and
configuration package 0.6.0 retain prior settings and four-module defaults.

The planner supports narrow `crm_inquiry` for K01/K05 and `project_tracking` for
K05/K31. The latter is internal generic project tracking; construction/EPC-specific
costing, safety, procurement and contractual workflows are not implemented.
All forty full operating kits remain candidates and refuse assembly when their
domain/capability requirements are missing. No kit has full operational acceptance.

See [project delivery scope and integration](PROJECT_DELIVERY_MODULE.md). The next
domain slice was scheduling/resource reservations, followed by accommodation and
orders/retail/food service. This work order does not remove the remaining kit work.

## Second implementation slice

Profile 1.5.0 adds `scheduling` and `scheduling-pg` 0.1.0: 30 internal artifacts,
18 unchanged external pins and seven owned database ledgers, with host 0.8.0,
config 0.7.0 and kits 0.2.0. Assembly v3 adds optional scheduling while preserving
v1/v2 compositions and old defaults. The independent appointment/rental consumers
share the same implementation for exclusive timed reservations; full K07/K10
operating kits remain blocked. See [reservation scope](RESOURCE_RESERVATIONS_MODULE.md).

KIT-05 scheduling foundation is accepted locally for capacity-one resources,
holds/expiry, confirmation, moves/cancellation, maintenance blocks, current grants,
idempotency/audit and restart/restore. At the 1.5.0 checkpoint accommodation was pending: room-night
inventory, room types/tariffs, guest stays, check-in/out and housekeeping need
separate domain rules. Next implement that bounded accommodation extension, then
orders/retail/food service. Remaining general/specialist families and full-programme
handoff remain in this ledger. No complete operating kit is promoted by this slice.

## Third implementation slice

Profile 1.6.0 adds accommodation 0.1.0 and scheduling's shared capacity kernel
0.2.0: 32 compiled artifacts, 18 unchanged external pins, eight owned database
ledgers, host 0.9.0, config 0.8.0 and kits 0.3.0. Assembly v4 adds optional
accommodation and preserves earlier contracts and defaults. Hotel/villa consumers
install identical domain code. Narrow K01 `accommodation_stays` now composes
individual rooms, frozen nightly rates, guest stay lifecycle and housekeeping.

KIT-05 is accepted for these bounded local scheduling and accommodation scopes.
It does not accept full appointment, rental or hotel kits: group/pooled bookings,
rate calendars, OTA/public booking, financial flows and other explicit requirements
remain open. See [accommodation scope and proof](ACCOMMODATION_MODULE.md).

Next implement KIT-06 shared catalogue/orders/stock and explicit retail and
restaurant extensions. KIT-07/08/09 and all forty full operating kits retain their
outstanding acceptance requirements. Current release acceptance is recorded in
[the release ledger](LOCAL_LIBRARY_RELEASE.md).

## Fourth implementation slice

Profile 1.7.0 adds commerce 0.1.0 and its PostgreSQL adapter: 34 artifacts,
18 unchanged external pins, nine owned ledgers, host 0.10.0, config 0.9.0 and kits
0.4.0. Assembly v5 adds optional commerce without changing earlier contracts or
defaults. Independent retail/wholesale consumers share one catalog/order/stock
implementation; narrow K03/K04 `commerce_orders` composes the validated scope.

The domain proves frozen prices, atomic expiring stock reservations, fulfillment,
partial restock/discard returns, current authority, idempotency, audit and recovery.
KIT-06 remains in progress until explicit restaurant ownership also passes its
acceptance. Continue with [food-service implementation](FOODSERVICE_MODULE_PLAN.md),
then KIT-07/08/09. All forty full operating kits retain their missing requirements;
no milestone completion ends the authorized programme. See
[commerce contract](COMMERCE_MODULE.md) and [release evidence](LOCAL_LIBRARY_RELEASE.md).

## Fifth implementation slice

Profile 1.8.0 adds food-service 0.1.0 and commerce's shared domain SPI 0.2.0:
36 artifacts, 18 unchanged external pins, ten owned ledgers, host 0.11.0, config
0.10.0 and kits 0.5.0. Assembly v6 adds optional restaurant service, requiring
shared commerce in the owner host; previous contracts and defaults are preserved.
The additive managed-menu migration preserves non-empty archived 1.7 records.

KIT-06 is accepted for bounded catalog/orders/stock and whole-ticket restaurant/
catering operation: physical tables, sealed recipe snapshots, backend prices,
ingredient reservation/consumption, kitchen/service progression and waste/refusal.
It does not accept any full operating kit. Per-item POS, financial flows, hardware,
public ordering and every other declared kit gap remain open. See
[food-service contract](FOODSERVICE_MODULE.md) and [release evidence](LOCAL_LIBRARY_RELEASE.md).

Continue KIT-07 with [shared support cases](SUPPORT_CASES_MODULE_PLAN.md), then
remaining general/specialist requirements and full-programme handoff. Specialist
backend selection/access and domain review remain explicit gates rather than
invented integration evidence. The user's uninterrupted-execution instruction
continues across this accepted slice.

## Sixth implementation slice

Profile 1.9.0 adds support/support-pg 0.1.0: 38 artifacts, 18 unchanged external
pins, eleven owned ledgers, host 0.12.0, config 0.11.0 and kits 0.6.0. Assembly v7
preserves previous contracts/defaults. Independent professional-service and
property-operations consumers use the same case/notes/assignment/resolution code.
Narrow K05/K06/K12/K18/K21 `support_cases` scopes compose current support evidence.

KIT-07 is in progress. Support acceptance covers private internal cases, current
resource assignment, investigation/waiting, explicit resolution/closure/reopening,
immutable prior cycles, internal targets, UUID recovery and restart/restore. It
accepts no contractual SLA, customer delivery, legal/clinical workflow or full kit.
See [support contract](SUPPORT_CASES_MODULE.md) and [release evidence](LOCAL_LIBRARY_RELEASE.md).

Continue with [purchasing and physical receipts](PURCHASING_MODULE_PLAN.md), then
remaining general/specialist requirements and programme handoff. All forty full
operating kits still retain their declared gaps. The uninterrupted-execution
instruction continues across this checkpoint.

## Seventh implementation slice

Profile 1.10.0 adds `purchasing` and `purchasing-pg` 0.1.0 alongside commerce
0.3.0 (with additive `003_purchase_receipts.sql` migration, private transaction SPI
`receivePurchaseStock`): 40 compiled artifacts, 18 unchanged external pins, twelve
owned ledgers, host 0.13.0, config 0.12.0 and kits 0.7.0. Assembly v8 preserves
previous contracts/defaults and introduces optional purchasing composition.

The domain proves supplier quotes, backend offer validation, reviewed drafts,
confirmed POs, physical partial inspection, rejection findings, stock receipts
into a single shared commerce stock pool, forced tenant RLS, machine mutation
denial, restart/restore, and owner data baseline preservation.
Narrow K02/K04/K18/K23 `purchase_receipts` scopes compose current purchasing evidence.

KIT-07 continues with Organizations, Proposals/Scope, and Capacity Estimation.

## Eighth implementation slice

Adds `organizations` and `organizations-pg` 0.1.0: 42 compiled artifacts,
18 unchanged external pins, thirteen owned ledgers (`001_organizations_affiliations.sql`),
and assembly v8 optional composition. Independent wholesale (`K04`) and
recruitment (`K21`) consumers share identical domain code and browser-safe dashboards.
Narrow `organization_accounts` scope proves organization master data, primary
affiliation uniqueness constraint (at most one active primary contact per organization),
secondary contact links, revision locking, forced tenant RLS, and restart/restore.

## Ninth implementation slice

Adds `proposals` and `proposals-pg` 0.1.0: 44 compiled artifacts, 18 unchanged
external pins, fourteen owned ledgers (`001_proposals_scope_amendments.sql`),
and assembly v8 optional composition. Independent agency (`K05`) and construction/EPC
(`K31`) consumers share identical domain code and browser-safe dashboards.
Narrow `proposals_scope` scope proves proposal draft deliverables, client acceptance,
immutable baseline Scope Version 1 snapshot, versioned change request amendments
advancing scope to v2+, forced tenant RLS, and restart/restore.

## Tenth implementation slice

Adds `capacity` and `capacity-pg` 0.1.0: 46 compiled artifacts, 18 unchanged
external pins, fifteen owned ledgers (`001_delivery_capacity_pools.sql`),
and assembly v8 optional composition. Independent agency (`K05`) and construction/EPC
(`K31`) consumers share identical domain code and browser-safe dashboards.
Narrow `delivery_capacity` scope proves capacity pool windows, deterministic daily
burn-rate calculations, queueing upon concurrency saturation, dynamic finish handover,
burned-unit progress updates, slot completion/cancellation release, forced tenant RLS,
and restart/restore.

## Eleventh implementation slice

Adds `assets` and `assets-pg` 0.1.0: 48 compiled artifacts, 18 unchanged external pins,
sixteen owned ledgers (`001_assets_custody_maintenance.sql`, reaching 16/16 schema capacity),
host 0.15.0, config 0.14.0, kits 0.9.0, and assembly v10 optional composition. Independent
vehicle/equipment rental (`K10`) and repair workshop (`K18`) consumers share identical domain
code and browser-safe dashboards.
Narrow `asset_register` scope proves serialized item identity, business-owned vs customer-owned
custody tracking, custody dispatch/return handovers with condition grading, preventive and
corrective maintenance logs with replaced parts audit, optimistic revision locking, forced tenant RLS,
machine mutation denial, resilient receipt recovery state machine, and restart/restore.

## Twelfth implementation slice

Adds `subscriptions` and `subscriptions-pg` 0.1.0: 50 compiled artifacts, 18 unchanged external pins,
seventeen owned ledgers (`001_subscriptions_entitlements_usage.sql`), host 0.16.0, config 0.15.0, kits 0.10.0,
and assembly v11 optional composition. Independent SaaS sales (`K09`) and gym/club (`K08`) consumers
share identical domain code and browser-safe dashboards.
Narrow `subscriptions_entitlements` scope proves recurring subscription lifecycles, explicit entitlements
(`metered`, `seat_count`, `boolean_access`, `pass_bundle`), backend payment confirmation, quota exhaustion refusal,
idempotent usage consumption audit, optimistic revision locking, forced tenant RLS, machine mutation denial,
resilient receipt recovery state machine (S1), and restart/restore.

## Thirteenth implementation slice

Adds `attendance` and `attendance-pg` 0.1.0: 52 compiled artifacts, 18 unchanged external pins,
eighteen owned ledgers (`001_attendance_sessions_rosters.sql`), host 0.17.0, config 0.16.0, kits 0.11.0,
and assembly v12 optional composition. Independent training/course (`K13`) and event/meetup
(`K15`) consumers share identical domain code and browser-safe dashboards.
Narrow `attendance_roster` scope proves bounded session creation with seat capacity, roster registration,
verified check-in events with per-roster idempotency keys, duplicate scan refusal and idempotent replay,
optimistic revision locking, session enrollment/attended counters, read-only tool boundaries, superuser and
schema-owner refusal, service-principal and inactive/zero-grant refusal, forced tenant RLS isolation,
immutable audit, resilient receipt recovery state machine (S1), and idempotent restart recovery.

All forty full operating kits continue to retain their declared domain and adapter gaps.
No complete operating kit is promoted by these narrow scopes.

## Fourteenth implementation slice

Adds `fulfillment` and `fulfillment-pg` 0.1.0: 54 compiled artifacts, 18 unchanged external pins,
nineteen owned ledgers (`001_fulfillment_jobs_evidence.sql`), host 0.18.0, config 0.17.0, kits 0.12.0,
and assembly v13 terminal composition. Independent warehouse/logistics (`K20`) and simple made-to-order
production (`K23`) consumers share identical domain code and browser-safe dashboards.
Narrow `fulfillment_dispatch` scope proves bounded dispatch and work-order evidence with a monotonic
receive-store-pick-pack-dispatch-deliver custody chain, returns gated on a prior dispatch, delivery
findings under a one-clear-per-reconciliation gate that blocks completion, material/output/scrap counters
kept on separate axes over append-only job lines, dedicated dispatch and produce grants, superuser and
schema-owner refusal, service-principal and inactive/zero-grant refusal, forced tenant RLS isolation,
immutable audit, resilient receipt recovery state machine (S1), and idempotent restart recovery.

All forty full operating kits continue to retain their declared domain and adapter gaps.
No complete operating kit is promoted by these narrow scopes.

## Fifteenth implementation slice

Adds `billing` and `billing-pg` 0.1.0: 56 compiled artifacts, 18 unchanged external pins,
twenty owned ledgers (`001_billing_invoices_settlement.sql`), host 0.19.0, config 0.18.0, kits 0.13.0,
and assembly v14 terminal composition. Independent asset/equipment rental (`K10`) and outpatient
clinic (`K25`) consumers share identical domain code and browser-safe dashboards.
Narrow `billing_invoicing` scope proves immutable invoice intent with a priced snapshot frozen at
creation, issue-time snapshot freezing, append-only correction chains that adjust what is owed without
rewriting a priced total, payment attempts that carry no field capable of asserting a result, settlement
recorded only by a correctly signed provider notification under the domain-separated
`archava/billing/provider-event/v1` digest, tampered-amount/foreign-key/foreign-domain/unconfigured-provider
refusals, the `billing.write` grant being required even for a correctly signed event, dedicated
issue/correct/void grants separate from write, refund reopening `settled → issued` while `void` stays terminal,
failed notifications moving no state at all, superuser and schema-owner refusal, service-principal and
inactive/zero-grant refusal, forced tenant RLS isolation, immutable audit, resilient receipt recovery
state machine (S1), and idempotent restart recovery.

All forty full operating kits continue to retain their declared domain and adapter gaps.
No complete operating kit is promoted by these narrow scopes.

## Sixteenth implementation slice

Adds `laundry` and `laundry-pg` 0.1.0: 58 compiled artifacts, 18 unchanged external pins,
twenty-one owned ledgers (`001_laundry_orders_processing.sql`), host 0.20.0, config 0.19.0, kits 0.14.0,
and assembly v15 terminal composition. Independent kilo laundry (`K19` weight) and dry
cleaning/steam pressing (`K19` item) consumers share identical domain code and browser-safe dashboards.
Narrow `laundry_processing` scope proves counted intake lines validated server-side with frozen
`line_total_minor` and `total_minor` never carried in the client payload, a monotonic
intake-tag-wash-dry-press-quality_check-ready-handover chain that refuses any step before intake and any
repeat or backwards step, quality findings under a one-clear-per-reconciliation gate that raises and
lowers `unreconciled_count` and refuses `ready` while an exception is open and until a quality check
exists, dedicated `laundry.process` machine grant separate from the counter `laundry.write` grant,
separate piece and gram accumulation counters, terminal `returned`/`canceled` states refusing further
evidence, superuser and schema-owner refusal, service-principal and inactive/zero-grant refusal, forced
tenant RLS isolation, immutable audit, resilient receipt recovery state machine (S1), and idempotent
restart recovery.
All forty full operating kits continue to retain their declared domain and adapter gaps.
No complete operating kit is promoted by these narrow scopes.
