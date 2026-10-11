import { z } from 'zod'
import {
  AccommodationError,
  accommodationIdSchema,
  accommodationPrincipalSchema,
  propertyPageSchema,
  accommodationPageSchema,
  roomAvailabilityQuerySchema,
  accommodationMutationSchema,
  type AccommodationPrincipal,
  type AccommodationRepository,
} from './contracts.js'

export class AccommodationService {
  constructor(private readonly repository: AccommodationRepository) {}
  private validate(tenant: string, principal: AccommodationPrincipal) {
    accommodationIdSchema.parse(tenant)
    accommodationPrincipalSchema.parse(principal)
  }
  access(tenant: string, principal: AccommodationPrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }
  properties(tenant: string, principal: AccommodationPrincipal, raw: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.properties(tenant, principal, propertyPageSchema.parse(raw))
  }
  property(tenant: string, principal: AccommodationPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.property(tenant, principal, z.uuid().parse(id))
  }
  roomTypes(tenant: string, principal: AccommodationPrincipal, raw: unknown) {
    this.validate(tenant, principal)
    return this.repository.roomTypes(tenant, principal, accommodationPageSchema.parse(raw))
  }
  rooms(tenant: string, principal: AccommodationPrincipal, raw: unknown) {
    this.validate(tenant, principal)
    return this.repository.rooms(tenant, principal, accommodationPageSchema.parse(raw))
  }
  room(tenant: string, principal: AccommodationPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.room(tenant, principal, z.uuid().parse(id))
  }
  stays(tenant: string, principal: AccommodationPrincipal, raw: unknown) {
    this.validate(tenant, principal)
    return this.repository.stays(tenant, principal, accommodationPageSchema.parse(raw))
  }
  detail(tenant: string, principal: AccommodationPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }
  availability(tenant: string, principal: AccommodationPrincipal, raw: unknown) {
    this.validate(tenant, principal)
    return this.repository.availability(tenant, principal, roomAvailabilityQuerySchema.parse(raw))
  }
  mutate(tenant: string, principal: AccommodationPrincipal, raw: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new AccommodationError('forbidden')
    return this.repository.mutate(tenant, principal, accommodationMutationSchema.parse(raw))
  }
  commandStatus(tenant: string, principal: AccommodationPrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new AccommodationError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
