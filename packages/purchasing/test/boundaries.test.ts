import { describe, it, expect, vi } from 'vitest'
import {
  PurchasingService,
  PurchaseError,
  PURCHASE_PERMISSIONS,
  purchaseMutationSchema,
  createPurchasingApi,
  createPurchasingTools,
  type PurchaseRepository,
} from '../src/index.js'
const id = '10000000-0000-4000-8000-000000000001',
  human = { kind: 'human' as const, id: 'owner' },
  bot = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' },
  mutation = (command: unknown) => ({ requestId: id, command })
const repository = () => {
  const unused = () => Promise.reject(new Error('PRIVATE credential database details'))
  return {
    access: () =>
      Promise.resolve({
        permissions: [...PURCHASE_PERMISSIONS],
        ownerId: 'owner',
        scope: 'all' as const,
      }),
    suppliers: vi.fn(unused),
    offers: vi.fn(unused),
    orders: vi.fn(unused),
    detail: vi.fn(unused),
    mutate: vi.fn(unused),
    commandStatus: vi.fn(unused),
  } satisfies PurchaseRepository
}
describe('purchasing execution boundaries', () => {
  it('refuses quoted costs, totals, state, payment, owner and tenant authority injected into purchase orders', () => {
    const repo = repository(),
      s = new PurchasingService(repo),
      base = { operation: 'order.create', supplierId: id, lines: [{ offerId: id, quantity: 1 }] }
    for (const extra of [
      { ownerId: 'staff' },
      { tenantId: 'other' },
      { totalEstimate: 0 },
      { currency: 'USD' },
      { paid: true },
      { state: 'received' },
      { unitCost: 0 },
    ])
      expect(() => s.mutate('retail', human, mutation({ ...base, ...extra }))).toThrow()
    expect(() =>
      s.mutate(
        'retail',
        human,
        mutation({ ...base, lines: [{ offerId: id, quantity: 1, unitCost: 0 }] }),
      ),
    ).toThrow()
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('bounds unique order and inspection lines, quantities and explicit reasons', () => {
    for (const lines of [
      [],
      [
        { offerId: id, quantity: 1 },
        { offerId: id, quantity: 2 },
      ],
      [{ offerId: id, quantity: 1000001 }],
      [{ offerId: id, quantity: 0 }],
    ])
      expect(
        purchaseMutationSchema.safeParse(
          mutation({ operation: 'order.create', supplierId: id, lines }),
        ).success,
      ).toBe(false)
    for (const lines of [
      [],
      [{ lineId: id, accepted: 0, rejected: 0 }],
      [{ lineId: id, accepted: 1, rejected: -1 }],
      [{ lineId: id, accepted: 600000, rejected: 600000 }],
      [
        { lineId: id, accepted: 1, rejected: 0 },
        { lineId: id, accepted: 1, rejected: 0 },
      ],
    ])
      expect(
        purchaseMutationSchema.safeParse(
          mutation({
            operation: 'order.receive',
            id,
            expectedRevision: 1,
            reason: 'Inspect',
            lines,
          }),
        ).success,
      ).toBe(false)
  })
  it('rejects machine mutations and receipt recovery before contacting persistence', () => {
    const repo = repository(),
      s = new PurchasingService(repo)
    expect(() =>
      s.mutate('retail', bot, mutation({ operation: 'supplier.create', code: 'A', name: 'A' })),
    ).toThrow(PurchaseError)
    expect(() => s.commandStatus('retail', bot, id)).toThrow(PurchaseError)
    expect(repo.mutate).not.toHaveBeenCalled()
    expect(repo.commandStatus).not.toHaveBeenCalled()
  })
  it('allows only declared private read tools and denies unknown execution tools', () => {
    const repo = repository(),
      t = createPurchasingTools(new PurchasingService(repo), 'retail', human)
    expect(t.definitions.every((x) => x.effect === 'read')).toBe(true)
    for (const name of ['purchasing.receive', 'purchasing.mutate', 'payment.mark-paid'])
      expect(() => t.invoke(name, {})).toThrow(PurchaseError)
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('refuses ambiguous repeated query authority, cross-origin cookie posts and oversized streaming payloads', async () => {
    const repo = repository(),
      api = createPurchasingApi(new PurchasingService(repo), () => Promise.resolve(human))
    for (const path of ['/orders?limit=2&limit=3', '/orders?tenantId=other', '/offers?active=yes'])
      expect((await api(new Request('http://local/api/purchasing/retail' + path))).status).toBe(400)
    expect(
      (
        await api(
          new Request('http://local/api/purchasing/retail/commands', {
            method: 'POST',
            headers: { cookie: 'session=private', origin: 'http://elsewhere' },
            body: '{}',
          }),
        )
      ).status,
    ).toBe(403)
    expect(
      (
        await api(
          new Request('http://local/api/purchasing/retail/commands', {
            method: 'POST',
            headers: { origin: 'http://local' },
            body: 'x'.repeat(16385),
          }),
        )
      ).status,
    ).toBe(413)
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('returns opaque unavailable errors without repository credentials or SQL details', async () => {
    const api = createPurchasingApi(new PurchasingService(repository()), () =>
        Promise.resolve(human),
      ),
      r = await api(new Request('http://local/api/purchasing/retail/orders'))
    expect(r.status).toBe(503)
    expect(await r.json()).toEqual({ error: 'unavailable' })
  })
})
