import { z } from 'zod'
import { SupportError, type SupportPrincipal } from './contracts.js'
import type { SupportService } from './service.js'
export const SUPPORT_READ_TOOLS = Object.freeze(
  ['cases', 'detail'].map((name) => Object.freeze({ name: 'support.' + name, effect: 'read' })),
)
export function createSupportTools(
  service: SupportService,
  tenant: string,
  principal: SupportPrincipal,
) {
  return {
    definitions: SUPPORT_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'support.cases') return service.cases(tenant, principal, input)
      if (name === 'support.detail') {
        const { id } = z.object({ id: z.uuid() }).strict().parse(input)
        return service.detail(tenant, principal, id)
      }
      throw new SupportError('not_found')
    },
  }
}
