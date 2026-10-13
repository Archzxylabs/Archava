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
const home = await mkdtemp(join(tmpdir(), 'archava-fulfillment-host-'))
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
  assert.ok(ready, 'isolated fulfillment host did not start')
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
const base = '/api/fulfillment/studio-demo'
const createJobCmd = {
  type: 'create_job',
  commandId: randomUUID(),
  jobCode: 'SHP-2026-0001',
  kind: 'dispatch',
  title: 'Restock Bandung DC',
  ownerRef: 'client-alpha',
}

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@ff.local',
      password: randomBytes(24).toString('base64url'),
      role: 'viewer',
      tenants: ['studio-demo'],
    }
  await operator(['user', 'add', await privateFile('viewer', viewer)])
  const off = {
    contacts: true,
    pipeline: false,
    assistant: false,
    nativeActions: false,
    attendance: false,
    fulfillment: false,
  }
  const on = { ...off, fulfillment: true }

  // Phase 1: the module is off, so the whole route surface is absent.
  await operator(['modules', await privateFile('modules-off', off)])
  await start()
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/jobs')).status, 404)
  await stop()
  check('fulfillment routes are absent while the module is disabled')

  // Phase 2: the module is on, but enabling it never auto-enrolls an account.
  await operator(['modules', await privateFile('modules-on', on)])
  await start()
  assert.equal((await fetch(origin + base + '/jobs')).status, 401)
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  assert.equal((await send(auth, base + '/jobs')).status, 403)
  await stop()
  check(
    'fulfillment routes require authentication and domain grants; enabling the module never auto-enrolls accounts',
  )

  // Phase 3: enroll the two principals and exercise the write boundary.
  for (const [file, account, access] of [
    ['ff-owner', owner, 'write'],
    ['ff-viewer', viewer, 'read'],
  ])
    await operator([
      'user',
      'fulfillment',
      await privateFile(file, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  assert.equal((await send(auth, base + '/access')).status, 200)
  assert.equal((await send(v, base + '/access')).status, 200)
  assert.equal(
    (await send(auth, base + '/commands', createJobCmd, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', createJobCmd, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createJobCmd)).status, 403)
  assert.equal((await send(auth, '/api/fulfillment/unconfigured/jobs')).status, 404)
  assert.equal(
    (await send(auth, base + '/tools', { tool: 'fulfillment.dispatchNow', input: {} })).status,
    404,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'fulfillment.listJobs',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  const createRes = await send(auth, base + '/commands', createJobCmd)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.jobId)
  assert.equal(receipt.status, 'draft')
  assert.equal(receipt.revision, 1)
  assert.equal((await send(auth, base + '/jobs/' + receipt.jobId)).status, 200)
  assert.equal((await send(auth, base + '/custody/' + receipt.jobId)).status, 200)
  assert.equal((await send(auth, base + '/lines/' + receipt.jobId)).status, 200)
  assert.equal((await send(auth, base + '/findings/' + receipt.jobId)).status, 200)
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal and fulfillment tool boundaries',
  )

  // Phase 4: a restart must retain state, memberships and the idempotent receipt.
  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createJobCmd.commandId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'recorded')
  assert.deepEqual(statusJson.receipt, receipt)
  const replayed = await send(auth, base + '/commands', createJobCmd)
  assert.equal(replayed.status, 200)
  assert.equal((await replayed.json()).idempotencyReplay, true)
  const readBack = await signIn(viewer)
  assert.equal((await send(readBack, base + '/access')).status, 200)
  assert.equal((await send(readBack, base + '/jobs/' + receipt.jobId)).status, 200)
  check('host restart retains fulfillment state, memberships and idempotent receipt recovery')

  // Phase 5: isolated Chromium drives the dashboard through the K20 workflow.
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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>180)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-fulfillment-status]")?.textContent));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#fulfillment")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  const clickButton = (scope, label) =>
    evaluate(
      `Array.from(document.querySelectorAll(${JSON.stringify(scope)})).find((b)=>b.textContent===${JSON.stringify(label)}).click()`,
    )

  await wait('!!document.querySelector("#login")')
  await fill('#login [name=username]', owner.username)
  await fill('#login [name=password]', owner.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!document.querySelector("#workspace").hidden && document.querySelector("#status").textContent===""',
  )
  await evaluate(
    'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
  )
  await wait('!document.querySelector("#fulfillment").hidden')
  await idle()
  assert.equal(await evaluate('document.querySelector("[data-fulfillment-recovery]").hidden'), true)

  await fill('#fulfillment [data-fulfillment-create] [name=jobCode]', 'SHP-UI-0001')
  await fill('#fulfillment [data-fulfillment-create] [name=title]', 'Browser dispatch run')
  await fill('#fulfillment [data-fulfillment-create] [name=ownerRef]', 'client-ui')
  await clickButton('#fulfillment [data-fulfillment-create] button', 'Create Job')
  await idle()
  assert.equal(
    (await evaluate('document.querySelector("[data-fulfillment-status]").textContent')).startsWith(
      'Command committed: create_job',
    ),
    true,
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-fulfillment-list]").textContent.includes("SHP-UI-0001")',
    ),
    true,
  )
  await evaluate(`
    Array.from(document.querySelectorAll('[data-fulfillment-list] li'))
      .find((li) => li.textContent.includes('SHP-UI-0001'))
      .querySelector('button').click()
  `)
  await wait('!!document.querySelector("#fulfillment [data-fulfillment-detail] h3")')
  await idle()
  assert.equal(
    await evaluate(
      'document.querySelector("#fulfillment [data-fulfillment-detail] h3").textContent.includes("Browser dispatch run")',
    ),
    true,
  )
  assert.equal(
    await evaluate('!!document.querySelector("#fulfillment [data-fulfillment-custody]")'),
    true,
  )
  await clickButton('#fulfillment [data-fulfillment-custody] button', 'Record Custody')
  await idle()
  await wait('!!document.querySelector("[data-fulfillment-custody-list] li")')
  assert.equal(
    await evaluate(
      'document.querySelector("[data-fulfillment-custody-list] li").textContent.includes("RECEIVED from warehouse-bay-3 to carrier-nusantara")',
    ),
    true,
  )
  assert.equal(
    await evaluate(
      '(()=>{const t=document.querySelector("[data-fulfillment-detail]").textContent;return t.includes("Owner / Client")&&t.includes("client-ui")})()',
    ),
    true,
  )
  check('browser DOM dashboard completes the receive-and-inspect fulfillment workflow')

  // Every S1 failure mode must leave the request in a recoverable uncertain state.
  const mode = (value) => evaluate(`window.__ffMode=${JSON.stringify(value)}`)
  await evaluate(`
    window.__ffMode = 'normal'
    const realFetch = window.fetch.bind(window)
    window.fetch = async (input, init) => {
      const url = String(input)
      const method = (init && init.method) || 'GET'
      const current = window.__ffMode
      if (current === 'network') throw new TypeError('simulated network interruption')
      if (current === 'http500' && method === 'POST')
        return new Response('', { status: 500 })
      if (current === 'garbage' && method === 'POST')
        return new Response('not-json{', { status: 200, headers: { 'content-type': 'application/json' } })
      if (current === 'mismatch' && method === 'POST')
        return new Response(JSON.stringify({
          commandId: '00000000-0000-4000-8000-000000000000',
          tenantId: 'studio-demo', action: 'create_job',
          jobId: '11111111-1111-4111-8111-111111111111',
          status: 'draft', revision: 1, executedAt: new Date().toISOString(),
        }), { status: 200, headers: { 'content-type': 'application/json' } })
      if (current === 'requery_down' && url.includes('/commands/'))
        throw new TypeError('simulated outage during recovery')
      return realFetch(input, init)
    }
  `)
  const recoveryVisible = () =>
    evaluate('!document.querySelector("[data-fulfillment-recovery]").hidden')
  const statusText = () =>
    evaluate('document.querySelector("[data-fulfillment-status]").textContent')
  const recovery = '#fulfillment [data-fulfillment-recovery] button'

  await fill('#fulfillment [data-fulfillment-create] [name=jobCode]', 'SHP-UI-S1')
  await mode('network')
  await clickButton('#fulfillment [data-fulfillment-create] button', 'Create Job')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Network interrupted while executing command/)

  await mode('http500')
  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Command failed: HTTP 500/)

  await mode('garbage')
  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Invalid or truncated command receipt/)

  await mode('mismatch')
  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Invalid or truncated command receipt/)

  await mode('requery_down')
  await clickButton(recovery, 'Re-query status')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Network interrupted while executing recovery query/)

  await mode('normal')
  await clickButton(recovery, 'Re-query status')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Command status is still unknown on the server/)

  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await evaluate('document.querySelector("[data-fulfillment-recovery]").hidden'), true)
  assert.match(await statusText(), /Command committed: create_job/)
  assert.equal(
    await evaluate(
      'document.querySelector("[data-fulfillment-list]").textContent.includes("SHP-UI-S1")',
    ),
    true,
  )
  check('browser DOM dashboard survives all 5 S1 failure modes through idempotent receipt recovery')

  const destination = join(root, 'dist/fulfillment-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'bounded dispatch and made-to-order work order workflow with resilient receipt recovery',
        checks,
        browser: 'isolated Chromium',
        providerCalls: 0,
        operationalKitsAccepted: [],
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(`\nFulfillment host proof verified with ${checks.length} checks\n`)
} finally {
  await page?.close()
  if (browser && browser.exitCode === null) browser.kill('SIGTERM')
  await stop()
  await rm(home, { recursive: true, force: true })
}
