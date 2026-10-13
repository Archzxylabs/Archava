import { z } from 'zod'
import { FulfillmentError, jobPageQuerySchema, type FulfillmentPrincipal } from './contracts.js'
import type { FulfillmentService } from './service.js'

export async function executeFulfillmentTool(
  service: FulfillmentService,
  tenant: string,
  principal: FulfillmentPrincipal,
  tool: string,
  input: unknown,
): Promise<unknown> {
  try {
    switch (tool) {
      case 'fulfillment.listJobs': {
        const query = jobPageQuerySchema.parse(input ?? {})
        return await service.listJobs(tenant, principal, query)
      }
      case 'fulfillment.jobDetail': {
        const { jobId } = z.object({ jobId: z.string().uuid() }).strict().parse(input)
        return await service.detail(tenant, principal, jobId)
      }
      case 'fulfillment.lines': {
        const { jobId } = z.object({ jobId: z.string().uuid() }).strict().parse(input)
        return await service.lines(tenant, principal, jobId)
      }
      case 'fulfillment.findings': {
        const { jobId } = z.object({ jobId: z.string().uuid() }).strict().parse(input)
        return await service.findings(tenant, principal, jobId)
      }
      case 'fulfillment.custody': {
        const { jobId } = z.object({ jobId: z.string().uuid() }).strict().parse(input)
        return await service.custody(tenant, principal, jobId)
      }
      default:
        throw new FulfillmentError('not_found', `Unknown fulfillment tool: ${tool}`)
    }
  } catch (error) {
    if (error instanceof z.ZodError) {
      throw new FulfillmentError('bad_request', error.message)
    }
    throw error
  }
}
