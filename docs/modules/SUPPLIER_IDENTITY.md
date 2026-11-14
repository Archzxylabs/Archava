# Supplier identity from CRM

Purchasing can register a supplier from an explicitly selected CRM contact and
refresh that supplier's profile after a newer CRM revision is reviewed. This is
a contact reference for a supplier role, not a universal party, legal entity,
vendor bank account, payable account or automatic duplicate merge.

## Commands and authority

`POST /erp/purchasing/commands` accepts the usual tenant/request envelope:

- `supplier.create-from-contact`: `code`, `contactId`, `expectedContactRevision`.
- `supplier.refresh-contact`: supplier `id`, `expectedRevision`,
  `expectedContactRevision`, and a nonempty `reason`.

Both use current server-resolved human membership, Purchasing supplier read/write,
Commerce catalog read, global Purchasing/Commerce scope, enabled CRM contacts and
current CRM `contacts.read`. CRM write permission is not required. Assigned CRM
access can select only contacts owned by that actor. Tenant, owner, role and name
claims cannot be supplied within these commands. A refresh uses the stored source
UUID and cannot select a different contact.

The new-command branch checks the reviewed CRM revision. Refresh also checks the
supplier revision and requires an advancing CRM revision. Stale review, missing
source, revoked authority or invalid payload creates no supplier, command receipt
or audit entry. Authorized replay returns the original receipt after later profile
changes; it still checks current CRM access. Command-status recovery follows
Purchasing's existing actor/grant policy and can recover a committed receipt
without reading a current CRM profile.

Both operations are in the canonical action catalogue and available to reviewed
mission execution. Approval remains mandatory for a mission write; registration
in the catalogue does not provide permission or imply approval.

## Persistence and history

[Migration 003](../../migrations/purchasing-pg/003_supplier_contact_identity.sql)
adds `supplier_contact_id` and `supplier_contact_revision` to suppliers and PO
headers, with tenant-bound CRM foreign keys and pair constraints. Source UUIDs
cannot change. New supplier names/revisions must match current CRM. An explicit
refresh updates the supplier name and stored source revision, increments supplier
revision and records the reason atomically.

A new PO copies the selected supplier's current name, code and contact snapshot.
Its identity fields remain immutable through confirmation and goods receipt.
Refreshing a supplier does not rewrite any existing PO, including drafts.
Receipts retain their existing immutable order reference, so the supplier/contact
history remains traceable through the PO. Accepted stock and acquisition cost
remain in the same transaction as receipt/audit/command recording.

Shared CRM snapshot reads use `FOR SHARE`, which blocks profile changes while
remaining compatible with foreign-key key-share locks. Refresh locks CRM before
the supplier. PO creation holds the supplier snapshot stable. This permits
concurrent refresh and PO creation to serialize to a coherent old or new snapshot.

Manual suppliers and all rows preceding the new migration retain null contact
references. Names never infer a link. One contact may serve multiple explicitly
coded supplier records; supplier code remains unique per tenant. The same contact
UUID can also identify a customer on a sales order independently of the supplier
ID; this does not create a universal party record.
Supplier names follow the database's 160 Unicode-character bound.

## Native interaction and evidence scope

`/app/erp?module=purchasing` provides optional CRM registration with name search,
100-match pagination, email/organization context, retained explicit selection and
selected-record refresh. Enter in search does not submit the outer supplier form.
The profile panel loads only the supplier's existing source, previews current CRM
values and requires a reason before applying. A change after loading is refused
and can be recovered by loading and reviewing again. PO detail links to CRM while
showing the historical snapshot. Supplier pickers include code and name.

The workspace lists up to 100 matching suppliers; its search locates older
records. This bounded list is separate from the paginated CRM contact selector.

Verification sources:
[router/database tests](../../crates/host/tests/workspace_flows/supplier_identity.rs),
[domain contracts](../../crates/domain/src/erp/tests.rs), and
[browser journey plus independent SQL](../../tools/e2e/supplier-identity.mjs).
Executed acceptance belongs to the [PRD tracker](../product/PRD_EXECUTION_WORK.md).
These checks do not accept universal-party management, AP/GL, public deployment,
production model providers or a customer pilot.
