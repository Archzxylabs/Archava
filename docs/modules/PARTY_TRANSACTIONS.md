# Party-linked sales and procurement

Published [business parties](BUSINESS_PARTIES.md) can supply a reviewed customer
identity for native sales, or a supplier identity for Purchasing. The same stable
party ID may hold both roles. Contact-backed and organization-backed masters
use the same transaction contract. Names never determine a match.

## Commands

Commerce accepts `order.create_from_party` with `partyId`,
`expectedPartyRevision` and product/quantity `lines`. Purchasing accepts
`supplier.create-from-party` with `code`, `partyId`, `expectedPartyRevision`,
and `supplier.refresh-party` with supplier `id`, supplier `expectedRevision`,
`expectedPartyRevision` and `reason`. These use each domain's existing
`tenant`, `requestId`, `command` envelope. The registered tools are
`commerce.order.create_from_party`, `purchasing.supplier.create-from-party`
and `purchasing.supplier.refresh-party` and follow current mission approval.
Clients cannot submit names, ownership, source kind, grants or captured versions.

Normal transaction grants and scope apply. The actor additionally needs current
Organizations `parties.read` and ownership scope over the selected party. Neither
`parties.write` nor raw CRM/source read is required to consume a published master.
New direct party registration/refresh commands also require the Organizations
module to be enabled. No grants are automatically added to installed users.

## Versions, roles and history

A new sale requires an active customer role and exact reviewed party revision.
A new linked supplier or reviewed supplier refresh requires an active supplier
role and exact party revision. New offers, new purchase orders and supplier
reactivation also require the linked supplier's cached party revision to equal
the current active supplier master. A party change therefore requires an explicit
supplier review/refresh before further procurement. An inactive supplier can
refresh its profile before reactivation.

The adapter locks the master before making the idempotency claim. A matching
replay rechecks current transaction and party visibility, then returns the
original receipt without requiring the old role or version to remain current.
Receipt-status recovery requires current authority over the recorded transaction;
it does not read fresh party master data. Fingerprint mismatch still conflicts.

Orders and invoices expose `customerPartyId`/`customerPartyRevision`; supplier
and purchase projections expose `supplierPartyId`/`supplierPartyRevision`.
These paths leave legacy contact-reference fields null, including for a party
whose own typed source is a CRM contact. Existing legacy paths remain unlinked.

Each new document retains the exact party ID/version and authoritative name.
Order-to-invoice conversion copies the historical order identity, prices and
lines. It does not consult current party roles or require fresh party-read access.
Reservation, fulfillment, purchase confirmation and receipt can complete existing
commitments after role deactivation or party-read revocation, under their normal
current transaction permissions. History does not change when the master changes.

## Database and upgrade boundary

Append-only Commerce 008, Purchasing 004 and Billing 007 add nullable paired
identity/version columns, exclusive contact/party source checks and composite
foreign keys to immutable `party_audit` versions. Triggers reject forged or
repointed identity, stale source snapshots and manual invoices claiming a party
sales source. Purchasing locks master before supplier to serialize profile
refresh and purchase creation. Historical commitments remain valid.

The native installer and fixture apply Identity, CRM, Organizations, Commerce,
then dependent ledgers. Prior ledger IDs/checksums remain unchanged. Upgrading
the previous profile preserves legacy documents with null party fields and
supports repeated migration application. No automatic backfill or name matching
is performed. Source and runtime package must be rebuilt for these migrations.

## Native review

Sales and supplier registration offer a business-party mode alongside existing
entry choices. The picker pages through scoped registry results, filters choices
to the requested role and retains a reviewed selection until an explicit refresh.
An exact-ID refresh does not depend on the previous search/name. Enter in search
searches without submitting the enclosing business form. The supplier profile
panel shows the captured party version and reviews a newer active supplier
version before applying it. Order, invoice and PO details display historical IDs
and versions.

This completes transaction references for the implemented native flows. It does
not establish legal identity verification, credit accounts, company/book
mapping, AP/AR/GL, bank reconciliation, financial close, tax conformance,
production deployment or market parity. Those remain separate PRD decisions and
acceptance scopes.
