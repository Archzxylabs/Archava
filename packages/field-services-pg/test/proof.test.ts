import { createHmac } from 'node:crypto'
import { describe, expect, it } from 'vitest'
import { FieldServicesError, type FieldCommand, type FieldPrincipal } from '@archava/field-services'
import { PostgresFieldRepository, fieldMigrationLedger } from '../src/index.js'
import { createStubPool, makeRow, type Row, type Query } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): FieldPrincipal => ({ kind: 'human', id })

const TECHNICIAN = ['field.read', 'field.write', 'field.dispatch', 'audit.read']
const APPROVER = ['field.read', 'field.signoff', 'audit.read']

const JOB_ID = 'job-b3f9c1d2'
const APPOINTMENT_REF = 'appt-2291'
const MIGRATION_ID = '001_field_services_dispatch_visits'

let sequence = 0
const commandId = () => `cmd-88${String(sequence++).padStart(2, '0')}`

function jobRow(overrides: Row = {}): Row {
  return makeRow({
    job_id: JOB_ID,
    tenant_id: tenant,
    customer_ref: 'cust-4471',
    site_ref: 'site-88',
    summary: 'Replace condenser coil',
    priority: 'high',
    status: 'requested',
    appointment_ref: APPOINTMENT_REF,
    asset_ref: null,
    assigned_principal: null,
    labor_hours: '0.00',
    parts_pieces: 0,
    revision: 0,
    created_by: 'tech-9',
    created_at: new Date('2026-10-12T09:00:00.000Z'),
    updated_at: new Date('2026-10-12T09:00:00.000Z'),
    ...overrides,
  })
}

type CommandRow = {
  commandId: string
  action: string
  jobId: string
  fingerprint: string
  receipt: unknown
}

type State = {
  job: Row | null
  targetStatus: string
  visits: Row[]
  lines: Row[]
  signoffs: Row[]
  commands: Map<string, CommandRow>
  audit: Row[]
  permissions: string[]
  active: boolean
}

function createState(overrides: Partial<State> = {}): State {
  return {
    job: jobRow(),
    targetStatus: 'assigned',
    visits: [],
    lines: [],
    signoffs: [],
    commands: new Map(),
    audit: [],
    permissions: TECHNICIAN,
    active: true,
    ...overrides,
  }
}

/** The same fingerprint the repository computes: keyed HMAC over the command. */
function fingerprintOf(command: FieldCommand): string {
  return createHmac('sha256', key).update(JSON.stringify(command)).digest('hex')
}

