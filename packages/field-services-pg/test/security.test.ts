import { describe, expect, it } from 'vitest'
import { type FieldCommand, type FieldPrincipal } from '@archava/field-services'
import { PostgresFieldRepository, fieldMigrationLedger } from '../src/index.js'
import { createStubPool, makeRow, type Row, type Query } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): FieldPrincipal => ({ kind: 'human', id })
const JOB_ID = 'job-b3f9c1d2'
const MIGRATION_ID = '001_field_services_dispatch_visits'

function jobRow(overrides: Row = {}): Row {
  return makeRow({
    job_id: JOB_ID,
    tenant_id: tenant,
    customer_ref: 'cust-4471',
    site_ref: 'site-88',
    summary: 'Replace condenser coil',
    priority: 'high',
    status: 'requested',
    appointment_ref: 'appt-2291',
    asset_ref: null,
    assigned_principal: 'tech-9',
    labor_hours: '0.00',
    parts_pieces: 0,
    revision: 0,
    created_by: 'tech-9',
    created_at: new Date('2026-10-12T09:00:00.000Z'),
    updated_at: new Date('2026-10-12T09:00:00.000Z'),
    ...overrides,
  })
}

/** Every branch `PostgresFieldRepository.create` reaches for. */
type Handshake = {
  unsafe: boolean
  usesuper: boolean
  ledger?: { id: string; checksum: string }[]
  membership?: { active: boolean; permissions: string[]; scope?: 'all' | 'assigned' } | null
  jobs?: Row[]
}

function poolFor(handshake: Handshake) {
  const jobs = handshake.jobs ?? [jobRow()]
  const state = { commands: new Map<string, { receipt: unknown }>() }
  return createStubPool((query: Query) => {
    const sql = query.text
    const params = query.params
    if (sql.includes('pg_roles')) return { rows: [{ unsafe: handshake.unsafe }] }
    if (sql.includes('schema_migrations'))
      return { rows: handshake.ledger ?? [{ id: MIGRATION_ID, checksum: '' }] }
    if (sql.includes('pg_user'))
      return { rows: [{ usename: 'role', usesuper: handshake.usesuper }] }
    if (sql.includes('lock_membership'))
      return {
        rows:
          handshake.membership === null
            ? []
            : [
                handshake.membership ?? {
                  active: true,
                  permissions: ['field.read'],
                  scope: 'all',
                },
              ],
      }
    if (sql.includes('SELECT * FROM archava_field.jobs')) {
      // A list query is where the SQL scope predicate applies; a row-level
      // security database enforces it there rather than in application code.
      if (sql.includes('assigned_principal = $2 OR created_by = $2'))
        return {
          rows: jobs.filter(
            (row) =>
              row.assigned_principal === String(params[1]) || row.created_by === String(params[1]),
          ),
        }
      if (sql.includes('WHERE tenant_id=$1 AND job_id=$2')) return { rows: [] }
      return { rows: jobs }
    }
    if (sql.includes('pg_advisory_xact_lock')) return { rows: [] }
    if (sql.includes('SELECT command_id, action, job_id, revision, fingerprint, receipt')) {
      const record = state.commands.get(String(params[1]))
      return {
        rows: record
          ? [
              {
                command_id: params[1],
                receipt: record.receipt,
                fingerprint: String(params[4]),
              },
            ]
          : [],
      }
    }
    if (sql.includes('INSERT INTO archava_field.commands')) {
      state.commands.set(String(params[0]), { receipt: params[6] })
      return { rows: [] }
    }
    return { rows: [] }
  })
}

async function createWith(handshake: Handshake, fingerprintKey = key) {
  // A migrated runtime records the packaged checksum; the stub mirrors that.
  const ledger = await fieldMigrationLedger()
  const checksum = ledger[0]?.checksum ?? ''
  return await PostgresFieldRepository.create(
    poolFor({ ...handshake, ledger: handshake.ledger ?? [{ id: MIGRATION_ID, checksum }] }),
    fingerprintKey,
  )
}

