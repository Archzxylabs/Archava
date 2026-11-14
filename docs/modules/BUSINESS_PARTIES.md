# Business-party master registry

A party is one stable business identity registered from an explicitly reviewed
CRM contact or Organizations record. Customer and supplier are independent roles
on that ID. Identical names do not merge contacts, organizations or parties.
A contact source does not assert that it is a verified natural/legal person.

This registry is the foundation for shared business master data. [Party-linked transactions](PARTY_TRANSACTIONS.md) accept reviewed party IDs
and enforce roles for new business. Legacy contact/supplier links remain explicit.
Registration does not backfill transactions, create supplier accounts, or establish a company/book,
credit account, tax registration or accounting ledger.

## Commands and authority

The existing Organizations command endpoint/envelope supports:

- `party.create_from_contact`: `contactId`, `expectedSourceRevision`, `customer`,
  `supplier`.
- `party.create_from_organization`: `organizationId`, `expectedSourceRevision`,
  `customer`, `supplier`.
- `party.set_roles`: party `id`, `expectedRevision`, both role flags and `reason`.
- `party.refresh_source`: party `id`, `expectedRevision`,
  `expectedSourceRevision` and `reason`.

A party registered from an organization also keeps that organization's tax id as
part of the reviewed snapshot (`taxId`, Organizations 005): stored as entered, changed
only by `party.refresh_source`, and null for a party registered from a contact. It is
not a tax registration and nothing verifies it. See the
[Indonesia tax pack](TAX_INDONESIA_PACK.md#tax-registration-identifiers).

Creation requires at least one role. Later clearing both roles is allowed; it
keeps the identity/history and does not disable existing legacy transactions.
Reactivation retains the same ID. No command accepts a caller-supplied name,
owner, permissions, alternate source or legal-entity claim.

Current human Organizations membership with **parties.read + parties.write** and
its all/assigned ownership scope authorizes writes and receipt recovery. These
are new explicit grants; existing organization-read or audit-read alone does not
publish party master data. Party history requires both parties.read and audit.read.

Creating/refreshing a profile also requires current source access: CRM contacts
read for a contact, or organizations read with the source's ownership scope for
an organization. Organization sources must be active for new execution. Profile
refresh never retargets the source. Role changes use the reviewed business master
and do not require fresh access to the raw source. Party read explicitly grants
access to its captured business data independently of current raw-source access;
opening the original source enforces that source's own permission.

Original create/refresh replay rechecks current raw-source access before reading
the committed receipt, while new-execution source/revision checks occur after
claim/replay resolution. Later renaming or archiving does not rewrite an original
receipt. Status recovery checks current party read/write and party scope without
requiring raw-source visibility. Requests share the existing Organizations
command-key namespace, so reusing a key across old/new operations conflicts.
Unknown keys return not_committed under authorized recovery access.

## Persistence and version history

[Organizations migration 003](../../migrations/organizations-pg/003_business_parties.sql)
adds parties and append-only party_audit under forced tenant RLS. Every party has
exactly one immutable tenant-bound contact or organization source, source revision,
authoritative name snapshot, customer/supplier flags, owner and revision. Unique
source indexes prevent a second party for the same typed source. Contact and
organization sources remain distinct even if their UUIDs or names coincide.

Creation/refresh locks the reviewed source before updating the party. Roles and
refresh use party revision CAS; a refresh must advance source revision. Source
rename, duplicate creation and concurrent role writes either serialize coherently
or refuse. There is no automatic name matching, merge or profile propagation.

Every version records its full party snapshot, actor, operation, reason and command
reference. A deferred database constraint requires a matching version count,
current snapshot and bound receipt at commit. Source/name forgery, source
repointing, history mutation, deletion and state changes without matching
audit/receipt fail. Claim, master change, audit and receipt share one transaction.

Applied SQL bytes/IDs remain unchanged. Existing profiles do not gain party rows
or application grants automatically. Native installation and disposable fixtures
register the new migration/runtime ACL; the prior-profile migration test continues
through the new profile while preserving unverified affiliation history.

## Native workspace and acceptance scope

Organizations includes reviewed registration from paginated contact/organization
pickers and a profile panel for role changes and source refresh. Sources retain
the exact selected UUID/revision; Enter in search never submits registration.
A stale source requires explicit refresh and review. Profile late responses cannot
replace another selection or update a disposed view. Management uses the loaded
Business parties collection page; collection search and Next page locate older rows.
History is available through the Party history collection with its own grants.

The same four operations are registered for reviewed mission execution. Approval
and current permissions remain mandatory. Local tests include an actual approved
mission; this does not establish new production-model planning or user benefit.

Sources: [domain grammar](../../crates/domain/src/erp/parties.rs),
[transactional adapter](../../crates/adapters/src/erp/parties.rs),
[host dispatch](../../crates/host/src/erp/parties.rs),
[native UI](../../crates/slice/src/operations/parties.rs),
[router/database tests](../../crates/host/tests/workspace_flows/parties.rs) and
[browser/SQL journey](../../tools/e2e/parties.mjs).
Executed acceptance belongs to the [PRD tracker](../product/PRD_EXECUTION_WORK.md).

[Managed-company registration](MANAGED_COMPANIES.md) is a separate explicit declaration
for companies operated by the workspace. Party registration does not create it.
