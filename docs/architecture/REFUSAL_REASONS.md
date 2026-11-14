# Refusal reasons

Business rules are enforced by database guards: trigger functions that `RAISE EXCEPTION '<sentence>' USING ERRCODE='23514'`. Without help, every such refusal reaches the caller as the same HTTP 409 with one general sentence per module. The refusal reason catalogue gives the refusals a person plausibly meets a stable code and a sentence that says what was refused and what to do next.

Sources: [catalogue](../../config/refusal-reasons.v1.json), [domain loader](../../crates/domain/src/erp/refusals.rs), [adapter classifier](../../crates/adapters/src/erp/mod.rs), [host response helpers](../../crates/host/src/erp/mod.rs), [drift check](../../tools/check-refusal-reasons.mjs).

## The catalogue

`config/refusal-reasons.v1.json` holds one entry per guard message:

```json
"Resolve draft journals before closing": {
  "code": "period_close_draft_journals",
  "message": "This period still has draft journals. Post or void every draft journal dated in the period, then close it.",
  "message_id": "Periode ini masih punya jurnal draf. …"
}
```

- The **key** is the guard's exact message. Applied migrations are frozen by checksum, so the text is a stable key. The whole message must match: a longer message that contains a key does not.
- `code` is stable snake_case API. Clients may branch on it. Never rename a code; add a new entry instead.
- `message` is English and `message_id` Indonesian, both written for the person using the product: what was refused, then what to do.

The file is one of the public Domain catalogues: `node tools/rust/domain-assets.mjs --write` copies it to `crates/domain/embedded/`, and `archava_domain::erp::refusals` (`for_guard`, `for_code`, `catalogue`) reads it in the adapters, the host and the WASM slice.

## How a refusal gets its reason

1. A guard raises. `purchase_database_error` (and `commerce_database_error`) in `crates/adapters/src/erp/mod.rs` looks the message up when the SQLSTATE is `23514`. A catalogued message becomes `PurchaseError::Refused(&Refusal)`; anything else is classified exactly as before (`Domain(ErpError::Conflict)` for the conflict states, `Database` otherwise).
2. Some rules are decided in Rust before the guard would fire. Those sites call `refused_by_code("<code>")` with the code of the guard they pre-empt (currently: an invoice from an unfulfilled or partly returned order in `billing_invoice.rs`; an offer or purchase line in another currency in `purchasing.rs`; a tax code on a book kept in an external system in `accounting.rs`; in `accounting/tax.rs` a confirmation by the person who entered the code on a book with independent posting, a confirmation of a version that is not waiting for one, a change to a withdrawn or retired version and a retirement without a last day; a sales order tax code without its price basis or the reverse in `commerce.rs`, and a tax code on an order without a company in `commerce_kernel.rs`; an invoice from an order whose tax code has no version that applies on the invoice date in `billing_invoice.rs`; a draft of a taxed invoice source that does not name exactly its receivable, revenue and tax control accounts in `accounting/sources.rs`; in `supplier_documents.rs` a supplier claim with one tax member but not the other, with a tax code that is unknown, an output code, unconfirmed or without a version on the document date, and with a tax amount above the goods total; a draft of a taxed purchase source that does not name exactly its goods received, tax control and payable accounts in `accounting/purchases.rs`; in `billing.rs` a tax invoice reference on an untaxed invoice, on an invoice that is not issued or settled, recorded a second time, or replaced when none is recorded; in `companies.rs` a tax registration statement on a company whose tax id is not digits and separators). An unknown code is a plain conflict.
3. The host answers a `Refused` as the same conflict it always was, plus the reason:

```json
{
  "decision": "refused",
  "reason": "conflict",
  "message": "This period still has draft journals. Post or void every draft journal dated in the period, then close it.",
  "detail": {
    "code": "period_close_draft_journals",
    "message": "This period still has draft journals. Post or void every draft journal dated in the period, then close it.",
    "messageId": "Periode ini masih punya jurnal draf. Posting atau batalkan semua jurnal draf bertanggal dalam periode ini, lalu tutup periodenya."
  }
}
```

The HTTP status (409), `decision` and `reason` are unchanged, so a client that compares `reason == "conflict"` keeps working. `message` is the specific English sentence instead of the module's general one. There is no request locale in the product yet, so both languages travel in `detail` and the client chooses. A refusal that is not catalogued has no `detail` and keeps the general sentence.

