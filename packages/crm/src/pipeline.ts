import { z } from 'zod'
import { CrmService } from './service.js'
import {
  CrmError,
  principalSchema,
  tenantSchema,
  idSchema,
  type CrmPrincipal,
} from './contracts.js'
import {
  opportunityMutationSchema,
  pipelinePageSchema,
  type CrmPipelineRepository,
} from './pipeline-contracts.js'
export * from './pipeline-contracts.js'

/** Optional CRM capability; every operation uses the same membership/resource authority. */
export class CrmPipelineService {
  private readonly crm: CrmService
  constructor(private readonly repository: CrmPipelineRepository) {
    this.crm = new CrmService(repository)
  }
  access(tenant: unknown, principal: unknown) {
    return this.crm.access(tenant, principal)
  }
  async list(tenant: unknown, principal: unknown, raw: unknown = {}) {
    if (!principal) throw new CrmError('unauthenticated', 'CRM authentication required.')
    return this.repository.listOpportunities(
      tenantSchema.parse(tenant),
      principalSchema.parse(principal),
      pipelinePageSchema.parse(raw),
    )
  }
  async detail(tenant: unknown, principal: unknown, rawId: unknown) {
    if (!principal) throw new CrmError('unauthenticated', 'CRM authentication required.')
    return this.repository.opportunityDetail(
      tenantSchema.parse(tenant),
      principalSchema.parse(principal),
      idSchema.parse(rawId),
    )
  }
  async mutate(tenant: unknown, principal: unknown, raw: unknown) {
    return this.crm.mutate(tenant, principal, opportunityMutationSchema.parse(raw))
  }
}
export function createPipelineTools(
  service: CrmPipelineService,
  tenant: string,
  principal: CrmPrincipal,
) {
  return {
    async invoke(name: string, input: unknown) {
      if (name === 'crm.opportunities.list') {
        const page = await service.list(tenant, principal, input)
        return {
          items: page.items.map(({ id, contactId, title, stage, revision }) => ({
            id,
            contactId,
            title,
            stage,
            revision,
          })),
          next: page.next,
        }
      }
      if (name === 'crm.opportunity.summary') {
        const { id } = z.object({ id: idSchema }).strict().parse(input)
        const detail = await service.detail(tenant, principal, id)
        return {
          id,
          title: detail.opportunity.title,
          stage: detail.opportunity.stage,
          revision: detail.opportunity.revision,
          transitions: detail.transitions.map(({ from, to, reason, revision, occurredAt }) => ({
            from,
            to,
            reason,
            revision,
            occurredAt,
          })),
        }
      }
      throw new CrmError('forbidden', 'Unsupported pipeline tool.')
    },
  }
}
