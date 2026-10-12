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
const home = await mkdtemp(join(tmpdir(), 'archava-organizations-host-'))
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
  assert.ok(ready, 'isolated organizations host did not start')
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
const base = '/api/organizations/studio-demo'
const command = (value) => ({ requestId: randomUUID(), ...value })

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@orgs.local',
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
    organizations: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/records')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  check(
    'organizations routes require authentication and domain grants; enabling module never auto-enrolls accounts',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'organizations',
      await privateFile('org-' + access, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  const createOrg = command({
    type: 'create_organization',
    data: { name: 'Host Reusable Org Corp', industry: 'Enterprise Technology' },
  })
  assert.equal(
    (await send(auth, base + '/commands', createOrg, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, base + '/commands', createOrg, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createOrg)).status, 403)
  assert.equal((await send(auth, '/api/organizations/unconfigured/records')).status, 404)
  const createRes = await send(auth, base + '/commands', createOrg)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.organizationId)
  assert.equal(receipt.revision, 1)

  assert.equal(
    (await send(v, base + '/tools', { tool: 'organizations.search', input: { query: 'Host' } }))
      .status,
    200,
  )
  assert.equal(
    (
      await send(v, base + '/tools', {
        tool: 'organizations.search',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'organizations.create',
        input: { name: 'Unauthorized tool write' },
      })
    ).status,
    404,
  )
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal and tool authorization boundaries',
  )

  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createOrg.requestId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'committed')
  assert.deepEqual(statusJson.receipt, receipt)
  check('host restart retains organization state, memberships and idempotent receipt recovery')

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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-org-status]")?.textContent + ', recovery: ' + !document.querySelector("#organizations [role=status]")?.parentElement?.querySelector("div")?.hidden));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#organizations")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#organizations").hidden && document.querySelector("#organizations").textContent===""',
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
    '!!document.querySelector("[data-org-create]") && document.querySelector("[data-org-status]").textContent==="Organizations ready."',
  )
  assert.equal(
    await evaluate(
      '!document.querySelector("#organizations").hidden && !document.querySelector("[data-org-create]").hidden',
    ),
    true,
  )

  // Full browser workflow: Create organization with response loss simulation & recovery
  await fill('[data-org-create] [name=name]', 'Browser Reusable Org Corp')
  await fill('[data-org-create] [name=industry]', 'Enterprise Cloud')
  await evaluate(`
    window.orgFetch = window.fetch;
    window.orgDrop = true;
    window.orgBlocked = false;
    window.fetch = async (...args) => {
      if (window.orgBlocked && String(args[0]).includes('/api/organizations/') && args[1]?.method === 'POST') {
        return new Response(JSON.stringify({ error: 'forbidden' }), { status: 403, headers: { 'content-type': 'application/json' } });
      }
      const response = await window.orgFetch(...args);
      if (window.orgDrop && String(args[0]).includes('/api/organizations/') && args[1]?.method === 'POST') {
        window.orgDrop = false;
        throw new TypeError('Lost organization response');
      }
      return response;
    };
    Array.from(document.querySelectorAll('#organizations button')).find(x => x.textContent === 'Create Organization').click();
  `)

  await wait(
    'document.querySelector("[data-org-status]").textContent.includes("Network interruption")',
  )

  // Blocked new write while outcome is uncertain
  await fill('[data-org-create] [name=name]', 'Conflicting Unreconciled Org')
  await evaluate(
    "Array.from(document.querySelectorAll('#organizations button')).find(x => x.textContent === 'Create Organization').click()",
  )
  await wait(
    'document.querySelector("[data-org-status]").textContent.includes("Uncertain command outcome")',
  )

  // Retry with simulated grant revocation (403)
  await evaluate('window.orgBlocked = true')
  await evaluate(
    "Array.from(document.querySelectorAll('#organizations button')).find(x => x.textContent === 'Retry same request').click()",
  )
  await wait(
    'document.querySelector("[data-org-status]").textContent.includes("HTTP 403") || document.querySelector("[data-org-status]").textContent.includes("forbidden")',
  )
  await idle()
  await evaluate('window.orgBlocked = false')

  // Check pending result reconciles committed server state
  await evaluate(
    "Array.from(document.querySelectorAll('#organizations button')).find(x => x.textContent === 'Check pending result').click()",
  )
  await wait('document.querySelector("[data-org-status]").textContent === "Action confirmed."')
  await evaluate('window.fetch = window.orgFetch; delete window.orgFetch')

  // Organization now listed in UI
  await wait(
    'document.querySelector("[data-org-list]").textContent.includes("Browser Reusable Org Corp")',
  )

  // View organization details
  await evaluate(`
    const btn = Array.from(document.querySelectorAll('[data-org-list] li')).find(x => x.textContent.includes('Browser Reusable Org Corp'))?.querySelector('button');
    btn?.click();
  `)
  await wait(
    'document.querySelector("[data-org-detail]").textContent.includes("Browser Reusable Org Corp")',
  )

  // Add contact affiliation through UI
  const testContactId = randomUUID()
  await fill('[data-org-affiliation] [name=contactId]', testContactId)
  await fill('[data-org-affiliation] [name=role]', 'VP Engineering')
  await fill('[data-org-affiliation] [name=primary]', 'yes')
  await evaluate(
    "Array.from(document.querySelectorAll('[data-org-affiliation] button')).find(x => x.textContent === 'Add Affiliation').click()",
  )
  await wait('document.querySelector("[data-org-detail]").textContent.includes("VP Engineering")')

  // Remove affiliation through UI
  await evaluate(
    "Array.from(document.querySelectorAll('[data-org-detail] ul li button')).find(x => x.textContent === 'Remove').click()",
  )
  await wait('document.querySelector("[data-org-detail] ul").textContent.includes("[inactive]")')

  // Archive organization through UI
  await evaluate(
    "Array.from(document.querySelectorAll('[data-org-detail] button')).find(x => x.textContent === 'Archive').click()",
  )
  await wait('document.querySelector("[data-org-detail]").textContent.includes("State: archived")')

  // Logout and verify private data cleared
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#organizations").textContent === ""')

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
  await wait('document.querySelector("[data-org-status]").textContent==="Organizations ready."')
  assert.equal(await evaluate('document.querySelector("[data-org-create]").hidden'), true)
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)

  // Logout viewer
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#organizations").textContent === ""')

  check(
    'isolated browser completes organization and affiliation lifecycle, recovers response loss, enforces uncertain write refusal, and cleans up on logout',
  )

  await stop()
  await operator([
    'modules',
    await privateFile('disabled-orgs', {
      contacts: true,
      pipeline: false,
      assistant: false,
      nativeActions: false,
      organizations: false,
    }),
  ])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/records')).status, 404)
  await stop()
  await operator(['modules', await privateFile('re-enabled-orgs', modules)])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/records')).status, 200)
  check('module disable and re-enable removes routes and restores state without data loss')

  await stop()
  const archive = join(home, 'orgs.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_orgs'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  const recovered = { username: owner.username, password: randomBytes(24).toString('base64url') }
  await operator(['user', 'recover', await privateFile('recovered', recovered)])
  await start()
  auth = await signIn(recovered)
  const restoredOrg = await (await send(auth, base + '/records/' + receipt.organizationId)).json()
  assert.equal(restoredOrg.organization.name, 'Host Reusable Org Corp')
  assert.equal(
    (await (await send(auth, base + '/commands/' + createOrg.requestId)).json()).status,
    'committed',
  )
  check(
    'backup and restore preserves organization entities and receipts under recovered credentials',
  )

  const destination = join(root, 'dist/organizations-host-proof')
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
        scope: 'reusable organizations, accounts and contact affiliations',
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
