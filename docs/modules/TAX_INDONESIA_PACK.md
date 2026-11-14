# Indonesia tax pack

Status: implemented on branch `feat/tax-indonesia-pack`, not merged. **Unverified.** Nobody with tax authority has reviewed anything in this pack. It states nothing about what Indonesian tax law, or the law of any country, requires. Nothing in it is compliant, official or verified, and nothing in it is described that way.

The pack adds fields and templates for the owner's nearer market, Indonesia, on top of the generic [tax on documents](../architecture/TAX.md). Which of them a company uses, and with what values, is the company's decision with its own tax adviser. Its rule is that design's rule 1: tax is the company's data. Every value below is entered by the company and stored as entered, and every screen that shows one says so: *entered by your company; Archava does not verify tax rates or rules*. This page describes what each field stores, not what a tax authority expects in it.

## What the pack stores and offers

| Part | What it is | Where |
| --- | --- | --- |
| Tax code templates | Starting values for the tax code form, per market | [catalogue](../../config/tax-templates.v1.json), [loader](../../crates/domain/src/erp/tax_templates.rs), the Tax codes panel |
| Tax registration identifiers | A company's statement that it is registered for value-added tax; a party's tax id | Organizations migration 005 |
| Tax ids on an invoice | The seller's and the buyer's tax id as they were when the invoice was created | Billing migration 011 |
| Tax invoice reference | A number issued outside Archava, recorded on an invoice | Billing migration 011, `record_tax_reference`, `replace_tax_reference` |
| Documents listing and CSV | The posted taxed documents of a period | `GET /erp/accounting/tax-report/documents` and `…/documents.csv` |

### Tax code templates

`config/tax-templates.v1.json` lists templates per market. Each has an id, a suggested code text, a kind (`output` or `input`), a rate and a base fraction as integer fractions, a label that names the template's own numbers, `verified: false` and a note. The market `ID` (Indonesia) has four:

| Template | Kind | Rate | Base | Suggested code |
| --- | --- | --- | --- | --- |
| `id-output-11` | output | 11/100 | 1/1 | `VAT-OUT-11` |
| `id-input-11` | input | 11/100 | 1/1 | `VAT-IN-11` |
| `id-output-12-base-11-12` | output | 12/100 | 11/12 | `VAT-OUT-12-B11-12` |
| `id-input-12-base-11-12` | input | 12/100 | 11/12 | `VAT-IN-12-B11-12` |

These are examples of numbers, not statements about which rate or base applies to any company or transaction. 12% on 11/12 of a price is the same amount as 11% on the whole price; a domain test checks that through the integer `TaxRule` on a spread of amounts and on both price bases.

A template is data, not a rule:

- Choosing one under **Start a tax code from a template** in the Tax codes panel only pre-fills the existing `tax_code.create` form with the code text, kind, rate and base. The book, the control account, the first day and the reason stay empty for the person to enter, and the code text and every number can be changed before saving.
- The saved command is the ordinary `tax_code.create`. It names no template, stores the version as `entered`, and the version applies only after the separate `tax_code.confirm` (by another person on a book with independent posting). A template never creates or confirms a code.
- There is no action that installs a market's templates together.
- Every template carries `verified: false`, and the picker shows the catalogue's notice: check every value with your own tax adviser.

The picker is offered to a person who holds `tax.manage`, on a book whose currency a market lists (`currencies`; `IDR` for the market `ID`). That list only decides where the picker is shown. A book in another currency sees the Tax codes panel as it was.

Another market is added as data: a new key under `markets` with its name, its currencies and its templates. The catalogue is one of the five public Domain catalogues: after editing it run `node tools/rust/domain-assets.mjs --write`. The domain tests refuse a template that is marked verified, has an invalid fraction or code text, or repeats a code within its market.

### Tax registration identifiers

Stored as entered. No length, checksum or format of any tax authority is known or checked.

**A managed company.** A company already keeps `taxId`: the reviewed copy of its source organization's tax id (Organizations 004), at most 50 characters of any text. It is changed where it lives: `update_organization` on the organization, then `company.refresh_source`. Organizations 005 adds `vatRegistered`, the company's own statement that it is registered for value-added tax, false on every existing company.

`company.set_tax_registration` (permission `companies.write` with `companies.read`): `id`, `expectedRevision`, `vatRegistered`, `reason`. It changes only the statement, in a revision of its own, with the usual receipt, replay, audit row and stale-revision check.

- The statement can be true only while the company's tax id is digits with spaces, dots, slashes or hyphens, starting and ending with a digit. That is the only check on a tax id anywhere. Otherwise the command is refused as `company_tax_registration_needs_tax_id`, and so is a `company.refresh_source` that would bring another kind of tax id into a company whose statement is true: clear the statement first, or correct the organization.
- The statement is a stored field. It does not change how any document is taxed, and no document reads it.

