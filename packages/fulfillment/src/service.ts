import {
  FulfillmentError,
  fulfillmentTenantSchema,
  fulfillmentPrincipalSchema,
  fulfillmentMutationSchema,
  jobPageQuerySchema,
  type FulfillmentRepository,
  type FulfillmentPrincipal,
  type FulfillmentMutation,
  type FulfillmentReceipt,
  type FulfillmentAccess,
  type FulfillmentCommandStatus,
  type JobPageQuery,
  type Job,
  type JobDetail,
  type JobLine,
  type Finding,
  type CustodyRecord,
} from './contracts.js'

export class FulfillmentService {
  constructor(private readonly repository: FulfillmentRepository) {}

  private assertHumanOperator(principal: FulfillmentPrincipal) {
    if (principal.kind !== 'human') {
      throw new FulfillmentError(
        'forbidden',
        'Automated service agents cannot record fulfillment mutations directly',
      )
    }
  }

  async mutate(
    tenant: string,
    principal: FulfillmentPrincipal,
    mutation: FulfillmentMutation,
  ): Promise<FulfillmentReceipt> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    const validMutation = fulfillmentMutationSchema.parse(mutation)
    this.assertHumanOperator(validPrincipal)
    return this.repository.execute(validTenant, validPrincipal, validMutation)
  }

  async execute(
    tenant: string,
    principal: FulfillmentPrincipal,
    mutation: FulfillmentMutation,
  ): Promise<FulfillmentReceipt> {
    return this.mutate(tenant, principal, mutation)
  }

  async getCommandStatus(
    tenant: string,
    principal: FulfillmentPrincipal,
    commandId: string,
  ): Promise<FulfillmentCommandStatus> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    return this.repository.getCommandStatus(validTenant, validPrincipal, commandId)
  }

  async getAccess(tenant: string, principal: FulfillmentPrincipal): Promise<FulfillmentAccess> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    return this.repository.getAccess(validTenant, validPrincipal)
  }

  async listJobs(
    tenant: string,
    principal: FulfillmentPrincipal,
    query: Partial<JobPageQuery> = {},
  ): Promise<readonly Job[]> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    const parsedQuery = jobPageQuerySchema.parse(query)
    return this.repository.listJobs(validTenant, validPrincipal, parsedQuery)
  }

  async detail(tenant: string, principal: FulfillmentPrincipal, jobId: string): Promise<JobDetail> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    const detail = await this.repository.getJobDetail(validTenant, validPrincipal, jobId)
    if (!detail) {
      throw new FulfillmentError('not_found', `Job ${jobId} not found`)
    }
    return detail
  }

  async lines(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly JobLine[]> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    return this.repository.listLines(validTenant, validPrincipal, jobId)
  }

  async findings(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly Finding[]> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    return this.repository.listFindings(validTenant, validPrincipal, jobId)
  }

  async custody(
    tenant: string,
    principal: FulfillmentPrincipal,
    jobId: string,
  ): Promise<readonly CustodyRecord[]> {
    const validTenant = fulfillmentTenantSchema.parse(tenant)
    const validPrincipal = fulfillmentPrincipalSchema.parse(principal)
    return this.repository.listCustody(validTenant, validPrincipal, jobId)
  }
}
