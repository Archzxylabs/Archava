import { z } from 'zod'
import { OrganizationError, type OrganizationPrincipal } from './contracts.js'
import type { OrganizationService } from './service.js'

export const ORGANIZATION_READ_TOOLS = Object.freeze(
  ['list', 'detail', 'affiliations'].map((name) =>
    Object.freeze({ name: 'organizations.' + name, effect: 'read' }),
  ),
)

export function createOrganizationTools(
  service: OrganizationService,
  tenant: string,
  principal: OrganizationPrincipal,
) {
  return {
    definitions: ORGANIZATION_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'organizations.list') {
        return service.listOrganizations(tenant, principal, input)
      }
      if (name === 'organizations.detail') {
        const { id } = z.object({ id: z.uuid() }).strict().parse(input)
        return service.detail(tenant, principal, id)
      }
      if (name === 'organizations.affiliations') {
        const { organizationId, ...rest } = z
          .object({
            organizationId: z.uuid(),
            limit: z.coerce.number().int().min(1).max(100).optional(),
            after: z.uuid().optional(),
            status: z.enum(['active', 'inactive']).optional(),
            role: z.string().trim().max(80).optional(),
          })
          .strict()
          .parse(input)
        return service.listAffiliations(tenant, principal, organizationId, rest)
      }
      throw new OrganizationError('not_found')
    },
  }
}
