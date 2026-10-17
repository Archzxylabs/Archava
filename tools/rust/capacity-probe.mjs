/**
 * The p3 reference generator for the capacity port.
 *
 * `packages/capacity-pg/src/repository.ts` has no unit test covering
 * `calculateSchedule`. The only test file in either capacity package
 * (`packages/capacity/test/boundaries.test.ts`) exercises `CapacityService` with
 * a stub repository whose `estimate` returns a hard-coded object — so nothing
 * anywhere pins what `calculateSchedule` actually returns.
 *
 * That leaves exactly one way to know: run it. This file transcribes the three
 * functions verbatim from the baseline and runs them over a fixed fixture set,
 * writing the results to `rust/crates/capacity/tests/reference.json`. The Rust
 * differential suite then asserts the port reproduces *those numbers*, so the
 * suite checks TypeScript's actual behaviour rather than a reading of the code by
 * whoever wrote the port.
 *
 * ## Why the transcription is verbatim and not an import
 *
 * `calculateSchedule` is not exported. `repository.ts:117` declares it as a
 * module-private function, so the only ways to reach it are to copy it or to
 * drive the whole repository through PostgreSQL. Copying is what this file does,
 * and the copy is kept line-for-line identical to the baseline — including its
 * naming, its `let`s and its `for` loops — so a diff against the baseline is a
 * diff of *this file's own scaffolding* and nothing else. An import would need
 * the estimation call path, a pool row and a live database, which is a fixture
 * this generator must not have.
 *
 * ## What the fixture set covers
 *
 * 37 cases, each named after the rule it pins rather than numbered:
 *
 *  - **Calendar edges** — Saturday and Sunday desired starts (the
 *    `nextBusinessDay` skip), a Friday start with a one-day and a two-day job
 *    (the no-op-on-weekdays property), a Friday allocation spanning a weekend,
 *    and three ISO forms a real caller sends: a `+07:00` offset, a `-05:00`
 *    offset, and a string with no milliseconds.
 *  - **The burn floor** — 39, 40, 41, 800 and 2000 units against a 200-hour
 *    week, so `estimatedCalendarDays` is checked where it rounds down, exact,
 *    up, and far up; plus a `weeklyCapacity: 1` pool where a one-hour job takes
 *    a week.
 *  - **The three `Math.max(1, ...)` floors** — a `requiredUnits: 0` job, which
 *    makes `workingDaysNeeded` 1 rather than 0 and `neededDailyBurn` 0.
 *  - **Concurrency** — `maxConcurrentJobs` of 0, 1, 3 and 100, and a pool where
 *    capacity is the binding limit rather than concurrency (79 units already
 *    allocated, so a 40-unit job fits where 80 would not).
 *  - **The fallbacks** — `remainingUnits` of 0 and of -5 both falling back to
 *    `requiredUnits`.
 *  - **The epsilon** — the case where the `+1e-6` tolerance at `:189` changes
 *    the answer, and its control, where it does not. Without both, a reader
 *    cannot tell whether the tolerance is load-bearing or decorative.
 *  - **Permanence** — an allocation running to 2040, which tests whether the
 *    search jumps to the blocking finish or creeps forward day by day.
 *
 * ## Two things this generator refuses to do
 *
 * It does not assert anything. A generator that both produced and checked its
 * own numbers would be comparing the port against the port's own assumptions.
 * The checking is `rust/crates/capacity/tests/capacity.rs`; this file's whole job
 * is to make the baseline's answers visible.
 *
 * It does not hide the wall clock. `no-desired-start` passes no `desiredStartAt`,
 * so the baseline uses `new Date()` and the output moves with the clock. The case
 * is still recorded — a reader can see what the shape is — and the Rust suite
 * skips it by name, naming the reason, rather than asserting against a value that
 * cannot hold.
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Node globals this generator touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here.
/* global URL, process */

// ---------------------------------------------------------------------------
// The baseline, transcribed verbatim from packages/capacity-pg/src/repository.ts
// ---------------------------------------------------------------------------

