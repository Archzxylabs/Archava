# T02 follow-up — `listEntitlements` answered every contact read with `not_found`

Task: `t_84230d16` (follow-up of T02 / A10, observation P3 of `runs/20261021-225335-audit.json`).
Date: 2026-10-21. Scope: `packages/subscriptions`, `packages/subscriptions-pg`.

## Defect

`PostgresSubscriptionRepository.listEntitlements` chose its branch with
`isUuid(targetId)`:

- UUID target → subscription-keyed read (parent probe, then child read);
- non-UUID target → contact-reference read (`e.contact_id=$2` + `EXISTS` parent).

But `entitlementSchema.contactId` is `z.string().uuid()`, so **every contact id
that can match a row is a UUID**. The value-only gate therefore sent every
contact read into the subscription branch, which found no subscription with that
id and threw `not_found`. The contact-reference branch was unreachable through
any public path, and `subscriptions.entitlements` — a tool whose whole purpose is
"list all entitlements granted to a specific contact" — could never return a
row. This was a live functional defect, not only dead code.

## Decision

Explicit target kind at the method level, no discriminator field and no
fallback chain:

| Entry point                                             | Kind         | Change                                                                                                 |
| ------------------------------------------------------- | ------------ | ------------------------------------------------------------------------------------------------------ |
| `listEntitlements(tenant, principal, subscriptionId)`   | subscription | `isUuid` inference and the contact branch removed; parameter renamed from `contactId`, which was a lie |
| `listContactEntitlements(tenant, principal, contactId)` | contact      | the contact read moved here with its `EXISTS (p.owner_id=$3)` predicate unchanged                      |
| `service.entitlements(...)`                             | subscription | unchanged 3-arg alias, keeps the published proof surface intact                                        |
| `subscriptions.entitlements` tool                       | contact      | now calls `listContactEntitlements`; the reported bug                                                  |
| `GET /records/:id/entitlements`                         | subscription | unchanged                                                                                              |

Rejected: a required `targetKind` field (a default value can silently misroute,
and the wire already carries the intent), a sequential fallback (two round-trips
and a blurred `not_found`, which contradicts the T02 indistinguishability
contract), and deleting the contact read (the tool promises it).

## Behaviour

Nothing changes for subscription reads: same parent probe, same owner predicate
placement, same child statement, same ordering. The contact read keeps the T02
ownership rule verbatim — `EXISTS` on the tenant-scoped parent, never an inner
join, so a row with `subscription_id IS NULL` survives under `all` scope and is
invisible under `assigned` because it inherits no owner.

## Verification

| Gate          | Command                                                                           | Result                                |
| ------------- | --------------------------------------------------------------------------------- | ------------------------------------- |
| unit          | `pnpm exec vitest run packages/subscriptions-pg/test packages/subscriptions/test` | 3 files / 22 tests passed, exit 0     |
| collateral    | `pnpm exec vitest run packages apps`                                              | 125 files / 1658 tests passed, exit 0 |
| types         | `pnpm typecheck`                                                                  | exit 0, zero `error TS`               |
| library       | `pnpm library:pack`                                                               | 62 packages packed, exit 0            |
| release proof | `TMPDIR=/tmp npm_config_offline=true node tools/library/verify-subscriptions.mjs` | 19/19 PASS, exit 0                    |

### Mutation controls

Both regressions were reintroduced on purpose and the suite failed, so the new
tests are load-bearing rather than decorative:

| Mutation                                                    | Result                                                                                               |
| ----------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| `AND p.owner_id=$3` → `AND $3 = $3` inside the contact read | exit 1, 4 failed, incl. `resolves entitlements by contact reference for an owned contact`            |
| tool routed back to `listEntitlements`                      | exit 1, 1 failed, `routes subscriptions.entitlements to the contact read, not the subscription read` |

Tree restored and re-verified green after each control.

## Residual (not in this task)

`rust/crates/adapters/src/erp/subscriptions.rs::read_entitlements` carries the
same `valid_uuid` inference and has no owner scoping at all. The host route is
subscription-keyed by construction and the TypeScript wire did not change, so it
is tracked as a separate Rust card rather than folded into this diff.
