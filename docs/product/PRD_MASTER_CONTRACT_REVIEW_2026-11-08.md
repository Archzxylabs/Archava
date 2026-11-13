# Retained master contracts 39–51 — 8 November 2026

Reviewed the frozen [historical master](../archive/history/PRD.md) chapters 39–51
against the [active PRD](PRD.md), [current architecture](../architecture/README.md)
and [inherited inventory](PRD_INHERITED_TRACEABILITY.v1.json). Its original bytes
remain unchanged. This is scope/traceability review, not implementation acceptance
or approval to retire behavior. Chapters 1–38 still require semantic review.

The previous 45 named entries covered Rust gates/milestones, EXEC, Owner and
benchmark criteria. They did not enumerate 44 explicit contracts in these master
chapters. Those contracts now have separate `MASTER-` IDs and retain open/partial
status. Mapping a chapter does not satisfy those contracts or the active PRD.

| Chapter | Active relationships | Retained requirement and material remaining work |
| --- | --- | --- |
| 39. Decision Intelligence | AGT-01/04/06, SEC-01/02 | Advisory signals cannot authorize, invent facts or replace structured truth. Keep provider-neutral contracts, off/shadow/assist, confidence/timeout/refusal fallback, redacted contexts and multilingual eval. Current Ava/Gemini proof does not accept every Jev/Decision task, privacy contract or held-out corpus. Future adapters/ranking stay future scope. |
| 40. Product/library ownership | CFG-03, UX-01, OPS-01 | Reuse includes business backend and UI, separate from the external company website. Controlled artifacts, data/export/IP/license terms and measured reuse remain explicit. Two developer clients do not prove owner self-service or all commercial obligations. No publication, contract transfer or percentage guarantee is inferred. |
| 41. Reuse layers | CFG-03, DAT-01, INT-01, SEC-01 | Preserve one resource authority, model-independent deterministic modules, bounded configuration and explicit vertical extensions. Cross-module transactions belong to tested application services. Persistent client evidence proves part of reuse; UI composition, external adapters and full profiles remain. |
| 42. Module contracts | CFG-01/02/03, OWN-01/03/06/08 | Trusted finite registry and typed actions, exact versions, dependencies/conflicts, config/resource/permission/ports/readiness/migration/UI/support metadata. Enablement, entitlement, permissions and readiness are distinct. Preserve data and recover pending effects on disable. Installed catalog entries alone do not prove every combination. |
| 43. Config and resolved plans | CFG-01/02/03, OWN-03/04/06, SEC-02 | Closed supported options, requested/configured/connected/healthy states, safe browser projections and quote/deployment scope identity. Unsupported/unpriced scope stays explicit. Pricebook, catalog, config and migrations have separate identities; labels cannot become executable rules or invent integration readiness. |
| 44. Tenant/identity/action policy | SEC-01/02, UX-03, AGT-02/03, DAT-02 | Session-owned tenant/principal, current grants/resource scope, FORCE RLS/non-owner runtime, one canonical action/attempt, bound expiring consent and unknown-outcome recovery. Preserve separate public/customer/employee/owner projections; current authenticated native proof does not accept every SDK/file/export/provider path. |
| 45. Versions and recovery | OPS-01, CFG-03, INT-02, SEC-02 | Exact supported versions, append-only schema history, prior/current compatibility and isolated recovery with measured limits. Binary rollback is not reverse SQL. Profile support windows, privacy/export/deletion/backup/provider retention and real operational commitments remain. |
| 46. CRM/ERP and kits | DAT-01/02, FIN-01/02/03/04/05/06, CFG-03, EXT-01 | Shared CRM retains revisions, grants, transitions, audit and real records; operating domains retain their own invariants. Narrow kit scopes and candidates do not mean complete industries or statutory accounting. Later authorizations and active selected-profile requirements govern expansion; no company website is needed for local proof. |
| 47. Artifacts and migration | CFG-03, UX-02/03, OPS-01 | Independent packed consumption, browser/server separation, controlled distribution, provenance/licenses and behavior comparison before retiring old surfaces. Rust supersedes the old ESM implementation proposal; supported behavior and compatibility requirements remain. New persistent CLI clients do not accept every browser/SDK consumer. |
| 48. Operations and validation | OPS-01/02, SEC-01/02, AGT-03/06 | Correlate releases/config/actors/attempts/resources/outcomes with metadata-first telemetry and durable audit. Health, alerts, support policy, actual restore and provider-specific outcomes remain separate. Never substitute mock success for live evidence or infer security coverage from test counts. |
| 49. Priorities/milestones | CFG-03, AGT-01, EXT-01, OPS-01 | Preserve evidence-based M0–M4 scope and the separate commercial-readiness denominator. Historical accepted local behavior remains context, not acceptance of new source. General workflow/plugin engines, arbitrary industries or infrastructure are not required by a library milestone. |
| 50. Execution contracts | CFG-01/02/03, OWN-01/03/06/07/08, SEC-01/02, OPS-01/02, UX-02/03 | Retain existing EXEC-01–06 entries: exact profiles, two compositions/lifecycle, shared presence/identity, security matrix, prescribed capacity workloads and upgrade/recovery. Historical TypeScript defaults do not reverse the selected Rust direction. Current c8 request-count load tests do not by themselves satisfy prescribed c1/5/20 timed workloads. Website/public/live-provider scope remains separately authorized. |
| 51. Owner self-service | OWN-01/02/03/04/05/06/07/08, CFG-01/02/03 | Business-language discovery → supported preview/cost/impact → confirmation → apply/status/recovery/history/compatible restore through UI and chat. Client owners and company operators remain distinct. CLI fixtures plus native label/toggle proof establish subsets, not every OWN-T01–10 criterion or a universal workflow builder. |

[Purchase/contact recheck](PRD_PURCHASE_CONTACT_RECHECK_2026-11-08.md),
[persistent consumers](PRD_PERSISTENT_CONSUMERS_2026-11-08.md),
[Rust review](PRD_RUST_CONTRACT_REVIEW_2026-11-08.md), architecture/module contracts
and the incomplete machine registers provide the current relationships. No
requirement was marked accepted, superseded or excluded by this review.