function poolFor(state: State, ledgerChecksum = '') {
  return createStubPool((query: Query) => {
    const sql = query.text
    const params = query.params

    if (sql.includes('pg_roles'))
      return {
        rows: [{ unsafe: false }],
      }
    if (sql.includes('schema_migrations'))
      return { rows: [{ id: MIGRATION_ID, checksum: ledgerChecksum }] }
    if (sql.includes('pg_user'))
      return { rows: [{ usename: 'archava_field_app', usesuper: false }] }
    if (sql.includes('lock_membership'))
      return {
        rows: [
          {
            principal_id: 'tech-9',
            kind: 'human',
            active: state.active,
            permissions: state.permissions,
            scope: 'all',
          },
        ],
      }

    // Recorded-command lookup inside a transaction, then the bare status query.
    if (sql.includes('SELECT command_id, action, job_id, revision, fingerprint, receipt')) {
      const record = state.commands.get(String(params[1]))
      return {
        rows: record
          ? [
              {
                command_id: record.commandId,
                action: record.action,
                job_id: record.jobId,
                revision: (record.receipt as { revision?: number })?.revision ?? 3,
                fingerprint: record.fingerprint,
                receipt: record.receipt,
              },
            ]
          : [],
      }
    }
    if (sql.includes('SELECT command_id, action, job_id, receipt')) {
      const record = state.commands.get(String(params[1]))
      return {
        rows: record
          ? [
              {
                command_id: record.commandId,
                action: record.action,
                job_id: record.jobId,
                receipt: record.receipt,
              },
            ]
          : [],
      }
    }
    if (sql.includes('INSERT INTO archava_field.commands')) {
      state.commands.set(String(params[0]), {
        commandId: String(params[0]),
        action: String(params[3]),
        jobId: String(params[1]),
        fingerprint: String(params[4]),
        receipt: JSON.parse(String(params[6])),
      })
      return { rows: [] }
    }

    if (sql.includes('INSERT INTO archava_field.jobs')) {
      state.job = jobRow({
        customer_ref: params[2],
        site_ref: params[3],
        summary: params[4],
        priority: params[5],
        appointment_ref: params[6],
        asset_ref: params[7],
        created_by: params[8],
      })
      return { rows: [state.job] }
    }

    if (
      sql.includes('SELECT * FROM archava_field.jobs WHERE tenant_id=$1 AND job_id=$2 FOR UPDATE')
    )
      return { rows: state.job ? [state.job] : [] }
    if (sql.includes('SELECT * FROM archava_field.jobs')) {
      if (sql.includes('WHERE tenant_id=$1 AND job_id=$2'))
        return { rows: state.job ? [state.job] : [] }
      return { rows: state.job ? [state.job] : [] }
    }

    if (sql.includes('UPDATE archava_field.jobs')) {
      // The status carried by the statement decides the next state, mirroring SQL:
      // a literal `status = 'x'` is read in place, while a bound `$3` carries the
      // one the caller computed (signoff can accept or reject, so it is not fixed).
      const literal = /status\s*=\s*'([a-z_]+)'/.exec(sql)?.[1]
      const status = literal ?? (sql.includes('status=$3') ? String(params[2]) : state.targetStatus)
      if (state.job) {
        state.job = jobRow({
          ...state.job,
          status,
          assigned_principal: sql.includes('assigned_principal=$3')
            ? params[2]
            : state.job.assigned_principal,
          labor_hours: sql.includes('labor_hours = labor_hours + $3')
            ? String(Number(state.job.labor_hours) + Number(params[2]))
            : state.job.labor_hours,
          parts_pieces: sql.includes('parts_pieces = parts_pieces + $4')
            ? Number(state.job.parts_pieces) + Number(params[3])
            : state.job.parts_pieces,
          revision: Number(state.job.revision) + 1,
          updated_at: new Date('2026-10-12T10:30:00.000Z'),
        })
      }
      return { rows: state.job ? [state.job] : [] }
    }

    if (sql.includes('INSERT INTO archava_field.visits')) {
      state.visits.push(
        makeRow({
          visit_id: params[0],
          job_id: params[1],
          outcome: params[2],
          note: params[3],
          technician_ref: params[4],
          visited_at: new Date('2026-10-12T10:30:00.000Z'),
        }),
      )
      return { rows: [] }
    }
    if (sql.includes('INSERT INTO archava_field.work_lines')) {
      state.lines.push(
        makeRow({
          line_id: params[0],
          job_id: params[1],
          kind: params[2],
          description: params[3],
          quantity: params[4],
          unit: params[5],
          part_ref: params[6],
          recorded_at: new Date('2026-10-12T10:31:00.000Z'),
        }),
      )
      return { rows: [] }
    }
    if (sql.includes('INSERT INTO archava_field.signoffs')) {
      state.signoffs.push(
        makeRow({
          signoff_id: params[0],
          job_id: params[1],
          signer_ref: params[2],
          accepted: params[3],
          comment: params[4],
          signed_at: new Date('2026-10-12T11:40:00.000Z'),
        }),
      )
      return { rows: [] }
    }
    if (sql.includes('INSERT INTO archava_field.audit')) {
      state.audit.push(makeRow({ entry_id: `ae-${state.audit.length}`, action: params[3] }))
      return { rows: [] }
    }

    if (sql.includes('SELECT * FROM archava_field.visits')) return { rows: state.visits }
    if (sql.includes('SELECT * FROM archava_field.work_lines')) return { rows: state.lines }
    if (sql.includes('SELECT * FROM archava_field.signoffs')) return { rows: state.signoffs }
    // Approve names its column rather than reading the row, so it needs its own arm.
    if (sql.includes('SELECT s.signoff_id FROM archava_field.signoffs')) {
      return {
        rows: state.signoffs
          .filter((row) => row.accepted === true)
          .map((row) => ({ signoff_id: row.signoff_id })),
      }
    }
    if (sql.includes('SELECT * FROM archava_field.audit')) return { rows: state.audit }
    return { rows: [] }
  })
}

async function repositoryFor(state: State): Promise<PostgresFieldRepository> {
  // The stub mirrors the packaged ledger exactly, the way a migrated runtime would.
  const ledger = await fieldMigrationLedger()
  const checksum = ledger[0]?.checksum ?? ''
  return await PostgresFieldRepository.create(poolFor(state, checksum), key)
}

