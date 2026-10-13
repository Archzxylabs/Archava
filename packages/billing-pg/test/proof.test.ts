import { describe, expect, it } from 'vitest'
import {
  billingReceiptSchema,
  billingMutationSchema,
  type BillingMutation,
  type BillingPrincipal,
} from '@archava/billing'
import { PostgresBillingRepository, billingMigrationLedger } from '../src/index.js'
import { createStubPool, makeRow, type Row, type Query } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): BillingPrincipal => ({ kind: 'human', id })

const OWNER = ['billing.read', 'billing.write', 'billing.settle', 'audit.read']
const CLERK = ['billing.read', 'billing.write']

const INVOICE_ID = '550e8400-e29b-41d4-a716-446655440000'
let commandSequence = 0
const commandId = () =>
  `550e8400-e29b-41d4-a716-44665544${String(commandSequence++).padStart(4, '0')}`

const now = () => new Date('2026-10-12T10:30:00.000Z')

function invoiceRow(overrides: Row = {}): Row {
  return makeRow({
    id: INVOICE_ID,
    tenant_id: tenant,
    invoice_code: 'INV-2026-0001',
    customer_ref: 'cust-alpha',
    currency: 'IDR',
    status: 'draft',
    gross_minor: '0',
    correction_minor: '0',
    settled_minor: '0',
    outstanding_minor: '0',
    consumer_ref: 'consumer-1',
    owner_id: 'owner',
    revision: 1,
    created_at: new Date('2026-10-12T10:00:00.000Z'),
    issued_at: null,
    settled_at: null,
    voided_at: null,
    ...overrides,
  })
}

type State = {
  invoice: Row
  lines: Row[]
  corrections: Row[]
  attempts: Row[]
  events: Row[]
  commands: Map<string, { fingerprint: string; receipt: unknown }>
  audit: Row[]
  permissions: string[]
  active: boolean
}

function createState(overrides: Partial<State> = {}): State {
  return {
    invoice: invoiceRow(),
    lines: [],
    corrections: [],
    attempts: [],
    events: [],
    commands: new Map(),
    audit: [],
    permissions: OWNER,
    active: true,
    ...overrides,
  }
}

/**
 * A state machine that stands in for the tables, so a whole settlement
 * lifecycle can be exercised without a database. It mirrors the arithmetic the
 * real schema enforces with CHECK constraints: `outstanding` is always
 * `gross + correction - settled`, and a settled invoice must have nothing owed.
 */
