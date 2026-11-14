# Trade currency

Status: design, accepted for implementation on 2026-11-14. Nothing in this document is built until the stage that names it is merged; the stage table at the end records what is.

The owner targets Indonesia and a global market. The general ledger already accepts any ISO 4217 book currency. Sales, purchasing, supplier documents and the accounting source bridges accept only Indonesian Rupiah, so a company whose book is not IDR can neither trade nor post. This document fixes the rules that remove that limit.

## Rules

1. **The book is the only currency authority.** A company's trade currency is the currency of its accounting book (`archava_accounting.books`, one per company, immutable). There is no currency setting on a company, a tenant, a product form or an order form. A record without a company, and a record of a company that has no book, is IDR, as today.
2. **Currency is decided when the record is created and never changes.** A product, sales order, supplier offer, purchase order and supplier document take their company's trade currency at insert. The database refuses a row whose currency differs from that rule, and the existing immutability triggers keep it.
3. **One currency per document chain.** An order's lines must be products of the order's currency. An invoice takes the currency of its order, a supplier document that of its purchase order (both already enforced). Nothing converts between currencies; there is no exchange rate anywhere in this design.
4. **Amounts are integers in the record's source unit.**
   - IDR records keep the existing unit, whole Rupiah (`whole_idr`): `150000` is Rp150.000. The ledger stores IDR with two decimals, so the source bridges keep multiplying by 100.
   - Records in any other currency are in that currency's ISO minor unit (`minor`): `1999` is USD 19.99, `1500` is JPY 1500, `1250` is KWD 1.250. The source bridges post these amounts unchanged.
   Existing rows are not rewritten. The asymmetry is deliberate: converting stored Rupiah to minor units would change every amount in every installed database and the payment gateway contract.
5. **A posting policy names its source unit and the bridge checks it.** `whole_idr` is valid only on an IDR book, `minor` only on a non-IDR book. A source whose currency differs from the book's is refused at capture, not skipped.
6. **Money is shown with its currency and decimals.** Screens, mail and imports format and parse an amount from the record's currency and the ISO exponent in `config/accounting-currencies.v1.json`. IDR keeps its current whole-Rupiah display and input.
7. **The payment gateway stays IDR-only.** Midtrans accepts Rupiah. A gateway link for a non-IDR invoice is refused with a stated reason; manual and bank settlement work in any currency.

## Not covered

- Foreign-currency documents inside one book (an IDR company invoicing in USD), exchange rates, revaluation and realised gains.
- Consolidation across companies with different currencies.
- Tax of any country. Changing currency makes no tax claim.
- Accommodation, which has no company and no ledger bridge, stays IDR.
- A company that traded in IDR before its non-IDR book was created. Its earlier records stay IDR and cannot be posted to that book; the policy guard refuses them.

## Known consequences

- **Create the book before the company's first product.** Products a company created before its non-IDR book stay IDR. New sales orders are in the book currency, so those products can no longer be sold (the line guard refuses them) and must be created again.
- **A product inserted while the book is being created can commit as IDR.** The insert guard takes no lock on the book. The result is the state above, not a wrong amount.
- **Until stage 2, purchasing and foodservice refuse a non-IDR product**, so no IDR purchase document can be built over one.
- **The insert guards have no startup readiness check**; the migration checksum is the only proof they are installed.

## Stages

| Stage | Scope | State |
| --- | --- | --- |
| 1 | Commerce: products and sales orders take the company's trade currency; database guard; reads return it | implemented on branch `feat/trade-currency-commerce`, not merged: `commerce-pg/011_company_trade_currency.sql` (format check, `trade_currency`, insert guards on products, orders and order lines), the adapter sets the currency of company-owned products and orders, the workspace read returns it. Purchasing still refuses a non-IDR product, and a non-IDR order cannot be invoiced or posted until stages 2-4 |
| 2 | Purchasing: offers, purchase orders and supplier documents | not started |
| 3 | Billing: invoices from orders and manual invoices; gateway refusal reason | not started |
| 4 | Accounting source bridges and policies accept the `minor` unit on non-IDR books | not started |
| 5 | Screens, mail and import format and parse by currency | not started |
| 6 | Browser acceptance of one non-IDR company from sale to posted ledger | not started |
