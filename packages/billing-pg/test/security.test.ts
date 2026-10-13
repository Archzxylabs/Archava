import { describe, expect, it } from 'vitest'
import {
  BILLING_PERMISSIONS,
  billingMutationSchema,
  type BillingMutation,
  type BillingPrincipal,
} from '@archava/billing'
import { PostgresBillingRepository, billingMigrationLedger } from '../src/index.js'
import { createStubPool } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): BillingPrincipal => ({ kind: 'human', id })
const automated = (id: string, onBehalfOf: string): BillingPrincipal => ({
  kind: 'service',
  id,
  onBehalfOf,
})

const INVOICE_ID = '770e8400-e29b-41d4-a716-446655440000'

const createInvoice: BillingMutation = billingMutationSchema.parse({
  type: 'create_invoice',
  commandId: '550e8400-e29b-41d4-a716-446655440000',
  invoiceCode: 'INV-2026-0001',
  customerRef: 'cust-alpha',
  currency: 'IDR',
  consumerRef: 'consumer-1',
  lines: [{ description: 'Consultation', quantity: 1, unitMinor: 500000 }],
})

type Options = {
  unsafe?: boolean
  usesuper?: boolean
  permissions?: string[]
  scope?: string
  active?: boolean
  migrations?: Array<{ id: string; checksum: string }>
}

/** An issued invoice row with a 500000 gross, nothing settled. */
function issuedInvoiceRow(): Record<string, unknown> {
  return {
    id: INVOICE_ID,
    tenant_id: tenant,
    invoice_code: 'INV-2026-0001',
    customer_ref: 'cust-alpha',
    currency: 'IDR',
    status: 'issued',
    gross_minor: '500000',
    correction_minor: '0',
    settled_minor: '0',
    outstanding_minor: '500000',
    consumer_ref: 'consumer-1',
    owner_id: 'owner',
    revision: 2,
    created_at: new Date('2026-10-12T10:00:00.000Z'),
    issued_at: new Date('2026-10-12T10:05:00.000Z'),
    settled_at: null,
    voided_at: null,
  }
}

/** A pool whose replies are keyed on the statement the repository actually issues. */
async function repositoryWith(options: Options = {}) {
  const migrations =
    options.migrations === undefined ? await billingMigrationLedger() : options.migrations
  const pool = createStubPool((query) => {
    if (query.text.includes('pg_roles')) return { rows: [{ unsafe: options.unsafe ?? false }] }
    if (query.text.includes('schema_migrations')) return { rows: migrations }
    if (query.text.includes('pg_user')) {
      return { rows: [{ usename: 'archava_billing_app', usesuper: options.usesuper ?? false }] }
    }
    if (query.text.includes('lock_membership')) {
      return {
        rows: [
          {
            principal_id: 'owner',
            kind: 'human',
            active: options.active ?? true,
            permissions: options.permissions ?? [...BILLING_PERMISSIONS],
            scope: options.scope ?? 'all',
          },
        ],
      }
    }
    if (
      query.text.includes('archava_billing.invoices') &&
      (query.text.includes('SELECT * FROM') || query.text.includes('RETURNING *'))
    ) {
      return { rows: [issuedInvoiceRow()] }
    }
    if (query.text.includes('INSERT INTO archava_billing.audit')) return { rows: [] }
    if (
      query.text.includes('INSERT INTO archava_billing.payment_attempts') ||
      query.text.includes('INSERT INTO archava_billing.payment_events') ||
      query.text.includes('INSERT INTO archava_billing.invoice_lines') ||
      query.text.includes('INSERT INTO archava_billing.invoice_corrections')
    ) {
      return {
        rows: [
          {
            id: 'e5b2c3d4-e29b-41d4-a716-446655440000',
            invoice_id: INVOICE_ID,
            provider_id: 'acquirer-demo',
            provider_event_id: 'evt_1',
            outcome: 'succeeded',
            amount_minor: '500000',
            currency: 'IDR',
            provider_ref: 'pi_123',
            description: 'Consultation',
            quantity: 1,
            unit_minor: '500000',
            line_total_minor: '500000',
            reason: 'Adjustment',
            reverses_id: null,
            recorded_by: 'owner',
            created_at: new Date('2026-10-12T10:30:00.000Z'),
            recorded_at: new Date('2026-10-12T10:30:00.000Z'),
            verified_at: new Date('2026-10-12T10:30:00.000Z'),
          },
        ],
      }
    }
    return undefined
  })
  const repository = await PostgresBillingRepository.create(pool, key)
  return { repository, pool }
}

