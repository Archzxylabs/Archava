/* global process, fetch, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, writeFile, readFile, rm, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { createServer } from 'node:net'
import { randomBytes, randomUUID } from 'node:crypto'

const root = resolve(import.meta.dirname, '../..'),
  runtime = join(root, 'dist/crm-runtime')
const home = await mkdtemp(join(tmpdir(), 'archava-scheduling-host-'))
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
  assert.ok(ready, 'isolated project host did not start')
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
const base = '/api/scheduling/studio-demo'
const command = (value) => ({ requestId: randomUUID(), command: value })
try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8'))
  const viewer = {
    username: 'viewer@scheduling.local',
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
    scheduling: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/resources')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/resources')).status, 403)
  check('scheduling requires login; enabling module never grants domain access automatically')
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'scheduling',
      await privateFile(access, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer),
    create = command({
      operation: 'resource.create',
      title: 'Host consultant',
      kind: 'staff',
      timezone: 'Asia/Jakarta',
    })
  assert.equal(
    (await send(auth, base + '/commands', create, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', create, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(auth, '/api/scheduling/unconfigured/resources')).status, 404)
  assert.equal((await send(v, base + '/commands', create)).status, 403)
  check(
    'host enforces current domain permissions, CSRF, same origin and configured tenant boundary',
  )
  const response = await send(auth, base + '/commands', create)
  assert.equal(response.status, 200)
  const receipt = await response.json()
  assert.equal(
    (await send(v, base + '/tools', { name: 'scheduling.resources', input: {} })).status,
    200,
  )
  assert.equal(
    (
      await send(v, base + '/tools', {
        name: 'scheduling.resources',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        name: 'scheduling.hold',
        input: { resourceId: receipt.id },
      })
    ).status,
    404,
  )
  assert.equal((await send(v, base + '/resources/' + receipt.id)).status, 200)
  assert.equal((await send(auth, '/api/scheduling/hotel-demo/resources/' + receipt.id)).status, 403)
  check('private read tools refuse tenant overrides and unregistered machine writes')
  await stop()
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  check('actual owner host restart retains resources and immutable command receipts')
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
  for (let i = 0; i < 60; i++) {
    await new Promise((done) => setTimeout(done, 250))
    try {
      if ((await fetch('http://127.0.0.1:' + port + '/json')).ok) {
        browserReady = true
        break
      }
    } catch {
      /* isolated browser starting */
    }
  }
  assert.ok(browserReady, 'isolated browser did not start')
  page = await Session.open()
  await page.navigate(origin)
  const evaluate = (expr) => page.evaluate(expr)
  const wait = (condition) =>
    evaluate(
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Scheduling UI timed out'));else setTimeout(tick,100)};tick()})`,
    )
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#scheduling").hidden && document.querySelector("#scheduling").textContent===""',
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
    '!!document.querySelector("[data-resource-create]") && document.querySelector("[data-scheduling-status]").textContent===""',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("#pipeline").hidden && document.querySelector("#conversation").hidden && !document.querySelector("#scheduling").hidden',
    ),
    true,
  )
  await fill('[data-resource-create] [name=title]', 'Browser resource <img src=x onerror=alert(1)>')
  await fill('[data-resource-create] [name=timezone]', 'Asia/Jakarta')
  await evaluate("document.querySelector('[data-resource-create]').requestSubmit()")
  await wait(
    'document.querySelector("[data-resource-list]").textContent.includes("Browser resource") && document.querySelector("[data-scheduling-status]").textContent===""',
  )
  const resources = await (await send(auth, base + '/resources')).json(),
    browserResource = resources.items.find((x) => x.title.startsWith('Browser resource'))
  assert.ok(browserResource)
  assert.equal(
    await evaluate('document.querySelector("[data-resource-list]").querySelector("img")===null'),
    true,
  )
  const startsAt = new Date(Date.now() + 24 * 3_600_000).toISOString(),
    endsAt = new Date(Date.now() + 25 * 3_600_000).toISOString()
  await fill('[data-reservation-create] [name=title]', 'Browser consultation')
  await fill('[data-reservation-create] [name=startsAt]', startsAt)
  await fill('[data-reservation-create] [name=endsAt]', endsAt)
  await evaluate(
    `window.scheduleFetch=window.fetch;window.scheduleDrop=true;window.scheduleRetryBlocked=false;window.fetch=async(...args)=>{if(window.scheduleRetryBlocked&&String(args[0]).includes('/api/scheduling/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.scheduleFetch(...args);if(window.scheduleDrop&&String(args[0]).includes('/api/scheduling/')&&args[1]?.method==='POST'){window.scheduleDrop=false;throw new Error('Lost scheduling response')}return response};document.querySelector('[data-reservation-create]').requestSubmit()`,
  )
  await wait(
    'document.querySelector("[data-scheduling-status]").textContent.includes("Lost scheduling response")',
  )
  await evaluate(
    "window.scheduleRetryBlocked=true;Array.from(document.querySelectorAll('#scheduling button')).find(x=>x.textContent==='Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-scheduling-status]").textContent.includes("permissions")',
  )
  await evaluate("document.querySelector('[data-reservation-create]').requestSubmit()")
  await wait(
    'document.querySelector("[data-scheduling-status]").textContent.includes("Resolve the pending")',
  )
  await evaluate(
    "window.scheduleRetryBlocked=false;Array.from(document.querySelectorAll('#scheduling button')).find(x=>x.textContent==='Check pending result').click()",
  )
  await wait(
    'document.querySelector("[data-reservation-list]").textContent.includes("Browser consultation") && document.querySelector("[data-scheduling-status]").textContent===""',
  )
  await evaluate('window.fetch=window.scheduleFetch;delete window.scheduleFetch')
  let reservations = await (
    await send(auth, base + '/reservations?resourceId=' + browserResource.id)
  ).json()
  assert.equal(reservations.items.length, 1)
  check(
    'browser recovers committed response loss and retains pending request after refused retry without duplicates or HTML injection',
  )
  const click = async (text) => {
    await evaluate(
      `Array.from(document.querySelectorAll('#scheduling button')).find(x=>x.textContent===${JSON.stringify(text)}).click()`,
    )
    await wait('document.querySelector("[data-scheduling-status]").textContent!=="Working…"')
  }
  await click('Confirm reservation')
  await wait('document.querySelector("[data-reservation-list]").textContent.includes("confirmed")')
  await fill('[data-reservation-create] [name=title]', 'Conflicting browser booking')
  await fill('[data-reservation-create] [name=startsAt]', startsAt)
  await fill('[data-reservation-create] [name=endsAt]', endsAt)
  await evaluate("document.querySelector('[data-reservation-create]').requestSubmit()")
  await wait(
    'document.querySelector("[data-scheduling-status]").textContent.includes("unavailable")',
  )
  assert.equal(
    (await (await send(auth, base + '/reservations?resourceId=' + browserResource.id)).json()).items
      .length,
    1,
  )
  const moveFrom = new Date(Date.now() + 26 * 3_600_000).toISOString(),
    moveTo = new Date(Date.now() + 27 * 3_600_000).toISOString()
  await evaluate(
    `const move=Array.from(document.querySelectorAll('[data-reservation-list] form')).find(x=>x.textContent.includes('Move reservation'));move.querySelector('[name=startsAt]').value=${JSON.stringify(moveFrom)};move.querySelector('[name=endsAt]').value=${JSON.stringify(moveTo)};move.querySelector('[name=reason]').value='Appointment changed';move.requestSubmit()`,
  )
  await wait('document.querySelector("[data-scheduling-status]").textContent===""')
  reservations = await (
    await send(auth, base + '/reservations?resourceId=' + browserResource.id)
  ).json()
  assert.equal(reservations.items[0].startsAt, moveFrom)
  assert.equal(reservations.items[0].revision, 3)
  await evaluate(
    "const cancel=Array.from(document.querySelectorAll('[data-reservation-list] form')).find(x=>x.textContent.includes('Cancel reservation'));cancel.querySelector('[name=reason]').value='Customer cancelled';cancel.requestSubmit()",
  )
  await wait(
    'document.querySelector("[data-reservation-list]").textContent.includes("cancelled") && document.querySelector("[data-scheduling-status]").textContent===""',
  )
  await click('Disable resource')
  assert.equal(await evaluate('document.querySelector("[data-reservation-create]").hidden'), true)
  await click('Enable resource')
  check(
    'browser completes hold, confirm, move, cancel and resource toggle while backend refuses conflicting bookings',
  )
  await evaluate('document.querySelector("#logout").click()')
  await wait(
    'document.querySelector("#scheduling").textContent==="" && !document.querySelector("#login").hidden',
  )
  await fill('#login [name=username]', viewer.username)
  await fill('#login [name=password]', viewer.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!!document.querySelector("[data-resource-create]") && document.querySelector("[data-scheduling-status]").textContent===""',
  )
  assert.equal(await evaluate('document.querySelector("[data-resource-create]").hidden'), true)
  await evaluate(
    "Array.from(document.querySelectorAll('[data-resource-list] button')).find(x=>x.textContent.includes('Browser resource')).click()",
  )
  await wait(
    'document.querySelector("[data-reservation-list]").textContent.includes("Browser consultation")',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-reservation-create]").hidden && document.querySelectorAll("[data-reservation-list] form").length===0',
    ),
    true,
  )
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check(
    'logout clears reservation data; viewer panel hides write controls without browser exceptions',
  )
  await page.close()
  page = null
  const browserStopped = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await browserStopped
  browser = null
  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, scheduling: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/resources')).status, 404)
  await stop()
  await operator(['modules', await privateFile('reenabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  assert.equal(
    (await (await send(auth, base + '/reservations?resourceId=' + browserResource.id)).json())
      .items[0].state,
    'cancelled',
  )
  check('disable and re-enable retain resources, reservations, grants, audit and receipts')
  await stop()
  const archive = join(home, 'host-scheduling.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_scheduling'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  const recovered = { username: owner.username, password: randomBytes(24).toString('base64url') }
  await operator(['user', 'recover', await privateFile('recovered', recovered)])
  await start()
  auth = await signIn(recovered)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  assert.equal(
    (await (await send(auth, base + '/reservations?resourceId=' + browserResource.id)).json())
      .items[0].state,
    'cancelled',
  )
  check(
    'actual host backup restore retains reservation grants and receipts after explicit identity recovery',
  )
  const destination = join(root, 'dist/scheduling-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'exclusive resource reservations',
        checks,
        browser: 'isolated Chromium',
        providerCalls: 0,
        operationalKitsAccepted: [],
      },
      null,
      2,
    ) + '\n',
  )
} finally {
  await page?.close()
  if (browser && browser.exitCode === null) {
    const done = new Promise((resolve) => browser.once('exit', resolve))
    browser.kill('SIGTERM')
    await done
  }
  await stop()
  await rm(home, { recursive: true, force: true })
}
