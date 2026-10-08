import { z } from 'zod'
import {
  CrmError,
  tenantSchema,
  principalSchema,
  idSchema,
  pageSchema,
  mutationSchema,
  followupDraftSchema,
  type CrmRepository,
  type CrmPrincipal,
} from './contracts.js'

export class CrmService {
  constructor(private readonly repository: CrmRepository) {}

  private identity(
    tenant: unknown,
    principal: unknown,
  ): { tenant: string; principal: CrmPrincipal } {
    if (principal === null || principal === undefined)
      throw new CrmError('unauthenticated', 'Sign in to access CRM.')
    return { tenant: tenantSchema.parse(tenant), principal: principalSchema.parse(principal) }
  }
  async access(tenant: unknown, principal: unknown) {
    const identity = this.identity(tenant, principal)
    return await this.repository.access(identity.tenant, identity.principal)
  }
  async list(tenant: unknown, principal: unknown, page: unknown = {}) {
    const identity = this.identity(tenant, principal)
    return await this.repository.list(identity.tenant, identity.principal, pageSchema.parse(page))
  }
  async detail(tenant: unknown, principal: unknown, id: unknown) {
    const identity = this.identity(tenant, principal)
    return await this.repository.detail(identity.tenant, identity.principal, idSchema.parse(id))
  }
  async mutate(tenant: unknown, principal: unknown, input: unknown) {
    const identity = this.identity(tenant, principal)
    // No machine write path until the corresponding gated Act boundary ships.
    if (identity.principal.kind !== 'human')
      throw new CrmError('forbidden', 'Machine CRM writes require a verified Act integration.')
    return await this.repository.mutate(
      identity.tenant,
      identity.principal,
      mutationSchema.parse(input),
    )
  }
}

/** Explicit read/prepare allowlist. These tools cannot mutate or send messages. */
export function createCrmTools(service: CrmService, tenant: string, principal: CrmPrincipal) {
  return {
    async invoke(name: string, input: unknown) {
      if (name === 'crm.contacts.list') {
        const page = await service.list(tenant, principal, input)
        return {
          items: page.items.map(({ id, fullName, revision }) => ({ id, fullName, revision })),
          next: page.next,
        }
      }
      if (name === 'crm.contact.summary') {
        const { id } = z.object({ id: idSchema }).strict().parse(input)
        const detail = await service.detail(tenant, principal, id)
        return {
          id,
          fullName: detail.contact.fullName,
          revision: detail.contact.revision,
          activities: detail.activities.map(({ kind, summary, createdAt }) => ({
            kind,
            summary,
            createdAt,
          })),
          followups: detail.followups.map(({ id, summary, dueAt, status }) => ({
            id,
            summary,
            dueAt,
            status,
          })),
        }
      }
      if (name === 'crm.followup.prepare') {
        const draft = followupDraftSchema.parse(input)
        const access = await service.access(tenant, principal)
        if (!access.permissions.includes('followups.write'))
          throw new CrmError('forbidden', 'Follow-up preparation is not permitted.')
        const { contact } = await service.detail(tenant, principal, draft.contactId)
        return {
          ...draft,
          tenantId: tenant,
          contactRevision: contact.revision,
          status: 'draft' as const,
          requiresApproval: true,
        }
      }
      throw new CrmError('forbidden', 'Unsupported CRM tool.')
    },
  }
}
