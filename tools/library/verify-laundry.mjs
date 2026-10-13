/* global process, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, rm, readFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomUUID, createHash } from 'node:crypto'
import { build } from 'esbuild'
import { installEnv } from './npm-env.mjs'

const root = resolve(import.meta.dirname, '../..'),
  library = join(root, 'dist/library'),
  profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8')),
  trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8')),
  env = { ...process.env },
  scratch = await mkdtemp(join(tmpdir(), 'archava-laundry-proof-')),
  checks = [],
  proofs = []

const check = (label) => {
  checks.push(label)
  process.stdout.write(`PASS ${checks.length} ${label}\n`)
}

const PGHOST = join(scratch, 'pg')
const PORT = 5400 + Math.floor(Math.random() * 1000)
const RUNTIME_ROLE = 'archava_laundry_app'
const TENANT = 'tenant-laundry-a'
const OTHER = 'tenant-laundry-b'
const KEY = 'l'.repeat(32)

const human = (id) => ({ kind: 'human', id })
const automated = (id, onBehalfOf) => ({ kind: 'service', id, onBehalfOf })
const ALL = ['laundry.read', 'laundry.write', 'laundry.process', 'audit.read']

const run = (command, args, options = {}) =>
  execFileSync(command, args, { encoding: 'utf8', env, stdio: 'pipe', ...options })

async function startCluster() {
  await mkdir(PGHOST, { recursive: true })
  run('initdb', ['-D', PGHOST, '-U', 'postgres', '--auth=trust', '--no-sync', '-E', 'UTF8'])
  run('pg_ctl', [
    '-D',
    PGHOST,
    '-l',
    join(scratch, 'postgres.log'),
    '-o',
    `-p ${PORT} -k ${PGHOST} -c listen_addresses='' -c fsync=off -c synchronous_commit=off`,
    'start',
  ])
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const probe = spawnSync('psql', [
      '-h',
      PGHOST,
      '-p',
      String(PORT),
      '-U',
      'postgres',
      '-d',
      'postgres',
      '-tAc',
      'select 1',
    ])
    if (probe.status === 0) return
    await new Promise((done) => setTimeout(done, 200))
  }
  throw new Error('PostgreSQL proof cluster did not become ready')
}

const asOwner = (database, statement) =>
  run('psql', [
    '-h',
    PGHOST,
    '-p',
    String(PORT),
    '-U',
    'postgres',
    '-d',
    database,
    '-v',
    'ON_ERROR_STOP=1',
    '-c',
    statement,
  ])

const failed = async (work, code) => {
  try {
    await work()
  } catch (error) {
    assert.equal(error.code, code, `expected ${code}, received ${error.code ?? error.message}`)
    return
  }
  throw new Error(`expected refusal with code ${code}`)
}

let clusterRunning = false

try {
  await startCluster()
  clusterRunning = true

  asOwner('postgres', `CREATE ROLE ${RUNTIME_ROLE} LOGIN`)
  asOwner('postgres', 'CREATE DATABASE owner_laundry')
  asOwner('postgres', 'CREATE DATABASE service_laundry')

  const consumers = []
  for (const name of ['kiloan', 'dryclean']) {
    const directory = join(scratch, name)
    await mkdir(directory, { recursive: true })
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify(
        {
          name: 'independent-laundry-' + name,
          private: true,
          type: 'module',
          overrides: Object.fromEntries(
            profile.catalog.externals.map((item) => [item.name, item.version]),
          ),
          dependencies: Object.fromEntries(
            profile.artifacts.map((item) => [item.name, 'file:' + join(library, item.tarball)]),
          ),
        },
        null,
        2,
      ),
    )
    run('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: directory,
      env: { ...installEnv(env), NODE_PATH: '' },
    })

    const require_ = createRequire(join(directory, 'package.json'))
    const load = async (pkg) => await import(pathToFileURL(require_.resolve(pkg)).href)

    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))

    const domain = await load('@archava/laundry')
    const postgres = await load('@archava/laundry-pg')
    const browser = await build({
      entryPoints: [require_.resolve('@archava/laundry/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((item) =>
        /laundry-pg|identity-pg|crm-pg|node:/.test(item),
      ),
      false,
    )

    consumers.push({ name, directory, domain, postgres, require: require_ })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require_.resolve('@archava/laundry')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((item) => item.implementationSha256)).size, 1)
  check(
    'two independently installed kiloan and dryclean consumers share unchanged laundry code and a browser-safe dashboard',
  )

  const installed = consumers[0]
  const { Pool } = installed.require('pg')
  const { LaundryService, executeLaundryTool } = installed.domain
  const pgModule = installed.postgres

  const connect = (user, database) => new Pool({ host: PGHOST, port: PORT, user, database, max: 4 })

  const ownerPool = connect('postgres', 'owner_laundry')
  const servicePool = connect('postgres', 'service_laundry')
  const poolDiagnostics = (pool, name) => {
    pool.on('error', (error) => process.stderr.write(`POOL-ERROR ${name}: ${error.message}\n`))
    return pool
  }
  poolDiagnostics(ownerPool, 'ownerPool')
  poolDiagnostics(servicePool, 'servicePool')

  await pgModule.migrateLaundry(ownerPool)
  await pgModule.migrateLaundry(servicePool)
  await pgModule.migrateLaundry(ownerPool)
  await pgModule.grantLaundryRuntime(ownerPool, RUNTIME_ROLE)
  await pgModule.grantLaundryRuntime(servicePool, RUNTIME_ROLE)
  check('laundry schema migrations apply cleanly, idempotently and match the packaged ledger')

  const runtime = connect(RUNTIME_ROLE, 'service_laundry')
  poolDiagnostics(runtime, 'runtimePool')
  const repository = await pgModule.PostgresLaundryRepository.create(runtime, KEY)
  const svc = new LaundryService(repository)
  const commandId = () => randomUUID()

  const seed = async (tenant, principalId, kind, permissions, scope = 'all') => {
    await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [tenant])
    await servicePool.query(
      'INSERT INTO archava_laundry.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5) ON CONFLICT DO NOTHING',
      [tenant, principalId, kind, permissions, scope],
    )
  }

  await seed(TENANT, 'owner', 'human', ALL)
  await seed(TENANT, 'viewer', 'human', ['laundry.read', 'audit.read'])
  await seed(TENANT, 'counter', 'human', ['laundry.read', 'laundry.write'])
  await seed(TENANT, 'machine', 'human', ['laundry.read', 'laundry.process'])
  await seed(TENANT, 'scanner', 'service', ALL)
  await seed(TENANT, 'inactive', 'human', ALL)
  await seed(OTHER, 'foreigner', 'human', ALL)
  await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
  await servicePool.query(
    "UPDATE archava_laundry.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  // 1. Two independent cold-installed consumers register priced orders.
  const weightOrder = await svc.execute(TENANT, human('owner'), {
    type: 'create_order',
    commandId: commandId(),
    orderCode: 'LDR-2026-0001',
    kind: 'weight',
    customerRef: 'customer-andi',
    currency: 'IDR',
    lines: [{ itemRef: 'wash-fold-kg', quantity: 3000, unit: 'gram', unitPriceMinor: 12 }],
  })
  const itemOrder = await svc.execute(TENANT, human('owner'), {
    type: 'create_order',
    commandId: commandId(),
    orderCode: 'LDR-2026-0002',
    kind: 'item',
    customerRef: 'customer-sari',
    currency: 'IDR',
    lines: [
      { itemRef: 'press-shirt', quantity: 3, unit: 'piece', unitPriceMinor: 9000 },
      { itemRef: 'dryclean-suit', quantity: 1, unit: 'piece', unitPriceMinor: 45000 },
    ],
  })
  assert.ok(weightOrder.orderId && itemOrder.orderId)
  assert.equal(weightOrder.status, 'received')
  assert.equal(itemOrder.status, 'received')
  check('cold-installed kiloan and dryclean consumers register priced weight and item orders')

  // 2. Server-computed frozen totals with split counters.
  const weightDetail = await svc.detail(TENANT, human('viewer'), weightOrder.orderId)
  assert.equal(weightDetail.order.totalMinor, 36000)
  assert.equal(weightDetail.order.weightGrams, 3000)
  assert.equal(weightDetail.order.itemCount, 0)
  assert.equal(weightDetail.order.currency, 'IDR')
  assert.equal(weightDetail.lines.length, 1)
  assert.equal(weightDetail.lines[0].lineTotalMinor, 36000)
  const itemDetail = await svc.detail(TENANT, human('viewer'), itemOrder.orderId)
  assert.equal(itemDetail.order.totalMinor, 72000)
  assert.equal(itemDetail.order.itemCount, 4)
  assert.equal(itemDetail.order.weightGrams, 0)
  assert.equal(itemDetail.lines.length, 2)
  check('server-computed totals freeze priced order intent with piece and gram counters split')

  // 3. Customer references stay separated.
  const listed = await svc.listOrders(TENANT, human('viewer'), { limit: 50, offset: 0 })
  assert.equal(
    listed.find((order) => order.id === weightOrder.orderId).customerRef,
    'customer-andi',
  )
  assert.equal(listed.find((order) => order.id === itemOrder.orderId).customerRef, 'customer-sari')
  check('customer references stay separated across independent laundry orders')

  // 4. The processing chain must begin at intake.
  await failed(
    () =>
      svc.execute(TENANT, human('machine'), {
        type: 'record_process',
        commandId: commandId(),
        orderId: itemOrder.orderId,
        event: 'wash',
        note: null,
        evidenceRef: null,
      }),
    'process_out_of_order',
  )
  check('processing chain refuses any step before intake')

  // 5. Machine events require the dedicated process grant.
  await failed(
    () =>
      svc.execute(TENANT, human('counter'), {
        type: 'record_process',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        event: 'wash',
        note: null,
        evidenceRef: null,
      }),
    'forbidden',
  )
  check('wash, dry and press require the dedicated machine process grant')

  // 6. Counter-side steps are withheld from machine-only operators.
  await failed(
    () =>
      svc.execute(TENANT, human('machine'), {
        type: 'record_process',
        commandId: commandId(),
        orderId: itemOrder.orderId,
        event: 'intake',
        note: null,
        evidenceRef: null,
      }),
    'forbidden',
  )
  check('counter-side intake requires the write grant and is withheld from machine operators')

  // 7. The kiloan workflow is recorded in strict monotonic order.
  for (const [event, actor, note, evidence] of [
    ['intake', 'counter', '3 pieces counted at counter', 'TAG-0001'],
    ['tag', 'counter', 'Tags stapled', 'TAG-0001'],
    ['wash', 'machine', 'Kiloan cycle 1', null],
    ['dry', 'machine', 'Tumble dry', null],
    ['press', 'machine', 'Fold and press', null],
  ]) {
    await svc.execute(TENANT, human(actor), {
      type: 'record_process',
      commandId: commandId(),
      orderId: weightOrder.orderId,
      event,
      note,
      evidenceRef: evidence,
    })
  }
  const chain = await svc.process(TENANT, human('viewer'), weightOrder.orderId)
  assert.equal(chain.length, 5)
  assert.equal(chain[0].event, 'intake')
  assert.equal(chain[4].event, 'press')
  check('intake-tag-wash-dry-press chain is recorded in strict monotonic order')

  // 8. Repeated or backwards steps are refused.
  await failed(
    () =>
      svc.execute(TENANT, human('counter'), {
        type: 'record_process',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        event: 'tag',
        note: null,
        evidenceRef: null,
      }),
    'process_out_of_order',
  )
  check('repeated or backwards process steps are refused')

  // 9. Quality findings raise the unreconciled counter.
  const stainFinding = await svc.execute(TENANT, human('owner'), {
    type: 'record_finding',
    commandId: commandId(),
    orderId: weightOrder.orderId,
    category: 'stain',
    disposition: 'partial',
    quantity: 2,
    reason: 'Ink stain remains after wash',
    evidenceRef: 'photo-11',
    reconcilesId: null,
  })
  assert.equal(
    (await svc.detail(TENANT, human('viewer'), weightOrder.orderId)).order.unreconciledCount,
    1,
  )
  check('quality findings raise the unreconciled exception counter')

  // 10. The order advances to processing; ready is refused while the exception is open.
  await svc.execute(TENANT, human('owner'), {
    type: 'change_order_status',
    commandId: commandId(),
    orderId: weightOrder.orderId,
    expectedRevision: 1,
    targetStatus: 'processing',
    reason: 'Batch started',
  })
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'change_order_status',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        expectedRevision: 2,
        targetStatus: 'ready',
        reason: 'Close the batch',
      }),
    'unreconciled_exceptions',
  )
  check('ready is refused while a quality finding is unreconciled')

  // 11. Stale revisions are conflicts, not silent overwrites.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'change_order_status',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        expectedRevision: 1,
        targetStatus: 'ready',
        reason: 'Stale write',
      }),
    'conflict',
  )
  check('optimistic revision conflicts are reported instead of overwriting concurrent work')

  // 12. An accepted reconciliation clears exactly one exception.
  await svc.execute(TENANT, human('owner'), {
    type: 'record_finding',
    commandId: commandId(),
    orderId: weightOrder.orderId,
    category: 'stain',
    disposition: 'accepted',
    quantity: null,
    reason: 'Stain removed on re-press',
    evidenceRef: null,
    reconcilesId: stainFinding.findingId,
  })
  assert.equal(
    (await svc.detail(TENANT, human('viewer'), weightOrder.orderId)).order.unreconciledCount,
    0,
  )
  check('an accepted reconciliation clears exactly one quality exception')

  // 13. The same finding cannot be reconciled twice.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'record_finding',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        category: 'stain',
        disposition: 'accepted',
        quantity: null,
        reason: 'Attempting a second reconciliation',
        evidenceRef: null,
        reconcilesId: stainFinding.findingId,
      }),
    'conflict',
  )
  check('a quality finding can be reconciled at most once')

  // 14. Ready is refused until a quality check exists.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'change_order_status',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        expectedRevision: 2,
        targetStatus: 'ready',
        reason: 'Batch looks done',
      }),
    'invalid_transition',
  )
  check('ready is refused until a quality check has been recorded')

  // 15. Cross-order reconciliation targets are refused.
  const otherFinding = await svc.execute(TENANT, human('owner'), {
    type: 'record_finding',
    commandId: commandId(),
    orderId: itemOrder.orderId,
    category: 'damage',
    disposition: 'partial',
    quantity: 1,
    reason: 'Loose button after press',
    evidenceRef: null,
    reconcilesId: null,
  })
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'record_finding',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        category: 'damage',
        disposition: 'accepted',
        quantity: null,
        reason: 'Compensation agreed',
        evidenceRef: null,
        reconcilesId: otherFinding.findingId,
      }),
    'conflict',
  )
  check('reconciliation targets from another order are refused')

  // 16. Quality check completes the chain and ready advances.
  await svc.execute(TENANT, human('counter'), {
    type: 'record_process',
    commandId: commandId(),
    orderId: weightOrder.orderId,
    event: 'quality_check',
    note: 'Passed inspection',
    evidenceRef: null,
  })
  const readyReceipt = await svc.execute(TENANT, human('owner'), {
    type: 'change_order_status',
    commandId: commandId(),
    orderId: weightOrder.orderId,
    expectedRevision: 2,
    targetStatus: 'ready',
    reason: 'Quality passed, ready for pickup',
  })
  assert.equal(readyReceipt.revision, 3)
  check('received-processing-ready lifecycle advances with quality-gated transitions')

  // 17. Handover completes custody and returned becomes terminal.
  for (const event of ['ready', 'handover']) {
    await svc.execute(TENANT, human('counter'), {
      type: 'record_process',
      commandId: commandId(),
      orderId: weightOrder.orderId,
      event,
      note: null,
      evidenceRef: 'RCPT-0001',
    })
  }
  await svc.execute(TENANT, human('owner'), {
    type: 'change_order_status',
    commandId: commandId(),
    orderId: weightOrder.orderId,
    expectedRevision: 3,
    targetStatus: 'returned',
    reason: 'Customer collected the order',
  })
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'record_finding',
        commandId: commandId(),
        orderId: weightOrder.orderId,
        category: 'missing',
        disposition: 'partial',
        quantity: 1,
        reason: 'Garment missing after return',
        evidenceRef: null,
        reconcilesId: null,
      }),
    'invalid_transition',
  )
  check('returned orders refuse further process or quality evidence')

  // 18. Idempotent replay of the same command.
  const stableRequest = {
    type: 'record_process',
    commandId: commandId(),
    orderId: itemOrder.orderId,
    event: 'intake',
    note: '2 pieces counted at counter',
    evidenceRef: 'TAG-0002',
  }
  const first = await svc.execute(TENANT, human('counter'), stableRequest)
  const secondAttempt = await svc.execute(TENANT, human('counter'), stableRequest)
  assert.equal(secondAttempt.idempotencyReplay, true)
  assert.equal(secondAttempt.processId, first.processId)
  assert.equal((await svc.process(TENANT, human('viewer'), itemOrder.orderId)).length, 1)
  check('idempotent replay returns the original receipt without recording a second mutation')

  // 19. An altered payload with the same command UUID is refused.
  await failed(
    () =>
      svc.execute(TENANT, human('counter'), {
        ...stableRequest,
        note: '9 pieces counted at counter',
      }),
    'conflict',
  )
  check('a replay with an altered payload is refused by fingerprint mismatch')

  // 20. Service principals cannot mutate directly.
  await failed(
    () =>
      svc.execute(TENANT, automated('bot-auto', 'user-1'), {
        type: 'create_order',
        commandId: commandId(),
        orderCode: 'LDR-2026-9999',
        kind: 'weight',
        currency: 'IDR',
        lines: [{ itemRef: 'wash-fold-kg', quantity: 1000, unit: 'gram', unitPriceMinor: 12 }],
      }),
    'forbidden',
  )
  check('service principals cannot record laundry mutations directly')

  // 21. Read-only tools, malformed input and unknown tools.
  const toolOrders = await executeLaundryTool(svc, TENANT, human('viewer'), 'laundry.listOrders', {
    limit: 50,
    offset: 0,
  })
  assert.ok(toolOrders.length >= 2)
  const toolDetail = await executeLaundryTool(svc, TENANT, human('viewer'), 'laundry.orderDetail', {
    orderId: weightOrder.orderId,
  })
  assert.equal(toolDetail.order.id, weightOrder.orderId)
  await failed(
    () =>
      executeLaundryTool(svc, TENANT, human('viewer'), 'laundry.orderDetail', {
        orderId: weightOrder.orderId,
        tenant: OTHER,
      }),
    'bad_request',
  )
  await failed(
    () => executeLaundryTool(svc, TENANT, human('owner'), 'laundry.refundMoney', {}),
    'not_found',
  )
  check(
    'read-only tools verify orders, process history and findings without writing; unknown tools are refused',
  )

  // 22. Command status recovery.
  const recovered = await svc.getCommandStatus(TENANT, human('viewer'), first.commandId)
  assert.equal(recovered.status, 'recorded')
  assert.equal((await svc.getCommandStatus(TENANT, human('viewer'), commandId())).status, 'unknown')
  check('command status query recovers a recorded receipt after an interruption')

  // 23. Cross-tenant disclosure is refused.
  await failed(() => svc.detail(OTHER, human('foreigner'), weightOrder.orderId), 'not_found')
  check('cross-tenant order identifiers are never disclosed')

  // 24. Inactive principals are refused.
  await failed(
    () =>
      svc.execute(TENANT, human('inactive'), {
        type: 'create_order',
        commandId: commandId(),
        orderCode: 'LDR-2026-9998',
        kind: 'item',
        currency: 'IDR',
        lines: [{ itemRef: 'press-shirt', quantity: 1, unit: 'piece', unitPriceMinor: 9000 }],
      }),
    'forbidden',
  )
  check('inactive principals are refused')

  // 25. Evidence is append-only and the priced intent is immutable at the database boundary.
  await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
  await assert.rejects(() =>
    servicePool.query('UPDATE archava_laundry.order_lines SET quantity = 999'),
  )
  await assert.rejects(() => servicePool.query('UPDATE archava_laundry.orders SET total_minor = 1'))
  await assert.rejects(() =>
    servicePool.query("UPDATE archava_laundry.orders SET currency = 'USD'"),
  )
  await assert.rejects(() => servicePool.query('DELETE FROM archava_laundry.process_events'))
  await assert.rejects(() => servicePool.query('DELETE FROM archava_laundry.orders'))
  check(
    'priced lines, process history and findings are append-only; order intent is immutable at the database boundary',
  )

  // 26. The runtime role is not privileged.
  const roleState = await runtime.query(
    'SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user',
  )
  assert.equal(roleState.rows[0].rolsuper, false)
  assert.equal(roleState.rows[0].rolbypassrls, false)
  check('runtime role is neither superuser nor bypass-RLS')

  // 27. Revoking a grant takes effect on the next command.
  asOwner(
    'service_laundry',
    `UPDATE archava_laundry.memberships SET permissions = ARRAY['laundry.read'] WHERE tenant_id = '${TENANT}' AND principal_id = 'counter'`,
  )
  await failed(
    () =>
      svc.execute(TENANT, human('counter'), {
        type: 'record_process',
        commandId: commandId(),
        orderId: itemOrder.orderId,
        event: 'tag',
        note: null,
        evidenceRef: null,
      }),
    'forbidden',
  )
  check('grant revocation takes effect on the next command')

  // 28. Migration checksums match the release catalog.
  const ledger = await pgModule.laundryMigrationLedger()
  const catalogEntry = trusted.packages.find((item) => item.name === '@archava/laundry-pg')
  assert.ok(catalogEntry)
  assert.deepEqual(
    catalogEntry.migrations.map((item) => item.file),
    ledger.map((item) => item.id + '.sql'),
  )
  assert.deepEqual(
    catalogEntry.migrations.map((item) => item.checksum),
    ledger.map((item) => item.checksum),
  )
  check('schema migration checksums match the release catalog')

  await ownerPool.end()
  await servicePool.end()
  await runtime.end()

  const destination = join(root, 'dist/laundry-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'laundry and dry cleaning intake, priced order intent, monotonic processing chain and quality-gated handover with resilient receipt state machine',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )

  process.stdout.write(`Laundry domain verified with ${checks.length} checks\n`)
} catch (error) {
  process.stderr.write('CAUGHT: ' + String(error?.stack ?? error) + '\n')
  try {
    process.stderr.write((await readFile(join(scratch, 'postgres.log'), 'utf8')) + '\n')
  } catch {
    /* no log available */
  }
  throw error
} finally {
  if (clusterRunning) {
    try {
      run('pg_ctl', ['-D', PGHOST, '-m', 'immediate', 'stop'])
    } catch {
      /* the cluster may already be gone */
    }
  }
  await rm(scratch, { recursive: true, force: true })
}
