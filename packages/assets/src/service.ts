import { z } from 'zod'
import {
  AssetError,
  assetTenantSchema,
  assetPrincipalSchema,
  assetPageSchema,
  assetMutationSchema,
  type AssetPrincipal,
  type AssetRepository,
} from './contracts.js'

export class AssetService {
  constructor(private readonly repository: AssetRepository) {}

  private validate(tenant: string, principal: AssetPrincipal) {
    assetTenantSchema.parse(tenant)
    assetPrincipalSchema.parse(principal)
  }

  access(tenant: string, principal: AssetPrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }

  listAssets(tenant: string, principal: AssetPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.listAssets(tenant, principal, assetPageSchema.parse(query))
  }

  detail(tenant: string, principal: AssetPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }

  listCustodyHistory(tenant: string, principal: AssetPrincipal, assetId: string) {
    this.validate(tenant, principal)
    return this.repository.listCustodyHistory(tenant, principal, z.uuid().parse(assetId))
  }

  listMaintenanceHistory(tenant: string, principal: AssetPrincipal, assetId: string) {
    this.validate(tenant, principal)
    return this.repository.listMaintenanceHistory(tenant, principal, z.uuid().parse(assetId))
  }

  mutate(tenant: string, principal: AssetPrincipal, input: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new AssetError('forbidden')
    return this.repository.mutate(tenant, principal, assetMutationSchema.parse(input))
  }

  commandStatus(tenant: string, principal: AssetPrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new AssetError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
