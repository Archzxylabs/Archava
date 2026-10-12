import { z } from 'zod'
import { AssetError, type AssetPrincipal } from './contracts.js'
import type { AssetService } from './service.js'

export const ASSET_READ_TOOLS = Object.freeze(
  ['list', 'detail', 'custodyHistory', 'maintenanceHistory', 'search'].map((name) =>
    Object.freeze({ name: 'assets.' + name, effect: 'read' }),
  ),
)

export function createAssetTools(service: AssetService, tenant: string, principal: AssetPrincipal) {
  return {
    definitions: ASSET_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'assets.list') {
        return service.listAssets(tenant, principal, input)
      }
      if (name === 'assets.search') {
        const { query } = z
          .object({
            query: z.string().trim().max(100).optional(),
          })
          .strict()
          .parse(input)
        return service.listAssets(tenant, principal, query ? { search: query } : {})
      }
      if (name === 'assets.detail') {
        const { id } = z.object({ id: z.uuid() }).strict().parse(input)
        return service.detail(tenant, principal, id)
      }
      if (name === 'assets.custodyHistory') {
        const { assetId } = z.object({ assetId: z.uuid() }).strict().parse(input)
        return service.listCustodyHistory(tenant, principal, assetId)
      }
      if (name === 'assets.maintenanceHistory') {
        const { assetId } = z.object({ assetId: z.uuid() }).strict().parse(input)
        return service.listMaintenanceHistory(tenant, principal, assetId)
      }
      throw new AssetError('not_found')
    },
  }
}
