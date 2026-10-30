# 9 October 2026 — durable provider runtime verification

Baseline: `4297bfe`. Node 22.22.3, pnpm 10.15.1, PostgreSQL 18.6 and local
Chromium. This is an incremental M3 operational slice; M3 is not complete.

## Delivered behavior

`@archava/provider-runtime` 0.1.0 adds generic provider admission/outcome recording
and a PostgreSQL adapter. It imports no CRM, identity or model SDK. Two independent
cold-installed consumers share one stored allowance and active lease using
identical artifacts. The artifact set now contains 14 internal packages; existing
CRM/CRM-PG/CRM-assistant artifacts retain their exact successful Gemini-pilot hashes.

The CRM host shares 30 admitted attempts per UTC day across businesses, 15-second
pacing and a 60-second lease around its 45-second bounded Gemini request. Failed
and interrupted requests remain charged. A duplicate UUID does not invoke again;
late results cannot replace `unknown` outcomes or reach CRM tools without durable
success recording. No model result/prompt/record/identity/key is stored.

The private `pnpm crm:ai-status` command reports allowance, hold, active UUID and
up to 20 recent safe outcomes/timings/HTTP statuses. Provider-backed replies and
usage/unavailable errors have an opaque correlation header. Budget denial returns
429 before external invocation; the browser explains local commands remain usable.

Restart preserves counters. Restore conservatively holds provider calls until
the next UTC day, preserves recorded outcomes, and leaves local CRM available.
This is request accounting for this installation, not token/cost accounting,
Google-project-wide limits, financial truth or durable autonomous Act execution.

## Passed verification

| Check                     | Result                                                                                                                                                                                      |
| ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm verify`             | All 8 gates; 1,377 tests in 89 files; 14 matching compiled artifacts across two independent core consumers.                                                                                 |
| `pnpm crm:verify`         | 26/26 PostgreSQL groups, including shared admission, eight denied overlapping calls, charged failures/interruption, RLS/pool cleanup, rollback, UTC rollover, checksums and actual restart. |
| `pnpm crm:verify-runtime` | 25/25 groups, including metered metadata, persistent exhaustion, correlated 429, real backup/restore hold and zero external requests under a network blocker.                               |
| `pnpm e2e:crm-runtime`    | 38/38 Chromium checks; quota messaging and all previous authenticated CRM/pipeline/Save/cleanup flows.                                                                                      |
| Actual-host live smoke    | One authenticated committed synthetic Gemini request succeeded and correlated with its metadata; business writes were zero.                                                                 |
| Actual-host restart       | Application/database restarted; allowance remained one used / 29 remaining, with the exact same successful outcome and timestamps.                                                          |
| Existing installation     | Private backup taken before upgrade; original data/account/key/membership fingerprints verified before/after migration and smoke; no test records added.                                    |

Evidence: [SQL and consumer hashes](audit/2026-10-09-provider-runtime-db-proof.json),
[host/recovery checks](audit/2026-10-09-provider-runtime-host-proof.json),
[browser summary](audit/2026-10-09-provider-runtime-browser-proof.json),
[actual metered request and restart](audit/2026-10-09-provider-runtime-live-smoke.json).

Ordinary verification uses disposable databases and no live key. The actual-host
smoke sends only the existing synthetic prompt; its temporary session is logged
out. The original installation remains running at `http://127.0.0.1:4175`.

## Preserved boundaries and remaining work

The new schema is `archava_provider`; CRM/identity migrations remain byte-identical.
Existing pinned pg/Zod versions are reused; no third-party package/version is
added to the workspace. Pricing/template configuration, runtime module registry,
archived PRD, standalone realtime template and both Act readiness directories are
unchanged from the baseline. The ARCHZXY website remains outside this repository.

Arbitrary private text still stays local, and only existing curated examples use
the operator-confirmed Free Gemini project. Saving follow-ups still requires an
explicit human click and existing backend permission/revision/idempotency checks.
Autonomous writes, payments, broader model/data evaluation, automated alerts,
production deployment, cost/retention policy and shared voice/avatar remain open.
No historical commercial percentage or Pending Act task is marked complete.

See [provider-runtime scope](PROVIDER_RUNTIME.md) for admission, recovery and
operator behavior. Existing pre-module provider usage is not backfilled.
