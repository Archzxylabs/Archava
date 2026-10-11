import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  PurchaseError,
  purchaseIdSchema,
  purchasePrincipalSchema,
  purchaseMembershipSchema,
  purchaseAccessSchema,
  purchasePageSchema,
  purchaseOrderPageSchema,
  purchaseMutationSchema,
  purchaseReceiptSchema,
  supplierSchema,
  offerSchema,
  purchaseOrderSchema,
  purchaseLineSchema,
  purchaseDetailSchema,
  goodsReceiptSchema,
  purchaseAuditSchema,
  purchaseLinesInputSchema,
  type PurchaseRepository,
  type PurchasePrincipal,
  type PurchaseAccess,
  type PurchasePermission,
  type PurchasePageQuery,
  type PurchaseOrderPageQuery,
  type PurchaseMutation,
  type PurchaseCommand,
  type PurchaseCommandStatus,
} from '@archava/purchasing'
import {
  CommerceError,
  commerceMembershipSchema,
  type CommerceAccess,
  type CommercePermission,
} from '@archava/commerce'
import {
  assertCommerceKernel,
  commerceInstant,
  lockCommerceProducts,
  receivePurchaseStock,
  type CommerceRow,
} from '@archava/commerce-pg/kernel'
import { purchasingMigrationLedger } from './migrate.js'
type Row = CommerceRow
type Context = PurchaseAccess & { commerce: CommerceAccess }
const canonical = (v: unknown): string =>
  Array.isArray(v)
    ? '[' + v.map(canonical).join(',') + ']'
    : v !== null && typeof v === 'object'
      ? '{' +
        Object.entries(v)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([k, x]) => JSON.stringify(k) + ':' + canonical(x))
          .join(',') +
        '}'
      : JSON.stringify(v)
const grant = (a: PurchaseAccess, ...p: PurchasePermission[]) => {
  if (p.some((x) => !a.permissions.includes(x))) throw new PurchaseError('forbidden')
}
const commerceGrant = (a: Context, ...p: CommercePermission[]) => {
  if (p.some((x) => !a.commerce.permissions.includes(x))) throw new PurchaseError('forbidden')
}
const global = (a: Context) => {
  if (a.scope !== 'all' || a.commerce.scope !== 'all') throw new PurchaseError('forbidden')
}
function authorize(a: Context, c: PurchaseCommand) {
  if (c.operation.startsWith('supplier.')) {
    grant(a, 'suppliers.read', 'suppliers.write')
    global(a)
  } else if (c.operation.startsWith('offer.')) {
    grant(a, 'offers.read', 'offers.write', 'suppliers.read')
    global(a)
  } else {
    grant(a, 'orders.read', 'orders.write')
    if (
      c.operation === 'order.create' ||
      c.operation === 'order.replace-lines' ||
      c.operation === 'order.confirm'
    )
      grant(a, 'suppliers.read', 'offers.read')
    if (c.operation === 'order.receive') {
      grant(a, 'receipts.read', 'receipts.write')
      commerceGrant(a, 'stock.read', 'stock.write')
      if (a.commerce.scope !== 'all') throw new PurchaseError('forbidden')
    }
  }
}
const supplier = (r: Row) =>
  supplierSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    code: r.code,
    name: r.name,
    active: r.active,
    revision: r.revision,
    createdAt: commerceInstant(r.created_at),
    updatedAt: commerceInstant(r.updated_at),
  })
const offer = (r: Row) =>
  offerSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    supplierId: r.supplier_id,
    productId: r.product_id,
    sku: r.sku,
    title: r.title,
    unit: r.unit,
    unitCost: r.unit_cost,
    currency: r.currency,
    active: r.active,
    revision: r.revision,
    createdAt: commerceInstant(r.created_at),
    updatedAt: commerceInstant(r.updated_at),
  })
