# Customer identity across CRM, orders and invoices

An operator can explicitly choose an existing CRM contact when creating a sales
order. Its tenant-bound UUID and reviewed revision are retained through fulfillment
and invoice conversion. Historical names stay frozen when the CRM profile changes.
This is a contact reference for the supported journey, not a universal business
party, legal entity, customer credit account, vendor role or duplicate merge.

## Contracts and authority

`commerce.order.create_from_contact` accepts `contactId`,
`expectedContactRevision` and the normal bounded product/quantity line sheet.
It does not accept a caller-supplied name, role or owner. Existing Commerce
catalog/order grants and current CRM `contacts.read` are required. CRM write
access is not required. Session, membership kind, delegated authority where
applicable, tenant and assigned-contact scope are resolved server-side.

The command locks CRM membership/contact before claiming or replaying the request.
New execution checks the reviewed revision and snapshots the authoritative name.
An authorized original replay keeps its existing order after a rename; current
CRM permission/scope is still checked. Historical order receipt reads use current
order-read authority independently of profile visibility.

Order/invoice projections expose `customerContactId` and
`customerContactRevision`. Invoice conversion copies the historical order values
without requiring new access to the current CRM profile. Opening the CRM link
separately enforces current CRM read authority. `order.create` with a free-text
name stays supported and unlinked; no matching names are automatically associated.

## Native UI

Sales provides **Use an existing CRM customer**. The authorized picker searches
names, follows keyset cursors through pages of up to 100 records, and shows email
and organization to distinguish same-name contacts. Selection retains the reviewed
identity/revision across searches and pages; stale execution requires fresh review.
Order/invoice details link to the CRM record and label names as historical snapshots.
**Refresh selected customer** reads the exact authorized CRM record again. It
updates the visible reviewed details/revision explicitly, allowing recovery after
a stale submission without guessing a new identity. Search results preserve the
selected snapshot until the user refreshes or changes the selection.
Enter in the customer-search field runs the search and prevents submission of
the enclosing order form. The selected record is retained when the results page changes.

## Persistence and migration dependency

- [Commerce 007](../../migrations/commerce-pg/007_customer_contact_identity.sql)
  adds nullable references, tenant-bound FK, index, insert-time snapshot checks
  and immutable linkage.
- [Billing 006](../../migrations/billing-pg/006_customer_contact_identity.sql)
  checks exact propagation from the source order, including null references.
- Existing records keep null provenance. There is no name matching, merge or data
  rewrite; referenced CRM records cannot be deleted through the FK relationships.
- Commerce now depends on CRM schema. Native manifest and fixture migrate
  **Identity → CRM → Commerce**, then dependent ledgers. Historical IDs, SQL bytes
  and checksums are unchanged; only ledger apply order changes for this dependency.
- Legacy kernel inserts remain usable in owned pre-007 upgrade fixtures. Current
  production startup still requires the complete embedded profile.

Writes remain inside non-owner, forced-RLS transactions. Customer names support
160 Unicode characters rather than a 160-byte limit.

## Source and acceptance

| Boundary | Source |
| --- | --- |
| Contract | [Commerce domain](../../crates/domain/src/erp/commerce.rs) |
| Current CRM authority | [Customer adapter](../../crates/adapters/src/erp/customer.rs) |
| Order persistence | [Commerce adapter](../../crates/adapters/src/erp/commerce.rs) |
| Invoice propagation | [Invoice adapter](../../crates/adapters/src/erp/billing_invoice.rs) |
| Picker | [Customer UI](../../crates/slice/src/operations/customer.rs) |
| Router/database tests | [Customer identity suite](../../crates/host/tests/workspace_flows/customer_identity.rs) |
| Browser journey | [Customer browser gate](../../tools/e2e/customer-identity.mjs) |

Tests cover same-name identity, rename/replay, stale input, read-only CRM grant,
scope/revocation, cross-tenant source, database forgery, Unicode, manual records,
rename concurrency and approved mission execution. Actual results belong to the
[execution tracker](../product/PRD_EXECUTION_WORK.md); test source and this
specification do not establish full product acceptance.
