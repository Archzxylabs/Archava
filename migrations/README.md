# SQL migration ownership

All 77 checked-in SQL migrations currently live under this directory, grouped into 31 ledger directories. The native manifest mounts 70; 7 remain reserved specifications. The owner-configuration migration was relocated from `crates/host/migrations/owner` to `owner-pg/`; its migration ID, schema namespace, SQL bytes, and checksum are unchanged.

The manifest is the only list of applied migrations. The persistent server embeds the migrations it verifies from it at build time ([build.rs](../crates/slice/build.rs)), and the disposable PostgreSQL fixture reads the same manifest in [pg-fixture](../crates/pg-fixture/src/lib.rs). A file's presence here does not mean a given runtime mounts it or its UI is complete.

## Applied versus unmounted ledgers

`manifest.v1.json` is the installer's contract: 70 of the 77 files, across 25 ledgers, carry an `id`, `path` and SHA-256 that the Ops installer verifies and applies. The remaining seven files are **unmounted**: no manifest entry, no embedded registration, and no runtime readiness check.

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

Accounting011/012 append original-document due agreements, immutable reviewed versions and the corresponding audit kind without changing earlier SQL. See [due terms and dated subledgers](../docs/modules/DOCUMENT_DUE_TERMS.md). No historical agreements or new user grants are inferred.

Accounting013 appends immutable declared opening balances: one reviewed lump on the receivable, payable or inventory control account the current policy names, bound to exactly one journal against an equity account. It imports no documents and grants no new permission. See [Accounting](../docs/modules/ACCOUNTING.md#declared-opening-balances).
