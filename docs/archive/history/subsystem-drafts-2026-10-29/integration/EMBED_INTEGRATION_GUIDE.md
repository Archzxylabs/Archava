# Archava 3rd-Party Integration Guide (Zero-NPM & Embedded Architecture)

This document provides external engineering teams (Next.js, React, Vue, WordPress, Shopify, Laravel) with the exact instructions to embed Archava ERP, POS, and Checkout without installing any heavy npm packages or maintaining a TypeScript runtime.

---

## 1. Single Script Tag Integration (Stripe.js / Midtrans Snap Pattern)

Add the Archava embed script before the closing `</body>` tag on any web page:

```html
<!-- Load Archava Universal Client -->
<script src="https://archzxy.com/v1/archava.js"></script>

<script>
  // Initialize with your tenant slug
  const archava = Archava('tenant_dr_kevin', {
    host: 'https://api.archzxy.com' // or local http://localhost:3000
  });

  // Trigger high-performance modal checkout
  function triggerCheckout() {
    archava.openCheckout({
      orderId: 'ORD-20261028-0042',
      amountCents: 18648000, // Rp 186.480 in minor cents
      onComplete: function(receipt) {
        console.log('Payment completed:', receipt);
        alert('Payment successful via QRIS / Bank Transfer!');
      },
      onCancel: function() {
        console.log('User dismissed checkout modal');
      }
    });
  }
</script>

<button onclick="triggerCheckout()" style="padding: 12px 24px; background: #2563eb; color: #fff; border-radius: 8px; border: none; cursor: pointer;">
  Pay with Archava
</button>
```

---

## 2. Inline Embedded Widget (Dashboards & Self-Service Portals)

To mount an interactive Archava cockpit (e.g. appointment booking, order status, clinic dashboard) directly into an existing HTML container:

```html
<div id="archava-widget-container" style="max-width: 900px; margin: 0 auto;"></div>

<script src="https://archzxy.com/v1/archava.js"></script>
<script>
  const archava = Archava('tenant_dr_kevin');
  archava.mount('#archava-widget-container', {
    view: 'erp',
    height: '750px'
  });
</script>
```

---

## 3. Server-to-Server Direct Headless API (cURL, Python, Go, PHP)

For backend microservices that communicate directly with the Rust host:

### Create Order & Ledger Intent
```bash
curl -X POST https://api.archzxy.com/commands \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer <SESSION_OR_SERVICE_TOKEN>" \
  -H "X-CSRF-Token: <CSRF_TOKEN>" \
  -d '{
    "tenant": "tenant_dr_kevin",
    "action": "commerce.order.create",
    "revision": 1,
    "idempotency_key": "9b1deb4d-3b7d-4bad-9bdd-2b0d7b3dcb6d",
    "input": {
      "customer_name": "Budi Santoso",
      "items": [
        { "sku": "DENTAL-CLEAN-01", "qty": 1, "price_cents": 35000000 }
      ]
    }
  }'
```

---

## 4. Hardware WebUSB Thermal Printing

For browser-based POS cashiers requiring direct ESC/POS thermal printing:
* The client browser uses native `navigator.usb` or WebSerial to transmit raw binary commands.
* The raw bytes conform to Epson ESC/POS standard generated directly by the Archava Engine.

---

## 5. Ava Decoupled Modality Embed (`data-mode` Presets)

External websites can embed Ava in any sensory combination by setting the `data-mode` attribute on the `<script>` tag or passing options to `mountAssistant()`:

### Declarative Script Tag (Zero Code)
```html
<!-- Example 1: Full Flagship Experience (Avatar + Voice + Live Chat & Action Cards) -->
<script src="https://archzxy.com/v1/archava.js"
        data-tenant="lumina-clinic"
        data-mode="full"
        data-position="bottom-right">
</script>

<!-- Example 2: Minimalist Chat Only (Text + QRIS/SOW Action Cards) -->
<script src="https://archzxy.com/v1/archava.js"
        data-tenant="toko-baju"
        data-mode="chat_only"
        data-position="bottom-right">
</script>

<!-- Example 3: Kiosk Receptionist Concierge (Avatar + Voice, No Chatbox) -->
<script src="https://archzxy.com/v1/archava.js"
        data-tenant="grand-villa-bali"
        data-mode="avatar_voice"
        data-position="kiosk">
</script>
```

### Supported Presets Matrix:
| `data-mode` | Chatbox | Voice Mic / Audio | Avatar Canvas | Best Fit Use-Case |
| :--- | :---: | :---: | :---: | :--- |
| `chat_only` | ✅ | ❌ | ❌ | eCommerce store, support tickets, mobile night browsing |
| `voice_only` | ❌ | ✅ | ❌ | Warehouse staff, delivery drivers, IVR hotline |
| `avatar_voice` | ❌ | ✅ | ✅ | Hotel lobby tablet, luxury clinic receptionist kiosk |
| `full` | ✅ | ✅ | ✅ | High-ticket consultation, B2B sales closing, flagship demo |
| `avatar_chat` | ✅ | ❌ | ✅ | Silent ambient presence (office / muted environments) |

### Programmatic Mounting API
```javascript
const client = Archava('lumina-clinic');

// Mount Ava with custom modality options
client.mountAssistant({
  mode: 'avatar_voice', // or 'chat_only', 'voice_only', 'full', 'avatar_chat'
  position: 'bottom-right', // or 'bottom-left', 'kiosk'
  themeColor: '#6366f1'
});
```
