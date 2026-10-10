/* global process, fetch, setTimeout, clearTimeout */
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import {
  mkdtemp,
  readFile,
  writeFile,
  rm,
  stat,
  mkdir,
  readdir,
  appendFile,
  chmod,
} from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomBytes, randomUUID } from 'node:crypto'
import { runCapacityBaseline } from './benchmark.mjs'
import { verifyAuthenticatedBrowser } from './browser-proof.mjs'

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
const providerModule = await import(
  pathToFileURL(require.resolve('@archava/provider-runtime')).href
)
const providerPg = await import(
  pathToFileURL(require.resolve('@archava/provider-runtime/postgres')).href
)
const providerPolicy = {
  bucketId: 'crm-gemini-pilot',
  providerId: 'google-gemini-crm',
  model: 'gemini-3.5-flash-lite',
  dailyLimit: 30,
  minimumIntervalMs: 15000,
  leaseMs: 60000,
}
const outboundMarker = join(home, 'outbound-attempts.log')
const networkGuard = join(home, 'block-external.mjs')
await writeFile(
  networkGuard,
  `import {appendFile} from 'node:fs/promises';
  const original=globalThis.fetch;
  globalThis.fetch=async(input,init)=>{const url=new URL(typeof input==='string' || input instanceof URL ? input : input.url);
  if(url.hostname!=='127.0.0.1'){await appendFile(${JSON.stringify(outboundMarker)},'blocked\\n');throw new Error('Fixture forbids external provider calls')}
  return original(input,init)};`,
  { mode: 0o600 },
)
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
  const child = spawn(process.execPath, ['--import', networkGuard, join(runtime, 'server.mjs')], {
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
  const implementation = require.resolve('@archava/crm')
  const originalImplementation = await readFile(implementation)
  try {
    await appendFile(implementation, '\n// incompatible local installation\n')
    assert.throws(
      () =>
        execFileSync(process.execPath, [join(runtime, 'server.mjs')], {
          cwd: root,
          env,
          stdio: 'pipe',
        }),
      (error) => error.status === 1 && error.stderr.toString().includes('installed_hash_mismatch'),
    )
    assert.equal(
      (await readdir(home)).length,
      1,
      'startup touched private state before profile validation',
    )
  } finally {
    await writeFile(implementation, originalImplementation)
  }
  check(
    'host startup refuses changed installed implementation before accessing private state or database',
  )
  operator(['setup'])
  const configPath = join(home, 'runtime.json')
  let settings = JSON.parse(await readFile(configPath, 'utf8'))
  settings.appPort = 0
  await privateJson(configPath, settings)
  const account = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8'))
  const geminiMarker = 'disabled-fixture-gemini-key-marker'
  await privateJson(join(home, 'gemini-planner.json'), {
    provider: 'gemini',
    model: 'gemini-3.5-flash-lite',
    projectTier: 'free',
    enabled: false,
    apiKey: geminiMarker,
  })
  operator(['setup'])
  const repeat = JSON.parse(await readFile(configPath, 'utf8'))
  assert.ok(
    settings.fingerprintKey === repeat.fingerprintKey &&
      settings.identityKey === repeat.identityKey,
  )
  assert.equal((await stat(configPath)).mode & 0o777, 0o600)
  assert.equal((await stat(home)).mode & 0o777, 0o700)
  check('private persistent installation, stable keys and idempotent setup')
  assert.equal(JSON.parse(operator(['backup-status'])).status, 'disabled')
  assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'disabled')
  assert.equal(
    (await readdir(home)).some((name) => name.startsWith('backup-state-')),
    false,
  )
  check(
    'existing installations have no automatic backup side effect until an explicit per-database policy is enabled',
  )

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
  assert.equal(owner.session.plannerMode, 'local')
  assert.equal(owner.session.tenants.length, 2)
  assert.equal((await get(origin, '/healthz')).status, 200)
  const html = await get(origin, '/').then((response) => response.text())
  assert.ok(!html.includes('reference-session') && !html.includes('Use reference'))
  const publicBundle = await get(origin, '/app.js').then((response) => response.text())
  assert.ok(
    !publicBundle.includes(settings.identityKey) &&
      !publicBundle.includes(settings.fingerprintKey) &&
      !publicBundle.includes(geminiMarker) &&
      !publicBundle.includes('generativelanguage.googleapis.com') &&
      !publicBundle.includes('password_hash'),
  )
  check('real credentials, cookie-only session, origin checks and browser/server isolation')
  const publicStart = await post(origin, '/api/presence/session', { tenantId: 'hotel-demo' })
  assert.equal(publicStart.status, 200)
  const guestCookie = publicStart.headers.get('set-cookie').split(';')[0]
  const presenceSession = await publicStart.json()
  assert.equal(presenceSession.kind, 'guest')
  const independent = join(home, 'independent-chat')
  await mkdir(independent)
  const supported = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(independent, 'package.json'),
    JSON.stringify({
      private: true,
      type: 'module',
      overrides: Object.fromEntries(
        (supported.catalog.externals ?? []).map((item) => [item.name, item.version]),
      ),
      dependencies: Object.fromEntries(
        supported.artifacts.map((a) => [a.name, 'file:' + join(root, 'dist/library', a.tarball)]),
      ),
    }),
  )
  execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
    cwd: independent,
    stdio: 'pipe',
  })
  const chatRequire = createRequire(join(independent, 'package.json'))
  const chatRelease = await import(
    pathToFileURL(chatRequire.resolve('@archava/config/release')).href
  )
  await chatRelease.verifyInstalledRelease(
    supported,
    supported.catalog,
    join(independent, 'node_modules'),
  )
  const { createPresenceClient } = await import(
    pathToFileURL(chatRequire.resolve('@archava/presence/client')).href
  )
  const chat = createPresenceClient(origin, (input, init) =>
    fetch(input, { ...init, headers: { ...init.headers, origin, cookie: guestCookie } }),
  )
  const turn = {
    version: '1.0.0',
    sessionId: presenceSession.sessionId,
    requestId: randomUUID(),
    pageRevision: 0,
    transport: 'chat',
    utterance: 'What is Archava?',
  }
  const chatAnswer = await chat.turn(turn)
  const standalone = join(home, 'voice_transcript.py')
  await writeFile(
    standalone,
    await readFile(join(root, 'examples/presence-consumers/voice_transcript.py')),
  )
  const voiceRequest = { ...turn, requestId: randomUUID() }
  const voiceAnswer = JSON.parse(
    execFileSync('python3', [standalone], {
      env: { PATH: process.env.PATH },
      input: JSON.stringify({ origin, guest_cookie: guestCookie, request: voiceRequest }),
      encoding: 'utf8',
      stdio: ['pipe', 'pipe', 'pipe'],
    }),
  )
  assert.equal(chatAnswer.text, voiceAnswer.text)
  assert.equal(voiceAnswer.transport, 'synthetic-voice-transcript')
  assert.deepEqual(voiceAnswer.allowedTools, ['public.help'])
  const publicIdentity = { cookie: guestCookie, csrf: '' }
  assert.equal((await post(origin, '/api/presence/turn', turn, publicIdentity)).status, 409)
  assert.equal(
    (
      await post(
        origin,
        '/api/presence/turn',
        { ...turn, requestId: randomUUID(), role: 'owner' },
        publicIdentity,
      )
    ).status,
    400,
  )
  assert.equal(
    (
      await post(
        origin,
        '/api/presence/turn',
        { ...turn, requestId: randomUUID(), requestedTool: 'crm.contacts.list' },
        publicIdentity,
      )
    ).status,
    403,
  )
  const otherStart = await post(origin, '/api/presence/session', { tenantId: 'hotel-demo' })
  const otherGuest = { cookie: otherStart.headers.get('set-cookie').split(';')[0], csrf: '' }
  assert.equal(
    (await post(origin, '/api/presence/turn', { ...turn, requestId: randomUUID() }, otherGuest))
      .status,
    403,
  )
  assert.deepEqual(
    await post(
      origin,
      '/api/presence/context',
      { version: '1.0.0', sessionId: turn.sessionId, section: 'protocol-section', revision: 1 },
      publicIdentity,
    ).then((r) => r.json()),
    { section: 'protocol-section', revision: 1 },
  )
  assert.equal(
    (await post(origin, '/api/presence/turn', { ...turn, requestId: randomUUID() }, publicIdentity))
      .status,
    409,
  )
  const ownerPresence = await post(
    origin,
    '/api/presence/session',
    { tenantId: 'hotel-demo' },
    owner,
  ).then((r) => r.json())
  assert.equal(ownerPresence.kind, 'owner')
  const ownerReply = await post(
    origin,
    '/api/presence/turn',
    {
      ...turn,
      sessionId: ownerPresence.sessionId,
      requestId: randomUUID(),
      utterance: 'tampilkan kontak',
    },
    owner,
  )
  assert.equal(ownerReply.status, 200)
  assert.equal((await ownerReply.json()).source, 'crm')
  assert.equal(
    (
      await post(
        origin,
        '/api/presence/turn',
        { ...turn, sessionId: ownerPresence.sessionId, requestId: randomUUID() },
        publicIdentity,
      )
    ).status,
    403,
  )
  check(
    'independent installed chat and standalone Python synthetic voice share the server policy; guest binding, exact page revisions, request replay, malformed identity, private tools and current owner/tenant access are enforced without provider calls',
  )
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
  assert.equal(
    (await post(origin, '/api/presence/session', { tenantId: 'studio-demo' }, staffSession)).status,
    403,
  )
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

  const opportunityIntent = {
    requestId: randomUUID(),
    command: {
      operation: 'opportunity.create',
      contactId: receipt.id,
      contactRevision: 1,
      title: 'Persisted hotel opportunity',
    },
  }
  const opportunityResponse = await post(
    origin,
    '/api/crm/hotel-demo/commands',
    opportunityIntent,
    owner,
  )
  assert.equal(opportunityResponse.status, 200)
  const opportunityReceipt = await opportunityResponse.json()
  const opportunityPath = '/api/crm/hotel-demo/opportunities/' + opportunityReceipt.id
  for (const [index, stage] of ['qualified', 'proposal', 'won'].entries()) {
    const response = await post(
      origin,
      '/api/crm/hotel-demo/commands',
      {
        requestId: randomUUID(),
        command: {
          operation: 'opportunity.move',
          id: opportunityReceipt.id,
          expectedRevision: index + 1,
          stage,
        },
      },
      owner,
    )
    assert.equal(response.status, 200)
  }
  const opportunityDetail = await get(origin, opportunityPath, owner).then((response) =>
    response.json(),
  )
  assert.equal(opportunityDetail.opportunity.stage, 'won')
  assert.equal(opportunityDetail.transitions.length, 4)
  assert.equal((await get(origin, opportunityPath, staffSession)).status, 404)
  assert.equal(
    (await get(origin, '/api/crm/studio-demo/opportunities/' + opportunityReceipt.id, owner))
      .status,
    404,
  )
  assert.equal((await get(origin, opportunityPath, view)).status, 200)
  assert.equal(
    (await post(origin, '/api/crm/hotel-demo/commands', opportunityIntent, view)).status,
    403,
  )
  const pipelineProjection = await post(
    origin,
    '/api/crm/hotel-demo/tools',
    { name: 'crm.opportunities.list', input: {} },
    owner,
  ).then((response) => response.json())
  assert.equal(pipelineProjection.items[0].id, opportunityReceipt.id)
  assert.equal('ownerId' in pipelineProjection.items[0], false)
  assert.notEqual(
    owner.session.tenants[0].pipelineLabels.won,
    owner.session.tenants[1].pipelineLabels.won,
  )
  check(
    'session-protected persistent pipeline, canonical stage history, separate grants and configured business labels',
  )

  const pipelineGrant = join(home, 'pipeline-grant.json')
  await privateJson(pipelineGrant, {
    username: account.username,
    tenants: ['hotel-demo'],
    access: 'none',
  })
  operator(['user', 'pipeline', pipelineGrant])
  operator(['setup'])
  assert.equal((await get(origin, '/api/crm/hotel-demo/opportunities', owner)).status, 403)
  assert.equal(
    (await post(origin, '/api/crm/hotel-demo/commands', opportunityIntent, owner)).status,
    403,
  )
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', owner)).status, 200)
  await privateJson(pipelineGrant, {
    username: account.username,
    tenants: ['hotel-demo'],
    access: 'write',
  })
  operator(['user', 'pipeline', pipelineGrant])
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', opportunityIntent, owner).then((response) =>
      response.json(),
    ),
    opportunityReceipt,
  )
  check(
    'operator explicitly enables pipeline; setup preserves existing grants and retry rechecks revocation',
  )

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
  let reviewedFollowup
  let reviewedReceipt
  const countPool = poolFor(settings)
  const snapshot = () =>
    countPool.query(`SELECT
    (SELECT count(*) FROM archava_crm.contacts) AS contacts,
    (SELECT count(*) FROM archava_crm.activities) AS activities,
    (SELECT count(*) FROM archava_crm.followups) AS followups,
    (SELECT count(*) FROM archava_crm.commands) AS commands,
    (SELECT count(*) FROM archava_crm.audit) AS audit,
    (SELECT count(*) FROM archava_crm.opportunities) AS opportunities,
    (SELECT count(*) FROM archava_crm.opportunity_transitions) AS transitions`)
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
    const opportunities = await post(
      origin,
      conversationPath,
      { utterance: 'tampilkan peluang' },
      owner,
    ).then((response) => response.json())
    assert.equal(opportunities.data.kind, 'opportunities')
    assert.equal(opportunities.data.items[0].id, opportunityReceipt.id)
    const selectedOpportunity = await post(
      origin,
      conversationPath,
      {
        utterance: 'ringkas peluang',
        opportunityId: opportunityReceipt.id,
      },
      owner,
    ).then((response) => response.json())
    assert.equal(selectedOpportunity.data.kind, 'opportunity')
    assert.equal(selectedOpportunity.data.opportunity.transitions.length, 4)
    assert.equal(
      (
        await post(
          origin,
          conversationPath,
          {
            utterance: 'ringkas peluang',
            opportunityId: receipt.id,
          },
          owner,
        )
      ).status,
      404,
    )
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
    check(
      'pipeline conversation keeps opportunity/contact selections distinct and reads the persisted stage history',
    )
    reviewedFollowup = {
      requestId: randomUUID(),
      command: {
        operation: 'followup.create',
        contactId: prepared.data.draft.contactId,
        contactRevision: prepared.data.draft.contactRevision,
        summary: prepared.data.draft.summary,
        dueAt: prepared.data.draft.dueAt,
      },
    }
    const approved = await Promise.all(
      Array.from({ length: 8 }, () =>
        post(origin, '/api/crm/hotel-demo/commands', reviewedFollowup, owner),
      ),
    )
    assert.ok(approved.every((response) => response.status === 200))
    const receipts = await Promise.all(approved.map((response) => response.json()))
    reviewedReceipt = receipts[0]
    assert.ok(
      receipts.every((receipt) => JSON.stringify(receipt) === JSON.stringify(reviewedReceipt)),
    )
    assert.equal(
      (
        await countPool.query('SELECT 1 FROM archava_crm.audit WHERE request_id=$1', [
          reviewedFollowup.requestId,
        ])
      ).rowCount,
      1,
    )
    assert.equal(
      (await post(origin, '/api/crm/hotel-demo/commands', reviewedFollowup, view)).status,
      403,
    )
    await countPool.query(
      "UPDATE archava_crm.memberships SET permissions=array_remove(permissions,'followups.write') WHERE principal_id=(SELECT id FROM archava_identity.users WHERE username=$1)",
      [account.username],
    )
    assert.equal(
      (await post(origin, '/api/crm/hotel-demo/commands', reviewedFollowup, owner)).status,
      403,
    )
    await countPool.query(
      "UPDATE archava_crm.memberships SET permissions=array_append(permissions,'followups.write') WHERE principal_id=(SELECT id FROM archava_identity.users WHERE username=$1)",
      [account.username],
    )
    const recoveryPath = '/api/crm/hotel-demo/commands/' + reviewedFollowup.requestId
    const beforeRecovery = (await snapshot()).rows[0]
    const recovered = await get(origin, recoveryPath, owner)
    assert.equal(recovered.headers.get('cache-control'), 'no-store')
    assert.deepEqual(await recovered.json(), {
      status: 'committed',
      requestId: reviewedFollowup.requestId,
      receipt: reviewedReceipt,
    })
    assert.equal((await get(origin, recoveryPath)).status, 401)
    for (const [path, caller] of [
      [recoveryPath, view],
      [recoveryPath, staffSession],
      ['/api/crm/studio-demo/commands/' + reviewedFollowup.requestId, owner],
    ]) {
      assert.deepEqual(await get(origin, path, caller).then((response) => response.json()), {
        status: 'unconfirmed',
        requestId: reviewedFollowup.requestId,
      })
    }
    assert.equal((await get(origin, '/api/crm/hotel-demo/commands/invalid', owner)).status, 400)
    await countPool.query(
      "UPDATE archava_crm.memberships SET permissions=array_remove(permissions,'followups.read') WHERE tenant_id='hotel-demo' AND principal_id=(SELECT id FROM archava_identity.users WHERE username=$1)",
      [account.username],
    )
    assert.equal((await get(origin, recoveryPath, owner)).status, 403)
    await countPool.query(
      "UPDATE archava_crm.memberships SET permissions=array_append(permissions,'followups.read') WHERE tenant_id='hotel-demo' AND principal_id=(SELECT id FROM archava_identity.users WHERE username=$1)",
      [account.username],
    )
    assert.deepEqual((await snapshot()).rows[0], beforeRecovery)
    check(
      'authenticated read-only command recovery binds initiator/tenant, masks absent receipts and rechecks current read permissions without mutations',
    )
    const currentContact = await get(
      origin,
      '/api/crm/hotel-demo/contacts/' + receipt.id,
      owner,
    ).then((response) => response.json())
    assert.equal(
      (
        await post(
          origin,
          '/api/crm/hotel-demo/commands',
          {
            requestId: randomUUID(),
            command: {
              operation: 'contact.update',
              id: receipt.id,
              expectedRevision: currentContact.contact.revision,
              data: { organization: 'Updated synthetic organization' },
            },
          },
          owner,
        )
      ).status,
      200,
    )
    const afterReview = (await snapshot()).rows[0]
    assert.equal(
      (
        await post(
          origin,
          '/api/crm/hotel-demo/commands',
          {
            ...reviewedFollowup,
            requestId: randomUUID(),
          },
          owner,
        )
      ).status,
      409,
    )
    assert.deepEqual((await snapshot()).rows[0], afterReview)
    check(
      'human-reviewed Save is idempotent, rechecks permissions and refuses stale drafts without new writes',
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
  const usagePool = poolFor(settings, 'archava_crm_app')
  try {
    const store = await providerPg.PostgresProviderAttemptStore.create(usagePool)
    await providerModule.runProvider({
      store,
      policy: providerPolicy,
      attemptId: randomUUID(),
      invoke: () => Promise.resolve('private-result-fixture'),
      httpStatus: () => 200,
    })
    const operatorPool = poolFor(settings)
    try {
      await operatorPool.query(
        "UPDATE archava_provider.buckets SET last_started_at=clock_timestamp()-interval '1 minute' WHERE bucket_id=$1",
        [providerPolicy.bucketId],
      )
    } finally {
      await operatorPool.end()
    }
    await assert.rejects(() =>
      providerModule.runProvider({
        store,
        policy: providerPolicy,
        attemptId: randomUUID(),
        invoke: () => Promise.reject({ code: 'timeout', body: geminiMarker }),
      }),
    )
    const usage = JSON.parse(operator(['ai-status']).toString())
    assert.equal(usage.attemptsToday, 2)
    assert.deepEqual(usage.recent.map((item) => item.state).sort(), ['failed', 'succeeded'])
    assert.ok(
      !JSON.stringify(usage).includes(geminiMarker) &&
        !JSON.stringify(usage).includes('private-result-fixture'),
    )
  } finally {
    await usagePool.end()
  }
  const exhaustedPool = poolFor(settings)
  try {
    await exhaustedPool.query(
      'UPDATE archava_provider.buckets SET used=daily_limit WHERE bucket_id=$1',
      [providerPolicy.bucketId],
    )
  } finally {
    await exhaustedPool.end()
  }
  await privateJson(join(home, 'gemini-planner.json'), {
    provider: 'gemini',
    model: providerPolicy.model,
    projectTier: 'free',
    enabled: true,
    apiKey: geminiMarker,
  })
  check(
    'operator sees safe success/failure/latency metadata; no keys, results or business identifiers stored',
  )
  await stop()
  assert.equal((await stat(join(home, 'data'))).isDirectory(), true)
  assert.throws(() =>
    execFileSync(join(bin, 'pg_ctl'), ['-D', join(home, 'data'), 'status'], { stdio: 'pipe' }),
  )
  origin = await boot()
  assert.equal(
    (await get(origin, '/api/session', owner).then((response) => response.json())).plannerMode,
    'gemini-synthetic',
  )
  const limited = await post(
    origin,
    conversationPath,
    { utterance: 'Bisa tampilkan daftar kontak CRM?' },
    owner,
  )
  assert.equal(limited.status, 429)
  assert.ok(Number(limited.headers.get('retry-after')) > 0)
  assert.match(limited.headers.get('x-archava-request-id'), /^[0-9a-f-]{36}$/)
  const exhaustedStatus = JSON.parse(operator(['ai-status']).toString())
  assert.equal(exhaustedStatus.attemptsToday, 30)
  assert.equal(exhaustedStatus.remaining, 0)
  assert.equal(exhaustedStatus.recent.length, 2)
  check(
    'daily usage survives host/database restart; exhausted Gemini request returns correlated 429 before any outbound call',
  )
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
  const modulesFile = join(home, 'module-selection.json')
  const minimal = { contacts: true, pipeline: false, assistant: false, nativeActions: false }
  await privateJson(modulesFile, minimal)
  assert.throws(() => operator(['modules', modulesFile]), 'live host must refuse lifecycle changes')
  await stop()
  operator(['modules', modulesFile])
  origin = await boot()
  assert.deepEqual(
    (await get(origin, '/api/session', owner).then((r) => r.json())).modules,
    minimal,
  )
  if (process.argv.includes('--browser-once')) {
    const credentials = join(home, 'minimal-browser-credentials.json')
    await privateJson(credentials, {
      owner: { username: account.username, password: account.password },
    })
    await verifyAuthenticatedBrowser({ root, home, origin, credentials, minimal: true })
  }
  assert.equal((await get(origin, '/api/crm/hotel-demo/contacts', owner)).status, 200)
  assert.equal((await get(origin, '/api/crm/hotel-demo/opportunities', owner)).status, 404)
  assert.equal(
    (await post(origin, '/api/crm/hotel-demo/conversation', { utterance: 'list contacts' }, owner))
      .status,
    404,
  )
  assert.equal(
    (
      await post(
        origin,
        '/api/crm/hotel-demo/tools',
        { name: 'crm.contacts.list', input: {} },
        owner,
      )
    ).status,
    404,
  )
  assert.equal(
    (
      await post(
        origin,
        '/api/crm/hotel-demo/commands',
        {
          requestId: randomUUID(),
          command: {
            operation: 'opportunity.create',
            contactId: receipt.id,
            contactRevision: 1,
            title: 'Disabled write',
            amountMinor: 1,
            currency: 'IDR',
          },
        },
        owner,
      )
    ).status,
    404,
  )
  assert.deepEqual(
    await get(origin, '/api/crm/hotel-demo/commands/' + first.requestId, owner).then((r) =>
      r.json(),
    ),
    { status: 'committed', requestId: first.requestId, receipt },
  )
  await stop()
  await privateJson(modulesFile, {
    contacts: true,
    pipeline: true,
    assistant: true,
    nativeActions: true,
  })
  operator(['modules', modulesFile])
  origin = await boot()
  assert.equal((await get(origin, '/api/crm/hotel-demo/opportunities', owner)).status, 200)
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', first, owner).then((r) => r.json()),
    receipt,
  )
  check(
    'minimal composition disables UI projection, tools, pipeline writes and assistant; re-enable retains records, identity grants and original UUID receipts without copying implementation',
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', reviewedFollowup, owner).then((response) =>
      response.json(),
    ),
    reviewedReceipt,
  )
  assert.equal(
    (
      await get(origin, '/api/crm/hotel-demo/contacts/' + receipt.id, owner).then((response) =>
        response.json(),
      )
    ).followups.length,
    1,
  )
  check('reviewed follow-up and original Save receipt persist across application/database restart')
  assert.deepEqual(
    await get(origin, '/api/crm/hotel-demo/commands/' + reviewedFollowup.requestId, owner).then(
      (response) => response.json(),
    ),
    { status: 'committed', requestId: reviewedFollowup.requestId, receipt: reviewedReceipt },
  )
  check(
    'application/database restart recovers the original Save receipt through GET without replaying its command',
  )
  assert.deepEqual(
    await get(origin, opportunityPath, owner).then((response) => response.json()),
    opportunityDetail,
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', opportunityIntent, owner).then((response) =>
      response.json(),
    ),
    opportunityReceipt,
  )
  check(
    'pipeline record, full stage history and retry receipt survive application/database restart',
  )

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

  const manualBackup = join(home, 'backup.dump')
  operator(['backup', manualBackup])
  assert.equal((await stat(manualBackup)).mode & 0o777, 0o600)
  assert.equal((await stat(manualBackup + '.runtime.json')).mode & 0o777, 0o600)
  const backupStatePath = join(home, 'backup-state-' + settings.database + '.json')
  const backupPolicyPath = join(home, 'backup-policy-' + settings.database + '.json')
  const backupHealth = () => JSON.parse(operator(['backup-status']))
  assert.equal(backupHealth().archiveIntact, true)
  assert.equal(backupHealth().status, 'disabled')
  assert.ok(!JSON.stringify(backupHealth()).includes(settings.fingerprintKey))
  assert.ok(!JSON.stringify(backupHealth()).includes(manualBackup))
  assert.equal((await stat(backupStatePath)).mode & 0o777, 0o600)
  assert.throws(() => operator(['backup-policy', 'enable', '0']))
  assert.throws(() => operator(['backup-policy', 'enable', '169']))
  const enabled = JSON.parse(operator(['backup-policy', 'enable', '24']))
  assert.equal(enabled.enabled, true)
  assert.equal(enabled.automaticDeletion, false)
  assert.equal((await stat(backupPolicyPath)).mode & 0o777, 0o600)
  assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'not_due')
  check(
    'shared manual and scheduled publisher validates custom archives, private metadata and bounded policy; diagnostics contain no configuration, archive path or credentials',
  )

  const backupPool = poolFor(settings),
    backupLock = await backupPool.connect()
  try {
    await backupLock.query('SELECT pg_advisory_lock(hashtextextended($1,0))', [
      'archava/local-backup/v1/' + settings.database,
    ])
    const before = await readFile(backupStatePath)
    assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'busy')
    assert.deepEqual(await readFile(backupStatePath), before)
  } finally {
    backupLock.release()
    await backupPool.end()
  }
  check(
    'database advisory lock excludes overlapping backup processes and releases without stale filesystem locks or a new success record',
  )

  const manualBytes = await readFile(manualBackup)
  const lastSuccess = JSON.parse(await readFile(backupStatePath, 'utf8')).lastSuccess
  assert.throws(() => operator(['backup', manualBackup]))
  assert.deepEqual(await readFile(manualBackup), manualBytes)
  assert.deepEqual(JSON.parse(await readFile(backupStatePath, 'utf8')).lastSuccess, lastSuccess)
  assert.equal(backupHealth().status, 'failed')
  assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'succeeded')
  let latest = JSON.parse(await readFile(backupStatePath, 'utf8'))
  await appendFile(latest.lastSuccess.file, 'fixture corruption')
  assert.equal(backupHealth().archiveIntact, false)
  assert.equal(backupHealth().status, 'stale')
  assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'succeeded')
  assert.equal(backupHealth().archiveIntact, true)
  assert.equal(backupHealth().status, 'fresh')
  assert.deepEqual(await readFile(manualBackup), manualBytes)
  assert.ok(!(await readdir(join(home, 'backups'))).some((name) => name.includes('.partial')))
  check(
    'existing archives are never overwritten; failed/corrupt snapshots are not fresh and a later backup recovers while retaining the previous material',
  )

  const archivesBeforeRestart = (await readdir(join(home, 'backups'))).filter((name) =>
    name.endsWith('.dump'),
  ).length
  await stop()
  origin = await boot()
  assert.equal(backupHealth().status, 'fresh')
  assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'not_due')
  assert.equal(
    (await readdir(join(home, 'backups'))).filter((name) => name.endsWith('.dump')).length,
    archivesBeforeRestart,
  )
  latest = JSON.parse(await readFile(backupStatePath, 'utf8'))
  latest.lastSuccess.completedAt = '2020-01-01T00:00:00.000Z'
  await privateJson(backupStatePath, latest)
  assert.equal(backupHealth().status, 'stale')
  await stop()
  origin = await boot()
  for (let tries = 0; backupHealth().status !== 'fresh' && tries < 80; tries++)
    await new Promise((done) => setTimeout(done, 100))
  assert.equal(backupHealth().status, 'fresh')
  assert.equal(
    (await readdir(join(home, 'backups'))).filter((name) => name.endsWith('.dump')).length,
    archivesBeforeRestart + 1,
  )
  assert.equal(JSON.parse(operator(['backup-run'])).outcome, 'not_due')
  const backup = JSON.parse(await readFile(backupStatePath, 'utf8')).lastSuccess.file
  assert.notEqual(backup, manualBackup)
  assert.equal((await get(origin, '/healthz')).status, 200)
  assert.equal(JSON.parse(operator(['doctor'])).backups.status, 'fresh')
  check(
    'real host restart retains freshness and catches up one overdue automatic snapshot without blocking CRM health or repeating the capture',
  )
  const privatePolicy = await readFile(backupPolicyPath)
  await chmod(backupPolicyPath, 0o644)
  assert.throws(() => operator(['backup-status']))
  assert.equal(JSON.parse(operator(['doctor'])).backups.status, 'unavailable')
  await stop()
  origin = await boot()
  assert.equal((await get(origin, '/healthz')).status, 200)
  await chmod(backupPolicyPath, 0o600)
  assert.deepEqual(await readFile(backupPolicyPath), privatePolicy)
  assert.equal(backupHealth().status, 'fresh')
  check(
    'unsafe private backup policy refuses execution and reports unavailable while a real host restart still serves CRM; repair preserves the prior valid archive',
  )
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
    'automatically published pg_dump archive plus runtime metadata actually restores into a fresh database and leaves the original database untouched',
  )

  await stop()
  origin = await boot('restored-runtime.json')
  const restoredBackups = JSON.parse(operator(['backup-status'], 'restored-runtime.json'))
  assert.equal(restoredBackups.enabled, false)
  assert.equal(restoredBackups.lastSuccessAt, null)
  assert.equal(JSON.parse(operator(['backup-run'], 'restored-runtime.json')).outcome, 'disabled')
  check(
    'restored databases have a separate disabled backup policy and cannot inherit primary freshness or automatic scheduling',
  )
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
  assert.equal(detail.followups[0]?.summary, reviewedFollowup.command.summary)
  assert.deepEqual(
    await get(origin, '/api/crm/hotel-demo/commands/' + reviewedFollowup.requestId, owner).then(
      (response) => response.json(),
    ),
    { status: 'committed', requestId: reviewedFollowup.requestId, receipt: reviewedReceipt },
  )
  check(
    'after backup/restore and explicit account recovery, receipt lookup remains read-only and keeps the original UUID',
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', reviewedFollowup, owner).then((response) =>
      response.json(),
    ),
    reviewedReceipt,
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', first, owner).then((response) =>
      response.json(),
    ),
    receipt,
  )
  const restored = JSON.parse(await readFile(join(home, 'restored-runtime.json'), 'utf8'))
  const recoveredUsage = JSON.parse(operator(['ai-status'], 'restored-runtime.json').toString())
  assert.equal(recoveredUsage.attemptsToday, 30)
  assert.equal(recoveredUsage.recent.length, 2)
  assert.ok(recoveredUsage.restoreHoldUntil)
  assert.equal(
    (
      await post(
        origin,
        conversationPath,
        { utterance: 'Bisa tampilkan daftar kontak CRM?' },
        owner,
      )
    ).status,
    429,
  )
  assert.equal(
    (await post(origin, conversationPath, { utterance: 'tampilkan kontak' }, owner)).status,
    200,
  )
  check(
    'backup restores provider outcomes; conservative UTC-day hold prevents quota reset while local CRM remains available',
  )
  assert.deepEqual(
    await get(origin, opportunityPath, owner).then((response) => response.json()),
    opportunityDetail,
  )
  assert.deepEqual(
    await post(origin, '/api/crm/hotel-demo/commands', opportunityIntent, owner).then((response) =>
      response.json(),
    ),
    opportunityReceipt,
  )
  check('pg_dump/pg_restore preserves pipeline history and the original idempotency receipt')
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
  await assert.rejects(
    () => stat(outboundMarker),
    (error) => error?.code === 'ENOENT',
  )
  check('live-planner quota/restore tests issue zero external provider requests')

  const nativeBase = '/api/crm/hotel-demo/actions/followup'
  const nativeContact = await post(
    origin,
    '/api/crm/hotel-demo/commands',
    create('Native approved fixture'),
    owner,
  ).then((r) => r.json())
  const nativeInputs = {
    contactId: nativeContact.id,
    contactRevision: 1,
    summary: 'Native approved fixture',
    dueAt: '2026-10-20T10:00:00Z',
  }
  assert.equal((await post(origin, nativeBase + '/review', nativeInputs, owner)).status, 503)
  const diagnostic = JSON.parse(operator(['doctor'], 'restored-runtime.json'))
  assert.equal(diagnostic.nativeActions, 'held')
  operator(['resume-actions'], 'restored-runtime.json')
  assert.equal(JSON.parse(operator(['doctor'], 'restored-runtime.json')).nativeActions, 'ready')
  check(
    'actual restored database holds native actions until explicit operator resume; diagnostics verify the supported local profile',
  )
  assert.equal((await post(origin, nativeBase + '/review', nativeInputs)).status, 401)
  assert.equal(
    (await post(origin, nativeBase + '/review', nativeInputs, owner, { 'x-archava-csrf': '' }))
      .status,
    403,
  )
  assert.equal(
    (
      await post(
        origin,
        nativeBase + '/review',
        { ...nativeInputs, principalId: 'forged', confirmed: true },
        owner,
      )
    ).status,
    400,
  )
  const reviewResponse = await post(origin, nativeBase + '/review', nativeInputs, owner)
  assert.equal(reviewResponse.status, 200)
  const reviewed = await reviewResponse.json()
  await stop()
  await privateJson(modulesFile, minimal)
  assert.throws(
    () => operator(['modules', modulesFile], 'restored-runtime.json'),
    'unresolved native review must block disable',
  )
  assert.equal(
    JSON.parse(await readFile(join(home, 'restored-runtime.json'), 'utf8')).modules.nativeActions,
    true,
  )
  origin = await boot('restored-runtime.json')
  check(
    'lifecycle refuses disabling unresolved native effects and retains authorized reconciliation',
  )
  const body = {
    inputs: nativeInputs,
    confirmation: { challengeId: reviewed.challenge.challengeId, token: reviewed.challenge.token },
  }
  assert.equal(
    (
      await post(
        origin,
        nativeBase + '/present',
        { ...body, inputs: { ...nativeInputs, summary: 'Altered after review' } },
        owner,
      )
    ).status,
    403,
  )
  assert.equal((await post(origin, nativeBase + '/present', body, owner)).status, 200)
  const status = await get(origin, '/api/crm/hotel-demo/commands/' + reviewed.intentId, owner).then(
    (r) => r.json(),
  )
  assert.equal(status.status, 'committed')
  assert.equal(status.receipt.kind, 'followup')
  assert.notEqual((await post(origin, nativeBase + '/present', body, owner)).status, 200)
  const nativeDetail = await get(
    origin,
    '/api/crm/hotel-demo/contacts/' + nativeContact.id,
    owner,
  ).then((r) => r.json())
  assert.equal(nativeDetail.followups.length, 1)
  assert.ok(!JSON.stringify(status).includes(reviewed.challenge.token))
  check(
    'authenticated native HTTP review/confirmation/receipt flow rejects anonymous, CSRF, forged authority, payload drift and repeat execution; exactly one task commits',
  )

  const queueOwnerResponse = await post(
    origin,
    '/api/crm/hotel-demo/commands',
    {
      requestId: randomUUID(),
      command: {
        operation: 'followup.create',
        contactId: nativeContact.id,
        contactRevision: nativeDetail.contact.revision,
        summary: 'Worklist owner overdue <b>literal</b>',
        dueAt: '2020-01-01T00:00:00Z',
      },
    },
    owner,
  )
  assert.equal(queueOwnerResponse.status, 200)
  const queueStaffSession = await login(origin, staff.username, recoveredStaff)
  const queueStaffContact = await post(
    origin,
    '/api/crm/hotel-demo/commands',
    create('Worklist staff contact'),
    queueStaffSession,
  ).then((r) => r.json())
  assert.equal(
    (
      await post(
        origin,
        '/api/crm/hotel-demo/commands',
        {
          requestId: randomUUID(),
          command: {
            operation: 'followup.create',
            contactId: queueStaffContact.id,
            contactRevision: 1,
            summary: 'Worklist staff overdue',
            dueAt: '2020-01-02T00:00:00Z',
          },
        },
        queueStaffSession,
      )
    ).status,
    200,
  )
  const queuePath = '/api/crm/hotel-demo/followups?bucket=overdue'
  assert.equal((await get(origin, queuePath)).status, 401)
  const ownerQueue = await get(origin, queuePath, owner).then((r) => r.json())
  assert.ok(
    ownerQueue.items.some((item) => item.summary === 'Worklist owner overdue <b>literal</b>'),
  )
  assert.ok(ownerQueue.items.some((item) => item.summary === 'Worklist staff overdue'))
  const staffQueue = await get(origin, queuePath, queueStaffSession).then((r) => r.json())
  assert.ok(staffQueue.items.some((item) => item.summary === 'Worklist staff overdue'))
  assert.ok(!JSON.stringify(staffQueue).includes('Worklist owner overdue'))
  assert.equal((await get(origin, '/api/crm/studio-demo/followups', queueStaffSession)).status, 403)
  assert.equal(
    (await get(origin, '/api/crm/hotel-demo/followups?afterId=' + randomUUID(), owner)).status,
    400,
  )
  const queueReply = await post(
    origin,
    '/api/crm/hotel-demo/conversation',
    { utterance: 'tampilkan follow-up terlambat' },
    queueStaffSession,
  ).then((r) => r.json())
  assert.equal(queueReply.data.kind, 'followups')
  assert.ok(!JSON.stringify(queueReply).includes('Worklist owner overdue'))
  check(
    'authenticated worklist and Archava share due tasks while enforcing current assigned staff and tenant scope; partial cursors are refused',
  )

  const completionBase = nativeBase + '/complete'
  const fixtureTask = nativeDetail.followups[0]
  assert.ok(fixtureTask)
  const completionInputs = { followupId: fixtureTask.id, expectedRevision: 1 }
  assert.equal((await post(origin, completionBase + '/review', completionInputs)).status, 401)
  assert.equal(
    (
      await post(origin, completionBase + '/review', completionInputs, owner, {
        'x-archava-csrf': '',
      })
    ).status,
    403,
  )
  assert.equal(
    (
      await post(
        origin,
        completionBase + '/review',
        { ...completionInputs, confirmed: true },
        owner,
      )
    ).status,
    400,
  )
  assert.equal(
    (await get(origin, '/api/crm/hotel-demo/followups/' + fixtureTask.id, queueStaffSession))
      .status,
    404,
  )
  const completionDraft = await post(
    origin,
    '/api/crm/hotel-demo/conversation',
    { utterance: 'selesaikan follow-up', followupId: fixtureTask.id },
    owner,
  ).then((r) => r.json())
  assert.equal(completionDraft.data.kind, 'completion_draft')
  assert.equal(
    (
      await get(origin, '/api/crm/hotel-demo/followups/' + fixtureTask.id, owner).then((r) =>
        r.json(),
      )
    ).followup.status,
    'open',
  )
  const completionReview = await post(
    origin,
    completionBase + '/review',
    completionInputs,
    owner,
  ).then((r) => r.json())
  const completeBody = {
    inputs: completionInputs,
    confirmation: {
      challengeId: completionReview.challenge.challengeId,
      token: completionReview.challenge.token,
    },
  }
  assert.equal(
    (
      await post(
        origin,
        completionBase + '/present',
        { ...completeBody, inputs: { ...completionInputs, expectedRevision: 2 } },
        owner,
      )
    ).status,
    409,
  )
  assert.equal((await post(origin, completionBase + '/present', completeBody, owner)).status, 200)
  const completionStatus = await get(
    origin,
    '/api/crm/hotel-demo/commands/' + completionReview.intentId,
    owner,
  ).then((r) => r.json())
  assert.equal(completionStatus.status, 'committed')
  assert.equal(completionStatus.receipt.id, fixtureTask.id)
  assert.equal(completionStatus.receipt.revision, 2)
  assert.notEqual(
    (await post(origin, completionBase + '/present', completeBody, owner)).status,
    200,
  )
  assert.equal(
    (
      await get(origin, '/api/crm/hotel-demo/followups/' + fixtureTask.id, owner).then((r) =>
        r.json(),
      )
    ).followup.status,
    'completed',
  )
  check(
    'authenticated completion preparation is read-only; current task review/confirmation commits once with exact receipt while anonymous, CSRF, forged flags, staff scope, changed revision and replay are refused',
  )

  // Keep the browser navigation fixture open; domain/API proof above used its original task.
  const navigationContact = await get(
    origin,
    '/api/crm/hotel-demo/contacts/' + nativeContact.id,
    owner,
  ).then((r) => r.json())
  assert.equal(
    (
      await post(
        origin,
        '/api/crm/hotel-demo/commands',
        {
          requestId: randomUUID(),
          command: {
            operation: 'followup.create',
            contactId: nativeContact.id,
            contactRevision: navigationContact.contact.revision,
            summary: 'Native approved fixture',
            dueAt: '2026-10-20T10:00:00Z',
          },
        },
        owner,
      )
    ).status,
    200,
  )

  const overviewPath = '/api/crm/hotel-demo/overview'
  assert.equal((await get(origin, overviewPath)).status, 401)
  assert.equal((await get(origin, '/api/crm/studio-demo/overview', queueStaffSession)).status, 403)
  assert.equal((await get(origin, overviewPath + '?scope=all', owner)).status, 400)
  const overviewPool = poolFor(restored)
  try {
    const unchanged = async () =>
      (
        await overviewPool.query(`SELECT
      (SELECT count(*) FROM archava_crm.commands) AS commands,
      (SELECT count(*) FROM archava_crm.audit) AS audit,
      (SELECT count(*) FROM archava_provider.attempts) AS provider,
      (SELECT count(*) FROM archava_crm_act.confirmation_challenges) AS confirmations`)
      ).rows
    const beforeOverview = await unchanged()
    for (const session of [owner, queueStaffSession]) {
      const access = await get(origin, '/api/crm/hotel-demo/access', session).then((r) => r.json())
      const response = await get(origin, overviewPath, session)
      assert.equal(response.status, 200)
      assert.equal(response.headers.get('cache-control'), 'no-store')
      const snapshot = await response.json()
      assert.equal(snapshot.tenantId, 'hotel-demo')
      assert.equal(snapshot.scope, access.scope)
      const expectedContacts = (
        await overviewPool.query(
          'SELECT count(*)::int AS count FROM archava_crm.contacts WHERE tenant_id=$1 AND ($2::boolean OR owner_id=$3)',
          ['hotel-demo', access.scope === 'all', access.ownerId],
        )
      ).rows[0].count
      assert.equal(snapshot.contacts, expectedContacts)
      const expectedTasks = (
        await overviewPool.query(
          `SELECT
        count(*) FILTER(WHERE f.status='open')::int AS open,
        count(*) FILTER(WHERE f.status='open' AND f.due_at<$4)::int AS overdue,
        count(*) FILTER(WHERE f.status='open' AND f.due_at>=$4)::int AS upcoming,
        count(*) FILTER(WHERE f.status='completed')::int AS completed
        FROM archava_crm.followups f JOIN archava_crm.contacts c ON c.tenant_id=f.tenant_id AND c.id=f.contact_id
        WHERE f.tenant_id=$1 AND ($2::boolean OR c.owner_id=$3)`,
          ['hotel-demo', access.scope === 'all', access.ownerId, snapshot.asOf],
        )
      ).rows[0]
      assert.deepEqual(snapshot.followups, expectedTasks)
      const reply = await post(
        origin,
        conversationPath,
        { utterance: 'ringkas kondisi CRM' },
        session,
      ).then((r) => r.json())
      assert.equal(reply.source, 'crm')
      assert.equal(reply.data.kind, 'overview')
      assert.equal(reply.data.snapshot.contacts, snapshot.contacts)
      assert.deepEqual(reply.data.snapshot.pipeline, snapshot.pipeline)
      assert.equal(reply.planner.providerId, 'archava-crm-local')
      assert.ok(!JSON.stringify(reply).includes('contact@example.test'))
    }
    const second = await get(origin, '/api/crm/studio-demo/overview', owner).then((r) => r.json())
    assert.equal(second.tenantId, 'studio-demo')
    assert.equal(second.contacts, 1)
    assert.deepEqual(await unchanged(), beforeOverview)
  } finally {
    await overviewPool.end()
  }
  check(
    'restored authenticated host and local Archava share complete current owner/staff CRM aggregates with no command, consent or provider call; forged query scope and cross-tenant access are refused',
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
  if (
    (process.argv.includes('--browser') || process.argv.includes('--browser-once')) &&
    !stopRequested
  ) {
    const credentials = join(home, 'browser-credentials.json')
    await privateJson(credentials, {
      owner: { username: actor, password: recoveredPassword },
      staff: { username: staff.username, password: recoveredStaff },
    })
    if (process.argv.includes('--browser-once'))
      await verifyAuthenticatedBrowser({ root, home, origin, credentials })
    else {
      await privateJson('/tmp/archava-crm-runtime-browser-target.json', { origin, credentials })
      process.stdout.write('Persistent CRM browser target ready; Ctrl-C stops the fixture.\n')
      await stopped
    }
  }
  if (process.argv.includes('--benchmark')) {
    const benchmarkPool = poolFor(restored)
    try {
      await runCapacityBaseline({
        root,
        home,
        origin,
        session: owner,
        pool: benchmarkPool,
        hostPid: live.child.pid,
      })
    } finally {
      await benchmarkPool.end()
    }
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
