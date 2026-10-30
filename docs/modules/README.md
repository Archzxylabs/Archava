# Business module contracts

These pages describe current Rust boundaries and distinguish implemented operator screens from API-only modules. Tier 1 operations use the native SSR/WASM workspace; acceptance still depends on the [canonical release tracker](../product/LAUNCH_READINESS.md) and dated execution evidence.

The [capability matrix](../product/BUSINESS_CAPABILITY_MATRIX.md) remains planning input. Earlier module contracts are [preserved with hashes](../archive/history/module-contracts-before-native-workspace-2026-10-29/manifest.json) for migration provenance.

| Document | Purpose |
| --- | --- |
| [ACCOMMODATION_MODULE.md](ACCOMMODATION_MODULE.md) | Accommodation module |
| [ASSETS_MODULE.md](ASSETS_MODULE.md) | Asset register module |
| [ATTENDANCE_MODULE.md](ATTENDANCE_MODULE.md) | Attendance, session rosters and event check-in module |
| [BILLING_MODULE.md](BILLING_MODULE.md) | Invoicing and payment settlement module |
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
