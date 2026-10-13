import { describe, expect, it } from 'vitest'
import {
  fulfillmentReceiptSchema,
  fulfillmentMutationSchema,
  type FulfillmentMutation,
  type FulfillmentPrincipal,
} from '@archava/fulfillment'
import { PostgresFulfillmentRepository, fulfillmentMigrationLedger } from '../src/index.js'
import { createStubPool, makeRow, type Row, type Query } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): FulfillmentPrincipal => ({ kind: 'human', id })

const OWNER = [
  'fulfillment.read',
  'fulfillment.write',
  'fulfillment.dispatch',
  'fulfillment.produce',
  'audit.read',
]
const PICKER = ['fulfillment.read', 'fulfillment.write']

const JOB_ID = '550e8400-e29b-41d4-a716-446655440000'
const FINDING_ID = '660e8400-e29b-41d4-a716-446655440000'
const CUSTODY_ID = '770e8400-e29b-41d4-a716-446655440000'
let commandSequence = 0
const commandId = () =>
  `550e8400-e29b-41d4-a716-44665544${String(commandSequence++).padStart(4, '0')}`

function jobRow(overrides: Row = {}): Row {
  return makeRow({
    id: JOB_ID,
    tenant_id: tenant,
    job_code: 'SHP-2026-0001',
    kind: 'dispatch',
    title: 'Restock Bandung DC',
    owner_ref: 'client-alpha',
    status: 'draft',
    consumed_count: 0,
    produced_count: 0,
    scrap_count: 0,
    unreconciled_count: 0,
    owner_id: 'owner',
    revision: 1,
    created_at: new Date('2026-10-12T10:00:00.000Z'),
    updated_at: new Date('2026-10-12T10:00:00.000Z'),
    ...overrides,
  })
}

type State = {
  job: Row
  lines: Row[]
  findings: Row[]
  custody: Row[]
  commands: Map<string, { fingerprint: string; receipt: unknown }>
  audit: Row[]
  permissions: string[]
  active: boolean
}

