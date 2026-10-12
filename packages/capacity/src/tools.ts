import { z } from 'zod'
import {
  CapacityError,
  capacityPageSchema,
  capacityEstimateRequestSchema,
  type CapacityPrincipal,
} from './contracts.js'
import type { CapacityService } from './service.js'

export function createCapacityTools(
  service: CapacityService,
  tenant: string,
  principal: CapacityPrincipal,
) {
  return {
    async invoke(tool: string, input: unknown) {
      if (tool === 'capacity.listPools' || tool === 'capacity.pools') {
        const query = capacityPageSchema.parse(input ?? {})
        return await service.listPools(tenant, principal, query)
      }
      if (tool === 'capacity.detailPool') {
        const { poolId } = z.object({ poolId: z.uuid() }).strict().parse(input)
        return await service.detailPool(tenant, principal, poolId)
      }
      if (tool === 'capacity.estimate') {
        const req = capacityEstimateRequestSchema.parse(input)
        return await service.estimate(tenant, principal, req)
      }
      throw new CapacityError('not_found')
    },
  }
}
