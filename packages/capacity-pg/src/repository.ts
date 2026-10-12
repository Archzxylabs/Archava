import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  CapacityError,
  capacityTenantSchema,
  capacityPrincipalSchema,
  capacityMembershipSchema,
  capacityPoolSchema,
  capacityAllocationSchema,
  commandReceiptSchema,
  type CapacityRepository,
  type CapacityPrincipal,
  type CapacityAccess,
  type CapacityPermission,
  type CapacityMutation,
  type CommandReceipt,
  type CommandStatusResult,
  type CapacityPageQuery,
  type CapacityPool,
  type CapacityAllocation,
  type CapacityPoolDetail,
  type CapacityEstimateRequest,
  type CapacityEstimate,
} from '@archava/capacity'
import { capacityMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid capacity timestamp')
  return x.toISOString()
}

const mapPool = (r: Row): CapacityPool =>
  capacityPoolSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    code: r.code,
    name: r.name,
    unit: r.unit,
    weeklyCapacity: Number(r.weekly_capacity),
    maxConcurrentJobs: Number(r.max_concurrent_jobs),
    state: r.state,
    ownerId: r.owner_id,
    revision: Number(r.revision),
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const mapAllocation = (r: Row): CapacityAllocation =>
  capacityAllocationSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    poolId: r.pool_id,
    workloadId: r.workload_id,
    workloadTitle: r.workload_title,
    requiredUnits: Number(r.required_units),
    remainingUnits: Number(r.remaining_units),
    scheduledStartAt: instant(r.scheduled_start_at),
    scheduledFinishAt: instant(r.scheduled_finish_at),
    state: r.state,
    ownerId: r.owner_id,
    revision: Number(r.revision),
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const grant = (a: CapacityAccess, p: CapacityPermission) => {
  if (!a.permissions.includes(p)) throw new CapacityError('forbidden')
}

const requiredCapacityPermission = (type: CapacityMutation['type']): CapacityPermission => {
  switch (type) {
    case 'create_pool':
    case 'update_pool':
    case 'archive_pool':
      return 'capacity.write'
    case 'commit_allocation':
    case 'update_allocation_progress':
    case 'complete_allocation':
    case 'cancel_allocation':
      return 'capacity.allocate'
  }
}

interface ScheduleResult {
  queuePosition: number
  scheduledStartAt: string
  scheduledFinishAt: string
  burnRatePerDay: number
  estimatedCalendarDays: number
}