A refused command is rolled back with its transaction: no command row or receipt is written, so sending the same request again runs the command again and is refused for the same reason while the state is unchanged.

The operations screens show `detail.message` when the answer has one, otherwise `message` (`message()` in `crates/slice/src/operations/mod.rs`).

## Adding an entry

1. Find the guard: `node tools/check-refusal-reasons.mjs --missing` lists every uncatalogued `23514` message per ledger. Read the guard's condition in the migration, not only its message: a guard often tests several things, and the sentence must be true for all of them.
2. Add the entry to `config/refusal-reasons.v1.json` with a new code and both sentences. Say what was refused and what to do next, in the user's words (no table, trigger or revision vocabulary unless the user sees it).
3. Run `node tools/rust/domain-assets.mjs --write`.
4. Add an assertion on `detail.code` to the integration test that already exercises the refusal (`crates/host/tests/erp_<module>.rs`). If the test shows a plain conflict, the adapter decides the rule before the guard: name the code there with `refused_by_code`, or leave it and say so.
5. Run `node --test tools/test/refusal-reasons.test.mjs` and `cargo test -p archava-domain --lib refusals`.

## Convention for new migrations

- A guard a user can reach raises with `USING ERRCODE='23514'` and a fixed sentence without format arguments (`%`). A message built with arguments has no stable text and cannot be catalogued.
- One meaning per message. Reusing a sentence for a different rule in another ledger gives both the same catalogue entry.
- A changed rule gets a new message in a new migration; the old key stays valid while its migration exists. If a later migration replaces a guard function and drops a message, remove or re-key the entry: the drift check only proves that the text still appears in `migrations/`, not that a live function still raises it.
- Add the catalogue entry in the same change as the guard.

## The drift check

`tools/test/refusal-reasons.test.mjs` (run by `node tools/rust/verify.mjs` and `node --test tools/test/`) fails when a catalogue key matches no `RAISE EXCEPTION` message, when a code is duplicated or not snake_case, or when a sentence is missing. It reports, without failing, how many `23514` guard messages of each ledger have no entry. `node tools/check-refusal-reasons.mjs` prints the same report. The domain crate's unit tests check code uniqueness and shape against the embedded copy.

## What is not covered

- **Most guards.** The catalogue starts with the trade-currency, posting-policy, opening-balance, tax-code, sales-tax, purchase-tax, tax-invoice-reference, company-tax-registration, period-close and closed-period refusals of accounting, commerce, purchasing and billing. The rest answer as before.
- **Other SQL states.** Only `23514` is read. Unique, foreign-key, range and lock conflicts (`23505`, `23503`, `22003`, `40P01`, `55P03`) stay general conflicts. Guards that raise without an `ERRCODE` (`P0001`, for example the Billing 005 provenance guards) stay database failures in the adapters; two of them are catalogued only because the billing adapter decides the same rule first.
- **Conflicts decided in Rust.** Stale revisions, missing rows and state checks in the adapters return a plain conflict unless the site names a code.
- **Ledgers with their own error type.** Attendance, subscriptions, support, field services, fulfillment, laundry and formal education map their own SQL errors without the classifier (a guard refusal there is a database failure or a module-specific answer), and convert a `PurchaseError` from the shared commerce kernel into a module error that has no reason variant, so a catalogued refusal arrives as a plain conflict. Mail and payments (`crates/adapters/src/erp/mail.rs`, `payments.rs`) never reach the classifier.
- **Reviewed workflows and ingestion** (`crates/host/src/workflows.rs`) answer `business_conflict` without `detail`.
- **Billing.** `billing::refuses` keeps its historical contract: only three accounting source guards (two of them catalogued) the catalogued tax guard of Billing 009 (no tax code version applies on the invoice date) the three catalogued tax guards of the Accounting 017 capture (the tax code is not a postable output code, its control account is inactive, its control account is a policy account) and the four catalogued tax invoice reference guards of Billing 011 are conflicts; every other billing guard is still a database failure, including the other Billing 009 guards, which no command reaches.
- **The agent.** A refused step reaches the mission runner with this body, but the goal's failure text is still a fixed sentence (`crates/host/src/agent/runner.rs`).
- **Locale.** The host does not choose a language; it returns both.
