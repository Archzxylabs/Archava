import { z } from 'zod'

export class SubscriptionError extends Error {
  constructor(
    public readonly code:
      | 'forbidden'
      | 'not_found'
      | 'conflict'
      | 'invalid_request'
      | 'unauthorized'
      | 'past_due'
      | 'exhausted'
      | 'suspended'
      | 'already_active',
    message?: string,
  ) {
    super(message ?? code)
    this.name = 'SubscriptionError'
  }
}

export const subscriptionTenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)

export const subscriptionPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: subscriptionTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: subscriptionTenantSchema,
      onBehalfOf: subscriptionTenantSchema,
    })
    .strict(),
])
export type SubscriptionPrincipal = z.infer<typeof subscriptionPrincipalSchema>

export const SUBSCRIPTION_PERMISSIONS = [
  'subscriptions.read',
  'subscriptions.write',
  'subscriptions.payment',
  'subscriptions.usage',
  'audit.read',
] as const
export type SubscriptionPermission = (typeof SUBSCRIPTION_PERMISSIONS)[number]

export const subscriptionMembershipSchema = z
  .object({
    principalId: subscriptionTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(SUBSCRIPTION_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type SubscriptionMembership = z.infer<typeof subscriptionMembershipSchema>

export const subscriptionAccessSchema = z
  .object({
    permissions: z.array(z.enum(SUBSCRIPTION_PERMISSIONS)),
    ownerId: subscriptionTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type SubscriptionAccess = z.infer<typeof subscriptionAccessSchema>

export const SUBSCRIPTION_STATUSES = [
  'draft',
  'active',
  'past_due',
  'paused',
  'canceled',
  'expired',
] as const
export type SubscriptionStatus = (typeof SUBSCRIPTION_STATUSES)[number]

export const BILLING_INTERVALS = ['weekly', 'monthly', 'quarterly', 'yearly'] as const
export type BillingInterval = (typeof BILLING_INTERVALS)[number]

export const ENTITLEMENT_TYPES = ['metered', 'seat_count', 'boolean_access', 'pass_bundle'] as const
export type EntitlementType = (typeof ENTITLEMENT_TYPES)[number]

export const ENTITLEMENT_STATUSES = ['active', 'suspended', 'revoked', 'exhausted'] as const
export type EntitlementStatus = (typeof ENTITLEMENT_STATUSES)[number]

export const subscriptionSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: subscriptionTenantSchema,
    contactId: z.string().uuid(),
    planCode: z.string().trim().min(1).max(100),
    planName: z.string().trim().min(1).max(200),
    status: z.enum(SUBSCRIPTION_STATUSES),
    billingInterval: z.enum(BILLING_INTERVALS),
    unitPrice: z.number().int().nonnegative(),
    currency: z.string().regex(/^[A-Z]{3}$/),
    quantity: z.number().int().positive().default(1),
    currentPeriodStart: z.string().datetime().nullable(),
    currentPeriodEnd: z.string().datetime().nullable(),
    cancelAtPeriodEnd: z.boolean().default(false),
    paymentReference: z.string().max(200).nullable(),
    ownerId: subscriptionTenantSchema,
    revision: z.number().int().nonnegative(),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
export type Subscription = z.infer<typeof subscriptionSchema>

export const entitlementSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: subscriptionTenantSchema,
    subscriptionId: z.string().uuid().nullable(),
    contactId: z.string().uuid(),
    featureCode: z.string().trim().min(1).max(100),
    featureName: z.string().trim().min(1).max(200),
    entitlementType: z.enum(ENTITLEMENT_TYPES),
    allocatedQuantity: z.number().int().nonnegative().nullable(),
    consumedQuantity: z.number().int().nonnegative().default(0),
    status: z.enum(ENTITLEMENT_STATUSES),
    validFrom: z.string().datetime(),
    validUntil: z.string().datetime().nullable(),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
export type Entitlement = z.infer<typeof entitlementSchema>

export const usageEventSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: subscriptionTenantSchema,
    entitlementId: z.string().uuid(),
    subscriptionId: z.string().uuid().nullable(),
    contactId: z.string().uuid(),
    unitsConsumed: z.number().int().positive(),
    idempotencyKey: z.string().trim().min(1).max(100),
    evidenceRef: z.string().trim().max(300).nullable(),
    recordedBy: subscriptionTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type UsageEvent = z.infer<typeof usageEventSchema>

export const subscriptionDetailSchema = z
  .object({
    subscription: subscriptionSchema,
    entitlements: z.array(entitlementSchema),
  })
  .strict()
export type SubscriptionDetail = z.infer<typeof subscriptionDetailSchema>

export const initialEntitlementInputSchema = z
  .object({
    featureCode: z.string().trim().min(1).max(100),
    featureName: z.string().trim().min(1).max(200),
    entitlementType: z.enum(ENTITLEMENT_TYPES),
    allocatedQuantity: z.number().int().nonnegative().nullable().optional(),
    validUntil: z.string().datetime().optional(),
  })
  .strict()
export type InitialEntitlementInput = z.infer<typeof initialEntitlementInputSchema>

export const subscriptionMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_subscription'),
      commandId: z.string().uuid(),
      contactId: z.string().uuid(),
      planCode: z.string().trim().min(1).max(100),
      planName: z.string().trim().min(1).max(200),
      billingInterval: z.enum(BILLING_INTERVALS),
      unitPrice: z.number().int().nonnegative(),
      currency: z.string().regex(/^[A-Z]{3}$/),
      quantity: z.number().int().positive().default(1),
      initialEntitlements: z.array(initialEntitlementInputSchema).default([]),
    })
    .strict(),
  z
    .object({
      type: z.literal('record_payment'),
      commandId: z.string().uuid(),
      subscriptionId: z.string().uuid(),
      paymentReference: z.string().trim().min(1).max(200),
      amountPaid: z.number().int().nonnegative(),
      periodStart: z.string().datetime(),
      periodEnd: z.string().datetime(),
    })
    .strict(),
  z
    .object({
      type: z.literal('consume_usage'),
      commandId: z.string().uuid(),
      entitlementId: z.string().uuid(),
      units: z.number().int().positive(),
      idempotencyKey: z.string().trim().min(1).max(100),
      evidenceRef: z.string().trim().max(300).nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('change_status'),
      commandId: z.string().uuid(),
      subscriptionId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      targetStatus: z.enum(['active', 'paused', 'canceled', 'past_due']),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
  z
    .object({
      type: z.literal('grant_entitlement'),
      commandId: z.string().uuid(),
      subscriptionId: z.string().uuid().nullable().optional(),
      contactId: z.string().uuid(),
      featureCode: z.string().trim().min(1).max(100),
      featureName: z.string().trim().min(1).max(200),
      entitlementType: z.enum(ENTITLEMENT_TYPES),
      allocatedQuantity: z.number().int().nonnegative().nullable().optional(),
      validUntil: z.string().datetime().nullable().optional(),
    })
    .strict(),
])
export type SubscriptionMutation = z.infer<typeof subscriptionMutationSchema>

export const subscriptionReceiptSchema = z
  .object({
    commandId: z.string().uuid(),
    tenantId: subscriptionTenantSchema,
    action: z.string(),
    subscriptionId: z.string().uuid().nullable().optional(),
    entitlementId: z.string().uuid().nullable().optional(),
    usageEventId: z.string().uuid().nullable().optional(),
    revision: z.number().int().nonnegative().optional(),
    status: z.string(),
    idempotencyReplay: z.boolean(),
    executedAt: z.string().datetime(),
  })
  .strict()
export type SubscriptionReceipt = z.infer<typeof subscriptionReceiptSchema>

export const subscriptionCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('unknown') }).strict(),
  z.object({ status: z.literal('recorded'), receipt: subscriptionReceiptSchema }).strict(),
])
export type SubscriptionCommandStatus = z.infer<typeof subscriptionCommandStatusSchema>

