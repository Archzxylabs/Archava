# Reviewed invoice source posting

This is the current implementation contract. Source posting is being validated;
it is separate from the dated [manual GL acceptance](../product/PRD_NATIVE_GL_ACCEPTANCE_2026-11-08.md).
It does not establish full FIN-03 or financial release acceptance.

An authorized accountant may configure `invoice_policy.set` for a native
book. Setup explicitly declares the [source unit](ACCOUNTING.md#source-units),
the pinned currency catalogue and three reviewed account roles: receivable
(asset/operating), revenue (revenue/operating), and payment clearing (asset/no
cash classification). On an IDR book the unit is `whole_idr`: whole Rupiah
convert exactly by 100 into the book's ISO exponent-2 units. On a book in any
other currency the unit is `minor`: the invoice amount is already the
currency's ISO minor unit and posts unchanged. The other unit, an invoice in
another currency than the book, and FX are refused. Policy changes use the
current revision and preserve immutable audit snapshots; book/unit identity
remains fixed.

The policy applies to future invoice issuance for that managed company. Existing
issued invoices remain opening/import work, including their subsequent changes;
setup does not infer historical balances. A draft issued after setup is captured.
Installing migrations creates no policy, book, journal or existing-user grant.

An invoice command saves its immutable financial source inside the same
transaction as the operating command, receipt and audit. Invoice issuance,
correction, void, verified provider collection and verified refund carry exact
signed amounts, source command/invoice/revision, UTC source date and original
policy snapshot. Failed provider notifications and payment attempts do not imply
money. Corrections and payments retain the issuance policy after later mapping
changes. The capture path needs the actor's current operating authority; it does
not grant that actor arbitrary financial permissions.

`invoice_source.draft` requires current financial and company authority. A human
reviews the captured amount, source date and the two mapped current account
versions. The server derives the amount and balanced lines from the source;
request-body prices, approvals and unrelated accounts cannot replace them. An
invoice/credit/void affects receivable and revenue. Verified collection/refund
affects receivable and payment clearing. Provider money is not booked as settled
bank cash; bank settlement and reconciliation remain separate work.

Normal independent posting, account activity/version, period and receipt rules
apply. A source has at most one live journal; a withdrawn draft may be reviewed
again under a new journal code. Posting and replay retain the source ID. A posted
source journal cannot be reversed independently of its operating source: use the
reviewed invoice correction or verified refund workflow. The reserved `billing:`
reference requires a matching source link, immutable audit and command receipt.
Zero sources retain an explicit no-monetary-effect state without a fake journal.

Period close refuses any nonzero captured source in that period without a posted
source journal. A closed source period refuses a later operating change
atomically. Reports expose all captured sources through the end date, including
unposted counts; these counts do not cover legacy issued invoices or bank
reconciliation. They are not a claim that every subledger is closed.

Once a policy exists, Owner preview/apply/restore keeps Billing and Accounting
enabled. Safe decommissioning or transfer is separate work. The shared owner
configuration fence serializes policy setup with configuration changes.

Accounting002 physically depends on the current company-owned Billing schema;
the native manifest and fixture migrate Billing before Accounting. Accounting003
binds source journal claims and verifies capture/control triggers at startup.
All eleven financial tables have FORCE RLS and runtime writer fences. Financial
source reads remain subject to financial/company access and are withheld from
provider planning context together with the other financial records.

The [source integration suite](../../crates/host/tests/erp_accounting/invoice_sources.rs)
uses real routers, typed adapters and disposable PostgreSQL. The bounded
[invoice-source acceptance](../product/PRD_INVOICE_POSTING_ACCEPTANCE_2026-11-08.md)
records full gate, native/browser, upgrade and encrypted restore evidence for its
exact source. Later working-tree changes require their own verification. AP, stock/COGS source
posting, due dates/aging, formal credit documents, provider-to-bank matching,
complete subledger close, localization and actual-user acceptance remain open.
