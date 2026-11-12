# Purchase sources and contact navigation recheck — 8 November 2026

Source `7b9ea46f8dbe087c5fc6591971f24ba7de05b0d74f4077f9f64ac48c9e8a5516`
passed the full gate: 1,288 Rust tests in 99 result suites, 35 helper tests,
formatting, Clippy and both WASM boundaries, with zero failed or ignored tests.
The fresh native distribution
`dist/native-candidate-20261108-prd-purchase-posting-02` verifies 78 artifacts.
This follows the [failed earlier candidate](PRD_PURCHASE_SOURCE_FINDING_2026-11-08.md);
its failures remain evidence.

The contact correction scopes asynchronous list results to their live reactive
owner, latest request and captured tenant/principal/session. The browser holds
the actual create-triggered list response until the contact-record route has
opened, then releases it after the list owner is disposed. That regression
passes without suppressing errors or creating another contact. A first recheck
then stopped at an invoice helper's obsolete policy caption; the caption assertion
was corrected while retaining the database policy revision assertions.

Run `target/e2e/prd-purchase-posting-20261108-04/result.json` passed **167 root
checks**, the product probes and **30 Ava checks**, with no page errors and
fixture cleanup exit 0. Seven purchase checks exercise native four-role policy
setup, accepted/rejected receipt quantities, exact landed fees, original policy
after a later update, goods claim, void, separately reviewed posting, pending
source close refusal and source-aware reports. Independent SQL finds two units
with original cost Rp14,101: inventory 1,410,100 book minor units, unbilled goods
−1,400,000, AP zero after void and fee accrual −10,100. No supplier payment is
inferred. The native book explicitly uses a synthetic single-operator policy;
independent author/approver behavior is covered by the Accounting API suite.

Owner changes preserve 19 business/financial table digests. Encrypted native
restore preserves all 15 financial tables. An actual previous distribution
upgrades by exactly Accounting004, reapplies zero migrations, preserves eleven
prior financial tables and the old command receipt, and supports isolated
encrypted-backup rollback using the old binary. Fresh owner bootstrap requires
no additional administrator party grants.

Five local read repetitions measured Commerce p95 20.30/18.27/19.52/18.35/18.81 ms
and Contacts 3.91/5.00/3.89/4.18/4.08 ms; 200 writes measured 6.44 ms p95.
All 5,200 measured requests had zero errors and met their existing local targets.
This synthetic result does not erase earlier latency misses or establish
production sizing or tail reliability.

**Visual review remains open for this source.** The purchase-review screenshot
shows a desktop action panel constrained to roughly 320 px with long, nested
account details. Functional success does not accept its readability. A wider
review layout and corrected combined invoice/purchase report caption are being
implemented and require a fresh build/browser result. This record does not
accept those later edits.

Evidence and exact helper snapshots are bound by
`target/prd-completion-20261107/purchase-contact-verification-02.json`.
AP terms/aging and payments, bank reconciliation, original-cost sales/returns,
full financial close, complete industry consumers, privacy and actual-user/ops
acceptance remain whole-PRD work. No public deployment occurred.
