import { z } from 'zod'
import { followupQueueQuerySchema, followupQueueSchema } from './worklist-contracts.js'
import { crmOverviewQuerySchema, crmOverviewSchema } from './overview-contracts.js'
import {
  CrmError,
  tenantSchema,
  principalSchema,
  idSchema,
  pageSchema,
  mutationSchema,
  followupDraftSchema,
  commandStatusSchema,
  followupDetailSchema,
  type CrmRepository,
  type CrmPrincipal,
} from './contracts.js'

export class CrmService {
  constructor(
    private readonly repository: CrmRepository,
    private readonly clock: () => string = () => new Date().toISOString(),
  ) {}

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
  async overview(tenant: unknown, principal: unknown, raw: unknown = {}) {
    const identity = this.identity(tenant, principal)
    crmOverviewQuerySchema.parse(raw)
    const asOf = z.iso.datetime().parse(this.clock())
    if (!this.repository.overview)
      throw new CrmError('unavailable', 'CRM overview is unavailable on this adapter.')
    const result = crmOverviewSchema.parse(
      await this.repository.overview(identity.tenant, identity.principal, asOf),
    )
    if (result.tenantId !== identity.tenant || result.asOf !== asOf)
      throw new CrmError('unavailable', 'CRM overview could not be verified.')
    return result
  }
  async list(tenant: unknown, principal: unknown, page: unknown = {}) {
    const identity = this.identity(tenant, principal)
    return await this.repository.list(identity.tenant, identity.principal, pageSchema.parse(page))
  }
  async detail(tenant: unknown, principal: unknown, id: unknown) {
    const identity = this.identity(tenant, principal)
    return await this.repository.detail(identity.tenant, identity.principal, idSchema.parse(id))
  }
  async listFollowups(tenant: unknown, principal: unknown, input: unknown = {}) {
    const identity = this.identity(tenant, principal)
    const query = followupQueueQuerySchema.parse(input)
    if (!this.repository.listFollowups)
      throw new CrmError('unavailable', 'Follow-up worklist is unavailable on this adapter.')
    const result = followupQueueSchema.parse(
      await this.repository.listFollowups(identity.tenant, identity.principal, query),
    )
    if (result.asOf !== query.asOf || result.items.length > query.limit)
      throw new CrmError('unavailable', 'Follow-up worklist could not be verified.')
    return result
  }
  async followup(tenant: unknown, principal: unknown, rawId: unknown) {
    const identity = this.identity(tenant, principal),
      id = idSchema.parse(rawId)
    if (!this.repository.followupDetail)
      throw new CrmError('unavailable', 'Follow-up detail is unavailable on this adapter.')
    const result = followupDetailSchema.parse(
      await this.repository.followupDetail(identity.tenant, identity.principal, id),
    )
    if (
      result.followup.id !== id ||
      result.followup.tenantId !== identity.tenant ||
      result.contact.id !== result.followup.contactId
    )
      throw new CrmError('unavailable', 'Follow-up detail could not be verified.')
    return result
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
  async commandStatus(tenant: unknown, principal: unknown, requestId: unknown) {
    const identity = this.identity(tenant, principal)
    const id = idSchema.parse(requestId)
    if (identity.principal.kind !== 'human')
      throw new CrmError('forbidden', 'Command recovery requires the human initiator.')
    if (!this.repository.commandStatus)
      throw new CrmError('unavailable', 'Command recovery is unavailable on this adapter.')
    const result = commandStatusSchema.parse(
      await this.repository.commandStatus(identity.tenant, identity.principal, id),
    )
    if (result.requestId !== id)
      throw new CrmError('unavailable', 'Command recovery could not be verified.')
    return result
  }

  async mutateApproved(tenant: unknown, principal: unknown, input: unknown, approval: unknown) {
    const identity = this.identity(tenant, principal),
      mutation = mutationSchema.parse(input)
    if (
      identity.principal.kind !== 'service' ||
      !['followup.create', 'followup.complete'].includes(mutation.command.operation) ||
      !this.repository.mutateApproved
    )
      throw new CrmError('forbidden', 'A verified native follow-up approval is required.')
    return await this.repository.mutateApproved(
      identity.tenant,
      identity.principal,
      mutation,
      approval,
    )
  }
  async approvedCommandStatus(tenant: unknown, principal: unknown, requestId: unknown) {
    const identity = this.identity(tenant, principal),
      id = idSchema.parse(requestId)
    if (identity.principal.kind !== 'service' || !this.repository.approvedCommandStatus)
      throw new CrmError('forbidden', 'Native receipt recovery is unavailable.')
    const result = commandStatusSchema.parse(
      await this.repository.approvedCommandStatus(identity.tenant, identity.principal, id),
    )
    if (result.requestId !== id)
      throw new CrmError('unavailable', 'Native receipt could not be verified.')
    return result
  }
}

/** Explicit read/prepare allowlist. These tools cannot mutate or send messages. */
export function createCrmTools(service: CrmService, tenant: string, principal: CrmPrincipal) {
  return {
    async invoke(name: string, input: unknown) {
      if (name === 'crm.overview') return service.overview(tenant, principal, input)
      if (name === 'crm.contacts.list') {
        const page = await service.list(tenant, principal, input)
        return {
          items: page.items.map(({ id, fullName, revision }) => ({ id, fullName, revision })),
          next: page.next,
        }
      }
      if (name === 'crm.followups.list') return service.listFollowups(tenant, principal, input)
      if (name === 'crm.followup.complete.prepare') {
        const { id } = z.object({ id: idSchema }).strict().parse(input)
        const access = await service.access(tenant, principal)
        if (!access.permissions.includes('followups.write'))
          throw new CrmError('forbidden', 'Follow-up completion preparation is not permitted.')
        const detail = await service.followup(tenant, principal, id)
        if (detail.followup.status !== 'open')
          throw new CrmError('conflict', 'This follow-up is already completed.')
        return { task: detail, requiresApproval: true }
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