The company profile on the Organizations screen shows the tax id and the statement under **Tax registration**, with the form.

**A business party.** Organizations 005 adds `taxId` to a party: the same reviewed snapshot of its source organization's tax id that a company keeps, exactly as entered (no format check). It is set when the party is registered from an organization and changes only through `party.refresh_source` after the organization changed. It is null for an organization without a tax id and for every party whose source is a CRM contact, because a contact has no tax id. A party registered before this migration has none; `party.refresh_source` at the source revision it already holds copies the organization's tax id, and nothing else, into it. No new party command was added.

### Tax ids on an invoice

When `create_invoice_from_order` creates a taxed invoice it stores `seller_tax_id`, the tax id of the invoice's company, and `buyer_tax_id`, the tax id of its customer party, as the two rows state them at that moment. Both are immutable: a later change of the company's or the party's tax id does not change the invoice, and an invoice created afterwards stores the new values. Either is null when the company or the party has none or when the customer is not a business party. An untaxed invoice stores neither. The database checks at insert that the two values are the company's and the party's, and refuses any later change, also by direct SQL as the serving role.

The workspace read returns `sellerTaxId` and `buyerTaxId`; the invoice screen shows them for a taxed invoice; the invoice mail adds `Seller tax id` and `Buyer tax id` lines when present.

### Tax invoice reference

A taxed invoice that is issued or settled can be given the reference of a tax invoice that was issued outside Archava: free text of 1 to 100 characters, with an optional transaction code of 1 to 10 characters that is the company's own classification. Archava attaches no meaning to either, generates neither, and checks no format.

| Command | Members | Effect |
| --- | --- | --- |
| `record_tax_reference` | `invoiceId`, `expectedRevision`, `reference`, optional `transactionCode` | Sets the first reference |
| `replace_tax_reference` | the same, and `reason` | Replaces the recorded reference |

Both need `billing.settle`, the permission that already governs changes to an issued invoice, and follow the billing command contract: a `commandId` claim, a receipt, replay of the original receipt, one audit row, and a stale `expectedRevision` is a conflict. Each raises the invoice revision by one and changes no amount and no status.

Refusals, each with its [reason](../architecture/REFUSAL_REASONS.md): `invoice_tax_reference_untaxed` (the invoice carries no tax), `invoice_tax_reference_not_issued` (a draft or a voided invoice), `invoice_tax_reference_already_recorded` (a second `record_tax_reference`), `invoice_tax_reference_missing` (`replace_tax_reference` without a recorded reference). Replacing a reference with the same reference and code is a plain conflict.

**History.** The invoice row keeps the current reference, its transaction code and `tax_reference_version` (0 without a reference, 1 once recorded, one more per replacement). The history is the existing immutable billing audit trail: the audit row of `record_tax_reference` states the value it set, and the audit row of `replace_tax_reference` states the value it replaced, the value it set and the reason. No table was added. The serving role cannot remove a reference, skip a version, or update or delete an audit row.

The workspace read returns `taxReference` (`reference`, `transactionCode`, `version`) or null; the invoice screen shows it with the form; the invoice mail adds a `Tax invoice reference` line when one is recorded.

### Documents listing and CSV