function poolFor(state: State, migrations: Array<{ id: string; checksum: string }>) {
  return createStubPool((query: Query) => {
    const sql = query.text
    if (sql.includes('pg_roles')) return { rows: [{ unsafe: false }] }
    if (sql.includes('schema_migrations')) return { rows: migrations }
    if (sql.includes('pg_user'))
      return { rows: [{ usename: 'archava_billing_app', usesuper: false }] }
    if (sql.includes('lock_membership')) {
      return {
        rows: [
          {
            principal_id: 'owner',
            kind: 'human',
            active: state.active,
            permissions: state.permissions,
            scope: 'all',
          },
        ],
      }
    }
    if (sql.includes('SELECT receipt, fingerprint FROM archava_billing.commands')) {
      const record = state.commands.get(String(query.params[1]))
      return { rows: record ? [{ receipt: record.receipt, fingerprint: record.fingerprint }] : [] }
    }
    if (sql.includes('SELECT receipt FROM archava_billing.commands')) {
      const record = state.commands.get(String(query.params[1]))
      return { rows: record ? [{ receipt: record.receipt }] : [] }
    }
    if (sql.includes('INSERT INTO archava_billing.commands')) {
      state.commands.set(String(query.params[1]), {
        fingerprint: String(query.params[3]),
        receipt: null,
      })
      return { rows: [] }
    }
    if (sql.includes('UPDATE archava_billing.commands SET receipt')) {
      const record = state.commands.get(String(query.params[2]))
      if (record) record.receipt = query.params[0]
      return { rows: [] }
    }
    if (sql.includes('INSERT INTO archava_billing.invoices')) {
      const gross = Number(query.params[5])
      state.invoice = invoiceRow({
        invoice_code: query.params[2],
        customer_ref: query.params[3],
        currency: query.params[4],
        gross_minor: String(gross),
        outstanding_minor: String(gross),
        consumer_ref: query.params[6],
        created_at: now(),
      })
      return { rows: [state.invoice] }
    }
    if (sql.includes('SELECT * FROM archava_billing.invoices')) return { rows: [state.invoice] }
    if (sql.includes('UPDATE archava_billing.invoices')) {
      const p = query.params.map((v) => v)
      if (sql.includes('settled_minor')) {
        state.invoice.settled_minor = String(p[0])
        state.invoice.outstanding_minor = String(p[1])
        state.invoice.status = p[2]
        state.invoice.revision = p[3]
        if (p[2] === 'settled') state.invoice.settled_at = now()
      } else if (sql.includes('correction_minor')) {
        state.invoice.correction_minor = String(p[0])
        state.invoice.outstanding_minor = String(p[1])
        state.invoice.revision = p[2]
      } else if (sql.includes("status='issued'")) {
        state.invoice.status = 'issued'
        state.invoice.issued_at = now()
        state.invoice.revision = p[0]
      } else if (sql.includes("status='void'")) {
        state.invoice.status = 'void'
        state.invoice.voided_at = now()
        state.invoice.revision = p[0]
      }
      return { rows: [state.invoice] }
    }
    if (sql.includes('INSERT INTO archava_billing.invoice_lines')) {
      const line = makeRow({
        id: String(query.params[1]),
        invoice_id: query.params[2],
        description: query.params[3],
        quantity: query.params[4],
        unit_minor: String(query.params[5]),
        line_total_minor: String(query.params[6]),
        recorded_by: query.params[7],
        recorded_at: now(),
      })
      state.lines.push(line)
      return { rows: [line] }
    }
    if (sql.includes('SELECT * FROM archava_billing.invoice_lines')) return { rows: state.lines }
    if (sql.includes('SELECT * FROM archava_billing.invoice_corrections')) {
      const target = state.corrections.find((row) => row.id === query.params[1])
      return { rows: target ? [target] : [] }
    }
    if (sql.includes('INSERT INTO archava_billing.invoice_corrections')) {
      const record = makeRow({
        id: String(query.params[1]),
        invoice_id: query.params[2],
        amount_minor: String(query.params[3]),
        reason: query.params[4],
        reverses_id: query.params[5],
        recorded_by: query.params[6],
        created_at: now(),
      })
      state.corrections.push(record)
      return { rows: [record] }
    }
    if (sql.includes('INSERT INTO archava_billing.payment_attempts')) {
      const record = makeRow({
        id: String(query.params[1]),
        invoice_id: query.params[2],
        amount_minor: String(query.params[3]),
        currency: query.params[4],
        provider_ref: query.params[5],
        recorded_by: query.params[6],
        created_at: now(),
      })
      state.attempts.push(record)
      return { rows: [record] }
    }
    if (sql.includes('INSERT INTO archava_billing.payment_events')) {
      const record = makeRow({
        id: String(query.params[1]),
        invoice_id: query.params[2],
        provider_id: query.params[3],
        provider_event_id: query.params[4],
        outcome: query.params[5],
        amount_minor: String(query.params[6]),
        currency: query.params[7],
        recorded_by: query.params[8],
        verified_at: now(),
      })
      state.events.push(record)
      return { rows: [record] }
    }
    if (sql.includes('SELECT * FROM archava_billing.payment_events')) return { rows: state.events }
    if (sql.includes('INSERT INTO archava_billing.audit')) {
      state.audit.push(
        makeRow({
          request_id: query.params[1],
          operation: query.params[3],
          revision: query.params[5],
        }),
      )
      return { rows: [] }
    }
    return undefined
  })
}

