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
const home = await mkdtemp(join(tmpdir(), 'archava-accommodation-host-'))
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
const base = '/api/accommodation/studio-demo'
const command = (value) => ({ requestId: randomUUID(), command: value })
try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8'))
  const viewer = {
    username: 'viewer@accommodation.local',
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
    accommodation: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/properties')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/properties')).status, 403)
  check('hotel module requires login and explicit grants; enabling it never grants domain access')
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'accommodation',
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
      operation: 'property.create',
      title: 'Owner host hotel',
      timezone: 'UTC',
      checkInTime: '00:00',
      checkOutTime: '00:00',
    })
  assert.equal(
    (await send(auth, base + '/commands', create, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', create, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', create)).status, 403)
  assert.equal((await send(auth, '/api/accommodation/unconfigured/properties')).status, 404)
  const result = await send(auth, base + '/commands', create)
  assert.equal(result.status, 200)
  const receipt = await result.json()
  assert.equal(
    (await send(v, base + '/tools', { name: 'accommodation.properties', input: {} })).status,
    200,
  )
  assert.equal(
    (
      await send(v, base + '/tools', {
        name: 'accommodation.properties',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        name: 'accommodation.check-in',
        input: { id: receipt.id },
      })
    ).status,
    404,
  )
  assert.equal(
    (await send(auth, '/api/accommodation/hotel-demo/properties/' + receipt.id)).status,
    403,
  )
  check('actual host enforces CSRF, same origin, configured tenant and read-only tool allowlist')
  await stop()
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  check('owner host restart retains property and actor-bound receipt')
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
  let ready = false
  for (let i = 0; i < 60; i++) {
    await new Promise((done) => setTimeout(done, 250))
    try {
      if ((await fetch('http://127.0.0.1:' + port + '/json')).ok) {
        ready = true
        break
      }
    } catch {
      /* browser starting */
    }
  }
  assert.ok(ready)
  page = await Session.open()
  await page.navigate(origin)
  const evaluate = (expr) => page.evaluate(expr)
  const wait = (condition) =>
    evaluate(
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Hotel UI timed out: '+${JSON.stringify(condition)}));else setTimeout(tick,100)};tick()})`,
    )
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  const status = 'document.querySelector("[data-accommodation-status]").textContent'
  const click = async (text) => {
    await evaluate(
      `Array.from(document.querySelectorAll('#accommodation button')).find(x=>x.textContent===${JSON.stringify(text)}).click()`,
    )
    await wait(status + '!=="Working…"')
  }
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#accommodation").hidden&&document.querySelector("#accommodation").textContent===""',
    ),
    true,
  )
  await fill('#login [name=username]', owner.username)
  await fill('#login [name=password]', owner.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!document.querySelector("#workspace").hidden&&document.querySelector("#status").textContent===""',
  )
  await evaluate(
    'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
  )
  await wait('!!document.querySelector("[data-property-create]")&&' + status + '===""')
  await fill('[data-property-create] [name=title]', 'Browser hotel <img src=x onerror=alert(1)>')
  await fill('[data-property-create] [name=timezone]', 'UTC')
  await fill('[data-property-create] [name=checkInTime]', '00:00')
  await fill('[data-property-create] [name=checkOutTime]', '00:00')
  await evaluate('document.querySelector("[data-property-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-property-list]").textContent.includes("Browser hotel")&&' +
      status +
      '===""',
  )
  assert.equal(
    await evaluate('document.querySelector("[data-property-list]").querySelector("img")===null'),
    true,
  )
  const properties = await (await send(auth, base + '/properties')).json(),
    p = properties.items.find((x) => x.title.startsWith('Browser hotel'))
  assert.ok(p)
  await fill('[data-room-type-create] [name=title]', 'Double')
  await fill('[data-room-type-create] [name=nightlyRate]', '750000')
  await evaluate('document.querySelector("[data-room-type-create]").requestSubmit()')
  await wait(
    '!!document.querySelector("[data-hotel-room-create] select option")&&' + status + '===""',
  )
  await fill('[data-hotel-room-create] [name=code]', '101')
  await evaluate('document.querySelector("[data-hotel-room-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-hotel-room-list]").textContent.includes("101")&&' +
      status +
      '===""',
  )
  await click('Mark room clean')
  await wait(
    'document.querySelector("[data-hotel-room-detail]").textContent.includes("clean")&&' +
      status +
      '===""',
  )
  await click('Inspect room')
  await wait(
    'document.querySelector("[data-hotel-room-detail]").textContent.includes("inspected")&&' +
      status +
      '===""',
  )
  check(
    'Chromium creates property, room type and room and completes clean then inspect without HTML injection',
  )
  const rooms = await (await send(auth, base + '/rooms?propertyId=' + p.id)).json(),
    room = rooms.items[0]
  const arrival = new Date().toISOString().slice(0, 10),
    departure = new Date(Date.now() + 86400000).toISOString().slice(0, 10)
  await fill('[data-stay-create] [name=guestName]', 'Browser guest <img src=x onerror=alert(1)>')
  await fill('[data-stay-create] [name=arrival]', arrival)
  await fill('[data-stay-create] [name=departure]', departure)
  await evaluate(
    `window.hotelFetch=window.fetch;window.hotelDrop=true;window.hotelRetryBlocked=false;window.fetch=async(...args)=>{if(window.hotelRetryBlocked&&String(args[0]).includes('/api/accommodation/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.hotelFetch(...args);if(window.hotelDrop&&String(args[0]).includes('/api/accommodation/')&&args[1]?.method==='POST'){window.hotelDrop=false;throw new Error('Lost hotel response')}return response};document.querySelector('[data-stay-create]').requestSubmit()`,
  )
  await wait(status + '.includes("Lost hotel response")')
  await evaluate('window.hotelRetryBlocked=true')
  await click('Retry same request')
  await wait(status + '.includes("permissions")')
  await evaluate('document.querySelector("[data-stay-create]").requestSubmit()')
  await wait(status + '.includes("Resolve the pending")')
  await evaluate('window.hotelRetryBlocked=false')
  await click('Check pending result')
  await wait(
    'document.querySelector("[data-stay-list]").textContent.includes("Browser guest")&&' +
      status +
      '===""',
  )
  await evaluate('window.fetch=window.hotelFetch;delete window.hotelFetch')
  let stays = await (await send(auth, base + '/stays?propertyId=' + p.id)).json()
  assert.equal(stays.items.length, 1)
  assert.equal(
    await evaluate('document.querySelector("[data-stay-list]").querySelector("img")===null'),
    true,
  )
  const guest = stays.items[0]
  const detail = await (await send(auth, base + '/stays/' + guest.id)).json()
  assert.equal(detail.nights.length, 1)
  assert.equal(detail.stay.totalEstimate, 750000)
  check(
    'browser recovers committed response loss, retains pending UUID after refused retry and creates no duplicate stay',
  )
  await click('Confirm stay')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("confirmed")&&' +
      status +
      '===""',
  )
  await click('Check in guest')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("in_house")&&' +
      status +
      '===""',
  )
  assert.equal((await (await send(auth, base + '/rooms/' + room.id)).json()).occupied, true)
  await click('Check out guest')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("checked_out")&&' +
      status +
      '===""',
  )
  assert.equal((await (await send(auth, base + '/rooms/' + room.id)).json()).housekeeping, 'dirty')
  await fill('[data-stay-create] [name=guestName]', 'Next guest')
  await evaluate('document.querySelector("[data-stay-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("Next guest")&&' +
      status +
      '===""',
  )
  await click('Confirm stay')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("confirmed")&&' +
      status +
      '===""',
  )
  await click('Check in guest')
  await wait(status + '.includes("required readiness")')
  await click('Mark room clean')
  await wait(status + '===""')
  await click('Inspect room')
  await wait(status + '===""')
  await click('Check in guest')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("in_house")&&' +
      status +
      '===""',
  )
  await click('Check out guest')
  await wait(
    'document.querySelector("[data-stay-detail]").textContent.includes("checked_out")&&' +
      status +
      '===""',
  )
  check(
    'browser completes stay lifecycle; next guest cannot check in after checkout until room is cleaned and inspected',
  )
  await evaluate('document.querySelector("#logout").click()')
  await wait(
    'document.querySelector("#accommodation").textContent===""&&!document.querySelector("#login").hidden',
  )
  await fill('#login [name=username]', viewer.username)
  await fill('#login [name=password]', viewer.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait('!!document.querySelector("[data-property-create]")&&' + status + '===""')
  assert.equal(await evaluate('document.querySelector("[data-property-create]").hidden'), true)
  await evaluate(
    "Array.from(document.querySelectorAll('[data-property-list] button')).find(x=>x.textContent.includes('Browser hotel')).click()",
  )
  await wait(
    'document.querySelector("[data-stay-list]").textContent.includes("Browser guest")&&' +
      status +
      '===""',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-room-type-create]").hidden&&document.querySelector("[data-hotel-room-create]").hidden&&document.querySelector("[data-stay-create]").hidden',
    ),
    true,
  )
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check('logout clears guest data and viewer panel hides write controls without browser exceptions')
  await page.close()
  page = null
  const browserStopped = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await browserStopped
  browser = null
  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, accommodation: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/properties')).status, 404)
  await stop()
  await operator(['modules', await privateFile('enabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  stays = await (await send(auth, base + '/stays?propertyId=' + p.id)).json()
  assert.equal(stays.items.length, 2)
  assert.ok(stays.items.every((x) => x.state === 'checked_out'))
  check('disabling and re-enabling hotel retains rooms, stays, grants, audit and receipts')
  await stop()
  const archive = join(home, 'hotel.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_hotel'])
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
    (await (await send(auth, base + '/stays/' + guest.id)).json()).stay.totalEstimate,
    750000,
  )
  assert.equal((await (await send(auth, base + '/rooms/' + room.id)).json()).housekeeping, 'dirty')
  check(
    'actual host backup restore retains rooms, quote, domain grants and receipts after explicit identity recovery',
  )
  const destination = join(root, 'dist/accommodation-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded room-night stays and housekeeping',
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
