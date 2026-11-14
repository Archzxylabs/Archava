# Verified invoice payments

The native path is **invoice → frozen gateway order → hosted checkout → authenticated status → Billing allocation**. The [readiness tracker](../product/LAUNCH_READINESS.md) owns acceptance. Local fixture results do not prove live merchant integration.

The [domain](../../crates/domain/src/erp/payments.rs) handles whole IDR and monotonic cumulative money. The [provider transport](../../crates/providers/src/midtrans.rs) uses fixed Midtrans Sandbox/Production HTTPS endpoints, Basic authentication, no redirects, bounded responses and a four-second timeout. The [adapter](../../crates/adapters/src/erp/payments.rs) owns durable correlations and allocation. The [host](../../crates/host/src/payments.rs) owns sessions, CSRF, the provider registry and authenticated Get Status. The [invoice panel](../../crates/slice/src/operations/payments.rs) never asserts payment itself. A payment the customer made outside the gateway, by bank transfer or in cash, is a separate, human-attested fact: a [manual receipt](../modules/MANUAL_CUSTOMER_RECEIPTS.md), recorded by a person with `billing.settle`, labelled manual and never verified by this path.

## Configuration and authority

Use [the configuration example](../../config/payment-providers.example.json). Set `ARCHAVA_PAYMENT_CONFIG` to an administrator-managed JSON file, normally `/etc/archava/payment-providers.json`. JSON stores the environment variable **name**, never the server key. Supply that variable through the service secret environment. Only `sandbox` or `production` is accepted; the production executable accepts no custom provider URL. Configuration is optional; an absent registry shows an unconfigured UI. An explicitly configured invalid registry refuses startup before acquiring a writer epoch. Configuration does not prove connectivity and startup makes no provider requests.

After applying the verified native migrations, provision a dedicated service with the owner connection:

```sh
archava-ops bootstrap-payment-service --tenant your-workspace --principal payment-gateway
```

The command requires `ARCHAVA_MIGRATION_DATABASE_URL` and a verified release profile, just like migration. It creates a service-kind Billing membership with `billing.read`, `billing.write` and scope `all`. It refuses an existing membership, including a human, and creates no login account. The configured service must remain active. Human link creation requires `billing.write` and current invoice visibility. Reads, recovery and explicit checks require current `billing.read` and visibility. Receipt lookup is also bound to its original human actor. Issue/correction/void controls follow `billing.settle`.

Configure the merchant notification URL as `/api/webhooks/payment/main-merchant` on the application's public HTTPS origin. The integration path chooses the tenant, service and credential. The legacy path without an integration refuses with 503; it no longer acknowledges financial settlement.

## Durable behavior and recovery

Creation freezes the issued IDR invoice's outstanding amount and revision, records the original request and a server-generated `archava-UUID` order, then commits the invocation claim **before** calling Snap. One active unpaid link is allowed per invoice. A concurrent replay uses the same claim; it never invokes creation twice. Invoice financial corrections and voiding are refused while an unpaid link can still be paid. The database repeats that guard.

A successful Snap response supplies a validated private checkout URL; this is not settlement. A lost/malformed response or a process exit leaves the original reference unconfirmed. UI recovery reads `/erp/billing/payments/status` with the original key. A retry of that key returns its existing correlation. A fresh link is blocked while the unpaid claim remains active. Get Status returning 404 is not proof that Snap creation never happened. If a URL was lost before storage, reconcile the order in the merchant dashboard; the application does not invent a URL or blindly replace the order. Automatic cancellation/token recovery is not implemented.

Midtrans's SHA-512 notification signature does not cover `transaction_status` or refund fields. The host first verifies the signature in constant time, matches the frozen order/amount, and queries Get Status. That response must match order, amount, transaction reference and supported currency/fraud rules before any money changes. Current service membership is rechecked after HTTP. Two-second per-link reservations bound repeated checks; transport failure returns an unconfirmed 503 with no settlement effect. Repeated or out-of-order observations cannot reduce already observed gross/refund totals or duplicate allocation. Only normalized observations are retained; raw response bodies and server keys are not stored in the Billing ledger.

External gross received/refunded and invoice credits/refunds are separate cumulative totals. A refund observed first allocates only the remaining external net. A changed invoice balance can leave real money unapplied; that amount remains visible for merchant-dashboard reconciliation/refund. It is not silently discarded or applied past the invoice balance. Unsupported or inconsistent states require review. This MVP does not execute merchant refunds or automatically reconcile unbound orders.

## Manual receipts beside the gateway

Both paths write payment events and both run the same settlement arithmetic, so they
are kept from settling the same money twice. While an invoice has a link that is not
closed and has received nothing, `record_manual_receipt` is refused
(`manual_receipt_open_payment_link`): check the link's status until the gateway reports
it failed or expired, or wait for it to be paid. A link is created for the invoice's
outstanding amount, so a manual part payment followed by a link for the remainder
works. A gateway event is never reversed by `reverse_manual_receipt`; gateway money goes
back through the gateway and its verified refund. The provider id namespace `manual:` is
reserved for manual receipts and refused on provider events. Gateway behaviour is
unchanged.

## Restore

Restore invalidates the copied writer identity and sets the global Billing payment-creation hold before reopening the isolated target. Existing correlations remain readable and can be verified, but new Snap calls are blocked. Reconcile all transactions since the backup against the merchant dashboard, including orders absent from the snapshot, before explicitly running:

```sh
archava-ops resume-payment-creation --confirm-reconciled
```

Only the Billing owner/administrator can release this hold. The serving runtime has SELECT access to the latch, without mutation permission. This hold protects Billing payment creation; email and realtime integration readiness are separate work.

## Evidence and protocol references

Executable coverage lives in [gateway router tests](../../crates/host/tests/gateway_payments.rs) (including a manual part payment beside a link), [transport tests](../../crates/providers/tests/midtrans_transport.rs), [service provisioning](../../crates/ops/tests/payment_service.rs), [restore rehearsal](../../crates/ops/tests/upgrade_restore.rs) and [browser acceptance](../../tools/e2e/payments.mjs). Execution results belong to the [evidence packet](../archive/audits/evidence/launch-2026-10-29/checks.json).

Protocol decisions were checked against the official [notification guide](https://docs.midtrans.com/reference/handle-notifications), [Get Status](https://docs.midtrans.com/reference/get-transaction-status), [Snap expiry](https://docs.midtrans.com/reference/expire-a-snap-session) and [Snap cancellation](https://docs.midtrans.com/reference/cancel-a-snap-session) on 2026-10-30. Snap cancellation requires its token; page expiry alone does not necessarily stop a payment method whose expiry is later. Those operations are not assumed to be available by order ID.
