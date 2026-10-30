# ARCHAVA Platform — Master Product Requirements Document

**Company:** ARCHZXY — AI-native Creative & Product Studio  
**Product:** Archava  
**Document version:** 1.0.0  
**Commercial baseline:** Pricing v1.0  
**Effective date:** 22 September 2026  
**Primary implementation workspace:** `/home/haikaru/Archverse/Lab/Archava_Platform/`  
**Status:** Build-ready baseline; pricing and provider economics require scheduled review.

---

## 1. Executive summary

Archava is ARCHZXY's reusable digital-employee platform for the web. It is not primarily a chatbot, avatar vendor, or website builder. Archava combines business knowledge, page awareness, customer context, realtime conversation, permitted actions, transaction orchestration, and optional human-looking realtime embodiment into one deployable experience.

The same Archava core must be sellable in three public **Presence** levels:

1. **Archava Chat** — text-based digital employee.
2. **Archava Voice** — Chat plus realtime spoken conversation.
3. **Archava Human** — Voice plus a realtime human-looking avatar.

Presence is intentionally separate from **Capability**:

1. **Assist** — understand, answer, recommend, compare, navigate, capture leads, hand off.
2. **Act** — Assist plus approved non-payment state-changing actions such as booking, form submission, CRM writes, transactional email, and standard API actions.
3. **Transact** — Act plus cart, checkout, payment initiation/status, confirmation, receipt, and supported post-purchase flows.
4. **Enterprise** — ERP, legacy, regulated, multi-system, dedicated-infrastructure, or otherwise complex custom orchestration.

Presence and Capability are also separate from the **Digital Environment** in which Archava lives:

- Existing client website
- Landing page
- Business website
- Commerce / booking environment
- Custom web application
- Enterprise system

This separation gives ARCHZXY a deterministic pricing and delivery system. The commercial formula is:

> **Implementation + Presence + Capability + Digital Environment + Integration Complexity + Consumption**

The platform must include an internal **Archava Studio** that accepts client requirements, selects the closest reusable template, enables the required modules, calculates an accurate quote from the machine-readable pricebook, and generates a proposal/configuration. The agent rules for this behavior are defined in `AGENT.md`.

---

## 2. Product thesis

### 2.1 What Archava is

Archava is an embodied business interface. A customer can ask natural-language questions, receive contextual answers, see personalized recommendations, navigate the website, trigger allowed actions, complete a transaction, receive confirmation/receipt, and return later for support.

The canonical end-to-end experience is:

`Discover → Ask → Understand → Compare → Decide → Act → Checkout/Book → Confirm → Receipt → Support → Return`

### 2.2 What ARCHZXY sells

ARCHZXY sells the **implemented business outcome**, not the underlying infrastructure vendor.

A client should buy:

- an intelligent customer-facing employee;
- integration into the client's website/business systems;
- optionally a website or web application built by ARCHZXY;
- ongoing platform operation and measured usage.

The client does **not** buy “Spatius integration”, “LiveKit integration”, “Gemini integration”, or a bundle of APIs. Provider names are implementation details unless the client asks for technical/procurement disclosure.

### 2.3 What must remain ARCHZXY IP

The following must remain first-party Archava logic and must not become inseparable from any single model/avatar vendor:

- context graph;
- page awareness;
- business/product knowledge model;
- customer/session memory abstraction;
- capability graph;
- permission/risk engine;
- outcome/journey engine;
- action registry;
- transaction orchestration;
- generative UI contracts;
- provider routing;
- business adapters;
- analytics/outcome taxonomy;
- template selection;
- commercial/pricing engine.

Provider implementations may be replaced without redefining the product.

---

## 3. Brand architecture

### ARCHZXY

**Positioning:** AI-native Creative & Product Studio.

ARCHZXY builds products, systems, creative experiences, websites, applications, and integrations for clients, while also developing proprietary products.

### Archava

**Positioning:** Digital employee / digital human platform for businesses.

Recommended product descriptor:

> **A realtime digital employee that understands your business, knows what your customer is looking at, and can help them all the way from question to action.**

For the public website, avoid leading with infrastructure terminology. Show the outcome first.

---

## 4. Goals

### 4.1 Product goals

Archava v1 must:

- work as Chat, Voice, or Human without changing the underlying business intelligence;
- understand the current webpage and relevant UI/business state;
- answer from structured business truth and unstructured knowledge;
- recommend and compare products/services contextually;
- execute only explicitly authorized actions;
- support lead capture and human handoff;
- support booking/action workflows;
- support end-to-end commerce/transaction flows when Transact is enabled;
- send transactional confirmations/receipts;
- expose useful analytics to the client;
- run on an existing website or an ARCHZXY-built environment;
- support a reusable template catalog for multiple industries;
- produce deterministic regional pricing from client requirements;
- be provider-agnostic for LLM/realtime voice/avatar infrastructure;
- fail gracefully from Human → Voice → Chat when required.

### 4.2 Business goals

The platform must let ARCHZXY:

- stop pricing projects ad hoc;
- quote a client quickly and consistently;
- reuse 70–90% of standard implementation work;
- preserve premium pricing despite lower infrastructure cost;
- sell in Indonesia with a localized IDR pricebook;
- sell globally with a separate USD pricebook;
- gradually shift from custom service revenue toward recurring Archava platform revenue;
- measure gross margin by client, provider, presence mode, and capability usage.

