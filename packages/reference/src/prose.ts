/**
 * A structured-truth record as a sentence, for the reference tenant.
 *
 * The record the port resolves is the *authority*, and it is a record: a price
 * snapshot carries every unit's every night, because a snapshot cannot know which
 * one was asked about. Handing that to a visitor verbatim is what the browser E2E
 * showed — a chat bubble containing a JSON blob, which is not an answer, and a
 * blob that grows as the tenant adds a unit or a night.
 *
 * So the sentence is written here, in the tenant that owns the schema, rather
 * than in the brain adapter. An adapter must stay schema-agnostic: it can turn a
 * value into a string but it cannot know that `amountMinor` is money or that
 * 2_415_000 is a rate for the night of the 10th. This module knows both, because
 * `./rates.js` defines them.
 *
 * Two claims this file makes and keeps:
 *
 * 1. It selects; it never invents. Every number in the output comes from the
 *    record it was handed. When the question names a unit the record covers, the
 *    answer is that unit's figure. When it names none, the answer is the range
 *    across what the record does cover — and a range is what it is, not a
 *    representative angle drawn from the middle of it.
 * 2. It is partial by construction and says so. The record is what the port
 *    resolved at the time of the turn; a later live check could differ. The
 *    visitor is told the window, which is the honest way to say "these figures
 *    are live as of this turn" without stamping a time nobody measured.
 */

import { minorUnitDivisor } from '@archava/config/pure'

/** A resolved subject's record, as the port answered it. */
type SubjectRecord = Readonly<Record<string, unknown>>

/** One nightly rate, read out of a price snapshot without trusting the shape. */
interface Rate {
  readonly unitId: string
  readonly night: string
  readonly amountMinor: number
}

const NUMBER = /^-?\d+$/

function isRecord(value: unknown): value is SubjectRecord {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function asString(value: unknown): string | null {
  return typeof value === 'string' && value !== '' ? value : null
}

function asNumber(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) ? value : null
}

/**
 * Minor units to a display string.
 *
 * The divisor comes from `minorUnitDivisor`, the platform's currency contract —
 * this function used to test `currency === 'IDR'` and divide everything else by
 * 100, which was two definitions of one fact in one platform. The second would
 * have kept under-quoting the moment the pricebook gained a zero-decimal
 * currency, because "not IDR" is a guess wearing the shape of a rule. An
 * unsupported currency throws instead: the reference tenant prices in IDR and
 * USD, so a third code is a defect in whatever produced it, and a `NaN` in a
 * sentence is a worse answer than a failure.
 *
 * The result is rounded to whole major units and grouped, because a resort
 * quotes a nightly rate as "2,415,000" rather than as cents.
 */
export function formatMoney(amountMinor: number, currency: string): string {
  const major = Math.round(amountMinor / minorUnitDivisor(currency))
  const grouped = Math.abs(major)
    .toString()
    .replace(/\B(?=(\d{3})+(?!\d))/g, ',')
  const sign = major < 0 ? '-' : ''
  return `${currency} ${sign}${grouped}`
}

/** The nightly rates a price snapshot carries, in the order it carries them. */
function ratesOf(record: SubjectRecord): readonly Rate[] {
  const nights = Array.isArray(record.nights) ? record.nights : []
  const rates: Rate[] = []
  for (const entry of nights) {
    if (!isRecord(entry)) continue
    const unitId = asString(entry.unitId)
    const night = asString(entry.night)
    const amountMinor = asNumber(entry.amountMinor)
    if (unitId === null || night === null || amountMinor === null) continue
    rates.push({ unitId, night, amountMinor })
  }
  return rates
}

/** The currency a price snapshot states, or `null` when it states none. */
function currencyOf(record: SubjectRecord): string | null {
  return asString(record.currency)
}

/**
 * Text reduced to what a unit id can be matched inside.
 *
 * A visitor says "treetop suite" where the record says `treetop-suite`: the same
 * name with a space where the id has a hyphen. Matching on the folded form is
 * what lets the visitor's phrasing reach the figure. It compares whole ids and
 * never fragments of one, so `valley` does not quietly pull in
 * `valley-pool-villa`.
 */
function fold(text: string): string {
  return text.toLowerCase().replace(/[^a-z0-9]+/g, '')
}

/**
 * A unit named in the question, if the question names one the snapshot covers.
 *
 * Longest id first, so a shorter id sharing a leading word cannot shadow it —
 * `valley-pool-villa` must be tried before `garden-twin`'s neighbour, and a
 * question naming the villa must not be answered with the twin.
 */
function namedUnit(utterance: string, unitIds: readonly string[]): string | null {
  const text = utterance.toLowerCase()
  const folded = fold(utterance)
  const ordered = [...unitIds].sort((left, right) => right.length - left.length)
  return ordered.find((id) => text.includes(id) || folded.includes(fold(id))) ?? null
}

/** Unique values in first-seen order, which is the order the record published. */
function unique(values: readonly string[]): readonly string[] {
  return values.filter((value, index) => values.indexOf(value) === index)
}

/** `a, b and c` — the list as a sentence reads it. */
function joinList(items: readonly string[]): string {
  // Indexed off the end rather than length-checked first: `items.length` is a
  // number and the element behind it is still `string | undefined` under
  // `noUncheckedIndexedAccess`, so the guard has to be on the value the code is
  // about to interpolate, not on the count it was reached through.
  const last = items[items.length - 1]
  if (last === undefined) return ''
  const head = items.slice(0, -1)
  return head.length === 0 ? last : `${head.join(', ')} and ${last}`
}