export const subscriptionPageQuerySchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    offset: z.number().int().nonnegative().default(0),
    status: z.enum(SUBSCRIPTION_STATUSES).optional(),
    contactId: z.string().uuid().optional(),
  })
  .strict()
export type SubscriptionPageQuery = z.infer<typeof subscriptionPageQuerySchema>

export interface SubscriptionRepository {
  execute(
    tenant: string,
    principal: SubscriptionPrincipal,
    mutation: SubscriptionMutation,
  ): Promise<SubscriptionReceipt>
  getCommandStatus(
    tenant: string,
    principal: SubscriptionPrincipal,
    commandId: string,
  ): Promise<SubscriptionCommandStatus>
  getAccess(tenant: string, principal: SubscriptionPrincipal): Promise<SubscriptionAccess>
  listSubscriptions(
    tenant: string,
    principal: SubscriptionPrincipal,
    query: SubscriptionPageQuery,
  ): Promise<readonly Subscription[]>
  getSubscriptionDetail(
    tenant: string,
    principal: SubscriptionPrincipal,
    id: string,
  ): Promise<SubscriptionDetail | null>
  listEntitlements(
    tenant: string,
    principal: SubscriptionPrincipal,
    contactId: string,
  ): Promise<readonly Entitlement[]>
  listUsageEvents(
    tenant: string,
    principal: SubscriptionPrincipal,
    entitlementId: string,
  ): Promise<readonly UsageEvent[]>
}
