/**
 * The p2 fixture generator.
 *
 * Cases are synthetic and deterministic: every date comes from the fixture
 * dates the baseline itself states, every unit id is either one of the four the
 * reference tenant owns or a deliberately invented one, and every window end is
 * either a night or the checkout. No owner data, no environment file, nothing
 * read from disk beyond the baseline's own exported tables.
 *
 * ## What the set has to cover, and why this shape
 *
 * The p1 generator could lean on an exhaustive cross-product, because the
 * pricebook's 168 combinations were small enough to enumerate. The rates slice
 * is different in the way that matters: its input space is *dates*, and dates
 * form a window. The interesting behaviour is not the middle of that window but
 * its edges and the places just outside them. So this set is built in four
 * groups:
 *
 * 1. **The exhaustive night cross-product** — every stated unit x every stated
 *    night, for `nightlyRate` and `nightlyAvailability`. 4 x 6 = 24 each. This
 *    is the part generation is genuinely good at: it cannot forget a closed
 *    night, and it cannot invent a unit that does not exist.
 * 2. **Every window pair** — `from` and `to` each drawn from the *bounds*
 *    (seven: six nights plus the checkout), so all 49 pairs including the
 *    backwards, the empty and the one-night ones. This is where the exclusive
 *    `to` lives, and a hand-written set would be guessing at which pairs matter.
 * 3. **The seams** — dates one step either side of the fixture window. The
 *    checkout `2026-10-11` as a *night*, the day before the window, the day
 *    after the window, and a month away. These are the cases a port gets wrong
 *    by accepting a legal bound as a night, or by extrapolating past the table.
 * 4. **The unknown and malformed identifiers** — unit ids that do not exist,
 *    dates that are not dates, and the case-mismatched variants. These are what
 *    the `None` paths exist for.
 *
 * ## Why the ids are deliberately odd
 *
 * `penthouse`, `garden-twin-extra` and `Garden-Twin` are not typos in this file.
 * They are the shapes a real caller produces: a unit the tenant does not own, a
 * near-miss id, and the same id with different casing. A port that resolves
 * identifiers case-insensitively, or that falls back to some default unit,
 * diverges on these rather than on anything a fixture table would catch.
 *
 * ## The one divergence this set is expected to find
 *
 * `__proto__` is the odd entry, and it is here on purpose. It is not a shape a
 * caller produces; it is a place where the two languages genuinely differ, and
 * the pilot's value is in showing it rather than in quietly not asking.
 *
 * The baseline reads `BASE_RATES[unitId]`. A plain object literal access walks
 * the prototype chain, so `BASE_RATES['__proto__']` is `Object.prototype` — not
 * `undefined` — and `roomsBuilt('__proto__')` answers `{}` instead of
 * `undefined`. The port's `BTreeMap::get` has no prototype chain to walk and
 * answers `None`, which serializes as `null`.
 *
 * ## How bad it actually gets, which is worse than "{}"
 *
 * The divergence is not only in the two unit-table reads. `nightlyRate` and
 * `stayQuote` consume that same lookup and return the bogus value to the caller:
 *
 *   nightlyRate('__proto__') -> amountMinor: {}          (an object in a number field)
 *   stayQuote('__proto__')    -> subtotalMinor: "0[object Object][object Object]
 *                                          [object Object][object Object]NaNNaN"
 *                               taxMinor: NaN
 *                               totalMinor:  "0[object Object]...NaNNaN"
 *
 * A **string** in the subtotal, tax and total fields. That is
 * `0 + {} + {} + ... + NaN + NaN` in JavaScript: the `0` is `sum`'s initial
 * value, the `[object Object]`s are the two weeknight rates, and the `NaN`s are
 * the two weekend rates, which became `NaN` because `roundToStep` was called on
 * a non-number. The type system that `Money` is supposed to carry does not stop
 * it, because the lookups are untyped table reads.
 *
 * So this is a real bug in the baseline, reachable only through a unit id the
 * caller controls, and it is one the port happens to be *right* about. It is
 * recorded as a divergence in the evidence file and explained there, rather than
 * being excised from the case set to make the run look clean. Removing it would
 * be exactly the "hide the difference to get a green number" failure the pilot
 * exists to avoid.
 *
 * ## Determinism
 *
 * No randomness at all, which is even stronger than p1's fixed-seed approach.
 * The whole space is small enough to enumerate, and an enumerable space that is
 * enumerated completely cannot drift when rerun.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'

// Node globals this generator touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names until
// they are named here.
/* global URL, process */

const OUT = fileURLToPath(new URL('./cases/rates.json', import.meta.url))

