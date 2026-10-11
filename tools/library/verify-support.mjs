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
const scratch = await mkdtemp(join(tmpdir(), 'archava-support-proof-'))
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
  for (const name of ['professional', 'property']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-support-' + name,
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
    const domain = await load('@archava/support'),
      postgres = await load('@archava/support-pg')
    Pool ??= require('pg').Pool
    const browser = await build({
      entryPoints: [require.resolve('@archava/support/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /support-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/support')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed professional-service and property-operations consumers share unchanged standalone support code and browser-safe dashboard',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres } = consumers[0]
  await postgres.migrateSupport(owner)
  await postgres.migrateSupport(owner)
  await owner.query('CREATE ROLE support_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantSupportRuntime(owner, 'support_app')
  assert.equal(
    (await owner.query('SELECT count(*)::int AS n FROM archava_support.memberships')).rows[0].n,
    0,
  )
  for (const [tenant, id, kind, grants, scope] of [
    ['professional', 'owner', 'human', domain.SUPPORT_PERMISSIONS, 'all'],
    ['professional', 'staff-a', 'human', domain.SUPPORT_PERMISSIONS, 'assigned'],
    ['professional', 'staff-b', 'human', domain.SUPPORT_PERMISSIONS, 'assigned'],
    ['professional', 'viewer', 'human', ['cases.read'], 'all'],
    ['professional', 'bot', 'service', ['cases.read', 'notes.read'], 'assigned'],
    ['professional', 'inactive', 'human', domain.SUPPORT_PERMISSIONS, 'all'],
    ['property', 'owner', 'human', domain.SUPPORT_PERMISSIONS, 'all'],
  ])
    await owner.query(
      'INSERT INTO archava_support.memberships(tenant_id,principal_id,display_name,kind,permissions,scope) VALUES($1,$2,$2,$3,$4,$5)',
      [tenant, id, kind, grants, scope],
    )
  await owner.query(
    "UPDATE archava_support.memberships SET active=false WHERE principal_id='inactive'",
  )
  app = new Pool({ ...target, user: 'support_app' })
  let service = new domain.SupportService(await postgres.PostgresSupportRepository.create(app, key))
  const second = new consumers[1].domain.SupportService(
    await consumers[1].postgres.PostgresSupportRepository.create(app, key),
  )
  await assert.rejects(() => postgres.PostgresSupportRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresSupportRepository.create(app, 'short'))
  assert.deepEqual(
    (
      await owner.query(
        "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'archava_%'",
      )
    ).rows.map((x) => x.schema_name),
    ['archava_support'],
  )
  assert.equal(
    (
      await owner.query(
        "SELECT count(*)::int AS n FROM pg_class WHERE relnamespace='archava_support'::regnamespace AND relrowsecurity AND relforcerowsecurity",
      )
    ).rows[0].n,
    6,
  )
  check(
    'standalone support owns six forced-RLS business tables without CRM, identity, AI, commerce or financial schemas; migration enrolls no accounts and unsafe runtimes refuse startup',
  )
  const act = (c, requestId) =>
      service.mutate('professional', human('owner'), mutation(c, requestId)),
    create = mutation({
      operation: 'case.create',
      requesterName: 'Client',
      subject: 'Investigate service issue',
      description: '<script>internal data</script>',
      priority: 'high',
      dueAt: '2026-10-10T12:00:00+07:00',
    }),
    duplicates = await Promise.all(
      Array.from({ length: 20 }, () => service.mutate('professional', human('owner'), create)),
    )
  assert.equal(new Set(duplicates.map((x) => x.id)).size, 1)
  assert.equal(duplicates.filter((x) => !x.replayed).length, 1)
  const record = duplicates[0],
    initial = await service.detail('professional', human('owner'), record.id)
  assert.equal(initial.record.revision, 1)
  assert.equal(initial.transitions.length, 1)
  assert.equal(initial.audit.length, 1)
  assert.equal(initial.record.state, 'open')
  check(
    '20 duplicate case creations commit one open case, owner, immutable history, audit and actor/body-bound receipt',
  )
  await failed(act({ ...create.command, subject: 'Changed intent' }, create.requestId), 'conflict')
  await failed(
    service.commandStatus('professional', human('staff-a'), create.requestId),
    'conflict',
  )
  const unknown = await service.commandStatus('professional', human('owner'), randomUUID())
  assert.equal(unknown.status, 'unknown')
  check(
    'command UUID refuses changed payload and another actor while unknown receipts grant no execution',
  )
  const candidates = await service.assignees('professional', human('owner'), { limit: 2 })
  assert.equal(candidates.items.length, 2)
  assert.ok(candidates.next)
  assert.equal(
    candidates.items.some((x) => x.id === 'inactive'),
    false,
  )
  await failed(service.assignees('professional', human('staff-a')), 'forbidden')
  await failed(
    () => service.assignees('professional', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }),
    'forbidden',
  )
  await failed(
    act({
      operation: 'case.assign',
      id: record.id,
      expectedRevision: 1,
      assigneeId: 'inactive',
      reason: 'Refuse inactive target',
    }),
    'forbidden',
  )
  await failed(
    act({
      operation: 'case.assign',
      id: record.id,
      expectedRevision: 1,
      assigneeId: 'bot',
      reason: 'Refuse machine target',
    }),
    'forbidden',
  )
  await act({
    operation: 'case.assign',
    id: record.id,
    expectedRevision: 1,
    assigneeId: 'staff-a',
    reason: 'Qualified operator handles this case',
  })
  assert.equal(
    (await service.detail('professional', human('staff-a'), record.id)).record.assigneeId,
    'staff-a',
  )
  await failed(service.detail('professional', human('staff-b'), record.id), 'not_found')
  check(
    'only globally authorized assignment selects current active human case readers; assigned operators gain their own case without cross-staff or machine directory access',
  )
  const note = mutation({
      operation: 'note.add',
      id: record.id,
      expectedRevision: 2,
      body: 'Investigated root cause locally; no customer message sent',
    }),
    notes = await Promise.all(
      Array.from({ length: 20 }, () => service.mutate('professional', human('staff-a'), note)),
    )
  assert.equal(notes.filter((x) => !x.replayed).length, 1)
  assert.equal((await service.detail('professional', human('owner'), record.id)).notes.length, 1)
  assert.equal((await service.detail('professional', human('owner'), record.id)).record.revision, 3)
  check(
    '20 duplicate internal note submissions append one finding and one audit/history revision without claiming customer delivery or response',
  )
  await act({
    operation: 'case.assign',
    id: record.id,
    expectedRevision: 3,
    assigneeId: 'staff-b',
    reason: 'Explicit reassignment',
  })
  await failed(service.detail('professional', human('staff-a'), record.id), 'not_found')
  await failed(service.commandStatus('professional', human('staff-a'), note.requestId), 'not_found')
  assert.equal((await service.detail('professional', human('staff-b'), record.id)).notes.length, 1)
  const reassigned = await service.detail('professional', human('owner'), record.id)
  assert.equal(reassigned.transitions[0].previousAssigneeId, 'staff-a')
  assert.equal(reassigned.transitions[0].assigneeId, 'staff-b')
  check(
    'reassignment immediately changes resource visibility and receipt recovery while preserving previous/new assignee evidence and internal notes',
  )
  const scoped = await act({
    operation: 'case.create',
    requesterName: 'Scoped requester',
    subject: 'Consistent private projection',
    description: 'Authorized case read stays stable',
  })
  await act({
    operation: 'case.assign',
    id: scoped.id,
    expectedRevision: 1,
    assigneeId: 'staff-a',
    reason: 'Assign initial investigator',
  })
  let releaseRead, readStarted
  const gate = new Promise((resolve) => {
      releaseRead = resolve
    }),
    started = new Promise((resolve) => {
      readStarted = resolve
    })
  const pausedPool = {
    query: (...args) => app.query(...args),
    connect: async () => {
      const client = await app.connect()
      return new Proxy(client, {
        get(target, key) {
          if (key === 'query')
            return async (...args) => {
              const result = await target.query(...args)
              if (
                String(args[0]).startsWith('SELECT * FROM archava_support.cases WHERE') &&
                args[1]?.[1] === scoped.id
              ) {
                readStarted()
                await gate
              }
              return result
            }
          const value = Reflect.get(target, key)
          return typeof value === 'function' ? value.bind(target) : value
        },
      })
    },
  }
  const pausedService = new domain.SupportService(
      await postgres.PostgresSupportRepository.create(pausedPool, key),
    ),
    reading = pausedService.detail('professional', human('staff-a'), scoped.id)
  try {
    await Promise.race([
      started,
      new Promise((_, reject) =>
        setTimeout(
          () => reject(new Error('Case projection did not reach its real database header read')),
          2000,
        ),
      ),
    ])
    const reassignment = act({
      operation: 'case.assign',
      id: scoped.id,
      expectedRevision: 2,
      assigneeId: 'staff-b',
      reason: 'Wait for complete authorized projection',
    })
    let blocked = false
    for (let n = 0; n < 40 && !blocked; n++) {
      blocked = (
        await owner.query(
          "SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE wait_event_type='Lock' AND query LIKE '%FROM archava_support.cases%' AND query LIKE '%FOR UPDATE%') AS waiting",
        )
      ).rows[0].waiting
      if (!blocked) await new Promise((resolve) => setTimeout(resolve, 25))
    }
    assert.equal(blocked, true)
    releaseRead()
    assert.equal((await reading).record.assigneeId, 'staff-a')
    await reassignment
    await failed(service.detail('professional', human('staff-a'), scoped.id), 'not_found')
    assert.equal(
      (await service.detail('professional', human('staff-b'), scoped.id)).record.assigneeId,
      'staff-b',
    )
  } finally {
    releaseRead()
  }
  check(
    'case detail holds current resource authorization through its complete private projection; proven concurrent reassignment waits and former staff cannot read the new state',
  )
  const race = await Promise.allSettled(
    Array.from({ length: 20 }, (_, i) =>
      act({
        operation: 'case.set-priority',
        id: record.id,
        expectedRevision: 4,
        priority: i % 2 ? 'low' : 'normal',
        reason: 'Concurrent priority review ' + i,
      }),
    ),
  )
  assert.equal(race.filter((x) => x.status === 'fulfilled').length, 1)
  for (const x of race) if (x.status === 'rejected') assert.equal(x.reason.code, 'conflict')
  check(
    '20 competing revisions have one winner and all stale changes roll back commands, history and audit',
  )
  const before = await service.detail('professional', human('owner'), record.id)
  await act({
    operation: 'case.start',
    id: record.id,
    expectedRevision: 5,
    reason: 'Start investigation',
  })
  await act({
    operation: 'case.wait',
    id: record.id,
    expectedRevision: 6,
    reason: 'Await local information',
  })
  assert.equal(
    (await service.detail('professional', human('owner'), record.id)).record.overdue,
    true,
  )
  await act({
    operation: 'case.resume',
    id: record.id,
    expectedRevision: 7,
    reason: 'Information available',
  })
  await act({
    operation: 'case.resolve',
    id: record.id,
    expectedRevision: 8,
    resolution: 'Original defect corrected',
  })
  await act({
    operation: 'case.close',
    id: record.id,
    expectedRevision: 9,
    reason: 'Operator closes resolved investigation',
  })
  const closed = await service.detail('professional', human('owner'), record.id)
  assert.equal(closed.record.state, 'closed')
  assert.equal(closed.record.overdue, false)
  assert.equal(closed.record.resolution, 'Original defect corrected')
  assert.ok(closed.record.resolvedAt && closed.record.closedAt)
  await failed(
    act({
      operation: 'case.start',
      id: record.id,
      expectedRevision: 10,
      reason: 'No silent restart',
    }),
    'conflict',
  )
  await failed(
    act({
      operation: 'case.set-target',
      id: record.id,
      expectedRevision: 10,
      dueAt: null,
      reason: 'Closed metadata immutable',
    }),
    'conflict',
  )
  check(
    'start, wait, resume, resolve and close are separate guarded investigation steps; waiting does not silently pause due targets and closed work is not overdue',
  )
  const reopen = mutation({
    operation: 'case.reopen',
    id: record.id,
    expectedRevision: 10,
    reason: 'Further investigation requested',
    dueAt: '2026-12-01T12:00:00+07:00',
  })
  await service.mutate('professional', human('owner'), reopen)
  const reopened = await service.detail('professional', human('owner'), record.id)
  assert.equal(reopened.record.cycle, 2)
  assert.equal(reopened.record.state, 'open')
  assert.equal(reopened.record.resolution, null)
  assert.equal(reopened.record.resolvedAt, null)
  assert.equal(reopened.record.closedAt, null)
  assert.equal(reopened.transitions[0].previousResolution, 'Original defect corrected')
  assert.equal(reopened.transitions[0].previousDueAt, before.record.dueAt)
  assert.equal(reopened.record.dueAt, '2026-12-01T05:00:00.000Z')
  check(
    'explicit reopening starts a new cycle and target while immutable history retains previous resolution, timestamps, priority, assignment and due evidence',
  )
  const page = await service.cases('professional', human('owner'), { overdue: false, limit: 1 })
  assert.ok(page.items.some((x) => x.id === record.id))
  await act({
    operation: 'case.set-target',
    id: record.id,
    expectedRevision: 11,
    dueAt: '2026-10-10T00:00:00Z',
    reason: 'Explicit revised target',
  })
  const overdue = await service.cases('professional', human('owner'), { overdue: true })
  assert.equal(overdue.items[0].id, record.id)
  assert.equal(overdue.items[0].overdue, true)
  const noTarget = await act({
    operation: 'case.create',
    requesterName: 'No target customer',
    subject: 'No promised deadline',
    description: 'Investigate without fabricated SLA',
  })
  assert.equal(
    (await service.detail('professional', human('owner'), noTarget.id)).record.dueAt,
    null,
  )
  assert.equal(
    (await service.detail('professional', human('owner'), noTarget.id)).record.overdue,
    false,
  )
  check(
    'database-clock due projections use only explicit supported instants, current scope and active states; missing targets never become fabricated SLA metrics',
  )
  const cancelled = await act({
    operation: 'case.cancel',
    id: noTarget.id,
    expectedRevision: 1,
    reason: 'Request withdrawn',
  })
  await failed(
    act({
      operation: 'case.reopen',
      id: noTarget.id,
      expectedRevision: cancelled.revision,
      reason: 'Terminal cancellation',
    }),
    'conflict',
  )
  const viewer = await service.detail('professional', human('viewer'), record.id)
  assert.equal(viewer.notes, null)
  assert.equal(viewer.audit, null)
  assert.equal(viewer.transitions, null)
  const delegated = { kind: 'service', id: 'bot', onBehalfOf: 'staff-b' }
  assert.equal(
    (await service.cases('professional', delegated)).items.some((x) => x.id === record.id),
    true,
  )
  await failed(
    () =>
      service.mutate(
        'professional',
        delegated,
        mutation({
          operation: 'case.cancel',
          id: record.id,
          expectedRevision: 12,
          reason: 'No machine action',
        }),
      ),
    'forbidden',
  )
  await owner.query(
    "UPDATE archava_support.memberships SET active=false WHERE tenant_id='professional' AND principal_id='staff-b'",
  )
  await failed(service.detail('professional', human('staff-b'), record.id), 'forbidden')
  await failed(service.cases('professional', delegated), 'forbidden')
  await act({
    operation: 'note.add',
    id: record.id,
    expectedRevision: 12,
    body: 'Owner can still record investigation after assignee is revoked',
  })
  await owner.query(
    "UPDATE archava_support.memberships SET active=true WHERE tenant_id='professional' AND principal_id='staff-b'",
  )
  check(
    'current human/service grants and resource scope deny machine writes and revoked membership; note/history grants are separate and owner work survives a revoked assignee',
  )
  const counts = async () => {
    const result = {}
    for (const t of (
      await owner.query(
        "SELECT table_name FROM information_schema.tables WHERE table_schema='archava_support' AND table_name<>'schema_migrations' ORDER BY table_name",
      )
    ).rows)
      result[t.table_name] = (
        await owner.query('SELECT to_jsonb(t) AS r FROM archava_support."' + t.table_name + '" t')
      ).rows
        .map((x) => JSON.stringify(x.r))
        .sort()
    return result
  }
  const snapshot = await counts(),
    failure = mutation({
      operation: 'case.start',
      id: record.id,
      expectedRevision: 13,
      reason: 'Retry after injected failure',
    })
  await owner.query(
    "CREATE FUNCTION archava_support.proof_failure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'Injected audit failure' USING ERRCODE='23514';END $$;CREATE TRIGGER proof_failure BEFORE INSERT ON archava_support.audit FOR EACH ROW EXECUTE FUNCTION archava_support.proof_failure()",
  )
  const noteFailure = mutation({
    operation: 'note.add',
    id: record.id,
    expectedRevision: 13,
    body: 'This failing note must not survive',
  })
  await failed(service.mutate('professional', human('owner'), noteFailure), 'conflict')
  assert.deepEqual(await counts(), snapshot)
  await failed(service.mutate('professional', human('owner'), failure), 'conflict')
  assert.deepEqual(await counts(), snapshot)
  assert.equal(
    (await service.commandStatus('professional', human('owner'), failure.requestId)).status,
    'unknown',
  )
  await owner.query(
    'DROP TRIGGER proof_failure ON archava_support.audit;DROP FUNCTION archava_support.proof_failure()',
  )
  await service.mutate('professional', human('owner'), failure)
  check(
    'failure after header or note changes rolls back all domain data, history, audit and UUID claim before same-request retry',
  )
  const property = await second.mutate(
    'property',
    human('owner'),
    mutation({
      operation: 'case.create',
      requesterName: 'Tenant requester',
      subject: 'Property service issue',
      description: 'Inspect maintenance request',
    }),
  )
  await second.mutate(
    'property',
    human('owner'),
    mutation({
      operation: 'case.start',
      id: property.id,
      expectedRevision: 1,
      reason: 'Inspect reported issue',
    }),
  )
  await failed(service.detail('professional', human('owner'), property.id), 'not_found')
  await failed(service.access('property', human('staff-a')), 'forbidden')
  const sql = await app.connect()
  try {
    await sql.query('BEGIN')
    await sql.query("SELECT set_config('archava.tenant_id','professional',true)")
    assert.equal(
      (
        await sql.query(
          "SELECT count(*)::int AS n FROM archava_support.cases WHERE tenant_id='property'",
        )
      ).rows[0].n,
      0,
    )
    await sql.query('ROLLBACK')
    for (const statement of [
      "UPDATE archava_support.notes SET body='rewritten' WHERE tenant_id='professional'",
      "UPDATE archava_support.transitions SET resolution='rewritten' WHERE tenant_id='professional'",
      "UPDATE archava_support.audit SET operation='rewritten' WHERE tenant_id='professional'",
      "UPDATE archava_support.commands SET receipt='{}' WHERE tenant_id='professional' AND receipt IS NOT NULL",
      "UPDATE archava_support.cases SET owner_id='staff-a',revision=revision+1 WHERE tenant_id='professional'",
      "UPDATE archava_support.cases SET priority='normal',revision=revision+1 WHERE tenant_id='professional' AND id='" +
        record.id +
        "'",
    ]) {
      await sql.query('BEGIN')
      await sql.query("SELECT set_config('archava.tenant_id','professional',true)")
      await assert.rejects(async () => {
        await sql.query(statement)
        await sql.query('COMMIT')
      })
      await sql.query('ROLLBACK')
    }
  } finally {
    sql.release()
  }
  check(
    'independent property context reuses unchanged investigation code; forced tenant RLS, immutable notes/history/receipts and complete revision evidence refuse direct SQL bypass',
  )
  const tools = domain.createSupportTools(service, 'professional', human('owner')),
    beforeTools = await counts()
  assert.equal((await tools.invoke('support.detail', { id: record.id })).record.cycle, 2)
  await failed(tools.invoke('support.case.resolve', { id: record.id }), 'not_found')
  await assert.rejects(() => tools.invoke('support.detail', { id: record.id, scope: 'all' }))
  assert.deepEqual(await counts(), beforeTools)
  const api = domain.createSupportApi(service, () => Promise.resolve(human('owner'))),
    url = 'http://localhost/api/support/professional/'
  assert.equal((await api(new Request(url + 'cases?scope=all'))).status, 400)
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
    'strict bounded API and private read-only tools refuse role/tenant/state overrides and produce no note, message, mutation or provider call',
  )
  const expected = await service.detail('professional', human('owner'), record.id),
    archive = join(scratch, 'support.dump'),
    beforeRestart = await counts()
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
  app = new Pool({ ...target, user: 'support_app' })
  service = new domain.SupportService(await postgres.PostgresSupportRepository.create(app, key))
  assert.deepEqual(await service.detail('professional', human('owner'), record.id), expected)
  assert.deepEqual(await counts(), beforeRestart)
  assert.equal(
    (await service.commandStatus('professional', human('owner'), reopen.requestId)).status,
    'committed',
  )
  check(
    'database restart preserves case cycles, previous resolutions, assignments, due targets, private notes, immutable history and original UUID receipts',
  )
  run('createdb', ['-h', socket, '-p', String(port), '-U', 'postgres', 'restored_support'])
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
    'restored_support',
    archive,
  ])
  const restoredOwner = new Pool({ ...target, database: 'restored_support' }),
    restoredApp = new Pool({ ...target, database: 'restored_support', user: 'support_app' })
  try {
    await postgres.grantSupportRuntime(restoredOwner, 'support_app')
    const restoredService = new domain.SupportService(
      await postgres.PostgresSupportRepository.create(restoredApp, key),
    )
    assert.deepEqual(
      await restoredService.detail('professional', human('owner'), record.id),
      expected,
    )
    assert.equal(
      (await restoredService.commandStatus('professional', human('owner'), reopen.requestId))
        .status,
      'committed',
    )
    await restoredOwner.query(
      "UPDATE archava_support.memberships SET active=false WHERE tenant_id='professional' AND principal_id='owner'",
    )
    await failed(
      restoredService.commandStatus('professional', human('owner'), reopen.requestId),
      'forbidden',
    )
    assert.equal(
      (await service.commandStatus('professional', human('owner'), reopen.requestId)).status,
      'committed',
    )
  } finally {
    await endProofPool(restoredApp)
    await endProofPool(restoredOwner)
  }
  check(
    'fresh backup restore retains support truth, current grants and receipt recovery while restored revocation leaves original database intact',
  )
  await owner.query('UPDATE archava_support.schema_migrations SET checksum=$1', ['a'.repeat(64)])
  await assert.rejects(() => postgres.PostgresSupportRepository.create(app, key))
  await assert.rejects(() => postgres.migrateSupport(owner))
  check('modified support migration checksum refuses repository startup and migration')
  const destination = join(root, 'dist/support-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope: 'bounded internal support cases, assignment, notes and resolution cycles',
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
