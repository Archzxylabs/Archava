# Asset register module

<!-- documentation-status -->
> **Status: module contract reference; historical acceptance context.** Reviewed for documentation ownership on 29 October 2026. Use the [current architecture](../architecture/README.md) and [developer guide](../DEVELOPER_GUIDE.md) for the active workspace. Original versions, commands, and acceptance statements below describe their recorded context; this page is not fresh release evidence.

Current Rust source: [domain](../../crates/domain/src/erp/assets.rs), [persistence](../../crates/adapters/src/erp/assets.rs), [HTTP](../../crates/host/src/erp/assets.rs), [integration tests](../../crates/host/tests/erp_assets.rs).

This contract covers the serialized asset inventory, custody tracking, condition lifecycle, and maintenance history implementation.
Release 1.12.0 accepted with fresh isolated domain, host, browser, upgrade and owner evidence.
It provides native equipment tracking and customer device servicing without changing existing CRM or scheduling authority.
Full rental fleet, vehicle leasing, workshop and field service operating kits retain their other requirements in [the programme ledger](../archive/history/progress/STARTER_KIT_IMPLEMENTATION.md).

## Ownership and composition

`@archava/assets` owns strict contracts, human service commands, private read tools, HTTP API and reusable DOM dashboard with hardened state machine receipt recovery. It depends only on pinned Zod and needs no CRM, AI or external provider.
`@archava/assets-pg` owns six forced-RLS business tables: `memberships`, `assets`, `custody_events`, `maintenance_records`, `commands`, and `audit`. Its checked migration ledger `001_assets_custody_maintenance.sql` is checksummed (`b77c70bddd01512defad588a04497380aafd923dd52c833ab30fc578c9c6cfec`) and becomes ledger ke-16 (tata kelola maksimum 16 ledgers).

Two independent business consumers demonstrate generalization:

1. **Consumer 1: Equipment / Vehicle Rental (`K10: asset-rental`)**:
   Tracks business-owned assets (`ownership: 'business_owned'`), serialized units, dispatch to client site, wear-and-tear returns, and periodic preventative overhauls.
2. **Consumer 2: Repair & Maintenance Workshops (`K18: repair-workshops`)**:
   Tracks customer-owned assets (`ownership: 'customer_owned'`), links to customer contact IDs, logs intake inspection, corrective component repair with parts replacement, and customer return.

## Lifecycle and Invariants

```text
business_owned: available → in_custody → available → in_maintenance → retired
customer_owned: intake → in_repair → repaired → returned_to_customer
```

- **Serialized Identity**: Each asset has an immutable tenant code, human title, serialized hardware/equipment number, category, and ownership designation (`business_owned` | `customer_owned`). Customer-owned units strictly require customer contact ID association.
- **Custody Events**: Every transfer, dispatch, check-in, intake, or customer return logs an immutable event with before/after condition rating (`excellent`, `good`, `fair`, `damaged`, `unserviceable`), custodian IDs, location, and human notes.
- **Maintenance Records**: Corrective repairs and preventive services log explicit descriptions, parts replaced, currency/cost, and post-service condition rating.
- **Optimistic Revision Locking**: All asset modifications and lifecycle transitions require matching `expectedRevision`; stale revisions fail atomically with `conflict`.
- **Idempotent Replay**: Command UUIDs are deduplicated via sha256 payload fingerprinting; replays return identical receipts without duplicate database rows.
- **Immutable Audit Triggers**: Triggers on `audit` and `commands` strictly prevent row tampering, updates, or deletions even from database superusers.

## Security & Verification Matrix

- **18 Synthetic Verification Checks (`pnpm assets:verify`)**: Multi-tenant isolation, superuser rejection, inactive account blocking, human-only mutation validation, custody handover, parts logging, optimistic locking, idempotent replay, and audit immutability.
- **5 Headless Browser E2E Checks (`pnpm assets:verify-host`)**: Full Chromium CDP test across five distinct network/transport failure modes (truncated JSON `'{'`, stream failure `TypeError`, invalid receipt schema, UUID mismatch, and grant revocation 403), proving zero duplicate asset records and state reconciliation.
