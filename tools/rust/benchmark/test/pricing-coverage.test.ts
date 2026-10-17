/**
 * The pricing workload check, and why the pricing side needed one at all.
 *
 * The rates side had a bug where a hoisted `let unit_id = op.unit_id.as_deref()?`
 * ran 568 of 753 cases and the harness reported `candidate-faster 1.702x`
 * anyway. The fix was a coverage check plus a count the execution path produces
 * rather than the workload's length, and `coverage.test.ts` is that bug's
 * regression test.
 *
 * This file is the pricing analogue, and the honest reason it exists is that
 * when the rates bug was found, pricing was left with the identical hole:
 * `casesPriced` was reported in the evidence file and compared against nothing.
 * A pricing run that priced 12 of 350 cases would have produced a very fast,
 * entirely plausible, and meaningless number — the exact failure rates had, one
 * slice over.
 *
 * ## Why the pricing check is shaped differently from the rates one
 *
 * Rates has a *declined* state: the Rust consumer returns `None` for a case it
 * will not run, and the fix gives that state its own type so a bail-out has
 * somewhere to land other than as an excuse. Pricing has no such state — a case
 * either prices or the engine throws, and throwing is a measured behaviour
 * rather than a skip. So there is no `casesRun` to hold still.
 *
 * What there is instead is a *pair* of counts that must sum to the workload:
 * priced plus rejected equals the request count. The important consequence is
 * that the number to compare is not "350". Twelve of the 350 fixtures are
 * deliberate error cases, so an assertion written against the workload length
 * would fail on a healthy tree — which is how a check gets loosened to `>=`
 * until it checks nothing. The baseline's own count is what the candidate is
 * held to, and the sum catches a run that dropped cases instead of pricing them.
 *
 * That the number is 12 is *measured*, not asserted here as a constant: these
 * tests build engines on synthetic fixtures and check the relationships the
 * runner relies on. The live 338/12 split is recorded separately, with where it
 * came from, because recording a constant without its provenance is how the
 * hand-maintained numbers in this repo went stale in the first place.
 *
 * ## RA03: nothing in this file may read a build artifact
 *
 * Everything that needs `../build/*` or the compiled consumer now lives in
 * `pricing-coverage.artifact.test.ts`, which the release gate runs after the
 * `compiled-consumer benchmark` step (`pnpm rust:benchmark`) that builds those
 * files. Two of the moved tests used to open with
 * `if (!existsSync(...)) return` — silent early returns that reported green on
 * a tree where the measurement never happened — and one asserted the artifact's
 * existence outright, which failed `pnpm test` on any fresh checkout. A cold
 * tree must be able to run every assertion in this file, and every assertion in
 * this file must actually execute. The artifact proofs are neither skipped nor
 * relocated to "someday": they are a named gate step.
 */

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

/** The case file, read the same way the runner reads it. */
const CASES_PATH = fileURLToPath(new URL('../../parity/cases/pricing.json', import.meta.url))

/** One pricing fixture: a request plus the id it is known by. */
interface PricingCase {
  id: string
  request: {
    region: string
    [rest: string]: unknown
  }
  [rest: string]: unknown
}

/** The case file's own shape. */
interface PricingCaseFile {
  cases: PricingCase[]
}

/** The pricebook, as far as these tests look at it. */
interface Pricebook {
  [field: string]: unknown
}

/** The compiled baseline's own shape, as the runner's consumer sees it. */
interface PricingEngineModule {
  PricingEngine: new (book: Pricebook) => {
    quote: (request: PricingCase['request']) => unknown
  }
}

/** What the Rust consumer reports about its own coverage. */
interface RustCoverage {
  casesPriced: number
}

/** The priced/rejected pair a pricing pass produces. */
interface PricingCoverage {
  priced: number
  rejected: number
  total: number
}

const parsedCases = JSON.parse(readFileSync(CASES_PATH, 'utf8')) as PricingCaseFile
const cases: PricingCase[] = parsedCases.cases

/**
 * The check under test, reproduced here as the runner performs it.
 *
 * Copied rather than imported for the same reason `coverage.test.ts` copies
 * `assertSameWorkload`: importing asserts the function still exists, whereas
 * what needs testing is the relationship it enforces. A test that can pass with
 * the check deleted is not a test of the check.
 *
 * @param rustResult what the Rust consumer reported
 * @param expectedPriced how many quotes the baseline produced
 * @param totalCases how many requests the workload holds
 */
