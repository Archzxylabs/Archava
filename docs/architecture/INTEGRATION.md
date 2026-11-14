# Host and browser integration

> **Status: source reference, reviewed 29 October 2026.** This guide consolidates host authentication/request contracts and browser embedding. No external-provider, cross-origin browser, or production deployment acceptance was rerun for this documentation change.

Use [developer setup](../DEVELOPER_GUIDE.md) and [deployment](../../deploy/README.md) to run the host. The implementation boundaries below follow local source; legacy drafts remain in [history](../archive/history/subsystem-drafts-2026-10-29/README.md).

## Host integration

<a id="host-integration-contract"></a>

### Entry points

Use the origin of your own configured deployment. The examples below use loopback; no public Archava production domain is implied.

| Surface | Registered path | Source |
| --- | --- | --- |
| Liveness/readiness | `GET /healthz`, `GET /readyz` | [router](../../crates/host/src/router.rs) |
| Sign-in/sign-out | `/login`, `/logout` | [router](../../crates/host/src/router.rs) and [identity](../../crates/host/src/identity.rs) |
| Contacts | `GET /contacts`, `GET /crm/records` | [CRM handler](../../crates/host/src/erp/crm.rs) |
| CRM commands/recovery | `POST /crm/commands`, `GET /crm/commands/status` | [typed CRM contract](../../crates/host/src/erp/crm.rs) |
| Owner configuration | `/own/config`, `/own/plan`, `/own/apply`, `/own/history`, `/own/status` | [owner handler](../../crates/host/src/own.rs) |
| Example domain command | `/erp/purchasing/commands` and `/erp/purchasing/commands/status` | [purchasing handler](../../crates/host/src/erp/purchasing.rs) |
| Browser bundle/embed | `/v1/archava.js`, `/embed/checkout` | [embed reference](INTEGRATION.md#browser-embedding) |

The complete set is in the router. A registered path alone is not an acceptance test of its business workflow.

### Authentication and write identity

The host accepts a verified session through its supported cookie or Bearer-token path. A caller-supplied tenant selects the requested scope; it does not grant access. Membership and grants are derived by the host.

For authenticated CRM/ERP mutations, the implemented CSRF header is **`x-archava-csrf`**. The old draft's generic `X-CSRF-Token` is not this header. Use the existing sign-in/session flow to obtain the relevant session and CSRF value; do not invent a service-token or API-key provisioning flow.

The CRM submission envelope uses `tenant`, `requestId`, and a typed `command`. Field names and variants are defined by the handler. The previous `action` / `idempotency_key` sample must not be copied as a valid CRM command. See [the actual create-contact client](../../crates/slice/src/contacts.rs), [follow-up client](../../crates/slice/src/draft.rs), and [CRM integration tests](../../crates/host/tests/crm.rs) for complete matching examples.

### Read-only local smoke check

With your configured server already running:

```bash
curl -fsS http://127.0.0.1:8347/healthz
curl -fsS http://127.0.0.1:8347/readyz
```

These are process/database checks, not proof that a payment, external provider, device, or whole ERP workflow works. For setup use [deployment](../../deploy/README.md); for local checks use [the developer guide](../DEVELOPER_GUIDE.md).

### Results and external integrations

A UI callback does not establish provider settlement. Use the domain's server-side receipt/status and provider-verification contract. This guide does not assert a universal outgoing webhook schema, signature header, or complete WebUSB SDK; each requires a concrete implementation and separate evidence.

## Browser embedding

<a id="browser-embed-reference"></a>

### Served client and initialization

The [host router](../../crates/host/src/router.rs) serves `/v1/archava.js` from [static/archava.js](../../static/archava.js). The source exposes `window.Archava(tenantId, options)`.

On the same origin as a configured host, initialization can look like this:

```html
<script src="/v1/archava.js"></script>
<script>
  const client = Archava('YOUR_TENANT');
</script>
```

The tenant argument is client-side selection, not authorization. A session, grants, final frame policy, cookies, and deployment origin still determine which embedded operations can actually run.

### Interfaces present in source

| Interface | What the current client does |
| --- | --- |
| `openCheckout(params)` | Creates an iframe targeting a signed `/embed/checkout` URL; the `sig` must come from `POST /embed/checkout/sign` (session + CSRF) |
| `mount(selector, options)` | Creates an iframe targeting `/app/<view>`; it does not create an unimplemented view |
| `mountAssistant(options)` | Renders a clearly labelled preview UI; live queries/actions require caller-supplied `onQuery` / `onAction` verified transport callbacks |
| `data-tenant` / `data-mode` attributes | Support declarative initialization in the client script |

Mode names and visible controls do not prove a complete live voice/avatar/provider pipeline. The Operations app view provides the native Tier 1 workspace; the assistant widget remains a preview. The assistant widget never claims a query or action succeeded without a caller-supplied verified transport result. Do not document arbitrary booking, catalog, or tracking paths as supported simply because a `view` string can be passed to `mount`.

### Checkout callbacks

The client checks message origin, iframe source, and order identity. The built-in checkout document is display-only until a real provider transport is mounted; it does not emit a settlement receipt. The SDK still rejects simulated events unless the caller explicitly opts into its separate sandbox callback path, and a simulated result must never be described as verified production payment.

Even a success callback is a browser message, not independent proof of a signed provider settlement. The signed-checkout mint loads an existing draft/reserved order under current Commerce membership and binds amount/currency to its server quote. A caller-supplied amount or currency must match that quote. Follow the actual server-side receipt/provider contract for business effects.

The checkout document itself carries an HMAC over the values it renders. A URL without a matching `sig` answers `403 checkout_signature_required`, so a merchant must obtain the URL from `POST /embed/checkout/sign` rather than assembling query parameters by hand. Framing is limited to the origins listed in `ARCHAVA_EMBED_FRAME_ANCESTORS`; with that variable unset the document sends `frame-ancestors 'none'` and cannot be embedded at all.

### Implementation and proof

This audit statically inspected the client and router. It did not execute a merchant checkout, embed on another origin, operate a USB printer, or connect an external voice/avatar provider. [Host contracts](INTEGRATION.md#host-integration), [deployment](../../deploy/README.md), and the route-specific tests are the next references for an integration implementation.

## November hardening contracts

Email replay checks the current human Billing grants and invoice scope before reading terminal or uncertain intents. Another actor cannot adopt an existing request key. A new key for identical email content returns `409 email_already_recorded` with the rejected `requestId`, `invoiceId`, and `duplicateOf` canonical key; it neither invokes the provider nor invents a receipt for the new key. The UI clears only this explicit matching refusal or a matching terminal invoice receipt.

Login buckets use forwarded client addresses only when the transport peer is explicitly configured in `ARCHAVA_TRUSTED_PROXIES`. The host walks the forwarded chain from right to left and stops at the first untrusted hop. Direct public peers cannot select a bucket through a forwarded header. See [deployment](../../deploy/README.md#5-tls-and-proxy) for the required proxy topology.
