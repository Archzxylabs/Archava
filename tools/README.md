# Repository tools

Current helpers use Node built-ins and the root Cargo workspace; no root npm/pnpm install is required.

| Command | Purpose |
| --- | --- |
| `node tools/check-structure.mjs` | Layer boundaries, embedded paths, and active tool imports |
| `node tools/check-modules.mjs` | Every module in `config/modules.v1.json` is completely registered; `--explain ID` lists each registration point and what is missing |
| `node tools/check-refusal-reasons.mjs` | Every key of `config/refusal-reasons.v1.json` is a database guard message; prints uncatalogued `23514` guard messages per ledger (`--missing` lists them); [refusal reasons](../docs/architecture/REFUSAL_REASONS.md) |
| `node tools/check-docs.mjs` | Local file/heading links in maintained docs; frozen records are excluded |
| `node tools/e2e/verify.mjs --output DIRECTORY` | Real Chromium CRM flow, lost-response recovery, role/tenant/CSRF negatives, and production-process restart on an owned disposable database |
| `node tools/rust/verify.mjs` | Structure, docs links, helper tests, formatting, Clippy, and all Rust tests |
| `node tools/rust/verify.mjs --lib --offline` | Explicitly smaller library-only verification |
| `node tools/rust/cold-consumers.mjs --output NEW_DIRECTORY` | Build and execute two isolated offline consumers of the same packaged core/domain contracts; [scope](consumers/README.md) |
| `node tools/rust/persistent-consumers.mjs --native PACKAGE --output NEW_DIRECTORY` | Build two cold public clients and verify configured persistent CRM/extensions on separate disposable native databases; [scope](consumers/README.md) |
| `node tools/rust/domain-assets.mjs --check` | Verify the five public package-local Domain catalogue snapshots; `--write` explicitly refreshes them from canonical config |
| `node tools/rust/build-slice.mjs` | Release server, preview fixture, WASM/bindgen and source/artifact provenance |
| `node tools/rust/package-native.mjs --out NEW_DIRECTORY` | Native server/Ops/site/migrations package; refuses stale inputs and verifies the resulting profile |
| `node tools/rust/dev-slice.mjs run` | Build and launch a disposable PostgreSQL preview |
| `node tools/rust/run-persistent.mjs` | Build and launch against an existing migrated database |
| `node tools/rust/load-benchmark.mjs` | Concurrency & p50/p95/p99 latency benchmark under concurrent loads |
| `node tools/release/sbom.mjs` | Inventory the Cargo dependency graph |

The standalone Rust benchmark is in `tools/rust/benchmark/rust`. Removed TypeScript-era baseline comparisons, package builders, security matrices, and proof producers are recoverable from git history; their evidence is historical and is not replaced by Cargo test success.

The browser gate requires the current release build (`node tools/rust/build-slice.mjs`), installed Chromium and PostgreSQL tools. It creates and closes its own server, database and browser profile. `E2E_BROWSER` selects an installed executable; `E2E_BROWSER_NO_SANDBOX=1` is an explicit fallback for environments that cannot launch Chromium sandboxing. It never attaches to a user browser or shared application database. This gate covers CRM, native Tier 1 operator journeys and mission reads, including deliberate lost responses and a production-binary restart. `ARCHAVA_E2E_PRODUCT=1` additionally exercises all 18 installed domains, native forms, Owner discovery, approved business missions, real Feed actions and atomic imports. Inspect `product.allObservedRequirementsMet` as well as the runner status. `ARCHAVA_E2E_MEDIA=1` requires an installed `E2E_LIVEKIT_BIN` and proves actual microphone/camera RTP plus model PCM/JPEG exchange against owned transports. It does not certify live providers or public deployment.

Optional extended local acceptance uses the same disposable cluster:

```bash
ARCHAVA_E2E_TLS=1 ARCHAVA_E2E_LOAD=1 \
ARCHAVA_E2E_OPS_ROOT="$PWD/dist/native-candidate-001" \
  node tools/e2e/verify.mjs --output target/e2e/extended
```

`ARCHAVA_E2E_TLS=1` requires installed Caddy and curl. It verifies HTTPS with
an explicit task CA, secure cookies and proxy login isolation; it never installs
that CA in system/user trust. `ARCHAVA_E2E_LOAD=1` seeds 10,000 contacts and
2,000 products, measures authenticated reads and 200 unique durable mutations,
and reports latency target results separately from correctness. Check
`load.latencyTargetsMet` in `result.json` before accepting performance.
`ARCHAVA_E2E_OPS_ROOT` selects a freshly packaged native distribution and
checks encrypted backup, isolated restore, business row digests and login with
external effects held. These options prove local behavior; public DNS/ACME,
production sizing, offsite recovery and operational RTO/RPO remain separate.

