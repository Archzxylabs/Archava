import { z } from 'zod'
import {
  capacityTenantSchema,
  capacityPrincipalSchema,
  capacityPageSchema,
  capacityEstimateRequestSchema,
  capacityMutationSchema,
  type CapacityRepository,
  type CapacityPrincipal,
  CapacityError,
} from './contracts.js'

export class CapacityService {
  constructor(private readonly repository: CapacityRepository) {}

  access(tenant: string, principal: CapacityPrincipal) {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    return this.repository.access(tenant, principal)
  }

  listPools(tenant: string, principal: CapacityPrincipal, query: unknown = {}) {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    return this.repository.listPools(tenant, principal, capacityPageSchema.parse(query))
  }

  detailPool(tenant: string, principal: CapacityPrincipal, poolId: string) {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    return this.repository.detailPool(tenant, principal, z.uuid().parse(poolId))
  }

  estimate(tenant: string, principal: CapacityPrincipal, raw: unknown) {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    return this.repository.estimate(tenant, principal, capacityEstimateRequestSchema.parse(raw))
  }

  mutate(tenant: string, principal: CapacityPrincipal, raw: unknown) {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    if (principal.kind !== 'human') {
      throw new CapacityError('forbidden', 'Capacity machine writes are not registered.')
    }
    return this.repository.mutate(tenant, principal, capacityMutationSchema.parse(raw))
  }

  commandStatus(tenant: string, principal: CapacityPrincipal, commandId: string) {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    if (principal.kind !== 'human') {
      throw new CapacityError('forbidden', 'Capacity machine reads on command status are not permitted.')
    }
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(commandId))
  }
}
