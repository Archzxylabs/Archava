import { z } from 'zod'
import {
  FoodError,
  foodIdSchema,
  foodPrincipalSchema,
  foodPageSchema,
  ticketPageSchema,
  foodMutationSchema,
  type FoodPrincipal,
  type FoodRepository,
} from './contracts.js'
export class FoodService {
  constructor(private readonly repository: FoodRepository) {}
  private validate(tenant: string, principal: FoodPrincipal) {
    foodIdSchema.parse(tenant)
    foodPrincipalSchema.parse(principal)
  }
  access(tenant: string, principal: FoodPrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }
  tables(tenant: string, principal: FoodPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.tables(tenant, principal, foodPageSchema.parse(query))
  }
  menu(tenant: string, principal: FoodPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.menu(tenant, principal, foodPageSchema.parse(query))
  }
  menuDetail(tenant: string, principal: FoodPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.menuDetail(tenant, principal, z.uuid().parse(id))
  }
  tickets(tenant: string, principal: FoodPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.tickets(tenant, principal, ticketPageSchema.parse(query))
  }
  detail(tenant: string, principal: FoodPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }
  mutate(tenant: string, principal: FoodPrincipal, input: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new FoodError('forbidden')
    return this.repository.mutate(tenant, principal, foodMutationSchema.parse(input))
  }
  commandStatus(tenant: string, principal: FoodPrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new FoodError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