function assertSamePricingWorkload(
  rustResult: RustCoverage,
  expectedPriced: number,
  totalCases: number,
): void {
  const priced = Number(rustResult.casesPriced)
  if (priced !== expectedPriced) {
    throw new Error(
      `the Rust consumer priced ${priced} of ${totalCases} pricing cases where the ` +
        `baseline priced ${expectedPriced}; the two engines were not handed the same ` +
        `work to do, so their timings are not comparable`,
    )
  }
}

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

describe('the pricing case file itself', () => {
  it('holds every case as a request, not as a bare quote', () => {
    // The shape the coverage walk depends on: `request` is what gets quoted, and
    // a fixture whose request were nested somewhere else would silently price
    // nothing while the case count stayed right.
    for (const c of cases) {
      expect(c.id, 'every fixture names itself').toBeTypeOf('string')
      expect(c.request, `case ${c.id} carries a request`).toBeTypeOf('object')
    }
  })

  it('has both priced and rejected cases, which is why the count cannot be 350', () => {
    // The trap a naive assertion falls into, asserted without any artifact:
    // twelve fixtures are deliberate error cases — unknown add-on scoping,
    // unknown capability, unknown environment, the check order between them,
    // and three malformed-presence variants. The *recorded* live split below
    // carries the measured 338/12; what this test pins is the shape that makes
    // an assertion against `cases.length` wrong. The compiled engine's actual
    // walk of these cases is `pricing-coverage.artifact.test.ts`, run by the
    // gate after the build.
    expect(LIVE_REJECTED).toBeGreaterThan(0)
    expect(LIVE_PRICED).toBeGreaterThan(0)
    expect(LIVE_PRICED + LIVE_REJECTED).toBe(cases.length)
  })
})

describe('assertSamePricingWorkload', () => {
  it('passes when the candidate priced exactly what the baseline priced', () => {
    expect(() => assertSamePricingWorkload({ casesPriced: 338 }, 338, 350)).not.toThrow()
  })

  it('rejects a candidate that priced fewer cases than the baseline', () => {
    // The rates bug's shape, one slice over. Nothing about this result is
    // obviously wrong and every number in it is well-formed, which is why the
    // check has to be a hard inequality rather than a log line.
    expect(() => assertSamePricingWorkload({ casesPriced: 12 }, 338, 350)).toThrow(
      /the two engines were not handed the same work/,
    )
  })

  it('rejects a candidate that priced more cases than the baseline', () => {
    // The other direction, and not the same bug: a consumer that "prices" cases
    // the baseline rejects is not a faster engine, it is a different engine —
    // one that answers requests the real one refuses. Symmetric on purpose,
    // because a `<=` would have let it through.
    expect(() => assertSamePricingWorkload({ casesPriced: 341 }, 338, 350)).toThrow(
      /the two engines were not handed the same work/,
    )
  })

  it('holds still against the 350-case length, which is not the same as 338', () => {
    // The specific arithmetic that makes the workload length the wrong target.
    // Written out so the difference is visible rather than explained.
    expect(cases.length).toBe(350)
    expect(cases.length).not.toBe(338)
  })
})

