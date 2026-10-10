/* global process, fetch, setTimeout, clearTimeout */
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, writeFile, rm, stat } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { randomUUID, createHash } from 'node:crypto'
const root = resolve(import.meta.dirname, '../../..'),
  priorCommit = 'f5cb768'
const scratch = await mkdtemp(join(tmpdir(), 'archava-upgrade-proof-')),
  home = join(scratch, 'state')
await mkdir(home, { mode: 0o700 })
let prior = process.argv.includes('--cold-prior')
    ? join(scratch, 'cold-prior-runtime')
    : join(root, 'dist/releases/1.0.0/runtime'),
  live,
  settings
const env = {
  PATH: process.env.PATH,
  LANG: 'C.UTF-8',
  ARCHAVA_CRM_HOME: home,
  ARCHAVA_CRM_PROFILE: 'runtime.json',
}
const checks = []
const check = (label) => {
  checks.push(label)
  process.stdout.write('PASS ' + label + '\n')
}
const guard = join(scratch, 'network-guard.mjs')
await writeFile(
  guard,
  "const original=globalThis.fetch;globalThis.fetch=(input,init)=>{const url=new URL(typeof input==='string'||input instanceof URL?input:input.url);if(url.hostname!=='127.0.0.1')throw new Error('Upgrade fixture forbids external providers');return original(input,init)};",
  { mode: 0o600 },
)
const operator = (runtime, args, profile = 'runtime.json') =>
  execFileSync(process.execPath, ['--import', guard, join(runtime, 'operator.mjs'), ...args], {
    cwd: root,
    env: { ...env, ARCHAVA_CRM_PROFILE: profile },
    stdio: 'pipe',
  })
