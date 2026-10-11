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

| Work                                                   | Acceptance                                                                                                                                                                             | Current state                                    |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------ |
| KIT-01 typed trusted catalogue                         | Forty families, explicit domain ownership/configuration/API/action/UI/operability requirements; immutable public contracts; no implicit support from research status                   | Accepted: local catalogue/planner scope          |
| KIT-02 deterministic requirement and readiness planner | Every selected requirement has a disposition; full kits with missing implementations refuse assembly; narrow accepted scopes remain explicitly narrow; typed API and local CLI         | Accepted: local catalogue/planner scope          |
| KIT-03 packaged consumption                            | Two cold-installed consumers use the same catalogue/planner without source imports; browser-safe contracts; existing profile/CRM behaviour preserved                                   | Accepted: verified independent consumers         |
| KIT-04 project services                                | A bounded persistent project/task/milestone workflow through authorized API/dashboard; scope/revision/audit/recovery; reuse without CRM forks                                          | Accepted: bounded local tracking                 |
| KIT-05 scheduling and accommodation                    | Time/resource scheduling and domain-specific room-night/stay/housekeeping ownership; concurrency/cancellation/restore evidence                                                         | Accepted: bounded reservations and accommodation |
| KIT-06 orders, retail and food service                 | Catalogue/orders/stock and restaurant table/kitchen rules; validated composition and failure semantics                                                                                 | In progress: commerce accepted, food pending     |
| KIT-07 remaining general kits                          | Build the 24 general families' explicit scopes from shared services and separate extensions; each needs its own evidence                                                               | Pending                                          |
| KIT-08 specialist kits                                 | Domain review plus selected authoritative-system adapters, supported configuration, privacy and live integration evidence; 16 research families are not native full-system commitments | Pending                                          |
| KIT-09 operator handoff                                | Reproducible releases, migration/recovery, acceptance manifest and honest support catalogue                                                                                            | Pending                                          |

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
