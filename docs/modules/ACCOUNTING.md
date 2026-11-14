# Company accounting

The mounted `archava_accounting` ledger provides explicitly configured company books, accounts, financial periods, immutable reviewed journal drafts, separate posting, draft voids and reversal drafts. Installing it does not create books, grant existing users financial access, or post historical trade. New owner bootstrap uses the installed permission catalogue; existing tenants need explicit administration and an explicit module enable when their stored configuration predates Accounting.

Each managed company has one configured official book. Setup fixes the reviewed company version, source authority, functional currency, ISO minor-unit exponent and independent posting policy. Native and external source declarations cannot be changed by editing a book. An external declaration is configuration only: its connector remains explicitly unready, and native account/journal writes refuse it. External accounting synchronization is not accepted yet.

Native journals require an open, non-overlapping period and 2–100 one-sided, positive, balanced lines. Exact decimal strings carry integer minor units; no floating-point money conversion runs in persistence or reports. Account identities/classifications and journal lines cannot be edited. A changed/inactive account prevents a reviewed draft from posting. When independent posting is configured, the draft author cannot post it. Corrections create a separately reviewed inverse draft in an open period and preserve the original posted entry. A posted original has at most one non-void reversal. Voids apply only to drafts.

The HTTP surface is `/erp/accounting/commands`, `/commands/status`, `/workspace`, and `/reports` under the same prefix. Every call derives the human actor from the session, applies the configured module gate and current financial membership, and resolves the company's current Organizations grant/scope. All writes use an atomic tenant transaction and bind command, actor, fingerprint, receipt and immutable audit. Replay/status rechecks current authority. Every financial table has FORCE RLS. Book UPDATE privilege is needed for PostgreSQL row locks; the immutable-book trigger refuses actual updates.

Company deactivation stops new operating business and new book setup. It does not abandon an existing native book: current authorized financial staff can finish drafts, post reviewed adjustments/reversals, manage accounts and close/reopen periods under the same financial rules. Inactive status never replaces current company visibility or financial permission.

The new control-ledger migration extends both writer-fence installation and readiness verification to Accounting. Accounting serving readiness checks that exact control history as well as financial table policies. Existing applied migrations remain unchanged.

Workspace collections support bounded, literal search, exact IDs and keyset pages. No selected company returns setup metadata without financial records. Financial reports hold the book read lock through their transaction, include all posted journals in the selected filter, exclude drafts/voids, and return exact aggregate strings. The trial balance and profit/loss use the selected activity range; the balance sheet uses all postings through its end. Unclosed cumulative earnings remain explicit. Cash flow assigns a cash journal only when its non-cash counterpart has one reviewed category; mixed/missing categories are reported as exceptions.

The native Accounting screen reuses reviewed forms and record recovery, offers company selection, exact decimal currency entry and reports. A typed minor-unit API is not a reason to ask an operator to calculate minor units manually. Stored financial data require an explicit current [AI sharing selection](AI_DATA_SHARING.md), financial amounts and the selected company before provider planning; current financial read grants still apply. Typed actions remain local. Ava journal review loads exact original journal lines/account names for human approval, and changed facts require review again. Native release acceptance remains tied to its actual candidate; full financial agent workflows remain separate work.

The synthetic router/PostgreSQL suite is [erp_accounting.rs](../../crates/host/tests/erp_accounting.rs). Its GL control example covers opening cash/equity, inventory/AP, supplier payment, COGS/inventory, AR/revenue and collection. These are explicit journal inputs. The separate [invoice posting contract](INVOICE_POSTING.md) adds future-only reviewed invoice sources, corrections and provider clearing; its bounded [browser/native acceptance](../product/PRD_INVOICE_POSTING_ACCEPTANCE_2026-11-08.md) is recorded for the exact candidate source. The [purchase source bridge](PURCHASE_POSTING.md) adds a future-confirmation purchase policy, accepted receipt/GRNI, goods claim/AP and void sources with separate journal review/posting. Its bounded local native acceptance is recorded with the [stock posting and shared financial journey](../product/PRD_STOCK_POSTING_ACCEPTANCE_2026-11-08.md). The [original stock cost bridge](STOCK_COST_POSTING.md) adds future fulfillment/restock capture and separate review/posting, with scoped native/browser/upgrade acceptance in that record. [Document terms and aging](DOCUMENT_DUE_TERMS.md), [supplier payments](SUPPLIER_PAYMENTS.md), [manual bank reconciliation](MANUAL_BANK_RECONCILIATION.md) and [gateway clearing](GATEWAY_BANK_CLEARING.md) are delivered by Accounting migrations 006–012 and described in their own pages. Still missing: credit-note documents, one payment allocated across several bills or invoices, a year-end close of earnings to retained earnings, exchange rates and revaluation, tax, aging grouped by customer or supplier, a dated valuation of stock layers, import of legacy opening documents, external connectors and end-user acceptance. This document is a current contract, not a release acceptance report.

