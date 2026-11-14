# SQL migration ownership

All 88 checked-in SQL migrations currently live under this directory, grouped into 31 ledger directories. The native manifest mounts 81; 7 remain reserved specifications. The owner-configuration migration was relocated from `crates/host/migrations/owner` to `owner-pg/`; its migration ID, schema namespace, SQL bytes, and checksum are unchanged.

The manifest is the only list of applied migrations. The persistent server embeds the migrations it verifies from it at build time ([build.rs](../crates/slice/build.rs)), and the disposable PostgreSQL fixture reads the same manifest in [pg-fixture](../crates/pg-fixture/src/lib.rs). A file's presence here does not mean a given runtime mounts it or its UI is complete.

## Applied versus unmounted ledgers

`manifest.v1.json` is the installer's contract: 81 of the 88 files, across 25 ledgers, carry an `id`, `path` and SHA-256 that the Ops installer verifies and applies. The remaining seven files are **unmounted**: no manifest entry, no embedded registration, and no runtime readiness check.

| Unmounted ledger | Files |
| --- | --- |
| `act-confirmation-pg` | `001_confirmation_challenges.sql` |
| `act-host-pg` | `001_host_intents.sql`, `002_host_scope_binding.sql` |
| `act-storage` | `001_act_attempts.sql` |
| `capacity-pg` | `001_delivery_capacity_pools.sql` |
| `crm-act` | `001_native_confirmations.sql` |
| `universal-pg` | `001_universal_enterprise_ledgers.sql` |

They are kept in place rather than deleted (the bytes may be the only record of an earlier design) and rather than promoted into the manifest (that would change what an installer verifies and applies, and none of the seven has a mounted route or readiness registration). The decision is enforced, not remembered: [`tools/check-structure.mjs`](../tools/check-structure.mjs) fails when a SQL file is in neither the manifest nor its explicit `UNMOUNTED_MIGRATIONS` list, when a listed checksum stops matching, or when a listed file disappears. Mounting one of these ledgers therefore requires the full four-step registration described in `AGENTS.md`, plus removing its row from that list.

Applied migrations are append-only. Add a numbered file and its ledger/readiness registration instead of editing historical SQL. Apply migrations with the owner role; serve requests through a non-owner runtime role. Do not rename schemas or migration IDs as part of directory cleanup.

The appended `owner-pg/002_reviewed_workflows.sql` stores immutable reviewed import/feed plans and first terminal receipts. Its checksum, embedded owner ledger and fixture/runtime grants are registered together. See [reviewed workflows](../docs/modules/REVIEWED_WORKFLOWS.md).

The appended `owner-pg/003_agent_runtime.sql` stores scoped agent conversations,
preferences, immutable goals, atomic recoverable turns, model attempt budgets and
worker routing keys. See [Ava runtime](../docs/modules/AGENT_RUNTIME.md) for the
narrow worker-routing metadata RLS exception and current-session execution checks.

The appended `billing-pg/005_order_invoice_provenance.sql` links converted invoices
and their lines to frozen Commerce sales orders with tenant-bound foreign keys,
unique provenance, immutability and deferred completeness guards. Existing manual
invoices keep null provenance. See [Billing](../docs/modules/BILLING_MODULE.md).


Commerce 007 and Billing 006 preserve explicit CRM customer identity through sales
orders and invoice conversion. CRM must migrate before Commerce; the manifest and
fixture apply Identity, CRM, Organizations, Commerce, then dependent ledgers. Historical migration
IDs, bytes and checksums are preserved. See [customer identity](../docs/modules/CUSTOMER_IDENTITY.md).

Purchasing 003 adds explicit supplier CRM source snapshots and reviewed supplier
profile refresh while preserving old PO/receipt history and unlinked manual rows.
It depends on CRM and is registered in the same mounted apply order. See
[supplier identity](../docs/modules/SUPPLIER_IDENTITY.md).

Organizations 002 enforces new CRM affiliation references and preserves legacy
rows as explicitly unverified. [Organization identity](../docs/modules/ORGANIZATION_IDENTITY.md)
describes reviewed creation, legacy verification and the NOT VALID FK boundary.

Organizations 003 adds a source-backed business-party registry and immutable
version history. [Business-party contracts](../docs/modules/BUSINESS_PARTIES.md)
define explicit grants and independent roles. [Party transaction contracts](../docs/modules/PARTY_TRANSACTIONS.md) cover the mounted sales/purchase/invoice references and historical versions.

Organizations 004 adds explicitly reviewed managed-company identities and immutable
version history. [Managed-company contracts](../docs/modules/MANAGED_COMPANIES.md) distinguish this master from transaction/company/book mapping.

Commerce 009, Purchasing 005 and Billing 008 add explicit company ownership and
historical document snapshots after Organizations 004. Existing rows stay
unassigned. [Company trade](../docs/modules/COMPANY_TRADE.md) defines product-based
stock partitioning, current human company access and historical completion rules.

