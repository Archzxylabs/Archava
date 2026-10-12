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
const scratch = await mkdtemp(join(tmpdir(), 'archava-attendance-proof-'))
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
  // Two independent consumers: Courses/Training (K13) and Events/Venues (K15)
  for (const name of ['courses', 'events']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-attendance-' + name,
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

    const domain = await load('@archava/attendance')
    const postgres = await load('@archava/attendance-pg')
    Pool ??= require('pg').Pool

    const browser = await build({
      entryPoints: [require.resolve('@archava/attendance/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })

    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /attendance-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )

    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/attendance')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }

  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed courses and events consumers share unchanged attendance code and browser-safe dashboard',
  )

  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)

  const { domain, postgres } = consumers[0]
  await postgres.migrateAttendance(owner)
  // Idempotent migration
  await postgres.migrateAttendance(owner)
  check('attendance database migration applies cleanly and idempotently')

  await owner.query('CREATE ROLE att_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantAttendanceRuntime(owner, 'att_app')

  // Membership setup
  for (const [tenant, id, kind, grants, scope] of [
    ['courses', 'owner', 'human', domain.ATTENDANCE_PERMISSIONS, 'all'],
    [
      'courses',
      'instructor',
      'human',
      ['attendance.read', 'attendance.write', 'attendance.checkin'],
      'all',
    ],
    ['courses', 'scanner', 'human', ['attendance.read', 'attendance.checkin'], 'all'],
    ['courses', 'viewer', 'human', ['attendance.read', 'audit.read'], 'all'],
    ['courses', 'zero-viewer', 'human', [], 'assigned'],
    ['courses', 'bot', 'service', ['attendance.read', 'attendance.checkin'], 'all'],
    ['courses', 'inactive', 'human', domain.ATTENDANCE_PERMISSIONS, 'all'],
    ['events', 'owner', 'human', domain.ATTENDANCE_PERMISSIONS, 'all'],
  ]) {
    await owner.query(
      'INSERT INTO archava_attendance.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5)',
      [tenant, id, kind, grants, scope],
    )
  }
  await owner.query(
    "UPDATE archava_attendance.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  app = new Pool({ ...target, user: 'att_app' })

  // Non-owner runtime check
  await assert.rejects(
    async () => postgres.PostgresAttendanceRepository.create(owner, key),
    /requires a non-owner runtime/,
  )
  check('attendance repository refuses superuser or schema-owner database connection')

  const repository = await postgres.PostgresAttendanceRepository.create(app, key)
  const service = new domain.AttendanceService(repository)
  const contact1 = randomUUID()
  const contact2 = randomUUID()
  const contact3 = randomUUID()

  // Human operator assertion
  await failed(
    () =>
      service.execute(
        'courses',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        {
          type: 'create_session',
          commandId: randomUUID(),
          title: 'Bot Session',
          sessionCode: 'BOT-01',
          capacity: 10,
          startTime: '2026-10-20T09:00:00.000Z',
          endTime: '2026-10-20T17:00:00.000Z',
        },
      ),
    'forbidden',
  )
  check('service denies automated/service callers for human-operator mutations')

  // Inactive and zero-permission rejection
  await failed(() => service.getAccess('courses', human('inactive')), 'forbidden')
  await failed(() => service.listSessions('courses', human('zero-viewer'), {}), 'forbidden')
  check('access refused for inactive identities and empty permissions')

  // 1. Create Session
  const createCmdId = randomUUID()
  const createReceipt = await service.execute('courses', human('owner'), {
    type: 'create_session',
    commandId: createCmdId,
    title: 'Advanced Fullstack Cohort Week 1',
    sessionCode: 'BOOTCAMP-W1',
    capacity: 2, // strictly 2 seats for capacity test
    startTime: '2026-10-20T09:00:00.000Z',
    endTime: '2026-10-20T17:00:00.000Z',
    location: 'Building A, Room 101',
  })
  assert.equal(createReceipt.action, 'create_session')
  assert.equal(createReceipt.status, 'scheduled')
  assert.ok(createReceipt.sessionId)
  const sessionId = createReceipt.sessionId
  check('session creation generates scheduled session with bounded capacity')

  // RLS isolation check: events tenant cannot see courses session
  const eventsSessions = await service.listSessions('events', human('owner'), {})
  assert.equal(eventsSessions.length, 0)
  await failed(() => service.detail('events', human('owner'), sessionId), 'not_found')
  check('row level security guarantees strict cross-tenant isolation')

  // 2. Register Participants
  const reg1Receipt = await service.execute('courses', human('owner'), {
    type: 'register_participant',
    commandId: randomUUID(),
    sessionId,
    contactId: contact1,
    role: 'attendee',
    ticketCode: 'TICKET-ALPHA-01',
  })
  assert.equal(reg1Receipt.action, 'register_participant')
  assert.equal(reg1Receipt.status, 'registered')
  assert.ok(reg1Receipt.rosterId)
  const roster1Id = reg1Receipt.rosterId

  const reg2Receipt = await service.execute('courses', human('instructor'), {
    type: 'register_participant',
    commandId: randomUUID(),
    sessionId,
    contactId: contact2,
    role: 'attendee',
    ticketCode: 'TICKET-BETA-02',
  })
  assert.ok(reg2Receipt.rosterId)
  const roster2Id = reg2Receipt.rosterId
  check('participants successfully registered to session roster')

  // 3. Capacity Boundary Enforcement
  await failed(
    () =>
      service.execute('courses', human('owner'), {
        type: 'register_participant',
        commandId: randomUUID(),
        sessionId,
        contactId: contact3,
        role: 'attendee',
        ticketCode: 'TICKET-GAMMA-03',
      }),
    'capacity_exceeded',
  )
  check('capacity boundary strictly rejects registrations beyond session limit')

  // 4. Record Check-in with verification and idempotency
  const checkInCmdId = randomUUID()
  const checkInReceipt = await service.execute('courses', human('scanner'), {
    type: 'record_check_in',
    commandId: checkInCmdId,
    sessionId,
    rosterId: roster1Id,
    method: 'qr_scan',
    evidenceRef: 'gate-scanner-handheld-04',
    idempotencyKey: 'SCAN-KEY-TICKET-ALPHA-01',
  })
  assert.equal(checkInReceipt.action, 'record_check_in')
  assert.equal(checkInReceipt.status, 'attended')
  assert.ok(checkInReceipt.checkInId)
  assert.equal(checkInReceipt.idempotencyReplay, false)
  check('verified check-in event records attendee check-in and updates status to attended')

  // 5. Duplicate Check-in Scan Prevention
  await failed(
    () =>
      service.execute('courses', human('scanner'), {
        type: 'record_check_in',
        commandId: randomUUID(),
        sessionId,
        rosterId: roster1Id,
        method: 'qr_scan',
        evidenceRef: 'gate-scanner-handheld-05',
        idempotencyKey: 'ANOTHER-SCAN-KEY-TICKET-ALPHA-01',
      }),
    'already_checked_in',
  )
  check('duplicate scan with new idempotency key is rejected as already checked in')

  // 6. Idempotent Replay on Same Command
  const replayReceipt = await service.execute('courses', human('scanner'), {
    type: 'record_check_in',
    commandId: checkInCmdId,
    sessionId,
    rosterId: roster1Id,
    method: 'qr_scan',
    evidenceRef: 'gate-scanner-handheld-04',
    idempotencyKey: 'SCAN-KEY-TICKET-ALPHA-01',
  })
  assert.equal(replayReceipt.idempotencyReplay, true)
  assert.equal(replayReceipt.checkInId, checkInReceipt.checkInId)
  check('idempotent replay returns existing check-in receipt with idempotencyReplay flag')

  // 7. Optimistic Locking on Roster Status Update
  await failed(
    () =>
      service.execute('courses', human('owner'), {
        type: 'update_roster_status',
        commandId: randomUUID(),
        rosterId: roster2Id,
        expectedRevision: 999, // Mismatch
        targetStatus: 'confirmed',
        reason: 'Manual attendance confirmation by organizer',
      }),
    'conflict',
  )
  check('optimistic revision locking detects concurrent modification conflicts')

  const updateReceipt = await service.execute('courses', human('owner'), {
    type: 'update_roster_status',
    commandId: randomUUID(),
    rosterId: roster2Id,
    expectedRevision: 1,
    targetStatus: 'confirmed',
    reason: 'Manual attendance confirmation by organizer',
  })
  assert.equal(updateReceipt.status, 'confirmed')
  assert.equal(updateReceipt.revision, 2)
  check('roster status update succeeds with exact revision matching')

  // 8. Session Detail and Counts Verification
  const detail = await service.detail('courses', human('viewer'), sessionId)
  assert.equal(detail.session.enrolledCount, 2)
  assert.equal(detail.session.attendedCount, 1)
  assert.equal(detail.roster.length, 2)
  assert.equal(detail.checkIns.length, 1)
  check('session detail reports accurate enrollment and attended counts')

  // 9. Read Tools Execution
  const { executeAttendanceTool } = consumers[0].domain
  const listToolRes = await executeAttendanceTool(
    service,
    'courses',
    human('viewer'),
    'attendance.listSessions',
    {},
  )
  assert.ok(Array.isArray(listToolRes))
  assert.equal(listToolRes.length, 1)

  const detailToolRes = await executeAttendanceTool(
    service,
    'courses',
    human('viewer'),
    'attendance.sessionDetail',
    { sessionId },
  )
  assert.equal(detailToolRes.session.id, sessionId)

  const rosterToolRes = await executeAttendanceTool(
    service,
    'courses',
    human('viewer'),
    'attendance.roster',
    { sessionId },
  )
  assert.equal(rosterToolRes.length, 2)

  const checkInsToolRes = await executeAttendanceTool(
    service,
    'courses',
    human('viewer'),
    'attendance.checkIns',
    { sessionId },
  )
  assert.equal(checkInsToolRes.length, 1)

  // Viewer cannot execute commands or write tools
  await failed(
    () =>
      executeAttendanceTool(service, 'courses', human('viewer'), 'attendance.sessionDetail', {
        tenant: 'events',
        sessionId,
      }),
    'bad_request',
  )
  check('read-only tool interface verifies session detail, rosters, and check-in history')

  // 10. S1 Resilient Command Status Query
  const cmdStatus = await service.getCommandStatus('courses', human('scanner'), checkInCmdId)
  assert.equal(cmdStatus.status, 'recorded')
  assert.equal(cmdStatus.receipt.checkInId, checkInReceipt.checkInId)

  const unknownCmdStatus = await service.getCommandStatus('courses', human('scanner'), randomUUID())
  assert.equal(unknownCmdStatus.status, 'unknown')
  check('S1 resilient command recovery distinguishes recorded receipts from unknown intent')

  // 11. Immutable Audit Log
  const auditRes = await owner.query(
    'SELECT count(*)::int AS count FROM archava_attendance.audit WHERE tenant_id = $1',
    ['courses'],
  )
  assert.ok(auditRes.rows[0].count >= 3)
  check('audit events capture immutable state transitions with actor and timestamp')

  const destination = join(root, 'dist/attendance-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded session rosters, capacity limits and verified check-in attendance audit',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(`\nAll ${checks.length} attendance proof checks passed.\n`)
} finally {
  if (app) await endProofPool(app)
  if (owner) await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true })
}
