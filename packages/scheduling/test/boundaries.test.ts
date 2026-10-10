import { describe, expect, it, vi } from 'vitest'
import {
  createSchedulingApi,
  createSchedulingTools,
  SchedulingService,
  SchedulingError,
  schedulingInstantSchema,
  schedulingIntervalSchema,
  schedulingTimezoneSchema,
  type SchedulingRepository,
} from '../src/index.js'

const id = 'f3d87d79-06a8-43d3-93a0-d27387fa2665'
const mutation = {
  requestId: id,
  command: {
    operation: 'reservation.hold',
    resourceId: id,
    title: 'Consultation',
    startsAt: '2026-10-12T10:00:00+07:00',
    endsAt: '2026-10-12T11:00:00+07:00',
  },
}
function fixture() {
  const repo = {
    access: vi.fn<SchedulingRepository['access']>(),
    resources: vi.fn<SchedulingRepository['resources']>(),
    resource: vi.fn<SchedulingRepository['resource']>(),
    reservations: vi.fn<SchedulingRepository['reservations']>(),
    detail: vi.fn<SchedulingRepository['detail']>(),
    availability: vi.fn<SchedulingRepository['availability']>(),
    mutate: vi.fn<SchedulingRepository['mutate']>(),
    commandStatus: vi.fn<SchedulingRepository['commandStatus']>(),
  }
  const service = new SchedulingService(repo),
    api = createSchedulingApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
  const request = (body: unknown, headers: Record<string, string> = {}) =>
    api(
      new Request('https://app.test/api/scheduling/studio/commands', {
        method: 'POST',
        headers: { origin: 'https://app.test', ...headers },
        body: typeof body === 'string' ? body : JSON.stringify(body),
      }),
    )
  return { repo, service, api, request }
}
describe('scheduling boundaries', () => {
  it('normalizes explicit offsets and rejects ambiguous, invalid or lossy time input', () => {
    expect(schedulingInstantSchema.parse('2026-10-12T10:00:00+07:00')).toBe(
      '2026-10-12T03:00:00.000Z',
    )
    for (const value of [
      '2026-10-12T10:00:00',
      '2026-02-30T10:00:00Z',
      '2026-10-12',
      '2026-10-12T10:00:00.0001Z',
      '2026-10-12T10:00Z',
      '1999-10-12T10:00:00Z',
    ])
      expect(schedulingInstantSchema.safeParse(value).success).toBe(false)
    expect(schedulingTimezoneSchema.safeParse('Mars/Olympus').success).toBe(false)
    expect(schedulingTimezoneSchema.parse('Asia/Jakarta')).toBe('Asia/Jakarta')
  })
  it('represents both DST occurrences with explicit offsets and keeps real elapsed duration', () => {
    const first = schedulingInstantSchema.parse('2026-11-01T01:30:00-04:00'),
      second = schedulingInstantSchema.parse('2026-11-01T01:30:00-05:00')
    expect(Date.parse(second) - Date.parse(first)).toBe(3_600_000)
    expect(schedulingIntervalSchema.parse({ startsAt: first, endsAt: second })).toEqual({
      startsAt: first,
      endsAt: second,
    })
  })
  it('bounds positive intervals to 31 days', () => {
    for (const endsAt of ['2026-10-12T03:00:00Z', '2026-10-12T02:00:00Z', '2026-12-12T03:00:00Z'])
      expect(
        schedulingIntervalSchema.safeParse({ startsAt: '2026-10-12T03:00:00Z', endsAt }).success,
      ).toBe(false)
  })
  it('binds command identity to host and refuses tenant, owner, state, expiry or capacity overrides', async () => {
    const f = fixture()
    f.repo.mutate.mockResolvedValue({ kind: 'reservation', id, resourceId: id, revision: 1 })
    expect((await f.request(mutation)).status).toBe(200)
    expect(f.repo.mutate.mock.calls[0]?.[0]).toBe('studio')
    expect(f.repo.mutate.mock.calls[0]?.[1]).toEqual({ kind: 'human', id: 'owner' })
    expect(f.repo.mutate.mock.calls[0]?.[2].command).toMatchObject({
      startsAt: '2026-10-12T03:00:00.000Z',
      holdSeconds: 900,
    })
    for (const extra of [
      { tenantId: 'other' },
      { ownerId: 'admin' },
      { state: 'confirmed' },
      { expiresAt: '2099-01-01T00:00:00Z' },
      { capacity: 99 },
    ])
      expect(
        (await f.request({ ...mutation, command: { ...mutation.command, ...extra } })).status,
      ).toBe(400)
    expect((await f.request({ ...mutation, principal: { id: 'admin' } })).status).toBe(400)
    expect(f.repo.mutate).toHaveBeenCalledTimes(1)
  })
  it('exposes read tools only and refuses principal overrides and machine writes before persistence', async () => {
    const f = fixture(),
      tools = createSchedulingTools(f.service, 'studio', { kind: 'human', id: 'owner' })
    await tools.invoke('scheduling.resources', {})
    expect(f.repo.resources).toHaveBeenCalledWith(
      'studio',
      { kind: 'human', id: 'owner' },
      { limit: 20 },
    )
    await expect(tools.invoke('scheduling.resources', { role: 'owner' })).rejects.toThrow()
    await expect(tools.invoke('scheduling.hold', mutation.command)).rejects.toThrow(SchedulingError)
    const bot = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' }
    expect(() => f.service.mutate('studio', bot, mutation)).toThrow(SchedulingError)
    expect(() => f.service.commandStatus('studio', bot, id)).toThrow(SchedulingError)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('rejects foreign origins, cookie POST without origin and oversized or malformed bodies', async () => {
    const f = fixture()
    expect((await f.request(mutation, { origin: 'https://evil.test' })).status).toBe(403)
    expect(
      (
        await f.api(
          new Request('https://app.test/api/scheduling/studio/commands', {
            method: 'POST',
            headers: { cookie: 'fixture=opaque' },
            body: JSON.stringify(mutation),
          }),
        )
      ).status,
    ).toBe(403)
    expect((await f.request('x'.repeat(16_385))).status).toBe(413)
    expect((await f.request('{oops')).status).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('requires authentication and refuses duplicate/unknown/invalid paging and boolean query values', async () => {
    const f = fixture()
    expect(
      (
        await createSchedulingApi(f.service, () => Promise.resolve(null))(
          new Request('https://app.test/api/scheduling/studio/resources'),
        )
      ).status,
    ).toBe(401)
    for (const query of ['tenant=other', 'limit=101', 'limit=1&limit=2', 'active=yes'])
      expect(
        (await f.api(new Request('https://app.test/api/scheduling/studio/resources?' + query)))
          .status,
      ).toBe(400)
    expect(f.repo.resources).not.toHaveBeenCalled()
  })
  it('requires bounded TTL, current revisions and reasons; database errors stay private', async () => {
    const f = fixture()
    for (const command of [
      { ...mutation.command, holdSeconds: 1801 },
      { operation: 'reservation.cancel', resourceId: id, id, expectedRevision: 1 },
      { operation: 'reservation.confirm', resourceId: id, id, expectedRevision: 0 },
      { operation: 'resource.create', title: 'Room', kind: 'room', timezone: 'UTC', capacity: 2 },
    ])
      expect((await f.request({ requestId: id, command })).status).toBe(400)
    f.repo.mutate.mockRejectedValue(new Error('secret-db-marker'))
    const response = await f.request(mutation)
    expect(response.status).toBe(503)
    expect(await response.text()).not.toContain('secret-db-marker')
  })
})
