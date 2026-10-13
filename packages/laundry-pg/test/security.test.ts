import { describe, expect, it } from 'vitest'
import {
  LAUNDRY_PERMISSIONS,
  LaundryError,
  laundryMutationSchema,
  type LaundryMutation,
  type LaundryPrincipal,
} from '@archava/laundry'
import { PostgresLaundryRepository, laundryMigrationLedger } from '../src/index.js'
import { createStubPool } from './stub-pool.js'

const tenant = 'tenant-a'
const key = 'f'.repeat(32)
const human = (id: string): LaundryPrincipal => ({ kind: 'human', id })
const automated = (id: string, onBehalfOf: string): LaundryPrincipal => ({
  kind: 'service',
  id,
  onBehalfOf,
})

const createOrder: LaundryMutation = laundryMutationSchema.parse({
  type: 'create_order',
  commandId: '550e8400-e29b-41d4-a716-446655440000',
  orderCode: 'LDR-2026-0001',
  kind: 'weight',
  currency: 'IDR',
  lines: [{ itemRef: 'wash-fold-kg', quantity: 3000, unit: 'gram', unitPriceMinor: 12 }],
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
    options.migrations === undefined ? await laundryMigrationLedger() : options.migrations
  const pool = createStubPool((query) => {
    if (query.text.includes('pg_roles')) return { rows: [{ unsafe: options.unsafe ?? false }] }
    if (query.text.includes('schema_migrations')) return { rows: migrations }
    if (query.text.includes('pg_user')) {
      return { rows: [{ usename: 'archava_laundry_app', usesuper: options.usesuper ?? false }] }
    }
    if (query.text.includes('lock_membership')) {
      return {
        rows: [
          {
            principal_id: 'owner',
            kind: 'human',
            active: options.active ?? true,
            permissions: options.permissions ?? [...LAUNDRY_PERMISSIONS],
            scope: options.scope ?? 'all',
          },
        ],
      }
    }
    if (query.text.includes('SELECT * FROM archava_laundry.orders')) {
      return {
        rows: [
          {
            id: '770e8400-e29b-41d4-a716-446655440000',
            tenant_id: tenant,
            order_code: 'LDR-2026-0001',
            kind: 'weight',
            customer_ref: null,
            status: 'processing',
            currency: 'IDR',
            total_minor: 36000,
            item_count: 0,
            weight_grams: 3000,
            unreconciled_count: 0,
            owner_id: 'owner',
            revision: 2,
            created_at: new Date('2026-10-12T10:00:00.000Z'),
            updated_at: new Date('2026-10-12T10:00:00.000Z'),
          },
        ],
      }
    }
    if (query.text.includes('INSERT INTO archava_laundry.audit')) return { rows: [] }
    return undefined
  })
  const repository = await PostgresLaundryRepository.create(pool, key)
  return { repository, pool }
}

describe('laundry security boundaries', () => {
  it('rejects permissions outside the laundry permission vocabulary', async () => {
    const { repository } = await repositoryWith({ permissions: ['bogus.permission'] })
    await expect(repository.getAccess(tenant, human('owner'))).rejects.toThrow()
  })

  it('refuses superuser connections before any command is executed', async () => {
    const pool = createStubPool((query) =>
      query.text.includes('pg_roles') ? { rows: [{ unsafe: true }] } : undefined,
    )
    await expect(PostgresLaundryRepository.create(pool, key)).rejects.toThrow(/non-owner runtime/)
  })

  it('refuses a runtime whose migration history does not match the packaged ledger', async () => {
    const pool = createStubPool((query) => {
      if (query.text.includes('pg_roles')) return { rows: [{ unsafe: false }] }
      if (query.text.includes('schema_migrations')) return { rows: [] }
      return undefined
    })
    await expect(PostgresLaundryRepository.create(pool, key)).rejects.toThrow(/migrations mismatch/)
  })

  it('refuses short fingerprint keys so command fingerprints stay unlinkable', async () => {
    const pool = createStubPool(() => undefined)
    await expect(PostgresLaundryRepository.create(pool, 'short')).rejects.toThrow(
      /at least 32 characters/,
    )
  })

  it('reports a missing write grant as forbidden instead of recording the order', async () => {
    const { repository } = await repositoryWith({
      permissions: ['laundry.read', 'audit.read'],
    })
    await expect(repository.execute(tenant, human('owner'), createOrder)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('refuses a service principal that attempts a direct laundry mutation', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, automated('bot-auto', 'user-1'), createOrder),
    ).rejects.toBeInstanceOf(LaundryError)
  })

  it('refuses inactive principals without disclosing whether they exist', async () => {
    const { repository } = await repositoryWith({ active: false })
    await expect(repository.execute(tenant, human('owner'), createOrder)).rejects.toMatchObject({
      code: 'forbidden',
    })
  })

  it('requires the dedicated machine grant for wash, dry and press events', async () => {
    const { repository } = await repositoryWith({
      permissions: ['laundry.read', 'laundry.write'],
    })
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_process',
        commandId: '880e8400-e29b-41d4-a716-446655440000',
        orderId: '770e8400-e29b-41d4-a716-446655440000',
        event: 'wash',
        note: null,
        evidenceRef: null,
      }),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('withholds counter-side process events from machine-only operators', async () => {
    const { repository } = await repositoryWith({
      permissions: ['laundry.read', 'laundry.process'],
    })
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_process',
        commandId: '990e8400-e29b-41d4-a716-446655440000',
        orderId: '770e8400-e29b-41d4-a716-446655440000',
        event: 'intake',
        note: null,
        evidenceRef: null,
      }),
    ).rejects.toMatchObject({ code: 'forbidden' })
  })

  it('rejects a reconciliation reference attached to a non-accepted finding', async () => {
    const { repository } = await repositoryWith({})
    await expect(
      repository.execute(tenant, human('owner'), {
        type: 'record_finding',
        commandId: 'aa0e8400-e29b-41d4-a716-446655440000',
        orderId: '770e8400-e29b-41d4-a716-446655440000',
        category: 'stain',
        disposition: 'partial',
        quantity: 2,
        reason: 'Ink stain remains',
        evidenceRef: null,
        reconcilesId: '660e8400-e29b-41d4-a716-446655440000',
      }),
    ).rejects.toMatchObject({ code: 'bad_request' })
  })
})
