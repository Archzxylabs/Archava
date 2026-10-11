import { z } from 'zod'
import { PurchaseError, type PurchasePrincipal } from './contracts.js'
import type { PurchasingService } from './service.js'
export const PURCHASE_READ_TOOLS = Object.freeze(
  ['suppliers', 'offers', 'orders', 'detail'].map((name) =>
    Object.freeze({ name: 'purchasing.' + name, effect: 'read' }),
  ),
)
export function createPurchasingTools(
  service: PurchasingService,
  tenant: string,
  principal: PurchasePrincipal,
) {
  return {
    definitions: PURCHASE_READ_TOOLS,
    invoke(name: string, input: unknown) {
      if (name === 'purchasing.suppliers') return service.suppliers(tenant, principal, input)
      if (name === 'purchasing.offers') return service.offers(tenant, principal, input)
      if (name === 'purchasing.orders') return service.orders(tenant, principal, input)
      if (name === 'purchasing.detail')
        return service.detail(
          tenant,
          principal,
          z.object({ id: z.uuid() }).strict().parse(input).id,
        )
      throw new PurchaseError('not_found')
    },
  }
}
