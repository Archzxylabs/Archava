import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  AssetError,
  assetTenantSchema,
  assetPrincipalSchema,
  assetMembershipSchema,
  assetSchema,
  assetCustodyEventSchema,
  assetMaintenanceRecordSchema,
  assetReceiptSchema,
  type AssetRepository,
  type AssetPrincipal,
  type AssetAccess,
  type AssetPermission,
  type AssetMutation,
  type AssetReceipt,
  type AssetCommandStatus,
  type AssetPageQuery,
  type Asset,
  type AssetCustodyEvent,
  type AssetMaintenanceRecord,
  type AssetDetail,
  type AssetStatus,
} from '@archava/assets'
import { assetMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

const instant = (x: unknown) => {
  if (!(x instanceof Date) || !Number.isFinite(x.getTime()))
    throw new Error('Invalid asset timestamp')
  return x.toISOString()
}

const nullable = (x: unknown) => (typeof x === 'string' ? x : null)

const mapAsset = (r: Row): Asset =>
  assetSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    code: r.code,
    name: r.name,
    category: r.category,
    serialNumber: nullable(r.serial_number),
    model: nullable(r.model),
    ownership: r.ownership,
    customerContactId: nullable(r.customer_contact_id),
    status: r.status,
    condition: r.condition,
    currentCustodianId: nullable(r.current_custodian_id),
    currentLocation: nullable(r.current_location),
    ownerId: r.owner_id,
    revision: r.revision,
    createdAt: instant(r.created_at),
    updatedAt: instant(r.updated_at),
  })

const mapCustodyEvent = (r: Row): AssetCustodyEvent =>
  assetCustodyEventSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    assetId: r.asset_id,
    eventType: r.event_type,
    fromCustodianId: nullable(r.from_custodian_id),
    toCustodianId: nullable(r.to_custodian_id),
    conditionBefore: r.condition_before,
    conditionAfter: r.condition_after,
    location: nullable(r.location),
    notes: nullable(r.notes),
    recordedBy: r.recorded_by,
    recordedAt: instant(r.recorded_at),
  })

const mapMaintenanceRecord = (r: Row): AssetMaintenanceRecord =>
  assetMaintenanceRecordSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    assetId: r.asset_id,
    serviceType: r.service_type,
    description: r.description,
    partsUsed: nullable(r.parts_used),
    conditionAfter: r.condition_after,
    performedBy: r.performed_by,
    recordedAt: instant(r.recorded_at),
  })

const grant = (a: AssetAccess, p: AssetPermission) => {
  if (!a.permissions.includes(p)) throw new AssetError('forbidden')
}

const requiredAssetPermission = (type: AssetMutation['type']): AssetPermission => {
  switch (type) {
    case 'create_asset':
    case 'update_asset':
    case 'retire_asset':
      return 'assets.write'
    case 'handover_asset':
      return 'assets.handover'
    case 'record_maintenance':
      return 'assets.maintenance'
  }
}

