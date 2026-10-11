import { z } from 'zod'
import {
  SupportError,
  supportIdSchema,
  supportPrincipalSchema,
  casePageSchema,
  assigneePageSchema,
  supportMutationSchema,
  type SupportPrincipal,
  type SupportRepository,
} from './contracts.js'
export class SupportService {
  constructor(private readonly repository: SupportRepository) {}
  private validate(tenant: string, principal: SupportPrincipal) {
    supportIdSchema.parse(tenant)
    supportPrincipalSchema.parse(principal)
  }
  access(tenant: string, principal: SupportPrincipal) {
    this.validate(tenant, principal)
    return this.repository.access(tenant, principal)
  }
  assignees(tenant: string, principal: SupportPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new SupportError('forbidden')
    return this.repository.assignees(tenant, principal, assigneePageSchema.parse(query))
  }
  cases(tenant: string, principal: SupportPrincipal, query: unknown = {}) {
    this.validate(tenant, principal)
    return this.repository.cases(tenant, principal, casePageSchema.parse(query))
  }
  detail(tenant: string, principal: SupportPrincipal, id: string) {
    this.validate(tenant, principal)
    return this.repository.detail(tenant, principal, z.uuid().parse(id))
  }
  mutate(tenant: string, principal: SupportPrincipal, input: unknown) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new SupportError('forbidden')
    return this.repository.mutate(tenant, principal, supportMutationSchema.parse(input))
  }
  commandStatus(tenant: string, principal: SupportPrincipal, id: string) {
    this.validate(tenant, principal)
    if (principal.kind !== 'human') throw new SupportError('forbidden')
    return this.repository.commandStatus(tenant, principal, z.uuid().parse(id))
  }
}
