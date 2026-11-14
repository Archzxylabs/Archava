# Managed-company identity

A managed company is a company the workspace explicitly declares that it operates.
It has a stable ID and company code, linked to one reviewed Organizations record.
The official name and tax/reference string are captured from that source, with its
revision. This declaration does not verify legal status, tax registration or
ownership. The same source can independently supply a business-party identity;
a customer/supplier role does not register a managed company automatically.

## Commands and current authority

The existing Organizations `{tenant, mutation: {requestId, type, data}}` envelope
supports four commands:

- `company.create_from_organization`: `code`, `organizationId`,
  `expectedSourceRevision`, `reason`.
- `company.refresh_source`: company `id`, `expectedRevision`,
  `expectedSourceRevision`, `reason`.
- `company.set_active`: company `id`, `expectedRevision`, `active`, `reason`.
- `company.set_tax_registration`: company `id`, `expectedRevision`,
  `vatRegistered`, `reason`. It stores the company's own statement that it is
  registered for value-added tax (Organizations 005), in a revision of its own. The
  statement can be true only while the captured tax/reference string is digits with
  spaces, dots, slashes or hyphens; nothing is verified. See the
  [Indonesia tax pack](TAX_INDONESIA_PACK.md#tax-registration-identifiers).

Code is 1–40 uppercase ASCII letters/digits/underscore/hyphen, starting with a
letter or digit. Revision/UUID/type/field validation is strict; reasons are
nonblank and bounded. Name, legal verification, country, currency, accounting mode,
owner and permission claims are not accepted from the command body.

The session/database determines the actor, tenant, active human membership and
Organizations all/assigned scope. Writes and receipt recovery require both
`companies.read` and `companies.write`. Source creation/refresh/reactivation also
requires current `organizations.read` and source visibility. Source must be active
and have a nonblank official name for new execution. Company read grants access
to the published captured profile independently of raw-source read. History
requires both `companies.read` and `audit.read`; organization/audit grants alone
do not expose company data.

The module must be enabled. New bootstrap owners receive the installed catalogue's
explicit company permissions; upgrading does not change existing users' grants.
The three `organizations.company.*` tools use normal reviewed mission approval
and the same current-authority transaction path.

## Review, version and recovery semantics

Creation captures the exact reviewed source revision. Source identity, company
ID, code and owner never change. A source rename does not propagate implicitly.
Refresh must advance the source revision and keeps the company's activity state.
Deactivation preserves identity/history and does not need raw-source visibility.
Reactivation requires an active source whose revision still matches the captured
company profile; a changed source must be reviewed/refreshed first.

The adapter takes source locks before company locks. Concurrent source updates,
duplicate registrations and competing company changes serialize to a coherent
version or refuse. Source/code uniqueness is tenant-bound; equal display or
official names do not merge distinct identities.

Each committed version has a complete immutable snapshot, actor, reason,
operation and bound command receipt in the same transaction. Replay rechecks
current company grants/scope and source visibility for source-consuming commands,
then returns the original receipt without requiring the old source revision/state
to remain current. Status recovery requires current company authority, without
reading the raw source. Request IDs share the Organizations namespace, so reuse
across company/party/legacy organization operations conflicts.

## Persistence, native UI and upgrade

[Organizations 004](../../migrations/organizations-pg/004_managed_companies.sql)
adds companies/company_audit with FORCE RLS, explicit runtime ACLs, source/code
uniqueness, immutable identity/history and deferred version/receipt completeness.
Source values are checked in database triggers as well as the adapter. Migration
IDs/bytes from earlier releases remain unchanged. The genuine prior profile has
Organizations 003 and current party-linked transaction ledgers. Repeated upgrade
preserves existing organizations, parties, command receipts and application grants;
it creates no company rows by inference.

Organizations provides a managed-company registration form with a scoped paginated
source picker, official-name/reference review, company code and reason. Enter in
source search does not submit registration. Refresh is explicit; stale review is
refused. Profile management uses the loaded Managed companies collection page;
search and Next page locate older records. It supports reviewed refresh and
activity changes. Late source responses cannot update another selection or a
disposed view. Company history is a separate permission-filtered collection.

Sources: [domain](../../crates/domain/src/erp/companies.rs),
[adapter](../../crates/adapters/src/erp/companies.rs),
[host](../../crates/host/src/erp/companies.rs),
[UI](../../crates/slice/src/operations/companies.rs),
[router/database tests](../../crates/host/tests/workspace_flows/companies.rs),
[browser/SQL journey](../../tools/e2e/companies.mjs).

## Remaining boundary

The registry is a declared company master. [Company trade](COMPANY_TRADE.md)
separately defines explicit product/document ownership and company-scoped access;
its acceptance is tracked in [PRD execution](../product/PRD_EXECUTION_WORK.md).
Legacy documents remain unassigned. Company/account/book mapping, consolidation,
legal verification, jurisdiction/currency, accounting-source choice, complete
AP/AR/GL, bank, close, tax conformance, production/pilot and market parity remain
separate PRD work. Accounting has not been selected on the owner's behalf.
