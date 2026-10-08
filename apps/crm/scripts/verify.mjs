/* global process, fetch, setTimeout, clearTimeout */
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { mkdtemp, readFile, writeFile, rm, stat, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomBytes, randomUUID } from 'node:crypto'

const root = resolve(import.meta.dirname, '../../..')
let stopRequested = false
let release = () => {}
const stopped = new Promise((done) => {
  release = done
})
process.on('SIGINT', () => {
  stopRequested = true
  release()
})
process.on('SIGTERM', () => {
  stopRequested = true
  release()
})
execFileSync(process.execPath, [join(import.meta.dirname, 'build.mjs')], {
  cwd: root,
  stdio: 'pipe',
})
const runtime = join(root, 'dist/crm-runtime')
const home = await mkdtemp(join(tmpdir(), 'archava-crm-runtime-'))
const env = {
  ...process.env,
  ARCHAVA_CRM_HOME: home,
  ARCHAVA_CRM_PROFILE: 'runtime.json',
  PGHOST: 'external-forbidden',
  PGUSER: 'external-forbidden',
  PGDATABASE: 'external-forbidden',
  PGSERVICE: 'external-forbidden',
  PGSERVICEFILE: '/does-not-exist',
  PGHOSTADDR: '192.0.2.1',
  PGPASSWORD: 'unused-fixture-marker',
  PGOPTIONS: '-c row_security=off',
  PGSSLMODE: 'require',
}
const require = createRequire(join(runtime, 'package.json'))
const { Pool } = require('pg')
const identityModule = await import(pathToFileURL(require.resolve('@archava/identity-pg')).href)
const checks = []
const check = (label) => {
  checks.push(label)
  process.stdout.write('PASS ' + label + '\n')
}
const secret = () => randomBytes(24).toString('base64url')
const privateJson = async (path, value) =>
  writeFile(path, JSON.stringify(value) + '\n', { mode: 0o600 })
const operator = (args, profile = 'runtime.json') =>
  execFileSync(process.execPath, [join(runtime, 'operator.mjs'), ...args], {
    cwd: root,
    env: { ...env, ARCHAVA_CRM_PROFILE: profile },
    stdio: 'pipe',
  })
