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
const home = await mkdtemp(join(tmpdir(), 'archava-purchasing-host-'))
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
const base = '/api/purchasing/studio-demo'
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
      username: 'viewer@purchasing.local',
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
    purchasing: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/suppliers')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'purchasing requires login and independent purchasing/commerce memberships; module flags never enroll accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    for (const domain of ['commerce', 'purchasing'])
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
    supplierCommand = command({ operation: 'supplier.create', code: 'HOST', name: 'Host supplier' })
  assert.equal(
    (await send(auth, base + '/commands', supplierCommand, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', supplierCommand, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', supplierCommand)).status, 403)
  assert.equal((await send(auth, '/api/purchasing/unconfigured/orders')).status, 404)
  const supplierResponse = await send(auth, base + '/commands', supplierCommand)
  assert.equal(supplierResponse.status, 200)
  const supplierReceipt = await supplierResponse.json()
  assert.equal(
    (await send(v, base + '/tools', { name: 'purchasing.suppliers', input: {} })).status,
    200,
  )
  assert.equal(
    (await send(v, base + '/tools', { name: 'purchasing.orders', input: { tenant: 'hotel-demo' } }))
      .status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        name: 'purchasing.order.receive',
        input: { id: supplierReceipt.id },
      })
    ).status,
    404,
  )
  check(
    'actual compiled host enforces cookie/session, CSRF, same origin, configured workspace, viewer and private read-only tool boundaries',
  )
  await stop()
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    {
      ...(await (await send(auth, base + '/commands/' + supplierCommand.requestId)).json()).receipt,
      replayed: false,
    },
    supplierReceipt,
  )
  check('host restart retains supplier identity, domain memberships and original UUID receipt')
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
    price: 15,
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
      /* Browser is starting. */
    }
  }
  assert.ok(ready)
  page = await Session.open()
  await page.navigate(origin)
  const evaluate = (expr) => page.evaluate(expr),
    wait = (condition) =>
      evaluate(
        `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Purchasing UI timed out: '+${JSON.stringify(condition)}));else setTimeout(tick,100)};tick()})`,
      ),
    fill = (selector, value) =>
      evaluate(
        `document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`,
      ),
    status = 'document.querySelector("[data-purchasing-status]").textContent'
  const idle = () =>
      wait('!Array.from(document.querySelectorAll("#purchasing button")).some(x=>x.disabled)'),
    click = async (text) => {
      await evaluate(
        `Array.from(document.querySelectorAll('#purchasing button')).find(x=>x.textContent===${JSON.stringify(text)}).click()`,
      )
      await idle()
    },
    login = async (account) => {
      await fill('#login [name=username]', account.username)
      await fill('#login [name=password]', account.password)
      await evaluate('document.querySelector("#login").requestSubmit()')
      await wait(
        '!document.querySelector("#workspace").hidden&&document.querySelector("#status").textContent===""',
      )
      await evaluate(
        'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
      )
      await wait(
        '!!document.querySelector("[data-purchasing-create]")&&' +
          status +
          '==="Purchasing ready."',
      )
    }
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#purchasing").hidden&&document.querySelector("#purchasing").textContent===""',
    ),
    true,
  )
  await login(owner)
  await fill('[data-purchasing-supplier] [name=code]', 'BROWSER')
  await fill('[data-purchasing-supplier] [name=name]', 'Supplier <img src=x onerror=alert(1)>')
  await evaluate('document.querySelector("[data-purchasing-supplier]").requestSubmit()')
  await wait(status + '==="Saved supplier.create."')
  await idle()
  const supplier = (await (await send(auth, base + '/suppliers')).json()).items.find(
    (x) => x.code === 'BROWSER',
  )
  assert.ok(supplier)
  await fill('[data-purchasing-offer] [name=supplierId]', supplier.id)
  await fill('[data-purchasing-offer] [name=productId]', ingredient.id)
  await fill('[data-purchasing-offer] [name=unitCost]', '8')
  await evaluate('document.querySelector("[data-purchasing-offer]").requestSubmit()')
  await wait(status + '==="Saved offer.create."')
  await idle()
  const offer = (await (await send(auth, base + '/offers')).json()).items.find(
    (x) => x.supplierId === supplier.id,
  )
  assert.ok(offer)
  assert.equal(await evaluate('document.querySelector("#purchasing img")===null'), true)
  check(
    'Chromium creates supplier records and backend base-unit offers from commerce products without HTML injection or duplicated stock records',
  )
  await fill('[data-purchasing-create] [name=supplierId]', supplier.id)
  await evaluate(
    'document.querySelector("[data-purchasing-create] [name=supplierId]").dispatchEvent(new Event("change"))',
  )
  await fill('[data-purchasing-create] [name=offerId]', offer.id)
  await fill('[data-purchasing-create] [name=quantity]', '100')
  await evaluate('document.querySelector("[data-purchasing-create]").requestSubmit()')
  await wait(status + '==="Saved order.create."')
  await idle()
  const po = (await (await send(auth, base + '/orders')).json()).items[0]
  assert.equal(po.totalEstimate, 800)
  await mutate(base, {
    operation: 'offer.set-cost',
    id: offer.id,
    expectedRevision: 1,
    unitCost: 9,
    reason: 'Updated supplier cost',
  })
  await click('Confirm purchase')
  await wait(status + '.includes("409")')
  assert.equal((await (await send(auth, base + '/orders/' + po.id)).json()).order.state, 'draft')
  await click('Refresh draft quote')
  await wait(status + '==="Saved order.replace-lines."')
  await click('Confirm purchase')
  await wait(status + '==="Saved order.confirm."')
  assert.equal(
    (await (await send(auth, base + '/orders/' + po.id)).json()).order.totalEstimate,
    900,
  )
  await mutate(base, {
    operation: 'offer.set-cost',
    id: offer.id,
    expectedRevision: 2,
    unitCost: 10,
    reason: 'Future price',
  })
  check(
    'browser refuses stale draft confirmation, refreshes the backend quote for review and confirms immutable purchase prices separately from sales prices',
  )
  let poDetail = await (await send(auth, base + '/orders/' + po.id)).json(),
    line = poDetail.lines[0]
  await fill('[data-purchasing-receive] [name=accepted-' + line.id + ']', '60')
  await fill('[data-purchasing-receive] [name=rejected-' + line.id + ']', '5')
  await fill(
    '[data-purchasing-receive] [name=reason]',
    'Delivery <img src=x onerror=alert(1)> inspected',
  )
  await evaluate(
    "window.purchaseFetch=window.fetch;window.purchaseDrop=true;window.purchaseBlocked=false;window.fetch=async(...args)=>{if(window.purchaseBlocked&&String(args[0]).includes('/api/purchasing/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.purchaseFetch(...args);if(window.purchaseDrop&&String(args[0]).includes('/api/purchasing/')&&args[1]?.method==='POST'){window.purchaseDrop=false;throw new Error('Lost purchase response')}return response};document.querySelector('[data-purchasing-receive]').requestSubmit()",
  )
  await wait(status + '.includes("Lost purchase response")')
  await evaluate('window.purchaseBlocked=true')
  await click('Retry same request')
  await wait(status + '.includes("403")')
  await evaluate('window.purchaseBlocked=false')
  await evaluate('document.querySelector("[data-purchasing-receive]").requestSubmit()')
  await wait(status + '.includes("Recover the previous Save")')
  await click('Check saved request')
  await wait(status + '==="Saved order.receive."')
  await idle()
  await evaluate('window.fetch=window.purchaseFetch;delete window.purchaseFetch')
  poDetail = await (await send(auth, base + '/orders/' + po.id)).json()
  assert.equal(poDetail.order.state, 'partial')
  assert.equal(poDetail.receipts.length, 1)
  assert.equal(
    (await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()).onHand,
    60,
  )
  assert.equal(await evaluate('document.querySelector("#purchasing img")===null'), true)
  check(
    'browser recovers lost receipt responses with the original UUID, retains pending outcome after denied retry and blocks new effects; accepted stock enters once and rejected goods stay separate',
  )
  await fill('[data-purchasing-receive] [name=accepted-' + line.id + ']', '35')
  await fill('[data-purchasing-receive] [name=reason]', 'Final 35 accepted')
  await evaluate('document.querySelector("[data-purchasing-receive]").requestSubmit()')
  await wait(status + '==="Saved order.receive."')
  await idle()
  const completed = await (await send(auth, base + '/orders/' + po.id)).json()
  assert.equal(completed.order.state, 'received')
  assert.equal(completed.lines[0].acceptedQuantity, 95)
  assert.equal(completed.lines[0].rejectedQuantity, 5)
  assert.equal(completed.order.totalEstimate, 900)
  assert.equal(
    (await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()).onHand,
    95,
  )
  assert.equal(Object.hasOwn(completed.order, 'paid'), false)
  check(
    'Chromium records partial and final inspection quantities, retains frozen supplier cost and distinguishes all accounted goods from usable stock and payment',
  )
  await fill('[data-purchasing-create] [name=quantity]', '10')
  await evaluate('document.querySelector("[data-purchasing-create]").requestSubmit()')
  await wait(status + '==="Saved order.create."')
  await idle()
  const shortId = await evaluate(
    'document.querySelector("[data-purchasing-detail]").dataset.orderId',
  )
  await click('Confirm purchase')
  await wait(status + '==="Saved order.confirm."')
  const short = (await (await send(auth, base + '/orders/' + shortId)).json()).lines[0]
  await fill('[data-purchasing-receive] [name=accepted-' + short.id + ']', '2')
  await fill('[data-purchasing-receive] [name=reason]', 'Partial arrival')
  await evaluate('document.querySelector("[data-purchasing-receive]").requestSubmit()')
  await wait(status + '==="Saved order.receive."')
  await idle()
  await fill('[data-purchasing-detail] section [name=reason]', 'Supplier cannot send remaining')
  await click('Short-close purchase')
  await wait(status + '==="Saved order.short-close."')
  assert.equal(
    (await (await send(auth, base + '/orders/' + shortId)).json()).order.state,
    'short_closed',
  )
  assert.equal(
    (await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()).onHand,
    97,
  )
  check(
    'browser short closure prevents further receipt while retaining earlier physical stock and findings',
  )
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#workspace").hidden')
  assert.equal(await evaluate('document.querySelector("#purchasing").textContent===""'), true)
  await login(viewer)
  assert.equal(
    await evaluate(
      'document.querySelector("[data-purchasing-create]").hidden&&document.querySelector("[data-purchasing-supplier]").hidden&&document.querySelector("[data-purchasing-offer]").hidden&&!Array.from(document.querySelectorAll("#purchasing button")).some(x=>/Confirm purchase|Record goods receipt|Short-close purchase/.test(x.textContent))',
    ),
    true,
  )
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check(
    'logout clears private purchasing data and viewer panel hides writes without browser exceptions',
  )
  await page.close()
  page = null
  const browserStopped = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await browserStopped
  browser = null
  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, purchasing: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/orders')).status, 404)
  await stop()
  await operator(['modules', await privateFile('enabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.equal((await (await send(auth, base + '/orders/' + po.id)).json()).order.state, 'received')
  assert.equal(
    (await (await send(auth, base + '/commands/' + supplierCommand.requestId)).json()).state,
    'committed',
  )
  check(
    'disable and re-enable remove purchasing routes while retaining offers, purchase findings, shared stock, grants and receipt recovery',
  )
  await stop()
  const archive = join(home, 'purchasing.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_purchasing'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  const recovered = { username: owner.username, password: randomBytes(24).toString('base64url') }
  await operator(['user', 'recover', await privateFile('recovered', recovered)])
  await start()
  auth = await signIn(recovered)
  const restored = await (await send(auth, base + '/orders/' + po.id)).json()
  assert.equal(restored.order.totalEstimate, 900)
  assert.equal(restored.receipts.length, 2)
  assert.equal(
    (await (await send(auth, commerceBase + '/stock/' + ingredient.id)).json()).onHand,
    97,
  )
  assert.equal(
    (await (await send(auth, base + '/commands/' + supplierCommand.requestId)).json()).state,
    'committed',
  )
  check(
    'actual host backup restore retains supplier quote, inspection findings, shared stock and original receipts after explicit identity recovery',
  )
  const destination = join(root, 'dist/purchasing-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded supplier offers, purchase confirmation and partial physical goods receipts',
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