describe('field services persistence proofs', () => {
  it('records the requested job without losing the scheduler appointment reference', async () => {
    const state = createState({ job: null })
    const repo = await repositoryFor(state)

    const receipt = await repo.execute(tenant, human('tech-9'), {
      action: 'create_job',
      commandId: commandId(),
      job: {
        jobId: JOB_ID,
        customerRef: 'cust-4471',
        siteRef: 'site-88',
        summary: 'Replace condenser coil',
        priority: 'high',
        appointmentRef: APPOINTMENT_REF,
        assetRef: null,
      },
    })

    expect(receipt.status).toBe('requested')
    expect(state.job?.appointment_ref).toBe(APPOINTMENT_REF)
  })

  it('accumulates labour hours and part pieces on separate counters', async () => {
    const state = createState({ job: jobRow({ status: 'visited' }), targetStatus: 'evidenced' })
    const repo = await repositoryFor(state)

    await repo.execute(tenant, human('tech-9'), {
      action: 'append_work_line',
      commandId: commandId(),
      jobId: JOB_ID,
      line: {
        lineId: 'wl-002',
        kind: 'labour',
        description: 'Diagnosis and replacement',
        quantity: 2.5,
        unit: 'hours',
        partRef: null,
      },
    })

    await repo.execute(tenant, human('tech-9'), {
      action: 'append_work_line',
      commandId: commandId(),
      jobId: JOB_ID,
      line: {
        lineId: 'wl-001',
        kind: 'part',
        description: 'Run capacitor 45/5 uF',
        quantity: 3,
        unit: 'pieces',
        partRef: 'part-889',
      },
    })

    // A part line must not add hours and a labour line must not add pieces.
    expect(Number(state.job?.labor_hours)).toBe(2.5)
    expect(Number(state.job?.parts_pieces)).toBe(3)
  })

  it('moves a visited job to evidenced when its first work line lands', async () => {
    const state = createState({ job: jobRow({ status: 'visited', revision: 1 }) })
    const repo = await repositoryFor(state)

    const receipt = await repo.execute(tenant, human('tech-9'), {
      action: 'append_work_line',
      commandId: commandId(),
      jobId: JOB_ID,
      line: {
        lineId: 'wl-010',
        kind: 'labour',
        description: 'Diagnosis and replacement',
        quantity: 1.5,
        unit: 'hours',
        partRef: null,
      },
    })

    expect(receipt.status).toBe('evidenced')
    expect(state.job?.status).toBe('evidenced')
  })

  it('refuses a signoff before the job has been visited and evidenced', async () => {
    const state = createState({ job: jobRow({ status: 'requested' }), permissions: APPROVER })
    const repo = await repositoryFor(state)

    await expect(
      repo.execute(tenant, human('tech-9'), {
        action: 'record_signoff',
        commandId: commandId(),
        jobId: JOB_ID,
        signoff: {
          signoffId: 'so-5520',
          signerRef: 'cust-4471',
          accepted: true,
          comment: null,
        },
      }),
    ).rejects.toMatchObject({ code: 'illegal_transition' })
  })

  it('refuses an approval that skips the signoff', async () => {
    const state = createState({
      job: jobRow({ status: 'evidenced', revision: 2 }),
      permissions: APPROVER,
    })
    const repo = await repositoryFor(state)

    await expect(
      repo.execute(tenant, human('tech-9'), {
        action: 'approve_job',
        commandId: commandId(),
        jobId: JOB_ID,
      }),
    ).rejects.toMatchObject({ code: 'illegal_transition' })
  })

  it('walks the whole forward chain to approval and then stops accepting evidence', async () => {
    const state = createState({ job: jobRow({ status: 'requested', revision: 0 }) })
    const repo = await repositoryFor(state)
    const principal = human('tech-9')

    const assigned = await repo.execute(tenant, principal, {
      action: 'assign_job',
      commandId: commandId(),
      jobId: JOB_ID,
      assignedPrincipal: 'tech-9',
    })
    expect(assigned.status).toBe('assigned')

    const visited = await repo.execute(tenant, principal, {
      action: 'record_visit',
      commandId: commandId(),
      jobId: JOB_ID,
      visit: {
        visitId: 'vis-7712',
        outcome: 'completed',
        note: 'Coil replaced, pressure tested',
        technicianRef: 'tech-9',
      },
    })
    expect(visited.status).toBe('visited')

    const evidenced = await repo.execute(tenant, principal, {
      action: 'append_work_line',
      commandId: commandId(),
      jobId: JOB_ID,
      line: {
        lineId: 'wl-002',
        kind: 'labour',
        description: 'Diagnosis and replacement',
        quantity: 2.5,
        unit: 'hours',
        partRef: null,
      },
    })
    expect(evidenced.status).toBe('evidenced')

    // Evidence already exists: a second line keeps the job where it is.
    const added = await repo.execute(tenant, principal, {
      action: 'append_work_line',
      commandId: commandId(),
      jobId: JOB_ID,
      line: {
        lineId: 'wl-001',
        kind: 'part',
        description: 'Run capacitor 45/5 uF',
        quantity: 1,
        unit: 'pieces',
        partRef: 'part-889',
      },
    })
    expect(added.status).toBe('evidenced')

    state.permissions = APPROVER
    const signer = human('cust-4471')
    const signedOff = await repo.execute(tenant, signer, {
      action: 'record_signoff',
      commandId: commandId(),
      jobId: JOB_ID,
      signoff: {
        signoffId: 'so-5520',
        signerRef: 'cust-4471',
        accepted: true,
        comment: null,
      },
    })
    expect(signedOff.status).toBe('signed_off')

    const approved = await repo.execute(tenant, signer, {
      action: 'approve_job',
      commandId: commandId(),
      jobId: JOB_ID,
    })
    expect(approved.status).toBe('approved')

    // A principal that can both write and dispatch still cannot touch a closed job.
    state.permissions = [...new Set([...TECHNICIAN, 'field.signoff'])]
    const writer = human('tech-9')
    await expect(
      repo.execute(tenant, writer, {
        action: 'append_work_line',
        commandId: commandId(),
        jobId: JOB_ID,
        line: {
          lineId: 'wl-009',
          kind: 'labour',
          description: 'Late visit',
          quantity: 1,
          unit: 'hours',
          partRef: null,
        },
      }),
    ).rejects.toMatchObject({ code: 'illegal_transition' })
  })

  it('pulls a signed-off job back to evidenced when its evidence is revised', async () => {
    const state = createState({
      job: jobRow({ status: 'signed_off', revision: 4 }),
      targetStatus: 'evidenced',
    })
    const repo = await repositoryFor(state)

    const receipt = await repo.execute(tenant, human('cust-4471'), {
      action: 'append_work_line',
      commandId: commandId(),
      jobId: JOB_ID,
      line: {
        lineId: 'wl-011',
        kind: 'part',
        description: 'Extra fitting',
        quantity: 2,
        unit: 'pieces',
        partRef: 'part-902',
      },
    })

    // The back-edge exists so the customer signs the corrected evidence again.
    expect(receipt.status).toBe('evidenced')
    expect(state.job?.status).toBe('evidenced')
  })

  it('refuses a second visit for a job that is already visited or beyond', async () => {
    const state = createState({ job: jobRow({ status: 'evidenced', revision: 2 }) })
    const repo = await repositoryFor(state)

    await expect(
      repo.execute(tenant, human('tech-9'), {
        action: 'record_visit',
        commandId: commandId(),
        jobId: JOB_ID,
        visit: {
          visitId: 'vis-7713',
          outcome: 'partial',
          note: 'Return trip',
          technicianRef: 'tech-9',
        },
      }),
    ).rejects.toMatchObject({ code: 'illegal_transition' })
  })

  it('replays the recorded receipt without recording a second mutation', async () => {
    const state = createState({ job: jobRow({ status: 'requested', revision: 0 }) })
    const repo = await repositoryFor(state)
    const command = {
      action: 'assign_job',
      commandId: commandId(),
      jobId: JOB_ID,
      assignedPrincipal: 'tech-9',
    } as FieldCommand

    const first = await repo.execute(tenant, human('tech-9'), command)
    state.commands.set(command.commandId, {
      commandId: command.commandId,
      action: 'assign_job',
      jobId: JOB_ID,
      fingerprint: fingerprintOf(command),
      receipt: first,
    })

    const replayed = await repo.execute(tenant, human('tech-9'), command)
    expect(replayed).toEqual(first)
    expect(state.audit).toHaveLength(1)
  })

  it('refuses a replay whose payload no longer matches the recorded fingerprint', async () => {
    const state = createState({ job: jobRow({ status: 'requested', revision: 0 }) })
    const repo = await repositoryFor(state)

    const command = {
      action: 'assign_job',
      commandId: commandId(),
      jobId: JOB_ID,
      assignedPrincipal: 'tech-9',
    } as FieldCommand
    await repo.execute(tenant, human('tech-9'), command)

    await expect(
      repo.execute(tenant, human('tech-9'), {
        ...command,
        assignedPrincipal: 'tech-other',
      } as FieldCommand),
    ).rejects.toMatchObject({ code: 'conflict' })
  })

  it('reports the recorded receipt as unknown when the command has no receipt yet', async () => {
    const state = createState()
    const repo = await repositoryFor(state)

    const command = {
      action: 'assign_job',
      commandId: commandId(),
      jobId: JOB_ID,
      assignedPrincipal: 'tech-9',
    } as FieldCommand
    state.commands.set(command.commandId, {
      commandId: command.commandId,
      action: 'assign_job',
      jobId: JOB_ID,
      fingerprint: fingerprintOf(command),
      receipt: null,
    })

    await expect(repo.execute(tenant, human('tech-9'), command)).rejects.toMatchObject({
      code: 'unknown_effect',
    })
  })

  it('recovers a recorded receipt through the command status query', async () => {
    const state = createState()
    const repo = await repositoryFor(state)
    const command = {
      action: 'assign_job',
      commandId: commandId(),
      jobId: JOB_ID,
      assignedPrincipal: 'tech-9',
    } as FieldCommand

    const first = await repo.execute(tenant, human('tech-9'), command)
    state.commands.set(command.commandId, {
      commandId: command.commandId,
      action: 'assign_job',
      jobId: JOB_ID,
      fingerprint: fingerprintOf(command),
      receipt: first,
    })

    const status = await repo.getCommandStatus(tenant, human('tech-9'), command.commandId)
    expect(status.status).toBe('accepted')
    expect(status.revision).toBe(first.revision)

    const unknown = await repo.getCommandStatus(tenant, human('tech-9'), 'cmd-unknown')
    expect(unknown.status).toBe('unknown')
  })

  it('withholds dispatch from a principal that may only read', async () => {
    const state = createState({ permissions: ['field.read'] })
    const repo = await repositoryFor(state)

    await expect(
      repo.execute(tenant, human('tech-9'), {
        action: 'assign_job',
        commandId: commandId(),
        jobId: JOB_ID,
        assignedPrincipal: 'tech-9',
      }),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('withholds approval from a principal without the signoff grant', async () => {
    const state = createState({
      job: jobRow({ status: 'signed_off', revision: 4 }),
      permissions: TECHNICIAN,
    })
    const repo = await repositoryFor(state)

    await expect(
      repo.execute(tenant, human('tech-9'), {
        action: 'approve_job',
        commandId: commandId(),
        jobId: JOB_ID,
      }),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('returns null instead of another tenant field job', async () => {
    const state = createState({ job: null })
    const repo = await repositoryFor(state)
    await expect(repo.getJobDetail(tenant, human('tech-9'), JOB_ID)).resolves.toBeNull()
  })

  it('returns a not_found failure for a command naming a missing job', async () => {
    const state = createState({ job: null })
    const repo = await repositoryFor(state)
    await expect(
      repo.execute(tenant, human('tech-9'), {
        action: 'assign_job',
        commandId: commandId(),
        jobId: JOB_ID,
        assignedPrincipal: 'tech-9',
      }),
    ).rejects.toMatchObject({ code: 'not_found' })
  })

  it('requires the audit grant before the trail is disclosed', async () => {
    const state = createState({ permissions: ['field.read'] })
    const repo = await repositoryFor(state)
    await expect(repo.listAudit(tenant, human('tech-9'), JOB_ID)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('records one audit entry per accepted command', async () => {
    const state = createState({ job: jobRow({ status: 'requested', revision: 0 }) })
    const repo = await repositoryFor(state)

    await repo.execute(tenant, human('tech-9'), {
      action: 'assign_job',
      commandId: commandId(),
      jobId: JOB_ID,
      assignedPrincipal: 'tech-9',
    })

    expect(state.audit).toHaveLength(1)
    expect(state.audit[0]?.action).toBe('assign_job')
  })

  it('describes the packaged migration ledger', async () => {
    const ledger = await fieldMigrationLedger()
    expect(ledger).toHaveLength(1)
    expect(ledger[0]?.id).toBe(MIGRATION_ID)
    expect(ledger[0]?.checksum).toMatch(/^[0-9a-f]{64}$/)
  })

  it('refuses an unknown command action as a bounded failure', async () => {
    const state = createState()
    const repo = await repositoryFor(state)
    await expect(
      repo.execute(tenant, human('tech-9'), { action: 'refund', commandId: 'cmd-0' } as never),
    ).rejects.toBeInstanceOf(FieldServicesError)
  })
})
