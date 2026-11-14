# Stock source functional proof and mobile finding — 8 November 2026

Source `03021cfedccbb1afe302609d2f41bc3b8a46e5fa9524e40b1b81396a0de22390`
contains 506 serving inputs. The full offline gate passed 1,302 Rust tests in
99 result suites, 36 helper tests, formatting, Clippy and both WASM boundaries.
The native server, fixture preview and WASM build completed; distribution
`dist/native-candidate-20261108-prd-stock-source-01` verified all 80 artifacts.

The actual Chromium/native run
`target/e2e/prd-stock-source-20261108-01/result.json` passed 175 root checks,
77 scoped product probes and 30 Ava checks, with no page errors and owned
PostgreSQL cleanup exit 0. Evidence is bound in
`target/prd-completion-20261107/stock-source-functional-verification-01.json`;
its exact public tooling snapshot is `stock-source-helpers-03` beside it.
The result does not accept stored Accounting data disclosure to an AI provider.

The new [original stock cost bridge](../modules/STOCK_COST_POSTING.md) captured
actual acquisition costs, retained the original inventory/COGS accounts on
restock after a policy change, created no monetary reversal for discard, refused
wrong review accounts and posted separately reviewed source journals. Native
restart retained the policy, all four stock sources, three posted stock journals
and the exact COGS amount. Encrypted backup/restore compared all 19 financial
table digests and served an independently checked stock report after restore.

The real previous native distribution created purchase receipt/bill journals,
acquired cost layers, fulfillment and a legacy restock before migration.
Upgrade applied exactly the two declared migrations, preserved its 15 original
financial tables and all original cost records, attached only the exact acquired
layer's inventory-account metadata, and invented no stock policy or historical
COGS posting. The encrypted prior backup also restored into a separate database
and served with the prior binary. Legacy return lineage was not guessed.

Five local read repetitions used 10,000 synthetic contacts, 2,000 products and
200 unique contact writes. Commerce p95 was 12.69/12.20/12.16/12.13/12.23 ms;
Contacts p95 was 3.45/2.96/2.45/2.18/2.31 ms. Every measured target passed
without request errors. This remains a scoped local workload, not production
sizing, availability or a hosted recovery guarantee.

Actual visual review retained a counterexample: purchase/stock source summaries
now lead with exact amounts and original accounts, but the mobile journal table
is too dense. Purpose/account text breaks into short fragments and a credit
column requires internal horizontal scrolling. The desktop purchase debit also
wraps its currency onto a separate line. The browser's document-overflow check
did not catch that internal table problem.

Readability acceptance therefore remains open. The next change must preserve
the same exact source amounts, account references, approval behavior and table
semantics while presenting complete account/debit/credit information at the
target mobile width. New native/WASM/browser screenshots must accept that new
source. This functional result must not be silently relabelled as final visual
acceptance or whole-product completion.
