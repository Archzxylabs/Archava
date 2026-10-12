import { z } from 'zod'

export class CapacityError extends Error {
  constructor(
    public readonly code:
      | 'forbidden'
      | 'not_found'
      | 'conflict'
      | 'invalid_request'
      | 'unauthorized'
      | 'pool_archived'
      | 'invalid_state'
      | 'insufficient_capacity',
    message?: string,
  ) {
    super(message ?? code)
    this.name = 'CapacityError'
  }
}

export const capacityTenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)

export const capacityPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: capacityTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: capacityTenantSchema,
      onBehalfOf: capacityTenantSchema,
    })
    .strict(),
])
export type CapacityPrincipal = z.infer<typeof capacityPrincipalSchema>

export const CAPACITY_PERMISSIONS = [
  'capacity.read',
  'capacity.write',
  'capacity.allocate',
  'audit.read',
] as const
export type CapacityPermission = (typeof CAPACITY_PERMISSIONS)[number]

export const capacityMembershipSchema = z
  .object({
    principalId: capacityTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(CAPACITY_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type CapacityMembership = z.infer<typeof capacityMembershipSchema>

export const capacityAccessSchema = z
  .object({
    permissions: z.array(z.enum(CAPACITY_PERMISSIONS)),
    ownerId: capacityTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type CapacityAccess = z.infer<typeof capacityAccessSchema>

const codeText = z
  .string()
  .trim()
  .regex(/^[A-Z0-9_]{2,40}$/)
const nameText = z.string().trim().min(1).max(160)
const optionalText2000 = z.string().trim().min(1).max(2000).nullable()
const revisionNumber = z.number().int().positive().max(2147483647)
const positiveNumber = z.number().positive().max(1000000)
const nonNegativeNumber = z.number().min(0).max(1000000)
export const instantSchema = z.iso.datetime({ offset: true })

export const CAPACITY_UNITS = ['hours', 'days', 'slots'] as const
export const capacityUnitSchema = z.enum(CAPACITY_UNITS)
export type CapacityUnit = z.infer<typeof capacityUnitSchema>

export const POOL_STATES = ['active', 'paused', 'archived'] as const
export const poolStateSchema = z.enum(POOL_STATES)
export type PoolState = z.infer<typeof poolStateSchema>

export const capacityPoolSchema = z
  .object({
    id: z.uuid(),
    tenantId: capacityTenantSchema,
    code: codeText,
    name: nameText,
    unit: capacityUnitSchema,
    weeklyCapacity: positiveNumber,
    maxConcurrentJobs: z.number().int().positive().max(100),
    state: poolStateSchema,
    ownerId: capacityTenantSchema,
    revision: revisionNumber,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type CapacityPool = z.infer<typeof capacityPoolSchema>

export const ALLOCATION_STATES = ['scheduled', 'in_progress', 'completed', 'cancelled'] as const
export const allocationStateSchema = z.enum(ALLOCATION_STATES)
export type AllocationState = z.infer<typeof allocationStateSchema>

export const capacityAllocationSchema = z
  .object({
    id: z.uuid(),
    tenantId: capacityTenantSchema,
    poolId: z.uuid(),
    workloadId: z.string().trim().min(1).max(100),
    workloadTitle: nameText,
    requiredUnits: positiveNumber,
    remainingUnits: nonNegativeNumber,
    scheduledStartAt: instantSchema,
    scheduledFinishAt: instantSchema,
    state: allocationStateSchema,
    ownerId: capacityTenantSchema,
    revision: revisionNumber,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type CapacityAllocation = z.infer<typeof capacityAllocationSchema>

export const capacityEstimateRequestSchema = z
  .object({
    poolId: z.uuid(),
    requiredUnits: positiveNumber,
    desiredStartAt: instantSchema.optional(),
  })
  .strict()
export type CapacityEstimateRequest = z.infer<typeof capacityEstimateRequestSchema>

export const capacityEstimateSchema = z
  .object({
    poolId: z.uuid(),
    unit: capacityUnitSchema,
    requiredUnits: positiveNumber,
    queuePosition: z.number().int().min(0),
    estimatedStartAt: instantSchema,
    estimatedFinishAt: instantSchema,
    burnRatePerDay: positiveNumber,
    estimatedCalendarDays: z.number().int().positive(),
  })
  .strict()
export type CapacityEstimate = z.infer<typeof capacityEstimateSchema>

export const capacityMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_pool'),
      commandId: z.uuid(),
      code: codeText,
      name: nameText,
      unit: capacityUnitSchema.default('hours'),
      weeklyCapacity: positiveNumber,
      maxConcurrentJobs: z.number().int().positive().max(100).default(3),
    })
    .strict(),
  z
    .object({
      type: z.literal('update_pool'),
      commandId: z.uuid(),
      poolId: z.uuid(),
      expectedRevision: revisionNumber,
      name: nameText,
      weeklyCapacity: positiveNumber,
      maxConcurrentJobs: z.number().int().positive().max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('archive_pool'),
      commandId: z.uuid(),
      poolId: z.uuid(),
      expectedRevision: revisionNumber,
    })
    .strict(),
  z
    .object({
      type: z.literal('commit_allocation'),
      commandId: z.uuid(),
      poolId: z.uuid(),
      workloadId: z.string().trim().min(1).max(100),
      workloadTitle: nameText,
      requiredUnits: positiveNumber,
      desiredStartAt: instantSchema.optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('update_allocation_progress'),
      commandId: z.uuid(),
      allocationId: z.uuid(),
      expectedRevision: revisionNumber,
      remainingUnits: nonNegativeNumber,
      state: z.enum(['in_progress', 'scheduled']).optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('complete_allocation'),
      commandId: z.uuid(),
      allocationId: z.uuid(),
      expectedRevision: revisionNumber,
    })
    .strict(),
  z
    .object({
      type: z.literal('cancel_allocation'),
      commandId: z.uuid(),
      allocationId: z.uuid(),
      expectedRevision: revisionNumber,
      reason: optionalText2000.default(null),
    })
    .strict(),
])
export type CapacityMutation = z.infer<typeof capacityMutationSchema>

export const capacityPageSchema = z
  .object({
    limit: z.number().int().positive().max(100).default(50),
    cursor: z.string().max(200).optional(),
    state: poolStateSchema.optional(),
    search: z.string().trim().max(100).optional(),
  })
  .strict()
export type CapacityPageQuery = z.infer<typeof capacityPageSchema>

export const commandReceiptSchema = z
  .object({
    commandId: z.uuid(),
    committedAt: instantSchema,
    poolId: z.uuid().optional(),
    allocationId: z.uuid().optional(),
    revision: revisionNumber,
    status: z.enum(['committed', 'conflict']),
  })
  .strict()
export type CommandReceipt = z.infer<typeof commandReceiptSchema>

export const commandStatusResultSchema = z.discriminatedUnion('status', [
  z.object({ status: z.literal('committed'), receipt: commandReceiptSchema }).strict(),
  z.object({ status: z.literal('unknown') }).strict(),
])
export type CommandStatusResult = z.infer<typeof commandStatusResultSchema>

export interface CapacityPoolDetail {
  readonly pool: CapacityPool
  readonly activeAllocations: readonly CapacityAllocation[]
  readonly currentLoadHours: number
}

export interface CapacityRepository {
  access(tenant: string, principal: CapacityPrincipal): Promise<CapacityAccess>
  listPools(
    tenant: string,
    principal: CapacityPrincipal,
    query: CapacityPageQuery,
  ): Promise<{ items: readonly CapacityPool[]; nextCursor?: string }>
  detailPool(
    tenant: string,
    principal: CapacityPrincipal,
    poolId: string,
  ): Promise<CapacityPoolDetail>
  estimate(
    tenant: string,
    principal: CapacityPrincipal,
    request: CapacityEstimateRequest,
  ): Promise<CapacityEstimate>
  mutate(
    tenant: string,
    principal: CapacityPrincipal,
    mutation: CapacityMutation,
  ): Promise<CommandReceipt>
  commandStatus(
    tenant: string,
    principal: CapacityPrincipal,
    commandId: string,
  ): Promise<CommandStatusResult>
}