export function calculateSchedule(
  pool: { weeklyCapacity: number; maxConcurrentJobs: number },
  activeAllocations: readonly CapacityAllocation[],
  requiredUnits: number,
  desiredStartAt?: string,
): ScheduleResult {
  const dailyBurnRate = pool.weeklyCapacity / 5
  if (dailyBurnRate <= 0) throw new CapacityError('insufficient_capacity')

  const now = new Date()
  let candidateStart = desiredStartAt ? new Date(desiredStartAt) : now

  while (candidateStart.getUTCDay() === 0 || candidateStart.getUTCDay() === 6) {
    candidateStart = new Date(candidateStart.getTime() + 24 * 60 * 60 * 1000)
    candidateStart.setUTCHours(0, 0, 0, 0)
  }

  const sortedAllocations = [...activeAllocations].sort(
    (a, b) => new Date(a.scheduledFinishAt).getTime() - new Date(b.scheduledFinishAt).getTime(),
  )

  let queuePosition = 0
  if (sortedAllocations.length >= pool.maxConcurrentJobs) {
    queuePosition = sortedAllocations.length - pool.maxConcurrentJobs + 1
    const releaseIdx = sortedAllocations.length - pool.maxConcurrentJobs
    const blocking = sortedAllocations[releaseIdx]
    if (blocking) {
      const blockingFinish = new Date(blocking.scheduledFinishAt)
      if (blockingFinish.getTime() > candidateStart.getTime()) {
        candidateStart = blockingFinish
        while (candidateStart.getUTCDay() === 0 || candidateStart.getUTCDay() === 6) {
          candidateStart = new Date(candidateStart.getTime() + 24 * 60 * 60 * 1000)
          candidateStart.setUTCHours(0, 0, 0, 0)
        }
      }
    }
  }

  let loopCount = 0
  while (loopCount < 365) {
    loopCount++
    while (candidateStart.getUTCDay() === 0 || candidateStart.getUTCDay() === 6) {
      candidateStart = new Date(candidateStart.getTime() + 24 * 60 * 60 * 1000)
      candidateStart.setUTCHours(0, 0, 0, 0)
    }

    const dayStart = candidateStart.getTime()
    const dayEnd = dayStart + 24 * 60 * 60 * 1000

    let usedCapacityToday = 0
    let concurrentJobsToday = 0
    let maxOverlappingFinish = candidateStart.getTime()

    for (const alloc of activeAllocations) {
      const aStart = new Date(alloc.scheduledStartAt).getTime()
      const aFinish = new Date(alloc.scheduledFinishAt).getTime()
      if (Math.max(aStart, dayStart) < Math.min(aFinish, dayEnd)) {
        const totalDurationDays = Math.max(
          1,
          Math.round((aFinish - aStart) / (24 * 60 * 60 * 1000)),
        )
        const allocDailyUnits = Math.min(dailyBurnRate, alloc.requiredUnits / totalDurationDays)
        usedCapacityToday += allocDailyUnits
        concurrentJobsToday++
        if (aFinish > maxOverlappingFinish) {
          maxOverlappingFinish = aFinish
        }
      }
    }

    const neededDaily = Math.min(dailyBurnRate, requiredUnits)
    if (
      concurrentJobsToday >= pool.maxConcurrentJobs ||
      usedCapacityToday + neededDaily > dailyBurnRate + 1e-6
    ) {
      if (queuePosition === 0) queuePosition = 1
      const nextTime = Math.max(maxOverlappingFinish, dayEnd)
      candidateStart = new Date(nextTime)
    } else {
      break
    }
  }

  const durationDays = Math.max(1, Math.ceil(requiredUnits / dailyBurnRate))
  let current = new Date(candidateStart.getTime())
  let daysAdded = 0
  while (daysAdded < durationDays) {
    current = new Date(current.getTime() + 24 * 60 * 60 * 1000)
    if (current.getUTCDay() !== 0 && current.getUTCDay() !== 6) {
      daysAdded++
    }
  }

  return {
    queuePosition,
    scheduledStartAt: candidateStart.toISOString(),
    scheduledFinishAt: current.toISOString(),
    burnRatePerDay: dailyBurnRate,
    estimatedCalendarDays: durationDays,
  }
}

