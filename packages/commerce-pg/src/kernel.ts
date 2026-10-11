import { randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  CommerceError,
  commerceIdSchema,
  productSchema,
  orderLinesInputSchema,
  commerceStockSchema,
  stockRecordSchema,
  type Product,
} from '@archava/commerce'
import { commerceMigrationLedger } from './migrate.js'

export type CommerceRow = Record<string, unknown>
export const commerceInstant = (value: unknown) => {
  if (!(value instanceof Date) || !Number.isFinite(value.getTime()))
    throw new Error('Invalid commerce timestamp')
  return value.toISOString()
}
export const commerceProduct = (row: CommerceRow | undefined) => {
  if (!row) throw new CommerceError('not_found')
  return productSchema.parse({
    id: row.id,
    tenantId: row.tenant_id,
    sku: row.sku,
    title: row.title,
    unit: row.unit,
    trackStock: row.track_stock,
    price: row.price,
    currency: row.currency,
    active: row.active,
    revision: row.revision,
    createdAt: commerceInstant(row.created_at),
    updatedAt: commerceInstant(row.updated_at),
  })
}
export const commerceClock = async (client: PoolClient) =>
  commerceInstant(
    (await client.query<CommerceRow>('SELECT clock_timestamp() AS observed')).rows[0]?.observed,
  )