---

## 5. Non-goals for v1

Archava v1 will **not** attempt to:

- build a new foundational LLM;
- build a production-grade realtime avatar renderer from scratch;
- replace established ERP/accounting platforms;
- replace Shopify/WooCommerce/Medusa where a mature commerce engine already exists;
- replace client CRM/PMS systems unless explicitly contracted;
- provide unlimited usage plans;
- provide autonomous high-risk payments/refunds without explicit authorization policy;
- promise exact enterprise pricing without discovery;
- hard-code one industry into the core product;
- create a unique source-code fork for every standard client.

---

## 6. Users and personas

### 6.1 End customer

A visitor/buyer/patient/guest/prospect interacting with Archava on a client's digital property.

Needs:

- fast answers;
- natural interaction;
- accurate product/service information;
- help deciding;
- seamless action/transaction;
- continuity after purchase;
- easy human escalation.

### 6.2 Client owner / business administrator

A business owner or team member using Archava as a customer-facing employee.

Needs:

- update content/product knowledge;
- view leads/conversations/outcomes;
- see bookings/orders/actions;
- control what Archava can do;
- review knowledge gaps;
- manage team handoff;
- understand ROI.

### 6.3 ARCHZXY operator

A product/solutions operator configuring Archava for a client.

Needs:

- select template;
- choose Presence/Capability/Environment;
- configure integrations;
- import knowledge;
- set permissions;
- set regional pricebook;
- calculate quote;
- generate proposal/config;
- monitor deployment and provider cost.

### 6.4 ARCHZXY engineer

Needs:

- shared modules/packages;
- provider abstractions;
- observability;
- testable integrations;
- tenant isolation;
- deterministic migrations;
- minimal client-specific code.

---

## 7. Commercial model

Archava pricing is intentionally modular, with four public/commercial dimensions.

### 7.1 Layer A — Presence

Presence answers: **How does the customer interact with Archava?**

#### Chat

Includes full Archava intelligence through text:

- business/product knowledge;
- page awareness;
- context;
- recommendations;
- supported actions according to capability level;
- lead capture;
- handoff;
- analytics.

#### Voice

Includes Chat plus:

- realtime audio conversation;
- interruption/barge-in;
- turn-taking;
- multilingual speech where provider supports it;
- graceful text fallback.

#### Human

Includes Voice plus:

- realtime human-looking avatar;
- one standard custom avatar setup within Presence setup where the selected provider/plan supports it;
- graceful Human → Voice → Chat fallback;
- client-side/network capability checks.

A realistic avatar must still be disclosed as AI in the UI where applicable.

### 7.2 Layer B — Capability

Capability answers: **What is Archava allowed to do?**

#### Assist — included

- Q&A;
- structured/unstructured product knowledge;
- page awareness;
- product/service recommendations;
- comparisons;
- navigation and UI guidance;
- lead capture;
- human handoff;
- baseline analytics.

#### Act — paid module

Includes Assist plus:

- booking/appointment actions;
- form submission;
- CRM writes;
- transactional email;
- standard external API state-changing actions;
- configurable approval/confirmation rules.

Includes one Standard integration in the list price.

#### Transact — paid module

Includes Act plus:

- catalog/cart actions;
- checkout orchestration;
- payment initiation;
- payment status;
- order/booking confirmation;
- receipt/transactional email;
- order/booking lookup;
- supported post-purchase transaction actions.

Includes two Standard integrations in the list price.

#### Enterprise

Used when requirements involve:

- ERP;
- legacy/undocumented systems;
- advanced multi-system workflows;
- dedicated deployment;
- strict compliance/SLA;
- complex identity/SSO;
- large data migration;
- custom multi-agent orchestration;
- high or unusual concurrency;
- advanced permission/audit requirements.

Enterprise is never auto-priced as a final fixed quote.

### 7.3 Layer C — Digital Environment

Answers: **Where does Archava live?**

#### Existing Site

One normal Archava embed into a suitable existing production website is included in Presence setup.

Additional remediation is charged separately if the website requires:

- extensive DOM reconstruction;
- custom routing instrumentation;
- framework migration;
- security remediation;
- duplicated/unstable UI state;
- unsupported/legacy architecture.

#### Landing

Target scope:

- one primary conversion page;
- responsive design;
- basic SEO;
- analytics;
- CTA/forms;
- legal/privacy essentials;
- lightweight CMS where necessary.

#### Business

Target scope:

- approximately 2–8 primary pages;
- CMS;
- services/products content;
- forms;
- analytics;
- SEO foundations;
- reusable design system.

#### Commerce / Booking

Target scope:

- catalog or service listing;
- detail views;
- booking/cart conversion flow;
- admin/content management;
- transaction-ready integration points.

Commerce engine/payment/PMS fees are not automatically included.

#### Custom Web App

Used when the project needs:

- user accounts;
- dashboards;
- custom data models;
- multi-role workflows;
- complex application behavior.

Published prices are “from” only.

#### Enterprise System

Custom scope only.

### 7.4 Layer D — Consumption

No public Archava plan is unlimited.

Usage must be metered separately from platform pricing.

Definitions are in `config/pricing.v1.json`.

Standard v1 meters:

- Chat → active conversations;
- Voice → realtime minutes;
- Human → realtime human/avatar minutes.

Third-party payment transaction fees and extraordinary external provider charges are separate.

---

## 8. Regional pricebooks

