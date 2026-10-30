# Act hardening: parallel delegation

## Starting state

Worktree: `/home/haikaru/Archverse/Lab/Archava_Act_Pilot`

Branch: `build/act-pilot`, starting commit `31b2f39096c8b048911ba52fa5770417f3b2a540`.

PR #3 is a draft into `build/decision-intelligence`. Do not merge it. Booking and email executors currently use injected gateway ports and fake gateways in tests. No real reservation or mail provider, persistent store, production confirmation UI, or live Jev result has been proved.

## Parallel ownership

| Agent | Exclusive edit scope                                                       | Goal                                                                      |
| ----- | -------------------------------------------------------------------------- | ------------------------------------------------------------------------- |
| a1    | `packages/act-booking/**`, `delegation/act-hardening/a1.md` Result section | Booking replay and ambiguous-outcome recovery contract plus offline tests |
| a2    | `packages/act-email/**`, `delegation/act-hardening/a2.md` Result section   | Email outbox/idempotency recovery contract plus offline tests             |
| a3    | `tools/decision-eval/**`, `delegation/act-hardening/a3.md` Result section  | Labelled evidence-sufficiency SHADOW evaluation plus offline tests        |

Read any file, but edit only your owned paths. Do not edit this README, root manifests, lockfile, `packages/assistant`, `packages/acl`, `packages/decision`, `packages/adapters`, `packages/act`, `apps/web`, PRD, or shared docs. Put a proposed patch for shared files in your Result section; the lead will reconcile it. Do not commit, push, merge, change branches, reset, stash, restore, or rewrite another agent's files. Do not run `pnpm install` or a broad formatter while other agents work. Run focused tests/typecheck on your owned package/tool.

Preserve the authority chain: ActionPolicy → confirmation → validation → trusted entity resolver → ActionExecutor → authoritative gateway. DecisionProvider cannot authorize or execute. Unknown external outcomes are never success and must never cause a blind second side effect. Keep tenant boundaries and PII-safe public output. Ordinary CI must stay network-free and credential-free.

When complete, write the Result section in your own file with changed files, design, exact commands/results, remaining external gaps, and any shared-file patch request. The lead audits and integrates after the owner says `all done`.
