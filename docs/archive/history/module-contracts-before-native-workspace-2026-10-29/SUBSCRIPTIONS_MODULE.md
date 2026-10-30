# Subscriptions, entitlements and usage tracking module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/subscriptions.rs), [persistence](../../crates/adapters/src/erp/subscriptions.rs), [HTTP](../../crates/host/src/erp/subscriptions.rs), [integration tests](../../crates/host/tests/erp_subscriptions.rs).

This contract covers recurring subscription lifecycles, explicit entitlements/quotas (`metered`, `seat_count`, `boolean_access`, `pass_bundle`), quota consumption audit with idempotency, and authoritative payment confirmation.
Release 1.13.0 accepted with fresh isolated domain, host, browser, upgrade and owner evidence.
It provides native membership and SaaS entitlement tracking without altering existing CRM or commerce authority.
Full SaaS and gym/club membership operating kits retain their other requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/subscriptions` owns strict contracts, human service commands, private read tools, HTTP API and reusable DOM dashboard with hardened state machine receipt recovery (S1). It depends only on pinned Zod and needs no external CRM or AI provider.
`@archava/subscriptions-pg` owns six forced-RLS business tables: `memberships`, `subscriptions`, `entitlements`, `usage_events`, `commands`, and `audit`. Its checked migration ledger `001_subscriptions_entitlements_usage.sql` is checksummed (`7c830dd1a52c957e3c94a4f47204af9ae7c95d623a67f9beb0184f230068caa4`) and constitutes ledger ke-17.

Two independent business consumers demonstrate generalization:

1. **Consumer 1: SaaS Platform Digital Subscriptions (`K09: saas-sales`)**:
   Tracks multi-seat team subscriptions (`billing_interval: 'monthly'`), initial metered API call allocations, monthly billing cycles, and automatic entitlement suspension upon `past_due` or `canceled` status.
2. **Consumer 2: Gym & Sports Club Memberships (`K08: gym-club`)**:
   Tracks member passes (`entitlement_type: 'pass_bundle'`), check-in attendance usage events with idempotency keys, quota exhaustion enforcement, and member renewal reactivation.

## Lifecycle and Invariants

```text
Subscription Lifecycle:
draft ──[record_payment]──> active ──[past_due / pause]──> past_due / paused ──[payment / resume]──> active
                              │
                              └──[cancel]──> canceled (revokes entitlements)
```

- **Recurring Lifecycles**: Subscriptions start in `draft` status until authoritative backend payment confirmation transitions them to `active`. Status changes to `canceled` or `paused` automatically propagate to child entitlements (`revoked` or `suspended`).
- **Entitlement Types**: Supports `metered` (API requests, storage MB), `seat_count` (licensed seats), `boolean_access` (feature flags), and `pass_bundle` (entry tickets, check-ins).
- **Quota Consumption & Idempotency**: Each usage event records `units_consumed`, an `idempotency_key`, and optional `evidence_ref`. Replaying the same idempotency key returns the original receipt without double-metering.
- **Quota Exhaustion**: Consuming past the allocated quota is rejected atomically with `exhausted`, preventing unauthorized overdrafts.
- **Optimistic Revision Locking**: All subscription status updates enforce exact `expectedRevision`; stale requests fail with `conflict`.
- **S1 Resilient Receipt State Machine**: The DOM dashboard dispatches commands with a client-generated UUID, blocks concurrent writes, persists pending state across failures, and provides idempotent recovery via "Check pending result" and "Retry same request".
- **Immutable Audit Triggers**: Triggers on `audit` and `commands` strictly prevent row deletion, modification, or truncation even from administrative connections.

## Security & Verification Matrix

- **18 Synthetic Verification Checks (`pnpm subscriptions:verify`)**: Multi-tenant isolation, superuser rejection, inactive account blocking, human-only mutation validation, payment transitions, idempotent usage consumption, quota exhaustion blocking, optimistic locking, and audit immutability.
- **5 Headless Browser E2E Checks (`pnpm subscriptions:verify-host`)**: Full Chromium CDP test suite across five transport/network failure modes (network drop, 503 unavailable, invalid receipt schema, duplicate write blocking, and grant revocation), proving zero duplicate subscription/usage records and consistent receipt reconciliation.
