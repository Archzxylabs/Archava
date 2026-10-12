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
const scratch = await mkdtemp(join(tmpdir(), 'archava-scheduling-proof-'))
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
const failed = async (promise, code) => assert.rejects(promise, (error) => error.code === code)

try {
  const consumers = []
  for (const name of ['appointments', 'rental']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-scheduling-' + name,
        private: true,
        type: 'module',
        overrides: Object.fromEntries(
          (profile.catalog.externals ?? []).map((x) => [x.name, x.version]),
        ),
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
    const load = async (name) => await import(pathToFileURL(require.resolve(name)).href)
    const release = await load('@archava/config/release')
    await release.verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))
    const domain = await load('@archava/scheduling'),
      postgres = await load('@archava/scheduling-pg')
    Pool ??= require('pg').Pool
    const dashboard = require.resolve('@archava/scheduling/dashboard')
    const browser = await build({
      entryPoints: [dashboard],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /scheduling-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/scheduling')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two cold-installed consumers share the same standalone scheduling module and browser-safe dashboard',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres } = consumers[0]
  await postgres.migrateScheduling(owner)
  await postgres.migrateScheduling(owner)
  await owner.query('CREATE ROLE scheduling_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantSchedulingRuntime(owner, 'scheduling_app')
  const permissions = domain.SCHEDULING_PERMISSIONS
  for (const [tenant, id, kind, grants, scope] of [
    ['appointments', 'owner', 'human', permissions, 'all'],
    ['appointments', 'viewer', 'human', permissions.filter((x) => x.endsWith('.read')), 'all'],
    ['appointments', 'staff', 'human', permissions, 'assigned'],
    ['appointments', 'bot', 'service', ['resources.read'], 'all'],
    ['rental', 'owner', 'human', permissions, 'all'],
  ])
    await owner.query(
      'INSERT INTO archava_scheduling.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
      [tenant, id, kind, grants, scope],
    )
  app = new Pool({ ...target, user: 'scheduling_app' })
  let service = new domain.SchedulingService(
    await postgres.PostgresSchedulingRepository.create(app, key),
  )
  const second = new consumers[1].domain.SchedulingService(
    await consumers[1].postgres.PostgresSchedulingRepository.create(app, key),
  )
  await assert.rejects(() => postgres.PostgresSchedulingRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresSchedulingRepository.create(app, 'short'))
  assert.deepEqual(
    (
      await owner.query(
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'archava_%'",
      )
    ).rows.map((x) => x.schema_name),
    ['archava_scheduling'],
  )
  check('standalone reservations require no CRM/LLM schema and refuse owner runtime or weak keys')
  const act = (command, requestId) =>
    service.mutate('appointments', human('owner'), mutation(command, requestId))
  const makeResource = (title = 'Consultant', kind = 'staff') =>
    act({ operation: 'resource.create', title, kind, timezone: 'Asia/Jakarta' })
  const resource = await makeResource()
  const rental = await second.mutate(
    'rental',
    human('owner'),
    mutation({
      operation: 'resource.create',
      title: 'Camera A',
      kind: 'asset',
      timezone: 'Asia/Jakarta',
    }),
  )
  const future = (hours) => new Date(Date.now() + hours * 3_600_000).toISOString()
  const startsAt = future(24),
    endsAt = future(25),
    later = future(26),
    last = future(27)
  const hold = {
    operation: 'reservation.hold',
    resourceId: resource.id,
    title: 'Consultation slot',
    startsAt,
    endsAt,
    holdSeconds: 900,
  }
  const submission = mutation(hold)
  const duplicates = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('appointments', human('owner'), submission)),
  )
  assert.equal(new Set(duplicates.map((x) => x.id)).size, 1)
  const booking = duplicates[0]
  assert.deepEqual(
    (
      await owner.query(
        'SELECT (SELECT count(*) FROM archava_scheduling.reservations) AS reservations,(SELECT count(*) FROM archava_scheduling.commands WHERE request_id=$1) AS commands,(SELECT count(*) FROM archava_scheduling.audit WHERE request_id=$1) AS audit',
        [submission.requestId],
      )
    ).rows[0],
    { reservations: '1', commands: '1', audit: '1' },
  )
  check('20 duplicate submissions create exactly one reservation, audit and immutable receipt')
  await failed(act({ ...hold, title: 'Payload drift' }, submission.requestId), 'conflict')
  await failed(service.mutate('appointments', human('viewer'), submission), 'forbidden')
  await failed(
    service.commandStatus('appointments', human('viewer'), submission.requestId),
    'forbidden',
  )
  await failed(second.resource('rental', human('owner'), resource.id), 'not_found')
  await failed(service.resource('appointments', human('staff'), resource.id), 'not_found')
  const staffResource = await service.mutate(
    'appointments',
    human('staff'),
    mutation({
      operation: 'resource.create',
      title: 'Staff owned',
      kind: 'staff',
      timezone: 'UTC',
    }),
  )
  assert.equal((await service.resources('appointments', human('staff'), {})).items.length, 1)
  assert.equal(
    (await service.resource('appointments', human('staff'), staffResource.id)).title,
    'Staff owned',
  )
  await owner.query(
    "UPDATE archava_scheduling.memberships SET active=false WHERE tenant_id='appointments' AND principal_id='owner'",
  )
  await failed(service.mutate('appointments', human('owner'), submission), 'forbidden')
  await failed(
    service.commandStatus('appointments', human('owner'), submission.requestId),
    'forbidden',
  )
  await failed(
    service.resources('appointments', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }, {}),
    'forbidden',
  )
  await owner.query(
    "UPDATE archava_scheduling.memberships SET active=true WHERE tenant_id='appointments' AND principal_id='owner'",
  )
  await service.resources('appointments', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }, {})
  await failed(
    service.reservations(
      'appointments',
      { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
      { resourceId: resource.id },
    ),
    'forbidden',
  )
  await assert.rejects(
    async () =>
      service.mutate(
        'appointments',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        submission,
      ),
    (x) => x.code === 'forbidden',
  )
  await act({ ...hold, resourceId: staffResource.id, title: 'Other owner private reservation' })
  assert.equal(
    (await service.reservations('appointments', human('staff'), { resourceId: staffResource.id }))
      .items.length,
    0,
  )
  assert.equal(
    (
      await service.availability('appointments', human('staff'), {
        resourceId: staffResource.id,
        startsAt,
        endsAt,
      })
    ).available,
    false,
  )
  await failed(
    service.mutate(
      'appointments',
      human('staff'),
      mutation({ ...hold, resourceId: staffResource.id }),
    ),
    'conflict',
  )
  check(
    'current grants, actor-bound receipts, assigned ownership, tenant isolation and service/human intersection refuse unauthorized access',
  )
  await act({
    operation: 'reservation.confirm',
    resourceId: resource.id,
    id: booking.id,
    expectedRevision: 1,
  })
  await failed(act({ ...hold, title: 'Overlapping' }), 'conflict')
  const availability = await service.availability('appointments', human('viewer'), {
    resourceId: resource.id,
    startsAt,
    endsAt,
  })
  assert.equal(availability.available, false)
  assert.equal(availability.authority, 'snapshot-only')
  assert.deepEqual(
    Object.keys(availability).sort(),
    ['resourceId', 'startsAt', 'endsAt', 'available', 'evaluatedAt', 'authority'].sort(),
  )
  const adjacent = await act({ ...hold, title: 'Adjacent slot', startsAt: endsAt, endsAt: later })
  await failed(
    act({
      operation: 'resource.set-active',
      resourceId: resource.id,
      expectedRevision: 1,
      active: false,
      reason: 'Closed',
    }),
    'conflict',
  )
  check(
    'confirmed bookings exclude overlaps, adjacent half-open slots work and live reservations prevent resource disabling',
  )
  await failed(
    act({
      operation: 'reservation.move',
      resourceId: resource.id,
      id: booking.id,
      expectedRevision: 1,
      startsAt: later,
      endsAt: last,
      reason: 'Stale edit',
    }),
    'conflict',
  )
  const moveConflict = mutation({
    operation: 'reservation.move',
    resourceId: resource.id,
    id: booking.id,
    expectedRevision: 2,
    startsAt: endsAt,
    endsAt: later,
    reason: 'Conflicting move',
  })
  await failed(service.mutate('appointments', human('owner'), moveConflict), 'conflict')
  assert.equal(
    (await service.commandStatus('appointments', human('owner'), moveConflict.requestId)).status,
    'not_committed',
  )
  assert.equal(
    (await service.detail('appointments', human('owner'), booking.id)).reservation.startsAt,
    startsAt,
  )
  const competing = await Promise.allSettled(
    [later, last].map((from, i) =>
      act({
        operation: 'reservation.move',
        resourceId: resource.id,
        id: booking.id,
        expectedRevision: 2,
        startsAt: from,
        endsAt: future(28 + i),
        reason: 'Move consultation',
      }),
    ),
  )
  assert.equal(competing.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal(
    competing.filter((x) => x.status === 'rejected' && x.reason.code === 'conflict').length,
    1,
  )
  assert.equal(
    (await service.detail('appointments', human('owner'), booking.id)).reservation.revision,
    3,
  )
  check('conflicting moves roll back completely and competing revisions permit exactly one edit')
  await act({
    operation: 'reservation.cancel',
    resourceId: resource.id,
    id: booking.id,
    expectedRevision: 3,
    reason: 'Rescheduled elsewhere',
  })
  await failed(
    act({
      operation: 'reservation.confirm',
      resourceId: resource.id,
      id: booking.id,
      expectedRevision: 4,
    }),
    'conflict',
  )
  await act({
    operation: 'reservation.cancel',
    resourceId: resource.id,
    id: adjacent.id,
    expectedRevision: 1,
    reason: 'Released fixture',
  })
  const overlapRace = await Promise.allSettled(
    Array.from({ length: 20 }, (_, i) => act({ ...hold, title: 'Contender ' + i })),
  )
  assert.equal(overlapRace.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal(
    overlapRace.filter((x) => x.status === 'rejected' && x.reason.code === 'conflict').length,
    19,
  )
  const winner = overlapRace.find((x) => x.status === 'fulfilled').value
  await act({
    operation: 'reservation.cancel',
    resourceId: resource.id,
    id: winner.id,
    expectedRevision: 1,
    reason: 'Release slot',
  })
  const reopened = await act(hold)
  check(
    '20 distinct competing requests cannot double-book; cancellation releases capacity and stays terminal',
  )
  const expiryResource = await makeResource('Hold expiry fixture')
  const expiring = mutation({ ...hold, resourceId: expiryResource.id, holdSeconds: 1 })
  const expiryReceipt = await service.mutate('appointments', human('owner'), expiring)
  const lock = await owner.connect()
  await lock.query('BEGIN')
  await lock.query(
    'SELECT id FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
    ['appointments', expiryResource.id],
  )
  let outcome
  const blocked = act({
    operation: 'reservation.confirm',
    resourceId: expiryResource.id,
    id: expiryReceipt.id,
    expectedRevision: 1,
  }).then(
    () => {
      outcome = 'committed'
    },
    (x) => {
      outcome = x.code
    },
  )
  try {
    let waiting = false
    for (let attempt = 0; attempt < 30; attempt++) {
      const observed = (
        await owner.query(
          "SELECT EXISTS(SELECT 1 FROM pg_stat_activity a,archava_scheduling.reservations r WHERE a.usename='scheduling_app' AND a.wait_event_type='Lock' AND a.query LIKE '%archava_scheduling.resources%FOR UPDATE%' AND r.id=$1 AND a.xact_start<r.expires_at) AS waiting",
          [expiryReceipt.id],
        )
      ).rows[0].waiting
      if (observed) {
        waiting = true
        break
      }
      await new Promise((done) => setTimeout(done, 20))
    }
    assert.equal(waiting, true, 'confirm transaction must wait for the resource before hold expiry')
    await new Promise((done) => setTimeout(done, 1300))
  } finally {
    await lock.query('COMMIT')
    lock.release()
  }
  await blocked
  assert.equal(outcome, 'conflict')
  const expired = await service.detail('appointments', human('owner'), expiryReceipt.id)
  assert.equal(expired.reservation.state, 'held')
  assert.equal(expired.reservation.effectiveState, 'expired')
  assert.equal(
    (
      await service.availability('appointments', human('owner'), {
        resourceId: expiryResource.id,
        startsAt,
        endsAt,
      })
    ).available,
    true,
  )
  assert.deepEqual(await service.mutate('appointments', human('owner'), expiring), expiryReceipt)
  assert.equal(
    (await service.detail('appointments', human('owner'), expiryReceipt.id)).reservation.expiresAt,
    expired.reservation.expiresAt,
  )
  await act({ ...hold, resourceId: expiryResource.id })
  check(
    'hold expiry uses database time after lock waits; receipt retries never extend or resurrect expired holds',
  )
  const blockResource = await makeResource('Maintenance fixture', 'asset')
  const block = await act({
    operation: 'reservation.block',
    resourceId: blockResource.id,
    title: 'Maintenance',
    startsAt,
    endsAt,
    reason: 'Service window',
  })
  await failed(act({ ...hold, resourceId: blockResource.id }), 'conflict')
  await failed(
    act({
      operation: 'reservation.confirm',
      resourceId: blockResource.id,
      id: block.id,
      expectedRevision: 1,
    }),
    'conflict',
  )
  await act({
    operation: 'reservation.cancel',
    resourceId: blockResource.id,
    id: block.id,
    expectedRevision: 1,
    reason: 'Maintenance finished',
  })
  await act({
    operation: 'resource.set-active',
    resourceId: blockResource.id,
    expectedRevision: 1,
    active: false,
    reason: 'Unavailable',
  })
  assert.equal(
    (
      await service.availability('appointments', human('owner'), {
        resourceId: blockResource.id,
        startsAt,
        endsAt,
      })
    ).available,
    false,
  )
  await failed(act({ ...hold, resourceId: blockResource.id }), 'conflict')
  await act({
    operation: 'resource.set-active',
    resourceId: blockResource.id,
    expectedRevision: 2,
    active: true,
    reason: 'Open',
  })
  await failed(act({ ...hold, startsAt: new Date(Date.now() - 10000).toISOString() }), 'conflict')
  const rentalBooking = await second.mutate(
    'rental',
    human('owner'),
    mutation({ ...hold, resourceId: rental.id, title: 'Camera hire' }),
  )
  assert.equal(
    (await second.detail('rental', human('owner'), rentalBooking.id)).reservation.title,
    'Camera hire',
  )
  check(
    'same module handles staff appointment and asset rental; maintenance blocks and disabled or past slots refuse booking',
  )
  const client = await app.connect()
  try {
    assert.equal(
      (await client.query('SELECT count(*)::integer AS count FROM archava_scheduling.reservations'))
        .rows[0].count,
      0,
    )
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','appointments',true)")
    assert.equal(
      (
        await client.query(
          "SELECT count(*)::integer AS count FROM archava_scheduling.resources WHERE tenant_id='rental'",
        )
      ).rows[0].count,
      0,
    )
    await client.query('ROLLBACK')
    for (const [sql, args] of [
      ['UPDATE archava_scheduling.memberships SET active=false', []],
      ['DELETE FROM archava_scheduling.audit', []],
      [
        'UPDATE archava_scheduling.commands SET receipt=NULL WHERE request_id=$1',
        [submission.requestId],
      ],
      [
        "INSERT INTO archava_scheduling.reservations(tenant_id,id,resource_id,owner_id,title,kind,state,starts_at,ends_at) VALUES('appointments',$1,$2,'owner','Raw overlap','booking','confirmed',$3,$4)",
        [randomUUID(), resource.id, startsAt, endsAt],
      ],
    ]) {
      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id','appointments',true)")
      await assert.rejects(() => client.query(sql, args))
      await client.query('ROLLBACK')
    }
  } finally {
    client.release()
  }
  check(
    'forced RLS, least runtime grants, immutable receipts and SQL overlap guard survive direct runtime SQL refusal checks',
  )
  const page = await service.resources('appointments', human('owner'), { limit: 1 })
  assert.ok(page.next)
  const next = await service.resources('appointments', human('owner'), {
    limit: 1,
    after: page.next,
  })
  assert.notEqual(page.items[0].id, next.items[0].id)
  const tools = domain.createSchedulingTools(service, 'appointments', human('viewer'))
  assert.equal(
    (await tools.invoke('scheduling.detail', { id: booking.id })).reservation.effectiveState,
    'cancelled',
  )
  await assert.rejects(() => tools.invoke('scheduling.resources', { tenant: 'rental' }))
  await failed(tools.invoke('scheduling.hold', hold), 'not_found')
  await owner.query(
    "UPDATE archava_scheduling.memberships SET permissions=ARRAY['resources.read','reservations.read']::text[] WHERE tenant_id='appointments' AND principal_id='viewer'",
  )
  assert.equal((await service.detail('appointments', human('viewer'), booking.id)).audit, null)
  const api = domain.createSchedulingApi(service, async () => human('owner'))
  assert.equal(
    (
      await api(
        new Request('http://local.test/api/scheduling/appointments/reservations/' + booking.id),
      )
    ).status,
    200,
  )
  assert.equal(
    (await api(new Request('http://local.test/api/scheduling/appointments/resources?roles=owner')))
      .status,
    400,
  )
  check(
    'bounded pages, optional audit visibility and read-only tools preserve authoritative API scope',
  )
  const expected = await service.detail('appointments', human('owner'), reopened.id),
    archive = join(scratch, 'scheduling.dump')
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
  app = new Pool({ ...target, user: 'scheduling_app' })
  service = new domain.SchedulingService(
    await postgres.PostgresSchedulingRepository.create(app, key),
  )
  assert.deepEqual(
    (await service.detail('appointments', human('owner'), reopened.id)).reservation,
    expected.reservation,
  )
  assert.deepEqual(
    (await service.commandStatus('appointments', human('owner'), submission.requestId)).receipt,
    booking,
  )
  check('database restart retains reservations, domain grants, audit and actor-bound receipts')
  await owner.query('CREATE DATABASE restored_scheduling')
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
    'restored_scheduling',
    archive,
  ])
  const restoredOwner = new Pool({ ...target, database: 'restored_scheduling' }),
    restoredApp = new Pool({ ...target, database: 'restored_scheduling', user: 'scheduling_app' })
  try {
    await postgres.migrateScheduling(restoredOwner)
    await postgres.grantSchedulingRuntime(restoredOwner, 'scheduling_app')
    const restored = new domain.SchedulingService(
      await postgres.PostgresSchedulingRepository.create(restoredApp, key),
    )
    const state = await restored.detail('appointments', human('owner'), reopened.id)
    assert.deepEqual(state.reservation, expected.reservation)
    assert.deepEqual(state.audit, expected.audit)
    assert.deepEqual(
      (await restored.commandStatus('appointments', human('owner'), submission.requestId)).receipt,
      booking,
    )
    await restoredOwner.query(
      "UPDATE archava_scheduling.memberships SET active=false WHERE tenant_id='appointments' AND principal_id='viewer'",
    )
    await failed(restored.resource('appointments', human('viewer'), resource.id), 'forbidden')
    await service.resource('appointments', human('viewer'), resource.id)
  } finally {
    await endProofPool(restoredApp)
    await endProofPool(restoredOwner)
  }
  check(
    'fresh backup restore preserves reservations and receipts while leaving the original database intact',
  )
  await owner.query('UPDATE archava_scheduling.schema_migrations SET checksum=$1', ['a'.repeat(64)])
  await assert.rejects(() => postgres.PostgresSchedulingRepository.create(app, key))
  await assert.rejects(() => postgres.migrateScheduling(owner))
  check('modified migration checksum refuses startup and migration')
  const destination = join(root, 'dist/scheduling-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope: 'exclusive resource reservations',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )
} finally {
  if (app && !app.ended) await endProofPool(app)
  if (owner && !owner.ended) await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true })
}
