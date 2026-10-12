import { describe, it, expect } from 'vitest'
import {
  subscriptionSchema,
  entitlementSchema,
  usageEventSchema,
  subscriptionMutationSchema,
  subscriptionReceiptSchema,
  SubscriptionError,
  type SubscriptionRepository,
  type SubscriptionReceipt,
} from '../src/contracts.js'
import { SubscriptionService } from '../src/service.js'

describe('subscriptions domain contracts and boundaries', () => {
  it('validates subscription model and enforces UUID and currency constraints', () => {
    const valid = subscriptionSchema.parse({
      id: '550e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      contactId: '660e8400-e29b-41d4-a716-446655440000',
      planCode: 'pro-monthly',
      planName: 'Pro Tier Monthly',
      status: 'active',
      billingInterval: 'monthly',
      unitPrice: 150000,
      currency: 'IDR',
      quantity: 5,
      currentPeriodStart: '2026-10-01T00:00:00.000Z',
      currentPeriodEnd: '2026-11-01T00:00:00.000Z',
      cancelAtPeriodEnd: false,
      paymentReference: 'PAY-12345',
      ownerId: 'operator-1',
      revision: 1,
      createdAt: '2026-10-01T00:00:00.000Z',
      updatedAt: '2026-10-01T00:00:00.000Z',
    })
    expect(valid.status).toBe('active')
    expect(valid.quantity).toBe(5)

    expect(() =>
      subscriptionSchema.parse({
        ...valid,
        currency: 'invalid-currency',
      }),
    ).toThrow()
  })

  it('validates entitlement and usage event schemas', () => {
    const entitlement = entitlementSchema.parse({
      id: '770e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      subscriptionId: '550e8400-e29b-41d4-a716-446655440000',
      contactId: '660e8400-e29b-41d4-a716-446655440000',
      featureCode: 'gym_passes',
      featureName: 'Class Pass Pack',
      entitlementType: 'pass_bundle',
      allocatedQuantity: 10,
      consumedQuantity: 2,
      status: 'active',
      validFrom: '2026-10-01T00:00:00.000Z',
      validUntil: '2026-12-31T23:59:59.000Z',
      createdAt: '2026-10-01T00:00:00.000Z',
      updatedAt: '2026-10-01T00:00:00.000Z',
    })
    expect(entitlement.allocatedQuantity).toBe(10)
    expect(entitlement.consumedQuantity).toBe(2)

    const usage = usageEventSchema.parse({
      id: '880e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      entitlementId: entitlement.id,
      subscriptionId: entitlement.subscriptionId,
      contactId: entitlement.contactId,
      unitsConsumed: 1,
      idempotencyKey: 'checkin-20261012-01',
      evidenceRef: 'Front desk scanner',
      recordedBy: 'operator-1',
      recordedAt: '2026-10-12T10:00:00.000Z',
    })
    expect(usage.unitsConsumed).toBe(1)
  })

  it('rejects direct service agent mutations through subscription service', async () => {
    const fakeRepo: SubscriptionRepository = {
      execute: () => Promise.resolve({} as unknown as SubscriptionReceipt),
      getCommandStatus: () => Promise.resolve({ status: 'unknown' }),
      getAccess: () =>
        Promise.resolve({
          ownerId: 'u1',
          permissions: [],
          scope: 'all' as const,
        }),
      listSubscriptions: () => Promise.resolve([]),
      getSubscriptionDetail: () => Promise.resolve(null),
      listEntitlements: () => Promise.resolve([]),
      listUsageEvents: () => Promise.resolve([]),
    }
    const service = new SubscriptionService(fakeRepo)

    const mutation = subscriptionMutationSchema.parse({
      type: 'create_subscription',
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      contactId: '660e8400-e29b-41d4-a716-446655440000',
      planCode: 'saas-starter',
      planName: 'SaaS Starter',
      billingInterval: 'monthly',
      unitPrice: 50000,
      currency: 'IDR',
      quantity: 1,
      initialEntitlements: [],
    })

    await expect(
      service.execute(
        'tenant-test',
        { kind: 'service', id: 'service-bot', onBehalfOf: 'user-1' },
        mutation,
      ),
    ).rejects.toThrowError(SubscriptionError)
  })

  it('validates subscription receipt schema', () => {
    const receipt = subscriptionReceiptSchema.parse({
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      tenantId: 'tenant-test',
      action: 'create_subscription',
      subscriptionId: '550e8400-e29b-41d4-a716-446655440000',
      revision: 1,
      status: 'draft',
      idempotencyReplay: false,
      executedAt: '2026-10-01T00:00:00.000Z',
    })
    expect(receipt.action).toBe('create_subscription')
  })
})
