import { z } from 'zod'
import {
  OrganizationError,
  orgTenantSchema,
  orgPrincipalSchema,
  organizationPageSchema,
  affiliationPageSchema,
  organizationMutationSchema,
  type OrganizationPrincipal,
  type OrganizationRepository,
} from './contracts.js'

export class OrganizationService {
  constructor(private readonly repository: OrganizationRepository) {}

  private validate(tenant: string, principal: OrganizationPrincipal) {
    orgTenantSchema.parse(tenant)
    orgPrincipalSchema.parse(principal)
  }

  access(tenant: string, principal: OrganizationPrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }

  listOrganizations(tenant: string, principal: OrganizationPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.listOrganizations(tenant, principal, organizationPageSchema.parse(query))
  }

  detail(tenant: string, principal: OrganizationPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }

  listAffiliations(
    tenant: string,
    principal: OrganizationPrincipal,
    organizationId: string,
    query: unknown = {},
  ) {
    this.validate(tenant, principal)
    return this.repository.listAffiliations(
      tenant,
      principal,
      z.uuid().parse(organizationId),
      affiliationPageSchema.parse(query),
    )
  }

  mutate(tenant: string, principal: OrganizationPrincipal, input: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new OrganizationError('forbidden')
    return this.repository.mutate(tenant, principal, organizationMutationSchema.parse(input))
  }

  commandStatus(tenant: string, principal: OrganizationPrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new OrganizationError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
