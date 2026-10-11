import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  CommerceError,
  commerceIdSchema,
  commercePrincipalSchema,
  commerceMembershipSchema,
  commerceMutationSchema,
  commerceReceiptSchema,
  productPageSchema,
  orderPageSchema,
  stockHistoryQuerySchema,
  orderSchema,
  orderLineSchema,
  orderReturnSchema,
  commerceAuditSchema,
  stockMovementSchema,
  type CommercePrincipal,
  type CommerceAccess,
  type CommercePermission,
  type CommerceRepository,
  type CommerceMutation,
  type CommerceReceipt,
  type CommerceCommandStatus,
  type ProductPageQuery,
  type OrderPageQuery,
  type StockHistoryQuery,
} from '@archava/commerce'
import {
  assertCommerceKernel,
  commerceInstant,
  commerceProduct,
  commerceClock,
  lockCommerceProducts,
  readCommerceStock,
  quoteCommerceLines,
  insertCommerceLines,
  createCommerceDraft,
  reserveCommerceOrder,
  fulfillCommerceOrder,
  cancelCommerceOrder,
  type CommerceRow,
} from './kernel.js'

const nullableInstant = (x: unknown) => (x === null ? null : commerceInstant(x))
const order = (row: CommerceRow, observed: string) =>
  orderSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    ownerId: row.owner_id,
    customerName: row.customer_name,
    state: row.state,
    effectiveState:
      row.state === 'reserved' &&
      row.expires_at instanceof Date &&
      row.expires_at.getTime() <= Date.parse(observed)
        ? 'expired'
        : row.state,
    totalEstimate: Number(row.total_estimate),
    currency: row.currency,
    expiresAt: nullableInstant(row.expires_at),
    fulfilledAt: nullableInstant(row.fulfilled_at),
    managedBy: row.managed_by,
    revision: row.revision,
    createdAt: commerceInstant(row.created_at),
    updatedAt: commerceInstant(row.updated_at),
  })
const line = (row: CommerceRow) =>
  orderLineSchema.parse({
    id: row.id,
    productId: row.product_id,
    sku: row.sku,
    title: row.title,
    unit: row.unit,
    trackStock: row.track_stock,
    quantity: row.quantity,
    price: row.price,
    productRevision: row.product_revision,
    returnedQuantity: row.returned_quantity,
  })
