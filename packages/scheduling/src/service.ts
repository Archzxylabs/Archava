import { z } from 'zod'
import {
  SchedulingError,
  schedulingIdSchema,
  schedulingPrincipalSchema,
  resourcePageSchema,
  reservationPageSchema,
  availabilityQuerySchema,
  schedulingMutationSchema,
  type SchedulingPrincipal,
  type SchedulingRepository,
} from './contracts.js'

export class SchedulingService {
  constructor(private readonly repository: SchedulingRepository) {}
  private validate(tenant: string, principal: SchedulingPrincipal) {
    schedulingIdSchema.parse(tenant)
    schedulingPrincipalSchema.parse(principal)
  }
  access(tenant: string, principal: SchedulingPrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }
  resources(tenant: string, principal: SchedulingPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.resources(tenant, principal, resourcePageSchema.parse(query))
  }
  resource(tenant: string, principal: SchedulingPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.resource(tenant, principal, z.uuid().parse(id))
  }
  reservations(tenant: string, principal: SchedulingPrincipal, query: unknown) {
    this.validate(tenant, principal)
    return this.repository.reservations(tenant, principal, reservationPageSchema.parse(query))
  }
  detail(tenant: string, principal: SchedulingPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }
  availability(tenant: string, principal: SchedulingPrincipal, query: unknown) {
    this.validate(tenant, principal)
    return this.repository.availability(tenant, principal, availabilityQuerySchema.parse(query))
  }
  mutate(tenant: string, principal: SchedulingPrincipal, raw: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new SchedulingError('forbidden')
    return this.repository.mutate(tenant, principal, schedulingMutationSchema.parse(raw))
  }
  commandStatus(tenant: string, principal: SchedulingPrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new SchedulingError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
