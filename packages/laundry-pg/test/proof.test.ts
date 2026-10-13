import { describe, expect, it } from 'vitest'
import {
  laundryReceiptSchema,
  laundryMutationSchema,
  type LaundryMutation,
  type LaundryPrincipal,
} from '@archava/laundry'
import { PostgresLaundryRepository, laundryMigrationLedger } from '../src/index.js'
import { createStubPool, makeRow, type Row, type Query } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): LaundryPrincipal => ({ kind: 'human', id })

const OWNER = ['laundry.read', 'laundry.write', 'laundry.process', 'audit.read']
const COUNTER = ['laundry.read', 'laundry.write']

const ORDER_ID = '550e8400-e29b-41d4-a716-446655440000'
const FINDING_ID = '660e8400-e29b-41d4-a716-446655440000'
const PROCESS_ID = '770e8400-e29b-41d4-a716-446655440000'
let commandSequence = 0
const commandId = () =>
  `550e8400-e29b-41d4-a716-44665544${String(commandSequence++).padStart(4, '0')}`

function orderRow(overrides: Row = {}): Row {
  return makeRow({
    id: ORDER_ID,
    tenant_id: tenant,
    order_code: 'LDR-2026-0001',
    kind: 'weight',
    customer_ref: 'customer-andi',
    status: 'received',
    currency: 'IDR',
    total_minor: 36000,
    item_count: 0,
    weight_grams: 3000,
    unreconciled_count: 0,
    owner_id: 'owner',
    revision: 1,
    created_at: new Date('2026-10-12T10:00:00.000Z'),
    updated_at: new Date('2026-10-12T10:00:00.000Z'),
    ...overrides,
  })
}

type State = {
  order: Row
  lines: Row[]
  process: Row[]
  findings: Row[]
  commands: Map<string, { fingerprint: string; receipt: unknown }>
  audit: Row[]
  permissions: string[]
  active: boolean
}

function createState(overrides: Partial<State> = {}): State {
  return {
    order: orderRow(),
    lines: [],
    process: [],
    findings: [],
    commands: new Map(),
    audit: [],
    permissions: OWNER,
    active: true,
    ...overrides,
  }
}

