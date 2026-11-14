# Gateway bank clearing

Accounting010 connects an already posted invoice collection or refund to
actual bank evidence. The source is a verified provider event or, since
Accounting020, a [manual receipt](MANUAL_CUSTOMER_RECEIPTS.md) or its reversal: a
clearing settlement clears a manual collection exactly like a verified one, against
a bank row dated on or after the day the money was received. A cash receipt is
cleared the same way against a statement row of the cash drawer, registered as a
bank account. The clearing source row names its `origin` (`manual` or `gateway`)
and the review words a manual source as manual, never as verified. It does not initiate a transfer or infer a payout from an
invoice status. The original source and policy select the book, clearing account
and direction. Gross and fee values are exact book minor-unit integers.

`clearing_settlement.draft` proposes a journal with the original clearing account,
the bank's current cash account and an explicit operating expense account when
the fee is positive. Collection cash equals gross minus fee; refund cash equals
gross plus fee. This moves clearing into cash without recognizing revenue or AR
again. Grouped payouts, FX and inferred fees require separate contracts.

The native form reviews source, bank row, gross, fee, cash and journal legs before
confirmation. Separate journal posting rechecks current permissions, account
versions, period and any independent actor requirement. Matching uses the exact
posted cash leg and reviewed bank row. Original claims and receipts are immutable.

Draft and posted gross reserve the original collection/refund capacity. Bank
reservations include both gateway settlements and supplier payments; concurrent
claims serialize on the book. Voiding a draft releases its reservation. Unmatching
bank evidence restores its reservation without erasing the posted cash movement.
Generic journal reversal is refused for an actual settlement.

Existing members receive no implicit `clearing.read` or `clearing.write` grants.
The private `clearing_sources` and `clearing_settlements` collections remain
withheld from provider disclosure even when all general data classes are selected.

The isolated implementation passed 237 domain tests, 46 Accounting integration
tests, 41 native view tests and both WASM checks before its root merge. Those
results do not establish acceptance of the combined current source. Full gates,
fresh native browser/upgrade/recovery and the unchanged cross-module business
oracle remain required. See [the delivery register](../product/PRD_DELIVERY_REGISTER.v1.json)
for the wider incomplete finance and agentic requirements.

## Combined verification — 2026-11-12

The merged source
`037f10ec3d50554eb28cca6e7bd7cfbf9787561f9df54a3da08ec8116a75fdbd`
passed the complete current gate with actual exit zero: 1,399 Rust tests in 99
suites, zero ignored tests, 36 helper tests, formatting, workspace all-target
Clippy with warnings denied, and both WASM boundary checks. This includes the
new raw-collection withholding regression and all 46 Accounting integrations.
Local evidence is retained in `clearing-gate-acceptance-01.json` and
`clearing-release-job-01.json` under `target/prd-completion-20261107`.

The release build and subsequent real browser, live Gemini, declared upgrade,
encrypted 28-table recovery and independent consumer acceptance remain in
progress. The new unchanged B01 and explicit gateway-fee browser journeys are
prepared but have not yet executed. This gate result alone does not accept those
journeys or complete FIN-04/FIN-05 or the whole PRD.

The later corrected source c7e4 passes the positive-fee native journey, B01,
production restart, upgrade/recovery and the final current-source full gate.
[Current bounded acceptance](../product/PRD_GATEWAY_CLEARING_ACCEPTANCE_2026-11-12.md)
records1400 Rust tests,215 broad native checks,28 restored financial tables and
remaining discovery/finance/provider limits. Earlier source037f and its failed
fee workflow remain separate immutable evidence.