async function repository(state: State) {
  return PostgresBillingRepository.create(poolFor(state, await billingMigrationLedger()), key)
}

const mutation = (value: Record<string, unknown>): BillingMutation =>
  billingMutationSchema.parse(value)

const signedEvent = (commandSeq: string, amountMinor: number, invoiceId = INVOICE_ID) =>
  mutation({
    type: 'record_provider_event',
    commandId: `550e8400-e29b-41d4-a716-44665544${commandSeq}`,
    invoiceId,
    providerId: 'acquirer-demo',
    providerEventId: `evt_${commandSeq}`,
    outcome: 'succeeded',
    amountMinor,
    currency: 'IDR',
    signature: 'a'.repeat(64),
  })

describe('billing persistence proofs', () => {
  it('creates a draft invoice whose gross is the sum of its priced lines', async () => {
    const state = createState()
    const repo = await repository(state)
    const receipt = billingReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'create_invoice',
          commandId: commandId(),
          invoiceCode: 'INV-2026-0001',
          customerRef: 'cust-alpha',
          currency: 'IDR',
          consumerRef: 'consumer-1',
          lines: [
            { description: 'Consultation', quantity: 2, unitMinor: 250000 },
            { description: 'Materials', quantity: 1, unitMinor: 100000 },
          ],
        }),
      ),
    )
    expect(receipt.action).toBe('create_invoice')
    expect(receipt.status).toBe('draft')
    expect(receipt.outstandingMinor).toBe(600000)
    expect(state.lines).toHaveLength(2)
    expect(state.invoice.gross_minor).toBe('600000')
    expect(state.audit[0]?.operation).toBe('create_invoice')
  })

  it('issues a draft, freezing it, and refuses a second issue', async () => {
    const state = createState({
      invoice: invoiceRow({ status: 'draft', gross_minor: '600000', outstanding_minor: '600000' }),
    })
    const repo = await repository(state)
    const first = await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'issue_invoice',
        commandId: commandId(),
        invoiceId: INVOICE_ID,
        expectedRevision: 1,
        reason: 'Approved',
      }),
    )
    expect(first.status).toBe('issued')
    expect(state.invoice.issued_at).not.toBeNull()

    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'issue_invoice',
          commandId: commandId(),
          invoiceId: INVOICE_ID,
          expectedRevision: 2,
          reason: 'Again',
        }),
      ),
    ).rejects.toMatchObject({ code: 'invalid_transition' })
  })

  it('appends a correction that adjusts what is owed without editing the snapshot', async () => {
    const state = createState({
      invoice: invoiceRow({ status: 'issued', gross_minor: '600000', outstanding_minor: '600000' }),
    })
    const repo = await repository(state)
    const receipt = billingReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'append_correction',
          commandId: commandId(),
          invoiceId: INVOICE_ID,
          amountMinor: -100000,
          reason: 'Goodwill credit',
        }),
      ),
    )
    expect(receipt.action).toBe('append_correction')
    expect(receipt.outstandingMinor).toBe(500000)
    expect(state.invoice.correction_minor).toBe('-100000')
    expect(state.invoice.gross_minor).toBe('600000')
    expect(state.corrections).toHaveLength(1)
  })

  it('records a payment attempt that changes nothing about the balance', async () => {
    const state = createState({
      invoice: invoiceRow({ status: 'issued', gross_minor: '600000', outstanding_minor: '600000' }),
    })
    const repo = await repository(state)
    const receipt = billingReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_payment_attempt',
          commandId: commandId(),
          invoiceId: INVOICE_ID,
          amountMinor: 600000,
          providerRef: 'pi_123',
        }),
      ),
    )
    expect(receipt.action).toBe('record_payment_attempt')
    expect(receipt.outstandingMinor).toBe(600000)
    expect(state.invoice.settled_minor).toBe('0')
    expect(state.invoice.status).toBe('issued')
    expect(state.attempts).toHaveLength(1)
    expect(state.events).toHaveLength(0)
  })

  it('settles an issued invoice only through a provider event', async () => {
    const state = createState({
      invoice: invoiceRow({
        status: 'issued',
        gross_minor: '600000',
        outstanding_minor: '600000',
        revision: 2,
      }),
    })
    const repo = await repository(state)
    const receipt = billingReceiptSchema.parse(
      await repo.execute(tenant, human('owner'), signedEvent('0101', 600000)),
    )
    expect(receipt.status).toBe('settled')
    expect(receipt.outstandingMinor).toBe(0)
    expect(state.invoice.settled_minor).toBe('600000')
    expect(state.invoice.status).toBe('settled')
    expect(state.invoice.settled_at).not.toBeNull()
    expect(state.events).toHaveLength(1)
  })

  it('reopens a settled invoice when a provider reports a refund', async () => {
    const state = createState({
      invoice: invoiceRow({
        status: 'settled',
        gross_minor: '600000',
        settled_minor: '600000',
        outstanding_minor: '0',
        revision: 3,
        settled_at: now(),
      }),
    })
    const repo = await repository(state)
    const receipt = billingReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_provider_event',
          commandId: commandId(),
          invoiceId: INVOICE_ID,
          providerId: 'acquirer-demo',
          providerEventId: 'evt_refund',
          outcome: 'refunded',
          amountMinor: 600000,
          currency: 'IDR',
          signature: 'b'.repeat(64),
        }),
      ),
    )
    expect(receipt.status).toBe('issued')
    expect(receipt.outstandingMinor).toBe(600000)
    expect(state.invoice.settled_minor).toBe('0')
    expect(state.invoice.status).toBe('issued')
  })

  it('records a failed provider event without moving any money', async () => {
    const state = createState({
      invoice: invoiceRow({ status: 'issued', gross_minor: '600000', outstanding_minor: '600000' }),
    })
    const repo = await repository(state)
    const receipt = billingReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_provider_event',
          commandId: commandId(),
          invoiceId: INVOICE_ID,
          providerId: 'acquirer-demo',
          providerEventId: 'evt_failed',
          outcome: 'failed',
          amountMinor: 600000,
          currency: 'IDR',
          signature: 'c'.repeat(64),
        }),
      ),
    )
    expect(receipt.status).toBe('failed')
    expect(receipt.outstandingMinor).toBe(600000)
    expect(state.invoice.settled_minor).toBe('0')
    // A failed notification is pure history: the invoice row — status, balance
    // and revision alike — is left exactly as it was.
    expect(state.invoice.revision).toBe(1)
    expect(state.invoice.status).toBe('issued')
    expect(state.events).toHaveLength(1)
  })

  it('replays an identical command idempotently and refuses a changed payload', async () => {
    const state = createState({
      invoice: invoiceRow({ status: 'issued', gross_minor: '600000', outstanding_minor: '600000' }),
    })
    const repo = await repository(state)
    const cmd = '0202'
    const event = signedEvent(cmd, 600000)

    const first = await repo.execute(tenant, human('owner'), event)
    const replay = await repo.execute(tenant, human('owner'), event)
    expect(replay.idempotencyReplay).toBe(true)
    expect(replay.eventId).toBe(first.eventId)
    expect(state.events).toHaveLength(1)

    await expect(
      repo.execute(tenant, human('owner'), signedEvent(cmd, 100000)),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('withholds settlement from a clerk who lacks the settle grant', async () => {
    const state = createState({
      permissions: CLERK,
      invoice: invoiceRow({ status: 'draft', gross_minor: '600000', outstanding_minor: '600000' }),
    })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'issue_invoice',
          commandId: commandId(),
          invoiceId: INVOICE_ID,
          expectedRevision: 1,
          reason: 'Nope',
        }),
      ),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })
})