export class PostgresCapacityRepository implements CapacityRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}

  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable capacity fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_capacity'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Capacity requires a non-owner runtime')

    const expected = await capacityMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_capacity.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Unsupported capacity ledger')

    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_capacity'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [['memberships', 'pools', 'allocations', 'commands', 'audit']],
      )
    ).rows[0]
    if (policies?.count !== 5) throw new Error('Capacity tenant policies must be forced')

    return new PostgresCapacityRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: CapacityPrincipal,
    work: (client: PoolClient, access: CapacityAccess) => Promise<T>,
  ): Promise<T> {
    capacityTenantSchema.parse(tenant)
    capacityPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])

      const member = async (id: string) => {
        const r = (
          await client.query<Row>('SELECT * FROM archava_capacity.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new CapacityError('forbidden')
        const m = capacityMembershipSchema.parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new CapacityError('forbidden')
        return m
      }

      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new CapacityError('forbidden')

      let access: CapacityAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new CapacityError('forbidden')
        access = {
          permissions: actor.permissions.filter((x: CapacityPermission) =>
            human.permissions.includes(x),
          ),
          ownerId: human.principalId,
          scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
        }
      }

      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      if (
        error &&
        typeof error === 'object' &&
        'code' in error &&
        ['23514', '23503', '23505', '22003', '40P01', '55P03'].includes(String(error.code))
      )
        throw new CapacityError('conflict')
      throw error
    } finally {
      client.release()
    }
  }

  async access(tenant: string, principal: CapacityPrincipal): Promise<CapacityAccess> {
    return await this.transaction(tenant, principal, (_c, a) => Promise.resolve(a))
  }

  async listPools(
    tenant: string,
    principal: CapacityPrincipal,
    query: CapacityPageQuery,
  ): Promise<{ items: readonly CapacityPool[]; nextCursor?: string }> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'capacity.read')
      const params: unknown[] = [tenant, query.limit + 1]
      let sql = 'SELECT * FROM archava_capacity.pools WHERE tenant_id=$1'
      let pIdx = 3

      if (access.scope === 'assigned') {
        sql += ` AND owner_id=$${pIdx++}`
        params.push(access.ownerId)
      }
      if (query.state) {
        sql += ` AND state=$${pIdx++}`
        params.push(query.state)
      }
      if (query.search) {
        sql += ` AND (name ILIKE $${pIdx} OR code ILIKE $${pIdx})`
        pIdx++
        params.push(`%${query.search}%`)
      }
      if (query.cursor) {
        sql += ` AND id > $${pIdx++}`
        params.push(query.cursor)
      }
      sql += ' ORDER BY id ASC LIMIT $2'

      const rows = (await client.query<Row>(sql, params)).rows
      const hasMore = rows.length > query.limit
      const sliced = hasMore ? rows.slice(0, query.limit) : rows
      const items = sliced.map(mapPool)
      const nextCursor =
        hasMore && sliced.length > 0 ? (sliced[sliced.length - 1]?.id as string) : undefined

      return { items, nextCursor }
    })
  }

  async detailPool(
    tenant: string,
    principal: CapacityPrincipal,
    poolId: string,
  ): Promise<CapacityPoolDetail> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'capacity.read')
      const poolRow = (
        await client.query<Row>(
          'SELECT * FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
          [tenant, poolId],
        )
      ).rows[0]
      if (!poolRow) throw new CapacityError('not_found')
      const pool = mapPool(poolRow)
      if (access.scope === 'assigned' && pool.ownerId !== access.ownerId)
        throw new CapacityError('not_found')

      const allocRows = (
        await client.query<Row>(
          "SELECT * FROM archava_capacity.allocations WHERE tenant_id=$1 AND pool_id=$2 AND state IN ('scheduled', 'in_progress') ORDER BY scheduled_finish_at ASC",
          [tenant, poolId],
        )
      ).rows
      const activeAllocations = allocRows.map(mapAllocation)
      const currentLoadHours = activeAllocations.reduce((acc, a) => acc + a.remainingUnits, 0)

      return { pool, activeAllocations, currentLoadHours }
    })
  }

  async estimate(
    tenant: string,
    principal: CapacityPrincipal,
    request: CapacityEstimateRequest,
  ): Promise<CapacityEstimate> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'capacity.read')
      const poolRow = (
        await client.query<Row>(
          'SELECT * FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
          [tenant, request.poolId],
        )
      ).rows[0]
      if (!poolRow) throw new CapacityError('not_found')
      const pool = mapPool(poolRow)
      if (access.scope === 'assigned' && pool.ownerId !== access.ownerId)
        throw new CapacityError('not_found')
      if (pool.state !== 'active') throw new CapacityError('pool_archived')

      const activeRows = (
        await client.query<Row>(
          "SELECT * FROM archava_capacity.allocations WHERE tenant_id=$1 AND pool_id=$2 AND state IN ('scheduled', 'in_progress') ORDER BY scheduled_finish_at ASC",
          [tenant, request.poolId],
        )
      ).rows
      const activeAllocations = activeRows.map(mapAllocation)

      const schedule = calculateSchedule(
        pool,
        activeAllocations,
        request.requiredUnits,
        request.desiredStartAt,
      )

      return {
        poolId: pool.id,
        unit: pool.unit,
        requiredUnits: request.requiredUnits,
        queuePosition: schedule.queuePosition,
        estimatedStartAt: schedule.scheduledStartAt,
        estimatedFinishAt: schedule.scheduledFinishAt,
        burnRatePerDay: schedule.burnRatePerDay,
        estimatedCalendarDays: schedule.estimatedCalendarDays,
      }
    })
  }

  async mutate(
    tenant: string,
    principal: CapacityPrincipal,
    mutation: CapacityMutation,
  ): Promise<CommandReceipt> {
    return await this.transaction(tenant, principal, async (client, access) => {
      if (principal.kind !== 'human') throw new CapacityError('forbidden')
      grant(access, requiredCapacityPermission(mutation.type))

      const fingerprint = createHmac('sha256', this.key)
        .update(
          JSON.stringify({
            domain: 'capacity',
            tenant,
            actor: principal.id,
            type: mutation.type,
            mutation,
          }),
        )
        .digest('hex')

      const existingCmd = (
        await client.query<Row>(
          'SELECT * FROM archava_capacity.commands WHERE tenant_id=$1 AND command_id=$2 FOR UPDATE',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.actor_id !== principal.id || existingCmd.fingerprint !== fingerprint)
          throw new CapacityError('conflict')
        if (existingCmd.receipt) {
          const receipt = commandReceiptSchema.parse(existingCmd.receipt)
          if (access.scope === 'assigned' && receipt.poolId) {
            const poolRow = (
              await client.query<Row>(
                'SELECT owner_id FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
                [tenant, receipt.poolId],
              )
            ).rows[0]
            if (poolRow && poolRow.owner_id !== access.ownerId) {
              throw new CapacityError('forbidden')
            }
          }
          return receipt
        }
        throw new CapacityError('conflict')
      }

      await client.query(
        'INSERT INTO archava_capacity.commands(tenant_id, command_id, actor_id, fingerprint) VALUES($1,$2,$3,$4)',
        [tenant, mutation.commandId, principal.id, fingerprint],
      )

      let receipt: CommandReceipt

      if (mutation.type === 'create_pool') {
        const poolId = randomUUID()

        const row = (
          await client.query<Row>(
            `INSERT INTO archava_capacity.pools(
              tenant_id, id, code, name, unit, weekly_capacity, max_concurrent_jobs, state, owner_id, revision
            ) VALUES($1,$2,$3,$4,$5,$6,$7,'active',$8,1)
            RETURNING *`,
            [
              tenant,
              poolId,
              mutation.code,
              mutation.name,
              mutation.unit,
              mutation.weeklyCapacity,
              mutation.maxConcurrentJobs,
              principal.id,
            ],
          )
        ).rows[0]
        if (!row) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(row.created_at),
          poolId,
          revision: 1,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, revision, occurred_at)
           VALUES($1,$2,$3,'create_pool',$4,1,$5)`,
          [tenant, mutation.commandId, principal.id, poolId, row.created_at],
        )
      } else if (mutation.type === 'update_pool') {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.poolId],
          )
        ).rows[0]
        if (!row) throw new CapacityError('not_found')
        if (access.scope === 'assigned' && row.owner_id !== access.ownerId)
          throw new CapacityError('forbidden')
        if (row.state === 'archived') throw new CapacityError('pool_archived')
        if (Number(row.revision) !== mutation.expectedRevision) throw new CapacityError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_capacity.pools
             SET name=$1, weekly_capacity=$2, max_concurrent_jobs=$3, revision=$4, updated_at=clock_timestamp()
             WHERE tenant_id=$5 AND id=$6
             RETURNING *`,
            [
              mutation.name,
              mutation.weeklyCapacity,
              mutation.maxConcurrentJobs,
              nextRev,
              tenant,
              mutation.poolId,
            ],
          )
        ).rows[0]
        if (!updated) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          poolId: mutation.poolId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, revision, occurred_at)
           VALUES($1,$2,$3,'update_pool',$4,$5,$6)`,
          [tenant, mutation.commandId, principal.id, mutation.poolId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'archive_pool') {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.poolId],
          )
        ).rows[0]
        if (!row) throw new CapacityError('not_found')
        if (access.scope === 'assigned' && row.owner_id !== access.ownerId)
          throw new CapacityError('forbidden')
        if (row.state === 'archived') throw new CapacityError('pool_archived')
        if (Number(row.revision) !== mutation.expectedRevision) throw new CapacityError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_capacity.pools
             SET state='archived', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.poolId],
          )
        ).rows[0]
        if (!updated) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          poolId: mutation.poolId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, revision, occurred_at)
           VALUES($1,$2,$3,'archive_pool',$4,$5,$6)`,
          [tenant, mutation.commandId, principal.id, mutation.poolId, nextRev, updated.updated_at],
        )
      } else if (mutation.type === 'commit_allocation') {
        const poolRow = (
          await client.query<Row>(
            'SELECT * FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.poolId],
          )
        ).rows[0]
        if (!poolRow) throw new CapacityError('not_found')
        const pool = mapPool(poolRow)
        if (access.scope === 'assigned' && pool.ownerId !== access.ownerId)
          throw new CapacityError('forbidden')
        if (pool.state !== 'active') throw new CapacityError('pool_archived')

        // Compute schedule based on current active allocations
        const activeRows = (
          await client.query<Row>(
            "SELECT * FROM archava_capacity.allocations WHERE tenant_id=$1 AND pool_id=$2 AND state IN ('scheduled', 'in_progress') ORDER BY scheduled_finish_at ASC FOR UPDATE",
            [tenant, mutation.poolId],
          )
        ).rows
        const activeAllocations = activeRows.map(mapAllocation)

        const schedule = calculateSchedule(
          pool,
          activeAllocations,
          mutation.requiredUnits,
          mutation.desiredStartAt,
        )

        const allocId = randomUUID()
        const allocRow = (
          await client.query<Row>(
            `INSERT INTO archava_capacity.allocations(
              tenant_id, id, pool_id, workload_id, workload_title, required_units, remaining_units,
              scheduled_start_at, scheduled_finish_at, state, owner_id, revision
            ) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'scheduled',$10,1)
            RETURNING *`,
            [
              tenant,
              allocId,
              mutation.poolId,
              mutation.workloadId,
              mutation.workloadTitle,
              mutation.requiredUnits,
              mutation.requiredUnits,
              schedule.scheduledStartAt,
              schedule.scheduledFinishAt,
              principal.id,
            ],
          )
        ).rows[0]
        if (!allocRow) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(allocRow.created_at),
          poolId: mutation.poolId,
          allocationId: allocId,
          revision: 1,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, allocation_id, revision, occurred_at)
           VALUES($1,$2,$3,'commit_allocation',$4,$5,1,$6)`,
          [tenant, mutation.commandId, principal.id, mutation.poolId, allocId, allocRow.created_at],
        )
      } else if (mutation.type === 'update_allocation_progress') {
        const allocRow = (
          await client.query<Row>(
            'SELECT * FROM archava_capacity.allocations WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.allocationId],
          )
        ).rows[0]
        if (!allocRow) throw new CapacityError('not_found')
        if (access.scope === 'assigned') {
          const poolRow = (
            await client.query<Row>(
              'SELECT owner_id FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
              [tenant, allocRow.pool_id],
            )
          ).rows[0]
          if (!poolRow || poolRow.owner_id !== access.ownerId) {
            throw new CapacityError('forbidden')
          }
        }
        if (!['scheduled', 'in_progress'].includes(allocRow.state as string))
          throw new CapacityError('invalid_state')
        if (Number(allocRow.revision) !== mutation.expectedRevision)
          throw new CapacityError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const nextState =
          mutation.state ?? (mutation.remainingUnits === 0 ? 'completed' : 'in_progress')

        const updated = (
          await client.query<Row>(
            `UPDATE archava_capacity.allocations
             SET remaining_units=$1, state=$2, revision=$3, updated_at=clock_timestamp()
             WHERE tenant_id=$4 AND id=$5
             RETURNING *`,
            [mutation.remainingUnits, nextState, nextRev, tenant, mutation.allocationId],
          )
        ).rows[0]
        if (!updated) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          poolId: String(updated.pool_id),
          allocationId: mutation.allocationId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, allocation_id, revision, occurred_at)
           VALUES($1,$2,$3,'update_allocation_progress',$4,$5,$6,$7)`,
          [
            tenant,
            mutation.commandId,
            principal.id,
            updated.pool_id,
            mutation.allocationId,
            nextRev,
            updated.updated_at,
          ],
        )
      } else if (mutation.type === 'complete_allocation') {
        const allocRow = (
          await client.query<Row>(
            'SELECT * FROM archava_capacity.allocations WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.allocationId],
          )
        ).rows[0]
        if (!allocRow) throw new CapacityError('not_found')
        if (access.scope === 'assigned') {
          const poolRow = (
            await client.query<Row>(
              'SELECT owner_id FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
              [tenant, allocRow.pool_id],
            )
          ).rows[0]
          if (!poolRow || poolRow.owner_id !== access.ownerId) {
            throw new CapacityError('forbidden')
          }
        }
        if (allocRow.state === 'completed' || allocRow.state === 'cancelled')
          throw new CapacityError('invalid_state')
        if (Number(allocRow.revision) !== mutation.expectedRevision)
          throw new CapacityError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_capacity.allocations
             SET remaining_units=0, state='completed', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.allocationId],
          )
        ).rows[0]
        if (!updated) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          poolId: String(updated.pool_id),
          allocationId: mutation.allocationId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, allocation_id, revision, occurred_at)
           VALUES($1,$2,$3,'complete_allocation',$4,$5,$6,$7)`,
          [
            tenant,
            mutation.commandId,
            principal.id,
            updated.pool_id,
            mutation.allocationId,
            nextRev,
            updated.updated_at,
          ],
        )
      } else if (mutation.type === 'cancel_allocation') {
        const allocRow = (
          await client.query<Row>(
            'SELECT * FROM archava_capacity.allocations WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, mutation.allocationId],
          )
        ).rows[0]
        if (!allocRow) throw new CapacityError('not_found')
        if (access.scope === 'assigned') {
          const poolRow = (
            await client.query<Row>(
              'SELECT owner_id FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
              [tenant, allocRow.pool_id],
            )
          ).rows[0]
          if (!poolRow || poolRow.owner_id !== access.ownerId) {
            throw new CapacityError('forbidden')
          }
        }
        if (allocRow.state === 'completed' || allocRow.state === 'cancelled')
          throw new CapacityError('invalid_state')
        if (Number(allocRow.revision) !== mutation.expectedRevision)
          throw new CapacityError('conflict')

        const nextRev = mutation.expectedRevision + 1
        const updated = (
          await client.query<Row>(
            `UPDATE archava_capacity.allocations
             SET state='cancelled', revision=$1, updated_at=clock_timestamp()
             WHERE tenant_id=$2 AND id=$3
             RETURNING *`,
            [nextRev, tenant, mutation.allocationId],
          )
        ).rows[0]
        if (!updated) throw new CapacityError('conflict')

        receipt = {
          commandId: mutation.commandId,
          committedAt: instant(updated.updated_at),
          poolId: String(updated.pool_id),
          allocationId: mutation.allocationId,
          revision: nextRev,
          status: 'committed',
        }

        await client.query(
          `INSERT INTO archava_capacity.audit(tenant_id, command_id, actor_id, operation, pool_id, allocation_id, revision, reason, occurred_at)
           VALUES($1,$2,$3,'cancel_allocation',$4,$5,$6,$7,$8)`,
          [
            tenant,
            mutation.commandId,
            principal.id,
            updated.pool_id,
            mutation.allocationId,
            nextRev,
            mutation.reason,
            updated.updated_at,
          ],
        )
      } else {
        throw new CapacityError('invalid_request')
      }

      await client.query(
        'UPDATE archava_capacity.commands SET receipt=$1 WHERE tenant_id=$2 AND command_id=$3',
        [JSON.stringify(receipt), tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async commandStatus(
    tenant: string,
    principal: CapacityPrincipal,
    commandId: string,
  ): Promise<CommandStatusResult> {
    return await this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'capacity.read')
      const row = (
        await client.query<Row>(
          'SELECT receipt, actor_id FROM archava_capacity.commands WHERE tenant_id=$1 AND command_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      const receipt = commandReceiptSchema.parse(row.receipt)
      if (access.scope === 'assigned') {
        if (row.actor_id !== principal.id) {
          throw new CapacityError('forbidden')
        }
        if (receipt.poolId) {
          const poolRow = (
            await client.query<Row>(
              'SELECT owner_id FROM archava_capacity.pools WHERE tenant_id=$1 AND id=$2',
              [tenant, receipt.poolId],
            )
          ).rows[0]
          if (poolRow && poolRow.owner_id !== access.ownerId) {
            throw new CapacityError('forbidden')
          }
        }
      }
      return {
        status: 'committed',
        receipt,
      }
    })
  }
}
