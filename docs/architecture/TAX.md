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
| A | Tax codes: table, guards, `tax_code.create` / `tax_code.confirm` / `tax_code.retire`, calculation rules in the domain, forms and list on the Accounting screen | implemented on branch `feat/tax-codes`, not merged: Accounting migration 016, [domain rules](../../crates/domain/src/erp/tax.rs), the three commands, the `tax_codes` workspace collection and the Accounting screen panel; see [Accounting](../modules/ACCOUNTING.md#tax-codes). A version is entered by one person and applies only after a reviewer confirms it. Nothing uses a code yet. The browser gate was not run. |
| B | Sales: tax code and price basis on a company sales order; invoice from order stores the tax snapshot; screens, mail and checkout show net, tax and total | not started |
| C | Sales posting: optional output tax role in the invoice policy; capture and draft post receivable, revenue and output tax; corrections split | not started |
| D | Purchases: tax code and entered tax on a supplier document; optional input tax role in the purchase policy | not started |
| E | Tax report tied to the control accounts; browser acceptance; schema hints for the agent | not started |

## Decisions taken in stage A

- **Confirmation is a second step on every book.** `tax_code.create` stores a version as `entered`; it applies only after `tax_code.confirm`. On a book with independent posting the confirmer must be another person than the one who entered it (the journal-posting rule); on any other book the same person may confirm. Creation never confirms.
- **Withdrawing.** `tax_code.retire` on an entered version withdraws it. It never applied, and it no longer holds its date range or fixes the kind of its code.
- **Retirement needs no second person.** Any holder of `tax.manage` retires a confirmed version, on every book.
- **Fractions are stored as entered.** `22/200` is not reduced to `11/100`.
- **"Applies on a date" defaults to the database server's current date.** The list returns the date it used as `appliesOn`; no tenant time zone is involved.

## Open questions

- **A control account deactivated after confirmation.** The stored code is unchanged and still applies; the list shows the account's current state. What posting does with such a code is for stage C.
