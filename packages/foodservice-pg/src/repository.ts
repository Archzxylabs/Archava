import { createHmac, randomUUID } from 'node:crypto'
import type { Pool, PoolClient } from 'pg'
import { z } from 'zod'
import {
  FoodError,
  foodIdSchema,
  foodPrincipalSchema,
  foodMembershipSchema,
  foodAccessSchema,
  foodPageSchema,
  ticketPageSchema,
  foodMutationSchema,
  foodReceiptSchema,
  foodTableSchema,
  menuItemSchema,
  recipeComponentsSchema,
  ticketSchema,
  ticketLineSchema,
  foodAuditSchema,
  type FoodRepository,
  type FoodPrincipal,
  type FoodAccess,
  type FoodPermission,
  type FoodPageQuery,
  type TicketPageQuery,
  type FoodMutation,
  type FoodCommand,
  type FoodCommandStatus,
} from '@archava/foodservice'
import {
  CommerceError,
  commerceMembershipSchema,
  type CommerceAccess,
  type CommercePermission,
} from '@archava/commerce'
import {
  assertCommerceKernel,
  commerceClock,
  commerceInstant,
  lockCommerceProducts,
  createCommerceDraft,
  createFoodMenuProduct,
  reserveCommerceOrder,
  prepareFoodCommerceOrder,
  updateFoodMenuProduct,
  cancelCommerceOrder,
  type CommerceRow,
} from '@archava/commerce-pg/kernel'
import { foodserviceMigrationLedger } from './migrate.js'
type Row = CommerceRow
type Context = FoodAccess & { commerce: CommerceAccess }
const canonical = (value: unknown): string =>
  Array.isArray(value)
    ? '[' + value.map(canonical).join(',') + ']'
    : value !== null && typeof value === 'object'
      ? '{' +
        Object.entries(value)
          .sort(([a], [b]) => a.localeCompare(b))
          .map(([k, v]) => JSON.stringify(k) + ':' + canonical(v))
          .join(',') +
        '}'
      : JSON.stringify(value)
const grant = (access: FoodAccess, permission: FoodPermission) => {
  if (!access.permissions.includes(permission)) throw new FoodError('forbidden')
}
const commerceGrant = (access: Context, ...permissions: CommercePermission[]) => {
  if (permissions.some((p) => !access.commerce.permissions.includes(p)))
    throw new FoodError('forbidden')
}
const global = (access: Context) => {
  if (access.scope !== 'all' || access.commerce.scope !== 'all') throw new FoodError('forbidden')
}
const table = (r: Row) =>
  foodTableSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    code: r.code,
    title: r.title,
    seats: r.seats,
    active: r.active,
    occupied: r.occupied,
    revision: r.revision,
    createdAt: commerceInstant(r.created_at),
    updatedAt: commerceInstant(r.updated_at),
  })
const menu = (r: Row) =>
  menuItemSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    sku: r.sku,
    title: r.title,
    price: r.price,
    currency: r.currency,
    station: r.station,
    active: r.active,
    recipeVersion: r.recipe_version,
    revision: r.revision,
    createdAt: commerceInstant(r.created_at),
    updatedAt: commerceInstant(r.updated_at),
  })
const ticket = (r: Row, observed: string) =>
  ticketSchema.parse({
    id: r.id,
    tenantId: r.tenant_id,
    ownerId: r.owner_id,
    orderId: r.order_id,
    customerName: r.customer_name,
    mode: r.mode,
    tableId: r.table_id,
    guests: r.guests,
    state: r.state,
    stockExpired:
      r.state === 'queued' &&
      r.expires_at instanceof Date &&
      r.expires_at.getTime() <= Date.parse(observed),
    expiresAt: r.expires_at === null ? null : commerceInstant(r.expires_at),
    totalEstimate: Number(r.total_estimate),
    currency: r.currency,
    revision: r.revision,
    createdAt: commerceInstant(r.created_at),
    updatedAt: commerceInstant(r.updated_at),
  })
const ticketSelect =
  'SELECT t.*,o.customer_name,o.total_estimate,o.currency,o.expires_at FROM archava_foodservice.tickets t JOIN archava_commerce.orders o ON o.tenant_id=t.tenant_id AND o.id=t.order_id'