function poolFor(state: State, migrations: Array<{ id: string; checksum: string }>) {
  const now = () => new Date('2026-10-12T10:30:00.000Z')
  return createStubPool((query: Query) => {
    const sql = query.text
    if (sql.includes('pg_roles')) return { rows: [{ unsafe: false }] }
    if (sql.includes('schema_migrations')) return { rows: migrations }
    if (sql.includes('pg_user'))
      return { rows: [{ usename: 'archava_laundry_app', usesuper: false }] }
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
    if (sql.includes('SELECT receipt, fingerprint FROM archava_laundry.commands')) {
      const id = String(query.params[1])
      const record = state.commands.get(id)
      return { rows: record ? [{ receipt: record.receipt, fingerprint: record.fingerprint }] : [] }
    }
    if (sql.includes('SELECT receipt FROM archava_laundry.commands')) {
      const record = state.commands.get(String(query.params[1]))
      return { rows: record ? [{ receipt: record.receipt }] : [] }
    }
    if (sql.includes('INSERT INTO archava_laundry.commands')) {
      state.commands.set(String(query.params[1]), {
        fingerprint: String(query.params[3]),
        receipt: null,
      })
      return { rows: [] }
    }
    if (sql.includes('UPDATE archava_laundry.commands SET receipt')) {
      const record = state.commands.get(String(query.params[2]))
      if (record) record.receipt = query.params[0]
      return { rows: [] }
    }
    if (sql.includes('INSERT INTO archava_laundry.orders')) {
      state.order = orderRow({
        order_code: query.params[2],
        kind: query.params[3],
        customer_ref: query.params[4],
        currency: query.params[5],
        total_minor: query.params[6],
        item_count: query.params[7],
        weight_grams: query.params[8],
        created_at: now(),
        updated_at: now(),
      })
      return { rows: [state.order] }
    }
    if (sql.includes('INSERT INTO archava_laundry.order_lines')) {
      const line = makeRow({
        id: String(query.params[1]),
        tenant_id: tenant,
        order_id: query.params[2],
        item_ref: query.params[3],
        quantity: query.params[4],
        unit: query.params[5],
        unit_price_minor: query.params[6],
        line_total_minor: query.params[7],
        recorded_by: query.params[8],
        recorded_at: now(),
      })
      state.lines.push(line)
      return { rows: [] }
    }
    if (sql.includes('SELECT * FROM archava_laundry.orders')) return { rows: [state.order] }
    if (sql.includes('UPDATE archava_laundry.orders')) {
      const params = query.params.map((value) => value)
      if (sql.includes('unreconciled_count')) state.order.unreconciled_count = params[0]
      else {
        state.order.status = params[0]
        state.order.revision = params[1]
      }
      state.order.updated_at = now()
      return { rows: [state.order] }
    }
    if (sql.includes('SELECT event FROM archava_laundry.process_events')) {
      return { rows: state.process.map((row) => ({ event: row.event })) }
    }
    if (sql.includes("event='quality_check'")) {
      const found = state.process.some((row) => row.event === 'quality_check')
      return { rows: found ? [{ '?column?': 1 }] : [] }
    }
    if (sql.includes('INSERT INTO archava_laundry.process_events')) {
      const record = makeRow({
        id: String(query.params[1]),
        tenant_id: tenant,
        order_id: query.params[2],
        event: query.params[3],
        note: query.params[4],
        evidence_ref: query.params[5],
        recorded_by: query.params[6],
        recorded_at: now(),
      })
      state.process.push(record)
      return { rows: [record] }
    }
    if (sql.includes('SELECT * FROM archava_laundry.findings')) {
      const target = state.findings.find((row) => row.id === query.params[1])
      return { rows: target ? [target] : [] }
    }
    if (sql.includes('SELECT 1 FROM archava_laundry.findings')) {
      const already = state.findings.some((row) => row.reconciles_id === query.params[1])
      return { rows: already ? [{ '?column?': 1 }] : [] }
    }
    if (sql.includes('INSERT INTO archava_laundry.findings')) {
      const finding = makeRow({
        id: String(query.params[1]),
        tenant_id: tenant,
        order_id: query.params[2],
        category: query.params[3],
        disposition: query.params[4],
        quantity: query.params[5],
        reason: query.params[6],
        evidence_ref: query.params[7],
        reconciles_id: query.params[8],
        recorded_by: query.params[9],
        recorded_at: now(),
      })
      state.findings.push(finding)
      return { rows: [finding] }
    }
    if (sql.includes('INSERT INTO archava_laundry.audit')) {
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
  return PostgresLaundryRepository.create(poolFor(state, await laundryMigrationLedger()), key)
}

const mutation = (value: Record<string, unknown>): LaundryMutation =>
  laundryMutationSchema.parse(value)

describe('laundry persistence proofs', () => {
  it('creates a received order with a server-computed frozen total and immutable audit', async () => {
    const state = createState()
    const repo = await repository(state)
    const receipt = laundryReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'create_order',
          commandId: commandId(),
          orderCode: 'LDR-2026-0001',
          kind: 'weight',
          customerRef: 'customer-andi',
          currency: 'IDR',
          lines: [{ itemRef: 'wash-fold-kg', quantity: 3000, unit: 'gram', unitPriceMinor: 12 }],
        }),
      ),
    )
    expect(receipt.action).toBe('create_order')
    expect(receipt.status).toBe('received')
    expect(receipt.revision).toBe(1)
    expect(state.order.total_minor).toBe(36000)
    expect(state.order.weight_grams).toBe(3000)
    expect(state.lines).toHaveLength(1)
    expect(state.audit).toHaveLength(1)
    expect(state.audit[0]?.operation).toBe('create_order')
  })

  it('splits piece and gram quantities onto separate counters', async () => {
    const state = createState()
    const repo = await repository(state)
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'create_order',
        commandId: commandId(),
        orderCode: 'LDR-2026-0002',
        kind: 'item',
        currency: 'IDR',
        lines: [
          { itemRef: 'press-shirt', quantity: 3, unit: 'piece', unitPriceMinor: 9000 },
          { itemRef: 'dryclean-suit', quantity: 1, unit: 'piece', unitPriceMinor: 45000 },
        ],
      }),
    )
    expect(state.order.item_count).toBe(4)
    expect(state.order.weight_grams).toBe(0)
    expect(state.order.total_minor).toBe(72000)
    expect(state.lines).toHaveLength(2)
  })

  it('forces the processing chain to begin at intake', async () => {
    const state = createState()
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_process',
          commandId: commandId(),
          orderId: ORDER_ID,
          event: 'wash',
        }),
      ),
    ).rejects.toMatchObject({ code: 'process_out_of_order' })
  })

  it('keeps the processing chain monotonic and refuses repeated or backwards stages', async () => {
    const state = createState()
    const repo = await repository(state)
    for (const event of ['intake', 'tag', 'wash', 'dry']) {
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_process',
          commandId: commandId(),
          orderId: ORDER_ID,
          event,
        }),
      )
    }
    expect(state.process).toHaveLength(4)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_process',
          commandId: commandId(),
          orderId: ORDER_ID,
          event: 'tag',
        }),
      ),
    ).rejects.toMatchObject({ code: 'process_out_of_order' })
  })

  it('increases the unreconciled counter for every quality exception', async () => {
    const state = createState()
    const repo = await repository(state)
    const receipt = laundryReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_finding',
          commandId: commandId(),
          orderId: ORDER_ID,
          category: 'stain',
          disposition: 'partial',
          quantity: 2,
          reason: 'Ink stain remains after wash',
        }),
      ),
    )
    expect(receipt.findingId).toBeDefined()
    expect(state.order.unreconciled_count).toBe(1)
    expect(state.findings).toHaveLength(1)
  })

  it('clears exactly one unreconciled exception per accepted reconciliation', async () => {
    const state = createState({ order: orderRow({ unreconciled_count: 1 }) })
    state.findings.push(
      makeRow({
        id: FINDING_ID,
        tenant_id: tenant,
        order_id: ORDER_ID,
        category: 'stain',
        disposition: 'partial',
        quantity: 2,
        reason: 'Ink stain remains',
        evidence_ref: null,
        reconciles_id: null,
        recorded_by: 'owner',
        recorded_at: new Date('2026-10-12T10:00:00.000Z'),
      }),
    )
    const repo = await repository(state)
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_finding',
        commandId: commandId(),
        orderId: ORDER_ID,
        category: 'stain',
        disposition: 'accepted',
        reason: 'Stain removed on re-press',
        reconcilesId: FINDING_ID,
      }),
    )
    expect(state.order.unreconciled_count).toBe(0)
    expect(state.findings).toHaveLength(2)
  })

  it('refuses a reconciliation whose target lives on another order', async () => {
    const state = createState()
    state.findings.push(
      makeRow({
        id: FINDING_ID,
        tenant_id: tenant,
        order_id: '99999999-9999-4999-8999-999999999999',
        category: 'damage',
        disposition: 'rejected',
        quantity: 1,
        reason: 'Torn lining',
        evidence_ref: null,
        reconciles_id: null,
        recorded_by: 'owner',
        recorded_at: new Date('2026-10-12T10:00:00.000Z'),
      }),
    )
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_finding',
          commandId: commandId(),
          orderId: ORDER_ID,
          category: 'damage',
          disposition: 'accepted',
          reason: 'Compensation agreed',
          reconcilesId: FINDING_ID,
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('refuses a second reconciliation of the same finding', async () => {
    const state = createState()
    state.findings.push(
      makeRow({
        id: FINDING_ID,
        tenant_id: tenant,
        order_id: ORDER_ID,
        category: 'stain',
        disposition: 'partial',
        quantity: 2,
        reason: 'Ink stain remains',
        evidence_ref: null,
        reconciles_id: null,
        recorded_by: 'owner',
        recorded_at: new Date('2026-10-12T10:00:00.000Z'),
      }),
      makeRow({
        id: PROCESS_ID,
        tenant_id: tenant,
        order_id: ORDER_ID,
        category: 'stain',
        disposition: 'accepted',
        quantity: null,
        reason: 'Stain removed',
        evidence_ref: null,
        reconciles_id: FINDING_ID,
        recorded_by: 'owner',
        recorded_at: new Date('2026-10-12T10:05:00.000Z'),
      }),
    )
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_finding',
          commandId: commandId(),
          orderId: ORDER_ID,
          category: 'stain',
          disposition: 'accepted',
          reason: 'Second attempt',
          reconcilesId: FINDING_ID,
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('blocks ready while a quality exception is still unreconciled', async () => {
    const state = createState({
      order: orderRow({ status: 'processing', revision: 3, unreconciled_count: 1 }),
    })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_order_status',
          commandId: commandId(),
          orderId: ORDER_ID,
          expectedRevision: 3,
          targetStatus: 'ready',
          reason: 'Close the batch',
        }),
      ),
    ).rejects.toMatchObject({ code: 'unreconciled_exceptions' })
  })

  it('requires a recorded quality check before the order is marked ready', async () => {
    const state = createState({
      order: orderRow({ status: 'processing', revision: 3, unreconciled_count: 0 }),
    })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_order_status',
          commandId: commandId(),
          orderId: ORDER_ID,
          expectedRevision: 3,
          targetStatus: 'ready',
          reason: 'Batch looks done',
        }),
      ),
    ).rejects.toMatchObject({ code: 'invalid_transition' })
  })

  it('advances a clean order through the lifecycle with optimistic revisions', async () => {
    const state = createState({ process: [{ event: 'quality_check' }] as Row[] })
    const repo = await repository(state)
    const processing = laundryReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_order_status',
          commandId: commandId(),
          orderId: ORDER_ID,
          expectedRevision: 1,
          targetStatus: 'processing',
          reason: 'Batch started',
        }),
      ),
    )
    expect(processing.revision).toBe(2)
    expect(state.order.status).toBe('processing')

    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_order_status',
          commandId: commandId(),
          orderId: ORDER_ID,
          expectedRevision: 1,
          targetStatus: 'ready',
          reason: 'Stale write',
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })

    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_order_status',
          commandId: commandId(),
          orderId: ORDER_ID,
          expectedRevision: 2,
          targetStatus: 'returned',
          reason: 'Skip straight to return',
        }),
      ),
    ).rejects.toMatchObject({ code: 'invalid_transition' })
  })

  it('refuses further evidence once the order is returned or canceled', async () => {
    const state = createState({ order: orderRow({ status: 'returned', revision: 4 }) })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_finding',
          commandId: commandId(),
          orderId: ORDER_ID,
          category: 'missing',
          disposition: 'partial',
          quantity: 1,
          reason: 'Garment missing after return',
        }),
      ),
    ).rejects.toMatchObject({ code: 'invalid_transition' })
  })

  it('replays the original receipt without recording a second mutation', async () => {
    const state = createState()
    const repo = await repository(state)
    const request = mutation({
      type: 'record_process',
      commandId: commandId(),
      orderId: ORDER_ID,
      event: 'intake',
      note: '3 pieces counted',
      evidenceRef: 'TAG-0001',
    })
    const first = await repo.execute(tenant, human('owner'), request)
    const replay = await repo.execute(tenant, human('owner'), request)
    expect(replay.idempotencyReplay).toBe(true)
    expect(replay.processId).toBe(first.processId)
    expect(state.process).toHaveLength(1)
  })

  it('refuses a replay whose payload no longer matches the recorded fingerprint', async () => {
    const state = createState()
    const repo = await repository(state)
    const id = commandId()
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_process',
        commandId: id,
        orderId: ORDER_ID,
        event: 'intake',
        note: '3 pieces counted',
      }),
    )
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_process',
          commandId: id,
          orderId: ORDER_ID,
          event: 'intake',
          note: '9 pieces counted',
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('withholds machine events from operators without the process grant', async () => {
    const state = createState({ permissions: COUNTER })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_process',
          commandId: commandId(),
          orderId: ORDER_ID,
          event: 'intake',
        }),
      ),
    ).resolves.toBeDefined()
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_process',
          commandId: commandId(),
          orderId: ORDER_ID,
          event: 'wash',
        }),
      ),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('recovers a recorded receipt through the command status query', async () => {
    const state = createState()
    const repo = await repository(state)
    const id = commandId()
    const created = await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_process',
        commandId: id,
        orderId: ORDER_ID,
        event: 'intake',
      }),
    )
    const status = await repo.getCommandStatus(tenant, human('owner'), id)
    expect(status.status).toBe('recorded')
    expect(status.status === 'recorded' && status.receipt.processId).toBe(created.processId)
    expect((await repo.getCommandStatus(tenant, human('owner'), commandId())).status).toBe(
      'unknown',
    )
  })
})
