/**
 * Vitest config for the artifact-bound proofs — `pnpm rust:verify-release`
 * runs these.
 *
 * Kept separate from `vitest.config.ts` for one reason, stated once so it
 * cannot drift: RA03 requires the default unit suite (`pnpm test`) to pass on a
 * tree with no `tools/rust/benchmark/build/` output, while the artifact proofs
 * require exactly that output and must FAIL when it is absent. Two opposite
 * requirements over one include list can only be satisfied by two configs.
 *
 * The pairing is deliberate and symmetric:
 *
 * - `vitest.config.ts` collects the package/app/tool test trees and excludes
 *   the `*.artifact.test.ts` suffix;
 * - this config includes that suffix (under the same three roots) and nothing
 *   else.
 *
 * The filename is the single selector. Widening a glob in either config cannot
 * silently move a file across the boundary — the exclusion and the inclusion
 * name the same suffix, so a proof that stops being excluded also stops being
 * collected here only if both are edited together.
 *
 * Gate step: `rust artifact proofs (vitest)` in `tools/rust/verify-release.mjs`,
 * ordered directly after `compiled-consumer benchmark` (`pnpm rust:benchmark`) —
 * the artifacts these tests read (`build/pricing-engine.mjs`, the workload
 * JSONs, the compiled `archava-bench` consumer) are produced by that benchmark
 * step, not by `rust:build-artifacts` (which builds `archava-native` for the
 * fast gate). A missing artifact means the benchmark step lied or the path
 * drifted, and the hard assertions here say so. The fast gate `pnpm rust:verify`
 * never runs this config: it builds the addon but not the benchmark outputs, and
 * its `vitest.config.ts` excludes this filename's suffix.
 */

import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    include: ['{packages,apps,tools}/**/*.artifact.test.ts'],
    environment: 'node',
    globals: false,
    reporters: ['default'],
  },
})