Accounting004 follows Purchasing006 and Billing in the native profile. Its four purchase policy/origin/source/journal-link tables inherit the existing dynamic Accounting write fence; no empty Control migration is added. Runtime grants, exact trigger readiness and native/fixture registration accompany the append-only SQL. Old confirmation histories are not backfilled. See [purchase posting](../docs/modules/PURCHASE_POSTING.md).

Accounting009 adds one immutable supplier-settlement table after the manual bank
profile. It preserves all prior migration bytes, extends membership vocabulary
without granting existing members new authority, and binds payments/returns to
actual bank rows and posted original bill sources. See [supplier payments](../docs/modules/SUPPLIER_PAYMENTS.md).

Accounting010 appends immutable gateway-to-bank settlements after Accounting009.
It shares bank-row reservations with supplier payments without rewriting prior
SQL, adds explicit clearing permissions without granting existing members new
authority, and binds each draft to a posted verified collection/refund source.
See [gateway bank clearing](../docs/modules/GATEWAY_BANK_CLEARING.md).


Commerce010 preserves an exact immutable return-cost reference on each new restock
layer, leaving historical ambiguous layers NULL. It retains private cost writers
and adds exact trigger/FK/index/privilege readiness for the mounted profile. This
is original-cost provenance, not COGS journal posting. See
[Commerce](../docs/modules/COMMERCE_MODULE.md).

Commerce011 replaces the IDR-only currency checks on products and sales orders
with a three-letter format check and insert guards: a company-owned row must carry
the currency of its company's accounting book, any other row IDR, and an order line
the currency of its order. Commerce migrates before Accounting, so the guard reads
`archava_accounting.books` late-bound and treats a missing ledger as "no book".
Existing rows are not rewritten. See [trade currency](../docs/architecture/MULTI_CURRENCY.md).

