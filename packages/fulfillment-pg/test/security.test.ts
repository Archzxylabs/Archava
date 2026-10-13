import { describe, expect, it } from 'vitest'
import {
  FULFILLMENT_PERMISSIONS,
  FulfillmentError,
  fulfillmentMutationSchema,
  type FulfillmentMutation,
  type FulfillmentPrincipal,
} from '@archava/fulfillment'
import { PostgresFulfillmentRepository, fulfillmentMigrationLedger } from '../src/index.js'
import { createStubPool } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): FulfillmentPrincipal => ({ kind: 'human', id })
const automated = (id: string, onBehalfOf: string): FulfillmentPrincipal => ({
  kind: 'service',
  id,
  onBehalfOf,
})

const createJob: FulfillmentMutation = fulfillmentMutationSchema.parse({
  type: 'create_job',
  commandId: '550e8400-e29b-41d4-a716-446655440000',
  jobCode: 'SHP-2026-0001',
  kind: 'dispatch',
  title: 'Restock Bandung DC',
})

type Options = {
  unsafe?: boolean
  usesuper?: boolean
  permissions?: string[]
  scope?: string
  active?: boolean
  migrations?: Array<{ id: string; checksum: string }>
}

/** A pool whose replies are keyed on the statement the repository actually issues. */
async function repositoryWith(options: Options = {}) {
  const migrations =
    options.migrations === undefined ? await fulfillmentMigrationLedger() : options.migrations
  const pool = createStubPool((query) => {
    if (query.text.includes('pg_roles')) return { rows: [{ unsafe: options.unsafe ?? false }] }
    if (query.text.includes('schema_migrations')) return { rows: migrations }
    if (query.text.includes('pg_user')) {
      return { rows: [{ usename: 'archava_fulfillment_app', usesuper: options.usesuper ?? false }] }
    }
    if (query.text.includes('lock_membership')) {
      return {
        rows: [
          {
            principal_id: 'owner',
            kind: 'human',
            active: options.active ?? true,
            permissions: options.permissions ?? [...FULFILLMENT_PERMISSIONS],
            scope: options.scope ?? 'all',
          },
        ],
      }
    }
    if (query.text.includes('SELECT * FROM archava_fulfillment.jobs')) {
      return {
        rows: [
          {
            id: '770e8400-e29b-41d4-a716-446655440000',
            tenant_id: tenant,
            job_code: 'SHP-2026-0001',
            kind: 'dispatch',
            title: 'Restock Bandung DC',
            owner_ref: null,
            status: 'in_progress',
            consumed_count: 0,
            produced_count: 0,
            scrap_count: 0,
            unreconciled_count: 0,
            owner_id: 'owner',
            revision: 2,
            created_at: new Date('2026-10-12T10:00:00.000Z'),
            updated_at: new Date('2026-10-12T10:00:00.000Z'),
          },
        ],
      }
    }
    if (query.text.includes('INSERT INTO archava_fulfillment.audit')) return { rows: [] }
    return undefined
  })
  const repository = await PostgresFulfillmentRepository.create(pool, key)
  return { repository, pool }
}

describe('fulfillment security boundaries', () => {
  it('rejects permissions outside the fulfillment permission vocabulary', async () => {
    const { repository } = await repositoryWith({ permissions: ['bogus.permission'] })
    await expect(repository.getAccess(tenant, human('owner'))).rejects.toThrow()
  })

  it('refuses superuser connections before any command is executed', async () => {
    const pool = createStubPool((query) =>
      query.text.includes('pg_roles') ? { rows: [{ unsafe: true }] } : undefined,
    )
    await expect(PostgresFulfillmentRepository.create(pool, key)).rejects.toThrow(
      /non-owner runtime/,
    )
  })

  it('refuses a runtime whose migration history does not match the packaged ledger', async () => {
    const pool = createStubPool((query) => {
      if (query.text.includes('pg_roles')) return { rows: [{ unsafe: false }] }
      if (query.text.includes('schema_migrations')) return { rows: [] }
      return undefined
    })
    await expect(PostgresFulfillmentRepository.create(pool, key)).rejects.toThrow(
      /migrations mismatch/,
    )
  })

  it('refuses short fingerprint keys so command fingerprints stay unlinkable', async () => {
    const pool = createStubPool(() => undefined)
    await expect(PostgresFulfillmentRepository.create(pool, 'short')).rejects.toThrow(
      /at least 32 characters/,
    )
  })

  it('reports a missing write grant as forbidden instead of recording the job', async () => {
    const { repository } = await repositoryWith({
      permissions: ['fulfillment.read', 'audit.read'],
    })
    await expect(repository.execute(tenant, human('owner'), createJob)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('refuses a service principal that attempts a direct fulfillment mutation', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, automated('bot-auto', 'user-1'), createJob),
    ).rejects.toBeInstanceOf(FulfillmentError)
  })

  it('refuses inactive principals without disclosing whether they exist', async () => {
    const { repository } = await repositoryWith({ active: false })
    await expect(repository.execute(tenant, human('owner'), createJob)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('requires the dedicated dispatch grant for carrier hand-off custody', async () => {
    const { repository } = await repositoryWith({
      permissions: ['fulfillment.read', 'fulfillment.write'],
    })
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_custody',
        commandId: '880e8400-e29b-41d4-a716-446655440000',
        jobId: '770e8400-e29b-41d4-a716-446655440000',
        event: 'dispatched',
        fromRef: 'dock-1',
        toRef: 'carrier-nusantara',
        evidenceRef: null,
        note: null,
      }),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('rejects a reconciliation reference attached to a non-accepted finding', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_finding',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        jobId: '770e8400-e29b-41d4-a716-446655440000',
        category: 'delivery',
        disposition: 'lost',
        quantity: 2,
        reason: 'Cartons never arrived',
        evidenceRef: null,
        reconcilesId: '660e8400-e29b-41d4-a716-446655440000',
      }),
    ).rejects.toMatchObject({ code: 'bad_request' })
  })
})
