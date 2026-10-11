import { z } from 'zod'
export const purchaseIdSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)
export const purchasePrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: purchaseIdSchema }).strict(),
  z
    .object({ kind: z.literal('service'), id: purchaseIdSchema, onBehalfOf: purchaseIdSchema })
    .strict(),
])
export type PurchasePrincipal = z.infer<typeof purchasePrincipalSchema>
export const PURCHASE_PERMISSIONS = [
  'suppliers.read',
  'suppliers.write',
  'offers.read',
  'offers.write',
  'orders.read',
  'orders.write',
  'receipts.read',
  'receipts.write',
  'audit.read',
] as const
export type PurchasePermission = (typeof PURCHASE_PERMISSIONS)[number]
export const purchaseMembershipSchema = z
  .object({
    principalId: purchaseIdSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(PURCHASE_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export const purchaseAccessSchema = purchaseMembershipSchema
  .pick({ permissions: true, scope: true })
  .extend({ ownerId: purchaseIdSchema })
  .strict()
export type PurchaseAccess = z.infer<typeof purchaseAccessSchema>
const title = z.string().trim().min(1).max(160),
  reason = z.string().trim().min(1).max(2000),
  revision = z.number().int().positive().max(2147483647)
export const purchaseCostSchema = z.number().int().min(0).max(1000000000)
export const purchaseQuantitySchema = z.number().int().min(1).max(1000000)
export const supplierSchema = z
  .object({
    id: z.uuid(),
    tenantId: purchaseIdSchema,
    code: z.string().regex(/^[A-Z0-9][A-Z0-9_-]{0,39}$/),
    name: title,
    active: z.boolean(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type Supplier = z.infer<typeof supplierSchema>
export const offerSchema = z
  .object({
    id: z.uuid(),
    tenantId: purchaseIdSchema,
    supplierId: z.uuid(),
    productId: z.uuid(),
    sku: z.string(),
    title,
    unit: z.enum(['unit', 'gram', 'millilitre']),
    unitCost: purchaseCostSchema,
    currency: z.literal('IDR'),
    active: z.boolean(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type SupplierOffer = z.infer<typeof offerSchema>
export const PURCHASE_STATES = [
  'draft',
  'confirmed',
  'partial',
  'received',
  'cancelled',
  'short_closed',
] as const
export const purchaseOrderSchema = z
  .object({
    id: z.uuid(),
    tenantId: purchaseIdSchema,
    ownerId: purchaseIdSchema,
    supplierId: z.uuid(),
    supplierCode: z.string(),
    supplierName: title,
    state: z.enum(PURCHASE_STATES),
    totalEstimate: z.number().int().min(0).max(500000000000000),
    currency: z.literal('IDR'),
    confirmedAt: z.iso.datetime().nullable(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type PurchaseOrder = z.infer<typeof purchaseOrderSchema>
export const purchaseLineSchema = z
  .object({
    id: z.uuid(),
    offerId: z.uuid(),
    offerRevision: revision,
    productId: z.uuid(),
    sku: z.string(),
    title,
    unit: z.enum(['unit', 'gram', 'millilitre']),
    quantity: purchaseQuantitySchema,
    unitCost: purchaseCostSchema,
    acceptedQuantity: z.number().int().min(0).max(1000000),
    rejectedQuantity: z.number().int().min(0).max(1000000),
  })
  .strict()
export const goodsReceiptSchema = z
  .object({
    requestId: z.uuid(),
    orderId: z.uuid(),
    lineId: z.uuid(),
    productId: z.uuid(),
    accepted: z.number().int().min(0).max(1000000),
    rejected: z.number().int().min(0).max(1000000),
    reason,
    actorId: purchaseIdSchema,
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const purchaseAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: purchaseIdSchema,
    operation: z.string().min(1).max(100),
    recordId: z.uuid(),
    revision,
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const purchaseDetailSchema = z
  .object({
    order: purchaseOrderSchema,
    lines: z.array(purchaseLineSchema).max(50),
    receipts: z.array(goodsReceiptSchema).max(100).nullable(),
    receiptsHasMore: z.boolean(),
    audit: z.array(purchaseAuditSchema).max(100).nullable(),
    auditHasMore: z.boolean(),
  })
  .strict()
export type PurchaseDetail = z.infer<typeof purchaseDetailSchema>
export const purchaseLinesInputSchema = z
  .array(z.object({ offerId: z.uuid(), quantity: purchaseQuantitySchema }).strict())
  .min(1)
  .max(50)
  .superRefine((v, c) => {
    if (new Set(v.map((x) => x.offerId)).size !== v.length)
      c.addIssue({ code: 'custom', message: 'Offers must be unique' })
  })
const update = { id: z.uuid(), expectedRevision: revision, reason }
export const goodsReceiptInputSchema = z
  .array(
    z
      .object({
        lineId: z.uuid(),
        accepted: z.number().int().min(0).max(1000000),
        rejected: z.number().int().min(0).max(1000000),
      })
      .strict()
      .refine((x) => x.accepted + x.rejected > 0 && x.accepted + x.rejected <= 1000000),
  )
  .min(1)
  .max(50)
  .superRefine((v, c) => {
    if (new Set(v.map((x) => x.lineId)).size !== v.length)
      c.addIssue({ code: 'custom', message: 'Receipt lines must be unique' })
  })
export const purchaseCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('supplier.create'),
      code: supplierSchema.shape.code,
      name: title,
    })
    .strict(),
  z
    .object({ operation: z.literal('supplier.set-active'), ...update, active: z.boolean() })
    .strict(),
  z
    .object({
      operation: z.literal('offer.create'),
      supplierId: z.uuid(),
      productId: z.uuid(),
      unitCost: purchaseCostSchema,
    })
    .strict(),
  z
    .object({ operation: z.literal('offer.set-cost'), ...update, unitCost: purchaseCostSchema })
    .strict(),
  z.object({ operation: z.literal('offer.set-active'), ...update, active: z.boolean() }).strict(),
  z
    .object({
      operation: z.literal('order.create'),
      supplierId: z.uuid(),
      lines: purchaseLinesInputSchema,
    })
    .strict(),
  z
    .object({
      operation: z.literal('order.replace-lines'),
      ...update,
      lines: purchaseLinesInputSchema,
    })
    .strict(),
  z.object({ operation: z.literal('order.confirm'), ...update }).strict(),
  z.object({ operation: z.literal('order.cancel'), ...update }).strict(),
  z.object({ operation: z.literal('order.short-close'), ...update }).strict(),
  z
    .object({ operation: z.literal('order.receive'), ...update, lines: goodsReceiptInputSchema })
    .strict(),
])
export type PurchaseCommand = z.infer<typeof purchaseCommandSchema>
export const purchaseMutationSchema = z
  .object({ requestId: z.uuid(), command: purchaseCommandSchema })
  .strict()
export type PurchaseMutation = z.infer<typeof purchaseMutationSchema>
export const purchaseReceiptSchema = z
  .object({
    requestId: z.uuid(),
    operation: purchaseCommandSchema.options[0].shape.operation.or(
      z.enum([
        'supplier.set-active',
        'offer.create',
        'offer.set-cost',
        'offer.set-active',
        'order.create',
        'order.replace-lines',
        'order.confirm',
        'order.cancel',
        'order.short-close',
        'order.receive',
      ]),
    ),
    id: z.uuid(),
    revision,
    replayed: z.boolean(),
  })
  .strict()
export type PurchaseReceipt = z.infer<typeof purchaseReceiptSchema>
export const purchaseStatusSchema = z.discriminatedUnion('state', [
  z.object({ state: z.literal('unknown') }).strict(),
  z.object({ state: z.literal('committed'), receipt: purchaseReceiptSchema }).strict(),
])
export type PurchaseCommandStatus = z.infer<typeof purchaseStatusSchema>
export const purchasePageSchema = z
  .object({
    after: z.uuid().optional(),
    limit: z.number().int().min(1).max(100).default(50),
    active: z.boolean().optional(),
    supplierId: z.uuid().optional(),
  })
  .strict()
export type PurchasePageQuery = z.infer<typeof purchasePageSchema>
export const purchaseOrderPageSchema = purchasePageSchema
  .omit({ active: true })
  .extend({ state: z.enum(PURCHASE_STATES).optional() })
  .strict()
export type PurchaseOrderPageQuery = z.infer<typeof purchaseOrderPageSchema>
export type PurchasePage<T> = { readonly items: readonly T[]; readonly next: string | null }
export class PurchaseError extends Error {
  constructor(readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable') {
    super(code)
    this.name = 'PurchaseError'
  }
}
export interface PurchaseRepository {
  access(tenant: string, principal: PurchasePrincipal): Promise<PurchaseAccess>
  suppliers(
    tenant: string,
    principal: PurchasePrincipal,
    query: PurchasePageQuery,
  ): Promise<PurchasePage<Supplier>>
  offers(
    tenant: string,
    principal: PurchasePrincipal,
    query: PurchasePageQuery,
  ): Promise<PurchasePage<SupplierOffer>>
  orders(
    tenant: string,
    principal: PurchasePrincipal,
    query: PurchaseOrderPageQuery,
  ): Promise<PurchasePage<PurchaseOrder>>
  detail(tenant: string, principal: PurchasePrincipal, id: string): Promise<PurchaseDetail>
  mutate(
    tenant: string,
    principal: PurchasePrincipal,
    input: PurchaseMutation,
  ): Promise<PurchaseReceipt>
  commandStatus(
    tenant: string,
    principal: PurchasePrincipal,
    id: string,
  ): Promise<PurchaseCommandStatus>
}
