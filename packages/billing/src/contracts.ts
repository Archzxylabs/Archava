import { z } from 'zod'

export const billingTenantSchema = z.string().trim().min(1).max(100)

export const billingPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: billingTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: billingTenantSchema,
      onBehalfOf: billingTenantSchema,
    })
    .strict(),
])
export type BillingPrincipal = z.infer<typeof billingPrincipalSchema>

export const BILLING_PERMISSIONS = [
  'billing.read',
  'billing.write',
  'billing.settle',
  'audit.read',
] as const
export type BillingPermission = (typeof BILLING_PERMISSIONS)[number]

export const billingMembershipSchema = z
  .object({
    principalId: billingTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(BILLING_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type BillingMembership = z.infer<typeof billingMembershipSchema>

export const billingAccessSchema = z
  .object({
    permissions: z.array(z.enum(BILLING_PERMISSIONS)),
    ownerId: billingTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type BillingAccess = z.infer<typeof billingAccessSchema>

/**
 * Money is never a float.
 *
 * Every amount in this domain is an integer count of the currency's minor unit,
 * so a subtotal can be summed and a settlement can be compared without a single
 * rounding step anywhere between the invoice snapshot and the balance. A float
 * total that compares unequal to a provider's integer total is exactly the kind
 * of drift that turns into a silently under-settled invoice.
 */
export const moneyMinorSchema = z.number().int()

export const currencySchema = z
  .string()
  .trim()
  .regex(/^[A-Z]{3}$/, 'Currency is a three-letter ISO code')

/**
 * Draft → issued is the immutability boundary.
 *
 * A draft is a working document: lines can still be built and rebuilt. Issuing
 * freezes the priced snapshot, and from that point the only way to change what
 * is owed is an appended correction. Settled and void are both terminal.
 */
export const INVOICE_STATUSES = ['draft', 'issued', 'settled', 'void'] as const
export type InvoiceStatus = (typeof INVOICE_STATUSES)[number]

export const INVOICE_STATUS_TARGETS = ['issued', 'settled', 'void'] as const

export const PAYMENT_OUTCOMES = ['succeeded', 'failed', 'refunded'] as const
export type PaymentOutcome = (typeof PAYMENT_OUTCOMES)[number]

/** Outcomes that reduce what is owed. `failed` records history and moves nothing. */
export const SETTLEMENT_OUTCOMES = ['succeeded', 'refunded'] as const

export const invoiceLineSchema = z
  .object({
    id: z.string().uuid(),
    invoiceId: z.string().uuid(),
    description: z.string().trim().min(1).max(300),
    quantity: z.number().int().positive(),
    unitMinor: moneyMinorSchema,
    lineTotalMinor: moneyMinorSchema,
    recordedBy: billingTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type InvoiceLine = z.infer<typeof invoiceLineSchema>

export const invoiceCorrectionSchema = z
  .object({
    id: z.string().uuid(),
    invoiceId: z.string().uuid(),
    amountMinor: moneyMinorSchema,
    reason: z.string().trim().min(1).max(500),
    reversesId: z.string().uuid().nullable(),
    createdAt: z.string().datetime(),
    recordedBy: billingTenantSchema,
  })
  .strict()
export type InvoiceCorrection = z.infer<typeof invoiceCorrectionSchema>

export const paymentAttemptSchema = z
  .object({
    id: z.string().uuid(),
    invoiceId: z.string().uuid(),
    amountMinor: moneyMinorSchema,
    currency: currencySchema,
    providerRef: z.string().trim().max(120).nullable(),
    recordedAt: z.string().datetime(),
    recordedBy: billingTenantSchema,
  })
  .strict()
export type PaymentAttempt = z.infer<typeof paymentAttemptSchema>

export const paymentEventSchema = z
  .object({
    id: z.string().uuid(),
    invoiceId: z.string().uuid(),
    providerId: z.string().trim().min(1).max(80),
    providerEventId: z.string().trim().min(1).max(120),
    outcome: z.enum(PAYMENT_OUTCOMES),
    amountMinor: moneyMinorSchema,
    currency: currencySchema,
    verifiedAt: z.string().datetime(),
    recordedBy: billingTenantSchema,
  })
  .strict()
export type PaymentEvent = z.infer<typeof paymentEventSchema>

export const invoiceSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: billingTenantSchema,
    invoiceCode: z.string().trim().min(1).max(50),
    customerRef: z.string().trim().min(1).max(120),
    currency: currencySchema,
    status: z.enum(INVOICE_STATUSES),
    grossMinor: moneyMinorSchema,
    correctionMinor: moneyMinorSchema,
    settledMinor: moneyMinorSchema,
    outstandingMinor: moneyMinorSchema,
    consumerRef: billingTenantSchema,
    revision: z.number().int().nonnegative(),
    createdAt: z.string().datetime(),
    issuedAt: z.string().datetime().nullable(),
    settledAt: z.string().datetime().nullable(),
    voidedAt: z.string().datetime().nullable(),
  })
  .strict()
export type Invoice = z.infer<typeof invoiceSchema>

export const invoiceDetailSchema = z
  .object({
    invoice: invoiceSchema,
    lines: z.array(invoiceLineSchema),
    corrections: z.array(invoiceCorrectionSchema),
    attempts: z.array(paymentAttemptSchema),
    events: z.array(paymentEventSchema),
  })
  .strict()
export type InvoiceDetail = z.infer<typeof invoiceDetailSchema>

export const invoicePageQuerySchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    offset: z.number().int().nonnegative().default(0),
    status: z.enum(INVOICE_STATUSES).optional(),
    customerRef: z.string().trim().max(120).optional(),
  })
  .strict()
export type InvoicePageQuery = z.infer<typeof invoicePageQuerySchema>

const pricedLineSchema = z
  .object({
    description: z.string().trim().min(1).max(300),
    quantity: z.number().int().positive(),
    unitMinor: moneyMinorSchema,
  })
  .strict()

export const billingMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_invoice'),
      commandId: z.string().uuid(),
      invoiceCode: z.string().trim().min(1).max(50),
      customerRef: z.string().trim().min(1).max(120),
      currency: currencySchema,
      consumerRef: billingTenantSchema,
      lines: z.array(pricedLineSchema).min(1).max(200),
    })
    .strict(),
  z
    .object({
      type: z.literal('issue_invoice'),
      commandId: z.string().uuid(),
      invoiceId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
  z
    .object({
      type: z.literal('append_correction'),
      commandId: z.string().uuid(),
      invoiceId: z.string().uuid(),
      amountMinor: moneyMinorSchema.refine((value) => value !== 0, 'Correction cannot be zero'),
      reason: z.string().trim().min(1).max(500),
      reversesId: z.string().uuid().nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('void_invoice'),
      commandId: z.string().uuid(),
      invoiceId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
  z
    .object({
      /**
       * Records that someone *tried* to take a payment. It carries an intent, an
       * amount and a provider reference, and it is structurally incapable of
       * asserting a result: there is no outcome field to send, because the result
       * is not this caller's to state.
       */
      type: z.literal('record_payment_attempt'),
      commandId: z.string().uuid(),
      invoiceId: z.string().uuid(),
      amountMinor: moneyMinorSchema.positive(),
      providerRef: z.string().trim().max(120).nullable().optional(),
    })
    .strict(),
  z
    .object({
      /**
       * The only mutation that can change what has been paid.
       *
       * Authority here is the signature, not the caller: the provider signs a
       * canonical rendering of exactly these fields and the service recomputes
       * that digest under the tenant's configured secret. A caller who can reach
       * this endpoint still cannot assert an amount, because the amount that is
       * applied is the signed one.
       */
      type: z.literal('record_provider_event'),
      commandId: z.string().uuid(),
      invoiceId: z.string().uuid(),
      providerId: z.string().trim().min(1).max(80),
      providerEventId: z.string().trim().min(1).max(120),
      outcome: z.enum(PAYMENT_OUTCOMES),
      amountMinor: moneyMinorSchema,
      currency: currencySchema,
      signature: z
        .string()
        .trim()
        .regex(/^[0-9a-f]{64}$/, 'Signature is 64 hex characters'),
    })
    .strict(),
])
export type BillingMutation = z.infer<typeof billingMutationSchema>

export const billingReceiptSchema = z
  .object({
    commandId: z.string().uuid(),
    tenantId: billingTenantSchema,
    action: z.string().min(1),
    invoiceId: z.string().uuid().nullable().optional(),
    lineId: z.string().uuid().nullable().optional(),
    correctionId: z.string().uuid().nullable().optional(),
    attemptId: z.string().uuid().nullable().optional(),
    eventId: z.string().uuid().nullable().optional(),
    status: z.string().min(1),
    outstandingMinor: moneyMinorSchema.nullable().optional(),
    revision: z.number().int().nonnegative().nullable().optional(),
    idempotencyReplay: z.boolean().default(false),
    executedAt: z.string().datetime(),
  })
  .strict()
export type BillingReceipt = z.infer<typeof billingReceiptSchema>

export const billingCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('unknown') }).strict(),
  z.object({ status: z.literal('recorded'), receipt: billingReceiptSchema }).strict(),
])
export type BillingCommandStatus = z.infer<typeof billingCommandStatusSchema>

