import { z } from 'zod'
import {
  proposalTenantSchema,
  proposalPrincipalSchema,
  proposalPageSchema,
  proposalMutationSchema,
  type ProposalRepository,
  type ProposalPrincipal,
  ProposalError,
} from './contracts.js'

export class ProposalService {
  constructor(private readonly repository: ProposalRepository) {}

  access(tenant: string, principal: ProposalPrincipal) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    return this.repository.access(tenant, principal)
  }

  list(tenant: string, principal: ProposalPrincipal, query: unknown = {}) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    return this.repository.list(tenant, principal, proposalPageSchema.parse(query))
  }

  detail(tenant: string, principal: ProposalPrincipal, id: string) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }

  listVersions(tenant: string, principal: ProposalPrincipal, proposalId: string) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    return this.repository.listVersions(tenant, principal, z.uuid().parse(proposalId))
  }

  listChangeRequests(tenant: string, principal: ProposalPrincipal, proposalId: string) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    return this.repository.listChangeRequests(tenant, principal, z.uuid().parse(proposalId))
  }

  mutate(tenant: string, principal: ProposalPrincipal, raw: unknown) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    if (principal.kind !== 'human') {
      throw new ProposalError('forbidden', 'Proposal machine writes are not registered.')
    }
    return this.repository.mutate(tenant, principal, proposalMutationSchema.parse(raw))
  }

  commandStatus(tenant: string, principal: ProposalPrincipal, commandId: string) {
    proposalTenantSchema.parse(tenant)
    proposalPrincipalSchema.parse(principal)
    if (principal.kind !== 'human') {
      throw new ProposalError(
        'forbidden',
        'Proposal machine reads on command status are not permitted.',
      )
    }
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(commandId))
  }
}
