import { z } from 'zod'

export class ProposalError extends Error {
  constructor(
    public readonly code:
      | 'forbidden'
      | 'not_found'
      | 'conflict'
      | 'invalid_request'
      | 'unauthorized'
      | 'invalid_state'
      | 'scope_frozen'
      | 'active_change_request_exists',
    message?: string,
  ) {
    super(message ?? code)
    this.name = 'ProposalError'
  }
}

export const proposalTenantSchema = z
  .string()
  .regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/)
  .max(100)

export const proposalPrincipalSchema = z.discriminatedUnion('kind', [
  z.object({ kind: z.literal('human'), id: proposalTenantSchema }).strict(),
  z
    .object({
      kind: z.literal('service'),
      id: proposalTenantSchema,
      onBehalfOf: proposalTenantSchema,
    })
    .strict(),
])
export type ProposalPrincipal = z.infer<typeof proposalPrincipalSchema>

export const PROPOSAL_PERMISSIONS = [
  'proposals.read',
  'proposals.write',
  'proposals.decide',
  'changerequests.read',
  'changerequests.write',
  'changerequests.decide',
  'audit.read',
] as const
export type ProposalPermission = (typeof PROPOSAL_PERMISSIONS)[number]

export const proposalMembershipSchema = z
  .object({
    principalId: proposalTenantSchema,
    kind: z.enum(['human', 'service']),
    permissions: z.array(z.enum(PROPOSAL_PERMISSIONS)),
    scope: z.enum(['all', 'assigned']),
    active: z.boolean(),
  })
  .strict()
export type ProposalMembership = z.infer<typeof proposalMembershipSchema>

export const proposalAccessSchema = z
  .object({
    permissions: z.array(z.enum(PROPOSAL_PERMISSIONS)),
    ownerId: proposalTenantSchema,
    scope: z.enum(['all', 'assigned']),
  })
  .strict()
export type ProposalAccess = z.infer<typeof proposalAccessSchema>

const titleText = z.string().trim().min(1).max(160)
const summaryText = z.string().trim().min(1).max(2000)
const optionalText2000 = z.string().trim().min(1).max(2000).nullable()
const currencyCode = z
  .string()
  .trim()
  .regex(/^[A-Z]{3}$/)
const revisionNumber = z.number().int().positive().max(2147483647)
const nonNegativeInt = z.number().int().min(0).max(2147483647)
const intDelta = z.number().int().min(-2147483647).max(2147483647)
export const instantSchema = z.iso.datetime({ offset: true })

export const PROPOSAL_STATES = ['draft', 'submitted', 'accepted', 'rejected', 'withdrawn'] as const
export const proposalStateSchema = z.enum(PROPOSAL_STATES)
export type ProposalState = z.infer<typeof proposalStateSchema>

export const PROPOSAL_TRANSITIONS: Readonly<Record<ProposalState, readonly ProposalState[]>> = {
  draft: ['submitted', 'withdrawn'],
  submitted: ['accepted', 'rejected', 'withdrawn'],
  accepted: [],
  rejected: ['draft'],
  withdrawn: [],
}

export const DELIVERABLE_TYPES = ['milestone', 'service', 'license', 'contingency'] as const
export const deliverableTypeSchema = z.enum(DELIVERABLE_TYPES)
export type DeliverableType = z.infer<typeof deliverableTypeSchema>

export const proposalScopeItemSchema = z
  .object({
    id: z.uuid(),
    proposalId: z.uuid(),
    scopeVersion: z.number().int().positive().max(2147483647),
    title: titleText,
    description: summaryText,
    estimatedDays: nonNegativeInt.nullable(),
    estimatedValue: nonNegativeInt,
    deliverableType: deliverableTypeSchema,
    createdAt: instantSchema,
  })
  .strict()
export type ProposalScopeItem = z.infer<typeof proposalScopeItemSchema>

export const proposalScopeVersionSchema = z
  .object({
    id: z.uuid(),
    proposalId: z.uuid(),
    scopeVersion: z.number().int().positive().max(2147483647),
    changeRequestId: z.uuid().nullable(),
    summary: summaryText,
    totalValue: nonNegativeInt,
    totalDays: nonNegativeInt,
    confirmedAt: instantSchema,
    confirmedBy: proposalTenantSchema,
    items: z.array(proposalScopeItemSchema),
  })
  .strict()
