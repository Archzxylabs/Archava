import { createHash, createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import {
  LaundryError,
  laundryReceiptSchema,
  laundryAccessSchema,
  orderSchema,
  orderLineSchema,
  processEventSchema,
  findingSchema,
  orderDetailSchema,
  PROCESS_STAGE,
  MACHINE_EVENTS,
  EXCEPTION_DISPOSITIONS,
  type LaundryRepository,
  type LaundryPrincipal,
  type LaundryMutation,
  type LaundryReceipt,
  type LaundryAccess,
  type LaundryCommandStatus,
  type LaundryPermission,
  type OrderPageQuery,
  type LaundryOrder,
  type OrderStatus,
  type OrderDetail,
  type OrderLine,
  type ProcessRecord,
  type Finding,
} from '@archava/laundry'
import { laundryMigrationLedger } from './migrate.js'

type Row = Record<string, unknown>

function instant(d: unknown): string {
  if (d instanceof Date) return d.toISOString()
  if (typeof d === 'string') return new Date(d).toISOString()
  return new Date().toISOString()
}

function nullable<T>(v: unknown): T | null {
  return v === null || v === undefined ? null : (v as T)
}

function grant(access: LaundryAccess, permission: LaundryPermission): void {
  if (!access.permissions.includes(permission)) {
    throw new LaundryError('forbidden', `Missing required permission: ${permission}`)
  }
}

const ALLOWED_TRANSITIONS: Readonly<Record<OrderStatus, readonly OrderStatus[]>> = Object.freeze({
  received: ['processing', 'canceled'],
  processing: ['ready', 'canceled'],
  ready: ['returned'],
  returned: [],
  canceled: [],
})

function isException(disposition: string): boolean {
  return (EXCEPTION_DISPOSITIONS as readonly string[]).includes(disposition)
}

function mapOrder(row: Row, tenant: string): LaundryOrder {
  return orderSchema.parse({
    id: row.id,
    tenantId: tenant,
    orderCode: row.order_code,
    kind: row.kind,
    customerRef: nullable<string>(row.customer_ref),
    status: row.status,
    currency: row.currency,
    totalMinor: row.total_minor,
    itemCount: row.item_count,
    weightGrams: row.weight_grams,
    unreconciledCount: row.unreconciled_count,
    revision: row.revision,
    createdAt: instant(row.created_at),
    updatedAt: instant(row.updated_at),
  })
}

function mapLine(row: Row, tenant: string): OrderLine {
  return orderLineSchema.parse({
    id: row.id,
    tenantId: tenant,
    orderId: row.order_id,
    itemRef: row.item_ref,
    quantity: row.quantity,
    unit: row.unit,
    unitPriceMinor: row.unit_price_minor,
    lineTotalMinor: row.line_total_minor,
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

function mapProcess(row: Row, tenant: string): ProcessRecord {
  return processEventSchema.parse({
    id: row.id,
    tenantId: tenant,
    orderId: row.order_id,
    event: row.event,
    note: nullable<string>(row.note),
    evidenceRef: nullable<string>(row.evidence_ref),
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

function mapFinding(row: Row, tenant: string): Finding {
  return findingSchema.parse({
    id: row.id,
    tenantId: tenant,
    orderId: row.order_id,
    category: row.category,
    disposition: row.disposition,
    quantity: nullable<number>(row.quantity),
    reason: row.reason,
    evidenceRef: nullable<string>(row.evidence_ref),
    reconcilesId: nullable<string>(row.reconciles_id),
    recordedBy: row.recorded_by,
    recordedAt: instant(row.recorded_at),
  })
}

export class PostgresLaundryRepository implements LaundryRepository {
  constructor(
    private readonly pool: Pool,
    private readonly key: string = '',
  ) {}

  static async create(pool: Pool, key: string): Promise<PostgresLaundryRepository> {
    if (key.length < 32)
      throw new Error('Stable laundry fingerprint key must have at least 32 characters')

    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_laundry'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Laundry requires a non-owner runtime')

    const expected = await laundryMigrationLedger()
    const actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_laundry.schema_migrations ORDER BY id',
      )
    ).rows
    if (
      actual.length !== expected.length ||
      actual.some(
        (x: { id: string; checksum: string }, i: number) =>
          x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum,
      )
    )
      throw new Error('Laundry schema migrations mismatch')

    return new PostgresLaundryRepository(pool, key)
  }

  private async transaction<T>(
    tenant: string,
    principal: LaundryPrincipal,
    work: (client: PoolClient, access: LaundryAccess) => Promise<T>,
  ): Promise<T> {
    const client = await this.pool.connect()
    try {
      const userRes = await client.query<{ usename: string; usesuper: boolean }>(
        'SELECT usename, usesuper FROM pg_user WHERE usename = current_user',
      )
      if (userRes.rows[0]?.usesuper) {
        throw new LaundryError('forbidden', 'Superuser connections are prohibited')
      }

      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id', $1, true)", [tenant])

      const memRes = await client.query<Row>(
        'SELECT * FROM archava_laundry.lock_membership($1, $2)',
        [tenant, principal.id],
      )
      const mem = memRes.rows[0]
      if (!mem || !mem.active) {
        throw new LaundryError('forbidden', 'Principal is inactive or not enrolled in tenant')
      }

      const access: LaundryAccess = {
        ownerId: principal.id,
        permissions: (mem.permissions as LaundryPermission[]) ?? [],
        scope: (mem.scope as 'all' | 'assigned') ?? 'assigned',
      }

      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK').catch(() => {})
      throw error
    } finally {
      client.release()
    }
  }

  async execute(
    tenant: string,
    principal: LaundryPrincipal,
    mutation: LaundryMutation,
  ): Promise<LaundryReceipt> {
    return this.transaction(tenant, principal, async (client, access) => {
      const fingerprint = this.key
        ? createHmac('sha256', this.key).update(JSON.stringify(mutation)).digest('hex')
        : createHash('sha256').update(JSON.stringify(mutation)).digest('hex')

      const existingCmd = (
        await client.query<Row>(
          'SELECT receipt, fingerprint FROM archava_laundry.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, mutation.commandId],
        )
      ).rows[0]

      if (existingCmd) {
        if (existingCmd.fingerprint !== fingerprint) {
          throw new LaundryError('conflict', 'Command payload fingerprint mismatch on replay')
        }
        if (existingCmd.receipt) {
          return laundryReceiptSchema.parse({
            ...existingCmd.receipt,
            idempotencyReplay: true,
          })
        }
      } else {
        await client.query(
          'INSERT INTO archava_laundry.commands(tenant_id, request_id, actor_id, fingerprint) VALUES($1, $2, $3, $4)',
          [tenant, mutation.commandId, access.ownerId, fingerprint],
        )
      }

      const audit = async (
        operation: string,
        orderId: string,
        revision: number,
        reason: string,
      ) => {
        await client.query(
          'INSERT INTO archava_laundry.audit(tenant_id, request_id, actor_id, operation, order_id, revision, reason) VALUES($1, $2, $3, $4, $5, $6, $7)',
          [tenant, mutation.commandId, access.ownerId, operation, orderId, revision, reason],
        )
      }

      const loadOrder = async (orderId: string) => {
        const row = (
          await client.query<Row>(
            'SELECT * FROM archava_laundry.orders WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, orderId],
          )
        ).rows[0]
        if (!row) throw new LaundryError('not_found', 'Laundry order not found')
        return row
      }

      const assertOpen = (order: Row) => {
        if (order.status === 'returned' || order.status === 'canceled') {
          throw new LaundryError(
            'invalid_transition',
            'Evidence cannot be recorded on a returned or canceled order',
          )
        }
      }

      let receipt: LaundryReceipt

      switch (mutation.type) {
        case 'create_order': {
          grant(access, 'laundry.write')
          const orderId = randomUUID()
          let totalMinor = 0
          let itemCount = 0
          let weightGrams = 0
          const lineValues: unknown[][] = []
          for (const line of mutation.lines) {
            const lineTotal = line.quantity * line.unitPriceMinor
            totalMinor += lineTotal
            if (line.unit === 'piece') itemCount += line.quantity
            else weightGrams += line.quantity
            lineValues.push([
              orderId,
              line.itemRef,
              line.quantity,
              line.unit,
              line.unitPriceMinor,
              lineTotal,
            ])
          }
          if (totalMinor > 2_147_483_647)
            throw new LaundryError('bad_request', 'Order total exceeds supported integer range')

          const orderRow = (
            await client.query<Row>(
              `INSERT INTO archava_laundry.orders(
                tenant_id, id, order_code, kind, customer_ref, status, currency, total_minor, item_count, weight_grams, owner_id
              ) VALUES($1, $2, $3, $4, $5, 'received', $6, $7, $8, $9, $10)
              RETURNING *`,
              [
                tenant,
                orderId,
                mutation.orderCode,
                mutation.kind,
                mutation.customerRef ?? null,
                mutation.currency,
                totalMinor,
                itemCount,
                weightGrams,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!orderRow) throw new LaundryError('not_found', 'Failed to create laundry order')

          for (const values of lineValues) {
            await client.query(
              `INSERT INTO archava_laundry.order_lines(
                tenant_id, id, order_id, item_ref, quantity, unit, unit_price_minor, line_total_minor, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9)`,
              [tenant, randomUUID(), ...values, access.ownerId],
            )
          }

          await audit(
            'create_order',
            orderId,
            1,
            `Created ${mutation.kind} order ${mutation.orderCode} totaling ${totalMinor} ${mutation.currency}`,
          )

          receipt = laundryReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'create_order',
            orderId,
            status: 'received',
            revision: 1,
            idempotencyReplay: false,
            executedAt: instant(orderRow.created_at),
          })
          break
        }

        case 'record_process': {
          const order = await loadOrder(mutation.orderId)
          assertOpen(order)
          if (MACHINE_EVENTS.has(mutation.event)) grant(access, 'laundry.process')
          else grant(access, 'laundry.write')

          const history = (
            await client.query<{ event: string }>(
              'SELECT event FROM archava_laundry.process_events WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
              [tenant, mutation.orderId],
            )
          ).rows

          if (history.length === 0) {
            if (mutation.event !== 'intake')
              throw new LaundryError(
                'process_out_of_order',
                'Processing chain must begin with intake',
              )
          } else {
            let maxStage = -1
            let furthest = 'intake'
            for (const item of history) {
              const stage = PROCESS_STAGE[item.event]
              if (stage !== undefined && stage > maxStage) {
                maxStage = stage
                furthest = item.event
              }
            }
            const stage = PROCESS_STAGE[mutation.event] ?? -1
            if (stage <= maxStage)
              throw new LaundryError(
                'process_out_of_order',
                `Process ${mutation.event} must follow ${furthest}`,
              )
          }

          const processId = randomUUID()
          const processRow = (
            await client.query<Row>(
              `INSERT INTO archava_laundry.process_events(
                tenant_id, id, order_id, event, note, evidence_ref, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7)
              RETURNING *`,
              [
                tenant,
                processId,
                mutation.orderId,
                mutation.event,
                mutation.note ?? null,
                mutation.evidenceRef ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!processRow) throw new LaundryError('not_found', 'Failed to record process step')

          await audit(
            'record_process',
            mutation.orderId,
            Number(order.revision),
            `Recorded process step ${mutation.event}`,
          )

          receipt = laundryReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_process',
            orderId: mutation.orderId,
            processId,
            status: mutation.event,
            revision: Number(order.revision),
            idempotencyReplay: false,
            executedAt: instant(processRow.recorded_at),
          })
          break
        }

        case 'record_finding': {
          grant(access, 'laundry.write')
          const order = await loadOrder(mutation.orderId)
          assertOpen(order)
          if (mutation.reconcilesId && mutation.disposition !== 'accepted')
            throw new LaundryError(
              'bad_request',
              'Only an accepted finding can reconcile an earlier exception',
            )

          let nextUnreconciled = Number(order.unreconciled_count)

          if (isException(mutation.disposition)) {
            nextUnreconciled += 1
          } else if (mutation.disposition === 'accepted' && mutation.reconcilesId) {
            const target = (
              await client.query<Row>(
                'SELECT * FROM archava_laundry.findings WHERE tenant_id=$1 AND id=$2',
                [tenant, mutation.reconcilesId],
              )
            ).rows[0]
            if (!target) throw new LaundryError('not_found', 'Reconciliation target not found')
            if (target.order_id !== mutation.orderId)
              throw new LaundryError(
                'conflict',
                'Reconciliation target belongs to a different order',
              )
            if (!isException(target.disposition as string))
              throw new LaundryError(
                'conflict',
                'Only partial or rejected findings require reconciliation',
              )
            const already = (
              await client.query<Row>(
                'SELECT 1 FROM archava_laundry.findings WHERE tenant_id=$1 AND reconciles_id=$2',
                [tenant, mutation.reconcilesId],
              )
            ).rows[0]
            if (already) throw new LaundryError('conflict', 'Finding has already been reconciled')
            nextUnreconciled = Math.max(0, nextUnreconciled - 1)
          }

          await client.query(
            'UPDATE archava_laundry.orders SET unreconciled_count=$1, updated_at=clock_timestamp() WHERE tenant_id=$2 AND id=$3',
            [nextUnreconciled, tenant, mutation.orderId],
          )

          const findingId = randomUUID()
          const findingRow = (
            await client.query<Row>(
              `INSERT INTO archava_laundry.findings(
                tenant_id, id, order_id, category, disposition, quantity, reason, evidence_ref, reconciles_id, recorded_by
              ) VALUES($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
              RETURNING *`,
              [
                tenant,
                findingId,
                mutation.orderId,
                mutation.category,
                mutation.disposition,
                mutation.quantity ?? null,
                mutation.reason,
                mutation.evidenceRef ?? null,
                mutation.reconcilesId ?? null,
                access.ownerId,
              ],
            )
          ).rows[0]
          if (!findingRow) throw new LaundryError('not_found', 'Failed to record finding')

          await audit(
            'record_finding',
            mutation.orderId,
            Number(order.revision),
            `Recorded ${mutation.category} finding (${mutation.disposition}): ${mutation.reason}`,
          )

          receipt = laundryReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'record_finding',
            orderId: mutation.orderId,
            findingId,
            status: mutation.disposition,
            revision: Number(order.revision),
            idempotencyReplay: false,
            executedAt: instant(findingRow.recorded_at),
          })
          break
        }

        case 'change_order_status': {
          grant(access, 'laundry.write')
          const order = await loadOrder(mutation.orderId)

          if (order.revision !== mutation.expectedRevision) {
            throw new LaundryError('conflict', 'Optimistic revision conflict on order')
          }

          const allowed = ALLOWED_TRANSITIONS[order.status as OrderStatus] ?? []
          if (!allowed.includes(mutation.targetStatus))
            throw new LaundryError(
              'invalid_transition',
              `Order cannot move from ${String(order.status)} to ${mutation.targetStatus}`,
            )

          if (mutation.targetStatus === 'ready') {
            if (Number(order.unreconciled_count) > 0)
              throw new LaundryError(
                'unreconciled_exceptions',
                'Quality findings must be reconciled before the order is marked ready',
              )
            const quality = (
              await client.query<Row>(
                "SELECT 1 FROM archava_laundry.process_events WHERE tenant_id=$1 AND order_id=$2 AND event='quality_check' LIMIT 1",
                [tenant, mutation.orderId],
              )
            ).rows[0]
            if (!quality)
              throw new LaundryError(
                'invalid_transition',
                'A quality check must be recorded before the order is marked ready',
              )
          }

          const nextRev = Number(order.revision) + 1
          const updated = (
            await client.query<Row>(
              `UPDATE archava_laundry.orders
               SET status=$1, revision=$2, updated_at=clock_timestamp()
               WHERE tenant_id=$3 AND id=$4
               RETURNING *`,
              [mutation.targetStatus, nextRev, tenant, mutation.orderId],
            )
          ).rows[0]
          if (!updated) throw new LaundryError('not_found', 'Order update failed')

          await audit('change_order_status', mutation.orderId, nextRev, mutation.reason)

          receipt = laundryReceiptSchema.parse({
            commandId: mutation.commandId,
            tenantId: tenant,
            action: 'change_order_status',
            orderId: mutation.orderId,
            status: mutation.targetStatus,
            revision: nextRev,
            idempotencyReplay: false,
            executedAt: instant(updated.updated_at),
          })
          break
        }
      }

      await client.query(
        'UPDATE archava_laundry.commands SET receipt=$1 WHERE tenant_id=$2 AND request_id=$3',
        [receipt, tenant, mutation.commandId],
      )

      return receipt
    })
  }

  async getCommandStatus(
    tenant: string,
    principal: LaundryPrincipal,
    commandId: string,
  ): Promise<LaundryCommandStatus> {
    return this.transaction(tenant, principal, async (client) => {
      const row = (
        await client.query<Row>(
          'SELECT receipt FROM archava_laundry.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, commandId],
        )
      ).rows[0]
      if (!row || !row.receipt) return { status: 'unknown' }
      return {
        status: 'recorded',
        receipt: laundryReceiptSchema.parse(row.receipt),
      }
    })
  }

  async getAccess(tenant: string, principal: LaundryPrincipal): Promise<LaundryAccess> {
    return this.transaction(tenant, principal, (_client, access) =>
      Promise.resolve(laundryAccessSchema.parse(access)),
    )
  }

  async listOrders(
    tenant: string,
    principal: LaundryPrincipal,
    query: OrderPageQuery,
  ): Promise<readonly LaundryOrder[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'laundry.read')
      const conditions: string[] = ['tenant_id = $1']
      const params: unknown[] = [tenant]

      if (query.status) {
        params.push(query.status)
        conditions.push(`status = $${params.length}`)
      }
      if (query.kind) {
        params.push(query.kind)
        conditions.push(`kind = $${params.length}`)
      }

      params.push(query.limit ?? 50)
      const limitParam = `$${params.length}`
      params.push(query.offset ?? 0)
      const offsetParam = `$${params.length}`

      const res = await client.query<Row>(
        `SELECT * FROM archava_laundry.orders
         WHERE ${conditions.join(' AND ')}
         ORDER BY created_at ASC, id ASC
         LIMIT ${limitParam} OFFSET ${offsetParam}`,
        params,
      )

      return res.rows.map((row) => mapOrder(row, tenant))
    })
  }

  async getOrderDetail(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<OrderDetail | null> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'laundry.read')
      const orderRow = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.orders WHERE tenant_id=$1 AND id=$2',
          [tenant, orderId],
        )
      ).rows[0]
      if (!orderRow) return null

      const lineRows = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.order_lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, orderId],
        )
      ).rows

      const processRows = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.process_events WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, orderId],
        )
      ).rows

      const findingRows = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.findings WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, orderId],
        )
      ).rows

      return orderDetailSchema.parse({
        order: mapOrder(orderRow, tenant),
        lines: lineRows.map((row) => mapLine(row, tenant)),
        process: processRows.map((row) => mapProcess(row, tenant)),
        findings: findingRows.map((row) => mapFinding(row, tenant)),
      })
    })
  }

  async listLines(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly OrderLine[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'laundry.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.order_lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, orderId],
        )
      ).rows
      return rows.map((row) => mapLine(row, tenant))
    })
  }

  async listProcess(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly ProcessRecord[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'laundry.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.process_events WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, orderId],
        )
      ).rows
      return rows.map((row) => mapProcess(row, tenant))
    })
  }

  async listFindings(
    tenant: string,
    principal: LaundryPrincipal,
    orderId: string,
  ): Promise<readonly Finding[]> {
    return this.transaction(tenant, principal, async (client, access) => {
      grant(access, 'laundry.read')
      const rows = (
        await client.query<Row>(
          'SELECT * FROM archava_laundry.findings WHERE tenant_id=$1 AND order_id=$2 ORDER BY recorded_at ASC, id ASC',
          [tenant, orderId],
        )
      ).rows
      return rows.map((row) => mapFinding(row, tenant))
    })
  }
}
