# ARCHAVA Platform — Master Product Requirements Document

**Company:** ARCHZXY — AI-native Creative & Product Studio\
**Product:** Archava\
**Document version:** 2.1.0\
**Commercial baseline:** Pricing v1.0\
**Effective date:** 10 October 2026 (execution scope); 7 October 2026 (reusable product scope); 22 September 2026 (unchanged Pricing v1.0)\
**Primary implementation workspace:** `/home/haikaru/Archverse/Lab/Archava_Platform/`\
**Status:** Research-informed product specification and incremental roadmap. Reusable modules and production readiness require the acceptance evidence below.

**Research:** [Reusable platform research](docs/REUSABLE_PLATFORM_RESEARCH.md), reviewed 7 October 2026.
**Research addendum:** [Business-model research](docs/BUSINESS_MODEL_RESEARCH_2026-10-08.md)
and [capability map](docs/BUSINESS_CAPABILITY_MATRIX.md), reviewed 8 October 2026.
**Previous specification:** [Preserved PRD v1.0.0](docs/history/PRD.v1.0.0.md).
**Scope of this revision:** ARCHZXY is the company; Archava is its flagship product. The internal ARCHZXY module library supports Archava and reusable CRM/ERP business capabilities. The ARCHZXY company website is a separate project. §§40–49 define the new reuse scope; existing section numbers remain stable. Document version, product/package versions and pricebook version are independent.

**Execution target:** §50 defines the next bounded local reusable-library release,
its ordered work and acceptance evidence, following the owner's local-first/free
choice. Completing that release does not complete the full commercial §34 target
or every future industry module. The [10 October re-audit](docs/PROJECT_REAUDIT_2026-10-10.md)
is the implementation baseline; requirement text does not replace its evidence.

---

## 1. Executive summary

Archava is ARCHZXY's flagship AI interface to a business. It combines business knowledge, page awareness, customer/employee context, realtime conversation, permitted actions, transaction orchestration and optional realtime avatar presence. ARCHZXY develops an internal module library for these capabilities and for deterministic CRM/ERP business modules that can be composed across business models.

Reuse covers data models, business rules, APIs, dashboard components, events, migrations and Archava tools. Business modules also work through dashboards/APIs independently of the LLM runtime. Shared primitives, supported configuration and explicit vertical extensions replace repeated client-specific rebuilding. Archava consumes their authoritative contracts; it does not own financial truth by inference.

This repository contains the product/library and reference consumers. The ARCHZXY company website, company sales funnel and studio delivery operations are consumer projects outside this repository. They may use the library and Archava SDK/API later.

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

The platform must include an internal **Archava Studio** that accepts client requirements, selects the closest reusable template, enables the required modules, calculates an accurate quote from the machine-readable pricebook, and generates a proposal/configuration. The agent rules for this behavior are defined in `agent.md`.

---

## 2. Product thesis

### 2.1 What Archava is

Archava is a conversational business interface for customers, employees and owners with distinct identities, data access and tool permissions. Customers can ask questions, receive contextual answers, navigate, perform allowed transactions and return for support. Authorized employees/owners can query operational records, prepare drafts and perform permitted workflows. Deterministic business systems remain authoritative for records and actions.

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

### 2.3 First-party ownership and reusable contracts

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

Provider implementations may be replaced without redefining the product. ARCHZXY ownership applies to its first-party work subject to contracts and dependency licenses. Client data, brand/content and contracted client-specific assets remain separately defined. Platform source delivery is not implied by reuse or a client application handoff; third-party software retains its own license/ownership. See §40 and the research license review.

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

The product must:

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
- expose independently consumable module contracts and compose CRM/ERP capabilities across business models;
- let business modules serve authorized dashboard/API consumers without depending on an LLM;
- produce deterministic regional pricing from client requirements;
- be provider-agnostic for LLM/realtime voice/avatar infrastructure;
- fail gracefully from Human → Voice → Chat when required.

### 4.2 Business goals

The platform must let ARCHZXY:

- stop pricing projects ad hoc;
- quote a client quickly and consistently;
- target substantial reuse of standard implementation work, measuring integration effort and forks rather than claiming a guaranteed 70–90% ratio;
- preserve premium pricing despite lower infrastructure cost;
- sell in Indonesia with a localized IDR pricebook;
- sell globally with a separate USD pricebook;
- gradually shift from custom service revenue toward recurring Archava platform revenue;
- measure gross margin by client, provider, presence mode, and capability usage.

---

## 5. Non-goals for the initial reusable release

The initial reusable release will **not** attempt to:

- build a new foundational LLM;
- build a production-grade realtime avatar renderer from scratch;
- deliver a universal ERP or statutory accounting/payroll engine;
- build the ARCHZXY company website or internal delivery/sales system in this product repository;
- replace Shopify/WooCommerce/Medusa where a mature commerce engine already exists;
- require replacement of client CRM/PMS systems; adapters remain a supported path;
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

A business owner or team member configuring customer-facing Archava or using an authenticated operational Archava surface.

Needs:

- update content/product knowledge;
- view leads/conversations/outcomes;
- see bookings/orders/actions;
- control what Archava can do;
- review knowledge gaps;
- manage team handoff;
- understand ROI;
- query tenant-scoped CRM/operational records;
- prepare and approve actions within their role and resource permissions.

Public customer and employee/owner access must use separate trusted principal/tool projections. A machine actor does not inherit an owner's permissions.

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

Templates are compositions rather than independent client forks. Their commercial defaults remain in the existing catalog; implementation readiness and dependency/version checks are separately required by §§42–43. They compose:

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

Workflow durability is a requirement; a workflow vendor is an implementation decision. Trigger.dev was the v1 planning candidate and is not installed in the current platform. Retain tenant-bound recovery/idempotency contracts and choose at most one queue/workflow tool through an ADR when a concrete flow requires it. BullMQ or Temporal are alternatives to evaluate for that need, not dependencies mandated by this revision.

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

## 22. Technical baseline and adoption rules

### 22.1 Current implementation to preserve

The audited platform uses pnpm workspaces, TypeScript, Zod, Node HTTP hosting, esbuild, Vitest and GitHub Actions. Workspace packages expose assistant, knowledge, SDK, policy, configuration, deterministic pricing and Act contracts. PostgreSQL attempt/confirmation adapters use injected SQL clients and explicit migrations; this is not a deployed general business database.

The 10 October checkpoint additionally verifies private compiled artifacts,
independent core/CRM consumers, persistent authenticated owner hosting, two native
approved follow-up actions and actual backup restoration. Preserve those working
implementations. They are bounded local business capabilities, not a full ERP or
production deployment. Current evidence is linked from build status.

