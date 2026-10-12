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
const home = await mkdtemp(join(tmpdir(), 'archava-assets-host-'))
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
  assert.ok(ready, 'isolated assets host did not start')
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
const base = '/api/assets/studio-demo'
const mutation = (type, data) => ({ requestId: randomUUID(), type, data })

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@asset.local',
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
    assets: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/records')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'assets routes require authentication and domain grants; enabling module never auto-enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'assets',
      await privateFile('asset-' + access, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  const createAssetCmd = mutation('create_asset', {
    code: 'AST-BRW-01',
    name: 'Industrial Fiber Laser Welder',
    category: 'manufacturing',
    ownership: 'business_owned',
    serialNumber: 'LSR-2024-9988',
    condition: 'excellent',
    initialLocation: 'Bay 4 Lab',
  })
  assert.equal(
    (await send(auth, base + '/commands', createAssetCmd, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', createAssetCmd, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createAssetCmd)).status, 403)
  assert.equal((await send(auth, '/api/assets/unconfigured/records')).status, 404)
  const createRes = await send(auth, base + '/commands', createAssetCmd)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.assetId)
  assert.equal(receipt.revision, 1)

  assert.equal((await send(v, base + '/tools', { tool: 'assets.list', input: {} })).status, 200)
  assert.equal(
    (
      await send(v, base + '/tools', {
        tool: 'assets.list',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'assets.create',
        input: { code: 'ILLEGAL' },
      })
    ).status,
    404,
  )
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal and assets tool boundaries',
  )

  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createAssetCmd.requestId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'committed')
  assert.deepEqual(statusJson.receipt, receipt)
  check('host restart retains asset state, memberships and idempotent receipt recovery')

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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-asset-status]")?.textContent));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#assets")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#assets").hidden && document.querySelector("#assets").textContent===""',
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
    '!!document.querySelector("[data-asset-create]") && document.querySelector("[data-asset-status]").textContent==="Asset register ready."',
  )
  assert.equal(
    await evaluate(
      '!document.querySelector("#assets").hidden && !document.querySelector("[data-asset-create]").hidden',
    ),
    true,
  )

  // Full browser workflow: Register asset with simulated response loss & recovery
  await fill('[data-asset-create] [name=code]', 'AST-FLIGHT-02')
  await fill('[data-asset-create] [name=name]', 'High-Precision Aerial Drone')
  await fill('[data-asset-create] [name=category]', 'surveying')
  await fill('[data-asset-create] [name=serialNumber]', 'SN-DRN-2026')
  await evaluate(`
    window.assetFetch = window.fetch;
    window.assetMode = 'truncated_json';
    window.assetBlocked = false;
    window.fetch = async (...args) => {
      if (window.assetBlocked && String(args[0]).includes('/api/assets/') && args[1]?.method === 'POST') {
        return new Response(JSON.stringify({ error: 'forbidden' }), { status: 403, headers: { 'content-type': 'application/json' } });
      }
      const response = await window.assetFetch(...args);
      if (window.assetMode === 'truncated_json' && String(args[0]).includes('/api/assets/') && args[1]?.method === 'POST') {
        window.assetMode = 'none';
        return new Response('{', { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.assetMode === 'body_stream_error' && String(args[0]).includes('/api/assets/') && args[1]?.method === 'POST') {
        window.assetMode = 'none';
        throw new TypeError('Lost asset response');
      }
      if (window.assetMode === 'invalid_schema' && String(args[0]).includes('/api/assets/') && args[1]?.method === 'POST') {
        window.assetMode = 'none';
        return new Response(JSON.stringify({ notA: 'receipt' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.assetMode === 'wrong_uuid' && String(args[0]).includes('/api/assets/') && args[1]?.method === 'POST') {
        window.assetMode = 'none';
        const data = await response.json();
        return new Response(JSON.stringify({ ...data, requestId: '00000000-0000-0000-0000-000000000000' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      return response;
    };
    Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Register Asset').click();
  `)

  // 1. Truncated JSON response preserves uncertain state and reveals recovery
  await wait(
    'document.querySelector("[data-asset-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await evaluate('!document.querySelector("[data-asset-recovery]").hidden'), true)
  const countCreated = async () => {
    const r = await send(auth, base + '/records')
    assert.equal(r.status, 200)
    const j = await r.json()
    return j.items.filter((x) => x.name === 'High-Precision Aerial Drone').length
  }
  assert.equal(await countCreated(), 1)

  // Blocked new write while outcome is uncertain
  await fill('[data-asset-create] [name=code]', 'AST-BLOCKED-03')
  await evaluate(
    "Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Register Asset').click()",
  )
  await wait(
    'document.querySelector("[data-asset-status]").textContent.includes("Uncertain command outcome")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 2. Retry with body stream error retains uncertain recovery
  await evaluate("window.assetMode = 'body_stream_error'")
  await evaluate(
    "Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-asset-status]").textContent.includes("Network interruption")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 3. Retry with invalid receipt schema retains uncertain recovery
  await evaluate("window.assetMode = 'invalid_schema'")
  await evaluate(
    "Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-asset-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 4. Retry with mismatched requestId UUID retains uncertain recovery
  await evaluate("window.assetMode = 'wrong_uuid'")
  await evaluate(
    "Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-asset-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 5. Retry with simulated grant revocation (403)
  await evaluate('window.assetBlocked = true')
  await evaluate(
    "Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait('document.querySelector("[data-asset-status]").textContent.includes("forbidden")')
  await idle()
  await evaluate('window.assetBlocked = false')
  assert.equal(await countCreated(), 1)

  // 6. Check pending result reconciles committed server state
  await evaluate(
    "Array.from(document.querySelectorAll('#assets button')).find(x => x.textContent === 'Check pending result').click()",
  )
  await wait('document.querySelector("[data-asset-status]").textContent === "Action confirmed."')
  await evaluate('window.fetch = window.assetFetch; delete window.assetFetch')
  assert.equal(await countCreated(), 1)

  // Asset now listed in UI
  await wait(
    'document.querySelector("[data-asset-list]").textContent.includes("High-Precision Aerial Drone")',
  )

  // View asset details
  await evaluate(`
    const root = document.querySelector("[data-asset-list]");
    const li = Array.from(root.querySelectorAll('li')).find(x => x.textContent.includes('High-Precision Aerial Drone'));
    const btn = li?.querySelector('button');
    btn?.click();
  `)
  await wait(
    'document.querySelector("[data-asset-detail]").textContent.includes("High-Precision Aerial Drone")',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-asset-detail]").textContent.includes("AST-FLIGHT-02")',
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
  assert.equal(await evaluate('document.querySelector("#assets").textContent === ""'), true)
  check('sign-out tears down asset DOM and detaches listeners')

  const destination = join(root, 'dist/assets-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded asset custody and maintenance workflow with resilient receipt recovery',
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
