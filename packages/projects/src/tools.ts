import { z } from 'zod'
import { ProjectError, projectPageSchema, type ProjectPrincipal } from './contracts.js'
import type { ProjectService } from './service.js'

export const PROJECT_READ_TOOLS = Object.freeze([
  Object.freeze({ name: 'projects.list', permission: 'projects.read', effect: 'read' }),
  Object.freeze({ name: 'projects.detail', permission: 'projects.read', effect: 'read' }),
])
/** Trusted host binds identity and tenant. Model input has no identity or permission fields. */
export function createProjectTools(
  service: ProjectService,
  tenant: string,
  principal: ProjectPrincipal,
) {
  return {
    definitions: PROJECT_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'projects.list')
        return await service.list(tenant, principal, projectPageSchema.parse(input))
      if (name === 'projects.detail') {
        const query = z.object({ id: z.uuid() }).strict().parse(input)
        return await service.detail(tenant, principal, query.id)
      }
      throw new ProjectError('not_found', 'Unknown project tool.')
    },
  }
}