/** `repository.ts:96-106`. */
function countWorkingDays(start, finish) {
  let count = 0
  let cur = new Date(start.getTime())
  while (cur.getTime() < finish.getTime()) {
    if (cur.getUTCDay() !== 0 && cur.getUTCDay() !== 6) {
      count++
    }
    cur = new Date(cur.getTime() + 24 * 60 * 60 * 1000)
  }
  return Math.max(1, count)
}

/** `repository.ts:108-115`. */
function nextBusinessDay(date) {
  let cur = new Date(date.getTime())
  while (cur.getUTCDay() === 0 || cur.getUTCDay() === 6) {
    cur = new Date(cur.getTime() + 24 * 60 * 60 * 1000)
    cur.setUTCHours(0, 0, 0, 0)
  }
  return cur
}

/** `packages/capacity/src/contracts.ts:3-19`. */
class CapacityError extends Error {
  constructor(code, message) {
    super(message ?? code)
    this.name = 'CapacityError'
    this.code = code
  }
}

/** `repository.ts:117-231`. */
function calculateSchedule(pool, activeAllocations, requiredUnits, desiredStartAt) {
  const dailyBurnRate = pool.weeklyCapacity / 5
  if (dailyBurnRate <= 0) throw new CapacityError('insufficient_capacity')
  const now = new Date()
  let candidateStart = nextBusinessDay(desiredStartAt ? new Date(desiredStartAt) : now)
  const initialDesiredTime = candidateStart.getTime()
  const workingDaysNeeded = Math.max(1, Math.ceil(requiredUnits / dailyBurnRate))
  const neededDailyBurn = Math.min(dailyBurnRate, requiredUnits / workingDaysNeeded)
  const allocDetails = activeAllocations.map((alloc) => {
    const aStart = new Date(alloc.scheduledStartAt)
    const aFinish = new Date(alloc.scheduledFinishAt)
    const wDays = countWorkingDays(aStart, aFinish)
    const dailyUnits = Math.min(
      dailyBurnRate,
      (alloc.remainingUnits > 0 ? alloc.remainingUnits : alloc.requiredUnits) / wDays,
    )
    return { id: alloc.id, start: aStart.getTime(), finish: aFinish.getTime(), dailyUnits }
  })
  let loopCount = 0
  let candidateFinish = new Date(candidateStart.getTime())
  let found = false
  while (loopCount < 1000) {
    loopCount++
    candidateStart = nextBusinessDay(candidateStart)
    const proposedWorkingDays = []
    let cursor = new Date(candidateStart.getTime())
    while (proposedWorkingDays.length < workingDaysNeeded) {
      if (cursor.getUTCDay() !== 0 && cursor.getUTCDay() !== 6)
        proposedWorkingDays.push(new Date(cursor.getTime()))
      cursor = new Date(cursor.getTime() + 24 * 60 * 60 * 1000)
    }
    candidateFinish = nextBusinessDay(cursor)
    let valid = true
    let earliestBlockingFinish = Infinity
    for (const day of proposedWorkingDays) {
      const dayStart = day.getTime()
      const dayEnd = dayStart + 24 * 60 * 60 * 1000
      let concurrentJobsToday = 0
      let usedCapacityToday = 0
      for (const a of allocDetails) {
        if (a.start < dayEnd && a.finish > dayStart) {
          concurrentJobsToday++
          usedCapacityToday += a.dailyUnits
          if (a.finish > dayStart && a.finish < earliestBlockingFinish)
            earliestBlockingFinish = a.finish
        }
      }
      if (
        concurrentJobsToday + 1 > pool.maxConcurrentJobs ||
        usedCapacityToday + neededDailyBurn > dailyBurnRate + 1e-6
      ) {
        valid = false
        break
      }
    }
    if (valid) {
      found = true
      break
    }
    const nextStart =
      earliestBlockingFinish !== Infinity && earliestBlockingFinish > candidateStart.getTime()
        ? new Date(earliestBlockingFinish)
        : new Date(candidateStart.getTime() + 24 * 60 * 60 * 1000)
    candidateStart = nextBusinessDay(nextStart)
  }
  if (!found) throw new CapacityError('insufficient_capacity')
  const elapsedCalendarDays = Math.max(
    1,
    Math.round((candidateFinish.getTime() - candidateStart.getTime()) / (24 * 60 * 60 * 1000)),
  )
  let queuePosition = 0
  if (candidateStart.getTime() > initialDesiredTime) {
    const blockingAllocs = allocDetails.filter((a) => a.finish > initialDesiredTime)
    queuePosition = Math.max(1, blockingAllocs.length)
  }
  return {
    queuePosition,
    scheduledStartAt: candidateStart.toISOString(),
    scheduledFinishAt: candidateFinish.toISOString(),
    burnRatePerDay: dailyBurnRate,
    estimatedCalendarDays: elapsedCalendarDays,
    workingDays: workingDaysNeeded,
  }
}