export async function assertCommerceKernel(pool: Pool) {
  const role = await pool.query<{ unsafe: boolean }>(
    "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_commerce'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
  )
  if (role.rows[0]?.unsafe !== false) throw new Error('Commerce requires non-owner runtime')
  const expected = await commerceMigrationLedger(),
    actual = (
      await pool.query<{ id: string; checksum: string }>(
        'SELECT id,checksum FROM archava_commerce.schema_migrations ORDER BY id',
      )
    ).rows
  if (
    actual.length !== expected.length ||
    actual.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
  )
    throw new Error('Unsupported commerce ledger')
  const policies = await pool.query<{ count: number }>(
    "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_commerce'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
    [
      [
        'memberships',
        'products',
        'stock_balances',
        'orders',
        'order_lines',
        'allocations',
        'commands',
        'movements',
        'returns',
        'audit',
      ],
    ],
  )
  if (policies.rows[0]?.count !== 10) throw new Error('Commerce tenant policies must be forced')
}
async function context(client: PoolClient, tenant: string) {
  commerceIdSchema.parse(tenant)
  const found = (
    await client.query<{ tenant: string | null }>(
      "SELECT current_setting('archava.tenant_id',true) AS tenant",
    )
  ).rows[0]?.tenant
  if (found !== tenant) throw new Error('Commerce kernel requires caller-owned tenant transaction')
}
/** Private transaction primitives. Identity, current domain authorization and command claims belong to the caller. */
export async function lockCommerceProducts(
  client: PoolClient,
  tenant: string,
  raw: readonly string[],
  ownerModule: 'commerce' | 'foodservice' = 'commerce',
): Promise<readonly Product[]> {
  await context(client, tenant)
  z.enum(['commerce', 'foodservice']).parse(ownerModule)
  const ids = z
    .array(z.uuid())
    .min(1)
    .max(150)
    .parse([...new Set(raw)].sort())
  const rows = (
    await client.query<CommerceRow>(
      'SELECT * FROM archava_commerce.products WHERE tenant_id=$1 AND id=ANY($2::uuid[]) AND (managed_by IS NULL OR managed_by=$3) ORDER BY id FOR UPDATE',
      [tenant, ids, ownerModule === 'foodservice' ? ownerModule : null],
    )
  ).rows
  if (rows.length !== ids.length) throw new CommerceError('not_found')
  return rows.map(commerceProduct)
}
export async function readCommerceStock(
  client: PoolClient,
  tenant: string,
  id: string,
  observed: string,
) {
  await context(client, tenant)
  z.uuid().parse(id)
  z.iso.datetime().parse(observed)
  const row = (
    await client.query<CommerceRow>(
      "SELECT b.*,coalesce((SELECT sum(a.quantity) FROM archava_commerce.allocations a WHERE a.tenant_id=b.tenant_id AND a.product_id=b.product_id AND a.state='held' AND a.expires_at>$3::timestamptz),0) AS reserved FROM archava_commerce.stock_balances b WHERE b.tenant_id=$1 AND b.product_id=$2",
      [tenant, id, observed],
    )
  ).rows[0]
  if (!row) throw new CommerceError('not_found')
  const onHand = commerceStockSchema.parse(row.quantity),
    reserved = commerceStockSchema.parse(Number(row.reserved))
  return stockRecordSchema.parse({
    productId: id,
    onHand,
    reserved,
    available: onHand - reserved,
    revision: row.revision,
    evaluatedAt: observed,
    authority: 'snapshot-only',
  })
}
export async function lockCommerceOrder(
  client: PoolClient,
  tenant: string,
  id: string,
  ownerModule: 'commerce' | 'foodservice',
) {
  z.enum(['commerce', 'foodservice']).parse(ownerModule)
  await context(client, tenant)
  z.uuid().parse(id)
  const row = (
    await client.query<CommerceRow>(
      'SELECT * FROM archava_commerce.orders WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
      [tenant, id],
    )
  ).rows[0]
  if (!row) throw new CommerceError('not_found')
  if (row.managed_by !== (ownerModule === 'commerce' ? null : 'foodservice'))
    throw new CommerceError('conflict')
  return row
}
export async function quoteCommerceLines(
  client: PoolClient,
  tenant: string,
  raw: unknown,
  ownerModule: 'commerce' | 'foodservice' = 'commerce',
) {
  const input = orderLinesInputSchema.parse(raw),
    products = await lockCommerceProducts(
      client,
      tenant,
      input.map((x) => x.productId),
      ownerModule,
    )
  return input.map((line) => {
    const p = products.find((x) => x.id === line.productId)
    if (!p || !p.active) throw new CommerceError('conflict')
    return {
      id: randomUUID(),
      productId: p.id,
      sku: p.sku,
      title: p.title,
      unit: p.unit,
      trackStock: p.trackStock,
      quantity: line.quantity,
      price: p.price,
      productRevision: p.revision,
      returnedQuantity: 0,
    }
  })
}
export async function insertCommerceLines(
  client: PoolClient,
  tenant: string,
  orderId: string,
  lines: Awaited<ReturnType<typeof quoteCommerceLines>>,
) {
  await context(client, tenant)
  for (const l of lines)
    await client.query(
      'INSERT INTO archava_commerce.order_lines(tenant_id,id,order_id,product_id,sku,title,unit,track_stock,quantity,price,product_revision) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)',
      [
        tenant,
        l.id,
        orderId,
        l.productId,
        l.sku,
        l.title,
        l.unit,
        l.trackStock,
        l.quantity,
        l.price,
        l.productRevision,
      ],
    )
}
export async function createCommerceDraft(
  client: PoolClient,
  raw: {
    tenant: string
    ownerId: string
    customerName: string
    lines: unknown
    managedBy?: 'foodservice'
  },
) {
  z.enum(['foodservice']).optional().parse(raw.managedBy)
  await context(client, raw.tenant)
  commerceIdSchema.parse(raw.ownerId)
  z.string().trim().min(1).max(160).parse(raw.customerName)
  const lines = await quoteCommerceLines(
      client,
      raw.tenant,
      raw.lines,
      raw.managedBy ?? 'commerce',
    ),
    id = randomUUID(),
    total = lines.reduce((sum, l) => sum + l.price * l.quantity, 0)
  if (!Number.isSafeInteger(total) || total > 500000000000000) throw new CommerceError('conflict')
  await client.query(
    'INSERT INTO archava_commerce.orders(tenant_id,id,owner_id,customer_name,total_estimate,managed_by) VALUES($1,$2,$3,$4,$5,$6)',
    [raw.tenant, id, raw.ownerId, raw.customerName, total, raw.managedBy ?? null],
  )
  await insertCommerceLines(client, raw.tenant, id, lines)
  return { id, revision: 1, lines, totalEstimate: total }
}
const allocationInput = z
  .array(
    z.object({ productId: z.uuid(), quantity: z.number().int().min(1).max(1000000000) }).strict(),
  )
  .max(100)
  .superRefine((xs, ctx) => {
    if (new Set(xs.map((x) => x.productId)).size !== xs.length)
      ctx.addIssue({ code: 'custom', message: 'Allocation products must be unique' })
  })
