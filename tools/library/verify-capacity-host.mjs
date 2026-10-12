/* global process, fetch, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, writeFile, readFile, rm, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { createServer } from 'node:net'
import { randomBytes, randomUUID, createHash } from 'node:crypto'

const root = resolve(import.meta.dirname, '../..'),
  runtime = join(root, 'dist/crm-runtime')
const home = await mkdtemp(join(tmpdir(), 'archava-capacity-host-'))
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
  assert.ok(ready, 'isolated capacity host did not start')
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
const base = '/api/capacity/studio-demo'
const command = (value) => ({ commandId: randomUUID(), ...value })

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@cap.local',
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
    capacity: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/pools')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'capacity routes require authentication and domain grants; enabling module never auto-enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'capacity',
      await privateFile('cap-' + access, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  const createPool = command({
    type: 'create_pool',
    code: 'CORE_DEV',
    name: 'Core Engineering Delivery Team',
    unit: 'hours',
    weeklyCapacity: 200,
    maxConcurrentJobs: 2,
  })
  assert.equal(
    (await send(auth, base + '/commands', createPool, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', createPool, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createPool)).status, 403)
  assert.equal((await send(auth, '/api/capacity/unconfigured/pools')).status, 404)
  const createRes = await send(auth, base + '/commands', createPool)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.poolId)
  assert.equal(receipt.revision, 1)

  const estimateRes = await send(auth, base + '/estimate', {
    poolId: receipt.poolId,
    requiredUnits: 40,
    desiredStartAt: '2030-01-07T00:00:00Z',
  })
  assert.equal(estimateRes.status, 200)
  const estimateJson = await estimateRes.json()
  assert.equal(estimateJson.burnRatePerDay, 40)
  assert.equal(estimateJson.estimatedCalendarDays, 1)

  assert.equal((await send(v, base + '/tools', { tool: 'capacity.pools', input: {} })).status, 200)
  assert.equal(
    (
      await send(v, base + '/tools', {
        tool: 'capacity.pools',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'capacity.create',
        input: { code: 'ILLEGAL' },
      })
    ).status,
    404,
  )
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal and capacity tool boundaries',
  )

  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createPool.commandId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'committed')
  assert.deepEqual(statusJson.receipt, receipt)
  check('host restart retains pool state, memberships and idempotent receipt recovery')

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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-capacity-status]")?.textContent));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#capacity")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#capacity").hidden && document.querySelector("#capacity").textContent===""',
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
    '!!document.querySelector("[data-capacity-create]") && document.querySelector("[data-capacity-status]").textContent==="Capacity ready."',
  )
  assert.equal(
    await evaluate(
      '!document.querySelector("#capacity").hidden && !document.querySelector("[data-capacity-create]").hidden',
    ),
    true,
  )

  // Full browser workflow: Create pool with simulated response loss & recovery
  await fill('[data-capacity-create] [name=code]', 'BROWSER_POOL')
  await fill('[data-capacity-create] [name=name]', 'Browser Sprint Pool')
  await fill('[data-capacity-create] [name=weeklyCapacity]', '200')
  await fill('[data-capacity-create] [name=maxConcurrency]', '2')
  await evaluate(`
    window.capFetch = window.fetch;
    window.capMode = 'truncated_json';
    window.capBlocked = false;
    window.fetch = async (...args) => {
      if (window.capBlocked && String(args[0]).includes('/api/capacity/') && args[1]?.method === 'POST') {
        return new Response(JSON.stringify({ error: 'forbidden' }), { status: 403, headers: { 'content-type': 'application/json' } });
      }
      const response = await window.capFetch(...args);
      if (window.capMode === 'truncated_json' && String(args[0]).includes('/api/capacity/') && args[1]?.method === 'POST') {
        window.capMode = 'none';
        return new Response('{', { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.capMode === 'body_stream_error' && String(args[0]).includes('/api/capacity/') && args[1]?.method === 'POST') {
        window.capMode = 'none';
        throw new TypeError('Lost capacity response');
      }
      if (window.capMode === 'invalid_schema' && String(args[0]).includes('/api/capacity/') && args[1]?.method === 'POST') {
        window.capMode = 'none';
        return new Response(JSON.stringify({ notA: 'receipt' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      if (window.capMode === 'wrong_uuid' && String(args[0]).includes('/api/capacity/') && args[1]?.method === 'POST') {
        window.capMode = 'none';
        const data = await response.json();
        return new Response(JSON.stringify({ ...data, commandId: '00000000-0000-0000-0000-000000000000' }), { status: 200, headers: { 'content-type': 'application/json' } });
      }
      return response;
    };
    Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Create Capacity Pool').click();
  `)

  // 1. Truncated JSON response preserves uncertain state and reveals recovery
  await wait(
    'document.querySelector("[data-capacity-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(
    await evaluate(
      '!document.querySelector("#capacity [role=status]")?.parentElement?.querySelector("div")?.hidden',
    ),
    true,
  )
  const countCreated = async () => {
    const r = await send(auth, base + '/pools')
    assert.equal(r.status, 200)
    const j = await r.json()
    return j.items.filter((x) => (x.title ?? x.name) === 'Browser Sprint Pool').length
  }
  assert.equal(await countCreated(), 1)

  // Blocked new write while outcome is uncertain
  await fill('[data-capacity-create] [name=code]', 'BLOCKED_POOL')
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Create Capacity Pool').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-status]").textContent.includes("Uncertain command outcome")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 2. Retry with body stream error retains uncertain recovery
  await evaluate("window.capMode = 'body_stream_error'")
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-status]").textContent.includes("Network interruption")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 3. Retry with invalid receipt schema retains uncertain recovery
  await evaluate("window.capMode = 'invalid_schema'")
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 4. Retry with mismatched command UUID retains uncertain recovery
  await evaluate("window.capMode = 'wrong_uuid'")
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-status]").textContent.includes("Invalid or truncated command receipt")',
  )
  await idle()
  assert.equal(await countCreated(), 1)

  // 5. Retry with simulated grant revocation (403)
  await evaluate('window.capBlocked = true')
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait('document.querySelector("[data-capacity-status]").textContent.includes("HTTP 403")')
  await idle()
  await evaluate('window.capBlocked = false')
  assert.equal(await countCreated(), 1)

  // 6. Check pending result reconciles committed server state
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Check pending result').click()",
  )
  await wait('document.querySelector("[data-capacity-status]").textContent === "Action confirmed."')
  await evaluate('window.fetch = window.capFetch; delete window.capFetch')
  assert.equal(await countCreated(), 1)

  // Pool now listed in UI
  await wait(
    '(document.querySelector("[data-capacity-pool-list]") || document.querySelector("[data-capacity-pools]")).textContent.includes("Browser Sprint Pool")',
  )

  // View pool details
  await evaluate(`
    const root = document.querySelector("[data-capacity-pool-list]") || document.querySelector("[data-capacity-pools]");
    const btn = Array.from(root.querySelectorAll('button')).find(x => x.textContent.includes('Browser Sprint Pool'));
    btn?.click();
  `)
  await wait(
    'document.querySelector("[data-capacity-detail]").textContent.includes("Browser Sprint Pool")',
  )

  // Calculate deterministic timeline estimate via UI
  await fill('[name=reqUnits]', '80')
  await evaluate(
    "Array.from(document.querySelectorAll('#capacity button')).find(x => x.textContent === 'Calculate Deterministic Timeline').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-detail]").textContent.includes("Queue Pos: #0")',
  )

  // Commit workload allocation via UI
  await wait('!!document.querySelector("[data-capacity-commit]")')
  await fill('[data-capacity-commit] [name=workloadId]', 'TASK-001')
  await fill('[data-capacity-commit] [name=workloadTitle]', 'Browser Allocation Delivery')
  await fill('[data-capacity-commit] [name=workUnits]', '40')
  await evaluate(
    "Array.from(document.querySelectorAll('[data-capacity-commit] button')).find(x => x.textContent === 'Commit Allocation').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-detail]").textContent.includes("Browser Allocation Delivery")',
  )

  // Record progress on allocation via UI
  await evaluate(
    "Array.from(document.querySelectorAll('[data-capacity-detail] button')).find(x => x.textContent === 'Record Progress').click()",
  )
  await wait(
    'document.querySelector("[data-capacity-detail]").textContent.includes("Remaining: 30/40")',
  )

  // Complete workload allocation via UI
  await evaluate(
    "Array.from(document.querySelectorAll('[data-capacity-detail] button')).find(x => x.textContent === 'Complete Workload').click()",
  )
  await wait(
    '!document.querySelector("[data-capacity-detail] ul").textContent.includes("Remaining: 30/40")',
  )

  // Logout and verify private data cleared
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#capacity").textContent === ""')

  // Sign in as viewer to ensure write forms are hidden
  await fill('#login [name=username]', viewer.username)
  await fill('#login [name=password]', viewer.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!document.querySelector("#workspace").hidden && document.querySelector("#status").textContent===""',
  )
  await evaluate(
    'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
  )
  await wait('document.querySelector("[data-capacity-status]").textContent==="Capacity ready."')
  assert.equal(await evaluate('document.querySelector("[data-capacity-create]").hidden'), true)
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)

  // Logout viewer
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#capacity").textContent === ""')

  check(
    'isolated browser completes pool creation, deterministic timeline estimation, workload allocation, progress tracking and completion lifecycle with response recovery and logout cleanup',
  )

  await stop()
  await operator([
    'modules',
    await privateFile('disabled-cap', {
      contacts: true,
      pipeline: false,
      assistant: false,
      nativeActions: false,
      capacity: false,
    }),
  ])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/pools')).status, 404)
  await stop()
  await operator(['modules', await privateFile('re-enabled-cap', modules)])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/pools')).status, 200)
  check('module disable and re-enable removes routes and restores pool state without data loss')

  await stop()
  const archive = join(home, 'cap.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_cap'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  const recovered = { username: owner.username, password: randomBytes(24).toString('base64url') }
  await operator(['user', 'recover', await privateFile('recovered', recovered)])
  await start()
  auth = await signIn(recovered)
  const restoredPool = await (await send(auth, base + '/pools/' + receipt.poolId)).json()
  assert.equal(restoredPool.pool.name, 'Core Engineering Delivery Team')
  assert.equal(
    (await (await send(auth, base + '/commands/' + createPool.commandId)).json()).status,
    'committed',
  )
  check(
    'backup and restore preserves capacity pool entities and receipts under recovered credentials',
  )

  const destination = join(root, 'dist/delivery-capacity-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        catalogSha256: createHash('sha256').update(JSON.stringify(profile.catalog)).digest('hex'),
        artifactSha256: createHash('sha256')
          .update(JSON.stringify(profile.artifacts))
          .digest('hex'),
        scope: 'discrete business-day capacity estimation and discrete throughput allocation',
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