ARCHZXY must maintain separate commercial pricebooks rather than FX-converting one list into another.

### 8.1 Global pricebook — USD

#### Presence

| Presence |  Setup | Monthly platform |             Included usage |            Overage |
| -------- | -----: | ---------------: | -------------------------: | -----------------: |
| Chat     | $2,000 |          $249/mo | 1,000 active conversations | $0.10/conversation |
| Voice    | $3,500 |          $399/mo |       500 realtime minutes |          $0.10/min |
| Human    | $5,500 |          $599/mo | 500 realtime human minutes |          $0.15/min |

#### Capability

| Capability |    Setup |  Monthly | Included Standard integrations |
| ---------- | -------: | -------: | -----------------------------: |
| Assist     | Included | Included |                              0 |
| Act        |  +$1,500 | +$149/mo |                              1 |
| Transact   |  +$3,000 | +$299/mo |                              2 |
| Enterprise |   Custom |   Custom |                         Custom |

#### Digital Environment

| Environment        |                                     Setup |
| ------------------ | ----------------------------------------: |
| Existing site      | standard embed included in Presence setup |
| Landing            |                                    $1,500 |
| Business           |                                    $3,000 |
| Commerce / Booking |                                    $5,000 |
| Custom Web App     |                               from $8,000 |
| Enterprise System  |                                    Custom |

#### Integration complexity

| Type       |       Setup |
| ---------- | ----------: |
| Standard   |        $750 |
| Custom     | from $1,500 |
| Advanced   | from $3,000 |
| Enterprise |      Custom |

### 8.2 Indonesia pricebook — IDR

#### Presence

| Presence |        Setup | Monthly platform |             Included usage |            Overage |
| -------- | -----------: | ---------------: | -------------------------: | -----------------: |
| Chat     |  Rp6.900.000 |  Rp1.490.000/bln | 1.000 active conversations | Rp500/conversation |
| Voice    | Rp10.900.000 |  Rp2.490.000/bln |       500 realtime minutes |        Rp1.000/min |
| Human    | Rp14.900.000 |  Rp3.490.000/bln | 500 realtime human minutes |        Rp1.500/min |

#### Capability

| Capability |        Setup |          Monthly | Included Standard integrations |
| ---------- | -----------: | ---------------: | -----------------------------: |
| Assist     |     Included |         Included |                              0 |
| Act        | +Rp4.900.000 |   +Rp750.000/bln |                              1 |
| Transact   | +Rp9.900.000 | +Rp1.500.000/bln |                              2 |
| Enterprise |       Custom |           Custom |                         Custom |

#### Digital Environment

| Environment        |                                     Setup |
| ------------------ | ----------------------------------------: |
| Existing site      | standard embed included in Presence setup |
| Landing            |                               Rp4.900.000 |
| Business           |                               Rp8.900.000 |
| Commerce / Booking |                              Rp14.900.000 |
| Custom Web App     |                         from Rp24.900.000 |
| Enterprise System  |                                    Custom |

#### Integration complexity

| Type       |             Setup |
| ---------- | ----------------: |
| Standard   |       Rp2.500.000 |
| Custom     |  from Rp5.000.000 |
| Advanced   | from Rp10.000.000 |
| Enterprise |            Custom |

### 8.3 Regional selection rule

Use Indonesia pricing only when:

- contracting/billing entity is in Indonesia **and** primary commercial delivery market is Indonesia; or
- ARCHZXY explicitly authorizes the Indonesia pricebook.

Otherwise use Global pricing.

Do not convert Global USD list prices directly to IDR as a substitute for the Indonesia pricebook.

The public website may default to a suggested regional view, but users must be able to switch pricing region manually.

Recommended routes:

- Global: `/pricing`
- Indonesia: `/id/pricing`

---

## 9. Discounts, bundles, floors, and quote validity

### 9.1 Automatic bundle discounts

Automatic discounts apply only to **fixed-price standard core setup modules**.

- Presence + one additional paid core component = **5% off eligible one-time core setup**.
- Presence + paid Environment + paid Capability = **10% off eligible one-time core setup**.

Paid core components are:

- Presence;
- fixed-price paid Environment;
- Act or Transact.

No auto-discount applies to:

- usage/overage;
- integrations;
- third-party pass-through;
- tax;
- premium provider charges;
- custom engineering;
- `setup_from` work;
- Enterprise work.

### 9.2 Annual recurring discount

Annual prepayment receives **10% off eligible recurring platform fees**.

Usage and overage remain pay-as-used and are not discounted by annual prepayment.

### 9.3 Discount authority

The automated agent has **zero discretionary discount authority** beyond the defined bundle and annual-prepay rules.

If a client asks for a lower price:

1. reduce scope;
2. choose a lower Presence tier where appropriate;
3. choose a lower Capability level where requirements permit;
4. simplify Environment;
5. remove optional integrations/add-ons.

Do not silently discount the same scope.

A human may authorize special discounting, but standard setup must not fall below **80% of list setup** without explicit founder/authorized approval.

### 9.4 Quote validity

Standard generated quotations are valid for **14 days**.

Provider pass-through prices and third-party fees may be revalidated at contract signing.

---

## 10. Recommended client-facing bundles

Bundles are marketing shortcuts, not separate architecture.

The pricing engine still calculates the underlying modules.

### Website Concierge

Typical:

- Chat
- Assist
- Landing or existing website

