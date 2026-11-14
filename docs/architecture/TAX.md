# Tax on documents

Status: design, accepted for implementation on 2026-11-14. Nothing here is built until the stage that names it is merged; the stage table at the end records what is.

No sales or purchase document in Archava carries tax today. This document fixes the rules for a generic value-added tax on documents. It makes no claim about the law of any country.

## Rules

1. **Tax is the company's data.** Archava ships no rate, no code and no rule as true. A company enters its own tax codes and a named reviewer confirms each one. Every screen that shows a tax code says it was entered by the company and is not verified by Archava. Nothing is described as compliant.
2. **No code, no tax.** A company without tax codes, and a document without a tax code, behaves exactly as it does today.
3. **A tax code belongs to one book.** It has a code, a kind (`output` for sales, `input` for purchases), a rate, a base fraction, a control account and an effective date range. Ranges of one code do not overlap. A version applies only once its reviewer has confirmed it. What a stored version says never changes; a change is a new version.
4. **Rates are fractions of integers.** A rate is `numerator/denominator` (11% is `11/100`). The base fraction, `1/1` by default, scales the amount the rate applies to (`11/12` taxes eleven twelfths of the price). No floating point is used anywhere.
5. **One tax code per document, with a stated price basis.** A document is either tax-exclusive (prices are net; tax is added) or tax-inclusive (prices contain the tax; tax is extracted).
   - Exclusive: `tax = round(net × base × rate)`, `gross = net + tax`.
   - Inclusive: `net = round(gross / (1 + base × rate))`, `tax = gross − net`.
   - Rounding is half away from zero, once per document, in the document's own unit (whole Rupiah for IDR, the ISO minor unit otherwise). `net + tax = gross` always holds.
6. **A document keeps what it was taxed with.** An invoice stores the code, its version, rate, base fraction, price basis, net and tax. Later changes to the code do not change the document. A correction of the invoice is split into net and tax with the invoice's stored rate.
7. **A supplier's tax is what the supplier's document says.** The entered tax amount is stored beside the amount Archava computes from the code; the entered amount is the one posted, and a difference is shown.
8. **Tax posts to its control account through the existing source bridges.** A sales invoice posts receivable (gross) against revenue (net) and output tax. A supplier bill posts goods received not invoiced (net) and input tax against payable (gross). The posting policies gain an optional tax role; without it a taxed document is refused at capture rather than posted without its tax.
9. **The tax report ties to the ledger.** For a period it shows output tax, input tax and the difference from posted sources, beside the balances of the control accounts, with any difference between the two shown.

## Not covered

- Tax codes per line, several taxes on one document, tax on tax, and exempt or zero-rated lines mixed with taxed ones.
- Withholding taxes, tax on payments, cash-basis tax, reverse charge and import tax.
- Filing, electronic invoice submission, authority-issued numbers or any connection to a tax authority.
- Tax on sales orders before invoicing, point-of-sale receipts, foodservice tickets, subscriptions and manual invoices without a company.

## Stages