const canonical = (value: unknown): string => {
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
function grant(access: CommerceAccess, permission: CommercePermission) {
  if (!access.permissions.includes(permission)) throw new CommerceError('forbidden')
}
function globalAccess(access: CommerceAccess) {
  if (access.scope !== 'all') throw new CommerceError('forbidden')
}
export class PostgresCommerceRepository implements CommerceRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly fingerprintKey: string,
  ) {}
  static async create(pool: Pool, fingerprintKey: string) {
    if (fingerprintKey.length < 32)
      throw new Error('Stable commerce fingerprint key must have at least 32 characters')
    await assertCommerceKernel(pool)
    return new PostgresCommerceRepository(pool, fingerprintKey)
  }
  private async transaction<T>(
    tenant: string,
    principal: CommercePrincipal,
    permission: CommercePermission | null,
    work: (client: PoolClient, access: CommerceAccess) => Promise<T>,
  ): Promise<T> {
    commerceIdSchema.parse(tenant)
    commercePrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string) => {
        const row = (
          await client.query<CommerceRow>('SELECT * FROM archava_commerce.lock_membership($1,$2)', [
            tenant,
            id,
          ])
        ).rows[0]
        if (!row) throw new CommerceError('forbidden')
        const result = commerceMembershipSchema.parse({
          principalId: row.principal_id,
          kind: row.kind,
          permissions: row.permissions,
          scope: row.scope,
          active: row.active,
        })
        if (!result.active) throw new CommerceError('forbidden')
        return result
      }
      const actor = await member(principal.id)
      if (actor.kind !== principal.kind) throw new CommerceError('forbidden')
      let access: CommerceAccess = {
        permissions: actor.permissions,
        ownerId: actor.principalId,
        scope: actor.scope,
      }
      if (principal.kind === 'service') {
        const human = await member(principal.onBehalfOf)
        if (human.kind !== 'human') throw new CommerceError('forbidden')
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
        error &&
        typeof error === 'object' &&
        'code' in error &&
        ['23514', '23503', '23505', '23P01', '22003', '40P01', '55P03'].includes(String(error.code))
      )
        throw new CommerceError('conflict')
      throw error
    } finally {
      client.release()
    }
  }
  private async ownedOrder(
    client: PoolClient,
    tenant: string,
    access: CommerceAccess,
    id: string,
    lock = false,
  ) {
    grant(access, 'orders.read')
    const row = (
      await client.query<CommerceRow>(
        `SELECT * FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4)${lock ? ' FOR UPDATE' : ''}`,
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new CommerceError('not_found')
    return row
  }
  access(tenant: string, principal: CommercePrincipal) {
    return this.transaction(tenant, principal, null, (_client, access) => Promise.resolve(access))
  }
  products(tenant: string, principal: CommercePrincipal, raw: ProductPageQuery) {
    const q = productPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'catalog.read', async (client) => {
      const observed = await commerceClock(client),
        rows = (
          await client.query<CommerceRow>(
            'SELECT * FROM archava_commerce.products WHERE tenant_id=$1 AND ($2::uuid IS NULL OR id>$2) AND ($3::boolean IS NULL OR active=$3) ORDER BY id LIMIT $4',
            [tenant, q.after ?? null, q.active ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map(commerceProduct)
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  product(tenant: string, principal: CommercePrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(tenant, principal, 'catalog.read', async (client) =>
      commerceProduct(
        (
          await client.query<CommerceRow>(
            'SELECT * FROM archava_commerce.products WHERE tenant_id=$1 AND id=$2',
            [tenant, id],
          )
        ).rows[0],
      ),
    )
  }
  stock(tenant: string, principal: CommercePrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(tenant, principal, 'stock.read', async (client, access) => {
      grant(access, 'catalog.read')
      await lockCommerceProducts(client, tenant, [id])
      return readCommerceStock(client, tenant, id, await commerceClock(client))
    })
  }
  movements(tenant: string, principal: CommercePrincipal, id: string, raw: StockHistoryQuery) {
    z.uuid().parse(id)
    const q = stockHistoryQuerySchema.parse(raw)
    return this.transaction(tenant, principal, 'stock.read', async (client, access) => {
      grant(access, 'audit.read')
      grant(access, 'catalog.read')
      await lockCommerceProducts(client, tenant, [id])
      const rows = (
        await client.query<CommerceRow>(
          'SELECT * FROM archava_commerce.movements WHERE tenant_id=$1 AND product_id=$2 ORDER BY occurred_at DESC,request_id DESC,kind LIMIT $3',
          [tenant, id, q.limit + 1],
        )
      ).rows
      return {
        items: rows.slice(0, q.limit).map((x) =>
          stockMovementSchema.parse({
            requestId: x.request_id,
            productId: x.product_id,
            orderId: x.order_id,
            kind: x.kind,
            quantity: x.quantity,
            reason: x.reason,
            occurredAt: commerceInstant(x.occurred_at),
          }),
        ),
        truncated: rows.length > q.limit,
        evaluatedAt: await commerceClock(client),
      }
    })
  }
  orders(tenant: string, principal: CommercePrincipal, raw: OrderPageQuery) {
    const q = orderPageSchema.parse(raw)
    return this.transaction(tenant, principal, 'orders.read', async (client, access) => {
      const observed = await commerceClock(client),
        rows = (
          await client.query<CommerceRow>(
            'SELECT * FROM archava_commerce.orders WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) AND ($4::uuid IS NULL OR id>$4) AND ($5::text IS NULL OR state=$5) ORDER BY id LIMIT $6',
            [
              tenant,
              access.scope === 'all',
              access.ownerId,
              q.after ?? null,
              q.state ?? null,
              q.limit + 1,
            ],
          )
        ).rows,
        items = rows.slice(0, q.limit).map((x) => order(x, observed))
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  detail(tenant: string, principal: CommercePrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(tenant, principal, 'orders.read', async (client, access) => {
      const row = await this.ownedOrder(client, tenant, access, id, true),
        observed = await commerceClock(client)
      const lines = (
        await client.query<CommerceRow>(
          'SELECT * FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id',
          [tenant, id],
        )
      ).rows.map(line)
      if (lines.length < 1 || lines.length > 50) throw new Error('Invalid stored order line count')
      const auditRows = access.permissions.includes('audit.read')
        ? (
            await client.query<CommerceRow>(
              'SELECT * FROM archava_commerce.audit WHERE tenant_id=$1 AND record_id=$2 ORDER BY occurred_at DESC,request_id DESC LIMIT 51',
              [tenant, id],
            )
          ).rows
        : null
      const returnRows = access.permissions.includes('audit.read')
        ? (
            await client.query<CommerceRow>(
              'SELECT * FROM archava_commerce.returns WHERE tenant_id=$1 AND order_id=$2 ORDER BY occurred_at DESC,request_id DESC,line_id LIMIT 51',
              [tenant, id],
            )
          ).rows
        : null
      return {
        order: order(row, observed),
        lines,
        returns:
          returnRows?.slice(0, 50).map((x) =>
            orderReturnSchema.parse({
              requestId: x.request_id,
              lineId: x.line_id,
              quantity: x.quantity,
              disposition: x.disposition,
              reason: x.reason,
              occurredAt: commerceInstant(x.occurred_at),
            }),
          ) ?? null,
        returnsHasMore: (returnRows?.length ?? 0) > 50,
        audit:
          auditRows?.slice(0, 50).map((x) =>
            commerceAuditSchema.parse({
              requestId: x.request_id,
              actorId: x.actor_id,
              operation: x.operation,
              recordId: x.record_id,
              revision: x.revision,
              reason: x.reason,
              occurredAt: commerceInstant(x.occurred_at),
            }),
          ) ?? null,
        auditHasMore: (auditRows?.length ?? 0) > 50,
        evaluatedAt: observed,
      }
    })
  }
  private async receiptAccess(
    client: PoolClient,
    tenant: string,
    access: CommerceAccess,
    receipt: CommerceReceipt,
  ) {
    if (receipt.kind === 'order') await this.ownedOrder(client, tenant, access, receipt.id)
    else {
      grant(access, 'catalog.read')
      if (receipt.kind === 'stock') grant(access, 'stock.read')
      commerceProduct(
        (
          await client.query<CommerceRow>(
            'SELECT * FROM archava_commerce.products WHERE tenant_id=$1 AND id=$2',
            [tenant, receipt.id],
          )
        ).rows[0],
      )
    }
  }
  commandStatus(
    tenant: string,
    principal: CommercePrincipal,
    id: string,
  ): Promise<CommerceCommandStatus> {
    if (principal.kind !== 'human') throw new CommerceError('forbidden')
    z.uuid().parse(id)
    return this.transaction(tenant, principal, null, async (client, access) => {
      const row = (
        await client.query<CommerceRow>(
          'SELECT * FROM archava_commerce.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, id],
        )
      ).rows[0]
      if (!row) return { status: 'not_committed', requestId: id }
      if (row.actor_id !== principal.id) throw new CommerceError('forbidden')
      const receipt = commerceReceiptSchema.parse(row.receipt)
      await this.receiptAccess(client, tenant, access, receipt)
      return { status: 'committed', requestId: id, receipt }
    })
  }
  mutate(
    tenant: string,
    principal: CommercePrincipal,
    raw: CommerceMutation,
  ): Promise<CommerceReceipt> {
    if (principal.kind !== 'human') throw new CommerceError('forbidden')
    const mutation = commerceMutationSchema.parse(raw),
      c = mutation.command,
      permission: CommercePermission = c.operation.startsWith('product.')
        ? 'catalog.write'
        : c.operation === 'stock.adjust'
          ? 'stock.write'
          : 'orders.write'
    const fingerprint = createHmac('sha256', this.fingerprintKey)
      .update(canonical({ tenant, principal, command: c }))
      .digest('hex')
    return this.transaction(tenant, principal, permission, async (client, access) => {
      if (c.operation.startsWith('product.') || c.operation === 'stock.adjust') {
        globalAccess(access)
        grant(access, 'catalog.read')
      }
      if (c.operation === 'stock.adjust') grant(access, 'stock.read')
      if (c.operation.startsWith('order.')) {
        grant(access, 'orders.read')
        grant(access, 'catalog.read')
      }
      if (c.operation === 'order.return') {
        grant(access, 'returns.write')
        if (c.lines.some((x) => x.disposition === 'restock')) grant(access, 'stock.write')
      }
      const claimed = await client.query(
        'INSERT INTO archava_commerce.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING RETURNING request_id',
        [tenant, mutation.requestId, principal.id, fingerprint],
      )
      if (!claimed.rows.length) {
        const row = (
          await client.query<CommerceRow>(
            'SELECT * FROM archava_commerce.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, mutation.requestId],
          )
        ).rows[0]
        if (!row || row.actor_id !== principal.id || row.fingerprint !== fingerprint)
          throw new CommerceError('conflict')
        const receipt = commerceReceiptSchema.parse(row.receipt)
        await this.receiptAccess(client, tenant, access, receipt)
        return receipt
      }
      let receipt: CommerceReceipt
      if (c.operation === 'product.create') {
        const id = randomUUID()
        await client.query(
          'INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price) VALUES($1,$2,$3,$4,$5,$6,$7)',
          [tenant, id, c.sku, c.title, c.unit, c.trackStock, c.price],
        )
        receipt = { kind: 'product', id, revision: 1 }
      } else if (c.operation === 'product.set-price' || c.operation === 'product.set-active') {
        const p = (await lockCommerceProducts(client, tenant, [c.id]))[0]
        if (
          !p ||
          p.revision !== c.expectedRevision ||
          p.revision >= 2147483647 ||
          (c.operation === 'product.set-price' ? p.price === c.price : p.active === c.active)
        )
          throw new CommerceError('conflict')
        if (c.operation === 'product.set-price')
          await client.query(
            'UPDATE archava_commerce.products SET price=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
            [tenant, p.id, c.price],
          )
        else
          await client.query(
            'UPDATE archava_commerce.products SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
            [tenant, p.id, c.active],
          )
        receipt = { kind: 'product', id: p.id, revision: p.revision + 1 }
      } else if (c.operation === 'stock.adjust') {
        const p = (await lockCommerceProducts(client, tenant, [c.productId]))[0]
        if (!p?.trackStock) throw new CommerceError('conflict')
        const stock = await readCommerceStock(client, tenant, p.id, await commerceClock(client))
        if (
          stock.revision !== c.expectedRevision ||
          stock.revision >= 2147483647 ||
          stock.onHand + c.quantity < stock.reserved ||
          stock.onHand + c.quantity > 1000000000
        )
          throw new CommerceError('conflict')
        await client.query(
          "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason) VALUES($1,$2,$3,'adjust',$4,$5)",
          [tenant, mutation.requestId, p.id, c.quantity, c.reason],
        )
        receipt = { kind: 'stock', id: p.id, revision: stock.revision + 1 }
      } else if (c.operation === 'order.create') {
        const created = await createCommerceDraft(client, {
          tenant,
          ownerId: access.ownerId,
          customerName: c.customerName,
          lines: c.lines,
        })
        receipt = { kind: 'order', id: created.id, revision: 1 }
      } else {
        const row = await this.ownedOrder(client, tenant, access, c.id, true)
        if (row.managed_by !== null) throw new CommerceError('conflict')
        const item = order(row, await commerceClock(client))
        if (item.revision !== c.expectedRevision || item.revision >= 2147483647)
          throw new CommerceError('conflict')
        if (c.operation === 'order.replace-lines') {
          if (item.state !== 'draft') throw new CommerceError('conflict')
          const lines = await quoteCommerceLines(client, tenant, c.lines),
            total = lines.reduce((sum, l) => sum + l.price * l.quantity, 0)
          await client.query(
            'DELETE FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2',
            [tenant, item.id],
          )
          await insertCommerceLines(client, tenant, item.id, lines)
          await client.query(
            'UPDATE archava_commerce.orders SET total_estimate=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
            [tenant, item.id, total],
          )
        } else if (c.operation === 'order.reserve')
          await reserveCommerceOrder(client, {
            tenant,
            id: item.id,
            ownerModule: 'commerce',
            holdSeconds: c.holdSeconds,
          })
        else if (c.operation === 'order.fulfill')
          await fulfillCommerceOrder(client, {
            tenant,
            id: item.id,
            ownerModule: 'commerce',
            requestId: mutation.requestId,
          })
        else if (c.operation === 'order.cancel')
          await cancelCommerceOrder(client, { tenant, id: item.id, ownerModule: 'commerce' })
        else {
          if (item.state !== 'fulfilled') throw new CommerceError('conflict')
          const lines = (
            await client.query<CommerceRow>(
              'SELECT * FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id',
              [tenant, item.id],
            )
          ).rows.map(line)
          const changed = c.lines.map((r) => {
            const sold = lines.find((x) => x.id === r.lineId)
            if (!sold || r.quantity > sold.quantity - sold.returnedQuantity)
              throw new CommerceError('conflict')
            return { ...r, sold }
          })
          await lockCommerceProducts(
            client,
            tenant,
            changed.map((x) => x.sold.productId),
          )
          for (const r of changed)
            await client.query(
              'INSERT INTO archava_commerce.returns(tenant_id,request_id,order_id,line_id,quantity,disposition,reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
              [tenant, mutation.requestId, item.id, r.lineId, r.quantity, r.disposition, c.reason],
            )
          for (const r of changed.filter((x) => x.disposition === 'restock' && x.sold.trackStock))
            await client.query(
              "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,order_id,kind,quantity,reason) VALUES($1,$2,$3,$4,'return',$5,$6)",
              [tenant, mutation.requestId, r.sold.productId, item.id, r.quantity, c.reason],
            )
          const complete = lines.every(
            (l) =>
              l.returnedQuantity + (changed.find((x) => x.lineId === l.id)?.quantity ?? 0) ===
              l.quantity,
          )
          await client.query(
            'UPDATE archava_commerce.orders SET state=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
            [tenant, item.id, complete ? 'returned' : 'fulfilled'],
          )
        }
        receipt = { kind: 'order', id: item.id, revision: item.revision + 1 }
      }
      await client.query(
        'INSERT INTO archava_commerce.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [
          tenant,
          mutation.requestId,
          principal.id,
          c.operation,
          receipt.id,
          receipt.revision,
          'reason' in c ? c.reason : null,
        ],
      )
      await client.query(
        'UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, mutation.requestId, receipt],
      )
      return receipt
    })
  }
}
