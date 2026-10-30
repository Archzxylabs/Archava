# Act production readiness: parallel delegation

## Starting state and objective

Worktree: `/home/haikaru/Archverse/Lab/Archava_Act_Readiness`

Branch: `build/act-production-readiness`, starting commit `b2e3f0325d09234563b1e3b4c3da1153f57d3bcb`. This is stacked on draft PR #3, which is stacked on PR #2. Do not merge or retarget either PR. A frozen-lockfile install has been completed in this worktree.

The previous phase has booking/email executors, replay/outbox boundaries, and offline tests. It does **not** have a production database, a selected PMS or mail provider, production server composition, verified server-side confirmation, or a live Jev evaluation. This round builds independent infrastructure seams that are useful without external credentials or a vendor choice. It does not claim PRD §34 completion.

## Exclusive ownership

| Agent | Edit scope                                                                               | Deliverable                                                                          |
| ----- | ---------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------ |
| a1    | `packages/act-storage/**` and Result in `delegation/act-production-readiness/a1.md`      | PostgreSQL attempt-store adapter and schema for both existing store ports            |
| a2    | `packages/act/**` and Result in `delegation/act-production-readiness/a2.md`              | fail-closed server-side Act composition factory that requires replay/outbox wrappers |
| a3    | `packages/act-confirmation/**` and Result in `delegation/act-production-readiness/a3.md` | server-side, one-time confirmation challenge/receipt service                         |

Read any source, but edit only your assigned paths. Do not change root manifests, `pnpm-lock.yaml`, shared docs, `packages/assistant`, `packages/acl`, `packages/act-booking`, `packages/act-email`, `apps/web`, or another agent's package. If an existing port must change, describe the exact proposed patch in your Result; the lead will reconcile it. Do not install dependencies, commit, push, merge, switch branches, reset, stash, or restore. Do not run a broad formatter while others edit. Run focused tests, typecheck, lint and format only for your area. A new package may have its own `package.json` and `tsconfig.json`; avoid new external dependencies in this parallel round.

Frozen authority path: ActionPolicy → verified user confirmation → input validation → trusted entity resolver → ActionExecutor → authoritative gateway. A direct call to an executor or a confirmation port never grants permission. A DecisionProvider never decides whether an action is allowed or succeeded. No unknown external outcome may trigger a blind second side effect. Keep secrets, raw form values and provider credentials out of browser bundles, attempt records, public errors and logs.

The lead will audit source and tests, reconcile shared integration, update documentation and PRs, then run full gates. Each agent must fill its Result section with changed files, exact commands/results, safety proof, remaining external gaps, and any shared-file patch request. The owner will say `all done` when the round is ready for lead audit.
