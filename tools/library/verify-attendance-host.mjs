/* global process, fetch, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { createServer } from 'node:net'
import { randomBytes, randomUUID } from 'node:crypto'

const root = resolve(import.meta.dirname, '../..'),
  runtime = join(root, 'dist/crm-runtime')
const home = await mkdtemp(join(tmpdir(), 'archava-attendance-host-'))
const env = { ...process.env, ARCHAVA_CRM_HOME: home, ARCHAVA_CRM_PROFILE: 'runtime.json' }
const operator = async (args) =>
  execFileSync(process.execPath, [join(runtime, 'operator.mjs'), ...args], {
    cwd: root,
    env,
    stdio: 'pipe',
    encoding: 'utf8',
  })
const privateFile = async (name, value) => {
  const path = join(home, name + '.json')
  await writeFile(path, JSON.stringify(value), { mode: 0o600 })
  return path
}
const checks = [],
  check = (label) => {
    checks.push(label)
    process.stdout.write('PASS ' + label + '\n')
  }
let server, browser, page, origin
let output = '',
  exited
const stop = async () => {
  if (server && server.exitCode === null) {
    server.kill('SIGTERM')
    await exited
  }
  server = null
}
const start = async () => {
  output = ''
  server = spawn(process.execPath, [join(runtime, 'server.mjs')], {
    cwd: root,
    env,
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  exited = new Promise((done) => server.once('exit', done))
  server.stdout.on('data', (value) => {
    output += value.toString()
  })
  let ready = false
  for (let i = 0; i < 100; i++) {
    await new Promise((done) => setTimeout(done, 100))
    const match = /CRM ready: (http:\/\/127\.0\.0\.1:\d+)/.exec(output)
    if (match) {
      origin = match[1]
      ready = true
      break
    }
    if (server.exitCode !== null) break
  }
  assert.ok(ready, 'isolated attendance host did not start')
}
const signIn = async (account) => {
  const response = await fetch(origin + '/api/login', {
    method: 'POST',
    headers: { origin, 'content-type': 'application/json' },
    body: JSON.stringify({ username: account.username, password: account.password }),
  })
  assert.equal(response.status, 200)
  const cookie = response.headers.get('set-cookie').split(';')[0]
  const session = await (await fetch(origin + '/api/session', { headers: { cookie } })).json()
  return { cookie, csrf: session.csrf }
}
const send = (auth, path, body, headers = {}) =>
  fetch(origin + path, {
    headers: {
      cookie: auth.cookie,
      ...(body ? { origin, 'content-type': 'application/json', 'x-archava-csrf': auth.csrf } : {}),
      ...headers,
    },
    ...(body ? { method: 'POST', body: JSON.stringify(body) } : {}),
  })
const base = '/api/attendance/studio-demo'

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@att.local',
      password: randomBytes(24).toString('base64url'),
      role: 'viewer',
      tenants: ['studio-demo'],
    }
  await operator(['user', 'add', await privateFile('viewer', viewer)])
  const modules = {
    contacts: true,
    pipeline: false,
    assistant: false,
    nativeActions: false,
    attendance: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/sessions')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'attendance routes require authentication and domain grants; enabling module never auto-enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'attendance',
      await privateFile('att-' + access, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  const createSessCmd = {
    type: 'create_session',
    commandId: randomUUID(),
    title: 'Host Verification Workshop',
    sessionCode: 'HVW-2026',
    capacity: 25,
    startTime: '2026-10-25T09:00:00.000Z',
    endTime: '2026-10-25T17:00:00.000Z',
    location: 'Studio Room B',
  }
  assert.equal(
    (await send(auth, base + '/commands', createSessCmd, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', createSessCmd, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createSessCmd)).status, 403)
  assert.equal((await send(auth, '/api/attendance/unconfigured/sessions')).status, 404)
  const createRes = await send(auth, base + '/commands', createSessCmd)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.sessionId)
  assert.equal(receipt.status, 'scheduled')
  assert.equal(receipt.revision, 1)

  assert.equal(
    (await send(v, base + '/tools', { tool: 'attendance.listSessions', input: {} })).status,
    200,
  )
  assert.equal(
    (
      await send(v, base + '/tools', {
        tool: 'attendance.listSessions',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'attendance.create',
        input: { title: 'ILLEGAL' },
      })
    ).status,
    404,
  )
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal and attendance tool boundaries',
  )

  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createSessCmd.commandId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'recorded')
  assert.deepEqual(statusJson.receipt, receipt)
  check('host restart retains attendance state, memberships and idempotent receipt recovery')

  const reservation = createServer()
  await new Promise((done) => reservation.listen(0, '127.0.0.1', done))
  const port = reservation.address().port
  await new Promise((done) => reservation.close(done))
  process.env.CDP_PORT = String(port)
  process.env.E2E_PROFILE = join(home, 'browser-profile')
  const { Session } = await import('../e2e/cdp.mjs')
  browser = spawn(
    process.env.E2E_BROWSER ?? '/usr/bin/brave',
    [
      '--headless=new',
      '--no-sandbox',
      '--disable-gpu',
      '--disable-background-networking',
      '--no-first-run',
      '--remote-debugging-port=' + port,
      '--user-data-dir=' + join(home, 'browser-profile'),
      'about:blank',
    ],
    { stdio: 'ignore' },
  )
  let browserReady = false
  for (let n = 0; n < 60; n++) {
    await new Promise((r) => setTimeout(r, 250))
    try {
      if ((await fetch('http://127.0.0.1:' + port + '/json')).ok) {
        browserReady = true
        break
      }
    } catch {
      /* wait */
    }
  }
  assert.ok(browserReady, 'isolated browser did not start')
  page = await Session.open()
  await page.navigate(origin)
  const evaluate = (expr) => page.evaluate(expr)
  const wait = (condition) =>
    evaluate(
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-attendance-status]")?.textContent));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#attendance")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#attendance").hidden && document.querySelector("#attendance").textContent===""',
    ),
    true,
  )
  await fill('#login [name=username]', owner.username)
  await fill('#login [name=password]', owner.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!document.querySelector("#workspace").hidden && document.querySelector("#status").textContent===""',
  )
  await evaluate(
    'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
  )
  await wait(
    '!!document.querySelector("[data-attendance-create]") && document.querySelector("[data-attendance-status]").textContent==="Attendance dashboard ready."',
  )
  assert.equal(
    await evaluate(
      '!document.querySelector("#attendance").hidden && !document.querySelector("[data-attendance-create]").hidden',
    ),
    true,
  )

  // Full browser workflow: Create session with simulated response loss & recovery
  await fill('[data-attendance-create] [name=title]', 'CDP Browser Cohort')
  await fill('[data-attendance-create] [name=sessionCode]', 'CDP-COHORT-01')
  await fill('[data-attendance-create] [name=capacity]', '40')
  await fill('[data-attendance-create] [name=location]', 'Online Conference')
  await evaluate(`
    window.attFetch = window.fetch;
    window.attMode = 'truncated_json';
    window.attBlocked = false;
    window.fetch = async (...args) => {
      if (window.attBlocked && String(args[0]).includes('/api/attendance/') && args[1]?.method === 'POST') {
        return new Response(JSON.stringify({ error: 'forbidden' }), { status: 403, headers: { 'content-type': 'application/json' } });
      }
      const response = await window.attFetch(...args);
      if (window.attMode === 'truncated_json' && String(args[0]).includes('/api/attendance/') && args[1]?.method === 'POST') {
        window.attMode = 'none';
        return new Response('{', { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.attMode === 'body_stream_error' && String(args[0]).includes('/api/attendance/') && args[1]?.method === 'POST') {
        window.attMode = 'none';
        throw new TypeError('Lost attendance response');
      }
      if (window.attMode === 'invalid_schema' && String(args[0]).includes('/api/attendance/')) {
        window.attMode = 'none';
        return new Response(JSON.stringify({ notA: 'receipt' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.attMode === 'wrong_uuid' && String(args[0]).includes('/api/attendance/')) {
        window.attMode = 'none';
        const data = await response.json();
        const broken = data && data.receipt
          ? { ...data, receipt: { ...data.receipt, commandId: '00000000-0000-0000-0000-000000000000' } }
          : { ...data, commandId: '00000000-0000-0000-0000-000000000000' };
        return new Response(JSON.stringify(broken), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      return response;
    };
    Array.from(document.querySelectorAll('#attendance button')).find(x => x.textContent === 'Create Session').click();
  `)

  // 1. Truncated JSON response preserves uncertain state and reveals recovery
  await wait(
    'document.querySelector("[data-attendance-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await evaluate('!document.querySelector("[data-attendance-recovery]").hidden'), true)
  const countCreated = async () => {
    const r = await send(auth, base + '/sessions')
    assert.equal(r.status, 200)
    const j = await r.json()
    const list = Array.isArray(j) ? j : (j.items ?? [])
    return list.filter((x) => x.title === 'CDP Browser Cohort').length
  }
  assert.equal(await countCreated(), 1)

  // Blocked new write while outcome is uncertain
  await fill('[data-attendance-create] [name=title]', 'CDP Blocked Session')
  await evaluate(
    "Array.from(document.querySelectorAll('#attendance button')).find(x => x.textContent === 'Create Session').click()",
  )
  await wait(
    'document.querySelector("[data-attendance-status]").textContent.includes("Uncertain command outcome")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 2. Retry with body stream error retains uncertain recovery
  await evaluate("window.attMode = 'body_stream_error'")
  await evaluate(
    "Array.from(document.querySelectorAll('#attendance button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-attendance-status]").textContent.includes("Network interrupted while executing command")',
  )
  await idle()
  assert.equal(await countCreated(), 1)
  assert.equal(await evaluate('!document.querySelector("[data-attendance-recovery]").hidden'), true)

  // 3. Re-query status with invalid schema retains uncertain recovery
  await evaluate("window.attMode = 'invalid_schema'")
  await evaluate(
    "Array.from(document.querySelectorAll('#attendance button')).find(x => x.textContent === 'Re-query status').click()",
  )
  await wait(
    'document.querySelector("[data-attendance-status]").textContent.includes("Invalid receipt received during recovery")',
  )
  await idle()
  assert.equal(await evaluate('!document.querySelector("[data-attendance-recovery]").hidden'), true)

  // 4. Re-query status with wrong command UUID retains uncertain recovery
  await evaluate("window.attMode = 'wrong_uuid'")
  await evaluate(
    "Array.from(document.querySelectorAll('#attendance button')).find(x => x.textContent === 'Re-query status').click()",
  )
  await wait(
    'document.querySelector("[data-attendance-status]").textContent.includes("Recovery receipt command ID mismatch")',
  )
  await idle()
  assert.equal(await evaluate('!document.querySelector("[data-attendance-recovery]").hidden'), true)

  // 5. Successful recovery resolves receipt and reconciles UI state
  await evaluate(
    "Array.from(document.querySelectorAll('#attendance button')).find(x => x.textContent === 'Re-query status').click()",
  )
  await wait(
    'document.querySelector("[data-attendance-status]").textContent.includes("Command recovered with recorded receipt")',
  )
  await idle()
  assert.equal(await evaluate('document.querySelector("[data-attendance-recovery]").hidden'), true)
  assert.equal(await countCreated(), 1)

  // Browser check-in workflow on the recovered session
  const attendeeContactId = randomUUID()
  await evaluate(
    "Array.from(document.querySelectorAll('[data-attendance-list] li')).find(x => x.textContent.includes('CDP Browser Cohort')).querySelector('button').click()",
  )
  await wait(
    'document.querySelector("[data-attendance-detail]").textContent.includes("CDP Browser Cohort")',
  )
  await fill('[data-attendance-register] [name=contactId]', attendeeContactId)
  await evaluate(
    "Array.from(document.querySelectorAll('[data-attendance-register] button')).find(x => x.textContent === 'Register').click()",
  )
  await wait(
    `document.querySelector("[data-attendance-roster]").textContent.includes(${JSON.stringify(attendeeContactId)})`,
  )
  assert.equal(await countCreated(), 1)
  await evaluate("document.querySelector('[data-attendance-checkin]').click()")
  await wait(
    'document.querySelector("[data-attendance-roster]").textContent.includes("Status: ATTENDED")',
  )
  assert.equal(
    await evaluate(
      `document.querySelector("[data-attendance-detail]").textContent.includes(${JSON.stringify(attendeeContactId)})`,
    ),
    true,
  )
  assert.equal(await countCreated(), 1)

  check(
    'browser DOM dashboard completes full session check-in workflow and survives all 5 S1 failure modes through idempotent receipt recovery',
  )
  const destination = join(root, 'dist/attendance-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded session check-in workflow with resilient receipt recovery',
        checks,
        browser: 'isolated Chromium',
        providerCalls: 0,
        operationalKitsAccepted: [],
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(`\nAll ${checks.length} attendance host verification checks passed.\n`)
} finally {
  await page?.close()
  if (browser && browser.exitCode === null) browser.kill('SIGTERM')
  await stop()
  await rm(home, { recursive: true, force: true })
}