describe('field services runtime security boundary', () => {
  it('refuses a runtime that owns the schema or bypasses row level security', async () => {
    await expect(createWith({ unsafe: true, usesuper: false, ledger: [] })).rejects.toThrow(
      /non-owner runtime/,
    )
  })

  it('refuses a superuser connection on the transaction path', async () => {
    // The superuser guard lives in the transaction, so it must be observed there.
    const repo = await createWith({
      unsafe: false,
      usesuper: true,
      ledger: undefined,
      membership: { active: true, permissions: ['field.read'] },
    })
    await expect(repo.listJobs(tenant, human('tech-9'), { limit: 50 })).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('refuses a runtime whose migration history does not match the packaged ledger', async () => {
    const ledger = await fieldMigrationLedger()
    const stale = [{ id: MIGRATION_ID, checksum: '0'.repeat(64) }]

    await expect(createWith({ unsafe: false, usesuper: false, ledger: stale })).rejects.toThrow(
      /migration/i,
    )
    expect(ledger).toHaveLength(1)
    expect(ledger[0]?.checksum).not.toBe(stale[0]?.checksum)
  })

  it('refuses an extra migration the packaged ledger does not declare', async () => {
    await expect(
      createWith({
        unsafe: false,
        usesuper: false,
        ledger: [
          { id: MIGRATION_ID, checksum: '' },
          { id: '002_extra.sql', checksum: 'd'.repeat(64) },
        ],
      }),
    ).rejects.toThrow(/migration/i)
  })

  it('refuses a fingerprint key shorter than the enforced floor', async () => {
    await expect(
      createWith({ unsafe: false, usesuper: false, ledger: [] }, 'short'),
    ).rejects.toThrow(/32 characters/)
  })

  it('refuses a principal that is not enrolled in the tenant', async () => {
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: null,
    })
    await expect(repo.getAccess(tenant, human('tech-9'))).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('refuses a deactivated principal', async () => {
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: { active: false, permissions: ['field.read'] },
    })
    await expect(repo.getAccess(tenant, human('tech-9'))).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('gives an assigned-scope principal only its own dispatch', async () => {
    const mine = jobRow({
      job_id: 'job-mine01',
      assigned_principal: 'tech-9',
      created_by: 'tech-9',
    })
    const theirs = jobRow({
      job_id: 'job-theirs',
      assigned_principal: 'tech-other',
      created_by: 'tech-other',
    })
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: { active: true, permissions: ['field.read'], scope: 'assigned' },
      jobs: [mine, theirs],
    })

    const jobs = await repo.listJobs(tenant, human('tech-9'), { limit: 50 })
    expect(jobs.map((job) => job.jobId)).toEqual(['job-mine01'])
  })

  it('gives an all-scope principal the whole tenant dispatch', async () => {
    const mine = jobRow({ job_id: 'job-mine01', assigned_principal: 'tech-9' })
    const theirs = jobRow({ job_id: 'job-theirs', assigned_principal: 'tech-other' })
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: { active: true, permissions: ['field.read'], scope: 'all' },
      jobs: [mine, theirs],
    })

    const jobs = await repo.listJobs(tenant, human('tech-9'), { limit: 50 })
    expect(jobs.map((job) => job.jobId).sort()).toEqual(['job-mine01', 'job-theirs'])
  })

  it('refuses a job lookup for another tenant', async () => {
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: { active: true, permissions: ['field.read'], scope: 'all' },
      jobs: [],
    })
    await expect(repo.getJobDetail(tenant, human('tech-9'), JOB_ID)).resolves.toBeNull()
  })

  it('refuses a command envelope that does not declare a known action', async () => {
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: { active: true, permissions: ['field.write'], scope: 'all' },
    })

    await expect(
      repo.execute(tenant, human('tech-9'), { action: 'refund', commandId: 'cmd-0' } as never),
    ).rejects.toMatchObject({ code: 'validation_failed' })
  })

  it('refuses a command that carries no idempotency key', async () => {
    const repo = await createWith({
      unsafe: false,
      usesuper: false,
      membership: { active: true, permissions: ['field.write'], scope: 'all' },
    })

    const command = {
      action: 'create_job',
      job: {
        jobId: JOB_ID,
        customerRef: 'cust-4471',
        siteRef: 'site-88',
        summary: 'Replace condenser coil',
        priority: 'high',
        appointmentRef: 'appt-2291',
        assetRef: null,
      },
    } as unknown as FieldCommand
    await expect(repo.execute(tenant, human('tech-9'), command)).rejects.toMatchObject({
      code: 'validation_failed',
    })
  })
})
