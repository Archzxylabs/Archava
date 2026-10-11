import { describe, it, expect } from 'vitest'
import {
  calendarNights,
  calendarToday,
  resolveCalendarInstant,
  CalendarTimeError,
} from '../src/calendar.js'
describe('calendar-night primitives', () => {
  it('counts calendar nights across DST rather than dividing elapsed time by 24 hours', () => {
    expect(calendarNights('2026-03-07', '2026-03-09')).toEqual(['2026-03-07', '2026-03-08'])
    const first = resolveCalendarInstant('2026-03-07', '00:00', 'America/New_York'),
      last = resolveCalendarInstant('2026-03-09', '00:00', 'America/New_York')
    expect((Date.parse(last) - Date.parse(first)) / 3_600_000).toBe(47)
    expect(
      (Date.parse(resolveCalendarInstant('2026-11-02', '00:00', 'America/New_York')) -
        Date.parse(resolveCalendarInstant('2026-10-31', '00:00', 'America/New_York'))) /
        3_600_000,
    ).toBe(49)
  })
  it('rejects nonexistent and ambiguous local times and a skipped calendar date', () => {
    for (const [date, time, zone] of [
      ['2026-03-08', '02:30', 'America/New_York'],
      ['2026-11-01', '01:30', 'America/New_York'],
      ['2011-12-30', '15:00', 'Pacific/Apia'],
    ] as const)
      expect(() => resolveCalendarInstant(date, time, zone)).toThrow(CalendarTimeError)
  })
  it('resolves quarter-hour timezones and derives the property date from authoritative UTC time', () => {
    expect(resolveCalendarInstant('2026-10-11', '15:00', 'Asia/Kathmandu')).toBe(
      '2026-10-11T09:15:00.000Z',
    )
    expect(calendarToday('2026-10-11T22:00:00.000Z', 'Asia/Tokyo')).toBe('2026-10-12')
  })
  it('requires valid date-only input, positive bounded nights and supported timezones', () => {
    for (const pair of [
      ['2026-02-30', '2026-03-02'],
      ['2026-10-11', '2026-10-11'],
      ['2026-10-12', '2026-10-11'],
      ['2026-10-11', '2026-12-11'],
    ] as const)
      expect(() => calendarNights(pair[0], pair[1])).toThrow()
    expect(() => resolveCalendarInstant('2026-10-11', '15:00', 'Mars/Olympus')).toThrow()
    expect(() => resolveCalendarInstant('2026-10-11', '24:00', 'UTC')).toThrow()
  })
})
