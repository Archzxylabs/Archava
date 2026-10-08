# Cleanup and first reuse proof

Implemented after the [7 October audit](PROJECT_AUDIT_2026-10-07.md), preserving
the existing platform and standalone template behavior.

## Delivered

- Updated all workspace Vitest/coverage instances to 4.1.11, pinned patched
  source-map-js 1.2.2, moved template Vite to 6.4.4 and aligned Node minimum/CI
  to 22.22.3. Root/CI previously allowed runtimes below a template dependency's requirement.
- Replaced runtime browser imports with explicit `@archava/config/pure` imports;
  removed the web app's special configuration alias. Fixed missing config export files.
- Added nine compiled private ESM/declaration artifacts and two independent
  hospitality/retail consumers. Every installed implementation file is hashed;
  all nine packages match across consumers. Class identity across exports is preserved.
- Added a thin, host-owned module registry and versioned deployment plan.
  Declared scope stays separate from template suggestions and verified host
  integration snapshots. Unsupported CRM stays planned. Duplicate/unknown modules,
  dependency cycles/opt-outs and cross-tenant evidence fail validation.
- Preserved V1 behavior through an explicitly deprecated compatibility resolver.
  New quote responses add a deployment plan. Catalog lookup now works from the
  nested server directory; both education and the other catalog industries pass
  the quote/config path. Existing pricebook and template catalog bytes are unchanged.
- Refused SDK tenant/config mismatches at session creation.
- Archived the unused Nx placeholder, indexed completed task history and kept
  three operational-readiness tasks Pending. Added format/reuse CI gates and
  refreshed the standalone template export.

## Evidence

Node **22.22.3**, pnpm **10.15.1**:

| Check                               | Result                                                                                           |
| ----------------------------------- | ------------------------------------------------------------------------------------------------ |
| `pnpm verify`                       | Eight gates passed: install, structure, format, lint, typecheck, tests, build, reuse             |
| Platform suite                      | 1,308 tests across 82 files passed                                                               |
| Artifact consumption                | Two separate cold installs, TypeScript checks, Node turns and browser builds; identical hashes   |
| Consumer browser                    | Both native DOM embeds render their own grounded content without runtime/resource errors         |
| Existing reference/frontend browser | 31/31 + 16/16 passed                                                                             |
| Marketing viewport                  | 360, 768, 1440 px without overflow/errors                                                        |
| Quote HTTP                          | An empty integration scope stays empty; client-authored host health is ignored                   |
| Template                            | Fresh npm install, 55 JavaScript tests, 4 Python 3.12 guardrails, build and source export passed |
| Dependency registry snapshot        | Platform all dependencies: zero advisories; template omit-dev: 13 moderate, zero high/critical   |

The full gate rerun includes the nested-catalog regression. The final pure
catalog lookup interface was subsequently checked with artifact/declaration and
focused lint checks. Browser favicon setup was corrected before successful runs.
Results apply to controlled offline fixtures and internal artifacts.

A [saved consumer proof](audit/2026-10-08-reuse-proof.json) records the versions,
whole-implementation hashes and differing consumer results. Dependency evidence is in [the sanitized snapshot](audit/2026-10-08-cleanup-dependencies.json).
Compiled artifacts/proof are generated under ignored `dist/library`; runnable
embeds are under `dist/reuse-demo`. Instructions are in
[library consumption](LIBRARY_CONSUMPTION.md). Local logs/screenshots are in
`/tmp/archava-cleanup-*`; generated outputs are not committed source.

Vitest migration and Vite major changes were reviewed against official
[Vitest guidance](https://vitest.dev/guide/migration/) and
[Vite 6 migration guidance](https://v6.vite.dev/guide/migration.html).
Source-map-js/Tinypool fixes address the registry findings documented in the
original audit. No automatic force-fix or avatar-vendor downgrade was used.

## Remaining boundaries

The template's 13 moderate package entries arise through vendor-pinned
OpenTelemetry components. Several parents pin core 2.7.1 exactly, so updating
the root transitive entry alone cannot resolve all advisories. Use a compatible
vendor/telemetry update with provider validation; do not present the template as
having a clean audit.

Next business milestone is a persistent contacts + activities/follow-up module
reused in two contexts. Native CRM/ERP persistence, production tenant identity,
durable ActHost intent/attempt recovery, provider reconciliation and shared
realtime intelligence remain unimplemented. Thin metadata is not a full module
installer or semver-range/migration engine. Planning readiness is not permission.

No provider calls, deployment, Docker image run, SQL verifier rerun or SDK Python
integration rerun occurred in this cleanup. The SQL checks remain evidence from
the prior audit; SDK integration requires its missing venv. Changes remain local,
without a new commit, PR or push.
