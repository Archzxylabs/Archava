import { z } from 'zod'
import { ProposalError, proposalPageSchema, type ProposalPrincipal } from './contracts.js'
import type { ProposalService } from './service.js'

export function createProposalTools(
  service: ProposalService,
  tenant: string,
  principal: ProposalPrincipal,
) {
  return {
    async invoke(tool: string, input: unknown) {
      if (tool === 'proposals.list') {
        const query = proposalPageSchema.parse(input ?? {})
        return await service.list(tenant, principal, query)
      }
      if (tool === 'proposals.detail') {
        const { id } = z.object({ id: z.uuid() }).strict().parse(input)
        return await service.detail(tenant, principal, id)
      }
      if (tool === 'proposals.versions') {
        const { proposalId } = z.object({ proposalId: z.uuid() }).strict().parse(input)
        return await service.listVersions(tenant, principal, proposalId)
      }
      if (tool === 'proposals.changeRequests') {
        const { proposalId } = z.object({ proposalId: z.uuid() }).strict().parse(input)
        return await service.listChangeRequests(tenant, principal, proposalId)
      }
      throw new ProposalError('not_found')
    },
  }
}