`GET /erp/accounting/tax-report/documents?tenant=<tenant>&bookId=<id>&from=<date>&to=<date>[&cursor=<sourceId>][&limit=<1..100>]` lists the sources the [tax report](ACCOUNTING.md#tax-report) adds up, one row per posted taxed source dated in the period. It needs the report's permissions (`tax.read` and `reports.read`, a native book in scope) and uses the report's period: the source date. Nothing is written.

| Member | Meaning |
| --- | --- |
| `sourceId`, `date`, `side`, `effect` | The captured source, its source date, `output` or `input`, and `issue`, `correction`, `void` or `bill` |
| `documentKind`, `documentId`, `documentCode` | `invoice` with the invoice code, or `supplier_document` with the supplier's reference |
| `counterpartyName`, `counterpartyTaxId` | The customer name and the buyer tax id the invoice stored; the supplier name, with no tax id (a supplier document stores none) |
| `companyTaxId` | The seller tax id the invoice stored; null for a supplier document |
| `taxCode`, `taxCodeVersion`, `rate`, `base` | The tax code version the document was taxed with |
| `taxableBaseMinor`, `taxMinor` | Net and tax in ledger minor units, as decimal strings; a correction or a void is negative |
| `taxReference`, `taxTransactionCode` | The reference the invoice carries now, on every row of that invoice |

Rows are ordered by source date and source id. `page` carries `limit`, `hasMore` and `nextCursor`. A captured source that is not posted, an untaxed document and a document that no bridge captured are not listed; the report counts those apart.

`GET /erp/accounting/tax-report/documents.csv?tenant=<tenant>&bookId=<id>&from=<date>&to=<date>` is the same listing for the whole period as a download: `text/csv; charset=utf-8`, file name `tax-documents-<book code>-<from>-to-<to>.csv`, RFC 4180 quoting, CRLF line ends. The first line says what the file is; the second names the columns; amounts are exact decimals of the book's currency.

```csv
"Draft export of data your company entered in Archava. It is not a tax return and is not in any tax authority's import format. Archava does not verify tax codes, rates, tax ids or tax invoice references."
Side,Document type,Effect,Document code,Date,Counterparty,Counterparty tax id,Company tax id,Tax code,Tax code version,Rate,Base,Taxable base,Tax,Currency,Tax invoice reference,Transaction code
output,invoice,issue,INV-TAXED,2026-11-14,"Toko ""Maju"", PT",02.345.678.9-012.000,01.234.567.8-999.000,VAT,1,11/100,1/1,30000.00,3300.00,IDR,010.000-26.00000001,01
output,invoice,correction,INV-TAXED,2026-11-14,"Toko ""Maju"", PT",02.345.678.9-012.000,01.234.567.8-999.000,VAT,1,11/100,1/1,-10000.00,-1100.00,IDR,010.000-26.00000001,01
input,supplier_document,bill,PRINTED,2026-11-14,Explicit supplier master,,,VAT-IN,1,11/100,1/1,140000.00,15401.00,IDR,,
```

A text cell that starts with `=`, `+`, `-` or `@` gets a leading apostrophe, so a spreadsheet does not read it as a formula. A period with more than 20,000 documents is refused as a conflict; export it in parts. The Tax report panel lists the documents under the report and links the download.

## What the pack does not do

- It does not file, submit or transmit anything, and it connects to no tax authority system. It does not emulate one, and the CSV is not in any authority's import format.
- It does not issue, generate, reserve or validate tax invoice numbers, and it produces no QR code.
- It does not verify a tax id, a rate, a base or a template, and it does not decide whether a company must register or charge tax.
- No withholding tax, luxury-goods tax, tax per line, payroll tax or tax on payments. The limits of the [generic design](../architecture/TAX.md#not-covered) stand.
- No template for another market yet.
- The supplier's tax id and the supplier's tax invoice number are not stored on a supplier document.
- A customer who is a CRM contact, or free text on a manual order, has no tax id to store.

[tax_indonesia.rs](../../crates/domain/src/erp/tax_indonesia.rs) is not part of the pack. It is an unused module of unverified `f64` reference formulas for payroll withholding and social contributions. Its value-added tax arithmetic and the invoice-shaped struct with an unvalidated QR payload were removed with this pack; no code calls what remains.

## Verification

- Domain: [tax_templates.rs](../../crates/domain/src/erp/tax_templates.rs), and the command grammars in [billing.rs](../../crates/domain/src/erp/billing.rs) and [companies.rs](../../crates/domain/src/erp/companies.rs).
- Router and PostgreSQL: [tax_documents.rs](../../crates/host/tests/erp_accounting/tax_documents.rs) (the tax ids on an invoice, the reference and its history, the listing and the exact CSV bytes, the refusals and direct SQL as the serving role) and [tax_registration.rs](../../crates/host/tests/workspace_flows/tax_registration.rs) (the company statement and the party tax id).
- Screens: unit tests beside the Tax codes panel, the company profile, the invoice tax block and the documents listing.
- Not run: the browser gate and the upgrade proof. `tools/e2e/prior-native.mjs` names the two new migrations and asserts that an upgrade fills none of the new columns, but it was not executed.

## Open questions

- **One tax id or two on a company.** The pack reuses the company's existing tax id (the organization's, any text) and adds only the statement. A separate, format-checked registration number entered on the company itself would not depend on the organization, at the price of two identifiers.
- **A tax id for a customer who is a person.** A party from a CRM contact has none. Either CRM contacts gain a tax id, or a party gets its own field with its own command.
- **The supplier side.** The listing has no tax id and no tax invoice number for a supplier bill. Storing them needs a Purchasing migration and members on `document.capture`.
- **Which company revision the seller tax id comes from.** It is the company's current tax id when the invoice is created, not the company revision the order pinned.
- **History as audit text.** The earlier reference is in the audit row's reason, as words. A table of reference versions would make it structured, at the cost of a table, a grant and a write-fence registration.
- **Who may record a reference.** `billing.settle` was reused; a permission of its own would let a tax clerk record references without the right to issue or void.
- **Who may read the listing.** It shows document codes, counterparty names and tax ids under the Accounting grants `tax.read` and `reports.read`, without the Billing or Purchasing read grants, like the report's outside-ledger counts.
- **Where the picker is shown.** By book currency. A company's market is not its currency; a market field on the company or the book would be the precise answer.
- **A reference per correction.** One reference per invoice is repeated on the rows of its corrections. A credit that has its own externally issued number has nowhere to keep it.
