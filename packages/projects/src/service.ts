import {
  projectTenantSchema,
  projectPrincipalSchema,
  projectPageSchema,
  projectMutationSchema,
  type ProjectRepository,
  type ProjectPrincipal,
  ProjectError,
} from './contracts.js'
import { z } from 'zod'

export class ProjectService {
  private readonly repository: ProjectRepository
  constructor(repository: ProjectRepository) {
    this.repository = repository
  }
  access(tenant: string, principal: ProjectPrincipal) {
    projectTenantSchema.parse(tenant)
    projectPrincipalSchema.parse(principal)
    return this.repository.access(tenant, principal)
  }
  list(tenant: string, principal: ProjectPrincipal, query: unknown = {}) {
    projectTenantSchema.parse(tenant)
    projectPrincipalSchema.parse(principal)
    return this.repository.list(tenant, principal, projectPageSchema.parse(query))
  }
  detail(tenant: string, principal: ProjectPrincipal, id: string) {
    projectTenantSchema.parse(tenant)
    projectPrincipalSchema.parse(principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }
  mutate(tenant: string, principal: ProjectPrincipal, raw: unknown) {
    projectTenantSchema.parse(tenant)
    projectPrincipalSchema.parse(principal)
    if (principal.kind !== 'human')
      throw new ProjectError('forbidden', 'Project machine writes are not registered.')
    return this.repository.mutate(tenant, principal, projectMutationSchema.parse(raw))
  }
  commandStatus(tenant: string, principal: ProjectPrincipal, id: string) {
    projectTenantSchema.parse(tenant)
    projectPrincipalSchema.parse(principal)
    if (principal.kind !== 'human') throw new ProjectError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
