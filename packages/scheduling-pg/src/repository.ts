import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  SchedulingError,
  schedulingIdSchema,
  schedulingPrincipalSchema,
  schedulingMembershipSchema,
  schedulingResourceSchema,
  reservationSchema,
  schedulingMutationSchema,
  schedulingReceiptSchema,
  resourcePageSchema,
  reservationPageSchema,
  availabilityQuerySchema,
  schedulingAuditSchema,
  type SchedulingRepository,
  type SchedulingPrincipal,
  type SchedulingAccess,
  type SchedulingPermission,
  type ResourcePageQuery,
  type ReservationPageQuery,
  type AvailabilityQuery,
  type SchedulingMutation,
  type SchedulingReceipt,
  type SchedulingCommandStatus,
} from '@archava/scheduling'
import { schedulingMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>
const instant = (value: unknown) => {
  if (!(value instanceof Date) || !Number.isFinite(value.getTime()))
    throw new Error('Invalid scheduling timestamp')
  return value.toISOString()
}
const resource = (row: Row) =>
  schedulingResourceSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    ownerId: row.owner_id,
    title: row.title,
    kind: row.kind,
    timezone: row.timezone,
    active: row.active,
    capacity: row.capacity,
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const reservation = (row: Row, observed: string) =>
  reservationSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    resourceId: row.resource_id,
    ownerId: row.owner_id,
    title: row.title,
    kind: row.kind,
    state: row.state,
    effectiveState:
      row.state === 'held' &&
      row.expires_at instanceof Date &&
      row.expires_at.getTime() <= Date.parse(observed)
        ? 'expired'
        : row.state,
    startsAt: instant(row.starts_at),
    endsAt: instant(row.ends_at),
    expiresAt: row.expires_at === null ? null : instant(row.expires_at),
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
function canonical(value: unknown): string {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']'
  if (typeof value === 'object' && value !== null)
    return (
      '{' +
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([key, item]) => JSON.stringify(key) + ':' + canonical(item))
        .join(',') +
      '}'
    )
  return JSON.stringify(value)
}
const requireGrant = (access: SchedulingAccess, permission: SchedulingPermission) => {
  if (!access.permissions.includes(permission)) throw new SchedulingError('forbidden')
}
const clock = async (client: PoolClient) =>
  instant((await client.query<Row>('SELECT clock_timestamp() AS observed')).rows[0]?.observed)