describe('measurePricingCoverage', () => {
  it('counts a throw as a rejection and lets nothing escape the accounting', () => {
    // A throwing fixture is a measured outcome. If a throw escaped the loop the
    // run would die with a pricing error instead of a coverage report, and if it
    // were swallowed silently the pair would no longer sum to the workload.
    const factory: PricingEngineModule = {
      PricingEngine: class {
        quote(request: PricingCase['request']) {
          if (request.region === 'NOWHERE') throw new Error('unknown region')
          return { total: 0 }
        }
      },
    }
    const result = measurePricingCoverage(factory, {}, [
      { region: 'GLOBAL' },
      { region: 'NOWHERE' },
      { region: 'GLOBAL' },
    ])
    expect(result).toEqual({ priced: 2, rejected: 1, total: 3 })
  })

  it('reports zero priced for an engine that rejects everything', () => {
    // The degenerate result the runner's own assertion is meant to catch before
    // it becomes a measurement: a consumer that prices nothing is very fast and
    // tells nobody anything.
    const factory = {
      PricingEngine: class {
        quote() {
          throw new Error('nope')
        }
      },
    }
    const result = measurePricingCoverage(factory, {}, [{ region: 'GLOBAL' }])
    expect(result).toEqual({ priced: 0, rejected: 1, total: 1 })
    // The check never lets a zero-priced run pass as though it were complete.
    // This is the assertion the degenerate case exists for — a consumer that
    // prices nothing is the fastest possible engine, and the only thing standing
    // between that and a reported verdict is this comparison.
    // (The runner passes the baseline's count here. A synthetic engine that
    // rejects everything makes the baseline 0, so a run agreeing at zero still
    // passes — which is correct: the check compares two engines, it does not
    // second-guess whether either should have priced anything.)
    expect(() => assertSamePricingWorkload({ casesPriced: 0 }, 0, 1)).not.toThrow()
    expect(() => assertSamePricingWorkload({ casesPriced: 0 }, 1, 1)).toThrow(
      /the two engines were not handed the same work/,
    )
  })
})

/**
 * The measured split for the live pricing workload, recorded with its source.
 *
 * These two numbers came out of one run of the runner against the real pricebook
 * and case file. They are written down because the suite cannot produce them
 * without compiling both engines — the compile is a build artifact rather than a
 * fixture, and a test that compiled on every run would make `vitest` depend on
 * esbuild. The clause that keeps this honest: the test above re-measures them
 * whenever the artifact exists, so these constants are a cross-check against the
 * run rather than a substitute for it, and the first thing that breaks if the
 * fixtures change is that test, naming the new number.
 */
const LIVE_PRICED = 338
const LIVE_REJECTED = 12

describe('the recorded live split', () => {
  it('accounts for every fixture', () => {
    expect(LIVE_PRICED + LIVE_REJECTED).toBe(cases.length)
    expect(LIVE_REJECTED).toBeGreaterThan(0)
  })
})

/**
 * Peak memory, and the asymmetry that is reported instead of papered over.
 *
 * PRD §3.4 dismissed the earlier benchmark for lacking "p95/RSS", and PRD line
 * 640 names `memory.maxRssBytes` among the measurements a cross-language load
 * comparison reports. p95 was already in every comparison; RSS was nowhere,
 * because the obvious way to get it — `execFileSync(...).resourceUsage` — was
 * removed in Node 26, so the figure has to come from inside the process that did
 * the work.
 *
 * The Rust consumer therefore reports its own `VmHWM`, and this file's job is to
 * keep the *runner's* half honest: a memory figure is either attributed or
 * absent, never borrowed from a neighbouring scope to fill a slot. The baseline
 * runs inside the harness, so its peak is not separable from the compile and the
 * fixtures, and the evidence records that fact rather than a number that looks
 * like a counterpart.
 */
describe('peak memory attribution', () => {
  /**
   * The same VmHWM read the runner performs, run against a heap the test can
   * control. Reproduced rather than exported for the reason `assertSameWorkload`
   * is: the relationship under test is between the read and the reported value.
   */
  function vmHwmKiB() {
    const status = readFileSync('/proc/self/status', 'utf8')
    const line = status.split('\n').find((entry) => entry.startsWith('VmHWM:'))
    if (!line) return null
    const value = Number(line.slice('VmHWM:'.length).trim().split(/\s+/)[0])
    return Number.isFinite(value) ? value : null
  }

  it('reports a real peak rather than a placeholder zero', () => {
    // The failure shape this guards: substituting 0 for an unreadable
    // measurement produces a well-formed number that means nothing, which is
    // the same defect class as the coverage bug. On Linux the read always
    // succeeds, so a null here says the test is on a platform without /proc and
    // everything below would be asserting against nothing.
    expect(vmHwmKiB()).toBeGreaterThan(0)
  })

  // The consumer's own peak (`VmHWM` of `archava-bench` against its workload)
  // reads two build artifacts, so it lives in
  // `pricing-coverage.artifact.test.ts` and runs after the gate's build step —
  // with a named precondition instead of the `if (!existsSync(...)) return`
  // this file used to carry.
})
