# Decision Intelligence delegation

This directory is the handoff between the lead orchestrator and agents a1–a3. The owner will say **all done** after the agents have finished; that is the signal for the lead to audit their results and integrate the work. Agents may start in parallel.

## Start here

1. Read [BASELINE_AUDIT.md](BASELINE_AUDIT.md) and your own file below.
2. Read the source relevant to your task. Repository reality wins over summaries.
3. Keep changes inside your assigned write paths. Read any other path freely.
4. Preserve the existing uncommitted adapter work. Do not reset, restore, stash, checkout, or rewrite history.
5. Record your result in the **Result** section of your own file. Include changed files, tests with exact commands/results, unresolved issues, and any interface decision the lead must reconcile.

| Agent       | Assignment                                                              | Exclusive write paths                                                                                                                                                                                                                 |
| ----------- | ----------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [a1](a1.md) | Provider-neutral decision package, rule provider, orchestrator          | `packages/decision/**`, `delegation/decision-intelligence/a1.md`                                                                                                                                                                      |
| [a2](a2.md) | Backward-compatible tenant config and offline evaluation corpus/metrics | `packages/config/src/client/**`, `packages/config/test/**`, `tools/decision-eval/**`, `delegation/decision-intelligence/a2.md`                                                                                                        |
| [a3](a3.md) | Official Jev research and isolated server adapter                       | `packages/adapters/src/jev-decision.ts`, `packages/adapters/test/jev-decision.test.ts`, `packages/adapters/package.json`, `packages/adapters/src/index.ts`, `docs/JEV_PROVIDER_RESEARCH.md`, `delegation/decision-intelligence/a3.md` |

`packages/adapters/src/decision.ts` and its existing test are a **frozen shared contract during parallel work**. It already exists as uncommitted work. Propose any breaking change in your result file; the lead will reconcile it after all done. `pnpm-lock.yaml`, root manifests/scripts, `PRD.md`, `agent.md`, README, architecture/status docs, `packages/assistant/**`, `packages/reference/**`, `apps/web/**`, and Git/PR operations belong to the lead in the integration phase. Do not run `pnpm install` in parallel if it would update the lockfile; the lead will refresh it once after integration.

## Shared task IDs and safety contract

Use these stable IDs in code/config/eval data: `intent_classification`, `knowledge_routing`, `clarification`, `handoff_recommendation`, `evidence_sufficiency`. A1 and a2 should use the same spelling without importing one another during parallel work. The lead will consolidate any duplicated task vocabulary.

The authority hierarchy is: structured source for facts; code for deterministic rules; `DecisionProvider` for bounded fuzzy judgment; `BrainProvider` for conversation; `ActionPolicy` for permission; validation for inputs; `ActionExecutor` for effects. Decision confidence never grants permission, confirms an action, or proves a fact. Deterministic structured truth cannot be downgraded to retrieval. Existing tenant configs must default to Foundation V1.1 behavior. Normal CI and the reference browser app must run without Jev credentials or network calls.

## Lead-owned integration after all done

The lead will audit each diff, resolve contract mismatches, wire approved decision seams into the assistant and reference app, add safety regressions, update product/architecture docs, run the complete gates, then handle branch/PR workflow. Do not mark your assignment as product-complete merely because your isolated package passes.
