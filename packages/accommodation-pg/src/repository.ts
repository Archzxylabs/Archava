import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  AccommodationError,
  accommodationIdSchema,
  accommodationPrincipalSchema,
  accommodationMembershipSchema,
  propertySchema,
  roomTypeSchema,
  roomSchema,
  staySchema,
  stayNightSchema,
  accommodationAuditSchema,
  propertyPageSchema,
  accommodationPageSchema,
  roomAvailabilityQuerySchema,
  accommodationMutationSchema,
  accommodationReceiptSchema,
  type AccommodationRepository,
  type AccommodationPrincipal,
  type AccommodationPermission,
  type AccommodationAccess,
  type PropertyPageQuery,
  type AccommodationPageQuery,
  type RoomAvailabilityQuery,
  type AccommodationMutation,
  type AccommodationReceipt,
  type AccommodationCommandStatus,
  type Property,
} from '@archava/accommodation'
import {
  calendarNights,
  calendarToday,
  resolveCalendarInstant,
  CalendarTimeError,
} from '@archava/scheduling/calendar'
import {
  assertOccupancyKernel,
  createAccommodationResource,
  lockAccommodationResource,
  accommodationOccupancyAvailable,
  holdAccommodationOccupancy,
  transitionAccommodationOccupancy,
  setAccommodationResourceActive,
} from '@archava/scheduling-pg/occupancy'
import { accommodationMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>
const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid accommodation timestamp')
  return x.toISOString()
}
const nullableInstant = (x: unknown) => (x === null ? null : instant(x))
const property = (row: Row) =>
  propertySchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    ownerId: row.owner_id,
    title: row.title,
    timezone: row.timezone,
    checkInTime: row.check_in_time,
    checkOutTime: row.check_out_time,
    currency: row.currency,
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const roomType = (row: Row | undefined) => {
  if (!row) throw new AccommodationError('not_found')
  return roomTypeSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    propertyId: row.property_id,
    title: row.title,
    maxGuests: row.max_guests,
    nightlyRate: row.nightly_rate,
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
}
const room = (row: Row) =>
  roomSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    propertyId: row.property_id,
    roomTypeId: row.room_type_id,
    code: row.code,
    active: row.active,
    housekeeping: row.housekeeping,
    occupied: row.occupied,
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const stayColumns =
  "s.*,to_char(s.arrival,'YYYY-MM-DD') AS arrival_date,to_char(s.departure,'YYYY-MM-DD') AS departure_date"
const roomColumns =
  "r.*,EXISTS(SELECT 1 FROM archava_accommodation.stays s WHERE s.tenant_id=r.tenant_id AND s.room_id=r.id AND s.state='in_house') AS occupied"
const stay = (row: Row, observed: string) =>
  staySchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    propertyId: row.property_id,
    roomId: row.room_id,
    ownerId: row.owner_id,
    guestName: row.guest_name,
    guests: row.guests,
    arrival: row.arrival_date,
    departure: row.departure_date,
    nights: row.nights,
    nightlyRate: row.nightly_rate,
    rateRevision: row.rate_revision,
    totalEstimate: Number(row.total_estimate),
    currency: row.currency,
    state: row.state,
    effectiveState:
      row.state === 'held' &&
      row.expires_at instanceof Date &&
      row.expires_at.getTime() <= Date.parse(observed)
        ? 'expired'
        : row.state,
    startsAt: instant(row.starts_at),
    endsAt: instant(row.ends_at),
    expiresAt: instant(row.expires_at),
    checkedInAt: nullableInstant(row.checked_in_at),
    checkedOutAt: nullableInstant(row.checked_out_at),
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
const clock = async (client: PoolClient) =>
  instant((await client.query<Row>('SELECT clock_timestamp() AS observed')).rows[0]?.observed)
