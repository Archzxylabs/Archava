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
const home = await mkdtemp(join(tmpdir(), 'archava-project-host-'))
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
const projectBase = '/api/projects/studio-demo'
const command = (value) => ({ requestId: randomUUID(), command: value })
try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json')
  const settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8'))
  const viewer = {
    username: 'viewer@project.local',
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
    projects: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + projectBase + '/records')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, projectBase + '/records')).status, 403)
  check(
    'project routes require login and enabling the module never grants domain access automatically',
  )
  await stop()
  for (const [account, access] of [
    [owner, 'write'],
    [viewer, 'read'],
  ])
    await operator([
      'user',
      'projects',
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
      operation: 'project.create',
      title: 'Host acceptance project',
      scopeSummary: 'Agreed fixture delivery',
    })
  assert.equal(
    (await send(auth, projectBase + '/commands', create, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (await send(auth, projectBase + '/commands', create, { origin: 'https://evil.test' })).status,
    403,
  )
  assert.equal((await send(auth, '/api/projects/unconfigured/records')).status, 404)
  assert.equal((await send(v, projectBase + '/commands', create)).status, 403)
  check('host enforces CSRF, same origin, configured tenants and viewer write refusal')
  const created = await send(auth, projectBase + '/commands', create)
  assert.equal(created.status, 200)
  const receipt = await created.json()
  const readTool = { name: 'projects.detail', input: { id: receipt.id } }
  assert.equal((await send(v, projectBase + '/tools', readTool)).status, 200)
  assert.equal(
    (
      await send(auth, projectBase + '/tools', {
        ...readTool,
        input: { id: receipt.id, tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (
      await send(auth, projectBase + '/tools', {
        name: 'projects.create',
        input: { title: 'Unregistered write' },
      })
    ).status,
    404,
  )
  check(
    'authenticated project tools allow current scoped reads and refuse model scope overrides or write tools',
  )
  assert.equal((await send(v, projectBase + '/records/' + receipt.id)).status, 200)
  assert.equal((await send(auth, '/api/projects/hotel-demo/records/' + receipt.id)).status, 403)
  check('owner and viewer access follows explicit per-tenant domain grants')
  await stop()
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, projectBase + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  check('actual owner host restart retains project records and command recovery')

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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Project UI timed out'));else setTimeout(tick,100)};tick()})`,
    )
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#projects").hidden && document.querySelector("#projects").textContent===""',
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
    '!!document.querySelector("[data-project-create]") && document.querySelector("[data-project-status]").textContent==="Up to date."',
  )
  assert.equal(
    await evaluate(
      'document.querySelector("#pipeline").hidden && document.querySelector("#conversation").hidden && !document.querySelector("#projects").hidden',
    ),
    true,
  )
  check('browser mounts projects in the configured composition and hides them before login')
  await fill('[data-project-create] [name=title]', 'Browser project <img src=x onerror=alert(1)>')
  await fill('[data-project-create] [name=scope]', 'Three agreed pages')
  // The server commits; the browser loses the response. Recovery must not create a second project.
  await evaluate(
    `window.projectFetch=window.fetch;window.projectDrop=true;window.projectRetryBlocked=false;window.fetch=async(...args)=>{if(window.projectRetryBlocked&&String(args[0]).includes('/api/projects/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.projectFetch(...args);if(window.projectDrop&&String(args[0]).includes('/api/projects/')&&args[1]?.method==='POST'){window.projectDrop=false;throw new Error('Lost fixture response')}return response};document.querySelector('[data-project-create]').requestSubmit()`,
  )
  await wait(
    'document.querySelector("[data-project-status]").textContent.includes("Lost fixture response")',
  )
  await evaluate(
    `window.projectRetryBlocked=true;Array.from(document.querySelectorAll('#projects button')).find(x=>x.textContent==='Retry same request').click()`,
  )
  await wait('document.querySelector("[data-project-status]").textContent.includes("permissions")')
  await evaluate('document.querySelector("[data-project-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-project-status]").textContent.includes("Resolve the pending")',
  )
  await evaluate('window.projectRetryBlocked=false')
  await evaluate(
    `Array.from(document.querySelectorAll('#projects button')).find(x=>x.textContent==='Check pending result').click()`,
  )
  await wait(
    'document.querySelector("[data-project-detail]").textContent.includes("Browser project") && document.querySelector("[data-project-status]").textContent==="Up to date."',
  )
  const snapshot = await (await send(auth, projectBase + '/records?search=Browser')).json()
  assert.equal(snapshot.items.length, 1)
  assert.equal(
    await evaluate('document.querySelector("[data-project-detail]").querySelector("img")===null'),
    true,
  )
  await evaluate('window.fetch=window.projectFetch;delete window.projectFetch')
  check(
    'browser recovers a committed response loss without duplicate creation and renders business text safely',
  )
  const submit = async (selector) => {
    await evaluate(`document.querySelector(${JSON.stringify(selector)}).requestSubmit()`)
    await wait('document.querySelector("[data-project-status]").textContent!=="Working…"')
  }
  await fill('[data-project-transition] select', 'active')
  await submit('[data-project-transition]')
  await fill('[data-project-milestone-create] [name=title]', 'Reviewed delivery')
  await submit('[data-project-milestone-create]')
  await fill('[data-project-task-create] [name=title]', 'Complete agreed work')
  await evaluate('document.querySelector("[data-project-task-create] select").selectedIndex=1')
  await submit('[data-project-task-create]')
  await fill('[data-project-transition] select', 'review')
  await submit('[data-project-transition]')
  await fill('[data-project-transition] select', 'delivered')
  await submit('[data-project-transition]')
  assert.equal(
    await evaluate(
      'document.querySelector("[data-project-status]").textContent.includes("incomplete")',
    ),
    true,
  )
  await fill('[data-project-task-transition] select', 'done')
  await submit('[data-project-task-transition]')
  await fill('[data-project-milestone-complete] [name=evidence]', 'Reviewed agreed work')
  await submit('[data-project-milestone-complete]')
  await fill('[data-project-transition] select', 'delivered')
  await submit('[data-project-transition]')
  assert.equal(
    await evaluate(
      'document.querySelector("[data-project-detail]").textContent.includes("Status: delivered") && !document.querySelector("[data-project-task-create]")',
    ),
    true,
  )
  check(
    'browser completes real project/task/milestone flow and the backend refuses premature delivery',
  )
  await evaluate('document.querySelector("#logout").click()')
  await wait(
    'document.querySelector("#projects").textContent==="" && !document.querySelector("#login").hidden',
  )
  await fill('#login [name=username]', viewer.username)
  await fill('#login [name=password]', viewer.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait('document.querySelector("[data-project-status]")?.textContent==="Up to date."')
  assert.equal(await evaluate('document.querySelector("[data-project-create]").hidden'), true)
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check(
    'logout clears project data and the viewer dashboard hides write controls without browser exceptions',
  )
  await page.close()
  page = null
  const browserStopped = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await browserStopped
  browser = null

  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, projects: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, projectBase + '/records')).status, 404)
  await stop()
  await operator(['modules', await privateFile('reenabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    (await (await send(auth, projectBase + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  check('disable and re-enable remove project routes while retaining projects, grants and receipts')
  await stop()
  const archive = join(home, 'host-projects.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_projects'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  await operator([
    'user',
    'recover',
    await privateFile('recovered', {
      username: owner.username,
      password: randomBytes(24).toString('base64url'),
    }),
  ])
  const recovered = JSON.parse(await readFile(join(home, 'recovered.json'), 'utf8'))
  await start()
  auth = await signIn(recovered)
  assert.deepEqual(
    (await (await send(auth, projectBase + '/commands/' + create.requestId)).json()).receipt,
    receipt,
  )
  check(
    'owner-host backup restore preserves project grants and receipts after explicit identity recovery',
  )
  const destination = join(root, 'dist/project-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'internal project delivery',
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
