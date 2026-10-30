# Invoicing and payment settlement module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/billing.rs), [persistence](../../crates/adapters/src/erp/billing.rs), [HTTP](../../crates/host/src/erp/billing.rs), [integration tests](../../crates/host/tests/erp_billing.rs).

This contract covers immutable invoice intent and priced snapshots, append-only correction chains, payment attempts that record intent without asserting a result, and settlement that a payment provider's own signed notification is the only thing able to record.
Candidate release 1.16.0 adds bounded billing authority without altering existing CRM, commerce, fulfillment or attendance authority.
Full invoicing and payment-operation kits retain their other requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/billing` owns strict contracts, human service commands, private read tools, HTTP API and a reusable DOM dashboard with a hardened state machine receipt recovery (S1). It depends only on pinned Zod and `node:crypto` and needs no external CRM or AI provider.
`@archava/billing-pg` owns eight forced-RLS business tables: `memberships`, `invoices`, `invoice_lines`, `invoice_corrections`, `payment_attempts`, `payment_events`, `commands`, and `audit`. Its checked migration ledger `001_billing_invoices_settlement.sql` is checksummed (`b14f434495c526384a726ddf83eb685b610d229d379d0974b2c38df59f96dfbc`) and constitutes ledger ke-20.

The package deliberately composes **nothing** from commerce or fulfillment: commerce remains the only authority for stock and pricing, fulfillment the only authority for custody and dispatch, and billing owns only the money record — what was priced, what was owed, what was settled and on whose say-so.

Two independent business consumers demonstrate generalization:

1. **Consumer 1: Outpatient clinics (`K25: outpatient-clinics`)**:
   Prices a consultation and laboratory panel against a closed encounter, issues the invoice, applies a goodwill credit as a correction, and settles only when the acquirer's signed notification arrives.
2. **Consumer 2: Asset and equipment rental (`K10: asset-rental`)**:
   Prices a hire period against a handover, issues the invoice, and settles through the processor's signed notification, with a refund that reopens rather than erases the settlement.

## Lifecycle and Invariants

```text
Invoice lifecycle:
draft ──[issue_invoice]──> issued ──[provider: succeeded, fully settled]──> settled
  │                            │                                              │
  │                            └──[provider: refunded]────────────────────────┘
  └──[void_invoice]───────────────> void                (refund reopens settled → issued)

Balance:
outstanding = gross + Σcorrections − Σsettlements
settlement only reaches zero outstanding → status `settled`
```

- **Human-only mutations**: `create_invoice`, `issue_invoice`, `append_correction`, `void_invoice` and `record_payment_attempt` are refused for service/automation principals at the service boundary.
- **`record_provider_event` is authorised by signature, not by a person**: it is the single mutation a service principal may carry, and carrying it still requires the `billing.write` grant. A correctly signed event from an automated principal is accepted; an unsigned or mis-signed one is `invalid_signature` whether or not the caller holds any grant.
- **The settlement path has no client-facing handle**: there is no mutation, tool or dashboard control that asserts "this invoice is paid". Payment success is recorded only by a provider notification whose HMAC verifies against the exact claim it makes.
- **An attempt carries no outcome**: `record_payment_attempt` has no currency field (it inherits the invoice's) and no field capable of asserting a result. Recording one changes no invoice state at all — not the balance, not the status, not even the revision.
- **A failed notification moves nothing**: `outcome: 'failed'` is pure history. The invoice row is never touched, so a rejection from the processor cannot create a spurious optimistic-concurrency conflict for a human working the invoice.
- **`issued` is the immutability boundary**: the priced snapshot (`gross_minor`, customer and consumer references) is frozen at creation and refused at the database boundary; corrections adjust what is owed as a separate append-only chain and never rewrite the priced total.
- **Settlement is balanced, not merely accepted**: a `succeeded` event is refused with `invalid_transition` unless the invoice is `issued`, and with `unbalanced_settlement` if it would drive the outstanding balance below zero. A `refunded` event may never exceed what was already settled. Status reaches `settled` only when the outstanding balance is exactly zero.
- **Refund reopens rather than erases**: `settled → issued` is a first-class transition, so a refund leaves both the original settlement and the refund visible as evidence instead of quietly rewriting history. `void` is the only terminal state.
- **Signature verification is one code path**: the digest covers the tenant, the invoice, the provider's own event id, the outcome, the amount in minor units and the currency, each length-framed under the single domain `archava/billing/provider-event/v1`. A digest minted under another domain, under another tenant's key, or over a tampered amount verifies against nothing. A provider with no configured secret fails closed.
- **Optimistic revision locking**: `issue_invoice` and `void_invoice` require the exact `expectedRevision`; stale requests fail with `conflict`.
- **Permission gating**: `create_invoice` and `record_payment_attempt` require `billing.write`; `issue_invoice`, `append_correction` and `void_invoice` require the dedicated `billing.settle`; read paths require `billing.read`, audit paths `audit.read`. `getAccess` parses through `billingAccessSchema`, so permissions outside the billing vocabulary are rejected instead of trusted.
- **S1 resilient receipt state machine**: the DOM dashboard assigns a client-generated command UUID, marks the outcome uncertain _before_ dispatch, blocks new writes while an earlier outcome is unknown, and offers **Re-query status** (authoritative receipt reconciliation, including detection of an invalid or mismatched receipt) and **Retry same request** (same UUID, same body).
- **Tenant isolation**: every table carries forced row-level security on `archava.tenant_id`; the runtime role is neither superuser nor bypass-RLS nor schema owner, and cross-tenant invoice lookups return not found.

## Security & Verification Matrix

- **30 Synthetic Verification Checks (`pnpm billing:verify`)**: two independently cold-installed clinic and rental consumers sharing unchanged billing code with a browser-safe dashboard, priced-snapshot creation and freezing, issue-then-reissue refusal, correction chains that adjust what is owed without touching the priced total, payment attempts that settle nothing, the structural absence of any outcome field on an attempt, signature-only settlement, tampered-amount / foreign-key / foreign-domain / unconfigured-provider refusals, the write grant being required even for a correctly signed event, refund reopening a settled invoice, failed notifications changing no state, over-settlement and over-refund refusal, service-principal refusal for human mutations, cross-tenant and inactive-principal refusal, idempotent replay and changed-payload conflict, command status recovery, read-only tool boundaries, append-only and immutability enforcement, status-transition enforcement, runtime role privilege refusal, grant revocation, and migration checksum parity with the release catalog.
- **10 Headless Browser E2E Checks (`pnpm billing:verify-host`)**: module-disabled 404, authentication and domain-grant boundaries (no auto-enrollment), compiled-host CSRF/origin/viewer/tool boundaries, the settlement boundary at the API surface, restart with idempotent receipt recovery, a Chromium CDP create→issue→settle workflow that asserts no control marks an invoice paid by client assertion, unsigned-notification refusal, signature-only settlement, and all five S1 receipt-recovery regressions (network interruption, HTTP 500 on command, non-JSON receipt, mismatched receipt command ID, and recovery-query outage plus authoritative `unknown` resolution).
- **22 persistence tests and 9 contract tests** cover the repository against a stub pool plus the package's domain boundaries.