| Stage | Scope | State |
| --- | --- | --- |
| A | Tax codes: table, guards, `tax_code.create` / `tax_code.confirm` / `tax_code.retire`, calculation rules in the domain, forms and list on the Accounting screen | merged (#44): Accounting migration 016, [domain rules](../../crates/domain/src/erp/tax.rs), the three commands, the `tax_codes` workspace collection and the Accounting screen panel; see [Accounting](../modules/ACCOUNTING.md#tax-codes). A version is entered by one person and applies only after a reviewer confirms it. The browser gate was not run. |
| B | Sales: tax code and price basis on a company sales order; invoice from order stores the tax snapshot; screens, mail and checkout show net, tax and total | implemented on branch `feat/tax-sales`, not merged: Commerce migration 012 (order tax code and price basis, insert guard), Billing migration 009 (invoice tax snapshot, correction tax, replaced provenance guard, the not-postable-yet guard), `order.create_for_company` takes `taxCode` and `priceIncludesTax`, `create_invoice_from_order` stores the version that applies on the invoice date, `append_correction` splits with the stored rule, the Commerce and Billing workspace reads, the Sales and Invoices screens and the invoice mail; see [Billing](../modules/BILLING_MODULE.md#tax-on-an-invoice-from-an-order) and the decisions below. A taxed invoice is refused for a company whose book has an invoice posting policy until stage C. The checkout document refuses a taxed order instead of showing a total. The browser gate was not run. |
| C | Sales posting: optional output tax role in the invoice policy; capture and draft post receivable, revenue and output tax; corrections split | not started |
| D | Purchases: tax code and entered tax on a supplier document; optional input tax role in the purchase policy | not started |
| E | Tax report tied to the control accounts; browser acceptance; schema hints for the agent | not started |

## Decisions taken in stage A

- **Confirmation is a second step on every book.** `tax_code.create` stores a version as `entered`; it applies only after `tax_code.confirm`. On a book with independent posting the confirmer must be another person than the one who entered it (the journal-posting rule); on any other book the same person may confirm. Creation never confirms.
- **Withdrawing.** `tax_code.retire` on an entered version withdraws it. It never applied, and it no longer holds its date range or fixes the kind of its code.
- **Retirement needs no second person.** Any holder of `tax.manage` retires a confirmed version, on every book.
- **Fractions are stored as entered.** `22/200` is not reduced to `11/100`.
- **"Applies on a date" defaults to the database server's current date.** The list returns the date it used as `appliesOn`; no tenant time zone is involved.

## Decisions taken in stage B

- **The order names, the invoice stores.** A company sales order stores the tax code text and the price basis and nothing else; its lines and `total_estimate` stay the priced integers (the net with an exclusive basis, the gross with an inclusive one). At creation the code must be a confirmed output code of the company's book that applies on the database's current date. The rate is taken when the invoice is made.
- **The invoice date is the day the invoice is created.** Billing has no document date: an invoice has `created_at`, stamped with the command's one clock sample. "Applies on the invoice date" means the database's date of that instant (`created_at::date`, the same calendar the tax code list uses). If no confirmed version applies on it the invoice is refused; it is never created untaxed. The version row is share-locked while the invoice is written.
- **Columns.** `gross_minor` stays the amount the customer owes and stays immutable. `tax_minor` is the tax part of it and is immutable with the snapshot (code id, code, version, kind, rate and base fraction, price basis); the net is `gross_minor − tax_minor` and is not stored. `correction_tax_minor` is the tax part of `correction_minor`, and each correction row stores its own `tax_minor`. The tax of the document after its corrections is `tax_minor + correction_tax_minor`. This mirrors the existing pair `gross_minor` / `correction_minor` instead of making `tax_minor` a running amount.
- **A correction is a gross amount, split by the corrected total.** `append_correction` changes what the customer owes on either price basis. Its tax part is `contained(gross + corrections after) − (tax + correction tax before)`, where `contained(g) = g − round(g / (1 + base × rate))`. On the gross of an exclusive document `contained` returns exactly the tax that was added, so one rule serves both bases, the parts of all corrections add up to the tax of the corrected total, and a total of zero has net and tax of zero. A single small correction can therefore carry no tax, or all of one unit; the sum is what is exact. The database recomputes the invoice's correction tax on every update and checks each correction row against it.
- **The database is the authority.** Billing 009 recomputes the tax from the order total and the stored fractions with integer arithmetic, requires the snapshot to be the order's tax code and basis and a confirmed output version of the company's book that applies on the invoice date, and compares the order total with `gross − tax` (exclusive) or `gross` (inclusive). A manual invoice cannot carry tax.
- **Not postable yet.** A taxed invoice is refused when the company's book has an invoice posting policy, at creation and again at issue (a policy can be set between the two), so the invoice bridge never captures a taxed invoice as if it were untaxed. Stage C removes this guard.
- **The Sales screen reads its tax codes from the Commerce workspace.** For the selected company the read returns `taxCodes`: code, version, rate, base and range of the confirmed output versions that apply today. It needs the Commerce `orders.read` grant and access to the company; it does not need the Accounting `tax.read` grant, and it returns no account and no person.
- **The checkout document refuses a taxed order.** It shows a draft or reserved order's total, and such an order has no tax amount yet (rule "Not covered": tax on sales orders before invoicing). `POST /embed/checkout/sign` answers 409 `checkout_order_taxed` instead of signing the order total.
- **Both members or neither.** `taxCode` without `priceIncludesTax`, or the reverse, is a conflict with its own reason, not a validation error. Only `order.create_for_company` has the two members.

## Open questions

- **A control account deactivated after confirmation.** The stored code is unchanged and still applies; the list shows the account's current state. What posting does with such a code is for stage C.
- **Which calendar dates a document.** Orders and invoices use the database server's date, like the tax code list. The accounting bridge dates a source by the UTC date of its receipt. Between midnight in the two calendars an invoice can take the tax code version of one day and post under the other. Stage C has to choose one.
- **A taxed draft whose code is retired before issue.** The snapshot is taken at creation; issuing later does not look at the code again.
- **Tax on the checkout document.** Showing an estimate with today's version would put an amount in a signed document that the invoice may not carry. Refusing is the stage B answer; an estimate would need the net, tax and total in the signed string.
- **A correction entered as a net amount.** `append_correction` takes what the customer owes less or more. A user who thinks in net amounts on an exclusive document has no way to enter one.
- **Existing taxed invoices when stage C arrives.** Invoices issued under stage B on a book without a policy are never captured (the bridge imports nothing issued before its policy). Stage C has to say whether their tax stays outside the ledger.
