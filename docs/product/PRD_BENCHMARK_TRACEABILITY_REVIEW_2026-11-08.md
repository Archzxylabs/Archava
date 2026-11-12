# ERP benchmark semantic review — 8 November 2026

Reviewed the complete [proposed B01–B07 protocol](ERP_PARITY_ACCEPTANCE_2026-11-06.md)
against the [active requirements](PRD_DELIVERY_REGISTER.v1.json). This is a source
mapping, not a claim that the benchmarks ran, matched a competitor or met a
customer SLA. The ten original chapters and seven named protocols are preserved.

| Protocol | Active relationship | Remaining acceptance boundary |
| --- | --- | --- |
| B01 transaction to close | DAT-01, DAT-02, FIN-01, FIN-02, FIN-03, FIN-04, FIN-05, FIN-06 | Actual PO/inspected receipt/vendor bill, source-backed AP/COGS, bank evidence/reconciliation, linked return/credit/refund and subledger close. Manual journals cannot substitute for operating sources. Native GL/invoice evidence is partial only. |
| B02 warehouse traceability | DAT-01, DAT-02, FIN-02, FIN-03, EXT-01 | Two real warehouses, transit, concurrency, explicit UOM, required lot/serial/quality/expiry, original return costs and financial reconciliation. A company stock pool or serialized asset is not this proof. Native or tested external WMS is permitted. |
| B03 manufacturing | DAT-01, DAT-02, FIN-01, FIN-05, EXT-01 | Chosen manufacturing profile with multilevel BOM/routing versions, two work centers, shortage/procurement/capacity, consumption/WIP/finished goods, scrap/quality/subcontracting and reconciled close. Pure BOM tests do not accept it. |
| B04 group/currency | DAT-01, FIN-01, FIN-05, FIN-06, SEC-01, EXT-01 | Legal entities differ from tenants/branches/warehouses. Two functional currencies, reviewed FX/intercompany, translation/elimination/consolidation and drill-down remain; tenant isolation alone is insufficient. |
| B05 agent goals | AGT-01, AGT-02, AGT-03, AGT-04, AGT-05, AGT-06, UX-01, OPS-02 | Frozen development versus held-out goals, language/ambiguity/partial failure/revocation/injection/provider cases, independent DB/receipt oracles, all attempts and task-level time/cost/effort. Existing six live goals and deterministic lookup tests are bounded subsets. |
| B06 identity/integration/ops | SEC-01, SEC-02, INT-01, INT-02, OPS-01, OWN-08 | Selected IAM controls, mapped provider truth and secrets/webhook/recovery, controlled migration/openings, real old-profile upgrade, encrypted restore, writer/rotation/received-alert and target offsite RTO/RPO/support proof. Local receipts do not imply exactly-once network effects. |
| B07 performance/TCO/pilot | OPS-02, UX-01, UX-02, AGT-05 | Registered comparable mixed workload, full latency/error/saturation/lag/recovery/cost data, cold/warm distinction and predeclared real-user pilot KPIs. Local Commerce 50ms remains unchanged; no competitor superiority or TCO result follows from a microbenchmark. |

Source chapters 1–2 require an explicit comparable profile, official accounting
source, data owners, jurisdictions/currencies, approval, workloads/model identity
and predeclared outcomes. The active accounting recommendation does not create a
real customer's books or approve a statutory policy. Manufacturing, warehouse
and group depth remain conditional on the offered profile, not silently accepted
or erased. Native or correctly reconciled external accounting are allowed.

Chapters 3–9 specify the seven protocols above, including all B01 negative and
AI-unavailable cases. Preserve original independent B01 totals before/after a
partial return; derive them from source transactions, never model text or manual
adjustments that bypass the intended workflow. The examples are synthetic
control fixtures, not tax/accounting advice for a real business.

Chapter 10 requires same-identity artifacts: scope/licensing, data-flow ownership,
dataset/oracles, logs and all failed/recovered attempts, role/entity matrix,
latency/task effort/cost, operational proof and explicit scoped disposition.
No protocol is accepted by this review. All seven remain partial/open; customer
profile and real-user evidence remain explicit requirements. Public deployment
is still deferred by the owner.
