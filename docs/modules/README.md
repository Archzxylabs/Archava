# Business module contracts

These pages describe current Rust boundaries and distinguish implemented operator screens from API-only modules. Tier 1 operations use the native SSR/WASM workspace; acceptance still depends on the [canonical release tracker](../product/LAUNCH_READINESS.md) and dated execution evidence.

The [capability matrix](../product/BUSINESS_CAPABILITY_MATRIX.md) remains planning input. Earlier module contracts are [preserved with hashes](../archive/history/module-contracts-before-native-workspace-2026-10-29/manifest.json) for migration provenance.

| Document | Purpose |
| --- | --- |
| [ACCOMMODATION_MODULE.md](ACCOMMODATION_MODULE.md) | Accommodation module |
| [ACCOUNTING.md](ACCOUNTING.md) | Explicit company books, immutable reviewed journals, periods and exact financial reports; operational source bridges remain in progress |
| [MONEY_UNITS.md](MONEY_UNITS.md) | Exact financial units, retained operating/display units and explicit source conversion requirements |
| [INVOICE_POSTING.md](INVOICE_POSTING.md) | Reviewed future IDR invoice capture, source journals, provider clearing and close controls; acceptance in progress |
| [ASSETS_MODULE.md](ASSETS_MODULE.md) | Asset register module |
| [ATTENDANCE_MODULE.md](ATTENDANCE_MODULE.md) | Attendance, session rosters and event check-in module |
| [BILLING_MODULE.md](BILLING_MODULE.md) | Invoicing and payment settlement module |
| [TAX_INDONESIA_PACK.md](TAX_INDONESIA_PACK.md) | Unverified tax code templates, tax registration fields, tax ids and an external tax invoice reference on invoices, and the documents listing with a CSV export |
| [COMMERCE_MODULE.md](COMMERCE_MODULE.md) | Commerce module |
| [CRM_MODULE.md](CRM_MODULE.md) | Contacts, activities and follow-up — internal CRM slice |
| [FIELD_SERVICES_MODULE.md](FIELD_SERVICES_MODULE.md) | Field services: dispatch, visits, and work evidence |
| [FOODSERVICE_MODULE.md](FOODSERVICE_MODULE.md) | Restaurant tables, recipes and kitchen tickets |
| [FORMAL_EDUCATION_MODULE.md](FORMAL_EDUCATION_MODULE.md) | Formal education: enrollment, attendance, and results |
| [FULFILLMENT_MODULE.md](FULFILLMENT_MODULE.md) | Fulfillment, dispatch and light production module |
| [LAUNDRY_MODULE.md](LAUNDRY_MODULE.md) | Laundry and dry cleaning processing module |
| [PROJECT_DELIVERY_MODULE.md](PROJECT_DELIVERY_MODULE.md) | Reusable project delivery module |
| [PURCHASING_MODULE.md](PURCHASING_MODULE.md) | Purchasing module |
| [RESOURCE_RESERVATIONS_MODULE.md](RESOURCE_RESERVATIONS_MODULE.md) | Resource reservations module |
| [SUBSCRIPTIONS_MODULE.md](SUBSCRIPTIONS_MODULE.md) | Subscriptions, entitlements and usage tracking module |
| [SUPPORT_CASES_MODULE.md](SUPPORT_CASES_MODULE.md) | Internal support cases and investigation history |

For a module change, review its domain contract, adapter transaction, host authorization and consumer flow together. Current commands belong in the [developer guide](../DEVELOPER_GUIDE.md); do not copy historical package commands into a new release procedure.

[Customer identity](CUSTOMER_IDENTITY.md) describes the shared CRM → sales order →
invoice relationship, scoped selection and historical snapshots. It is a
cross-module contract rather than an additional business module.

[Supplier identity](SUPPLIER_IDENTITY.md) covers CRM-linked registration, reviewed
profile refresh and immutable purchase history alongside
[customer identity](CUSTOMER_IDENTITY.md).

[Organization identity](ORGANIZATION_IDENTITY.md) covers reviewed CRM affiliations
and explicit verification of legacy contact references.

[Business parties](BUSINESS_PARTIES.md) defines reviewed master identities,
independent customer/supplier roles and immutable version history.

[Party-linked transactions](PARTY_TRANSACTIONS.md) connect the published master to sales,
purchasing and source-order invoicing while retaining historical identity.

[Managed companies](MANAGED_COMPANIES.md) records explicitly reviewed company
identities and version history; company/book and transaction mapping remain separate.

[Company-owned trade](COMPANY_TRADE.md) defines explicit company product ownership, stock/PO/sales/invoice scope and current human access.

[Unposted supplier goods documents](SUPPLIER_DOCUMENTS.md) cover explicit vendor claims and exact accepted-receipt allocation without posting accounting.

[Owner CRM presentation](OWNER_PRESENTATION.md) documents scoped configured labels,
initial party provisioning and unchanged canonical lifecycle/history boundaries.

[Purchase posting](PURCHASE_POSTING.md) defines the current future-confirmation IDR receipt/GRNI, goods claim/AP and void-source integration under validation. Its official supplier settlement, bank reconciliation and complete close remain separate work.

- [Original stock cost posting](STOCK_COST_POSTING.md) — captured acquisition/fulfillment/return costs with separate financial review.

[AI data sharing](AI_DATA_SHARING.md) defines reviewed provider/category/company
selection, local fallback, projected request data and the native compatibility
boundary. Native/browser acceptance remains source-specific.

[Manual bank evidence and matching](MANUAL_BANK_RECONCILIATION.md) defines Accounting006–007 statement imports, explicit bank grants, posted-cash allocations and immutable corrections. Complete bank/financial close remains pending.

[Supplier bank payments](SUPPLIER_PAYMENTS.md) defines Accounting009 source-backed payment/return drafts, live reservations, separate posting and reviewed bank matches. Native acceptance and full AP/AR close remain under validation.

[Gateway bank clearing](GATEWAY_BANK_CLEARING.md) defines Accounting010 verified collection/refund sources, explicit fees and actual bank evidence. Combined native acceptance remains under validation.

- [Original-document due terms and dated AR/AP reports](DOCUMENT_DUE_TERMS.md) — explicit reviewed dates, private source/history reads, whole-book exact aging and independent posted-source/GL controls; merged-release acceptance is tracked separately.
