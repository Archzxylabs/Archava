import { z } from 'zod'

export const projectTenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)
export const projectPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: projectTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: projectTenantSchema,
      onBehalfOf: projectTenantSchema,
    })
    .strict(),
])
export type ProjectPrincipal = z.infer<typeof projectPrincipalSchema>
export const PROJECT_PERMISSIONS = [
  'projects.read',
  'projects.write',
  'tasks.read',
  'tasks.write',
  'milestones.read',
  'milestones.write',
  'audit.read',
] as const
export type ProjectPermission = (typeof PROJECT_PERMISSIONS)[number]
export const projectMembershipSchema = z
  .object({
    principalId: projectTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(PROJECT_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export interface ProjectAccess {
  readonly permissions: readonly ProjectPermission[]
  readonly ownerId: string
  readonly scope: 'all' | 'assigned'
}
export const PROJECT_STATES = [
  'draft',
  'active',
  'review',
  'delivered',
  'closed',
  'cancelled',
] as const
export const projectStateSchema = z.enum(PROJECT_STATES)
export type ProjectState = z.infer<typeof projectStateSchema>
export const PROJECT_TRANSITIONS: Readonly<Record<ProjectState, readonly ProjectState[]>> = {
  draft: ['active', 'cancelled'],
  active: ['review', 'cancelled'],
  review: ['active', 'delivered', 'cancelled'],
  delivered: ['closed'],
  closed: [],
  cancelled: [],
}
export const TASK_STATES = ['todo', 'doing', 'done', 'cancelled'] as const
export const taskStateSchema = z.enum(TASK_STATES)
export type TaskState = z.infer<typeof taskStateSchema>
export const TASK_TRANSITIONS: Readonly<Record<TaskState, readonly TaskState[]>> = {
  todo: ['doing', 'done', 'cancelled'],
  doing: ['todo', 'done', 'cancelled'],
  done: ['doing'],
  cancelled: ['todo'],
}
const title = z.string().trim().min(1).max(160)
const summary = z.string().trim().min(1).max(2000)
const revision = z.number().int().positive().max(2_147_483_647)
const instant = z.iso.datetime()
export const projectSchema = z
  .object({
    id: z.uuid(),
    tenantId: projectTenantSchema,
    ownerId: projectTenantSchema,
    title,
    scopeSummary: summary,
    scopeVersion: z.literal(1),
    scopeConfirmedAt: instant.nullable(),
    scopeConfirmedBy: projectTenantSchema.nullable(),
    state: projectStateSchema,
    revision,
    dueAt: instant.nullable(),
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type Project = z.infer<typeof projectSchema>
export const projectTaskSchema = z
  .object({
    id: z.uuid(),
    tenantId: projectTenantSchema,
    projectId: z.uuid(),
    milestoneId: z.uuid().nullable(),
    title,
    required: z.boolean(),
    state: taskStateSchema,
    revision,
    dueAt: instant.nullable(),
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type ProjectTask = z.infer<typeof projectTaskSchema>
export const projectMilestoneSchema = z
  .object({
    id: z.uuid(),
    tenantId: projectTenantSchema,
    projectId: z.uuid(),
    title,
    required: z.boolean(),
    state: z.enum(['open', 'reached']),
    revision,
    dueAt: instant.nullable(),
    reachedAt: instant.nullable(),
    evidence: summary.nullable(),
    createdAt: instant,
    updatedAt: instant,
  })
  .strict()
export type ProjectMilestone = z.infer<typeof projectMilestoneSchema>
const projectRef = { projectId: z.uuid(), projectRevision: revision }
export const projectCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('project.create'),
      title,
      scopeSummary: summary,
      dueAt: instant.nullable().default(null),
    })
    .strict(),
  z
    .object({
      operation: z.literal('project.transition'),
      ...projectRef,
      state: projectStateSchema,
      reason: summary.optional(),
    })
    .strict()
    .refine((x) => x.state !== 'cancelled' || !!x.reason, 'Cancellation reason required'),
  z
    .object({
      operation: z.literal('task.create'),
      ...projectRef,
      title,
      milestoneId: z.uuid().nullable().default(null),
      required: z.boolean().default(true),
      dueAt: instant.nullable().default(null),
    })
    .strict(),
  z
    .object({
      operation: z.literal('task.transition'),
      ...projectRef,
      id: z.uuid(),
      expectedRevision: revision,
      state: taskStateSchema,
      reason: summary.optional(),
    })
    .strict()
    .refine((x) => x.state !== 'cancelled' || !!x.reason, 'Cancellation reason required'),
  z
    .object({
      operation: z.literal('milestone.create'),
      ...projectRef,
      title,
      required: z.boolean().default(true),
      dueAt: instant.nullable().default(null),
    })
    .strict(),
  z
    .object({
      operation: z.literal('milestone.complete'),
      ...projectRef,
      id: z.uuid(),
      expectedRevision: revision,
      evidence: summary,
    })
    .strict(),
])
export type ProjectCommand = z.infer<typeof projectCommandSchema>
export const projectMutationSchema = z
  .object({ requestId: z.uuid(), command: projectCommandSchema })
  .strict()
export type ProjectMutation = z.infer<typeof projectMutationSchema>
export const projectReceiptSchema = z
  .object({
    kind: z.enum(['project', 'task', 'milestone']),
    id: z.uuid(),
    projectId: z.uuid(),
    revision,
    projectRevision: revision,
  })
  .strict()
export type ProjectReceipt = z.infer<typeof projectReceiptSchema>
export type ProjectCommandStatus =
  | { status: 'committed'; requestId: string; receipt: ProjectReceipt }
  | { status: 'not_committed'; requestId: string }
export const projectPageSchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(20),
    after: z.uuid().optional(),
    search: z.string().trim().max(120).default(''),
    state: projectStateSchema.optional(),
  })
  .strict()
export type ProjectPageQuery = z.infer<typeof projectPageSchema>
export interface ProjectPage {
  readonly items: readonly Project[]
  readonly next: string | null
}
export const projectAuditSchema = z
  .object({
    requestId: z.uuid(),
    actorId: projectTenantSchema,
    operation: z.string(),
    projectId: z.uuid(),
    resourceId: z.uuid(),
    revision,
    projectRevision: revision,
    reason: summary.nullable(),
    occurredAt: instant,
  })
  .strict()
export type ProjectAudit = z.infer<typeof projectAuditSchema>
export interface ProjectDetail {
  readonly project: Project
  readonly tasks: readonly ProjectTask[] | null
  readonly milestones: readonly ProjectMilestone[] | null
  readonly audit: readonly ProjectAudit[] | null
  readonly auditHasMore: boolean
}
export const projectAccessSchema = z
  .object({
    permissions: z.array(z.enum(PROJECT_PERMISSIONS)),
    ownerId: projectTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export const projectDetailSchema = z
  .object({
    project: projectSchema,
    tasks: z.array(projectTaskSchema).max(100).nullable(),
    milestones: z.array(projectMilestoneSchema).max(100).nullable(),
    audit: z.array(projectAuditSchema).max(50).nullable(),
    auditHasMore: z.boolean(),
  })
  .strict()
export const projectCommandStatusSchema = z.discriminatedUnion('status', [
  z
    .object({ status: z.literal('committed'), requestId: z.uuid(), receipt: projectReceiptSchema })
    .strict(),
  z.object({ status: z.literal('not_committed'), requestId: z.uuid() }).strict(),
])
export interface ProjectRepository {
  access(tenant: string, principal: ProjectPrincipal): Promise<ProjectAccess>
  list(tenant: string, principal: ProjectPrincipal, query: ProjectPageQuery): Promise<ProjectPage>
  detail(tenant: string, principal: ProjectPrincipal, id: string): Promise<ProjectDetail>
  mutate(
    tenant: string,
    principal: ProjectPrincipal,
    mutation: ProjectMutation,
  ): Promise<ProjectReceipt>
  commandStatus(
    tenant: string,
    principal: ProjectPrincipal,
    requestId: string,
  ): Promise<ProjectCommandStatus>
}
export class ProjectError extends Error {
  readonly code: 'forbidden' | 'not_found' | 'conflict' | 'unavailable'
  constructor(code: ProjectError['code'], message = 'Project request refused.') {
    super(message)
    this.code = code
  }
}
export const PROJECT_MANIFEST = {
  id: 'project_delivery',
  version: '0.1.0',
  apiVersion: '1.0.0',
  owns: ['projects', 'tasks', 'milestones', 'commands', 'audit'],
  permissions: PROJECT_PERMISSIONS,
  sourceOfTruth: 'domain-repository',
  scope:
    'internal scope confirmation and delivery tracking; no client signature, invoice or external effect',
} as const
