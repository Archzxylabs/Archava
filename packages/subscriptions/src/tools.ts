import { z } from 'zod'
import {
  SubscriptionError,
  subscriptionPageQuerySchema,
  type SubscriptionPrincipal,
} from './contracts.js'
import type { SubscriptionService } from './service.js'

export const subscriptionTools = [
  {
    name: 'subscriptions.list',
    description: 'List tenant subscriptions with optional status filter',
    parameters: subscriptionPageQuerySchema,
  },
  {
    name: 'subscriptions.detail',
    description: 'Get subscription details including active entitlements',
    parameters: z.object({ id: z.string().uuid() }).strict(),
  },
  {
    name: 'subscriptions.entitlements',
    description: 'List all entitlements granted to a specific contact',
    parameters: z.object({ contactId: z.string().uuid() }).strict(),
  },
  {
    name: 'subscriptions.usageHistory',
    description: 'View usage audit log for a specific entitlement',
    parameters: z.object({ entitlementId: z.string().uuid() }).strict(),
  },
] as const

export async function executeSubscriptionTool(
  service: SubscriptionService,
  tenant: string,
  principal: SubscriptionPrincipal,
  toolName: string,
  args: unknown,
): Promise<unknown> {
  switch (toolName) {
    case 'subscriptions.list': {
      const query = subscriptionPageQuerySchema.parse(args ?? {})
      return service.listSubscriptions(tenant, principal, query)
    }
    case 'subscriptions.detail': {
      const parsed = z.object({ id: z.string().uuid() }).parse(args)
      return service.getSubscriptionDetail(tenant, principal, parsed.id)
    }
    case 'subscriptions.entitlements': {
      const parsed = z.object({ contactId: z.string().uuid() }).parse(args)
      return service.listEntitlements(tenant, principal, parsed.contactId)
    }
    case 'subscriptions.usageHistory': {
      const parsed = z.object({ entitlementId: z.string().uuid() }).parse(args)
      return service.listUsageEvents(tenant, principal, parsed.entitlementId)
    }
    default:
      throw new SubscriptionError('not_found')
  }
}
