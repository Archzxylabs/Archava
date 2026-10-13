import { z } from 'zod'

export const laundryTenantSchema = z.string().trim().min(1).max(100)

export const laundryPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: laundryTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: laundryTenantSchema,
      onBehalfOf: laundryTenantSchema,
    })
    .strict(),
])
export type LaundryPrincipal = z.infer<typeof laundryPrincipalSchema>

export const LAUNDRY_PERMISSIONS = [
  'laundry.read',
  'laundry.write',
  'laundry.process',
  'audit.read',
] as const
export type LaundryPermission = (typeof LAUNDRY_PERMISSIONS)[number]

export const laundryMembershipSchema = z
  .object({
    principalId: laundryTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(LAUNDRY_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type LaundryMembership = z.infer<typeof laundryMembershipSchema>

export const laundryAccessSchema = z
  .object({
    permissions: z.array(z.enum(LAUNDRY_PERMISSIONS)),
    ownerId: laundryTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type LaundryAccess = z.infer<typeof laundryAccessSchema>

/** Weight-based (kilo) and item-based (garment) intake share one lifecycle. */
export const ORDER_KINDS = ['weight', 'item'] as const
export type OrderKind = (typeof ORDER_KINDS)[number]

export const ORDER_STATUSES = ['received', 'processing', 'ready', 'returned', 'canceled'] as const
export type OrderStatus = (typeof ORDER_STATUSES)[number]

/** `gram` keeps weight quantities integer; `piece` counts garments. */
export const LINE_UNITS = ['piece', 'gram'] as const
export type LineUnit = (typeof LINE_UNITS)[number]

export const PROCESS_EVENTS = [
  'intake',
  'tag',
  'wash',
  'dry',
  'press',
  'quality_check',
  'ready',
  'handover',
] as const
export type ProcessEvent = (typeof PROCESS_EVENTS)[number]

/** Ordered processing chain; each recorded step must strictly advance it. */
export const PROCESS_STAGE: Readonly<Record<string, number>> = Object.freeze({
  intake: 0,
  tag: 1,
  wash: 2,
  dry: 3,
  press: 4,
  quality_check: 5,
  ready: 6,
  handover: 7,
})

/** Machine-side events that require the dedicated `laundry.process` grant. */
export const MACHINE_EVENTS: ReadonlySet<string> = new Set(['wash', 'dry', 'press'])

export const FINDING_CATEGORIES = ['stain', 'damage', 'missing', 'count'] as const
export type FindingCategory = (typeof FINDING_CATEGORIES)[number]

export const FINDING_DISPOSITIONS = ['accepted', 'partial', 'rejected'] as const
export type FindingDisposition = (typeof FINDING_DISPOSITIONS)[number]

/** Findings that block `ready` until an accepted reconciliation clears them. */
export const EXCEPTION_DISPOSITIONS = ['partial', 'rejected'] as const

export const currencySchema = z.string().regex(/^[A-Z]{3}$/, 'Currency must be a 3-letter ISO code')

export const orderLineSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: laundryTenantSchema,
    orderId: z.string().uuid(),
    itemRef: z.string().trim().min(1).max(80),
    quantity: z.number().int().positive(),
    unit: z.enum(LINE_UNITS),
    unitPriceMinor: z.number().int().nonnegative(),
    lineTotalMinor: z.number().int().nonnegative(),
    recordedBy: laundryTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type OrderLine = z.infer<typeof orderLineSchema>

export const orderSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: laundryTenantSchema,
    orderCode: z.string().trim().min(1).max(50),
    kind: z.enum(ORDER_KINDS),
    customerRef: z.string().trim().max(80).nullable(),
    status: z.enum(ORDER_STATUSES),
    currency: currencySchema,
    totalMinor: z.number().int().nonnegative(),
    itemCount: z.number().int().nonnegative().default(0),
    weightGrams: z.number().int().nonnegative().default(0),
    unreconciledCount: z.number().int().nonnegative().default(0),
    revision: z.number().int().positive().default(1),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
export type LaundryOrder = z.infer<typeof orderSchema>

export const processEventSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: laundryTenantSchema,
    orderId: z.string().uuid(),
    event: z.enum(PROCESS_EVENTS),
    note: z.string().trim().max(500).nullable(),
    evidenceRef: z.string().trim().max(300).nullable(),
    recordedBy: laundryTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type ProcessRecord = z.infer<typeof processEventSchema>

export const findingSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: laundryTenantSchema,
    orderId: z.string().uuid(),
    category: z.enum(FINDING_CATEGORIES),
    disposition: z.enum(FINDING_DISPOSITIONS),
    quantity: z.number().int().positive().nullable(),
    reason: z.string().trim().min(1).max(500),
    evidenceRef: z.string().trim().max(300).nullable(),
    reconcilesId: z.string().uuid().nullable(),
    recordedBy: laundryTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type Finding = z.infer<typeof findingSchema>

export const orderDetailSchema = z
  .object({
    order: orderSchema,
    lines: z.array(orderLineSchema),
    process: z.array(processEventSchema),
    findings: z.array(findingSchema),
  })
  .strict()
export type OrderDetail = z.infer<typeof orderDetailSchema>

export const lineInputSchema = z
  .object({
    itemRef: z.string().trim().min(1).max(80),
    quantity: z.number().int().positive(),
    unit: z.enum(LINE_UNITS),
    unitPriceMinor: z.number().int().nonnegative(),
  })
  .strict()
export type LineInput = z.infer<typeof lineInputSchema>

export const laundryMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_order'),
      commandId: z.string().uuid(),
      orderCode: z.string().trim().min(1).max(50),
      kind: z.enum(ORDER_KINDS),
      customerRef: z.string().trim().max(80).nullable().optional(),
      currency: currencySchema,
      lines: z.array(lineInputSchema).min(1).max(50),
    })
    .strict(),
  z
    .object({
      type: z.literal('record_process'),
      commandId: z.string().uuid(),
      orderId: z.string().uuid(),
      event: z.enum(PROCESS_EVENTS),
      note: z.string().trim().max(500).nullable().optional(),
      evidenceRef: z.string().trim().max(300).nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('record_finding'),
      commandId: z.string().uuid(),
      orderId: z.string().uuid(),
      category: z.enum(FINDING_CATEGORIES),
      disposition: z.enum(FINDING_DISPOSITIONS),
      quantity: z.number().int().positive().nullable().optional(),
      reason: z.string().trim().min(1).max(500),
      evidenceRef: z.string().trim().max(300).nullable().optional(),
      reconcilesId: z.string().uuid().nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('change_order_status'),
      commandId: z.string().uuid(),
      orderId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      targetStatus: z.enum(['processing', 'ready', 'returned', 'canceled']),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
])
export type LaundryMutation = z.infer<typeof laundryMutationSchema>

export const laundryReceiptSchema = z
  .object({
    commandId: z.string().uuid(),
    tenantId: laundryTenantSchema,
    action: z.string().min(1),
    orderId: z.string().uuid().nullable().optional(),
    processId: z.string().uuid().nullable().optional(),
    findingId: z.string().uuid().nullable().optional(),
    status: z.string().min(1),
    revision: z.number().int().nonnegative().nullable().optional(),
    idempotencyReplay: z.boolean().default(false),
    executedAt: z.string().datetime(),
  })
  .strict()
export type LaundryReceipt = z.infer<typeof laundryReceiptSchema>

export const laundryCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('unknown') }).strict(),
  z.object({ status: z.literal('recorded'), receipt: laundryReceiptSchema }).strict(),
])
export type LaundryCommandStatus = z.infer<typeof laundryCommandStatusSchema>

