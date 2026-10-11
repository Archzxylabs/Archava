import { z } from 'zod'
import {
  CommerceError,
  commerceIdSchema,
  commercePrincipalSchema,
  productPageSchema,
  orderPageSchema,
  stockHistoryQuerySchema,
  commerceMutationSchema,
  type CommercePrincipal,
  type CommerceRepository,
} from './contracts.js'
export class CommerceService {
  constructor(private readonly repository: CommerceRepository) {}
  private validate(tenant: string, principal: CommercePrincipal) {
    commerceIdSchema.parse(tenant)
    commercePrincipalSchema.parse(principal)
  }
  access(tenant: string, principal: CommercePrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }
  products(tenant: string, principal: CommercePrincipal, raw: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.products(tenant, principal, productPageSchema.parse(raw))
  }
  product(tenant: string, principal: CommercePrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.product(tenant, principal, z.uuid().parse(id))
  }
  stock(tenant: string, principal: CommercePrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.stock(tenant, principal, z.uuid().parse(id))
  }
  movements(tenant: string, principal: CommercePrincipal, id: string, raw: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.movements(
      tenant,
      principal,
      z.uuid().parse(id),
      stockHistoryQuerySchema.parse(raw),
    )
  }
  orders(tenant: string, principal: CommercePrincipal, raw: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.orders(tenant, principal, orderPageSchema.parse(raw))
  }
  detail(tenant: string, principal: CommercePrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }
  mutate(tenant: string, principal: CommercePrincipal, raw: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new CommerceError('forbidden')
    return this.repository.mutate(tenant, principal, commerceMutationSchema.parse(raw))
  }
  commandStatus(tenant: string, principal: CommercePrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new CommerceError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
