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
const home = await mkdtemp(join(tmpdir(), 'archava-foodservice-host-'))
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
const base = '/api/foodservice/studio-demo'
const commerceBase = '/api/commerce/studio-demo'
const command = (value) => ({ requestId: randomUUID(), command: value })
try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@food.local',
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
    commerce: true,
    foodservice: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/menu')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'food-service requires login plus independent food and commerce grants; enabling flags never enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    for (const domain of ['commerce', 'foodservice'])
      await operator([
        'user',
        domain,
        await privateFile(domain + '-' + access, {
          username: account.username,
          tenants: ['studio-demo'],
          access,
          scope: 'all',
        }),
      ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer),
    tableCommand = command({
      operation: 'table.create',
      code: 'HOST',
      title: 'Host table',
      seats: 2,
    })
  assert.equal(
    (await send(auth, base + '/commands', tableCommand, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', tableCommand, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', tableCommand)).status, 403)
  assert.equal((await send(auth, '/api/foodservice/unconfigured/menu')).status, 404)
  const tableResponse = await send(auth, base + '/commands', tableCommand)
  assert.equal(tableResponse.status, 200)
  const tableReceipt = await tableResponse.json()
  assert.equal(
    (await send(v, base + '/tools', { name: 'foodservice.menu', input: {} })).status,
    200,
  )
  assert.equal(
    (
      await send(v, base + '/tools', {
        name: 'foodservice.tickets',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        name: 'foodservice.ticket.prepare',
        input: { id: tableReceipt.id },
      })
    ).status,
    404,
  )
  check(
    'actual host enforces session, CSRF, same origin, configured workspace and private read-only tool boundaries',
  )
  await stop()
  await start()
  auth = await signIn(owner)
  const restarted = (await (await send(auth, base + '/commands/' + tableCommand.requestId)).json())
    .receipt
  assert.deepEqual({ ...restarted, replayed: false }, tableReceipt)
  check('host restart retains dining table and original actor-bound command receipt')
  const mutate = async (path, c) => {
    const r = await send(auth, path + '/commands', command(c))
    assert.equal(r.status, 200)
    return await r.json()
  }
  const ingredient = await mutate(commerceBase, {
    operation: 'product.create',
    sku: 'RICE',
    title: 'Rice ingredient',
    unit: 'gram',
    price: 1,
  })
  await mutate(commerceBase, {
    operation: 'stock.adjust',
    productId: ingredient.id,
    expectedRevision: 1,
    quantity: 1000,
    reason: 'Ingredients delivered',
  })
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
  for (let n = 0; n < 60; n++) {
    await new Promise((r) => setTimeout(r, 250))
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
  const evaluate = (expr) => page.evaluate(expr),
    wait = (condition) =>
      evaluate(
        `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Food UI timed out: '+${JSON.stringify(condition)}));else setTimeout(tick,100)};tick()})`,
      ),
    fill = (selector, value) =>
      evaluate(
        `document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`,
      ),
    status = 'document.querySelector("[data-food-status]").textContent'
  const click = async (text) => {
    await evaluate(
      `Array.from(document.querySelectorAll('#foodservice button')).find(x=>x.textContent===${JSON.stringify(text)}).click()`,
    )
    await wait(`!Array.from(document.querySelectorAll('#foodservice button')).some(x=>x.disabled)`)
  }
  const login = async (account) => {
    await fill('#login [name=username]', account.username)
    await fill('#login [name=password]', account.password)
    await evaluate('document.querySelector("#login").requestSubmit()')
    await wait(
      '!document.querySelector("#workspace").hidden&&document.querySelector("#status").textContent===""',
    )
    await evaluate(
      'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
    )
    await wait('!!document.querySelector("[data-food-menu]")&&' + status + '==="Restaurant ready."')
  }
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#foodservice").hidden&&document.querySelector("#foodservice").textContent===""',
    ),
    true,
  )
  await login(owner)
  await fill('[data-food-table-create] [name=code]', 'BROWSER')
  await fill('[data-food-table-create] [name=title]', 'Browser table')
  await fill('[data-food-table-create] [name=seats]', '4')
  await evaluate('document.querySelector("[data-food-table-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-food-tables]").textContent.includes("Browser table")&&' +
      status +
      '==="Restaurant ready."',
  )
  await fill('[data-food-menu-create] [name=sku]', 'MEAL')
  await fill('[data-food-menu-create] [name=title]', 'Browser meal <img src=x onerror=alert(1)>')
  await fill('[data-food-menu-create] [name=price]', '25000')
  await fill('[data-food-menu-create] [name=ingredientId]', ingredient.id)
  await fill('[data-food-menu-create] [name=ingredientQuantity]', '100')
  await click('Add recipe ingredient')
  await evaluate('document.querySelector("[data-food-menu-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-food-menu]").textContent.includes("Browser meal")&&' +
      status +
      '==="Restaurant ready."',
  )
  assert.equal(
    await evaluate('document.querySelector("[data-food-menu]").querySelector("img")===null'),
    true,
  )
  const menu = (await (await send(auth, base + '/menu')).json()).items[0],
    table = (await (await send(auth, base + '/tables')).json()).items.find(
      (x) => x.code === 'BROWSER',
    )
  assert.ok(menu && table)
  assert.equal(
    (await (await send(auth, commerceBase + '/products')).json()).items.some(
      (x) => x.id === menu.id,
    ),
    false,
  )
  check(
    'Chromium creates physical tables and managed menu recipes from shared base-unit ingredients without HTML injection or duplicate catalog exposure',
  )
  await fill('[data-food-cart] [name=menuId]', menu.id)
  await fill('[data-food-cart] [name=quantity]', '2')
  await click('Add ticket line')
  await fill(
    '[data-food-ticket-create] [name=customerName]',
    'Browser guest <img src=x onerror=alert(1)>',
  )
  await fill('[data-food-ticket-create] [name=tableId]', table.id)
  await evaluate(
    "window.foodFetch=window.fetch;window.foodDrop=true;window.foodBlocked=false;window.fetch=async(...args)=>{if(window.foodBlocked&&String(args[0]).includes('/api/foodservice/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.foodFetch(...args);if(window.foodDrop&&String(args[0]).includes('/api/foodservice/')&&args[1]?.method==='POST'){window.foodDrop=false;throw new Error('Lost food response')}return response};document.querySelector('[data-food-ticket-create]').requestSubmit()",
  )
  await wait(status + '.includes("Lost food response")')
  await evaluate('window.foodBlocked=true')
  await click('Retry same request')
  await wait(status + '.includes("403")')
  await evaluate('window.foodBlocked=false')
  // A new create remains blocked until the original request is reconciled.
  await evaluate('document.querySelector("[data-food-ticket-create]").requestSubmit()')
  await wait(status + '.includes("Resolve the pending request first")')
  await click('Check pending result')
  await wait(
    'document.querySelector("[data-food-detail]").textContent.includes("draft")&&' +
      status +
      '==="Restaurant ready."',
  )
  await evaluate('window.fetch=window.foodFetch;delete window.foodFetch')
  const tickets = (await (await send(auth, base + '/tickets')).json()).items
  assert.equal(tickets.length, 1)
  const ticket = tickets[0]
  assert.equal(ticket.totalEstimate, 50000)
  assert.equal(
    await evaluate('document.querySelector("[data-food-tickets]").querySelector("img")===null'),
    true,
  )
  check(
    'browser recovers committed response loss, retains pending UUID after denied retry and blocks new effects until one original ticket is reconciled',
  )
  await click('Queue for kitchen')
  await wait(
    'document.querySelector("[data-food-detail]").textContent.includes("queued")&&' +
      status +
      '==="Restaurant ready."',
  )
  let stock = await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()
  assert.equal(stock.onHand, 1000)
  assert.equal(stock.reserved, 200)
  await click('Start preparation')
  await wait(
    'document.querySelector("[data-food-detail]").textContent.includes("preparing")&&' +
      status +
      '==="Restaurant ready."',
  )
  stock = await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()
  assert.equal(stock.onHand, 800)
  assert.equal(stock.reserved, 0)
  for (const [text, state] of [
    ['Mark ready', 'ready'],
    ['Mark served', 'served'],
    ['Close service', 'closed'],
  ]) {
    await click(text)
    await wait(
      'document.querySelector("[data-food-detail]").textContent.includes(' +
        JSON.stringify(state) +
        ')&&' +
        status +
        '==="Restaurant ready."',
    )
  }
  assert.equal(
    (await (await send(auth, base + '/tables')).json()).items.find((x) => x.id === table.id)
      .occupied,
    false,
  )
  assert.equal(
    Object.hasOwn((await (await send(auth, base + '/tickets/' + ticket.id)).json()).ticket, 'paid'),
    false,
  )
  check(
    'Chromium queues, prepares, marks ready, serves and closes; shared ingredient stock consumes once and closing service frees table without asserting payment',
  )
  // Open another actual ticket and cancel after preparation; ingredients stay consumed.
  await fill('[data-food-cart] [name=menuId]', menu.id)
  await fill('[data-food-cart] [name=quantity]', '1')
  await click('Add ticket line')
  await fill('[data-food-ticket-create] [name=customerName]', 'Waste ticket')
  await fill('[data-food-ticket-create] [name=tableId]', table.id)
  await evaluate('document.querySelector("[data-food-ticket-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-food-detail] h3").textContent==="Waste ticket"&&' +
      status +
      '==="Restaurant ready."',
  )
  await click('Queue for kitchen')
  await wait(
    'document.querySelector("[data-food-detail]").textContent.includes("queued")&&' +
      status +
      '==="Restaurant ready."',
  )
  await click('Start preparation')
  await wait(
    'document.querySelector("[data-food-detail]").textContent.includes("preparing")&&' +
      status +
      '==="Restaurant ready."',
  )
  await fill('[data-food-detail] [name=reason]', 'Prepared meal wasted')
  await evaluate('document.querySelector("[data-food-detail] form").requestSubmit()')
  await wait(
    'document.querySelector("[data-food-detail]").textContent.includes("cancelled")&&' +
      status +
      '==="Restaurant ready."',
  )
  assert.equal(
    (await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()).onHand,
    700,
  )
  check(
    'browser cancellation after cooking records waste and preserves consumed ingredients instead of manufacturing a restock or refund',
  )
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#foodservice").textContent===""')
  await login(viewer)
  assert.equal(
    await evaluate(
      'document.querySelector("[data-food-table-create]").hidden&&document.querySelector("[data-food-menu-create]").hidden&&document.querySelector("[data-food-ticket-create]").hidden',
    ),
    true,
  )
  await click('Open ticket')
  assert.equal(
    await evaluate(
      'document.querySelector("[data-food-detail]").querySelectorAll("form").length===0',
    ),
    true,
  )
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check(
    'logout clears restaurant data and viewer dashboard hides all write controls without browser exceptions',
  )
  await page.close()
  page = null
  const browserStopped = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await browserStopped
  browser = null
  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, foodservice: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/menu')).status, 404)
  await stop()
  await operator(['modules', await privateFile('enabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.equal(
    (await (await send(auth, base + '/tickets/' + ticket.id)).json()).ticket.state,
    'closed',
  )
  assert.equal(
    (await (await send(auth, base + '/commands/' + tableCommand.requestId)).json()).state,
    'committed',
  )
  check(
    'disable and re-enable remove restaurant routes while retaining recipes, tickets, stock truth, grants and original receipts',
  )
  await stop()
  const archive = join(home, 'food.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_food'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  const recovered = { username: owner.username, password: randomBytes(24).toString('base64url') }
  await operator(['user', 'recover', await privateFile('recovered', recovered)])
  await start()
  auth = await signIn(recovered)
  assert.equal(
    (await (await send(auth, base + '/tickets/' + ticket.id)).json()).ticket.totalEstimate,
    50000,
  )
  assert.equal(
    (await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()).onHand,
    700,
  )
  assert.equal(
    (await (await send(auth, base + '/commands/' + tableCommand.requestId)).json()).state,
    'committed',
  )
  check(
    'actual owner-host backup restore retains recipes, original quotes, physical service state, ingredient ledger and receipt recovery after explicit identity recovery',
  )
  const destination = join(root, 'dist/foodservice-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded restaurant tables, recipes and whole-ticket kitchen/service workflow',
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