export const orderPageQuerySchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    offset: z.number().int().nonnegative().default(0),
    status: z.enum(ORDER_STATUSES).optional(),
    kind: z.enum(ORDER_KINDS).optional(),
  })
  .strict()
export type OrderPageQuery = z.infer<typeof orderPageQuerySchema>

export interface LaundryRepository {
  execute(
    tenant: string,
    principal: LaundryPrincipal,
    mutation: LaundryMutation,
  ): Promise<LaundryReceipt>
  getCommandStatus(
    tenant: string,
    principal: LaundryPrincipal,
    commandId: string,
  ): Promise<LaundryCommandStatus>
  getAccess(tenant: string, principal: LaundryPrincipal): Promise<LaundryAccess>
  listOrders(
    tenant: string,
    principal: LaundryPrincipal,
    query: OrderPageQuery,
  ): Promise<readonly LaundryOrder[]>
  getOrderDetail(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<OrderDetail | null>
  listLines(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly OrderLine[]>
  listProcess(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly ProcessRecord[]>
  listFindings(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly Finding[]>
}

export type LaundryErrorCode =
  | 'forbidden'
  | 'unauthorized'
  | 'not_found'
  | 'conflict'
  | 'bad_request'
  | 'invalid_transition'
  | 'process_out_of_order'
  | 'unreconciled_exceptions'

export class LaundryError extends Error {
  constructor(
    public readonly code: LaundryErrorCode,
    message?: string,
  ) {
    super(message ?? `Laundry error: ${code}`)
    this.name = 'LaundryError'
  }
}
