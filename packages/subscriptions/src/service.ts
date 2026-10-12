import {
  SubscriptionError,
  subscriptionTenantSchema,
  subscriptionPrincipalSchema,
  subscriptionMutationSchema,
  type SubscriptionRepository,
  type SubscriptionPrincipal,
  type SubscriptionMutation,
  type SubscriptionReceipt,
  type SubscriptionAccess,
  type SubscriptionCommandStatus,
  type SubscriptionPageQuery,
  type Subscription,
  type SubscriptionDetail,
  type Entitlement,
  type UsageEvent,
} from './contracts.js'

export class SubscriptionService {
  constructor(private readonly repository: SubscriptionRepository) {}

  private assertHumanOperator(principal: SubscriptionPrincipal) {
    if (principal.kind !== 'human') {
      throw new SubscriptionError(
        'forbidden',
        'Direct subscription mutation requires a verified human operator',
      )
    }
  }

  async execute(
    tenant: string,
    principal: SubscriptionPrincipal,
    mutation: SubscriptionMutation,
  ): Promise<SubscriptionReceipt> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    subscriptionMutationSchema.parse(mutation)

    this.assertHumanOperator(principal)

    return this.repository.execute(tenant, principal, mutation)
  }

  async getCommandStatus(
    tenant: string,
    principal: SubscriptionPrincipal,
    commandId: string,
  ): Promise<SubscriptionCommandStatus> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    return this.repository.getCommandStatus(tenant, principal, commandId)
  }

  async getAccess(tenant: string, principal: SubscriptionPrincipal): Promise<SubscriptionAccess> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    return this.repository.getAccess(tenant, principal)
  }

  async listSubscriptions(
    tenant: string,
    principal: SubscriptionPrincipal,
    query: SubscriptionPageQuery,
  ): Promise<readonly Subscription[]> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    return this.repository.listSubscriptions(tenant, principal, query)
  }

  async getSubscriptionDetail(
    tenant: string,
    principal: SubscriptionPrincipal,
    id: string,
  ): Promise<SubscriptionDetail | null> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    return this.repository.getSubscriptionDetail(tenant, principal, id)
  }

  async listEntitlements(
    tenant: string,
    principal: SubscriptionPrincipal,
    contactId: string,
  ): Promise<readonly Entitlement[]> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    return this.repository.listEntitlements(tenant, principal, contactId)
  }

  async listUsageEvents(
    tenant: string,
    principal: SubscriptionPrincipal,
    entitlementId: string,
  ): Promise<readonly UsageEvent[]> {
    subscriptionTenantSchema.parse(tenant)
    subscriptionPrincipalSchema.parse(principal)
    return this.repository.listUsageEvents(tenant, principal, entitlementId)
  }

  async mutate(
    tenant: string,
    principal: SubscriptionPrincipal,
    mutation: SubscriptionMutation,
  ): Promise<SubscriptionReceipt> {
    return this.execute(tenant, principal, mutation)
  }

  async detail(
    tenant: string,
    principal: SubscriptionPrincipal,
    id: string,
  ): Promise<SubscriptionDetail> {
    const res = await this.getSubscriptionDetail(tenant, principal, id)
    if (!res) throw new SubscriptionError('not_found')
    return res
  }

  async entitlements(
    tenant: string,
    principal: SubscriptionPrincipal,
    targetId: string,
  ): Promise<readonly Entitlement[]> {
    return this.listEntitlements(tenant, principal, targetId)
  }

  async usageEvents(
    tenant: string,
    principal: SubscriptionPrincipal,
    entitlementId: string,
  ): Promise<readonly UsageEvent[]> {
    return this.listUsageEvents(tenant, principal, entitlementId)
  }
}
