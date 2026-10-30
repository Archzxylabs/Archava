# Archava Universal Business Module Library: Global Taxonomy & Extension Registry

<!-- documentation-status -->
> **Status: requirements/proposal reference; implementation acceptance is separate.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

> **Standard:** Aligned with APQC Process Classification Framework (PCF v7.3), SAP S/4HANA Reference Business Architecture, and Odoo Modular Ecosystem.  
> **Original designation:** Architecture Master Specification — proposed catalogue, not implementation inventory  
> **Target:** 100% Comprehensive Coverage of Global Business Models

---

## 1. The 10 Universal Enterprise Pillars

To fulfill the mandate of being **"The most comprehensive business module library ever built"**, Archava organizes all global economic activity into **10 Master Industry Pillars**. 

Every module in this registry follows Archava's core invariant:
* **Deterministic Pure Rust Core** (`rust/crates/domain` & `rust/crates/adapters`)
* **Multi-tenant PostgreSQL with Row-Level Security (RLS)**
* **Cryptographic Idempotency (SHA-256) per Command**
* **Atomic Single-Transaction Guarantees**

```
┌────────────────────────────────────────────────────────────────────────┐
│                   ARCHAVA 10 UNIVERSAL INDUSTRY PILLARS                │
├────────────────────┬────────────────────┬──────────────────────────────┤
│ 1. Commerce & POS  │ 2. Services & Ops  │ 3. Hospitality & Living      │
│ 4. Manufacturing   │ 5. Healthcare/EMR  │ 6. Fintech & Payroll/HR      │
│ 7. Education & LMS │ 8. Fleet & Auto    │ 9. Real Estate & Property    │
│ 10. Creative AI & Marketplace                                          │
└────────────────────┴────────────────────┴──────────────────────────────┘
```

---

## 2. Comprehensive Module Registry (Audit & Roadmap)

### Pillar 1: Commerce, Retail & Point of Sale (POS)
*Covering: Brick-and-mortar stores, supermarkets, e-commerce, consignment, wholesale distributors.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `commerce` | **COMPLETED** | Catalog pricing, immutable order quotes, stock holds, returns (`archava_commerce`). |
| `purchasing` | **COMPLETED** | Supplier price books, Purchase Orders (PO), physical stock receipt verification (`archava_purchasing`). |
| `foodservice` | **COMPLETED** | Table allocation, kitchen tickets, recipe stock consumption (`archava_foodservice`). |
| `pos_terminal` | **COMPLETED** | Offline-first cash register, split payments (cash/qris/card), shift sessions, cash drawer reconciliation. |
| `consignment` | **COMPLETED** | Third-party goods custody, supplier revenue share split, settlement life, unsold return chains. |

---

### Pillar 2: Services, Contracting & Field Operations
*Covering: Professional consultancies, field maintenance, cleaning services, equipment rental, salons & spas.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `projects` | **COMPLETED** | Milestones, task delivery chains, agreed scope baselines (`archava_projects`). |
| `scheduling` | **COMPLETED** | Timed resource reservation, appointment booking, moves, cancellations (`archava_scheduling`). |
| `field_services` | **COMPLETED** | Technician dispatch, mobile visits, parts used, customer sign-off (`archava_field_services`). |
| `laundry` | **COMPLETED** | Garment intake, quality inspection, wash/dry batch tracking, customer delivery (`archava_laundry`). |
| `assets` | **COMPLETED** | Serialized equipment register, custody handover, preventive maintenance logs (`archava_assets`). |
| `equipment_rental` | **COMPLETED** | Serialized machinery/camera leasing, security deposits, condition checklist inspections, overdue penalties. |
| `salon_spa` | **COMPLETED** | Stylist/therapist chair scheduling, multi-service treatment chains, consumable lotion/oil batch deductions. |

---

### Pillar 3: Hospitality, Living & Event Venues
*Covering: Hotels, villas, guest houses, co-living spaces, wedding halls, conference centers.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `accommodation` | **COMPLETED** | Room-night stays, housekeeping status, check-in/out locks (`archava_accommodation`). |
| `venue_booking` | **COMPLETED** | Banquet hall & convention space reservations, layouts (theater/banquet), AV & catering package bundles. |

---

### Pillar 4: Manufacturing, Assembly & Quality Control
*Covering: Apparel manufacturing, food processing, electronics assembly, furniture workshops.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `fulfillment` | **COMPLETED** | Warehouse dispatch custody chains, made-to-order light work orders (`archava_fulfillment`). |
| `mrp_discrete` | **COMPLETED** | Multi-level Bill of Materials (BOM), scrap allowances, component reservations, production orders. |
| `quality_control` | **COMPLETED** | Inspection checkpoints (incoming/in-process/final), AQL sampling limits, critical defect quarantine. |

