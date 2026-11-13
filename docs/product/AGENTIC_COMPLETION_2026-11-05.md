# Ava agentic core completion — 5 November 2026

Archava now connects the customer journey **goal → authorized context → reviewed
plan → approval → business execution → verified receipt/report**. Home and Desk
use the new goal API. The earlier [agentic audit](AGENTIC_AUDIT_2026-11-05.md)
and [simulation report](SIMULATION_REPORT_2026-11-05.md) remain historical records
of the earlier serving build.

Functional acceptance passed locally. The Commerce latency target remains open,
and live-model quality and public deployment require separate acceptance.

## Implemented and exercised

- Durable personal conversations, tenant-scoped preferences, immutable child
  plans, original-request replay and atomic plan/turn persistence.
- Authorized context across all 18 installed domains, including Indonesian and
  English Scheduling requests. Missing input produces clarification.
- Configurable structured Gemini planning, finite registered tools, strict fields,
  bounded calls/context, observed references and verified prior-step ID/revision
  bindings. The local fallback provides bounded context, replenishment and
  native action details.
- Existing Cockpit approvals and domain handlers execute real mutations with
  stable keys and current grants. Actual receipts establish completion.
- Worker continuation, process restart, session revocation, changed-fact review,
  pause/resume/cancel and receipt-backed progress reports.
- Agent-first Home/Desk controls, restored conversations, uncertain-response
  recovery, native action details, mobile rendering and safe late responses.
- Explicit voice input handoff into the same reviewed business goal path.

The [runtime contract](../modules/AGENT_RUNTIME.md) documents environment settings,
limits, current-session delegation and the narrow RLS routing-metadata exception.
The [implementation checklist](AGENTIC_IMPLEMENTATION_WORK.md) records this scope.

## Verification

| Check | Observed result |
| --- | --- |
| Full workspace gate | PASS; **1,142 Rust tests**, 98 suites; format, Clippy, structure, docs and WASM checks |
| Final follow-up gate | PASS; 568 library tests, Clippy and WASM checks after the last query-handler convention adjustment |
| Node helper suites | PASS; 24 tests total |
| Agent PostgreSQL suite | PASS; 13 tests, including 36 bilingual context probes and actual effects/replay across all 18 domains |
| Chromium acceptance | PASS; **75 checks**, including 30 current agent journey checks and delayed-response navigation |
| Operational product probes | **75 met, 0 gaps** in the tested requirements |
| Dependency audit | Zero known vulnerabilities; existing transitive `paste` maintenance warning retained |
| Release build/package | PASS; native server, preview, WASM and **59 verified distribution artifacts** |
| Owned media/TLS/recovery | PASS; voice handoff, real WebRTC, TLS peer isolation, writer lifecycle and encrypted restore through login |
| JavaScript/WASM errors | Zero in final acceptance, including a deliberately delayed response after client navigation |
| Fixture cleanup | PASS; all gate-owned browser/server/database/media/proxy processes closed |

The 18 domain execution probes use explicit registered action inputs. The 36
routing probes verify authorized context and clarification. These counts do not
establish live-model reasoning quality on the original 50 natural-language goals.

Material failures found and fixed during this work:

- CRM committed a mutation but its established snake_case request ID was not
  recognized by the mission receipt verifier. Verification now accepts that
  envelope only when its key matches the original stable step key; replay creates
  no duplicate record.
- Replanning could lose the original Commerce context after a supplier-focused
  follow-up. Revised plans retain the required original modules and recheck
  current costs/stock before new work.
- A turn-store failure could leave an orphaned plan. Plan and turn now commit
  atomically; recovery distinguishes in-flight, committed and uncommitted requests.
- Paused/cancelled goals retained an outdated mission snapshot. They now record
  the actual returned mission state.
- A delayed agent response could read a disposed UI signal after navigation.
  The refresh path now checks signal lifetime; the browser regression verifies
  zero runtime errors after a two-second delayed response.
- Quoted or negated conversational controls could be mistaken for authorization.
  Chat controls now open review; only explicit confirmation with the matching
  plan hash can change or resume work.

## Performance observation

The local scale run used 10,000 synthetic contacts, 2,000 products, concurrency
8, 500 reads per phase and 200 unique mutations. Both runs had zero request
errors/unexpected statuses and verified all 200 mutation receipts.

| Phase | Combined p95 | Isolated p95 | Target |
| --- | ---: | ---: | ---: |
| Contacts | 6.55 ms | 5.39 ms | 50 ms |
| Commerce workspace | **74.28 ms** | **54.89 ms** | **50 ms** |
| Contact creation | 7.83 ms | 8.05 ms | 250 ms |

**The Commerce latency target was missed in both runs.** Functional runner status
and latency targets are recorded separately; `latencyTargetsMet` remains false.
The isolated rerun does not replace or hide the combined result. Production
sizing, sustained workload and deployment performance remain unverified.

## Candidate and reproducibility

Native candidate: `dist/native-candidate-20261105-agentic-03`.

Source SHA-256: `2e3747ac377a3a305bb5d11207a67d0ecd182948980d19d718caa81cbcc56307`.

Source/build/package hashes and final browser evidence were recomputed and
compared. Maintained documentation changes are outside the serving-source hash.
Earlier candidates and failed attempts remain available as local evidence.

The commands used pinned Rust 1.96.0, Node 22.22.3 and wasm-bindgen-cli 0.2.129:

```bash
RUSTC_WRAPPER= node tools/rust/verify.mjs --offline --wasm
RUSTC_WRAPPER= node tools/rust/verify.mjs --lib --offline --wasm
node --test tools/test/product-audit.test.mjs
cargo audit --json
RUSTC_WRAPPER= node tools/rust/build-slice.mjs
RUSTC_WRAPPER= node tools/rust/package-native.mjs --out dist/native-candidate-20261105-agentic-03
```

Final browser run enabled `ARCHAVA_E2E_PRODUCT`, `ARCHAVA_E2E_AGENT_RUNTIME`,
`ARCHAVA_E2E_MEDIA`, `ARCHAVA_E2E_TLS` and `ARCHAVA_E2E_LOAD`; native Ops used the
candidate above. It used installed Brave and LiveKit with a task-owned browser
profile/database and the explicitly selected no-sandbox browser fallback.

Local evidence:

- `target/agentic-completion-20261105/full-gate-04.log`
- `target/agentic-completion-20261105/final-gate-05.log`
- `target/agentic-completion-20261105/runtime-tests-08.log`
- `target/agentic-completion-20261105/dependency-audit.json`
- `target/agentic-completion-20261105/release-build-07.log`
- `target/agentic-completion-20261105/package-03.log`
- `target/agentic-completion-20261105/final-verification.json`
- `target/e2e/agentic-completion-20261105-04/result.json`
- `target/e2e/agentic-completion-20261105-04/agent-runtime.json`
- `target/e2e/agentic-completion-load-isolated-20261105-01/result.json`
- Desktop/mobile screenshots under the final browser evidence directory.

## External acceptance

Owned Gemini HTTP/media fixtures exercise actual protocols and host enforcement.
Live reasoning/transcription quality, provider account/model availability, real
integrations and public deployment were not accepted by these tests. General
natural-language planning requires a configured model enabled for that account.

The public website and deployment remain deferred as requested. No DNS, public
Netlify site, real customer records or external business communications changed.
