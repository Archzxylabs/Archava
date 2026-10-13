import { describe, it, expect } from 'vitest'
import {
  invoiceSchema,
  invoiceLineSchema,
  invoiceCorrectionSchema,
  paymentAttemptSchema,
  paymentEventSchema,
  billingMutationSchema,
  moneyMinorSchema,
  currencySchema,
  type BillingRepository,
  type BillingReceipt,
  type BillingPrincipal,
  type BillingMutation,
} from '../src/contracts.js'
import { BillingService, settledFromEvents, outstandingFrom } from '../src/service.js'
import {
  providerEventDigest,
  signaturesEqual,
  isSignatureKey,
  SIGNATURE_KEY_MIN_BYTES,
} from '../src/signature.js'

const INVOICE_ID = '550e8400-e29b-41d4-a716-446655440000'
const EVENT_ID = '660e8400-e29b-41d4-a716-446655440000'
const CORRECTION_ID = '770e8400-e29b-41d4-a716-446655440000'
const KEY = new Uint8Array(SIGNATURE_KEY_MIN_BYTES).fill(0xab)

const humanPrincipal: BillingPrincipal = { kind: 'human', id: 'user-1' }
const servicePrincipal: BillingPrincipal = {
  kind: 'service',
  id: 'agent-auto',
  onBehalfOf: 'user-1',
}

const fakeRepo: BillingRepository = {
  execute: () => Promise.resolve({} as unknown as BillingReceipt),
  getCommandStatus: () => Promise.resolve({ status: 'unknown' }),
  getAccess: () => Promise.resolve({ ownerId: 'u1', permissions: [], scope: 'all' as const }),
  listInvoices: () => Promise.resolve([]),
  getInvoiceDetail: () => Promise.resolve(null),
  listLines: () => Promise.resolve([]),
  listCorrections: () => Promise.resolve([]),
  listAttempts: () => Promise.resolve([]),
  listEvents: () => Promise.resolve([]),
}

const signedEvent = (
  outcome: 'succeeded' | 'failed' | 'refunded',
  amountMinor: number,
  invoiceId = INVOICE_ID,
) => {
  const claim = {
    tenantId: 'tenant-test',
    invoiceId,
    providerId: 'acquirer-demo',
    providerEventId: 'evt_9f3a',
    outcome,
    amountMinor,
    currency: 'IDR',
  }
  return {
    type: 'record_provider_event' as const,
    commandId: '990e8400-e29b-41d4-a716-446655440000',
    invoiceId,
    providerId: 'acquirer-demo',
    providerEventId: 'evt_9f3a',
    outcome,
    amountMinor,
    currency: 'IDR' as const,
    signature: providerEventDigest(KEY, claim),
  }
}

