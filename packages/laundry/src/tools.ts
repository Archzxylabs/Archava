import { z } from 'zod'
import { LaundryError, orderPageQuerySchema, type LaundryPrincipal } from './contracts.js'
import type { LaundryService } from './service.js'

export async function executeLaundryTool(
  service: LaundryService,
  tenant: string,
  principal: LaundryPrincipal,
  tool: string,
  input: unknown,
): Promise<unknown> {
  try {
    switch (tool) {
      case 'laundry.listOrders': {
        const query = orderPageQuerySchema.parse(input ?? {})
        return await service.listOrders(tenant, principal, query)
      }
      case 'laundry.orderDetail': {
        const { orderId } = z.object({ orderId: z.string().uuid() }).strict().parse(input)
        return await service.detail(tenant, principal, orderId)
      }
      case 'laundry.process': {
        const { orderId } = z.object({ orderId: z.string().uuid() }).strict().parse(input)
        return await service.process(tenant, principal, orderId)
      }
      case 'laundry.findings': {
        const { orderId } = z.object({ orderId: z.string().uuid() }).strict().parse(input)
        return await service.findings(tenant, principal, orderId)
      }
      default:
        throw new LaundryError('not_found', `Unknown laundry tool: ${tool}`)
    }
  } catch (error) {
    if (error instanceof z.ZodError) {
      throw new LaundryError('bad_request', error.message)
    }
    throw error
  }
}
