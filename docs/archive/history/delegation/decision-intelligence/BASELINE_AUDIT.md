# Forensic baseline — 2026-09-27

## Git state before delegation files

- Current branch: `build/decision-intelligence`, tracking `origin/build/decision-intelligence`.
- Starting `HEAD`, `main`, `origin/main`, and `build/decision-intelligence`: `c95d05c92e2d2c520a8e86e49da296ecc889cad1` (`foundation-v1.1`). No commits ahead of Foundation V1.1.
- No staged changes. Foundation tags are intact.
- Existing unstaged tracked changes: `packages/adapters/package.json`, `packages/adapters/src/index.ts`, `pnpm-lock.yaml`.
- Existing untracked work: `packages/adapters/src/decision.ts`, `packages/adapters/test/decision.test.ts`.
- These files are previous Decision Intelligence work and must be preserved. The port defines `DecisionProvider`, typed boolean/choice/score questions/results, refusals, and basic question validation. It is not yet connected to a turn or to a provider. Its response validation, task policy, privacy enforcement, fallback, and mode handling are still absent.

## Source audit relevant to the split

- `packages/assistant/src/turn.ts` runs a turn with deterministic classification before truth/retrieval, then masks/projects context, calls `BrainProvider`, and gates action requests through `ActionPolicy`, input validation, entity resolution, and `ActionExecutor`. `TurnOutcome` separates answer basis, knowledge gap, handoff, and events.
- `packages/knowledge/src/structured-truth.ts` owns the current keyword/possessive classification; `packages/knowledge/src/retrieval.ts` stays tenant scoped. A new decision may escalate an ambiguous case to an identified structured-truth subject, but must not downgrade an existing structured-truth case.
- `packages/config/src/client/schema.ts` is strict and has no Decision Intelligence field. An optional field with an explicit off default policy is required without changing old configs' behavior. `tools/configurator/src/client-config.ts` emits existing configs and is left for lead reconciliation.
- `apps/web/src/slice.ts` wires the offline reference brain, knowledge, truth, policy, resolver, and page executor. `apps/web/src/page.ts` runs the SDK observation/turn/render order. The current browser is not a server-side provider host; credentials must never be added to this bundle.
- `packages/adapters/src/decision.ts` is a new, uncommitted port adjacent to `BrainProvider`; Jev should live behind it, not inside `BrainProvider`.
- `packages/acl` owns permissions, `packages/assistant/src/validation.ts` owns action inputs/entities, and `packages/assistant/src/execution.ts` owns execution state. `packages/pricing` and `config/pricing.v1.json` remain untouched.
- `pnpm-workspace.yaml` discovers `packages/*`; `vitest.config.ts` aliases package source automatically. `pnpm structure` checks declared workspace imports and cross-package filesystem imports. CI runs frozen install, structure, lint, typecheck, test, build.
- `PRD.md` and `agent.md` preserve the product and quote constitution. Decision Intelligence needs additive documentation, not pricing or Presence/Capability changes.

## Checks at handoff

- `pnpm structure`: passed, 12 workspace projects.
- `pnpm --filter @archava/adapters typecheck`: passed.
- `pnpm exec vitest run packages/adapters/test/decision.test.ts`: passed, 18 tests.
- These are targeted baseline checks, not the full pre-PR gate set.

The lead inspected the requested Git metadata/diffs, the foundation docs and config, package manifests, and relevant implementation across core, SDK, knowledge, ACL, assistant, adapters, reference, config, pricing, chat, web, configurator, scripts, E2E and CI before writing the assignments. The complete gates still belong to the integration phase.