function canonical(value: unknown): string {
  if (Array.isArray(value)) return '[' + value.map(canonical).join(',') + ']'
  if (value !== null && typeof value === 'object')
    return (
      '{' +
      Object.entries(value)
        .sort(([a], [b]) => a.localeCompare(b))
        .map(([k, v]) => JSON.stringify(k) + ':' + canonical(v))
        .join(',') +
      '}'
    )
  return JSON.stringify(value)
}
const grant = (access: AccommodationAccess, permission: AccommodationPermission) => {
  if (!access.permissions.includes(permission)) throw new AccommodationError('forbidden')
}
function period(p: Property, arrival: string, departure: string) {
  const nights = calendarNights(arrival, departure)
  return {
    nights,
    startsAt: resolveCalendarInstant(arrival, p.checkInTime, p.timezone),
    endsAt: resolveCalendarInstant(departure, p.checkOutTime, p.timezone),
  }
}

/** Own domain records, composed in one transaction with the scheduling occupancy kernel. */
export class PostgresAccommodationRepository implements AccommodationRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly fingerprintKey: string,
  ) {}
  static async create(pool: Pool, fingerprintKey: string) {
    if (fingerprintKey.length < 32)
      throw new Error('Stable accommodation fingerprint key must have at least 32 characters')
    const role = await pool.query<{ unsafe: boolean }>(
      "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_accommodation'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
    )
    if (role.rows[0]?.unsafe !== false)
      throw new Error('Accommodation requires a non-owner runtime')
    const expected = await accommodationMigrationLedger(),
      actual = (
        await pool.query<{ id: string; checksum: string }>(
          'SELECT id,checksum FROM archava_accommodation.schema_migrations ORDER BY id',
        )
      ).rows
    if (
      actual.length !== expected.length ||
      actual.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
    )
      throw new Error('Unsupported accommodation migration checksum')
    const policies = await pool.query<{ count: number }>(
      "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_accommodation'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
      [
        [
          'memberships',
          'properties',
          'room_types',
          'rooms',
          'stays',
          'stay_nights',
          'commands',
          'audit',
        ],
      ],
    )
    if (policies.rows[0]?.count !== 8)
      throw new Error('Accommodation tenant policies must be forced')
    await assertOccupancyKernel(pool)
    return new PostgresAccommodationRepository(pool, fingerprintKey)
  }
  private async transaction<T>(
    tenant: string,
    principal: AccommodationPrincipal,
    permission: AccommodationPermission | null,
    work: (client: PoolClient, access: AccommodationAccess) => Promise<T>,
  ): Promise<T> {
    accommodationIdSchema.parse(tenant)
    accommodationPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string) => {
        const row = (
          await client.query<Row>('SELECT * FROM archava_accommodation.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!row) throw new AccommodationError('forbidden')
        const parsed = accommodationMembershipSchema.parse({
          principalId: row.principal_id,
          kind: row.kind,
          permissions: row.permissions,
          scope: row.scope,
          active: row.active,
        })
        if (!parsed.active) throw new AccommodationError('forbidden')
        return parsed
      }
      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new AccommodationError('forbidden')
      let access: AccommodationAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new AccommodationError('forbidden')
        access = {
          permissions: actor.permissions.filter((x) => human.permissions.includes(x)),
          ownerId: human.principalId,
          scope: actor.scope === 'assigned' || human.scope === 'assigned' ? 'assigned' : 'all',
        }
      }
      if (permission) grant(access, permission)
      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      if (
        error instanceof CalendarTimeError ||
        (error &&
          typeof error === 'object' &&
          'code' in error &&
          ['23P01', '23514', '23503', '23505', '22003'].includes(String(error.code)))
      )
        throw new AccommodationError('conflict')
      throw error
    } finally {
      client.release()
    }
  }
  private async ownedProperty(
    client: PoolClient,
    tenant: string,
    access: AccommodationAccess,
    id: string,
  ) {
    grant(access, 'properties.read')
    const row = (
      await client.query<Row>(
        'SELECT * FROM archava_accommodation.lock_property($1,$2) WHERE ($3::boolean OR owner_id=$4)',
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new AccommodationError('not_found')
    return property(row)
  }
  private async roomRecord(client: PoolClient, tenant: string, propertyId: string, id: string) {
    const row = (
      await client.query<Row>(
        `SELECT ${roomColumns} FROM archava_accommodation.rooms r WHERE r.tenant_id=$1 AND r.property_id=$2 AND r.id=$3`,
        [tenant, propertyId, id],
      )
    ).rows[0]
    if (!row) throw new AccommodationError('not_found')
    return room(row)
  }
  private async stayRecord(
    client: PoolClient,
    tenant: string,
    access: AccommodationAccess,
    id: string,
  ) {
    const row = (
      await client.query<Row>(
        `SELECT ${stayColumns} FROM archava_accommodation.stays s WHERE s.tenant_id=$1 AND s.id=$2 AND ($3::boolean OR s.owner_id=$4)`,
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new AccommodationError('not_found')
    return row
  }
  access(tenant: string, principal: AccommodationPrincipal) {
    return this.transaction(tenant, principal, null, (_client, access) => Promise.resolve(access))
  }
  properties(tenant: string, principal: AccommodationPrincipal, raw: PropertyPageQuery) {
    const q = propertyPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'properties.read', async (client, access) => {
      const observed = await clock(client),
        rows = (
          await client.query<Row>(
            'SELECT * FROM archava_accommodation.properties WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) AND ($4::uuid IS NULL OR id>$4) ORDER BY id LIMIT $5',
            [tenant, access.scope === 'all', access.ownerId, q.after ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map(property)
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  property(tenant: string, principal: AccommodationPrincipal, rawId: string) {
    const id = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'properties.read', (client, access) =>
      this.ownedProperty(client, tenant, access, id),
    )
  }
  roomTypes(tenant: string, principal: AccommodationPrincipal, raw: AccommodationPageQuery) {
    const q = accommodationPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'rooms.read', async (client, access) => {
      await this.ownedProperty(client, tenant, access, q.propertyId)
      const observed = await clock(client),
        rows = (
          await client.query<Row>(
            'SELECT * FROM archava_accommodation.room_types WHERE tenant_id=$1 AND property_id=$2 AND ($3::uuid IS NULL OR id>$3) ORDER BY id LIMIT $4',
            [tenant, q.propertyId, q.after ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map(roomType)
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  rooms(tenant: string, principal: AccommodationPrincipal, raw: AccommodationPageQuery) {
    const q = accommodationPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'rooms.read', async (client, access) => {
      await this.ownedProperty(client, tenant, access, q.propertyId)
      const observed = await clock(client),
        rows = (
          await client.query<Row>(
            `SELECT ${roomColumns} FROM archava_accommodation.rooms r WHERE r.tenant_id=$1 AND r.property_id=$2 AND ($3::uuid IS NULL OR r.id>$3) ORDER BY r.id LIMIT $4`,
            [tenant, q.propertyId, q.after ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map(room)
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  stays(tenant: string, principal: AccommodationPrincipal, raw: AccommodationPageQuery) {
    const q = accommodationPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'stays.read', async (client, access) => {
      await this.ownedProperty(client, tenant, access, q.propertyId)
      const observed = await clock(client),
        rows = (
          await client.query<Row>(
            `SELECT ${stayColumns} FROM archava_accommodation.stays s WHERE s.tenant_id=$1 AND s.property_id=$2 AND ($3::boolean OR s.owner_id=$4) AND ($5::uuid IS NULL OR s.id>$5) ORDER BY s.id LIMIT $6`,
            [
              tenant,
              q.propertyId,
              access.scope === 'all',
              access.ownerId,
              q.after ?? null,
              q.limit + 1,
            ],
          )
        ).rows,
        items = rows.slice(0, q.limit).map((row) => stay(row, observed))
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  room(tenant: string, principal: AccommodationPrincipal, rawId: string) {
    const id = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'rooms.read', async (client, access) => {
      const located = (
        await client.query<Row>(
          'SELECT property_id FROM archava_accommodation.rooms WHERE tenant_id=$1 AND id=$2',
          [tenant, id],
        )
      ).rows[0]
      if (!located) throw new AccommodationError('not_found')
      const p = await this.ownedProperty(client, tenant, access, String(located.property_id))
      await lockAccommodationResource(client, tenant, id)
      return await this.roomRecord(client, tenant, p.id, id)
    })
  }
  detail(tenant: string, principal: AccommodationPrincipal, rawId: string) {
    const id = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'stays.read', async (client, access) => {
      grant(access, 'rooms.read')
      const located = await this.stayRecord(client, tenant, access, id),
        p = await this.ownedProperty(client, tenant, access, String(located.property_id))
      await lockAccommodationResource(client, tenant, String(located.room_id))
      const observed = await clock(client),
        record = stay(await this.stayRecord(client, tenant, access, id), observed),
        r = await this.roomRecord(client, tenant, p.id, record.roomId)
      const nights = (
        await client.query<Row>(
          "SELECT to_char(night_date,'YYYY-MM-DD') AS night_date,rate,rate_revision FROM archava_accommodation.stay_nights WHERE tenant_id=$1 AND stay_id=$2 ORDER BY night_date LIMIT 32",
          [tenant, id],
        )
      ).rows.map((row) =>
        stayNightSchema.parse({
          date: row.night_date,
          rate: row.rate,
          rateRevision: row.rate_revision,
        }),
      )
      if (nights.length !== record.nights) throw new Error('Invalid stay night ledger')
      const rows = access.permissions.includes('audit.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_accommodation.audit WHERE tenant_id=$1 AND record_id=$2 ORDER BY occurred_at DESC,request_id DESC LIMIT 51',
              [tenant, id],
            )
          ).rows
        : null
      const audit =
        rows?.slice(0, 50).map((row) =>
          accommodationAuditSchema.parse({
            requestId: row.request_id,
            actorId: row.actor_id,
            operation: row.operation,
            propertyId: row.property_id,
            recordId: row.record_id,
            revision: row.revision,
            reason: row.reason,
            occurredAt: instant(row.occurred_at),
          }),
        ) ?? null
      return {
        stay: record,
        property: p,
        room: r,
        nights,
        audit,
        auditHasMore: (rows?.length ?? 0) > 50,
        evaluatedAt: observed,
      }
    })
  }
  availability(tenant: string, principal: AccommodationPrincipal, raw: RoomAvailabilityQuery) {
    const q = roomAvailabilityQuerySchema.parse(raw)
    return this.transaction(tenant, principal, 'rooms.read', async (client, access) => {
      const p = await this.ownedProperty(client, tenant, access, q.propertyId)
      await this.roomRecord(client, tenant, p.id, q.roomId)
      const shared = await lockAccommodationResource(client, tenant, q.roomId),
        observed = await clock(client),
        r = await this.roomRecord(client, tenant, p.id, q.roomId)
      const type = roomType(
          (
            await client.query<Row>(
              'SELECT * FROM archava_accommodation.room_types WHERE tenant_id=$1 AND id=$2 FOR SHARE',
              [tenant, r.roomTypeId],
            )
          ).rows[0],
        ),
        dates = period(p, q.arrival, q.departure),
        today = calendarToday(observed, p.timezone)
      const available =
        r.active &&
        shared.active &&
        q.guests <= type.maxGuests &&
        q.arrival >= today &&
        Date.parse(dates.endsAt) > Date.parse(observed) &&
        !(q.arrival === today && r.occupied) &&
        (await accommodationOccupancyAvailable(
          client,
          tenant,
          r.id,
          dates.startsAt,
          dates.endsAt,
          observed,
        ))
      return {
        propertyId: p.id,
        roomId: r.id,
        arrival: q.arrival,
        departure: q.departure,
        available,
        nights: dates.nights.length,
        nightlyRate: type.nightlyRate,
        totalEstimate: type.nightlyRate * dates.nights.length,
        currency: 'IDR' as const,
        housekeeping: r.housekeeping,
        occupied: r.occupied,
        evaluatedAt: observed,
        authority: 'snapshot-only' as const,
      }
    })
  }
  private async receiptAccess(
    client: PoolClient,
    tenant: string,
    access: AccommodationAccess,
    receipt: AccommodationReceipt,
  ) {
    await this.ownedProperty(client, tenant, access, receipt.propertyId)
    if (receipt.kind === 'room' || receipt.kind === 'room-type') grant(access, 'rooms.read')
    if (receipt.kind === 'stay') {
      grant(access, 'stays.read')
      grant(access, 'rooms.read')
      await this.stayRecord(client, tenant, access, receipt.id)
    }
  }
  commandStatus(
    tenant: string,
    principal: AccommodationPrincipal,
    rawId: string,
  ): Promise<AccommodationCommandStatus> {
    if (principal.kind !== 'human') throw new AccommodationError('forbidden')
    const requestId = z.uuid().parse(rawId)
    return this.transaction(tenant, principal, 'properties.read', async (client, access) => {
      const row = (
        await client.query<Row>(
          'SELECT * FROM archava_accommodation.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, requestId],
        )
      ).rows[0]
      if (!row) return { status: 'not_committed', requestId }
      if (row.actor_id !== principal.id) throw new AccommodationError('forbidden')
      const receipt = accommodationReceiptSchema.parse(row.receipt)
      await this.receiptAccess(client, tenant, access, receipt)
      return { status: 'committed', requestId, receipt }
    })
  }
  mutate(
    tenant: string,
    principal: AccommodationPrincipal,
    raw: AccommodationMutation,
  ): Promise<AccommodationReceipt> {
    if (principal.kind !== 'human') throw new AccommodationError('forbidden')
    const mutation = accommodationMutationSchema.parse(raw),
      c = mutation.command
    const permission: AccommodationPermission =
      c.operation === 'property.create'
        ? 'properties.write'
        : c.operation === 'room.housekeeping'
          ? c.state === 'inspected'
            ? 'rooms.inspect'
            : 'housekeeping.write'
          : c.operation.startsWith('room')
            ? 'rooms.write'
            : 'stays.write'
    const fingerprint = createHmac('sha256', this.fingerprintKey)
      .update(canonical({ tenant, principal, command: c }))
      .digest('hex')
    return this.transaction(tenant, principal, permission, async (client, access) => {
      grant(access, 'properties.read')
      if (c.operation !== 'property.create') grant(access, 'rooms.read')
      if (c.operation.startsWith('stay.')) grant(access, 'stays.read')
      const claimed = await client.query(
        'INSERT INTO archava_accommodation.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id',
        [tenant, mutation.requestId, principal.id, fingerprint],
      )
      if (!claimed.rows.length) {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_accommodation.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, mutation.requestId],
          )
        ).rows[0]
        if (!row || row.actor_id !== principal.id || row.fingerprint !== fingerprint)
          throw new AccommodationError('conflict')
        const receipt = accommodationReceiptSchema.parse(row.receipt)
        await this.receiptAccess(client, tenant, access, receipt)
        return receipt
      }
      let receipt: AccommodationReceipt
      if (c.operation === 'property.create') {
        const id = randomUUID()
        await client.query(
          'INSERT INTO archava_accommodation.properties(tenant_id,id,owner_id,title,timezone,check_in_time,check_out_time) VALUES($1,$2,$3,$4,$5,$6,$7)',
          [tenant, id, access.ownerId, c.title, c.timezone, c.checkInTime, c.checkOutTime],
        )
        receipt = { kind: 'property', id, propertyId: id, revision: 1 }
      } else {
        const p = await this.ownedProperty(client, tenant, access, c.propertyId)
        if (c.operation === 'room-type.create') {
          const id = randomUUID()
          await client.query(
            'INSERT INTO archava_accommodation.room_types(tenant_id,id,property_id,title,max_guests,nightly_rate) VALUES($1,$2,$3,$4,$5,$6)',
            [tenant, id, p.id, c.title, c.maxGuests, c.nightlyRate],
          )
          receipt = { kind: 'room-type', id, propertyId: p.id, revision: 1 }
        } else if (c.operation === 'room-type.set-rate') {
          const row = (
            await client.query<Row>(
              'SELECT * FROM archava_accommodation.room_types WHERE tenant_id=$1 AND property_id=$2 AND id=$3 FOR UPDATE',
              [tenant, p.id, c.id],
            )
          ).rows[0]
          if (!row) throw new AccommodationError('not_found')
          const item = roomType(row)
          if (
            item.revision !== c.expectedRevision ||
            item.revision >= 2_147_483_647 ||
            item.nightlyRate === c.nightlyRate
          )
            throw new AccommodationError('conflict')
          await client.query(
            'UPDATE archava_accommodation.room_types SET nightly_rate=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
            [tenant, item.id, c.nightlyRate],
          )
          receipt = {
            kind: 'room-type',
            id: item.id,
            propertyId: p.id,
            revision: item.revision + 1,
          }
        } else if (c.operation === 'room.create') {
          const type = (
            await client.query<Row>(
              'SELECT * FROM archava_accommodation.room_types WHERE tenant_id=$1 AND property_id=$2 AND id=$3 FOR SHARE',
              [tenant, p.id, c.roomTypeId],
            )
          ).rows[0]
          if (!type) throw new AccommodationError('not_found')
          const id = randomUUID()
          await createAccommodationResource(client, {
            tenant,
            id,
            title: c.code,
            timezone: p.timezone,
          })
          await client.query(
            'INSERT INTO archava_accommodation.rooms(tenant_id,id,property_id,room_type_id,code) VALUES($1,$2,$3,$4,$5)',
            [tenant, id, p.id, c.roomTypeId, c.code],
          )
          receipt = { kind: 'room', id, propertyId: p.id, revision: 1 }
        } else if (
          c.operation === 'room.set-active' ||
          c.operation === 'room.housekeeping' ||
          c.operation === 'stay.hold'
        ) {
          await this.roomRecord(client, tenant, p.id, c.roomId)
          const shared = await lockAccommodationResource(client, tenant, c.roomId),
            observed = await clock(client),
            r = await this.roomRecord(client, tenant, p.id, c.roomId)
          if (c.operation === 'room.set-active' || c.operation === 'room.housekeeping') {
            if (r.revision !== c.expectedRevision || r.revision >= 2_147_483_647)
              throw new AccommodationError('conflict')
            if (c.operation === 'room.set-active') {
              if (r.active === c.active || r.occupied) throw new AccommodationError('conflict')
              await setAccommodationResourceActive(client, tenant, r.id, c.active)
              await client.query(
                'UPDATE archava_accommodation.rooms SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
                [tenant, r.id, c.active],
              )
            } else {
              const allowed = {
                dirty: ['clean'],
                clean: ['dirty', 'inspected'],
                inspected: ['dirty'],
              }
              if (r.occupied || !allowed[r.housekeeping].includes(c.state))
                throw new AccommodationError('conflict')
              await client.query(
                'UPDATE archava_accommodation.rooms SET housekeeping=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
                [tenant, r.id, c.state],
              )
            }
            receipt = { kind: 'room', id: r.id, propertyId: p.id, revision: r.revision + 1 }
          } else {
            const dates = period(p, c.arrival, c.departure),
              today = calendarToday(observed, p.timezone)
            const type = roomType(
              (
                await client.query<Row>(
                  'SELECT * FROM archava_accommodation.room_types WHERE tenant_id=$1 AND id=$2 FOR SHARE',
                  [tenant, r.roomTypeId],
                )
              ).rows[0],
            )
            if (
              !r.active ||
              !shared.active ||
              c.guests > type.maxGuests ||
              c.arrival < today ||
              Date.parse(dates.endsAt) <= Date.parse(observed) ||
              (c.arrival === today && r.occupied)
            )
              throw new AccommodationError('conflict')
            const id = randomUUID(),
              expiresAt = new Date(Date.parse(observed) + c.holdSeconds * 1000).toISOString()
            await holdAccommodationOccupancy(client, {
              tenant,
              id,
              resourceId: r.id,
              startsAt: dates.startsAt,
              endsAt: dates.endsAt,
              expiresAt,
            })
            await client.query(
              'INSERT INTO archava_accommodation.stays(tenant_id,id,property_id,room_id,owner_id,guest_name,guests,arrival,departure,nights,nightly_rate,rate_revision,total_estimate,starts_at,ends_at,expires_at) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)',
              [
                tenant,
                id,
                p.id,
                r.id,
                access.ownerId,
                c.guestName,
                c.guests,
                c.arrival,
                c.departure,
                dates.nights.length,
                type.nightlyRate,
                type.revision,
                type.nightlyRate * dates.nights.length,
                dates.startsAt,
                dates.endsAt,
                expiresAt,
              ],
            )
            for (const date of dates.nights)
              await client.query(
                'INSERT INTO archava_accommodation.stay_nights(tenant_id,stay_id,night_date,rate,rate_revision) VALUES($1,$2,$3,$4,$5)',
                [tenant, id, date, type.nightlyRate, type.revision],
              )
            receipt = { kind: 'stay', id, propertyId: p.id, revision: 1 }
          }
        } else {
          const located = await this.stayRecord(client, tenant, access, c.id)
          if (located.property_id !== p.id) throw new AccommodationError('not_found')
          await lockAccommodationResource(client, tenant, String(located.room_id))
          const observed = await clock(client),
            item = stay(await this.stayRecord(client, tenant, access, c.id), observed)
          if (
            item.revision !== c.expectedRevision ||
            item.revision >= 2_147_483_647 ||
            ['expired', 'checked_out', 'cancelled'].includes(item.effectiveState)
          )
            throw new AccommodationError('conflict')
          if (c.operation === 'stay.confirm') {
            if (item.effectiveState !== 'held' || Date.parse(item.endsAt) <= Date.parse(observed))
              throw new AccommodationError('conflict')
            await transitionAccommodationOccupancy(
              client,
              tenant,
              item.roomId,
              item.id,
              'confirmed',
            )
            await client.query(
              "UPDATE archava_accommodation.stays SET state='confirmed',revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
              [tenant, item.id],
            )
          } else if (c.operation === 'stay.cancel') {
            if (!['held', 'confirmed'].includes(item.state))
              throw new AccommodationError('conflict')
            await transitionAccommodationOccupancy(
              client,
              tenant,
              item.roomId,
              item.id,
              'cancelled',
            )
            await client.query(
              "UPDATE archava_accommodation.stays SET state='cancelled',revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
              [tenant, item.id],
            )
          } else if (c.operation === 'stay.check-in') {
            const r = await this.roomRecord(client, tenant, p.id, item.roomId)
            if (
              item.state !== 'confirmed' ||
              !r.active ||
              r.occupied ||
              r.housekeeping !== 'inspected' ||
              Date.parse(item.startsAt) > Date.parse(observed) ||
              Date.parse(item.endsAt) <= Date.parse(observed)
            )
              throw new AccommodationError('conflict')
            await client.query(
              "UPDATE archava_accommodation.stays SET state='in_house',checked_in_at=clock_timestamp(),revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
              [tenant, item.id],
            )
          } else {
            if (item.state !== 'in_house') throw new AccommodationError('conflict')
            await transitionAccommodationOccupancy(
              client,
              tenant,
              item.roomId,
              item.id,
              'cancelled',
            )
            await client.query(
              "UPDATE archava_accommodation.stays SET state='checked_out',checked_out_at=clock_timestamp(),revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
              [tenant, item.id],
            )
          }
          receipt = { kind: 'stay', id: item.id, propertyId: p.id, revision: item.revision + 1 }
        }
      }
      await client.query(
        'INSERT INTO archava_accommodation.audit(tenant_id,request_id,actor_id,operation,property_id,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7,$8)',
        [
          tenant,
          mutation.requestId,
          principal.id,
          c.operation,
          receipt.propertyId,
          receipt.id,
          receipt.revision,
          'reason' in c ? c.reason : null,
        ],
      )
      await client.query(
        'UPDATE archava_accommodation.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, mutation.requestId, receipt],
      )
      return receipt
    })
  }
}
