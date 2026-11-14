# Native business operations

The mounted operator product covers 19 business domains. The shared
[form catalogue](../../config/operator-actions.v1.json) describes 193 canonical
operations, of which two Billing provider events require the verified payment
integration. The other 191 operations have native review forms. A form's presence
is not evidence that every lifecycle combination has been exercised in a browser.

The existing CRM, Commerce, Purchasing, Billing, Accounting and Support screens
remain the primary workspaces. The other 13 domains use the native
[operator view](../../crates/slice/src/operations/operator.rs) at
`/app/erp?module=<id>`. Purchasing and Support also expose the additional actions
through this view's form component. Fields include nested records, editable line
arrays, reference lookups, finite choices, local dates/times and exact integer
money values. They do not require users to enter JSON.

The catalogue is interpreted by
[archava-domain::operator](../../crates/domain/src/operator.rs), shared by the UI
and the host's [finite business dispatcher](../../crates/host/src/business.rs).
It describes input, not authority. The dispatcher calls the existing authenticated
domain handlers. Membership, scope, role, current grants, optimistic revisions,
Owner module availability and domain rules are checked at persistence time.

## Reads and scope

`GET /erp/<id>/workspace?tenant=<tenant>` supplies bounded collection reads for
the 13 additional workspaces. Optional `collection`, `id`, `cursor`, `limit` and
`search` select a collection, detail or page. Limits are 1–100. The seven new
projections use stable keyset cursors; the six existing domain readers retain
their own access policy. Search filters a bounded page, which is disclosed in the
UI. Continue to the next page to inspect further matches.

[Projection adapters](../../crates/adapters/src/erp/projections.rs) use static SQL
identifiers, locked memberships and tenant transactions. Child collections join
the parent ownership policy. Typed readers preserve Education self/guardian and
published-content rules. A user does not gain a grant by enabling a module.

## Review and recovery

Every native submission retains its original request key in a marker scoped to
tenant and principal. Markers contain recovery references rather than business
payloads. A successful response must identify the original request and a resource.
Accommodation's legacy bare receipt is verified against the status endpoint under
the original key before being accepted. A missing, malformed or mismatched receipt
leaves the action unresolved. Reload, Check status and Retry original preserve the
same key; a new intent cannot replace an unresolved submission.

Owner Settings exposes the full installed module catalogue, dependency checks,
business-language discovery and a chat preview. Apply submits the exact reviewed
request/hash. Enabled state is enforced in the domain transactions under a shared
configuration fence; disabling a module affects API reads and writes as well as
its UI. Explicit owner bootstrap grants are separate from normal configuration.

The feature audit is preserved in
[the dated baseline](../product/FEATURE_COMPLETENESS_2026-11-04.md). Acceptance
results and limits are tracked in [the completion ledger](../product/FEATURE_COMPLETION_WORK.md).

Purchasing includes CRM-backed supplier registration and explicit reviewed profile
refresh. The [supplier identity contract](SUPPLIER_IDENTITY.md) describes current
authority, immutable source links and historical PO/receipt snapshots.

Organizations includes reviewed CRM relationships and explicit legacy verification;
[organization identity](ORGANIZATION_IDENTITY.md) defines the new source checks,
compatibility requirements and historical-data boundary.

[Business parties](BUSINESS_PARTIES.md) adds four native registry operations under
Organizations, with separate party grants and reviewed source selection.

[Party-linked transactions](PARTY_TRANSACTIONS.md) add reviewed customer and supplier
selection, explicit supplier profile refresh and historical document identity.

[Managed companies](MANAGED_COMPANIES.md) adds registration, reviewed source refresh
and activity controls under the Organizations permission boundary.

[Company trade](COMPANY_TRADE.md) adds explicit company selection and historical identity to Catalog, Sales, Purchasing and Invoices.

[Supplier goods documents](SUPPLIER_DOCUMENTS.md) add native capture, held findings, explicit correction, exact match and void for a selected company PO.
