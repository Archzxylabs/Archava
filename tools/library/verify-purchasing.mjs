/* global process, Request */
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
const scratch = await mkdtemp(join(tmpdir(), 'archava-purchasing-proof-'))
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
  for (const name of ['wholesale', 'restaurant']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-purchasing-' + name,
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
    const domain = await load('@archava/purchasing'),
      postgres = await load('@archava/purchasing-pg')
    Pool ??= require('pg').Pool
    const browser = await build({
      entryPoints: [require.resolve('@archava/purchasing/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /purchasing-pg|commerce-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({
      domain,
      postgres,
      name,
      directory,
      commerce: await load('@archava/commerce'),
      commercePg: await load('@archava/commerce-pg'),
    })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/purchasing')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed wholesale and restaurant consumers share unchanged purchasing code and browser-safe dashboard',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres, commerce, commercePg } = consumers[0]
  // Prove a non-empty archived release before installing the new stock migration.
  const legacyDirectory = join(scratch, 'legacy'),
    legacyLibrary = join(root, 'dist/releases/1.9.0/library'),
    legacy = JSON.parse(await readFile(join(legacyLibrary, 'release-profile.json'), 'utf8'))
  await mkdir(legacyDirectory)
  await writeFile(
    join(legacyDirectory, 'package.json'),
    JSON.stringify({
      private: true,
      type: 'module',
      dependencies: Object.fromEntries(
        legacy.artifacts.map((x) => [x.name, 'file:' + join(legacyLibrary, x.tarball)]),
      ),
      overrides: Object.fromEntries(legacy.catalog.externals.map((x) => [x.name, x.version])),
    }),
  )
  execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
    cwd: legacyDirectory,
    stdio: 'pipe',
    env: { ...installEnv(env), NODE_PATH: '' },
  })
  const oldRequire = createRequire(join(legacyDirectory, 'package.json')),
    oldLoad = async (name) => import(pathToFileURL(oldRequire.resolve(name)).href),
    oldDomain = await oldLoad('@archava/commerce'),
    oldPg = await oldLoad('@archava/commerce-pg')
  await oldPg.migrateCommerce(owner)
  await owner.query('CREATE ROLE purchasing_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await oldPg.grantCommerceRuntime(owner, 'purchasing_app')
  await owner.query(
    "INSERT INTO archava_commerce.memberships VALUES('legacy','owner','human',$1,'all',true)",
    [oldDomain.COMMERCE_PERMISSIONS],
  )
  app = new Pool({ ...target, user: 'purchasing_app' })
  const oldService = new oldDomain.CommerceService(
    await oldPg.PostgresCommerceRepository.create(app, key),
  )
  const historicalProduct = await oldService.mutate(
    'legacy',
    human('owner'),
    mutation({
      operation: 'product.create',
      sku: 'OLD',
      title: 'Old record',
      unit: 'unit',
      trackStock: true,
      price: 1700,
    }),
  )
  await oldService.mutate(
    'legacy',
    human('owner'),
    mutation({
      operation: 'stock.adjust',
      productId: historicalProduct.id,
      expectedRevision: 1,
      quantity: 20,
      reason: 'Original receipt',
    }),
  )
  const historicalInput = mutation({
      operation: 'order.create',
      customerName: 'Synthetic historical customer',
      lines: [{ productId: historicalProduct.id, quantity: 2 }],
    }),
    historicalOrder = await oldService.mutate('legacy', human('owner'), historicalInput)
  await oldService.mutate(
    'legacy',
    human('owner'),
    mutation({
      operation: 'order.reserve',
      id: historicalOrder.id,
      expectedRevision: 1,
      holdSeconds: 600,
    }),
  )
  await oldService.mutate(
    'legacy',
    human('owner'),
    mutation({ operation: 'order.fulfill', id: historicalOrder.id, expectedRevision: 2 }),
  )
  const beforeTables = (
    await owner.query(
      "SELECT table_name FROM information_schema.tables WHERE table_schema='archava_commerce' AND table_type='BASE TABLE' AND table_name<>'schema_migrations' ORDER BY table_name",
    )
  ).rows
  const snapshots = []
  const quote = (x) => '"' + x.replaceAll('"', '""') + '"',
    hash = (x) => createHash('sha256').update(JSON.stringify(x)).digest('hex')
  for (const r of beforeTables) {
    const columns = (
        await owner.query(
          "SELECT column_name FROM information_schema.columns WHERE table_schema='archava_commerce' AND table_name=$1 ORDER BY ordinal_position",
          [r.table_name],
        )
      ).rows.map((x) => x.column_name),
      rows = (
        await owner.query(
          'SELECT to_jsonb(t) AS record FROM (SELECT ' +
            columns.map(quote).join(',') +
            ' FROM archava_commerce.' +
            quote(r.table_name) +
            ') t',
        )
      ).rows
        .map((x) => hash(x.record))
        .sort()
    snapshots.push({ table: r.table_name, columns, rows })
  }
  const oldLedger = (
    await owner.query('SELECT id,checksum FROM archava_commerce.schema_migrations ORDER BY id')
  ).rows
  await commercePg.migrateCommerce(owner)
  await commercePg.migrateCommerce(owner)
  await postgres.migratePurchasing(owner)
  await postgres.migratePurchasing(owner)
  for (const s of snapshots)
    assert.deepEqual(
      (
        await owner.query(
          'SELECT to_jsonb(t) AS record FROM (SELECT ' +
            s.columns.map(quote).join(',') +
            ' FROM archava_commerce.' +
            quote(s.table) +
            ') t',
        )
      ).rows
        .map((x) => hash(x.record))
        .sort(),
      s.rows,
    )
  assert.deepEqual(
    (
      await owner.query('SELECT id,checksum FROM archava_commerce.schema_migrations ORDER BY id')
    ).rows.slice(0, oldLedger.length),
    oldLedger,
  )
  await commercePg.grantCommerceRuntime(owner, 'purchasing_app')
  await postgres.grantPurchasingRuntime(owner, 'purchasing_app')
  let sales = new commerce.CommerceService(
      await commercePg.PostgresCommerceRepository.create(app, key),
    ),
    service = new domain.PurchasingService(
      await postgres.PostgresPurchaseRepository.create(app, key),
    )
  assert.equal(
    (await sales.commandStatus('legacy', human('owner'), historicalInput.requestId)).receipt.id,
    historicalOrder.id,
  )
  assert.equal(
    (await sales.detail('legacy', human('owner'), historicalOrder.id)).order.totalEstimate,
    3400,
  )
  check(
    'additive stock migration preserves all original commerce rows, columns, grants, quotes, audits and UUID recovery from non-empty archived 1.9 release',
  )
  assert.equal(
    (await owner.query('SELECT count(*)::integer AS n FROM archava_purchasing.memberships')).rows[0]
      .n,
    0,
  )
  assert.equal(
    (
      await owner.query(
        "SELECT count(*)::integer AS n FROM pg_class WHERE relnamespace='archava_purchasing'::regnamespace AND relrowsecurity AND relforcerowsecurity",
      )
    ).rows[0].n,
    9,
  )
  await assert.rejects(() => postgres.PostgresPurchaseRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresPurchaseRepository.create(app, 'short'))
  check(
    'nine independent purchasing business tables use forced tenant RLS; unsafe runtime and short keys fail startup and migration enrolls no users',
  )
  for (const tenant of ['wholesale', 'restaurant'])
    for (const [id, kind, permissions, scope] of [
      ['owner', 'human', domain.PURCHASE_PERMISSIONS, 'all'],
      [
        'viewer',
        'human',
        domain.PURCHASE_PERMISSIONS.filter((x) => x.endsWith('.read') && x !== 'audit.read'),
        'all',
      ],
      ['staff', 'human', domain.PURCHASE_PERMISSIONS, 'assigned'],
      ['bot', 'service', domain.PURCHASE_PERMISSIONS.filter((x) => x.endsWith('.read')), 'all'],
    ]) {
      await owner.query(
        'INSERT INTO archava_purchasing.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
        [tenant, id, kind, permissions, scope],
      )
      await owner.query(
        'INSERT INTO archava_commerce.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
        [
          tenant,
          id,
          kind,
          kind === 'service'
            ? commerce.COMMERCE_PERMISSIONS.filter((x) => x.endsWith('.read'))
            : commerce.COMMERCE_PERMISSIONS,
          'all',
        ],
      )
    }
  const t = 'wholesale',
    ownerPerson = human('owner'),
    write = (command) => service.mutate(t, ownerPerson, mutation(command)),
    sale = (command) => sales.mutate(t, ownerPerson, mutation(command))
  const p1 = await sale({
      operation: 'product.create',
      sku: 'FLOUR',
      title: 'Flour',
      unit: 'gram',
      trackStock: true,
      price: 90,
    }),
    p2 = await sale({
      operation: 'product.create',
      sku: 'BOX',
      title: 'Box',
      unit: 'unit',
      trackStock: true,
      price: 500,
    })
  const supplierInput = mutation({
      operation: 'supplier.create',
      code: 'SUPPLIER',
      name: 'Supply Co',
    }),
    duplicates = await Promise.all(
      Array.from({ length: 20 }, () => service.mutate(t, ownerPerson, supplierInput)),
    ),
    supplierId = duplicates[0].id
  assert.equal(new Set(duplicates.map((x) => x.id)).size, 1)
  assert.equal((await service.suppliers(t, ownerPerson)).items.length, 1)
  await failed(
    service.mutate(t, ownerPerson, {
      ...supplierInput,
      command: { ...supplierInput.command, name: 'Changed' },
    }),
    'conflict',
  )
  await failed(service.mutate(t, human('viewer'), supplierInput), 'forbidden')
  await failed(service.commandStatus(t, human('staff'), supplierInput.requestId), 'conflict')
  assert.equal((await service.commandStatus(t, ownerPerson, randomUUID())).state, 'unknown')
  check(
    '20 duplicate supplier creations commit one record and audit; same UUID refuses changed input and another actor without granting execution for unknown receipts',
  )
  const a = await write({ operation: 'offer.create', supplierId, productId: p1.id, unitCost: 15 }),
    b = await write({ operation: 'offer.create', supplierId, productId: p2.id, unitCost: 200 })
  const po = await write({
    operation: 'order.create',
    supplierId,
    lines: [
      { offerId: a.id, quantity: 100 },
      { offerId: b.id, quantity: 4 },
    ],
  })
  assert.equal((await service.detail(t, ownerPerson, po.id)).order.totalEstimate, 2300)
  await write({
    operation: 'offer.set-cost',
    id: a.id,
    expectedRevision: 1,
    unitCost: 17,
    reason: 'New supply rate',
  })
  await failed(
    write({
      operation: 'order.confirm',
      id: po.id,
      expectedRevision: 1,
      reason: 'Confirm stale price',
    }),
    'conflict',
  )
  assert.equal((await service.detail(t, ownerPerson, po.id)).order.revision, 1)
  await write({
    operation: 'order.replace-lines',
    id: po.id,
    expectedRevision: 1,
    lines: [
      { offerId: a.id, quantity: 100 },
      { offerId: b.id, quantity: 4 },
    ],
    reason: 'Review refreshed quote',
  })
  assert.equal((await service.detail(t, ownerPerson, po.id)).order.totalEstimate, 2500)
  await write({
    operation: 'order.confirm',
    id: po.id,
    expectedRevision: 2,
    reason: 'Reviewed quote',
  })
  await write({
    operation: 'offer.set-cost',
    id: a.id,
    expectedRevision: 2,
    unitCost: 99,
    reason: 'Future price',
  })
  assert.equal((await service.detail(t, ownerPerson, po.id)).order.totalEstimate, 2500)
  assert.equal((await sales.products(t, ownerPerson)).items.find((x) => x.id === p1.id).price, 90)
  check(
    'purchase quotes use backend supplier costs distinct from sales prices; stale drafts refuse confirmation, explicit refresh is reviewed and confirmed quotes survive later rate changes',
  )
  const confirmed = await service.detail(t, ownerPerson, po.id),
    l1 = confirmed.lines.find((x) => x.productId === p1.id),
    l2 = confirmed.lines.find((x) => x.productId === p2.id)
  const receive = mutation({
    operation: 'order.receive',
    id: po.id,
    expectedRevision: 3,
    reason: 'Delivery A inspected',
    lines: [
      { lineId: l1.id, accepted: 60, rejected: 5 },
      { lineId: l2.id, accepted: 1, rejected: 0 },
    ],
  })
  const receipts = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate(t, ownerPerson, receive)),
  )
  assert.equal(new Set(receipts.map((x) => x.revision)).size, 1)
  assert.equal((await sales.stock(t, ownerPerson, p1.id)).onHand, 60)
  assert.equal((await sales.stock(t, ownerPerson, p2.id)).onHand, 1)
  const partial = await service.detail(t, ownerPerson, po.id)
  assert.equal(partial.order.state, 'partial')
  assert.equal(partial.receipts.length, 2)
  assert.equal(partial.lines.find((x) => x.id === l1.id).rejectedQuantity, 5)
  const movements = await sales.movements(t, ownerPerson, p1.id)
  assert.equal(movements.items.filter((x) => x.kind === 'purchase').length, 1)
  assert.equal(movements.items[0].purchaseId, po.id)
  assert.equal((await service.commandStatus(t, ownerPerson, receive.requestId)).receipt.revision, 4)
  await failed(sales.commandStatus(t, ownerPerson, receive.requestId), 'forbidden')
  check(
    '20 duplicate partial receipts commit one source-bound purchase receipt and one inbound movement per accepted product; rejected goods never become usable stock or generic command authority',
  )
  await failed(
    write({
      operation: 'order.receive',
      id: po.id,
      expectedRevision: 4,
      reason: 'Excess',
      lines: [{ lineId: l1.id, accepted: 36, rejected: 0 }],
    }),
    'conflict',
  )
  await failed(
    write({
      operation: 'order.cancel',
      id: po.id,
      expectedRevision: 4,
      reason: 'Cannot erase physical receipt',
    }),
    'conflict',
  )
  assert.equal((await sales.stock(t, ownerPerson, p1.id)).onHand, 60)
  check(
    'over-receipt and cancellation after physical findings fail atomically without changing stock, purchase totals or original evidence',
  )
  const competing = await Promise.allSettled(
    Array.from({ length: 20 }, () =>
      write({
        operation: 'order.receive',
        id: po.id,
        expectedRevision: 4,
        reason: 'Concurrent next arrival',
        lines: [{ lineId: l1.id, accepted: 10, rejected: 0 }],
      }),
    ),
  )
  assert.equal(competing.filter((x) => x.status === 'fulfilled').length, 1)
  assert.ok(
    competing.filter((x) => x.status === 'rejected').every((x) => x.reason.code === 'conflict'),
  )
  assert.equal((await sales.stock(t, ownerPerson, p1.id)).onHand, 70)
  check(
    '20 different commands for one purchase revision have one physical winner; stale findings roll back both module commands, audit and stock',
  )
  await write({
    operation: 'order.receive',
    id: po.id,
    expectedRevision: 5,
    reason: 'Final arrival inspected',
    lines: [
      { lineId: l1.id, accepted: 25, rejected: 0 },
      { lineId: l2.id, accepted: 2, rejected: 1 },
    ],
  })
  const completed = await service.detail(t, ownerPerson, po.id)
  assert.equal(completed.order.state, 'received')
  assert.equal((await sales.stock(t, ownerPerson, p1.id)).onHand, 95)
  assert.equal((await sales.stock(t, ownerPerson, p2.id)).onHand, 3)
  await failed(
    write({
      operation: 'order.receive',
      id: po.id,
      expectedRevision: 6,
      reason: 'Duplicate delivery with new ID',
      lines: [{ lineId: l1.id, accepted: 1, rejected: 0 }],
    }),
    'conflict',
  )
  await failed(
    write({
      operation: 'order.replace-lines',
      id: po.id,
      expectedRevision: 6,
      lines: [{ offerId: a.id, quantity: 1 }],
      reason: 'Change confirmed quote',
    }),
    'conflict',
  )
  check(
    'received means all ordered units have explicit accepted/rejected findings; confirmation freezes lines and completed orders cannot receive more goods',
  )
  const short = await write({
    operation: 'order.create',
    supplierId,
    lines: [{ offerId: a.id, quantity: 10 }],
  })
  await write({
    operation: 'order.confirm',
    id: short.id,
    expectedRevision: 1,
    reason: 'Confirmed',
  })
  const shortLine = (await service.detail(t, ownerPerson, short.id)).lines[0]
  await write({
    operation: 'order.receive',
    id: short.id,
    expectedRevision: 2,
    reason: 'Partial arrival',
    lines: [{ lineId: shortLine.id, accepted: 2, rejected: 0 }],
  })
  await write({
    operation: 'order.short-close',
    id: short.id,
    expectedRevision: 3,
    reason: 'Supplier cannot send balance',
  })
  await failed(
    write({
      operation: 'order.receive',
      id: short.id,
      expectedRevision: 4,
      reason: 'Closed',
      lines: [{ lineId: shortLine.id, accepted: 1, rejected: 0 }],
    }),
    'conflict',
  )
  assert.equal((await sales.stock(t, ownerPerson, p1.id)).onHand, 97)
  const reject = await write({
    operation: 'order.create',
    supplierId,
    lines: [{ offerId: b.id, quantity: 2 }],
  })
  await write({
    operation: 'order.confirm',
    id: reject.id,
    expectedRevision: 1,
    reason: 'Confirmed',
  })
  await write({
    operation: 'order.receive',
    id: reject.id,
    expectedRevision: 2,
    reason: 'All damaged',
    lines: [
      {
        lineId: (await service.detail(t, ownerPerson, reject.id)).lines[0].id,
        accepted: 0,
        rejected: 2,
      },
    ],
  })
  assert.equal((await sales.stock(t, ownerPerson, p2.id)).onHand, 3)
  check(
    'short closure retains earlier stock and prevents later receipts; an all-rejected delivery records both audits with zero stock movements',
  )
  const expensive = await write({
    operation: 'offer.set-cost',
    id: b.id,
    expectedRevision: 1,
    unitCost: 1000000000,
    reason: 'Large cost boundary',
  })
  assert.equal(expensive.revision, 2)
  await failed(
    write({ operation: 'order.create', supplierId, lines: [{ offerId: b.id, quantity: 1000000 }] }),
    'conflict',
  )
  await write({
    operation: 'offer.set-cost',
    id: b.id,
    expectedRevision: 2,
    unitCost: 200,
    reason: 'Restore synthetic cost',
  })
  const capped = await sale({
    operation: 'product.create',
    sku: 'CAPPED',
    title: 'Full stock',
    unit: 'unit',
    trackStock: true,
    price: 1,
  })
  await sale({
    operation: 'stock.adjust',
    productId: capped.id,
    expectedRevision: 1,
    quantity: 1000000000,
    reason: 'Capacity boundary',
  })
  const cappedOffer = await write({
      operation: 'offer.create',
      supplierId,
      productId: capped.id,
      unitCost: 1,
    }),
    overflow = await write({
      operation: 'order.create',
      supplierId,
      lines: [
        { offerId: cappedOffer.id, quantity: 1 },
        { offerId: a.id, quantity: 1 },
      ],
    })
  await write({
    operation: 'order.confirm',
    id: overflow.id,
    expectedRevision: 1,
    reason: 'Confirmed',
  })
  const overflowDetail = await service.detail(t, ownerPerson, overflow.id)
  const prevOnHand = (await sales.stock(t, ownerPerson, p1.id)).onHand
  await failed(
    write({
      operation: 'order.receive',
      id: overflow.id,
      expectedRevision: 2,
      reason: 'Overflow rolls back all SKUs',
      lines: overflowDetail.lines.map((l) => ({ lineId: l.id, accepted: 1, rejected: 0 })),
    }),
    'conflict',
  )
  assert.equal((await sales.stock(t, ownerPerson, p1.id)).onHand, prevOnHand)
  assert.equal((await service.detail(t, ownerPerson, overflow.id)).receipts.length, 0)
  check(
    'bounded backend totals and stock capacity overflow fail with atomic multi-product rollback and no partial physical receipt',
  )
  const snap = async () => {
    const out = {}
    for (const schema of ['archava_purchasing', 'archava_commerce'])
      for (const r of (
        await owner.query(
          "SELECT table_name FROM information_schema.tables WHERE table_schema=$1 AND table_type='BASE TABLE' ORDER BY table_name",
          [schema],
        )
      ).rows)
        out[schema + '.' + r.table_name] = (
          await owner.query(
            'SELECT to_jsonb(t) AS record FROM ' + schema + '.' + quote(r.table_name) + ' t',
          )
        ).rows
          .map((x) => hash(x.record))
          .sort()
    return out
  }
  const rollback = await write({
    operation: 'order.create',
    supplierId,
    lines: [{ offerId: a.id, quantity: 3 }],
  })
  await write({
    operation: 'order.confirm',
    id: rollback.id,
    expectedRevision: 1,
    reason: 'Confirmed',
  })
  const rollbackInput = mutation({
      operation: 'order.receive',
      id: rollback.id,
      expectedRevision: 2,
      reason: 'Injected persistence failure',
      lines: [
        {
          lineId: (await service.detail(t, ownerPerson, rollback.id)).lines[0].id,
          accepted: 3,
          rejected: 0,
        },
      ],
    }),
    before = await snap()
  await owner.query(
    "CREATE FUNCTION archava_purchasing.fail_proof_audit() RETURNS trigger LANGUAGE plpgsql AS $$BEGIN RAISE EXCEPTION 'Injected failure';END $$;CREATE TRIGGER fail_proof BEFORE INSERT ON archava_purchasing.audit FOR EACH ROW EXECUTE FUNCTION archava_purchasing.fail_proof_audit()",
  )
  await assert.rejects(() => service.mutate(t, ownerPerson, rollbackInput))
  assert.deepEqual(await snap(), before)
  await owner.query(
    'DROP TRIGGER fail_proof ON archava_purchasing.audit;DROP FUNCTION archava_purchasing.fail_proof_audit()',
  )
  await service.mutate(t, ownerPerson, rollbackInput)
  assert.equal((await service.detail(t, ownerPerson, rollback.id)).order.state, 'received')
  check(
    'failure after purchase findings and shared stock writes rolls back all rows in both schemas; the original UUID can safely retry',
  )
  const own = await service.mutate(
    t,
    human('staff'),
    mutation({ operation: 'order.create', supplierId, lines: [{ offerId: a.id, quantity: 1 }] }),
  )
  assert.equal((await service.orders(t, human('staff'))).items.length, 1)
  await failed(service.detail(t, human('staff'), po.id), 'not_found')
  await failed(service.detail('restaurant', ownerPerson, po.id), 'not_found')
  await failed(
    () =>
      service.mutate(
        t,
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        mutation({ operation: 'supplier.create', code: 'FORGED', name: 'Bot' }),
      ),
    'forbidden',
  )
  await failed(
    () =>
      service.commandStatus(
        t,
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        receive.requestId,
      ),
    'forbidden',
  )
  const bot = { kind: 'service', id: 'bot', onBehalfOf: 'staff' }
  assert.equal((await service.orders(t, bot)).items.length, 1)
  await owner.query(
    "UPDATE archava_purchasing.memberships SET active=false WHERE tenant_id=$1 AND principal_id='staff'",
    [t],
  )
  await failed(service.orders(t, bot), 'forbidden')
  await failed(service.commandStatus(t, human('staff'), own.requestId), 'forbidden')
  await owner.query(
    "UPDATE archava_purchasing.memberships SET active=true WHERE tenant_id=$1 AND principal_id='staff'",
    [t],
  )
  await owner.query(
    "UPDATE archava_commerce.memberships SET permissions=array_remove(permissions,'stock.write') WHERE tenant_id=$1 AND principal_id='owner'",
    [t],
  )
  await failed(service.commandStatus(t, ownerPerson, receive.requestId), 'forbidden')
  await owner.query(
    "UPDATE archava_commerce.memberships SET permissions=$2 WHERE tenant_id=$1 AND principal_id='owner'",
    [t, commerce.COMMERCE_PERMISSIONS],
  )
  check(
    'current independent purchasing/commerce grants, creator scope and service/human intersection govern reads and recovery; machines receive no write authority',
  )
  const service2 = new consumers[1].domain.PurchasingService(
      await consumers[1].postgres.PostgresPurchaseRepository.create(app, key),
    ),
    sales2 = new consumers[1].commerce.CommerceService(
      await consumers[1].commercePg.PostgresCommerceRepository.create(app, key),
    ),
    rProduct = await sales2.mutate(
      'restaurant',
      ownerPerson,
      mutation({
        operation: 'product.create',
        sku: 'RICE',
        title: 'Rice',
        unit: 'gram',
        trackStock: true,
        price: 12,
      }),
    ),
    rSupplier = await service2.mutate(
      'restaurant',
      ownerPerson,
      mutation({ operation: 'supplier.create', code: 'FARM', name: 'Farm' }),
    ),
    rOffer = await service2.mutate(
      'restaurant',
      ownerPerson,
      mutation({
        operation: 'offer.create',
        supplierId: rSupplier.id,
        productId: rProduct.id,
        unitCost: 8,
      }),
    ),
    rPo = await service2.mutate(
      'restaurant',
      ownerPerson,
      mutation({
        operation: 'order.create',
        supplierId: rSupplier.id,
        lines: [{ offerId: rOffer.id, quantity: 1000 }],
      }),
    )
  await service2.mutate(
    'restaurant',
    ownerPerson,
    mutation({
      operation: 'order.confirm',
      id: rPo.id,
      expectedRevision: 1,
      reason: 'Ingredients',
    }),
  )
  await service2.mutate(
    'restaurant',
    ownerPerson,
    mutation({
      operation: 'order.receive',
      id: rPo.id,
      expectedRevision: 2,
      reason: 'Ingredient delivery',
      lines: [
        {
          lineId: (await service2.detail('restaurant', ownerPerson, rPo.id)).lines[0].id,
          accepted: 1000,
          rejected: 0,
        },
      ],
    }),
  )
  assert.equal((await sales2.stock('restaurant', ownerPerson, rProduct.id)).onHand, 1000)
  check(
    'independent restaurant ingredient buying uses the same supplier/order/receipt implementation and shared commerce stock without a fork',
  )
  const api = domain.createPurchasingApi(service, () => Promise.resolve(ownerPerson)),
    url = 'http://localhost/api/purchasing/wholesale'
  for (const path of [
    '/orders?tenantId=restaurant',
    '/offers?active=unknown',
    '/orders?limit=2&limit=3',
  ])
    assert.equal((await api(new Request(url + path))).status, 400)
  assert.equal(
    (
      await api(
        new Request(url + '/tools', {
          method: 'POST',
          headers: { origin: 'http://localhost', 'content-type': 'application/json' },
          body: JSON.stringify({ name: 'purchasing.receive', input: {} }),
        }),
      )
    ).status,
    404,
  )
  const beforeTools = await snap()
  await domain
    .createPurchasingTools(service, t, ownerPerson)
    .invoke('purchasing.detail', { id: po.id })
  assert.deepEqual(await snap(), beforeTools)
  const redacted = await service.detail(t, human('viewer'), po.id)
  assert.equal(redacted.audit, null)
  assert.equal(redacted.receipts.length, completed.receipts.length)
  check(
    'strict bounded API and read-only tools refuse caller authority and mutations; private cost/history projections use separate current permissions and produce no provider calls',
  )
  const sqlRefuse = async (sql) => {
    const c = await app.connect()
    try {
      await c.query('BEGIN')
      await c.query("SELECT set_config('archava.tenant_id',$1,true)", [t])
      await assert.rejects(async () => {
        await c.query(sql)
        await c.query('COMMIT')
      })
    } finally {
      await c.query('ROLLBACK')
      c.release()
    }
  }
  await sqlRefuse("UPDATE archava_purchasing.receipts SET reason='Rewritten'")
  await sqlRefuse('UPDATE archava_purchasing.lines SET unit_cost=0')
  await sqlRefuse("UPDATE archava_purchasing.commands SET receipt='{}'")
  await sqlRefuse("UPDATE archava_purchasing.audit SET reason='Rewritten'")
  await sqlRefuse(
    "INSERT INTO archava_purchasing.suppliers(tenant_id,id,code,name) VALUES('wholesale','" +
      randomUUID() +
      "','NOAUDIT','Unbound')",
  )
  await sqlRefuse(
    "INSERT INTO archava_commerce.movements(tenant_id,request_id,product_id,kind,quantity,purchase_id,reason) VALUES('wholesale','" +
      receive.requestId +
      "','" +
      p1.id +
      "','purchase',1,'" +
      po.id +
      "','Extra units')",
  )
  const c = await app.connect()
  try {
    await c.query('BEGIN')
    await c.query("SELECT set_config('archava.tenant_id',$1,true)", [t])
    assert.equal(
      (
        await c.query(
          "SELECT count(*)::integer AS n FROM archava_purchasing.orders WHERE tenant_id='restaurant'",
        )
      ).rows[0].n,
      0,
    )
  } finally {
    await c.query('ROLLBACK')
    c.release()
  }
  check(
    'forced tenant RLS, immutable confirmed lines and findings, full revision/audit receipts and exact inbound quantity binding resist direct runtime SQL bypass',
  )
  const expected = await snap()
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  start()
  owner = new Pool(target)
  app = new Pool({ ...target, user: 'purchasing_app' })
  service = new domain.PurchasingService(await postgres.PostgresPurchaseRepository.create(app, key))
  sales = new commerce.CommerceService(await commercePg.PostgresCommerceRepository.create(app, key))
  assert.deepEqual(await snap(), expected)
  assert.equal((await service.commandStatus(t, ownerPerson, receive.requestId)).receipt.revision, 4)
  check(
    'database restart preserves supplier quotes, purchase/inspection evidence, shared physical stock, current grants and actor-bound UUID recovery',
  )
  const archive = join(scratch, 'backup.dump')
  run('pg_dump', [
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    '-d',
    'postgres',
    '-Fc',
    '-f',
    archive,
  ])
  run('createdb', ['-h', socket, '-p', String(port), '-U', 'postgres', 'restored'])
  run('pg_restore', [
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    '-d',
    'restored',
    '--no-owner',
    archive,
  ])
  const restored = new Pool({ ...target, user: 'purchasing_app', database: 'restored' }),
    restoredOwner = new Pool({ ...target, database: 'restored' })
  try {
    const restoredService = new domain.PurchasingService(
        await postgres.PostgresPurchaseRepository.create(restored, key),
      ),
      restoredSales = new commerce.CommerceService(
        await commercePg.PostgresCommerceRepository.create(restored, key),
      )
    assert.deepEqual(
      await restoredService.detail(t, ownerPerson, po.id),
      await service.detail(t, ownerPerson, po.id),
    )
    assert.equal(
      (await restoredSales.stock(t, ownerPerson, p1.id)).onHand,
      (await sales.stock(t, ownerPerson, p1.id)).onHand,
    )
    assert.equal(
      (await restoredService.commandStatus(t, ownerPerson, receive.requestId)).receipt.revision,
      4,
    )
    await restoredOwner.query(
      "UPDATE archava_purchasing.memberships SET active=false WHERE tenant_id=$1 AND principal_id='owner'",
      [t],
    )
    await failed(restoredService.commandStatus(t, ownerPerson, receive.requestId), 'forbidden')
    assert.equal(
      (await service.commandStatus(t, ownerPerson, receive.requestId)).state,
      'committed',
    )
  } finally {
    await endProofPool(restored)
    await endProofPool(restoredOwner)
  }
  check(
    'fresh backup restoration retains purchase/stock provenance and receipts while independent restored grant revocation leaves the original untouched',
  )
  await owner.query("UPDATE archava_purchasing.schema_migrations SET checksum='changed'")
  await assert.rejects(() => postgres.PostgresPurchaseRepository.create(app, key))
  await assert.rejects(() => postgres.migratePurchasing(owner))
  check('modified purchasing migration checksum refuses startup and migration')
  const destination = join(root, 'dist/purchasing-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope:
          'bounded supplier offers, immutable purchase orders and partial physical stock receipts',
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