const menuSelect =
  'SELECT m.*,p.sku,p.title,p.price,p.currency,p.active,p.revision AS product_revision FROM archava_foodservice.menu_items m JOIN archava_commerce.products p ON p.tenant_id=m.tenant_id AND p.id=m.id'
function permission(command: FoodCommand): FoodPermission {
  if (command.operation.startsWith('table.')) return 'tables.write'
  if (command.operation.startsWith('menu.')) return 'menu.write'
  if (command.operation === 'ticket.prepare' || command.operation === 'ticket.ready')
    return 'kitchen.write'
  return 'tickets.write'
}
function authorize(access: Context, command: FoodCommand) {
  grant(access, permission(command))
  if (command.operation.startsWith('table.')) {
    grant(access, 'tables.read')
    global(access)
  } else if (command.operation.startsWith('menu.')) {
    grant(access, 'menu.read')
    global(access)
    commerceGrant(access, 'catalog.write', 'stock.read')
  } else {
    grant(access, 'tickets.read')
    commerceGrant(access, 'orders.write', 'stock.read')
    if (command.operation === 'ticket.create') {
      grant(access, 'menu.read')
      if (command.mode === 'dine_in') grant(access, 'tables.read')
    }
  }
}
export class PostgresFoodRepository implements FoodRepository {
  private constructor(
    private readonly pool: Pool,
    private readonly key: string,
  ) {}
  static async create(pool: Pool, key: string) {
    if (key.length < 32)
      throw new Error('Stable food-service fingerprint key must have at least 32 characters')
    await assertCommerceKernel(pool)
    const role = (
      await pool.query<{ unsafe: boolean }>(
        "SELECT (rolsuper OR rolbypassrls OR EXISTS(SELECT 1 FROM pg_class WHERE relnamespace='archava_foodservice'::regnamespace AND relowner=pg_roles.oid)) AS unsafe FROM pg_roles WHERE rolname=current_user",
      )
    ).rows[0]
    if (role?.unsafe !== false) throw new Error('Food-service requires non-owner runtime')
    const expected = await foodserviceMigrationLedger(),
      actual = (
        await pool.query<{ id: string; checksum: string }>(
          'SELECT id,checksum FROM archava_foodservice.schema_migrations ORDER BY id',
        )
      ).rows
    if (
      actual.length !== expected.length ||
      actual.some((x, i) => x.id !== expected[i]?.id || x.checksum !== expected[i]?.checksum)
    )
      throw new Error('Unsupported food-service ledger')
    const policies = (
      await pool.query<{ count: number }>(
        "SELECT count(*)::integer AS count FROM pg_class WHERE relnamespace='archava_foodservice'::regnamespace AND relname=ANY($1::text[]) AND relrowsecurity AND relforcerowsecurity",
        [
          [
            'memberships',
            'tables',
            'menu_items',
            'recipe_versions',
            'recipe_components',
            'tickets',
            'ticket_lines',
            'commands',
            'audit',
          ],
        ],
      )
    ).rows[0]
    if (policies?.count !== 9) throw new Error('Food-service tenant policies must be forced')
    return new PostgresFoodRepository(pool, key)
  }
  private async transaction<T>(
    tenant: string,
    principal: FoodPrincipal,
    work: (client: PoolClient, access: Context) => Promise<T>,
  ): Promise<T> {
    foodIdSchema.parse(tenant)
    foodPrincipalSchema.parse(principal)
    const client = await this.pool.connect()
    try {
      await client.query('BEGIN')
      await client.query("SET LOCAL statement_timeout='10s'")
      await client.query("SET LOCAL lock_timeout='3s'")
      await client.query("SELECT set_config('archava.tenant_id',$1,true)", [tenant])
      const member = async (id: string, domain: 'foodservice' | 'commerce') => {
        const r = (
          await client.query<Row>(`SELECT * FROM archava_${domain}.lock_membership($1,$2)`, [
            tenant,
            id,
          ])
        ).rows[0]
        if (!r) throw new FoodError('forbidden')
        const shape = {
          principalId: r.principal_id,
          kind: r.kind,
          permissions: r.permissions,
          scope: r.scope,
          active: r.active,
        }
        const m =
          domain === 'foodservice'
            ? foodMembershipSchema.parse(shape)
            : commerceMembershipSchema.parse(shape)
        if (!m.active) throw new FoodError('forbidden')
        return m
      }
      const f = foodMembershipSchema.parse(await member(principal.id, 'foodservice')),
        c = commerceMembershipSchema.parse(await member(principal.id, 'commerce'))
      if (f.kind !== principal.kind || c.kind !== principal.kind) throw new FoodError('forbidden')
      let access: Context = {
        permissions: f.permissions,
        ownerId: f.principalId,
        scope: f.scope,
        commerce: { permissions: c.permissions, ownerId: c.principalId, scope: c.scope },
      }
      if (principal.kind === 'service') {
        const hf = foodMembershipSchema.parse(await member(principal.onBehalfOf, 'foodservice')),
          hc = commerceMembershipSchema.parse(await member(principal.onBehalfOf, 'commerce'))
        if (hf.kind !== 'human' || hc.kind !== 'human') throw new FoodError('forbidden')
        access = {
          permissions: f.permissions.filter((x) => hf.permissions.includes(x)),
          ownerId: hf.principalId,
          scope: f.scope === 'assigned' || hf.scope === 'assigned' ? 'assigned' : 'all',
          commerce: {
            permissions: c.permissions.filter((x) => hc.permissions.includes(x)),
            ownerId: hc.principalId,
            scope: c.scope === 'assigned' || hc.scope === 'assigned' ? 'assigned' : 'all',
          },
        }
      }
      if (access.commerce.scope === 'assigned') access.scope = 'assigned'
      commerceGrant(access, 'catalog.read', 'orders.read')
      const result = await work(client, access)
      await client.query('COMMIT')
      return result
    } catch (error) {
      await client.query('ROLLBACK')
      if (error instanceof CommerceError) throw new FoodError(error.code)
      if (
        error &&
        typeof error === 'object' &&
        'code' in error &&
        ['23514', '23503', '23505', '23P01', '22003', '40P01', '55P03'].includes(String(error.code))
      )
        throw new FoodError('conflict')
      throw error
    } finally {
      client.release()
    }
  }
  private async ownedTicket(
    client: PoolClient,
    tenant: string,
    access: Context,
    id: string,
    lock = false,
  ) {
    grant(access, 'tickets.read')
    const row = (
      await client.query<Row>(
        `${ticketSelect} WHERE t.tenant_id=$1 AND t.id=$2 AND ($3::boolean OR t.owner_id=$4)${lock ? ' FOR UPDATE OF t' : ''}`,
        [tenant, id, access.scope === 'all', access.ownerId],
      )
    ).rows[0]
    if (!row) throw new FoodError('not_found')
    return row
  }
  access(tenant: string, principal: FoodPrincipal) {
    return this.transaction(tenant, principal, (_client, a) =>
      Promise.resolve(
        foodAccessSchema.parse({ permissions: a.permissions, ownerId: a.ownerId, scope: a.scope }),
      ),
    )
  }
  tables(tenant: string, principal: FoodPrincipal, raw: FoodPageQuery) {
    const q = foodPageSchema.parse(raw)
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'tables.read')
      const observed = await commerceClock(client),
        rows = (
          await client.query<Row>(
            "SELECT b.*,EXISTS(SELECT 1 FROM archava_foodservice.tickets t WHERE t.tenant_id=b.tenant_id AND t.table_id=b.id AND t.state NOT IN ('closed','cancelled')) AS occupied FROM archava_foodservice.tables b WHERE b.tenant_id=$1 AND ($2::uuid IS NULL OR b.id>$2) AND ($3::boolean IS NULL OR b.active=$3) ORDER BY b.id LIMIT $4",
            [tenant, q.after ?? null, q.active ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map(table)
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  menu(tenant: string, principal: FoodPrincipal, raw: FoodPageQuery) {
    const q = foodPageSchema.parse(raw)
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'menu.read')
      const observed = await commerceClock(client),
        rows = (
          await client.query<Row>(
            `${menuSelect} WHERE m.tenant_id=$1 AND ($2::uuid IS NULL OR m.id>$2) AND ($3::boolean IS NULL OR p.active=$3) ORDER BY m.id LIMIT $4`,
            [tenant, q.after ?? null, q.active ?? null, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map(menu)
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  menuDetail(tenant: string, principal: FoodPrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'menu.read')
      commerceGrant(a, 'stock.read')
      const r = (
        await client.query<Row>(`${menuSelect} WHERE m.tenant_id=$1 AND m.id=$2`, [tenant, id])
      ).rows[0]
      if (!r) throw new FoodError('not_found')
      return {
        item: menu(r),
        recipe: await this.recipe(client, tenant, id, Number(r.recipe_version)),
      }
    })
  }
  private async recipe(client: PoolClient, tenant: string, id: string, version: number) {
    return recipeComponentsSchema.parse(
      (
        await client.query<Row>(
          'SELECT product_id,quantity FROM archava_foodservice.recipe_components WHERE tenant_id=$1 AND menu_id=$2 AND version=$3 ORDER BY product_id',
          [tenant, id, version],
        )
      ).rows.map((r) => ({ productId: r.product_id, quantity: r.quantity })),
    )
  }
  tickets(tenant: string, principal: FoodPrincipal, raw: TicketPageQuery) {
    const q = ticketPageSchema.parse(raw)
    return this.transaction(tenant, principal, async (client, a) => {
      grant(a, 'tickets.read')
      const observed = await commerceClock(client),
        rows = (
          await client.query<Row>(
            `${ticketSelect} WHERE t.tenant_id=$1 AND ($2::uuid IS NULL OR t.id>$2) AND ($3::text IS NULL OR t.state=$3) AND ($4::boolean OR t.owner_id=$5) ORDER BY t.id LIMIT $6`,
            [tenant, q.after ?? null, q.state ?? null, a.scope === 'all', a.ownerId, q.limit + 1],
          )
        ).rows,
        items = rows.slice(0, q.limit).map((r) => ticket(r, observed))
      return {
        items,
        next: rows.length > q.limit ? (items.at(-1)?.id ?? null) : null,
        evaluatedAt: observed,
      }
    })
  }
  detail(tenant: string, principal: FoodPrincipal, id: string) {
    z.uuid().parse(id)
    return this.transaction(tenant, principal, async (client, a) => {
      const r = await this.ownedTicket(client, tenant, a, id),
        observed = await commerceClock(client),
        rows = (
          await client.query<Row>(
            'SELECT f.*,l.title,l.sku,l.price FROM archava_foodservice.ticket_lines f JOIN archava_commerce.order_lines l ON l.tenant_id=f.tenant_id AND l.id=f.order_line_id WHERE f.tenant_id=$1 AND f.ticket_id=$2 ORDER BY f.menu_id',
            [tenant, id],
          )
        ).rows
      const lines = rows.map((l) =>
        ticketLineSchema.parse({
          menuId: l.menu_id,
          orderLineId: l.order_line_id,
          title: l.title,
          sku: l.sku,
          quantity: l.quantity,
          price: l.price,
          station: l.station,
          recipeVersion: l.recipe_version,
        }),
      )
      const audit = a.permissions.includes('audit.read')
        ? (
            await client.query<Row>(
              'SELECT * FROM archava_foodservice.audit WHERE tenant_id=$1 AND record_id=$2 ORDER BY occurred_at DESC,request_id DESC LIMIT 100',
              [tenant, id],
            )
          ).rows.map((x) =>
            foodAuditSchema.parse({
              requestId: x.request_id,
              actorId: x.actor_id,
              operation: x.operation,
              recordId: x.record_id,
              revision: x.revision,
              reason: x.reason,
              occurredAt: commerceInstant(x.occurred_at),
            }),
          )
        : null
      return { ticket: ticket(r, observed), lines, audit }
    })
  }
  private fingerprint(tenant: string, principal: FoodPrincipal, input: FoodMutation) {
    return createHmac('sha256', this.key)
      .update(canonical({ domain: 'foodservice', tenant, principal, command: input.command }))
      .digest('hex')
  }
  private async recovery(
    client: PoolClient,
    tenant: string,
    principal: FoodPrincipal,
    a: Context,
    row: Row,
  ) {
    if (row.actor_id !== principal.id || !row.receipt) throw new FoodError('conflict')
    const receipt = foodReceiptSchema.parse(row.receipt),
      op = receipt.operation
    if (op.startsWith('table.')) {
      grant(a, 'tables.write')
      grant(a, 'tables.read')
      global(a)
    } else if (op.startsWith('menu.')) {
      grant(a, 'menu.write')
      grant(a, 'menu.read')
      global(a)
      commerceGrant(a, 'catalog.write', 'stock.read')
    } else {
      grant(a, op === 'ticket.prepare' || op === 'ticket.ready' ? 'kitchen.write' : 'tickets.write')
      commerceGrant(a, 'orders.write', 'stock.read')
      const current = await this.ownedTicket(client, tenant, a, receipt.id)
      if (op === 'ticket.create') {
        grant(a, 'menu.read')
        if (current.mode === 'dine_in') grant(a, 'tables.read')
      }
    }
    return foodReceiptSchema.parse({ ...receipt, replayed: true })
  }
  commandStatus(tenant: string, principal: FoodPrincipal, id: string): Promise<FoodCommandStatus> {
    z.uuid().parse(id)
    if (principal.kind !== 'human') throw new FoodError('forbidden')
    return this.transaction(tenant, principal, async (client, a) => {
      const r = (
        await client.query<Row>(
          'SELECT * FROM archava_foodservice.commands WHERE tenant_id=$1 AND request_id=$2',
          [tenant, id],
        )
      ).rows[0]
      if (!r) return { state: 'unknown' }
      return { state: 'committed', receipt: await this.recovery(client, tenant, principal, a, r) }
    })
  }
  mutate(tenant: string, principal: FoodPrincipal, raw: FoodMutation) {
    const input = foodMutationSchema.parse(raw)
    if (principal.kind !== 'human') throw new FoodError('forbidden')
    return this.transaction(tenant, principal, async (client, a) => {
      const c = input.command
      authorize(a, c)
      const fingerprint = this.fingerprint(tenant, principal, input),
        claimed = await client.query(
          'INSERT INTO archava_foodservice.commands(tenant_id,request_id,actor_id,fingerprint) VALUES($1,$2,$3,$4) ON CONFLICT DO NOTHING',
          [tenant, input.requestId, principal.id, fingerprint],
        )
      if (claimed.rowCount === 0) {
        const previous = (
          await client.query<Row>(
            'SELECT * FROM archava_foodservice.commands WHERE tenant_id=$1 AND request_id=$2',
            [tenant, input.requestId],
          )
        ).rows[0]
        if (!previous || previous.fingerprint !== fingerprint) throw new FoodError('conflict')
        return this.recovery(client, tenant, principal, a, previous)
      }
      const changed = await this.execute(client, tenant, principal, a, input),
        receipt = foodReceiptSchema.parse({
          requestId: input.requestId,
          operation: c.operation,
          id: changed.id,
          revision: changed.revision,
          replayed: false,
        })
      await client.query(
        'INSERT INTO archava_foodservice.audit(tenant_id,request_id,actor_id,operation,record_id,revision,reason) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [
          tenant,
          input.requestId,
          principal.id,
          c.operation,
          receipt.id,
          receipt.revision,
          'reason' in c ? c.reason : null,
        ],
      )
      await client.query(
        'UPDATE archava_foodservice.commands SET receipt=$3 WHERE tenant_id=$1 AND request_id=$2',
        [tenant, input.requestId, receipt],
      )
      return receipt
    })
  }
  private async writeRecipe(
    client: PoolClient,
    tenant: string,
    id: string,
    version: number,
    raw: unknown,
  ) {
    const recipe = recipeComponentsSchema.parse(raw),
      products = await lockCommerceProducts(
        client,
        tenant,
        recipe.map((x) => x.productId),
      )
    if (products.some((p) => !p.trackStock || !p.active)) throw new FoodError('conflict')
    await client.query(
      'INSERT INTO archava_foodservice.recipe_versions(tenant_id,menu_id,version) VALUES($1,$2,$3)',
      [tenant, id, version],
    )
    for (const c of recipe)
      await client.query(
        'INSERT INTO archava_foodservice.recipe_components(tenant_id,menu_id,version,product_id,quantity) VALUES($1,$2,$3,$4,$5)',
        [tenant, id, version, c.productId, c.quantity],
      )
    await client.query(
      'UPDATE archava_foodservice.recipe_versions SET sealed=true WHERE tenant_id=$1 AND menu_id=$2 AND version=$3',
      [tenant, id, version],
    )
  }
  private async allocations(client: PoolClient, tenant: string, id: string) {
    const rows = (
      await client.query<Row>(
        'SELECT c.product_id,sum(c.quantity::bigint*l.quantity) AS quantity FROM archava_foodservice.ticket_lines l JOIN archava_foodservice.recipe_components c ON c.tenant_id=l.tenant_id AND c.menu_id=l.menu_id AND c.version=l.recipe_version WHERE l.tenant_id=$1 AND l.ticket_id=$2 GROUP BY c.product_id ORDER BY c.product_id',
        [tenant, id],
      )
    ).rows
    if (rows.length < 1 || rows.length > 100) throw new FoodError('conflict')
    return rows.map((r) => ({
      productId: z.uuid().parse(r.product_id),
      quantity: z.number().int().min(1).max(1000000000).parse(Number(r.quantity)),
    }))
  }
  private async execute(
    client: PoolClient,
    tenant: string,
    principal: FoodPrincipal,
    a: Context,
    input: FoodMutation,
  ): Promise<{ id: string; revision: number }> {
    const c = input.command
    if (c.operation === 'table.create') {
      const id = randomUUID()
      await client.query(
        'INSERT INTO archava_foodservice.tables(tenant_id,id,code,title,seats) VALUES($1,$2,$3,$4,$5)',
        [tenant, id, c.code, c.title, c.seats],
      )
      return { id, revision: 1 }
    }
    if (c.operation === 'table.set-active') {
      const r = (
        await client.query<Row>(
          'SELECT * FROM archava_foodservice.tables WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
          [tenant, c.id],
        )
      ).rows[0]
      if (!r) throw new FoodError('not_found')
      if (r.revision !== c.expectedRevision) throw new FoodError('conflict')
      await client.query(
        'UPDATE archava_foodservice.tables SET active=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
        [tenant, c.id, c.active],
      )
      return { id: c.id, revision: c.expectedRevision + 1 }
    }
    if (c.operation === 'menu.create') {
      const p = await createFoodMenuProduct(client, tenant, {
        sku: c.sku,
        title: c.title,
        price: c.price,
      })
      await client.query(
        'INSERT INTO archava_foodservice.menu_items(tenant_id,id,station) VALUES($1,$2,$3)',
        [tenant, p.id, c.station],
      )
      await this.writeRecipe(client, tenant, p.id, 1, c.recipe)
      return { id: p.id, revision: 1 }
    }
    if (
      c.operation === 'menu.set-price' ||
      c.operation === 'menu.set-active' ||
      c.operation === 'menu.replace-recipe'
    ) {
      const m = (
        await client.query<Row>(`${menuSelect} WHERE m.tenant_id=$1 AND m.id=$2 FOR UPDATE OF m`, [
          tenant,
          c.id,
        ])
      ).rows[0]
      if (!m) throw new FoodError('not_found')
      if (m.revision !== c.expectedRevision) throw new FoodError('conflict')
      let version = Number(m.recipe_version)
      if (c.operation === 'menu.replace-recipe') {
        version++
        await this.writeRecipe(client, tenant, c.id, version, c.recipe)
      } else
        await updateFoodMenuProduct(client, tenant, c.id, {
          kind: c.operation === 'menu.set-price' ? 'price' : 'active',
          value: c.operation === 'menu.set-price' ? c.price : c.active,
          expectedRevision: m.product_revision,
        })
      await client.query(
        'UPDATE archava_foodservice.menu_items SET recipe_version=$3,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
        [tenant, c.id, version],
      )
      return { id: c.id, revision: c.expectedRevision + 1 }
    }
    if (c.operation === 'ticket.create') {
      if (c.tableId) {
        const b = (
          await client.query<Row>(
            'SELECT * FROM archava_foodservice.tables WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
            [tenant, c.tableId],
          )
        ).rows[0]
        if (!b || !b.active || Number(b.seats) < c.guests) throw new FoodError('conflict')
      }
      const ids = c.lines.map((x) => x.menuId).sort(),
        menus = (
          await client.query<Row>(
            `${menuSelect} WHERE m.tenant_id=$1 AND m.id=ANY($2::uuid[]) ORDER BY m.id FOR SHARE OF m`,
            [tenant, ids],
          )
        ).rows
      if (menus.length !== ids.length || menus.some((m) => !m.active))
        throw new FoodError('conflict')
      const draft = await createCommerceDraft(client, {
          tenant,
          ownerId: a.ownerId,
          customerName: c.customerName,
          managedBy: 'foodservice',
          lines: c.lines.map((l) => ({ productId: l.menuId, quantity: l.quantity })),
        }),
        id = randomUUID()
      await client.query(
        'INSERT INTO archava_foodservice.tickets(tenant_id,id,owner_id,order_id,mode,table_id,guests) VALUES($1,$2,$3,$4,$5,$6,$7)',
        [tenant, id, a.ownerId, draft.id, c.mode, c.tableId ?? null, c.guests],
      )
      for (const l of c.lines) {
        const m = menus.find((x) => x.id === l.menuId),
          o = draft.lines.find((x) => x.productId === l.menuId)
        if (!m || !o) throw new FoodError('conflict')
        await client.query(
          'INSERT INTO archava_foodservice.ticket_lines(tenant_id,ticket_id,order_id,order_line_id,menu_id,recipe_version,quantity,station) VALUES($1,$2,$3,$4,$5,$6,$7,$8)',
          [tenant, id, draft.id, o.id, l.menuId, m.recipe_version, l.quantity, m.station],
        )
      }
      await this.allocations(client, tenant, id)
      return { id, revision: 1 }
    }
    const t = await this.ownedTicket(client, tenant, a, c.id, true)
    if (t.revision !== c.expectedRevision) throw new FoodError('conflict')
    let state: string,
      prepared = false
    if (c.operation === 'ticket.queue') {
      if (t.state !== 'draft') throw new FoodError('conflict')
      await reserveCommerceOrder(client, {
        tenant,
        id: String(t.order_id),
        ownerModule: 'foodservice',
        holdSeconds: c.holdSeconds,
        allocations: await this.allocations(client, tenant, c.id),
      })
      state = 'queued'
    } else if (c.operation === 'ticket.prepare') {
      if (t.state !== 'queued') throw new FoodError('conflict')
      await prepareFoodCommerceOrder(client, {
        tenant,
        id: String(t.order_id),
        requestId: input.requestId,
        actorId: principal.id,
        fingerprint: this.fingerprint(tenant, principal, input),
      })
      state = 'preparing'
      prepared = true
    } else if (c.operation === 'ticket.cancel') {
      if (t.state === 'closed' || t.state === 'cancelled') throw new FoodError('conflict')
      if (t.prepared_at === null)
        await cancelCommerceOrder(client, {
          tenant,
          id: String(t.order_id),
          ownerModule: 'foodservice',
        })
      state = 'cancelled'
    } else {
      const transitions = {
          'ticket.ready': ['preparing', 'ready'],
          'ticket.serve': ['ready', 'served'],
          'ticket.close': ['served', 'closed'],
        } as const,
        transition = transitions[c.operation]
      if (!transition || t.state !== transition[0]) throw new FoodError('conflict')
      state = transition[1]
    }
    await client.query(
      'UPDATE archava_foodservice.tickets SET state=$3,prepared_at=CASE WHEN $4::boolean THEN clock_timestamp() ELSE prepared_at END,cancellation_reason=$5,revision=revision+1,updated_at=clock_timestamp() WHERE tenant_id=$1 AND id=$2',
      [tenant, c.id, state, prepared, 'reason' in c ? c.reason : null],
    )
    return { id: c.id, revision: c.expectedRevision + 1 }
  }
}
