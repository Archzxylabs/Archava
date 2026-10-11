import { describe, it, expect, vi } from 'vitest'
import {
  CommerceService,
  CommerceError,
  createCommerceApi,
  createCommerceTools,
  type CommerceRepository,
} from '../src/index.js'
const id = 'e8125f0d-93e8-4bb6-830a-10af37ea0a1a'
const mutation = {
  requestId: id,
  command: {
    operation: 'order.create',
    customerName: 'Synthetic customer',
    lines: [{ productId: id, quantity: 2 }],
  },
}
function fixture() {
  const repo = {
    access: vi.fn<CommerceRepository['access']>(),
    products: vi.fn<CommerceRepository['products']>(),
    product: vi.fn<CommerceRepository['product']>(),
    stock: vi.fn<CommerceRepository['stock']>(),
    movements: vi.fn<CommerceRepository['movements']>(),
    orders: vi.fn<CommerceRepository['orders']>(),
    detail: vi.fn<CommerceRepository['detail']>(),
    mutate: vi.fn<CommerceRepository['mutate']>(),
    commandStatus: vi.fn<CommerceRepository['commandStatus']>(),
  }
  const service = new CommerceService(repo),
    api = createCommerceApi(service, () => Promise.resolve({ kind: 'human', id: 'owner' }))
  const post = (body: unknown, headers: Record<string, string> = {}) =>
    api(
      new Request('https://app.test/api/commerce/shop/commands', {
        method: 'POST',
        headers: { origin: 'https://app.test', ...headers },
        body: typeof body === 'string' ? body : JSON.stringify(body),
      }),
    )
  return { repo, service, api, post }
}
describe('commerce public authority boundaries', () => {
  it('binds identity to the host and refuses invented line price, total, payment and owner fields', async () => {
    const f = fixture()
    f.repo.mutate.mockResolvedValue({ kind: 'order', id, revision: 1 })
    expect((await f.post(mutation)).status).toBe(200)
    expect(f.repo.mutate.mock.calls[0]?.[0]).toBe('shop')
    expect(f.repo.mutate.mock.calls[0]?.[1]).toEqual({ kind: 'human', id: 'owner' })
    for (const extra of [
      { totalEstimate: 0 },
      { paid: true },
      { ownerId: 'admin' },
      { currency: 'USD' },
      { managedBy: 'foodservice' },
      { expiresAt: '2099-01-01T00:00:00Z' },
    ])
      expect(
        (await f.post({ ...mutation, command: { ...mutation.command, ...extra } })).status,
      ).toBe(400)
    expect(
      (
        await f.post({
          ...mutation,
          command: { ...mutation.command, lines: [{ productId: id, quantity: 2, price: 0 }] },
        })
      ).status,
    ).toBe(400)
    expect((await f.post({ ...mutation, principal: { kind: 'human', id: 'admin' } })).status).toBe(
      400,
    )
    expect(f.repo.mutate).toHaveBeenCalledTimes(1)
  })
  it('requires unique bounded positive integer quantities and controlled catalog units', async () => {
    const f = fixture()
    for (const lines of [
      [],
      [{ productId: id, quantity: 0 }],
      [{ productId: id, quantity: 1.5 }],
      [{ productId: id, quantity: 10001 }],
      [
        { productId: id, quantity: 1 },
        { productId: id, quantity: 1 },
      ],
      Array.from({ length: 51 }, () => ({ productId: crypto.randomUUID(), quantity: 1 })),
    ])
      expect((await f.post({ ...mutation, command: { ...mutation.command, lines } })).status).toBe(
        400,
      )
    for (const extra of [
      { price: 0.5 },
      { price: 1000000001 },
      { unit: 'kg' },
      { sku: 'invalid space' },
    ])
      expect(
        (
          await f.post({
            requestId: id,
            command: {
              operation: 'product.create',
              sku: 'SKU',
              title: 'Product',
              price: 1,
              ...extra,
            },
          })
        ).status,
      ).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('requires current revisions, reasons and received return dispositions without refund authority', async () => {
    const f = fixture()
    for (const command of [
      { operation: 'order.fulfill', id, expectedRevision: 0 },
      { operation: 'order.reserve', id, expectedRevision: 1, holdSeconds: 1801 },
      {
        operation: 'stock.adjust',
        productId: id,
        expectedRevision: 1,
        quantity: 0,
        reason: 'Count',
      },
      { operation: 'order.cancel', id, expectedRevision: 1 },
      {
        operation: 'order.return',
        id,
        expectedRevision: 1,
        lines: [{ lineId: id, quantity: 1, disposition: 'restock' }],
      },
      {
        operation: 'order.return',
        id,
        expectedRevision: 1,
        lines: [{ lineId: id, quantity: 1, disposition: 'refund' }],
        reason: 'Returned',
      },
      {
        operation: 'order.return',
        id,
        expectedRevision: 1,
        lines: [{ lineId: id, quantity: 1, disposition: 'restock' }],
        reason: 'Returned',
        refundSuccessful: true,
      },
    ])
      expect((await f.post({ requestId: id, command })).status).toBe(400)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('refuses machine writes and receipt recovery before persistence and registers read tools only', async () => {
    const f = fixture(),
      bot = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' },
      tools = createCommerceTools(f.service, 'shop', bot)
    expect(() => f.service.mutate('shop', bot, mutation)).toThrow(CommerceError)
    expect(() => f.service.commandStatus('shop', bot, id)).toThrow(CommerceError)
    await tools.invoke('commerce.stock', { id })
    expect(f.repo.stock.mock.calls[0]?.[0]).toBe('shop')
    await expect(tools.invoke('commerce.stock', { id, tenant: 'other' })).rejects.toThrow()
    await expect(tools.invoke('commerce.fulfill', mutation.command)).rejects.toThrow(CommerceError)
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('enforces origin, login, bounded streams and strict query pages without reaching mutation', async () => {
    const f = fixture()
    expect((await f.post(mutation, { origin: 'https://evil.test' })).status).toBe(403)
    expect(
      (
        await f.api(
          new Request('https://app.test/api/commerce/shop/commands', {
            method: 'POST',
            headers: { cookie: 'session=fixture' },
            body: JSON.stringify(mutation),
          }),
        )
      ).status,
    ).toBe(403)
    const anonymous = createCommerceApi(f.service, () => Promise.resolve(null))
    expect(
      (await anonymous(new Request('https://app.test/api/commerce/shop/products'))).status,
    ).toBe(401)
    let cancelled = false
    const stream = new ReadableStream({
      start(c) {
        c.enqueue(new TextEncoder().encode('x'.repeat(18000)))
      },
      cancel() {
        cancelled = true
      },
    })
    const large = new Request('https://app.test/api/commerce/shop/commands', {
      method: 'POST',
      headers: { origin: 'https://app.test' },
      body: stream,
      duplex: 'half',
    } as RequestInit)
    expect((await f.api(large)).status).toBe(413)
    expect(cancelled).toBe(true)
    for (const path of [
      'products?active=no',
      'products?limit=101',
      'orders?scope=all',
      'products?active=true&active=false',
      'stock/' + id + '/movements?after=' + id,
    ])
      expect((await f.api(new Request('https://app.test/api/commerce/shop/' + path))).status).toBe(
        400,
      )
    expect(f.repo.mutate).not.toHaveBeenCalled()
  })
  it('returns opaque backend errors and current refusal codes without private SQL or credential values', async () => {
    const f = fixture()
    f.repo.mutate.mockRejectedValue(new Error('fixture-password and private SQL'))
    const response = await f.post(mutation)
    expect(response.status).toBe(503)
    expect(await response.text()).not.toContain('fixture-password')
    f.repo.mutate.mockRejectedValue(new CommerceError('conflict'))
    expect((await f.post(mutation)).status).toBe(409)
  })
})
