import { describe, it, expect, vi } from 'vitest'
import {
  AccommodationService,
  createAccommodationApi,
  createAccommodationTools,
  AccommodationError,
  type AccommodationRepository,
} from '../src/index.js'
const id = 'f3d87d79-06a8-43d3-93a0-d27387fa2665'
const mutation = {
  requestId: id,
  command: {
    operation: 'stay.hold',
    propertyId: id,
    roomId: id,
    arrival: '2026-10-12',
    departure: '2026-10-14',
    guestName: 'Synthetic guest',
    guests: 2,
  },
}
function fixture() {
  const repo = {
    access: vi.fn<AccommodationRepository['access']>(),
    properties: vi.fn<AccommodationRepository['properties']>(),
    property: vi.fn<AccommodationRepository['property']>(),
    roomTypes: vi.fn<AccommodationRepository['roomTypes']>(),
    rooms: vi.fn<AccommodationRepository['rooms']>(),
    room: vi.fn<AccommodationRepository['room']>(),
    stays: vi.fn<AccommodationRepository['stays']>(),
    detail: vi.fn<AccommodationRepository['detail']>(),
    availability: vi.fn<AccommodationRepository['availability']>(),
    mutate: vi.fn<AccommodationRepository['mutate']>(),
    commandStatus: vi.fn<AccommodationRepository['commandStatus']>(),
  }
  const service = new AccommodationService(repo),
    api = createAccommodationApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
  const request = (body: unknown, headers: Record<string, string> = {}) =>
    api(
      new Request('https://app.test/api/accommodation/hotel/commands', {
        method: 'POST',
        headers: { origin: 'https://app.test', ...headers },
        body: typeof body === 'string' ? body : JSON.stringify(body),
      }),
    )
  return { repo, service, api, request }
}
describe('accommodation boundaries', () => {
  it('binds human identity to host and prevents model-supplied prices, payment, expiry or guest-owner authority', async () => {
    const f = fixture()
    f.repo.mutate.mockResolvedValue({ kind: 'stay', id, propertyId: id, revision: 1 })
    expect((await f.request(mutation)).status).toBe(200)
    expect(f.repo.mutate.mock.calls[0]?.[1]).toEqual({ kind: 'human', id: 'owner' })
    expect(f.repo.mutate.mock.calls[0]?.[2].command).toMatchObject({ holdSeconds: 900 })
    for (const extra of [
      { nightlyRate: 1 },
      { totalEstimate: 0 },
      { paid: true },
      { ownerId: 'admin' },
      { expiresAt: '2099-01-01T00:00:00Z' },
      { checkedInAt: '2026-10-12T00:00:00Z' },
    ])
      expect(
        (await f.request({ ...mutation, command: { ...mutation.command, ...extra } })).status,
      ).toBe(400)
    expect((await f.request({ ...mutation, tenantId: 'other' })).status).toBe(400)
    expect(f.repo.mutate).toHaveBeenCalledTimes(1)
  })
  it('requires calendar dates, bounded nights/TTL/guests and integer operator rate inputs', async () => {
    const f = fixture()
    for (const extra of [
      { arrival: '2026-10-12T10:00:00Z' },
      { departure: '2026-10-12' },
      { departure: '2026-12-12' },
      { arrival: '2026-02-30' },
      { guests: 0 },
      { holdSeconds: 1801 },
    ])
      expect(
        (await f.request({ ...mutation, command: { ...mutation.command, ...extra } })).status,
      ).toBe(400)
    expect(
      (
        await f.request({
          requestId: id,
          command: {
            operation: 'room-type.create',
            propertyId: id,
            title: 'Suite',
            maxGuests: 2,
            nightlyRate: 1.5,
          },
        })
      ).status,
    ).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('refuses unregistered machine writes and actor recovery before reaching persistence', () => {
    const f = fixture(),
      bot = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' }
    expect(() => f.service.mutate('hotel', bot, mutation)).toThrow(AccommodationError)
    expect(() => f.service.commandStatus('hotel', bot, id)).toThrow(AccommodationError)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('exposes only bound read tools and refuses role/tenant/property authority overrides', async () => {
    const f = fixture(),
      tools = createAccommodationTools(f.service, 'hotel', { kind: 'human', id: 'owner' })
    await tools.invoke('accommodation.rooms', { propertyId: id })
    expect(f.repo.rooms.mock.calls[0]?.[0]).toBe('hotel')
    await expect(
      tools.invoke('accommodation.rooms', { propertyId: id, tenant: 'other' }),
    ).rejects.toThrow()
    await expect(tools.invoke('accommodation.detail', { id, role: 'owner' })).rejects.toThrow()
    await expect(tools.invoke('accommodation.check-in', mutation.command)).rejects.toThrow(
      AccommodationError,
    )
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('requires current revisions and reasons and refuses mismatched arrival/checkout policy', async () => {
    const f = fixture()
    for (const command of [
      { operation: 'stay.cancel', propertyId: id, id, expectedRevision: 1 },
      { operation: 'stay.check-in', propertyId: id, id, expectedRevision: 0 },
      {
        operation: 'room.housekeeping',
        propertyId: id,
        roomId: id,
        expectedRevision: 1,
        state: 'inspected',
      },
      {
        operation: 'property.create',
        title: 'Hotel',
        timezone: 'UTC',
        checkInTime: '11:00',
        checkOutTime: '15:00',
      },
    ])
      expect((await f.request({ requestId: id, command })).status).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('enforces same origin, identity, bounded streams and strict query filters', async () => {
    const f = fixture()
    expect((await f.request(mutation, { origin: 'https://evil.test' })).status).toBe(403)
    expect(
      (
        await f.api(
          new Request('https://app.test/api/accommodation/hotel/commands', {
            method: 'POST',
            headers: { cookie: 'fixture=opaque' },
            body: JSON.stringify(mutation),
          }),
        )
      ).status,
    ).toBe(403)
    expect((await f.request('x'.repeat(16385))).status).toBe(413)
    expect((await f.request('{oops')).status).toBe(400)
    expect(
      (
        await createAccommodationApi(f.service, () => Promise.resolve(null))(
          new Request('https://app.test/api/accommodation/hotel/properties'),
        )
      ).status,
    ).toBe(401)
    for (const query of ['role=owner', 'limit=101', 'limit=1&limit=2'])
      expect(
        (await f.api(new Request('https://app.test/api/accommodation/hotel/properties?' + query)))
          .status,
      ).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('does not disclose database credentials or private failure messages', async () => {
    const f = fixture()
    f.repo.mutate.mockRejectedValue(new Error('private-db-marker'))
    const response = await f.request(mutation)
    expect(response.status).toBe(503)
    expect(await response.text()).not.toContain('private-db-marker')
  })
})
