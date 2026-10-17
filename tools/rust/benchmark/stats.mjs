/**
 * The statistics a benchmark is allowed to report, and the verdict rule that
 * turns them into a claim.
 *
 * This module is pure on purpose: every function takes numbers and returns
 * numbers, with no filesystem, no clock and no child process. That is what lets
 * `test/benchmark.test.ts` exercise the verdict rule against hand-built
 * distributions — a rule only ever run on one live measurement is a rule that
 * has never been shown to work.
 *
 * ## Why these functions and not a library
 *
 * `percentile`, `median` and a coefficient of variation are a dozen lines each,
 * and a benchmark harness whose statistics depend on a package that is not
 * otherwise in the dependency tree is a harness whose dependency has to be
 * justified separately. None of these is hard enough to be worth a dependency,
 * and `statistics` is the one part of this file where a bug is invisible: a
 * wrong median still looks like a plausible number, which is exactly why it is
 * tested rather than assumed.
 *
 * ## The rule that matters most here
 *
 * `compare` refuses to call a winner when the two samples overlap. PRD PERF-02
 * says "inconclusive is a valid result" and forbids a benchmark that reports a
 * win from a single measurement. Returning `inconclusive` is the honest answer
 * when the spread is wide, and a harness that cannot say it will always
 * eventually say something wrong.
 */

/**
 * The numeric summary of one sample. Every field is derived from the same input
 * array, so a caller reporting any of them reports all of them — there is no
 * way to quote the median without also having the range.
 *
 * @typedef {object} Summary
 * @property {number} n            how many observations
 * @property {number} min          smallest observation
 * @property {number} max          largest observation
 * @property {number} median       the 50th percentile, interpolated
 * @property {number} mean         arithmetic mean
 * @property {number} stdev        sample standard deviation (n-1); 0 for n < 2
 * @property {number} cv           coefficient of variation, stdev / mean; 0 when
 *                                 the mean is 0
 * @property {number} p25          interpolated 25th percentile
 * @property {number} p75          interpolated 75th percentile
 * @property {number} p95          interpolated 95th percentile
 */

/**
 * Sorted copy of the input, so the caller's array is never reordered.
 *
 * @param {number[]} values the observations
 * @returns {number[]} a new array, ascending
 */
function sorted(values) {
  return [...values].sort((a, b) => a - b)
}

/**
 * The interpolated percentile, the same definition `Array.prototype` consumers
 * expect (R-7, the default in NumPy and in `d3-array`).
 *
 * The interpolation matters for small samples: with 7 repetitions the
 * "nearest rank" definition collapses p25 and median onto the same observation,
 * which throws away the only information a small sample has about its spread.
 *
 * @param {number[]} values observations, any order
 * @param {number} p which percentile, 0-100
 * @returns {number} the interpolated value
 */
export function percentile(values, p) {
  if (values.length === 0) throw new Error('percentile of an empty sample is undefined')
  const xs = sorted(values)
  const rank = (p / 100) * (xs.length - 1)
  const low = Math.floor(rank)
  const high = Math.ceil(rank)
  if (low === high) return xs[low]
  const weight = rank - low
  return xs[low] * (1 - weight) + xs[high] * weight
}

/**
 * @param {number[]} values observations
 * @returns {number} the median
 */
export function median(values) {
  return percentile(values, 50)
}

/**
 * Sample standard deviation with Bessel's correction (n-1), which is the
 * unbiased estimator of the population's spread.
 *
 * @param {number[]} values observations
 * @returns {number} stdev, or 0 when there is fewer than one degree of freedom
 */
export function stdev(values) {
  if (values.length < 2) return 0
  const mean = values.reduce((a, b) => a + b, 0) / values.length
  const squared = values.reduce((acc, v) => acc + (v - mean) ** 2, 0)
  return Math.sqrt(squared / (values.length - 1))
}

/**
 * The coefficient of variation: spread as a fraction of magnitude.
 *
 * This is the number that decides whether a comparison is worth making. Two
 * samples that differ by 5% with a 40% coefficient of variation have not
 * demonstrated anything, and reporting the 5% alone would be a claim the data
 * does not support.
 *
 * @param {number[]} values observations
 * @returns {number} cv; 0 when the mean is 0, because a sample of all-zero
 * observations has no relative spread to report
 */
export function variation(values) {
  const mean = values.reduce((a, b) => a + b, 0) / values.length
  if (mean === 0) return 0
  return stdev(values) / Math.abs(mean)
}

/**
 * @param {number[]} values observations
 * @returns {Summary} the full summary
 */
export function summary(values) {
  if (values.length === 0) throw new Error('summary of an empty sample is undefined')
  const xs = sorted(values)
  const mean = xs.reduce((a, b) => a + b, 0) / xs.length
  return {
    n: xs.length,
    min: xs[0],
    max: xs[xs.length - 1],
    median: percentile(xs, 50),
    mean,
    stdev: stdev(xs),
    cv: variation(xs),
    p25: percentile(xs, 25),
    p75: percentile(xs, 75),
    p95: percentile(xs, 95),
  }
}

/**
 * How far apart two samples must be before a difference between them is
 * treated as resolved rather than as noise.
 *
 * 0.5 means: the *better* sample's worst observation must be at least 50%
 * lower than the *worse* sample's best observation. Chosen to be conservative,
 * not to be strict — a benchmark that declares a winner from overlapping
 * samples is the failure mode this exists to prevent.
 */
