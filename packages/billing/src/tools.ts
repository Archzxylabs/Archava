import { z } from 'zod'
import { BillingError, invoicePageQuerySchema, type BillingPrincipal } from './contracts.js'
import type { BillingService } from './service.js'

/**
 * Read-only tools.
 *
 * Every branch returns data and performs no mutation, and there is deliberately
 * no tool that records an attempt or an event: settlement is a write that
 * requires a signature, and a tool boundary is not a place to route one.
 */
export async function executeBillingTool(
  service: BillingService,
  tenant: string,
  principal: BillingPrincipal,
  tool: string,
  input: unknown,
): Promise<unknown> {
  try {
    switch (tool) {
      case 'billing.listInvoices': {
        const query = invoicePageQuerySchema.parse(input ?? {})
        return await service.listInvoices(tenant, principal, query)
      }
      case 'billing.invoiceDetail': {
        const { invoiceId } = z.object({ invoiceId: z.string().uuid() }).strict().parse(input)
        return await service.detail(tenant, principal, invoiceId)
      }
      case 'billing.lines': {
        const { invoiceId } = z.object({ invoiceId: z.string().uuid() }).strict().parse(input)
        return await service.lines(tenant, principal, invoiceId)
      }
      case 'billing.corrections': {
        const { invoiceId } = z.object({ invoiceId: z.string().uuid() }).strict().parse(input)
        return await service.corrections(tenant, principal, invoiceId)
      }
      case 'billing.attempts': {
        const { invoiceId } = z.object({ invoiceId: z.string().uuid() }).strict().parse(input)
        return await service.attempts(tenant, principal, invoiceId)
      }
      case 'billing.events': {
        const { invoiceId } = z.object({ invoiceId: z.string().uuid() }).strict().parse(input)
        return await service.events(tenant, principal, invoiceId)
      }
      default:
        throw new BillingError('not_found', `Unknown billing tool: ${tool}`)
    }
  } catch (error) {
    if (error instanceof z.ZodError) {
      throw new BillingError('bad_request', error.message)
    }
    throw error
  }
}
