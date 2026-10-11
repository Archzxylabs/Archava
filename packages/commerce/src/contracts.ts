import { z } from 'zod'

export const commerceIdSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)
export const commercePrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: commerceIdSchema }).strict(),
  z
    .object({ kind: z.literal('service'), id: commerceIdSchema, onBehalfOf: commerceIdSchema })
    .strict(),
])
export type CommercePrincipal = z.infer<typeof commercePrincipalSchema>
export const COMMERCE_PERMISSIONS = [
  'catalog.read',
  'catalog.write',
  'stock.read',
  'stock.write',
  'orders.read',
  'orders.write',
  'returns.write',
  'audit.read',
] as const
export type CommercePermission = (typeof COMMERCE_PERMISSIONS)[number]
export const commerceMembershipSchema = z
  .object({
    principalId: commerceIdSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(COMMERCE_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export const commerceAccessSchema = z
  .object({
    permissions: z.array(z.enum(COMMERCE_PERMISSIONS)),
    ownerId: commerceIdSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type CommerceAccess = z.infer<typeof commerceAccessSchema>
const title = z.string().trim().min(1).max(160)
const reason = z.string().trim().min(1).max(2000)
const revision = z.number().int().positive().max(2147483647)
export const commercePriceSchema = z.number().int().min(0).max(1000000000)
export const commerceStockSchema = z.number().int().min(0).max(1000000000)
export const commerceUnitSchema = z.enum(['unit', 'gram', 'millilitre'])
export const productSchema = z
  .object({
    id: z.uuid(),
    tenantId: commerceIdSchema,
    sku: z.string().regex(/^[A-Z0-9][A-Z0-9_-]{0,39}$/),
    title,
    unit: commerceUnitSchema,
    trackStock: z.boolean(),
    price: commercePriceSchema,
    currency: z.literal('IDR'),
    active: z.boolean(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type Product = z.infer<typeof productSchema>
export const stockRecordSchema = z
  .object({
    productId: z.uuid(),
    onHand: commerceStockSchema,
    reserved: commerceStockSchema,
    available: commerceStockSchema,
    revision,
    evaluatedAt: z.iso.datetime(),
    authority: z.literal('snapshot-only'),
  })
  .strict()
export type StockRecord = z.infer<typeof stockRecordSchema>
export const orderLineInputSchema = z
  .object({ productId: z.uuid(), quantity: z.number().int().min(1).max(10000) })
  .strict()
export const orderLinesInputSchema = z
  .array(orderLineInputSchema)
  .min(1)
  .max(50)
  .superRefine((lines, ctx) => {
    if (new Set(lines.map((x) => x.productId)).size !== lines.length)
      ctx.addIssue({ code: 'custom', message: 'Products must be unique within an order' })
  })
export const orderSchema = z
  .object({
    id: z.uuid(),
    tenantId: commerceIdSchema,
    ownerId: commerceIdSchema,
    customerName: title,
    state: z.enum(['draft', 'reserved', 'fulfilled', 'cancelled', 'returned']),
    effectiveState: z.enum(['draft', 'reserved', 'expired', 'fulfilled', 'cancelled', 'returned']),
    totalEstimate: z.number().int().min(0).max(500000000000000),
    currency: z.literal('IDR'),
    expiresAt: z.iso.datetime().nullable(),
    fulfilledAt: z.iso.datetime().nullable(),
    managedBy: z.enum(['foodservice']).nullable(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type CommerceOrder = z.infer<typeof orderSchema>
export const orderLineSchema = z
  .object({
    id: z.uuid(),
    productId: z.uuid(),
    sku: z.string().min(1).max(40),
    title,
    unit: commerceUnitSchema,
    trackStock: z.boolean(),
    quantity: z.number().int().min(1).max(10000),
    price: commercePriceSchema,
    productRevision: revision,
    returnedQuantity: z.number().int().min(0).max(10000),
  })
  .strict()
export const stockMovementSchema = z
  .object({
    requestId: z.uuid(),
    productId: z.uuid(),
    orderId: z.uuid().nullable(),
    kind: z.enum(['adjust', 'consume', 'return', 'purchase']),
    purchaseId: z.uuid().nullable().optional(),
    quantity: z
      .number()
      .int()
      .min(-1000000000)
      .max(1000000000)
      .refine((x) => x !== 0),
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const orderReturnSchema = z
  .object({
    requestId: z.uuid(),
    lineId: z.uuid(),
    quantity: z.number().int().min(1).max(10000),
    disposition: z.enum(['restock', 'discard']),
    reason,
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const commerceAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: commerceIdSchema,
    operation: z.string().min(1).max(80),
    recordId: z.uuid(),
    revision,
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const orderDetailSchema = z
  .object({
    order: orderSchema,
    lines: z.array(orderLineSchema).min(1).max(50),
    returns: z.array(orderReturnSchema).max(50).nullable(),
    returnsHasMore: z.boolean(),
    audit: z.array(commerceAuditSchema).max(50).nullable(),
    auditHasMore: z.boolean(),
    evaluatedAt: z.iso.datetime(),
  })
  .strict()
export type OrderDetail = z.infer<typeof orderDetailSchema>
export const commerceCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('product.create'),
      sku: productSchema.shape.sku,
      title,
      unit: commerceUnitSchema.default('unit'),
      trackStock: z.boolean().default(true),
      price: commercePriceSchema,
    })
    .strict(),
  z
    .object({
      operation: z.literal('product.set-price'),
      id: z.uuid(),
      expectedRevision: revision,
      price: commercePriceSchema,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('product.set-active'),
      id: z.uuid(),
      expectedRevision: revision,
      active: z.boolean(),
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('stock.adjust'),
      productId: z.uuid(),
      expectedRevision: revision,
      quantity: z
        .number()
        .int()
        .min(-1000000000)
        .max(1000000000)
        .refine((x) => x !== 0),
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('order.create'),
      customerName: title,
      lines: orderLinesInputSchema,
    })
    .strict(),
  z
    .object({
      operation: z.literal('order.replace-lines'),
      id: z.uuid(),
      expectedRevision: revision,
      lines: orderLinesInputSchema,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('order.reserve'),
      id: z.uuid(),
      expectedRevision: revision,
      holdSeconds: z.number().int().min(1).max(1800).default(900),
    })
    .strict(),
  z
    .object({ operation: z.literal('order.fulfill'), id: z.uuid(), expectedRevision: revision })
    .strict(),
  z
    .object({
      operation: z.literal('order.cancel'),
      id: z.uuid(),
      expectedRevision: revision,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('order.return'),
      id: z.uuid(),
      expectedRevision: revision,
      lines: z
        .array(
          z
            .object({
              lineId: z.uuid(),
              quantity: z.number().int().min(1).max(10000),
              disposition: z.enum(['restock', 'discard']),
            })
            .strict(),
        )
        .min(1)
        .max(50),
      reason,
    })
    .strict()
    .superRefine((x, ctx) => {
      if (new Set(x.lines.map((l) => l.lineId)).size !== x.lines.length)
        ctx.addIssue({ code: 'custom', message: 'Return lines must be unique' })
    }),
])
export type CommerceCommand = z.infer<typeof commerceCommandSchema>
export const commerceMutationSchema = z
  .object({ requestId: z.uuid(), command: commerceCommandSchema })
  .strict()
export type CommerceMutation = z.infer<typeof commerceMutationSchema>
export const commerceReceiptSchema = z
  .object({ kind: z.enum(['product', 'stock', 'order']), id: z.uuid(), revision })
  .strict()
export type CommerceReceipt = z.infer<typeof commerceReceiptSchema>
export const commerceCommandStatusSchema = z.discriminatedUnion('status', [
  z
    .object({ status: z.literal('committed'), requestId: z.uuid(), receipt: commerceReceiptSchema })
    .strict(),
  z.object({ status: z.literal('not_committed'), requestId: z.uuid() }).strict(),
])
export type CommerceCommandStatus = z.infer<typeof commerceCommandStatusSchema>
export const commercePageSchema = z
  .object({ limit: z.number().int().min(1).max(100).default(20), after: z.uuid().optional() })
  .strict()
export type CommercePageQuery = z.infer<typeof commercePageSchema>
export const productPageSchema = commercePageSchema.extend({ active: z.boolean().optional() })
export type ProductPageQuery = z.infer<typeof productPageSchema>
export const orderPageSchema = commercePageSchema.extend({
  state: z.enum(['draft', 'reserved', 'fulfilled', 'cancelled', 'returned']).optional(),
})
export type OrderPageQuery = z.infer<typeof orderPageSchema>
export const stockHistoryQuerySchema = z
  .object({ limit: z.number().int().min(1).max(100).default(20) })
  .strict()
export type StockHistoryQuery = z.infer<typeof stockHistoryQuerySchema>
export interface CommercePage<T> {
  readonly items: readonly T[]
  readonly next: string | null
  readonly evaluatedAt: string
}
export interface CommerceRepository {
  access(tenant: string, principal: CommercePrincipal): Promise<CommerceAccess>
  products(
    tenant: string,
    principal: CommercePrincipal,
    query: ProductPageQuery,
  ): Promise<CommercePage<Product>>
  product(tenant: string, principal: CommercePrincipal, id: string): Promise<Product>
  stock(tenant: string, principal: CommercePrincipal, id: string): Promise<StockRecord>
  movements(
    tenant: string,
    principal: CommercePrincipal,
    id: string,
    query: StockHistoryQuery,
  ): Promise<{
    readonly items: readonly z.infer<typeof stockMovementSchema>[]
    readonly truncated: boolean
    readonly evaluatedAt: string
  }>
  orders(
    tenant: string,
    principal: CommercePrincipal,
    query: OrderPageQuery,
  ): Promise<CommercePage<CommerceOrder>>
  detail(tenant: string, principal: CommercePrincipal, id: string): Promise<OrderDetail>
  mutate(
    tenant: string,
    principal: CommercePrincipal,
    mutation: CommerceMutation,
  ): Promise<CommerceReceipt>
  commandStatus(
    tenant: string,
    principal: CommercePrincipal,
    id: string,
  ): Promise<CommerceCommandStatus>
}
export class CommerceError extends Error {
  constructor(readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable') {
    super('Commerce request refused.')
  }
}
export const COMMERCE_MANIFEST = Object.freeze({
  id: 'commerce',
  version: '0.1.0',
  apiVersion: '1.0.0',
  permissions: COMMERCE_PERMISSIONS,
  sourceOfTruth: 'tenant-owned catalog, orders and stock ledger',
  effects: 'human-authorized transactional commands',
  financialAuthority: false,
})
