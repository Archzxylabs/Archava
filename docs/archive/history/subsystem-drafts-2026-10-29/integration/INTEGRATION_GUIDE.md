# Archava Third-Party Client & Partner Integration Guide

> **Audience:** External Developers, Agency Partners, and Frontend Engineers.  
> **Supported Frameworks:** Next.js, React, Vue, Svelte, Angular, WordPress, Shopify, Laravel, Flutter, iOS, Android.  
> **Pattern:** The "Stripe.js / Snap" Universal Embed Script & Headless REST Gateway.

---

## 1. Quickstart: 30-Second Integration via Script Tag

To embed Archava checkout, customer portals, or interactive booking into any website, add the universal client script:

```html
<!-- 1. Include Archava Universal Embed Script -->
<script src="https://api.archzxy.com/v1/archava.js"></script>

<!-- 2. Trigger Archava Features with Clean JavaScript -->
<script>
  // Initialize with your assigned tenant slug
  const archava = Archava('tenant_dr_kevin', {
    host: 'https://api.archzxy.com' // Optional override for staging/local
  });

  // Example: High-Performance Modal Checkout
  function openPaymentModal() {
    archava.openCheckout({
      orderId: 'ORD-20261028-0042',
      amountCents: 18648000, // Rp 186.480 in minor cents (IDR)
      onComplete: function(receipt) {
        console.log('Payment verified by Archava:', receipt);
        alert('Payment successful via QRIS / Virtual Account!');
      },
      onCancel: function() {
        console.log('User dismissed checkout modal');
      }
    });
  }
</script>

<button onclick="openPaymentModal()" style="background:#2563eb;color:#fff;padding:12px 24px;border-radius:8px;border:none;cursor:pointer;">
  Pay with Archava
</button>
```

---

## 2. Inline Widget Mounting (Dashboards, Booking & Portals)

To mount an interactive Archava cockpit (e.g. appointment scheduling, laundry tracker, or patient portal) directly into an existing DOM container on WordPress, Shopify, or React:

```html
<div id="archava-widget-portal" style="width: 100%; max-width: 1024px; margin: 0 auto;"></div>

<script src="https://api.archzxy.com/v1/archava.js"></script>
<script>
  const archava = Archava('tenant_dr_kevin');
  
  archava.mount('#archava-widget-portal', {
    view: 'erp',       // Options: 'erp', 'booking', 'tracking', 'catalog'
    height: '750px'
  });
</script>
```

---

## 3. Direct Headless REST API (Backend-to-Backend)

For backend microservices (Go, Python, PHP, Ruby, Java, Node.js) communicating directly with Archava:

### Authentication & Headers
All mutation requests require:
* `Authorization: Bearer <SESSION_OR_SERVICE_TOKEN>`
* `X-CSRF-Token: <CSRF_TOKEN>`
* `Content-Type: application/json`

### Creating an Order & Incurring Ledger Entries
```bash
curl -X POST https://api.archzxy.com/commands \
  -H "Authorization: Bearer sec_tok_live_8f93b1..." \
  -H "X-CSRF-Token: csrf_tok_3a1..." \
  -H "Content-Type: application/json" \
  -d '{
    "tenant": "tenant_dr_kevin",
    "action": "commerce.order.create",
    "revision": 1,
    "idempotency_key": "c3b8e5a1-0982-4f11-8933-28f9a0c8b321",
    "input": {
      "customer_name": "Rian Kusuma",
      "items": [
        { "sku": "DENTAL-SCALING-PRO", "qty": 1, "price_cents": 45000000 }
      ]
    }
  }'
```

### Checking Transaction / Receipt Status
```bash
curl -X GET "https://api.archzxy.com/commands/status?tenant=tenant_dr_kevin&idempotency_key=c3b8e5a1-0982-4f11-8933-28f9a0c8b321" \
  -H "Authorization: Bearer sec_tok_live_8f93b1..."
```

---

## 4. Hardware WebUSB Thermal Receipt Printing

For browser-based POS cashiers requiring direct printer execution without local print dialogs:

```typescript
import { formatPosReceipt, type ReceiptData } from '@archava/sdk';

// 1. Prepare typed receipt data
const receipt: ReceiptData = {
  merchantName: 'Archava Dental Clinic',
  branchName: 'Senopati Branch',
  address: 'Jl. Senopati No. 42, Jakarta Selatan',
  orderNo: 'ORD-20261028-0042',
  cashierName: 'Archxy',
  dateTime: '2026-10-28 11:45:00',
  items: [
    { name: 'Dental Scaling & Polish', qty: 1, priceCents: 45000000, totalCents: 45000000 }
  ],
  subtotalCents: 45000000,
  taxPpnCents: 4950000,
  discountCents: 0,
  grandTotalCents: 49950000,
  paymentMethod: 'QRIS',
  cashTenderedCents: 49950000,
  changeDueCents: 0,
  qrisPayload: '00020101021226670016ID.CO.QRIS.WWW...',
  footerMessage: 'Semoga Gigi Anda Sehat Selalu!'
};

// 2. Generate ESC/POS byte sequence
const rawBytes: Uint8Array = formatPosReceipt(receipt, 32);

// 3. Send raw byte stream to WebUSB / WebSerial printer device
const device = await navigator.usb.requestDevice({ filters: [{ vendorId: 0x04b8 }] }); // Epson
await device.open();
await device.selectConfiguration(1);
await device.claimInterface(0);
await device.transferOut(1, rawBytes);
```

---

## 5. Webhook Integration (Payment & Telematics)

Archava delivers real-time notifications to your webhook endpoints upon state changes:

### Payment Success Webhook Payload
```json
{
  "event": "payment.succeeded",
  "tenant": "tenant_dr_kevin",
  "timestamp": 1793189400,
  "data": {
    "order_id": "ORD-20261028-0042",
    "amount_cents": 49950000,
    "payment_method": "qris",
    "general_ledger_journal_id": "0193b2a0-4921-7000-8000-000000000005"
  }
}
```
All webhook payloads carry an `X-Archava-Signature` header computed as `HMAC-SHA256(payload, webhook_secret)`.
