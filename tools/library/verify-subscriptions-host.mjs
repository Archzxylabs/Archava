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
const home = await mkdtemp(join(tmpdir(), 'archava-subscriptions-host-'))
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
  assert.ok(ready, 'isolated subscriptions host did not start')
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
const base = '/api/subscriptions/studio-demo'

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@sub.local',
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
    subscriptions: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/records')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'subscriptions routes require authentication and domain grants; enabling module never auto-enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'subscriptions',
      await privateFile('sub-' + access, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  const createSubCmd = {
    type: 'create_subscription',
    commandId: randomUUID(),
    contactId: randomUUID(),
    planCode: 'pro-monthly',
    planName: 'Professional Plan',
    billingInterval: 'monthly',
    unitPrice: 49900,
    currency: 'USD',
    quantity: 1,
    initialEntitlements: [
      {
        featureCode: 'seats',
        featureName: 'Team Seats',
        entitlementType: 'seat_count',
        allocatedQuantity: 5,
      },
    ],
  }
  assert.equal(
    (await send(auth, base + '/commands', createSubCmd, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', createSubCmd, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createSubCmd)).status, 403)
  assert.equal((await send(auth, '/api/subscriptions/unconfigured/records')).status, 404)
  const createRes = await send(auth, base + '/commands', createSubCmd)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.subscriptionId)
  assert.equal(receipt.status, 'draft')
  assert.equal(receipt.revision, 1)

  assert.equal(
    (await send(v, base + '/tools', { tool: 'subscriptions.list', input: {} })).status,
    200,
  )
  assert.equal(
    (
      await send(v, base + '/tools', {
        tool: 'subscriptions.list',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'subscriptions.create',
        input: { planCode: 'ILLEGAL' },
      })
    ).status,
    404,
  )
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal and subscriptions tool boundaries',
  )

  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createSubCmd.commandId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'recorded')
  assert.deepEqual(statusJson.receipt, receipt)
  check('host restart retains subscription state, memberships and idempotent receipt recovery')

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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-subscription-status]")?.textContent));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#subscriptions")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#subscriptions").hidden && document.querySelector("#subscriptions").textContent===""',
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
    '!!document.querySelector("[data-subscription-create]") && document.querySelector("[data-subscription-status]").textContent==="Subscriptions dashboard ready."',
  )
  assert.equal(
    await evaluate(
      '!document.querySelector("#subscriptions").hidden && !document.querySelector("[data-subscription-create]").hidden',
    ),
    true,
  )

  // Full browser workflow: Create subscription with simulated response loss & recovery
  const browserContactId = randomUUID()
  await fill('[data-subscription-create] [name=contactId]', browserContactId)
  await fill('[data-subscription-create] [name=planCode]', 'cloud-growth')
  await fill('[data-subscription-create] [name=planName]', 'Cloud Growth Plan')
  await fill('[data-subscription-create] [name=unitPrice]', '9900')
  await fill('[data-subscription-create] [name=featureCode]', 'metered_queries')
  await fill('[data-subscription-create] [name=featureName]', 'Monthly Metered Queries')
  await fill('[data-subscription-create] [name=featureQty]', '500')
  await evaluate(`
    window.subFetch = window.fetch;
    window.subMode = 'truncated_json';
    window.subBlocked = false;
    window.fetch = async (...args) => {
      if (window.subBlocked && String(args[0]).includes('/api/subscriptions/') && args[1]?.method === 'POST') {
        return new Response(JSON.stringify({ error: 'forbidden' }), { status: 403, headers: { 'content-type': 'application/json' } });
      }
      const response = await window.subFetch(...args);
      if (window.subMode === 'truncated_json' && String(args[0]).includes('/api/subscriptions/') && args[1]?.method === 'POST') {
        window.subMode = 'none';
        return new Response('{', { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.subMode === 'body_stream_error' && String(args[0]).includes('/api/subscriptions/') && args[1]?.method === 'POST') {
        window.subMode = 'none';
        throw new TypeError('Lost subscription response');
      }
      if (window.subMode === 'invalid_schema' && String(args[0]).includes('/api/subscriptions/') && args[1]?.method === 'POST') {
        window.subMode = 'none';
        return new Response(JSON.stringify({ notA: 'receipt' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.subMode === 'wrong_uuid' && String(args[0]).includes('/api/subscriptions/') && args[1]?.method === 'POST') {
        window.subMode = 'none';
        const data = await response.json();
        return new Response(JSON.stringify({ ...data, commandId: '00000000-0000-0000-0000-000000000000' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      return response;
    };
    Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Create Subscription').click();
  `)

  // 1. Truncated JSON response preserves uncertain state and reveals recovery
  await wait(
    'document.querySelector("[data-subscription-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(
    await evaluate('!document.querySelector("[data-subscription-recovery]").hidden'),
    true,
  )
  const countCreated = async () => {
    const r = await send(auth, base + '/records')
    assert.equal(r.status, 200)
    const j = await r.json()
    const list = Array.isArray(j) ? j : (j.items ?? [])
    return list.filter((x) => x.planName === 'Cloud Growth Plan').length
  }
  assert.equal(await countCreated(), 1)

  // Blocked new write while outcome is uncertain
  await fill('[data-subscription-create] [name=planCode]', 'cloud-blocked')
  await evaluate(
    "Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Create Subscription').click()",
  )
  await wait(
    'document.querySelector("[data-subscription-status]").textContent.includes("Uncertain command outcome")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 2. Retry with body stream error retains uncertain recovery
  await evaluate("window.subMode = 'body_stream_error'")
  await evaluate(
    "Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-subscription-status]").textContent.includes("Network interruption")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 3. Retry with invalid receipt schema retains uncertain recovery
  await evaluate("window.subMode = 'invalid_schema'")
  await evaluate(
    "Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-subscription-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 4. Retry with mismatched commandId UUID retains uncertain recovery
  await evaluate("window.subMode = 'wrong_uuid'")
  await evaluate(
    "Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-subscription-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 5. Retry with simulated grant revocation (403)
  await evaluate('window.subBlocked = true')
  await evaluate(
    "Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-subscription-status]").textContent.includes("forbidden")',
  )
  await idle()
  await evaluate('window.subBlocked = false')
  assert.equal(await countCreated(), 1)

  // 6. Check pending result reconciles committed server state
  await evaluate(
    "Array.from(document.querySelectorAll('#subscriptions button')).find(x => x.textContent === 'Check pending result').click()",
  )
  await wait(
    'document.querySelector("[data-subscription-status]").textContent === "Action confirmed."',
  )
  await evaluate('window.fetch = window.subFetch; delete window.subFetch')
  assert.equal(await countCreated(), 1)

  // Subscription now listed in UI
  await wait(
    'document.querySelector("[data-subscription-list]").textContent.includes("Cloud Growth Plan")',
  )

  // View subscription details
  await evaluate(`
    const root = document.querySelector("[data-subscription-list]");
    const li = Array.from(root.querySelectorAll('li')).find(x => x.textContent.includes('Cloud Growth Plan'));
    const btn = li?.querySelector('button');
    btn?.click();
  `)
  await wait(
    'document.querySelector("[data-subscription-detail]").textContent.includes("Cloud Growth Plan")',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-subscription-detail]").textContent.includes("cloud-growth")',
    ),
    true,
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-subscription-detail]").textContent.includes("Monthly Metered Queries")',
    ),
    true,
  )
  check(
    'browser dashboard surfaces recovery panel, blocks duplicate records on failure modes and reconciles pending receipts',
  )

  // Verify logout cleanup
  await evaluate('document.querySelector("#logout").click()')
  await wait(
    '!document.querySelector("#login").hidden && document.querySelector("#workspace").hidden',
  )
  assert.equal(await evaluate('document.querySelector("#subscriptions").textContent === ""'), true)
  check('sign-out tears down subscriptions DOM and detaches listeners')

  const destination = join(root, 'dist/subscriptions-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'bounded subscription, entitlement and usage workflow with resilient receipt recovery',
        checks,
        browser: 'isolated Chromium',
        providerCalls: 0,
        operationalKitsAccepted: [],
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(`\nAll ${checks.length} host E2E checks passed successfully.\n`)
} finally {
  await page?.close()
  if (browser && browser.exitCode === null) {
    const done = new Promise((resolve) => browser.once('exit', resolve))
    browser.kill('SIGTERM')
    await done
  }
  await stop()
  await rm(home, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 })
}