export const OVERLAP_FRACTION = 0.5

/**
 * The verdict on one measured comparison.
 *
 * @typedef {'baseline-faster' | 'candidate-faster' | 'within-noise' | 'inconclusive'} Verdict
 */

/**
 * Compare two samples and refuse to over-claim.
 *
 * Three questions, in order, and the order is the logic:
 *
 * 1. Are the ranges separated? Since lower time is better, the samples are
 *    separable only when the better sample's *worst* observation is still
 *    clearly below the worse sample's *best* observation. If they overlap the
 *    answer is `within-noise`, regardless of how the medians look.
 * 2. Does the median difference hold up against the spread? A difference
 *    smaller than the wider sample's coefficient of variation is also noise.
 * 3. Otherwise there is a real difference, and the direction decides who wins.
 *
 * `inconclusive` is returned only when the samples are too small or too
 * degenerate to compare at all (fewer than 2 observations on either side), which
 * is distinct from `within-noise`: "measured and found indistinguishable" is a
 * result, "not measured properly" is not.
 *
 * @param {object} input the comparison inputs
 * @param {number[]} input.baseline the baseline observations (microseconds)
 * @param {number[]} input.candidate the candidate observations (microseconds)
 * @param {number} [input.overlapFraction] override for {@link OVERLAP_FRACTION}
 * @returns {{verdict: Verdict, reason: string, baseline: Summary, candidate: Summary, speedup: number}} the verdict and the summaries it came from
 */
export function compare({ baseline, candidate, overlapFraction = OVERLAP_FRACTION }) {
  if (baseline.length < 2 || candidate.length < 2) {
    return {
      verdict: 'inconclusive',
      reason:
        'fewer than two measured observations on one side, so there is no spread to ' +
        'compare against; PERF-02 requires at least five independent repetitions',
      baseline: baseline.length > 0 ? summary(baseline) : null,
      candidate: candidate.length > 0 ? summary(candidate) : null,
      speedup: NaN,
    }
  }
  const b = summary(baseline)
  const c = summary(candidate)
  const speedup = b.median / c.median
  const faster = speedup > 1 ? 'candidate' : 'baseline'
  const worse = faster === 'candidate' ? b : c
  const better = faster === 'candidate' ? c : b
  // Separation, stated once and in the direction that matters: the better
  // sample's worst observation must still clear the worse sample's best one by
  // more than `overlapFraction`. Written as a single comparison rather than a
  // pair of range checks — an earlier draft spelled it as an `||` of two
  // expressions, one of which compared a number against `false` and was always
  // false, so the test silently reduced to "the better sample's *max* is below
  // the worse sample's *min*" and every overlapping pair was called separated.
  const separated = better.max * (1 + overlapFraction) < worse.min
  if (!separated) {
    // Two genuinely different reasons land here, and saying the wrong one is a
    // false statement about the data sitting in the same file.
    //
    // The ranges *interleave* — `better.max` is above `worse.min`, so some
    // baseline observation could beat some candidate one — or they do not, and
    // the gap is merely too small to satisfy the margin this rule asks for. The
    // first run of this harness on the rates workload hit the second case and
    // reported the first reason: ranges 416.76–620.64 against 646.71–688.71,
    // which are 26µs apart and clearly do not cover each other. The verdict was
    // right and the sentence explaining it was not, which is how an evidence
    // file ends up asserting something its own columns refute.
    const interleaved = better.max >= worse.min
    const reason = interleaved
      ? `the samples overlap: ${faster} is faster at the median (${speedup.toFixed(3)}x) but ` +
        `the ranges (${better.min.toFixed(2)}–${better.max.toFixed(2)} vs ` +
        `${worse.min.toFixed(2)}–${worse.max.toFixed(2)}) cover each other`
      : `the ranges are disjoint but the gap is too small to call: ${faster} is faster at ` +
        `the median (${speedup.toFixed(3)}x), yet ${better.max.toFixed(2)} against ` +
        `${worse.min.toFixed(2)} leaves a margin of only ` +
        `${((1 - better.max / worse.min) * 100).toFixed(2)}% where this rule requires more ` +
        `than ${(overlapFraction * 100).toFixed(0)}%`
    return {
      verdict: 'within-noise',
      reason,
      baseline: b,
      candidate: c,
      speedup,
    }
  }
  // The median difference also has to exceed the combined spread. Two samples
  // can be range-separated and still differ by less than their own noise, which
  // happens when both are tight around a nearly identical value.
  const combinedCv = Math.max(b.cv, c.cv)
  const relativeDifference = Math.abs(1 - 1 / speedup)
  if (relativeDifference < combinedCv) {
    return {
      verdict: 'within-noise',
      reason:
        `the median differs by ${(relativeDifference * 100).toFixed(2)}% but the wider ` +
        `sample's coefficient of variation is ${(combinedCv * 100).toFixed(2)}%`,
      baseline: b,
      candidate: c,
      speedup,
    }
  }
  return {
    verdict: faster === 'candidate' ? 'candidate-faster' : 'baseline-faster',
    reason:
      `${faster} is faster by ${speedup.toFixed(3)}x at the median, with non-overlapping ` +
      `ranges (${better.min.toFixed(2)}–${better.max.toFixed(2)} vs ` +
      `${worse.min.toFixed(2)}–${worse.max.toFixed(2)})`,
    baseline: b,
    candidate: c,
    speedup,
  }
}