// ---------------------------------------------------------------------------
// The fixture set
// ---------------------------------------------------------------------------

// Fixed dates in the 2030-01 week, so every case is weekday-stable regardless of
// when the generator runs. 2030-01-07 is a Monday.
const MON = '2030-01-07T00:00:00.000Z' // Monday
const WED = '2030-01-09T00:00:00.000Z'
const FRI = '2030-01-11T00:00:00.000Z' // Friday
const SAT = '2030-01-12T00:00:00.000Z' // Saturday
const SUN = '2030-01-13T00:00:00.000Z' // Sunday
const NEXT_MON = '2030-01-14T00:00:00.000Z'

/** An allocation row as the baseline's caller reads it out of PostgreSQL. */
const alloc = (id, start, finish, required, remaining) => ({
  id,
  scheduledStartAt: start,
  scheduledFinishAt: finish,
  requiredUnits: required,
  remainingUnits: remaining,
})

const cases = [
  // -- Calendar edges ------------------------------------------------------
  {
    name: 'idle-pool-80h-desired-monday',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 80,
    desired: MON,
  },
  {
    name: 'idle-pool-40h-desired-monday',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: MON,
  },
  {
    name: 'desired-saturday',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: SAT,
  },
  {
    name: 'desired-sunday',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: SUN,
  },
  {
    name: 'desired-friday-40h',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: FRI,
  },
  {
    name: 'desired-friday-80h',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 80,
    desired: FRI,
  },
  {
    name: 'weekend-spanning-allocation',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [alloc('wk', FRI, NEXT_MON, 40, 40)],
    units: 40,
    desired: FRI,
  },
  {
    name: 'allocation-in-the-past',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [alloc('old', '2020-01-06T00:00:00.000Z', '2020-01-08T00:00:00.000Z', 80, 80)],
    units: 40,
    desired: MON,
  },
  {
    name: 'two-sequential-allocations',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [alloc('a1', MON, WED, 80, 80), alloc('a2', WED, FRI, 80, 80)],
    units: 40,
    desired: MON,
  },

  // -- ISO forms a real caller sends ---------------------------------------
  {
    name: 'offset-tz-desired',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: '2030-01-07T07:00:00+07:00',
  },
  {
    name: 'offset-tz-desired-no-frac',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: '2030-01-11T07:00:00+07:00',
  },
  {
    name: 'offset-tz-negative',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: '2030-01-07T00:00:00-05:00',
  },
  {
    name: 'no-milliseconds',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: '2030-01-07T00:00:00Z',
  },
  {
    name: 'timestamp-with-time-of-day',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: '2030-01-08T13:45:30.500Z',
  },

  // -- The burn floor and the day estimates --------------------------------
  {
    name: 'empty-pool-1h',
    pool: { weeklyCapacity: 5, maxConcurrentJobs: 3 },
    allocs: [],
    units: 1,
    desired: MON,
  },
  {
    name: 'fractional-burn-remainder',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 41,
    desired: MON,
  },
  {
    name: 'units-below-one-day',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 39,
    desired: MON,
  },
  {
    name: 'huge-job-800h',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 800,
    desired: MON,
  },
  {
    name: 'longest-horizon-2000h',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 2000,
    desired: MON,
  },
  {
    name: 'tiny-weekly-capacity',
    pool: { weeklyCapacity: 1, maxConcurrentJobs: 1 },
    allocs: [],
    units: 1,
    desired: MON,
  },
  {
    name: 'zero-units',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 0,
    desired: MON,
  },

  // -- Concurrency, and the capacity limit that is not concurrency --------
  {
    name: 'saturated-concurrency-1',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [alloc('a1', MON, WED, 80, 80)],
    units: 40,
    desired: MON,
  },
  {
    name: 'saturated-concurrency-3',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 3 },
    allocs: [
      alloc('a1', MON, WED, 80, 80),
      alloc('a2', MON, WED, 80, 80),
      alloc('a3', MON, WED, 80, 80),
    ],
    units: 40,
    desired: MON,
  },
  {
    name: 'concurrency-3-two-active',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 3 },
    allocs: [alloc('a1', MON, WED, 80, 80), alloc('a2', MON, WED, 80, 80)],
    units: 40,
    desired: MON,
  },
  {
    name: 'capacity-tight-not-concurrency',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 5 },
    allocs: [alloc('a1', MON, WED, 79, 79)],
    units: 40,
    desired: MON,
  },
  {
    name: 'capacity-exactly-fits',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 5 },
    allocs: [alloc('a1', MON, WED, 80, 80)],
    units: 40,
    desired: MON,
  },
  {
    name: 'max-concurrent-100',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 100 },
    allocs: [],
    units: 40,
    desired: MON,
  },
  {
    name: 'unsatisfiable-concurrency',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 0 },
    allocs: [],
    units: 40,
    desired: MON,
  },

  // -- The remainingUnits fallbacks ----------------------------------------
  {
    name: 'remaining-zero-falls-back-to-required',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 5 },
    allocs: [alloc('a1', MON, WED, 40, 0)],
    units: 40,
    desired: MON,
  },
  {
    name: 'remaining-negative-falls-back',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 5 },
    allocs: [alloc('a1', MON, WED, 40, -5)],
    units: 40,
    desired: MON,
  },

  // -- The +1e-6 capacity tolerance, and its control ----------------------
  // burn = 3/5 = 0.6, the job needs 2 of the working days, and the pre-existing
  // allocation burns 1/5 = 0.2 per day. In exact arithmetic 0.2 + 0.4 == 0.6 is a
  // *fit*; in f64 it is 0.6000000000000001, one ULP over. The tolerance is what
  // makes Monday valid, so this case fails on a port that omits the epsilon.
  {
    name: 'epsilon-admits-one-ulp-over-capacity',
    pool: { weeklyCapacity: 3, maxConcurrentJobs: 5 },
    allocs: [alloc('ulp', MON, '2030-01-12T00:00:00.000Z', 1, 1)],
    units: 0.8,
    desired: MON,
  },
  // Control: a real 40-unit overrun is NOT admitted, so the tolerance is a
  // tolerance and not a pass-through. Both are needed or the reader cannot tell
  // which kind of case the epsilon is.
  {
    name: 'epsilon-does-not-admit-a-real-overrun',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 5 },
    allocs: [alloc('over', MON, WED, 80, 80)],
    units: 40,
    desired: MON,
  },

  // -- Permanence ---------------------------------------------------------
  {
    name: 'permanent-saturation',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [alloc('forever', MON, '2040-01-01T00:00:00.000Z', 80, 80)],
    units: 40,
    desired: MON,
  },

  // -- The entry guard, and the floor that keeps a zero-length span total ---
  //
  // `dailyBurnRate <= 0` at repository.ts:124 is the earliest refusal in the
  // function, reached before the allocation projection is built and therefore
  // independent of every allocation and unit count below. It has no fixture in
  // the groups above, because the only pool there with a small weekly capacity
  // is `tiny-weekly-capacity` (1), which is the *positive* boundary — a
  // one-hour job takes a week and is scheduled. 0 is the refusal boundary
  // itself, and -50 reaches the same branch through the `< 0` half rather than
  // the `=== 0` half.
  //
  // Neither is a schema-legal row: `capacityPoolSchema` declares
  // `weeklyCapacity: z.number().positive().max(1000000)`. Both are here anyway,
  // for the reason the baseline itself demonstrates — it validates nothing and
  // computes whatever it is handed, so a port that modelled the burn rate as an
  // unsigned quantity would make -50 unrepresentable rather than refused.
  {
    name: 'burn-rate-zero-refuses',
    pool: { weeklyCapacity: 0, maxConcurrentJobs: 5 },
    allocs: [],
    units: 40,
    desired: MON,
  },
  {
    name: 'burn-rate-negative-refuses',
    pool: { weeklyCapacity: -50, maxConcurrentJobs: 5 },
    allocs: [],
    units: 40,
    desired: MON,
  },

  // A zero-length allocation span, which drives `countWorkingDays` to its
  // `Math.max(1, count)` floor: start and finish are the same Monday, the loop
  // body never runs, `count` is 0, and the floor makes it 1. That is what keeps
  // `dailyUnits` — `Math.min(dailyBurnRate, units / wDays)` — a division by one
  // instead of a division by zero producing `Infinity`, which would then make
  // every following day read as over capacity.
  //
  // Not reachable by asserting a schema invariant: `instantSchema` does not
  // order the two timestamps, and nothing in the baseline checks. The fixture
  // exists so a port that dropped the floor fails instead of producing `Inf`.
  {
    name: 'zero-length-allocation-burns-one-day',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [alloc('instant', MON, MON, 40, 40)],
    units: 40,
    desired: MON,
  },

  // -- The one wall-clock case, last because it cannot be compared --------
  {
    name: 'no-desired-start',
    pool: { weeklyCapacity: 200, maxConcurrentJobs: 1 },
    allocs: [],
    units: 40,
    desired: undefined,
  },
]

