/* global process, Request, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm, stat } from 'node:fs/promises'
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
const scratch = await mkdtemp(join(tmpdir(), 'archava-accommodation-proof-'))
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
  for (const name of ['hotel', 'villa']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-accommodation-' + name,
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
    const load = async (name) => import(pathToFileURL(require.resolve(name)).href)
    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))
    const domain = await load('@archava/accommodation'),
      postgres = await load('@archava/accommodation-pg'),
      scheduling = await load('@archava/scheduling-pg'),
      timed = await load('@archava/scheduling')
    Pool ??= require('pg').Pool
    const browser = await build({
      entryPoints: [require.resolve('@archava/accommodation/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /accommodation-pg|scheduling-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({ domain, postgres, scheduling, timed, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/accommodation')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independent cold-installed hotel and villa consumers share unchanged standalone domain and browser-safe UI',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres, scheduling, timed } = consumers[0]
  // Upgrade a real non-empty 1.5 schema, retaining every original column and business receipt.
  let oldRuntime = process.argv.includes('--cold-prior')
    ? join(scratch, 'cold-prior')
    : join(root, 'dist/releases/1.5.0/crm-runtime')
  try {
    await stat(join(oldRuntime, 'release-profile.json'))
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
    const priorSource = join(scratch, 'prior-source')
    await mkdir(priorSource)
    const bytes = execFileSync('git', ['archive', '0e396bd'], {
      cwd: root,
      maxBuffer: 64 * 1024 * 1024,
    })
    execFileSync('tar', ['-xf', '-', '-C', priorSource], { input: bytes })
    execFileSync('pnpm', ['install', '--frozen-lockfile', '--ignore-scripts'], {
      cwd: priorSource,
      stdio: 'pipe',
    })
    execFileSync('pnpm', ['crm:build'], { cwd: priorSource, stdio: 'pipe' })
    oldRuntime = join(priorSource, 'dist/crm-runtime')
  }
  assert.equal(
    JSON.parse(await readFile(join(oldRuntime, 'release-profile.json'), 'utf8')).catalog.version,
    '1.5.0',
  )
  const oldRoot = join(oldRuntime, 'node_modules')
  const oldRequire = createRequire(join(oldRoot, '../package.json'))
  const oldPg = await import(pathToFileURL(oldRequire.resolve('@archava/scheduling-pg')).href)
  const oldDomain = await import(pathToFileURL(oldRequire.resolve('@archava/scheduling')).href)
  await oldPg.migrateScheduling(owner)
  await owner.query('CREATE ROLE hotel_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await oldPg.grantSchedulingRuntime(owner, 'hotel_app')
  await owner.query(
    "INSERT INTO archava_scheduling.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('legacy','owner','human',$1,'all')",
    [oldDomain.SCHEDULING_PERMISSIONS],
  )
  app = new Pool({ ...target, user: 'hotel_app' })
  const legacy = new oldDomain.SchedulingService(
    await oldPg.PostgresSchedulingRepository.create(app, key),
  )
  const oldResource = await legacy.mutate(
    'legacy',
    human('owner'),
    mutation({
      operation: 'resource.create',
      title: 'Preserved consultant',
      kind: 'staff',
      timezone: 'UTC',
    }),
  )
  const oldRequest = mutation({
    operation: 'reservation.hold',
    resourceId: oldResource.id,
    title: 'Existing meeting',
    startsAt: new Date(Date.now() + 86400000).toISOString(),
    endsAt: new Date(Date.now() + 90000000).toISOString(),
    holdSeconds: 1800,
  })
  const oldReceipt = await legacy.mutate('legacy', human('owner'), oldRequest)
  const legacyBefore = await legacy.detail('legacy', human('owner'), oldReceipt.id)
  const oldColumns = (
    await owner.query(
      "SELECT column_name FROM information_schema.columns WHERE table_schema='archava_scheduling' AND table_name='resources' ORDER BY ordinal_position",
    )
  ).rows.map((x) => x.column_name)
  const resourceBefore = (
    await owner.query('SELECT ' + oldColumns.join(',') + ' FROM archava_scheduling.resources')
  ).rows
  await scheduling.migrateScheduling(owner)
  await scheduling.grantSchedulingRuntime(owner, 'hotel_app')
  const modernTimed = new timed.SchedulingService(
    await scheduling.PostgresSchedulingRepository.create(app, key),
  )
  assert.deepEqual(
    (await modernTimed.detail('legacy', human('owner'), oldReceipt.id)).reservation,
    legacyBefore.reservation,
  )
  assert.deepEqual(
    (await modernTimed.commandStatus('legacy', human('owner'), oldRequest.requestId)).receipt,
    oldReceipt,
  )
  assert.deepEqual(
    (await owner.query('SELECT ' + oldColumns.join(',') + ' FROM archava_scheduling.resources'))
      .rows,
    resourceBefore,
  )
  assert.equal(
    (
      await owner.query(
        'SELECT count(*)::integer AS n FROM archava_scheduling.occupancy WHERE id=$1 AND owner_module=$2',
        [oldReceipt.id, 'timed'],
      )
    ).rows[0].n,
    1,
  )
  await modernTimed.mutate(
    'legacy',
    human('owner'),
    mutation({
      operation: 'reservation.confirm',
      resourceId: oldResource.id,
      id: oldReceipt.id,
      expectedRevision: 1,
    }),
  )
  check(
    'additive shared-kernel migration preserves non-empty 1.5 records and receipts and backfills capacity without new commands',
  )
  await postgres.migrateAccommodation(owner)
  await postgres.migrateAccommodation(owner)
  await postgres.grantAccommodationRuntime(owner, 'hotel_app')
  for (const [tenant, id, kind, grants, scope] of [
    ['hotel', 'owner', 'human', domain.ACCOMMODATION_PERMISSIONS, 'all'],
    [
      'hotel',
      'viewer',
      'human',
      domain.ACCOMMODATION_PERMISSIONS.filter((x) => x.endsWith('.read')),
      'all',
    ],
    ['hotel', 'staff', 'human', domain.ACCOMMODATION_PERMISSIONS, 'assigned'],
    ['hotel', 'bot', 'service', ['properties.read', 'rooms.read'], 'all'],
    ['villa', 'owner', 'human', domain.ACCOMMODATION_PERMISSIONS, 'all'],
  ])
    await owner.query(
      'INSERT INTO archava_accommodation.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
      [tenant, id, kind, grants, scope],
    )
  let service = new domain.AccommodationService(
    await postgres.PostgresAccommodationRepository.create(app, key),
  )
  const second = new consumers[1].domain.AccommodationService(
    await consumers[1].postgres.PostgresAccommodationRepository.create(app, key),
  )
  await assert.rejects(() => postgres.PostgresAccommodationRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresAccommodationRepository.create(app, 'short'))
  assert.deepEqual(
    (
      await owner.query(
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'archava_%' ORDER BY schema_name",
      )
    ).rows.map((x) => x.schema_name),
    ['archava_accommodation', 'archava_scheduling'],
  )
  check(
    'standalone hotel requires no CRM or LLM schema and refuses owner runtime and weak stable keys',
  )
  const act = (c, requestId) => service.mutate('hotel', human('owner'), mutation(c, requestId))
  const setup = async (svc, tenant, title, timezone = 'UTC') => {
    const command = (c) => svc.mutate(tenant, human('owner'), mutation(c))
    const p = await command({
      operation: 'property.create',
      title,
      timezone,
      checkInTime: '00:00',
      checkOutTime: '00:00',
    })
    const type = await command({
      operation: 'room-type.create',
      propertyId: p.id,
      title: 'Standard',
      maxGuests: 2,
      nightlyRate: 500000,
    })
    const room = await command({
      operation: 'room.create',
      propertyId: p.id,
      roomTypeId: type.id,
      code: '101',
    })
    return { p, type, room, command }
  }
  const hotel = await setup(service, 'hotel', 'Hotel A'),
    villa = await setup(second, 'villa', 'Villa B')
  const date = (days) => new Date(Date.now() + days * 86400000).toISOString().slice(0, 10)
  const hold = {
    operation: 'stay.hold',
    propertyId: hotel.p.id,
    roomId: hotel.room.id,
    arrival: date(2),
    departure: date(4),
    guestName: 'Guest A',
    guests: 2,
    holdSeconds: 1800,
  }
  await failed(service.property('hotel', human('staff'), hotel.p.id), 'not_found')
  await failed(service.property('villa', human('owner'), hotel.p.id), 'not_found')
  await failed(service.property('missing', human('owner'), hotel.p.id), 'forbidden')
  await failed(service.mutate('hotel', human('viewer'), mutation(hold)), 'forbidden')
  await failed(
    () =>
      service.mutate('hotel', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }, mutation(hold)),
    'forbidden',
  )
  const bot = { kind: 'service', id: 'bot', onBehalfOf: 'owner' }
  assert.equal((await service.rooms('hotel', bot, { propertyId: hotel.p.id })).items.length, 1)
  await failed(service.stays('hotel', bot, { propertyId: hotel.p.id }), 'forbidden')
  await failed(
    service.rooms(
      'hotel',
      { kind: 'service', id: 'bot', onBehalfOf: 'staff' },
      { propertyId: hotel.p.id },
    ),
    'not_found',
  )
  check(
    'tenant and parent ownership, read-only humans and intersected service grants enforce current domain boundaries',
  )
  const submission = mutation(hold)
  const results = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('hotel', human('owner'), submission)),
  )
  assert.equal(new Set(results.map((x) => x.id)).size, 1)
  const booking = results[0]
  let detail = await service.detail('hotel', human('owner'), booking.id)
  assert.equal(detail.nights.length, 2)
  assert.equal(detail.stay.totalEstimate, 1000000)
  assert.equal(detail.audit.length, 1)
  assert.equal(
    (
      await owner.query(
        'SELECT count(*)::integer AS n FROM archava_scheduling.occupancy WHERE id=$1',
        [booking.id],
      )
    ).rows[0].n,
    1,
  )
  await failed(act({ ...hold, guestName: 'Changed' }, submission.requestId), 'conflict')
  await failed(service.commandStatus('hotel', human('viewer'), submission.requestId), 'forbidden')
  await owner.query(
    "UPDATE archava_accommodation.memberships SET permissions=$1 WHERE tenant_id='hotel' AND principal_id='owner'",
    [domain.ACCOMMODATION_PERMISSIONS.filter((x) => x !== 'stays.write')],
  )
  await failed(service.mutate('hotel', human('owner'), submission), 'forbidden')
  await owner.query(
    "UPDATE archava_accommodation.memberships SET permissions=$1 WHERE tenant_id='hotel' AND principal_id='owner'",
    [domain.ACCOMMODATION_PERMISSIONS],
  )
  assert.deepEqual(
    (await service.commandStatus('hotel', human('owner'), submission.requestId)).receipt,
    booking,
  )
  check(
    '20 duplicate requests create one atomic stay, quote, occupancy, audit and actor-bound receipt; retries recheck write grants',
  )
  await act({
    operation: 'room-type.set-rate',
    propertyId: hotel.p.id,
    id: hotel.type.id,
    expectedRevision: 1,
    nightlyRate: 600000,
    reason: 'New published tariff',
  })
  assert.deepEqual(
    (await service.detail('hotel', human('owner'), booking.id)).nights,
    detail.nights,
  )
  await failed(act({ ...hold, guests: 3 }), 'conflict')
  await failed(act({ ...hold, roomId: villa.room.id }), 'not_found')
  await failed(
    act({
      operation: 'room.create',
      propertyId: hotel.p.id,
      roomTypeId: villa.type.id,
      code: '102',
    }),
    'not_found',
  )
  const before = (
    await owner.query(
      "SELECT count(*)::integer AS n FROM archava_scheduling.resources WHERE tenant_id='hotel'",
    )
  ).rows[0].n
  const duplicateRoom = mutation({
    operation: 'room.create',
    propertyId: hotel.p.id,
    roomTypeId: hotel.type.id,
    code: '101',
  })
  await failed(service.mutate('hotel', human('owner'), duplicateRoom), 'conflict')
  assert.equal(
    (
      await owner.query(
        "SELECT count(*)::integer AS n FROM archava_scheduling.resources WHERE tenant_id='hotel'",
      )
    ).rows[0].n,
    before,
  )
  assert.equal(
    (await service.commandStatus('hotel', human('owner'), duplicateRoom.requestId)).status,
    'not_committed',
  )
  check(
    'rate changes affect only future quotes; capacity and property boundaries reject invalid stays and roll back orphan resources',
  )
  await act({
    operation: 'stay.confirm',
    propertyId: hotel.p.id,
    id: booking.id,
    expectedRevision: 1,
  })
  await failed(
    act({
      operation: 'stay.check-in',
      propertyId: hotel.p.id,
      id: booking.id,
      expectedRevision: 2,
    }),
    'conflict',
  )
  await failed(
    act({
      operation: 'room.set-active',
      propertyId: hotel.p.id,
      roomId: hotel.room.id,
      expectedRevision: 1,
      active: false,
      reason: 'Close',
    }),
    'conflict',
  )
  await failed(act(hold), 'conflict')
  const adjacent = await act({ ...hold, arrival: date(4), departure: date(5) })
  await act({
    operation: 'stay.cancel',
    propertyId: hotel.p.id,
    id: adjacent.id,
    expectedRevision: 1,
    reason: 'Release adjacent fixture',
  })
  await act({
    operation: 'stay.cancel',
    propertyId: hotel.p.id,
    id: booking.id,
    expectedRevision: 2,
    reason: 'Release fixture',
  })
  const race = await Promise.allSettled(
    Array.from({ length: 20 }, (_, i) => act({ ...hold, guestName: 'Contender ' + i })),
  )
  assert.equal(race.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal(
    race.filter((x) => x.status === 'rejected' && x.reason.code === 'conflict').length,
    19,
  )
  const winner = race.find((x) => x.status === 'fulfilled').value
  assert.equal(
    (await service.detail('hotel', human('owner'), winner.id)).stay.totalEstimate,
    1200000,
  )
  await act({
    operation: 'stay.cancel',
    propertyId: hotel.p.id,
    id: winner.id,
    expectedRevision: 1,
    reason: 'Release race fixture',
  })
  check(
    '20 independent room-night contenders produce one winner; adjacency, cancellation and terminal states preserve capacity',
  )
  const expiring = mutation({ ...hold, holdSeconds: 1 })
  const exp = await service.mutate('hotel', human('owner'), expiring)
  const lock = await owner.connect()
  await lock.query('BEGIN')
  await lock.query(
    'SELECT id FROM archava_scheduling.resources WHERE tenant_id=$1 AND id=$2 FOR UPDATE',
    ['hotel', hotel.room.id],
  )
  let outcome
  const blocked = act({
    operation: 'stay.confirm',
    propertyId: hotel.p.id,
    id: exp.id,
    expectedRevision: 1,
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
          "SELECT EXISTS(SELECT 1 FROM pg_stat_activity a,archava_accommodation.stays s WHERE a.usename='hotel_app' AND a.wait_event_type='Lock' AND a.query LIKE '%archava_scheduling.resources%FOR UPDATE%' AND s.id=$1 AND a.xact_start<s.expires_at) AS waiting",
          [exp.id],
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
    (await service.detail('hotel', human('owner'), exp.id)).stay.effectiveState,
    'expired',
  )
  assert.deepEqual(await service.mutate('hotel', human('owner'), expiring), exp)
  assert.equal(
    (
      await service.availability('hotel', human('owner'), {
        propertyId: hotel.p.id,
        roomId: hotel.room.id,
        arrival: hold.arrival,
        departure: hold.departure,
        guests: 2,
      })
    ).available,
    true,
  )
  check(
    'expiry is evaluated by database clock after resource waits and receipt recovery never resurrects an expired hold',
  )
  const current = await act({ ...hold, arrival: date(0), departure: date(1), guestName: 'Walk-in' })
  await act({
    operation: 'stay.confirm',
    propertyId: hotel.p.id,
    id: current.id,
    expectedRevision: 1,
  })
  await failed(
    act({
      operation: 'stay.check-in',
      propertyId: hotel.p.id,
      id: current.id,
      expectedRevision: 2,
    }),
    'conflict',
  )
  await failed(
    act({
      operation: 'room.housekeeping',
      propertyId: hotel.p.id,
      roomId: hotel.room.id,
      expectedRevision: 1,
      state: 'inspected',
      reason: 'Skip cleaning',
    }),
    'conflict',
  )
  await act({
    operation: 'room.housekeeping',
    propertyId: hotel.p.id,
    roomId: hotel.room.id,
    expectedRevision: 1,
    state: 'clean',
    reason: 'Cleaned',
  })
  await act({
    operation: 'room.housekeeping',
    propertyId: hotel.p.id,
    roomId: hotel.room.id,
    expectedRevision: 2,
    state: 'inspected',
    reason: 'Inspected',
  })
  await act({
    operation: 'stay.check-in',
    propertyId: hotel.p.id,
    id: current.id,
    expectedRevision: 2,
  })
  assert.equal((await service.room('hotel', human('owner'), hotel.room.id)).occupied, true)
  await failed(
    act({
      operation: 'room.housekeeping',
      propertyId: hotel.p.id,
      roomId: hotel.room.id,
      expectedRevision: 4,
      state: 'clean',
      reason: 'Occupied',
    }),
    'conflict',
  )
  await act({
    operation: 'stay.check-out',
    propertyId: hotel.p.id,
    id: current.id,
    expectedRevision: 3,
  })
  const checked = await service.detail('hotel', human('owner'), current.id)
  assert.equal(checked.room.housekeeping, 'dirty')
  assert.equal(checked.room.occupied, false)
  assert.equal(checked.stay.totalEstimate, 600000)
  assert.equal(checked.nights.length, 1)
  assert.equal(
    (
      await service.availability('hotel', human('owner'), {
        propertyId: hotel.p.id,
        roomId: hotel.room.id,
        arrival: date(0),
        departure: date(1),
        guests: 2,
      })
    ).available,
    true,
  )
  await failed(
    act({
      operation: 'stay.check-in',
      propertyId: hotel.p.id,
      id: current.id,
      expectedRevision: 4,
    }),
    'conflict',
  )
  check(
    'current-day walk-in requires confirmed stay and inspected room; checkout dirties room and releases capacity without rewriting quote',
  )
  const lateRoom = await act({
    operation: 'room.create',
    propertyId: hotel.p.id,
    roomTypeId: hotel.type.id,
    code: 'LATE',
  })
  const lateId = randomUUID(),
    lateStart = new Date(Date.now() - 86400000).toISOString(),
    lateEnd = new Date(Date.now() - 1000).toISOString()
  // Seed an overdue historical guest in this disposable proof only. API never accepts clock overrides or backdated writes.
  const fixture = await owner.connect()
  try {
    await fixture.query('BEGIN')
    await fixture.query('ALTER TABLE archava_scheduling.occupancy DISABLE TRIGGER occupancy_guard')
    await fixture.query('ALTER TABLE archava_accommodation.stays DISABLE TRIGGER stay_guard')
    await fixture.query('ALTER TABLE archava_accommodation.stay_nights DISABLE TRIGGER night_guard')
    await fixture.query(
      "INSERT INTO archava_scheduling.occupancy(tenant_id,id,resource_id,owner_module,state,starts_at,ends_at,expires_at) VALUES('hotel',$1,$2,'accommodation','confirmed',$3,$4,$5)",
      [lateId, lateRoom.id, lateStart, lateEnd, lateStart],
    )
    await fixture.query(
      "INSERT INTO archava_accommodation.stays(tenant_id,id,property_id,room_id,owner_id,guest_name,guests,arrival,departure,nights,nightly_rate,rate_revision,total_estimate,state,starts_at,ends_at,expires_at,checked_in_at) VALUES('hotel',$1,$2,$3,'owner','Overdue historical guest',1,$4,$5,1,600000,2,600000,'in_house',$6,$7,$6,$6)",
      [lateId, hotel.p.id, lateRoom.id, date(-1), date(0), lateStart, lateEnd],
    )
    await fixture.query(
      "INSERT INTO archava_accommodation.stay_nights(tenant_id,stay_id,night_date,rate,rate_revision) VALUES('hotel',$1,$2,600000,2)",
      [lateId, date(-1)],
    )
    await fixture.query('ALTER TABLE archava_scheduling.occupancy ENABLE TRIGGER occupancy_guard')
    await fixture.query('ALTER TABLE archava_accommodation.stays ENABLE TRIGGER stay_guard')
    await fixture.query('ALTER TABLE archava_accommodation.stay_nights ENABLE TRIGGER night_guard')
    await fixture.query('COMMIT')
  } catch (error) {
    await fixture.query('ROLLBACK')
    throw error
  } finally {
    fixture.release()
  }
  const lateQuery = {
    propertyId: hotel.p.id,
    roomId: lateRoom.id,
    arrival: date(0),
    departure: date(1),
    guests: 1,
  }
  assert.equal((await service.availability('hotel', human('owner'), lateQuery)).available, false)
  await failed(act({ ...hold, ...lateQuery }), 'conflict')
  await failed(
    act({
      operation: 'room.set-active',
      propertyId: hotel.p.id,
      roomId: lateRoom.id,
      expectedRevision: 1,
      active: false,
      reason: 'Still occupied',
    }),
    'conflict',
  )
  await act({
    operation: 'stay.check-out',
    propertyId: hotel.p.id,
    id: lateId,
    expectedRevision: 1,
  })
  assert.equal((await service.availability('hotel', human('owner'), lateQuery)).available, true)
  check(
    'overdue physical guest blocks current-day booking and disabling even after planned capacity ends; actual checkout releases it',
  )
  const staffAct = (c) => service.mutate('hotel', human('staff'), mutation(c))
  const staffProperty = await staffAct({
    operation: 'property.create',
    title: 'Assigned property',
    timezone: 'UTC',
  })
  assert.equal((await service.property('hotel', human('staff'), staffProperty.id)).ownerId, 'staff')
  const staffType = await staffAct({
    operation: 'room-type.create',
    propertyId: staffProperty.id,
    title: 'Assigned type',
    maxGuests: 2,
    nightlyRate: 100000,
  })
  const staffRoom = await staffAct({
    operation: 'room.create',
    propertyId: staffProperty.id,
    roomTypeId: staffType.id,
    code: 'STAFF',
  })
  const staffStay = await staffAct({ ...hold, propertyId: staffProperty.id, roomId: staffRoom.id })
  await service.detail('hotel', human('staff'), staffStay.id)
  const ownerStay = await act({
    ...hold,
    propertyId: staffProperty.id,
    roomId: staffRoom.id,
    arrival: date(4),
    departure: date(5),
  })
  assert.equal(
    (await service.stays('hotel', human('staff'), { propertyId: staffProperty.id })).items.length,
    1,
  )
  await failed(service.detail('hotel', human('staff'), ownerStay.id), 'not_found')
  check(
    'assigned operator can manage own property and stay while another operator stay remains hidden under same property',
  )
  const dst = await setup(service, 'hotel', 'DST hotel', 'America/New_York')
  const dstReceipt = await dst.command({
    ...hold,
    propertyId: dst.p.id,
    roomId: dst.room.id,
    arrival: '2027-03-13',
    departure: '2027-03-15',
  })
  const dstStay = (await service.detail('hotel', human('owner'), dstReceipt.id)).stay
  assert.equal(dstStay.nights, 2)
  assert.equal(Date.parse(dstStay.endsAt) - Date.parse(dstStay.startsAt), 47 * 3600000)
  const villaBooking = await villa.command({
    ...hold,
    propertyId: villa.p.id,
    roomId: villa.room.id,
    guestName: 'Villa guest',
  })
  assert.equal((await second.detail('villa', human('owner'), villaBooking.id)).stay.nights, 2)
  check(
    'calendar nights remain correct across DST and unchanged module supports independent hotel and villa configurations',
  )
  await owner.query(
    "INSERT INTO archava_scheduling.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES('hotel','owner','human',$1,'all')",
    [timed.SCHEDULING_PERMISSIONS],
  )
  assert.equal((await modernTimed.resources('hotel', human('owner'))).items.length, 0)
  await failed(modernTimed.resource('hotel', human('owner'), hotel.room.id), 'not_found')
  await failed(
    modernTimed.mutate(
      'hotel',
      human('owner'),
      mutation({
        operation: 'reservation.hold',
        resourceId: hotel.room.id,
        title: 'Bypass hotel',
        startsAt: dstStay.startsAt,
        endsAt: dstStay.endsAt,
        holdSeconds: 900,
      }),
    ),
    'not_found',
  )
  check(
    'generic reservation API cannot expose or mutate accommodation-managed rooms even with all generic grants',
  )
  const client = await app.connect()
  try {
    assert.equal(
      (await client.query('SELECT count(*)::integer AS n FROM archava_accommodation.stays')).rows[0]
        .n,
      0,
    )
    for (const sql of [
      'UPDATE archava_accommodation.memberships SET active=false',
      'UPDATE archava_accommodation.stay_nights SET rate=1',
      'UPDATE archava_accommodation.commands SET receipt=NULL',
      'DELETE FROM archava_accommodation.audit',
    ]) {
      await client.query('BEGIN')
      await client.query("SELECT set_config('archava.tenant_id','hotel',true)")
      await assert.rejects(() => client.query(sql))
      await client.query('ROLLBACK')
    }
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','hotel',true)")
    assert.equal(
      (
        await client.query(
          "SELECT count(*)::integer AS n FROM archava_accommodation.stays WHERE tenant_id='villa'",
        )
      ).rows[0].n,
      0,
    )
    await client.query('ROLLBACK')
  } finally {
    client.release()
  }
  const tools = domain.createAccommodationTools(service, 'hotel', bot)
  assert.equal(
    (await tools.invoke('accommodation.rooms', { propertyId: hotel.p.id })).items.length,
    2,
  )
  await assert.rejects(() =>
    tools.invoke('accommodation.rooms', { propertyId: hotel.p.id, tenant: 'villa' }),
  )
  await failed(tools.invoke('accommodation.check-in', { id: current.id }), 'not_found')
  const api = domain.createAccommodationApi(service, async () => human('viewer'))
  assert.equal(
    (await api(new Request('http://local.test/api/accommodation/hotel/properties?role=owner')))
      .status,
    400,
  )
  assert.equal(
    (
      await api(
        new Request('http://local.test/api/accommodation/hotel/commands', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(mutation(hold)),
        }),
      )
    ).status,
    403,
  )
  check(
    'forced tenant policies, immutable quotes and receipts, append-only audit and strictly read-only tool registry resist bypass',
  )
  const expected = await service.detail('hotel', human('owner'), current.id),
    archive = join(scratch, 'accommodation.dump')
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
  app = new Pool({ ...target, user: 'hotel_app' })
  service = new domain.AccommodationService(
    await postgres.PostgresAccommodationRepository.create(app, key),
  )
  const restarted = await service.detail('hotel', human('owner'), current.id)
  assert.deepEqual(restarted.stay, expected.stay)
  assert.deepEqual(restarted.nights, expected.nights)
  assert.deepEqual(restarted.audit, expected.audit)
  assert.deepEqual(
    (await service.commandStatus('hotel', human('owner'), submission.requestId)).receipt,
    booking,
  )
  check(
    'database restart retains room readiness, stays, immutable nightly rates, grants, audit and receipts',
  )
  await owner.query('CREATE DATABASE restored_accommodation')
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
    'restored_accommodation',
    archive,
  ])
  const ro = new Pool({ ...target, database: 'restored_accommodation' }),
    ra = new Pool({ ...target, database: 'restored_accommodation', user: 'hotel_app' })
  try {
    await scheduling.migrateScheduling(ro)
    await postgres.migrateAccommodation(ro)
    await postgres.grantAccommodationRuntime(ro, 'hotel_app')
    const restored = new domain.AccommodationService(
      await postgres.PostgresAccommodationRepository.create(ra, key),
    )
    const d = await restored.detail('hotel', human('owner'), current.id)
    assert.deepEqual(d.stay, expected.stay)
    assert.deepEqual(d.nights, expected.nights)
    assert.deepEqual(d.audit, expected.audit)
    assert.deepEqual(
      (await restored.commandStatus('hotel', human('owner'), submission.requestId)).receipt,
      booking,
    )
    await ro.query(
      "UPDATE archava_accommodation.memberships SET active=false WHERE tenant_id='hotel' AND principal_id='viewer'",
    )
    await failed(restored.room('hotel', human('viewer'), hotel.room.id), 'forbidden')
    await service.room('hotel', human('viewer'), hotel.room.id)
  } finally {
    await endProofPool(ra)
    await endProofPool(ro)
  }
  check(
    'fresh backup restore retains hotel records and capacity while grant changes do not affect original database',
  )
  await owner.query('UPDATE archava_accommodation.schema_migrations SET checksum=$1', [
    'a'.repeat(64),
  ])
  await assert.rejects(() => postgres.PostgresAccommodationRepository.create(app, key))
  await assert.rejects(() => postgres.migrateAccommodation(owner))
  check('modified hotel migration ledger refuses repository startup and migration')
  const destination = join(root, 'dist/accommodation-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope: 'bounded room-night stays and housekeeping',
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