export async function reserveCommerceOrder(
  client: PoolClient,
  raw: {
    tenant: string
    id: string
    ownerModule: 'commerce' | 'foodservice'
    holdSeconds: number
    allocations?: unknown
  },
) {
  const o = await lockCommerceOrder(client, raw.tenant, raw.id, raw.ownerModule)
  if (o.state !== 'draft') throw new CommerceError('conflict')
  const holdSeconds = z.number().int().min(1).max(1800).parse(raw.holdSeconds)
  const lines = (
    await client.query<CommerceRow>(
      'SELECT * FROM archava_commerce.order_lines WHERE tenant_id=$1 AND order_id=$2 ORDER BY product_id',
      [raw.tenant, raw.id],
    )
  ).rows
  if (lines.length < 1 || lines.length > 50) throw new CommerceError('conflict')
  if (raw.ownerModule === 'commerce' && raw.allocations !== undefined)
    throw new CommerceError('conflict')
  const allocations = allocationInput.parse(
    raw.ownerModule === 'commerce'
      ? lines
          .filter((x) => x.track_stock)
          .map((x) => ({ productId: x.product_id, quantity: x.quantity }))
      : raw.allocations,
  )
  const products = await lockCommerceProducts(
    client,
    raw.tenant,
    [...lines.map((x) => z.uuid().parse(x.product_id)), ...allocations.map((x) => x.productId)],
    raw.ownerModule,
  )
  if (
    products.some((x) => !x.active) ||
    allocations.some((x) => !products.find((p) => p.id === x.productId)?.trackStock)
  )
    throw new CommerceError('conflict')
  const observed = await commerceClock(client),
    expiresAt = new Date(Date.parse(observed) + holdSeconds * 1000).toISOString()
  for (const a of allocations) {
    const stock = await readCommerceStock(client, raw.tenant, a.productId, observed)
    if (stock.available < a.quantity) throw new CommerceError('conflict')
    await client.query(
      "INSERT INTO archava_commerce.allocations(tenant_id,order_id,product_id,quantity,state,expires_at) VALUES($1,$2,$3,$4,'held',$5)",
      [raw.tenant, raw.id, a.productId, a.quantity, expiresAt],
    )
  }
  await client.query(
    "UPDATE archava_commerce.orders SET state='reserved',expires_at=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
    [raw.tenant, raw.id, expiresAt],
  )
  return { revision: Number(o.revision) + 1, expiresAt }
}
export async function fulfillCommerceOrder(
  client: PoolClient,
  raw: { tenant: string; id: string; ownerModule: 'commerce' | 'foodservice'; requestId: string },
) {
  const o = await lockCommerceOrder(client, raw.tenant, raw.id, raw.ownerModule)
  z.uuid().parse(raw.requestId)
  const allocations = (
    await client.query<CommerceRow>(
      'SELECT * FROM archava_commerce.allocations WHERE tenant_id=$1 AND order_id=$2 ORDER BY product_id',
      [raw.tenant, raw.id],
    )
  ).rows
  if (allocations.length)
    await lockCommerceProducts(
      client,
      raw.tenant,
      allocations.map((x) => z.uuid().parse(x.product_id)),
    )
  const observed = await commerceClock(client)
  if (
    o.state !== 'reserved' ||
    !(o.expires_at instanceof Date) ||
    o.expires_at.getTime() <= Date.parse(observed) ||
    allocations.some((x) => x.state !== 'held')
  )
    throw new CommerceError('conflict')
  for (const a of allocations) {
    await client.query(
      "UPDATE archava_commerce.allocations SET state='consumed' WHERE tenant_id=$1 AND order_id=$2 AND product_id=$3",
      [raw.tenant, raw.id, a.product_id],
    )
    await client.query(
      "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,order_id,kind,quantity) VALUES($1,$2,$3,$4,'consume',$5)",
      [raw.tenant, raw.requestId, a.product_id, raw.id, -Number(a.quantity)],
    )
  }
  await client.query(
    "UPDATE archava_commerce.orders SET state='fulfilled',fulfilled_at=clock_timestamp(),revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
    [raw.tenant, raw.id],
  )
  return { revision: Number(o.revision) + 1 }
}
export async function cancelCommerceOrder(
  client: PoolClient,
  raw: { tenant: string; id: string; ownerModule: 'commerce' | 'foodservice' },
) {
  const o = await lockCommerceOrder(client, raw.tenant, raw.id, raw.ownerModule)
  if (o.state !== 'draft' && o.state !== 'reserved') throw new CommerceError('conflict')
  const allocations = (
    await client.query<CommerceRow>(
      'SELECT * FROM archava_commerce.allocations WHERE tenant_id=$1 AND order_id=$2 ORDER BY product_id',
      [raw.tenant, raw.id],
    )
  ).rows
  if (allocations.length)
    await lockCommerceProducts(
      client,
      raw.tenant,
      allocations.map((x) => z.uuid().parse(x.product_id)),
    )
  await client.query(
    "UPDATE archava_commerce.allocations SET state='released' WHERE tenant_id=$1 AND order_id=$2 AND state='held'",
    [raw.tenant, raw.id],
  )
  await client.query(
    "UPDATE archava_commerce.orders SET state='cancelled',revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2",
    [raw.tenant, raw.id],
  )
  return { revision: Number(o.revision) + 1 }
}

