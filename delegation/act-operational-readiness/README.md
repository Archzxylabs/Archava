# Act operational readiness: parallel delegation

Integration status (9 October 2026): the lead completed the offline library work
in the primary workspace. The prompts below are retained as historical scope;
the Results record delivered code and evidence. Production owner/provider wiring
is still pending. See [durable Act operations](../../docs/DURABLE_ACT_OPERATIONS.md).

Workspace: `/home/haikaru/Archverse/Lab/Archava_Platform`. Baseline: consolidated `main` after PRs #2–#5 and #9 were merged. The old `Archava_Act_*` worktrees have been removed; continue in this primary workspace on the lead-selected working branch. Inspect `git status --short` and current HEAD before editing; preserve all work. Do not switch branches, reset, rebase, stash, merge, or create another worktree.

This round addresses three independent gaps identified by the PR #5 audit. No agent may claim a production Act workflow, live Postmark send, or real PMS booking. Ordinary tests must remain offline and credential-free. The reference browser must still work offline.

| Agent | Exclusive write scope                                                    | Deliverable                                                                           |
| ----- | ------------------------------------------------------------------------ | ------------------------------------------------------------------------------------- |
| a1    | `packages/act-host/**`, new `packages/act-host-pg/**`, Result in `a1.md` | atomic durable host-attempt lifecycle                                                 |
| a2    | `packages/act-postmark/**`, Result in `a2.md`                            | conservative positive-only status reconciliation if official Postmark APIs support it |
| a3    | new `tools/act-integration/**`, Result in `a3.md`                        | offline end-to-end composition harness across the public Act seams                    |

Read any file, edit only your scope. If a shared contract or root file needs a patch, describe it precisely in your Result for the lead. Do not edit another agent's scope, root manifest/lockfile, shared docs, `apps/web`, or CI. Do not install dependencies, commit, push, open a PR, or merge. Run focused tests, typecheck, lint, format, and `pnpm structure` where relevant. The lead owns integration, root lockfile/docs, full gates, Git, and the stacked PR.

Authority remains: structured truth for facts; deterministic code for rules; `ActionPolicy` for permission; validation and tenant-scoped entity resolution for inputs; one-time server verification for consent; `ActionExecutor` and authoritative providers for effects. Model confidence and browser flags grant no authority. Unknown booking or mail outcomes never trigger a blind second write. No raw form data, credentials, token, upstream idempotency key, or provider payload in logs or browser results.

Each agent fills the Result section of its own file with changed files, exact commands/results, safety proof, remaining gaps, and shared patch requests. When all three Results are ready, the owner tells the lead `all done`.
