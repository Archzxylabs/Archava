# Purchase source candidate and contact navigation finding — 8 November 2026

Source `513e51aa6f8af1917eedb19d623177e65361d5a227a80081da0f0244e674db9a`
(501 inputs) implements the [future confirmed-PO purchase bridge](../modules/PURCHASE_POSTING.md).
Its full gate exited 0: **1,287 Rust tests**, 99 result suites (73 nonempty),
35 helper tests, zero failed/ignored, formatting, Clippy and both WASM boundaries.
The combined Accounting suite passed 23 tests, including separate independent
posting, immutable original policy, exact receipt/bill/void amounts, current
permissions and actual API setup/confirmation races. Applied Accounting004 SHA-256
`32b3e9d46066bae9ce419bb06876e084b8a470a10bdd10b957d30501d743d041`
and all earlier SQL are immutable.

Fresh release build completed: native server 11m00s, preview 3m00s, browser/WASM
2m37s. The immutable local distribution
`dist/native-candidate-20261108-prd-purchase-posting-01` verifies 78 artifacts.
Two fresh cold public-contract consumers built offline from identical core/domain
archives in independent temporary roots, passed ten tests and both apps, and
removed their scratch directories. This is contract reuse, not full configured
persistent K04/K05 acceptance.

**This candidate failed browser acceptance.** Run
`target/e2e/prd-purchase-posting-20261108-01/result.json` passed login/hydration,
then found a disposed reactive signal panic while opening a newly created contact.
It stopped before the new purchase UI journey. Cleanup exited 0. Native packaging
and API tests do not override this failure.

The create acknowledgement starts an asynchronous contacts-list refresh. Opening
the new contact can dispose the list route before that refresh returns; the old
callback then reads `selected_id.get_untracked()` from its disposed owner.
A second owned run deliberately held that actual list response until the record
page had opened, then released it. It reproduced the same panic at F02, retained
the original one-contact/no-extra-effect database counts, failed and cleaned up 0:
`target/e2e/prd-purchase-posting-20261108-02/result.json`.

The proposed correction gives list requests an owner-scoped generation and binds
them to the captured tenant/principal/session CSRF identity. Superseded or disposed
responses do not access page signals. A validated contact-create acknowledgement
still clears its original scoped pending marker after navigation, while private
UI results cannot paint a different session. A focused reactive-owner test passed
for supersession, session replacement and actual owner cleanup. This newer source
requires its own complete gate, fresh build/package and deterministic browser
recheck; this record does not accept that fix or the purchase UI prematurely.

Evidence: `target/prd-completion-20261107/purchase-source-gate-summary-01.json`,
`purchase-source-build-02.log`, `purchase-source-package-01.log`,
`cold-consumers-04/result.json`, `purchase-source-browser-01.log`,
`purchase-source-browser-02.log` and `contact-lifetime-test-01.log`.
The [current delivery register](PRD_DELIVERY_REGISTER.v1.json) remains incomplete;
AP aging/payment, bank, COGS/full close, privacy, full consumer/profile and user/ops
acceptance are separate outstanding requirements. No public deployment occurred.