The portable realtime template has a separate React/Vite frontend, Node API and Python LiveKit worker. Its own provider dependencies and deploy recipes do not prove shared Chat/Voice/Human intelligence or live readiness. See [architecture](docs/ARCHITECTURE.md) and [build status](docs/BUILD_STATUS.md).

### 22.2 Proposed architecture

Use a modular monolith for the initial business runtime, with reusable packages and explicit public service/API/event contracts. Keep the Python realtime worker behind a typed boundary. Business modules own records, migrations and domain invariants; the assistant core owns intelligence/orchestration contracts. Dashboards and Archava use the same authorized business service. Module boundaries do not require independent network services.

PostgreSQL is the proposed durable record store. Begin with explicit migrations and a tested tenant/auth boundary. Persistence, CMS, backend frameworks, caches and workflow engines are chosen incrementally; no framework adoption may erase existing safety behavior or passing regressions.

### 22.3 Tools are candidates, not an installation checklist

The v1 wishlist named Nx, Next.js, Payload, Drizzle, pgvector, Redis, R2, Trigger.dev, PostHog, OpenTelemetry, Phoenix, Sentry, Infisical and several security/load tools. Their mention does not establish implementation or mandate adoption. NestJS, Turborepo and BullMQ/Temporal are also candidates, not automatic replacements.

Use an ADR to select a tool only when it solves a current requirement, its dependencies/licenses are reviewed and a small spike demonstrates benefit and operational fit for the team. Select one initial observability/error path. Keep deterministic domain/permission contracts independent of vendors. Kubernetes, arbitrary plugins and service decomposition require measured need.

### 22.4 Business engines and integrations

Build bounded first-party CRM/ERP capability modules from repeated requirements. Use adapters where clients already have authoritative CRM/PMS/ERP/commerce systems. Frappe/ERPNext, Odoo, Twenty and Medusa are integration/build-versus-buy candidates or design references; none becomes a mandatory platform dependency through this PRD.

Keep existing Postmark, booking and presence ports; payment providers retain the §19 authoritative verification requirement. Validate provider-specific behavior in controlled live tests before advertising an integration as ready. Pricing v1 and existing configured defaults remain unchanged by research; suggested providers cannot be treated as connected services.

---

## 23. Multi-tenancy

The proposed hosted default is shared application code and PostgreSQL tables scoped by tenant, after authenticated principal binding and persistence are implemented. Use server-side authorization plus database isolation; ordinary runtime credentials must not bypass RLS. Tenant context is established per transaction, including on reused connections. See §44. Shared code never implies shared tenant records.

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

The reusable-platform milestones are defined in §49. The active implementation
order and definition of done are the bounded local release in §50. Inventory,
independent core consumption and the first persistent CRM slice already have
local acceptance evidence; do not restart those completed milestones merely
because the broader roadmap still lists their original requirements.

Existing Chat/Voice/Human, Act/Transact, Studio and reference-product commitments remain broader product targets. Shared presence and live business integrations progress incrementally while preserving the current core. No mandatory stack rewrite or company-website implementation is a prerequisite to reuse.

Completing a library milestone does not satisfy §34 commercial readiness. Likewise, existing offline test coverage is not proof that new business modules, hosting, identity or external providers are ready.

---

## 34. Acceptance criteria for v1 commercial readiness

The existing full-product v1 commercial readiness gate is retained. PRD v2 extends product scope; it does not declare this gate met. Smaller reusable-library milestones are evaluated separately in §49.

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

The current architecture/module research is recorded in [Reusable platform research](docs/REUSABLE_PLATFORM_RESEARCH.md), including 34 primary sources, dated findings, alternatives, limitations and the decisions they inform.

The September 2026 market/provider anchors that informed Pricing v1 are preserved in the [v1 specification](docs/history/PRD.v1.0.0.md). They are historical inputs, not verified current offers or refreshed economics in this revision. Review actual provider plans, account availability, license editions and costs before commercial launch or a new pricebook release.

Research does not authorize changes to quote numbers or contractual commitments. `config/pricing.v1.json` remains the authoritative active pricebook until explicitly revised. New native CRM/ERP modules have no invented automatic prices; unsupported/unpriced scope requires manual review.

---

## 37. Source-of-truth hierarchy

Authority is separated by subject:

| Subject                                        | Authority                                                                                                                        |
| ---------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------- |
| Active standard quote numbers/rules            | `config/pricing.v1.json`; research, prose or models do not alter them.                                                           |
| Legacy template defaults                       | `config/templates.v1.json`; defaults are suggestions/configuration, not readiness evidence.                                      |
| Quote-selection behavior                       | `agent.md`, constrained by active pricebook and current requirements.                                                            |
| Product scope and future acceptance            | This PRD, including §§40–50. Section 50 selects the current finite local scope; supporting research/proposals explain decisions. |
| Client contractual scope/ownership/obligations | The applicable signed agreement/change order; a later PRD/pricebook does not silently modify it.                                 |
| Operational business records                   | The designated authoritative business module or external system, including verified payment outcomes.                            |
| Permissions and actions                        | Server-verified identity, module/resource authorization and deterministic action/confirmation policy.                            |
| What is implemented                            | Source, verification evidence and `docs/BUILD_STATUS.md`; requirement text is not implementation proof.                          |

If a requirement, pricebook, client agreement or supported configuration conflicts, stop automatic finalization and surface the conflict. Never bypass security or silently rewrite commitments to reconcile it. The v2 module registry will govern validated composition once implemented; it does not retroactively make legacy free-form toggles valid.

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

**History:** §39 was introduced additively without changing prior sections. PRD v2 subsequently clarifies product scope and implementation planning while preserving these authority boundaries and Pricing v1.0. The original wording is retained in the v1 archive.

---

## 40. Company, product and internal library boundaries

### 40.1 Product ownership

ARCHZXY is the company. Archava is its flagship AI interface product. ARCHZXY develops and maintains an internal module library that supports Archava and reusable business applications. A library module is reusable functionality with a public contract and an owner; a starter kit is a tested composition; a client implementation supplies configuration and bounded extensions.

This revision requires reuse of business modules themselves, including their backend and UI, rather than limiting reuse to AI connectors. It does not require every module to depend on the assistant or every Archava deployment to include CRM/ERP.

The ARCHZXY company website is built outside this product repository and consumes supported SDK/API/library contracts. Its sales consultation, proposal/agreement/deposit, project delivery capacity, warranty and maintenance workflows are consumer requirements. Product tooling may expose quote/configuration primitives, but no company-specific workflow becomes the shared core's default.

### 40.2 Ownership and distribution

