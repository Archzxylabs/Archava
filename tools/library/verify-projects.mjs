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

const root = resolve(import.meta.dirname, '../..'),
  library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-project-proof-'))
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
  for (const name of ['agency', 'consulting']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-project-' + name,
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
      env: { ...env, NODE_PATH: '' },
    })
    const require = createRequire(join(directory, 'package.json'))
    const load = async (name) => await import(pathToFileURL(require.resolve(name)).href)
    const release = await load('@archava/config/release')
    await release.verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))
    const domain = await load('@archava/projects'),
      postgres = await load('@archava/projects-pg')
    Pool ??= require('pg').Pool
    const dashboard = require.resolve('@archava/projects/dashboard')
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
        /projects-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )
    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/projects')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two cold-installed consumers share the same standalone project module and browser-safe dashboard',
  )
  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)
  const { domain, postgres } = consumers[0]
  await postgres.migrateProjects(owner)
  await postgres.migrateProjects(owner)
  await owner.query('CREATE ROLE project_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantProjectsRuntime(owner, 'project_app')
  const permissions = domain.PROJECT_PERMISSIONS
  for (const [tenant, id, kind, grants, scope] of [
    ['agency', 'owner', 'human', permissions, 'all'],
    ['agency', 'viewer', 'human', permissions.filter((x) => x.endsWith('.read')), 'all'],
    ['agency', 'staff', 'human', permissions, 'assigned'],
    ['agency', 'bot', 'service', ['projects.read', 'tasks.read'], 'all'],
    ['consulting', 'owner', 'human', permissions, 'all'],
  ])
    await owner.query(
      'INSERT INTO archava_projects.memberships(tenant_id,principal_id,kind,permissions,scope) VALUES($1,$2,$3,$4,$5)',
      [tenant, id, kind, grants, scope],
    )
  app = new Pool({ ...target, user: 'project_app' })
  let repository = await postgres.PostgresProjectRepository.create(app, key)
  let service = new domain.ProjectService(repository)
  const second = new consumers[1].domain.ProjectService(
    await consumers[1].postgres.PostgresProjectRepository.create(app, key),
  )
  await assert.rejects(() => postgres.PostgresProjectRepository.create(owner, key))
  await assert.rejects(() => postgres.PostgresProjectRepository.create(app, 'short'))
  const schemas = (
    await owner.query(
      "SELECT schema_name FROM information_schema.schemata WHERE schema_name LIKE 'archava_%'",
    )
  ).rows
  assert.deepEqual(
    schemas.map((x) => x.schema_name),
    ['archava_projects'],
  )
  check(
    'project domain operates without CRM or LLM and refuses owner runtime and weak fingerprint keys',
  )

  const draft = mutation({
    operation: 'project.create',
    title: 'Agency website',
    scopeSummary: 'Five agreed pages',
  })
  const race = await Promise.all(
    Array.from({ length: 20 }, () => service.mutate('agency', human('owner'), draft)),
  )
  assert.equal(new Set(race.map((x) => x.id)).size, 1)
  let projectId = race[0].id
  const totals = (
    await owner.query(
      'SELECT (SELECT count(*) FROM archava_projects.projects) AS projects,(SELECT count(*) FROM archava_projects.audit) AS audit,(SELECT count(*) FROM archava_projects.commands) AS commands',
    )
  ).rows[0]
  assert.deepEqual(totals, { projects: '1', audit: '1', commands: '1' })
  check('20 concurrent submissions commit exactly one project, audit entry and immutable receipt')
  await failed(
    service.mutate('agency', human('owner'), {
      ...draft,
      command: { ...draft.command, title: 'Other scope' },
    }),
    'conflict',
  )
  await failed(service.mutate('agency', human('staff'), draft), 'conflict')
  assert.deepEqual(
    (await service.commandStatus('agency', human('owner'), draft.requestId)).receipt,
    race[0],
  )
  await failed(service.commandStatus('agency', human('viewer'), draft.requestId), 'forbidden')
  check(
    'idempotency binds tenant, actor and exact payload; receipt recovery is read-only and actor-bound',
  )

  await failed(second.detail('consulting', human('owner'), projectId), 'not_found')
  const consulting = await second.mutate(
    'consulting',
    human('owner'),
    mutation({
      operation: 'project.create',
      title: 'Consulting delivery',
      scopeSummary: 'Review and recommendations',
    }),
  )
  assert.equal((await second.list('consulting', human('owner'), {})).items.length, 1)
  assert.equal((await service.list('agency', human('owner'), {})).items.length, 1)
  await failed(service.detail('agency', human('staff'), projectId), 'not_found')
  const assigned = await service.mutate(
    'agency',
    human('staff'),
    mutation({
      operation: 'project.create',
      title: 'Assigned work',
      scopeSummary: 'Only staff-owned project',
    }),
  )
  assert.deepEqual(
    (await service.list('agency', human('staff'), {})).items.map((x) => x.id),
    [assigned.id],
  )
  check('tenant separation and assigned ownership hold across both consumer contexts')

  await failed(
    service.mutate(
      'agency',
      human('viewer'),
      mutation({ operation: 'project.create', title: 'Forbidden', scopeSummary: 'Read only' }),
    ),
    'forbidden',
  )
  assert.throws(
    () => service.mutate('agency', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }, draft),
    (error) => error.code === 'forbidden',
  )
  const bot = await service.detail(
    'agency',
    { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
    projectId,
  )
  assert.equal(bot.milestones, null)
  assert.equal(bot.audit, null)
  await owner.query(
    "UPDATE archava_projects.memberships SET active=false WHERE tenant_id='agency' AND principal_id='owner'",
  )
  await failed(
    service.detail('agency', { kind: 'service', id: 'bot', onBehalfOf: 'owner' }, projectId),
    'forbidden',
  )
  await failed(service.commandStatus('agency', human('owner'), draft.requestId), 'forbidden')
  await owner.query(
    "UPDATE archava_projects.memberships SET active=true WHERE tenant_id='agency' AND principal_id='owner'",
  )
  check(
    'viewer cannot write; machine reads intersect current human grants; revocation blocks stale receipts',
  )

  const state = async () => await service.detail('agency', human('owner'), projectId)
  const act = async (command) =>
    await service.mutate(
      'agency',
      human('owner'),
      mutation({ ...command, projectId, projectRevision: (await state()).project.revision }),
    )
  await failed(act({ operation: 'project.transition', state: 'delivered' }), 'conflict')
  await act({ operation: 'project.transition', state: 'active' })
  const confirmed = (await state()).project
  assert.equal(confirmed.scopeSummary, 'Five agreed pages')
  assert.equal(confirmed.scopeConfirmedBy, 'owner')
  assert.ok(confirmed.scopeConfirmedAt)
  const milestoneReceipt = await act({ operation: 'milestone.create', title: 'Launch acceptance' })
  await failed(
    act({
      operation: 'milestone.complete',
      id: milestoneReceipt.id,
      expectedRevision: 1,
      evidence: 'No work yet',
    }),
    'conflict',
  )
  const taskReceipt = await act({
    operation: 'task.create',
    title: 'Publish five agreed pages',
    milestoneId: milestoneReceipt.id,
  })
  await act({ operation: 'project.transition', state: 'review' })
  await failed(act({ operation: 'project.transition', state: 'delivered' }), 'conflict')
  await failed(
    act({
      operation: 'milestone.complete',
      id: milestoneReceipt.id,
      expectedRevision: 1,
      evidence: 'Still incomplete',
    }),
    'conflict',
  )
  check(
    'scope confirmation and valid state transitions cannot bypass unfinished required work or empty milestones',
  )

  const stale = (await state()).project.revision
  const childRace = await Promise.allSettled(
    ['One', 'Two'].map((title) =>
      service.mutate(
        'agency',
        human('owner'),
        mutation({
          operation: 'task.create',
          projectId,
          projectRevision: stale,
          title,
          required: false,
        }),
      ),
    ),
  )
  assert.equal(childRace.filter((x) => x.status === 'fulfilled').length, 1)
  assert.equal(
    childRace.filter((x) => x.status === 'rejected' && x.reason.code === 'conflict').length,
    1,
  )
  assert.equal((await state()).project.revision, stale + 1)
  check(
    'competing child mutations serialize on the parent revision and do not silently overwrite work',
  )

  const forged = mutation({
    operation: 'task.create',
    projectId,
    projectRevision: (await state()).project.revision,
    title: 'Cross-project task',
    milestoneId: randomUUID(),
  })
  await failed(service.mutate('agency', human('owner'), forged), 'not_found')
  assert.equal(
    (await service.commandStatus('agency', human('owner'), forged.requestId)).status,
    'not_committed',
  )
  const childForeign = mutation({
    operation: 'task.transition',
    projectId: consulting.id,
    projectRevision: 1,
    id: taskReceipt.id,
    expectedRevision: 1,
    state: 'done',
  })
  await failed(second.mutate('consulting', human('owner'), childForeign), 'not_found')
  check('cross-project child references fail atomically without orphan commands or audits')

  await act({
    operation: 'task.transition',
    id: taskReceipt.id,
    expectedRevision: 1,
    state: 'cancelled',
    reason: 'Fixture cancelled',
  })
  await failed(act({ operation: 'project.transition', state: 'delivered' }), 'conflict')
  await act({
    operation: 'task.transition',
    id: taskReceipt.id,
    expectedRevision: 2,
    state: 'todo',
  })
  await act({
    operation: 'task.transition',
    id: taskReceipt.id,
    expectedRevision: 3,
    state: 'done',
  })
  await failed(act({ operation: 'project.transition', state: 'delivered' }), 'conflict')
  await act({
    operation: 'milestone.complete',
    id: milestoneReceipt.id,
    expectedRevision: 1,
    evidence: 'All agreed pages reviewed',
  })
  await failed(
    act({ operation: 'task.transition', id: taskReceipt.id, expectedRevision: 4, state: 'doing' }),
    'conflict',
  )
  await failed(
    act({
      operation: 'task.create',
      title: 'Late milestone work',
      milestoneId: milestoneReceipt.id,
    }),
    'conflict',
  )
  await act({ operation: 'project.transition', state: 'delivered' })
  await failed(act({ operation: 'task.create', title: 'Silently added feature' }), 'conflict')
  await act({ operation: 'project.transition', state: 'closed' })
  await failed(act({ operation: 'project.transition', state: 'active' }), 'conflict')
  const finished = await state()
  assert.equal(finished.project.scopeConfirmedAt, confirmed.scopeConfirmedAt)
  assert.ok(finished.audit.some((x) => x.reason === 'Fixture cancelled'))
  check(
    'cancelled required tasks still block delivery; reached milestones and closed projects refuse extra work',
  )

  const client = await app.connect()
  try {
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','agency',true)")
    assert.equal(
      (await client.query("SELECT * FROM archava_projects.projects WHERE tenant_id='consulting'"))
        .rowCount,
      0,
    )
    await assert.rejects(() =>
      client.query("UPDATE archava_projects.memberships SET active=false WHERE tenant_id='agency'"),
    )
    await client.query('ROLLBACK')
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','agency',true)")
    await assert.rejects(() =>
      client.query('UPDATE archava_projects.projects SET scope_summary=$2 WHERE id=$1', [
        projectId,
        'Unapproved scope rewrite',
      ]),
    )
    await client.query('ROLLBACK')
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','agency',true)")
    await assert.rejects(() =>
      client.query('UPDATE archava_projects.commands SET receipt=NULL WHERE request_id=$1', [
        draft.requestId,
      ]),
    )
    await client.query('ROLLBACK')
    await client.query('BEGIN')
    await client.query("SELECT set_config('archava.tenant_id','agency',true)")
    await assert.rejects(() => client.query('DELETE FROM archava_projects.audit'))
    await client.query('ROLLBACK')
    assert.equal(
      (await client.query('SELECT count(*)::integer AS count FROM archava_projects.projects'))
        .rows[0].count,
      0,
    )
  } finally {
    client.release()
  }
  check(
    'forced RLS, denied membership writes, immutable receipts and append-only audit protect the SQL boundary',
  )
  const tools = domain.createProjectTools(service, 'agency', human('owner'))
  assert.equal((await tools.invoke('projects.detail', { id: projectId })).project.state, 'closed')
  await assert.rejects(() => tools.invoke('projects.list', { tenant: 'consulting' }))
  await failed(
    tools.invoke('projects.create', { title: 'Unregistered machine write' }),
    'not_found',
  )
  check('assistant tools bind trusted tenant and identity and expose read operations only')

  const api = domain.createProjectApi(service, async () => human('owner'))
  const response = await api(
    new Request('http://local.test/api/projects/agency/records/' + projectId),
  )
  assert.equal(response.status, 200)
  assert.equal((await response.json()).project.state, 'closed')
  const unknown = await api(
    new Request('http://local.test/api/projects/agency/records?roles=owner'),
  )
  assert.equal(unknown.status, 400)
  check('installed API returns the authoritative project state and rejects unknown query authority')

  const before = await state(),
    archive = join(scratch, 'projects.dump')
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
  app = new Pool({ ...target, user: 'project_app' })
  repository = await postgres.PostgresProjectRepository.create(app, key)
  service = new domain.ProjectService(repository)
  assert.deepEqual(await state(), before)
  assert.deepEqual(
    (await service.commandStatus('agency', human('owner'), draft.requestId)).receipt,
    race[0],
  )
  check('database restart preserves projects, immutable scope confirmation, audit and receipts')
  await owner.query('CREATE DATABASE restored_projects')
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
    'restored_projects',
    archive,
  ])
  const restoredOwner = new Pool({ ...target, database: 'restored_projects' }),
    restoredApp = new Pool({ ...target, database: 'restored_projects', user: 'project_app' })
  try {
    await postgres.migrateProjects(restoredOwner)
    await postgres.grantProjectsRuntime(restoredOwner, 'project_app')
    const recovered = new domain.ProjectService(
      await postgres.PostgresProjectRepository.create(restoredApp, key),
    )
    assert.deepEqual(await recovered.detail('agency', human('owner'), projectId), before)
    assert.deepEqual(
      (await recovered.commandStatus('agency', human('owner'), draft.requestId)).receipt,
      race[0],
    )
    await restoredOwner.query(
      "UPDATE archava_projects.memberships SET active=false WHERE tenant_id='agency' AND principal_id='viewer'",
    )
    await failed(recovered.detail('agency', human('viewer'), projectId), 'forbidden')
    assert.equal(
      (await service.detail('agency', human('viewer'), projectId)).project.state,
      'closed',
    )
  } finally {
    await endProofPool(restoredApp)
    await endProofPool(restoredOwner)
  }
  check(
    'backup restore into a new database retains domain grants and receipts without changing the original',
  )
  const destination = join(root, 'dist/project-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        scope: 'internal project/task/milestone tracking',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
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