Currency metadata comes from [SIX ISO 4217](https://www.six-group.com/en/products-services/financial-information/market-reference-data/data-standards.html), List One published 2026-09-17. ISO minor-unit exponents are distinct from a provider's API amount units; for example this snapshot gives IDR exponent 2. A new catalogue must not silently reinterpret existing books or existing trade/payment amounts.

[Manual bank evidence and matching](MANUAL_BANK_RECONCILIATION.md) defines Accounting006–007 statement imports, explicit bank grants, posted-cash allocations and immutable corrections. Complete bank/financial close remains pending.

## Source units

The invoice, purchase and stock posting policies each name the unit their source amounts are in (`sourceUnit`). There are two, and the database decides which one fits a book (Accounting migration 014):

| Unit | Valid on | Source amount | Ledger amount |
| --- | --- | --- | --- |
| `whole_idr` | a native IDR book with two minor units | whole Rupiah: `15000` is Rp15.000 | multiplied by 100 |
| `minor` | a native book in any other currency | the currency's ISO minor unit: `1999` is USD 19.99, `1500` is JPY 1500, `1250` is KWD 1.250 | unchanged |

A policy with the other unit is refused as a conflict, and the unit of an existing policy cannot be changed; a book's currency is fixed, so a book only ever has one unit. Each captured source keeps the policy snapshot it was captured under, including the unit: an invoice's corrections and payments use the snapshot of its issue, and a purchase order's receipts and bills the snapshot pinned when it was confirmed.

The bridges convert nothing between currencies. A source in another currency than its book is refused when it would be captured, which refuses the operating command: issuing the invoice, confirming the purchase order, or fulfilling or returning the sales order. A company that already owns a product, sales order, purchase order or invoice cannot open a book in another currency than IDR (Accounting migration 015; `book.create` is refused as a conflict, an IDR book is still accepted), so the commands no longer produce such a source; the refusal remains for a database that already holds one. See [trade currency](../architecture/MULTI_CURRENCY.md#known-consequences). Without a policy nothing is captured and nothing is refused. The control, aging and close reports work on ledger minor amounts and are the same for both units.

The operator policy forms offer both units and, while the configured book's currency is known, hold the one that book requires; the command API accepts both. A captured source is reviewed and drafted from the screen on any book whose captured unit is the book's; its amounts are already book minor units and are shown with the book's currency and decimals.

## Close readiness

`GET /erp/accounting/close-readiness?tenant=<tenant>&periodId=<id>` states what still stands between a period of a native book and its close. It requires `reports.read` and the same book visibility as the reports, writes nothing, and returns:

| Field | Meaning |
| --- | --- |
| `period` | id, book, code, state, revision, start and end date |
| `blockers` | one entry per outstanding condition: `reason`, `count`, `message` |
| `bankReviews` | each bank policy in range with `checkReady` and `reviewed` |
| `ready` | the period is open and `blockers` is empty |
| `canClose` | `ready`, and the caller holds `periods.close` |

The reasons are the conditions the period's database close guards refuse on: `draft_journals`, `unposted_invoice_sources`, `unposted_purchase_sources`, `unposted_stock_sources`, `unmatched_bank_rows`, `banks_without_policy` and `bank_reviews_incomplete`. The guards remain the authority; `period.close` can still be refused if something changes between the read and the command. A guard added in a migration must be added to [close.rs](../../crates/adapters/src/erp/accounting/close.rs).

Not part of this report: agreement of the receivable and payable subledgers with their control accounts (see the [subledger report](DOCUMENT_DUE_TERMS.md)), a dated stock valuation and a year-end earnings close.

The Accounting screen shows it as "Period close check" under Financial reports: choose a period, and it lists the outstanding conditions and whether the period can be closed.

[close.rs](../../crates/host/tests/erp_accounting/close.rs) walks one period through a draft journal, a bank without a policy, unmatched rows, incomplete coverage and a missing review, asserting at each step that the report lists the condition and `period.close` is refused, and that the close succeeds once the report is ready. A second test covers an unposted invoice source. Unposted receipt and stock cost sources are covered by `close_readiness_counts_unposted_receipt_and_stock_sources` in [stock_sources.rs](../../crates/host/tests/erp_accounting/stock_sources.rs).

## Declared opening balances

`opening_balance.draft` (permission `journals.write`) declares one opening amount on a control account so that the control comparisons explain it instead of reporting it as a difference. Fields: `bookId`, `control` (`receivable`, `payable` or `inventory`), `code`, `date`, `controlAccount`, `offsetAccount`, `amountMinor`, `reason`.

- It creates a two-line draft journal with the reserved reference `opening:<control>:<code>` and one immutable `opening_balances` row that shares the journal's id (Accounting013). Posting, voiding and reversing use the existing `journal.post`, `journal.void` and `journal.reverse`, so independent posting applies unchanged.
- The control account must be the account the book's current policy names for that role: `receivable` from the invoice policy, `payable` and `inventory` from the purchase policy. The policy therefore has to exist first, and an account a policy no longer names cannot take a new opening.
- The offset account must be an active equity account.
- The amount is positive and lands on the control account's natural side: a debit for receivable and inventory, a credit for payable.
- A generic `journal.draft` cannot use an `opening:` reference, and a stored declaration cannot be updated or deleted.

In the [subledger report](DOCUMENT_DUE_TERMS.md) and the [inventory control comparison](STOCK_COST_POSTING.md#inventory-control-comparison), a posted opening dated on or before the cutoff is added to `expectedMinor` and shown as `openingMinor`; a posted reversal dated on or before the cutoff removes it. The subledger report also returns `declaredOpeningMinor`.

Limits: an opening is one lump per declaration with no documents behind it. It is not aged, has no customer or supplier, is not part of the document totals and cannot be settled or allocated against. Credit-side receivable openings, debit-side payable openings, document import and stock quantities are not supported; an inventory opening changes the ledger control only and creates no cost layer. The Accounting screen offers the action through the generic reviewed form; there is no dedicated opening view.

[openings.rs](../../crates/host/tests/erp_accounting/openings.rs) covers the receivable case (draft not recognized, independent posting, cutoff, a generic journal still a difference, reversal) and the refusals; `declared_inventory_and_payable_openings_are_explained_by_their_controls` in [stock_sources.rs](../../crates/host/tests/erp_accounting/stock_sources.rs) covers inventory and payable.

## Tax codes

Stage A of [tax on documents](../architecture/TAX.md): a company enters its own tax codes. Archava ships no rate and verifies none. From stage B a company sales order can name a confirmed output code and the invoice made from it stores the version that applies ([Billing](BILLING_MODULE.md#tax-on-an-invoice-from-an-order)); from stage C a taxed invoice posts its tax to the code's control account through the invoice bridge ([invoice source posting](INVOICE_POSTING.md#tax-on-a-captured-source)); from stage D a supplier document can name a confirmed input code with the supplier's tax amount ([supplier documents](SUPPLIER_DOCUMENTS.md#tax-on-a-supplier-document)) and its matched bill posts that tax to the code's control account through the purchase bridge ([purchase posting](PURCHASE_POSTING.md#tax-on-a-supplier-bill)). No posting policy, report or agent step reads a code.

A tax code belongs to one native book. One row of `archava_accounting.tax_codes` (Accounting016) is one version of a code:

| Field | Meaning |
| --- | --- |
| `code`, `version` | The code (capital letters, digits, `-`, `_`; at most 40) and its version, counted from 1 per book and code |
| `kind` | `output` (sales) or `input` (purchases); the same for every version of a code that was not withdrawn |
| `rate` | `numerator/denominator` of integers, from 0 to 1 inclusive; a zero rate is allowed. Denominators are 1 to 1,000,000. Stored as entered, not reduced |
| `base` | The fraction of the price the rate applies to, above 0 and at most 1; `1/1` when omitted |
| `controlAccount` | An active account at its reviewed revision: class `liability` for `output`, class `asset` for `input`, in both cases with cash role `operating` (so not a cash account) |
| `effectiveFrom`, `effectiveTo` | First and last day the version applies, both inclusive; `effectiveTo` is empty until the version is retired |
| `state` | `entered`, `confirmed`, `withdrawn` or `retired` |
| `createdBy`, `reviewedBy` | The actor of `tax_code.create`, and the actor of `tax_code.confirm` (empty until then) |

A version moves through these states and no others:

| From | Command | To | Revision |
| --- | --- | --- | --- |
| (new) | `tax_code.create` | `entered`: stored, does not apply | 1 |
| `entered` | `tax_code.confirm` | `confirmed`: applies from `effectiveFrom` | 2 |
| `entered` | `tax_code.retire` | `withdrawn`: never applied | 2 |
| `confirmed` | `tax_code.retire` with `effectiveTo` | `retired`: applied through `effectiveTo` | 3 |

Commands (permission `tax.manage`, and `tax.read` as well):

- `tax_code.create`: `bookId`, `code`, `kind`, `rate`, optional `base`, `controlAccount`, `effectiveFrom`, `reason`. It stores a new open-ended version as `entered` on every book; creation never confirms. Its receipt carries `taxCodeState: "entered"`.
- `tax_code.confirm`: `id`, `expectedRevision`, `reason`. On a book with independent posting the confirmer must be another person than the one who entered the version, the same rule as journal posting; on any other book the person who entered it may confirm it. The control account is checked again: it must still be active at the revision the version was entered with.
- `tax_code.retire`: `id`, `expectedRevision`, optional `effectiveTo`, `reason`. On a confirmed version it sets the last day, which is required and may not precede the first. On an entered version it withdraws the version; a last day, if sent, is not stored. Retiring and withdrawing need no second person on any book.

Only a confirmed or retired version applies on a date. The ranges of one code may not overlap: entered, confirmed and retired versions count, withdrawn ones do not, so a second version is refused until the first is retired with an earlier last day or withdrawn. What a stored version says cannot be edited, and a version cannot be deleted. Every insert and update locks the book row, so two concurrent writers of one book are serialised and the second sees the first one's row.

`tax.read` and `tax.manage` are new permissions. The migration grants them to nobody: an existing membership has neither until an administrator adds them.

The `tax_codes` collection of `/erp/accounting/workspace` (permission `tax.read`) lists the versions. `asOf=YYYY-MM-DD` chooses the date for `applies`; without it the database server's current date is used (no tenant time zone is involved), and `appliesOn` names the date either way. The Accounting screen lists the collection in a "Tax codes" panel under Financial reports with each version's state, who entered it and who confirmed it, and the sentence "Entered by your company; Archava does not verify tax rates or rules." Codes are entered, confirmed, withdrawn and retired through the generic reviewed forms.

The calculation rules of the design (rule 5) are in [tax.rs](../../crates/domain/src/erp/tax.rs): `TaxRule::exclusive` and `TaxRule::inclusive`, integer-only, rounded half away from zero. Invoices from orders use both; a supplier document uses the exclusive rule for its computed tax (`TaxRule::entered`).

Refusals carry their own [reason](../architecture/REFUSAL_REASONS.md): `tax_code_control_account_wrong_class`, `tax_code_control_account_changed`, `tax_code_ranges_overlap`, `tax_code_kind_changed`, `tax_code_independent_reviewer_required`, `tax_code_not_awaiting_confirmation`, `tax_code_last_day_required`, `tax_code_ends_before_start`, `tax_code_already_closed` and `tax_code_native_book_required`. A rate or base outside its bounds is refused as an invalid submission before anything is read.

Limits:

- A version cannot be entered with a last day; it is created open-ended and retired after confirmation.
- A control account that is deactivated after confirmation does not change the stored code, which still applies; the workspace row shows the account's current `active` and `revision`. On a book with an invoice posting policy an invoice taxed with that version cannot be issued while the account is inactive (`invoice_tax_control_account_changed`): reactivate the account, or for later invoices retire the version and enter a new one with an active account. The same holds for an input code on a book with a purchase posting policy: a supplier claim taxed with that version cannot be matched while the account is inactive (`supplier_bill_tax_control_account_changed`). An entered version whose account changed cannot be confirmed: withdraw it and enter it again.
- The create and confirm steps are separate forms; the screen has no single guided flow for them.

[tax.rs](../../crates/host/tests/erp_accounting/tax.rs) covers entering, the creator's refused confirmation and a second person's confirmation on a book with independent posting, self-confirmation on a book without it, listing by date, replay of each command, withdrawing and re-entering over the same range, retiring and a second version; confirmation after the control account changed; each refusal and the permissions; immutability by direct SQL as the serving and owner roles; concurrent versions and concurrent confirmations of one code; and the readiness function. The upgrade assertion that an existing membership gains no tax permission is in `tools/e2e/prior-native.mjs`, which this stage did not run.