Client data, brand/content and contracted client-specific assets have explicit ownership/export terms. First-party Archava/core modules, generic improvements and internal tooling remain ARCHZXY IP unless a contract explicitly transfers them. Third-party licenses remain applicable; source access to a client application is distinct from a platform IP transfer. Browser-delivered code is observable and cannot be treated as a secret.

Initially distribute the library internally through controlled artifacts. Public npm publication, client platform-source delivery or an IP buyout is a separate commercial/release decision. Reuse does not imply a public open-source license or deployment in one shared database.

### 40.3 Success measures

Measure integration time, copied/forked core code, client-specific patches, module changes per delivery, supported-profile regression rate, upgrade effort and maintenance load. The 70–90% reuse aspiration is a hypothesis, not a release criterion or guaranteed customer promise. Prefer measured reduction in repeated work while preserving behavior/security.

---

## 41. Reuse layers and domain ownership

| Layer                          | Responsibility                                                                                            | Boundary                                                                                                |
| ------------------------------ | --------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Shared primitives              | Tenant/principal contracts, permissions, identifiers, money/time conventions, audit/event interfaces      | Keep small. Domain-sensitive records do not become globally readable through a shared contact identity. |
| Archava core                   | Context, knowledge/truth ports, conversation, decision advisory, action gating, generative UI and handoff | No mandatory CRM, inventory, company workflow or provider implementation dependency.                    |
| Presence/SDK                   | Chat/Voice/Human surfaces, session transport, page awareness and host integration                         | Same intelligence/action authority; browser artifacts cannot include server credentials.                |
| Business modules               | Contacts, CRM, booking, inventory, orders, billing and later demand-backed capabilities                   | Each owns its records, validation, APIs, migrations and state transitions. Can operate without an LLM.  |
| Integration/action adapters    | Map public domain contracts to native modules or existing client systems                                  | Explicit authoritative resource ownership, credential scope and failure/recovery semantics.             |
| Vertical kits/extensions       | Hotel, retail, services, education and other domain compositions                                          | Supported configuration plus specific records/invariants; no client fork of shared modules.             |
| Client configuration/extension | Branding, allowed features, knowledge, supported workflows and client-only requirements                   | Validated options and registered extensions; no arbitrary tenant code execution.                        |

**REUSE-01:** the same released module implementation must serve at least two reference consumers without copying business logic, importing parent workspace source or adding tenant-name branches to core.

**REUSE-02:** a deterministic business module must accept authorized dashboard/API calls without requiring an LLM/model session; Archava must use the same domain service and state-transition rules.

**REUSE-03:** each resource has one authoritative owner. Cross-module access uses public services, IDs and events. A module cannot silently update another module's internal tables. Transactions spanning modules require an explicit application-service boundary and tested failure semantics; external effects are not made atomic by a database transaction.

**REUSE-04:** configuration changes supported options, not domain invariants. Material differences in scheduling, inventory valuation, patient care or school administration use explicit vertical extensions. A generic contact can be linked to a guest/student/patient record, but those records retain domain-specific permissions and meaning.

**REUSE-05:** existing client systems remain supported via adapters. Read projections may be cached with provenance/freshness, but must not become a competing editable source of truth. Reconcile mappings and deletes explicitly.

---

## 42. Module contract

### 42.1 Manifest requirements

Begin with an explicit registry of trusted first-party module implementations loaded at build/startup. The registry does not download or evaluate packages named by browser/model input. New modules need implementation evidence as well as metadata.

| Field                               | Requirement                                                                                             |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------- |
| `id`, `version`, `contractVersion`  | Stable ID, declared public API version and exact implementation version.                                |
| `requires`, `optional`, `conflicts` | Dependency constraints and incompatible combinations; optional dependencies do not activate themselves. |
| `configSchema`                      | Closed validated options and a migration policy for persisted config.                                   |
| `resources`                         | Owned records/services and data-classification boundary where persistence exists.                       |
| `permissions`, `actions`            | Distinct resource grants and typed Archava action contracts; neither grants permission automatically.   |
| `requiredPorts`, `credentialRefs`   | Registered implementation requirements and server-only secret references.                               |
| `requiredEnvironment`               | Required runtime setting names, validation and public/server classification; never secret values.       |
| `api`, `events`                     | Public service/API/event contracts with versioning and retry semantics.                                 |
| `migrations`                        | Ordered migrations, schema compatibility and deployment/recovery instructions for stored data.          |
| `ui`                                | Optional trusted UI entry points/extension slots and compatible UI contract.                            |
| `healthChecks`                      | Configuration/readiness checks appropriate to the module/provider.                                      |
| `maintainer`, `license`             | Ownership, support responsibility and third-party license inventory.                                    |

A stateless module may omit persistence/UI fields. A module that stores business records cannot defer schema/migration ownership indefinitely. Start with the fields needed by actual modules; this specification does not require a universal plugin engine.

Illustrative future manifest, **not an existing installed module or schema**:

```json
{
  "id": "business.crm",
  "version": "0.1.0",
  "contractVersion": "crm.v1",
  "requires": {
    "foundation.identity": ">=0.1.0 <0.2.0",
    "business.contacts": ">=0.1.0 <0.2.0"
  },
  "optional": ["archava.assistant", "integration.email"],
  "conflicts": [],
  "configSchema": "crm.config.v1",
  "resources": { "owned": ["crm.activities", "crm.opportunities"] },
  "permissions": ["crm.read", "crm.activity.write"],
  "actions": ["crm.activity.create"],
  "requiredPorts": ["records", "authorization", "audit"],
  "credentialRefs": [],
  "requiredEnvironment": [],
  "api": "crm.api.v1",
  "events": ["crm.activity.created.v1"],
  "migrations": ["crm.0001"],
  "ui": ["crm.workspace.v1"],
  "healthChecks": ["crm.records"],
  "maintainer": "ARCHZXY",
  "license": "internal-first-party"
}
```

### 42.2 Resolution and lifecycle

**MOD-01:** resolve declared dependencies against a finite supported release profile. Reject unknown IDs, unsatisfied ranges, cycles, conflicts and attempts to disable required dependencies. Produce explicit reasons rather than silently substituting implementations.

**MOD-02:** installation/configuration, commercial entitlement, UI visibility, runtime readiness and user/action permission are separate states. Turning a module on grants no role/resource access. A module can be installed but unavailable because its required port is missing.

**MOD-03:** disabling a module prevents new invocations and removes its active UI/tools from the supported projection. It must not delete business data, invalidate legal retention or abandon already-started effects without a recovery policy. Uninstall/data purge requires an explicit authorized lifecycle.

**MOD-04:** additional actions use the existing versioned closed action registry,
risk classification and validated executor boundary. Manifest strings alone do
not create executable tools or extend that registry. Preserve the separately
registered native follow-up creation/completion actions and their distinct consent.