/** Private domain product creation: caller must hold current catalog administration grants. */
export async function createFoodMenuProduct(client: PoolClient, tenant: string, raw: unknown) {
  await context(client, tenant)
  const input = z
    .object({
      sku: z.string().regex(/^[A-Z0-9][A-Z0-9_-]{0,39}$/),
      title: z.string().trim().min(1).max(160),
      price: z.number().int().min(0).max(1000000000),
    })
    .strict()
    .parse(raw)
  const id = randomUUID()
  return commerceProduct(
    (
      await client.query<CommerceRow>(
        "INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,managed_by) VALUES($1,$2,$3,$4,'unit',false,$5,'foodservice') RETURNING *",
        [tenant, id, input.sku, input.title, input.price],
      )
    ).rows[0],
  )
}

/** Private menu policy: no generic API receives this product administration port. */
export async function updateFoodMenuProduct(
  client: PoolClient,
  tenant: string,
  id: string,
  raw: unknown,
) {
  await context(client, tenant)
  z.uuid().parse(id)
  const input = z
    .discriminatedUnion('kind', [
      z
        .object({
          kind: z.literal('price'),
          value: z.number().int().min(0).max(1000000000),
          expectedRevision: z.number().int().positive(),
        })
        .strict(),
      z
        .object({
          kind: z.literal('active'),
          value: z.boolean(),
          expectedRevision: z.number().int().positive(),
        })
        .strict(),
    ])
    .parse(raw)
  const row = (
    await client.query<CommerceRow>(
      "SELECT * FROM archava_commerce.products WHERE tenant_id=$1 AND id=$2 AND managed_by='foodservice' FOR UPDATE",
      [tenant, id],
    )
  ).rows[0]
  if (!row) throw new CommerceError('not_found')
  if (row.revision !== input.expectedRevision) throw new CommerceError('conflict')
  await client.query(
    `UPDATE archava_commerce.products SET ${input.kind === 'price' ? 'price' : 'active'}=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2`,
    [tenant, id, input.value],
  )
}

