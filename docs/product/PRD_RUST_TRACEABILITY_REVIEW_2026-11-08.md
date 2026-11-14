# Full Rust roadmap/gate mapping — 8 November 2026

Reviewed source: [Full Rust PRD](PRD_FULL_RUST.md), sections 22–23, against the
[active delivery register](PRD_DELIVERY_REGISTER.v1.json). The original source,
milestone exits and gate criteria remain authoritative. Mapping is not acceptance
or supersession. All 22 named items below remain partial; other source chapters,
Owner criteria and inherited master requirements still need semantic review.

The [native GL acceptance](PRD_NATIVE_GL_ACCEPTANCE_2026-11-08.md) supplies bounded
proof for source `fe4ec91c…`: complete local gates, real browser, immutable native
package, actual previous distribution upgrade and isolated encrypted rollback.
It does not accept the newer invoice-source code, independent cold library
consumers, the entire inherited inventory, production-host operations or real
users. Counts of crates/actions/tests are not completion percentages.

| Original item | Active requirement relationship | Existing evidence and work still required |
| --- | --- | --- |
| R00 baseline | DAT-01, SEC-01, OPS-01, OWN-08 | Source/artifact hashes and scoped dated findings exist. Finish behavior/entrypoint inventory disposition, corrected-baseline review and consumer/profile mapping. |
| R01 foundations/UI feasibility | SEC-01, UX-02, OPS-01 | Real Rust host, non-owner PostgreSQL and SSR/WASM financial/CRM flows are locally proven. Register complete build/UI budgets and source-profile acceptance; feasibility is not owner cutover. |
| R02 pure engines | CFG-03, OPS-02 | Portable engine crates have tests and explicit boundaries. Reconcile original parity/error cases, compiled baselines and two cold shipped consumers per accepted closure. |
| R03 complete consumer | AGT-01, AGT-03, SEC-01, UX-02, CFG-03 | Reviewed commands, recovery, current grant/scope tests and native UI exist. Complete K04/K05 independent consumer proof with identical shipped core/CRM. |
| R04 agreements/projects | DAT-01, DAT-02, EXT-01, CFG-03 | Native module contracts and scoped operations exist. Verify full declared agreement/capacity lifecycle, immutable history/races and two supported consumers. |
| R05 occupancy/assets | DAT-01, DAT-02, EXT-01 | Shared scheduling/occupancy boundaries and operator flows exist. Complete profile-specific overlap/cancel/recovery and packaged consumer/API/UI evidence. |
| R06 stock cohort | DAT-01, DAT-02, FIN-02, FIN-03 | Shared physical stock, receipts and frozen cost contracts exist. Connect source accounting, returns and selected fulfillment/asset effects; finish complete cohort oracles and consumer proof. |
| R07 billing/operations | FIN-01, FIN-03, FIN-04, FIN-05, EXT-01 | Native billing and bounded journals exist; invoice-source work is under validation. Finish supported financial/subscription/entitlement/dispatch lifecycle and relationships. |
| R08 verticals | EXT-01, SEC-01, CFG-03, UX-02 | Named operator/API boundaries exist. Accept supported specs, references/delegated scope, recovery and independent consumers for profiles actually offered. |
| R09 interface/SDK/realtime | AGT-01, AGT-04, AGT-06, UX-03, SEC-02, CFG-03 | Owned chat/media/agent evidence is bounded. Complete knowledge/context/decision parity, actual offered media, privacy/consent and two host integrations. |
| R10 operations/cutover | OPS-01, OPS-02, INT-02, OWN-08 | Native CLI, hashes, fencing and actual old-binary restore/upgrade rehearsal exist. Finish full-profile data/settings/key preservation, operator evidence and accepted release identity. Public deployment remains deferred by owner. |
| R11 retirement | CFG-03, OPS-01, OPS-02, OWN-08, EXT-01 | Rust runtime and structure gates exist. Resolve every original behavior disposition and all gates, document compatibility exceptions and prove no mandatory legacy product engine. Forty-kit readiness is separate. |
| G01 baseline | DAT-01, OPS-01, OWN-08 | Dirty-source input hashes are recorded per candidate. Complete full frozen inventory and corrected-behavior review; a previous candidate does not accept changed code. |
| G02 contracts | DAT-02, INT-01, CFG-03 | Typed commands, exact values, receipt/replay tests exist. Finish profile-wide wire/error/rounding/fingerprint compatibility and explicitly accepted corrections. |
| G03 authority | SEC-01, SEC-02, AGT-02 | Real negative database/HTTP tests cover current actor, tenant, grants and scope. Finish sensitive-route/tool/consumer coverage, external channel identity and selected enterprise controls. |
| G04 integrity | DAT-01, DAT-02, FIN-01, FIN-02, FIN-03, FIN-05 | Atomic commands, reviewed references, source guards and stock contracts exist. Finish AP/bank/COGS/subledger lifecycle and complete selected-profile concurrency/control totals. |
| G05 reuse | CFG-03, OWN-01 | Two tenants, two binaries or an in-workspace adapter test do not satisfy two independent cold consumers. Build each from identical shipped artifacts with a minimal dependency closure and no checkout/absolute-source imports. |
| G06 UI/SDK | UX-01, UX-02, UX-03, SEC-02 | Native Chromium, mobile and owned media evidence exists. Complete offered accessibility/embed/media and actual persona usability; screenshots or forms existing alone are insufficient. |
| G07 data/recovery | OPS-01, INT-02, AGT-03 | Actual prior native upgrade, encrypted isolated restore, row digests and pending receipt recovery are bounded local proof. Verify all supported prior profiles, keys/settings/microseconds/history and operational recovery expectations. |
| G08 quality/performance | OPS-02, CFG-03 | Pinned Rust/Node/WASM and full lint/test gates exist. Complete advisory/feature matrix and registered workload/UI/build budgets with adequate repeated measurements; no microbenchmark substitutes for production tails. |
| G09 identity | OPS-01, OWN-08 | Native GL artifacts/proofs share one recorded source; failed attempts remain recorded. Finish current invoice candidate gate/build/browser/package, then bind the entire declared profile and explain every required failure. |
| G10 operations/retirement | OPS-01, OWN-08, INT-01, UX-03 | Native operating CLI/fencing/runbook exist. Close accepted inventory and declared worker/webhook/SDK/compatibility paths without a silent legacy fallback. |

Section 22 orders work; it does not authorize pausing after each slice. Its
transaction-cohort boundary, local-first cutover limits and explicit exclusions
of speculative fleets, dynamic native plugins and universal/forty-kit rewrites
are preserved. Private registry/new deployment targets stay conditional on a
real consumer requirement. Future industry requirements are not silently erased.

Section 23's separate migration/product/industry/security reporting and ban on
unsupported universal claims are preserved. The machine register still rejects
whole-product completion while requirements, profile and inherited proof remain
open. No acceptance, retirement, scope exclusion or public release is granted by
this review.
