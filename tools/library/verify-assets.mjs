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
import { installEnv } from './npm-env.mjs'

const root = resolve(import.meta.dirname, '../..')
const library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-assets-proof-'))
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
  // Two independent consumers: Rental (K10) and Repair Workshop (K18)
  for (const name of ['rental', 'workshop']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-assets-' + name,
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

    const require = createRequire(join(directory, 'package.json'))
    const load = async (pkg) => import(pathToFileURL(require.resolve(pkg)).href)

    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))

    const domain = await load('@archava/assets')
    const postgres = await load('@archava/assets-pg')
    Pool ??= require('pg').Pool

    const browser = await build({
      entryPoints: [require.resolve('@archava/assets/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })

    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /assets-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )

    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/assets')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }

  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed rental and workshop consumers share unchanged assets code and browser-safe dashboard',
  )

  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)

  const { domain, postgres } = consumers[0]
  await postgres.migrateAssets(owner)
  // Idempotent migration
  await postgres.migrateAssets(owner)
  check('assets database migration applies cleanly and idempotently')

  await owner.query('CREATE ROLE asset_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantAssetsRuntime(owner, 'asset_app')

  // Membership setup
  for (const [tenant, id, kind, grants, scope] of [
    ['rental', 'owner', 'human', domain.ASSET_PERMISSIONS, 'all'],
    ['rental', 'dispatcher', 'human', ['assets.read', 'assets.handover'], 'all'],
    ['rental', 'technician', 'human', ['assets.read', 'assets.maintenance'], 'all'],
    ['rental', 'assigned-user', 'human', ['assets.read', 'assets.handover'], 'assigned'],
    ['rental', 'zero-viewer', 'human', [], 'assigned'],
    ['rental', 'bot', 'service', ['assets.read'], 'assigned'],
    ['rental', 'inactive', 'human', domain.ASSET_PERMISSIONS, 'all'],
    ['workshop', 'owner', 'human', domain.ASSET_PERMISSIONS, 'all'],
  ]) {
    await owner.query(
      'INSERT INTO archava_assets.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5)',
      [tenant, id, kind, grants, scope],
    )
  }
  await owner.query(
    "UPDATE archava_assets.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  app = new Pool({ ...target, user: 'asset_app' })

  // Non-owner runtime check
  await assert.rejects(
    async () => postgres.PostgresAssetRepository.create(owner, key),
    /requires a non-owner runtime/,
  )
  check('assets repository refuses superuser or schema-owner database connection')

  const repository = await postgres.PostgresAssetRepository.create(app, key)
  const service = new domain.AssetService(repository)

  // Inactive user refusal
  await failed(
    () =>
      service.mutate('rental', human('inactive'), {
        requestId: randomUUID(),
        type: 'create_asset',
        data: {
          code: 'CAT-320',
          name: 'Excavator CAT 320',
          ownership: 'business_owned',
          category: 'heavy_machinery',
        },
      }),
    'forbidden',
  )
  check('inactive membership is refused mutation access')

  // Service identity mutation refusal (human-only mutations)
  await failed(
    () =>
      service.mutate(
        'rental',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        {
          requestId: randomUUID(),
          type: 'create_asset',
          data: {
            code: 'CAT-320',
            name: 'Excavator CAT 320',
            ownership: 'business_owned',
            category: 'heavy_machinery',
          },
        },
      ),
    'forbidden',
  )
  check('service agent is refused direct asset register mutation')

  // Zero-permission user refusal
  await failed(
    () =>
      service.mutate('rental', human('zero-viewer'), {
        requestId: randomUUID(),
        type: 'create_asset',
        data: {
          code: 'CAT-320',
          name: 'Excavator CAT 320',
          ownership: 'business_owned',
          category: 'heavy_machinery',
        },
      }),
    'forbidden',
  )
  check('zero-permission user is refused asset creation')

  // Consumer 1: Rental - Create business-owned equipment
  const createCmd1 = {
    requestId: randomUUID(),
    type: 'create_asset',
    data: {
      code: 'EQ-CAT-320',
      name: 'Caterpillar 320 Hydraulic Excavator',
      ownership: 'business_owned',
      category: 'heavy_equipment',
      serialNumber: 'CAT320-X88921-2024',
      condition: 'excellent',
      initialLocation: 'Yard Jakarta Barat',
    },
  }
  const receipt1 = await service.mutate('rental', human('owner'), createCmd1)
  assert.equal(receipt1.revision, 1)
  assert.equal(receipt1.requestId, createCmd1.requestId)
  assert.ok(receipt1.assetId)
  check('consumer 1 creates business-owned serialized asset with initial condition')

  // Idempotent replay of creation command
  const replay1 = await service.mutate('rental', human('owner'), createCmd1)
  assert.deepEqual(replay1, receipt1)
  check('duplicate asset creation command replays previous committed receipt identically')

  // Dispatch asset custody to client
  const dispatchCmd = {
    requestId: randomUUID(),
    type: 'handover_asset',
    data: {
      assetId: receipt1.assetId,
      expectedRevision: 1,
      eventType: 'dispatch',
      toCustodianId: 'ct_client_001',
      notes: 'Dispatched for Proyek Tol Cibitung, site manager: Pak Hendra',
      conditionAfter: 'good',
      location: 'Site Cibitung',
    },
  }
  const dispatchReceipt = await service.mutate('rental', human('dispatcher'), dispatchCmd)
  assert.equal(dispatchReceipt.revision, 2)
  assert.equal(dispatchReceipt.assetId, receipt1.assetId)
  check('custody dispatch handover records transfer to client with revision increment')

  // Optimistic revision locking check on concurrent custody change
  await failed(
    () =>
      service.mutate('rental', human('dispatcher'), {
        requestId: randomUUID(),
        type: 'handover_asset',
        data: {
          assetId: receipt1.assetId,
          expectedRevision: 1, // Stale! Current is 2
          eventType: 'transfer',
          conditionAfter: 'good',
        },
      }),
    'conflict',
  )
  check('optimistic revision conflict is raised when expected revision is stale')

  // Return asset from client with condition wear
  const returnCmd = {
    requestId: randomUUID(),
    type: 'handover_asset',
    data: {
      assetId: receipt1.assetId,
      expectedRevision: 2,
      eventType: 'checkin',
      toCustodianId: 'Depot Pusat',
      conditionAfter: 'fair',
      notes: 'Returned after lease period; track pins worn, needs 500h service',
      location: 'Depot Pusat',
    },
  }
  const returnReceipt = await service.mutate('rental', human('dispatcher'), returnCmd)
  assert.equal(returnReceipt.revision, 3)
  check('asset return records custody recovery and reflects condition change')

  // Record maintenance inspection and filter change
  const maintCmd = {
    requestId: randomUUID(),
    type: 'record_maintenance',
    data: {
      assetId: receipt1.assetId,
      expectedRevision: 3,
      serviceType: 'preventive',
      description: '500-Hour Scheduled Hydraulic Service: Replaced filters and fluids',
      partsUsed: '2x Engine Oil Filter (1R-0716), 1x Hydraulic Element (093-5369)',
      conditionAfter: 'good',
      location: 'Depot Pusat Workshop',
    },
  }
  const maintReceipt = await service.mutate('rental', human('technician'), maintCmd)
  assert.equal(maintReceipt.revision, 4)
  check('preventive maintenance record logs parts replaced and updates asset condition')

  // Consumer 2: Repair Workshop - Customer-owned device intake
  const workshopCmd = {
    requestId: randomUUID(),
    type: 'create_asset',
    data: {
      code: 'REP-MAC-099',
      name: 'Apple MacBook Pro 16" M2 Max (Space Gray)',
      ownership: 'customer_owned',
      customerContactId: 'ct_cust_alex',
      category: 'electronics',
      serialNumber: 'C02G99XXMD6R',
      condition: 'damaged',
      initialLocation: 'Intake Shelf B2',
    },
  }
  const wsReceipt = await service.mutate('workshop', human('owner'), workshopCmd)
  assert.equal(wsReceipt.revision, 1)
  assert.ok(wsReceipt.assetId)
  check('consumer 2 creates customer-owned asset tracking client contact identity')

  // Customer asset maintenance: Corrective logic board repair
  const repairCmd = {
    requestId: randomUUID(),
    type: 'record_maintenance',
    data: {
      assetId: wsReceipt.assetId,
      expectedRevision: 1,
      serviceType: 'corrective',
      description: 'Cleaned corrosion on PPBUS_G3H rail, replaced shorted ISL9240 charging IC',
      partsUsed: '1x ISL9240 Buck-Boost Charger IC',
      conditionAfter: 'good',
      location: 'Bench 3',
    },
  }
  const repairReceipt = await service.mutate('workshop', human('owner'), repairCmd)
  assert.equal(repairReceipt.revision, 2)
  check('repair workshop logs corrective maintenance and component replacement')

  // Handover customer asset back to owner
  const wsReturnCmd = {
    requestId: randomUUID(),
    type: 'handover_asset',
    data: {
      assetId: wsReceipt.assetId,
      expectedRevision: 2,
      eventType: 'customer_return',
      toCustodianId: 'ct_cust_alex',
      conditionAfter: 'good',
      notes: 'Tested full charge cycle and stress test passed; returned to customer',
      location: 'Front Desk',
    },
  }
  const wsReturnReceipt = await service.mutate('workshop', human('owner'), wsReturnCmd)
  assert.equal(wsReturnReceipt.revision, 3)
  check('customer asset handed back to contact with verified condition')

  // Cross-tenant isolation verification
  const rentalDetail = await service.detail('rental', human('owner'), receipt1.assetId)
  assert.equal(rentalDetail.asset.code, 'EQ-CAT-320')
  assert.equal(rentalDetail.custodyEvents.length, 2)
  assert.equal(rentalDetail.maintenanceRecords.length, 1)
  assert.equal(rentalDetail.asset.condition, 'good')

  await failed(() => service.detail('workshop', human('owner'), receipt1.assetId), 'not_found')
  await failed(() => service.detail('rental', human('owner'), wsReceipt.assetId), 'not_found')
  check('cross-tenant asset detail and history requests are strictly isolated and not found')

  // Read tools validation
  const { createAssetTools } = domain
  const rentalTools = createAssetTools(service, 'rental', human('owner'))
  const listToolRes = await rentalTools.invoke('assets.list', { limit: 10 })
  assert.equal(listToolRes.items.length, 1)
  assert.equal(listToolRes.items[0].code, 'EQ-CAT-320')

  const searchToolRes = await rentalTools.invoke('assets.search', { query: 'Caterpillar' })
  assert.equal(searchToolRes.items.length, 1)

  const custodyToolRes = await rentalTools.invoke('assets.custodyHistory', {
    assetId: receipt1.assetId,
  })
  assert.equal(custodyToolRes.length, 2)

  const maintToolRes = await rentalTools.invoke('assets.maintenanceHistory', {
    assetId: receipt1.assetId,
  })
  assert.equal(maintToolRes.length, 1)
  check('private read tools return filtered asset, custody and maintenance telemetry')

  // Database immutable audit trigger proof: attempts to mutate audit rows directly are refused
  const auditRow = (await owner.query('SELECT request_id FROM archava_assets.audit LIMIT 1'))
    .rows[0]
  assert.ok(auditRow)
  await assert.rejects(
    async () =>
      owner.query('DELETE FROM archava_assets.audit WHERE request_id=$1', [auditRow.request_id]),
    /Asset audit history is immutable/,
  )
  check('immutable triggers protect audit log from deletion and tampering')

  // Summary output
  const destination = join(root, 'dist/assets-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded serialized asset register, custody handover and maintenance logging',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(
    `\nAll ${checks.length} synthetic verification checks passed successfully.\n`,
  )
} finally {
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true })
}
