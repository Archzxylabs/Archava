# Invoice source posting — bounded local acceptance, 8 November 2026

Accepted scope: [future whole-IDR invoice posting](../modules/INVOICE_POSTING.md),
at source `f88831b033be413d9adb80c1f6b33bff1a17e63b812eaf0e4b2ab4f3301bb9dd`
(494 inputs). This does not accept full FIN-03, the whole PRD or production release.

The complete Rust gate passed 1,269 tests in 99 suites, 29 Node helper tests,
formatting, Clippy and both WASM boundaries. The financial integration suite has
17 passing scenarios, including six source/adapter/authority/race/readiness cases.
Fresh native/server/preview/WASM builds and the 77-artifact candidate
`dist/native-candidate-20261108-prd-invoice-posting-01` passed verification.

Chromium run `target/e2e/prd-invoice-posting-20261108-02/` passed 154 root checks,
77 scoped product probes and 30 owned Ava checks, with zero JavaScript errors and
fixture cleanup exit 0. Actual desktop and 390px source-report screenshots were
inspected. Native setup reviews units and accounts; native order invoicing,
issue, credit, source review/post, authenticated owned gateway collection/refund
and native void preserve five separate source journals. AR, revenue and clearing
return to zero; the original histories remain. A pending source refuses close;
posted source coverage, Owner dependencies and reports survive native restart.

HTTPS/client isolation passed through an owned local proxy without changing
system trust. Encrypted restore preserved all eleven financial table digests and
served the restored reports. The actual previous GL native distribution seeded a
posted USD 100.00 book. Current migration applied only its two new suffix files,
reapplied zero, retained prior financial/business rows and original command
receipts, and created no implicit source policies/postings or financial grants.
Its encrypted backup also restored into a separate database and served through
the previous binary with the original balance.

Attempt 01 failed because the new owned gateway helper labelled a full refund as
partial. The parser refused that inconsistent observation. Correcting the fixture
label required no production source change; attempt 02 is the complete pass.
Build attempt 01 was stopped after the gate found an outdated catalogue-count
assertion. The corrected gate and build are both attempt 02; failed/stopped logs
remain recorded.

Two counterfindings remain open. The isolated [packaged domain consumer failed](PRD_COLD_PACKAGE_FINDING_2026-11-08.md)
because embedded catalogues were missing. The unchanged local load workload had
zero request/status errors but Commerce p95 **237.78 ms** exceeded its 50 ms local
target. Contacts read was 9.07 ms against 50 ms; contact-create was 23.58 ms against
250 ms. These are bounded local observations, not accepted production tails or
capacity. A green browser runner does not hide `latencyTargetsMet:false`.

All input/artifact/helper hashes, failures, recovery and load details are bound
in `target/prd-completion-20261107/invoice-posting-verification-01.json`.
Legacy issued invoices, other source units/currencies/FX, AP, stock/COGS sources,
terms/aging, formal credit documents, bank reconciliation, complete subledger
close, localization, external accounting and actual-user acceptance remain open.
No public deployment or real customer/payment/email effect occurred. Work continues.
