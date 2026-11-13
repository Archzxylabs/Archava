# Ava context recheck and five local load repetitions — 8 November 2026

Same production inputs and immutable native package as the [first context finding](PRD_AGENT_CONTEXT_FINDING_2026-11-08.md):
`f6b74cfe7d0b7647ac7797dcebd77bf1a444c85d6c8324da907406ca9f0805dd`,
497 inputs, 77 verified artifacts. No production optimization or threshold change
was made between these runs. The first **55.98 ms** Commerce p95 miss remains
counterevidence; these subsequent measurements do not rewrite it or establish its
cause.

Before remeasurement, the load helper added an explicit bounded repetition count.
The second complete owned browser run used five read repetitions, each with 20
warmups then 500 measured requests at concurrency 8, on its same fixed synthetic
10,000-contact/2,000-product dataset. Contact mutations ran once afterwards:
200 distinct requests with 200 durable receipts. Every repetition and error count
is retained, with the unchanged 50 ms read and 250 ms mutation targets.

| Workload | p95 milliseconds, all five read repetitions | Median / range |
| --- | --- | --- |
| Contacts | 3.09, 4.13, 3.43, 2.98, 2.82 | 3.09 / 2.82–4.13 |
| Commerce | 15.23, 14.45, 14.32, 14.54, 14.84 | 14.54 / 14.32–15.23 |
| Contact create, one 200-request phase | 5.71 | Separate mutation workload |

All 5,200 measured requests completed without errors or unexpected statuses. The
second run reports `latencyTargetsMet: true`. These results show the registered
local workload meeting its target on this run. They do not establish production
tail reliability, explain the earlier variation, accept every UI/build/RSS budget
or supply a compiled baseline/TCO comparison. OPS-02 remains partial.

The complete browser recheck exited 0 with 160 root checks, 77 bounded product
probes, 30 owned Ava checks, current owner/native recovery/upgrade/rollback,
zero page errors and cleanup 0. The original full gate of 1,276 Rust/35 helper
checks is reused against exactly unchanged production inputs; all artifact hashes
were recomputed. No skill, independent agent, external deployment or live-model
acceptance is inferred. The RustSec unmaintained dependency finding remains open.

Evidence: `target/e2e/prd-agent-byte-budget-20261108-02/result.json` and
`target/prd-completion-20261107/agent-context-verification-02.json`, including exact
helper snapshots. The [first finding](PRD_AGENT_CONTEXT_FINDING_2026-11-08.md), its
failed latency target, earlier candidate failures and all dated proofs remain
immutable. The [delivery register](PRD_DELIVERY_REGISTER.v1.json) retains the full
unfinished PRD and explicit release-profile requirements.
