import { z } from 'zod'

export const fulfillmentTenantSchema = z.string().trim().min(1).max(100)

export const fulfillmentPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: fulfillmentTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: fulfillmentTenantSchema,
      onBehalfOf: fulfillmentTenantSchema,
    })
    .strict(),
])
export type FulfillmentPrincipal = z.infer<typeof fulfillmentPrincipalSchema>

export const FULFILLMENT_PERMISSIONS = [
  'fulfillment.read',
  'fulfillment.write',
  'fulfillment.dispatch',
  'fulfillment.produce',
  'audit.read',
] as const
export type FulfillmentPermission = (typeof FULFILLMENT_PERMISSIONS)[number]

export const fulfillmentMembershipSchema = z
  .object({
    principalId: fulfillmentTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(FULFILLMENT_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type FulfillmentMembership = z.infer<typeof fulfillmentMembershipSchema>

export const fulfillmentAccessSchema = z
  .object({
    permissions: z.array(z.enum(FULFILLMENT_PERMISSIONS)),
    ownerId: fulfillmentTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type FulfillmentAccess = z.infer<typeof fulfillmentAccessSchema>

/** Dispatch work orders and made-to-order production work orders share one lifecycle. */
export const JOB_KINDS = ['dispatch', 'production'] as const
export type JobKind = (typeof JOB_KINDS)[number]

export const JOB_STATUSES = ['draft', 'released', 'in_progress', 'completed', 'canceled'] as const
export type JobStatus = (typeof JOB_STATUSES)[number]

export const LINE_DIRECTIONS = ['consume', 'produce'] as const
export type LineDirection = (typeof LINE_DIRECTIONS)[number]

export const LINE_DISPOSITIONS = ['normal', 'scrap'] as const
export type LineDisposition = (typeof LINE_DISPOSITIONS)[number]

export const FINDING_CATEGORIES = ['condition', 'quantity', 'delivery'] as const
export type FindingCategory = (typeof FINDING_CATEGORIES)[number]

export const FINDING_DISPOSITIONS = ['accepted', 'partial', 'rejected', 'lost', 'damaged'] as const
export type FindingDisposition = (typeof FINDING_DISPOSITIONS)[number]

export const CUSTODY_EVENTS = [
  'received',
  'stored',
  'picked',
  'packed',
  'dispatched',
  'delivered',
  'returned',
] as const
export type CustodyEvent = (typeof CUSTODY_EVENTS)[number]

/** Ordered custody chain; `returned` is a rollback and is governed separately. */
export const CUSTODY_STAGE: Readonly<Record<string, number>> = Object.freeze({
  received: 0,
  stored: 1,
  picked: 2,
  packed: 3,
  dispatched: 4,
  delivered: 5,
})

/** Findings that block completion until a later acceptance reconciles them. */
export const EXCEPTION_DISPOSITIONS = ['partial', 'rejected', 'lost', 'damaged'] as const

export const jobSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: fulfillmentTenantSchema,
    jobCode: z.string().trim().min(1).max(50),
    kind: z.enum(JOB_KINDS),
    title: z.string().trim().min(1).max(200),
    ownerRef: z.string().trim().max(80).nullable(),
    status: z.enum(JOB_STATUSES),
    consumedCount: z.number().int().nonnegative().default(0),
    producedCount: z.number().int().nonnegative().default(0),
    scrapCount: z.number().int().nonnegative().default(0),
    unreconciledCount: z.number().int().nonnegative().default(0),
    revision: z.number().int().positive().default(1),
    createdAt: z.string().datetime(),
    updatedAt: z.string().datetime(),
  })
  .strict()
export type Job = z.infer<typeof jobSchema>

export const jobLineSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: fulfillmentTenantSchema,
    jobId: z.string().uuid(),
    direction: z.enum(LINE_DIRECTIONS),
    itemRef: z.string().trim().min(1).max(40),
    quantity: z.number().int().positive(),
    disposition: z.enum(LINE_DISPOSITIONS),
    evidenceRef: z.string().trim().max(300).nullable(),
    recordedBy: fulfillmentTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type JobLine = z.infer<typeof jobLineSchema>

export const findingSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: fulfillmentTenantSchema,
    jobId: z.string().uuid(),
    category: z.enum(FINDING_CATEGORIES),
    disposition: z.enum(FINDING_DISPOSITIONS),
    quantity: z.number().int().positive().nullable(),
    reason: z.string().trim().min(1).max(500),
    evidenceRef: z.string().trim().max(300).nullable(),
    reconcilesId: z.string().uuid().nullable(),
    recordedBy: fulfillmentTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type Finding = z.infer<typeof findingSchema>

export const custodyEventSchema = z
  .object({
    id: z.string().uuid(),
    tenantId: fulfillmentTenantSchema,
    jobId: z.string().uuid(),
    event: z.enum(CUSTODY_EVENTS),
    fromRef: z.string().trim().max(80).nullable(),
    toRef: z.string().trim().max(80).nullable(),
    evidenceRef: z.string().trim().max(300).nullable(),
    note: z.string().trim().max(500).nullable(),
    recordedBy: fulfillmentTenantSchema,
    recordedAt: z.string().datetime(),
  })
  .strict()
export type CustodyRecord = z.infer<typeof custodyEventSchema>

export const jobDetailSchema = z
  .object({
    job: jobSchema,
    lines: z.array(jobLineSchema),
    findings: z.array(findingSchema),
    custody: z.array(custodyEventSchema),
  })
  .strict()
export type JobDetail = z.infer<typeof jobDetailSchema>

export const fulfillmentMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_job'),
      commandId: z.string().uuid(),
      jobCode: z.string().trim().min(1).max(50),
      kind: z.enum(JOB_KINDS),
      title: z.string().trim().min(1).max(200),
      ownerRef: z.string().trim().max(80).nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('record_line'),
      commandId: z.string().uuid(),
      jobId: z.string().uuid(),
      direction: z.enum(LINE_DIRECTIONS),
      itemRef: z.string().trim().min(1).max(40),
      quantity: z.number().int().positive(),
      disposition: z.enum(LINE_DISPOSITIONS).default('normal'),
      evidenceRef: z.string().trim().max(300).nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('record_finding'),
      commandId: z.string().uuid(),
      jobId: z.string().uuid(),
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
      type: z.literal('record_custody'),
      commandId: z.string().uuid(),
      jobId: z.string().uuid(),
      event: z.enum(CUSTODY_EVENTS),
      fromRef: z.string().trim().max(80).nullable().optional(),
      toRef: z.string().trim().max(80).nullable().optional(),
      evidenceRef: z.string().trim().max(300).nullable().optional(),
      note: z.string().trim().max(500).nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('change_job_status'),
      commandId: z.string().uuid(),
      jobId: z.string().uuid(),
      expectedRevision: z.number().int().nonnegative(),
      targetStatus: z.enum(['released', 'in_progress', 'completed', 'canceled']),
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
])
export type FulfillmentMutation = z.infer<typeof fulfillmentMutationSchema>

export const fulfillmentReceiptSchema = z
  .object({
    commandId: z.string().uuid(),
    tenantId: fulfillmentTenantSchema,
    action: z.string().min(1),
    jobId: z.string().uuid().nullable().optional(),
    lineId: z.string().uuid().nullable().optional(),
    findingId: z.string().uuid().nullable().optional(),
    custodyId: z.string().uuid().nullable().optional(),
    status: z.string().min(1),
    revision: z.number().int().nonnegative().nullable().optional(),
    idempotencyReplay: z.boolean().default(false),
    executedAt: z.string().datetime(),
  })
  .strict()
export type FulfillmentReceipt = z.infer<typeof fulfillmentReceiptSchema>

export const fulfillmentCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('unknown') }).strict(),
  z.object({ status: z.literal('recorded'), receipt: fulfillmentReceiptSchema }).strict(),
])
export type FulfillmentCommandStatus = z.infer<typeof fulfillmentCommandStatusSchema>

