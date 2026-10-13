import {
  LaundryError,
  laundryTenantSchema,
  laundryPrincipalSchema,
  laundryMutationSchema,
  orderPageQuerySchema,
  type LaundryRepository,
  type LaundryPrincipal,
  type LaundryMutation,
  type LaundryReceipt,
  type LaundryAccess,
  type LaundryCommandStatus,
  type OrderPageQuery,
  type LaundryOrder,
  type OrderDetail,
  type OrderLine,
  type ProcessRecord,
  type Finding,
} from './contracts.js'

export class LaundryService {
  constructor(private readonly repository: LaundryRepository) {}

  private assertHumanOperator(principal: LaundryPrincipal) {
    if (principal.kind !== 'human') {
      throw new LaundryError(
        'forbidden',
        'Automated service agents cannot record laundry mutations directly',
      )
    }
  }

  async mutate(
    tenant: string,
    principal: LaundryPrincipal,
    mutation: LaundryMutation,
  ): Promise<LaundryReceipt> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    const validMutation = laundryMutationSchema.parse(mutation)
    this.assertHumanOperator(validPrincipal)
    return this.repository.execute(validTenant, validPrincipal, validMutation)
  }

  async execute(
    tenant: string,
    principal: LaundryPrincipal,
    mutation: LaundryMutation,
  ): Promise<LaundryReceipt> {
    return this.mutate(tenant, principal, mutation)
  }

  async getCommandStatus(
    tenant: string,
    principal: LaundryPrincipal,
    commandId: string,
  ): Promise<LaundryCommandStatus> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    return this.repository.getCommandStatus(validTenant, validPrincipal, commandId)
  }

  async getAccess(tenant: string, principal: LaundryPrincipal): Promise<LaundryAccess> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    return this.repository.getAccess(validTenant, validPrincipal)
  }

  async listOrders(
    tenant: string,
    principal: LaundryPrincipal,
    query: Partial<OrderPageQuery> = {},
  ): Promise<readonly LaundryOrder[]> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    const parsedQuery = orderPageQuerySchema.parse(query)
    return this.repository.listOrders(validTenant, validPrincipal, parsedQuery)
  }

  async detail(tenant: string, principal: LaundryPrincipal, orderId: string): Promise<OrderDetail> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    const detail = await this.repository.getOrderDetail(validTenant, validPrincipal, orderId)
    if (!detail) {
      throw new LaundryError('not_found', `Laundry order ${orderId} not found`)
    }
    return detail
  }

  async lines(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly OrderLine[]> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    return this.repository.listLines(validTenant, validPrincipal, orderId)
  }

  async process(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly ProcessRecord[]> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    return this.repository.listProcess(validTenant, validPrincipal, orderId)
  }

  async findings(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly Finding[]> {
    const validTenant = laundryTenantSchema.parse(tenant)
    const validPrincipal = laundryPrincipalSchema.parse(principal)
    return this.repository.listFindings(validTenant, validPrincipal, orderId)
  }
}
