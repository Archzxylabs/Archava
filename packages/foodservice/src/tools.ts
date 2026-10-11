import { z } from 'zod'
import { FoodError, type FoodPrincipal } from './contracts.js'
import type { FoodService } from './service.js'
export const FOOD_READ_TOOLS = Object.freeze(
  ['tables', 'menu', 'menuDetail', 'tickets', 'detail'].map((name) =>
    Object.freeze({ name: 'foodservice.' + name, effect: 'read' }),
  ),
)
export function createFoodTools(service: FoodService, tenant: string, principal: FoodPrincipal) {
  return {
    definitions: FOOD_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'foodservice.tables') return service.tables(tenant, principal, input)
      if (name === 'foodservice.menu') return service.menu(tenant, principal, input)
      if (name === 'foodservice.tickets') return service.tickets(tenant, principal, input)
      if (name === 'foodservice.menuDetail' || name === 'foodservice.detail') {
        const { id } = z.object({ id: z.uuid() }).strict().parse(input)
        return name === 'foodservice.menuDetail'
          ? service.menuDetail(tenant, principal, id)
          : service.detail(tenant, principal, id)
      }
      throw new FoodError('not_found')
    },
  }
}