/**
 * The fixture dates, spelled here rather than imported.
 *
 * They are the *published* contract of the reference tenant — `REFERENCE_NIGHTS`
 * plus the checkout bound — and a generator that imported them would silently
 * follow the baseline if the baseline's window ever moved. Spelling them out
 * means the case file is a fixed target: if the product's window changes, the
 * generator fails to agree and the drift is visible instead of being absorbed.
 *
 * 2026-10-05 is a Monday, so the 9th and 10th are the weekend nights.
 */
const NIGHTS = ['2026-10-05', '2026-10-06', '2026-10-07', '2026-10-08', '2026-10-09', '2026-10-10']
/** Every date a stay may end on: the nights, plus the checkout after the last. */
const BOUNDS = [...NIGHTS, '2026-10-11']

/** The units the reference tenant owns. */
const UNITS = ['garden-twin', 'deluxe-valley', 'treetop-suite', 'valley-pool-villa']

/**
 * Identifiers no tenant owns, or owns spelled differently.
 *
 * Listing them as a table keeps the intent legible: each entry names the shape
 * of caller mistake it stands for, so a reviewer can see that the set is trying
 * to catch something specific rather than padding the count.
 *
 * `__proto__` is included even though it is not a tenant error but a *language*
 * error, and it is the one entry here that is expected to diverge. See the note
 * under the table — it is recorded as a finding rather than dropped.
 */
const UNKNOWN_UNITS = [
  'penthouse',
  'garden-twin-extra',
  'Garden-Twin',
  'GARDEN-TWIN',
  '',
  'garden twin',
  '__proto__',
]

/**
 * Dates outside the fixture window: the day before, the day after the checkout,
 * a month away, and two non-dates.
 */
const SEAM_DATES = ['2026-10-04', '2026-10-11', '2026-10-12', '2026-11-01', '', 'not-a-date']

/** The fixture dates the closed-night table is known to turn on. */
const NOTABLE_DATES = ['2026-10-07', '2026-10-08', '2026-10-09', '2026-10-10']

/**
 * Build the case list.
 *
 * @returns {Array<{id: string, op: string, note: string} & Record<string, string>>}
 */
