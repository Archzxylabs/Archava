import { z } from 'zod'
import {
  PurchaseError,
  purchaseIdSchema,
  purchasePrincipalSchema,
  purchasePageSchema,
  purchaseOrderPageSchema,
  purchaseMutationSchema,
  type PurchasePrincipal,
  type PurchaseRepository,
} from './contracts.js'
export class PurchasingService {
  constructor(private readonly repository: PurchaseRepository) {}
  private validate(t: string, p: PurchasePrincipal) {
    purchaseIdSchema.parse(t)
    purchasePrincipalSchema.parse(p)
  }
  access(t: string, p: PurchasePrincipal) {
    this.validate(t, p)
    return this.repository.access(t, p)
  }
  suppliers(t: string, p: PurchasePrincipal, q: unknown = {}) {
    this.validate(t, p)
    return this.repository.suppliers(t, p, purchasePageSchema.parse(q))
  }
  offers(t: string, p: PurchasePrincipal, q: unknown = {}) {
    this.validate(t, p)
    return this.repository.offers(t, p, purchasePageSchema.parse(q))
  }
  orders(t: string, p: PurchasePrincipal, q: unknown = {}) {
    this.validate(t, p)
    return this.repository.orders(t, p, purchaseOrderPageSchema.parse(q))
  }
  detail(t: string, p: PurchasePrincipal, id: string) {
    this.validate(t, p)
    return this.repository.detail(t, p, z.uuid().parse(id))
  }
  mutate(t: string, p: PurchasePrincipal, v: unknown) {
    this.validate(t, p)
    if (p.kind !== 'human') throw new PurchaseError('forbidden')
    return this.repository.mutate(t, p, purchaseMutationSchema.parse(v))
  }
  commandStatus(t: string, p: PurchasePrincipal, id: string) {
    this.validate(t, p)
    if (p.kind !== 'human') throw new PurchaseError('forbidden')
    return this.repository.commandStatus(t, p, z.uuid().parse(id))
  }
}