Purchasing007 does the same for supplier offers, purchase orders and supplier
documents: the three IDR-only checks become the format check, a purchase order must
carry its company's trade currency, an offer that of the company owning its product
(and the product's own currency), and a purchase line must name an offer and a
product of its order's currency. A supplier document keeps the Purchasing006 guard
that binds it to its order's currency. The rule lives in
`archava_purchasing.trade_currency`, a copy of the Commerce function, so the guards
do not depend on Commerce011 being applied. Existing rows are not rewritten.

Accounting011/012 append original-document due agreements, immutable reviewed versions and the corresponding audit kind without changing earlier SQL. See [due terms and dated subledgers](../docs/modules/DOCUMENT_DUE_TERMS.md). No historical agreements or new user grants are inferred.

Accounting013 appends immutable declared opening balances: one reviewed lump on the receivable, payable or inventory control account the current policy names, bound to exactly one journal against an equity account. It imports no documents and grants no new permission. See [Accounting](../docs/modules/ACCOUNTING.md#declared-opening-balances).

Accounting014 lets the invoice, purchase and stock posting policies name a second
source unit. `whole_idr` stays valid only on a native IDR book; `minor` is valid only
on a native book in another currency and posts source amounts unchanged. The policy
guards and the capture and effect functions are replaced in place (same signatures, so
the triggers the readiness functions identify are unchanged), a source whose currency
differs from its book's is refused, and no row is rewritten. See
[source units](../docs/modules/ACCOUNTING.md#source-units).

Accounting015 adds an insert guard on `archava_accounting.books`: a book in another
currency than IDR is refused when its company already owns a Commerce product or
sales order, a Purchasing order or a Billing invoice, which are all IDR without a
book. The guard locks the company row (`FOR NO KEY UPDATE`) against the shared lock
of the Commerce 009, Purchasing 005 and Billing 008 company guards, reads the three
earlier ledgers directly (Accounting migrates after them) and needs no new runtime
grant. IDR books and existing books are unaffected. See
[trade currency](../docs/architecture/MULTI_CURRENCY.md#known-consequences).

Accounting016 appends `archava_accounting.tax_codes`: one row per version of a
company's own tax code (kind, rate and base fraction as integer fractions, control
account, effective date range, and a state: entered, confirmed, withdrawn or retired),
with FORCE RLS, an insert/update guard that locks the book row and allows only
confirmation, withdrawal and retirement, the shared audit-and-receipt constraint, a
claim trigger that binds the creator and the reviewer to their own commands, and
`tax_guards_complete()`. It adds
`tax.read` and `tax.manage` to the membership permission vocabulary and `tax_code` to
the audit kinds, grants neither permission to an existing membership, creates no row
and ships no rate. The runtime role needs `SELECT,INSERT,UPDATE` on the table and
`EXECUTE` on the readiness function. Nothing reads a tax code yet. See
[tax codes](../docs/modules/ACCOUNTING.md#tax-codes).

Commerce012 adds `tax_code` and `price_includes_tax` to sales orders (both null or
both set, immutable, only on a company order) and an insert guard: the code must be a
confirmed output tax code of the company's book that applies on the database's current
date. Commerce migrates before Accounting, so the guard reads
`archava_accounting.tax_codes` late-bound, like the book in Commerce011. Existing
orders are not rewritten and the order stores no rate.

Billing009 adds the invoice tax snapshot (`tax_minor`, the tax code version, its rate
and base fraction and the price basis), `correction_tax_minor` and
`invoice_corrections.tax_minor`, all `0`/null on existing rows; integer tax arithmetic
functions; an insert/update guard on invoices and an insert guard on corrections; and
replaces `archava_billing.assert_order_invoice()` of Billing005 in place (same
signature, so its two deferred triggers are unchanged): the order total is compared
with `gross - tax` for an exclusive basis, and the tax is recomputed from the order
and the snapshot. It also refuses a taxed invoice for a company whose book has an
invoice posting policy, read late-bound from `archava_accounting.invoice_policies`,
until tax posts to the ledger. No new table and no new runtime grant: the serving role
already reads `archava_accounting.tax_codes`, `books` and `invoice_policies`. See
[tax on documents](../docs/architecture/TAX.md).

Billing010 replaces `archava_billing.guard_invoice_tax()` of Billing009 in place (same
signature, so its trigger is unchanged) without the refusal of a taxed invoice on a
book with an invoice posting policy; every other rule of that guard is kept.
`archava_billing.invoice_posting_policy()` stays installed and is no longer called.
No row is rewritten and no grant changes.

Accounting017 adds `tax_minor` (ledger minor units, default 0), `tax_account_id` and
`tax_account_revision` (the account's revision at capture) to `archava_accounting.invoice_sources`, with a check that tax
has an account, the sign of the amount and is no larger, and a foreign key to the
book's accounts. Existing rows get tax 0 and no account. It adds
`archava_accounting.invoice_effect_tax(text,uuid)`, the tax part of an invoice effect
as Billing stored it, and replaces `guard_invoice_source()`,
`capture_invoice_source()` and `guard_journal_source()` in place with their
signatures, so the triggers `source_guards_complete()` identifies by function OID are
unchanged. `invoice_effect(text,uuid)` is not replaced. The new function keeps the
default `EXECUTE` of the other effect functions; no runtime grant is added and no
table is created. Billing010 migrates before it. See
[invoice source posting](../docs/modules/INVOICE_POSTING.md#tax-on-a-captured-source).

Purchasing008 adds the supplier document tax snapshot to
`archava_purchasing.supplier_documents`: `tax_minor` (the tax amount printed on the
supplier's document) and `tax_computed_minor` (the tax the code gives on the goods
total), both default 0, and the tax code version (id, code, version, rate and base
fraction), null on existing rows, with a check that a document has either the whole
snapshot or none and that neither tax exceeds `declared_total`. It adds integer tax
arithmetic functions and the guard `guard_supplier_document_tax()` with its trigger
`supplier_document_tax_guard`: a captured version stores the confirmed input version of
the company's book that applies on the document date and the tax that version gives; a
match, a hold and a void cannot change the tax. Purchasing migrates before Accounting,
so the guard reads `archava_accounting.tax_codes` late-bound, like the book in
Purchasing007. Version snapshots are the whole row, so a version stored after this
migration carries the tax keys and earlier versions are not rewritten. No new table and
no new runtime grant: the serving role already reads `archava_accounting.tax_codes` and
`books`.

Accounting018 adds `tax_minor` (ledger minor units, default 0), `tax_account_id` and
`tax_account_revision` (the account's revision at capture) to
`archava_accounting.purchase_sources`, with a check that tax has an account, belongs to
a bill or a void, has the sign of the goods and is no larger, and a foreign key to the
book's accounts. Existing rows get tax 0 and no account. It adds
`archava_accounting.purchase_effect_tax(text,uuid)`, the tax part of a purchase effect
as Purchasing stored it on the document version, and replaces `guard_purchase_source()`,
`capture_purchase_source()`, `purchase_legs(text,uuid)`,
`guard_purchase_journal_source()` and `payable_balance(text,uuid,uuid)` in place with
their signatures, so the triggers `purchase_guards_complete()` and
`payable_guards_complete()` identify by function OID are unchanged.
`purchase_effect(text,uuid)` is not replaced. `payable_balance` counts a bill and its
posted void in gross (goods plus tax). The new function keeps the default `EXECUTE` of
the other effect functions; no runtime grant is added and no table is created.
Purchasing008 migrates before it. See
[purchase posting](../docs/modules/PURCHASE_POSTING.md#tax-on-a-supplier-bill).