/** No CRM/LLM dependency. All mutations serialize on the resource; the SQL trigger also guards overlaps. */
export class PostgresSchedulingRepository implements SchedulingRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly fingerprintKey: string,
  ) {}
  static async create(pool: Pool, fingerprintKey: string) {
    if (fingerprintKey.length < 32)
      throw new Error('Stable scheduling fingerprint key must have at least 32 characters')
    const role = await pool.query<{ unsafe: boolean }>(
      "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_scheduling'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
    )
    if (role.rows[0]?.unsafe !== false)
      throw new Error('Scheduling requires a non-owner runtime without superuser/BYPASSRLS')
    const expected = await schedulingMigrationLedger()
    const existing = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_scheduling.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      existing.length !== expected.length ||
      existing.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
    )
      throw new Error('Unsupported scheduling schema version/checksum')
    const policies = await pool.query<{ count: number }>(
      "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_scheduling'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
      [['memberships', 'resources', 'reservations', 'commands', 'audit']],
    )
    if (policies.rows[0]?.count !== 5)
      throw new Error('Scheduling tenant row policies must be enabled/forced')
    return new PostgresSchedulingRepository(pool, fingerprintKey)
  }
  private async transaction<T>(
    tenant: string,
    principal: SchedulingPrincipal,
    permission: SchedulingPermission | null,
    work: (client: PoolClient, access: SchedulingAccess) => Promise<T>,
  ): Promise<T> {
    schedulingIdSchema.parse(tenant)
    schedulingPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string) => {
        const row = (
          await client.query<Row>('SELECT * FROM archava_scheduling.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!row) throw new SchedulingError('forbidden')
        const parsed = schedulingMembershipSchema.parse({
          principalId: row.principal_id,
          kind: row.kind,
          permissions: row.permissions,
          scope: row.scope,
          active: row.active,
        })
        if (!parsed.active) throw new SchedulingError('forbidden')
        return parsed
      }
      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new SchedulingError('forbidden')
      let access: SchedulingAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new SchedulingError('forbidden')
        access = {
          permissions: actor.permissions.filter((x) => human.permissions.includes(x)),
          ownerId: human.principalId,
          scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
        }
      }
      if (permission) requireGrant(access, permission)
      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      if (
        error &&
        typeof error === 'object' &&
        'code' in error &&
        ['23P01', '23514', '23503', '22003'].includes(String(error.code))
      )
        throw new SchedulingError('conflict')
      throw error
    } finally {
      client.release()
    }
  }
  private async ownedResource(
    client: PoolClient,
    tenant: string,
    access: SchedulingAccess,
    id: string,
    lock: 'share' | 'update' = 'share',
  ) {
    const row = (
      await client.query<Row>(
        `SELECT * FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) FOR ${lock === 'update' ? 'UPDATE' : 'SHARE'}`,
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new SchedulingError('not_found')
    return resource(row)
  }
  private async ownedReservation(
    client: PoolClient,
    tenant: string,
    access: SchedulingAccess,
    id: string,
  ) {
    const row = (
      await client.query<Row>(
        'SELECT * FROM archava_scheduling.reservations WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4)',
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new SchedulingError('not_found')
    return row
  }
  access(tenant: string, principal: SchedulingPrincipal) {
    return this.transaction(tenant, principal, null, (_client, access) => Promise.resolve(access))
  }
  resources(tenant: string, principal: SchedulingPrincipal, raw: ResourcePageQuery) {
    const query = resourcePageSchema.parse(raw)
    return this.transaction(tenant, principal, 'resources.read', async (client, access) => {
      const observed = await clock(client)
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_scheduling.resources WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) AND ($4::uuid IS NULL OR id>$4) AND ($5::boolean IS NULL OR active=$5) ORDER BY id LIMIT $6',
          [
            tenant,
            access.scope === 'all',
            access.ownerId,
            query.after ?? null,
            query.active ?? null,
            query.limit + 1,
          ],
        )
      ).rows
      const items = rows.slice(0, query.limit).map(resource)
      return {
        items,
        next: rows.length > query.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  resource(tenant: string, principal: SchedulingPrincipal, rawId: string) {
    const id = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'resources.read', (client, access) =>
      this.ownedResource(client, tenant, access, id),
    )
  }
  reservations(tenant: string, principal: SchedulingPrincipal, raw: ReservationPageQuery) {
    const query = reservationPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'reservations.read', async (client, access) => {
      requireGrant(access, 'resources.read')
      await this.ownedResource(client, tenant, access, query.resourceId)
      const observed = await clock(client)
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_scheduling.reservations WHERE tenant_id=$1 AND resource_id=$2 AND ($3::boolean OR owner_id=$4) AND ($5::uuid IS NULL OR id>$5) ORDER BY id LIMIT $6',
          [
            tenant,
            query.resourceId,
            access.scope === 'all',
            access.ownerId,
            query.after ?? null,
            query.limit + 1,
          ],
        )
      ).rows
      const items = rows.slice(0, query.limit).map((row) => reservation(row, observed))
      return {
        items,
        next: rows.length > query.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  detail(tenant: string, principal: SchedulingPrincipal, rawId: string) {
    const id = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'reservations.read', async (client, access) => {
      requireGrant(access, 'resources.read')
      // Locate first, then lock resource and re-read: all writers use the same resource-first order.
      const located = await this.ownedReservation(client, tenant, access, id)
      await this.ownedResource(client, tenant, access, String(located.resource_id))
      const row = await this.ownedReservation(client, tenant, access, id)
      const observed = await clock(client)
      const rows = access.permissions.includes('audit.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_scheduling.audit WHERE tenant_id=$1 AND record_id=$2 ORDER BY occurred_at DESC,request_id DESC LIMIT 51',
              [tenant, id],
            )
          ).rows
        : null
      const audit =
        rows?.slice(0, 50).map((item) =>
          schedulingAuditSchema.parse({
            requestId: item.request_id,
            actorId: item.actor_id,
            operation: item.operation,
            resourceId: item.resource_id,
            recordId: item.record_id,
            revision: item.revision,
            reason: item.reason,
            occurredAt: instant(item.occurred_at),
          }),
        ) ?? null
      return {
        reservation: reservation(row, observed),
        evaluatedAt: observed,
        audit,
        auditHasMore: (rows?.length ?? 0) > 50,
      }
    })
  }
  availability(tenant: string, principal: SchedulingPrincipal, raw: AvailabilityQuery) {
    const query = availabilityQuerySchema.parse(raw)
    return this.transaction(tenant, principal, 'resources.read', async (client, access) => {
      const record = await this.ownedResource(client, tenant, access, query.resourceId)
      const observed = await clock(client)
      // Count all conflicts. Never reveal another reservation's identity, title or owner.
      const clash = (
        await client.query<{ conflict: boolean }>(
          "SELECT EXISTS(SELECT 1 FROM archava_scheduling.reservations WHERE tenant_id=$1 AND resource_id=$2 AND (state='confirmed' OR (state='held' AND expires_at>$5)) AND starts_at<$4 AND ends_at>$3) AS conflict",
          [tenant, query.resourceId, query.startsAt, query.endsAt, observed],
        )
      ).rows[0]?.conflict
      return {
        ...query,
        available:
          record.active && Date.parse(query.startsAt) > Date.parse(observed) && clash === false,
        evaluatedAt: observed,
        authority: 'snapshot-only' as const,
      }
    })
  }
  private async receiptAccess(
    client: PoolClient,
    tenant: string,
    access: SchedulingAccess,
    receipt: SchedulingReceipt,
  ) {
    requireGrant(access, 'resources.read')
    await this.ownedResource(client, tenant, access, receipt.resourceId)
    if (receipt.kind === 'reservation') {
      requireGrant(access, 'reservations.read')
      await this.ownedReservation(client, tenant, access, receipt.id)
    }
  }
  commandStatus(
    tenant: string,
    principal: SchedulingPrincipal,
    rawId: string,
  ): Promise<SchedulingCommandStatus> {
    if (principal.kind !== 'human') throw new SchedulingError('forbidden')
    const requestId = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'resources.read', async (client, access) => {
      const row = (
        await client.query<Row>(
          'SELECT * FROM archava_scheduling.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, requestId],
        )
      ).rows[0]
      if (!row) return { status: 'not_committed', requestId }
      if (row.actor_id !== principal.id) throw new SchedulingError('forbidden')
      const receipt = schedulingReceiptSchema.parse(row.receipt)
      await this.receiptAccess(client, tenant, access, receipt)
      return { status: 'committed', requestId, receipt }
    })
  }
  mutate(
    tenant: string,
    principal: SchedulingPrincipal,
    raw: SchedulingMutation,
  ): Promise<SchedulingReceipt> {
    if (principal.kind !== 'human') throw new SchedulingError('forbidden')
    const mutation = schedulingMutationSchema.parse(raw),
      command = mutation.command
    const permission = command.operation.startsWith('resource.')
      ? 'resources.write'
      : 'reservations.write'
    const fingerprint = createHmac('sha256', this.fingerprintKey)
      .update(canonical({ tenant, principal, command }))
      .digest('hex')
    return this.transaction(tenant, principal, permission, async (client, access) => {
      requireGrant(access, 'resources.read')
      if (permission === 'reservations.write') requireGrant(access, 'reservations.read')
      if (command.operation === 'reservation.block') requireGrant(access, 'resources.write')
      const claimed = await client.query(
        'INSERT INTO archava_scheduling.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id',
        [tenant, mutation.requestId, principal.id, fingerprint],
      )
      if (!claimed.rows.length) {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_scheduling.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, mutation.requestId],
          )
        ).rows[0]
        if (!row || row.actor_id !== principal.id || row.fingerprint !== fingerprint)
          throw new SchedulingError('conflict')
        const receipt = schedulingReceiptSchema.parse(row.receipt)
        await this.receiptAccess(client, tenant, access, receipt)
        return receipt
      }
      let receipt: SchedulingReceipt
      if (command.operation === 'resource.create') {
        const id = randomUUID()
        await client.query(
          'INSERT INTO archava_scheduling.resources(tenant_id,id,owner_id,title,kind,timezone) VALUES($1,$2,$3,$4,$5,$6)',
          [tenant, id, access.ownerId, command.title, command.kind, command.timezone],
        )
        receipt = { kind: 'resource', id, resourceId: id, revision: 1 }
      } else {
        const record = await this.ownedResource(
          client,
          tenant,
          access,
          command.resourceId,
          'update',
        )
        // Time is sampled after waiting for the resource lock, including the confirm path.
        const observed = await clock(client)
        if (command.operation === 'resource.set-active') {
          if (
            record.revision !== command.expectedRevision ||
            record.active === command.active ||
            record.revision >= 2_147_483_647
          )
            throw new SchedulingError('conflict')
          await client.query(
            'UPDATE archava_scheduling.resources SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
            [tenant, record.id, command.active],
          )
          receipt = {
            kind: 'resource',
            id: record.id,
            resourceId: record.id,
            revision: record.revision + 1,
          }
        } else if (
          command.operation === 'reservation.hold' ||
          command.operation === 'reservation.block'
        ) {
          if (!record.active || Date.parse(command.startsAt) <= Date.parse(observed))
            throw new SchedulingError('conflict')
          const id = randomUUID(),
            held = command.operation === 'reservation.hold'
          await client.query(
            'INSERT INTO archava_scheduling.reservations(tenant_id,id,resource_id,owner_id,title,kind,state,starts_at,ends_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)',
            [
              tenant,
              id,
              record.id,
              access.ownerId,
              command.title,
              held ? 'booking' : 'block',
              held ? 'held' : 'confirmed',
              command.startsAt,
              command.endsAt,
              held
                ? new Date(Date.parse(observed) + command.holdSeconds * 1000).toISOString()
                : null,
            ],
          )
          receipt = { kind: 'reservation', id, resourceId: record.id, revision: 1 }
        } else {
          const row = await this.ownedReservation(client, tenant, access, command.id)
          const item = reservation(row, observed)
          if (item.resourceId !== record.id) throw new SchedulingError('not_found')
          if (
            item.revision !== command.expectedRevision ||
            item.revision >= 2_147_483_647 ||
            ['cancelled', 'expired'].includes(item.effectiveState)
          )
            throw new SchedulingError('conflict')
          if (item.kind === 'block') requireGrant(access, 'resources.write')
          if (command.operation === 'reservation.confirm') {
            if (
              item.kind !== 'booking' ||
              item.effectiveState !== 'held' ||
              Date.parse(item.startsAt) <= Date.parse(observed)
            )
              throw new SchedulingError('conflict')
            await client.query(
              "UPDATE archava_scheduling.reservations SET state='confirmed',revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
              [tenant, item.id],
            )
          } else if (command.operation === 'reservation.cancel') {
            await client.query(
              "UPDATE archava_scheduling.reservations SET state='cancelled',revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
              [tenant, item.id],
            )
          } else {
            if (
              !record.active ||
              Date.parse(item.startsAt) <= Date.parse(observed) ||
              Date.parse(command.startsAt) <= Date.parse(observed) ||
              (command.startsAt === item.startsAt && command.endsAt === item.endsAt)
            )
              throw new SchedulingError('conflict')
            await client.query(
              'UPDATE archava_scheduling.reservations SET starts_at=$3,ends_at=$4,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
              [tenant, item.id, command.startsAt, command.endsAt],
            )
          }
          receipt = {
            kind: 'reservation',
            id: item.id,
            resourceId: record.id,
            revision: item.revision + 1,
          }
        }
      }
      await client.query(
        'INSERT INTO archava_scheduling.audit(tenant_id,request_id,actor_id,operation,resource_id,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8)',
        [
          tenant,
          mutation.requestId,
          principal.id,
          command.operation,
          receipt.resourceId,
          receipt.id,
          receipt.revision,
          'reason' in command ? command.reason : null,
        ],
      )
      await client.query(
        'UPDATE archava_scheduling.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, mutation.requestId, receipt],
      )
      return receipt
    })
  }
}
