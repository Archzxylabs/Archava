import {
  FieldServicesError,
  fieldTenantSchema,
  fieldPrincipalSchema,
  fieldCommandSchema,
  fieldJobPageQuerySchema,
  type FieldRepository,
  type FieldPrincipal,
  type FieldCommand,
  type FieldReceipt,
  type FieldCommandStatus,
  type FieldAccess,
  type FieldJob,
  type FieldJobDetail,
  type FieldJobPageQuery,
  type FieldAuditEntry,
} from './contracts.js'

/**
 * The service validates at the boundary and delegates every effect to the
 * repository. It holds no state of its own, so the same service class runs
 * against the PostgreSQL adapter and any in-memory test double.
 */
export class FieldServicesService {
  constructor(private readonly repository: FieldRepository) {}

  async mutate(
    tenant: string,
    principal: FieldPrincipal,
    mutation: FieldCommand,
  ): Promise<FieldReceipt> {
    const validTenant = fieldTenantSchema.parse(tenant)
    const validPrincipal = fieldPrincipalSchema.parse(principal)
    const validMutation = fieldCommandSchema.parse(mutation)
    return this.repository.execute(validTenant, validPrincipal, validMutation)
  }

  async execute(
    tenant: string,
    principal: FieldPrincipal,
    mutation: FieldCommand,
  ): Promise<FieldReceipt> {
    return this.mutate(tenant, principal, mutation)
  }

  async getCommandStatus(
    tenant: string,
    principal: FieldPrincipal,
    commandId: string,
  ): Promise<FieldCommandStatus> {
    const validTenant = fieldTenantSchema.parse(tenant)
    const validPrincipal = fieldPrincipalSchema.parse(principal)
    return this.repository.getCommandStatus(validTenant, validPrincipal, commandId)
  }

  async getAccess(tenant: string, principal: FieldPrincipal): Promise<FieldAccess> {
    const validTenant = fieldTenantSchema.parse(tenant)
    const validPrincipal = fieldPrincipalSchema.parse(principal)
    return this.repository.getAccess(validTenant, validPrincipal)
  }

  async listJobs(
    tenant: string,
    principal: FieldPrincipal,
    query: Partial<FieldJobPageQuery> = {},
  ): Promise<readonly FieldJob[]> {
    const validTenant = fieldTenantSchema.parse(tenant)
    const validPrincipal = fieldPrincipalSchema.parse(principal)
    const parsedQuery = fieldJobPageQuerySchema.parse(query)
    return this.repository.listJobs(validTenant, validPrincipal, parsedQuery)
  }

  async detail(tenant: string, principal: FieldPrincipal, jobId: string): Promise<FieldJobDetail> {
    const validTenant = fieldTenantSchema.parse(tenant)
    const validPrincipal = fieldPrincipalSchema.parse(principal)
    const detail = await this.repository.getJobDetail(validTenant, validPrincipal, jobId)
    if (!detail) throw new FieldServicesError('not_found', `Job ${jobId} not found`)
    return detail
  }

  async audit(
    tenant: string,
    principal: FieldPrincipal,
    jobId: string,
  ): Promise<readonly FieldAuditEntry[]> {
    const validTenant = fieldTenantSchema.parse(tenant)
    const validPrincipal = fieldPrincipalSchema.parse(principal)
    return this.repository.listAudit(validTenant, validPrincipal, jobId)
  }
}
