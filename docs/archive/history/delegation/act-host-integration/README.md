# Act host integration: parallel delegation

Worktree: `/home/haikaru/Archverse/Lab/Archava_Act_Host`

Branch: `build/act-host-integration`, based on `4073500a22f59509bf1c836471d44328cd8c0042` (draft PR #4). PR #4 is stacked on PR #3, which is stacked on PR #2. Do not merge, rebase, reset, stash, or change branches. Inspect `git status --short` first and preserve all work.

The prior round delivered a PostgreSQL booking/email attempt-store adapter, tenant-bound Act composition, and a server confirmation challenge service. It did **not** deliver a durable challenge store, a trusted server host, a mail provider adapter, a selected PMS, or a production Act workflow. The reference browser remains offline. This round builds three independent seams without credentials. It does not claim PRD §34 completion.

## Exclusive edit ownership

| Agent | May edit                                                | Deliverable                                                                                |
| ----- | ------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| a1    | `packages/act-confirmation-pg/**` and Result in `a1.md` | PostgreSQL implementation of the atomic challenge store                                    |
| a2    | `packages/act-host/**` and Result in `a2.md`            | trusted server adapter for issuing/verifying confirmation and running one bound Act action |
| a3    | `packages/act-postmark/**` and Result in `a3.md`        | official-doc-verified transactional mail gateway adapter with mocked transport             |

Read any source, but edit only your assigned area. New package manifests and tsconfigs inside the assigned package are allowed. Do not edit another agent's package, root manifests/lockfile, shared docs, `packages/assistant`, `packages/acl`, `packages/act`, `apps/web`, or `.github`. If a shared contract must change, write an exact patch request in your Result. Do not install dependencies, commit, push, open/merge PRs, or run a broad formatter. Run focused tests/typecheck/lint/format using root tooling; ordinary CI remains credential-free and network-free. The lead will integrate packages, update the lockfile/docs, audit, run all gates, and prepare the stacked PR.

Frozen authority: structured truth for facts; deterministic code for rules; `ActionPolicy` for permission; trusted validation/entity resolution for input safety; server-verified one-time confirmation for consent; `ActionExecutor` and authoritative providers for effects. A `DecisionProvider` grants none of these. A browser flag or model confidence is never confirmation or execution success. Unknown booking or mail outcomes must not trigger a blind second write. Keep credentials, raw input, raw upstream idempotency keys, and provider payloads out of browser bundles and logs.

Each agent must fill its Result section with exact changed files, commands/results, safety proof, remaining gaps, and any required shared-file patch. The owner will say `all done` when ready for lead audit.
