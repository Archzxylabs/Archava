/**
 * Field services: bounded technician dispatch, site visits, work evidence and approval.
 * This is a narrow local scope, not a full KIT-17 operating system. Scheduling,
 * asset register, inventory and invoicing remain the owned domains they already
 * belong to; this package records the dispatch handover and its evidence only.
 */
import { z } from 'zod'

export const fieldTenantSchema = z.string().trim().min(1).max(100)

export const fieldPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: fieldTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: fieldTenantSchema,
      onBehalfOf: fieldTenantSchema,
    })
    .strict(),
])
export type FieldPrincipal = z.infer<typeof fieldPrincipalSchema>

export const FIELD_PERMISSIONS = [
  'field.read',
  'field.write',
  'field.dispatch',
  'field.signoff',
  'audit.read',
] as const
export type FieldPermission = (typeof FIELD_PERMISSIONS)[number]

export const fieldMembershipSchema = z
  .object({
    principalId: fieldTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(FIELD_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type FieldMembership = z.infer<typeof fieldMembershipSchema>

export const fieldAccessSchema = z
  .object({
    permissions: z.array(z.enum(FIELD_PERMISSIONS)),
    ownerId: fieldTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type FieldAccess = z.infer<typeof fieldAccessSchema>

/** Monotonic work-order status. Approved is the money boundary. */
export const JOB_STATUSES = [
  'requested',
  'assigned',
  'visited',
  'evidenced',
  'signed_off',
  'approved',
  'canceled',
] as const
export type JobStatus = (typeof JOB_STATUSES)[number]

/**
 * The forward chain: evidence must follow a visit, signoff must follow
 * evidence, and approval is the last forward step. Terminal statuses are
 * reached explicitly and refuse further evidence.
 */
export const JOB_TRANSITIONS: Readonly<Record<JobStatus, readonly JobStatus[]>> = Object.freeze({
  requested: ['assigned', 'canceled'],
  assigned: ['visited', 'canceled'],
  visited: ['evidenced', 'canceled'],
  evidenced: ['signed_off'],
  signed_off: ['approved', 'evidenced'],
  approved: [],
  canceled: [],
})

export const PRIORITIES = ['low', 'normal', 'high', 'urgent'] as const
export type Priority = (typeof PRIORITIES)[number]

export const VISIT_OUTCOMES = ['completed', 'partial', 'no_access'] as const
export type VisitOutcome = (typeof VISIT_OUTCOMES)[number]

/**
 * Counters stay on separate axes: labour duration is hours, parts are pieces.
 * Mirrors the laundry piece/gram split so one number can never stand in for both.
 */
export const workLineSchema = z
  .object({
    lineId: z.string().trim().min(1).max(100),
    kind: z.enum(['labour', 'part']),
    description: z.string().trim().min(1).max(500),
    quantity: z.number().positive(),
    /** Hours for `labour`, pieces for `part`. */
    unit: z.enum(['hours', 'pieces']),
    /** Server-resolved reference for parts; null for labour lines. */
    partRef: z.string().trim().min(1).max(100).nullable(),
  })
  .strict()
  .superRefine((line, ctx) => {
    // Hours measure duration, pieces count items: neither may masquerade as the other.
    if (line.kind === 'labour' && line.unit !== 'hours') {
      ctx.addIssue({
        code: 'custom',
        message: 'Labour lines are measured in hours',
        path: ['unit'],
      })
    }
    if (line.kind === 'part' && line.unit !== 'pieces') {
      ctx.addIssue({ code: 'custom', message: 'Part lines are counted in pieces', path: ['unit'] })
    }
    if (line.kind === 'part' && !Number.isInteger(line.quantity)) {
      ctx.addIssue({
        code: 'custom',
        message: 'Part quantities are whole pieces',
        path: ['quantity'],
      })
    }
    if (line.kind === 'labour' && line.partRef !== null) {
      ctx.addIssue({
        code: 'custom',
        message: 'Labour lines carry no part reference',
        path: ['partRef'],
      })
    }
  })
export type WorkLine = z.infer<typeof workLineSchema>

export const fieldJobSchema = z
  .object({
    jobId: fieldTenantSchema,
    customerRef: fieldTenantSchema,
    siteRef: fieldTenantSchema,
    summary: z.string().trim().min(1).max(500),
    priority: z.enum(PRIORITIES),
    status: z.enum(JOB_STATUSES),
    revision: z.number().int().nonnegative(),
    /** The scheduler-owned appointment this dispatch must not lose. */
    appointmentRef: fieldTenantSchema,
    /** The asset-register item the visit was performed against, when any. */
    assetRef: fieldTenantSchema.nullable(),
    assignedPrincipal: fieldTenantSchema.nullable(),
    laborHours: z.number().nonnegative(),
    partsPieces: z.number().int().nonnegative(),
    createdBy: fieldTenantSchema,
    createdAt: z.string().min(1),
    updatedAt: z.string().min(1),
  })
  .strict()
export type FieldJob = z.infer<typeof fieldJobSchema>

export const visitRecordSchema = z
  .object({
    visitId: fieldTenantSchema,
    jobId: fieldTenantSchema,
    outcome: z.enum(VISIT_OUTCOMES),
    note: z.string().trim().min(1).max(2000),
    technicianRef: fieldTenantSchema,
    visitedAt: z.string().min(1),
  })
  .strict()
export type VisitRecord = z.infer<typeof visitRecordSchema>

export const signoffRecordSchema = z
  .object({
    signoffId: fieldTenantSchema,
    jobId: fieldTenantSchema,
    signerRef: fieldTenantSchema,
    accepted: z.boolean(),
    comment: z.string().trim().max(2000).nullable(),
    signedAt: z.string().min(1),
  })
  .strict()
export type SignoffRecord = z.infer<typeof signoffRecordSchema>

export const fieldJobDetailSchema = z
  .object({
    job: fieldJobSchema,
    visits: z.array(visitRecordSchema),
    workLines: z.array(workLineSchema),
    signoff: signoffRecordSchema.nullable(),
  })
  .strict()
export type FieldJobDetail = z.infer<typeof fieldJobDetailSchema>

export const fieldJobPageQuerySchema = z
  .object({
    status: z.enum(JOB_STATUSES).optional(),
    limit: z.number().int().positive().max(200).default(50),
    cursor: fieldTenantSchema.optional(),
  })
  .strict()
export type FieldJobPageQuery = z.infer<typeof fieldJobPageQuerySchema>

export class FieldServicesError extends Error {
  constructor(
    readonly code:
      | 'validation_failed'
      | 'forbidden'
      | 'not_found'
      | 'conflict'
      | 'illegal_transition'
      | 'unknown_effect',
    message: string,
  ) {
    super(message)
    this.name = 'FieldServicesError'
  }
}

/** Command envelope. Idempotency keys make retries replay rather than repeat. */
export const fieldCommandSchema = z.discriminatedUnion('action', [
  z
    .object({
      action: z.literal('create_job'),
      commandId: fieldTenantSchema,
      job: z
        .object({
          jobId: fieldTenantSchema,
          customerRef: fieldTenantSchema,
          siteRef: fieldTenantSchema,
          summary: z.string().trim().min(1).max(500),
          priority: z.enum(PRIORITIES).default('normal'),
          appointmentRef: fieldTenantSchema,
          assetRef: fieldTenantSchema.nullable().default(null),
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      action: z.literal('assign_job'),
      commandId: fieldTenantSchema,
      jobId: fieldTenantSchema,
      assignedPrincipal: fieldTenantSchema,
    })
    .strict(),
  z
    .object({
      action: z.literal('record_visit'),
      commandId: fieldTenantSchema,
      jobId: fieldTenantSchema,
      visit: z
        .object({
          visitId: fieldTenantSchema,
          outcome: z.enum(VISIT_OUTCOMES),
          note: z.string().trim().min(1).max(2000),
          technicianRef: fieldTenantSchema,
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      action: z.literal('append_work_line'),
      commandId: fieldTenantSchema,
      jobId: fieldTenantSchema,
      line: workLineSchema,
    })
    .strict(),
  z
    .object({
      action: z.literal('record_signoff'),
      commandId: fieldTenantSchema,
      jobId: fieldTenantSchema,
      signoff: z
        .object({
          signoffId: fieldTenantSchema,
          signerRef: fieldTenantSchema,
          accepted: z.boolean(),
          comment: z.string().trim().max(2000).nullable().default(null),
        })
        .strict(),
    })
    .strict(),
  z
    .object({
      action: z.literal('approve_job'),
      commandId: fieldTenantSchema,
      jobId: fieldTenantSchema,
    })
    .strict(),
  z
    .object({
      action: z.literal('cancel_job'),
      commandId: fieldTenantSchema,
      jobId: fieldTenantSchema,
      reason: z.string().trim().min(1).max(500),
    })
    .strict(),
])
export type FieldCommand = z.infer<typeof fieldCommandSchema>
export type FieldMutation = FieldCommand

export const JOB_ACTIONS = [
  'create_job',
  'assign_job',
  'record_visit',
  'append_work_line',
  'record_signoff',
  'approve_job',
  'cancel_job',
] as const
export type JobAction = (typeof JOB_ACTIONS)[number]
export const jobActionSchema = z.enum(JOB_ACTIONS)

export const fieldReceiptSchema = z
  .object({
    commandId: fieldTenantSchema,
    action: jobActionSchema,
    jobId: fieldTenantSchema,
    status: z.enum(JOB_STATUSES),
    revision: z.number().int().nonnegative(),
    recordedAt: z.string().min(1),
  })
  .strict()
export type FieldReceipt = z.infer<typeof fieldReceiptSchema>

export const fieldCommandStatusSchema = z
  .object({
    commandId: fieldTenantSchema,
    action: jobActionSchema,
    jobId: fieldTenantSchema,
    status: z.enum(['accepted', 'rejected', 'unknown']),
    revision: z.number().int().nonnegative().nullable(),
    recordedAt: z.string().min(1).nullable(),
  })
  .strict()
export type FieldCommandStatus = z.infer<typeof fieldCommandStatusSchema>

export const fieldAuditEntrySchema = z
  .object({
    entryId: fieldTenantSchema,
    jobId: fieldTenantSchema,
    action: jobActionSchema,
    actorRef: fieldTenantSchema,
    commandId: fieldTenantSchema,
    at: z.string().min(1),
  })
  .strict()
export type FieldAuditEntry = z.infer<typeof fieldAuditEntrySchema>

/** The repository port the PostgreSQL adapter satisfies. */
export interface FieldRepository {
  execute(tenant: string, principal: FieldPrincipal, command: FieldCommand): Promise<FieldReceipt>
  getCommandStatus(
    tenant: string,
    principal: FieldPrincipal,
    commandId: string,
  ): Promise<FieldCommandStatus>
  getAccess(tenant: string, principal: FieldPrincipal): Promise<FieldAccess>
  listJobs(
    tenant: string,
    principal: FieldPrincipal,
    query: FieldJobPageQuery,
  ): Promise<readonly FieldJob[]>
  getJobDetail(
    tenant: string,
    principal: FieldPrincipal,
    jobId: string,
  ): Promise<FieldJobDetail | null>
  listAudit(
    tenant: string,
    principal: FieldPrincipal,
    jobId: string,
  ): Promise<readonly FieldAuditEntry[]>
}
