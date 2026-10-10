import { z } from 'zod'
import { SchedulingError, type SchedulingPrincipal } from './contracts.js'
import type { SchedulingService } from './service.js'

export const SCHEDULING_READ_TOOLS = Object.freeze([
  Object.freeze({ name: 'scheduling.resources', permission: 'resources.read', effect: 'read' }),
  Object.freeze({ name: 'scheduling.availability', permission: 'resources.read', effect: 'read' }),
  Object.freeze({
    name: 'scheduling.reservations',
    permission: 'reservations.read',
    effect: 'read',
  }),
  Object.freeze({ name: 'scheduling.detail', permission: 'reservations.read', effect: 'read' }),
])
export function createSchedulingTools(
  service: SchedulingService,
  tenant: string,
  principal: SchedulingPrincipal,
) {
  return {
    definitions: SCHEDULING_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'scheduling.resources') return await service.resources(tenant, principal, input)
      if (name === 'scheduling.availability')
        return await service.availability(tenant, principal, input)
      if (name === 'scheduling.reservations')
        return await service.reservations(tenant, principal, input)
      if (name === 'scheduling.detail')
        return await service.detail(
          tenant,
          principal,
          z.object({ id: z.uuid() }).strict().parse(input).id,
        )
      throw new SchedulingError('not_found')
    },
  }
}
