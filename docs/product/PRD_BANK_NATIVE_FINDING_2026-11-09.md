# Bank native receipt finding — 9 November 2026

This is counterevidence, not native bank acceptance. The source was
`ce17f01bb23979bea2091808ea9cabd1ac355d49ae839b6a2fbeb3057740f33a`.
Its full gate passed 1,367 Rust tests in 99 suites and 36 helper tests, both
WebAssembly checks and lint. A fresh 84-artifact native package was verified.
Those results did not establish the new bank browser journey.

## Actual browser failure

The focused Chromium journey registered a bank, configured independent review,
imported positive/negative decimal amounts with exact control totals, and matched
both transactions to actual posted cash. The native unmatch then reported a
missing matching acknowledgement.

A second reproduction retained the actual owned HTTP response and PostgreSQL
command/audit before fixture cleanup. The server returned **200**. Its one stored
command receipt and immutable audit identified the new `bank_unmatch`; the audit
record's `allocation_id` identified the original match. No second unmatch was
needed. The UI could not acknowledge the original-to-new resource relationship.
The generic financial resource helper also displayed the company ID for bank
resources because its financial-kind list omitted them.

Two regression tests on the original implementation failed: the original-match
binding and the new resource display. An earlier isolated fixture probe returned
403 before exercising bank actions because it lacked Organization memberships;
that separate setup failure is not a product bank failure.

Owned evidence lives under `target/prd-completion-20261107/`:
`bank-unmatch-receipt-finding-01/actual-wire-and-sql.json`,
`bank-receipt-negative-01.log`, and the three focused browser output directories
under `target/e2e/prd-bank-controls-focused-20261109-*`. They contain synthetic
data only. Every failed browser fixture reported successful cleanup and no
JavaScript/WASM runtime error.

## Repair and acceptance boundary

The repair adds an explicit optional original bank allocation reference to the
receipt. Status/replay derives missing legacy references from the immutable
unmatch without rewriting historical receipt/audit bytes. Native acknowledgement
binds that original reference, operation and new resource kind. Financial
resource display recognizes the bank and purchase/stock policy kinds. Source
journal acknowledgements bind all three source types; bank confirmation also
binds its reviewed evidence hash.

The source inspection also found a fixed 100-item native array limit against a
1,000-row bank contract. The form now follows the declared limit and disables Add
at that bound. Fresh browser evidence must establish an actual 101-row save and
the 1,000-item control boundary; a two-row import cannot establish those cases.

After the repair, five focused native receipt tests passed, including the two
original failures and mismatched-source/hash refusal. Full new-source integration,
release build, actual browser, installed-role upgrade and encrypted recovery are
still pending. Neither this finding nor the earlier 19-table/84-artifact candidate
accepts the repaired source or complete financial/subledger readiness.

See [manual bank controls](../modules/MANUAL_BANK_RECONCILIATION.md) and the
[delivery register](PRD_DELIVERY_REGISTER.v1.json).