export interface BillingRepository {
  execute(
    tenant: string,
    principal: BillingPrincipal,
    mutation: BillingMutation,
  ): Promise<BillingReceipt>
  getCommandStatus(
    tenant: string,
    principal: BillingPrincipal,
    commandId: string,
  ): Promise<BillingCommandStatus>
  getAccess(tenant: string, principal: BillingPrincipal): Promise<BillingAccess>
  listInvoices(
    tenant: string,
    principal: BillingPrincipal,
    query: InvoicePageQuery,
  ): Promise<readonly Invoice[]>
  getInvoiceDetail(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<InvoiceDetail | null>
  listLines(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly InvoiceLine[]>
  listCorrections(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly InvoiceCorrection[]>
  listAttempts(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly PaymentAttempt[]>
  listEvents(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly PaymentEvent[]>
}

export type BillingErrorCode =
  | 'forbidden'
  | 'unauthorized'
  | 'not_found'
  | 'conflict'
  | 'bad_request'
  | 'invalid_transition'
  | 'invoice_immutable'
  | 'invalid_signature'
  | 'unbalanced_settlement'

export class BillingError extends Error {
  constructor(
    public readonly code: BillingErrorCode,
    message?: string,
  ) {
    super(message ?? `Billing error: ${code}`)
    this.name = 'BillingError'
  }
}
