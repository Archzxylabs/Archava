/**
 * Ambient types for `stats.mjs`, the benchmark statistics module.
 *
 * The statistics are hand-rolled rather than a dependency (see the header of
 * `stats.mjs` itself), so there is no package whose types TypeScript could
 * resolve — and the file is plain JavaScript, which a `.ts` importer cannot see
 * into at all. This is the second file in the repo that needs ambient types for
 * a `.mjs` module, and it follows the same pattern as
 * `tools/library/manifest-module.d.ts` rather than inventing a different one.
 *
 * ## Why an ambient `declare module` rather than a sibling `.d.mts`
 *
 * `tools/library/manifest-module.d.ts` records the reason; it applies here
 * unchanged: TypeScript resolves a `.mjs` import only through a `.d.mts`, and a
 * `.d.mts` file trips the repo's ESLint config out of its typed-rule block. The
 * ambient form is typed without being linted, which is the combination the repo
 * already chose.
 *
 * ## What the types are, and where they came from
 *
 * Every declaration below restates the JSDoc in `stats.mjs` — the same names,
 * the same field names, the same meanings. That is deliberate: the source of
 * truth is the implementation, and a hand-maintained type that drifts from it
 * is worse than no type. If a signature here disagrees with `stats.mjs`, the
 * file is wrong and the tests are the thing that will eventually notice.
 *
 * Declared under a wildcard path so `benchmark.test.ts` can keep importing
 * these names from the module itself rather than from a types file, the way
 * every other test in the repo imports what it uses.
 */

declare module '*/stats.mjs' {
  /**
   * The numeric summary of one sample. Every field is derived from the same
   * input array, so a caller reporting any of them reports all of them — there
   * is no way to quote the median without also having the range.
   */
  export interface Summary {
    /** how many observations */
    n: number
    /** smallest observation */
    min: number
    /** largest observation */
    max: number
    /** the 50th percentile, interpolated */
    median: number
    /** arithmetic mean */
    mean: number
    /** sample standard deviation (n-1); 0 for n < 2 */
    stdev: number
    /** coefficient of variation, stdev / mean; 0 when the mean is 0 */
    cv: number
    /** interpolated 25th percentile */
    p25: number
    /** interpolated 75th percentile */
    p75: number
    /** interpolated 95th percentile */
    p95: number
  }

  /** The verdict on one measured comparison. */
  export type Verdict = 'baseline-faster' | 'candidate-faster' | 'within-noise' | 'inconclusive'

  /** What `compare` returns: the verdict, the summary it came from, and why. */
  export interface Comparison {
    verdict: Verdict
    reason: string
    baseline: Summary
    candidate: Summary
    speedup: number
  }

  /** The inputs `compare` accepts; see its JSDoc for how the three questions are ordered. */
  export interface ComparisonInput {
    /** the baseline observations (microseconds) */
    baseline: number[]
    /** the candidate observations (microseconds) */
    candidate: number[]
    /** override for {@link OVERLAP_FRACTION} */
    overlapFraction?: number
  }

  /**
   * The fraction of the better sample's range that must stay clear of the worse
   * sample's range before the samples count as separated. 0.5 means the ranges
   * may not touch within half the better sample's own spread.
   */
  export const OVERLAP_FRACTION: number

  /**
   * The interpolated value at percentile `p` (0-100) of `values`.
   *
   * Throws on an empty sample, because a percentile of nothing is not zero.
   *
   * @param values observations, any order
   * @param p which percentile, 0-100
   * @returns the interpolated value
   */
  export const percentile: (values: number[], p: number) => number

  /**
   * @param values observations
   * @returns the median
   */
  export const median: (values: number[]) => number

  /**
   * @param values observations
   * @returns stdev, or 0 when there is fewer than one degree of freedom
   */
  export const stdev: (values: number[]) => number

  /**
   * @param values observations
   * @returns cv; 0 when the mean is 0, because a sample of all-zero observations
   * has no spread to report relative to
   */
  export const variation: (values: number[]) => number

  /**
   * @param values observations
   * @returns the full summary of the sample
   */
  export const summary: (values: number[]) => Summary

  /**
   * Compare two samples and refuse to over-claim — the reason this module is
   * pure and tested rather than assumed.
   *
   * @param input the comparison inputs
   * @returns the verdict, the summaries it came from, and the reason string
   */
  export const compare: (input: ComparisonInput) => Comparison
}