const order = (r: Row) =>
  purchaseOrderSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    ownerId: r.owner_id,
    supplierId: r.supplier_id,
    supplierCode: r.supplier_code,
    supplierName: r.supplier_name,
    state: r.state,
    totalEstimate: Number(r.total_estimate),
    currency: r.currency,
    confirmedAt: r.confirmed_at === null ? null : commerceInstant(r.confirmed_at),
    revision: r.revision,
    createdAt: commerceInstant(r.created_at),
    updatedAt: commerceInstant(r.updated_at),
  })
const line = (r: Row) =>
  purchaseLineSchema.parse({
    id: r.id,
    offerId: r.offer_id,
    offerRevision: r.offer_revision,
    productId: r.product_id,
    sku: r.sku,
    title: r.title,
    unit: r.unit,
    quantity: r.quantity,
    unitCost: r.unit_cost,
    acceptedQuantity: r.accepted_quantity,
    rejectedQuantity: r.rejected_quantity,
  })
const offerSelect =
  'SELECT o.*,p.sku,p.title,p.unit FROM archava_purchasing.offers o JOIN archava_commerce.products p ON p.tenant_id=o.tenant_id AND p.id=o.product_id'
export class PostgresPurchaseRepository implements PurchaseRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}
  static async create(pool: Pool, key: string) {
    if (key.length < 32) throw new Error('Purchasing requires private command fingerprint key')
    await assertCommerceKernel(pool)
    const unsafe = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_purchasing'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]?.unsafe
    if (unsafe !== false) throw new Error('Purchasing requires non-owner runtime')
    const expected = await purchasingMigrationLedger(),
      actual = (
        await pool.query<{ id: string; checksum: string }>(
          'SELECT id,checksum FROM archava_purchasing.schema_migrations ORDER BY id',
        )
      ).rows
    if (
      actual.length !== expected.length ||
      actual.some((v, i) => v.id !== expected[i]?.id || v.checksum !== expected[i]?.checksum)
    )
      throw new Error('Unsupported purchasing ledger')
    const policies = (
      await pool.query<{ n: number }>(
        "SELECT count(*)::integer AS n FROM pg_class WHERE relnamespace='archava_purchasing'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [
          [
            'memberships',
            'suppliers',
            'offers',
            'orders',
            'lines',
            'receipts',
            'receipt_lines',
            'commands',
            'audit',
          ],
        ],
      )
    ).rows[0]?.n
    if (policies !== 9) throw new Error('Purchasing tenant policies must be forced')
    return new PostgresPurchaseRepository(pool, key)
  }
  private async transaction<T>(
    tenant: string,
    principal: PurchasePrincipal,
    work: (c: PoolClient, a: Context) => Promise<T>,
  ): Promise<T> {
    purchaseIdSchema.parse(tenant)
    purchasePrincipalSchema.parse(principal)
    const c = await this.pool.connect()
    try {
      await c.query('BEGIN')
      await c.query("SET LOCAL statement_timeout='10s'")
      await c.query("SET LOCAL lock_timeout='3s'")
      await c.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string, d: 'purchasing' | 'commerce') => {
        const r = (
          await c.query<Row>(`SELECT * FROM archava_${d}.lock_membership($1,$2)`, [tenant, id])
        ).rows[0]
        if (!r) throw new PurchaseError('forbidden')
        const m = (d === 'purchasing' ? purchaseMembershipSchema : commerceMembershipSchema).parse({
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        })
        if (!m.active) throw new PurchaseError('forbidden')
        return m
      }
      const p = purchaseMembershipSchema.parse(await member(principal.id, 'purchasing')),
        b = commerceMembershipSchema.parse(await member(principal.id, 'commerce'))
      if (p.kind !== principal.kind || b.kind !== principal.kind)
        throw new PurchaseError('forbidden')
      let a: Context = {
        permissions: p.permissions,
        ownerId: p.principalId,
        scope: p.scope,
        commerce: { permissions: b.permissions, ownerId: b.principalId, scope: b.scope },
      }
      if (principal.kind === 'service') {
        const hp = purchaseMembershipSchema.parse(await member(principal.onBehalfOf, 'purchasing')),
          hb = commerceMembershipSchema.parse(await member(principal.onBehalfOf, 'commerce'))
        if (hp.kind !== 'human' || hb.kind !== 'human') throw new PurchaseError('forbidden')
        a = {
          permissions: p.permissions.filter((x) => hp.permissions.includes(x)),
          ownerId: hp.principalId,
          scope: p.scope === 'assigned' || hp.scope === 'assigned' ? 'assigned' : 'all',
          commerce: {
            permissions: b.permissions.filter((x) => hb.permissions.includes(x)),
            ownerId: hb.principalId,
            scope: b.scope === 'assigned' || hb.scope === 'assigned' ? 'assigned' : 'all',
          },
        }
      }
      if (a.commerce.scope === 'assigned') a.scope = 'assigned'
      commerceGrant(a, 'catalog.read')
      const v = await work(c, a)
      await c.query('COMMIT')
      return v
    } catch (e) {
      await c.query('ROLLBACK')
      if (e instanceof CommerceError) throw new PurchaseError(e.code)
      if (
        e &&
        typeof e === 'object' &&
        'code' in e &&
        ['23514', '23503', '23505', '22003', '40P01', '55P03'].includes(String(e.code))
      )
        throw new PurchaseError('conflict')
      throw e
    } finally {
      c.release()
    }
  }
  access(t: string, p: PurchasePrincipal) {
    return this.transaction(t, p, (_c, a) =>
      Promise.resolve(
        purchaseAccessSchema.parse({
          permissions: a.permissions,
          ownerId: a.ownerId,
          scope: a.scope,
        }),
      ),
    )
  }
  suppliers(t: string, p: PurchasePrincipal, raw: PurchasePageQuery) {
    const q = purchasePageSchema.parse(raw)
    return this.transaction(t, p, async (c, a) => {
      grant(a, 'suppliers.read')
      const rows = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND ($2::uuid IS NULL OR id>$2) AND ($3::boolean IS NULL OR active=$3) AND ($4::uuid IS NULL OR id=$4) ORDER BY id LIMIT $5',
          [t, q.after ?? null, q.active ?? null, q.supplierId ?? null, q.limit + 1],
        )
      ).rows
      return {
        items: rows.slice(0, q.limit).map(supplier),
        next: rows.length > q.limit ? String(rows[q.limit - 1]?.id) : null,
      }
    })
  }
  offers(t: string, p: PurchasePrincipal, raw: PurchasePageQuery) {
    const q = purchasePageSchema.parse(raw)
    return this.transaction(t, p, async (c, a) => {
      grant(a, 'offers.read')
      const rows = (
        await c.query<Row>(
          offerSelect +
            ' WHERE o.tenant_id=$1 AND ($2::uuid IS NULL OR o.id>$2) AND ($3::boolean IS NULL OR o.active=$3) AND ($4::uuid IS NULL OR o.supplier_id=$4) ORDER BY o.id LIMIT $5',
          [t, q.after ?? null, q.active ?? null, q.supplierId ?? null, q.limit + 1],
        )
      ).rows
      return {
        items: rows.slice(0, q.limit).map(offer),
        next: rows.length > q.limit ? String(rows[q.limit - 1]?.id) : null,
      }
    })
  }
  orders(t: string, p: PurchasePrincipal, raw: PurchaseOrderPageQuery) {
    const q = purchaseOrderPageSchema.parse(raw)
    return this.transaction(t, p, async (c, a) => {
      grant(a, 'orders.read')
      const rows = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.orders WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3) AND ($4::uuid IS NULL OR id>$4) AND ($5::text IS NULL OR state=$5) AND ($6::uuid IS NULL OR supplier_id=$6) ORDER BY id LIMIT $7',
          [
            t,
            a.scope === 'all',
            a.ownerId,
            q.after ?? null,
            q.state ?? null,
            q.supplierId ?? null,
            q.limit + 1,
          ],
        )
      ).rows
      return {
        items: rows.slice(0, q.limit).map(order),
        next: rows.length > q.limit ? String(rows[q.limit - 1]?.id) : null,
      }
    })
  }
  private async ownedOrder(c: PoolClient, t: string, a: Context, id: string, lock = false) {
    grant(a, 'orders.read')
    const r = (
      await c.query<Row>(
        `SELECT * FROM archava_purchasing.orders WHERE tenant_id=$1 AND id=$2 AND ($3::boolean OR owner_id=$4) FOR ${lock ? 'UPDATE' : 'SHARE'}`,
        [t, id, a.scope === 'all', a.ownerId],
      )
    ).rows[0]
    if (!r) throw new PurchaseError('not_found')
    return r
  }
  detail(t: string, p: PurchasePrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(t, p, async (c, a) => {
      const r = await this.ownedOrder(c, t, a, id),
        lines = (
          await c.query<Row>(
            'SELECT * FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id',
            [t, id],
          )
        ).rows
      let receipts: Row[] | null = null,
        audit: Row[] | null = null
      if (a.permissions.includes('receipts.read'))
        receipts = (
          await c.query<Row>(
            'SELECT l.*,r.actor_id,r.reason,r.occurred_at FROM archava_purchasing.receipt_lines l JOIN archava_purchasing.receipts r ON r.tenant_id=l.tenant_id AND r.request_id=l.request_id WHERE l.tenant_id=$1 AND l.order_id=$2 ORDER BY r.occurred_at DESC,l.request_id DESC,l.line_id LIMIT 101',
            [t, id],
          )
        ).rows
      if (a.permissions.includes('audit.read'))
        audit = (
          await c.query<Row>(
            'SELECT * FROM archava_purchasing.audit WHERE tenant_id=$1 AND record_id=$2 ORDER BY occurred_at DESC,request_id DESC LIMIT 101',
            [t, id],
          )
        ).rows
      return purchaseDetailSchema.parse({
        order: order(r),
        lines: lines.map(line),
        receipts:
          receipts?.slice(0, 100).map((x) =>
            goodsReceiptSchema.parse({
              requestId: x.request_id,
              orderId: x.order_id,
              lineId: x.line_id,
              productId: x.product_id,
              accepted: x.accepted,
              rejected: x.rejected,
              actorId: x.actor_id,
              reason: x.reason,
              occurredAt: commerceInstant(x.occurred_at),
            }),
          ) ?? null,
        receiptsHasMore: (receipts?.length ?? 0) > 100,
        audit:
          audit?.slice(0, 100).map((x) =>
            purchaseAuditSchema.parse({
              requestId: x.request_id,
              actorId: x.actor_id,
              operation: x.operation,
              recordId: x.record_id,
              revision: x.revision,
              reason: x.reason,
              occurredAt: commerceInstant(x.occurred_at),
            }),
          ) ?? null,
        auditHasMore: (audit?.length ?? 0) > 100,
      })
    })
  }
  private async recovery(c: PoolClient, t: string, p: PurchasePrincipal, a: Context, r: Row) {
    if (r.actor_id !== p.id || !r.receipt) throw new PurchaseError('conflict')
    const receipt = purchaseReceiptSchema.parse(r.receipt),
      op = receipt.operation
    if (op.startsWith('supplier.')) {
      grant(a, 'suppliers.read', 'suppliers.write')
      global(a)
    } else if (op.startsWith('offer.')) {
      grant(a, 'offers.read', 'offers.write', 'suppliers.read')
      global(a)
    } else {
      grant(a, 'orders.read', 'orders.write')
      if (op === 'order.create' || op === 'order.replace-lines' || op === 'order.confirm')
        grant(a, 'suppliers.read', 'offers.read')
      if (op === 'order.receive') {
        grant(a, 'receipts.read', 'receipts.write')
        commerceGrant(a, 'stock.read', 'stock.write')
        if (a.commerce.scope !== 'all') throw new PurchaseError('forbidden')
      }
      await this.ownedOrder(c, t, a, receipt.id)
    }
    return purchaseReceiptSchema.parse({ ...receipt, replayed: true })
  }
  commandStatus(t: string, p: PurchasePrincipal, id: string): Promise<PurchaseCommandStatus> {
    z.uuid().parse(id)
    if (p.kind !== 'human') throw new PurchaseError('forbidden')
    return this.transaction(t, p, async (c, a) => {
      const r = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.commands WHERE tenant_id=$1 AND request_id=$2',
          [t, id],
        )
      ).rows[0]
      return r
        ? { state: 'committed', receipt: await this.recovery(c, t, p, a, r) }
        : { state: 'unknown' }
    })
  }
  mutate(t: string, p: PurchasePrincipal, raw: PurchaseMutation) {
    const input = purchaseMutationSchema.parse(raw)
    if (p.kind !== 'human') throw new PurchaseError('forbidden')
    return this.transaction(t, p, async (c, a) => {
      authorize(a, input.command)
      const fingerprint = createHmac('sha256', this.key)
          .update(
            canonical({ domain: 'purchasing', tenant: t, principal: p, command: input.command }),
          )
          .digest('hex'),
        claimed = await c.query(
          'INSERT INTO archava_purchasing.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING',
          [t, input.requestId, p.id, fingerprint],
        )
      if (claimed.rowCount === 0) {
        const r = (
          await c.query<Row>(
            'SELECT * FROM archava_purchasing.commands WHERE tenant_id=$1 AND request_id=$2',
            [t, input.requestId],
          )
        ).rows[0]
        if (!r || r.fingerprint !== fingerprint) throw new PurchaseError('conflict')
        return this.recovery(c, t, p, a, r)
      }
      const changed = await this.execute(c, t, p, a, input, fingerprint),
        receipt = purchaseReceiptSchema.parse({
          requestId: input.requestId,
          operation: input.command.operation,
          id: changed.id,
          revision: changed.revision,
          replayed: false,
        })
      await c.query(
        'INSERT INTO archava_purchasing.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [
          t,
          input.requestId,
          p.id,
          receipt.operation,
          receipt.id,
          receipt.revision,
          'reason' in input.command ? input.command.reason : null,
        ],
      )
      await c.query(
        'UPDATE archava_purchasing.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [t, input.requestId, receipt],
      )
      return receipt
    })
  }
  private async quote(c: PoolClient, t: string, supplierId: string, raw: unknown) {
    const input = purchaseLinesInputSchema.parse(raw),
      s = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR SHARE',
          [t, supplierId],
        )
      ).rows[0]
    if (!s || s.active !== true) throw new PurchaseError('conflict')
    const offers = (
      await c.query<Row>(
        offerSelect +
          ' WHERE o.tenant_id=$1 AND o.supplier_id=$2 AND o.id=ANY($3::uuid[]) ORDER BY o.id FOR SHARE OF o',
        [t, supplierId, input.map((x) => x.offerId)],
      )
    ).rows
    if (offers.length !== input.length || offers.some((x) => x.active !== true))
      throw new PurchaseError('conflict')
    const products = await lockCommerceProducts(
      c,
      t,
      offers.map((x) => String(x.product_id)),
    )
    if (products.some((x) => !x.active || !x.trackStock)) throw new PurchaseError('conflict')
    const lines = input.map((x) => {
      const o = offers.find((r) => r.id === x.offerId),
        product = products.find((p) => p.id === o?.product_id)
      if (!o || !product) throw new PurchaseError('conflict')
      return {
        id: randomUUID(),
        offerId: x.offerId,
        offerRevision: z.number().int().positive().parse(o.revision),
        productId: product.id,
        sku: product.sku,
        title: product.title,
        unit: product.unit,
        quantity: x.quantity,
        unitCost: z.number().int().min(0).max(1000000000).parse(o.unit_cost),
      }
    })
    const total = lines.reduce((v, x) => v + x.quantity * x.unitCost, 0)
    if (!Number.isSafeInteger(total) || total > 500000000000000) throw new PurchaseError('conflict')
    return { supplier: s, lines, total }
  }
  private async writeLines(
    c: PoolClient,
    t: string,
    id: string,
    lines: Awaited<ReturnType<PostgresPurchaseRepository['quote']>>['lines'],
  ) {
    for (const l of lines)
      await c.query(
        'INSERT INTO archava_purchasing.lines(tenant_id,id,order_id,offer_id,offer_revision,product_id,sku,title,unit,quantity,unit_cost) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)',
        [
          t,
          l.id,
          id,
          l.offerId,
          l.offerRevision,
          l.productId,
          l.sku,
          l.title,
          l.unit,
          l.quantity,
          l.unitCost,
        ],
      )
  }
  private async execute(
    c: PoolClient,
    t: string,
    p: PurchasePrincipal,
    a: Context,
    input: PurchaseMutation,
    fingerprint: string,
  ): Promise<{ id: string; revision: number }> {
    const cmd = input.command
    if (cmd.operation === 'supplier.create') {
      const id = randomUUID()
      await c.query(
        'INSERT INTO archava_purchasing.suppliers(tenant_id,id,code,name) VALUES($1,$2,$3,$4)',
        [t, id, cmd.code, cmd.name],
      )
      return { id, revision: 1 }
    }
    if (cmd.operation === 'supplier.set-active') {
      const r = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
          [t, cmd.id],
        )
      ).rows[0]
      if (!r) throw new PurchaseError('not_found')
      if (
        r.revision !== cmd.expectedRevision ||
        r.active === cmd.active ||
        cmd.expectedRevision >= 2147483647
      )
        throw new PurchaseError('conflict')
      await c.query(
        'UPDATE archava_purchasing.suppliers SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
        [t, cmd.id, cmd.active],
      )
      return { id: cmd.id, revision: cmd.expectedRevision + 1 }
    }
    if (cmd.operation === 'offer.create') {
      const s = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.suppliers WHERE tenant_id=$1 AND id=$2 FOR SHARE',
          [t, cmd.supplierId],
        )
      ).rows[0]
      if (!s || s.active !== true) throw new PurchaseError('conflict')
      const product = (await lockCommerceProducts(c, t, [cmd.productId]))[0]
      if (!product?.active || !product.trackStock) throw new PurchaseError('conflict')
      const id = randomUUID()
      await c.query(
        'INSERT INTO archava_purchasing.offers(tenant_id,id,supplier_id,product_id,unit_cost) VALUES($1,$2,$3,$4,$5)',
        [t, id, cmd.supplierId, cmd.productId, cmd.unitCost],
      )
      return { id, revision: 1 }
    }
    if (cmd.operation === 'offer.set-cost' || cmd.operation === 'offer.set-active') {
      const r = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.offers WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
          [t, cmd.id],
        )
      ).rows[0]
      if (!r) throw new PurchaseError('not_found')
      if (
        r.revision !== cmd.expectedRevision ||
        cmd.expectedRevision >= 2147483647 ||
        (cmd.operation === 'offer.set-cost'
          ? r.unit_cost === cmd.unitCost
          : r.active === cmd.active)
      )
        throw new PurchaseError('conflict')
      await c.query(
        `UPDATE archava_purchasing.offers SET ${cmd.operation === 'offer.set-cost' ? 'unit_cost' : 'active'}=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2`,
        [t, cmd.id, cmd.operation === 'offer.set-cost' ? cmd.unitCost : cmd.active],
      )
      return { id: cmd.id, revision: cmd.expectedRevision + 1 }
    }
    if (cmd.operation === 'order.create') {
      const q = await this.quote(c, t, cmd.supplierId, cmd.lines),
        id = randomUUID()
      await c.query(
        'INSERT INTO archava_purchasing.orders(tenant_id,id,owner_id,supplier_id,supplier_code,supplier_name,total_estimate) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [t, id, a.ownerId, cmd.supplierId, q.supplier.code, q.supplier.name, q.total],
      )
      await this.writeLines(c, t, id, q.lines)
      return { id, revision: 1 }
    }
    const r = await this.ownedOrder(c, t, a, cmd.id, true),
      o = order(r)
    if (o.revision !== cmd.expectedRevision || o.revision >= 2147483647)
      throw new PurchaseError('conflict')
    if (cmd.operation === 'order.replace-lines') {
      if (o.state !== 'draft') throw new PurchaseError('conflict')
      const q = await this.quote(c, t, o.supplierId, cmd.lines)
      await c.query('DELETE FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2', [
        t,
        o.id,
      ])
      await this.writeLines(c, t, o.id, q.lines)
      await c.query(
        'UPDATE archava_purchasing.orders SET total_estimate=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
        [t, o.id, q.total],
      )
      return { id: o.id, revision: o.revision + 1 }
    }
    let state: string
    if (cmd.operation === 'order.confirm') {
      if (o.state !== 'draft') throw new PurchaseError('conflict')
      const old = (
          await c.query<Row>(
            'SELECT * FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id',
            [t, o.id],
          )
        ).rows.map(line),
        q = await this.quote(
          c,
          t,
          o.supplierId,
          old.map((l) => ({ offerId: l.offerId, quantity: l.quantity })),
        )
      if (
        q.lines.some((l) => {
          const prev = old.find((x) => x.offerId === l.offerId)
          return !prev || prev.offerRevision !== l.offerRevision || prev.unitCost !== l.unitCost
        })
      )
        throw new PurchaseError('conflict')
      state = 'confirmed'
    } else if (cmd.operation === 'order.cancel') {
      if (!['draft', 'confirmed'].includes(o.state)) throw new PurchaseError('conflict')
      state = 'cancelled'
    } else if (cmd.operation === 'order.short-close') {
      if (!['confirmed', 'partial'].includes(o.state)) throw new PurchaseError('conflict')
      state = 'short_closed'
    } else if (cmd.operation === 'order.receive') {
      if (!['confirmed', 'partial'].includes(o.state)) throw new PurchaseError('conflict')
      const lines = (
        await c.query<Row>(
          'SELECT * FROM archava_purchasing.lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY id',
          [t, o.id],
        )
      ).rows.map(line)
      for (const finding of cmd.lines) {
        const l = lines.find((x) => x.id === finding.lineId)
        if (
          !l ||
          l.acceptedQuantity + l.rejectedQuantity + finding.accepted + finding.rejected > l.quantity
        )
          throw new PurchaseError('conflict')
      }
      await c.query(
        'INSERT INTO archava_purchasing.receipts(tenant_id,request_id,order_id,actor_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6)',
        [t, input.requestId, o.id, p.id, o.revision + 1, cmd.reason],
      )
      const stock: { productId: string; quantity: number }[] = []
      for (const finding of cmd.lines) {
        const l = lines.find((x) => x.id === finding.lineId)
        if (!l) throw new PurchaseError('conflict')
        await c.query(
          'INSERT INTO archava_purchasing.receipt_lines(tenant_id,request_id,order_id,line_id,product_id,accepted,rejected) VALUES($1,$2,$3,$4,$5,$6,$7)',
          [t, input.requestId, o.id, l.id, l.productId, finding.accepted, finding.rejected],
        )
        await c.query(
          'UPDATE archava_purchasing.lines SET accepted_quantity=accepted_quantity+$3,rejected_quantity=rejected_quantity+$4 WHERE tenant_id=$1 AND id=$2',
          [t, l.id, finding.accepted, finding.rejected],
        )
        if (finding.accepted > 0) stock.push({ productId: l.productId, quantity: finding.accepted })
        l.acceptedQuantity += finding.accepted
        l.rejectedQuantity += finding.rejected
      }
      await receivePurchaseStock(c, {
        tenant: t,
        requestId: input.requestId,
        actorId: p.id,
        fingerprint,
        id: o.id,
        revision: o.revision + 1,
        reason: cmd.reason,
        lines: stock,
      })
      state = lines.every((l) => l.acceptedQuantity + l.rejectedQuantity === l.quantity)
        ? 'received'
        : 'partial'
    } else throw new PurchaseError('conflict')
    await c.query(
      "UPDATE archava_purchasing.orders SET state=$3,confirmed_at=CASE WHEN $3='confirmed' THEN clock_timestamp() ELSE confirmed_at END,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
      [t, o.id, state],
    )
    return { id: o.id, revision: o.revision + 1 }
  }
}