describe('billing security boundaries', () => {
  it('rejects permissions outside the billing permission vocabulary', async () => {
    const { repository } = await repositoryWith({ permissions: ['bogus.permission'] })
    await expect(repository.getAccess(tenant, human('owner'))).rejects.toThrow()
  })

  it('refuses superuser connections before any command is executed', async () => {
    const pool = createStubPool((query) =>
      query.text.includes('pg_roles') ? { rows: [{ unsafe: true }] } : undefined,
    )
    await expect(PostgresBillingRepository.create(pool, key)).rejects.toThrow(/non-owner runtime/)
  })

  it('refuses a runtime whose migration history does not match the packaged ledger', async () => {
    const pool = createStubPool((query) => {
      if (query.text.includes('pg_roles')) return { rows: [{ unsafe: false }] }
      if (query.text.includes('schema_migrations')) return { rows: [] }
      return undefined
    })
    await expect(PostgresBillingRepository.create(pool, key)).rejects.toThrow(/migrations mismatch/)
  })

  it('refuses short fingerprint keys so command fingerprints stay unlinkable', async () => {
    const pool = createStubPool(() => undefined)
    await expect(PostgresBillingRepository.create(pool, 'short')).rejects.toThrow(
      /at least 32 characters/,
    )
  })

  it('reports a missing write grant as forbidden instead of recording the invoice', async () => {
    const { repository } = await repositoryWith({ permissions: ['billing.read', 'audit.read'] })
    await expect(repository.execute(tenant, human('owner'), createInvoice)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('requires the dedicated settle grant to issue, correct or void an invoice', async () => {
    const { repository } = await repositoryWith({
      permissions: ['billing.read', 'billing.write'],
    })
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'issue_invoice',
        commandId: '880e8400-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        expectedRevision: 2,
        reason: 'Approved',
      }),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('refuses inactive principals without disclosing whether they exist', async () => {
    const { repository } = await repositoryWith({ active: false })
    await expect(repository.execute(tenant, human('owner'), createInvoice)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('permits a service principal to carry a provider event, since the signature is the authority', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, automated('provider-hook', 'user-1'), {
        type: 'record_provider_event',
        commandId: 'e1b2c3d4-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        providerId: 'acquirer-demo',
        providerEventId: 'evt_service',
        outcome: 'succeeded',
        amountMinor: 500000,
        currency: 'IDR',
        signature: 'e'.repeat(64),
      }),
    ).resolves.toMatchObject({ action: 'record_provider_event' })
  })

  it('records a payment attempt without ever touching settled or outstanding', async () => {
    const { repository, pool } = await repositoryWith({})
    await repository.execute(tenant, human('owner'), {
      type: 'record_payment_attempt',
      commandId: '990e8400-e29b-41d4-a716-446655440000',
      invoiceId: INVOICE_ID,
      amountMinor: 500000,
      providerRef: 'pi_123',
    })

    const updatesInvoices = pool.clientQueries.filter(
      (q) =>
        q.text.includes('UPDATE archava_billing.invoices') &&
        (q.text.includes('settled_minor') || q.text.includes('outstanding_minor')),
    )
    expect(updatesInvoices).toHaveLength(0)

    const inserted = pool.clientQueries.find((q) =>
      q.text.includes('INSERT INTO archava_billing.payment_attempts'),
    )
    expect(inserted).toBeDefined()
    expect(inserted?.text).not.toContain('settled_minor')
  })

  it('rejects a provider event whose currency does not match the invoice', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_provider_event',
        commandId: 'a1b2c3d4-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        providerId: 'acquirer-demo',
        providerEventId: 'evt_currency',
        outcome: 'succeeded',
        amountMinor: 500000,
        currency: 'USD',
        signature: 'd'.repeat(64),
      }),
    ).rejects.toMatchObject({ code: 'unbalanced_settlement' })
  })

  it('settles only through a provider event, updating the balance on the invoice', async () => {
    const { repository, pool } = await repositoryWith({})
    await repository.execute(tenant, human('owner'), {
      type: 'record_provider_event',
      commandId: 'b1b2c3d4-e29b-41d4-a716-446655440000',
      invoiceId: INVOICE_ID,
      providerId: 'acquirer-demo',
      providerEventId: 'evt_1',
      outcome: 'succeeded',
      amountMinor: 500000,
      currency: 'IDR',
      signature: 'a'.repeat(64),
    })

    const settle = pool.clientQueries.find(
      (q) => q.text.includes('UPDATE archava_billing.invoices') && q.text.includes('settled_minor'),
    )
    expect(settle).toBeDefined()
    expect(settle?.params).toContain(500000)

    const inserted = pool.clientQueries.find((q) =>
      q.text.includes('INSERT INTO archava_billing.payment_events'),
    )
    expect(inserted).toBeDefined()
  })

  it('rejects a provider event that would settle more than the outstanding balance', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_provider_event',
        commandId: 'c1b2c3d4-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        providerId: 'acquirer-demo',
        providerEventId: 'evt_over',
        outcome: 'succeeded',
        amountMinor: 999999,
        currency: 'IDR',
        signature: 'b'.repeat(64),
      }),
    ).rejects.toMatchObject({ code: 'unbalanced_settlement' })
  })

  it('rejects a refund that exceeds what was already settled', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_provider_event',
        commandId: 'd1b2c3d4-e29b-41d4-a716-446655440000',
        invoiceId: INVOICE_ID,
        providerId: 'acquirer-demo',
        providerEventId: 'evt_refund_over',
        outcome: 'refunded',
        amountMinor: 500000,
        currency: 'IDR',
        signature: 'c'.repeat(64),
      }),
    ).rejects.toMatchObject({ code: 'unbalanced_settlement' })
  })
})
