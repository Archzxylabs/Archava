import { z } from 'zod'
export const supportIdSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)
export const supportPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: supportIdSchema }).strict(),
  z
    .object({ kind: z.literal('service'), id: supportIdSchema, onBehalfOf: supportIdSchema })
    .strict(),
])
export type SupportPrincipal = z.infer<typeof supportPrincipalSchema>
export const SUPPORT_PERMISSIONS = [
  'cases.read',
  'cases.write',
  'cases.assign',
  'notes.read',
  'notes.write',
  'audit.read',
] as const
export type SupportPermission = (typeof SUPPORT_PERMISSIONS)[number]
export const supportMembershipSchema = z
  .object({
    principalId: supportIdSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(SUPPORT_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export const supportAccessSchema = z
  .object({
    permissions: z.array(z.enum(SUPPORT_PERMISSIONS)),
    ownerId: supportIdSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type SupportAccess = z.infer<typeof supportAccessSchema>
const title = z.string().trim().min(1).max(160),
  body = z.string().trim().min(1).max(4000),
  reason = z.string().trim().min(1).max(2000),
  revision = z.number().int().positive().max(2147483647)
export const SUPPORT_STATES = [
  'open',
  'in_progress',
  'waiting',
  'resolved',
  'closed',
  'cancelled',
] as const
export const supportPrioritySchema = z.enum(['low', 'normal', 'high'])
export const supportInstantSchema = z.iso
  .datetime({ offset: true })
  .refine(
    (x) =>
      Number.isFinite(Date.parse(x)) &&
      Date.parse(x) >= Date.UTC(2000, 0, 1) &&
      Date.parse(x) < Date.UTC(2100, 0, 1),
    'Unsupported target date',
  )
export const caseSchema = z
  .object({
    id: z.uuid(),
    tenantId: supportIdSchema,
    ownerId: supportIdSchema,
    assigneeId: supportIdSchema.nullable(),
    requesterName: title,
    subject: title,
    description: body,
    priority: supportPrioritySchema,
    state: z.enum(SUPPORT_STATES),
    dueAt: z.iso.datetime().nullable(),
    overdue: z.boolean(),
    cycle: revision,
    resolution: body.nullable(),
    resolvedAt: z.iso.datetime().nullable(),
    closedAt: z.iso.datetime().nullable(),
    revision,
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type SupportCase = z.infer<typeof caseSchema>
export const supportNoteSchema = z
  .object({
    id: z.uuid(),
    caseId: z.uuid(),
    authorId: supportIdSchema,
    body,
    requestId: z.uuid(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const supportTransitionSchema = z
  .object({
    requestId: z.uuid(),
    actorId: supportIdSchema,
    operation: z.string().min(1).max(100),
    previousAssigneeId: supportIdSchema.nullable(),
    assigneeId: supportIdSchema.nullable(),
    previousPriority: supportPrioritySchema.nullable(),
    priority: supportPrioritySchema,
    previousDueAt: z.iso.datetime().nullable(),
    dueAt: z.iso.datetime().nullable(),
    from: z.enum(SUPPORT_STATES).nullable(),
    to: z.enum(SUPPORT_STATES),
    cycle: revision,
    previousResolution: body.nullable(),
    resolution: body.nullable(),
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const supportAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: supportIdSchema,
    operation: z.string().min(1).max(100),
    recordId: z.uuid(),
    revision,
    reason: reason.nullable(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export const caseDetailSchema = z
  .object({
    record: caseSchema,
    notes: z.array(supportNoteSchema).max(100).nullable(),
    notesHasMore: z.boolean(),
    transitions: z.array(supportTransitionSchema).max(100).nullable(),
    transitionsHasMore: z.boolean(),
    audit: z.array(supportAuditSchema).max(100).nullable(),
    auditHasMore: z.boolean(),
  })
  .strict()
export type CaseDetail = z.infer<typeof caseDetailSchema>
export const casePageSchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    after: z.uuid().optional(),
    state: z.enum(SUPPORT_STATES).optional(),
    priority: supportPrioritySchema.optional(),
    overdue: z.boolean().optional(),
  })
  .strict()
export const assigneePageSchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(50),
    after: supportIdSchema.optional(),
  })
  .strict()
export type AssigneePageQuery = z.infer<typeof assigneePageSchema>
export const assigneeSchema = z
  .object({ id: supportIdSchema, displayName: z.string().min(1).max(254) })
  .strict()
export interface AssigneePage {
  items: readonly z.infer<typeof assigneeSchema>[]
  next: string | null
}
export type CasePageQuery = z.infer<typeof casePageSchema>
export interface CasePage {
  items: readonly SupportCase[]
  next: string | null
  evaluatedAt: string
}
const target = { id: z.uuid(), expectedRevision: revision }
export const supportCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('case.create'),
      requesterName: title,
      subject: title,
      description: body,
      priority: supportPrioritySchema.default('normal'),
      dueAt: supportInstantSchema.nullable().default(null),
    })
    .strict(),
  ...(['case.start', 'case.wait', 'case.resume', 'case.close', 'case.cancel'] as const).map(
    (operation) => z.object({ operation: z.literal(operation), ...target, reason }).strict(),
  ),
  z.object({ operation: z.literal('case.resolve'), ...target, resolution: body }).strict(),
  z
    .object({
      operation: z.literal('case.reopen'),
      ...target,
      reason,
      dueAt: supportInstantSchema.nullable().default(null),
    })
    .strict(),
  z
    .object({
      operation: z.literal('case.assign'),
      ...target,
      assigneeId: supportIdSchema.nullable(),
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('case.set-priority'),
      ...target,
      priority: supportPrioritySchema,
      reason,
    })
    .strict(),
  z
    .object({
      operation: z.literal('case.set-target'),
      ...target,
      dueAt: supportInstantSchema.nullable(),
      reason,
    })
    .strict(),
  z.object({ operation: z.literal('note.add'), ...target, body }).strict(),
])
export const supportMutationSchema = z
  .object({ requestId: z.uuid(), command: supportCommandSchema })
  .strict()
export type SupportMutation = z.infer<typeof supportMutationSchema>
export type SupportCommand = SupportMutation['command']
export const supportReceiptSchema = z
  .object({
    requestId: z.uuid(),
    operation: z.string().min(1).max(100),
    id: z.uuid(),
    revision,
    replayed: z.boolean(),
  })
  .strict()
export type SupportReceipt = z.infer<typeof supportReceiptSchema>
export const supportCommandStatusSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('unknown') }).strict(),
  z.object({ status: z.literal('committed'), receipt: supportReceiptSchema }).strict(),
])
export type SupportCommandStatus = z.infer<typeof supportCommandStatusSchema>
export class SupportError extends Error {
  constructor(readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable') {
    super(code)
    this.name = 'SupportError'
  }
}
export interface SupportRepository {
  access(tenant: string, principal: SupportPrincipal): Promise<SupportAccess>
  assignees(
    tenant: string,
    principal: SupportPrincipal,
    query: AssigneePageQuery,
  ): Promise<AssigneePage>
  cases(tenant: string, principal: SupportPrincipal, query: CasePageQuery): Promise<CasePage>
  detail(tenant: string, principal: SupportPrincipal, id: string): Promise<CaseDetail>
  mutate(
    tenant: string,
    principal: SupportPrincipal,
    input: SupportMutation,
  ): Promise<SupportReceipt>
  commandStatus(
    tenant: string,
    principal: SupportPrincipal,
    id: string,
  ): Promise<SupportCommandStatus>
}
