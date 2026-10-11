import { z } from 'zod'
import { AccommodationError, type AccommodationPrincipal } from './contracts.js'
import type { AccommodationService } from './service.js'
export const ACCOMMODATION_READ_TOOLS = Object.freeze(
  ['properties', 'room-types', 'rooms', 'stays', 'availability', 'detail'].map((name) =>
    Object.freeze({
      name: 'accommodation.' + name,
      effect: 'read',
      permission:
        name === 'properties'
          ? 'properties.read'
          : name === 'stays' || name === 'detail'
            ? 'stays.read'
            : 'rooms.read',
    }),
  ),
)
export function createAccommodationTools(
  service: AccommodationService,
  tenant: string,
  principal: AccommodationPrincipal,
) {
  return {
    definitions: ACCOMMODATION_READ_TOOLS,
    async invoke(name: string, input: unknown) {
      if (name === 'accommodation.properties')
        return await service.properties(tenant, principal, input)
      if (name === 'accommodation.room-types')
        return await service.roomTypes(tenant, principal, input)
      if (name === 'accommodation.rooms') return await service.rooms(tenant, principal, input)
      if (name === 'accommodation.stays') return await service.stays(tenant, principal, input)
      if (name === 'accommodation.availability')
        return await service.availability(tenant, principal, input)
      if (name === 'accommodation.detail')
        return await service.detail(
          tenant,
          principal,
          z.object({ id: z.uuid() }).strict().parse(input).id,
        )
      throw new AccommodationError('not_found')
    },
  }
}