export const jobPageQuerySchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    offset: z.number().int().nonnegative().default(0),
    status: z.enum(JOB_STATUSES).optional(),
    kind: z.enum(JOB_KINDS).optional(),
  })
  .strict()
export type JobPageQuery = z.infer<typeof jobPageQuerySchema>

export interface FulfillmentRepository {
  execute(
    tenant: string,
    principal: FulfillmentPrincipal,
    mutation: FulfillmentMutation,
  ): Promise<FulfillmentReceipt>
  getCommandStatus(
    tenant: string,
    principal: FulfillmentPrincipal,
    commandId: string,
  ): Promise<FulfillmentCommandStatus>
  getAccess(tenant: string, principal: FulfillmentPrincipal): Promise<FulfillmentAccess>
  listJobs(
    tenant: string,
    principal: FulfillmentPrincipal,
    query: JobPageQuery,
  ): Promise<readonly Job[]>
  getJobDetail(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<JobDetail | null>
  listLines(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly JobLine[]>
  listFindings(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly Finding[]>
  listCustody(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly CustodyRecord[]>
}

export type FulfillmentErrorCode =
  | 'forbidden'
  | 'unauthorized'
  | 'not_found'
  | 'conflict'
  | 'bad_request'
  | 'invalid_transition'
  | 'custody_out_of_order'
  | 'unreconciled_exceptions'

export class FulfillmentError extends Error {
  constructor(
    public readonly code: FulfillmentErrorCode,
    message?: string,
  ) {
    super(message ?? `Fulfillment error: ${code}`)
    this.name = 'FulfillmentError'
  }
}