describe('billing domain contracts and settlement authority', () => {
  it('rejects direct service agent mutations through billing service', async () => {
    const service = new BillingService(fakeRepo, () => KEY)
    const mutation = billingMutationSchema.parse({
      type: 'create_invoice',
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      invoiceCode: 'INV-2026-001',
      customerRef: 'cust-alpha',
      currency: 'IDR',
      consumerRef: 'consumer-1',
      lines: [{ description: 'Consult', quantity: 1, unitMinor: 500000 }],
    })

    await expect(service.mutate('tenant-test', servicePrincipal, mutation)).rejects.toThrow(
      /Automated service agents/,
    )
  })

  it('refuses a provider event without a valid signature', async () => {
    const service = new BillingService(fakeRepo, () => KEY)
    const forged = signedEvent('succeeded', 500000)
    const tampered = { ...forged, amountMinor: 999999 }

    await expect(
      service.mutate('tenant-test', humanPrincipal, tampered as BillingMutation),
    ).rejects.toThrow(/signature/i)
  })

  it('refuses a provider event when no signing secret is configured', async () => {
    const service = new BillingService(fakeRepo, () => null)
    const event = signedEvent('succeeded', 500000)

    await expect(service.mutate('tenant-test', humanPrincipal, event)).rejects.toThrow(
      /No provider signing secret/,
    )
  })

  it('verifies a correctly signed provider event through to the repository', async () => {
    let received: BillingMutation | null = null
    const repo: BillingRepository = {
      ...fakeRepo,
      execute: (_t, _p, mutation) => {
        received = mutation
        return Promise.resolve({} as BillingReceipt)
      },
    }
    const service = new BillingService(repo, () => KEY)
    const event = signedEvent('succeeded', 500000)

    await service.mutate('tenant-test', humanPrincipal, event)
    expect(received).not.toBeNull()
    expect((received as unknown as { amountMinor: number }).amountMinor).toBe(500000)
  })

  it('rejects a payment attempt mutation that tries to assert an outcome', () => {
    expect(() =>
      billingMutationSchema.parse({
        type: 'record_payment_attempt',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        amountMinor: 500000,
        outcome: 'succeeded',
      }),
    ).toThrow()
  })

  it('rejects malformed money, currency and zero corrections', () => {
    expect(() => moneyMinorSchema.parse(500.5)).toThrow()
    expect(() => currencySchema.parse('idr')).toThrow()
    expect(() => currencySchema.parse('rupiah')).toThrow()
    expect(() =>
      billingMutationSchema.parse({
        type: 'append_correction',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        amountMinor: 0,
        reason: 'No-op',
      }),
    ).toThrow()
  })

  it('validates immutable invoice snapshot, corrections and events', () => {
    const invoice = invoiceSchema.parse({
      id: INVOICE_ID,
      tenantId: 'tenant-test',
      invoiceCode: 'INV-2026-001',
      customerRef: 'cust-alpha',
      currency: 'IDR',
      status: 'issued',
      grossMinor: 1000000,
      correctionMinor: -100000,
      settledMinor: 900000,
      outstandingMinor: 0,
      consumerRef: 'consumer-1',
      revision: 3,
      createdAt: '2026-10-12T10:00:00.000Z',
      issuedAt: '2026-10-12T10:05:00.000Z',
      settledAt: null,
      voidedAt: null,
    })
    expect(invoice.grossMinor).toBe(1000000)
    expect(outstandingFrom(invoice)).toBe(0)

    expect(() => invoiceSchema.parse({ ...invoice, status: 'unpaid' })).toThrow()
    expect(() => invoiceSchema.parse({ ...invoice, grossMinor: 1000000.5 })).toThrow()

    const correction = invoiceCorrectionSchema.parse({
      id: CORRECTION_ID,
      invoiceId: INVOICE_ID,
      amountMinor: -100000,
      reason: 'Goodwill credit',
      reversesId: null,
      createdAt: '2026-10-12T10:30:00.000Z',
      recordedBy: 'user-1',
    })
    expect(correction.amountMinor).toBe(-100000)

    const line = invoiceLineSchema.parse({
      id: '880e8400-e29b-41d4-a716-446655440000',
      invoiceId: INVOICE_ID,
      description: 'Specialist consultation',
      quantity: 2,
      unitMinor: 500000,
      lineTotalMinor: 1000000,
      recordedBy: 'user-1',
      recordedAt: '2026-10-12T10:00:00.000Z',
    })
    expect(line.lineTotalMinor).toBe(1000000)
    expect(() => invoiceLineSchema.parse({ ...line, quantity: 0 })).toThrow()
    expect(() => invoiceLineSchema.parse({ ...line, unitMinor: 500.75 })).toThrow()

    const attempt = paymentAttemptSchema.parse({
      id: EVENT_ID,
      invoiceId: INVOICE_ID,
      amountMinor: 900000,
      currency: 'IDR',
      providerRef: 'pi_123',
      recordedAt: '2026-10-12T10:31:00.000Z',
      recordedBy: 'user-1',
    })
    expect(attempt.amountMinor).toBe(900000)

    const event = paymentEventSchema.parse({
      id: '990e8400-e29b-41d4-a716-446655440000',
      invoiceId: INVOICE_ID,
      providerId: 'acquirer-demo',
      providerEventId: 'evt_9f3a',
      outcome: 'succeeded',
      amountMinor: 900000,
      currency: 'IDR',
      verifiedAt: '2026-10-12T10:32:00.000Z',
      recordedBy: 'system',
    })
    expect(event.outcome).toBe('succeeded')
    expect(() => paymentEventSchema.parse({ ...event, outcome: 'maybe' })).toThrow()
    expect(() => paymentEventSchema.parse({ ...event, signature: 'short' })).toThrow()
  })

  it('derives settlement only from provider events, never from attempts', () => {
    expect(settledFromEvents([])).toBe(0)
    expect(
      settledFromEvents([
        { outcome: 'failed', amountMinor: 100000 },
        { outcome: 'succeeded', amountMinor: 500000 },
        { outcome: 'refunded', amountMinor: 100000 },
      ]),
    ).toBe(400000)
    expect(outstandingFrom({ grossMinor: 1000000, correctionMinor: 0, settledMinor: 400000 })).toBe(
      600000,
    )
  })

  it('signs and compares provider digests without leaking a mismatch', () => {
    const claim = {
      tenantId: 'tenant-test',
      invoiceId: INVOICE_ID,
      providerId: 'acquirer-demo',
      providerEventId: 'evt_9f3a',
      outcome: 'succeeded',
      amountMinor: 500000,
      currency: 'IDR',
    }
    const digest = providerEventDigest(KEY, claim)
    expect(signaturesEqual(digest, digest)).toBe(true)
    expect(
      signaturesEqual(digest, providerEventDigest(KEY, { ...claim, amountMinor: 500001 })),
    ).toBe(false)
    expect(isSignatureKey(KEY)).toBe(true)
    expect(isSignatureKey(new Uint8Array(8))).toBe(false)
    expect(isSignatureKey('short-secret')).toBe(false)
  })
})