function buildCases() {
  const cases = []

  /**
   * @param {string} id
   * @param {string} op
   * @param {string} note
   * @param {Record<string, string>} args
   */
  const add = (id, op, note, args) => cases.push({ id, op, note, ...args })

  // ---- Group 1: the exhaustive night cross-product ------------------------
  // Every unit on every night, for both per-night accessors. This is the group
  // that would catch a dropped closed night, a missing weekend night, or an
  // off-by-one in the rate table.
  for (const unitId of UNITS) {
    for (const night of NIGHTS) {
      add(`nightly-rate-${unitId}-${night}`, 'nightlyRate', 'unit x night rate', { unitId, night })
      add(`nightly-availability-${unitId}-${night}`, 'nightlyAvailability', 'unit x night', {
        unitId,
        night,
      })
    }
  }

  // ---- Group 2: every window pair -----------------------------------------
  // All 49 pairs of bounds, for every windowed op. Built from `BOUNDS` rather
  // than `NIGHTS` so the backwards, empty and one-night pairs are in the set by
  // construction instead of by someone remembering them.
  for (const from of BOUNDS) {
    for (const to of BOUNDS) {
      const label = `${from}_${to}`
      add(`nights-between-${label}`, 'nightsBetween', 'window to night list', { from, to })
      add(
        `window-no-unit-${label}`,
        'stayAvailability',
        'a window asked about without a sellable unit',
        { unitId: 'penthouse', from, to },
      )
    }
  }
  // Per-unit stay pricing over every window, which is the arithmetic path with
  // the most steps: nights -> rates -> subtotal -> tax -> total.
  for (const unitId of UNITS) {
    for (const from of BOUNDS) {
      for (const to of BOUNDS) {
        add(`stay-quote-${unitId}-${from}_${to}`, 'stayQuote', 'unit x window money', {
          unitId,
          from,
          to,
        })
        add(`stay-availability-${unitId}-${from}_${to}`, 'stayAvailability', 'unit x window', {
          unitId,
          from,
          to,
        })
      }
    }
  }
  // The two listing ops, over every window pair.
  for (const from of BOUNDS) {
    for (const to of BOUNDS) {
      add(`bookable-${from}_${to}`, 'bookableStays', 'sellable units', { from, to })
      add(`cheapest-${from}_${to}`, 'cheapestBookable', 'sellable units, ordered', { from, to })
    }
  }

  // ---- Group 3: the seams --------------------------------------------------
  // Just outside the window on either side. `2026-10-11` is a *bound* and is
  // the single most likely thing for a port to accept as a night; `2026-10-12`
  // is the first date the fixtures do not name as a checkout.
  for (const date of SEAM_DATES) {
    const label = date || 'empty'
    add(`seam-nightly-rate-${label}`, 'nightlyRate', 'a date outside the window', {
      unitId: UNITS[0],
      night: date,
    })
    add(`seam-nights-from-${label}`, 'nightsBetween', 'window starting outside', {
      from: date,
      to: BOUNDS[BOUNDS.length - 1],
    })
    add(`seam-nights-to-${label}`, 'nightsBetween', 'window ending outside', {
      from: NIGHTS[0],
      to: date,
    })
    add(`seam-stay-quote-${label}`, 'stayQuote', 'a stay the fixtures do not state', {
      unitId: UNITS[0],
      from: NIGHTS[0],
      to: date,
    })
  }

  // The closed nights, restated on their own: a closed night is the state where
  // `available` is false but the rate still exists, so both accessors have to
  // be checked against the same night for the pair to mean anything.
  for (const night of NOTABLE_DATES) {
    for (const unitId of UNITS) {
      add(`notable-night-${unitId}-${night}`, 'nightlyAvailability', 'a night the table turns on', {
        unitId,
        night,
      })
      add(`notable-rate-${unitId}-${night}`, 'nightlyRate', 'a rate on a notable night', {
        unitId,
        night,
      })
    }
  }

  // ---- Group 4: the unknown and odd identifiers ---------------------------
  for (const unitId of UNKNOWN_UNITS) {
    const label = unitId === '__proto__' ? 'proto' : unitId || 'empty'
    add(`unknown-units-built-${label}`, 'roomsBuilt', 'no such unit', { unitId })
    add(`unknown-units-rate-${label}`, 'baseRate', 'no such unit', { unitId })
    add(`unknown-units-nightly-${label}`, 'nightlyRate', 'no such unit', {
      unitId,
      night: NIGHTS[0],
    })
    add(`unknown-units-availability-${label}`, 'nightlyAvailability', 'no such unit', {
      unitId,
      night: NIGHTS[0],
    })
    add(`unknown-units-stay-${label}`, 'stayQuote', 'no such unit', {
      unitId,
      from: NIGHTS[0],
      to: BOUNDS[BOUNDS.length - 1],
    })
  }

  // The two predicates, over every bound and every seam date. Cheap cases, but
  // they are the only ones that pin the `to`-is-a-bound-but-not-a-night rule
  // directly.
  for (const date of [...BOUNDS, ...SEAM_DATES]) {
    const label = date || 'empty'
    add(`is-night-${label}`, 'isReferenceNight', 'is this date a night?', { night: date })
    add(`is-weekend-${label}`, 'isWeekendNight', 'does this date carry the uplift?', {
      night: date,
    })
  }

  return cases
}

const cases = buildCases()

mkdirSync(fileURLToPath(new URL('./cases', import.meta.url)), { recursive: true })
const payload = {
  slice: 'p2',
  generatedBy: 'node/gen-cases-rates.mjs',
  note:
    'Synthetic fixture dates and unit ids only. The window and the units are ' +
    'the reference tenant’s published fixtures; the unknown ids and ' +
    'out-of-window dates are deliberately invented.',
  count: cases.length,
  cases,
}
writeFileSync(OUT, `${JSON.stringify(payload, null, 2)}\n`)

// The counts are printed rather than only written, so the run that produced the
// file is legible from the terminal without opening it. The generator's own
// tally is what the harness re-checks against the file, so these two numbers
// must agree.
const byOp = new Map()
for (const item of cases) byOp.set(item.op, (byOp.get(item.op) ?? 0) + 1)
for (const [op, count] of [...byOp.entries()].sort()) {
  process.stdout.write(`${String(count).padStart(5)}  ${op}\n`)
}
process.stdout.write(`${String(cases.length).padStart(5)}  total\n`)
process.stdout.write(`wrote ${OUT}\n`)

/**
 * The generated file is re-read and checked, rather than trusting the write.
 *
 * A generator that reports success without confirming what it left on disk can
 * truncate silently, leaving a file whose declared `count` matches nothing.
 * Reading it back closes that.
 */
const written = JSON.parse(readFileSync(OUT, 'utf8'))
if (written.count !== written.cases.length) {
  throw new Error(
    `generator wrote an inconsistent file: ${written.count} vs ${written.cases.length}`,
  )
}
for (const [index, item] of written.cases.entries()) {
  if (typeof item.id !== 'string' || item.id.length === 0) {
    throw new Error(`case ${index} has no id`)
  }
}