async function boot(runtime, profile = 'runtime.json') {
  const child = spawn(process.execPath, ['--import', guard, join(runtime, 'server.mjs')], {
    cwd: root,
    env: { ...env, ARCHAVA_CRM_PROFILE: profile },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  const exited = new Promise((done) => child.once('exit', (code) => done(code)))
  live = { child, exited }
  return new Promise((done, fail) => {
    let text = ''
    const timeout = setTimeout(() => fail(new Error('Upgrade fixture startup timeout')), 15000)
    child.stdout.on('data', (chunk) => {
      text += chunk.toString()
      const match = /CRM ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(text)
      if (match) {
        clearTimeout(timeout)
        done(match[1])
      }
    })
    child.stderr.on('data', () => {})
    child.once('exit', () => {
      clearTimeout(timeout)
      fail(new Error('Upgrade fixture startup refused'))
    })
  })
}
async function stop() {
  if (!live) return
  const previous = live
  live = null
  previous.child.kill('SIGTERM')
  assert.equal(await previous.exited, 0)
}
async function login(origin, account) {
  const response = await fetch(origin + '/api/login', {
    method: 'POST',
    headers: { origin, 'content-type': 'application/json' },
    body: JSON.stringify(account),
  })
  assert.equal(response.status, 200)
  const cookie = response.headers.get('set-cookie').split(';')[0]
  const session = await fetch(origin + '/api/session', { headers: { cookie } }).then((r) =>
    r.json(),
  )
  return { cookie, csrf: session.csrf }
}
const post = async (origin, path, input, session) => {
  const response = await fetch(origin + path, {
    method: 'POST',
    headers: {
      origin,
      'content-type': 'application/json',
      cookie: session.cookie,
      'x-archava-csrf': session.csrf,
    },
    body: JSON.stringify(input),
  })
  assert.equal(response.status, 200)
  return response.json()
}
const get = (origin, path, session) => fetch(origin + path, { headers: { cookie: session.cookie } })
const current = join(root, 'dist/crm-runtime')
const { Pool } = createRequire(join(current, 'package.json'))('pg')
const poolFor = (profile) =>
  new Pool({
    host: join(home, 'socket'),
    port: profile.databasePort,
    database: profile.database,
    user: 'archava_operator',
    password: () => '',
    ssl: false,
  })
async function snapshot(pool) {
  const result = {}
  for (const table of [
    'memberships',
    'contacts',
    'activities',
    'followups',
    'commands',
    'audit',
    'opportunities',
    'opportunity_transitions',
  ]) {
    const rows = await pool.query(`SELECT * FROM archava_crm.${table}`)
    result[table] = rows.rows.map((row) => JSON.stringify(row)).sort()
  }
  return result
}
try {
  try {
    await stat(join(prior, 'server.mjs'))
  } catch (error) {
    if (error.code !== 'ENOENT') throw error
    const archive = join(scratch, 'prior-source')
    await mkdir(archive)
    const bytes = execFileSync('git', ['archive', priorCommit], {
      cwd: root,
      maxBuffer: 64 * 1024 * 1024,
    })
    execFileSync('tar', ['-xf', '-', '-C', archive], { input: bytes })
    execFileSync('pnpm', ['install', '--frozen-lockfile', '--ignore-scripts'], {
      cwd: archive,
      stdio: 'pipe',
    })
    execFileSync('pnpm', ['crm:build'], { cwd: archive, stdio: 'pipe' })
    prior = join(archive, 'dist/crm-runtime')
  }
  const old = JSON.parse(await readFile(join(prior, 'release-profile.json'), 'utf8'))
  assert.equal(old.catalog.version, '1.0.0')
  operator(prior, ['setup'])
  settings = JSON.parse(await readFile(join(home, 'runtime.json'), 'utf8'))
  settings.appPort = 0
  await writeFile(join(home, 'runtime.json'), JSON.stringify(settings) + '\n', { mode: 0o600 })
  const initial = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8'))
  const account = { username: initial.username, password: initial.password }
  let origin = await boot(prior),
    session = await login(origin, account)
  const requestId = randomUUID(),
    command = {
      operation: 'contact.create',
      data: { fullName: 'Synthetic upgrade retained contact', email: null },
    }
  const receipt = await post(
    origin,
    '/api/crm/hotel-demo/commands',
    { requestId, command },
    session,
  )
  const inputs = {
    contactId: receipt.id,
    contactRevision: 1,
    summary: 'Synthetic upgrade native follow-up',
    dueAt: '2026-11-01T10:00:00Z',
  }
  const review = await post(origin, '/api/crm/hotel-demo/actions/followup/review', inputs, session)
  await post(
    origin,
    '/api/crm/hotel-demo/actions/followup/present',
    {
      inputs,
      confirmation: { challengeId: review.challenge.challengeId, token: review.challenge.token },
    },
    session,
  )
  const nativeReceipt = await get(
    origin,
    '/api/crm/hotel-demo/commands/' + review.intentId,
    session,
  ).then((r) => r.json())
  assert.equal(nativeReceipt.status, 'committed')
  const beforePool = poolFor(settings)
  const before = await snapshot(beforePool)
  await beforePool.end()
  const oldControl = JSON.parse(
    await readFile(join(home, 'native-actions-' + settings.database + '.json'), 'utf8'),
  )
  await stop()
  check(
    'previous supported 1.0.0 installation creates real data, grants, immutable audit and manual/native UUID receipts in an isolated PostgreSQL cluster',
  )
  operator(current, ['setup'])
  origin = await boot(current)
  assert.equal(
    (await get(origin, '/api/session', session).then((r) => r.json())).authenticated,
    true,
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', { requestId, command }, session),
    receipt,
  )
  assert.deepEqual(
    await get(origin, '/api/crm/hotel-demo/commands/' + review.intentId, session).then((r) =>
      r.json(),
    ),
    nativeReceipt,
  )
  const afterPool = poolFor(settings)
  const after = await snapshot(afterPool)
  await afterPool.end()
  assert.deepEqual(after, before)
  assert.equal(JSON.parse(operator(current, ['doctor'])).release.verified, true)
  check(
    'new supported profile upgrades the prior installation without changing records, memberships, audit, command UUIDs or authoritative receipts',
  )
  await stop()
  const backup = join(home, 'upgrade-backup.dump')
  operator(current, ['backup', backup])
  operator(current, ['restore', backup, 'archava_upgrade_recovered'])
  const recovered = JSON.parse(await readFile(join(home, 'restored-runtime.json'), 'utf8'))
  const restoredControl = JSON.parse(
    await readFile(join(home, 'native-actions-' + recovered.database + '.json'), 'utf8'),
  )
  assert.equal(restoredControl.held, true)
  assert.notEqual(restoredControl.epoch, oldControl.epoch)
  origin = await boot(current, 'restored-runtime.json')
  assert.equal(
    (await get(origin, '/api/session', session).then((r) => r.json())).authenticated,
    false,
  )
  const restoredPool = poolFor(recovered)
  const restoredSnapshot = await snapshot(restoredPool)
  await restoredPool.end()
  assert.deepEqual(restoredSnapshot, before)
  const usage = JSON.parse(operator(current, ['ai-status'], 'restored-runtime.json'))
  assert.ok(usage.restoreHoldUntil)
  check(
    'fresh restore preserves upgraded data/grants/audit/receipts while invalidating identity and holding provider/native execution with a fresh approval epoch',
  )
  await stop()
  const recoveryFile = join(home, 'recover.json')
  const password = 'Synthetic recovery ' + randomUUID()
  await writeFile(recoveryFile, JSON.stringify({ username: initial.username, password }) + '\n', {
    mode: 0o600,
  })
  operator(current, ['user', 'recover', recoveryFile], 'restored-runtime.json')
  operator(current, ['resume-actions'], 'restored-runtime.json')
  origin = await boot(current, 'restored-runtime.json')
  session = await login(origin, { username: initial.username, password })
  assert.deepEqual(
    await get(origin, '/api/crm/hotel-demo/commands/' + review.intentId, session).then((r) =>
      r.json(),
    ),
    nativeReceipt,
  )
  const refused = await fetch(origin + '/api/crm/hotel-demo/actions/followup/present', {
    method: 'POST',
    headers: {
      origin,
      'content-type': 'application/json',
      cookie: session.cookie,
      'x-archava-csrf': session.csrf,
    },
    body: JSON.stringify({
      inputs,
      confirmation: { challengeId: review.challenge.challengeId, token: review.challenge.token },
    }),
  })
  assert.notEqual(refused.status, 200)
  check(
    'explicit identity recovery and native resume recover the original UUID read-only; old-epoch/consumed consent cannot execute again',
  )
  const profile = JSON.parse(await readFile(join(current, 'release-profile.json'), 'utf8'))
  const output = join(root, 'dist/upgrade-proof')
  await mkdir(output, { recursive: true })
  await writeFile(
    join(output, 'results.json'),
    JSON.stringify(
      {
        schemaVersion: '1.0.0',
        priorCommit,
        fromProfile: old.catalog.version,
        toProfile: profile.catalog.version,
        node: process.version,
        checks,
        providerCalls: 0,
        fixture:
          'isolated synthetic PostgreSQL; previous compiled installation or cold locked git archive',
        retainedSnapshotSha256: createHash('sha256').update(JSON.stringify(before)).digest('hex'),
        binaryRollbackIsSchemaRollback: false,
      },
      null,
      2,
    ) + '\n',
  )
} finally {
  await stop()
  try {
    const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
    execFileSync(join(bin, 'pg_ctl'), ['-D', join(home, 'data'), '-m', 'fast', '-w', 'stop'], {
      stdio: 'pipe',
    })
  } catch {
    /* fixture may already be stopped */
  }
  await rm(scratch, { recursive: true, force: true })
}
