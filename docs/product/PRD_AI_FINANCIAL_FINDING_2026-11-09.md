# AI sharing and financial execution finding — 9 November 2026

Candidate `d67999a940d1ccf9987a1b2b9b1c1cade16861595eab7d00c75cf89feb69ec25`
passed 1,331 Rust tests across 99 result suites, 36 gate helper tests, formatting,
Clippy and both WASM boundaries. Its native package contains 81 verified
artifacts. It did **not** pass the complete native browser run and is not an
accepted financial Ava release.

## Observed production-binary behavior

Run `target/e2e/prd-ai-sharing-20261109-03/result.json` passed 130 checks before
the failure, with no browser runtime errors and PostgreSQL cleanup exit 0.
Three new privacy checks passed: native Owner selection and signed confirmation;
actual owned-provider requests without unselected names, prices or raw history;
and native revocation followed by local planning with no further provider calls.
These are bounded synthetic loopback results, not real-provider acceptance.

The financial test then submitted an existing journal for review. Ava displayed
the exact IDR 7051.00 amount and original account names on desktop and mobile.
After explicit approval, the goal entered `holding` instead of completing.
The recorded mission step remained pending without a verified posting receipt.

The finite dispatch in `crates/host/src/business.rs` recognized Accounting tools
through the public catalogue but had no Accounting HTTP-handler branch. Execution
therefore refused before invoking the financial handler. Earlier refusal/stale
tests did not cover a successful Accounting action through this dispatcher.

## Correction and regression

The correction routes Accounting to its existing authenticated command handler.
It changes no financial permission, company scope or posting policy. Applied
SQL remains unchanged.

A new real-PostgreSQL regression first reproduced `holding` instead of
`completed` on the unfixed dispatcher. After correction, all 30 Ava HTTP tests
and all 28 Accounting HTTP tests passed. The new case proves exact posting and
one-effect replay; an independent-posting book still refuses its draft author;
Bob cannot adopt Alice's personal goal, but can post through his own separately
reviewed goal. The resulting journal identifies the actual posting human,
retains two immutable journal audit events and consumes no model attempts.

Visual inspection also found legacy Desk definition-list styles overriding
nested agent facts on mobile. The current correction scopes those styles to the
agent and keeps financial technical references collapsed below the visible
money/account review. Sharing checkbox layout receives its intended grid
specificity. These UI corrections require fresh native/WASM/browser proof.

The new source `2afdf222dadbca2d302db5a5c0e20e9fdf35e2a29d8a71be036ca180781628c0`
is undergoing that verification. The earlier package remains preserved as a
failed candidate; its 1,331 passing tests do not accept the new source.

## Evidence retained

The local evidence root is `target/prd-completion-20261107/`:

- `disclosure-dispatch-finding-01/manifest.json` binds the failing native result,
  the original dispatcher and the independently reproduced negative test.
- `disclosure-accounting-dispatch-before-01.log` records the expected regression
  failure; `disclosure-accounting-dispatch-after-01.log` and
  `disclosure-accounting-dispatch-finance-01.log` record the focused passes.
- Browser runs 01 and 02 retain separate runner/fixture failures: provider testing
  was placed before production restart, and a synthetic SKU violated the existing
  uppercase contract. The runner/order and fixture were corrected without
  relaxing the application contract or privacy assertions. Both cleaned up.

Whole-product completion, exhaustive financial agent workflows, held-out model
quality, full safe history and production operations remain separate obligations.
See [execution work](PRD_EXECUTION_WORK.md) and [AI sharing](../modules/AI_DATA_SHARING.md).