export class PostgresAssetRepository implements AssetRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}

  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable asset fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_assets'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Assets requires a non-owner runtime')

    const expected = await assetMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_assets.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Unsupported assets ledger')

    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_assets'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [['memberships', 'assets', 'custody_events', 'maintenance_records', 'commands', 'audit']],
      )
    ).rows[0]
    if (policies?.count !== 6) throw new Error('Assets tenant policies must be forced')

    return new PostgresAssetRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: AssetPrincipal,
    work: (client: PoolClient, access: AssetAccess) => Promise<T>,
  ): Promise<T> {
    assetTenantSchema.parse(tenant)
    assetPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])

      const member = async (id: string) => {
        const r = (
          await client.query<Row>('SELECT * FROM archava_assets.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new AssetError('forbidden')
        const m = assetMembershipSchema.parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new AssetError('forbidden')
        return m
      }

      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new AssetError('forbidden')

      let access: AssetAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new AssetError('forbidden')
        access = {
          permissions: actor.permissions.filter((x: AssetPermission) =>
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
        throw new AssetError('conflict')
      throw error
    } finally {
      client.release()
    }
  }

  async access(tenantId: string, principal: AssetPrincipal): Promise<AssetAccess> {
    return this.transaction(tenantId, principal, (_client, access) => Promise.resolve(access))
  }

  async listAssets(
    tenantId: string,
    principal: AssetPrincipal,
    query: AssetPageQuery,
  ): Promise<{ items: Asset[]; nextCursor: string | null }> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'assets.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenantId]

      if (access.scope === 'assigned') {
        params.push(access.ownerId)
        conditions.push(
          `(owner_id = $${params.length} OR current_custodian_id = $${params.length})`,
        )
      }
      if (query.category) {
        params.push(query.category)
        conditions.push(`category = $${params.length}`)
      }
      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      if (query.ownership) {
        params.push(query.ownership)
        conditions.push(`ownership = $${params.length}`)
      }
      if (query.search) {
        params.push(`%${query.search}%`)
        conditions.push(`(name ILIKE $${params.length} OR code ILIKE $${params.length})`)
      }
      if (query.after) {
        params.push(query.after)
        conditions.push(`id > $${params.length}`)
      }

      params.push(query.limit + 1)
      const sql = `
        SELECT * FROM archava_assets.assets
        WHERE ${conditions.join(' AND ')}
        ORDER BY id ASC
        LIMIT $${params.length}
      `
      const rows = (await client.query<Row>(sql, params)).rows
      const hasMore = rows.length > query.limit
      const items = rows.slice(0, query.limit).map(mapAsset)
      const nextCursor = hasMore ? (items[items.length - 1]?.id ?? null) : null

      return { items, nextCursor }
    })
  }

  async detail(tenantId: string, principal: AssetPrincipal, id: string): Promise<AssetDetail> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'assets.read')
      const assetRow = (
        await client.query<Row>(
          'SELECT * FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2',
          [tenantId, id],
        )
      ).rows[0]
      if (!assetRow) throw new AssetError('not_found')
      if (
        access.scope === 'assigned' &&
        assetRow.owner_id !== access.ownerId &&
        assetRow.current_custodian_id !== access.ownerId
      ) {
        throw new AssetError('forbidden')
      }

      const custodyRows = (
        await client.query<Row>(
          'SELECT * FROM archava_assets.custody_events WHERE tenant_id = $1 AND asset_id = $2 ORDER BY recorded_at DESC LIMIT 50',
          [tenantId, id],
        )
      ).rows.map(mapCustodyEvent)

      const maintRows = (
        await client.query<Row>(
          'SELECT * FROM archava_assets.maintenance_records WHERE tenant_id = $1 AND asset_id = $2 ORDER BY recorded_at DESC LIMIT 50',
          [tenantId, id],
        )
      ).rows.map(mapMaintenanceRecord)

      return {
        asset: mapAsset(assetRow),
        custodyEvents: custodyRows,
        maintenanceRecords: maintRows,
      }
    })
  }

  async listCustodyHistory(
    tenantId: string,
    principal: AssetPrincipal,
    assetId: string,
  ): Promise<AssetCustodyEvent[]> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'assets.read')
      const assetRow = (
        await client.query<Row>(
          'SELECT owner_id, current_custodian_id FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2',
          [tenantId, assetId],
        )
      ).rows[0]
      if (!assetRow) throw new AssetError('not_found')
      if (
        access.scope === 'assigned' &&
        assetRow.owner_id !== access.ownerId &&
        assetRow.current_custodian_id !== access.ownerId
      ) {
        throw new AssetError('forbidden')
      }

      return (
        await client.query<Row>(
          'SELECT * FROM archava_assets.custody_events WHERE tenant_id = $1 AND asset_id = $2 ORDER BY recorded_at DESC LIMIT 100',
          [tenantId, assetId],
        )
      ).rows.map(mapCustodyEvent)
    })
  }

  async listMaintenanceHistory(
    tenantId: string,
    principal: AssetPrincipal,
    assetId: string,
  ): Promise<AssetMaintenanceRecord[]> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'assets.read')
      const assetRow = (
        await client.query<Row>(
          'SELECT owner_id, current_custodian_id FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2',
          [tenantId, assetId],
        )
      ).rows[0]
      if (!assetRow) throw new AssetError('not_found')
      if (
        access.scope === 'assigned' &&
        assetRow.owner_id !== access.ownerId &&
        assetRow.current_custodian_id !== access.ownerId
      ) {
        throw new AssetError('forbidden')
      }

      return (
        await client.query<Row>(
          'SELECT * FROM archava_assets.maintenance_records WHERE tenant_id = $1 AND asset_id = $2 ORDER BY recorded_at DESC LIMIT 100',
          [tenantId, assetId],
        )
      ).rows.map(mapMaintenanceRecord)
    })
  }

  async mutate(
    tenantId: string,
    principal: AssetPrincipal,
    mutation: AssetMutation,
  ): Promise<AssetReceipt> {
    return this.transaction(tenantId, principal, async (client, access) => {
      if (principal.kind !== 'human') throw new AssetError('forbidden')
      grant(access, requiredAssetPermission(mutation.type))

      const fingerprint = createHmac('sha256', this.key)
        .update(
          JSON.stringify({
            domain: 'assets',
            tenant: tenantId,
            actor: principal.id,
            type: mutation.type,
            data: mutation.data,
          }),
        )
        .digest('hex')

      const existing = (
        await client.query<Row>(
          'SELECT * FROM archava_assets.commands WHERE tenant_id = $1 AND request_id = $2 FOR UPDATE',
          [tenantId, mutation.requestId],
        )
      ).rows[0]

      if (existing) {
        if (existing.actor_id !== principal.id || existing.fingerprint !== fingerprint)
          throw new AssetError('conflict')
        if (existing.receipt) {
          const parsed = assetReceiptSchema.parse(existing.receipt)
          if (access.scope === 'assigned' && parsed.assetId) {
            const row = (
              await client.query<Row>(
                'SELECT owner_id, current_custodian_id FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2',
                [tenantId, parsed.assetId],
              )
            ).rows[0]
            if (
              row &&
              row.owner_id !== access.ownerId &&
              row.current_custodian_id !== access.ownerId
            ) {
              throw new AssetError('forbidden')
            }
          }
          return parsed
        }
        throw new AssetError('conflict')
      }

      await client.query(
        'INSERT INTO archava_assets.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
        [tenantId, mutation.requestId, principal.id, fingerprint],
      )

      let receipt: AssetReceipt
      const now = (await client.query<{ now: Date }>('SELECT clock_timestamp() AS now')).rows[0]
        ?.now
      const committedAt = instant(now)

      if (mutation.type === 'create_asset') {
        const assetId = randomUUID()
        const d = mutation.data
        const initialStatus: AssetStatus =
          d.ownership === 'customer_owned' ? 'in_custody' : 'available'

        await client.query(
          `INSERT INTO archava_assets.assets(
            tenant_id, id, code, name, category, serial_number, model, ownership, customer_contact_id,
            status, condition, current_custodian_id, current_location, owner_id, revision, created_at, updated_at
          ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, 1, $15, $15)`,
          [
            tenantId,
            assetId,
            d.code,
            d.name,
            d.category,
            d.serialNumber ?? null,
            d.model ?? null,
            d.ownership,
            d.customerContactId ?? null,
            initialStatus,
            d.condition,
            d.ownership === 'customer_owned' ? principal.id : null,
            d.initialLocation ?? null,
            principal.id,
            now,
          ],
        )

        // Log initial intake event if customer-owned
        if (d.ownership === 'customer_owned') {
          const eventId = randomUUID()
          await client.query(
            `INSERT INTO archava_assets.custody_events(
              tenant_id, id, asset_id, event_type, from_custodian_id, to_custodian_id,
              condition_before, condition_after, location, notes, recorded_by, recorded_at
            ) VALUES($1, $2, $3, 'service_intake', $4, $5, $6, $6, $7, 'Initial service intake from customer', $5, $8)`,
            [
              tenantId,
              eventId,
              assetId,
              d.customerContactId ?? null,
              principal.id,
              d.condition,
              d.initialLocation ?? null,
              now,
            ],
          )
        }

        await client.query(
          `INSERT INTO archava_assets.audit(
            tenant_id, request_id, actor_id, operation, asset_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'create_asset', $4, 1, $5)`,
          [tenantId, mutation.requestId, principal.id, assetId, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'create_asset',
          assetId,
          revision: 1,
          committedAt,
        }
      } else if (mutation.type === 'update_asset') {
        const d = mutation.data
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.assetId],
          )
        ).rows[0]
        if (!row) throw new AssetError('not_found')
        if (
          access.scope === 'assigned' &&
          row.owner_id !== access.ownerId &&
          row.current_custodian_id !== access.ownerId
        )
          throw new AssetError('forbidden')
        if (row.status === 'retired') throw new AssetError('archived')
        if (Number(row.revision) !== d.expectedRevision) throw new AssetError('conflict')

        const nextRevision = d.expectedRevision + 1
        await client.query(
          `UPDATE archava_assets.assets SET
            name = COALESCE($3, name),
            category = COALESCE($4, category),
            serial_number = CASE WHEN $5::text IS NOT NULL THEN $5 ELSE serial_number END,
            model = CASE WHEN $6::text IS NOT NULL THEN $6 ELSE model END,
            current_location = CASE WHEN $7::text IS NOT NULL THEN $7 ELSE current_location END,
            revision = $8,
            updated_at = $9
          WHERE tenant_id = $1 AND id = $2`,
          [
            tenantId,
            d.assetId,
            d.name ?? null,
            d.category ?? null,
            d.serialNumber !== undefined ? d.serialNumber : null,
            d.model !== undefined ? d.model : null,
            d.currentLocation !== undefined ? d.currentLocation : null,
            nextRevision,
            now,
          ],
        )

        await client.query(
          `INSERT INTO archava_assets.audit(
            tenant_id, request_id, actor_id, operation, asset_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'update_asset', $4, $5, $6)`,
          [tenantId, mutation.requestId, principal.id, d.assetId, nextRevision, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'update_asset',
          assetId: d.assetId,
          revision: nextRevision,
          committedAt,
        }
      } else if (mutation.type === 'handover_asset') {
        const d = mutation.data
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.assetId],
          )
        ).rows[0]
        if (!row) throw new AssetError('not_found')
        if (
          access.scope === 'assigned' &&
          row.owner_id !== access.ownerId &&
          row.current_custodian_id !== access.ownerId
        )
          throw new AssetError('forbidden')
        if (row.status === 'retired') throw new AssetError('archived')
        if (row.status === 'returned_to_customer') throw new AssetError('archived')
        if (Number(row.revision) !== d.expectedRevision) throw new AssetError('conflict')

        let nextStatus: AssetStatus = row.status as AssetStatus
        let nextCustodian: string | null = nullable(row.current_custodian_id)
        const conditionBefore = row.condition as string

        if (d.eventType === 'dispatch') {
          if (row.status === 'in_custody') throw new AssetError('already_checked_out')
          if (row.condition === 'unserviceable' || row.condition === 'damaged') {
            throw new AssetError('invalid_condition', 'Cannot dispatch damaged/unserviceable asset')
          }
          nextStatus = 'in_custody'
          nextCustodian = d.toCustodianId ?? null
        } else if (d.eventType === 'checkin') {
          if (row.status !== 'in_custody') throw new AssetError('not_in_custody')
          if (d.conditionAfter === 'damaged' || d.conditionAfter === 'unserviceable') {
            nextStatus = 'under_maintenance'
          } else {
            nextStatus = 'available'
          }
          nextCustodian = null
        } else if (d.eventType === 'transfer') {
          if (row.status !== 'in_custody') throw new AssetError('not_in_custody')
          nextStatus = 'in_custody'
          nextCustodian = d.toCustodianId ?? null
        } else if (d.eventType === 'service_intake') {
          nextStatus = 'under_maintenance'
          nextCustodian = d.toCustodianId ?? principal.id
        } else if (d.eventType === 'customer_return') {
          if (row.ownership !== 'customer_owned') {
            throw new AssetError(
              'invalid_request',
              'Only customer-owned assets can be returned to customer',
            )
          }
          nextStatus = 'returned_to_customer'
          nextCustodian = null
        }

        const nextRevision = d.expectedRevision + 1
        await client.query(
          `UPDATE archava_assets.assets SET
            status = $3,
            condition = $4,
            current_custodian_id = $5,
            current_location = COALESCE($6, current_location),
            revision = $7,
            updated_at = $8
          WHERE tenant_id = $1 AND id = $2`,
          [
            tenantId,
            d.assetId,
            nextStatus,
            d.conditionAfter,
            nextCustodian,
            d.location ?? null,
            nextRevision,
            now,
          ],
        )

        const eventId = randomUUID()
        await client.query(
          `INSERT INTO archava_assets.custody_events(
            tenant_id, id, asset_id, event_type, from_custodian_id, to_custodian_id,
            condition_before, condition_after, location, notes, recorded_by, recorded_at
          ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)`,
          [
            tenantId,
            eventId,
            d.assetId,
            d.eventType,
            row.current_custodian_id,
            d.toCustodianId ?? null,
            conditionBefore,
            d.conditionAfter,
            d.location ?? null,
            d.notes ?? null,
            principal.id,
            now,
          ],
        )

        await client.query(
          `INSERT INTO archava_assets.audit(
            tenant_id, request_id, actor_id, operation, asset_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'handover_asset', $4, $5, $6)`,
          [tenantId, mutation.requestId, principal.id, d.assetId, nextRevision, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'handover_asset',
          assetId: d.assetId,
          revision: nextRevision,
          committedAt,
        }
      } else if (mutation.type === 'record_maintenance') {
        const d = mutation.data
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.assetId],
          )
        ).rows[0]
        if (!row) throw new AssetError('not_found')
        if (
          access.scope === 'assigned' &&
          row.owner_id !== access.ownerId &&
          row.current_custodian_id !== access.ownerId
        )
          throw new AssetError('forbidden')
        if (row.status === 'retired') throw new AssetError('archived')
        if (Number(row.revision) !== d.expectedRevision) throw new AssetError('conflict')

        let nextStatus: AssetStatus = row.status as AssetStatus
        if (
          row.status === 'under_maintenance' &&
          (d.conditionAfter === 'excellent' ||
            d.conditionAfter === 'good' ||
            d.conditionAfter === 'fair')
        ) {
          nextStatus = row.ownership === 'customer_owned' ? 'under_maintenance' : 'available'
        }

        const nextRevision = d.expectedRevision + 1
        await client.query(
          `UPDATE archava_assets.assets SET
            status = $3,
            condition = $4,
            current_location = COALESCE($5, current_location),
            revision = $6,
            updated_at = $7
          WHERE tenant_id = $1 AND id = $2`,
          [
            tenantId,
            d.assetId,
            nextStatus,
            d.conditionAfter,
            d.location ?? null,
            nextRevision,
            now,
          ],
        )

        const maintId = randomUUID()
        await client.query(
          `INSERT INTO archava_assets.maintenance_records(
            tenant_id, id, asset_id, service_type, description, parts_used, condition_after, performed_by, recorded_at
          ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)`,
          [
            tenantId,
            maintId,
            d.assetId,
            d.serviceType,
            d.description,
            d.partsUsed ?? null,
            d.conditionAfter,
            principal.id,
            now,
          ],
        )

        await client.query(
          `INSERT INTO archava_assets.audit(
            tenant_id, request_id, actor_id, operation, asset_id, revision, occurred_at
          ) VALUES($1, $2, $3, 'record_maintenance', $4, $5, $6)`,
          [tenantId, mutation.requestId, principal.id, d.assetId, nextRevision, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'record_maintenance',
          assetId: d.assetId,
          revision: nextRevision,
          committedAt,
        }
      } else {
        // retire_asset
        const d = mutation.data
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2 FOR UPDATE',
            [tenantId, d.assetId],
          )
        ).rows[0]
        if (!row) throw new AssetError('not_found')
        if (
          access.scope === 'assigned' &&
          row.owner_id !== access.ownerId &&
          row.current_custodian_id !== access.ownerId
        )
          throw new AssetError('forbidden')
        if (row.status === 'retired') throw new AssetError('archived')
        if (Number(row.revision) !== d.expectedRevision) throw new AssetError('conflict')

        const nextRevision = d.expectedRevision + 1
        await client.query(
          `UPDATE archava_assets.assets SET
            status = 'retired',
            current_custodian_id = null,
            revision = $3,
            updated_at = $4
          WHERE tenant_id = $1 AND id = $2`,
          [tenantId, d.assetId, nextRevision, now],
        )

        await client.query(
          `INSERT INTO archava_assets.audit(
            tenant_id, request_id, actor_id, operation, asset_id, revision, reason, occurred_at
          ) VALUES($1, $2, $3, 'retire_asset', $4, $5, $6, $7, $8)`,
          [tenantId, mutation.requestId, principal.id, d.assetId, nextRevision, d.reason, now],
        )

        receipt = {
          requestId: mutation.requestId,
          type: 'retire_asset',
          assetId: d.assetId,
          revision: nextRevision,
          committedAt,
        }
      }

      await client.query(
        'UPDATE archava_assets.commands SET receipt = $3 WHERE tenant_id = $1 AND request_id = $2',
        [tenantId, mutation.requestId, JSON.stringify(receipt)],
      )

      return receipt
    })
  }

  async commandStatus(
    tenantId: string,
    principal: AssetPrincipal,
    requestId: string,
  ): Promise<AssetCommandStatus> {
    return this.transaction(tenantId, principal, async (client, access) => {
      grant(access, 'assets.read')
      const row = (
        await client.query<Row>(
          'SELECT * FROM archava_assets.commands WHERE tenant_id = $1 AND request_id = $2',
          [tenantId, requestId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'not_committed' }
      const receipt = assetReceiptSchema.parse(row.receipt)
      if (access.scope === 'assigned') {
        if (row.actor_id !== principal.id) {
          throw new AssetError('forbidden')
        }
        if (receipt.assetId) {
          const assetRow = (
            await client.query<Row>(
              'SELECT owner_id, current_custodian_id FROM archava_assets.assets WHERE tenant_id = $1 AND id = $2',
              [tenantId, receipt.assetId],
            )
          ).rows[0]
          if (
            assetRow &&
            assetRow.owner_id !== access.ownerId &&
            assetRow.current_custodian_id !== access.ownerId
          ) {
            throw new AssetError('forbidden')
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
