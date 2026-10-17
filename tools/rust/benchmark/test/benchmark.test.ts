/**
 * The verdict rule under test.
 *
 * `stats.mjs` is the one module in the benchmark whose output cannot be checked
 * against a reference implementation, because there is nothing to compare a
 * *verdict* to — it is a judgement this harness makes. So it is tested against
 * distributions built by hand to sit exactly on the boundaries the rule claims
 * to respect, and the interesting cases are the ones where the medians and the
 * verdict point in different directions.
 *
 * Three of these tests exist because a bug was in them:
 *
 * - The overlap test was written as an `||` of two expressions, one of which
 *   compared a number against `false` and was therefore always false. That
 *   reduced the test to a single range comparison and made every overlapping
 *   pair look separated, so `within-noise` was unreachable. The fix is one
 *   expression, and `refuses to separate two samples whose ranges interleave`
 *   below is the test that would have caught it.
 * - `variation([0, 0, 0])` returns 0 rather than NaN, so a degenerate sample
 *   does not poison the cv comparison downstream.
 * - `compare` on a single observation per side must not crash, and must still
 *   report a summary for the side it did measure.
 */

import { describe, expect, it } from 'vitest'
import {
  compare,
  median,
  OVERLAP_FRACTION,
  percentile,
  stdev,
  summary,
  variation,
} from '../stats.mjs'

describe('percentile and median', () => {
  it('interpolates rather than rounding to a rank', () => {
    // 4 observations with rank (0.25 * 3) = 0.75, so p25 sits 3/4 of the way
    // from 10 to 20. A nearest-rank implementation would answer 10 and lose the
    // only spread information a 4-observation sample has.
    expect(percentile([10, 20, 30, 40], 25)).toBe(17.5)
    expect(percentile([10, 20, 30, 40], 50)).toBe(25)
    expect(percentile([10, 20, 30, 40], 75)).toBe(32.5)
  })

  it('returns an exact observation when the rank lands on one', () => {
    expect(percentile([1, 2, 3], 50)).toBe(2)
    expect(percentile([5], 95)).toBe(5)
  })

  it('does not reorder the caller array', () => {
    const values = [30, 10, 20]
    median(values)
    expect(values).toEqual([30, 10, 20])
  })

  it('refuses an empty sample instead of returning NaN', () => {
    expect(() => percentile([], 50)).toThrow(/empty sample/)
  })
})

describe('stdev and variation', () => {
  it('uses the sample stdev, not the population one', () => {
    // Population stdev of [1,2,3,4] is ~1.118; sample stdev is ~1.291.
    expect(stdev([1, 2, 3, 4])).toBeCloseTo(1.291, 3)
  })

  it('is 0 for a single observation', () => {
    expect(stdev([42])).toBe(0)
  })

  it('reports relative spread, not absolute', () => {
    // Both samples have the *same* absolute stdev of 10, and very different
    // relative spread: a 10-microsecond wobble around 100 is nothing, the same
    // wobble around 20 is half the measurement. Absolute spread would call these
    // equal, which is what makes a percentile-vs-mean claim dishonest.
    expect(stdev([90, 100, 110])).toBeCloseTo(10, 6)
    expect(stdev([10, 20, 30])).toBeCloseTo(10, 6)
    expect(variation([90, 100, 110])).toBeCloseTo(0.1, 6)
    expect(variation([10, 20, 30])).toBeCloseTo(0.5, 6)
  })

  it('is 0 and not NaN for a sample of all zeros', () => {
    expect(variation([0, 0, 0])).toBe(0)
  })
})

describe('summary', () => {
  it('reports every field from the same observations', () => {
    const result = summary([5, 1, 4, 2, 3])
    expect(result).toMatchObject({
      n: 5,
      min: 1,
      max: 5,
      median: 3,
      mean: 3,
    })
    expect(result.p25).toBeLessThanOrEqual(result.median)
    expect(result.median).toBeLessThanOrEqual(result.p75)
    expect(result.p75).toBeLessThanOrEqual(result.p95)
  })
})

describe('compare', () => {
  it('calls a candidate win only when the ranges are separated', () => {
    const result = compare({ baseline: [100, 102, 104], candidate: [10, 11, 12] })
    expect(result.verdict).toBe('candidate-faster')
    expect(result.speedup).toBeGreaterThan(1)
  })

  it('calls a baseline win in the other direction', () => {
    const result = compare({ baseline: [10, 11, 12], candidate: [100, 102, 104] })
    expect(result.verdict).toBe('baseline-faster')
  })

  it('refuses to separate two samples whose ranges interleave', () => {
    // Medians 13 vs 12 look like a candidate win, but the ranges cover each
    // other: any baseline draw could be slower than any candidate draw.
    const result = compare({ baseline: [10, 12, 14, 16], candidate: [6, 10, 14, 18] })
    expect(result.verdict).toBe('within-noise')
  })

  it('says the ranges cover each other only when they actually do', () => {
    // The other half of the refusing rule, and a defect found in the harness
    // rather than in a fixture: the reason string was written as if every
    // non-separated pair interleaved. A pair whose ranges are disjoint but
    // too close to satisfy the margin gets within-noise too, and describing
    // that as "the ranges cover each other" is a false statement about the
    // columns printed beside it.
    const disjoint = compare({
      baseline: [416.76, 552.45, 558.72, 620.64],
      candidate: [646.71, 652.12],
    })
    expect(disjoint.verdict).toBe('within-noise')
    expect(disjoint.reason).not.toMatch(/cover each other/)
    expect(disjoint.reason).toMatch(/disjoint/)
    // And the interleaved case must still say what it does.
    const interleaved = compare({ baseline: [10, 12, 14, 16], candidate: [6, 10, 14, 18] })
    expect(interleaved.reason).toMatch(/cover each other/)
  })

  it('reports within-noise when the median difference is smaller than the spread', () => {
    // Tight, non-overlapping, and yet only ~1% apart while each sample varies by
    // about 1.5% of its own median. A difference inside the noise floor is noise,
    // and this is the case where the range test passes and the cv test catches it.
    const result = compare({ baseline: [1000, 1010, 1020], candidate: [990, 995, 1000] })
    expect(result.verdict).toBe('within-noise')
  })

  it('is inconclusive below two observations, which is not the same as noise', () => {
    const single = compare({ baseline: [100], candidate: [1] })
    expect(single.verdict).toBe('inconclusive')
    // The distinctive property: the side that was measured still gets a summary,
    // so the evidence file says what was measured rather than showing a null.
    expect(single.baseline).not.toBeNull()
    expect(single.candidate).not.toBeNull()
    expect(compare({ baseline: [], candidate: [1, 2] }).verdict).toBe('inconclusive')
  })

  it('treats equal samples as within-noise, not as a candidate win', () => {
    expect(compare({ baseline: [5, 5, 5], candidate: [5, 5, 5] }).verdict).toBe('within-noise')
  })

  it('honours an explicit overlap fraction', () => {
    // A pair whose ranges interleave is called separated when the caller asks for
    // no gap at all, and rejected when it asks for the default fractional gap.
    const baseline = [100, 101]
    const candidate = [60, 71]
    expect(compare({ baseline, candidate, overlapFraction: 0 }).verdict).toBe('candidate-faster')
    expect(compare({ baseline, candidate, overlapFraction: 0.5 }).verdict).toBe('within-noise')
  })

  it('exposes the fraction it used, so a caller cannot have to guess it', () => {
    expect(OVERLAP_FRACTION).toBe(0.5)
  })
})
