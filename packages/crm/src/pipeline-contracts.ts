import { z } from 'zod'
import type { CrmAccess, CrmPrincipal, CrmRepository } from './contracts.js'

export const PIPELINE_STAGES = ['new', 'qualified', 'proposal', 'won', 'lost'] as const
export const pipelineStageSchema = z.enum(PIPELINE_STAGES)
export type PipelineStage = z.infer<typeof pipelineStageSchema>
export const PIPELINE_TRANSITIONS: Readonly<Record<PipelineStage, readonly PipelineStage[]>> = {
  new: ['qualified', 'lost'],
  qualified: ['new', 'proposal', 'lost'],
  proposal: ['qualified', 'won', 'lost'],
  won: [],
  lost: [],
}
export const pipelineLabelsSchema = z
  .object({
    new: z.string().trim().min(1).max(80),
    qualified: z.string().trim().min(1).max(80),
    proposal: z.string().trim().min(1).max(80),
    won: z.string().trim().min(1).max(80),
    lost: z.string().trim().min(1).max(80),
  })
  .strict()
export const DEFAULT_PIPELINE_LABELS = {
  new: 'New',
  qualified: 'Qualified',
  proposal: 'Proposal',
  won: 'Won',
  lost: 'Lost',
}
export const opportunityCommandSchema = z.discriminatedUnion('operation', [
  z
    .object({
      operation: z.literal('opportunity.create'),
      contactId: z.uuid(),
      contactRevision: z.number().int().positive(),
      title: z.string().trim().min(1).max(160),
    })
    .strict(),
  z
    .object({
      operation: z.literal('opportunity.move'),
      id: z.uuid(),
      expectedRevision: z.number().int().positive(),
      stage: pipelineStageSchema,
      reason: z.string().trim().min(1).max(1000).optional(),
    })
    .strict()
    .refine((value) => value.stage !== 'lost' || !!value.reason, 'A loss reason is required.'),
])
export const opportunityMutationSchema = z
  .object({ requestId: z.uuid(), command: opportunityCommandSchema })
  .strict()
export type OpportunityCommand = z.infer<typeof opportunityCommandSchema>
export const opportunitySchema = z
  .object({
    id: z.uuid(),
    tenantId: z.string(),
    contactId: z.uuid(),
    ownerId: z.string(),
    title: z.string().min(1).max(160),
    stage: pipelineStageSchema,
    revision: z.number().int().positive(),
    lossReason: z.string().max(1000).nullable(),
    createdAt: z.iso.datetime(),
    updatedAt: z.iso.datetime(),
  })
  .strict()
export type Opportunity = z.infer<typeof opportunitySchema>
export const transitionSchema = z
  .object({
    requestId: z.uuid(),
    opportunityId: z.uuid(),
    from: pipelineStageSchema.nullable(),
    to: pipelineStageSchema,
    reason: z.string().max(1000).nullable(),
    actorId: z.string(),
    revision: z.number().int().positive(),
    occurredAt: z.iso.datetime(),
  })
  .strict()
export type OpportunityTransition = z.infer<typeof transitionSchema>
export const pipelinePageSchema = z
  .object({
    limit: z.number().int().min(1).max(100).default(20),
    after: z.uuid().optional(),
    stage: pipelineStageSchema.optional(),
    contactId: z.uuid().optional(),
    search: z.string().trim().max(120).default(''),
  })
  .strict()
export type PipelinePage = z.infer<typeof pipelinePageSchema>
export interface OpportunityPage {
  readonly items: readonly Opportunity[]
  readonly next: string | null
}
export interface OpportunityDetail {
  readonly opportunity: Opportunity
  readonly transitions: readonly OpportunityTransition[]
}
export interface CrmPipelineRepository extends CrmRepository {
  listOpportunities(
    tenant: string,
    principal: CrmPrincipal,
    page: PipelinePage,
  ): Promise<OpportunityPage>
  opportunityDetail(tenant: string, principal: CrmPrincipal, id: string): Promise<OpportunityDetail>
}
export const CRM_PIPELINE_MANIFEST = {
  id: 'crm_sales_pipeline',
  version: '0.2.0',
  apiVersion: '1.0.0',
  status: 'implemented' as const,
  requires: ['crm_contacts'],
  permissions: ['pipeline.read', 'pipeline.write'],
  persistence: { adapter: '@archava/crm-pg', migration: '002_sales_pipeline' },
  tools: ['crm.opportunities.list', 'crm.opportunity.summary'],
}
export function canReadPipeline(access: CrmAccess) {
  return (
    access.permissions.includes('pipeline.read') && access.permissions.includes('contacts.read')
  )
}