/** Compose the shared command, consumption and audit with the caller's domain transaction. */
export async function prepareFoodCommerceOrder(
  client: PoolClient,
  raw: { tenant: string; id: string; requestId: string; actorId: string; fingerprint: string },
) {
  await context(client, raw.tenant)
  z.uuid().parse(raw.requestId)
  commerceIdSchema.parse(raw.actorId)
  z.string()
    .regex(/^[a-f0-9]{64}$/)
    .parse(raw.fingerprint)
  const claimed = await client.query(
    'INSERT INTO archava_commerce.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING',
    [raw.tenant, raw.requestId, raw.actorId, raw.fingerprint],
  )
  if (claimed.rowCount !== 1) throw new CommerceError('conflict')
  const changed = await fulfillCommerceOrder(client, {
      tenant: raw.tenant,
      id: raw.id,
      requestId: raw.requestId,
      ownerModule: 'foodservice',
    }),
    receipt = {
      requestId: raw.requestId,
      operation: 'order.fulfill',
      id: raw.id,
      revision: changed.revision,
      replayed: false,
    }
  await client.query(
    'UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
    [raw.tenant, raw.requestId, receipt],
  )
  await client.query(
    'INSERT INTO archava_commerce.audit(tenant_id,request_id,actor_id,operation,record_id,revision) VALUES($1,$2,$3,$4,$5,$6)',
    [raw.tenant, raw.requestId, raw.actorId, 'order.fulfill', raw.id, changed.revision],
  )
  return changed
}

/** Private inbound port; purchasing owns the authorized source receipt in this transaction. */
export async function receivePurchaseStock(
  client: PoolClient,
  raw: {
    tenant: string
    requestId: string
    actorId: string
    fingerprint: string
    id: string
    revision: number
    reason: string
    lines: readonly { productId: string; quantity: number }[]
  },
) {
  await context(client, raw.tenant)
  const input = z
    .object({
      requestId: z.uuid(),
      actorId: commerceIdSchema,
      fingerprint: z.string().regex(/^[a-f0-9]{64}$/),
      id: z.uuid(),
      revision: z.number().int().positive().max(2147483647),
      reason: z.string().trim().min(1).max(2000),
      lines: z
        .array(
          z
            .object({ productId: z.uuid(), quantity: z.number().int().min(1).max(1000000) })
            .strict(),
        )
        .max(50),
    })
    .strict()
    .parse({
      requestId: raw.requestId,
      actorId: raw.actorId,
      fingerprint: raw.fingerprint,
      id: raw.id,
      revision: raw.revision,
      reason: raw.reason,
      lines: raw.lines,
    })
  if (new Set(input.lines.map((x) => x.productId)).size !== input.lines.length)
    throw new CommerceError('conflict')
  const claimed = await client.query(
    'INSERT INTO archava_commerce.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING',
    [raw.tenant, input.requestId, input.actorId, input.fingerprint],
  )
  if (claimed.rowCount !== 1) throw new CommerceError('conflict')
  const products = input.lines.length
    ? await lockCommerceProducts(
        client,
        raw.tenant,
        input.lines.map((x) => x.productId),
      )
    : []
  if (products.some((p) => !p.trackStock)) throw new CommerceError('conflict')
  await client.query(
    'UPDATE archava_commerce.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
    [
      raw.tenant,
      input.requestId,
      { domain: 'purchasing', id: input.id, revision: input.revision, lines: input.lines },
    ],
  )
  for (const p of products) {
    const l = input.lines.find((x) => x.productId === p.id)
    if (!l) throw new CommerceError('conflict')
    const stock = await readCommerceStock(client, raw.tenant, p.id, await commerceClock(client))
    if (stock.onHand + l.quantity > 1000000000 || stock.revision >= 2147483647)
      throw new CommerceError('conflict')
    await client.query(
      "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,reason,purchase_id) VALUES($1,$2,$3,'purchase',$4,$5,$6)",
      [raw.tenant, input.requestId, p.id, l.quantity, input.reason, input.id],
    )
  }
  await client.query(
    'INSERT INTO archava_commerce.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
    [
      raw.tenant,
      input.requestId,
      input.actorId,
      'purchase.receive',
      input.id,
      input.revision,
      input.reason,
    ],
  )
}
