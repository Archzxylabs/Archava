# Manual bank controls — local acceptance, 11 November 2026

This accepts the named synthetic manual-bank profile on source
`efbedbf4324eb23795f46120b8b19ecedd1f23b1919cf89c3850d392620cfa5f`.
It does not accept the whole PRD, all business profiles or production deployment.
The [native receipt failure](PRD_BANK_NATIVE_FINDING_2026-11-09.md) remains preserved
as counterevidence for the preceding source.

## Profile and observed behavior

A human explicitly configures a managed company, native USD book, cash account,
manual bank registration and reconciliation starting period. This profile imports
owned synthetic statements; it executes no bank transfer and uses no bank secret.

The actual Chromium/SSR/WASM/HTTP/PostgreSQL journey verifies signed decimal entry,
complete import totals, exact database amounts, replay, matches to posted cash,
immutable unmatching and reviewed replacement. A deliberately mangled unmatch
acknowledgement stays uncertain, then recovers the authoritative original request
without another write. An importer sees the independent-review restriction;
attempting native save receives **403** and creates no attestation.

An independent reviewer sees complete coverage, opening/closing bank versus book,
remaining statement amounts and unresolved cash entries. A concurrent real
unmatch/rematch preserves money totals but changes the evidence hash; the stale
review is refused. A fresh review and replay create one attestation. November
closes only after the complete control is reviewed.

A separate future December statement saves **101 rows**, each USD 0.01, with
exact movement `101` minor units and closing `10101`. This import creates no
journal, remains unmatched, and does not invalidate the reviewed November period.
The actual Add control supports 1,000 draft rows, disables at that bound and
cannot append a 1,001st. This is not a claim that 1,000 rows were submitted.
Desktop and 390px mobile review artifacts were inspected; balances, check labels
and collapsed technical references remain readable without page overflow.

## Evidence and recovery

| Check | Observed evidence |
| --- | --- |
| Workspace gate output | 1,371 Rust tests / 99 suites; no failures or ignores; 36 helper tests, formatting, all-target Clippy and both WASM checks |
| Native package | 84 verified artifacts; current source and all four serving artifacts match build provenance |
| Focused Chromium journey | 11 passed checks; no runtime errors; fixture cleanup 0 |
| Complete current browser gate | **204** passed checks, 77/77 product probes, 30 Ava checks; errors and page errors empty; process exit 0 |
| Restore | 26 financial-table row/digest comparisons; all seven bank tables populated; closed bank proof/hash survives native restore; external effects remain held |
| Prior native upgrades | Two actual prior distributions add 6 and 4 declared migrations; original rows/receipts survive; no automatic bank data or new grants; isolated encrypted rollback works |
| Legacy bank receipt | Actual preceding bank executable commits the old receipt format; repaired executable recovers/replays it on the same SQL profile, with unchanged stored receipt/audit and one unmatch |
| Reuse | Two separate public-crate consumers / 12 pure tests; two independently built persistent clients / 4 tests and 11 configured native checks; all owned scratch/database cleanup verified |
| Load | Five repetitions each: Contacts p95 2.21–2.70 ms, Commerce 11.18–12.09 ms; 200 writes / 200 receipts, p95 3.25 ms; no errors; local targets met |

The old full-gate/build handles were unavailable when work resumed on 11 November.
Their complete logs and build provenance were inspected and bound to the current
source; their process exit codes are recorded as **null**, not invented. The new
package and complete browser/persistent runners have explicitly captured exit 0.

Native package: `dist/native-candidate-20261111-prd-bank-controls-02`.
Release-profile SHA-256:
`3dace850c13f86dc16b2c1d6e28123063a248386812556eda8506c87fb97bfa1`.
Local evidence: `target/e2e/prd-bank-controls-20261111-01/result.json`,
`target/e2e/prd-bank-controls-focused-20261111-05/result.json`, and
`target/prd-completion-20261107/bank-persistent-consumers-02/result.json`.
Frozen shipping inputs and helper files are retained locally as
`bank-source-04-tree` and `bank-helpers-02` in that evidence root.

Core archive SHA-256:
`91e849f5e33453556e1d73c1c6b18e6ad2a1b2fd6036764468e8c3236fc5769b`.
Domain archive SHA-256:
`1ce366c52bb0cac000f8efa03a7c8298713f4140af92b8466e8ff5088781422d`.
Both current persistent clients use these exact archives.

## Remaining requirements

Supplier payments, gateway clearing-to-bank settlements, fee/difference handling,
terms/aging, opening imports, complete subledger/stock close, real bank formats,
statement corrections, localization and real-user/profile acceptance remain.
No live Google planning or live-bank acceptance is implied by owned transport tests.
The current multi-ledger downgrade test does not isolate the Owner migration as
its sole refusal cause. Production host sizing, offsite recovery and RTO/RPO remain
separate acceptance. Public deployment stays deferred by the owner.

FIN-04 and FIN-05 remain **partial** in the
[delivery register](PRD_DELIVERY_REGISTER.v1.json). Continue the entire PRD;
[manual bank controls](../modules/MANUAL_BANK_RECONCILIATION.md) document this
implementation boundary.