function createState(overrides: Partial<State> = {}): State {
  return {
    job: jobRow(),
    lines: [],
    findings: [],
    custody: [],
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
      return { rows: [{ usename: 'archava_fulfillment_app', usesuper: false }] }
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
    if (sql.includes('SELECT receipt, fingerprint FROM archava_fulfillment.commands')) {
      const id = String(query.params[1])
      const record = state.commands.get(id)
      return { rows: record ? [{ receipt: record.receipt, fingerprint: record.fingerprint }] : [] }
    }
    if (sql.includes('SELECT receipt FROM archava_fulfillment.commands')) {
      const record = state.commands.get(String(query.params[1]))
      return { rows: record ? [{ receipt: record.receipt }] : [] }
    }
    if (sql.includes('INSERT INTO archava_fulfillment.commands')) {
      state.commands.set(String(query.params[1]), {
        fingerprint: String(query.params[3]),
        receipt: null,
      })
      return { rows: [] }
    }
    if (sql.includes('UPDATE archava_fulfillment.commands SET receipt')) {
      const record = state.commands.get(String(query.params[2]))
      if (record) record.receipt = query.params[0]
      return { rows: [] }
    }
    if (sql.includes('INSERT INTO archava_fulfillment.jobs')) {
      state.job = jobRow({
        job_code: query.params[2],
        kind: query.params[3],
        title: query.params[4],
        owner_ref: query.params[5],
        created_at: now(),
        updated_at: now(),
      })
      return { rows: [state.job] }
    }
    if (sql.includes('SELECT * FROM archava_fulfillment.jobs')) return { rows: [state.job] }
    if (sql.includes('UPDATE archava_fulfillment.jobs')) {
      const params = query.params.map((value) => value)
      if (sql.includes('unreconciled_count')) state.job.unreconciled_count = params[0]
      else if (sql.includes('consumed_count')) {
        state.job.consumed_count = params[0]
        state.job.produced_count = params[1]
        state.job.scrap_count = params[2]
      } else {
        state.job.status = params[0]
        state.job.revision = params[1]
      }
      state.job.updated_at = now()
      return { rows: [state.job] }
    }
    if (sql.includes('INSERT INTO archava_fulfillment.job_lines')) {
      const line = makeRow({
        id: String(query.params[1]),
        tenant_id: tenant,
        job_id: query.params[2],
        direction: query.params[3],
        item_ref: query.params[4],
        quantity: query.params[5],
        disposition: query.params[6],
        evidence_ref: query.params[7],
        recorded_by: query.params[8],
        recorded_at: now(),
      })
      state.lines.push(line)
      return { rows: [line] }
    }
    if (sql.includes('SELECT * FROM archava_fulfillment.job_lines')) return { rows: state.lines }
    if (sql.includes('SELECT event FROM archava_fulfillment.custody_events')) {
      return { rows: state.custody.map((row) => ({ event: row.event })) }
    }
    if (sql.includes('INSERT INTO archava_fulfillment.custody_events')) {
      const record = makeRow({
        id: String(query.params[1]),
        tenant_id: tenant,
        job_id: query.params[2],
        event: query.params[3],
        from_ref: query.params[4],
        to_ref: query.params[5],
        evidence_ref: query.params[6],
        note: query.params[7],
        recorded_by: query.params[8],
        recorded_at: now(),
      })
      state.custody.push(record)
      return { rows: [record] }
    }
    if (sql.includes('SELECT * FROM archava_fulfillment.findings')) {
      const target = state.findings.find((row) => row.id === query.params[1])
      return { rows: target ? [target] : [] }
    }
    if (sql.includes('SELECT 1 FROM archava_fulfillment.findings')) {
      const already = state.findings.some((row) => row.reconciles_id === query.params[1])
      return { rows: already ? [{ '?column?': 1 }] : [] }
    }
    if (sql.includes('INSERT INTO archava_fulfillment.findings')) {
      const finding = makeRow({
        id: String(query.params[1]),
        tenant_id: tenant,
        job_id: query.params[2],
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
    if (sql.includes('SELECT * FROM archava_fulfillment.findings')) return { rows: state.findings }
    if (sql.includes('INSERT INTO archava_fulfillment.audit')) {
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
  return PostgresFulfillmentRepository.create(
    poolFor(state, await fulfillmentMigrationLedger()),
    key,
  )
}

const mutation = (value: Record<string, unknown>): FulfillmentMutation =>
  fulfillmentMutationSchema.parse(value)

describe('fulfillment persistence proofs', () => {
  it('creates a draft job and records an immutable audit entry', async () => {
    const state = createState()
    const repo = await repository(state)
    const receipt = fulfillmentReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'create_job',
          commandId: commandId(),
          jobCode: 'SHP-2026-0001',
          kind: 'dispatch',
          title: 'Restock Bandung DC',
          ownerRef: 'client-alpha',
        }),
      ),
    )
    expect(receipt.action).toBe('create_job')
    expect(receipt.status).toBe('draft')
    expect(receipt.revision).toBe(1)
    expect(state.audit).toHaveLength(1)
    expect(state.audit[0]?.operation).toBe('create_job')
  })

  it('keeps consumed, produced and scrap counters on separate axes', async () => {
    const state = createState({ job: jobRow({ kind: 'production', job_code: 'WO-2026-0001' }) })
    const repo = await repository(state)
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_line',
        commandId: commandId(),
        jobId: JOB_ID,
        direction: 'consume',
        itemRef: 'TEAK-PLANK-18',
        quantity: 18,
        disposition: 'normal',
      }),
    )
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_line',
        commandId: commandId(),
        jobId: JOB_ID,
        direction: 'produce',
        itemRef: 'DESK-TEAK-01',
        quantity: 5,
        disposition: 'normal',
      }),
    )
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_line',
        commandId: commandId(),
        jobId: JOB_ID,
        direction: 'produce',
        itemRef: 'DESK-TEAK-01',
        quantity: 1,
        disposition: 'scrap',
      }),
    )
    expect(state.job.consumed_count).toBe(18)
    expect(state.job.produced_count).toBe(5)
    expect(state.job.scrap_count).toBe(1)
    expect(state.lines).toHaveLength(3)
  })

  it('increases the unreconciled counter for every delivery exception', async () => {
    const state = createState()
    const repo = await repository(state)
    const receipt = fulfillmentReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_finding',
          commandId: commandId(),
          jobId: JOB_ID,
          category: 'delivery',
          disposition: 'partial',
          quantity: 4,
          reason: 'Two cartons short on arrival',
        }),
      ),
    )
    expect(receipt.findingId).toBeDefined()
    expect(state.job.unreconciled_count).toBe(1)
    expect(state.findings).toHaveLength(1)
  })

  it('clears exactly one unreconciled exception per accepted reconciliation', async () => {
    const state = createState({ job: jobRow({ unreconciled_count: 1 }) })
    state.findings.push(
      makeRow({
        id: FINDING_ID,
        tenant_id: tenant,
        job_id: JOB_ID,
        category: 'delivery',
        disposition: 'partial',
        quantity: 4,
        reason: 'Two cartons short',
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
        jobId: JOB_ID,
        category: 'delivery',
        disposition: 'accepted',
        reason: 'Short cartons rescheduled on the next trip',
        reconcilesId: FINDING_ID,
      }),
    )
    expect(state.job.unreconciled_count).toBe(0)
    expect(state.findings).toHaveLength(2)
  })

  it('refuses to reconcile a finding that is not an exception', async () => {
    const state = createState()
    state.findings.push(
      makeRow({
        id: FINDING_ID,
        tenant_id: tenant,
        job_id: JOB_ID,
        category: 'condition',
        disposition: 'accepted',
        quantity: null,
        reason: 'Packing fine',
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
          jobId: JOB_ID,
          category: 'condition',
          disposition: 'accepted',
          reason: 'Second look',
          reconcilesId: FINDING_ID,
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('refuses a reconciliation whose target lives on another job', async () => {
    const state = createState()
    state.findings.push(
      makeRow({
        id: FINDING_ID,
        tenant_id: tenant,
        job_id: '99999999-9999-4999-8999-999999999999',
        category: 'delivery',
        disposition: 'lost',
        quantity: 1,
        reason: 'Never arrived',
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
          jobId: JOB_ID,
          category: 'delivery',
          disposition: 'accepted',
          reason: 'Covered by insurance',
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
        job_id: JOB_ID,
        category: 'delivery',
        disposition: 'partial',
        quantity: 4,
        reason: 'Two cartons short',
        evidence_ref: null,
        reconciles_id: null,
        recorded_by: 'owner',
        recorded_at: new Date('2026-10-12T10:00:00.000Z'),
      }),
      makeRow({
        id: CUSTODY_ID,
        tenant_id: tenant,
        job_id: JOB_ID,
        category: 'delivery',
        disposition: 'accepted',
        quantity: null,
        reason: 'Already covered by the carrier claim',
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
          jobId: JOB_ID,
          category: 'delivery',
          disposition: 'accepted',
          reason: 'Already covered',
          reconcilesId: FINDING_ID,
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('blocks completion while an exception is still unreconciled', async () => {
    const state = createState({
      job: jobRow({ status: 'in_progress', revision: 3, unreconciled_count: 1 }),
    })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_job_status',
          commandId: commandId(),
          jobId: JOB_ID,
          expectedRevision: 3,
          targetStatus: 'completed',
          reason: 'Close the run',
        }),
      ),
    ).rejects.toMatchObject({ code: 'unreconciled_exceptions' })
  })

  it('advances a clean job through the lifecycle with optimistic revisions', async () => {
    const state = createState()
    const repo = await repository(state)
    const released = fulfillmentReceiptSchema.parse(
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_job_status',
          commandId: commandId(),
          jobId: JOB_ID,
          expectedRevision: 1,
          targetStatus: 'released',
          reason: 'Release for dispatch',
        }),
      ),
    )
    expect(released.revision).toBe(2)
    expect(state.job.status).toBe('released')

    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_job_status',
          commandId: commandId(),
          jobId: JOB_ID,
          expectedRevision: 1,
          targetStatus: 'in_progress',
          reason: 'Stale write',
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })

    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'change_job_status',
          commandId: commandId(),
          jobId: JOB_ID,
          expectedRevision: 2,
          targetStatus: 'completed',
          reason: 'Skip straight to closed',
        }),
      ),
    ).rejects.toMatchObject({ code: 'invalid_transition' })
  })

  it('refuses further evidence once the job is completed or canceled', async () => {
    const state = createState({ job: jobRow({ status: 'completed', revision: 4 }) })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_line',
          commandId: commandId(),
          jobId: JOB_ID,
          direction: 'consume',
          itemRef: 'CARTON-STD',
          quantity: 3,
        }),
      ),
    ).rejects.toMatchObject({ code: 'invalid_transition' })
  })

  it('forces the custody chain to begin at received', async () => {
    const state = createState()
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_custody',
          commandId: commandId(),
          jobId: JOB_ID,
          event: 'packed',
          fromRef: 'bay-3',
          toRef: 'dock-1',
        }),
      ),
    ).rejects.toMatchObject({ code: 'custody_out_of_order' })
  })

  it('keeps the custody chain monotonic and refuses repeated stages', async () => {
    const state = createState()
    const repo = await repository(state)
    for (const event of ['received', 'stored', 'picked', 'packed']) {
      await repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_custody',
          commandId: commandId(),
          jobId: JOB_ID,
          event,
          fromRef: null,
          toRef: null,
        }),
      )
    }
    expect(state.custody).toHaveLength(4)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_custody',
          commandId: commandId(),
          jobId: JOB_ID,
          event: 'stored',
          fromRef: null,
          toRef: null,
        }),
      ),
    ).rejects.toMatchObject({ code: 'custody_out_of_order' })
  })

  it('refuses a return until the goods have been dispatched', async () => {
    const state = createState()
    const repo = await repository(state)
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_custody',
        commandId: commandId(),
        jobId: JOB_ID,
        event: 'received',
        fromRef: null,
        toRef: null,
      }),
    )
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_custody',
          commandId: commandId(),
          jobId: JOB_ID,
          event: 'returned',
          fromRef: null,
          toRef: null,
        }),
      ),
    ).rejects.toMatchObject({ code: 'custody_out_of_order' })
  })

  it('replays the original receipt without recording a second mutation', async () => {
    const state = createState()
    const repo = await repository(state)
    const request = mutation({
      type: 'record_custody',
      commandId: commandId(),
      jobId: JOB_ID,
      event: 'received',
      fromRef: 'supplier-jaya',
      toRef: 'warehouse-bay-3',
    })
    const first = await repo.execute(tenant, human('owner'), request)
    const replay = await repo.execute(tenant, human('owner'), request)
    expect(replay.idempotencyReplay).toBe(true)
    expect(replay.custodyId).toBe(first.custodyId)
    expect(state.custody).toHaveLength(1)
  })

  it('refuses a replay whose payload no longer matches the recorded fingerprint', async () => {
    const state = createState()
    const repo = await repository(state)
    const id = commandId()
    await repo.execute(
      tenant,
      human('owner'),
      mutation({
        type: 'record_custody',
        commandId: id,
        jobId: JOB_ID,
        event: 'received',
        fromRef: 'supplier-jaya',
        toRef: 'warehouse-bay-3',
      }),
    )
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_custody',
          commandId: id,
          jobId: JOB_ID,
          event: 'received',
          fromRef: 'someone-else',
          toRef: 'warehouse-bay-9',
        }),
      ),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('requires the produce grant before production lines are accepted', async () => {
    const state = createState({
      job: jobRow({ kind: 'production', job_code: 'WO-2026-0001' }),
      permissions: PICKER,
    })
    const repo = await repository(state)
    await expect(
      repo.execute(
        tenant,
        human('owner'),
        mutation({
          type: 'record_line',
          commandId: commandId(),
          jobId: JOB_ID,
          direction: 'consume',
          itemRef: 'TEAK-PLANK-18',
          quantity: 18,
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
        type: 'record_custody',
        commandId: id,
        jobId: JOB_ID,
        event: 'received',
        fromRef: null,
        toRef: null,
      }),
    )
    const status = await repo.getCommandStatus(tenant, human('owner'), id)
    expect(status.status).toBe('recorded')
    expect(status.status === 'recorded' && status.receipt.custodyId).toBe(created.custodyId)
    expect((await repo.getCommandStatus(tenant, human('owner'), commandId())).status).toBe(
      'unknown',
    )
  })
})
