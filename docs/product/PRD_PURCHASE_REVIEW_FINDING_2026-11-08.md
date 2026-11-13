# Purchase review layout and remaining presentation gap — 8 November 2026

Source `e638ed2ec933e55b29b66328d1b73f15db6269f7f02a5718379fdfa52f28c028`
passed 1,288 Rust tests in 99 result suites, formatting, Clippy and both WASM
boundaries. The original helper gate passed 35 tests; the subsequent current
helper gate passed 36, including retained-master omission detection. Native
server/preview/WASM builds exited 0 (12m00s/2m51s/2m02s), and
`dist/native-candidate-20261108-prd-purchase-review-01` verifies 78 artifacts.

`target/e2e/prd-purchase-review-20261108-01/result.json` passed 167 root checks,
77 product probes and 30 Ava checks, with no page errors and fixture cleanup 0.
The purchase review now has at least 650 px on the tested desktop viewport and
fits a 390 px mobile page without horizontal overflow. The real report UI names
both invoice and purchase sources and shows all three posted purchase effects.
Exact source amounts/policy, void, separate posting and SQL balances retain the
[prior functional evidence](PRD_PURCHASE_CONTACT_RECHECK_2026-11-08.md).
Owner preserves 19 table digests; encrypted native restore preserves 15 financial
tables. The actual preceding source profile upgrades by one migration, reapplies
zero and retains eleven prior financial tables/receipt identities with isolated
encrypted old-binary rollback.

Five local read repetitions measured Commerce p95
15.09/14.26/15.46/15.43/14.73 ms and Contacts
2.70/3.47/2.97/2.98/3.09 ms. The 200 writes measured 4.68 ms p95. All 5,200
measured requests had zero errors and met their existing local targets. This
does not erase previous misses or establish production sizing, tail reliability
or the separate prescribed c1/5/20 timed workload.

**Presentation is still incomplete.** Visual inspection of the actual desktop
and mobile screenshots finds raw account IDs and revisions before the goods/fee
amounts; nested original mappings make the decision unnecessarily long. The new
width fixes the cramped panel but does not establish customer-friendly review.
The next change should lead with exact business amounts and original-policy
account effects, with technical references available on expansion. The submitted
command, source identity, current account revisions and backend authority must
remain bound. This record does not accept later UI edits.

Exact artifacts/helpers and observed limits are bound in
`target/prd-completion-20261107/purchase-review-verification-01.json`.
Whole PRD, full financial close, privacy, selected profiles and actual-user/ops
acceptance remain incomplete. No public deployment occurred.