/** The range over a set of amounts, said the way a range reads. */
function spanOf(amounts: readonly number[], currency: string): string {
  const low = Math.min(...amounts)
  const high = Math.max(...amounts)
  return low === high
    ? formatMoney(low, currency)
    : `${formatMoney(low, currency)} to ${formatMoney(high, currency)}`
}

const MONTHS = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
] as const

/** An ISO date as `5 October`, or the raw string when it is not one. */
function dayOf(iso: string): string {
  const parts = iso.split('-')
  const month = MONTHS[Number(parts[1]) - 1]
  const day = parts[2]
  if (day === undefined || month === undefined || !NUMBER.test(day)) return iso
  return `${Number(day)} ${month}`
}

/** The snapshot's window as `5 October to 11 October`, or `null` when absent. */
function windowOf(record: SubjectRecord): string | null {
  const window = isRecord(record.window) ? record.window : null
  const from = window === null ? null : asString(window.from)
  const to = window === null ? null : asString(window.to)
  if (from === null || to === null) return null
  return `${dayOf(from)} to ${dayOf(to)}`
}

/** The price snapshot as one answer, narrowed to the unit the question named. */
function priceProse(record: SubjectRecord, utterance: string): string {
  const rates = ratesOf(record)
  if (rates.length === 0) return 'I have no live rate for that.'

  const currency = currencyOf(record) ?? 'IDR'
  const window = windowOf(record)
  const prefix = window === null ? '' : `${window}: `
  const named = namedUnit(utterance, unique(rates.map((rate) => rate.unitId)))

  // One unit named: its own figures, and nothing else. A snapshot holding several
  // nights for it is reported as the range over those nights, because lifting one
  // figure out of the range would answer a night the visitor did not ask about.
  if (named !== null) {
    const own = rates.filter((rate) => rate.unitId === named).map((rate) => rate.amountMinor)
    return `${prefix}${named} is ${spanOf(own, currency)} per night.`
  }

  // Nothing named: the span across everything the snapshot covers, plus each
  // unit's own. Not one unit's rate wearing the label "the price".
  const perUnit = unique(rates.map((rate) => rate.unitId))
    .map((unitId) => {
      const own = rates.filter((rate) => rate.unitId === unitId).map((rate) => rate.amountMinor)
      return `${unitId} ${spanOf(own, currency)}`
    })
    .join('; ')
  const all = rates.map((rate) => rate.amountMinor)
  return `${prefix}rates run ${spanOf(all, currency)} per night (${perUnit}).`
}

/** The stock snapshot as one answer: how many of each kind there are. */
function stockProse(record: SubjectRecord): string {
  const units = Array.isArray(record.units) ? record.units : []
  const counts: string[] = []
  for (const entry of units) {
    if (!isRecord(entry)) continue
    const roomsBuilt = asNumber(entry.roomsBuilt)
    if (roomsBuilt === null) continue
    const label = asString(entry.name) ?? asString(entry.unitId)
    if (label === null) continue
    counts.push(`${roomsBuilt} × ${label}`)
  }
  if (counts.length === 0) return 'I have no live count for that.'
  return `We have ${joinList(counts)}.`
}

/** The availability snapshot as one answer: what can be booked, for how much. */
function availabilityProse(record: SubjectRecord): string {
  const bookable = Array.isArray(record.bookable) ? record.bookable : []
  const stays: string[] = []
  for (const entry of bookable) {
    if (!isRecord(entry)) continue
    const unitId = asString(entry.unitId)
    const nights = asNumber(entry.nights)
    const totalMinor = asNumber(entry.totalMinor)
    if (unitId === null || nights === null || totalMinor === null) continue
    const currency = asString(entry.currency) ?? 'IDR'
    stays.push(`${unitId} for ${nights} nights at ${formatMoney(totalMinor, currency)}`)
  }
  const window = windowOf(record)
  const prefix = window === null ? '' : `${window}: `
  if (stays.length === 0) return `${prefix}nothing is bookable.`
  return `${prefix}we can book ${joinList(stays)}.`
}

/** Subjects this tenant can put into words, and where each one's words come from. */
const RENDERERS: Readonly<Record<string, (record: SubjectRecord, utterance: string) => string>> = {
  price: priceProse,
  stock: stockProse,
  availability: availabilityProse,
}

/**
 * A structured-truth record as a sentence, or `null` when it cannot be one.
 *
 * `null` is not a failure to hide: the caller keeps the record it already has, and
 * a value the tenant cannot put into words is better shown as itself than swapped
 * for nothing. The platform defines eight subjects and this fixture renders the
 * three it owns, so `null` is a real outcome rather than a defensive branch.
 */
export function renderStructuredTruth(
  values: Readonly<Record<string, unknown>>,
  utterance: string,
): string | null {
  const sentences: string[] = []
  const unwritten: string[] = []

  for (const [subject, value] of Object.entries(values)) {
    const renderer = RENDERERS[subject]
    if (renderer === undefined || !isRecord(value)) {
      // Not wordable here. Named rather than quietly dropped, because silence
      // about part of the record is a gap the turn report would not show.
      unwritten.push(subject)
      continue
    }
    const sentence = renderer(value, utterance)
    if (sentence !== '') sentences.push(sentence)
  }

  if (sentences.length === 0) return null
  const answer = sentences.join(' ')
  return unwritten.length === 0
    ? answer
    : `${answer} (I cannot put ${joinList(unwritten)} into words.)`
}
