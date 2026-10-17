/**
 * The artifact-bound pricing proofs — run by the gate, after the build.
 *
 * ## Why this file exists, and why it is not in the default unit suite
 *
 * RA03: a fresh `pnpm test` on a tree that has never run `pnpm rust:benchmark`
 * must pass without prior dist/target/owner evidence. The original
 * `pricing-coverage.test.ts` broke that rule twice over: it asserted
 * `existsSync(BUILT_ENGINE)` as a unit test (so an unbuilt tree failed the whole
 * suite), and two of its tests opened with `if (!existsSync(...)) return` — a
 * silent early return that made "required assertions actually run" false by
 * construction. A test that returns before asserting anything reports green for
 * the exact condition it was written to catch.
 *
 * So the split is by *what evidence the assertion needs*, not by topic:
 *
 * - `pricing-coverage.test.ts` — pure relationships (the workload arithmetic,
 *   the coverage walker, the recorded split). No artifacts. Passes cold.
 * - this file — every assertion that reads a built artifact. Each one fails
 *   hard when the artifact is absent; there is no skip and no early return
 *   here, because this file is only ever executed after the
 *   `compiled-consumer benchmark` step (`pnpm rust:benchmark`) inside
 *   `pnpm rust:verify-release` (`tools/rust/verify-release.mjs`), where a
 *   missing artifact is a genuine gate failure. That step — not
 *   `rust:build-artifacts`, which compiles `archava-native` for the fast gate —
 *   is what builds the files read below: the esbuild engine bundle, the
 *   workload JSONs, and the `archava-bench` consumer binary.
 *
 * `vitest.artifacts.config.ts` is what keeps the default `vitest.config.ts`
 * include from collecting this file: the exclusion is by filename
 * (`*.artifact.test.ts`), stated once in both configs, so widening a glob in
 * one place cannot silently re-couple the unit suite to the build output.
 *
 * The gate step that runs this file: `rust artifact proofs (vitest)` in
 * `tools/rust/verify-release.mjs`, ordered directly after
 * `compiled-consumer benchmark`.
 */

import { describe, expect, it } from 'vitest'
import { existsSync, readFileSync, statSync } from 'node:fs'
import { fileURLToPath, pathToFileURL } from 'node:url'

/** The case file, read the same way the runner reads it. */
const CASES_PATH = fileURLToPath(new URL('../../parity/cases/pricing.json', import.meta.url))

/** A pricebook, parsed the same way the runner parses it. */
const PRICEBOOK_PATH = fileURLToPath(new URL('../../../../config/pricing.v1.json', import.meta.url))

/** The compiled baseline, the runner's own build output. */
const BUILT_ENGINE = fileURLToPath(new URL('../build/pricing-engine.mjs', import.meta.url))

/** The compiled Rust consumer the peak-memory proof reads. */
const RUST_BIN = fileURLToPath(new URL('../rust/target/release/archava-bench', import.meta.url))

const BUILD_DIR = fileURLToPath(new URL('../build', import.meta.url))
const WORKLOAD = `${BUILD_DIR}/pricing-workload.json`

/** One pricing fixture: a request plus the id it is known by. */
interface PricingCase {
  id: string
  request: {
    region: string
    [rest: string]: unknown
  }
  [rest: string]: unknown
}

interface PricingCaseFile {
  cases: PricingCase[]
}

interface Pricebook {
  [field: string]: unknown
}

interface PricingEngineModule {
  PricingEngine: new (book: Pricebook) => {
    quote: (request: PricingCase['request']) => unknown
  }
}

/** The priced/rejected pair a pricing pass produces. */
interface PricingCoverage {
  priced: number
  rejected: number
  total: number
}

const parsedCases = JSON.parse(readFileSync(CASES_PATH, 'utf8')) as PricingCaseFile
const cases: PricingCase[] = parsedCases.cases
const pricebook = JSON.parse(readFileSync(PRICEBOOK_PATH, 'utf8')) as Pricebook