### AI Receptionist

Typical:

- Voice
- Act
- Business environment
- calendar/booking + transactional email

### Digital Human

Typical:

- Human
- Act
- Business environment
- one or more standard actions

### AI Commerce

Typical:

- Human
- Transact
- Commerce environment
- commerce/payment + transactional email

### Enterprise Digital Employee

Custom:

- any Presence;
- complex Capability;
- custom environment;
- legacy/ERP/dedicated infra;
- custom SLA.

Public pricing cards should remain simple. A “Configure Archava” flow should reveal the modular layers only after the user engages.

---

## 11. Deterministic pricing calculation

The source of truth is `config/pricing.v1.json`.

### 11.1 One-time price

`eligible_core_setup = presence.setup + fixed_environment.setup + fixed_capability.setup`

Bundle discount:

- one paid core component: 0%;
- two paid core components: 5%;
- three paid core components: 10%.

Then:

`one_time_total = discounted_core_setup + chargeable_integrations + design_addons + custom_engineering`

Round once at final quote level:

- Global → nearest $50;
- Indonesia → nearest Rp100.000.

### 11.2 Monthly base price

`monthly_base = presence.monthly + capability.monthly + selected_support.monthly`

Environment has no standard recurring fee in pricing v1.

Third-party services may be pass-through or client-owned subscriptions.

### 11.3 Integration calculation

`included_standard_integrations` comes from Capability.

Only Standard integrations may consume the included allowance.

Examples:

- Act with Cal.com only → no extra integration charge.
- Act with Cal.com + HubSpot → one Standard integration is included, one is charged.
- Transact with Shopify + Midtrans → both Standard integrations included.
- Transact with Shopify + Midtrans + custom legacy ERP → Shopify/Midtrans included; legacy ERP is Advanced and charged separately.

Custom/Advanced integrations never consume the free Standard integration allowance.

### 11.4 Usage forecast

The base monthly price includes standard usage.

If forecast usage exceeds the included quantity, the quote must show:

- base recurring price;
- included usage;
- estimated overage separately;
- unit overage rate.

Do not hide expected usage charges inside a single monthly number.

---

## 12. Example calculations

### 12.1 Indonesia — Chat + Landing + Assist

List setup:

- Chat: Rp6.900.000
- Landing: Rp4.900.000
- Assist: included

Two paid core components → 5% bundle discount.

`(6.900.000 + 4.900.000) × 0,95 = 11.210.000`

Rounded → **Rp11.200.000 setup**.

Monthly → **Rp1.490.000/bln** plus overage above 1,000 active conversations.

### 12.2 Indonesia — Voice + Business + Act

List setup:

- Voice: Rp10.900.000
- Business: Rp8.900.000
- Act: Rp4.900.000

Three paid core components → 10% bundle discount.

`24.700.000 × 0,90 = 22.230.000`

Rounded → **Rp22.200.000 setup**.

Monthly:

`2.490.000 + 750.000 = Rp3.240.000/bln`

Act includes one Standard integration.

### 12.3 Indonesia — Human + Commerce + Transact

List setup:

- Human: Rp14.900.000
- Commerce: Rp14.900.000
- Transact: Rp9.900.000

Three paid core components → 10% bundle discount.

`39.700.000 × 0,90 = 35.730.000`

Rounded → **Rp35.700.000 setup**.

Monthly:

`3.490.000 + 1.500.000 = Rp4.990.000/bln`

Includes two Standard integrations.

### 12.4 Global — Voice + Business + Act

List setup:

- Voice: $3,500
- Business: $3,000
- Act: $1,500

Three components → 10% bundle discount.

`$8,000 × 0.90 = $7,200 setup`

Monthly:

`$399 + $149 = $548/mo`

---

## 13. Template catalog

Source of truth: `config/templates.v1.json`.

Initial templates:

1. Generic Business
2. Hospitality / Hotel / Villa / Resort
3. Restaurant / F&B
4. Clinic / Dental / Wellness
5. Real Estate
6. E-commerce / Retail
7. Professional Services / B2B
8. Education / Courses

Templates are not independent codebases. They are compositions of:

- page presets;
- data schemas;
- content types;
- Archava modules;
- recommended integrations;
- default Presence/Capability suggestions;
- design variants.

A client implementation selects the closest template and then toggles modules.

### 13.1 Template-selection principle

Do not create a new template merely because a client has a different brand.

Create a new template only when:

- the information architecture is materially different;
- the workflow/capability graph is materially different;
- three or more clients are likely to reuse the pattern; or
- the existing template would require extensive exceptions.

---

## 14. Archava Studio requirements

Archava Studio is an internal ARCHZXY application, not the client CMS.

### 14.1 New-client configurator

Inputs:

- client name;
- market/region;
- industry;
- existing website URL/status;
- business goals;
- Presence preference;
- actions required;
- transaction requirements;
- expected traffic/usage;
- languages;
- required integrations;
- brand assets;
- compliance/security needs;
- custom design requirements;
- target launch date;
- known budget (optional).

### 14.2 Studio outputs

The Studio must produce:

- selected primary template;
- selected Environment;
- Presence;
- Capability;
- selected modules;
- integration classification;
- implementation assumptions;
- one-time price;
- monthly base price;
- included usage and overage;
- annual-prepay option;
- implementation notes;
- machine-readable client config;
- proposal-ready summary.

### 14.3 Pricebook behavior

