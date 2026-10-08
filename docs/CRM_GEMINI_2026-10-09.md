# 9 October 2026 — Gemini CRM pilot verification

Baseline: `8ea80ee`. Environment: Node 22.22.3, pnpm 10.15.1, PostgreSQL 18.6,
local Chromium. This is the first bounded live-planning slice of M3, with explicit
human operation. M3 and the existing Act operational/production gates remain open.

## Delivered behavior

- Optional `@archava/crm-assistant` 0.2.0 `/server` planner uses pinned
  `gemini-3.5-flash-lite`, structured intent output and a committed synthetic
  corpus. Two independently installed consumers use identical implementation
  and tarball hashes. CRM data and arbitrary private text stay outside Google.
- The conversation adapter can list/search opportunities and summarize persisted
  stage history with separate grants and explicit opportunity selection.
- A human reviews a contact-bound follow-up and clicks Save. The existing command
  API rechecks permissions, scope and revision, retains retry identity and stores
  an audited task. Conversation turns and machine principals cannot write or send.
- The authenticated host chooses Gemini only for exact pilot examples, exposes
  its planner mode, and uses a shared credential-free browser panel. A missing or
  disabled private configuration preserves local commands.

## Passed checks

| Check                         | Evidence                                                                                                                                                                                       |
| ----------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `pnpm verify`                 | All 8 gates; 1,366 tests in 88 files; core reuse across two independent installs.                                                                                                              |
| `pnpm crm:gemini-eval --live` | 13/13 live synthetic cases across hospitality/professional services; zero writes during reads/drafts; one explicitly reviewed task after eight identical retries and actual database restart.  |
| `pnpm crm:verify`             | 20/20 real PostgreSQL groups; matching compiled module hashes across businesses.                                                                                                               |
| `pnpm crm:verify-runtime`     | 21/21 groups; sessions/CSRF, permissions/revision/retry, application/database restart and real `pg_dump`/`pg_restore`. Zero provider calls.                                                    |
| `pnpm e2e:crm-runtime`        | 37/37 browser checks, including example rendering without automatic submission, opportunity selection/history, explicit Save after response loss, stale drafts and late receipts after logout. |
| Final runtime smoke           | Actual local credentials/session route to Gemini and return an authorized contacts projection; zero business mutations. The smoke session was explicitly logged out.                           |
| Existing installation         | Original business data, account/key/grant fingerprints retained; no test contacts/opportunities added. Private Gemini configuration enabled only after the operator confirmed a Free project.  |

The final library artifact hashes exactly match the successful live-evaluation
artifacts. Live proof records the safe provider snapshots and 13 successful
attempts in that run. Earlier development attempts stopped on timeout/network
failure; the final run passed after bounded deadlines and host DNS preference
were adjusted. No request has an automatic retry or alternate-model/tier fallback.
The current project tier was confirmed by the operator; the API key itself does
not verify billing status.

Durable, credential-free evidence:

- [Live cases and artifact hashes](audit/2026-10-09-crm-gemini-live-proof.json).
- [Real SQL checks and consumer hashes](audit/2026-10-09-crm-gemini-db-proof.json).
- [Persistent host and recovery checks](audit/2026-10-09-crm-gemini-runtime-proof.json).
- [Browser check summary](audit/2026-10-09-crm-gemini-browser-proof.json).

The browser's authenticated **pilot mode** example-rendering check uses a
controlled session-metadata fixture and submits nothing to a provider. The
separate live evaluation and actual-host smoke exercise Google. Ordinary CI never
reads a live key. All record-writing test cases use disposable private databases.

## Preserved boundaries

No new dependency or database migration. CRM/CRM-PG remain 0.2.0; both original
migrations remain byte-identical. Pricing/template configuration, the runtime
module registry, archived PRD, standalone realtime template and both Act readiness
task directories are unchanged from the baseline. Archived PRD SHA-256 remains
`1a47f2617029310364a52553cec3a0272414b06eb1c2866ac119241cbd05871e`.
The ARCHZXY company website remains a separate future consumer.

This release proves constrained intent routing and human-mediated task storage.
It does not prove general generative conversation, autonomous approvals/actions,
durable provider budgets, production observability/deployment, payments or shared
voice/avatar. Follow-up storage does not send a message. Free-service data and
usage limits are specified in [pilot scope](CRM_GEMINI_PILOT.md).

## Local operation

The original installation is running at `http://127.0.0.1:4175`. Sign in, choose
one of six example requests in the Archava panel, and submit. For a follow-up,
select a contact, supply its message/due time, request a draft and explicitly Save
after review. Wait at least 15 seconds between Gemini submissions. Arbitrary text
continues to use local commands. No private key is shown, bundled or committed.