/**
 * How many pricing requests the baseline quotes, and how many it rejects.
 *
 * @param engineFactory the compiled baseline module
 * @param book the pricebook
 * @param requests the case file's requests, in order
 * @returns the priced/rejected pair, which must sum to the request count
 */
function measurePricingCoverage(
  engineFactory: PricingEngineModule,
  book: Pricebook,
  requests: PricingCase['request'][],
): PricingCoverage {
  const engine = new engineFactory.PricingEngine(book)
  let priced = 0
  let rejected = 0
  for (const request of requests) {
    try {
      engine.quote(request)
      priced += 1
    } catch {
      rejected += 1
    }
  }
  return { priced, rejected, total: requests.length }
}

/**
 * The recorded split these proofs cross-check — declared here rather than
 * imported so this file stays self-contained under its own config, and pinned
 * to the same provenance: one run of the runner against the real pricebook.
 */
const LIVE_PRICED = 338
const LIVE_REJECTED = 12

describe('the compiled artifacts the proofs read', () => {
  it('exists: the baseline engine is built before this file runs', () => {
    // The assertion RA03 moved out of the unit suite, unchanged in strength:
    // no skip, no early return. In the default suite this was the single
    // failing test on a fresh tree (reaudit 2026-10-16, pricing-coverage
    // test.ts:169); here, running after the `compiled-consumer benchmark`
    // step, a missing file is exactly what should fail the gate.
    expect(
      existsSync(BUILT_ENGINE),
      'run `pnpm rust:benchmark` (the release gate runs it as the step directly before this one)',
    ).toBe(true)
  })

  it('exists: the compiled Rust consumer and its workload are present', () => {
    expect(
      existsSync(RUST_BIN),
      'the benchmark binary and workload are build artifacts of `pnpm rust:benchmark`',
    ).toBe(true)
    expect(existsSync(WORKLOAD), 'the workload JSON is written by the same build').toBe(true)
  })
})

describe('the baseline against the real pricebook', () => {
  it('prices and rejects a pair that accounts for all 350 fixtures', async () => {
    // The only test here that needs the compiled engine, and the one that makes
    // the recorded numbers checkable rather than folklore. It used to open with
    // `if (!existsSync(BUILT_ENGINE)) return` — a silent early return whose
    // absence of output read as a pass. The artifact-existence precondition is
    // now a named assertion above, so by the time this runs there is no branch
    // left to take but the measurement itself.
    const engineModule = (await import(pathToFileURL(BUILT_ENGINE).href)) as PricingEngineModule
    const covered = measurePricingCoverage(
      engineModule,
      pricebook,
      cases.map((c) => c.request),
    )
    expect(covered.total).toBe(cases.length)
    expect(covered.priced + covered.rejected).toBe(covered.total)
    // Both halves are non-empty, which is the property that makes an assertion
    // against `cases.length` wrong and an assertion against the baseline right.
    expect(covered.priced).toBeGreaterThan(0)
    expect(covered.rejected).toBeGreaterThan(0)
    // And the recorded figures are still what the engine does.
    expect(covered.priced).toBe(LIVE_PRICED)
    expect(covered.rejected).toBe(LIVE_REJECTED)
  })
})

describe('peak memory attribution', () => {
  it("carries the consumer's own peak into the evidence, not a substituted one", async () => {
    // Precondition asserted above (`exists: the compiled Rust consumer…`);
    // no early return, same reason as the pricebook walk.
    const { execFileSync } = await import('node:child_process')
    const out = JSON.parse(execFileSync(RUST_BIN, [WORKLOAD], { encoding: 'utf8' })) as {
      peakRssKiB: number
    }
    expect(out.peakRssKiB).toBeTypeOf('number')
    expect(out.peakRssKiB).toBeGreaterThan(0)
    // It is the peak of the process that did the work, so it has to be at
    // least the whole case set it just loaded. A figure below the input it
    // read cannot be that process's high-water mark.
    const workloadBytes = statSync(WORKLOAD).size
    expect(out.peakRssKiB * 1024).toBeGreaterThan(workloadBytes)
  })
})
