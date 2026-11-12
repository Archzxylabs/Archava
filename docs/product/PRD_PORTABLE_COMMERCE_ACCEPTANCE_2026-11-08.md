# Portable contracts and Commerce query — 8 November 2026

Bounded local verification, **not whole-PRD or production acceptance**. Source:
`fcd94e70cfd4388b9ca7fcd41798213d4bfae1c424b822c6eb9de74bd4556f28` (497 inputs).
The Owner findings below remain required work and are not hidden by runner PASS.

The full gate exits 0: **1,270 Rust tests in 99 suites, 0 failed/ignored; 35 Node
helper tests; structure/docs/format/Clippy and both WASM boundaries**. Fresh native,
preview and WASM/bindgen builds completed. The native distribution is
`dist/native-candidate-20261108-prd-portable-commerce-01`, **77 verified artifacts**.

## Changes and actual proof

- Public Domain catalogues are embedded inside the crate, with canonical-byte
  guards. [Two cold contract consumers](PRD_COLD_CONSUMERS_2026-11-08.md) independently
  build/test/run the identical archives offline. This is a reusable-contract
  subset, not complete configured/persistent agency/wholesale acceptance.
- Commerce filters the product page before stock allocation aggregation. Native
  company columns avoid whole-row JSON serialization during filtering; declared
  old-schema fixtures retain their compatibility path. Current tenant/company,
  exact-ID/search-before-limit, stock permissions and projections are preserved.
- The owned API probe returns 100 projections exactly equal to the old query and
  checks a stock product outside that page: quantity 10,000, reserved 100,
  available 9,900. Five SQL measurements are 4.62–4.87ms versus the old query's
  71.81–94.45ms in that same diagnostic fixture.
- Actual authenticated API load p95 is **31.11ms Commerce**, 4.50ms contacts and
  6.50ms create, with no request/status errors. Browser run 03 separately measures
  Commerce **33.21ms**; final run 04 measures **17.31ms**. Workload thresholds stay
  50ms reads/250ms writes, with 10,000 contacts, 2,000 products, 500 reads per
  endpoint/concurrency 8 and 200 unique mutations/receipts. Earlier 161.12ms
  diagnostic and 237.78ms invoice-candidate results remain historical failures.
  These local repeated observations do not establish production sizing or B07.
- Native invoice policy setup/update loads the current policy revision from the
  selected book. Browser proof covers first setup and a second review without
  manual internal-revision input, then five exact source journals and close.

Final Chromium run 04 exits 0: **159 root checks, 77 product probes, 30 owned Ava
checks, 0 JS errors and cleanup exit 0**. TLS/client isolation, actual WebRTC with
owned transports, native restart, encrypted restore and financial digests pass.
Actual prior f888 package creates a posted USD100 book and posted IDR invoice
source; current code applies zero SQL suffix, preserves eleven financial tables,
replays the original command and restores into a separate old-binary database.
The prior fixture explicitly provisions missing party permissions before seeding;
this is not an upgrade-created grant or default-installer acceptance.

## Counterevidence and remaining work

The new Owner journey proves real native disable, dropped post-commit response,
original receipt recovery, manual/chat plan identity and compatible restore, with
15 business/financial table digests unchanged. It also observes
`ownerConfiguration.configuredLabelRendered:false` and
`allObservedRequirementsMet:false`: labels persist but the contact pipeline still
renders canonical names. DOM evidence establishes the gap; the screenshot is a
record-page viewport and does not by itself show the pipeline below the fold.

Run 03 fails while the immutable prior f888 installer owner creates a customer
party: HTTP403. Its bootstrap catalogue lacks `parties.read/write`; current fcd
catalogue has the same omission. Current-fresh impact is supported by that source
inspection, not a claimed additional native run. The next production batch must
fix and verify initial owner provisioning without granting existing tenants new
permissions through an upgrade or runtime config change.

Browser runs 01–02 are retained fixture failures: misplaced Owner test before a
restorable baseline, then an incorrect expectation of nonempty extra effects for
an optional disable. The runner placement/assertion and Request-cloning fault
injection were corrected. Run 03 retains the real old-installer permission failure;
run 04 declares explicit prior administration and tests preservation/rollback.
No failed result is overwritten.

AP/stock/COGS sources, bank reconciliation, due terms/aging, complete subledger
close, Ava held-out/privacy/owner work, offered-profile/consumer compatibility,
localization and actual-user acceptance remain whole-PRD requirements. Public
deployment stays deferred. No readiness percentage or competitor superiority is
inferred from these passing finite checks.

Bound evidence: `target/prd-completion-20261107/portable-commerce-verification-01.json`.
The full gate/build/package/browser/API/cold-consumer files and browser helper
hashes are recorded there. Later source changes require fresh native/gate/browser
proof and do not inherit this candidate's acceptance automatically.
