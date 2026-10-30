# Act pilot + Jev shadow evaluation: parallel delegation

## Baseline and objective

This delegation starts from commit 2a689970c1e5d58ccdcead493fc321589e24bd80 on branch build/act-pilot in the separate worktree:

/home/haikaru/Archverse/Lab/Archava_Act_Pilot

The branch is based on the Decision Intelligence implementation currently proposed in PR #2. Do not merge PR #2 or write Act changes to build/decision-intelligence. The original worktree remains untouched. A frozen-lockfile install was completed in this worktree before delegation.

The immediate product objective is a reviewable Act pilot: provider-neutral booking and email executors that can be composed through the existing ActionPolicy → input validation → trusted entity resolver → user confirmation → ActionExecutor path. In parallel, build an explicit live Jev evaluation command so the decision layer can be judged in SHADOW mode when a credential and data terms are available. No agent should claim that a mock proves an external booking, email delivery, or live Jev quality.

## Parallel ownership

| Agent | Exclusive code ownership                                                                                   | Deliverable                                                                           |
| ----- | ---------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| a1    | packages/act-booking/\*\* and the Result section of delegation/act-pilot/a1.md                             | booking.create executor and injected authoritative booking gateway                    |
| a2    | packages/act-email/\*\* and the Result section of delegation/act-pilot/a2.md                               | email.send executor, tenant template lookup, and injected delivery gateway            |
| a3    | tools/decision-eval/\*\*, docs/JEV_EVAL_READINESS.md, and the Result section of delegation/act-pilot/a3.md | explicit synthetic Jev shadow eval, metrics, safety tests, current official-doc audit |

Agents may read any repository file. They must edit only their owned paths. Do not change shared package manifests outside your package, pnpm-lock.yaml, root package.json, packages/assistant, packages/acl, packages/decision, packages/adapters, apps/web, PRD.md, or common docs. If a safe solution requires one of those changes, describe the exact patch requested in your Result section. The lead owns reconciliation and integration after all three finish.

Do not run pnpm install while others are editing manifests. Run focused typecheck and tests with the dependencies already installed. Do not commit, push, switch branches, reset, stash, restore, or overwrite others' work. Git status may contain another agent's files. Do not run a broad formatter that rewrites other paths.

## Frozen shared contract

ActionExecutionRequest and ActionExecutor are exported from @archava/assistant. A booking or email executor receives tenantId, sessionId, action, validated inputs, and idempotencyKey. It returns succeeded only when its external side effect has a verified outcome; otherwise it returns a safe failed result. It does not grant permission, infer confirmation, resolve entity existence, choose a price, or make a DecisionProvider authoritative. L3 actions booking.create and email.send require Act and user confirmation under @archava/acl. Existing Foundation behavior must remain intact.

The reference browser is offline and currently has no live booking or email service. External provider choice, credentials, tenant configuration, production wiring, end-to-end Act scenario, full gates, documentation updates, and any PR are lead responsibilities after delegation. Do not use a fixture as proof of a real booking or email.

## Lead acceptance after all done

The lead will audit each file, integrate the two executors behind a server-side dispatcher and trusted tenant configuration, provide an authoritative entity resolver and confirmation flow, run focused end-to-end Act tests and all repository gates, update delivery progress, and decide which real external systems can be tested with owner-provided credentials. The lead must keep Jev optional and in SHADOW until measured. If an external provider cannot be validated, mark the pilot honestly as mocked/port-ready rather than commercially complete.