Studio must read prices from versioned config. No price may be hardcoded in UI logic.

A quote must record:

- pricebook version;
- effective date;
- selected region;
- quote generation date;
- validity end date.

### 14.4 Configuration modes

Studio should eventually support:

- form/wizard mode;
- expert JSON/YAML import/export;
- clone existing client configuration;
- compare two configurations;
- generate recommended/budget/premium variants.

---

## 15. Client Admin requirements

The client-facing admin is separate from Archava Studio.

Client Admin should provide role-controlled access to:

- pages/content;
- products/services where applicable;
- media;
- FAQ/knowledge sources;
- leads;
- bookings/orders where applicable;
- Archava conversations/outcomes;
- handoff queues;
- analytics;
- permission/settings allowed for the client's role.

Client users must not receive access to:

- ARCHZXY provider secrets;
- global pricing configuration;
- cross-tenant data;
- internal margin/COGS data;
- unsupported action permissions;
- raw system prompts unless intentionally exposed.

---

## 16. Page awareness and Context Graph

Page awareness is a core Archava differentiator.

The SDK should maintain a normalized context graph containing, when available:

- current route/page;
- current section/viewport;
- visible product/service entities;
- selected item/variant;
- comparison state;
- form state;
- cart state;
- checkout step;
- authenticated customer context;
- previous interaction/session context;
- known UI errors;
- active modal/panel;
- available actions on the page.

Primary context source should be structured SDK/DOM/application state, with vision as a fallback or enhancement—not the sole source of truth.

Sensitive fields must be masked before model/provider exposure according to policy.

---

## 17. Knowledge architecture

Archava must distinguish **structured truth** from **retrieval knowledge**.

### Structured truth

Must be read from live authoritative sources where relevant:

- price;
- stock;
- availability;
- booking status;
- customer/order data;
- payment status;
- shipping status;
- account state.

Do not answer these from stale vector retrieval when a live system exists.

### Retrieval knowledge

Use for:

- FAQs;
- product descriptions;
- service explanations;
- policies;
- manuals;
- brochures;
- training documents;
- public website content.

Selected baseline:

- PostgreSQL + pgvector;
- Gemini Embedding-class provider by adapter;
- Firecrawl for public-web ingestion;
- Docling for document parsing;
- source provenance stored per chunk/document.

Every answer should be able to preserve source provenance internally for debugging/evals.

---

## 18. Action, capability, and permission model

Every external action must be registered with:

- capability name;
- tenant;
- description;
- required inputs;
- risk level;
- confirmation mode;
- allowed roles;
- idempotency behavior;
- audit behavior;
- failure/rollback strategy where available.

Recommended action levels:

- **L0 Read** — product/availability/order status.
- **L1 UI** — navigate, highlight, filter, compare.
- **L2 Reversible** — fill draft, add/remove cart item.
- **L3 External state** — send form, create booking, send email, CRM write.
- **L4 Money/account** — checkout, payment, cancellation, refund, shipping/account changes.
- **L5 Admin** — never autonomous by default.

Capability tier does not override risk rules. A Transact client still requires appropriate confirmation for L4 actions.

---

## 19. Transaction lifecycle

For Transact implementations, Archava should support a state machine such as:

`discovery → selection → cart → checkout → payment_pending → payment_confirmed → confirmation_sent → fulfillment/support`

Required characteristics:

- idempotent transaction actions;
- server-side validation;
- no client-supplied price as authoritative truth;
- payment provider webhook verification;
- order/booking identifier captured from authoritative backend;
- transactional receipt/confirmation;
- error recovery;
- human handoff if policy or state requires it.

Payment gateway fees are merchant/client costs unless contracted otherwise.

For Indonesia, Midtrans is the default payment adapter baseline; alternatives such as Xendit remain supported by adapter.

---

## 20. Durable workflows

Some flows outlive a realtime conversation.

Examples:

- payment pending;
- reminder tomorrow;
- abandoned booking follow-up;
- human approval;
- refund review;
- order/shipping notification;
- post-purchase sequence.

Selected baseline: **Trigger.dev** for v1.

Workflows must be resumable, retryable, observable, and tenant-scoped.

Do not couple durable business workflows to the lifetime of a LiveKit room or model session.

---

## 21. Presence provider architecture

Presence is a replaceable adapter.

### v1 default

- Realtime transport/orchestration: LiveKit
- Realtime AI brain: Gemini Live-class provider through adapter
- Default Human rendering: Spatius
- Premium renderer option: Tavus or approved alternative

Spatius must not be hardcoded outside the provider adapter because:

- vendor terms/pricing may change;
- client device/network requirements vary;
- premium clients may require a different embodiment provider.

The frontend must run a Human-mode preflight:

- renderer support;
- network reachability;
- session token/bootstrap;
- basic performance readiness.

On failure:

`Human → Voice → Chat`

Archava must remain usable.

---

## 22. Technical baseline

### Application / monorepo

- Nx + pnpm
- Next.js
- TypeScript
- Tailwind + private shadcn registry
- Storybook for reusable UI/component review

### CMS/admin

- Payload
- multi-tenant architecture

### Data

- Neon PostgreSQL
- pgvector
- Drizzle
- Upstash Redis for ephemeral/cache/rate-limit/locks
- Cloudflare R2 for files/media

### Realtime

- LiveKit Agents
- provider abstraction for realtime model
- provider abstraction for avatar/presence

