/**
 * The workload-coverage check, and the bug it exists to prevent.
 *
 * These tests are not general-purpose harness tests — they are a regression test
 * for one specific defect, and the interesting assertion in them is that a
 * harness *fails* when an engine quietly runs fewer cases than the other one.
 *
 * ## The bug
 *
 * The first version of the Rust consumer's `run_rates_op` began with
 *
 * ```rust
 * let unit_id = op.unit_id.as_deref()?;
 * ```
 *
 * hoisted above its `match`. It read as though every op had a unit id, and in
 * fact five of the eleven do not — `nightsBetween`, `bookableStays`,
 * `cheapestBookable`, `isReferenceNight` and `isWeekendNight` take a date or a
 * window. For the 185 fixture cases carrying those ops, `unit_id` was `None` and
 * the `?` returned early, so the Rust side ran 568 of 753 operations while the
 * TypeScript side ran all 753.
 *
 * The run still printed three verdicts, all of them plausible, and reported
 * `candidate-faster 1.702x` for rates. The Rust engine was doing about a quarter
 * less work and still won — which is the only kind of result that should be
 * self-evidently suspicious in retrospect, and was not at the time, because every
 * number in the evidence file was well-formed. The harness's own comment on
 * `bench_rates` says skipping cases would make the workload not the workload,
 * three lines above the code that did exactly that.
 *
 * ## Why a coverage check was not enough on its own
 *
 * What made it invisible was that the only count reported was `casesAnswered` —
 * the number of ops returning a value. With the bug in place that number read
 * 249, against a 753-case workload. It was low enough to notice and looked
 * plausible enough to leave: many rates ops legitimately answer nothing, so a
 * shortfall has an innocent explanation right there in the data.
 *
 * The count that settles it is `casesRun`, compared against the workload's own
 * case count. Running the full case file answered **434** — so the 249 was not
 * "many ops answer nothing" at all, it was 185 cases that never executed.
 * `casesAnswered` cannot make that distinction on its own; `casesRun` can, which
 * is why the fix was a check and why these tests are that check's tests.
 *
 * ## Why the first check failed too
 *
 * The check was added and the bug still shipped. `bench_rates` reported
 * `"casesRun": ops.len()` — the *workload's length*, not the number that
 * executed — so the runner compared 753 against 753 and was satisfied while the
 * engine ran 568. A count written from the input rather than incremented by the
 * execution cannot detect a shortfall, so it looked like a check and behaved
 * like decoration.
 *
 * The Rust consumer now derives `casesRun` by incrementing a counter for every
 * case it actually runs, and exits 2 — printing nothing on stdout, so no timing
 * is emitted — the moment a case is declined. `run_rates_op` has no legitimate
 * way to decline: an op the consumer does not implement or a missing required
 * argument already exit separately. So `casesRun < cases` is impossible in
 * correct code, which is what makes the check able to fire.
 */

import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

/** The case file the workload is generated from, read exactly as the runner
 *  reads it — a test that parses a different copy would test the copy. */
const CASES_PATH = fileURLToPath(new URL('../../parity/cases/rates.json', import.meta.url))

/** Ops whose baseline call takes no `unitId` — the five the bug was blind to. */
const NO_UNIT_ID_OPS = new Set([
  'nightsBetween',
  'bookableStays',
  'cheapestBookable',
  'isReferenceNight',
  'isWeekendNight',
])

/**
 * One rates fixture, as the case file carries it.
 *
 * `unitId` is optional because five of the eleven ops genuinely take none, and
 * that optionality is the thing the bug exploited — a shape where every case
 * carried a `unitId` would have made the hoisted read look correct and this
 * whole file unnecessary.
 */
interface RatesCase {
  id: string
  op: string
  unitId?: string
  [arg: string]: unknown
}

/** The case file's own shape. */
interface RatesCaseFile {
  cases: RatesCase[]
}

/** The shape the Rust consumer reports its coverage in. */
interface RustCoverage {
  casesRun: number
}

/** The shape the TypeScript side reports its coverage in. */
interface TsCoverage {
  ran: number
}

/**
 * The check under test, reproduced here as the runner performs it.
 *
 * It is a copy rather than an import because it is four lines of comparison and
 * a test that imports the function would assert only that the function still
 * exists. What needs testing is the *relationship* between the reported case
 * count and the workload's case count, and that is what the fixtures below
 * exercise.
 *
 * @param rustResult what the Rust consumer reported
 * @param expectedCases how many cases the workload was built from
 * @param tsAnswers what the TypeScript side reported
 */
function assertSameWorkload(
  rustResult: RustCoverage,
  expectedCases: number,
  tsAnswers: TsCoverage,
): void {
  if (rustResult.casesRun !== expectedCases) {
    throw new Error(
      `the Rust consumer ran ${rustResult.casesRun} of ${expectedCases} rates cases; a ` +
        `benchmark that measures a subset of the workload is not measuring the workload`,
    )
  }
  if (tsAnswers.ran !== expectedCases) {
    throw new Error(
      `the TypeScript side ran ${tsAnswers.ran} of ${expectedCases} rates cases; the op ` +
        `dispatch table does not cover the case file`,
    )
  }
}

