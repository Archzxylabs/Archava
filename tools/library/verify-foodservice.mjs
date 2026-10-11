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

const root = resolve(import.meta.dirname, '../..'),
  library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-foodservice-proof-'))
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
  for (const name of ['restaurant', 'catering']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-foodservice-' + name,
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
      env: { ...env, NODE_PATH: '' },
    })
    const require = createRequire(join(directory, 'package.json')),
      load = async (name) => import(pathToFileURL(require.resolve(name)).href)
    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))
    const domain = await load('@archava/foodservice'),
      postgres = await load('@archava/foodservice-pg')
    Pool ??= require('pg').Pool
    const browser = await build({
      entryPoints: [require.resolve('@archava/foodservice/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /foodservice-pg|commerce-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({
      domain,
      postgres,
      commerce: await load('@archava/commerce'),
      commercePg: await load('@archava/commerce-pg'),
      name,
      directory,
    })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/foodservice')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed restaurant and catering consumers share unchanged food-service code and browser-safe dashboard',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres, commerce, commercePg } = consumers[0]
  // Seed a real non-empty 1.7 installation before the additive shared-kernel migration.
  const oldLibrary = join(root, 'dist/releases/1.7.0/library'),
    oldProfile = JSON.parse(await readFile(join(oldLibrary, 'release-profile.json'), 'utf8')),
    oldDir = join(scratch, 'prior-retail')
  await mkdir(oldDir)
  await writeFile(
    join(oldDir, 'package.json'),
    JSON.stringify({
      private: true,
      type: 'module',
      overrides: Object.fromEntries(oldProfile.catalog.externals.map((x) => [x.name, x.version])),
      dependencies: Object.fromEntries(
        oldProfile.artifacts.map((x) => [x.name, 'file:' + join(oldLibrary, x.tarball)]),
      ),
    }),
  )
  execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
    cwd: oldDir,
    stdio: 'pipe',
    env: { ...env, NODE_PATH: '' },
  })
  const oldRequire = createRequire(join(oldDir, 'package.json')),
    oldDomain = await import(pathToFileURL(oldRequire.resolve('@archava/commerce')).href),
    oldPg = await import(pathToFileURL(oldRequire.resolve('@archava/commerce-pg')).href)
  await oldPg.migrateCommerce(owner)
  await owner.query('CREATE ROLE food_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await oldPg.grantCommerceRuntime(owner, 'food_app')
  await owner.query(
    "INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('prior','owner','human',$1,'all')",
    [commerce.COMMERCE_PERMISSIONS],
  )
  app = new Pool({ ...target, user: 'food_app' })
  const priorService = new oldDomain.CommerceService(
      await oldPg.PostgresCommerceRepository.create(app, key),
    ),
    priorProduct = await priorService.mutate(
      'prior',
      human('owner'),
      mutation({
        operation: 'product.create',
        sku: 'PRIOR',
        title: 'Historical product',
        price: 7000,
      }),
    ),
    priorCommand = mutation({
      operation: 'stock.adjust',
      productId: priorProduct.id,
      expectedRevision: 1,
      quantity: 5,
      reason: 'Original physical stock',
    })
  await priorService.mutate('prior', human('owner'), priorCommand)
  const priorOrder = await priorService.mutate(
      'prior',
      human('owner'),
      mutation({
        operation: 'order.create',
        customerName: 'Prior retailer',
        lines: [{ productId: priorProduct.id, quantity: 2 }],
      }),
    ),
    priorBefore = await priorService.detail('prior', human('owner'), priorOrder.id)
  const priorSnapshot = async () => {
    const tables = (
        await owner.query(
          "SELECT table_name FROM information_schema.tables WHERE table_schema='archava_commerce' AND table_type='BASE TABLE' ORDER BY table_name",
        )
      ).rows,
      result = {}
    for (const t of tables) {
      const cols = (
        await owner.query(
          "SELECT column_name FROM information_schema.columns WHERE table_schema='archava_commerce' AND table_name=$1 AND column_name<>'managed_by' ORDER BY ordinal_position",
          [t.table_name],
        )
      ).rows
        .map((x) => '"' + x.column_name + '"')
        .join(',')
      const rows = (
        await owner.query(
          'SELECT to_jsonb(t) AS r FROM (SELECT ' +
            cols +
            ' FROM archava_commerce."' +
            t.table_name +
            '") t',
        )
      ).rows
        .map((x) => JSON.stringify(x.r))
        .sort()
      result[t.table_name] = rows
    }
    return result
  }
  const before = await priorSnapshot()
  await commercePg.migrateCommerce(owner)
  await commercePg.migrateCommerce(owner)
  const after = await priorSnapshot()
  for (const name of Object.keys(before)) {
    if (name === 'schema_migrations') assert.equal(after[name].length, before[name].length + 1)
    else assert.deepEqual(after[name], before[name])
  }
  await postgres.migrateFoodService(owner)
  await postgres.migrateFoodService(owner)
  await commercePg.grantCommerceRuntime(owner, 'food_app')
  await postgres.grantFoodServiceRuntime(owner, 'food_app')
  const commerceService = new commerce.CommerceService(
    await commercePg.PostgresCommerceRepository.create(app, key),
  )
  assert.deepEqual(
    {
      ...(await commerceService.detail('prior', human('owner'), priorOrder.id)),
      evaluatedAt: priorBefore.evaluatedAt,
    },
    priorBefore,
  )
  assert.equal(
    (await commerceService.commandStatus('prior', human('owner'), priorCommand.requestId)).status,
    'committed',
  )
  check(
    'non-empty archived 1.7 commerce upgrades additively with original rows, quotes, stock, grants, command UUIDs and receipts unchanged',
  )
  for (const [tenant, id, kind, scope, permissions] of [
    ['restaurant', 'owner', 'human', 'all', domain.FOOD_PERMISSIONS],
    ['restaurant', 'staff', 'human', 'assigned', domain.FOOD_PERMISSIONS],
    [
      'restaurant',
      'viewer',
      'human',
      'all',
      domain.FOOD_PERMISSIONS.filter((x) => x.endsWith('.read') && x !== 'audit.read'),
    ],
    ['restaurant', 'bot', 'service', 'assigned', ['tables.read', 'menu.read', 'tickets.read']],
    ['catering', 'owner', 'human', 'all', domain.FOOD_PERMISSIONS],
  ]) {
    await owner.query(
      'INSERT INTO archava_foodservice.memberships(tenant_id,principal_id,kind,scope,permissions) VALUES($1,$2,$3,$4,$5)',
      [tenant, id, kind, scope, permissions],
    )
    await owner.query(
      'INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,scope,permissions) VALUES($1,$2,$3,$4,$5)',
      [
        tenant,
        id,
        kind,
        scope,
        kind === 'service'
          ? ['catalog.read', 'orders.read', 'stock.read']
          : commerce.COMMERCE_PERMISSIONS,
      ],
    )
  }
  let service = new domain.FoodService(await postgres.PostgresFoodRepository.create(app, key))
  const second = new consumers[1].domain.FoodService(
    await consumers[1].postgres.PostgresFoodRepository.create(app, key),
  )
  await assert.rejects(() => postgres.PostgresFoodRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresFoodRepository.create(app, 'short'))
  assert.deepEqual(
    (
      await owner.query(
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'archava_%' ORDER BY schema_name",
      )
    ).rows.map((x) => x.schema_name),
    ['archava_commerce', 'archava_foodservice'],
  )
  const act = (c, requestId) =>
      service.mutate('restaurant', human('owner'), mutation(c, requestId)),
    stock = async (p) => commerceService.stock('restaurant', human('owner'), p.id),
    make = async (sku, quantity, unit = 'gram') => {
      const p = await commerceService.mutate(
        'restaurant',
        human('owner'),
        mutation({ operation: 'product.create', sku, title: sku, price: 1, unit }),
      )
      await commerceService.mutate(
        'restaurant',
        human('owner'),
        mutation({
          operation: 'stock.adjust',
          productId: p.id,
          expectedRevision: 1,
          quantity,
          reason: 'Received ingredients',
        }),
      )
      return p
    }
  const rice = await make('RICE', 1000),
    oil = await make('OIL', 500, 'millilitre'),
    dish = await act({
      operation: 'menu.create',
      sku: 'MEAL',
      title: '<script>meal</script>',
      price: 20000,
      recipe: [
        { productId: rice.id, quantity: 100 },
        { productId: oil.id, quantity: 20 },
      ],
    }),
    table = await act({ operation: 'table.create', code: 'A1', title: 'Window table', seats: 4 })
  assert.equal(
    (
      await owner.query(
        "SELECT count(*)::int AS n FROM pg_class WHERE relnamespace='archava_foodservice'::regnamespace AND relrowsecurity AND relforcerowsecurity",
      )
    ).rows[0].n,
    9,
  )
  check(
    'standalone restaurant owns nine forced-RLS business tables and composes only shared commerce without CRM, identity, AI or payment schemas',
  )
  assert.equal(
    (await commerceService.products('restaurant', human('owner'))).items.some(
      (x) => x.id === dish.id,
    ),
    false,
  )
  for (const command of [
    {
      operation: 'order.create',
      customerName: 'Bypass recipe',
      lines: [{ productId: dish.id, quantity: 1 }],
    },
    {
      operation: 'product.set-price',
      id: dish.id,
      expectedRevision: 1,
      price: 0,
      reason: 'Bypass domain',
    },
  ])
    await failed(
      commerceService.mutate('restaurant', human('owner'), mutation(command)),
      'not_found',
    )
  await failed(commerceService.product('restaurant', human('owner'), dish.id), 'not_found')
  check(
    'generic catalog/order APIs hide and refuse managed menu products so selling a portion cannot bypass recipe ownership',
  )
  const create = mutation({
      operation: 'ticket.create',
      customerName: 'Table guest',
      mode: 'dine_in',
      tableId: table.id,
      guests: 2,
      lines: [{ menuId: dish.id, quantity: 2 }],
    }),
    duplicates = await Promise.all(
      Array.from({ length: 20 }, () => service.mutate('restaurant', human('owner'), create)),
    )
  assert.equal(new Set(duplicates.map((x) => x.id)).size, 1)
  assert.equal(duplicates.filter((x) => !x.replayed).length, 1)
  const ticket = duplicates[0],
    initial = await service.detail('restaurant', human('owner'), ticket.id)
  assert.equal(initial.ticket.totalEstimate, 40000)
  assert.equal(initial.lines[0].recipeVersion, 1)
  assert.equal((await stock(rice)).onHand, 1000)
  check(
    '20 duplicate ticket submissions commit one managed quote, frozen recipe, table claim, actor receipt and audit without consuming ingredients',
  )
  await failed(
    act({
      operation: 'table.set-active',
      id: table.id,
      expectedRevision: 1,
      active: false,
      reason: 'Cannot hide occupied table',
    }),
    'conflict',
  )
  await failed(act({ ...create.command, customerName: 'Too many guests', guests: 5 }), 'conflict')
  await failed(act({ ...create.command, customerName: 'Second guest' }), 'conflict')
  assert.equal((await service.tables('restaurant', human('staff'))).items[0].occupied, true)
  check(
    'table occupancy is exclusive, seat bounds are authoritative and an open ticket prevents disabling the physical table',
  )
  await act({
    operation: 'menu.set-price',
    id: dish.id,
    expectedRevision: 1,
    price: 30000,
    reason: 'New rate',
  })
  await act({
    operation: 'menu.replace-recipe',
    id: dish.id,
    expectedRevision: 2,
    recipe: [
      { productId: rice.id, quantity: 150 },
      { productId: oil.id, quantity: 30 },
    ],
    reason: 'New recipe for future tickets',
  })
  assert.deepEqual(
    (await service.detail('restaurant', human('owner'), ticket.id)).lines,
    initial.lines,
  )
  const future = await act({
    operation: 'ticket.create',
    customerName: 'Future takeaway',
    mode: 'takeaway',
    lines: [{ menuId: dish.id, quantity: 1 }],
  })
  assert.equal(
    (await service.detail('restaurant', human('owner'), future.id)).ticket.totalEstimate,
    30000,
  )
  assert.equal(
    (await service.detail('restaurant', human('owner'), future.id)).lines[0].recipeVersion,
    2,
  )
  check(
    'menu price and sealed recipe changes affect future tickets only; existing portions retain original backend price and ingredient version',
  )
  const queue = mutation({
    operation: 'ticket.queue',
    id: ticket.id,
    expectedRevision: 1,
    holdSeconds: 1800,
  })
  const queued = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('restaurant', human('owner'), queue)),
  )
  assert.equal(queued.filter((x) => !x.replayed).length, 1)
  assert.equal((await stock(rice)).reserved, 200)
  assert.equal((await stock(oil)).reserved, 40)
  assert.equal((await stock(rice)).onHand, 1000)
  check(
    'queue reserves aggregated frozen ingredient quantities atomically and retries do not consume stock or extend holds',
  )
  const prepare = mutation({ operation: 'ticket.prepare', id: ticket.id, expectedRevision: 2 })
  const prepared = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('restaurant', human('owner'), prepare)),
  )
  assert.equal(prepared.filter((x) => !x.replayed).length, 1)
  assert.equal((await stock(rice)).onHand, 800)
  assert.equal((await stock(oil)).onHand, 460)
  assert.equal((await stock(rice)).reserved, 0)
  assert.equal(
    (
      await owner.query(
        'SELECT count(*)::int AS n FROM archava_commerce.movements WHERE tenant_id=$1 AND request_id=$2',
        ['restaurant', prepare.requestId],
      )
    ).rows[0].n,
    2,
  )
  check(
    '20 preparation retries consume each ingredient once in the shared ledger and atomically commit kitchen state, both audits and receipts',
  )
  await act({
    operation: 'ticket.cancel',
    id: ticket.id,
    expectedRevision: 3,
    reason: 'Prepared food wasted after cancellation',
  })
  assert.equal((await stock(rice)).onHand, 800)
  assert.equal((await service.tables('restaurant', human('owner'))).items[0].occupied, false)
  await failed(act({ operation: 'ticket.ready', id: ticket.id, expectedRevision: 4 }), 'conflict')
  check(
    'cancellation after preparation records waste, frees the table and retains consumed ingredients without resurrection or refund',
  )
  await act({ operation: 'ticket.queue', id: future.id, expectedRevision: 1, holdSeconds: 1800 })
  await act({
    operation: 'ticket.cancel',
    id: future.id,
    expectedRevision: 2,
    reason: 'Cancelled before cooking',
  })
  assert.equal((await stock(rice)).reserved, 0)
  assert.equal((await stock(rice)).onHand, 800)
  check(
    'preparation-free cancellation releases ingredients without changing physical stock or erasing frozen quotes',
  )
  const raceTable = await act({
      operation: 'table.create',
      code: 'RACE',
      title: 'Last table',
      seats: 2,
    }),
    tableRace = await Promise.allSettled(
      Array.from({ length: 20 }, (_, i) =>
        act({
          operation: 'ticket.create',
          customerName: 'Table race ' + i,
          mode: 'dine_in',
          tableId: raceTable.id,
          lines: [{ menuId: dish.id, quantity: 1 }],
        }),
      ),
    )
  assert.equal(tableRace.filter((x) => x.status === 'fulfilled').length, 1)
  for (const x of tableRace) if (x.status === 'rejected') assert.equal(x.reason.code, 'conflict')
  check(
    '20 distinct tickets competing for the last physical table have one winner and all losing drafts roll back',
  )
  const egg = await make('LAST_EGG', 1, 'unit'),
    eggDish = await act({
      operation: 'menu.create',
      sku: 'EGG_MEAL',
      title: 'Egg meal',
      price: 5000,
      recipe: [{ productId: egg.id, quantity: 1 }],
    }),
    eggTickets = await Promise.all(
      Array.from({ length: 20 }, (_, i) =>
        act({
          operation: 'ticket.create',
          customerName: 'Egg race ' + i,
          mode: 'takeaway',
          lines: [{ menuId: eggDish.id, quantity: 1 }],
        }),
      ),
    ),
    eggRace = await Promise.allSettled(
      eggTickets.map((t) =>
        act({ operation: 'ticket.queue', id: t.id, expectedRevision: 1, holdSeconds: 1800 }),
      ),
    )
  assert.equal(eggRace.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal((await stock(egg)).reserved, 1)
  for (const x of eggRace) if (x.status === 'rejected') assert.equal(x.reason.code, 'conflict')
  check(
    '20 restaurant tickets competing for the last ingredient have one winner across the same retail stock lock and ledger',
  )
  const count = async () => {
    const result = {}
    for (const schema of ['archava_commerce', 'archava_foodservice']) {
      for (const t of (
        await owner.query(
          "SELECT table_name FROM information_schema.tables WHERE table_schema=$1 AND table_type='BASE TABLE' ORDER BY table_name",
          [schema],
        )
      ).rows) {
        result[schema + '.' + t.table_name] = (
          await owner.query('SELECT to_jsonb(t) AS r FROM ' + schema + '."' + t.table_name + '" t')
        ).rows
          .map((x) => JSON.stringify(x.r))
          .sort()
      }
    }
    return result
  }
  const insufficient = await act({
      operation: 'menu.create',
      sku: 'SHORT',
      title: 'Two ingredient shortage',
      price: 1000,
      recipe: [
        { productId: rice.id, quantity: rice.id < oil.id ? 1 : 1000 },
        { productId: oil.id, quantity: oil.id < rice.id ? 1 : 1000 },
      ],
    }),
    shortTicket = await act({
      operation: 'ticket.create',
      customerName: 'Shortage',
      mode: 'takeaway',
      lines: [{ menuId: insufficient.id, quantity: 1 }],
    }),
    beforeShortage = await count()
  await failed(
    act({ operation: 'ticket.queue', id: shortTicket.id, expectedRevision: 1 }),
    'conflict',
  )
  assert.deepEqual(await count(), beforeShortage)
  const rollback = await act({
    operation: 'ticket.create',
    customerName: 'Atomic failure',
    mode: 'takeaway',
    lines: [{ menuId: dish.id, quantity: 1 }],
  })
  await act({ operation: 'ticket.queue', id: rollback.id, expectedRevision: 1, holdSeconds: 1800 })
  const beforeRollback = await count(),
    rollbackCommand = mutation({
      operation: 'ticket.prepare',
      id: rollback.id,
      expectedRevision: 2,
    })
  await owner.query(
    "CREATE FUNCTION archava_foodservice.proof_audit_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Injected domain audit failure' USING ERRCODE='23514';END $$;CREATE TRIGGER proof_audit_failure BEFORE INSERT ON archava_foodservice.audit FOR EACH ROW EXECUTE FUNCTION archava_foodservice.proof_audit_failure()",
  )
  await failed(service.mutate('restaurant', human('owner'), rollbackCommand), 'conflict')
  assert.deepEqual(await count(), beforeRollback)
  await owner.query(
    'DROP TRIGGER proof_audit_failure ON archava_foodservice.audit;DROP FUNCTION archava_foodservice.proof_audit_failure()',
  )
  await service.mutate('restaurant', human('owner'), rollbackCommand)
  check(
    'multi-ingredient shortage and failure after stock consumption roll back every allocation, movement, state, quote, command and audit before safe same-UUID retry',
  )
  const expiring = await act({
    operation: 'ticket.create',
    customerName: 'Lock expiry',
    mode: 'dine_in',
    tableId: table.id,
    lines: [{ menuId: dish.id, quantity: 1 }],
  })
  await act({ operation: 'ticket.queue', id: expiring.id, expectedRevision: 1, holdSeconds: 1 })
  const blocker = await app.connect()
  await blocker.query('BEGIN')
  await blocker.query("SELECT set_config('archava.tenant_id','restaurant',true)")
  await blocker.query(
    'SELECT id FROM archava_commerce.products WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
    ['restaurant', rice.id],
  )
  const beforeExpiryStock = (await stock(oil)).onHand,
    expiryCommand = mutation({ operation: 'ticket.prepare', id: expiring.id, expectedRevision: 2 }),
    waiting = service.mutate('restaurant', human('owner'), expiryCommand),
    rejected = failed(waiting, 'conflict')
  let waitObserved = false
  for (let n = 0; n < 40 && !waitObserved; n++) {
    waitObserved = (
      await owner.query(
        "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%FROM archava_commerce.products%') AS waiting",
      )
    ).rows[0].waiting
    if (!waitObserved) await new Promise((r) => setTimeout(r, 25))
  }
  assert.equal(waitObserved, true)
  await new Promise((r) => setTimeout(r, 1200))
  await blocker.query('COMMIT')
  blocker.release()
  await rejected
  assert.equal((await stock(oil)).onHand, beforeExpiryStock)
  assert.equal(
    (await service.detail('restaurant', human('owner'), expiring.id)).ticket.stockExpired,
    true,
  )
  assert.equal(
    (await service.tables('restaurant', human('owner'))).items.find((x) => x.id === table.id)
      .occupied,
    true,
  )
  assert.equal(
    (await service.commandStatus('restaurant', human('owner'), expiryCommand.requestId)).state,
    'unknown',
  )
  await act({
    operation: 'ticket.cancel',
    id: expiring.id,
    expectedRevision: 2,
    reason: 'Stock hold expired; guest service cancelled',
  })
  check(
    'preparation expiry uses the database clock after a proven ingredient lock wait; no stock consumed, receipt created or physical table silently released',
  )
  const lifecycle = await act({
    operation: 'ticket.create',
    customerName: 'Complete service',
    mode: 'dine_in',
    tableId: table.id,
    lines: [{ menuId: dish.id, quantity: 1 }],
  })
  for (const [operation, expectedRevision] of [
    ['ticket.queue', 1],
    ['ticket.prepare', 2],
    ['ticket.ready', 3],
    ['ticket.serve', 4],
    ['ticket.close', 5],
  ])
    await act({ operation, id: lifecycle.id, expectedRevision })
  const closed = await service.detail('restaurant', human('owner'), lifecycle.id)
  assert.equal(closed.ticket.state, 'closed')
  assert.equal(
    (await service.tables('restaurant', human('owner'))).items.find((x) => x.id === table.id)
      .occupied,
    false,
  )
  assert.equal(Object.hasOwn(closed.ticket, 'paid'), false)
  await failed(
    act({
      operation: 'ticket.cancel',
      id: lifecycle.id,
      expectedRevision: 6,
      reason: 'Terminal refusal',
    }),
    'conflict',
  )
  check(
    'ready, serve and close follow explicit kitchen/service transitions, free table only at close and never assert financial settlement',
  )
  const staff = await service.mutate(
    'restaurant',
    human('staff'),
    mutation({
      operation: 'ticket.create',
      customerName: 'Staff ticket',
      mode: 'takeaway',
      lines: [{ menuId: dish.id, quantity: 1 }],
    }),
  )
  await failed(service.detail('restaurant', human('staff'), lifecycle.id), 'not_found')
  await failed(
    service.mutate(
      'restaurant',
      human('staff'),
      mutation({ operation: 'table.create', code: 'NO_ADMIN', title: 'Denied', seats: 2 }),
    ),
    'forbidden',
  )
  const delegated = { kind: 'service', id: 'bot', onBehalfOf: 'staff' }
  assert.deepEqual(
    (await service.tickets('restaurant', delegated)).items.map((x) => x.id),
    [staff.id],
  )
  await failed(
    () =>
      service.mutate(
        'restaurant',
        delegated,
        mutation({ operation: 'ticket.prepare', id: staff.id, expectedRevision: 1 }),
      ),
    'forbidden',
  )
  await owner.query(
    "UPDATE archava_commerce.memberships SET active=false WHERE tenant_id='restaurant' AND principal_id='staff'",
  )
  await failed(service.detail('restaurant', human('staff'), staff.id), 'forbidden')
  await failed(service.commandStatus('restaurant', human('staff'), staff.requestId), 'forbidden')
  await owner.query(
    "UPDATE archava_commerce.memberships SET active=true WHERE tenant_id='restaurant' AND principal_id='staff'",
  )
  await failed(service.commandStatus('restaurant', human('staff'), create.requestId), 'conflict')
  await failed(service.access('catering', human('staff')), 'forbidden')
  check(
    'current food and commerce grants intersect service/human authority and assigned ownership; inactive commerce membership immediately refuses reads, writes and receipt recovery',
  )
  const cateringStock = new consumers[1].commerce.CommerceService(
      await consumers[1].commercePg.PostgresCommerceRepository.create(app, key),
    ),
    ingredient = await cateringStock.mutate(
      'catering',
      human('owner'),
      mutation({
        operation: 'product.create',
        sku: 'RICE',
        title: 'Catering rice',
        price: 1,
        unit: 'gram',
      }),
    )
  await cateringStock.mutate(
    'catering',
    human('owner'),
    mutation({
      operation: 'stock.adjust',
      productId: ingredient.id,
      expectedRevision: 1,
      quantity: 500,
      reason: 'Catering delivery',
    }),
  )
  const cateringMenu = await second.mutate(
      'catering',
      human('owner'),
      mutation({
        operation: 'menu.create',
        sku: 'MEAL',
        title: 'Catering meal',
        price: 10000,
        recipe: [{ productId: ingredient.id, quantity: 100 }],
      }),
    ),
    cateringTicket = await second.mutate(
      'catering',
      human('owner'),
      mutation({
        operation: 'ticket.create',
        customerName: 'Catering batch',
        mode: 'takeaway',
        lines: [{ menuId: cateringMenu.id, quantity: 3 }],
      }),
    )
  await second.mutate(
    'catering',
    human('owner'),
    mutation({ operation: 'ticket.queue', id: cateringTicket.id, expectedRevision: 1 }),
  )
  await second.mutate(
    'catering',
    human('owner'),
    mutation({ operation: 'ticket.prepare', id: cateringTicket.id, expectedRevision: 2 }),
  )
  assert.equal((await cateringStock.stock('catering', human('owner'), ingredient.id)).onHand, 200)
  check(
    'independent catering context uses identical managed menu, recipe and stock execution with isolated tenant inventory and quotes',
  )
  const sql = await app.connect()
  try {
    await sql.query('BEGIN')
    await sql.query("SELECT set_config('archava.tenant_id','restaurant',true)")
    assert.equal(
      (
        await sql.query(
          "SELECT count(*)::int AS n FROM archava_foodservice.tickets WHERE tenant_id='catering'",
        )
      ).rows[0].n,
      0,
    )
    await sql.query('ROLLBACK')
    for (const statement of [
      "UPDATE archava_foodservice.recipe_components SET quantity=999 WHERE tenant_id='restaurant'",
      "UPDATE archava_foodservice.audit SET operation='rewrite' WHERE tenant_id='restaurant'",
      "UPDATE archava_foodservice.ticket_lines SET recipe_version=2 WHERE tenant_id='restaurant'",
      "UPDATE archava_commerce.products SET managed_by=NULL,revision=revision+1 WHERE tenant_id='restaurant' AND managed_by='foodservice'",
    ]) {
      await sql.query('BEGIN')
      await sql.query("SELECT set_config('archava.tenant_id','restaurant',true)")
      await assert.rejects(sql.query(statement))
      await sql.query('ROLLBACK')
    }
    await sql.query('BEGIN')
    await sql.query("SELECT set_config('archava.tenant_id','restaurant',true)")
    await assert.rejects(
      sql.query(
        'INSERT INTO archava_foodservice.recipe_components(tenant_id,menu_id,version,product_id,quantity) VALUES($1,$2,1,$3,1)',
        ['restaurant', eggDish.id, rice.id],
      ),
    )
    await sql.query('ROLLBACK')
  } finally {
    sql.release()
  }
  check(
    'forced tenant RLS, sealed recipe components, immutable ticket lines and audit, and managed product identity refuse direct SQL history or domain bypass',
  )
  const tools = domain.createFoodTools(service, 'restaurant', human('owner')),
    toolBefore = await count()
  assert.equal(
    (await tools.invoke('foodservice.detail', { id: lifecycle.id })).ticket.state,
    'closed',
  )
  await failed(tools.invoke('foodservice.ticket.prepare', { id: lifecycle.id }), 'not_found')
  await assert.rejects(() =>
    tools.invoke('foodservice.detail', { id: lifecycle.id, tenant: 'catering' }),
  )
  assert.deepEqual(await count(), toolBefore)
  assert.equal((await service.detail('restaurant', human('viewer'), lifecycle.id)).audit, null)
  const api = domain.createFoodApi(service, () => Promise.resolve(human('owner'))),
    url = 'http://localhost/api/foodservice/restaurant/'
  assert.equal((await api(new Request(url + 'tickets?scope=all'))).status, 400)
  assert.equal(
    (
      await api(
        new Request(url + 'commands', {
          method: 'POST',
          headers: { cookie: 'session=fake' },
          body: '{}',
        }),
      )
    ).status,
    403,
  )
  check(
    'private read-only tools and strict bounded API refuse authority overrides and mutations; audit is hidden without a current audit grant',
  )
  const expected = await service.detail('restaurant', human('owner'), lifecycle.id),
    archive = join(scratch, 'food.dump'),
    beforeRestart = await count()
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
  app = new Pool({ ...target, user: 'food_app' })
  service = new domain.FoodService(await postgres.PostgresFoodRepository.create(app, key))
  assert.deepEqual(await service.detail('restaurant', human('owner'), lifecycle.id), expected)
  assert.deepEqual(await count(), beforeRestart)
  assert.equal(
    (await service.commandStatus('restaurant', human('owner'), prepare.requestId)).state,
    'committed',
  )
  check(
    'database restart preserves tickets, prices, sealed recipes, physical tables, consumed ingredient ledger, grants, both audits and original command receipts',
  )
  run('createdb', ['-h', socket, '-p', String(port), '-U', 'postgres', 'restored_food'])
  run('pg_restore', [
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    '--no-owner',
    '--no-privileges',
    '-d',
    'restored_food',
    archive,
  ])
  const restoredOwner = new Pool({ ...target, database: 'restored_food' }),
    restoredApp = new Pool({ ...target, database: 'restored_food', user: 'food_app' })
  try {
    await commercePg.grantCommerceRuntime(restoredOwner, 'food_app')
    await postgres.grantFoodServiceRuntime(restoredOwner, 'food_app')
    const restoredService = new domain.FoodService(
      await postgres.PostgresFoodRepository.create(restoredApp, key),
    )
    assert.deepEqual(
      await restoredService.detail('restaurant', human('owner'), lifecycle.id),
      expected,
    )
    assert.equal(
      (await restoredService.commandStatus('restaurant', human('owner'), prepare.requestId)).state,
      'committed',
    )
    await restoredOwner.query(
      "UPDATE archava_foodservice.memberships SET active=false WHERE tenant_id='restaurant' AND principal_id='owner'",
    )
    await failed(
      restoredService.commandStatus('restaurant', human('owner'), prepare.requestId),
      'forbidden',
    )
    assert.equal(
      (await service.commandStatus('restaurant', human('owner'), prepare.requestId)).state,
      'committed',
    )
  } finally {
    await endProofPool(restoredApp)
    await endProofPool(restoredOwner)
  }
  check(
    'fresh backup restore retains domain and shared stock truth plus receipt recovery while restored grant changes leave the original database intact',
  )
  await owner.query('UPDATE archava_foodservice.schema_migrations SET checksum=$1', [
    'a'.repeat(64),
  ])
  await assert.rejects(() => postgres.PostgresFoodRepository.create(app, key))
  await assert.rejects(() => postgres.migrateFoodService(owner))
  check('modified food-service migration checksum refuses repository startup and migration')
  const destination = join(root, 'dist/foodservice-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope:
          'bounded restaurant tables, versioned recipes and whole-ticket kitchen/service lifecycle',
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