export type ProposalScopeVersion = z.infer<typeof proposalScopeVersionSchema>

export const proposalSchema = z
  .object({
    id: z.uuid(),
    tenantId: proposalTenantSchema,
    title: titleText,
    summary: summaryText,
    organizationId: z.uuid().nullable(),
    contactId: proposalTenantSchema.nullable(),
    leadId: proposalTenantSchema.nullable(),
    currency: currencyCode,
    totalEstimatedValue: nonNegativeInt,
    state: proposalStateSchema,
    currentScopeVersion: z.number().int().positive().max(2147483647),
    acceptedAt: instantSchema.nullable(),
    acceptedBy: proposalTenantSchema.nullable(),
    linkedProjectId: z.uuid().nullable(),
    ownerId: proposalTenantSchema,
    revision: revisionNumber,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type Proposal = z.infer<typeof proposalSchema>

export const CHANGE_REQUEST_STATES = [
  'draft',
  'submitted',
  'approved',
  'rejected',
  'withdrawn',
] as const
export const changeRequestStateSchema = z.enum(CHANGE_REQUEST_STATES)
export type ChangeRequestState = z.infer<typeof changeRequestStateSchema>

export const CHANGE_REQUEST_TRANSITIONS: Readonly<
  Record<ChangeRequestState, readonly ChangeRequestState[]>
> = {
  draft: ['submitted', 'withdrawn'],
  submitted: ['approved', 'rejected', 'withdrawn'],
  approved: [],
  rejected: [],
  withdrawn: [],
}

export const proposalChangeRequestSchema = z
  .object({
    id: z.uuid(),
    tenantId: proposalTenantSchema,
    proposalId: z.uuid(),
    changeNumber: z.number().int().positive().max(2147483647),
    title: titleText,
    reason: summaryText,
    deltaSummary: summaryText,
    timeDeltaDays: intDelta,
    costDeltaValue: intDelta,
    resultingScopeVersion: z.number().int().positive().max(2147483647).nullable(),
    state: changeRequestStateSchema,
    submittedAt: instantSchema.nullable(),
    decidedAt: instantSchema.nullable(),
    decidedBy: proposalTenantSchema.nullable(),
    ownerId: proposalTenantSchema,
    revision: revisionNumber,
    createdAt: instantSchema,
    updatedAt: instantSchema,
  })
  .strict()
export type ProposalChangeRequest = z.infer<typeof proposalChangeRequestSchema>

export const scopeItemDraftInputSchema = z
  .object({
    title: titleText,
    description: summaryText,
    estimatedDays: nonNegativeInt.nullable().default(null),
    estimatedValue: nonNegativeInt.default(0),
    deliverableType: deliverableTypeSchema.default('service'),
  })
  .strict()
export type ScopeItemDraftInput = z.infer<typeof scopeItemDraftInputSchema>

export const proposalMutationSchema = z.discriminatedUnion('type', [
  z
    .object({
      type: z.literal('create_proposal'),
      commandId: z.uuid(),
      title: titleText,
      summary: summaryText,
      organizationId: z.uuid().nullable().default(null),
      contactId: proposalTenantSchema.nullable().default(null),
      leadId: proposalTenantSchema.nullable().default(null),
      currency: currencyCode.default('IDR'),
      scopeItems: z.array(scopeItemDraftInputSchema).min(1).max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('update_proposal'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
      title: titleText,
      summary: summaryText,
      organizationId: z.uuid().nullable().optional(),
      contactId: proposalTenantSchema.nullable().optional(),
      leadId: proposalTenantSchema.nullable().optional(),
    })
    .strict(),
  z
    .object({
      type: z.literal('set_scope_items'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
      scopeItems: z.array(scopeItemDraftInputSchema).min(1).max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('submit_proposal'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
    })
    .strict(),
  z
    .object({
      type: z.literal('accept_proposal'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
      linkedProjectId: z.uuid().nullable().default(null),
    })
    .strict(),
  z
    .object({
      type: z.literal('reject_proposal'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
      reason: optionalText2000.default(null),
    })
    .strict(),
  z
    .object({
      type: z.literal('withdraw_proposal'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
      reason: optionalText2000.default(null),
    })
    .strict(),
  z
    .object({
      type: z.literal('link_project'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      expectedRevision: revisionNumber,
      projectId: z.uuid(),
    })
    .strict(),
  z
    .object({
      type: z.literal('create_change_request'),
      commandId: z.uuid(),
      proposalId: z.uuid(),
      title: titleText,
      reason: summaryText,
      deltaSummary: summaryText,
      timeDeltaDays: intDelta.default(0),
      costDeltaValue: intDelta.default(0),
      newScopeItems: z.array(scopeItemDraftInputSchema).min(1).max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('update_change_request'),
      commandId: z.uuid(),
      changeRequestId: z.uuid(),
      expectedRevision: revisionNumber,
      title: titleText,
      reason: summaryText,
      deltaSummary: summaryText,
      timeDeltaDays: intDelta,
      costDeltaValue: intDelta,
      newScopeItems: z.array(scopeItemDraftInputSchema).min(1).max(100),
    })
    .strict(),
  z
    .object({
      type: z.literal('submit_change_request'),
      commandId: z.uuid(),
      changeRequestId: z.uuid(),
      expectedRevision: revisionNumber,
    })
    .strict(),
  z
    .object({
      type: z.literal('approve_change_request'),
      commandId: z.uuid(),
      changeRequestId: z.uuid(),
      expectedRevision: revisionNumber,
    })
    .strict(),
  z
    .object({
      type: z.literal('reject_change_request'),
      commandId: z.uuid(),
      changeRequestId: z.uuid(),
      expectedRevision: revisionNumber,
      reason: optionalText2000.default(null),
    })
    .strict(),
  z
    .object({
      type: z.literal('withdraw_change_request'),
      commandId: z.uuid(),
      changeRequestId: z.uuid(),
      expectedRevision: revisionNumber,
      reason: optionalText2000.default(null),
    })
    .strict(),
])
export type ProposalMutation = z.infer<typeof proposalMutationSchema>

export const proposalPageSchema = z
  .object({
    limit: z.number().int().positive().max(100).default(50),
    cursor: z.string().max(200).optional(),
    state: proposalStateSchema.optional(),
    organizationId: z.uuid().optional(),
    search: z.string().trim().max(100).optional(),
  })
  .strict()
export type ProposalPageQuery = z.infer<typeof proposalPageSchema>

export const commandReceiptSchema = z
  .object({
    commandId: z.uuid(),
    committedAt: instantSchema,
    proposalId: z.uuid().optional(),
    changeRequestId: z.uuid().optional(),
    scopeVersion: z.number().int().positive().optional(),
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

export interface ProposalDetail {
  readonly proposal: Proposal
  readonly currentItems: readonly ProposalScopeItem[]
  readonly versions: readonly ProposalScopeVersion[]
  readonly changeRequests: readonly ProposalChangeRequest[]
}

export interface ProposalRepository {
  access(tenant: string, principal: ProposalPrincipal): Promise<ProposalAccess>
  list(
    tenant: string,
    principal: ProposalPrincipal,
    query: ProposalPageQuery,
  ): Promise<{ items: readonly Proposal[]; nextCursor?: string }>
  detail(tenant: string, principal: ProposalPrincipal, proposalId: string): Promise<ProposalDetail>
  listVersions(
    tenant: string,
    principal: ProposalPrincipal,
    proposalId: string,
  ): Promise<readonly ProposalScopeVersion[]>
  listChangeRequests(
    tenant: string,
    principal: ProposalPrincipal,
    proposalId: string,
  ): Promise<readonly ProposalChangeRequest[]>
  mutate(
    tenant: string,
    principal: ProposalPrincipal,
    mutation: ProposalMutation,
  ): Promise<CommandReceipt>
  commandStatus(
    tenant: string,
    principal: ProposalPrincipal,
    commandId: string,
  ): Promise<CommandStatusResult>
}
