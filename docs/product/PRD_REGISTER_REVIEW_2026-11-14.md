# Delivery register review, 2026-11-14

Review record for [the whole-PRD delivery register](PRD_DELIVERY_REGISTER.v1.json) under release profile `id-idr`. It assigns each of the 35 requirements a status from the clauses of its requirement text and records, for every clause, what in the repository asserts it.

## Status of this record

| | |
| --- | --- |
| Date | 2026-11-14 |
| Commit described | `85620b8fb9c69a96aa0d85697041d8665b12e78e` (`origin/main`, the merge of pull request #52) |
| Profile | `id-idr` (Indonesia, IDR); `global` is noted per requirement |
| What "proven" means | A named test or browser check asserts the clause, as read in its source. It does not mean the reviewer ran it. |
| Tests opened | 50 distinct host tests were opened and their assertions read, plus the pass lists of the browser files `tax.mjs`, `owner-configuration.mjs`, `store-finance-cycle.mjs`, `load.mjs` and the header of `ops.mjs`. Entries marked `(opened)` or `(PASS text read)` are those; entries marked `(not opened)` were located by name, from the owner test map or from the module documents. |
| Test results | Nothing was run for this review. At the time of writing the main-branch CI run for the commit was still in progress; the runs for #50 and #51 were green, and the daily security audit passed after #51. CI runs the Rust suite and `tools/e2e/verify.mjs` without the opt-in switches (`ARCHAVA_E2E_OPS_ROOT`, `_LOAD`, `_MEDIA`, `_AGENT_RUNTIME`, `_AGENT_LIVE`, `_PRODUCT`), so checks that need them are not in CI. |
| Added after the first pass | Browser checks for the manual receipt screen and the Indonesia tax pack ([`tools/e2e/receipts.mjs`](../../tools/e2e/receipts.mjs), `RECEIPT-native-…` and `TAXPACK-native-…`) merged after this review was first written. The two clauses they cover are marked proven; the PASS texts were read, the checks were not run by the reviewer. |
| Clause counts | **Clause counts are not a percentage of product completeness.** The reviewer chose the clauses (about 7.5 per requirement); a clause can be one assertion or a month of work, and the same fact can appear under two requirements. |

## 1. Counts

Requirements by status:

| | accepted | partial | open |
| --- | ---: | ---: | ---: |
| Register before this review | 0 | 24 | 11 |
| Register after this review | 0 | 33 | 2 |

Status changes: FIN-06 open to partial, OWN-01 open to partial, OWN-02 open to partial, OWN-03 open to partial, OWN-04 open to partial, OWN-05 open to partial, OWN-06 open to partial, OWN-07 open to partial, OWN-08 open to partial. Nothing is `accepted`: no requirement has a dated acceptance record for a named commit, and every one still has at least one absent, built-unproven or outside-the-repo clause except where stated below.

| Clauses (261 in total) | proven | built, unproven | absent | outside the repo |
| --- | ---: | ---: | ---: | ---: |
| All 35 requirements | 149 | 39 | 52 | 21 |

Of the clauses the reviewer separated, a test asserts about 57 percent, which is a statement about these clauses and not about the product.

Per requirement:

| Id | Register before | After | proven | built, unproven | absent | outside | Section |
| --- | --- | --- | ---: | ---: | ---: | ---: | --- |
| DAT-01 | partial | partial | 6 | 2 | 0 | 1 | [DAT-01](#dat-01) |
| DAT-02 | partial | partial | 7 | 0 | 2 | 0 | [DAT-02](#dat-02) |
| FIN-01 | partial | partial | 6 | 0 | 2 | 1 | [FIN-01](#fin-01) |
| FIN-02 | partial | partial | 7 | 0 | 3 | 0 | [FIN-02](#fin-02) |
| FIN-03 | partial | partial | 9 | 1 | 3 | 1 | [FIN-03](#fin-03) |
| FIN-04 | partial | partial | 7 | 0 | 4 | 1 | [FIN-04](#fin-04) |
| FIN-05 | partial | partial | 8 | 0 | 2 | 1 | [FIN-05](#fin-05) |
| FIN-06 | open | partial | 8 | 1 | 4 | 1 | [FIN-06](#fin-06) |
| AGT-01 | partial | partial | 5 | 0 | 2 | 1 | [AGT-01](#agt-01) |
| AGT-02 | partial | partial | 5 | 2 | 1 | 0 | [AGT-02](#agt-02) |
| AGT-03 | partial | partial | 3 | 2 | 1 | 0 | [AGT-03](#agt-03) |
| AGT-04 | partial | partial | 2 | 1 | 2 | 1 | [AGT-04](#agt-04) |
| AGT-05 | partial | partial | 3 | 0 | 2 | 0 | [AGT-05](#agt-05) |
| AGT-06 | partial | partial | 5 | 1 | 2 | 1 | [AGT-06](#agt-06) |
| CFG-01 | partial | partial | 3 | 2 | 1 | 0 | [CFG-01](#cfg-01) |
| CFG-02 | partial | partial | 4 | 3 | 0 | 0 | [CFG-02](#cfg-02) |
| CFG-03 | partial | partial | 3 | 3 | 0 | 0 | [CFG-03](#cfg-03) |
| UX-01 | open | open | 1 | 1 | 3 | 1 | [UX-01](#ux-01) |
| UX-02 | partial | partial | 3 | 3 | 0 | 1 | [UX-02](#ux-02) |
| UX-03 | partial | partial | 3 | 0 | 1 | 1 | [UX-03](#ux-03) |
| INT-01 | partial | partial | 4 | 0 | 2 | 1 | [INT-01](#int-01) |
| INT-02 | partial | partial | 2 | 2 | 2 | 1 | [INT-02](#int-02) |
| SEC-01 | partial | partial | 6 | 0 | 1 | 1 | [SEC-01](#sec-01) |
| SEC-02 | partial | partial | 4 | 1 | 1 | 1 | [SEC-02](#sec-02) |
| OPS-01 | partial | partial | 2 | 2 | 1 | 2 | [OPS-01](#ops-01) |
| OPS-02 | partial | partial | 1 | 1 | 1 | 1 | [OPS-02](#ops-02) |
| EXT-01 | open | open | 2 | 0 | 1 | 2 | [EXT-01](#ext-01) |
| OWN-01 | open | partial | 3 | 1 | 1 | 0 | [OWN-01](#own-01) |
| OWN-02 | open | partial | 2 | 1 | 2 | 0 | [OWN-02](#own-02) |
| OWN-03 | open | partial | 4 | 1 | 2 | 0 | [OWN-03](#own-03) |
| OWN-04 | open | partial | 2 | 4 | 2 | 0 | [OWN-04](#own-04) |
| OWN-05 | open | partial | 5 | 1 | 1 | 0 | [OWN-05](#own-05) |
| OWN-06 | open | partial | 6 | 0 | 0 | 0 | [OWN-06](#own-06) |
| OWN-07 | open | partial | 3 | 2 | 0 | 0 | [OWN-07](#own-07) |
| OWN-08 | open | partial | 5 | 1 | 0 | 0 | [OWN-08](#own-08) |

Requirements with **no outside-the-repo clause**, where in-repo work alone could make them acceptable: DAT-02, FIN-02, AGT-02, AGT-03, AGT-05, CFG-01, CFG-02, CFG-03, OWN-01, OWN-02, OWN-03, OWN-04, OWN-05, OWN-06, OWN-07, OWN-08.

Requirements whose scope the owner could narrow instead of building (each needs a recorded owner scope decision to become `not_applicable`; none is proposed): UX-03 if no Presence mode is sold under `id-idr`; AGT-05 (priority P1); the requisition and tolerance clauses of FIN-02 (the PRD says "sesuai scope"); the external-accounting clause of FIN-01 if the owner chooses native GL; MFA and SSO in SEC-01 if the profile does not require them.

Could be accepted for `id-idr` but not for `global`, once their in-repo work is done: DAT-02, FIN-02 and FIN-03 (for `global` they additionally need foreign-currency documents and a payment channel that is not IDR-only: the Midtrans gateway refuses non-IDR invoices, while the manual receipt path works in any book currency); INT-01 and INT-02 (gateway and import are IDR-only); FIN-06 (the engine is generic, but only Indonesia has fields and templates, and no other country is named). The OWN, CFG, AGT, SEC and OPS requirements do not depend on the profile.

## 2. In-repo work with the most leverage

Ordered by how many requirements it moves. Each item removes absent or built-unproven clauses; none removes an outside clause.

1. **Owner-configuration closure** (OWN-05, OWN-07, OWN-08 and CFG-02 fully; OWN-01 to OWN-04 and CFG-01 need design decisions first). Concretely: plan and confirmation expiry with a test (OWN-05); a browser check that applies, restarts the server and recovers the same receipt, and one for an `unknown` pending id (OWN-07, CFG-02); assertions on the history screen rows in `tools/e2e/owner-configuration.mjs` (OWN-08); an HTTP test that forged `readiness`, `cost` and `price` on `/own/plan` are refused, a test that an agent goal against a disabled module is refused, and a browser check that a disabled ERP module loses its navigation entry (OWN-03, CFG-02); real readiness (`needs_setup` when a module has no book, policy or tax code), module permissions, records to settle, disruption and a recovery path in the preview (OWN-01, OWN-03, OWN-04, CFG-01); a stored business profile and a clarifying-question loop (OWN-01, OWN-02).
2. **One acceptance record for a named commit** (a prerequisite for every `accepted`; it is what OWN-06 lacks). Re-run and record the full Rust gate and browser gate on the commit, the opt-in upgrade proof (`ARCHAVA_E2E_OPS_ROOT` with a previous package), the cold and persistent consumer scripts (`tools/rust/cold-consumers.mjs`, `tools/rust/persistent-consumers.mjs`), and the load and agent-runtime suites. The last dated acceptance records are from 2026-11-12 and earlier.
3. **Agent control package** (AGT-02, AGT-03, AGT-05, CFG-02). A semantic-mismatch oracle and a prompt-injection-through-data test; a disabled-module refusal test; a written mapping from the PRD statuses to goal states with `partial` and `reconciled` made explicit; a cancel-after-commit test; two more proactive triggers (overdue receivables from the aging report, period-close blockers) with the same review path and a stop and resume switch.
4. **Sales settlement completion** (FIN-03, and it shrinks FIN-04, FIN-05 and DAT-01). A customer payment that does not come through the gateway is now recorded (manual customer receipts, merged as #52, with reversal, taxed, USD and cash cases). What remains: one payment across several invoices, customer credit and unapplied cash that a later invoice can use, credit notes as documents, and a dedicated cash book.
5. **Purchasing and unit rules** (FIN-02, DAT-02). Three-way-match tolerance with hold for review; an explicit decision on requisition; foreign-currency documents inside one book (the importer's case); fractional quantity and unit of measure, or an explicit whole-unit statement; a tenant time zone.

Next in line, each shrinking several requirements without completing any alone: finance close completion (dated stock valuation against the ledger, year-end close, counted-stock source, opening documents: FIN-01, FIN-05, FIN-06, INT-02); the held-out evaluation set and a recorded live run (AGT-01, AGT-04, AGT-06); user and grant administration and the MFA decision (SEC-01, UX-01); browser checks for the receipt screen and the Indonesia tax pack (FIN-03, FIN-06); retention, export and delete of agent history (SEC-02); a cost and availability report from stored model attempts (AGT-06); restore-time measurement and an alert-fire test (OPS-01).

## 3. What only the owner or an outside party can provide

**Production host and domain** (OPS-01, OPS-02, INT-01, UX-03, SEC-01): a chosen host, a public HTTPS origin for payment notifications, a clean-host restore and disaster-recovery drill, production hardware for load and sizing, TURN and network conditions for live media. The register records production as `deferred_by_owner_until_website`.

**Pilot users** (UX-01, UX-02, UX-03, DAT-01, FIN-04, FIN-05, EXT-01, AGT-01, INT-02): a real new owner for onboarding timing, a real finance user reconciling a real month, real customers, suppliers and items, a real migration source, user evidence per offered industry capability.

**A tax professional or accountant** (FIN-06, FIN-05, FIN-01, DAT-01 for the books): review of tax codes, templates, posting rules, CSV columns and the chart of accounts; acceptance of the ledger as an official book; confirmation of what PPh, e-Faktur and Coretax need if they are promised. The independent review of the tax path that merged as #50 read the code; it was not a review by a tax professional.

**Provider and payment accounts** (AGT-01, AGT-04, AGT-06, SEC-02, INT-01, FIN-03, FIN-04, UX-03): a Gemini key with enough quota (and a second provider if one is promised); a Midtrans merchant account (sandbox, then production); real bank statement sample files from the Indonesian bank or banks the pilot uses; a mail provider; LiveKit and Gemini Live accounts for live voice.

**Legal and commercial decisions** (FIN-01, FIN-04, EXT-01, OWN-04, CFG-01, OPS-01, OPS-02, SEC-01, SEC-02, INT-01, FIN-06): the accounting source for `id-idr` (native GL is the engineering recommendation); offered industry profiles; the pricing and entitlement model (so cost and readiness in the owner preview can be real); RTO, RPO and SLA targets; data-protection, retention and residency policy and provider terms; whether MFA or SSO is required; which integrations are promised first; which tax features are promised (withholding, e-Faktur filing); which Presence modes are sold.

## 4. The release profile

`tools/product/check-delivery.mjs` computes `profileReady` as `releaseProfile.status === "accepted"` and reads nothing else of the profile: not `profiles[]`, `openBeforeAcceptance`, `accountingSource`, `offeredIndustryProfiles` or `productionDeployment`, and the `profileId` of an accepted requirement is only checked for being present. The register's `releaseProfile.status` is `profiles_selected`, so profile acceptance would be a human assertion. What the register's own lists say must be true first, and where this review changed them:

| Item | State at this commit |
| --- | --- |
| `id-idr` accounting source undecided | Still true (`accountingSource` is null). Owner decision. |
| `id-idr` tax | Was "no transaction carries Indonesian tax". Sales orders, invoices and supplier documents now carry company-entered tax, post to control accounts and report (#44 to #49, reviewed in #50; `docs/architecture/TAX.md`, `docs/modules/TAX_INDONESIA_PACK.md`). Still true: no rule or rate has been reviewed by a tax professional, withholding and payroll taxes are not built (the formulas in `crates/domain/src/erp/tax_indonesia.rs` are used by no module), and there is no tenant time zone, so document and tax dates are UTC calendar dates. |
| `id-idr` production host, pilot user, priority integration | Still true. |
| `global` currency constraints | Was "a non-IDR company cannot sell, buy or post sources". Outdated for trade: a company whose book is not IDR can trade, invoice, record manual receipts and post sources (#35 to #40 and #52; `docs/architecture/MULTI_CURRENCY.md`; `CURRENCY-native-*` browser checks). Still IDR-only: the payment gateway, product and opening-stock import, accommodation and foodservice. There is no exchange rate or foreign-currency document inside a book, and a company that already trades cannot open a non-IDR book. |
| `global` country, tax regime, first currency | Still true: none is named. |
| `global.status` and `PRD.md` section 5 | Both said the `global` profile could not be built. Corrected together with this review: the status is now `selected`, and section 5 states that a non-IDR company can trade, invoice and post while the payment gateway, import, accommodation and foodservice remain IDR-only and no exchange rate exists. |
| Profile definition in `PRD.md` section 5 | A profile defines customer types, personas, country, legal entity, currency, mandatory processes, data volume, concurrency, integrations, SLO and environment. `id-idr` names market, jurisdiction and currency only. |
| `offeredIndustryProfiles`, `productionDeployment` | `[]` and `deferred_by_owner_until_website`; owner decisions needed for EXT-01, OPS-01 and OPS-02. |
| `inheritedRequirements` | `requires_traceability`: 89 inherited items in `PRD_INHERITED_TRACEABILITY.v1.json`, 74 `partial` and 15 `open`. Separate from `profileReady` but required for `wholeProductComplete`. |
| `acceptanceSuites` | `OWN-T01..OWN-T10` is mapped in `OWNER_ACCEPTANCE_TEST_MAP.md`; `B01..B07` are still proposed protocols. Both entries are left unchanged by this review. |

## 5. Statements elsewhere that are out of date

Found while reading; this review changes none of them.

- `docs/modules/ACCOUNTING.md`, "Still missing: ... tax, ...": tax is delivered (the same page has a "Tax codes" section and a tax report).
- `docs/product/PRD.md` section 5 says profile `global` cannot be built until transactions are no longer limited to IDR; a non-IDR company can now trade from sale to posted ledger.
- The owner catalogue (`crates/host/src/own.rs`) marks every module entitled, installed, `Ready` and `Included`, with pricing `included@1`. Every owner test that asserts readiness or cost asserts a constant; this is easy to over-read.
- The register's previous `remaining` text repeated one boilerplate sentence ("Requirement-specific mapped execution and selected-profile acceptance remain ...") for about twenty requirements; it named no work. It is replaced.
- The register cited `PRD_AGENT_CONTEXT_FINDING_2026-11-08.md` (an Ava byte-budget finding) as evidence for OWN-04, OWN-07, OWN-08 and SEC-01, and two agent findings for OPS-01; none supports those requirements and they are dropped. CFG-01 and CFG-02 listed the specification `OWNER_CONFIGURATION_PRD.md` as their only evidence; it is dropped.

## 6. OWN-01 to OWN-08 and the OWN-T01 to OWN-T10 scenarios

OWN-01 to OWN-08 are the eight stages of the owner flow in [the owner configuration PRD](OWNER_CONFIGURATION_PRD.md) section 4 (inspect, discover, plan, preview, confirm, apply, recover, history and restore). OWN-T01 to OWN-T10 are the ten proof scenarios of its section 9, recorded as `partial` in [the inherited traceability inventory](PRD_INHERITED_TRACEABILITY.v1.json). `PRD.md` maps CFG-01 to OWN-T02, T03, T05 and T09, CFG-02 to OWN-T04, T06, T07, T08 and T10, and CFG-03 to OWN-T01. No table maps OWN-01 to OWN-08 to the scenarios; the mapping below is the reviewer's, from the clause text, and [the owner acceptance test map](OWNER_ACCEPTANCE_TEST_MAP.md) gives the test per scenario clause.

| Requirement | Main proof scenarios | Clauses those scenarios do not reach |
| --- | --- | --- |
| OWN-01 inspect | T10 inspect, T04 (a, b, c), T01 (c) | business profile, real unmet needs |
| OWN-02 discover | T02 (d), T05 (d) | questions, model discovery |
| OWN-03 plan | T02, T05, T07 (e, f), T09 (f) | compatibility, data lifecycle, real readiness and price, HTTP-level forged readiness and cost |
| OWN-04 preview | T03 (a to f), T05 (a) | records to settle, disruption, recovery path, cost amounts |
| OWN-05 confirm | T04 (d, e), T06 (c), T09 (a to d) | expiry |
| OWN-06 apply | T04 (f, g), T06 (a, b, d), T07 (a to d, f), T10 confirm | none found |
| OWN-07 recover | T06 (d, e, f, g), T04 (g, h) | real restart in the browser, `unknown` id in the browser |
| OWN-08 history and restore | T08 (a to g), T10 history and restore | history row content in the browser |

## 7. Requirements

Each section gives the requirement text as in the register, the status after this review, a clause table and the `remaining` list written to the register. In a clause table, "proven" means a test asserts the clause; evidence entries are marked `(opened)` or `(PASS text read)` when the reviewer read their assertions and `(not opened)` when they were cited by name only.

### DAT-01

**Requirement.** Customer, supplier, item, entity, dokumen dan external reference memiliki identitas stabil serta hubungan yang tervalidasi. Dari order dapat ditelusuri fulfillment, invoice, payment dan accounting; nama bebas tidak menjadi satu-satunya penghubung

Register before: `partial`. After: **`partial`**. Clauses: 6 proven, 2 built-unproven, 0 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Customer and supplier identity is a stable id with reviewed versions | proven | [`crates/host/tests/workspace_flows/parties.rs`](../../crates/host/tests/workspace_flows/parties.rs)`::party_registry_keeps_contact_and_organization_identities_distinct_with_independent_roles` (not opened); [`crates/host/tests/workspace_flows/customer_identity.rs`](../../crates/host/tests/workspace_flows/customer_identity.rs)`::customer_identity_survives_same_names_renaming_fulfillment_invoice_and_replay` (not opened); [`tools/e2e/parties.mjs`](../../tools/e2e/parties.mjs) `PARTY-role-lifecycle-stable-id` (not opened) | Same-name records and renames are covered. |
| Company (legal entity) identity is explicit, immutable and versioned | proven | [`crates/host/tests/workspace_flows/companies.rs`](../../crates/host/tests/workspace_flows/companies.rs)`::managed_company_identity_is_explicit_distinct_and_keeps_reviewed_version_history` (not opened); [`tools/e2e/companies.mjs`](../../tools/e2e/companies.mjs) `COMPANY-distinct-identities-and-source-uniqueness` (not opened) |  |
| Item identity (product id separates stock and FIFO from SKU) | proven | [`crates/host/tests/workspace_flows/company_trade.rs`](../../crates/host/tests/workspace_flows/company_trade.rs)`::company_trade_keeps_purchase_stock_sales_cost_and_invoice_in_one_company` (not opened); [`docs/product/PRD_COMPANY_TRADE_ACCEPTANCE_2026-11-07.md`](PRD_COMPANY_TRADE_ACCEPTANCE_2026-11-07.md) | The acceptance record describes the test; its body was not opened. |
| Relations are validated; the database refuses repointing and forged snapshots | proven | [`crates/host/tests/workspace_flows/customer_identity.rs`](../../crates/host/tests/workspace_flows/customer_identity.rs)`::customer_identity_database_prevents_repointing_and_false_snapshots` (not opened); [`crates/host/tests/workspace_flows.rs`](../../crates/host/tests/workspace_flows.rs)`::order_invoice_database_rejects_repointing_and_incomplete_source_sheets` (not opened); [`crates/host/tests/workspace_flows/supplier_documents.rs`](../../crates/host/tests/workspace_flows/supplier_documents.rs)`::supplier_documents_database_refuses_identity_capacity_and_fabricated_history` (not opened) |  |
| External references keep identity (gateway order, vendor reference, provider events) | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::unknown_creation_keeps_original_order_and_blocks_a_fresh_link_even_after_replay` (not opened); [`crates/host/tests/workspace_flows/supplier_documents.rs`](../../crates/host/tests/workspace_flows/supplier_documents.rs)`::supplier_documents_later_receipt_requires_new_review_and_replay_keeps_held_history` (not opened) | Only the gateway and supplier-reference cases exist; a manual receipt carries a payer reference but it is free text. |
| Each hop order, fulfilment, invoice, payment, accounting source is linked and asserted | proven | [`tools/e2e/store-finance-cycle.mjs`](../../tools/e2e/store-finance-cycle.mjs) `STORECYCLE-actual-sale-invoice-and-verified-collection`, `STORECYCLE-exact-one-book-return-refund-credit-oracle` (PASS text read); [`crates/host/tests/erp_accounting/invoice_sources.rs`](../../crates/host/tests/erp_accounting/invoice_sources.rs)`::real_invoice_sources_post_exact_ar_revenue_clearing_correction_refund_and_void` (not opened); [`crates/host/tests/erp_accounting/stock_sources.rs`](../../crates/host/tests/erp_accounting/stock_sources.rs)`::actual_stock_costs_restock_original_accounts_after_policy_change_and_post_separately` (not opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_bank_transfer_settles_the_invoice_and_reaches_bank_cash_through_clearing` (opened) | The journey runs the chain in one book and asserts 8 captured and 8 posted sources; a manual receipt follows the same invoice to collection to journal to bank chain. |
| A user can open the whole trace from the order (one screen or query that follows every hop) | built, unproven | none found | Links exist in the data (an invoice id on each source, journal sources), but no test asserts a trace read or screen from order to journal. |
| Free-text names are never the only link | built, unproven | [`crates/host/tests/workspace_flows/customer_identity.rs`](../../crates/host/tests/workspace_flows/customer_identity.rs)`::customer_identity_retains_unicode_names_and_does_not_backfill_manual_customers` (not opened) | The party path is proven; the legacy manual-name path is still allowed and deliberately not backfilled. |
| Selected-profile lifecycle on real data | outside the repo | none possible in the repo | Needs a real company's customers, suppliers and items (pilot users). |

**`remaining`:**
- Decide whether orders and invoices that name a customer only by free text stay legal under `id-idr`, and refuse them or add a policy if not.
- Add one asserted trace from an order through its invoice and source to the journal, in a host test or the store finance journey.
- Pilot users reconcile their real customers, suppliers and items against Archava.

**Change to the register:** Register `remaining` ("Authoritative accounting/book/source links") is proven for sales, purchase, stock, gateway and manual-receipt sources and is dropped; evidence gains the finance journey and the invoice source test.

**Profile note:** Same for `global`.

### DAT-02

**Requirement.** Uang, quantity, currency, rounding, waktu dan revisi memakai aturan eksplisit. Transisi menjaga audit, idempotency dan concurrency; koreksi tidak menghapus histori atau mengubah original cost secara diam-diam

Register before: `partial`. After: **`partial`**. Clauses: 7 proven, 0 built-unproven, 2 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Money is exact integer units with an explicit unit per record (whole Rupiah for IDR, ISO minor unit otherwise) | proven | [`docs/architecture/MULTI_CURRENCY.md`](../architecture/MULTI_CURRENCY.md); [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::totals_exceeding_javascript_integer_precision_are_exact_strings_in_all_reports` (not opened); [`tools/e2e/trade-currency.mjs`](../../tools/e2e/trade-currency.mjs) `CURRENCY-native-product-price`, `CURRENCY-native-source-posting` (not opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_receipt_on_a_usd_book_posts_minor_units_unchanged` (opened) | The CURRENCY ids are named from the check's pass list; the ledger test body was not opened. |
| Currency is fixed at insert from the company's book; one currency per document chain; a non-IDR book is refused once the company trades | proven | [`crates/host/tests/erp_commerce.rs`](../../crates/host/tests/erp_commerce.rs)`::a_non_idr_book_is_refused_once_the_company_trades` (opened); [`crates/host/tests/erp_accounting/invoice_sources.rs`](../../crates/host/tests/erp_accounting/invoice_sources.rs)`::a_planted_idr_invoice_under_a_usd_book_is_refused_not_captured` (opened); [`crates/host/tests/erp_purchasing.rs`](../../crates/host/tests/erp_purchasing.rs)`::the_database_refuses_a_purchase_currency_other_than_the_company_book` (not opened) |  |
| Foreign-currency documents inside one book, exchange rates, revaluation | absent | [`docs/architecture/MULTI_CURRENCY.md`](../architecture/MULTI_CURRENCY.md) | An IDR company cannot buy from a USD supplier; this matters to an importer. |
| Rounding is once per document, half away from zero, in the document unit; landed cost allocation conserves every unit | proven | [`crates/host/tests/erp_accounting/tax_sales.rs`](../../crates/host/tests/erp_accounting/tax_sales.rs)`::a_usd_invoice_is_taxed_in_minor_units_with_one_rounding_per_document` (not opened); [`crates/host/tests/workspace_flows.rs`](../../crates/host/tests/workspace_flows.rs)`::landed_cost_fifo_partial_restock_discard_and_restocked_fifo_conserve_every_rupiah` (opened) |  |
| Quantity rules are explicit (positive integers with database bounds) | proven | [`crates/host/tests/erp_commerce.rs`](../../crates/host/tests/erp_commerce.rs)`::concurrent_reservations_cannot_oversell_and_failure_leaves_no_command` (not opened) | The rule is explicit but narrow: integer quantities only, no fractional quantity and no unit-of-measure conversion. |
| Time rule is explicit: every tax date and source date is a UTC calendar date | proven | [`docs/architecture/TAX.md`](../architecture/TAX.md); [`crates/host/tests/erp_accounting/tax_review.rs`](../../crates/host/tests/erp_accounting/tax_review.rs)`::tax_dates_are_utc_dates_and_a_taxed_invoice_cannot_be_back_dated` (not opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_bank_transfer_settles_the_invoice_and_reaches_bank_cash_through_clearing` (opened) | TAX.md "Decisions taken after review" states the rule and the test sets a session in another time zone; a manual receipt is dated the UTC day the money was received. Previously the order and tax code dates followed the database session; that was changed after review. |
| A tenant time zone: a business day at 03:00 in UTC+7 is that day | absent | [`docs/architecture/TAX.md`](../architecture/TAX.md) | A match at 03:00 UTC+7 on the first of a month is dated the last day of the previous month and can be refused as closed. TAX.md lists this as future work. |
| Revision, audit, idempotency and concurrency on transitions | proven | [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_key_reused_for_a_different_command_conflicts_on_execute_and_in_race` (not opened); [`crates/host/tests/erp_billing.rs`](../../crates/host/tests/erp_billing.rs)`::lost_response_replays_the_recorded_receipt` (not opened); [`crates/host/tests/workspace_flows.rs`](../../crates/host/tests/workspace_flows.rs)`::stale_serving_process_cannot_write_after_writer_takeover` (not opened) |  |
| Corrections keep history and original cost | proven | [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::journal_correction_keeps_original_immutable_and_reverses_in_a_new_open_period` (not opened); [`crates/host/tests/erp_accounting/stock_sources.rs`](../../crates/host/tests/erp_accounting/stock_sources.rs)`::actual_stock_costs_restock_original_accounts_after_policy_change_and_post_separately` (not opened); [`tools/e2e/stock-posting.mjs`](../../tools/e2e/stock-posting.mjs) `STOCKPOST-original-return-policy` (not opened) |  |

**`remaining`:**
- Foreign-currency documents and exchange rates inside one book are absent, so an IDR company cannot source from a USD supplier.
- Fractional quantity and unit-of-measure conversion are absent; the product states neither that it sells whole units only nor offers conversion.
- A tenant time zone is absent, so every business day boundary is UTC.
- Document-level opening balances are absent (see FIN-06 and INT-02).

**Change to the register:** Register `remaining` ("Financial currency/rounding/period/opening") is mostly done: currency, rounding, period and declared openings have tests. Evidence gains the multi-currency and money-unit documents.

**Profile note:** Same for `global`.

### FIN-01

**Requirement.** Ledger resmi persisten menerima posting seimbang, source document, periode, pembalikan dan referensi. Jika eksternal, mapping dan status sinkronisasinya dapat direkonsiliasi; library GL saja tidak lulus

Register before: `partial`. After: **`partial`**. Clauses: 6 proven, 0 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Balanced, atomic, exact postings; unbalanced and stale-account drafts refused | proven | [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::stale_accounts_unbalanced_lines_overlap_and_drafts_block_financial_finalization` (not opened); [`tools/e2e/accounting.mjs`](../../tools/e2e/accounting.mjs) `ACCOUNTING-native-decimal-review-and-post` (not opened) |  |
| Source document links: invoice, manual customer receipt, purchase receipt and bill, stock cost, supplier payment, gateway clearing, bank | proven | [`crates/host/tests/erp_accounting/invoice_sources.rs`](../../crates/host/tests/erp_accounting/invoice_sources.rs)`::real_invoice_sources_post_exact_ar_revenue_clearing_correction_refund_and_void` (not opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_bank_transfer_settles_the_invoice_and_reaches_bank_cash_through_clearing` (opened); [`crates/host/tests/erp_accounting/purchase_sources.rs`](../../crates/host/tests/erp_accounting/purchase_sources.rs)`::purchase_receipt_matched_bill_void_post_exact_original_costs_with_independent_review` (not opened); [`crates/host/tests/erp_accounting/clearing.rs`](../../crates/host/tests/erp_accounting/clearing.rs)`::clearing_actual_collection_fee_and_refund_move_cash_once_without_double_ar_or_revenue` (opened) | A manual receipt is a `collection` source dated the day the money was received. |
| Source coverage of every operational event that moves value (counted stock adjustments, subscriptions, assets, accommodation, foodservice, laundry) | absent | [`docs/modules/STOCK_COST_POSTING.md`](../modules/STOCK_COST_POSTING.md); [`docs/architecture/MULTI_CURRENCY.md`](../architecture/MULTI_CURRENCY.md) | Only sales, manual and gateway receipts, purchases, stock fulfilment and restock, supplier payment and bank have bridges. Accommodation has no ledger bridge. |
| Periods: open, non-overlapping, closing blocked by unposted sources, authorized reopen | proven | [`crates/host/tests/erp_accounting/close.rs`](../../crates/host/tests/erp_accounting/close.rs)`::close_readiness_names_each_outstanding_condition_and_agrees_with_the_close` (opened); [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::journal_correction_keeps_original_immutable_and_reverses_in_a_new_open_period` (not opened) | The reopen assertion (reader refused, authorized allowed) is near line 584 of the second test; only that part was read. |
| Reversal keeps the original and allows at most one non-void reversal | proven | [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::journal_correction_keeps_original_immutable_and_reverses_in_a_new_open_period` (not opened) |  |
| References are reserved and tied to their source (`billing:`, `opening:` namespaces) | proven | [`crates/host/tests/erp_accounting/purchase_sources.rs`](../../crates/host/tests/erp_accounting/purchase_sources.rs)`::purchase_source_review_replay_current_authority_and_reserved_journal_namespace_are_enforced` (not opened); [`crates/host/tests/erp_accounting/openings.rs`](../../crates/host/tests/erp_accounting/openings.rs)`::opening_declarations_refuse_wrong_accounts_forged_references_and_edits` (not opened) |  |
| Persistent across restart and restore | proven | [`tools/e2e/verify.mjs`](../../tools/e2e/verify.mjs) `BANK-production-restart`, `STOCKPOST-production-restart` (not opened); [`tools/e2e/ops.mjs`](../../tools/e2e/ops.mjs) `OPS-native-encrypted-restore-and-login` (PASS text read) | The ops check is opt-in (`ARCHAVA_E2E_OPS_ROOT`) and is not in CI. |
| External accounting: mapping and sync status reconcilable | absent | [`docs/modules/ACCOUNTING.md`](../modules/ACCOUNTING.md) | ACCOUNTING.md: an external declaration is configuration only and its connector remains unready. Not applicable if the owner decides native GL for `id-idr`. |
| An accountant accepts the ledger as the official book of a real company | outside the repo | none possible in the repo | Needs a competent accountant and a real company. |

**`remaining`:**
- The owner has not recorded the accounting source for `id-idr` (native GL is the engineering recommendation); once recorded as native, the external-sync clause no longer applies.
- Counted stock adjustments and any other value-moving event the profile offers have no source bridge to the ledger.
- An accountant reviews the chart, the posting policies and one real period.

**Change to the register:** Register `remaining` listed AP/AR payments and aging, bank, opening and subledger close; those are proven and belong to FIN-02 to FIN-05. Only stock adjustments and the profile decision stay here.

**Profile note:** Same for `global`.

### FIN-02

**Requirement.** Requisition/PO sesuai scope, partial/rejected receipt, vendor bill, toleransi three-way match, AP dan supplier payment terhubung; mismatch ditahan untuk review sesuai policy

Register before: `partial`. After: **`partial`**. Clauses: 7 proven, 0 built-unproven, 3 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Requisition | absent | none found | No requisition exists in the domain or module documents; the PRD says "sesuai scope" and the profile has not stated whether requisitions are offered. |
| Offer, purchase order, confirm, receive adds stock exactly once | proven | [`crates/host/tests/erp_purchasing.rs`](../../crates/host/tests/erp_purchasing.rs)`::authorized_flow_changes_stock_exactly_once` (not opened); [`tools/e2e/operations.mjs`](../../tools/e2e/operations.mjs) `D03-purchasing-ui` (not opened) |  |
| Partial, over-acceptance and rejected-only receipts | proven | [`crates/host/tests/erp_purchasing.rs`](../../crates/host/tests/erp_purchasing.rs)`::over_acceptance_refuses_without_stock` (not opened); [`crates/host/tests/erp_purchasing.rs`](../../crates/host/tests/erp_purchasing.rs)`::rejected_only_delivery_adds_no_stock` (opened) |  |
| Vendor bill claim matched to accepted goods; double allocation impossible | proven | [`crates/host/tests/workspace_flows/supplier_documents.rs`](../../crates/host/tests/workspace_flows/supplier_documents.rs)`::supplier_documents_competing_claims_cannot_double_allocate_accepted_goods` (not opened); [`tools/e2e/supplier-documents.mjs`](../../tools/e2e/supplier-documents.mjs) `SUPPLIERDOC-native-correct-and-match`, `SUPPLIERDOC-no-double-billing` (not opened) |  |
| Three-way match tolerance (configurable) | absent | [`docs/modules/SUPPLIER_DOCUMENTS.md`](../modules/SUPPLIER_DOCUMENTS.md); [`docs/architecture/TAX.md`](../architecture/TAX.md) | Matching is exact on quantities, unit prices and net total; TAX.md: no tolerance holds a claim. |
| A mismatch is held for review and can be corrected | proven | [`crates/host/tests/workspace_flows/supplier_documents.rs`](../../crates/host/tests/workspace_flows/supplier_documents.rs)`::supplier_documents_hold_price_total_rejection_and_correct_explicitly` (opened); [`tools/e2e/supplier-documents.mjs`](../../tools/e2e/supplier-documents.mjs) `SUPPLIERDOC-native-held-claim` (not opened) |  |
| AP position, terms, aging and control comparison | proven | [`crates/host/tests/erp_accounting/payables.rs`](../../crates/host/tests/erp_accounting/payables.rs)`::payable_partial_payment_actual_return_and_bank_matching_keep_exact_independent_balances` (opened); [`tools/e2e/due-terms.mjs`](../../tools/e2e/due-terms.mjs) `DUE-native-whole-book-report-and-supplier-terms` (not opened); [`docs/modules/DOCUMENT_DUE_TERMS.md`](../modules/DOCUMENT_DUE_TERMS.md) |  |
| Supplier payment tied to the bill and to bank evidence | proven | [`tools/e2e/supplier-payments.mjs`](../../tools/e2e/supplier-payments.mjs) `PAYABLE-native-payment-review`, `PAYABLE-native-return-final-payment-and-SQL` (not opened); [`crates/host/tests/erp_accounting/payables.rs`](../../crates/host/tests/erp_accounting/payables.rs)`::payable_bank_reservation_blocks_an_unrelated_posted_journal_and_survives_unmatch` (not opened) |  |
| Input tax on the bill posts and is paid in gross | proven | [`crates/host/tests/erp_accounting/tax_purchases.rs`](../../crates/host/tests/erp_accounting/tax_purchases.rs)`::a_taxed_supplier_bill_posts_goods_input_tax_and_a_gross_payable_and_is_paid_in_gross` (opened); [`tools/e2e/tax.mjs`](../../tools/e2e/tax.mjs) `TAX-native-supplier-claim-tax`, `TAX-native-bill-source-posting` (PASS text read) |  |
| Purchases in a currency other than the book | absent | [`docs/architecture/MULTI_CURRENCY.md`](../architecture/MULTI_CURRENCY.md) | No exchange rate exists; see DAT-02. |

**`remaining`:**
- No requisition exists, and the profile has not stated whether requisitions and approval limits are offered.
- No configurable three-way-match tolerance exists, so a small price or quantity difference cannot be accepted within a limit or held beyond it.
- Purchases in a currency other than the company book are absent (see DAT-02).

**Change to the register:** Register `remaining` said supplier payment, terms and aging remain; they are proven. Only tolerance and requisition remain. Evidence gains the due-terms and tax documents.

**Profile note:** Same for `global`, with a larger foreign-currency gap.

### FIN-03

**Requirement.** Order, fulfillment, invoice/credit note, AR, terms/aging, payment/refund dan inventory cost terhubung. Retry atau webhook berulang tidak menggandakan revenue, stok atau pembayaran

Register before: `partial`. After: **`partial`**. Clauses: 9 proven, 1 built-unproven, 3 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Order, reserve, fulfil, return, restock with exact FIFO cost | proven | [`crates/host/tests/erp_commerce.rs`](../../crates/host/tests/erp_commerce.rs)`::physical_order_reservation_fulfillment_partial_return_and_recovery` (not opened); [`crates/host/tests/workspace_flows.rs`](../../crates/host/tests/workspace_flows.rs)`::landed_cost_fifo_partial_restock_discard_and_restocked_fifo_conserve_every_rupiah` (opened) |  |
| Invoice from a fulfilled order, issue, correction, void; posted to AR and revenue | proven | [`crates/host/tests/workspace_flows.rs`](../../crates/host/tests/workspace_flows.rs)`::order_invoice_preserves_frozen_source_and_replays_without_another_invoice` (not opened); [`tools/e2e/invoice-posting.mjs`](../../tools/e2e/invoice-posting.mjs) `INVOICEPOST-native-issuance-source`, `INVOICEPOST-native-credit-source` (not opened) |  |
| Credit note as a document (own number, allocation to invoices) | absent | [`docs/modules/ACCOUNTING.md`](../modules/ACCOUNTING.md); [`docs/modules/BILLING_MODULE.md`](../modules/BILLING_MODULE.md) | A credit is a negative correction on the same invoice. |
| AR terms, due dates and aging with whole-book control comparison | proven | [`crates/host/tests/erp_accounting/subledger.rs`](../../crates/host/tests/erp_accounting/subledger.rs)`::subledger_real_invoice_effects_cutoff_due_history_and_generic_control_difference` (opened); [`tools/e2e/due-terms.mjs`](../../tools/e2e/due-terms.mjs) `DUE-native-source-reset-unknown-and-original-replay` (not opened) |  |
| Customer payment through the verified gateway, partial refund, overpayment retained as unapplied | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::gateway_challenges_unsigned_status_and_conserves_settlement_refunds_and_replays` (opened); [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::verified_overpayment_is_retained_as_unapplied_money_and_refund_first_is_net_only` (not opened); [`tools/e2e/payments.mjs`](../../tools/e2e/payments.mjs) `PAY-UI-verified-settlement`, `PAY-UI-refund` (not opened) |  |
| Customer payment outside the gateway (bank transfer, cash) recorded against an invoice, unverified, reversible, part or full | proven | [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_bank_transfer_settles_the_invoice_and_reaches_bank_cash_through_clearing` (opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::part_payments_settle_at_zero_and_a_receipt_beyond_the_balance_is_refused` (opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_receipt_is_reversed_in_part_and_in_full_and_a_gateway_event_never_is` (opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_receipt_is_refused_with_its_reason_and_stores_nothing` (opened); [`docs/modules/MANUAL_CUSTOMER_RECEIPTS.md`](../modules/MANUAL_CUSTOMER_RECEIPTS.md) | A person with `billing.settle` records the receipt; it settles the invoice, becomes a collection source dated the day the money arrived, and reaches AR, clearing and bank. The receipt and its reversal are asserted, including taxed, USD and cash receipts. |
| Manual and gateway money cannot settle the same invoice twice | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::a_manual_part_payment_leaves_the_rest_to_a_link_and_an_open_link_holds_manual_receipts` (opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::manual_receipts_need_a_person_who_may_settle_and_are_idempotent_and_serial` (opened) | An open unpaid payment link blocks a manual receipt; a link after a manual part payment is for the remainder; racing receipts end with one recorded and one conflict. |
| The manual receipt form and reversal form on the invoice screen | proven | [`crates/slice/src/operations/receipts.rs`](../../crates/slice/src/operations/receipts.rs); [`docs/modules/MANUAL_CUSTOMER_RECEIPTS.md`](../modules/MANUAL_CUSTOMER_RECEIPTS.md) | [`tools/e2e/receipts.mjs`](../../tools/e2e/receipts.mjs) `RECEIPT-native-partial-bank-transfer`, `RECEIPT-native-cash-remainder-settles`, `RECEIPT-native-partial-reversal-reopens`, `RECEIPT-native-reader-sees-no-forms` (PASS text read) drive the forms on the native invoice screen. |
| One payment allocated across several invoices | absent | [`docs/modules/ACCOUNTING.md`](../modules/ACCOUNTING.md); [`docs/modules/MANUAL_CUSTOMER_RECEIPTS.md`](../modules/MANUAL_CUSTOMER_RECEIPTS.md) | A receipt is recorded against one invoice. |
| Customer credit or unapplied cash that a later invoice can use | absent | [`docs/modules/MANUAL_CUSTOMER_RECEIPTS.md`](../modules/MANUAL_CUSTOMER_RECEIPTS.md) | A manual receipt above the outstanding amount is refused; the gateway keeps an overpayment as unapplied money and nothing applies it to another invoice. |
| Inventory cost (COGS) tied to the sale | proven | [`tools/e2e/store-finance-cycle.mjs`](../../tools/e2e/store-finance-cycle.mjs) `STORECYCLE-exact-one-book-return-refund-credit-oracle` (PASS text read); [`tools/e2e/stock-posting.mjs`](../../tools/e2e/stock-posting.mjs) `STOCKPOST-independent-posted-cost-oracle` (not opened) |  |
| Retries and repeated webhooks do not double revenue, stock or payment | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::exact_payment_receipt_recovery_is_not_limited_by_the_recent_history_window` (not opened); [`crates/host/tests/erp_accounting/invoice_sources.rs`](../../crates/host/tests/erp_accounting/invoice_sources.rs)`::source_draft_race_has_one_live_result_and_closed_source_period_rolls_back_invoice` (not opened); [`tools/e2e/operations.mjs`](../../tools/e2e/operations.mjs) `ERP-lost-response-recovery` (not opened) |  |
| Output tax on the invoice, posted and refunded in gross | proven | [`crates/host/tests/erp_accounting/tax_posting.rs`](../../crates/host/tests/erp_accounting/tax_posting.rs)`::a_taxed_invoice_posts_receivable_revenue_and_output_tax_and_is_paid_in_gross` (opened); [`tools/e2e/tax.mjs`](../../tools/e2e/tax.mjs) `TAX-native-exclusive-sale`, `TAX-native-issue-and-source-posting`, `TAX-native-correction-split` (PASS text read); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_receipt_of_the_gross_settles_a_taxed_invoice_and_leaves_the_tax_report_alone` (opened) |  |
| A live payment on a real merchant account | outside the repo | none possible in the repo | Needs a Midtrans merchant account (sandbox, then production) and a public HTTPS origin for notifications. |

**`remaining`:**
- Credit notes as documents with their own number and allocation to invoices are absent.
- One payment allocated across several invoices is absent.
- Customer credit or unapplied cash that a later invoice can use is absent; a manual receipt above the outstanding amount is refused.
- Customer payment channels other than the Midtrans Snap gateway and the manual path are absent, and a live gateway run needs a merchant account and a public HTTPS origin from the owner.

**Change to the register:** Register `remaining` said AR terms and aging remain (they are proven) and listed credit documents. The earlier finding that no manual customer payment path exists is resolved by manual customer receipts; credit notes, allocation across invoices and customer credit remain absent.

**Profile note:** Differs for `global`: the gateway is IDR-only, but a non-IDR company can now record manual receipts (a USD book is asserted), so its payment path is manual only.

### FIN-04

**Requirement.** Bank/cash records, statement import, fee, settlement, matching dan exception dapat direkonsiliasi terhadap saldo serta ledger resmi. Provider payment status saja tidak memenuhi requirement

Register before: `partial`. After: **`partial`**. Clauses: 7 proven, 0 built-unproven, 4 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Bank accounts, signed statement import, exact evidence, replay | proven | [`tools/e2e/bank-reconciliation.mjs`](../../tools/e2e/bank-reconciliation.mjs) `BANK-native-registration-and-policy`, `BANK-native-signed-import-and-replay`, `BANK-native-101-row-import` (not opened); [`crates/host/tests/erp_accounting/bank.rs`](../../crates/host/tests/erp_accounting/bank.rs)`::bank_import_is_atomic_exact_evidence_with_replay_and_separate_read_permission` (not opened) | Import is a typed array of rows with a row limit. |
| Real bank statement formats (BCA, Mandiri, BNI, BRI CSV; MT940; bank APIs) | absent | [`docs/modules/MANUAL_BANK_RECONCILIATION.md`](../modules/MANUAL_BANK_RECONCILIATION.md) | None of the Indonesian bank formats is parsed. |
| Statement correction or replacement | absent | [`crates/host/tests/erp_accounting/bank.rs`](../../crates/host/tests/erp_accounting/bank.rs)`::bank_import_refuses_bad_totals_currency_overlaps_duplicates_and_late_row_edits` (not opened) | An overlapping import is refused; there is no replace path. |
| Matching statement rows to posted cash with partial, unmatch and history | proven | [`crates/host/tests/erp_accounting/bank.rs`](../../crates/host/tests/erp_accounting/bank.rs)`::bank_matches_require_posted_cash_allow_partial_unmatch_and_preserve_history` (not opened); [`tools/e2e/bank-reconciliation.mjs`](../../tools/e2e/bank-reconciliation.mjs) `BANK-native-match-correction-and-reviewer-hold`, `BANK-native-stale-review-refused` (not opened) |  |
| Gateway fee and settlement reach the bank as separate evidence | proven | [`crates/host/tests/erp_accounting/clearing.rs`](../../crates/host/tests/erp_accounting/clearing.rs)`::clearing_actual_collection_fee_and_refund_move_cash_once_without_double_ar_or_revenue` (opened); [`tools/e2e/gateway-clearing.mjs`](../../tools/e2e/gateway-clearing.mjs) `CLEARING-native-fees-refund-and-exact-bank` (not opened) |  |
| A manual bank-transfer or cash receipt reaches bank cash only through clearing against a statement row | proven | [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_bank_transfer_settles_the_invoice_and_reaches_bank_cash_through_clearing` (opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_cash_receipt_waits_in_clearing_until_it_is_settled_against_a_statement_row` (opened) | The receipt waits in the clearing account; a clearing settlement dated on or after the receipt date moves it to bank cash. A cash drawer is a bank account with statement rows entered by hand. |
| A dedicated cash book and automatic matching of statement rows to sources | absent | [`docs/modules/MANUAL_CUSTOMER_RECEIPTS.md`](../modules/MANUAL_CUSTOMER_RECEIPTS.md) | Both are listed as future work; every match is made by a person. |
| Grouped payouts, foreign-currency payouts | absent | [`docs/modules/MANUAL_BANK_RECONCILIATION.md`](../modules/MANUAL_BANK_RECONCILIATION.md) |  |
| Exceptions: unmatched rows block close; a period needs independent attestation | proven | [`crates/host/tests/erp_accounting/bank_periods.rs`](../../crates/host/tests/erp_accounting/bank_periods.rs)`::full_bank_period_requires_coverage_and_independent_current_attestation_before_close` (opened); [`tools/e2e/bank-reconciliation.mjs`](../../tools/e2e/bank-reconciliation.mjs) `BANK-native-close-independent-SQL` (not opened) |  |
| Reconcile against opening and closing balance and the ledger | proven | [`crates/host/tests/erp_accounting/bank_periods.rs`](../../crates/host/tests/erp_accounting/bank_periods.rs)`::changed_cash_evidence_requires_a_new_review_and_backdating_cannot_break_a_closed_bank_period` (not opened); [`docs/modules/MANUAL_BANK_RECONCILIATION.md`](../modules/MANUAL_BANK_RECONCILIATION.md) | The document states the opening and closing balance check; the test body was not opened. |
| Provider status alone does not create bank cash | proven | [`tools/e2e/invoice-posting.mjs`](../../tools/e2e/invoice-posting.mjs) `INVOICEPOST-owned-verified-collection-clearing` (not opened); [`docs/modules/GATEWAY_BANK_CLEARING.md`](../modules/GATEWAY_BANK_CLEARING.md) |  |
| A real bank, real statements, a real finance user | outside the repo | none possible in the repo | Needs a bank account's statements and a finance user. |

**`remaining`:**
- Statement parsers for the Indonesian bank formats the pilot uses are absent and need real sample statement files from the pilot's bank.
- Statement replacement or correction without breaking matches and closed periods is absent.
- Grouped and foreign-currency payouts are absent.
- A dedicated cash book and automatic matching of statement rows are absent.
- A finance user reconciles one real month against a real bank account.

**Change to the register:** Register `remaining` (real bank formats, corrections, grouped and FX payouts, real-user acceptance) is still accurate; its evidence is dated 2026-11-11 and 2026-11-12 and is re-proven by the current tests listed here.

**Profile note:** Same for `global`.

### FIN-05

**Requirement.** Trial balance, laba rugi, neraca, arus kas, ledger detail, subledger dan period close cocok ke sumber; closed period menolak perubahan tidak sah dan membuka kembali melalui policy berizin

Register before: `partial`. After: **`partial`**. Clauses: 8 proven, 0 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Trial balance, P&L, balance sheet, cash flow exact and complete beyond one page | proven | [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::posted_purchase_and_sale_controls_produce_exact_trial_profit_balance_and_cash_reports` (not opened); [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::report_aggregates_include_posted_journals_beyond_every_workspace_page` (not opened); [`tools/e2e/store-finance-cycle.mjs`](../../tools/e2e/store-finance-cycle.mjs) `STORECYCLE-exact-one-book-return-refund-credit-oracle` (PASS text read) |  |
| Ledger detail and history | proven | [`tools/e2e/accounting.mjs`](../../tools/e2e/accounting.mjs) `ACCOUNTING-native-history-and-mobile` (not opened) |  |
| AR and AP subledgers tie to control accounts with an explicit difference, no plug | proven | [`crates/host/tests/erp_accounting/subledger.rs`](../../crates/host/tests/erp_accounting/subledger.rs)`::subledger_real_invoice_effects_cutoff_due_history_and_generic_control_difference` (opened); [`crates/host/tests/erp_accounting/payables.rs`](../../crates/host/tests/erp_accounting/payables.rs)`::subledger_supplier_cutoff_excludes_later_posted_payments_and_requires_posted_returns` (not opened) |  |
| Inventory control compared with the ledger | proven | [`crates/host/tests/erp_accounting/stock_sources.rs`](../../crates/host/tests/erp_accounting/stock_sources.rs)`::inventory_control_compares_posted_stock_and_receipt_sources_with_the_ledger` (opened); [`tools/e2e/stock-posting.mjs`](../../tools/e2e/stock-posting.mjs) `STOCKPOST-native-inventory-control-comparison` (not opened) | A control comparison, not a valuation. |
| Dated stock valuation of cost layers against the ledger | absent | [`docs/modules/STOCK_COST_POSTING.md`](../modules/STOCK_COST_POSTING.md) | The document says it is not delivered. |
| Tax report ties to control accounts | proven | [`crates/host/tests/erp_accounting/tax_report.rs`](../../crates/host/tests/erp_accounting/tax_report.rs)`::the_tax_report_ties_posted_output_and_input_tax_to_the_control_accounts` (opened); [`tools/e2e/tax.mjs`](../../tools/e2e/tax.mjs) `TAX-native-tax-report` (PASS text read) |  |
| Period close matches sources; close readiness names each blocker | proven | [`crates/host/tests/erp_accounting/close.rs`](../../crates/host/tests/erp_accounting/close.rs)`::close_readiness_names_each_outstanding_condition_and_agrees_with_the_close` (opened); [`crates/host/tests/erp_accounting/purchase_sources.rs`](../../crates/host/tests/erp_accounting/purchase_sources.rs)`::purchase_sources_prevent_premature_close_and_database_forgery_and_guard_tampering` (not opened); [`tools/e2e/due-terms.mjs`](../../tools/e2e/due-terms.mjs) `CLOSE-native-period-close-check` (not opened) |  |
| A closed period refuses unauthorized change | proven | [`tools/e2e/accounting.mjs`](../../tools/e2e/accounting.mjs) `ACCOUNTING-native-closed-period-refusal` (not opened); [`tools/e2e/stock-posting.mjs`](../../tools/e2e/stock-posting.mjs) `STOCKPOST-pending-close-and-discard` (not opened); [`crates/host/tests/erp_accounting/bank_periods.rs`](../../crates/host/tests/erp_accounting/bank_periods.rs)`::changed_cash_evidence_requires_a_new_review_and_backdating_cannot_break_a_closed_bank_period` (not opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::a_manual_receipt_is_refused_with_its_reason_and_stores_nothing` (opened) | A manual receipt dated in a closed period is refused with `invoice_in_closed_period`. |
| Reopen only through a permitted policy | proven | [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::journal_correction_keeps_original_immutable_and_reverses_in_a_new_open_period` (not opened) | The `periods.reopen` permission is in `crates/domain/src/erp/accounting.rs`. |
| Year-end close of earnings to retained earnings | absent | [`docs/modules/ACCOUNTING.md`](../modules/ACCOUNTING.md) |  |
| A finance professional agrees the statements are right for a real period | outside the repo | none possible in the repo |  |

**`remaining`:**
- A dated stock valuation of cost layers against the ledger is absent, so the inventory subledger is only a control comparison.
- A year-end close of earnings to retained earnings is absent.
- Opening documents (open invoices and bills with parties and dates) are absent, so AR and AP aging omit pre-existing items (see FIN-06 and INT-02).
- A finance professional reviews the statements of one real period.

**Change to the register:** Register `remaining` is mostly right; inventory control comparison, declared openings and close readiness exist, while layer valuation and document-level openings remain. Evidence gains close readiness and the tax report.

**Profile note:** Same for `global`.

### FIN-06

**Requirement.** Tax, localization, opening balances dan effective dating mengikuti aturan scope yang ditelaah pihak kompeten. Export/filing serta integrasi resmi diuji bila dijanjikan; komentar PSAK/IFRS tidak menjadi sertifikasi

Register before: `open`. After: **`partial`**. Clauses: 8 proven, 1 built-unproven, 4 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Generic value-added tax on sales and purchases, company-entered codes, confirmed by a second person, versioned | proven | [`crates/host/tests/erp_accounting/tax.rs`](../../crates/host/tests/erp_accounting/tax.rs)`::tax_codes_are_entered_confirmed_by_a_second_person_listed_retired_and_versioned` (opened); [`crates/host/tests/erp_accounting/tax_sales.rs`](../../crates/host/tests/erp_accounting/tax_sales.rs)`::an_exclusive_tax_code_adds_tax_to_the_invoice_and_corrections_split_with_the_stored_rate` (opened); [`crates/host/tests/erp_accounting/tax_purchases.rs`](../../crates/host/tests/erp_accounting/tax_purchases.rs)`::a_taxed_supplier_bill_posts_goods_input_tax_and_a_gross_payable_and_is_paid_in_gross` (opened) |  |
| Tax-exclusive and tax-inclusive prices, base fraction (11/12), zero rate, one rounding per document | proven | [`crates/host/tests/erp_accounting/tax_sales.rs`](../../crates/host/tests/erp_accounting/tax_sales.rs)`::an_inclusive_tax_code_extracts_tax_and_the_invoice_is_paid_corrected_and_mailed` (not opened); [`crates/host/tests/erp_accounting/tax_posting.rs`](../../crates/host/tests/erp_accounting/tax_posting.rs)`::inclusive_scaled_and_zero_rate_codes_post_the_tax_their_invoice_stores` (not opened); [`tools/e2e/tax.mjs`](../../tools/e2e/tax.mjs) `TAX-native-inclusive-order`, `TAX-native-base-fraction-code` (PASS text read) |  |
| Tax posts to the code's control account; corrections and voids split the tax; stale account refused | proven | [`crates/host/tests/erp_accounting/tax_posting.rs`](../../crates/host/tests/erp_accounting/tax_posting.rs)`::corrections_and_a_void_of_a_taxed_invoice_reverse_net_and_tax_to_zero` (not opened); [`crates/host/tests/erp_accounting/tax_posting.rs`](../../crates/host/tests/erp_accounting/tax_posting.rs)`::capture_refuses_a_stale_tax_account_and_the_database_refuses_a_source_with_another_tax` (not opened); [`tools/e2e/tax.mjs`](../../tools/e2e/tax.mjs) `TAX-native-correction-split`, `TAX-native-inactive-tax-account-refusal` (PASS text read) |  |
| Effective dating of tax codes: date ranges, no overlap, version chosen by document date | proven | [`crates/host/tests/erp_accounting/tax_sales.rs`](../../crates/host/tests/erp_accounting/tax_sales.rs)`::orders_and_invoices_refuse_tax_codes_that_do_not_apply_with_specific_reasons` (opened); [`crates/host/tests/erp_accounting/tax.rs`](../../crates/host/tests/erp_accounting/tax.rs)`::tax_codes_refuse_wrong_accounts_overlaps_other_books_and_missing_authority` (not opened) |  |
| Guards found by the independent review of the tax path: UTC tax dates, no back-dated taxed invoice, a tax control account never a posting-policy account, a ledger bound on taxed claims, a reversal only after its original is posted, startup readiness of the tax guards | proven | [`crates/host/tests/erp_accounting/tax_review.rs`](../../crates/host/tests/erp_accounting/tax_review.rs)`::tax_dates_are_utc_dates_and_a_taxed_invoice_cannot_be_back_dated` (not opened); [`crates/host/tests/erp_accounting/tax_review.rs`](../../crates/host/tests/erp_accounting/tax_review.rs)`::a_tax_control_account_and_a_posting_policy_account_exclude_each_other` (not opened); [`crates/host/tests/erp_accounting/tax_review.rs`](../../crates/host/tests/erp_accounting/tax_review.rs)`::a_taxed_claim_beyond_the_ledger_bound_is_refused_at_capture` (opened); [`crates/host/tests/erp_accounting/tax_review.rs`](../../crates/host/tests/erp_accounting/tax_review.rs)`::a_reversal_is_not_drafted_before_its_original_document_is_posted` (opened); [`crates/host/tests/erp_accounting/tax_review.rs`](../../crates/host/tests/erp_accounting/tax_review.rs)`::the_host_is_not_ready_without_the_tax_guards_outside_accounting` (not opened); [`docs/architecture/TAX.md`](../architecture/TAX.md) | The review read the code and found no arithmetic defect; it was not a review by a tax professional. TAX.md "Open questions" lists what stays open, for example a rate fixed at invoice creation and an invoice on a book without an invoice policy that never reaches the ledger. |
| Tax report ties to control accounts, lists pending and outside-ledger documents | proven | [`crates/host/tests/erp_accounting/tax_report.rs`](../../crates/host/tests/erp_accounting/tax_report.rs)`::the_tax_report_ties_posted_output_and_input_tax_to_the_control_accounts` (opened); [`crates/host/tests/erp_accounting/tax_report.rs`](../../crates/host/tests/erp_accounting/tax_report.rs)`::taxed_documents_that_were_never_captured_are_listed_outside_the_ledger` (not opened); [`tools/e2e/tax.mjs`](../../tools/e2e/tax.mjs) `TAX-native-tax-report` (PASS text read) |  |
| Indonesia field pack: templates, VAT registration statement, tax ids on invoices, externally issued tax invoice reference, documents listing and CSV | proven | [`crates/host/tests/erp_accounting/tax_documents.rs`](../../crates/host/tests/erp_accounting/tax_documents.rs)`::the_documents_listing_and_its_csv_give_the_posted_taxed_sources_of_a_period` (opened); [`crates/host/tests/erp_accounting/tax_documents.rs`](../../crates/host/tests/erp_accounting/tax_documents.rs)`::a_taxed_invoice_keeps_its_tax_ids_and_takes_one_replaceable_tax_reference` (not opened); [`crates/host/tests/workspace_flows/tax_registration.rs`](../../crates/host/tests/workspace_flows/tax_registration.rs)`::a_company_states_its_tax_registration_on_a_tax_id_of_digits_and_separators` (not opened); [`docs/modules/TAX_INDONESIA_PACK.md`](../modules/TAX_INDONESIA_PACK.md) | Host tests only. |
| The same pack seen in the browser (tax ids, reference form, CSV link) | proven | [`docs/modules/TAX_INDONESIA_PACK.md`](../modules/TAX_INDONESIA_PACK.md) | [`tools/e2e/receipts.mjs`](../../tools/e2e/receipts.mjs) `TAXPACK-native-template-tax-code`, `TAXPACK-native-company-vat-statement`, `TAXPACK-native-invoice-tax-ids-and-reference`, `TAXPACK-native-documents-listing-and-csv` (PASS text read). |
| Declared opening balances for receivable, payable and inventory | proven | [`crates/host/tests/erp_accounting/openings.rs`](../../crates/host/tests/erp_accounting/openings.rs)`::a_declared_opening_receivable_is_explained_by_the_control_comparison` (opened); [`crates/host/tests/erp_accounting/stock_sources.rs`](../../crates/host/tests/erp_accounting/stock_sources.rs)`::declared_inventory_and_payable_openings_are_explained_by_their_controls` (not opened); [`tools/e2e/stock-posting.mjs`](../../tools/e2e/stock-posting.mjs) `OPENING-native-declared-opening-form` (not opened) | One lump per declaration; no documents, no aging, no customer or supplier. |
| Opening documents (open invoices and bills with parties and dates), credit-side and debit-side openings, stock quantities | absent | [`docs/modules/ACCOUNTING.md`](../modules/ACCOUNTING.md) | ACCOUNTING.md "Declared opening balances: Limits". |
| Withholding tax (PPh), payroll tax and BPJS, luxury-goods tax, tax per line, tax on payments | absent | [`docs/architecture/TAX.md`](../architecture/TAX.md) | The PPh 21 and BPJS formulas in `crates/domain/src/erp/tax_indonesia.rs` are reference code that no module uses. Needed only if the profile promises them. |
| Export or filing integration to the Indonesian authority (e-Faktur, Coretax, SPT) | absent | [`docs/modules/TAX_INDONESIA_PACK.md`](../modules/TAX_INDONESIA_PACK.md) | The CSV is a draft export, not an authority format. The PRD requires testing only if promised. |
| Indonesian localization beyond tax fields: Bahasa Indonesia interface, standard chart of accounts | absent | [`docs/architecture/REFUSAL_REASONS.md`](../architecture/REFUSAL_REASONS.md) | Refusal messages carry an Indonesian text but screens show English; accounts are created by hand. |
| Review of every rule by a competent party (an Indonesian tax adviser or accountant) | outside the repo | none possible in the repo | The pack's own document says nobody with tax authority has reviewed it. |

**`remaining`:**
- No tax rule, rate, template, posting rule or CSV column has been reviewed by a tax professional, and the owner must name one.
- Withholding tax (PPh), payroll tax and BPJS are not built, and the owner has not decided whether the profile promises them.
- An e-Faktur, Coretax or SPT export is absent, and the owner has not decided whether it is promised.
- Opening documents with parties, dates and terms are absent (see INT-02).
- A Bahasa Indonesia interface and a standard chart of accounts are absent.
- There is no tenant time zone, so document and tax dates are UTC calendar dates.

**Change to the register:** Register status `open` with empty evidence was stale: the five-stage tax engine and the Indonesia pack are merged, the independent review of the tax path is merged, and declared openings exist. `remaining` ("selected jurisdictions/currencies") is half stale because jurisdiction and currency were selected on 2026-11-14.

**Profile note:** Same for `global`.

### AGT-01

**Requirement.** Tujuan natural-language menjadi rencana typed yang didukung, atau klarifikasi/gap yang jujur. Follow-up mempertahankan tujuan dan preferensi yang relevan; dependency memakai hasil terverifikasi. Perubahan scope setelah partial completion menampilkan efek yang sudah terjadi

Register before: `partial`. After: **`partial`**. Clauses: 5 proven, 0 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| A goal becomes a typed plan of registered actions (owned model fixture) | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::configured_model_plans_registered_actions_and_passes_verified_ids_between_steps` (not opened); [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::stock_goal_review_approval_execution_and_conversational_report_are_connected` (not opened) | Proves the host contract with a fixture model, not model quality. |
| Missing input produces a clarification, not a draft | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::changed_supplier_facts_require_a_new_review_and_cancelled_work_never_runs` (opened) | The test reaches state `clarifying` and cancels it; only those assertions were read. |
| Dependencies use only verified earlier results; invented ids refused | proven | [`crates/host/tests/agent_runtime/lookups.rs`](../../crates/host/tests/agent_runtime/lookups.rs)`::lookup_protocol_refuses_writes_network_targets_and_invented_ids_before_business_effects` (not opened); [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::supplier_document_agent_executes_reviewed_claim_then_match_once_and_refuses_revoked_approval` (not opened) |  |
| A follow-up keeps the goal and company scope | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::company_scoped_agent_binds_verified_product_and_stock_and_holds_after_revocation` (not opened); [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::conversations_are_durable_scoped_idempotent_and_do_not_misroute_scheduling` (not opened) |  |
| A follow-up keeps relevant preferences and prior turns for the model | absent | [`docs/modules/AGENT_RUNTIME.md`](../modules/AGENT_RUNTIME.md) | Raw conversation history and saved preference text are withheld from the provider until a safe design exists. |
| A scope change after partial completion shows effects already made and does not repeat them | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::changed_supplier_facts_require_a_new_review_and_cancelled_work_never_runs` (opened) | A replan creates a child goal; the test completes it with one effect. Whether the screen lists the already completed steps is not asserted. |
| Natural-language quality: Indonesian and English, cross-domain, same-name records, stale data, prompt injection (B05: 50 supported, 30 negative, 20 recovery, 3 repeats) | absent | [`docs/product/ERP_PARITY_ACCEPTANCE_2026-11-06.md`](ERP_PARITY_ACCEPTANCE_2026-11-06.md) | No held-out dataset and no runner against a model exists in the repo; `tools/product/simulation-scenarios.mjs` is a deterministic chat script. |
| A real model passes a live sample | outside the repo | [`tools/e2e/agent-live.mjs`](../../tools/e2e/agent-live.mjs) | The check is opt-in and needs a paid provider key with quota; a passing run is not recorded in the repository. |

**`remaining`:**
- No held-out evaluation set or runner (B05) exists, so natural-language quality in Bahasa Indonesia and English is unmeasured.
- A live run against a real model has no dated record in the repository; it needs a provider key with enough quota from the owner.
- Safe historical memory (preferences and prior goals) is withheld from the model and has no design or test.
- No test asserts that the revised-plan screen lists steps that already completed.

**Change to the register:** Register `remaining` was one boilerplate sentence. Evidence cited findings dated 2026-11-05 to 11; the final-round schema fix (#30) is not cited by any of them.

**Profile note:** Same for `global`.

### AGT-02

**Requirement.** Preview menampilkan target, kuantitas, uang, dependency dan dampak penting. Approval terikat rencana/revisi; authority dicek kembali saat eksekusi. Stale plan, revoke, salah entity dan semantic mismatch tidak menghasilkan write

Register before: `partial`. After: **`partial`**. Clauses: 5 proven, 2 built-unproven, 1 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Preview of target, quantity, money and impact for financial actions | proven | [`tools/e2e/agent-finance.mjs`](../../tools/e2e/agent-finance.mjs) `AVA-native-exact-financial-review` (not opened); [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::journal_agent_review_carries_exact_original_lines_and_stale_posting_holds_before_effects` (not opened) |  |
| Preview for the other domains | built, unproven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::reviewed_native_actions_execute_once_with_verified_receipts_in_all_eighteen_domains` (not opened) | Executes once in 18 domains; the preview content per domain is not asserted. |
| Approval is bound to plan hash and revision | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::changed_supplier_facts_require_a_new_review_and_cancelled_work_never_runs` (opened); [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_consumed_approval_cannot_authorize_a_second_command` (not opened); [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_forged_role_and_approval_in_the_body_change_nothing` (opened) |  |
| Authority rechecked at execution; revoke stops work | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::worker_obeys_parent_session_revocation_and_cannot_be_forged_with_headers` (opened); [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::supplier_document_agent_executes_reviewed_claim_then_match_once_and_refuses_revoked_approval` (not opened) |  |
| Stale plan: changed facts need a new review and nothing runs | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::changed_supplier_facts_require_a_new_review_and_cancelled_work_never_runs` (opened); [`tools/e2e/agent-finance.mjs`](../../tools/e2e/agent-finance.mjs) `AVA-native-stale-financial-hold` (not opened) |  |
| Wrong entity or company: no write | proven | [`crates/host/tests/agent_runtime/lookups.rs`](../../crates/host/tests/agent_runtime/lookups.rs)`::selected_company_cannot_review_a_different_companys_record_even_when_both_are_readable` (opened) |  |
| Semantic mismatch (a plan whose effect differs from the stated goal is caught) | absent | none found | Review is by a human; no oracle compares the proposed effect with the goal. |
| Disabled module: the agent cannot dispatch to it | built, unproven | [`crates/host/src/own.rs`](../../crates/host/src/own.rs) | The module gate applies to ERP command routes; no test in the agent runtime suites mentions a disabled module. |

**`remaining`:**
- No semantic-mismatch oracle exists that compares a plan's proposed effect with the stated goal.
- No test asserts that an agent step against a disabled module is refused and writes nothing.
- No test asserts the preview content of the non-financial domains.
- Whether the planner reads existing tax codes before proposing a taxed document is verified by no test (TAX.md says so).

**Change to the register:** Register `remaining` is partly done: pausing, resuming and independent posting by an agent have tests. Full financial workflows with tax-coded documents remain unproven.

**Profile note:** Same for `global`.

### AGT-03

**Requirement.** Status membedakan proposed, awaiting approval, running, partial, committed, reconciled, failed dan cancelled sesuai kontrak. Laporan sukses membutuhkan receipt/effect nyata. Retry/restart mempertahankan correlation; cancel tidak berpura-pura membatalkan efek yang sudah commit

Register before: `partial`. After: **`partial`**. Clauses: 3 proven, 2 built-unproven, 1 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Status vocabulary matches the contract | built, unproven | [`migrations/owner-pg/003_agent_runtime.sql`](../../migrations/owner-pg/003_agent_runtime.sql) | The states are clarifying, planned, approved, running, holding, completed, failed, cancelled, needs_review and awaiting_session. No state is named `partial` or `reconciled`, and the mapping from the PRD terms is not written down. |
| Success is reported only with a receipt and a verified checkpoint | proven | [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::business_mission_requires_approval_refuses_forged_results_and_executes_once` (not opened); [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::reviewed_native_actions_execute_once_with_verified_receipts_in_all_eighteen_domains` (not opened) |  |
| Retry and restart keep correlation and run the effect once | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::durable_worker_recovers_a_transient_failure_without_persisting_a_bearer` (not opened); [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::failed_turn_commit_rolls_back_the_plan_and_original_request_can_be_retried` (not opened); [`tools/e2e/agent-runtime.mjs`](../../tools/e2e/agent-runtime.mjs) `AVA-process-kill-restart-continuation` (not opened) | The browser check needs `ARCHAVA_E2E_AGENT_RUNTIME=1` and is not in CI. |
| Pause and resume record the real mission state | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::paused_agent_work_records_the_actual_mission_state_and_resumes_once` (not opened) |  |
| Cancel stops remaining work and never claims to undo a committed effect | built, unproven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::changed_supplier_facts_require_a_new_review_and_cancelled_work_never_runs` (opened) | Cancel of a draft is asserted; refusing to abandon a committed but unreconciled effect is documented in AGENT_RUNTIME.md and has no test. |
| Reconciled: an effect confirmed against the business record after the fact | absent | none found |  |

**`remaining`:**
- The mapping from the PRD statuses to goal and step states is not written down, and `partial` (some steps committed, others held) and `reconciled` have no state.
- No test asserts cancel after a committed step.
- No state or statement says that a receipt is the reconciliation of an effect.

**Change to the register:** Register `remaining` was one boilerplate sentence; evidence cited only 2026-11-05 to 09 records.

**Profile note:** Same for `global`.

### AGT-04

**Requirement.** Knowledge dan data bisnis mempunyai sumber, timestamp, cakupan dan izin. Query/aggregate mencakup filter yang diminta atau mengungkap batasnya; data di luar halaman pertama tetap ditemukan. Catatan/dokumen tidak menjadi instruksi yang melampaui authority

Register before: `partial`. After: **`partial`**. Clauses: 2 proven, 1 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Coverage and permission stated per collection (included, hasMore, wholeCollection) | proven | [`crates/host/tests/agent_runtime/lookups.rs`](../../crates/host/tests/agent_runtime/lookups.rs)`::model_reads_named_product_beyond_one_hundred_rows_and_rechecks_exact_target_before_execution` (opened); [`crates/host/tests/agent_runtime/lookups.rs`](../../crates/host/tests/agent_runtime/lookups.rs)`::large_authorized_workspace_is_byte_bounded_and_exact_review_guards_still_execute` (not opened) |  |
| Data beyond the first page is found (products, support cases, Unicode text) | proven | [`crates/host/tests/agent_runtime/lookups.rs`](../../crates/host/tests/agent_runtime/lookups.rs)`::support_search_finds_literal_unicode_target_beyond_first_page_and_preserves_assignment_scope` (not opened) |  |
| Source timestamp on what the model is shown | built, unproven | [`docs/modules/AGENT_RUNTIME.md`](../modules/AGENT_RUNTIME.md) | Context carries a fingerprint and coverage; the observation time of each source is not asserted. |
| Aggregates over the whole filter (sums, counts, group by) or a disclosed limit | absent | [`docs/modules/AGENT_RUNTIME.md`](../modules/AGENT_RUNTIME.md) | The limit is disclosed ("complete-filter aggregates" are not established); the capability is absent. |
| Notes, documents and records cannot instruct beyond authority (prompt injection through data) | absent | none found | Authority is checked at execution regardless (AGT-02), which bounds harm; no test feeds an instruction through a record. The simulation safety records run against a deterministic chat, not a model. |
| A held-out dataset and a current-source live regression | outside the repo | none possible in the repo | See AGT-01; needs a provider key with quota. |

**`remaining`:**
- Whole-filter aggregate lookups (count, sum) with coverage are absent.
- No test feeds an instruction through a support note, CRM note or supplier reference and asserts no write and no leakage.
- The held-out set (AGT-01) has not been run against the current source.

**Change to the register:** Register `remaining` is still accurate.

**Profile note:** Same for `global`.

### AGT-05

**Requirement.** Pemantauan stok, tugas jatuh tempo, invoice atau exception dapat menghasilkan rekomendasi proaktif dalam scope terkonfigurasi. Trigger, deduplication, budget, notifikasi dan berhenti/resume dapat diaudit; tidak ada autonomous external spending tanpa policy/authority yang diperlukan

Register before: `partial`. After: **`partial`**. Clauses: 3 proven, 0 built-unproven, 2 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Low-stock recommendation from real stock and supplier offers | proven | [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::feed_observes_real_stock_and_dispatches_one_recoverable_purchase_draft` (opened) | Reviewed, one draft purchase order, recoverable. |
| Other triggers: due tasks, overdue invoices, exceptions | absent | [`docs/modules/REVIEWED_WORKFLOWS.md`](../modules/REVIEWED_WORKFLOWS.md) | Other former sample actions are explicitly unavailable. AR aging and close readiness exist but nothing recommends from them. |
| Deduplication of an existing open order | proven | [`docs/modules/REVIEWED_WORKFLOWS.md`](../modules/REVIEWED_WORKFLOWS.md); [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::feed_observes_real_stock_and_dispatches_one_recoverable_purchase_draft` (opened) | The document states the suppression; the first assertions of the test were read, not a deduplication assertion. |
| Budget, notification and stop or resume of monitoring | absent | none found |  |
| No autonomous external spending | proven | [`docs/modules/REVIEWED_WORKFLOWS.md`](../modules/REVIEWED_WORKFLOWS.md); [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::business_mission_requires_approval_refuses_forged_results_and_executes_once` (not opened) | The recommendation creates a draft and the confirmation is explicit. |

**`remaining`:**
- Only the low-stock trigger exists; overdue receivables and period-close blockers have no recommendation.
- Per-tenant thresholds, a notification channel and an audited stop and resume switch for monitoring are absent.
- The owner has not decided whether this P1 requirement is in scope for the first `id-idr` release; scoping it out needs a recorded owner scope decision.

**Change to the register:** Register `remaining` was one boilerplate sentence.

**Profile note:** Same for `global`.

### AGT-06

**Requirement.** Model/profile, budget, quota, timeout dan retry eksplisit. Provider down, output rusak atau referensi salah menghasilkan recovery/klarifikasi yang aman dan manual fallback. Model diganti hanya melalui evaluasi konfigurasi baru; semua kegagalan masuk laporan biaya dan availability

Register before: `partial`. After: **`partial`**. Clauses: 5 proven, 1 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Explicit model, budget (three calls per turn), turn and transport limits, timeout, retry | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::provider_failure_is_safe_recoverable_budgeted_and_keeps_manual_work_available` (opened); [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::staged_contact_reads_share_three_total_calls_with_routing_and_never_repeat_an_identical_read` (not opened) |  |
| Provider down or output rejected: safe category, no secret echo, retry only with a new request | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::provider_failure_is_safe_recoverable_budgeted_and_keeps_manual_work_available` (opened) | The test asserts `agent_planner_unavailable`, the upstream status and that a private marker is absent. |
| Broken or malformed output: one bounded format correction, no resend of rejected text | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::one_binding_format_correction_stays_within_budget_and_never_resends_failed_proposal_text` (not opened); [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::contract_format_correction_preserves_exact_values_and_never_resends_rejected_fields` (not opened) |  |
| The last planning call cannot ask for a read | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs); [`docs/modules/AGENT_RUNTIME.md`](../modules/AGENT_RUNTIME.md) | The final-round schema fix (#30) added a test in `disclosure.rs`; that test was not opened. |
| Manual fallback when AI is unavailable or revoked | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::no_disclosure_sends_zero_provider_requests_and_manual_actions_still_commit` (opened); [`tools/e2e/ai-disclosure.mjs`](../../tools/e2e/ai-disclosure.mjs) `AI-native-revoke-local-fallback` (not opened) |  |
| Models change only through an evaluated configuration | absent | none found | No evaluation gate guards `GEMINI_AGENT_MODEL`. |
| Failures enter a cost and availability report | built, unproven | [`migrations/owner-pg/003_agent_runtime.sql`](../../migrations/owner-pg/003_agent_runtime.sql) | Model attempts are stored; no report or screen reads them. |
| Real-provider acceptance of the live turns | outside the repo | [`tools/e2e/agent-live.mjs`](../../tools/e2e/agent-live.mjs); [`docs/product/PRD_GEMINI_SCHEMA_FINDING_2026-11-09.md`](PRD_GEMINI_SCHEMA_FINDING_2026-11-09.md) | Needs a provider key with quota. The register's earlier text (a failed third live turn and HTTP 429 at candidate 60e65) predates the final-round schema fix. |
| Held-out trials and a second provider | absent | none found | Only a Gemini GenerateContent transport exists. |

**`remaining`:**
- No cost and availability report reads the stored model attempts.
- No evaluation gate guards a change of model or profile; the held-out set (AGT-01) would serve.
- A live acceptance run on the current source has no dated record in the repository, and a second provider is absent if one is promised; both need a provider key and quota from the owner.

**Change to the register:** Register `remaining` described a failed live run at candidate 60e65 as unverified pending provider availability; it predates #30 and is replaced.

**Profile note:** Same for `global`.

### CFG-01

**Requirement.** Owner menjelaskan bisnis dan mendapatkan susunan modul/options yang memang tersedia dan kompatibel. Preview memperlihatkan dependency, izin, biaya/readiness dan gap; extension yang belum tersedia tidak ditampilkan seolah sudah aktif

Register before: `partial`. After: **`partial`**. Clauses: 3 proven, 2 built-unproven, 1 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| A business sentence (including hybrids) becomes a module set with what it needs | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::chat_and_dashboard_agree_on_business_sentences_and_blocked_plans` (not opened); [`crates/host/tests/product_workspaces.rs`](../../crates/host/tests/product_workspaces.rs)`::thirteen_workspaces_read_real_scoped_rows_and_fail_closed_on_current_grants` (not opened) | `/own/chat` is a deterministic keyword parser, not a model. |
| Preview names dependency blocks (missing dependency, mandatory) | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::disabling_a_required_module_is_blocked_and_nothing_is_written` (not opened); [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-blocked-plan-is-shown-and-never-applied` (PASS text read) |  |
| Preview shows required permissions per module | absent | none found | `/own/config` returns only the owner's `grants.read` and `grants.apply`; nothing says which permissions a user needs per module. |
| Preview shows real readiness and cost | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::the_preview_states_changes_readiness_and_cost_and_writes_nothing` (opened) | The catalogue in `crates/host/src/own.rs` marks every module entitled, installed, `Ready` and `Included`, so the test asserts constants. Real readiness and pricing do not exist. |
| An unavailable or unknown extension is refused and never shown as active | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::the_preview_states_changes_readiness_and_cost_and_writes_nothing` (opened) | An unknown module is `unknown_module`, cannot be applied and writes nothing. |
| A capability gap is stated as such for a request the product cannot serve | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::the_chat_plan_equals_the_dashboard_plan_and_applies` (not opened) | An unsupported sentence gets a 400 `unrecognized_request`, not a structured gap record. |

**`remaining`:**
- Every module is entitled, installed, `Ready` and priced `included@1` in the host catalogue, so a module never needs setup (no book, no policy, no tax code) and readiness is a constant.
- The preview does not show the permissions each module needs.
- An unsupported request returns an error, not a structured capability-gap result.

**Change to the register:** Register `remaining` was one boilerplate sentence and its only evidence was the specification `OWNER_CONFIGURATION_PRD.md`, which is replaced by the host tests and the browser check.

**Profile note:** Same for `global`.

### CFG-02

**Requirement.** Apply memakai current management grant dan versi konfigurasi. Dua tab, retry dan restart memberi satu perubahan yang sah. Disable memperbarui UI/API/tools sekaligus dan menjaga data/history; restore konfigurasi tidak memundurkan transaksi keuangan

Register before: `partial`. After: **`partial`**. Clauses: 4 proven, 3 built-unproven, 0 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Apply uses the current grant and base revision | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_missing_or_revoked_grant_refuses_the_operation` (opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_stale_base_and_a_foreign_confirmation_refuse_without_writing` (not opened) |  |
| Two tabs, retry and a lost response give one change | proven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-two-tab-stale-plan-refused`, `OWNER-native-disable-lost-response-recovery` (PASS text read); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::racing_applies_converge_to_one_revision_and_one_receipt` (opened) |  |
| A restart gives one change | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_rebuilt_host_recovers_the_same_receipt_and_history_from_postgres` (opened) | A rebuilt router over the same database, not a process restart; no browser check restarts the server between commit and recovery. |
| Disable removes the API on every module and keeps the data | proven | [`crates/host/tests/product_workspaces.rs`](../../crates/host/tests/product_workspaces.rs)`::thirteen_workspaces_read_real_scoped_rows_and_fail_closed_on_current_grants` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::disabling_the_pipeline_refuses_the_backend_and_enabling_restores_it` (opened) | The toggle loop covers 13 workspace modules plus commerce, purchasing, billing, support and accounting. |
| Disable removes the UI entry of an ERP module | built, unproven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-disable-lost-response-recovery` (PASS text read) | The browser asserts the CRM pipeline only. |
| Disable removes the agent tool | built, unproven | [`crates/host/tests/assistant.rs`](../../crates/host/tests/assistant.rs)`::disabling_the_pipeline_module_removes_the_tool_and_zeroes_the_counts` (not opened) | CRM pipeline only; dispatch to a disabled ERP module is not tested. |
| Restore creates a new revision and does not roll back financial history | proven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-compatible-restore`, `OWNER-native-label-restore-preserves-history` (PASS text read); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::restore_lands_as_a_new_revision_and_keeps_history_and_business_rows` (not opened) | Digests cover 23 CRM and accounting tables, which are non-empty at that point of the browser gate. |

**`remaining`:**
- No browser check disables an ERP module and asserts that its navigation entry and screen disappear.
- No test asserts that an agent goal against a disabled module is refused (see AGT-02).
- No check restarts the server process between commit and recovery of an owner change.

**Change to the register:** Register `remaining` was one boilerplate sentence and its only evidence was the specification.

**Profile note:** Same for `global`.

### CFG-03

**Requirement.** Dua consumer/business model memakai kontrak dan rilis core/module yang sama tanpa source fork. Kombinasi mempunyai manifest, compatibility, permission dan upgrade evidence; keberadaan crate tidak cukup

Register before: `partial`. After: **`partial`**. Clauses: 3 proven, 3 built-unproven, 0 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Two businesses on one release differ only by owner configuration | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::two_businesses_share_one_release_and_differ_only_by_owner_configuration` (opened) | Only the first 60 lines were read: two tenants with different revisions, labels and module sets. |
| Two independent clients build against the shipped core and domain packages | built, unproven | [`tools/rust/cold-consumers.mjs`](../../tools/rust/cold-consumers.mjs); [`tools/rust/persistent-consumers.mjs`](../../tools/rust/persistent-consumers.mjs); [`docs/product/PRD_COLD_CONSUMERS_2026-11-08.md`](PRD_COLD_CONSUMERS_2026-11-08.md) | Run on source fcd94e on 2026-11-08; not in CI and not re-run on the current source. |
| A manifest per module and a registration gate | proven | [`config/modules.v1.json`](../../config/modules.v1.json); [`tools/check-modules.mjs`](../../tools/check-modules.mjs); [`docs/architecture/MODULE_REGISTRY.md`](../architecture/MODULE_REGISTRY.md) | Run by `verify.mjs`. |
| Compatibility between combinations | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::disabling_a_required_module_is_blocked_and_nothing_is_written` (not opened) | Dependency rules exist per module; there is no matrix of tested combinations. |
| Upgrade evidence from previous packages to the current one | built, unproven | [`tools/e2e/prior-native.mjs`](../../tools/e2e/prior-native.mjs) `OPS-prior-native-financial-upgrade`, `OPS-prior-native-isolated-rollback` (not opened) | Needs `ARCHAVA_E2E_OPS_ROOT` and a previous package; not in CI and not recorded for the current commit. |
| Permissions per combination | proven | [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::an_assigned_scope_is_refused_above_the_scope_gate` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_never_granted_or_revoked_principal_is_refused_on_every_owner_route` (not opened) |  |

**`remaining`:**
- The cold and persistent consumer scripts were last recorded on source fcd94e and are neither in CI nor re-run on the current source.
- No two named business-model compositions for `id-idr` are tested end to end.
- The opt-in upgrade proof from a previous package has no record for the current commit.

**Change to the register:** Register `remaining` is accurate but dated 2026-11-08; evidence gains the module registry and the two-tenant test.

**Profile note:** Same for `global`.

### UX-01

**Requirement.** Owner/operator baru dapat menyiapkan bisnis, mengimpor data uji dan menyelesaikan tugas pertama lewat flow yang dimengerti. Ukur waktu onboarding, hambatan dan kebutuhan bantuan terhadap profil pengguna, termasuk migrasi existing system

Register before: `open`. After: **`open`**. Clauses: 1 proven, 1 built-unproven, 3 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Owner bootstrap without a terminal | absent | [`deploy/NATIVE_INSTALL.md`](../../deploy/NATIVE_INSTALL.md) | A tenant and its first owner are created by the operator CLI; there is no self-service sign-up. |
| Grant, revoke and invite users from the product | absent | [`docs/product/OWNER_ACCEPTANCE_TEST_MAP.md`](OWNER_ACCEPTANCE_TEST_MAP.md) | The product has no grant-management screen; the browser checks change grants through SQL. |
| Import test data (contacts, products, opening stock) with review and rollback | proven | [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::import_binds_the_review_to_actor_tenant_and_hash_then_commits_atomically_once` (not opened) |  |
| Setup of company, book, accounts, policies and tax codes through guided steps | built, unproven | [`tools/e2e/accounting.mjs`](../../tools/e2e/accounting.mjs); [`tools/e2e/store-finance-cycle.mjs`](../../tools/e2e/store-finance-cycle.mjs) | Possible through reviewed forms; the journeys set up through API calls; there is no guided setup and no measurement. |
| Time, obstacles and help needed by a real new user | outside the repo | none possible in the repo | Needs a pilot user. |
| Migration of an existing system | absent | [`docs/modules/REVIEWED_WORKFLOWS.md`](../modules/REVIEWED_WORKFLOWS.md) | No customer, supplier, open-invoice or ledger import exists. |

**`remaining`:**
- User administration (grant, revoke, invite, offboard) is absent from the product.
- No guided first-run flow sets up the company, book, chart, policies and tax codes.
- A real new owner (pilot user) has not been timed or observed setting up the business.
- Migration import of parties and open documents is absent (see INT-02).

**Change to the register:** Register status `open` is right.

**Profile note:** Same for `global`.

### UX-02

**Requirement.** UI/API menyediakan operasi manual yang setara ketika AI tidak tersedia. Status, error, approval dan recovery memakai bahasa bisnis, mendukung keyboard dan viewport sasaran; bukti dapat dibuka tanpa membaca log atau raw JSON

Register before: `partial`. After: **`partial`**. Clauses: 3 proven, 3 built-unproven, 0 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Manual equivalent when AI is off or revoked | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::no_disclosure_sends_zero_provider_requests_and_manual_actions_still_commit` (opened); [`tools/e2e/ai-disclosure.mjs`](../../tools/e2e/ai-disclosure.mjs) `AI-native-revoke-local-fallback` (not opened) |  |
| Native forms exist for every registered command | proven | [`docs/modules/NATIVE_OPERATIONS.md`](../modules/NATIVE_OPERATIONS.md); [`tools/e2e/operations.mjs`](../../tools/e2e/operations.mjs) `D02-commerce-ui`, `D03-purchasing-ui`, `D09-billing-ui`, `D12-support-ui` (not opened) | Generic reviewed forms; depth varies. The manual receipt form is a dedicated screen with no browser check at this commit. |
| Errors in business language with a next step | built, unproven | [`docs/architecture/REFUSAL_REASONS.md`](../architecture/REFUSAL_REASONS.md); [`tools/check-refusal-reasons.mjs`](../../tools/check-refusal-reasons.mjs) | A catalogue of guard messages with English and Indonesian text; an uncatalogued refusal keeps the general sentence; screens show English only. |
| Keyboard operation | built, unproven | [`tools/e2e/customer-identity.mjs`](../../tools/e2e/customer-identity.mjs) `CUSTOMER-keyboard-search-no-write` (not opened); [`tools/e2e/party-transactions.mjs`](../../tools/e2e/party-transactions.mjs) `PARTYTX-picker-pagination-and-keyboard` (not opened) | Pickers are covered; there is no whole-app keyboard or accessibility audit. |
| Target viewports (390 px and desktop) | proven | [`tools/e2e/verify.mjs`](../../tools/e2e/verify.mjs) `UI-mobile-layout` (not opened); [`tools/e2e/operations.mjs`](../../tools/e2e/operations.mjs) `UI-operations-mobile` (not opened); [`tools/e2e/invoice-posting.mjs`](../../tools/e2e/invoice-posting.mjs) `INVOICEPOST-native-source-mobile` (not opened) | Only 390 px and desktop are asserted. |
| Evidence opens without logs or raw JSON | built, unproven | [`tools/e2e/invoice-posting.mjs`](../../tools/e2e/invoice-posting.mjs) `INVOICEPOST-native-review-source-and-close-refusal` (not opened); [`tools/e2e/accounting.mjs`](../../tools/e2e/accounting.mjs) `ACCOUNTING-native-history-and-mobile` (not opened) | Source reviews and history are screens; generic forms still show JSON-like fields for some modules. |
| Real users complete the tasks | outside the repo | none possible in the repo | Needs pilot users. |

**`remaining`:**
- There is no accessibility pass (labels, focus order, contrast) and no automated check for it in the browser gate.
- Screens show English only; an Indonesian interface needs a request locale the product does not have.
- Pilot users have not been observed completing the sold tasks.

**Change to the register:** Register `remaining` was one boilerplate sentence.

**Profile note:** Same for `global`.

### UX-03

**Requirement.** Customer/employee/owner dan embed/channel memakai identity serta data scope yang benar. Chat, live voice, interruption dan avatar diterima terpisah untuk paket yang menawarkan masing-masing mode; transcript saja tidak membuktikan voice quality

Register before: `partial`. After: **`partial`**. Clauses: 3 proven, 0 built-unproven, 1 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Identity and tenant scope on Presence and checkout embed | proven | [`crates/host/tests/presence_lifecycle.rs`](../../crates/host/tests/presence_lifecycle.rs)`::checkout_signature_mint_requires_membership_in_the_signed_tenant` (opened); [`crates/host/tests/presence_lifecycle.rs`](../../crates/host/tests/presence_lifecycle.rs)`::review_gemini_foreign_tenant_session_override_is_refused` (not opened); [`tools/e2e/presence.mjs`](../../tools/e2e/presence.mjs) `PRESENCE-cross-user-and-tenant-refusal` (not opened) |  |
| Chat mode lifecycle | proven | [`crates/host/tests/presence_lifecycle.rs`](../../crates/host/tests/presence_lifecycle.rs)`::test_presence_session_lifecycle_and_turn_and_degrade` (not opened); [`tools/e2e/presence.mjs`](../../tools/e2e/presence.mjs) `PRESENCE-loopback-lifecycle` (not opened) |  |
| Live voice transport and revocation on owned fixtures | proven | [`crates/host/tests/voice_media.rs`](../../crates/host/tests/voice_media.rs)`::verified_owned_wss_carries_pcm_video_and_model_audio_then_closes_on_revocation` (not opened); [`tools/e2e/media.mjs`](../../tools/e2e/media.mjs) `VOICE-native-media` (not opened) | The browser check needs `ARCHAVA_E2E_MEDIA=1` and is not in CI. |
| Live voice quality, interruption and latency with a real provider | outside the repo | [`docs/modules/LIVE_MEDIA.md`](../modules/LIVE_MEDIA.md) | LIVE_MEDIA.md: it does not prove availability, quality, quota or billing of a real provider account. |
| Avatar mode | absent | none found | Only needed if the profile sells an avatar. |

**`remaining`:**
- The owner has not decided which Presence modes `id-idr` sells; if none, a recorded owner scope decision can make this not applicable.
- Real-provider acceptance of live voice quality, interruption and latency needs Gemini Live and LiveKit accounts and a TURN setup from the owner.
- Avatar mode is absent and matters only if it is sold.

**Change to the register:** Register `remaining` is accurate.

**Profile note:** Same for `global`.

### INT-01

**Requirement.** Connector memakai kontrak berversi, scope/secret lifecycle dan signature/correlation yang benar. External effect memiliki outbox, retry dan reconciliation; webhook berulang/terlambat, response hilang dan partial failure teruji. Tidak mengklaim exactly-once universal untuk jaringan eksternal

Register before: `partial`. After: **`partial`**. Clauses: 4 proven, 0 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Signature and correlation checked before any money moves | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::gateway_challenges_unsigned_status_and_conserves_settlement_refunds_and_replays` (opened) | The Midtrans notification signature is verified, then the status is read from the provider. |
| Repeated, late and lost provider responses keep one effect | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::unknown_creation_keeps_original_order_and_blocks_a_fresh_link_even_after_replay` (not opened); [`tools/e2e/email.mjs`](../../tools/e2e/email.mjs) `EMAIL-duplicate-intent-recovery`, `EMAIL-unknown-retry-same-key`, `EMAIL-late-response-after-navigation` (not opened) |  |
| Outbox, retry and reconciliation for outbound effects (invoice email) | proven | [`crates/host/tests/invoice_email.rs`](../../crates/host/tests/invoice_email.rs)`::send_invoice_email_records_outbox_and_accepts` (not opened); [`crates/host/tests/invoice_email.rs`](../../crates/host/tests/invoice_email.rs)`::review_gemini_lost_ack_then_ui_style_new_key_must_not_duplicate_email` (not opened) |  |
| Scope revocation mid-flight | proven | [`crates/host/tests/gateway_payments.rs`](../../crates/host/tests/gateway_payments.rs)`::service_revocation_while_get_status_is_in_flight_refuses_all_financial_effects` (not opened) |  |
| Versioned connector contract and secret rotation lifecycle | absent | [`docs/architecture/PAYMENTS.md`](../architecture/PAYMENTS.md); [`config/payment-providers.example.json`](../../config/payment-providers.example.json) | Secret environment variable names are stored, not secrets; the connector contract has no version field and no rotation test exists. |
| Other priority integrations (bank API, e-Faktur, marketplace, ERP) | absent | [`docs/architecture/INTEGRATION.md`](../architecture/INTEGRATION.md) | None is chosen; the `mcp`, `rpa` and `campaigns` crates are not mounted. |
| A live provider run (Midtrans sandbox or production, a mail provider) | outside the repo | none possible in the repo | Needs merchant and mail accounts and a public HTTPS origin for the notification URL. |

**`remaining`:**
- The owner has not named the priority integrations for `id-idr` (payment, bank, tax filing).
- The connector contract has no version field and no secret rotation test exists.
- No run against a Midtrans sandbox exists; it needs a merchant account and a public HTTPS origin from the owner.
- A mail provider run needs a mail provider account from the owner.

**Change to the register:** Register `remaining` was one boilerplate sentence; evidence gains the payment architecture and the gateway and email tests.

**Profile note:** For `global` the only payment provider is IDR-only; a non-IDR company has no payment integration and only manual receipts.

### INT-02

**Requirement.** Import memvalidasi mapping, duplicate identity, opening balance dan control totals. Dry run, rollback/kompensasi yang sesuai serta rekonsiliasi source-to-target tersedia; data production tidak dipakai sebagai fixture uji

Register before: `partial`. After: **`partial`**. Clauses: 2 proven, 2 built-unproven, 2 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Dry run: reviewed plan bound to actor, tenant and hash | proven | [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::import_binds_the_review_to_actor_tenant_and_hash_then_commits_atomically_once` (not opened) | Contacts, products and opening inventory; 500 rows and 50 columns. |
| Atomic commit with rollback of the whole batch | proven | [`crates/host/tests/product_workflows.rs`](../../crates/host/tests/product_workflows.rs)`::import_binds_the_review_to_actor_tenant_and_hash_then_commits_atomically_once` (not opened) | Same test; the rollback assertions were not read. |
| Duplicate identity detection (same SKU, same contact) | built, unproven | [`crates/host/src/ingestion.rs`](../../crates/host/src/ingestion.rs) | A reviewed diff exists; duplicate rules are not asserted by name. |
| Opening balances: ledger openings and opening stock | built, unproven | [`crates/host/tests/erp_accounting/openings.rs`](../../crates/host/tests/erp_accounting/openings.rs)`::a_declared_opening_receivable_is_explained_by_the_control_comparison` (opened) | Lump declarations exist; the import does not carry AR or AP documents. |
| Control totals and source-to-target reconciliation | absent | none found | Row counts only. |
| Import of parties (customer, supplier), open invoices, open bills, tax ids, bank history | absent | [`docs/modules/REVIEWED_WORKFLOWS.md`](../modules/REVIEWED_WORKFLOWS.md) |  |
| A real customer migration without production data as a fixture | outside the repo | none possible in the repo | Needs a real migration source from a pilot customer. |

**`remaining`:**
- Import of organizations and parties with tax ids, and of open AR and AP documents with dates and terms, is absent.
- No control-total check compares source totals with a declared opening, and no dry-run report reconciles source to target.
- A real migration source from a pilot customer has not been imported.

**Change to the register:** Register `remaining` was one boilerplate sentence.

**Profile note:** Worse for `global`: import is IDR-only (MULTI_CURRENCY.md).

### SEC-01

**Requirement.** Actor, tenant, entity, role, service account, independent approver dan revoke ditegakkan server. MFA/SSO/provisioning/offboarding wajib bila profil mensyaratkan; RLS lokal tidak dihitung sebagai bukti seluruh IAM enterprise

Register before: `partial`. After: **`partial`**. Clauses: 6 proven, 0 built-unproven, 1 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Actor, tenant and role come from session and database, never the body | proven | [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_forged_role_and_approval_in_the_body_change_nothing` (opened); [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_receipt_of_another_tenant_is_invisible_at_the_db_boundary` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_foreign_tenant_role_or_approval_is_refused_without_writing` (not opened) |  |
| Entity and company scope | proven | [`crates/host/tests/workspace_flows/company_trade.rs`](../../crates/host/tests/workspace_flows/company_trade.rs)`::company_scope_and_revocation_apply_to_reads_mutation_replay_and_status` (not opened) |  |
| Service principal cannot approve; independent approver and L4 self-resume refusal | proven | [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_service_principal_cannot_approve` (not opened); [`crates/host/tests/cockpit_hold_resume.rs`](../../crates/host/tests/cockpit_hold_resume.rs)`::test_cockpit_l4_self_resume_refusal_and_independent_approval` (not opened); [`crates/host/tests/erp_accounting.rs`](../../crates/host/tests/erp_accounting.rs)`::independent_posting_replay_current_grants_scope_and_company_revocation_are_enforced` (not opened); [`crates/host/tests/erp_accounting/manual_receipts.rs`](../../crates/host/tests/erp_accounting/manual_receipts.rs)`::manual_receipts_need_a_person_who_may_settle_and_are_idempotent_and_serial` (opened) | A manual customer receipt needs a person with `billing.settle`; a service principal is refused. |
| Revocation on execute, status, replay and recovery after restart; sessions | proven | [`crates/host/tests/authority.rs`](../../crates/host/tests/authority.rs)`::a_revoked_grant_is_refused_on_recovery_after_restart` (not opened); [`crates/host/tests/session.rs`](../../crates/host/tests/session.rs)`::a_restart_does_not_resurrect_a_revoked_session` (not opened) |  |
| Writer fencing: a stale process cannot write | proven | [`crates/host/tests/workspace_flows.rs`](../../crates/host/tests/workspace_flows.rs)`::stale_serving_process_cannot_write_after_writer_takeover` (not opened); [`tools/e2e/writer.mjs`](../../tools/e2e/writer.mjs) `Ops-native-writer-lifecycle` (not opened); [`docs/architecture/WRITE_FENCING.md`](../architecture/WRITE_FENCING.md) | Not high availability, as the PRD says. |
| MFA, SSO, user provisioning and offboarding in the product | absent | none found | No MFA, TOTP, OIDC, SAML or SSO code exists; whether `id-idr` requires them is a profile decision. |
| Dependency advisories are scanned daily and the scan passes | proven | [`.github/workflows/security-audit.yml`](../../.github/workflows/security-audit.yml); [`.cargo/audit.toml`](../../.cargo/audit.toml) | After #51 the daily `Security audit` workflow passes on main (a manual run on 2026-11-14). The only ignored advisory, RUSTSEC-2026-0459 for `libsqlite3-sys`, concerns a crate in `Cargo.lock` that no build graph contains, and the workflow fails if it ever enters the graph. The unmaintained `paste` crate (RUSTSEC-2024-0436) is still a supply-chain review item. |
| Enterprise IAM review | outside the repo | none possible in the repo |  |

**`remaining`:**
- User and grant administration with audit is absent from the product (see UX-01).
- The owner has not decided whether `id-idr` requires MFA or SSO; if it does, neither exists.
- The unmaintained `paste` dependency (RUSTSEC-2024-0436) has no recorded owner review.
- An enterprise IAM review needs an outside reviewer.

**Change to the register:** Register `remaining` carried a boilerplate sentence and the RustSec note; the daily audit was failing on main until #51 and passes now.

**Profile note:** Same for `global`.

### SEC-02

**Requirement.** Owner memahami data apa yang dikirim ke provider. Knowledge/context field, retention, export/delete, audit dan residency mengikuti policy profil; secret tidak masuk prompt/log. Self-host backend tidak berarti inference selalu lokal; pilihan tanpa provider diuji bila ditawarkan

Register before: `partial`. After: **`partial`**. Clauses: 4 proven, 1 built-unproven, 1 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| The owner chooses areas, categories and companies in a signed review; the payload follows exactly | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::actual_provider_payload_withholds_unselected_names_money_modules_and_credentials` (opened); [`tools/e2e/ai-disclosure.mjs`](../../tools/e2e/ai-disclosure.mjs) `AI-native-sharing-selection`, `AI-native-actual-egress-projection` (not opened); [`crates/host/tests/own/disclosure.rs`](../../crates/host/tests/own/disclosure.rs)`::owner_disclosure_approval_binds_whole_policy_actor_expiry_and_one_effect` (not opened) |  |
| Company consent filters real records; revocation stops later rounds | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::company_consent_filters_real_organization_records_even_with_workspace_permission` (not opened); [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::revocation_during_a_model_request_stops_lookup_and_prevents_a_second_disclosure` (not opened) |  |
| Historical derived text is not resent after assignment or policy narrowing | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::reassignment_cannot_resend_old_derived_case_text_through_a_parent_goal` (not opened); [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::a_later_planning_round_never_resends_a_case_after_assignment_access_is_lost` (not opened) |  |
| A no-provider option works | proven | [`crates/host/tests/agent_runtime/disclosure.rs`](../../crates/host/tests/agent_runtime/disclosure.rs)`::no_disclosure_sends_zero_provider_requests_and_manual_actions_still_commit` (opened); [`tools/e2e/ai-disclosure.mjs`](../../tools/e2e/ai-disclosure.mjs) `OWNER-native-preview-and-apply-send-no-provider-request` (PASS text read) |  |
| Secrets are absent from prompt and logs | built, unproven | [`docs/modules/AI_DATA_SHARING.md`](../modules/AI_DATA_SHARING.md) | Known credentials are filtered; arbitrary secrets in free text are not classified. |
| Local retention, export, delete and residency of conversation history | absent | [`docs/modules/AGENT_RUNTIME.md`](../modules/AGENT_RUNTIME.md) | No export or delete path for conversations exists. |
| Provider terms, retention and residency validated; Indonesian data-protection obligations | outside the repo | none possible in the repo | Needs the provider agreement and a legal review. |

**`remaining`:**
- Retention, export and delete of agent conversations and goals per an owner policy are absent.
- Arbitrary secrets typed into free text are not classified out of prompts.
- The provider terms, retention and residency, and the Indonesian data-protection obligations, need a legal review by the owner before the policy is recorded per profile.

**Change to the register:** Register `remaining` is accurate except its "fresh release acceptance underway" sentence dated 2026-11-09.

**Profile note:** Same for `global`.

### OPS-01

**Requirement.** Backup terenkripsi, restore ke host bersih, upgrade/migration, audit/alert dan runbook diuji. Tetapkan serta ukur RTO/RPO; single-writer fencing bukan bukti HA, dan restore lokal bukan bukti disaster recovery produksi

Register before: `partial`. After: **`partial`**. Clauses: 2 proven, 2 built-unproven, 1 absent, 2 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Encrypted backup and restore into an isolated database with digests of 31 financial tables | proven | [`tools/e2e/ops.mjs`](../../tools/e2e/ops.mjs) `OPS-native-encrypted-restore-and-login` (PASS text read); [`deploy/NATIVE_INSTALL.md`](../../deploy/NATIVE_INSTALL.md) | Opt-in (`ARCHAVA_E2E_OPS_ROOT`), run on a disposable local cluster, not in CI. |
| Restore to a clean host | outside the repo | none possible in the repo | Needs the chosen production host. |
| Upgrade and migration from earlier packages, with rollback | proven | [`tools/e2e/prior-native.mjs`](../../tools/e2e/prior-native.mjs) `OPS-prior-native-financial-upgrade`, `OPS-prior-native-isolated-rollback` (not opened) | Opt-in and needs a previous package; migrations added after the proof was recorded (for example the tax pack) were stated as not run in it. |
| Audit trail and alerting | built, unproven | [`deploy/monitoring/alerts.yml`](../../deploy/monitoring/alerts.yml); [`deploy/monitoring/prometheus.yml`](../../deploy/monitoring/prometheus.yml) | Templates; no test that an alert fires and no receiver is configured. |
| Runbook exercised | built, unproven | [`deploy/NATIVE_INSTALL.md`](../../deploy/NATIVE_INSTALL.md) | The commands are tested one by one; no full drill exists. |
| RTO and RPO set and measured | absent | [`tools/e2e/ops.mjs`](../../tools/e2e/ops.mjs) | The ops check records capture time only; no target is recorded and no restore time is reported. |
| Disaster recovery on the production host | outside the repo | none possible in the repo | Deployment is `deferred_by_owner_until_website`. |

**`remaining`:**
- No RTO or RPO target is recorded and the ops check reports no restore time or data age.
- No test makes an alert fire or confirms a receiver, and the runbook has not been drilled end to end.
- No production host is chosen, so a clean-host restore and a disaster-recovery drill cannot run; the owner chooses the host.
- The upgrade proof from a previous package is not recorded for the current commit.

**Change to the register:** Register `remaining` is accurate. Evidence cited two agent findings that record a byte budget and a load miss, not operations; they are dropped.

**Profile note:** Same for `global`.

### OPS-02

**Requirement.** Sizing, workload, concurrency, data volume dan konfigurasi dikunci. Laporkan p50/p95, errors, saturation, recovery dan biaya; pertahankan target Commerce 50 ms yang masih terbuka sampai ada perubahan requirement beralasan dan tercatat

Register before: `partial`. After: **`partial`**. Clauses: 1 proven, 1 built-unproven, 1 absent, 1 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Fixed synthetic scale and workload (10,000 contacts, 2,000 products, 500 reads, 200 writes) | proven | [`tools/e2e/load.mjs`](../../tools/e2e/load.mjs) `LOAD-authenticated-data-and-mutations` (PASS text read) | Opt-in (`ARCHAVA_E2E_LOAD=1`); not in CI. |
| p95 and errors measured against the target | built, unproven | [`tools/e2e/load.mjs`](../../tools/e2e/load.mjs); [`docs/product/PRD_AGENT_CONTEXT_FINDING_2026-11-08.md`](PRD_AGENT_CONTEXT_FINDING_2026-11-08.md) | The register records a miss (55.98 ms) and then five passes; nothing reruns it on the current source. |
| Saturation, recovery under load, cost per workload | absent | none found |  |
| Selected hardware and production profile | outside the repo | none possible in the repo | Needs the production host choice. |

**`remaining`:**
- No mixed workload with finance reports and agent turns, and no saturation curve, exists.
- The Commerce p95 target has not been re-run on the current source; its last records are from 2026-11-08.
- Sizing, hardware and cost per workload need the production host chosen by the owner.

**Change to the register:** Register `remaining` is accurate but each entry names source from 2026-11-08.

**Profile note:** Same for `global`.

### EXT-01

**Requirement.** Each offered industry capability has a complete supported lifecycle, correction, negative authority, finance links and user evidence.

Register before: `open`. After: **`open`**. Clauses: 2 proven, 0 built-unproven, 1 absent, 2 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Offered industry profiles are defined | outside the repo | [`docs/product/PRD_DELIVERY_REGISTER.v1.json`](PRD_DELIVERY_REGISTER.v1.json) | `offeredIndustryProfiles` is empty; the owner decisions of 2026-11-08 and 2026-11-14 keep breadth without choosing a first industry. |
| Lifecycle and correction per module (19 modules have host suites) | proven | [`crates/host/tests/agent_runtime.rs`](../../crates/host/tests/agent_runtime.rs)`::all_eighteen_business_domains_supply_authorized_agent_context_in_both_languages` (not opened); [`config/modules.v1.json`](../../config/modules.v1.json) | Depth varies: commerce, purchasing, billing and accounting are deep; laundry, field services and education have two tests each. |
| Negative authority per module | proven | [`crates/host/tests/erp_purchasing.rs`](../../crates/host/tests/erp_purchasing.rs)`::missing_purchasing_grant_refuses_the_ladder` (not opened); [`crates/host/tests/erp_billing.rs`](../../crates/host/tests/erp_billing.rs)`::insufficient_grants_refuse_before_any_statement` (not opened) | The pattern repeats per suite. |
| Finance links for the offered modules | absent | [`docs/architecture/MULTI_CURRENCY.md`](../architecture/MULTI_CURRENCY.md); [`docs/product/BUSINESS_CAPABILITY_MATRIX.md`](BUSINESS_CAPABILITY_MATRIX.md) | Only trade, billing, stock and purchasing post to the ledger; subscriptions, assets, accommodation, foodservice and laundry have none, and manufacturing and payroll are deferred. |
| User evidence per offered capability | outside the repo | none possible in the repo | Needs pilot users. |

**`remaining`:**
- The owner has not named the offered industry profiles (the PRD suggests trade and project services).
- Subscriptions, assets, accommodation, foodservice and laundry have no finance links, and none has an acceptance journey like the store finance cycle.
- Pilot users have not produced evidence for any offered capability.

**Change to the register:** Register status `open` is right.

**Profile note:** Same for `global`.

### OWN-01

**Requirement.** Owner dapat melihat current business profile, installed/enabled capabilities, configurable options, permissions yang ia miliki, dan kebutuhan yang belum tersedia. Menampilkan katalog tidak memberikan entitlement atau grants.

Register before: `open`. After: **`partial`**. Clauses: 3 proven, 1 built-unproven, 1 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Installed, enabled and configurable capabilities are shown | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::disabling_the_pipeline_refuses_the_backend_and_enabling_restores_it` (opened); [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-rendered-preview` (PASS text read) | Maps to OWN-T10 (inspect) and OWN-T01 (c). |
| The owner's own permissions (`grants.read`, `grants.apply`) and a read-only state | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_read_only_owner_previews_but_cannot_apply_or_restore` (not opened); [`tools/e2e/verify.mjs`](../../tools/e2e/verify.mjs) `OWNER-native-read-only-owner` (not opened) |  |
| Showing the catalogue grants no entitlement or permission | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_never_granted_or_revoked_principal_is_refused_on_every_owner_route` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::crm_presentation_uses_current_labels_without_granting_owner_access` (not opened) | OWN-T04. |
| A current business profile (what kind of business this is) | absent | none found | The configuration holds an enabled set and labels; the sentence the owner typed is parsed and not stored. |
| Unmet needs: what is not available or not ready | built, unproven | [`crates/host/src/own.rs`](../../crates/host/src/own.rs) | The fields exist, but every module is entitled, installed and `Ready`, so no module is ever unavailable or `needs_setup` and the display is vacuous. |

**`remaining`:**
- A business profile (the owner's description and the modules it implies) is not stored or shown.
- Every module is `Ready`, so unmet needs (no book, no policy, no tax code) can never appear.
- No browser check asserts the unmet-needs display, which cannot be produced yet.

**Change to the register:** Register status `open` with empty evidence was stale: inspect is built and tested in the host suite and the browser.

**Profile note:** Same for `global`.

### OWN-02

**Requirement.** Archava menanyakan hanya business details yang belum jelas, kemudian menghasilkan structured requirements. Jangan memaksa memilih satu label industri untuk bisnis hybrid atau meminta owner memahami crate/schema/env vars.

Register before: `open`. After: **`partial`**. Clauses: 2 proven, 1 built-unproven, 2 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| A hybrid business in one sentence yields a module set (hotel plus laundry, no restaurant) | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::chat_and_dashboard_agree_on_business_sentences_and_blocked_plans` (not opened); [`crates/host/tests/product_workspaces.rs`](../../crates/host/tests/product_workspaces.rs)`::thirteen_workspaces_read_real_scoped_rows_and_fail_closed_on_current_grants` (not opened) | A keyword parser (`parse_chat`) in Indonesian and English. |
| Structured requirements are produced (enable, disable, labels) | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::the_chat_plan_equals_the_dashboard_plan_and_applies` (not opened) | OWN-T02. |
| Asks only what is unclear (clarifying questions) | absent | [`crates/host/src/own.rs`](../../crates/host/src/own.rs) | Unrecognised input is a 400 with a fixed hint; there is no question loop. |
| A language model proposes structured input | absent | none found | No owner-configuration tool exists in the agent runtime; OWN-03 allows a model only to propose input. |
| The owner never needs crate, schema or environment vocabulary | built, unproven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-chat-manual-plan-identity` (PASS text read) | The sentence vocabulary is business words; no user has tried it. |

**`remaining`:**
- No question loop asks about unclear or missing details (for example whether a hotel serves food).
- No model-backed proposer exists that outputs the same typed request with the same review.
- Real owners' sentences, from pilot users, are not yet test data.

**Change to the register:** Register status `open`: the hybrid and structured-request clauses are tested.

**Profile note:** Same for `global`.

### OWN-03

**Requirement.** Backend menyusun immutable proposed configuration dari trusted catalogue dan current state. Dependency resolution, compatibility, permissions, data lifecycle, readiness dan pricing dievaluasi deterministik. LLM hanya mengusulkan structured input; tidak menjalankan schema edits atau arbitrary code.

Register before: `open`. After: **`partial`**. Clauses: 4 proven, 1 built-unproven, 2 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| An immutable plan with a stable identity from catalogue, pricing and current revision | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::the_preview_states_changes_readiness_and_cost_and_writes_nothing` (opened); [`crates/own/tests/engine.rs`](../../crates/own/tests/engine.rs) | The plan hash is FNV-1a, a product identity and not a security boundary (`crates/own/src/lib.rs`). |
| Dependency resolution; a mandatory module cannot be disabled | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::disabling_a_required_module_is_blocked_and_nothing_is_written` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_mandatory_module_is_blocked_and_nothing_is_written` (not opened) |  |
| Source-policy pins block disabling (accounting is pinned by stock, purchase and invoice policies) | proven | [`crates/host/tests/erp_accounting/purchase_sources.rs`](../../crates/host/tests/erp_accounting/purchase_sources.rs)`::purchase_policy_pins_owner_dependencies_and_financial_sources_remain_permission_scoped` (not opened); [`crates/host/tests/erp_accounting/stock_sources.rs`](../../crates/host/tests/erp_accounting/stock_sources.rs)`::stock_policy_alone_pins_owner_dependencies_without_a_purchase_or_invoice_policy` (not opened) |  |
| Compatibility between modules (incompatible pairs) | absent | [`crates/own/src/lib.rs`](../../crates/own/src/lib.rs) | `BlockedReason` has no incompatible variant: only requires, mandatory, entitled, installed and unavailable. |
| Data lifecycle: records and jobs that disabling affects | absent | none found |  |
| Readiness and pricing evaluated | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_confirmation_from_a_previous_catalogue_or_pricing_refuses` (opened) | Identity is checked, but pricing is the constant `included@1` and readiness is `Ready`; nothing real is evaluated. |
| A model cannot add fields, roles, approvals, readiness or prices | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_foreign_tenant_role_or_approval_is_refused_without_writing` (not opened); [`crates/own/tests/engine.rs`](../../crates/own/tests/engine.rs) | Over HTTP only `role` and `approval` are tested (422); `readiness`, `cost` and `price` are tested in the engine only. |

**`remaining`:**
- No HTTP test asserts that forged `readiness`, `cost` and `price` fields on `/own/plan` are refused.
- The catalogue has no incompatibility rule and no data-lifecycle rule, and no statement says none exists yet.
- Readiness and pricing inputs are constants, so nothing real is evaluated.
- No test pins a resource between plan and apply (item 14 of the owner test map).

**Change to the register:** Register status `open`; the owner test map was corrected and now lists items 12, 14, 17 and 18 as still without a test.

**Profile note:** Same for `global`.

### OWN-04

**Requirement.** Tampilkan dalam bahasa bisnis: apa yang ditambah/diubah, fitur/UI/workflows yang terdampak, dependencies, active records/jobs yang perlu diselesaikan, biaya implementasi/recurring/usage bila berlaku, perkiraan gangguan, dan recovery path. Missing price/readiness menjadi explicit unknown/review state.

Register before: `open`. After: **`partial`**. Clauses: 2 proven, 4 built-unproven, 2 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| What is added or changed, rendered from the host plan, writing nothing | proven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-rendered-preview` (PASS text read); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::the_preview_states_changes_readiness_and_cost_and_writes_nothing` (opened) | Three tables are unchanged after a preview. |
| Dependencies and block reasons | proven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-blocked-plan-is-shown-and-never-applied` (PASS text read) |  |
| Affected features, screens and workflows (beyond the module name) | built, unproven | [`crates/own/src/lib.rs`](../../crates/own/src/lib.rs) | An effect carries module, readiness and cost class only. |
| Active records and jobs that must be settled before a change | absent | none found |  |
| Implementation, recurring and usage cost amounts | built, unproven | [`crates/own/src/lib.rs`](../../crates/own/src/lib.rs) | `CostClass` has included, recurring, usage and review; the host catalogue is all `included` and there are no amounts. |
| Expected disruption and the recovery path | absent | none found | Restore exists as a separate screen; the preview does not state it. |
| Missing price or readiness appears as an explicit unknown or review state | built, unproven | [`crates/own/src/lib.rs`](../../crates/own/src/lib.rs) | `CostClass::Review` and `Readiness::NeedsSetup` exist; no test makes the host produce them. |
| Business language rather than ids | built, unproven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-rendered-preview` (PASS text read) | Module ids are shown; no wording review was done. |

**`remaining`:**
- The plan and screen do not state records and jobs to settle, expected disruption or the recovery path.
- Cost has classes but no amounts, and no test shows the explicit `Review` or `NeedsSetup` state that the host never produces.
- The preview names modules by id and has had no wording review.

**Change to the register:** Register status `open`. Its evidence cited an Ava byte-budget finding that does not support this requirement; dropped.

**Profile note:** Same for `global`.

### OWN-05

**Requirement.** Persetujuan terikat tenant/principal, operation, plan/input hash, base config revision, catalogue/release compatibility, pricing snapshot bila relevan, affected resources dan expiry. Tidak ada `approved: true` dari model/body yang dapat menggantikan server-verifiable approval.

Register before: `open`. After: **`partial`**. Clauses: 5 proven, 1 built-unproven, 1 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Bound to tenant and principal | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_foreign_tenant_role_or_approval_is_refused_without_writing` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::another_principal_cannot_read_or_replay_a_foreign_request_id` (not opened) |  |
| Bound to plan hash and operation | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_stale_base_and_a_foreign_confirmation_refuse_without_writing` (not opened) | A wrong hash is `confirmation_mismatch`. |
| Bound to base revision | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_stale_base_and_a_foreign_confirmation_refuse_without_writing` (not opened); [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-two-tab-stale-plan-refused` (PASS text read) | `stale_base`. |
| Bound to catalogue and pricing identity | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_confirmation_from_a_previous_catalogue_or_pricing_refuses` (opened) | `catalogue_changed` and `pricing_changed`. |
| Bound to affected resources | built, unproven | [`crates/host/tests/own/disclosure.rs`](../../crates/host/tests/own/disclosure.rs)`::owner_disclosure_companies_are_resolved_from_current_real_company_grants` (not opened) | Proven for AI sharing (companies, provider account, model). For module toggles the affected resource is the module set, already in the hash. |
| Expiry of a configuration plan | absent | [`crates/host/src/own.rs`](../../crates/host/src/own.rs) | No expiry in the plan or the confirmation; only the AI-sharing review has a ten-minute signed lifetime. |
| No `approved: true` or role from the body | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_foreign_tenant_role_or_approval_is_refused_without_writing` (not opened); [`crates/host/tests/own/disclosure.rs`](../../crates/host/tests/own/disclosure.rs)`::owner_disclosure_approval_binds_whole_policy_actor_expiry_and_one_effect` (not opened) | Unknown body fields are 422. |

**`remaining`:**
- A configuration plan and its confirmation have no expiry, so an old confirmation stays valid while the revision and catalogue are unchanged.
- The approval has no keyed signature (the plan hash is FNV-1a) and the owner has not decided whether it needs one like the AI-sharing review.

**Change to the register:** Register status `open`.

**Profile note:** Same for `global`.

### OWN-06

**Requirement.** Backend mengecek ulang current authority dan preconditions, mengklaim request idempotency, lalu menerapkan melalui supported transaction/job. Tampilkan progress/outcome dari authoritative backend. “Applied” hanya setelah actual runtime configuration/activation yang relevan terkonfirmasi.

Register before: `open`. After: **`partial`**. Clauses: 6 proven, 0 built-unproven, 0 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Current authority rechecked at apply, including after revocation and on replay | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_missing_or_revoked_grant_refuses_the_operation` (opened); [`tools/e2e/verify.mjs`](../../tools/e2e/verify.mjs) `OWNER-native-revoked-owner-refused` (not opened) |  |
| Preconditions rechecked: stale base, wrong hash, catalogue, pricing, blocked plan | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_confirmation_from_a_previous_catalogue_or_pricing_refuses` (opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_mandatory_module_is_blocked_and_nothing_is_written` (not opened) |  |
| Idempotency claim: duplicate submission and racing applies give one revision, one receipt, one audit row | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_duplicate_submission_replays_one_receipt_and_one_effect` (opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::racing_applies_converge_to_one_revision_and_one_receipt` (opened) |  |
| Applied in one database transaction with receipt and audit | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_duplicate_submission_replays_one_receipt_and_one_effect` (opened) | Row counts in `config_revisions`, `apply_receipts` and `config_audit` are asserted together. |
| Outcome shown from the backend after apply and after a lost response | proven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-disable-lost-response-recovery` (PASS text read); [`tools/e2e/verify.mjs`](../../tools/e2e/verify.mjs) `F05-owner-confirmation` (not opened) |  |
| "Applied" only once the module toggle governs runtime requests | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::disabling_the_pipeline_refuses_the_backend_and_enabling_restores_it` (opened); [`crates/host/tests/product_workspaces.rs`](../../crates/host/tests/product_workspaces.rs)`::thirteen_workspaces_read_real_scoped_rows_and_fail_closed_on_current_grants` (not opened) | The next request after apply is refused or served by the new configuration; no asynchronous activation exists to wait for. |

**`remaining`:**
- No dated acceptance record names a commit for OWN-06 together with the full Rust gate and the browser gate that ran on it; acceptance needs one.
- Every module in the host catalogue is entitled, installed, `Ready` and priced `included@1` (`crates/host/src/own.rs`), so no module needs setup or activation and "applied only after activation" is satisfied by an immediate toggle; acceptance needs a catalogue whose readiness and cost facts are not constants, or an owner decision that constants are the intended state of this release.

**Change to the register:** Register status `open` with empty evidence was stale: all six clauses have tests. The weakest acceptance in the register, because the activation clause is satisfied vacuously.

**Profile note:** Same for `global`.

### OWN-07

**Requirement.** Setelah refresh, connection loss atau worker restart, owner dapat mengecek request yang sama tanpa double activation/charge. Unknown outcome direkonsiliasi sebelum retry. Current authority tetap diperiksa untuk status/replay.

Register before: `open`. After: **`partial`**. Clauses: 3 proven, 2 built-unproven, 0 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| A committed request reads back its receipt after a lost response | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_committed_request_id_reads_back_its_receipt_after_a_lost_response` (not opened); [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-disable-lost-response-recovery` (PASS text read) | The browser drops the real commit acknowledgement and a reload recovers the same receipt. |
| No double activation on retry or replay | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_duplicate_submission_replays_one_receipt_and_one_effect` (opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::racing_applies_converge_to_one_revision_and_one_receipt` (opened) | No charge exists to double (every cost is `included`). |
| An uncommitted id answers `unknown` and writes nothing; the page keeps the same id | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::an_uncommitted_request_id_answers_unknown_and_writes_nothing` (not opened); [`crates/slice/src/owner.rs`](../../crates/slice/src/owner.rs) | The API is proven; no browser check covers an id the server never committed (item T06-g of the owner test map). |
| Authority checked on status and replay | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_missing_or_revoked_grant_refuses_the_operation` (opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_never_granted_or_revoked_principal_is_refused_on_every_owner_route` (not opened) |  |
| Server restart between commit and recovery | built, unproven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::a_rebuilt_host_recovers_the_same_receipt_and_history_from_postgres` (opened) | A rebuilt router over the same database, not a process restart; the browser gate restarts the server for other modules, not for the owner step. |

**`remaining`:**
- No browser or process-level check applies an owner change, restarts the server and recovers the same receipt.
- No browser check retries a pending id the server never committed and asserts one effect.

**Change to the register:** Register status `open`. One step from acceptance: only the restart and unknown-id checks are missing.

**Profile note:** Same for `global`.

### OWN-08

**Requirement.** Tampilkan versi konfigurasi dan audit perubahan. Restore supported prior configuration membuat versi baru setelah compatibility check; tidak berarti mengembalikan seluruh database ke masa lalu atau membatalkan payment, stock, invoice, agreement dan pekerjaan bisnis yang sudah terjadi.

Register before: `open`. After: **`partial`**. Clauses: 5 proven, 1 built-unproven, 0 absent, 0 outside the repo.

| Clause | Class | Evidence | Note |
| --- | --- | --- | --- |
| Versions and audit rows are readable, with request id, plan hash, actor and outcome | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::restore_keeps_historical_labels_traceable_and_only_appends` (opened) | The assertions on revision, actor and plan hash and the append-only check on three tables were read. |
| The history screen shows them | built, unproven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-compatible-restore` (PASS text read) | The browser waits for the restore control per revision; history row content (actor, outcome) is not asserted. |
| Restore creates a new revision and removes labels the target never had | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::restore_lands_as_a_new_revision_and_keeps_history_and_business_rows` (not opened); [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::restore_removes_the_labels_the_target_snapshot_never_had` (not opened) |  |
| Compatibility is checked before restore | proven | [`crates/own/tests/engine.rs`](../../crates/own/tests/engine.rs); [`crates/host/tests/own/disclosure.rs`](../../crates/host/tests/own/disclosure.rs)`::owner_disclosure_provider_account_and_model_changes_require_new_review_but_offline_revoke_works` (not opened) | The engine proof uses a synthetic catalogue; over HTTP the catalogue is constant, so only the AI-sharing path shows a blocked restore. |
| Business and financial history is not rolled back | proven | [`tools/e2e/owner-configuration.mjs`](../../tools/e2e/owner-configuration.mjs) `OWNER-native-label-restore-preserves-history` (PASS text read) | Per-table row counts and md5 digests of 23 CRM and accounting tables are unchanged; the accounting tables are non-empty at that point. |
| Earlier receipts, audit rows and grants unchanged after restore; historical labels traceable | proven | [`crates/host/tests/own.rs`](../../crates/host/tests/own.rs)`::restore_keeps_historical_labels_traceable_and_only_appends` (opened) |  |

**`remaining`:**
- The browser check does not assert the history screen's rows (actor, outcome, revision, label changes).
- No HTTP test blocks a restore whose target now violates a dependency, because the catalogue cannot yet be moved in a test.

**Change to the register:** Register status `open`. One step from acceptance. Evidence cited an unrelated Ava finding; dropped.

**Profile note:** Same for `global`.

## 8. What was not verified

- Nothing was run. "Proven" means the named test or check asserts the clause in the source at the commit above; it was not seen to pass, apart from the CI summary in the status block.
- Absent features were found by name search (requisition, MFA, avatar, unit of measure, credit notes, prompt-injection tests); a feature under another name would be missed.
- A live Gemini run, the opt-in upgrade proof and the load run are not recorded in the repository for this commit, so none counts as evidence here.