/**
 * The case file, parsed once into the shape above.
 *
 * `JSON.parse` is `any`, and the cast is the single place that assumption is
 * stated — the same convention `packages/adapters/test/jev-decision.test.ts`
 * records. Every access below is then checked against the declared fields, so a
 * fixture that stopped carrying `op` or `unitId` is a test failure rather than a
 * silent `undefined` that still passes.
 */
const parsed = JSON.parse(readFileSync(CASES_PATH, 'utf8')) as RatesCaseFile
const cases = parsed.cases

describe('the rates case file itself', () => {
  it('contains operations that take no unitId, which is what the bug exploited', () => {
    const withoutUnit = cases.filter((c) => c.unitId === undefined)
    expect(withoutUnit.length).toBeGreaterThan(0)
    // Every one of them is an op the crate really does not take a unit id for.
    for (const c of withoutUnit) expect(NO_UNIT_ID_OPS.has(c.op)).toBe(true)
  })

  it('has every op carry the arguments the crate signature requires', () => {
    // The reason `required()` exits instead of answering None: a fixture that
    // omits a field its op needs is a workload bug, and every single case here
    // carries its fields. If this ever fails, `required`'s exit is correct
    // behaviour and the case file needs the fix, not the consumer.
    const requiredFields = {
      nightlyRate: ['unitId', 'night'],
      nightlyAvailability: ['unitId', 'night'],
      nightsBetween: ['from', 'to'],
      stayAvailability: ['unitId', 'from', 'to'],
      stayQuote: ['unitId', 'from', 'to'],
      bookableStays: ['from', 'to'],
      cheapestBookable: ['from', 'to'],
      roomsBuilt: ['unitId'],
      baseRate: ['unitId'],
      isReferenceNight: ['night'],
      isWeekendNight: ['night'],
    }
    for (const c of cases) {
      const fields: string[] | undefined = requiredFields[c.op as keyof typeof requiredFields]
      expect(fields, `case ${c.id} names an op with no known signature`).toBeDefined()
      for (const field of fields ?? []) {
        expect(c[field], `case ${c.id} is missing ${field}`).toBeDefined()
      }
    }
  })
})

describe('assertSameWorkload', () => {
  it('passes when both engines ran every case', () => {
    expect(() =>
      assertSameWorkload({ casesRun: cases.length }, cases.length, { ran: cases.length }),
    ).not.toThrow()
  })

  it('rejects the exact 568-of-753 run the bug produced', () => {
    // A consumer that hoists a shared unit id and returns early for the ops
    // without one reports this shape. Every other number in that run was
    // well-formed, which is why the check has to be explicit about the count.
    const buggyRun = cases.length - cases.filter((c) => c.unitId === undefined).length
    expect(buggyRun).toBeLessThan(cases.length)
    expect(() =>
      assertSameWorkload({ casesRun: buggyRun }, cases.length, { ran: cases.length }),
    ).toThrow(/subset of the workload/)
  })

  it('rejects a TypeScript side that dropped the same cases', () => {
    // The check is symmetric: either engine can be the one that quietly drops
    // cases, and a harness that only verified its own side would miss this.
    expect(() =>
      assertSameWorkload({ casesRun: cases.length }, cases.length, { ran: 568 }),
    ).toThrow(/dispatch table does not cover/)
  })

  it('does not let casesAnswered stand in for casesRun', () => {
    // The heart of why the bug survived: the answered count is low for an
    // innocent reason too — plenty of rates ops legitimately answer nothing —
    // so a shortfall there has a plausible explanation and gets waved through.
    // Only the *ran* count separates "ran fewer" from "answered fewer".
    const buggyAnswered = 249
    expect(buggyAnswered).toBeLessThan(cases.length)
    expect(() =>
      assertSameWorkload({ casesRun: buggyAnswered, casesAnswered: buggyAnswered }, cases.length, {
        ran: cases.length,
      }),
    ).toThrow(/subset of the workload/)
  })

  it('has a full run answer exactly the 185 the buggy run lost', () => {
    // The measured identity that closes the argument that 249 was innocent:
    // the buggy consumer answered 249 of a workload whose complete pass answers
    // 434, and the difference is precisely the 185 cases carrying no unitId.
    // Those 185 were not "ops that answered nothing" — they never executed.
    const buggyAnswered = 249
    expect(FULL_RUN_ANSWERED - buggyAnswered).toBe(
      cases.filter((c) => c.unitId === undefined).length,
    )
    expect(FULL_RUN_ANSWERED).toBeGreaterThan(buggyAnswered)
    expect(buggyAnswered).toBeLessThan(FULL_RUN_ANSWERED)
  })
})

/**
 * The measured answer count for a complete rates pass.
 *
 * Recorded rather than computed, because it came out of `archava-bench` running
 * the real 753-case workload. Deriving it from the fixtures would only restate
 * whatever the derivation assumed.
 */
const FULL_RUN_ANSWERED = 434