---

### Pillar 5: Healthcare, Clinics & Wellness (EMR/EHR)
*Covering: Dental clinics, aesthetic & skincare clinics, general practitioner clinics, veterinary hospitals.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `clinical_records` | **COMPLETED** | EMR/EHR encounters, physiological vital bounds, ICD-10 diagnosis codes, clinical notes. |
| `patient_queue` | **COMPLETED** | Outpatient clinic queue, triage priority (Emergency P1 / Urgent P2), doctor paging, TV feed. |
| `pharmacy_dispense`| **COMPLETED** | Prescription validation, batch & expiry screening, allergy checks, licensed pharmacist signoff. |

---

### Pillar 6: Financial Services, Billing & Human Resources (HR)
*Covering: Corporate billing, recurring SaaS subscriptions, payroll, employee expense claims.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `billing` | **COMPLETED** | Immutable invoicing, correction credit notes, provider-signed payment settlement (`archava_billing`). |
| `subscriptions` | **COMPLETED** | Recurring subscription plans, entitlements, usage quotas, automatic renewals (`archava_subscriptions`). |
| `payroll` | **COMPLETED** | Indonesian statutory labor payroll (BPJS TK JHT/JP, BPJS Kes, PPh 21, overtime pay, take-home salary). |
| `expenses` | **COMPLETED** | Employee reimbursement claims, receipt SHA-256 hash, multi-level manager approvals, finance payouts. |

---

### Pillar 7: Education, Academies & Event Certification
*Covering: Private schools, bootcamps, corporate training centers, certification bodies.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `formal_education` | **COMPLETED** | Candidate admissions, student course enrollment, session grading, transcripts (`archava_formal_education`). |
| `attendance` | **COMPLETED** | Bounded attendance sessions, student/attendee rosters, verified check-ins (`archava_attendance`). |
| `lms_digital` | **COMPLETED** | Digital course syllabus progress, video lesson tracking, SHA-256 tamper-evident certificates. |

---

### Pillar 8: Automotive, Fleet & Workshop Logistics
*Covering: Vehicle repair workshops, car dealerships, delivery truck fleet operations.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `fleet_management` | **COMPLETED** | Logistics fleet register, monotonic odometer tracking, fuel consumption and km/L efficiency logs. |
| `auto_workshop` | **COMPLETED** | Vehicle repair work orders (SPK), diagnostic intake, spare parts deduction, mechanic labor, test-drive signoff. |

---

### Pillar 9: Property & Real Estate Management
*Covering: Commercial office leasing, apartment rentals, shophouse (ruko) tenancy.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `property_lease` | **COMPLETED** | Long-term tenancy contracts, security deposits, monthly service charge (IPL), deposit settlements. |

---

### Pillar 10: Creative Generative Studio & Multi-Vendor Marketplace
*Covering: E-commerce platforms with independent vendors, advertising agencies, brand creative teams.*

| Module ID | Status | Capabilities & Ledger |
| :--- | :---: | :--- |
| `studio_canvas` | **COMPLETED** | Node-based visual graph (Higgsfield API pipeline), Soul ID character consistency, ad asset synthesis. |
| `marketplace` | **COMPLETED** | Multi-vendor merchant accounts, split escrow payments, platform take-rate commission, seller payouts. |

---

## 3. The Live Interactive Preview Architecture

During Ava's initial consultation on `archzxy.com`:

```
┌─────────────────────────────────┬─────────────────────────────────┐
│     LEFT SCREEN: AVA VOICE      │    RIGHT SCREEN: LIVE SANDBOX   │
│                                 │                                 │
│  [ Realtime 3D Video Avatar ]   │  [ Rendered Store / Clinic ]    │
│                                 │   - Client's Brand & Colors     │
│  "Berdasarkan kebutuhan Anda,   │   - Sample Products & Pricing   │
│  ini preview langsung sistem    │   - Interactive Booking Calendar│
│  GlowClinic Anda!"              │   - Embedded Ava Mini Widget    │
│                                 │                                 │
│  [ Mic Active ] [ Chat Input ]  │  [ Test Click ] [ Add to Cart ] │
└─────────────────────────────────┴─────────────────────────────────┘
```

### Technical Workflow:
1. **Dynamic Intent Synthesis:** As the client describes their business, Ava's agent emits a lightweight `PreviewProjection` JSON via WebRTC data channel.
2. **Sub-second Rendering:** The client browser mounts the `PreviewProjection` without provisioning backend databases.
3. **Interactive Testing:** The visitor can click items, add sample products to cart, and test the Ava mini-widget live on their simulated storefront.
4. **Seamless Conversion:** Clicking "Deploy This System" moves the generated preview state directly into the SOW & Privy signature step!
