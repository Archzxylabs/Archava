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
  scratch = await mkdtemp(join(tmpdir(), 'archava-fulfillment-proof-')),
  checks = [],
  proofs = []

const check = (label) => {
  checks.push(label)
  process.stdout.write(`PASS ${checks.length} ${label}\n`)
}

const PGHOST = join(scratch, 'pg')
const PORT = 5400 + Math.floor(Math.random() * 1000)
const RUNTIME_ROLE = 'archava_fulfillment_app'
const TENANT = 'tenant-fulfillment-a'
const OTHER = 'tenant-fulfillment-b'
const KEY = 'f'.repeat(32)

const human = (id) => ({ kind: 'human', id })
const automated = (id, onBehalfOf) => ({ kind: 'service', id, onBehalfOf })
const ALL = [
  'fulfillment.read',
  'fulfillment.write',
  'fulfillment.dispatch',
  'fulfillment.produce',
  'audit.read',
]

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
  asOwner('postgres', 'CREATE DATABASE owner_fulfillment')
  asOwner('postgres', 'CREATE DATABASE service_fulfillment')

  const consumers = []
  for (const name of ['logistics', 'production']) {
    const directory = join(scratch, name)
    await mkdir(directory, { recursive: true })
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify(
        {
          name: 'independent-fulfillment-' + name,
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

    const domain = await load('@archava/fulfillment')
    const postgres = await load('@archava/fulfillment-pg')
    const browser = await build({
      entryPoints: [require_.resolve('@archava/fulfillment/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((item) =>
        /fulfillment-pg|identity-pg|crm-pg|node:/.test(item),
      ),
      false,
    )

    consumers.push({ name, directory, domain, postgres, require: require_ })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require_.resolve('@archava/fulfillment')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((item) => item.implementationSha256)).size, 1)
  check(
    'two independently installed logistics and production consumers share unchanged fulfillment code and a browser-safe dashboard',
  )

  const installed = consumers[0]
  const { Pool } = installed.require('pg')
  const { FulfillmentService, executeFulfillmentTool } = installed.domain
  const pgModule = installed.postgres

  const connect = (user, database) => new Pool({ host: PGHOST, port: PORT, user, database, max: 4 })

  const ownerPool = connect('postgres', 'owner_fulfillment')
  const servicePool = connect('postgres', 'service_fulfillment')

  await pgModule.migrateFulfillment(ownerPool)
  await pgModule.migrateFulfillment(servicePool)
  await pgModule.grantFulfillmentRuntime(ownerPool, RUNTIME_ROLE)
  await pgModule.grantFulfillmentRuntime(servicePool, RUNTIME_ROLE)

  const runtime = connect(RUNTIME_ROLE, 'service_fulfillment')
  const repository = await pgModule.PostgresFulfillmentRepository.create(runtime, KEY)
  const svc = new FulfillmentService(repository)
  const commandId = () => randomUUID()

  const seed = async (tenant, principalId, kind, permissions, scope = 'all') => {
    await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [tenant])
    await servicePool.query(
      'INSERT INTO archava_fulfillment.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5) ON CONFLICT DO NOTHING',
      [tenant, principalId, kind, permissions, scope],
    )
  }

  await seed(TENANT, 'owner', 'human', ALL)
  await seed(TENANT, 'viewer', 'human', ['fulfillment.read', 'audit.read'])
  await seed(TENANT, 'picker', 'human', ['fulfillment.read', 'fulfillment.write'])
  await seed(TENANT, 'dispatcher', 'human', [
    'fulfillment.read',
    'fulfillment.write',
    'fulfillment.dispatch',
  ])
  await seed(TENANT, 'producer', 'human', [
    'fulfillment.read',
    'fulfillment.write',
    'fulfillment.produce',
  ])
  await seed(TENANT, 'scanner', 'service', ALL)
  await seed(TENANT, 'inactive', 'human', ALL)
  await seed(OTHER, 'foreigner', 'human', ALL)
  await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
  await servicePool.query(
    "UPDATE archava_fulfillment.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  // 1. Two independent cold-installed consumers.
  const dispatch = await svc.execute(TENANT, human('owner'), {
    type: 'create_job',
    commandId: commandId(),
    jobCode: 'SHP-2026-0001',
    kind: 'dispatch',
    title: 'Restock Bandung DC',
    ownerRef: 'client-alpha',
  })
  const made = await svc.execute(TENANT, human('owner'), {
    type: 'create_job',
    commandId: commandId(),
    jobCode: 'WO-2026-0001',
    kind: 'production',
    title: 'Custom teak desk run',
    ownerRef: null,
  })
  assert.ok(dispatch.jobId && made.jobId)
  check(
    'cold-installed logistics and production consumers register bounded dispatch and made-to-order work orders',
  )

  const dispatchJob = dispatch.jobId
  const madeJob = made.jobId

  // 2. Material, output and scrap traceability.
  await svc.execute(TENANT, human('producer'), {
    type: 'record_line',
    commandId: commandId(),
    jobId: madeJob,
    direction: 'consume',
    itemRef: 'TEAK-PLANK-18',
    quantity: 18,
    disposition: 'normal',
    evidenceRef: 'BOM v2',
  })
  await svc.execute(TENANT, human('producer'), {
    type: 'record_line',
    commandId: commandId(),
    jobId: madeJob,
    direction: 'produce',
    itemRef: 'DESK-TEAK-01',
    quantity: 5,
    disposition: 'normal',
    evidenceRef: null,
  })
  await svc.execute(TENANT, human('producer'), {
    type: 'record_line',
    commandId: commandId(),
    jobId: madeJob,
    direction: 'produce',
    itemRef: 'DESK-TEAK-01',
    quantity: 1,
    disposition: 'scrap',
    evidenceRef: 'SCRAP-01',
  })
  const madeDetail = await svc.detail(TENANT, human('viewer'), madeJob)
  assert.equal(madeDetail.job.consumedCount, 18)
  assert.equal(madeDetail.job.producedCount, 5)
  assert.equal(madeDetail.job.scrapCount, 1)
  assert.equal(madeDetail.lines.length, 3)
  check('input, output and scrap quantities stay traceable through append-only job lines')

  // 3. The produce grant is separate from the generic write grant.
  await failed(
    () =>
      svc.execute(TENANT, human('picker'), {
        type: 'record_line',
        commandId: commandId(),
        jobId: madeJob,
        direction: 'consume',
        itemRef: 'TEAK-PLANK-18',
        quantity: 2,
        disposition: 'normal',
        evidenceRef: null,
      }),
    'forbidden',
  )
  check('production material lines require the dedicated produce grant')

  // 4. Client ownership stays separated.
  const second = await svc.execute(TENANT, human('owner'), {
    type: 'create_job',
    commandId: commandId(),
    jobCode: 'SHP-2026-0002',
    kind: 'dispatch',
    title: 'Restock Surabaya DC',
    ownerRef: 'client-bravo',
  })
  const secondJob = second.jobId
  const listed = await svc.listJobs(TENANT, human('viewer'), { limit: 50, offset: 0 })
  assert.equal(listed.find((job) => job.id === dispatchJob).ownerRef, 'client-alpha')
  assert.equal(listed.find((job) => job.id === secondJob).ownerRef, 'client-bravo')
  check('client ownership is recorded per job and never mixes between dispatch runs')

  // 5. The custody chain must begin at received.
  await failed(
    () =>
      svc.execute(TENANT, human('dispatcher'), {
        type: 'record_custody',
        commandId: commandId(),
        jobId: dispatchJob,
        event: 'packed',
        fromRef: 'bay-3',
        toRef: 'dock-1',
        evidenceRef: null,
        note: null,
      }),
    'custody_out_of_order',
  )
  check('custody chain refuses any stage before the goods are received')

  // 6. The full K20 workflow is recorded in strict order.
  for (const [event, fromRef, toRef] of [
    ['received', 'supplier-jaya', 'warehouse-bay-3'],
    ['stored', 'warehouse-bay-3', 'rack-a12'],
    ['picked', 'rack-a12', 'picker-desk'],
    ['packed', 'picker-desk', 'dock-1'],
    ['dispatched', 'dock-1', 'carrier-nusantara'],
    ['delivered', 'carrier-nusantara', 'client-alpha-dc'],
  ]) {
    await svc.execute(TENANT, human('dispatcher'), {
      type: 'record_custody',
      commandId: commandId(),
      jobId: dispatchJob,
      event,
      fromRef,
      toRef,
      evidenceRef: 'DO-77120',
      note: null,
    })
  }
  const chain = await svc.custody(TENANT, human('viewer'), dispatchJob)
  assert.equal(chain.length, 6)
  assert.equal(chain[0].event, 'received')
  assert.equal(chain[5].event, 'delivered')
  check('receive-store-pick-pack-dispatch-deliver custody chain is recorded in strict order')

  // 7. Monotonic chain; repeated or backwards stages are refused.
  await failed(
    () =>
      svc.execute(TENANT, human('dispatcher'), {
        type: 'record_custody',
        commandId: commandId(),
        jobId: dispatchJob,
        event: 'packed',
        fromRef: null,
        toRef: null,
        evidenceRef: null,
        note: null,
      }),
    'custody_out_of_order',
  )
  check('custody chain is monotonic and refuses repeated or backwards stages')

  // 8. Returns need a prior dispatch.
  await svc.execute(TENANT, human('dispatcher'), {
    type: 'record_custody',
    commandId: commandId(),
    jobId: secondJob,
    event: 'received',
    fromRef: 'supplier-jaya',
    toRef: 'warehouse-bay-4',
    evidenceRef: null,
    note: null,
  })
  await failed(
    () =>
      svc.execute(TENANT, human('dispatcher'), {
        type: 'record_custody',
        commandId: commandId(),
        jobId: secondJob,
        event: 'returned',
        fromRef: null,
        toRef: null,
        evidenceRef: null,
        note: null,
      }),
    'custody_out_of_order',
  )
  check('returns are refused until the goods have actually been dispatched')

  // 9. Carrier hand-off needs the dispatch grant.
  await failed(
    () =>
      svc.execute(TENANT, human('picker'), {
        type: 'record_custody',
        commandId: commandId(),
        jobId: secondJob,
        event: 'dispatched',
        fromRef: 'dock-1',
        toRef: 'carrier-nusantara',
        evidenceRef: null,
        note: null,
      }),
    'forbidden',
  )
  check('carrier hand-off requires the dedicated dispatch grant')

  // 10. Delivery findings raise the unreconciled counter.
  const finding = await svc.execute(TENANT, human('dispatcher'), {
    type: 'record_finding',
    commandId: commandId(),
    jobId: secondJob,
    category: 'delivery',
    disposition: 'partial',
    quantity: 4,
    reason: 'Two cartons short on arrival',
    evidenceRef: 'POD-11',
    reconcilesId: null,
  })
  assert.equal((await svc.detail(TENANT, human('viewer'), secondJob)).job.unreconciledCount, 1)
  check('partial delivery findings raise the unreconciled exception counter')

  // 11. The job cannot be completed while the exception is open.
  await svc.execute(TENANT, human('owner'), {
    type: 'change_job_status',
    commandId: commandId(),
    jobId: secondJob,
    expectedRevision: 1,
    targetStatus: 'released',
    reason: 'Release for dispatch',
  })
  await svc.execute(TENANT, human('owner'), {
    type: 'change_job_status',
    commandId: commandId(),
    jobId: secondJob,
    expectedRevision: 2,
    targetStatus: 'in_progress',
    reason: 'Dispatch started',
  })
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'change_job_status',
        commandId: commandId(),
        jobId: secondJob,
        expectedRevision: 3,
        targetStatus: 'completed',
        reason: 'Close the run',
      }),
    'unreconciled_exceptions',
  )
  check('completion is refused while a delivery finding is still unreconciled')

  // 12. A stale revision is a conflict, not a silent overwrite.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        type: 'change_job_status',
        commandId: commandId(),
        jobId: secondJob,
        expectedRevision: 1,
        targetStatus: 'completed',
        reason: 'Stale write',
      }),
    'conflict',
  )
  check('optimistic revision conflicts are reported instead of overwriting concurrent work')

  // 13. An accepted reconciliation clears exactly one exception.
  await svc.execute(TENANT, human('dispatcher'), {
    type: 'record_finding',
    commandId: commandId(),
    jobId: secondJob,
    category: 'delivery',
    disposition: 'accepted',
    quantity: null,
    reason: 'Short cartons rescheduled on the next trip',
    evidenceRef: null,
    reconcilesId: finding.findingId,
  })
  assert.equal((await svc.detail(TENANT, human('viewer'), secondJob)).job.unreconciledCount, 0)
  check('an accepted reconciliation clears exactly one delivery exception')

  // 14. The same finding cannot be reconciled twice.
  await failed(
    () =>
      svc.execute(TENANT, human('dispatcher'), {
        type: 'record_finding',
        commandId: commandId(),
        jobId: secondJob,
        category: 'delivery',
        disposition: 'accepted',
        quantity: null,
        reason: 'Attempting a second reconciliation',
        evidenceRef: null,
        reconcilesId: finding.findingId,
      }),
    'conflict',
  )
  check('a delivery finding can be reconciled at most once')

  // 15. The completed job is frozen.
  await svc.execute(TENANT, human('owner'), {
    type: 'change_job_status',
    commandId: commandId(),
    jobId: secondJob,
    expectedRevision: 3,
    targetStatus: 'completed',
    reason: 'Findings reconciled, close the run',
  })
  await failed(
    () =>
      svc.execute(TENANT, human('picker'), {
        type: 'record_line',
        commandId: commandId(),
        jobId: secondJob,
        direction: 'consume',
        itemRef: 'CARTON-STD',
        quantity: 3,
        disposition: 'normal',
        evidenceRef: null,
      }),
    'invalid_transition',
  )
  check('completed jobs refuse further evidence')

  // 16. Idempotent replay.
  const stableRequest = {
    type: 'record_custody',
    commandId: commandId(),
    jobId: madeJob,
    event: 'received',
    fromRef: 'supplier-jaya',
    toRef: 'workshop-bay-1',
  }
  const first = await svc.execute(TENANT, human('owner'), stableRequest)
  const secondAttempt = await svc.execute(TENANT, human('owner'), stableRequest)
  assert.equal(secondAttempt.idempotencyReplay, true)
  assert.equal(secondAttempt.custodyId, first.custodyId)
  check('idempotent replay returns the original receipt without recording a second mutation')

  // 17. Service principals cannot mutate directly.
  await failed(
    () =>
      svc.execute(TENANT, automated('bot-auto', 'user-1'), {
        type: 'create_job',
        commandId: commandId(),
        jobCode: 'SHP-2026-9999',
        kind: 'dispatch',
        title: 'Unattended run',
        ownerRef: null,
      }),
    'forbidden',
  )
  check('service principals cannot record fulfillment mutations directly')

  // 18. Read-only tools.
  const toolJobs = await executeFulfillmentTool(
    svc,
    TENANT,
    human('viewer'),
    'fulfillment.listJobs',
    {
      limit: 50,
      offset: 0,
    },
  )
  assert.ok(toolJobs.length >= 3)
  const toolDetail = await executeFulfillmentTool(
    svc,
    TENANT,
    human('viewer'),
    'fulfillment.jobDetail',
    { jobId: dispatchJob },
  )
  assert.equal(toolDetail.job.id, dispatchJob)
  await failed(
    () =>
      executeFulfillmentTool(svc, TENANT, human('viewer'), 'fulfillment.jobDetail', {
        jobId: dispatchJob,
        tenant: OTHER,
      }),
    'bad_request',
  )
  check('read-only tools verify jobs, lines, findings and custody history without writing')

  // 19. Command status recovery.
  const recovered = await svc.getCommandStatus(TENANT, human('viewer'), first.commandId)
  assert.equal(recovered.status, 'recorded')
  assert.equal((await svc.getCommandStatus(TENANT, human('viewer'), commandId())).status, 'unknown')
  check('command status query recovers a recorded receipt after an interruption')

  // 20. Cross-tenant disclosure is refused.
  await failed(() => svc.detail(OTHER, human('foreigner'), dispatchJob), 'not_found')
  check('cross-tenant job identifiers are never disclosed')

  // 21. Inactive principals are refused.
  await failed(
    () =>
      svc.execute(TENANT, human('inactive'), {
        type: 'create_job',
        commandId: commandId(),
        jobCode: 'SHP-2026-0003',
        kind: 'dispatch',
        title: 'Disabled operator run',
        ownerRef: null,
      }),
    'forbidden',
  )
  check('inactive principals are refused')

  // 22. Unknown tools are refused.
  await failed(
    () => executeFulfillmentTool(svc, TENANT, human('owner'), 'fulfillment.dispatchNow', {}),
    'not_found',
  )
  check('unknown tool names are refused')

  // 23. Evidence is append-only at the database boundary.
  await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
  await assert.rejects(() =>
    servicePool.query('UPDATE archava_fulfillment.job_lines SET quantity = 999'),
  )
  await assert.rejects(() => servicePool.query('DELETE FROM archava_fulfillment.job_lines'))
  await assert.rejects(() => servicePool.query('DELETE FROM archava_fulfillment.jobs'))
  check('material, output, scrap and custody evidence is append-only at the database boundary')

  // 24. The runtime role is not privileged.
  const roleState = await runtime.query(
    'SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user',
  )
  assert.equal(roleState.rows[0].rolsuper, false)
  assert.equal(roleState.rows[0].rolbypassrls, false)
  check('runtime role is neither superuser nor bypass-RLS')

  // 25. Revoking a grant takes effect on the next command.
  asOwner(
    'service_fulfillment',
    `UPDATE archava_fulfillment.memberships SET permissions = ARRAY['fulfillment.read'] WHERE tenant_id = '${TENANT}' AND principal_id = 'picker'`,
  )
  await failed(
    () =>
      svc.execute(TENANT, human('picker'), {
        type: 'record_custody',
        commandId: commandId(),
        jobId: madeJob,
        event: 'stored',
        fromRef: null,
        toRef: null,
        evidenceRef: null,
        note: null,
      }),
    'forbidden',
  )
  check('grant revocation takes effect on the next command')

  // 26. Migration checksums match the release catalog.
  const ledger = await pgModule.fulfillmentMigrationLedger()
  const catalogEntry = trusted.packages.find((item) => item.name === '@archava/fulfillment-pg')
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

  const destination = join(root, 'dist/fulfillment-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'bounded dispatch custody chains and made-to-order work order evidence with resilient receipt state machine',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )

  process.stdout.write(`Fulfillment domain verified with ${checks.length} checks\n`)
} catch (error) {
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