### Durable background work

- Trigger.dev

### Integrations

- Postmark transactional email default
- Midtrans Indonesia payment default
- Xendit/Stripe adapters
- Cal.com appointment adapter
- Shopify/WooCommerce/Medusa commerce adapters
- CRM adapters
- ERPNext/Frappe adapter for ERP-oriented deployments rather than rebuilding generic ERP

### Analytics/observability

- PostHog product analytics
- OpenTelemetry
- Arize Phoenix OSS for AI traces/evals
- Sentry for application errors/performance

### Security/dev tooling

- Infisical or equivalent secret management
- GitHub Actions
- Vitest
- pytest + Ruff + MyPy for Python services
- Playwright E2E
- k6 load testing
- Semgrep
- Trivy
- OWASP ZAP staging scans

---

## 23. Multi-tenancy

Standard clients should default to a shared platform/tenant model when requirements permit.

Each tenant must isolate:

- content;
- knowledge;
- customer data;
- sessions;
- conversations;
- integrations;
- secrets references;
- permissions;
- analytics;
- pricing/contract metadata.

Enterprise clients may use dedicated deployment/database/storage when contracted.

No cross-tenant retrieval is permitted.

---

## 24. Archava Web SDK

The SDK must be installable into an existing site with minimal friction.

Target API concept:

```html
<script src="https://cdn.archava.example/sdk.js"></script>
<script>
  Archava.init({ clientId: 'client_xyz' })
</script>
```

Implementation should use strong style isolation (for example Shadow DOM where suitable) and expose integrations for modern app frameworks.

SDK responsibilities:

- initialize UI/presence;
- collect approved page context;
- synchronize route/section/entity state;
- receive UI actions;
- render highlights/generative UI surfaces;
- handle presence fallback;
- emit analytics;
- respect consent/privacy settings.

SDK must never expose server API keys.

---

## 25. Generative UI

Generative UI is a strategic differentiator, but must be schema-controlled rather than arbitrary model-generated frontend code.

Archava should be able to request trusted UI primitives such as:

- recommendation list;
- comparison table;
- booking picker;
- product shortlist;
- order summary;
- FAQ/source card;
- CTA;
- human handoff card.

Model output selects from a validated registry and supplies validated props. It does not execute arbitrary client-side code.

---

## 26. Human handoff

Handoff must preserve context.

Staff should receive:

- customer identity where permitted;
- reason for handoff;
- concise conversation summary;
- selected product/service/order;
- actions already attempted;
- current page/context;
- errors/failure state.

The customer should not need to repeat the entire story.

Handoff events must be tracked in analytics.

---

## 27. Analytics and outcomes

Client dashboard should evolve beyond message counts.

Minimum events:

- conversation_started;
- meaningful_question_answered;
- recommendation_shown;
- comparison_started;
- lead_captured;
- booking_started;
- booking_completed;
- cart_action;
- checkout_started;
- transaction_completed;
- receipt_sent;
- handoff_requested;
- handoff_completed;
- knowledge_gap;
- tool_failure;
- presence_fallback.

Derived metrics:

- conversation → lead conversion;
- conversation → booking conversion;
- conversation → transaction conversion;
- revenue influenced where attribution is defensible;
- handoff rate;
- answer success/knowledge gap rate;
- common objections;
- frequently compared products;
- drop-off stage;
- provider cost per successful outcome.

Outcome-based billing may be offered for custom Enterprise contracts later, but v1 public pricing remains platform + usage.

---

## 28. Security and privacy requirements

Mandatory:

- server-side secrets only;
- tenant-scoped authorization;
- least privilege;
- explicit tool allowlists;
- action-level permission checks;
- audit logs for state-changing actions;
- idempotency for sensitive actions;
- rate limits;
- safe provider failover;
- prompt/tool injection defenses;
- PII masking/minimization where appropriate;
- retention controls;
- authenticated webhook verification;
- dependency/SAST/container/DAST checks;
- AI disclosure for realistic avatar interactions where required.

High-risk business actions must not rely solely on natural-language intent without confirmation/policy checks.

---

## 29. Reliability and performance targets

Initial targets, to be validated in production:

- graceful session recovery/fallback;
- 99.9% platform target for standard hosted control-plane components after production maturity;
- Human preflight failure must not make Chat/Voice unavailable;
- tool calls must have timeouts/retries appropriate to action type;
- transaction actions must be idempotent;
- realtime sessions must expose latency telemetry;
- page-awareness event processing must not noticeably degrade page performance.

Performance metrics:

- time to Archava UI ready;
- time to realtime connection;
- time to first spoken response;
- turn latency;
- avatar first frame;
- avatar FPS/client performance;
- tool latency;
- checkout completion latency;
- fallback rate.

---

## 30. Testing strategy

### Unit

- pricing calculator;
- template classifier;
- capability classification;
- integration classification;
- permission engine;
- provider adapters;
- context normalization.

### Contract/integration

- LiveKit/model/avatar provider contracts;
- payment gateway webhooks;
- email;
- calendar;
- commerce;
- CRM.

### E2E

At minimum test:

- Chat Assist on existing site;
- Voice Act booking flow;
- Human Transact commerce flow;
- payment success;
- payment failure;
- receipt email;
- handoff;
- Human → Voice fallback;
- stale/incorrect knowledge protection;
- tenant isolation.

### Commercial-engine tests

Every pricebook release must test:

- Global and Indonesia quotes;
- bundle discounts;
- integration allowance;
- rounding;
- annual prepay;
- overage;
- custom/Enterprise stop conditions.

---

## 31. Reference products

The platform should ship with reference configurations, not separate forks:

1. **Archava Stay** — hospitality/resource booking.
2. **Archava Clinic** — services/appointments.
3. **Archava Realty** — lead qualification/viewings.
4. **Archava Store** — commerce/checkout/post-purchase.

Each reference must exercise a materially different workflow. The purpose is to prove the platform is generic and prevent White-Rock-specific assumptions from returning to the core.

---

## 32. Canonical showcase

The flagship Archava demo should show the product end-to-end rather than only the avatar.

Recommended scenario:

1. Customer lands on product/service page.
2. Archava knows what is visible.
3. Customer asks an ambiguous contextual question.
4. Archava resolves the reference correctly.
5. Archava recommends/compares options.
6. The UI adapts to the recommendation.
7. Customer selects.
8. Archava performs permitted action/cart/booking.
9. Customer proceeds to checkout/payment.
10. Payment succeeds.
11. Archava confirms and sends receipt/confirmation email.
12. Customer asks a post-purchase question.
13. Archava answers from order/booking truth.

This is the minimum demo that proves Archava is not merely a talking avatar.

---

## 33. Build phases

### Phase 0 — Freeze specification

- approve PRD;
- approve pricebook;
- approve templates;
- approve `AGENT.md`.

### Phase 1 — Platform foundation

- Nx workspace;
- Next/Payload/Postgres baseline;
- tenant model;
- config/pricing loaders;
- CI/security baseline.

### Phase 2 — Archava Core

- context graph;
- page awareness;
- knowledge;
- capability/permission registry;
- Chat presence;
- analytics.

### Phase 3 — Realtime

- LiveKit;
- Gemini Live provider;
- Voice;
- Spatius Human provider;
- fallback routing.

### Phase 4 — Act

- forms;
- lead capture;
- calendar/booking;
- transactional email;
- CRM adapter.

### Phase 5 — Transact

- commerce adapter;
- Midtrans/Stripe/Xendit adapters;
- checkout;
- payment verification;
- receipt;
- post-purchase.

### Phase 6 — Studio & commercial engine

- requirement wizard;
- template selection;
- quote calculator;
- proposal generation;
- config export.

### Phase 7 — References and polish

- Stay;
- Clinic;
- Realty;
- Store;
- benchmark/reliability/performance pass.

---

## 34. Acceptance criteria for v1 commercial readiness

Archava v1 is commercially ready when:

- all three Presence modes work through one shared intelligence core;
- at least four reference configurations run without industry hardcoding;
- requirement → template → module selection is deterministic;
- price calculator matches pricebook tests exactly;
- quotes clearly separate setup, monthly, usage, third-party/pass-through, and tax;
- Human falls back gracefully;
- page awareness works on at least one existing-site embed and one ARCHZXY environment;
- Act can complete one booking/CRM/email workflow;
- Transact can complete one full payment → confirmation → receipt workflow;
- tenant isolation tests pass;
- critical security scanning/tests pass;
- observability captures model/tool/action traces;
- provider costs can be measured per tenant;
- proposal generation can be produced from a client brief without manually inventing a price.

---

## 35. Commercial and pricing review schedule

Pricebook is versioned independently from product code.

Review at least quarterly, and immediately if:

- default model pricing materially changes;
- Spatius/default avatar pricing materially changes;
- LiveKit/hosting cost materially changes;
- gross margin falls below internal target;
- global or Indonesia close rates indicate systematic mispricing;
- a new package creates recurring delivery exceptions.

Do not change old signed contracts retroactively unless contract terms permit it.

---

## 36. Research basis and market anchors

This baseline was informed by current 2026 market/provider data, including:

- Clutch September 2026 pricing guides: typical web projects on the platform commonly below $10k; reviewed software/AI projects often in the $10k–$49k/$49.999 range.
- Intercom Fin 2026 outcome pricing: $0.99 common Fin outcomes and higher pricing for qualified sales outcomes, demonstrating movement toward value/outcome pricing.
- Paddle localized pricing documentation: explicit country-level pricing overrides based on willingness to pay/purchasing power rather than only FX conversion.
- Stripe Adaptive Pricing: reported international revenue uplift from localized pricing, supporting localized buyer experience.
- Spatius September 2026 public pricing: very low avatar-rendering economics, commercial usage on paid plans, and high included-minute tiers.
- LiveKit official Spatius integration: client-side rendering and native LiveKit Agents integration.
- Midtrans public Indonesia pricing: no implementation/monthly fee for payment service; merchant pays transaction fees per successful payment method.
- Payload official multi-tenant plugin: tenant-scoped collections/admin behavior.
- Trigger.dev public pricing/docs: long-running resilient tasks without standard timeouts and optional self-hosting.
- Cloudflare R2 public pricing: low object-storage cost with no egress-bandwidth charge.
- PostHog public usage model: generous free analytics/session-replay tiers suitable for early deployments.

Provider/market evidence is used to set a rational baseline, not to guarantee future third-party pricing. Machine-readable ARCHZXY pricebooks remain the authoritative quote source.

---

## 37. Source-of-truth hierarchy

For implementation and quoting, use this precedence:

1. `config/pricing.v1.json` — prices and commercial rules.
2. `config/templates.v1.json` — template/module defaults.
3. `AGENT.md` — deterministic selection/quotation behavior.
4. This PRD — product/architecture requirements.
5. Client-specific signed Statement of Work / contract.

If a contradiction exists, stop automatic finalization and flag the conflict for ARCHZXY review.

---

## 38. Final product rule

A standard client should feel that ARCHZXY built a tailored digital employee for their business, while ARCHZXY should feel that it configured and extended a repeatable platform rather than rebuilding the product from scratch.

That is the core productization test for Archava.

---

## 39. Decision Intelligence — internal bounded judgment

Decision Intelligence extends the Context Graph and turn pipeline as an **internal** orchestration layer. It is not a Presence or Capability tier, a priced add-on, a replacement for `BrainProvider`, or a source of facts. `DecisionProvider` is the vendor-neutral product contract; Jev is the first external adapter, and Laya remains a future seam. No pricing rule changes in this phase.

### 39.1 Authority and non-purpose

| Question                                                        | Authority                                     | Decision Intelligence role                              |
| --------------------------------------------------------------- | --------------------------------------------- | ------------------------------------------------------- |
| Price, stock, availability, booking/order/payment/account state | Structured authoritative DB/API               | May recommend routing; cannot supply or override a fact |
| Currency math, ARCHZXY/client pricing, tenant capability        | Deterministic code and pricebook              | None                                                    |
| Open-ended answer and conversation                              | `BrainProvider`, grounded by approved sources | May supply bounded advisory signals                     |
| Permission and confirmation                                     | `ActionPolicy`                                | Cannot allow an action or waive confirmation            |
| Input safety and entity existence                               | Validation and authoritative resolver         | Cannot validate by probability                          |
| Side effect and success                                         | `ActionExecutor`                              | Cannot execute or report success                        |

The order is **fact → deterministic rule → bounded fuzzy judgment → conversational reasoning → permission → input validation → side effect**. A confidence of 1.0 grants no authority. Structured truth always outranks retrieval, including when a provider recommends the reverse. If a live source is unavailable, retrieval cannot become an authoritative transactional answer.

### 39.2 Contract, tasks, and rollout

A `DecisionRequest` contains a tenant scope, redacted utterance, locale, projected context and closed typed questions. A `DecisionResponse` contains Boolean, Choice or Score answers with confidence, or explicit refusals. The first catalogue has intent classification, knowledge routing assist, clarification recommendation, handoff recommendation, and evidence sufficiency. A bounded ranking primitive can reorder only already-valid candidates; no ranking task is enabled yet. Recommendation ranking, next-best-action, lead qualification, comparison relevance, Generative UI selection, support priority, conversation stage, adaptive Presence, outcome detection and post-answer quality judging are **Planned**.

Tenants may configure `off`, `shadow`, or `assist` per task. Missing configuration defaults to Foundation V1.1 behavior. `off` makes no provider call. `shadow` records a candidate and disagreement while leaving the turn unchanged. `assist` may apply only an enabled task whose response validates and clears that task's confidence floor and direction policy. Tenant floors may tighten the built-in floor. Handoff and clarification may be raised but mandatory deterministic cases cannot be suppressed. Knowledge routing may escalate to structured truth only with trusted subject candidates; it may never downgrade protected structured truth.

The `DecisionOrchestrator` selects the provider, builds catalogue questions, enforces mode, timeout, response validation, per-task confidence, fallback, and trace metadata. `RuleDecisionProvider` is the offline deterministic baseline and fallback. A server-side `JevDecisionProvider` implements the same port with a mocked transport test suite; live external verification requires credentials and is separate from ordinary CI. Provider failure, timeout, refusal, malformed output and low confidence return the deterministic baseline. No business package branches on Jev identity.

### 39.3 Tenant, privacy, observability, and evaluation

Requests are tenant-scoped. Only projected, redacted context and limited published evidence may cross a provider boundary; raw forms, secrets, payment cards, sensitive customer state and unrelated tenant data must not. Decision traces and events are metadata-first: task, provider, mode, candidate label, confidence, effective source, fallback, disagreement, runtime change and latency. The event vocabulary includes `decision_evaluated`, `decision_fallback_used`, `decision_low_confidence`, and `decision_provider_failure`. Provider payloads and credentials must not be logged.

An offline English, Indonesian and mixed-language corpus must report accuracy, per-label counts and routing critical failures. Any structured-truth-to-retrieval downgrade in effective runtime is a severe failure. Safety tests must prove that Act/Transact limits, confirmation, validation, entity resolution and executor success remain owned by their Foundation boundaries. Ordinary install, structure, lint, typecheck, test, build, verify, format and browser E2E gates must remain network-free and green. Live Jev smoke is optional until the owner supplies credentials and approves applicable retention terms.

### 39.4 Acceptance for this phase

The layer is reviewable when the provider-neutral contract, Rule provider, orchestrator, three modes, initial task catalogue, server-side Jev adapter, mocked transport tests, runtime safety regressions, offline eval, reference debug trace, additive tenant schema and documentation are present and ordinary gates pass. Laya and other future adapters are not implemented. This phase does not satisfy the separate §34 commercial-readiness criteria for real Act and Transact workflows.

**Prior PRD sections touched:** none. §39 is additive; §§16–18, 21, 23, 26–30, 34 and the commercial rules retain their existing authority and wording.