---

## 43. Client configuration and resolved plans

### 43.1 Inputs and outputs

Evolve existing `ClientConfig` incrementally. Future configuration selects tenant/deployment, template/kit, supported release profile, requested modules/options, knowledge/truth sources, presence/capability, identity policy and integration references. Runtime uses a validated, versioned resolved plan with exact modules and implementations.

**CFG-01:** preserve separate requested, supported, configured, connected and healthy states. Legacy `enabled: true` is not proof that an integration exists or can execute. Template common integrations begin as suggestions; automatic provisioning requires explicit validated selection and readiness.

**CFG-02:** distinguish supported pipeline/stage labels, field options and workflow settings from executable code. No arbitrary scripts, SQL, dynamic imports or unreviewed schemas come from tenant/browser/model configuration.

**CFG-03:** a public browser config is a safe projection. Secrets, internal margins, employee roles and privileged tool definitions stay server-side. Client IDs select intended deployments; they are not authorization credentials.

**CFG-04:** quote selection and deployment selection use the same supported scope. A suggested integration cannot be promoted to connected scope or omitted from price/manual-review treatment by a config merge. Planned/unpriced native modules receive an explicit unsupported/manual-scope result, never an invented price.

### 43.2 Compatibility

Introduce explicit config versions and migrators alongside the old schema. Legacy configs keep current reference behavior until deliberately migrated. When old free-form module names cannot resolve, report a migration/unsupported issue; do not quietly enable new modules or change pricebook tiers.

The pricebook, legacy template catalog, deployment config and module catalog have independent versions. Freeze the resolved scope with the quote/deployment evidence, including the pricebook version used. The existing resolver remains planning/reference behavior until these new guarantees are implemented and tested.

---

## 44. Tenancy, identity and tool permissions

### 44.1 Trusted identity

**TEN-01:** bind server-owned tenant scope to an authenticated employee/member, scoped service identity or constrained public customer session. Do not trust role, tenant, approval or provider credentials supplied by browser/model input. Public visitors do not need employee login, but receive a separate limited principal/tool/data projection.

**TEN-02:** isolate tenant records, retrieval, cache keys, exports, files, credentials, events, jobs, rate budgets and action attempts. Resource-level ownership also applies within a tenant, such as one customer's booking or one employee's permitted contact set.

**TEN-03:** for shared PostgreSQL data, use tenant-bound keys/queries and tested RLS defense in depth. Runtime roles must not be superusers or `BYPASSRLS`; address table-owner bypass explicitly. Establish transaction-local scope and test reused connections. Migration/admin credentials are separate and their cross-tenant operations explicitly authorized.

Dedicated deployment/database may serve justified isolation requirements using the same library contracts. Data migration between modes must preserve resource IDs, audit/correlation and deletion/retention policies. A client-specific code fork is not required for dedicated hosting.

### 44.2 Authorization

**AUTH-01:** effective action authorization intersects initiating-principal grants/resource ownership, service allowlist, tenant capability/module policy, validated inputs, approval and current business preconditions. A model confidence, enabled module or paid tier grants no independent authority.

**AUTH-02:** business APIs enforce permissions and invariants for every entry point: dashboard, direct API, Archava and background worker. UI controls are conveniences, not enforcement. Preserve L0–L5 and capability boundaries, including existing L5 non-autonomy. General CRM writes require Act; the existing bounded Assist lead-capture allowance does not authorize arbitrary CRM mutation.

**AUTH-03:** bind confirmation to canonical payload, target/resource revision, expiry, tenant and principal/session. Recheck time-sensitive permissions and preconditions before execution. Keep raw provider keys, credentials and approval tokens out of prompts/logs.

**AUTH-04:** preserve one canonical intent/attempt across retries and restarts. Distinguish a retried action from an intentionally new identical action. Atomic durable attempt identity and one-time approval consumption must be demonstrated before the corresponding real writes. Unknown results remain unknown until authoritative reconciliation or a defined operator recovery.

These requirements extend the existing `ActionExecutor`, `GatedAction`, policy and
host contracts incrementally. Local authenticated CRM hosting and durable native
intents now have the evidence recorded in the 10 October re-audit. Those local
implementations do not establish public identity or live external-provider readiness.

---

## 45. Versioning, migrations and recovery

**REL-01:** declare a public contract before applying semantic versions. Initially support a small set of tested release profiles with exact resolved versions instead of every possible module combination. Document supported runtimes/UI contracts and incompatible changes.

**REL-02:** separate product release, module package, config schema, API/event, database schema and pricebook versions. A module version bump does not authorize a price change or automatic client upgrade.

**REL-03:** each persistence module owns ordered migrations and a migration ledger. Test an existing-data upgrade, interrupted run, supported restart/recovery and prior/current code compatibility. Use expand/contract changes for shared services where mixed deployments may coexist. Destructive changes require explicit data/recovery planning; code rollback is not proof of data rollback.

**REL-04:** tenants sharing tables cannot independently apply incompatible schema histories. Coordinate a shared-service schema release. Dedicated deployments may remain pinned within a documented support window. Record which profiles are tested; do not claim compatibility from version constraints alone.

**REL-05:** before hosted persistent modules go live, back up database/files/config as applicable and demonstrate restoration to an isolated environment. Record observed recovery time, backup age and consistency. Assign support-tier RPO/RTO commitments only after capacity and restore evidence exist.

**REL-06:** privacy lifecycle covers exports, deletion/retention, backups, audit and provider data. Deleting a contact must reconcile dependent records and retention duties; do not blindly cascade financial/audit deletion. Curated support knowledge records problem, root cause, affected version, fix and prevention rather than ingesting all conversations automatically.

---

## 46. CRM / ERP across business models

### 46.1 First business slice

The proposed first slice is tenant-scoped contacts plus activities/follow-up tasks. Demonstrate it in hospitality inquiry and professional-service inquiry reference contexts; replace these examples if actual repeated customer demand identifies a better slice. Reference contexts do not require building the ARCHZXY company website.

**CRM-01:** provide bounded contact list/detail/create/update and activity/task operations, permission checks, record revisions and durable audit. Typed validation and pagination must bound query/mutation scope. Automatic bulk messaging or arbitrary workflow execution is outside the first slice.

**CRM-02:** dashboard and Archava operate on the same authorized records/service. Archava can list contacts, retrieve activity context and prepare a follow-up; mutation/sending follows existing capability/approval rules. External email delivery is enabled only when its separate Act/provider gates pass.

**CRM-03:** two contexts share the same released contacts/activity implementation. Labels, supported fields and views can differ through config; genuinely new domain records live in extensions. Seed/demo data are isolated fixtures and never represented as real customer records.

