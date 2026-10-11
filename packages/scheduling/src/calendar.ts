import { z } from 'zod'
import { schedulingTimezoneSchema } from './contracts.js'

export const calendarDateSchema = z.iso.date().refine((x) => /^20\d\d-|^2100-/.test(x))
export const calendarTimeSchema = z.string().regex(/^(?:[01]\d|2[0-3]):[0-5]\d$/)
export class CalendarTimeError extends Error {
  constructor() {
    super('The requested local time is missing or ambiguous in its timezone.')
  }
}
const ordinal = (date: string) => Date.parse(calendarDateSchema.parse(date) + 'T00:00:00Z')
export function calendarNights(arrival: string, departure: string): readonly string[] {
  const first = ordinal(arrival),
    last = ordinal(departure),
    count = (last - first) / 86_400_000
  if (!Number.isInteger(count) || count < 1 || count > 31) throw new CalendarTimeError()
  return Array.from({ length: count }, (_, i) =>
    new Date(first + i * 86_400_000).toISOString().slice(0, 10),
  )
}
function formatter(timezone: string) {
  return new Intl.DateTimeFormat('en-CA', {
    timeZone: schedulingTimezoneSchema.parse(timezone),
    year: 'numeric',
    month: '2-digit',
    day: '2-digit',
    hour: '2-digit',
    minute: '2-digit',
    second: '2-digit',
    hourCycle: 'h23',
  })
}
function wallClock(value: number, format: Intl.DateTimeFormat) {
  const parts = Object.fromEntries(
    format.formatToParts(new Date(value)).map((p) => [p.type, p.value]),
  )
  return `${parts['year']}-${parts['month']}-${parts['day']}T${parts['hour']}:${parts['minute']}:${parts['second']}`
}
export function calendarToday(instant: string, timezone: string): string {
  const value = Date.parse(z.iso.datetime().parse(instant))
  return calendarDateSchema.parse(wallClock(value, formatter(timezone)).slice(0, 10))
}
/** Enumerate surrounding offsets and require exactly one round-tripping instant. No silent DST guessing. */
export function resolveCalendarInstant(date: string, time: string, timezone: string): string {
  const target = `${calendarDateSchema.parse(date)}T${calendarTimeSchema.parse(time)}:00`
  const naive = Date.parse(target + 'Z'),
    format = formatter(timezone),
    offsets = new Set<number>()
  for (let hours = -48; hours <= 48; hours += 4) {
    const probe = naive + hours * 3_600_000
    offsets.add(Date.parse(wallClock(probe, format) + 'Z') - probe)
  }
  const candidates = [...offsets]
    .map((offset) => naive - offset)
    .filter((value) => wallClock(value, format) === target)
  const resolved = candidates[0]
  if (candidates.length !== 1 || resolved === undefined) throw new CalendarTimeError()
  return new Date(resolved).toISOString()
}
