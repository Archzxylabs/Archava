/* global process, Request, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomBytes, randomUUID, createHash } from 'node:crypto'
import { build } from 'esbuild'
import { endProofPool } from './pg-proof.mjs'
import { installEnv } from './npm-env.mjs'

const root = resolve(import.meta.dirname, '../..'),
  library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-commerce-proof-'))
const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('PG')))
const run = (name, args) =>
  execFileSync(join(bin, name), args, { encoding: 'utf8', stdio: 'pipe', env })
const socket = join(scratch, 'socket'),
  data = join(scratch, 'data'),
  port = 20000 + Math.floor(Math.random() * 25000)
await mkdir(socket)
let running = false,
  owner,
  app,
  Pool
const start = () => {
  run('pg_ctl', [
    '-D',
    data,
    '-l',
    join(scratch, 'postgres.log'),
    '-w',
    'start',
    '-o',
    `-k ${socket} -p ${port} -c listen_addresses=''`,
  ])
  running = true
}
const stop = () => {
  if (running) {
    run('pg_ctl', ['-D', data, '-m', 'fast', '-w', 'stop'])
    running = false
  }
}
const target = {
  host: socket,
  port,
  user: 'postgres',
  database: 'postgres',
  password: () => '',
  ssl: false,
  max: 24,
}
const key = randomBytes(32).toString('hex'),
  checks = [],
  proofs = []
const check = (label) => {
  checks.push(label)
  process.stdout.write('PASS ' + label + '\n')
}
const human = (id) => ({ kind: 'human', id })
const mutation = (command, requestId = randomUUID()) => ({ requestId, command })
const failed = async (promise, code) =>
  assert.rejects(
    typeof promise === 'function' ? async () => promise() : promise,
    (error) => error.code === code,
  )

try {
  const consumers = []
  for (const name of ['retail', 'wholesale']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-commerce-' + name,
        private: true,
        type: 'module',
        overrides: Object.fromEntries(profile.catalog.externals.map((x) => [x.name, x.version])),
        dependencies: Object.fromEntries(
          profile.artifacts.map((x) => [x.name, 'file:' + join(library, x.tarball)]),
        ),
      }),
    )
    execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: directory,
      stdio: 'pipe',
      env: { ...installEnv(env), NODE_PATH: '' },
    })
    const require = createRequire(join(directory, 'package.json')),
      load = async (name) => import(pathToFileURL(require.resolve(name)).href)
    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))
    const domain = await load('@archava/commerce'),
      postgres = await load('@archava/commerce-pg')
    Pool ??= require('pg').Pool
    const browser = await build({
      entryPoints: [require.resolve('@archava/commerce/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /commerce-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/commerce')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed retail and wholesale consumers share unchanged standalone commerce and browser-safe dashboard',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres } = consumers[0]
  await postgres.migrateCommerce(owner)
  await postgres.migrateCommerce(owner)
  await owner.query('CREATE ROLE commerce_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantCommerceRuntime(owner, 'commerce_app')
  for (const [tenant, id, kind, grants, scope] of [
    ['retail', 'owner', 'human', domain.COMMERCE_PERMISSIONS, 'all'],
    [
      'retail',
      'viewer',
      'human',
      domain.COMMERCE_PERMISSIONS.filter((x) => x.endsWith('.read') && x !== 'audit.read'),
      'all',
    ],
    ['retail', 'staff', 'human', domain.COMMERCE_PERMISSIONS, 'assigned'],
    ['retail', 'bot', 'service', ['catalog.read', 'stock.read'], 'all'],
    ['wholesale', 'owner', 'human', domain.COMMERCE_PERMISSIONS, 'all'],
  ])
    await owner.query(
      'INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
      [tenant, id, kind, grants, scope],
    )
  app = new Pool({ ...target, user: 'commerce_app' })
  let service = new domain.CommerceService(
    await postgres.PostgresCommerceRepository.create(app, key),
  )
  const second = new consumers[1].domain.CommerceService(
    await consumers[1].postgres.PostgresCommerceRepository.create(app, key),
  )
  await assert.rejects(() => postgres.PostgresCommerceRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresCommerceRepository.create(app, 'short'))
  assert.deepEqual(
    (
      await owner.query(
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'archava_%'",
      )
    ).rows.map((x) => x.schema_name),
    ['archava_commerce'],
  )
  check(
    'standalone commerce owns ten forced-RLS business tables and needs no CRM, AI or payment schema; unsafe runtimes refuse startup',
  )
  const act = (c, requestId) => service.mutate('retail', human('owner'), mutation(c, requestId))
  const make = async (sku, quantity = 0) => {
    const p = await act({ operation: 'product.create', sku, title: sku, price: 1000 })
    if (quantity)
      await act({
        operation: 'stock.adjust',
        productId: p.id,
        expectedRevision: 1,
        quantity,
        reason: 'Received physical stock',
      })
    return p
  }
  const product = await make('COFFEE', 10),
    untracked = await act({
      operation: 'product.create',
      sku: 'SERVICE',
      title: 'Delivery service',
      price: 5000,
      trackStock: false,
    })
  const wholesale = await second.mutate(
    'wholesale',
    human('owner'),
    mutation({
      operation: 'product.create',
      sku: 'COFFEE',
      title: 'Bulk material',
      price: 100,
      unit: 'gram',
    }),
  )
  await second.mutate(
    'wholesale',
    human('owner'),
    mutation({
      operation: 'stock.adjust',
      productId: wholesale.id,
      expectedRevision: 1,
      quantity: 1000,
      reason: 'Received grams',
    }),
  )
  const stock = () => service.stock('retail', human('owner'), product.id)
  assert.equal((await stock()).onHand, 10)
  await failed(service.product('wholesale', human('owner'), product.id), 'not_found')
  await failed(service.product('unknown', human('owner'), product.id), 'forbidden')
  await failed(
    service.mutate(
      'retail',
      human('viewer'),
      mutation({ operation: 'product.create', sku: 'VIEWER', title: 'Refused', price: 1 }),
    ),
    'forbidden',
  )
  await failed(
    service.mutate(
      'retail',
      human('staff'),
      mutation({ operation: 'product.create', sku: 'STAFF', title: 'Refused', price: 1 }),
    ),
    'forbidden',
  )
  const bot = { kind: 'service', id: 'bot', onBehalfOf: 'owner' }
  assert.equal((await service.stock('retail', bot, product.id)).onHand, 10)
  await failed(service.orders('retail', bot), 'forbidden')
  await failed(
    () =>
      service.mutate(
        'retail',
        bot,
        mutation({
          operation: 'order.create',
          customerName: 'Bot',
          lines: [{ productId: product.id, quantity: 1 }],
        }),
      ),
    'forbidden',
  )
  check(
    'tenant isolation, global catalog/stock administration, assigned scope and service/human grant intersection enforce current authority',
  )
  const create = mutation({
    operation: 'order.create',
    customerName: 'Retail customer',
    lines: [{ productId: product.id, quantity: 3 }],
  })
  const duplicates = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('retail', human('owner'), create)),
  )
  assert.equal(new Set(duplicates.map((x) => x.id)).size, 1)
  const booking = duplicates[0],
    initial = await service.detail('retail', human('owner'), booking.id)
  assert.equal(initial.lines.length, 1)
  assert.equal(initial.audit.length, 1)
  assert.equal(initial.order.totalEstimate, 3000)
  assert.equal((await stock()).reserved, 0)
  await failed(
    act({ ...create.command, customerName: 'Payload changed' }, create.requestId),
    'conflict',
  )
  await failed(service.commandStatus('retail', human('viewer'), create.requestId), 'forbidden')
  check(
    '20 duplicate order submissions create one draft, frozen line quote, audit and actor-bound receipt without reserving stock',
  )
  await act({
    operation: 'product.set-price',
    id: product.id,
    expectedRevision: 1,
    price: 2000,
    reason: 'New catalog price',
  })
  assert.deepEqual(
    (await service.detail('retail', human('owner'), booking.id)).lines,
    initial.lines,
  )
  const fresh = await act({ ...create.command, customerName: 'New quoted customer' })
  assert.equal((await service.detail('retail', human('owner'), fresh.id)).order.totalEstimate, 6000)
  assert.equal((await stock()).revision, 2)
  await failed(
    act({
      operation: 'stock.adjust',
      productId: untracked.id,
      expectedRevision: 1,
      quantity: 1,
      reason: 'Untracked stock',
    }),
    'conflict',
  )
  check(
    'catalog price changes affect future quotes only; stock revisions and quantities stay independent and untracked products refuse adjustments',
  )
  const reserve = mutation({
    operation: 'order.reserve',
    id: booking.id,
    expectedRevision: 1,
    holdSeconds: 1800,
  })
  const reservations = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('retail', human('owner'), reserve)),
  )
  assert.ok(reservations.every((x) => x.revision === 2))
  assert.equal((await stock()).reserved, 3)
  assert.equal((await stock()).onHand, 10)
  assert.equal((await stock()).available, 7)
  await failed(
    act({
      operation: 'stock.adjust',
      productId: product.id,
      expectedRevision: 2,
      quantity: -8,
      reason: 'Cannot remove reserved stock',
    }),
    'conflict',
  )
  await owner.query(
    "UPDATE archava_commerce.memberships SET permissions=$1 WHERE tenant_id='retail' AND principal_id='owner'",
    [domain.COMMERCE_PERMISSIONS.filter((x) => x !== 'orders.write')],
  )
  await failed(service.mutate('retail', human('owner'), reserve), 'forbidden')
  await owner.query(
    "UPDATE archava_commerce.memberships SET permissions=$1 WHERE tenant_id='retail' AND principal_id='owner'",
    [domain.COMMERCE_PERMISSIONS],
  )
  assert.deepEqual(
    (await service.commandStatus('retail', human('owner'), reserve.requestId)).receipt,
    reservations[0],
  )
  check(
    'stock reservation is atomic and idempotent; on-hand quantity stays unchanged and retries recheck current write grants',
  )
  const deliver = mutation({ operation: 'order.fulfill', id: booking.id, expectedRevision: 2 })
  const deliveries = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('retail', human('owner'), deliver)),
  )
  assert.ok(deliveries.every((x) => x.revision === 3))
  assert.equal((await stock()).onHand, 7)
  assert.equal((await stock()).reserved, 0)
  assert.equal(
    (
      await owner.query(
        "SELECT count(*)::integer AS n FROM archava_commerce.movements WHERE order_id=$1 AND kind='consume'",
        [booking.id],
      )
    ).rows[0].n,
    1,
  )
  assert.equal(
    (await service.detail('retail', human('owner'), booking.id)).order.totalEstimate,
    3000,
  )
  check(
    '20 duplicate fulfillment retries consume stock once, preserve the quoted price and never imply payment success',
  )
  const lineId = initial.lines[0].id,
    partial = mutation({
      operation: 'order.return',
      id: booking.id,
      expectedRevision: 3,
      lines: [{ lineId, quantity: 1, disposition: 'restock' }],
      reason: 'Received one unopened item',
    })
  const returned = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('retail', human('owner'), partial)),
  )
  assert.ok(returned.every((x) => x.revision === 4))
  assert.equal((await stock()).onHand, 8)
  let detail = await service.detail('retail', human('owner'), booking.id)
  assert.equal(detail.order.state, 'fulfilled')
  assert.equal(detail.lines[0].returnedQuantity, 1)
  assert.equal(detail.returns.length, 1)
  const tooMany = mutation({
    operation: 'order.return',
    id: booking.id,
    expectedRevision: 4,
    lines: [{ lineId, quantity: 3, disposition: 'restock' }],
    reason: 'Excess return',
  })
  await failed(service.mutate('retail', human('owner'), tooMany), 'conflict')
  assert.equal(
    (await service.commandStatus('retail', human('owner'), tooMany.requestId)).status,
    'not_committed',
  )
  await act({
    operation: 'order.return',
    id: booking.id,
    expectedRevision: 4,
    lines: [{ lineId, quantity: 2, disposition: 'discard' }],
    reason: 'Received remaining damaged items',
  })
  detail = await service.detail('retail', human('owner'), booking.id)
  assert.equal(detail.order.state, 'returned')
  assert.equal(detail.lines[0].returnedQuantity, 3)
  assert.equal(detail.order.totalEstimate, 3000)
  assert.equal((await stock()).onHand, 8)
  assert.equal(
    (await service.movements('retail', human('owner'), product.id)).items.reduce(
      (sum, x) => sum + x.quantity,
      0,
    ),
    8,
  )
  await failed(act({ operation: 'order.fulfill', id: booking.id, expectedRevision: 5 }), 'conflict')
  check(
    'partial returns and retries restock once; excess returns roll back; discarded items do not restock or rewrite quoted value',
  )
  const last = await make('LAST', 1),
    orderIds = []
  for (let i = 0; i < 20; i++)
    orderIds.push(
      await act({
        operation: 'order.create',
        customerName: 'Contender ' + i,
        lines: [{ productId: last.id, quantity: 1 }],
      }),
    )
  const race = await Promise.allSettled(
    orderIds.map((o) =>
      act({ operation: 'order.reserve', id: o.id, expectedRevision: 1, holdSeconds: 1800 }),
    ),
  )
  assert.equal(race.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal(
    race.filter((x) => x.status === 'rejected' && x.reason.code === 'conflict').length,
    19,
  )
  const winner = race.find((x) => x.status === 'fulfilled').value
  assert.equal((await service.stock('retail', human('owner'), last.id)).available, 0)
  await act({
    operation: 'order.cancel',
    id: winner.id,
    expectedRevision: 2,
    reason: 'Released last unit',
  })
  assert.equal((await service.stock('retail', human('owner'), last.id)).available, 1)
  await failed(
    act({ operation: 'order.reserve', id: winner.id, expectedRevision: 3, holdSeconds: 900 }),
    'conflict',
  )
  check(
    '20 distinct orders competing for the last unit have one winner; cancellation releases holds and cannot resurrect terminal orders',
  )
  const empty = await make('EMPTY')
  if (empty.id < product.id)
    await act({
      operation: 'stock.adjust',
      productId: empty.id,
      expectedRevision: 1,
      quantity: 1,
      reason: 'Make lexically first product available',
    })
  const combined = await act({
    operation: 'order.create',
    customerName: 'Atomic multi-product',
    lines: [
      { productId: product.id, quantity: empty.id < product.id ? 9 : 1 },
      { productId: empty.id, quantity: 1 },
    ],
  })
  const invalidReserve = mutation({
    operation: 'order.reserve',
    id: combined.id,
    expectedRevision: 1,
    holdSeconds: 900,
  })
  await failed(service.mutate('retail', human('owner'), invalidReserve), 'conflict')
  assert.equal((await stock()).reserved, 0)
  assert.equal((await service.detail('retail', human('owner'), combined.id)).order.state, 'draft')
  assert.equal(
    (
      await owner.query(
        'SELECT count(*)::integer AS n FROM archava_commerce.allocations WHERE order_id=$1',
        [combined.id],
      )
    ).rows[0].n,
    0,
  )
  assert.equal(
    (await service.commandStatus('retail', human('owner'), invalidReserve.requestId)).status,
    'not_committed',
  )
  check(
    'multi-product shortage rolls back every allocation, order change, command and audit without orphan reservations',
  )
  const expiringProduct = await make('EXPIRY', 1),
    expiringOrder = await act({
      operation: 'order.create',
      customerName: 'Expiry fixture',
      lines: [{ productId: expiringProduct.id, quantity: 1 }],
    }),
    expiry = mutation({
      operation: 'order.reserve',
      id: expiringOrder.id,
      expectedRevision: 1,
      holdSeconds: 1,
    })
  const expiryReceipt = await service.mutate('retail', human('owner'), expiry),
    lock = await owner.connect()
  await lock.query('BEGIN')
  await lock.query(
    'SELECT id FROM archava_commerce.products WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
    ['retail', expiringProduct.id],
  )
  let outcome
  const blocked = act({
    operation: 'order.fulfill',
    id: expiringOrder.id,
    expectedRevision: 2,
  }).then(
    () => {
      outcome = 'committed'
    },
    (e) => {
      outcome = e.code
    },
  )
  try {
    let waiting = false
    for (let i = 0; i < 30; i++) {
      waiting = (
        await owner.query(
          "SELECT EXISTS(SELECT 1 FROM pg_stat_activity a,archava_commerce.orders o WHERE a.usename='commerce_app' AND a.wait_event_type='Lock' AND a.query LIKE '%archava_commerce.products%FOR UPDATE%' AND o.id=$1 AND a.xact_start<o.expires_at) AS waiting",
          [expiringOrder.id],
        )
      ).rows[0].waiting
      if (waiting) break
      await new Promise((done) => setTimeout(done, 20))
    }
    assert.equal(waiting, true)
    await new Promise((done) => setTimeout(done, 1300))
  } finally {
    await lock.query('COMMIT')
    lock.release()
  }
  await blocked
  assert.equal(outcome, 'conflict')
  assert.equal(
    (await service.detail('retail', human('owner'), expiringOrder.id)).order.effectiveState,
    'expired',
  )
  assert.equal((await service.stock('retail', human('owner'), expiringProduct.id)).available, 1)
  assert.deepEqual(await service.mutate('retail', human('owner'), expiry), expiryReceipt)
  await act({
    operation: 'order.cancel',
    id: expiringOrder.id,
    expectedRevision: 2,
    reason: 'Closed expired hold',
  })
  check(
    'hold expiry uses database clock after a proven product lock wait; original receipts never extend or resurrect reservations',
  )
  const staffCreate = mutation({
      operation: 'order.create',
      customerName: 'Assigned customer',
      lines: [{ productId: product.id, quantity: 1 }],
    }),
    staffOrder = await service.mutate('retail', human('staff'), staffCreate)
  await service.detail('retail', human('staff'), staffOrder.id)
  await failed(service.detail('retail', human('staff'), booking.id), 'not_found')
  assert.equal((await service.orders('retail', human('staff'))).items.length, 1)
  await failed(service.mutate('retail', human('staff'), create), 'conflict')
  await owner.query(
    "UPDATE archava_commerce.memberships SET active=false WHERE tenant_id='retail' AND principal_id='staff'",
  )
  await failed(service.commandStatus('retail', human('staff'), staffCreate.requestId), 'forbidden')
  await owner.query(
    "UPDATE archava_commerce.memberships SET active=true WHERE tenant_id='retail' AND principal_id='staff'",
  )
  check(
    'assigned operator owns only own orders; command UUIDs stay actor-bound and inactive membership immediately refuses recovery',
  )
  const concurrent = await act({
      operation: 'order.create',
      customerName: 'Draft revision fixture',
      lines: [{ productId: product.id, quantity: 1 }],
    }),
    edits = await Promise.allSettled(
      [2, 3].map((quantity) =>
        act({
          operation: 'order.replace-lines',
          id: concurrent.id,
          expectedRevision: 1,
          lines: [{ productId: product.id, quantity }],
          reason: 'Changed draft quantity',
        }),
      ),
    )
  assert.equal(edits.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal(
    edits.filter((x) => x.status === 'rejected' && x.reason.code === 'conflict').length,
    1,
  )
  assert.equal((await service.detail('retail', human('owner'), concurrent.id)).order.revision, 2)
  const bulk = await second.mutate(
    'wholesale',
    human('owner'),
    mutation({
      operation: 'order.create',
      customerName: 'Bulk customer',
      lines: [{ productId: wholesale.id, quantity: 100 }],
    }),
  )
  await second.mutate(
    'wholesale',
    human('owner'),
    mutation({ operation: 'order.reserve', id: bulk.id, expectedRevision: 1, holdSeconds: 1800 }),
  )
  await second.mutate(
    'wholesale',
    human('owner'),
    mutation({ operation: 'order.fulfill', id: bulk.id, expectedRevision: 2 }),
  )
  assert.equal((await second.stock('wholesale', human('owner'), wholesale.id)).onHand, 900)
  assert.equal((await second.detail('wholesale', human('owner'), bulk.id)).lines[0].unit, 'gram')
  check(
    'competing draft revisions cannot overwrite work and the same module fulfills both unit retail and gram-based wholesale',
  )
  const duplicateProduct = mutation({
    operation: 'product.create',
    sku: 'COFFEE',
    title: 'Duplicate SKU',
    price: 1,
  })
  await failed(service.mutate('retail', human('owner'), duplicateProduct), 'conflict')
  assert.equal(
    (await service.commandStatus('retail', human('owner'), duplicateProduct.requestId)).status,
    'not_committed',
  )
  const client = await app.connect()
  try {
    assert.equal(
      (await client.query('SELECT count(*)::integer AS n FROM archava_commerce.orders')).rows[0].n,
      0,
    )
    for (const [sql, args] of [
      ['UPDATE archava_commerce.memberships SET active=false', []],
      [
        'UPDATE archava_commerce.stock_balances SET quantity=quantity+1 WHERE product_id=$1',
        [product.id],
      ],
      ['UPDATE archava_commerce.commands SET receipt=NULL WHERE request_id=$1', [create.requestId]],
      ['DELETE FROM archava_commerce.order_lines WHERE order_id=$1', [booking.id]],
      ['UPDATE archava_commerce.movements SET quantity=1', []],
      ['DELETE FROM archava_commerce.audit', []],
    ]) {
      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id','retail',true)")
      await assert.rejects(() => client.query(sql, args))
      await client.query('ROLLBACK')
    }
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','retail',true)")
    assert.equal(
      (
        await client.query(
          "SELECT count(*)::integer AS n FROM archava_commerce.products WHERE tenant_id='wholesale'",
        )
      ).rows[0].n,
      0,
    )
    await client.query('ROLLBACK')
  } finally {
    client.release()
  }
  check(
    'forced tenant RLS, immutable movements and committed quotes, derived balances and append-only audit refuse direct SQL bypass',
  )
  const tools = domain.createCommerceTools(service, 'retail', bot)
  assert.equal((await tools.invoke('commerce.stock', { id: product.id })).onHand, 8)
  await assert.rejects(() =>
    tools.invoke('commerce.stock', { id: product.id, tenant: 'wholesale' }),
  )
  await failed(tools.invoke('commerce.fulfill', { id: booking.id }), 'not_found')
  const api = domain.createCommerceApi(service, async () => human('viewer'))
  assert.equal(
    (await api(new Request('http://local.test/api/commerce/retail/orders?scope=all'))).status,
    400,
  )
  assert.equal(
    (await api(new Request('http://local.test/api/commerce/retail/products?active=no'))).status,
    400,
  )
  assert.equal(
    (
      await api(
        new Request('http://local.test/api/commerce/retail/commands', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(create),
        }),
      )
    ).status,
    403,
  )
  const hidden = await service.detail('retail', { kind: 'human', id: 'viewer' }, booking.id)
  assert.equal(hidden.audit, null)
  assert.equal(hidden.returns, null)
  check(
    'strict bounded API and private read-only tools refuse caller authority overrides and hide history without audit grant',
  )
  const expected = await service.detail('retail', human('owner'), booking.id),
    expectedStock = await stock(),
    archive = join(scratch, 'commerce.dump')
  run('pg_dump', [
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    '--format=custom',
    '--no-owner',
    '--no-privileges',
    '--file',
    archive,
    'postgres',
  ])
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  start()
  owner = new Pool(target)
  app = new Pool({ ...target, user: 'commerce_app' })
  service = new domain.CommerceService(await postgres.PostgresCommerceRepository.create(app, key))
  const restarted = await service.detail('retail', human('owner'), booking.id)
  assert.deepEqual(restarted.order, expected.order)
  assert.deepEqual(restarted.lines, expected.lines)
  assert.deepEqual(restarted.returns, expected.returns)
  assert.deepEqual(restarted.audit, expected.audit)
  assert.equal(
    (await service.stock('retail', human('owner'), product.id)).onHand,
    expectedStock.onHand,
  )
  assert.deepEqual(
    (await service.commandStatus('retail', human('owner'), partial.requestId)).receipt,
    returned[0],
  )
  check(
    'database restart preserves stock ledger, quoted orders, returns, domain grants and actor-bound command receipts',
  )
  await owner.query('CREATE DATABASE restored_commerce')
  run('pg_restore', [
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    '--exit-on-error',
    '--single-transaction',
    '--no-owner',
    '--no-privileges',
    '--dbname',
    'restored_commerce',
    archive,
  ])
  const ro = new Pool({ ...target, database: 'restored_commerce' }),
    ra = new Pool({ ...target, database: 'restored_commerce', user: 'commerce_app' })
  try {
    await postgres.migrateCommerce(ro)
    await postgres.grantCommerceRuntime(ro, 'commerce_app')
    const restored = new domain.CommerceService(
        await postgres.PostgresCommerceRepository.create(ra, key),
      ),
      restoredDetail = await restored.detail('retail', human('owner'), booking.id)
    assert.deepEqual(restoredDetail.order, expected.order)
    assert.deepEqual(restoredDetail.lines, expected.lines)
    assert.deepEqual(restoredDetail.returns, expected.returns)
    assert.deepEqual(restoredDetail.audit, expected.audit)
    assert.equal(
      (await restored.stock('retail', human('owner'), product.id)).onHand,
      expectedStock.onHand,
    )
    assert.deepEqual(
      (await restored.commandStatus('retail', human('owner'), partial.requestId)).receipt,
      returned[0],
    )
    await ro.query(
      "UPDATE archava_commerce.memberships SET active=false WHERE tenant_id='retail' AND principal_id='viewer'",
    )
    await failed(restored.product('retail', human('viewer'), product.id), 'forbidden')
    await service.product('retail', human('viewer'), product.id)
  } finally {
    await endProofPool(ra)
    await endProofPool(ro)
  }
  check(
    'fresh backup restoration preserves quantities and receipt recovery while independent grant changes leave original database untouched',
  )
  await owner.query('UPDATE archava_commerce.schema_migrations SET checksum=$1', ['a'.repeat(64)])
  await assert.rejects(() => postgres.PostgresCommerceRepository.create(app, key))
  await assert.rejects(() => postgres.migrateCommerce(owner))
  check('changed commerce migration checksum refuses repository startup and migration')
  const destination = join(root, 'dist/commerce-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope: 'bounded catalog, stock and orders with physical returns',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )
} catch (error) {
  process.stderr.write(String(error.stack ?? error) + '\n')
  throw error
} finally {
  if (app && !app.ended) await endProofPool(app)
  if (owner && !owner.ended) await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true })
}
