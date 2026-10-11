/* global process */
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

const root = resolve(import.meta.dirname, '../..')
const library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-capacity-proof-'))
const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('PG')))
const run = (name, args) =>
  execFileSync(join(bin, name), args, { encoding: 'utf8', stdio: 'pipe', env })

const socket = join(scratch, 'socket')
const data = join(scratch, 'data')
const port = 20000 + Math.floor(Math.random() * 25000)
await mkdir(socket)

let running = false
let owner
let app
let Pool

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

const key = randomBytes(32).toString('hex')
const checks = []
const proofs = []

const check = (label) => {
  checks.push(label)
  process.stdout.write('PASS ' + label + '\n')
}

const human = (id) => ({ kind: 'human', id })
const failed = async (fn, code) => {
  try {
    await (typeof fn === 'function' ? fn() : fn)
    assert.fail(`Expected rejection with code ${code}, but call succeeded`)
  } catch (error) {
    if (error?.code !== code) {
      throw error
    }
    assert.equal(error.code, code)
  }
}

try {
  const consumers = []
  // Two independent consumers: Agency (K05) and Construction/EPC (K31)
  for (const name of ['agency', 'epc']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-capacity-' + name,
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

    const require = createRequire(join(directory, 'package.json'))
    const load = async (pkg) => import(pathToFileURL(require.resolve(pkg)).href)

    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))

    const domain = await load('@archava/capacity')
    const postgres = await load('@archava/capacity-pg')
    Pool ??= require('pg').Pool

    const browser = await build({
      entryPoints: [require.resolve('@archava/capacity/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })

    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /capacity-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )

    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/capacity')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }

  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed agency and EPC consumers share unchanged capacity code and browser-safe dashboard',
  )

  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)

  const { domain, postgres } = consumers[0]
  await postgres.migrateCapacity(owner)
  // Idempotent migration
  await postgres.migrateCapacity(owner)
  check('capacity database migration applies cleanly and idempotently')

  await owner.query('CREATE ROLE cap_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantCapacityRuntime(owner, 'cap_app')

  // Membership setup
  for (const [tenant, id, kind, grants, scope] of [
    ['agency', 'owner', 'human', domain.CAPACITY_PERMISSIONS, 'all'],
    ['agency', 'planner', 'human', ['capacity.read', 'capacity.allocate'], 'assigned'],
    ['agency', 'bot', 'service', ['capacity.read'], 'assigned'],
    ['agency', 'inactive', 'human', domain.CAPACITY_PERMISSIONS, 'all'],
    ['epc', 'owner', 'human', domain.CAPACITY_PERMISSIONS, 'all'],
  ]) {
    await owner.query(
      'INSERT INTO archava_capacity.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5)',
      [tenant, id, kind, grants, scope],
    )
  }
  await owner.query(
    "UPDATE archava_capacity.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  app = new Pool({ ...target, user: 'cap_app' })

  // Non-owner runtime check
  await assert.rejects(
    async () => postgres.PostgresCapacityRepository.create(owner, key),
    /requires a non-owner runtime/,
  )
  check('capacity repository refuses superuser or schema-owner database connection')

  const repository = await postgres.PostgresCapacityRepository.create(app, key)
  const service = new domain.CapacityService(repository)

  // Inactive user refusal
  await failed(
    () =>
      service.mutate('agency', human('inactive'), {
        type: 'create_pool',
        commandId: randomUUID(),
        code: 'DEV_TEAM',
        name: 'Dev Team',
        unit: 'hours',
        weeklyCapacity: 160,
        maxConcurrentJobs: 2,
      }),
    'forbidden',
  )
  check('inactive membership is denied mutation authority')

  // Machine mutation refusal
  await failed(
    () =>
      service.mutate(
        'agency',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        {
          type: 'create_pool',
          commandId: randomUUID(),
          code: 'DEV_TEAM',
          name: 'Dev Team',
          unit: 'hours',
          weeklyCapacity: 160,
          maxConcurrentJobs: 2,
        },
      ),
    'forbidden',
  )
  check('machine service identity is denied capacity write operations')

  // Human owner creates capacity pool with units and concurrent slots
  const createCmdId = randomUUID()
  const createReceipt = await service.mutate('agency', human('owner'), {
    type: 'create_pool',
    commandId: createCmdId,
    code: 'CORE_SPRINT',
    name: 'Core Fullstack Sprint Team',
    unit: 'hours',
    weeklyCapacity: 200, // 40h/business-day
    maxConcurrentJobs: 1, // 1 concurrent job to test queue serialization strictly
  })

  assert.equal(createReceipt.status, 'committed')
  assert.equal(createReceipt.commandId, createCmdId)
  assert.equal(createReceipt.revision, 1)
  const poolId = createReceipt.poolId
  assert.ok(poolId)
  check('human owner creates capacity pool with units and receives authoritative receipt')

  // Idempotent command replay
  const replayReceipt = await service.mutate('agency', human('owner'), {
    type: 'create_pool',
    commandId: createCmdId,
    code: 'CORE_SPRINT',
    name: 'Core Fullstack Sprint Team',
    unit: 'hours',
    weeklyCapacity: 200,
    maxConcurrentJobs: 1,
  })
  assert.deepEqual(replayReceipt, createReceipt)
  check('idempotent command replay by same actor returns identical receipt')

  // Command replay with altered payload rejected
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'create_pool',
        commandId: createCmdId,
        code: 'CORE_SPRINT_MODIFIED',
        name: 'Altered Team',
        unit: 'hours',
        weeklyCapacity: 300,
        maxConcurrentJobs: 2,
      }),
    'conflict',
  )
  check('command replay with altered payload is rejected with conflict')

  // Command status query
  const statusRes = await service.commandStatus('agency', human('owner'), createCmdId)
  assert.equal(statusRes.status, 'committed')
  assert.deepEqual(statusRes.receipt, createReceipt)
  const unknownStatus = await service.commandStatus('agency', human('owner'), randomUUID())
  assert.equal(unknownStatus.status, 'unknown')
  check('command status distinguishes committed from unknown requests')

  // Capacity pool search and detail query
  const detail = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(detail.pool.id, poolId)
  assert.equal(detail.pool.code, 'CORE_SPRINT')
  assert.equal(detail.pool.weeklyCapacity, 200)
  assert.equal(detail.pool.maxConcurrentJobs, 1)
  assert.equal(detail.pool.state, 'active')
  assert.equal(detail.currentLoadHours, 0)
  assert.equal(detail.activeAllocations.length, 0)

  const listRes = await service.listPools('agency', human('owner'), { search: 'CORE_SPRINT' })
  assert.equal(listRes.items.length, 1)
  assert.equal(listRes.items[0]?.id, poolId)
  check('capacity pool search and detail query return authoritative attributes')

  // Tenant RLS isolation
  await failed(
    () => service.detailPool('epc', human('owner'), poolId),
    'not_found',
  )
  const epcList = await service.listPools('epc', human('owner'), {})
  assert.equal(epcList.items.length, 0)
  check('forced tenant RLS completely isolates capacity pools across tenants')

  // Optimistic revision check
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'update_pool',
        commandId: randomUUID(),
        poolId,
        expectedRevision: 99,
        name: 'Invalid Revision Update',
        weeklyCapacity: 220,
        maxConcurrentJobs: 1,
      }),
    'conflict',
  )
  check('optimistic revision check rejects update against stale revision')

  // Capacity pool update advances revision
  const updateCmdId = randomUUID()
  const updateReceipt = await service.mutate('agency', human('owner'), {
    type: 'update_pool',
    commandId: updateCmdId,
    poolId,
    expectedRevision: 1,
    name: 'Core Fullstack Sprint Team - Expanded',
    weeklyCapacity: 200,
    maxConcurrentJobs: 1,
  })
  assert.equal(updateReceipt.revision, 2)
  const updatedDetail = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(updatedDetail.pool.name, 'Core Fullstack Sprint Team - Expanded')
  assert.equal(updatedDetail.pool.revision, 2)
  check('capacity pool update advances revision atomically')

  // Capacity estimation on empty pool
  // With 200 hours/week, daily burn rate = 40 hours/day.
  // Workload of 80 hours requires Math.ceil(80 / 40) = 2 calendar days.
  const now = new Date()
  const estimate1 = await service.estimate('agency', human('owner'), {
    poolId,
    requiredUnits: 80,
    desiredStartAt: now.toISOString(),
  })
  assert.equal(estimate1.queuePosition, 0)
  assert.equal(estimate1.burnRatePerDay, 40)
  assert.equal(estimate1.estimatedCalendarDays, 2)
  const est1Start = new Date(estimate1.estimatedStartAt).getTime()
  const est1Finish = new Date(estimate1.estimatedFinishAt).getTime()
  assert.equal(est1Finish - est1Start, 2 * 24 * 60 * 60 * 1000)
  check('capacity estimate computes deterministic daily burn rate, duration and immediate start on idle pool')

  // Commit first allocation
  const alloc1CmdId = randomUUID()
  const alloc1Receipt = await service.mutate('agency', human('owner'), {
    type: 'commit_allocation',
    commandId: alloc1CmdId,
    poolId,
    workloadId: 'SPRINT-101',
    workloadTitle: 'Core Platform Authentication & Multi-Tenancy',
    requiredUnits: 80,
    desiredStartAt: now.toISOString(),
  })
  assert.equal(alloc1Receipt.status, 'committed')
  assert.equal(alloc1Receipt.revision, 1)
  const alloc1Id = alloc1Receipt.allocationId
  assert.ok(alloc1Id)

  const detailAfterAlloc1 = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(detailAfterAlloc1.activeAllocations.length, 1)
  assert.equal(detailAfterAlloc1.currentLoadHours, 80)
  assert.equal(detailAfterAlloc1.activeAllocations[0]?.id, alloc1Id)
  assert.equal(detailAfterAlloc1.activeAllocations[0]?.state, 'scheduled')
  check('first committed allocation takes active execution slot and increases pool load')

  // Capacity estimate for second allocation when concurrency limit is reached (maxConcurrentJobs = 1)
  // Second allocation of 40 hours requires ceil(40 / 40) = 1 day.
  // Since pool is saturated, queue position is 1, and estimated start is exactly allocation 1's scheduledFinishAt!
  const estimate2 = await service.estimate('agency', human('owner'), {
    poolId,
    requiredUnits: 40,
    desiredStartAt: now.toISOString(),
  })
  assert.equal(estimate2.queuePosition, 1)
  assert.equal(estimate2.estimatedCalendarDays, 1)
  const alloc1FinishDate = detailAfterAlloc1.activeAllocations[0]?.scheduledFinishAt
  assert.equal(estimate2.estimatedStartAt, alloc1FinishDate)
  const est2Finish = new Date(estimate2.estimatedFinishAt).getTime()
  const est2Start = new Date(estimate2.estimatedStartAt).getTime()
  assert.equal(est2Finish - est2Start, 1 * 24 * 60 * 60 * 1000)
  check('concurrency saturation queues second estimate behind active allocation with dynamic finish handover')

  // Commit second allocation - scheduled start shifts deterministically to allocation 1 finish date
  const alloc2CmdId = randomUUID()
  const alloc2Receipt = await service.mutate('agency', human('owner'), {
    type: 'commit_allocation',
    commandId: alloc2CmdId,
    poolId,
    workloadId: 'SPRINT-102',
    workloadTitle: 'Audit Trail Export & S3 Integration',
    requiredUnits: 40,
    desiredStartAt: now.toISOString(),
  })
  assert.equal(alloc2Receipt.status, 'committed')
  const alloc2Id = alloc2Receipt.allocationId
  assert.ok(alloc2Id)

  const detailAfterAlloc2 = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(detailAfterAlloc2.activeAllocations.length, 2)
  assert.equal(detailAfterAlloc2.currentLoadHours, 120) // 80 + 40
  const alloc2 = detailAfterAlloc2.activeAllocations.find((a) => a.id === alloc2Id)
  assert.ok(alloc2)
  assert.equal(alloc2.scheduledStartAt, alloc1FinishDate)
  check('committed queue allocation shifts scheduled start to preceding allocation finish date')

  // Progress update on allocation 1: partial burn reduces remaining units
  const progressCmdId = randomUUID()
  const progressReceipt = await service.mutate('agency', human('owner'), {
    type: 'update_allocation_progress',
    commandId: progressCmdId,
    allocationId: alloc1Id,
    expectedRevision: 1,
    remainingUnits: 30, // 50 burned, 30 remaining
    state: 'in_progress',
  })
  assert.equal(progressReceipt.revision, 2)
  const detailAfterProgress = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(detailAfterProgress.currentLoadHours, 70) // 30 remaining + 40
  const alloc1Updated = detailAfterProgress.activeAllocations.find((a) => a.id === alloc1Id)
  assert.equal(alloc1Updated?.remainingUnits, 30)
  assert.equal(alloc1Updated?.state, 'in_progress')
  check('progress update records burned units and atomically recalculates active pool load')

  // Complete allocation 1: slot is freed, state becomes completed
  const completeCmdId = randomUUID()
  const completeReceipt = await service.mutate('agency', human('owner'), {
    type: 'complete_allocation',
    commandId: completeCmdId,
    allocationId: alloc1Id,
    expectedRevision: 2,
  })
  assert.equal(completeReceipt.revision, 3)

  const detailAfterComplete = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(detailAfterComplete.activeAllocations.length, 1) // only alloc2 remains active
  assert.equal(detailAfterComplete.currentLoadHours, 40)
  assert.equal(detailAfterComplete.activeAllocations[0]?.id, alloc2Id)
  check('allocation completion frees slot and removes allocation from active load')

  // Cancel allocation 2: cancellation releases all remaining units
  const cancelCmdId = randomUUID()
  const cancelReceipt = await service.mutate('agency', human('owner'), {
    type: 'cancel_allocation',
    commandId: cancelCmdId,
    allocationId: alloc2Id,
    expectedRevision: 1,
    reason: 'Scope deferred to next fiscal quarter by client agreement.',
  })
  assert.equal(cancelReceipt.revision, 2)

  const detailAfterCancel = await service.detailPool('agency', human('owner'), poolId)
  assert.equal(detailAfterCancel.activeAllocations.length, 0)
  assert.equal(detailAfterCancel.currentLoadHours, 0)
  check('allocation cancellation releases remaining demand units and empties pool queue')

  // Archive capacity pool: archived pool rejects subsequent allocations
  const archiveCmdId = randomUUID()
  const archiveReceipt = await service.mutate('agency', human('owner'), {
    type: 'archive_pool',
    commandId: archiveCmdId,
    poolId,
    expectedRevision: 2,
  })
  assert.equal(archiveReceipt.revision, 3)

  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'commit_allocation',
        commandId: randomUUID(),
        poolId,
        workloadId: 'NEW_SPRINT',
        workloadTitle: 'Deferred feature',
        requiredUnits: 20,
      }),
    'pool_archived',
  )
  check('archived capacity pool strictly refuses new demand allocations')

  // Second independent consumer: Construction / EPC (K31)
  const { domain: epcDomain, postgres: epcPostgres } = consumers[1]
  const epcRepo = await epcPostgres.PostgresCapacityRepository.create(app, key)
  const epcService = new epcDomain.CapacityService(epcRepo)

  const epcPoolCmdId = randomUUID()
  const epcPoolReceipt = await epcService.mutate('epc', human('owner'), {
    type: 'create_pool',
    commandId: epcPoolCmdId,
    code: 'RIG_HEAVY_01',
    name: 'Heavy Hydraulic Rigging Unit #01',
    unit: 'days',
    weeklyCapacity: 5, // 1 rig-day per business day
    maxConcurrentJobs: 1,
  })
  const epcPoolId = epcPoolReceipt.poolId
  assert.ok(epcPoolId)

  // Commit EPC job: 4 days foundation drilling
  const epcAllocReceipt = await epcService.mutate('epc', human('owner'), {
    type: 'commit_allocation',
    commandId: randomUUID(),
    poolId: epcPoolId,
    workloadId: 'EPC-FOUNDATION-01',
    workloadTitle: 'Grid 4 Deep Piling Rigging',
    requiredUnits: 4,
    desiredStartAt: now.toISOString(),
  })
  assert.equal(epcAllocReceipt.status, 'committed')

  const epcDetail = await epcService.detailPool('epc', human('owner'), epcPoolId)
  assert.equal(epcDetail.pool.unit, 'days')
  assert.equal(epcDetail.pool.weeklyCapacity, 5)
  assert.equal(epcDetail.currentLoadHours, 4)
  assert.equal(epcDetail.activeAllocations.length, 1)
  check('EPC consumer operates independent heavy rigging capacity pool and day-unit allocation')

  // Service identity read-only via human permission intersection
  const botPrincipal = { kind: 'service', id: 'bot', onBehalfOf: 'owner' }
  const botList = await service.listPools('agency', botPrincipal, {})
  assert.equal(botList.items.length, 1)
  assert.equal(botList.items[0]?.code, 'CORE_SPRINT')
  check('service identity reads capacity pool data through human permission intersection')

  // Restart PostgreSQL and verify persistence
  await app.end()
  await owner.end()
  stop()
  start()
  owner = new Pool(target)
  app = new Pool({ ...target, user: 'cap_app' })

  const restartedRepo = await postgres.PostgresCapacityRepository.create(app, key)
  const restartedService = new domain.CapacityService(restartedRepo)
  const persistedAgency = await restartedService.detailPool('agency', human('owner'), poolId)
  assert.equal(persistedAgency.pool.code, 'CORE_SPRINT')
  assert.equal(persistedAgency.pool.state, 'archived')
  assert.equal(persistedAgency.pool.revision, 3)

  const persistedEpc = await restartedService.detailPool('epc', human('owner'), epcPoolId)
  assert.equal(persistedEpc.pool.code, 'RIG_HEAVY_01')
  assert.equal(persistedEpc.currentLoadHours, 4)
  assert.equal(persistedEpc.activeAllocations.length, 1)
  assert.equal(persistedEpc.activeAllocations[0]?.workloadId, 'EPC-FOUNDATION-01')
  check('capacity pools, allocations and lifecycle states persist across PostgreSQL host restart')

  // pg_dump and pg_restore test
  const dumpFile = join(scratch, 'capacity.dump')
  run('pg_dump', ['-Fc', '-f', dumpFile, '-h', socket, '-p', String(port), '-U', 'postgres', 'postgres'])

  await owner.query('CREATE DATABASE cap_restored')
  run('pg_restore', [
    '-d',
    'cap_restored',
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    dumpFile,
  ])

  const restoredApp = new Pool({ ...target, database: 'cap_restored', user: 'cap_app' })
  const restoredRepo = await postgres.PostgresCapacityRepository.create(restoredApp, key)
  const restoredService = new domain.CapacityService(restoredRepo)
  const restoredDetail = await restoredService.detailPool('epc', human('owner'), epcPoolId)
  assert.equal(restoredDetail.pool.code, 'RIG_HEAVY_01')
  assert.equal(restoredDetail.currentLoadHours, 4)
  assert.equal(restoredDetail.activeAllocations.length, 1)
  assert.equal(restoredDetail.activeAllocations[0]?.workloadId, 'EPC-FOUNDATION-01')

  const restoredStatus = await restoredService.commandStatus('agency', human('owner'), createCmdId)
  assert.equal(restoredStatus.status, 'committed')
  assert.equal(restoredStatus.receipt.poolId, poolId)
  await endProofPool(restoredApp)
  check('pg_dump and pg_restore preserve all capacity pool records, allocations and command receipts')

  process.stdout.write(`\nALL ${checks.length} CAPACITY ESTIMATION & DELIVERY CHECKS PASSED PERFECTLY!\n`)
} finally {
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true }).catch(() => {})
}
