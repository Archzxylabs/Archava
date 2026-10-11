import { z } from 'zod'
import { CommerceError, type CommercePrincipal } from './contracts.js'
import type { CommerceService } from './service.js'
export const COMMERCE_READ_TOOLS = Object.freeze(
  ['products', 'product', 'stock', 'orders', 'detail'].map((name) =>
    Object.freeze({ name: 'commerce.' + name, effect: 'read' }),
  ),
)
export function createCommerceTools(
  service: CommerceService,
  tenant: string,
  principal: CommercePrincipal,
) {
  return {
    definitions: COMMERCE_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'commerce.products') return service.products(tenant, principal, input)
      if (name === 'commerce.orders') return service.orders(tenant, principal, input)
      if (name === 'commerce.product' || name === 'commerce.stock' || name === 'commerce.detail') {
        const { id } = z.object({ id: z.uuid() }).strict().parse(input)
        if (name === 'commerce.product') return service.product(tenant, principal, id)
        if (name === 'commerce.stock') return service.stock(tenant, principal, id)
        return service.detail(tenant, principal, id)
      }
      throw new CommerceError('not_found')
    },
  }
}