let live = null
async function boot(profile = 'runtime.json') {
  const child = spawn(process.execPath, [join(runtime, 'server.mjs')], {
    cwd: root,
    env: { ...env, ARCHAVA_CRM_PROFILE: profile },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  live = { child, origin: '' }
  const exited = new Promise((done) => child.once('exit', (code, signal) => done({ code, signal })))
  live.exited = exited
  const origin = await new Promise((done, fail) => {
    let output = ''
    const timeout = setTimeout(() => fail(new Error('CRM runtime startup timed out')), 15000)
    child.stdout.on('data', (chunk) => {
      output += chunk.toString()
      const match = output.match(/CRM ready: (http:\/\/127\.0\.0\.1:\d+)/)
      if (match) {
        clearTimeout(timeout)
        done(match[1])
      }
    })
    child.stderr.on('data', () => {})
    child.once('exit', () => {
      clearTimeout(timeout)
      fail(new Error('CRM runtime failed before ready'))
    })
  })
  live.origin = origin
  return origin
}
async function stop() {
  if (!live) return
  const previous = live
  live = null
  previous.child.kill('SIGTERM')
  const result = await previous.exited
  assert.equal(result.code, 0, 'graceful CRM shutdown failed')
}
async function login(origin, username, password) {
  const response = await fetch(origin + '/api/login', {
    method: 'POST',
    headers: { origin, 'content-type': 'application/json' },
    body: JSON.stringify({ username, password }),
  })
  assert.equal(response.status, 200, 'valid login refused')
  const raw = response.headers.get('set-cookie')
  assert.ok(raw?.includes('HttpOnly') && raw.includes('SameSite=Strict'))
  const cookie = raw.split(';')[0]
  const session = await fetch(origin + '/api/session', { headers: { cookie } }).then((response) =>
    response.json(),
  )
  assert.equal(session.authenticated, true)
  return { cookie, csrf: session.csrf, session }
}
const get = (origin, path, session) =>
  fetch(origin + path, { headers: session ? { cookie: session.cookie } : {} })
const post = (origin, path, body, session, extra = {}) =>
  fetch(origin + path, {
    method: 'POST',
    headers: {
      origin,
      'content-type': 'application/json',
      ...(session ? { cookie: session.cookie, 'x-archava-csrf': session.csrf } : {}),
      ...extra,
    },
    body: JSON.stringify(body),
  })
const create = (name) => ({
  requestId: randomUUID(),
  command: { operation: 'contact.create', data: { fullName: name, email: 'contact@example.test' } },
})
const poolFor = (settings, user = 'archava_operator') =>
  new Pool({
    host: join(home, 'socket'),
    port: settings.databasePort,
    database: settings.database,
    user,
    ssl: false,
    password: () => '',
    options: '-c row_security=on',
  })
const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
try {
  operator(['setup'])
  const configPath = join(home, 'runtime.json')
  let settings = JSON.parse(await readFile(configPath, 'utf8'))
  settings.appPort = 0
  await privateJson(configPath, settings)
  const account = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8'))
  operator(['setup'])
  const repeat = JSON.parse(await readFile(configPath, 'utf8'))
  assert.ok(
    settings.fingerprintKey === repeat.fingerprintKey &&
      settings.identityKey === repeat.identityKey,
  )
  assert.equal((await stat(configPath)).mode & 0o777, 0o600)
  assert.equal((await stat(home)).mode & 0o777, 0o700)
  check('private persistent installation, stable keys and idempotent setup')

  const staff = {
    username: 'staff@archava.local',
    password: secret(),
    role: 'staff',
    tenants: ['hotel-demo'],
  }
  const viewer = {
    username: 'viewer@archava.local',
    password: secret(),
    role: 'viewer',
    tenants: ['hotel-demo'],
  }
  for (const [name, value] of [
    ['staff', staff],
    ['viewer', viewer],
  ]) {
    const file = join(home, name + '.json')
    await privateJson(file, value)
    operator(['user', 'add', file])
  }
  const duplicate = {
    username: 'rollback@archava.local',
    password: secret(),
    role: 'staff',
    tenants: ['hotel-demo', 'hotel-demo'],
  }
  const badFile = join(home, 'bad-enrollment.json')
  await privateJson(badFile, duplicate)
  assert.throws(() => operator(['user', 'add', badFile]))
  let origin = await boot()
  const ownerPool = poolFor(settings)
  try {
    assert.equal(
      (
        await ownerPool.query(
          'SELECT count(*)::integer AS n FROM archava_identity.users WHERE username=$1',
          [duplicate.username],
        )
      ).rows[0].n,
      0,
    )
    await assert.rejects(() =>
      identityModule.PostgresIdentity.create(ownerPool, settings.identityKey),
    )
  } finally {
    await ownerPool.end()
  }
  const appPool = poolFor(settings, 'archava_crm_app')
  try {
    await assert.rejects(() => appPool.query('UPDATE archava_identity.users SET active=false'))
    await assert.rejects(() =>
      appPool.query(
        "INSERT INTO archava_identity.users(id,username,password_hash) VALUES('forged','forged@example.test','invalid')",
      ),
    )
  } finally {
    await appPool.end()
  }
  check('atomic operator enrollment and non-owner runtime cannot enroll or activate accounts')

  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts')).status, 401)
  assert.equal(
    (await get(origin, '/api/session').then((response) => response.json())).authenticated,
    false,
  )
  assert.equal(
    (
      await post(origin, '/api/login', {
        username: account.username,
        password: account.password,
        role: 'owner',
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await post(
        origin,
        '/api/login',
        { username: account.username, password: account.password },
        null,
        { origin: 'https://attacker.test' },
      )
    ).status,
    403,
  )
  let owner = await login(origin, account.username, account.password)
  assert.equal(owner.session.tenants.length, 2)
  assert.equal((await get(origin, '/healthz')).status, 200)
  const html = await get(origin, '/').then((response) => response.text())
  assert.ok(!html.includes('reference-session') && !html.includes('Use reference'))
  const publicBundle = await get(origin, '/app.js').then((response) => response.text())
  assert.ok(
    !publicBundle.includes(settings.identityKey) &&
      !publicBundle.includes(settings.fingerprintKey) &&
      !publicBundle.includes('password_hash'),
  )
  check('real credentials, cookie-only session, origin checks and browser/server isolation')
  check('private connections ignore ambient database credentials, service files and destinations')

  const first = create('Owner contact')
  const receiptResponse = await post(origin, '/api/crm/hotel-demo/commands', first, owner)
  assert.equal(receiptResponse.status, 200)
  const receipt = await receiptResponse.json()
  const studioCommand = create('Studio client')
  assert.equal(
    (await post(origin, '/api/crm/studio-demo/commands', studioCommand, owner)).status,
    200,
  )
  assert.equal(
    (
      await post(origin, '/api/crm/hotel-demo/commands', create('No CSRF'), owner, {
        'x-archava-csrf': '',
      })
    ).status,
    403,
  )
  assert.equal(
    (
      await post(origin, '/api/crm/hotel-demo/commands', create('Wrong CSRF'), owner, {
        'x-archava-csrf': 'a'.repeat(64),
      })
    ).status,
    403,
  )
  const staffSession = await login(origin, staff.username, staff.password)
  assert.equal(staffSession.session.tenants.length, 1)
  assert.equal(
    (
      await get(origin, '/api/crm/hotel-demo/contacts', staffSession).then((response) =>
        response.json(),
      )
    ).items.length,
    0,
  )
  assert.equal((await get(origin, '/api/crm/studio-demo/contacts', staffSession)).status, 403)
  assert.equal(
    (await post(origin, '/api/crm/hotel-demo/commands', create('Staff contact'), staffSession))
      .status,
    200,
  )
  const view = await login(origin, viewer.username, viewer.password)
  assert.equal(
    (await post(origin, '/api/crm/hotel-demo/commands', create('Viewer forbidden'), view)).status,
    403,
  )
  check('owner/staff/viewer permissions, session-bound CSRF and tenant/resource isolation')

  const note = {
    requestId: randomUUID(),
    command: {
      operation: 'activity.add',
      contactId: receipt.id,
      contactRevision: 1,
      kind: 'note',
      summary: 'Persistent inquiry note',
    },
  }
  assert.equal((await post(origin, '/api/crm/hotel-demo/commands', note, owner)).status, 200)
  const projection = await post(
    origin,
    '/api/crm/hotel-demo/tools',
    { name: 'crm.contact.summary', input: { id: receipt.id } },
    owner,
  ).then((response) => response.json())
  assert.equal(projection.fullName, 'Owner contact')
  assert.ok(!('email' in projection))
  const conversationPath = '/api/crm/hotel-demo/conversation'
  const countPool = poolFor(settings)
  const snapshot = () =>
    countPool.query(`SELECT
    (SELECT count(*) FROM archava_crm.contacts) AS contacts,
    (SELECT count(*) FROM archava_crm.activities) AS activities,
    (SELECT count(*) FROM archava_crm.followups) AS followups,
    (SELECT count(*) FROM archava_crm.commands) AS commands,
    (SELECT count(*) FROM archava_crm.audit) AS audit`)
  try {
    const before = (await snapshot()).rows[0]
    const conversation = await post(
      origin,
      conversationPath,
      { utterance: 'ringkas Owner contact' },
      owner,
    ).then((response) => response.json())
    assert.equal(conversation.source, 'crm')
    assert.equal(conversation.data.contact.id, receipt.id)
    assert.equal(conversation.data.contact.activities[0].summary, 'Persistent inquiry note')
    assert.ok(!JSON.stringify(conversation).includes('contact@example.test'))
    const studioConversation = await post(
      origin,
      '/api/crm/studio-demo/conversation',
      { utterance: 'tampilkan kontak' },
      owner,
    ).then((response) => response.json())
    assert.equal(studioConversation.data.items[0].fullName, 'Studio client')
    assert.ok(!JSON.stringify(studioConversation).includes('Owner contact'))
    const prepared = await post(
      origin,
      conversationPath,
      {
        utterance: 'siapkan follow-up Owner contact',
        draft: { summary: 'Please confirm the meeting time', dueAt: '2026-10-15T02:00:00.000Z' },
      },
      owner,
    ).then((response) => response.json())
    assert.equal(prepared.data.kind, 'draft')
    assert.equal(prepared.data.draft.status, 'draft')
    assert.equal(prepared.data.draft.requiresApproval, true)
    assert.equal(prepared.data.draft.contactId, receipt.id)
    const send = await post(
      origin,
      conversationPath,
      { utterance: 'kirim semua follow-up sekarang' },
      owner,
    ).then((response) => response.json())
    assert.equal(send.data.kind, 'help')
    assert.equal(send.source, 'none')
    assert.deepEqual((await snapshot()).rows[0], before)
    check(
      'conversation reads real CRM in two businesses and prepares drafts without commands, audit writes or sending',
    )

    assert.equal(
      (await post(origin, conversationPath, { utterance: 'tampilkan kontak' })).status,
      401,
    )
    assert.equal(
      (
        await post(origin, conversationPath, { utterance: 'tampilkan kontak' }, owner, {
          'x-archava-csrf': '',
        })
      ).status,
      403,
    )
    assert.equal(
      (
        await post(
          origin,
          conversationPath,
          { utterance: 'tampilkan kontak', role: 'owner' },
          owner,
        )
      ).status,
      400,
    )
    assert.equal(
      (
        await post(
          origin,
          conversationPath,
          { utterance: 'tampilkan kontak', tenantId: 'studio-demo' },
          owner,
        )
      ).status,
      400,
    )
    assert.equal(
      (
        await post(
          origin,
          '/api/crm/studio-demo/conversation',
          { utterance: 'tampilkan kontak' },
          staffSession,
        )
      ).status,
      403,
    )
    assert.equal(
      (
        await post(
          origin,
          conversationPath,
          { utterance: 'ringkas', contactId: receipt.id },
          staffSession,
        )
      ).status,
      404,
    )
    assert.equal(
      (await post(origin, conversationPath, { utterance: 'siapkan follow-up Owner contact' }, view))
        .status,
      403,
    )
    const assigned = await post(
      origin,
      conversationPath,
      { utterance: 'tampilkan kontak' },
      staffSession,
    ).then((response) => response.json())
    assert.deepEqual(
      assigned.data.items.map((item) => item.fullName),
      ['Staff contact'],
    )
    assert.deepEqual((await snapshot()).rows[0], before)
    check(
      'conversation rejects anonymous, forged roles/tenants, missing CSRF, viewer preparation and cross-resource staff reads',
    )
  } finally {
    await countPool.end()
  }
  const actor = owner.session.username
  const p = poolFor(settings)
  try {
    const hashes = await p.query('SELECT token_hash FROM archava_identity.sessions')
    assert.ok(
      hashes.rows.every(
        (row) => row.token_hash.length === 64 && !owner.cookie.includes(row.token_hash),
      ),
    )
    await p.query(
      "UPDATE archava_identity.sessions SET last_seen_at=now()-interval '31 minutes' WHERE user_id=(SELECT id FROM archava_identity.users WHERE username=$1)",
      [viewer.username],
    )
    assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', view)).status, 401)
    const newView = await login(origin, viewer.username, viewer.password)
    await p.query(
      "UPDATE archava_identity.sessions SET expires_at=now()-interval '1 second' WHERE user_id=(SELECT id FROM archava_identity.users WHERE username=$1)",
      [viewer.username],
    )
    assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', newView)).status, 401)
    await p.query(
      'UPDATE archava_crm.memberships SET active=false WHERE principal_id=(SELECT id FROM archava_identity.users WHERE username=$1)',
      [viewer.username],
    )
    const revoked = await login(origin, viewer.username, viewer.password)
    assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', revoked)).status, 403)
    assert.equal(
      (await post(origin, conversationPath, { utterance: 'tampilkan kontak' }, revoked)).status,
      403,
    )
    await p.query(
      'UPDATE archava_crm.memberships SET active=true WHERE principal_id=(SELECT id FROM archava_identity.users WHERE username=$1)',
      [viewer.username],
    )
  } finally {
    await p.end()
  }
  check(
    'authorized tool projection, hashed sessions, idle/absolute expiry and membership revocation',
  )

  const logoutSession = await login(origin, account.username, account.password)
  assert.equal((await post(origin, '/api/logout', {}, logoutSession)).status, 200)
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', logoutSession)).status, 401)
  await stop()
  assert.equal((await stat(join(home, 'data'))).isDirectory(), true)
  assert.throws(() =>
    execFileSync(join(bin, 'pg_ctl'), ['-D', join(home, 'data'), 'status'], { stdio: 'pipe' }),
  )
  origin = await boot()
  assert.equal(
    (await get(origin, '/api/session', owner).then((response) => response.json())).authenticated,
    true,
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', first, owner).then((response) =>
      response.json(),
    ),
    receipt,
  )
  assert.equal(
    (await get(origin, '/api/crm/hotel-demo/contacts', owner).then((response) => response.json()))
      .items.length,
    2,
  )
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', logoutSession)).status, 401)
  check('application/database restart retains records, sessions, revoked logout and retry receipt')

  for (let i = 0; i < 5; i++)
    assert.equal(
      (await post(origin, '/api/login', { username: 'absent@archava.local', password: secret() }))
        .status,
      401,
    )
  assert.equal(
    (await post(origin, '/api/login', { username: 'absent@archava.local', password: secret() }))
      .status,
    429,
  )
  const changed = secret()
  const otherOwner = await login(origin, account.username, account.password)
  assert.equal(
    (await post(origin, '/api/password', { current: account.password, next: changed }, owner))
      .status,
    200,
  )
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', otherOwner)).status, 401)
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', owner)).status, 401)
  assert.equal(
    (await post(origin, '/api/login', { username: account.username, password: account.password }))
      .status,
    401,
  )
  owner = await login(origin, account.username, changed)
  check('durable login budget and password change invalidates every old session')

  const backup = join(home, 'backup.dump')
  operator(['backup', backup])
  assert.equal((await stat(backup)).mode & 0o777, 0o600)
  assert.equal((await stat(backup + '.runtime.json')).mode & 0o777, 0o600)
  assert.equal(
    (await post(origin, '/api/crm/hotel-demo/commands', create('After snapshot'), owner)).status,
    200,
  )
  const newest = secret()
  const resetFile = join(home, 'reset.json')
  await privateJson(resetFile, { username: account.username, password: newest })
  operator(['user', 'reset', resetFile])
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', owner)).status, 401)
  const primaryOwner = await login(origin, account.username, newest)
  const primaryCount = (
    await get(origin, '/api/crm/hotel-demo/contacts', primaryOwner).then((response) =>
      response.json(),
    )
  ).items.length
  assert.equal(primaryCount, 3)
  operator(['restore', backup, 'archava_recovered'])
  assert.equal(
    (
      await get(origin, '/api/crm/hotel-demo/contacts', primaryOwner).then((response) =>
        response.json(),
      )
    ).items.length,
    primaryCount,
  )
  check(
    'private pg_dump archive plus runtime metadata; restore leaves the original database untouched',
  )

  await stop()
  origin = await boot('restored-runtime.json')
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', owner)).status, 401)
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', primaryOwner)).status, 401)
  assert.equal(
    (await post(origin, '/api/login', { username: account.username, password: changed })).status,
    401,
  )
  const recoveredPassword = secret()
  await privateJson(resetFile, { username: account.username, password: recoveredPassword })
  operator(['user', 'recover', resetFile], 'restored-runtime.json')
  owner = await login(origin, account.username, recoveredPassword)
  assert.equal(
    (await get(origin, '/api/crm/hotel-demo/contacts', owner).then((response) => response.json()))
      .items.length,
    2,
  )
  const detail = await get(origin, '/api/crm/hotel-demo/contacts/' + receipt.id, owner).then(
    (response) => response.json(),
  )
  assert.equal(detail.activities[0]?.summary, 'Persistent inquiry note')
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', first, owner).then((response) =>
      response.json(),
    ),
    receipt,
  )
  const restored = JSON.parse(await readFile(join(home, 'restored-runtime.json'), 'utf8'))
  assert.ok(
    restored.fingerprintKey === settings.fingerprintKey &&
      restored.identityKey !== settings.identityKey,
  )
  assert.equal(
    (await post(origin, '/api/login', { username: staff.username, password: staff.password }))
      .status,
    401,
  )
  check(
    'restored records/audit/retry identity survive; old credentials and sessions require explicit recovery',
  )

  const restoredPool = poolFor(restored)
  try {
    const audit = await restoredPool.query(
      'SELECT count(*)::integer AS n FROM archava_crm.audit WHERE request_id=$1',
      [first.requestId],
    )
    assert.equal(audit.rows[0].n, 1)
    await restoredPool.query('UPDATE archava_identity.login_buckets SET attempts=60 WHERE key=$1', [
      identityModuleKey(restored.identityKey, 'source', '127.0.0.1'),
    ])
    const response = await post(
      origin,
      '/api/login',
      { username: 'another@archava.local', password: secret() },
      null,
      { 'x-forwarded-for': '203.0.113.99' },
    )
    assert.equal(response.status, 429)
    await restoredPool.query('DELETE FROM archava_identity.login_buckets')
  } finally {
    await restoredPool.end()
  }
  check(
    'restore preserves one mutation audit and spoofed forwarding headers cannot bypass login budget',
  )

  // Keep only the identities deliberately recovered by the operator.
  const recoveredStaff = secret()
  const staffRecovery = join(home, 'staff-recovery.json')
  await privateJson(staffRecovery, { username: staff.username, password: recoveredStaff })
  operator(['user', 'recover', staffRecovery], 'restored-runtime.json')
  const oldState = join(home, 'data')
  execFileSync(join(bin, 'pg_ctl'), ['-D', oldState, '-m', 'fast', '-w', 'stop'], { stdio: 'pipe' })
  const unhealthy = await get(origin, '/healthz')
  assert.equal(unhealthy.status, 503)
  assert.ok(!(await unhealthy.text()).includes('postgres'))
  execFileSync(
    join(bin, 'pg_ctl'),
    [
      '-D',
      oldState,
      '-l',
      join(home, 'postgres.log'),
      '-w',
      'start',
      '-o',
      `-k '${join(home, 'socket')}' -p ${settings.databasePort} -c listen_addresses=''`,
    ],
    { stdio: 'pipe' },
  )
  assert.equal((await get(origin, '/healthz')).status, 200)
  check(
    'database failures are observable through sanitized health responses and recover without process crash',
  )

  await mkdir(join(root, 'dist/crm-runtime-proof'), { recursive: true })
  await writeFile(
    join(root, 'dist/crm-runtime-proof/results.json'),
    JSON.stringify(
      {
        node: process.version,
        checks,
        checkpoint: '4f24164',
        database: 'real private persistent PostgreSQL; pg_dump/pg_restore exercised',
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(`${checks.length}/${checks.length} persistent CRM runtime checks passed.\n`)
  if (process.argv.includes('--browser') && !stopRequested) {
    const credentials = join(home, 'browser-credentials.json')
    await privateJson(credentials, {
      owner: { username: actor, password: recoveredPassword },
      staff: { username: staff.username, password: recoveredStaff },
    })
    await privateJson('/tmp/archava-crm-runtime-browser-target.json', { origin, credentials })
    process.stdout.write('Persistent CRM browser target ready; Ctrl-C stops the fixture.\n')
    await stopped
  }
} finally {
  await stop()
  try {
    execFileSync(join(bin, 'pg_ctl'), ['-D', join(home, 'data'), '-m', 'fast', '-w', 'stop'], {
      stdio: 'pipe',
    })
  } catch {
    /* already stopped */
  }
  await rm(home, { recursive: true, force: true })
}

function identityModuleKey(key, label, value) {
  return require('node:crypto')
    .createHmac('sha256', key)
    .update(label + '\0' + value)
    .digest('hex')
}
