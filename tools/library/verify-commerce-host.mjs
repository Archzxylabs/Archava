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
const home = await mkdtemp(join(tmpdir(), 'archava-commerce-host-'))
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
const base = '/api/commerce/studio-demo'
const command = (value) => ({ requestId: randomUUID(), command: value })
try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@commerce.local',
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
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/products')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/products')).status, 403)
  check(
    'commerce requires login and explicit domain grants; enabling the module never enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'commerce',
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
      operation: 'product.create',
      sku: 'HOST',
      title: 'Host product',
      price: 1000,
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
  assert.equal((await send(auth, '/api/commerce/unconfigured/products')).status, 404)
  const response = await send(auth, base + '/commands', create)
  assert.equal(response.status, 200)
  const receipt = await response.json()
  assert.equal(
    (await send(v, base + '/tools', { name: 'commerce.products', input: {} })).status,
    200,
  )
  assert.equal(
    (await send(v, base + '/tools', { name: 'commerce.products', input: { tenant: 'hotel-demo' } }))
      .status,
    400,
  )
  assert.equal(
    (await send(auth, base + '/tools', { name: 'commerce.fulfill', input: { id: receipt.id } }))
      .status,
    404,
  )
  assert.equal((await send(auth, '/api/commerce/hotel-demo/products/' + receipt.id)).status, 403)
  check(
    'actual host enforces session, CSRF, same origin, configured tenant and private read-only tool boundaries',
  )
  await stop()
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  check('owner host restart retains catalog and immutable actor-bound receipt')
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
  const evaluate = (expr) => page.evaluate(expr),
    wait = (condition) =>
      evaluate(
        `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Commerce UI timed out: '+${JSON.stringify(condition)}));else setTimeout(tick,100)};tick()})`,
      )
  const fill = (selector, value) =>
      evaluate(
        `document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`,
      ),
    status = 'document.querySelector("[data-commerce-status]").textContent'
  const click = async (text) => {
    await evaluate(
      `Array.from(document.querySelectorAll('#commerce button')).find(x=>x.textContent===${JSON.stringify(text)}).click()`,
    )
    await wait(status + '!=="Working…"')
  }
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#commerce").hidden&&document.querySelector("#commerce").textContent===""',
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
  await wait('!!document.querySelector("[data-commerce-product-create]")&&' + status + '===""')
  await fill('[data-commerce-product-create] [name=sku]', 'BROWSER')
  await fill(
    '[data-commerce-product-create] [name=title]',
    'Browser coffee <img src=x onerror=alert(1)>',
  )
  await fill('[data-commerce-product-create] [name=price]', '7500')
  await evaluate('document.querySelector("[data-commerce-product-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-commerce-product-list]").textContent.includes("Browser coffee")&&' +
      status +
      '===""',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-commerce-product-list]").querySelector("img")===null',
    ),
    true,
  )
  const products = await (await send(auth, base + '/products')).json(),
    product = products.items.find((x) => x.sku === 'BROWSER')
  assert.ok(product)
  await fill('[data-commerce-stock-adjust] [name=quantity]', '5')
  await fill('[data-commerce-stock-adjust] [name=reason]', 'Received five physical units')
  await evaluate('document.querySelector("[data-commerce-stock-adjust]").requestSubmit()')
  await wait(
    'document.querySelector("[data-commerce-product-detail]").textContent.includes("On hand: 5")&&' +
      status +
      '===""',
  )
  check(
    'Chromium creates catalog and receives physical stock through audited adjustment without HTML injection',
  )
  await fill('[data-commerce-cart] [name=productId]', product.id)
  await fill('[data-commerce-cart] [name=quantity]', '2')
  await click('Add order line')
  await fill(
    '[data-commerce-order-create] [name=customerName]',
    'Browser customer <img src=x onerror=alert(1)>',
  )
  await evaluate(
    `window.commerceFetch=window.fetch;window.commerceDrop=true;window.commerceRetryBlocked=false;window.fetch=async(...args)=>{if(window.commerceRetryBlocked&&String(args[0]).includes('/api/commerce/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.commerceFetch(...args);if(window.commerceDrop&&String(args[0]).includes('/api/commerce/')&&args[1]?.method==='POST'){window.commerceDrop=false;throw new Error('Lost commerce response')}return response};document.querySelector('[data-commerce-order-create]').requestSubmit()`,
  )
  await wait(status + '.includes("Lost commerce response")')
  await evaluate('window.commerceRetryBlocked=true')
  await click('Retry same request')
  await wait(status + '.includes("permissions")')
  await evaluate('document.querySelector("[data-commerce-order-create]").requestSubmit()')
  await wait(status + '.includes("Resolve the pending")')
  await evaluate('window.commerceRetryBlocked=false')
  await click('Check pending result')
  await wait(
    'document.querySelector("[data-commerce-order-list]").textContent.includes("Browser customer")&&' +
      status +
      '===""',
  )
  await evaluate('window.fetch=window.commerceFetch;delete window.commerceFetch')
  let orders = await (await send(auth, base + '/orders')).json()
  assert.equal(orders.items.length, 1)
  const order = orders.items[0]
  assert.equal(order.totalEstimate, 15000)
  assert.equal(
    await evaluate(
      'document.querySelector("[data-commerce-order-list]").querySelector("img")===null',
    ),
    true,
  )
  check(
    'browser recovers committed response loss and retains pending UUID after denied retry without duplicate orders',
  )
  await click('Reserve stock')
  await wait(
    'document.querySelector("[data-commerce-order-detail]").textContent.includes("reserved")&&' +
      status +
      '===""',
  )
  let stock = await (await send(auth, base + '/stock/' + product.id)).json()
  assert.equal(stock.onHand, 5)
  assert.equal(stock.reserved, 2)
  assert.equal(stock.available, 3)
  const negative = command({
    operation: 'stock.adjust',
    productId: product.id,
    expectedRevision: stock.revision,
    quantity: -4,
    reason: 'Cannot remove reserved units',
  })
  assert.equal((await send(auth, base + '/commands', negative)).status, 409)
  await click('Fulfill order')
  await wait(
    'document.querySelector("[data-commerce-order-detail]").textContent.includes("fulfilled")&&' +
      status +
      '===""',
  )
  stock = await (await send(auth, base + '/stock/' + product.id)).json()
  assert.equal(stock.onHand, 3)
  assert.equal(stock.reserved, 0)
  check(
    'browser reserves and fulfills stock; backend refuses reductions below held quantities and never marks payment',
  )
  let detail = await (await send(auth, base + '/orders/' + order.id)).json()
  const line = detail.lines[0]
  await fill('[data-commerce-return] [name="' + line.id + '"]', '1')
  await fill('[data-commerce-return] [name=reason]', 'Received one sealed item')
  await evaluate('document.querySelector("[data-commerce-return]").requestSubmit()')
  await wait(
    'document.querySelector("[data-commerce-order-detail]").textContent.includes("Returned 1")&&' +
      status +
      '===""',
  )
  stock = await (await send(auth, base + '/stock/' + product.id)).json()
  assert.equal(stock.onHand, 4)
  await fill('[data-commerce-return] [name="' + line.id + '"]', '1')
  await fill('[data-commerce-return] [name="disposition-' + line.id + '"]', 'discard')
  await fill('[data-commerce-return] [name=reason]', 'Received one damaged item')
  await evaluate('document.querySelector("[data-commerce-return]").requestSubmit()')
  await wait(
    'document.querySelector("[data-commerce-order-detail]").textContent.includes("returned")&&' +
      status +
      '===""',
  )
  detail = await (await send(auth, base + '/orders/' + order.id)).json()
  assert.equal(detail.order.state, 'returned')
  assert.equal(detail.order.totalEstimate, 15000)
  assert.equal(detail.lines[0].returnedQuantity, 2)
  assert.equal((await (await send(auth, base + '/stock/' + product.id)).json()).onHand, 4)
  await fill('[data-commerce-price] [name=price]', '9000')
  await fill('[data-commerce-price] [name=reason]', 'New future order price')
  await evaluate('document.querySelector("[data-commerce-price]").requestSubmit()')
  await wait(status + '===""')
  assert.equal((await (await send(auth, base + '/products/' + product.id)).json()).price, 9000)
  assert.equal(
    (await (await send(auth, base + '/orders/' + order.id)).json()).order.totalEstimate,
    15000,
  )
  await click('Disable product')
  await wait(status + '===""')
  await click('Enable product')
  await wait(status + '===""')
  check(
    'browser records partial restock and discarded returns; rate changes preserve original order quote and product toggles retain stock',
  )
  await evaluate('document.querySelector("#logout").click()')
  await wait(
    'document.querySelector("#commerce").textContent===""&&!document.querySelector("#login").hidden',
  )
  await fill('#login [name=username]', viewer.username)
  await fill('#login [name=password]', viewer.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait('!!document.querySelector("[data-commerce-product-create]")&&' + status + '===""')
  assert.equal(
    await evaluate(
      'document.querySelector("[data-commerce-product-create]").hidden&&document.querySelector("[data-commerce-cart]").hidden&&document.querySelector("[data-commerce-order-create]").hidden',
    ),
    true,
  )
  await evaluate(
    "Array.from(document.querySelectorAll('[data-commerce-order-list] button')).find(x=>x.textContent.includes('Browser customer')).click()",
  )
  await wait(
    'document.querySelector("[data-commerce-order-detail]").textContent.includes("Returned 2")&&' +
      status +
      '===""',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-commerce-order-detail]").querySelectorAll("form").length===0',
    ),
    true,
  )
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check('logout clears order data and viewer dashboard hides writes without browser exceptions')
  await page.close()
  page = null
  const browserStopped = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await browserStopped
  browser = null
  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, commerce: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/products')).status, 404)
  await stop()
  await operator(['modules', await privateFile('enabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  orders = await (await send(auth, base + '/orders')).json()
  assert.equal(orders.items[0].state, 'returned')
  assert.equal((await (await send(auth, base + '/stock/' + product.id)).json()).onHand, 4)
  check(
    'disable and re-enable commerce retain catalog, stock movements, orders, domain grants and receipts',
  )
  await stop()
  const archive = join(home, 'commerce.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_commerce'])
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
    (await (await send(auth, base + '/orders/' + order.id)).json()).order.totalEstimate,
    15000,
  )
  assert.equal((await (await send(auth, base + '/stock/' + product.id)).json()).onHand, 4)
  check(
    'actual host backup restore retains stock ledger, original quote, returns and receipt recovery after explicit identity recovery',
  )
  const destination = join(root, 'dist/commerce-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded catalog, stock and orders with physical returns',
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
