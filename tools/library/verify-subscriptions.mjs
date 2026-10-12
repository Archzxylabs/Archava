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
const scratch = await mkdtemp(join(tmpdir(), 'archava-subscriptions-proof-'))
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
  // Two independent consumers: SaaS (K09) and Gym/Club (K08)
  for (const name of ['saas', 'gym']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-subscriptions-' + name,
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

    const domain = await load('@archava/subscriptions')
    const postgres = await load('@archava/subscriptions-pg')
    Pool ??= require('pg').Pool

    const browser = await build({
      entryPoints: [require.resolve('@archava/subscriptions/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })

    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /subscriptions-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )

    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/subscriptions')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }

  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed SaaS and gym consumers share unchanged subscriptions code and browser-safe dashboard',
  )

  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)

  const { domain, postgres } = consumers[0]
  await postgres.migrateSubscriptions(owner)
  // Idempotent migration
  await postgres.migrateSubscriptions(owner)
  check('subscriptions database migration applies cleanly and idempotently')

  await owner.query('CREATE ROLE sub_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantSubscriptionsRuntime(owner, 'sub_app')

  // Membership setup
  for (const [tenant, id, kind, grants, scope] of [
    ['saas', 'owner', 'human', domain.SUBSCRIPTION_PERMISSIONS, 'all'],
    ['saas', 'billing-agent', 'human', ['subscriptions.read', 'subscriptions.payment'], 'all'],
    ['saas', 'api-client', 'human', ['subscriptions.read', 'subscriptions.usage'], 'all'],
    ['saas', 'assigned-user', 'human', ['subscriptions.read', 'subscriptions.usage'], 'assigned'],
    ['saas', 'zero-viewer', 'human', [], 'assigned'],
    ['saas', 'bot', 'service', ['subscriptions.read'], 'assigned'],
    ['saas', 'inactive', 'human', domain.SUBSCRIPTION_PERMISSIONS, 'all'],
    ['gym', 'owner', 'human', domain.SUBSCRIPTION_PERMISSIONS, 'all'],
  ]) {
    await owner.query(
      'INSERT INTO archava_subscriptions.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5)',
      [tenant, id, kind, grants, scope],
    )
  }
  await owner.query(
    "UPDATE archava_subscriptions.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  app = new Pool({ ...target, user: 'sub_app' })

  // Non-owner runtime check
  await assert.rejects(
    async () => postgres.PostgresSubscriptionRepository.create(owner, key),
    /requires a non-owner runtime/,
  )
  check('subscriptions repository refuses superuser or schema-owner database connection')

  const repository = await postgres.PostgresSubscriptionRepository.create(app, key)
  const service = new domain.SubscriptionService(repository)
  const contact1 = randomUUID()
  const contact2 = randomUUID()

  // Inactive user refusal
  await failed(
    () =>
      service.mutate('saas', human('inactive'), {
        type: 'create_subscription',
        commandId: randomUUID(),
        contactId: contact1,
        planCode: 'enterprise-cloud',
        planName: 'Enterprise Cloud Subscription',
        billingInterval: 'yearly',
        unitPrice: 1200000,
        currency: 'USD',
        quantity: 1,
        initialEntitlements: [],
      }),
    'forbidden',
  )
  check('inactive membership is refused mutation access')

  // Service identity mutation refusal (human-only mutations)
  await failed(
    () =>
      service.mutate(
        'saas',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        {
          type: 'create_subscription',
          commandId: randomUUID(),
          contactId: contact1,
          planCode: 'enterprise-cloud',
          planName: 'Enterprise Cloud Subscription',
          billingInterval: 'yearly',
          unitPrice: 1200000,
          currency: 'USD',
          quantity: 1,
          initialEntitlements: [],
        },
      ),
    'forbidden',
  )
  check('service agent is refused direct subscription mutation')

  // Zero-permission user refusal
  await failed(
    () =>
      service.mutate('saas', human('zero-viewer'), {
        type: 'create_subscription',
        commandId: randomUUID(),
        contactId: contact1,
        planCode: 'enterprise-cloud',
        planName: 'Enterprise Cloud Subscription',
        billingInterval: 'yearly',
        unitPrice: 1200000,
        currency: 'USD',
        quantity: 1,
        initialEntitlements: [],
      }),
    'forbidden',
  )
  check('zero-permission user is refused subscription creation')

  // Consumer 1: SaaS (K09) - Create subscription with draft status and initial metered / seat allocations
  const createSubCmd = {
    type: 'create_subscription',
    commandId: randomUUID(),
    contactId: contact1,
    planCode: 'cloud-scale',
    planName: 'Cloud Scale Pro Tier',
    billingInterval: 'monthly',
    unitPrice: 29900,
    currency: 'USD',
    quantity: 1,
    initialEntitlements: [
      {
        featureCode: 'api_calls_quota',
        featureName: 'Monthly API Calls (Metered)',
        entitlementType: 'metered',
        allocatedQuantity: 1000,
      },
      {
        featureCode: 'seat_licenses',
        featureName: 'Provisioned Seat Licenses',
        entitlementType: 'seat_count',
        allocatedQuantity: 10,
      },
    ],
  }
  const subReceipt = await service.mutate('saas', human('owner'), createSubCmd)
  assert.equal(subReceipt.revision, 1)
  assert.equal(subReceipt.commandId, createSubCmd.commandId)
  assert.equal(subReceipt.status, 'draft')
  assert.ok(subReceipt.subscriptionId)
  check('consumer 1 creates draft subscription with initial metered and seat allocations')

  // Load details and verify entitlements were created
  const subDetail1 = await service.detail('saas', human('owner'), subReceipt.subscriptionId)
  assert.equal(subDetail1.subscription.status, 'draft')
  assert.equal(subDetail1.entitlements.length, 2)
  const apiEnt = subDetail1.entitlements.find((e) => e.featureCode === 'api_calls_quota')
  assert.ok(apiEnt)
  assert.equal(apiEnt.allocatedQuantity, 1000)
  assert.equal(apiEnt.consumedQuantity, 0)

  // Authoritative backend payment confirmation transitions draft -> active
  const now = new Date()
  const periodEnd = new Date(now.getTime() + 30 * 86400000)
  const payCmd = {
    type: 'record_payment',
    commandId: randomUUID(),
    subscriptionId: subReceipt.subscriptionId,
    paymentReference: 'PAY-STRIPE-CH_994821',
    amountPaid: 29900,
    periodStart: now.toISOString(),
    periodEnd: periodEnd.toISOString(),
  }
  const payReceipt = await service.mutate('saas', human('billing-agent'), payCmd)
  assert.equal(payReceipt.revision, 2)
  assert.equal(payReceipt.status, 'active')
  assert.equal(payReceipt.commandId, payCmd.commandId)
  check('authoritative backend payment confirmation transitions subscription from draft to active')

  // Idempotent replay of record_payment command
  const replayPay = await service.mutate('saas', human('billing-agent'), payCmd)
  assert.deepEqual(replayPay, { ...payReceipt, idempotencyReplay: true })
  check('duplicate payment record command replays previous committed receipt identically')

  // Usage event consumption decrements entitlement remaining quantity
  const useCmd1 = {
    type: 'consume_usage',
    commandId: randomUUID(),
    entitlementId: apiEnt.id,
    units: 250,
    idempotencyKey: 'api-batch-001',
    evidenceRef: 'Batch compute job run #104',
  }
  const useReceipt1 = await service.mutate('saas', human('api-client'), useCmd1)
  assert.equal(useReceipt1.action, 'consume_usage')
  assert.equal(useReceipt1.status, 'active')

  const entAfter1 = await service.entitlements('saas', human('owner'), subReceipt.subscriptionId)
  const apiAfter1 = entAfter1.find((e) => e.id === apiEnt.id)
  assert.equal(apiAfter1.consumedQuantity, 250)
  check('usage event consumption decrements entitlement remaining quantity with idempotency key')

  // Replaying same usage event idempotently returns identical receipt without double-metering
  const useReplay = await service.mutate('saas', human('api-client'), useCmd1)
  assert.deepEqual(useReplay, { ...useReceipt1, idempotencyReplay: true })
  const entAfterReplay = await service.entitlements(
    'saas',
    human('owner'),
    subReceipt.subscriptionId,
  )
  const apiAfterReplay = entAfterReplay.find((e) => e.id === apiEnt.id)
  assert.equal(apiAfterReplay.consumedQuantity, 250)
  check('replaying same usage event idempotently returns identical receipt without double-metering')

  // Quota exhaustion refusal: Consuming beyond allocated quota (remaining 750, request 800)
  await failed(
    () =>
      service.mutate('saas', human('api-client'), {
        type: 'consume_usage',
        commandId: randomUUID(),
        entitlementId: apiEnt.id,
        units: 800,
        idempotencyKey: 'api-batch-exceed',
      }),
    'exhausted',
  )

  // Consume exactly remaining 750 units, exhausting the entitlement
  const useExhaustCmd = {
    type: 'consume_usage',
    commandId: randomUUID(),
    entitlementId: apiEnt.id,
    units: 750,
    idempotencyKey: 'api-batch-finish',
  }
  const exhaustReceipt = await service.mutate('saas', human('api-client'), useExhaustCmd)
  assert.equal(exhaustReceipt.status, 'exhausted')

  const entExhausted = (
    await service.entitlements('saas', human('owner'), subReceipt.subscriptionId)
  ).find((e) => e.id === apiEnt.id)
  assert.equal(entExhausted.consumedQuantity, 1000)
  assert.equal(entExhausted.status, 'exhausted')
  check('quota exhaustion refusal blocks overconsumption and transitions entitlement to exhausted')

  // Status transition to past_due halts further usage consumption
  // First grant a fresh entitlement to test past_due gating
  const grantCmd = {
    type: 'grant_entitlement',
    commandId: randomUUID(),
    subscriptionId: subReceipt.subscriptionId,
    contactId: contact1,
    featureCode: 'storage_gb',
    featureName: 'Cloud Storage Bucket',
    entitlementType: 'metered',
    allocatedQuantity: 500,
  }
  await service.mutate('saas', human('owner'), grantCmd)
  const subDetailFresh = await service.detail('saas', human('owner'), subReceipt.subscriptionId)
  const storageEnt = subDetailFresh.entitlements.find((e) => e.featureCode === 'storage_gb')
  assert.ok(storageEnt)

  // Now set status to past_due
  const pastDueCmd = {
    type: 'change_status',
    commandId: randomUUID(),
    subscriptionId: subReceipt.subscriptionId,
    expectedRevision: 2,
    targetStatus: 'past_due',
    reason: 'Payment processor invoice collection failed',
  }
  const pastDueReceipt = await service.mutate('saas', human('owner'), pastDueCmd)
  assert.equal(pastDueReceipt.status, 'past_due')
  assert.equal(pastDueReceipt.revision, 3)

  // Usage consumption on past_due subscription is refused with 'past_due'
  await failed(
    () =>
      service.mutate('saas', human('api-client'), {
        type: 'consume_usage',
        commandId: randomUUID(),
        entitlementId: storageEnt.id,
        units: 10,
        idempotencyKey: 'storage-write-001',
      }),
    'past_due',
  )
  check('status transition to past_due halts further usage consumption')

  // Reactivation / payment clears past_due and restores entitlement active state
  const reactivatePayCmd = {
    type: 'record_payment',
    commandId: randomUUID(),
    subscriptionId: subReceipt.subscriptionId,
    paymentReference: 'PAY-STRIPE-RETRY_SUCCESS',
    amountPaid: 29900,
    periodStart: now.toISOString(),
    periodEnd: periodEnd.toISOString(),
  }
  const reactivateReceipt = await service.mutate('saas', human('billing-agent'), reactivatePayCmd)
  assert.equal(reactivateReceipt.status, 'active')
  assert.equal(reactivateReceipt.revision, 4)

  // Now usage consumption succeeds again!
  const storageUseRes = await service.mutate('saas', human('api-client'), {
    type: 'consume_usage',
    commandId: randomUUID(),
    entitlementId: storageEnt.id,
    units: 10,
    idempotencyKey: 'storage-write-001',
  })
  assert.equal(storageUseRes.status, 'active')
  check('reactivation payment clears past_due and restores entitlement active state')

  // Optimistic revision conflict is raised when expected revision is stale
  await failed(
    () =>
      service.mutate('saas', human('owner'), {
        type: 'change_status',
        commandId: randomUUID(),
        subscriptionId: subReceipt.subscriptionId,
        expectedRevision: 2, // Current is 4!
        targetStatus: 'paused',
        reason: 'Temporary maintenance pause',
      }),
    'conflict',
  )
  check('optimistic revision conflict is raised when expected revision is stale')

  // Consumer 2: Gym / Club (K08) - Membership with pass bundle
  const gymSubCmd = {
    type: 'create_subscription',
    commandId: randomUUID(),
    contactId: contact2,
    planCode: 'fitness-pass-10',
    planName: '10-Session Fitness Pass Bundle',
    billingInterval: 'monthly',
    unitPrice: 150000,
    currency: 'IDR',
    quantity: 1,
    initialEntitlements: [
      {
        featureCode: 'gym_entry_pass',
        featureName: 'Club Access Check-in Passes',
        entitlementType: 'pass_bundle',
        allocatedQuantity: 10,
      },
    ],
  }
  const gymReceipt = await service.mutate('gym', human('owner'), gymSubCmd)
  assert.equal(gymReceipt.revision, 1)

  // Activate gym pass via payment
  const gymPayCmd = {
    type: 'record_payment',
    commandId: randomUUID(),
    subscriptionId: gymReceipt.subscriptionId,
    paymentReference: 'CASH-RECEPTION-0881',
    amountPaid: 150000,
    periodStart: now.toISOString(),
    periodEnd: new Date(now.getTime() + 90 * 86400000).toISOString(),
  }
  await service.mutate('gym', human('owner'), gymPayCmd)

  const gymDetail = await service.detail('gym', human('owner'), gymReceipt.subscriptionId)
  const passEnt = gymDetail.entitlements.find((e) => e.featureCode === 'gym_entry_pass')
  assert.ok(passEnt)
  assert.equal(passEnt.allocatedQuantity, 10)

  // Check in member: consume 1 pass
  const gymCheckIn = await service.mutate('gym', human('owner'), {
    type: 'consume_usage',
    commandId: randomUUID(),
    entitlementId: passEnt.id,
    units: 1,
    idempotencyKey: 'reception-checkin-day-1',
    evidenceRef: 'Reception turnstile tap RFID #9901',
  })
  assert.equal(gymCheckIn.status, 'active')

  const gymDetailAfter = await service.detail('gym', human('owner'), gymReceipt.subscriptionId)
  const passAfter = gymDetailAfter.entitlements.find((e) => e.featureCode === 'gym_entry_pass')
  assert.equal(passAfter.consumedQuantity, 1)
  check('consumer 2 creates gym membership pass bundle and records check-in pass consumption')

  // Cross-tenant subscription detail, entitlement, and usage requests are strictly isolated and not found
  await failed(() => service.detail('gym', human('owner'), subReceipt.subscriptionId), 'not_found')
  await failed(() => service.detail('saas', human('owner'), gymReceipt.subscriptionId), 'not_found')
  await failed(
    () => service.entitlements('gym', human('owner'), subReceipt.subscriptionId),
    'not_found',
  )
  check(
    'cross-tenant subscription detail and entitlement requests are strictly isolated and not found',
  )

  // Database immutable audit trigger proof: attempts to mutate audit rows directly are refused
  const auditRow = (await owner.query('SELECT request_id FROM archava_subscriptions.audit LIMIT 1'))
    .rows[0]
  assert.ok(auditRow)
  await assert.rejects(
    async () =>
      owner.query('DELETE FROM archava_subscriptions.audit WHERE request_id=$1', [
        auditRow.request_id,
      ]),
    /Subscription audit history is immutable/,
  )
  check('immutable triggers protect audit log from deletion and tampering')

  // Summary output
  const destination = join(root, 'dist/subscriptions-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded recurring subscriptions, entitlements and usage tracking',
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