**CRM-04:** later leads/opportunities and sales stages must have explicit transition, assignment, visibility and audit policies. Stage relabeling cannot bypass domain rules. A contact, guest, student and patient are distinct business concepts even when linked to a shared identity.

### 46.2 Operational capability growth

ERP is a composition of operational modules, not a mandatory application installed for every tenant. Add capabilities only after repeated requirements establish their actual rules.

| Candidate   | Shared capability                                        | Domain-specific obligations                                                           |
| ----------- | -------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| Inventory   | Items, locations and movement/reservation contracts      | Units, lots/expiry, valuation, concurrency and negative-stock rules.                  |
| Booking     | Resource/time reservation, availability and confirmation | Room-night versus appointment semantics, overbooking/cancellation and timezone rules. |
| Orders      | Lines, lifecycle and fulfillment references              | Returns, partial fulfillment, service delivery and external-commerce ownership.       |
| Billing     | Invoice/payment references and verified settlement       | Currency, taxes, numbering, corrections, retention and accounting integration.        |
| Procurement | Suppliers, purchase request/order and receipt            | Approval limits, lead times and stock/accounting integration.                         |
| Projects    | Tasks, milestones and approved scope references          | Workflow/assignment, changes and capacity rules belonging to that business.           |

**ERP-01:** define each module's invariants before implementation and validate them on actual business scenarios. Stock quantity and time-slot capacity must not be collapsed into a single generic CRUD table. An invoice/payment module is not an assertion of full accounting, tax or payroll compliance.

**ERP-02:** keep posted financial/stock history traceable. Corrections use authorized adjustment/reversal semantics appropriate to the source system. An LLM cannot edit immutable financial truth, mark payments successful or make accounting policy decisions.

**ERP-03:** support external authoritative systems with scoped adapters when client needs exceed first-party module scope. Avoid synchronized writable duplicates unless there is an explicit ownership/conflict/reconciliation design.

### 46.3 Vertical kits

Retail can compose contacts/CRM, product inventory, orders and billing. Hospitality can compose contacts/CRM, resource booking and billing with rooms/housekeeping extensions. Education can compose contacts, billing and notifications with students/attendance extensions. These are target compositions, not shipped kits or promises that all data/rules are interchangeable.

Regulated medical operations, broad payroll/statutory accounting and manufacturing remain later specialized requirements. Every kit declares supported modules/config versions and contract tests. Reusable improvements feed back into the library only after removing client identifiers/secrets and validating general applicability.

### 46.4 Research catalog for business discovery

The [business capability map](docs/BUSINESS_CAPABILITY_MATRIX.md) is a non-runtime
planning addendum. It distinguishes operating patterns, revenue, channels/actors
and industry extensions, with illustrative combinations and candidate ownership.
A business can combine several patterns. Candidate names do not register modules,
grant permissions, assign prices or establish supported vertical kits. Actual
implementation status and acceptance evidence remain authoritative. Expand the
catalog from validated requirements while preserving §§41–46 and the M3 gates.

---

### 46.5 Starter-kit coverage research

The [10 October starter-kit research](docs/STARTER_KIT_COVERAGE_RESEARCH_2026-10-10.md)
proposes a planning catalogue of 40 kit families: 24 general business and 16
specialist, with 20 shared capability candidates and ten configuration dimensions.
These are ARCHZXY design choices, not a universal minimum or market-coverage claim.
The [machine-readable research catalogue](docs/audit/2026-10-10-starter-kit-catalog.json)
records workflow, composition candidates, invariants, evidence depth and residual
gaps against all 22 KBLI 2025 categories. It is not consumed by the runtime registry.

A kit must address its actual business operations: hotel room/stay/housekeeping,
restaurant order/preparation/service, and other domain-owned workflows. Shared CRM
is a component of relevant kits, not acceptance of an operational industry kit.
The eight existing website/planning templates do not establish these capabilities.
No complete operational kit from this research catalogue is currently accepted;
existing local library/CRM acceptance remains unchanged.

Require a traceable disposition for each mandatory requirement. Unsupported,
unimplemented, unconfigured, unhealthy or domain-unreviewed requirements block
assembly; a generic template cannot conceal them. A proposed external adapter
does not count as supported until its scoped integration acceptance exists.
Kit/profile selection never grants authorization or invents a price. Unpriced
native scope retains manual review under the unchanged pricebook.

Preparing this catalogue does not authorize implementing all families, paid calls,
publishing, the company website or a universal ERP. Build bounded kits from
validated demand, preserve §§41–50, and generalize from proven reuse. Missing
workflow evidence remains explicit, especially taxonomy-only specialist families.

---

### 46.6 Authorized kit implementation programme

The user subsequently authorized implementation of the researched programme and
delegated its order. Track the objective and outstanding work in
[the kit implementation ledger](docs/STARTER_KIT_IMPLEMENTATION.md). The research
catalogue alone remains insufficient evidence of operational support.

Implement shared capabilities and bounded kit scopes incrementally, with explicit
domain ownership and real persistence/integration evidence. The first extension
is internal project/task/milestone tracking through a reusable domain service,
PostgreSQL adapter, strict API, read tools and owner dashboard. Its local scope and
limits are defined in [the project module contract](docs/PROJECT_DELIVERY_MODULE.md).
It does not create client contracts, accept payments, promise capacity or expand
scope through an LLM.

Profile 1.4.0 extends historical profile 1.3.0 with the kit planner and project
module. Assembly contract 2.0.0 adds an optional project flag; contract 1.0.0 and
the existing four-module defaults remain supported. New domain grants are explicit.
The new schema is additive; original CRM behaviour, data and audit remain intact.

Profile 1.5.0 adds exclusive resource reservations through `scheduling` and
`scheduling-pg`, without requiring CRM in the domain packages. The
[reservation contract](docs/RESOURCE_RESERVATIONS_MODULE.md) defines capacity one,
explicit time offsets, database-clock holds, overlap protection, current grants,
atomic audit/receipts and local owner operation. Assembly 3.0.0 adds optional
`scheduling` while supporting old v1/v2 compositions. K07/K10 may select the narrow
`resource_reservations` scope; full operating kits remain blocked. This does not
accept hotel room nights, recurring calendars, pooled capacity, customer booking,
rental lifecycle, payment or external effects. Accommodation is the next domain
extension; existing scopes and data must survive that work.

Kit assembly must resolve every requirement against a verified installed profile.
Unknown, missing, disabled and domain-review requirements are blockers. Narrow
`crm_inquiry`, `project_tracking` or `resource_reservations` scopes must never imply acceptance of the
corresponding full operating kit. New scope does not change the approved pricebook.
An assembly is configuration, not authentication, execution approval or deployment.

