# Native gateway-fee account revision — 2026-11-12

The first complete source gate passed, but the real browser found a supported
positive-fee form could not reach confirmation. Passing source/domain/database
checks did not accept this native customer workflow.

The affected source was
`037f10ec3d50554eb28cca6e7bd7cfbf9787561f9df54a3da08ec8116a75fdbd`.
Its full gate passed 1399 Rust tests/99 suites and its native package verified 86
artifacts. The package is preserved as
`dist/native-candidate-20261112-prd-gateway-clearing-01`; its release-profile SHA
is `2fe812e1c7874cf0b3216949a626a11bff243e2d5890867e6bfe4309e77e0a71`.

The first focused run completed the unchanged B01 financial SQL/native oracle,
then its test clicked the report button before it became available. The test now
waits for the enabled current report control. The second run completed the entire
B01 helper, including the report and original settlement receipt metadata. Its
actual intermediate bank/inventory/profit/stock values were 600000/600000/200000/6
whole IDR/units; final values were bank450000, inventory700000, assets and
equity1150000, profit150000, stock7, AR/AP/clearing zero. All three actual bank
rows were matched. This is a bounded synthetic one-book result, not production
or whole-financial-close acceptance.

The separate positive-fee book then failed before a confirmable draft appeared.
The displayed refusal was “Reviewed account version: enter a valid value.”
`feeAccount` is optional and omitted from initial field defaults. Selecting its
account created only `accountId`; revision binding intentionally writes existing
version slots, so the missing `expectedAccountRevision` was never filled. The
customer has no manual internal-version field to repair it.

The fix initializes the optional group's declared fields when a fee becomes
positive, preserves a current positive selection, clears it when the fee becomes
zero, and requires a fresh selection when reenabled. A regression exercises the
actual catalogue defaults, reference/version binding, cleaned native envelope
and strict domain parse; unrelated account versions stay intact. One first test
attempt incorrectly passed the full HTTP envelope to the strict domain parser;
it was corrected to unwrap requestId/command as the host does. That failing log
is retained separately from the actual product finding.

The latest frozen source is
`c7e4b8cff4aa4a3c440fec070cdaa23ba5d2cd58708434f5a79f043fe53113e1`.
Its library gate passed with actual exit zero: 668 tests/22 suites, including all
42 UI tests, plus 36 helper tests, Clippy and both WASM checks. Only
`crates/slice/src/operations/operator.rs` differs from the earlier full-gate source;
backend and migration bytes are unchanged. This library check is not a current
full integration gate. A fresh build/package, unchanged positive-fee native
journey, broader native/recovery/consumer checks, real Gemini journey and a final
current-source full gate remain queued or in progress.

Local failed results, screenshots, SQL proof and actual process exits remain in
`target/e2e/prd-clearing-focused-20261112-01` and `-02`, and the source-bound
`clearing-release-job-*`/`clearing-validation-job-*` records under
`target/prd-completion-20261107`. Never relabel those failed runs as passes. See
[gateway clearing](../modules/GATEWAY_BANK_CLEARING.md) and the
[delivery register](PRD_DELIVERY_REGISTER.v1.json) for the wider incomplete scope.
