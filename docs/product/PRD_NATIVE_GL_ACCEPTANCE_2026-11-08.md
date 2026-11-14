# Native GL foundation — local acceptance, 8 November 2026

This accepts the bounded [native book contract](../modules/ACCOUNTING.md),
not the whole PRD or a complete financial product. Source:
`fe4ec91c74f6d42886b662a55f028b88d5229f976f5617bae001f0a0481ba1f5`
(489 inputs). Existing dated candidates and reports remain unchanged.

The complete Rust gate passed 1,261 tests in 99 suites, 29 Node helper tests,
formatting, Clippy and WASM boundaries. Eleven PostgreSQL financial scenarios
cover current authority, independent posting, exact amounts, immutable records,
periods, reversals, races, full filtered reports, old-profile upgrade and inactive
company completion. All databases are disposable fixtures.

Fresh native/server/preview/WASM builds passed. Candidate
`dist/native-candidate-20261108-prd-native-accounting-01` contains 75 verified
artifacts. Chromium run `target/e2e/prd-native-accounting-20261108-04/` passed
143 root checks, 77 scoped product probes and 30 owned Ava checks, with no
JavaScript errors and fixture cleanup exit 0. Desktop and 390px financial
screenshots were visually inspected.

Encrypted restore preserved complete digests for all eight financial tables,
then served readiness, login and balanced financial reports. The actual previous
supplier-document distribution created the upgrade input; the new CLI applied
only the two declared migrations, reapplied zero, preserved source rows and
receipts, and replayed the original command. Existing owners received no implicit
financial grants or books. The previous encrypted backup also restored into a
separate database and served through the previous binary.

Build, gate, browser, helper and package hashes are recorded together in
`target/prd-completion-20261107/native-gl-verification-01.json`. Browser attempts
01–03 retain failures from incorrect helper API/response assumptions; attempt 04
is the complete passing run. Those corrections changed browser helpers only.

Manual journals do not prove operational source posting, AP/AR, bank
reconciliation, subledger close, tax/localization, opening import or external
accounting. The owned media/provider fixtures do not prove live provider or
actual-user acceptance. Production-host RTO/RPO, release profile and inherited
obligations remain open in the [delivery register](PRD_DELIVERY_REGISTER.v1.json).
No public deployment occurred. Work continues beyond this foundation.