Profile 1.6.0 adds [bounded accommodation](docs/ACCOMMODATION_MODULE.md): individual
rooms, calendar nights, frozen nightly rates, holds/confirmation, guest check-in/out
and room readiness. Capacity composes in one PostgreSQL transaction with the
shared scheduling kernel through additive migration 002; generic scheduling tools
cannot operate hotel-managed rooms. Assembly v4 adds optional accommodation and
preserves v1/v2/v3 contracts. Narrow K01 `accommodation_stays` acceptance does not
complete hotel operations, payment, OTA integration or public booking. Enabling
the module never grants human or machine permissions automatically.

Profile 1.7.0 adds [bounded commerce](docs/COMMERCE_MODULE.md): backend-priced
catalog items, immutable order quotes, expiring stock reservations, fulfillment
and partial restock/discard returns. Retail and wholesale share the same pure
module and PostgreSQL ledger without CRM or an LLM. Assembly v5 adds optional
commerce and preserves prior contracts/defaults. Narrow K03/K04 `commerce_orders`
is not full retail/wholesale acceptance. One stock pool, integer base units and
IDR estimates do not establish warehouse, tax, invoice or payment support.

Profile 1.8.0 adds [food-service ownership](docs/FOODSERVICE_MODULE.md): physical
tables, sealed recipe versions, frozen ticket lines and explicit kitchen/service
transitions. The shared commerce kernel owns every cross-domain stock/price write;
additive commerce migration 002 privately marks managed menu portions. Generic
commerce cannot sell or alter these portions. Queue holds ingredients; preparation
consumes once; waste cancellation does not restore raw ingredients. Assembly v6
requires commerce, preserves older contracts/defaults and supports narrow K02
`foodservice_tickets`. This does not establish per-item POS, payment or public booking.

Next build shared [support-case ownership](docs/SUPPORT_CASES_MODULE_PLAN.md) and
remaining KIT-07 requirements. The user requested
continued execution across accepted milestones until the programme is complete.
All 40 full operating families
remain in the work ledger. Specialist
adapters require an explicit authoritative backend and supported domain contract;
an unspecified integration cannot be labelled operational.

## 47. Packaging and incremental migration

**DIST-01:** prove consumption through packed artifacts in at least two host/configuration contexts outside workspace source resolution. Declare transitive dependencies and runtime support. No package may rely on parent paths, test aliases, private environment files or another project's build output.

**DIST-02:** publish separate public browser and server surfaces. Browser output must not import Node-only adapters, provider keys or private operational data. Preserve SDK consent/page-awareness contracts; business dashboard components can be reused without coupling host-page design to the assistant widget.

**DIST-03:** use internal controlled artifacts initially. A registry, CDN or public SDK bundle requires an explicit distribution/access/update policy. Inspect packaged contents, source/build hashes, licenses and dependency advisories. Provenance does not prove absence of malicious code and may disclose source/build metadata; review registry capabilities before enabling it for private IP.

**MIG-01:** map existing package exports and dependencies first. Keep working ports, assistant safety, pricing and browser reference behavior. Add compiled exports/artifacts and a compatibility path incrementally rather than moving all source or switching frameworks at once.

**MIG-02:** add manifest/resolution contracts alongside legacy config, then prove two compositions. Introduce trusted auth/tenant hosting, persistent business records and durable action recovery at their gates. Shared Voice/Human integration must preserve template consent/session/page protocol behavior and provider failure evidence.

**MIG-03:** route one scoped consumer to the new contract and compare behavior before removing legacy paths. Record unsupported differences. No existing passing suite or legitimate behavior is removed merely to satisfy a new module architecture.

---

## 48. Operational and verification requirements

### 48.1 Observable modules

**OPS-01:** correlate tenant, initiating principal/service actor, request/turn, config/release profile, approval, action attempt, resource and authoritative outcome. Telemetry is metadata-first; provider payloads, credentials, tokens and unnecessary PII must not be logged.

**OPS-02:** separate durable audit from sampled diagnostic traces. Record denials, approvals, changes and outcomes with restricted append/write access and tenant-scoped reads. This is not a guarantee of immutability against all privileged administrators.

**OPS-03:** expose host/module/provider health and readiness, action/webhook/worker failures, unknown outcomes, latency, usage and cost anomalies. Configuration completeness is not provider readiness. Alert through a defined operator channel with an owner/runbook; no autonomous customer communication is implied.

**OPS-04:** define support hours, severity/response policy, maintenance windows, upstream limitations and defect warranty versus paid maintenance for each release/contract. The §29 mature uptime target is aspirational until observed; no unlimited support or unmeasured SLA is created by this revision.

### 48.2 Test evidence

| Gate                 | Required proof                                                                                                                                  |
| -------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- |
| Existing behavior    | Preserve relevant structure/lint/type/unit/build/format/browser and template guardrail suites. Clearly record live/SDK tests not run.           |
| Artifact consumption | Install packed artifacts outside monorepo aliases, build browser and server consumers, inspect bundle/contents for server-only code or secrets. |
| Module/config        | Unknown IDs, cycles/ranges/conflicts, disabled dependency, missing port, requested-versus-ready state and legacy migration.                     |
| Domain reuse         | Same CRM slice in two contexts; dashboard and Archava share data/permission rules; supported options versus vertical extension are explicit.    |
| Identity/tenant      | Forged roles/tenant/approval, cross-tenant/customer reads/writes, retrieval/cache/file/export/worker paths and pooled connection reuse.         |
| Actions/recovery     | Race, restart, one-time consume, new-intent versus retry, stale approval/resource, unknown provider outcome and safe reconciliation.            |
| Persistent modules   | Existing-data migration, compatibility/recovery, record invariants, deletion/export/retention and restoration drill.                            |
| Presence/providers   | Shared core equivalence, session/context binding, interruption/fallback and controlled provider behavior/cost.                                  |
| Security/release     | Applicable dependency/license/advisory checks, injection reaching tools, resource authorization and packaged artifact review.                   |

**VAL-01:** report offline fixtures, mocked transports, local database evidence and live provider/deployment evidence separately. Passing a mocked test is not a claim of payment/email/CRM production success. New acceptance requires source and runnable evidence, not only this PRD.

No broad performance or financial guarantee is inferred from desk research. Establish realistic latency/capacity budgets from measured representative sessions and data volumes before production commitments.

---

## 49. Priorities, milestone acceptance and rollout

### 49.1 Four priority buckets

These categories describe the original reusable-platform rollout. Consult current
evidence for already accepted work; §50 selects the remaining tasks for the next
finite local release. One exact local profile is immediate work; broader profile
combinations and provisioning remain later scope.

