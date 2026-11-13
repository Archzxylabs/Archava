# Verified organization/contact relationships

Organizations and CRM contacts are separate identities. An affiliation explicitly
links one contact to an organization with a role and optional primary-contact
status. Matching organization/contact names do not establish a relationship.
This increment strengthens existing affiliations; it does not create a universal
party, customer credit account, legal entity verification or accounting book.

## Commands and current authority

The Organizations endpoint keeps its `{tenant, mutation:{requestId,type,data}}`
envelope. New native operations are:

- `create_reviewed_affiliation`: `organizationId`,
  `expectedOrganizationRevision`, `contactId`, `expectedContactRevision`, `role`,
  optional `title`, `department` and `isPrimary`.
- `verify_affiliation_contact`: existing affiliation `id`, `organizationId`,
  `expectedOrganizationRevision`, `expectedContactRevision` and `reason`.

Both require current human membership, Organizations/affiliations read,
affiliations write, enabled CRM contacts and CRM `contacts.read`. Each ledger's
assigned scope applies independently. Identity, tenant, owner and permissions
come from current session/database authority. New execution checks both reviewed
revisions. Replay checks current source access without reapplying the old CAS,
so a later rename does not duplicate or rewrite the relationship.

The existing `create_affiliation` contract now requires a real, currently visible
CRM contact too. It captures the current source at execution; clients wanting
reviewed revisions should use the new operation. Activation of an existing
relationship also checks current CRM read access. Deactivation/removal and
organization archive can preserve/release historical references without requiring
new CRM access. Receipt-status recovery retains the existing Organizations
actor/grant policy independently of current CRM profile visibility.

Strict parsing rejects undeclared keys even when optional fields are omitted;
callers cannot supply owner/permission claims or frozen contact-name fields.
The new operations are registered for native forms and approved missions. A
mission write still requires explicit step approval and current authority.

## Persistence and legacy upgrade

[Migration 002](../../migrations/organizations-pg/002_verified_contact_affiliations.sql)
adds paired `contact_revision`/`contact_name` snapshot fields and a tenant-bound
CRM foreign key. New affiliation references must exist in CRM. The snapshot is
captured from that exact UUID and cannot be rewritten or repointed after it is
verified. Contact renaming preserves the captured name/revision. Organization
master fields retain their existing independent revisioned lifecycle.

The FK is deliberately `NOT VALID`: pre-002 affiliations could hold opaque or
missing contact UUIDs. Migration does not reject their history or mark them valid.
Existing rows keep null snapshot fields and remain explicitly unverified. New
references are enforced immediately. The unvalidated constraint is not evidence
that historical data has been cleaned or reconciled.

`verify_affiliation_contact` loads the immutable stored UUID, requires current
source access and both reviewed revisions, and fills an unverified snapshot once.
It does not retarget the contact. Missing source requires removal of the old
relationship and creation of a reviewed one. Unverified inactive relationships
cannot be reactivated or promoted to primary until verified. Old active rows keep
their historical state and unverified marker; migration does not silently decide
their business status. Archive can deactivate them.

Source contact locks precede organization locks. A source rename or concurrent
organization write either leaves a coherent reviewed snapshot or refuses the
new command. Claim, affiliation change, aggregate revision, audit and receipt
share one runtime transaction. Primary-contact uniqueness is still enforced.

Existing create/activation clients need current CRM read grants after upgrade;
missing access is a refusal, not an empty or fabricated contact. Runtime startup
still requires the complete embedded migration profile. Old SQL bytes/IDs and
historical acceptance reports are unchanged.

## Native interaction

`/app/erp?module=organizations` adds a reviewed relationship form. Organization and
CRM pickers each search authorized records, page up to 100 matches, retain the
selected snapshot and explicitly refresh that record. Legal name/tax reference
and email distinguish same-name records. Enter inside a search field searches
without submitting the enclosing relationship form.

The legacy panel labels old relationships unverified. It uses the loaded
Affiliations collection page; its existing search/Next page locate older rows.
Loading fetches the fixed organization and CRM source for review. A later source
change refuses verification; a new load permits a new review. Verified records
leave the pending list. Missing sources remain unverified with an actionable
refusal. Late reads cannot replace another selected record or a disposed view.

Verification sources are [router/database tests](../../crates/host/tests/workspace_flows/organization_identity.rs),
[domain contracts](../../crates/domain/src/erp/organizations.rs), and
[browser plus independent SQL](../../tools/e2e/organization-identity.mjs).
The database suite includes a real pre-002 profile, owner-role migration
continuation and reapplication, while runtime writes use the non-owner role.
Executed acceptance belongs to the [PRD tracker](../product/PRD_EXECUTION_WORK.md).