// ---------------------------------------------------------------------------
// The run
// ---------------------------------------------------------------------------

const out = []
for (const c of cases) {
  // The recorded input drops the `undefined`, so the JSON is valid on disk.
  // `no-desired-start` records `desired: null` and the reader knows to pass
  // nothing.
  const entry = {
    name: c.name,
    pool: c.pool,
    allocs: c.allocs,
    units: c.units,
    desired: c.desired ?? null,
  }
  try {
    const r = calculateSchedule(c.pool, c.allocs, c.units, c.desired)
    entry.ok = {
      queuePosition: r.queuePosition,
      scheduledStartAt: r.scheduledStartAt,
      scheduledFinishAt: r.scheduledFinishAt,
      burnRatePerDay: r.burnRatePerDay,
      estimatedCalendarDays: r.estimatedCalendarDays,
      workingDays: r.workingDays,
    }
  } catch (e) {
    entry.err = e instanceof CapacityError ? e.code : `unexpected:${e.constructor.name}`
  }
  out.push(entry)
}

const payload = {
  slice: 'p3',
  generatedBy: 'node tools/rust/capacity-probe.mjs',
  source: 'packages/capacity-pg/src/repository.ts:117-231',
  count: out.length,
  cases: out,
}

const OUT = fileURLToPath(
  new URL('../../rust/crates/capacity/tests/reference.json', import.meta.url),
)
writeFileSync(OUT, `${JSON.stringify(payload, null, 2)}\n`)

// The counts are printed rather than only written, so the run that produced the
// file is legible from the terminal without opening it.
const errors = out.filter((e) => e.err)
const ok = out.length - errors.length
process.stdout.write(`${String(ok).padStart(5)}  scheduled\n`)
process.stdout.write(`${String(errors.length).padStart(5)}  insufficient_capacity\n`)
process.stdout.write(`${String(out.length).padStart(5)}  total\n`)
process.stdout.write(`wrote ${OUT}\n`)

// The generated file is re-read and checked, rather than trusting the write. A
// generator that reports success without confirming what it left on disk can
// truncate silently, leaving a file whose declared count matching nothing.
const written = JSON.parse(readFileSync(OUT, 'utf8'))
if (written.count !== written.cases.length) {
  throw new Error(
    `generator wrote an inconsistent file: ${written.count} vs ${written.cases.length}`,
  )
}
for (const [index, item] of written.cases.entries()) {
  if (typeof item.name !== 'string' || item.name.length === 0) {
    throw new Error(`case ${index} has no name`)
  }
  if (!item.ok && !item.err) {
    throw new Error(`case ${item.name} has neither a result nor an error`)
  }
  if (item.ok && item.err) {
    throw new Error(`case ${item.name} has both a result and an error`)
  }
}