| Priority                              | Deliverable                                                                                                                                                                  | Exit evidence                                                                                                                                     |
| ------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| **MUST BUILD NOW**                    | Existing public contract inventory, browser/server packaging, thin module registry/config readiness and independent consumption proof                                        | Same core artifact works in two consumers without source copies or workspace aliases; legacy regressions pass.                                    |
| **MUST BUILD NOW for design**         | Shared CRM/ERP primitives, resource ownership and first contacts/activity slice contract                                                                                     | Two business scenarios with reviewable schema/API/UI/tools and config/extension boundaries.                                                       |
| **MUST BUILD NOW before live writes** | Trusted principal/tenant hosting, atomic durable intent/attempt, confirmation/recovery and audit                                                                             | Race/restart/forged-confirmation/stale-resource/unknown-outcome cases pass before external effects.                                               |
| **SHOULD BUILD NEXT**                 | One persistent reusable CRM slice, shared Voice/Human contracts, one authoritative adapter                                                                                   | Two business compositions use the same module; dashboard/Archava access the same authorized data; controlled live evidence for claimed providers. |
| **BUILD LATER**                       | Demand-backed ERP capabilities, vertical kits, additional providers, broader supported profile combinations and provisioning                                                 | Repeated requirements, operator/support capacity, migration and domain evidence.                                                                  |
| **DO NOT BUILD YET**                  | Company website inside this repo; universal ERP, no-code schema/workflow platform, arbitrary plugins/version combinations, wholesale stack rewrite, microservices/Kubernetes | Revisit only for concrete requirements with evidence the simpler design cannot meet them.                                                         |

### 49.2 Milestones

**M0 — Specification alignment:** this research, PRD, supporting audit and operating rules agree on company/product/library boundaries. Current implementation status and unpriced scope are explicit. Completion of M0 is documentation evidence only.

**M1 — Archava reuse proof:** two independent consumer builds use identical core artifact versions with different knowledge, branding and allowed actions. Browser/server separation, config validation and cross-tenant denial hold; no client-specific branch enters core. Reference fixtures are acceptable here, labelled offline.

**M2 — Deterministic business module proof:** the first contacts/activity slice has persistent records, public service/API, dashboard components, tools, permissions, migration and audit. Two contexts use it with configuration/extensions and no fork. The dashboard works without an LLM. Existing customer/principal access tests remain enforced.

**M3 — Controlled operational release:** the selected integration/presence actually works in a controlled environment. Durable intent/recovery, auth, provider correlation, health/telemetry, backup restoration and applicable security/advisory gates pass. CRM readiness, presence readiness and each action provider are reported separately.

**M4 — Demand-backed library expansion:** add the next CRM/ERP module or kit only when its domain requirements and support owner are explicit. Publish supported profiles/changelogs and prove existing-data upgrade/recovery. Commercialize only the modules whose readiness and pricing scope are established.

M1–M4 do not silently waive any broader §34 gate or authorize unsupported high-risk actions. Assess v1 commercial readiness separately. The existing historical 30% delivery estimate applies to its earlier v1 scorecard, not to this v2 module-library scope.

### 49.3 Work order and open decisions

The original sequence started with inventory/packaging, thin metadata, two-consumer
proof and one persistent business slice. Those local milestones now have recorded
acceptance. Continue from the supported-profile and composition work in §50;
preserve their existing security/persistence proofs and generalize only from
demonstrated requirements.

Distribution registry, first production provider, backend framework/ORM and queue vendor remain implementation ADR decisions. Native module versus an existing backend must be assessed on total integration, license, migration and operation cost. Do not delay all contract work while waiting for those choices, and do not mark a planned option as supported.

This PRD upgrade authorizes specification work and establishes the target. It does not implement modules, publish packages, create the company website, change pricebook numbers or certify deployment readiness.

---

## 50. Current execution scope and definition of done

### 50.1 Finite release target

Deliver one supported local reusable-library release that a developer can install
and configure in two independent consumers without copying core/business logic.
The shared CRM works without an LLM; supported assistant tools use the same
authorized domain services. Include safe composition/lifecycle behavior, existing
native approvals/recovery, a typed shared-presence contract, scoped security
verification and a measured local capacity baseline.

The owner selected local-first/free operation. Current implementation evidence is
the 10 October audit at feature revision `80ea845`; later documentation changes
do not change that feature baseline. Core and CRM reuse plus the agreed owner
pilot are accepted foundations, not tasks to rebuild. New requirements below
remain unimplemented until their specific evidence passes.

This target excludes the separate ARCHZXY website, its automated sales/delivery
workflow, arbitrary ERP industries, public production hosting, paid services,
general private-data AI planning, actual payment/email/booking effects and live
audio/avatar provider acceptance. The shared-presence contract is a local/offline
integration milestone; it is not permission to advertise working realtime providers.

### 50.2 Defaults for implementation

| Decision                 | Current default                                                                                                               | Revisit when                                                                                            |
| ------------------------ | ----------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Architecture and folders | Preserve the current TypeScript modular monolith, Node hosts and explicit PostgreSQL adapters/migrations                      | A measured requirement demonstrates that an incremental adapter cannot meet it                          |
| Distribution             | Private compiled ESM/declaration artifacts; begin with one exact supported release profile                                    | Routine releases across real consumers justify a private registry                                       |
| Business scope           | Existing contacts/activity/follow-up/pipeline domain; hospitality and professional-service inquiry references                 | A concrete repeated requirement defines another module/extension and its support owner                  |
| Configuration            | Trusted host registry, closed validated schemas, exact resolved versions and explicit readiness                               | Another supported combination requires a small compatibility extension                                  |
| Presence integration     | Versioned server contract and synthetic transcript/session transport first; preserve template independence                    | A separately scoped live provider test has the required account, data policy and cost boundary          |
| AI operation             | Keep local commands and the existing synthetic-only Free-project Gemini pilot; no new provider calls required by this release | Approved private-data policy or another specifically scoped provider capability exists                  |
| Operation                | Loopback/POSIX local installation and isolated test databases                                                                 | An always-on/public deployment with hosting, identity, monitoring and recovery requirements is selected |

Unknown future vendor/framework choices do not block these defaults. Do not install
an ORM, queue, workflow engine or backend framework just to fill a roadmap cell.

### 50.3 Ordered work and acceptance

Implement EXEC-01 through EXEC-06 in order; retain applicable prior gates. These
are execution requirements, not declarations of implemented functionality.

**EXEC-01 — Supported release profile.** Record a versioned host-owned profile with
exact package versions/hashes, public/config contract versions, owned migration
versions, required adapters and operator checks. Validate it at packaging/install
and host startup. Refuse unsupported package/config/schema combinations with
explicit reasons. Artifact filenames, package versions and schema compatibility
must agree. Both independently installed consumers must use the supported profile;
their implementation hashes must match. Preserve existing browser/server separation.