Product completeness is a separate requirement audit:

```bash
ARCHAVA_E2E_PRODUCT=1 node tools/e2e/verify.mjs --output target/e2e/product-audit
node tools/product/audit.mjs \
  --browser target/e2e/product-audit/result.json \
  --rust-log target/remediation-20261104/verify-01.log \
  --baseline target/remediation-20261104/delivery.json \
  --output target/product-audit-inventory.json
```

The join command requires a recorded baseline with the current serving source
SHA-256 and an executed full Rust log. The paths above select the dated local
baseline; they are not files provisioned by a clean checkout. A new candidate
needs its own current build/evidence record. The tool refuses source/artifact
drift or missing/failed/ignored API-suite evidence. Exit **2** means product
requirements remain incomplete; exit 1 means the audit itself could not finish.
The browser runner completing successfully does not turn requirement gaps into
passing features. Static operation references are listed separately from
runtime proof and do not establish execution coverage or a readiness percentage.

### Agentic product diagnostic

`ARCHAVA_E2E_AGENTIC=1 node tools/e2e/verify.mjs --output <new-directory>` reuses
fresh release artifacts and an owned database/browser to probe the goal-driven
agent journey. Read `agentic.agenticReady`, `agentic.counts` and
`agentic-observations.json`; runner `passed` only means the diagnostic completed.
The earlier [audit](../docs/product/AGENTIC_AUDIT_2026-11-05.md) records readiness
gaps separately from passing executor/approval/recovery foundations.

### Combined simulations

Add `ARCHAVA_E2E_SIMULATIONS=1` for the [customer-goal catalogue](product/simulation-scenarios.mjs)
and [runtime lifecycle/boundary probes](e2e/simulations.mjs). It can run together
with `ARCHAVA_E2E_PRODUCT=1`, `ARCHAVA_E2E_AGENTIC=1`, `ARCHAVA_E2E_MEDIA=1`,
`ARCHAVA_E2E_TLS=1`, `ARCHAVA_E2E_LOAD=1` and a current native package selected by
`ARCHAVA_E2E_OPS_ROOT`. Media also needs `E2E_LIVEKIT_BIN`. Use a new output
folder and an existing fresh release build.

Inspect `simulations.counts`, `agentic.agenticReady`,
`product.allObservedRequirementsMet` and `load.latencyTargetsMet` separately.
Runner completion is not a product-readiness verdict. Runtime goals are distinct
from the library-only MCP/campaign/RPA/eval simulations covered by the full Rust
gate. See the [combined simulation report](../docs/product/SIMULATION_REPORT_2026-11-05.md).

### Live Gemini acceptance

After a fresh release build, explicitly select a GenerateContent text model
available to the account and a private env file containing `GEMINI_API_KEY`
or `GEMINI_AGENT_API_KEY`:

```bash
node tools/e2e/run-agent-live.mjs \
  --credentials /absolute/path/to/private.env \
  --model gemini-3-flash-preview --thinking-level minimal \
  --output target/e2e/live-new-run
```

The launcher reads only the key into the child environment and pins Google's
canonical HTTPS endpoint. It never loads the entire env file into the shell,
prints the key, writes credentials to the repository, or changes persistent
runtime configuration. It makes real billable/provider-quota inference calls;
business records remain synthetic in its disposable PostgreSQL fixture.
`--thinking-level` is optional and applies only to supported Gemini 3 text models;
omit it for Gemini 2.5 or the provider default. It maps to
`GEMINI_AGENT_THINKING_LEVEL` only in the child environment. Actual available
models and provider quota remain account-specific.
Installed Chromium and PostgreSQL tools are required, with the same browser
selection and task-owned cleanup as the base gate. Use a new evidence directory.

The browser check submits an Indonesian procurement goal from Home, approves
the model's actual plan in Desk, verifies supplier/offer rows independently in
SQL, checks replay, requests a revised cost with fresh approval, updates an
English CRM goal, checks missing-input clarification and attempted approval
bypass, and asks a grounded read-only question. Memory, receipt reports and
reload are also checked. Inspect `agentLive.verified` and `agent-live.json`.
At most six planning turns/twelve provider calls are allowed, paced at least
twenty seconds apart for provider per-minute quotas; report, memory and
original-request replay must add no planning attempts. Live inference cannot be
combined with deterministic planner/runtime diagnostic flags. Other acceptance
options remain separate. Voice and exhaustive business-goal quality need their
own acceptance.
The [live acceptance record](../docs/product/AGENT_LIVE_ACCEPTANCE_2026-11-05.md)
contains the executed model/profile, final evidence and preserved failures.
