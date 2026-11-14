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
7. **The payment gateway stays IDR-only.** Midtrans accepts Rupiah. A gateway link for a non-IDR invoice is refused; today the refusal is a generic conflict with no specific reason. Manual and bank settlement work in any currency.

## Not covered

- Foreign-currency documents inside one book (an IDR company invoicing in USD), exchange rates, revaluation and realised gains.
- Consolidation across companies with different currencies.
- Tax of any country. Changing currency makes no tax claim.
- Accommodation, which has no company and no ledger bridge, stays IDR.
- A company that traded in IDR before its non-IDR book was created. Its earlier records stay IDR and cannot be posted to that book; the source bridges refuse them (see the consequences below).

## Known consequences

- **Create the book before the company's first product.** Products a company created before its non-IDR book stay IDR. New sales orders are in the book currency, so those products can no longer be sold (the line guard refuses them) and must be created again.
- **A product inserted while the book is being created can commit as IDR.** The insert guard takes no lock on the book. The result is the state above, not a wrong amount.
- **Foodservice refuses a non-IDR product.** It has no company, so its recipes stay IDR; a company's non-IDR product cannot be an ingredient.
- **Offers and purchase orders follow the same rule as products.** An offer made before the company's non-IDR book stays IDR, like its product, and cannot enter a new purchase order; no new offer can be made on such a product. A purchase order drafted in IDR before the book stays IDR and can still be confirmed and received while the book has no purchase policy.
- **Once a non-IDR book has a posting policy, the company's unfinished IDR records are blocked at the step that would capture them.** Each refusal is a conflict, the operating command is rolled back and nothing is captured. With an invoice policy, an IDR draft invoice cannot be issued. With a purchase policy, an IDR draft purchase order cannot be confirmed. With a stock policy, an IDR sales order cannot be fulfilled or returned. Records that passed that step before the policy existed are not affected and stay outside the ledger: an IDR invoice already issued can be corrected, paid and voided, and an IDR purchase order already confirmed can be received and billed. There is no way to finish the blocked records other than before the policy is set; whether they should be skipped instead is an open question.
- **A supplier document command must name its purchase order's currency.** Any other well-formed code is a conflict, not a validation error. The capture form still pre-fills IDR until stage 5.
- **A non-IDR order confirmed before its purchase policy reaches no ledger.** Confirmation records a purchase origin only when the book has a purchase policy, and the bridge never imports an order confirmed earlier, so its receipts and supplier bills are not captured. The same holds for an invoice issued before the invoice policy.
- **The policy forms still offer only `whole_idr` until stage 5.** The command API accepts `minor`; the operator form catalogue does not offer it yet, so a non-IDR book's policies can only be set through the API.
- **The insert guards have no startup readiness check**; the migration checksum is the only proof they are installed.

## Stages

| Stage | Scope | State |
| --- | --- | --- |
| 1 | Commerce: products and sales orders take the company's trade currency; database guard; reads return it | implemented on branch `feat/trade-currency-commerce`, not merged: `commerce-pg/011_company_trade_currency.sql` (format check, `trade_currency`, insert guards on products, orders and order lines), the adapter sets the currency of company-owned products and orders, the workspace read returns it. Purchasing still refuses a non-IDR product, and a non-IDR order cannot be invoiced or posted until stages 2-4 |
| 2 | Purchasing: offers, purchase orders and supplier documents | implemented on branch `feat/trade-currency-purchasing`, not merged: `purchasing-pg/007_company_trade_currency.sql` (format check on offers, orders and supplier documents; `archava_purchasing.trade_currency`; insert guards on offers, orders and purchase lines), the adapter sets the currency of offers and company purchase orders and locks products in the document's currency, the supplier document command accepts its order's currency, the workspace read returns the offer currency. Receipts write the order's unit cost and landed fee to stock unchanged. A non-IDR purchase is not captured for the ledger until stage 4 |
| 3 | Billing: invoices from orders and manual invoices; gateway refusal reason | implemented on branch `feat/trade-currency-ledger`, not merged: an invoice made from an order takes the order's currency; manual invoices have no company and are unchanged; the gateway link still refuses a non-IDR invoice as a conflict (no specific reason yet) |
| 4 | Accounting source bridges and policies accept the `minor` unit on non-IDR books | implemented on branch `feat/trade-currency-ledger`, not merged: `accounting-pg/014_source_unit_minor.sql` allows `source_unit` `whole_idr` or `minor` on the invoice, purchase and stock policies; the policy guards accept `whole_idr` only on a native IDR book with two minor units and `minor` only on a native book in another currency; the capture and effect functions are replaced in place, take the factor (100 or 1) from the policy snapshot the source is captured under, refuse a source whose currency differs from the book's, and match a provider event in its invoice's currency. The command API accepts both units and refuses a change of unit; the workspace notices name the book's unit. The policy forms still offer only `whole_idr` (stage 5) |
| 5 | Screens, mail and import format and parse by currency | not started |
| 6 | Browser acceptance of one non-IDR company from sale to posted ledger | not started |