**EXEC-02 — Safe composition and module lifecycle.** Expose validated declared
module selection for the supported profile and a developer-facing local assembly
command with a dry-run plan. Initial compositions cover the existing deterministic
CRM domain, its optional sales pipeline and optional assistant/native-action
surface; prerequisite and availability rules remain explicit. Generate only
configuration/entry references to shared installed implementation, not copied
domain code. Refuse unknown modules, incompatible selections, disabled required
dependencies and output-directory overwrite. Show unresolved legacy planning names
rather than silently aliasing them to installed modules.

Prove minimal and fuller supported compositions in independent consumers. Disabled
features expose no new UI/tools/invocations or mutation authority. Changes apply
through a validated build/restart; runtime hot-swapping is not required. Preserve
stored records, audit and original receipt identities across disable/re-enable.
Do not delete module data. Refuse a lifecycle change that would abandon unresolved
effects unless the supported recovery policy preserves authorized reconciliation.
Receipt recovery never becomes a new execution permission.

Expose the planner/validator through a typed library/API contract that the future
ARCHZXY website's Archava can consume. Treat model-produced selections as untrusted
configuration requiring the same deterministic validation. A valid assembly plan
does not grant deployment, secret access, pricing or business-action authority.
Building that company website or autonomous deployment is outside this release.

**EXEC-03 — Shared intelligence/presence boundary.** Define a versioned server
request/result/error contract for approved context, session scope, request identity
and bounded tool access. Bind public visitor and authenticated owner contexts
server-side; clients cannot supply roles, grants or reusable approval authority.
Do not let the standalone Python consumer import parent source or access owner
database credentials. Preserve page/session revision, guest-binding and expiry
contracts. Keep one common intelligence/policy implementation, with explicit
public versus owner tool projections.

Prove chat and a synthetic voice-transcript transport reach that common server
implementation from independently installed consumers. Test forged identity,
cross-tenant context, replay/expiry, malformed schema, private-tool access,
timeouts and truthful provider-unavailable/fallback results. CRM reads/actions
must retain their existing domain authorization and exact native consent. No live
audio or avatar call is required to pass this offline contract milestone.

**EXEC-04 — Scoped security and dependency verification.** Maintain a versioned
control/evidence matrix for the supported local profile: credential/session/CSRF
checks; current tenant/resource/service-human authorization; strict inputs;
browser/server separation; one-time action-bound consent; revision/replay/unknown
outcome handling; private secrets/artifacts; and safe backup/restore. Every
applicable control has a source path, a runnable positive/refusal check and an
explicit result. Preserve actual PostgreSQL race/restart tests rather than
replacing them with mocks. A count of tests is not a security coverage percentage.

Record exact dependency versions, lockfile/install-script policy, advisory
findings and distribution/license-review scope. Applicable unresolved high/critical
findings block support of that affected profile until corrected or mitigated with
evidence. Moderate findings require recorded impact/disposition; a scanner's
automatic downgrade/override is not compatibility proof. The template's existing
OpenTelemetry-chain findings and missing Python SDK integration evidence remain
explicit separate realtime release gates, not a clean security claim for the
whole repository. Do not claim a penetration test or certification from this matrix.

**EXEC-05 — Local capacity baseline.** Add a reproducible, bounded benchmark against
an isolated synthetic PostgreSQL/host installation. Use two tenants, at least 200
contacts per tenant and representative tasks/opportunities; measure authenticated
read workloads at concurrency 1, 5 and 20 for 15 seconds each after warm-up.
Keep login outside the timed read workload so password hashing is not mislabeled
as query capacity. Record workload/fixture size, runtime/database versions,
concurrency, sample counts, throughput, p50/p95 latency, failures/timeouts and
machine resources. Never load-test the owner's original data or call providers.

Run bounded mutation/replay races separately and verify tenant scope, single
committed effects, audit consistency and recovery under contention. Functional
correctness/refusal and a complete reproducible report are local acceptance;
there is no invented production users-per-second or SLA threshold. Report the
observed operating envelope and limits. §29's hosted targets remain separate.

**EXEC-06 — Upgrade, recovery and release handoff.** Prove a previously supported
installation can reach the new profile while retaining data, grants, audit and
UUID receipts. Use isolated upgrade fixtures; never overwrite the owner's database
as a test. Restore to a fresh target, invalidate restored identity, preserve native
and provider holds, and verify explicit recovery with a fresh approval epoch.
Document binary/config/schema compatibility and the recovery path; do not imply
that reverting a binary reverses a schema migration.

Complete a repeatable cold-install/build/start/doctor/backup/restore guide, a
profile/module inventory, changelog and sanitized evidence manifest. Verify both
business consumers and the applicable authenticated Chromium flow. Keep existing
root/platform, CRM, Act and template regression commands; their scopes remain
distinct. Update current architecture/build status from actual outcomes.

### 50.4 Work rules and external dependencies

Continue reversible local implementation, documentation and applicable validation
within these defaults without repeated routine confirmation. Preserve current
tests and working behavior. Record material design choices and explain scope
changes; no default silently expands the release or changes the pricebook.

Missing provider/hosting credentials, paid usage, public publishing, a new data
disclosure policy or unresolved product requirements need their concrete inputs
before dependent work. Keep independent authorized local tasks moving while an
input is missing; do not mark the blocked capability passed or substitute a mock
for live evidence. Existing authorization and cost/data restrictions remain in force.

The company website and provider/commercial milestones can consume the completed
release later. Their integration is future work with separate acceptance; the
present release need not wait for an entire enterprise/ERP roadmap.

### 50.5 Completion and progress reporting

Local implementation evidence for profile `1.3.0` is recorded in
[release acceptance](docs/LOCAL_LIBRARY_RELEASE.md) and the
[sanitized manifest](docs/audit/local-library/manifest.json). All six items have
passed their finite local acceptance; live/provider/production gates retain their
separate requirements. The criteria below continue to apply to changes.

The local reusable-library release is complete only when all six execution items
have passing applicable acceptance, both independent consumers use supported
artifacts without forks, existing regression behavior is preserved, and current
guides/evidence agree. Open live/provider/production gates must still be visible.

Report EXEC-01–EXEC-06 as not started, in progress, blocked on a named input, or
accepted, with evidence. Do not derive product percentages from code volume,
number of tests or completed checklist rows. Historical v1 commercial and scoped
owner estimates retain their denominators. Security controls, measured capacity,
module composition and provider readiness are reported separately.

When this finite release is accepted, state that outcome and its limitations.
Select the next demand-backed release explicitly; do not invent more industries,
infrastructure or features simply to keep implementation running indefinitely.
