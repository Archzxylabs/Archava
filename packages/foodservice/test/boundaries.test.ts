import { describe, it, expect, vi } from 'vitest'
import {
  FoodService,
  FoodError,
  FOOD_PERMISSIONS,
  foodMutationSchema,
  createFoodApi,
  createFoodTools,
  type FoodRepository,
} from '../src/index.js'
const id = '10000000-0000-4000-8000-000000000001',
  human = { kind: 'human' as const, id: 'owner' },
  bot = { kind: 'service' as const, id: 'bot', onBehalfOf: 'owner' }
const input = (command: unknown) => ({ requestId: id, command })
function repository() {
  const unused = () => Promise.reject(new Error('PRIVATE database details'))
  return {
    access: () =>
      Promise.resolve({ permissions: [...FOOD_PERMISSIONS], ownerId: 'owner', scope: 'all' }),
    tables: unused,
    menu: unused,
    menuDetail: unused,
    tickets: unused,
    detail: unused,
    mutate: vi.fn(unused),
    commandStatus: vi.fn(unused),
  } satisfies FoodRepository
}
describe('restaurant execution boundaries', () => {
  it('refuses model-supplied ticket prices, stock overrides, payment flags and actor identity before persistence', () => {
    const repo = repository(),
      service = new FoodService(repo),
      create = {
        operation: 'ticket.create',
        customerName: 'Guest',
        mode: 'takeaway',
        lines: [{ menuId: id, quantity: 1 }],
      }
    for (const extra of [
      { totalEstimate: 0 },
      { paid: true },
      { stockAvailable: true },
      { ownerId: 'someone' },
      { recipeVersion: 999 },
      { refund: 100 },
    ])
      expect(() => service.mutate('restaurant', human, input({ ...create, ...extra }))).toThrow()
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('requires a table only for dine-in and uses bounded integer base quantities for recipes and portions', () => {
    const create = {
      operation: 'ticket.create',
      customerName: 'Guest',
      mode: 'dine_in',
      lines: [{ menuId: id, quantity: 1 }],
    }
    expect(foodMutationSchema.safeParse(input(create)).success).toBe(false)
    expect(foodMutationSchema.safeParse(input({ ...create, tableId: id })).success).toBe(true)
    expect(
      foodMutationSchema.safeParse(input({ ...create, mode: 'takeaway', tableId: id })).success,
    ).toBe(false)
    for (const quantity of [0, -1, 1.5, 101])
      expect(
        foodMutationSchema.safeParse(
          input({ ...create, tableId: id, lines: [{ menuId: id, quantity }] }),
        ).success,
      ).toBe(false)
    for (const quantity of [0, 1.5, 1000001])
      expect(
        foodMutationSchema.safeParse(
          input({
            operation: 'menu.create',
            sku: 'MEAL',
            title: 'Meal',
            price: 1000,
            recipe: [{ productId: id, quantity }],
          }),
        ).success,
      ).toBe(false)
  })
  it('refuses duplicate menu/ingredient entries and unaudited cancellation or recipe changes', () => {
    expect(
      foodMutationSchema.safeParse(
        input({
          operation: 'ticket.create',
          customerName: 'Guest',
          mode: 'takeaway',
          lines: [
            { menuId: id, quantity: 1 },
            { menuId: id, quantity: 1 },
          ],
        }),
      ).success,
    ).toBe(false)
    expect(
      foodMutationSchema.safeParse(
        input({
          operation: 'menu.create',
          sku: 'MEAL',
          title: 'Meal',
          price: 100,
          recipe: [
            { productId: id, quantity: 1 },
            { productId: id, quantity: 1 },
          ],
        }),
      ).success,
    ).toBe(false)
    for (const operation of ['ticket.cancel', 'menu.replace-recipe'])
      expect(
        foodMutationSchema.safeParse(input({ operation, id, expectedRevision: 1 })).success,
      ).toBe(false)
  })
  it('machine tools grant no write or receipt recovery and refuse caller-supplied authority', async () => {
    const repo = repository(),
      service = new FoodService(repo),
      tools = createFoodTools(service, 'restaurant', bot)
    expect(() =>
      service.mutate(
        'restaurant',
        bot,
        input({ operation: 'ticket.ready', id, expectedRevision: 1 }),
      ),
    ).toThrow(FoodError)
    expect(() => service.commandStatus('restaurant', bot, id)).toThrow(FoodError)
    await expect(tools.invoke('foodservice.ticket.prepare', { id })).rejects.toMatchObject({
      code: 'not_found',
    })
    await expect(
      tools.invoke('foodservice.detail', { id, tenant: 'another', scope: 'all' }),
    ).rejects.toThrow()
    expect(tools.definitions.every((x) => x.effect === 'read')).toBe(true)
    expect(repo.mutate).not.toHaveBeenCalled()
  })
  it('API requires identity/origin, rejects repeated or unknown query fields and limits streamed bodies', async () => {
    const service = new FoodService(repository()),
      api = createFoodApi(service, () => Promise.resolve(human)),
      url = 'http://localhost/api/foodservice/restaurant/'
    expect(
      (await createFoodApi(service, () => Promise.resolve(null))(new Request(url + 'access')))
        .status,
    ).toBe(401)
    for (const suffix of [
      'tickets?scope=all',
      'tickets?limit=1&limit=2',
      'menu?active=1',
      'access?actor=owner',
    ])
      expect((await api(new Request(url + suffix))).status).toBe(400)
    for (const headers of [
      new Headers({ cookie: 'session=token' }),
      new Headers({ origin: 'http://attacker.example' }),
    ])
      expect(
        (await api(new Request(url + 'commands', { method: 'POST', headers, body: '{}' }))).status,
      ).toBe(403)
    expect(
      (await api(new Request(url + 'commands', { method: 'POST', body: ' '.repeat(17000) })))
        .status,
    ).toBe(413)
  })
  it('returns opaque dependency failures without private database details', async () => {
    const api = createFoodApi(new FoodService(repository()), () => Promise.resolve(human)),
      response = await api(new Request('http://localhost/api/foodservice/restaurant/tickets'))
    expect(response.status).toBe(503)
    expect(await response.json()).toEqual({ error: 'unavailable' })
  })
})
