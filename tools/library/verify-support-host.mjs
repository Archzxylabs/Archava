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
const home = await mkdtemp(join(tmpdir(), 'archava-support-host-'))
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
const base = '/api/support/studio-demo'
const command = (value) => ({ requestId: randomUUID(), command: value })
try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@support.local',
      password: randomBytes(24).toString('base64url'),
      role: 'viewer',
      tenants: ['studio-demo'],
    },
    staff = {
      username: 'staff@support.local',
      password: randomBytes(24).toString('base64url'),
      role: 'staff',
      tenants: ['studio-demo'],
    }
  for (const user of [viewer, staff])
    await operator(['user', 'add', await privateFile(user.role, user)])
  const modules = {
    contacts: true,
    pipeline: false,
    assistant: false,
    nativeActions: false,
    support: true,
  }
  await operator(['modules', await privateFile('modules', modules)])
  await start()
  assert.equal((await fetch(origin + base + '/cases')).status, 401)
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/cases')).status, 403)
  check(
    'support requires login and independent explicit domain grants; enabling flags never enrolls accounts',
  )
  await stop()
  for (const [account, access, scope] of [
    [owner, 'write', 'all'],
    [viewer, 'read', 'all'],
    [staff, 'write', 'assigned'],
  ])
    await operator([
      'user',
      'support',
      await privateFile('support-' + account.role, {
        username: account.username,
        tenants: ['studio-demo'],
        access,
        scope,
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer),
    s = await signIn(staff),
    create = command({
      operation: 'case.create',
      requesterName: 'Host customer',
      subject: 'Host issue',
      description: 'Investigate locally',
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
  assert.equal((await send(auth, '/api/support/unconfigured/cases')).status, 404)
  const response = await send(auth, base + '/commands', create)
  assert.equal(response.status, 200)
  const receipt = await response.json()
  assert.equal((await send(v, base + '/tools', { name: 'support.cases', input: {} })).status, 200)
  assert.equal(
    (
      await send(v, base + '/tools', {
        name: 'support.detail',
        input: { id: receipt.id, tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  assert.equal(
    (await send(auth, base + '/tools', { name: 'support.case.resolve', input: { id: receipt.id } }))
      .status,
    404,
  )
  assert.equal((await send(s, base + '/cases/' + receipt.id)).status, 404)
  const candidates = await (await send(auth, base + '/assignees')).json(),
    staffId = candidates.items.find((x) => x.displayName === staff.username)?.id
  assert.ok(staffId)
  assert.equal((await send(s, base + '/assignees')).status, 403)
  assert.equal((await send(v, base + '/assignees')).status, 403)
  check(
    'actual host enforces CSRF, same origin, configured workspace, assigned scope and private read-only tools; only assignment administrators see the human directory',
  )
  await stop()
  await start()
  auth = await signIn(owner)
  assert.deepEqual(
    {
      ...(await (await send(auth, base + '/commands/' + create.requestId)).json()).receipt,
      replayed: false,
    },
    receipt,
  )
  check('host restart retains support case, immutable history and original actor-bound receipt')
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
        `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>150)fail(new Error('Support UI timed out: '+${JSON.stringify(condition)}));else setTimeout(tick,100)};tick()})`,
      ),
    fill = (selector, value) =>
      evaluate(
        `document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`,
      ),
    status = 'document.querySelector("[data-support-status]").textContent'
  const click = async (text) => {
    await evaluate(
      `Array.from(document.querySelectorAll('#support button')).find(x=>x.textContent===${JSON.stringify(text)}).click()`,
    )
    await wait('!Array.from(document.querySelectorAll("#support button")).some(x=>x.disabled)')
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
    await wait('!!document.querySelector("[data-support-list]")&&' + status + '==="Support ready."')
  }
  await wait('!!document.querySelector("#login")')
  assert.equal(
    await evaluate(
      'document.querySelector("#support").hidden&&document.querySelector("#support").textContent===""',
    ),
    true,
  )
  await login(owner)
  await fill('[data-support-create] [name=requesterName]', 'Browser requester')
  await fill('[data-support-create] [name=subject]', 'Browser issue <img src=x onerror=alert(1)>')
  await fill('[data-support-create] [name=description]', 'Investigate <script>data only</script>')
  await evaluate(
    "window.supportFetch=window.fetch;window.supportDrop=true;window.supportBlocked=false;window.fetch=async(...args)=>{if(window.supportBlocked&&String(args[0]).includes('/api/support/')&&args[1]?.method==='POST')return new Response('{}',{status:403});const response=await window.supportFetch(...args);if(window.supportDrop&&String(args[0]).includes('/api/support/')&&args[1]?.method==='POST'){window.supportDrop=false;throw new Error('Lost support response')}return response};document.querySelector('[data-support-create]').requestSubmit()",
  )
  await wait(status + '.includes("Lost support response")')
  await evaluate('window.supportBlocked=true')
  await click('Retry same request')
  await wait(status + '.includes("403")')
  await evaluate('window.supportBlocked=false')
  await evaluate('document.querySelector("[data-support-create]").requestSubmit()')
  await wait(status + '.includes("Resolve the pending request first")')
  await click('Check pending result')
  await wait(
    'document.querySelector("[data-support-detail] h3").textContent.includes("Browser issue")&&' +
      status +
      '==="Support ready."',
  )
  await evaluate('window.fetch=window.supportFetch;delete window.supportFetch')
  const cases = (await (await send(auth, base + '/cases')).json()).items
  assert.equal(cases.length, 2)
  const record = cases.find((x) => x.subject.includes('Browser issue'))
  assert.ok(record)
  assert.equal(
    await evaluate(
      'document.querySelector("[data-support-list]").querySelector("img")===null&&document.querySelector("[data-support-detail]").querySelector("script")===null',
    ),
    true,
  )
  check(
    'Chromium creates one support case without HTML injection and recovers committed response loss while denied retry retains the pending UUID and blocks new effects',
  )
  await fill('[data-support-detail] [name=reason]', 'Assign qualified operator')
  await fill('[data-support-detail] [name=assigneeId]', staffId)
  await click('Update assignment')
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes(' +
      JSON.stringify(staff.username) +
      ')&&' +
      status +
      '==="Support ready."',
  )
  const staffAuth = await signIn(staff)
  assert.equal((await send(staffAuth, base + '/cases/' + record.id)).status, 200)
  await fill('[data-support-note] [name=body]', 'Root cause found <img src=x onerror=alert(1)>')
  await evaluate('document.querySelector("[data-support-note]").requestSubmit()')
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes("Root cause found")&&' +
      status +
      '==="Support ready."',
  )
  assert.equal(
    await evaluate('document.querySelector("[data-support-detail]").querySelector("img")===null'),
    true,
  )
  check(
    'browser assigns only eligible operators and records immutable internal findings through actual authorized services without customer-message claims',
  )
  await click('Start investigation')
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes("in_progress")&&' +
      status +
      '==="Support ready."',
  )
  await fill('[data-support-detail] [name=reason]', 'Waiting for local details')
  await click('Wait for information')
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes("waiting")&&' +
      status +
      '==="Support ready."',
  )
  await click('Resume investigation')
  await fill('[data-support-detail] [name=resolution]', 'Original defect corrected')
  await evaluate(
    'document.querySelector("[data-support-detail] [name=resolution]").closest("form").requestSubmit()',
  )
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes("resolved")&&' +
      status +
      '==="Support ready."',
  )
  await click('Close case')
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes("closed")&&' +
      status +
      '==="Support ready."',
  )
  await fill('[data-support-detail] form [name=reason]', 'Further investigation requested')
  await evaluate(
    'document.querySelector("[data-support-detail] form [name=reason]").closest("form").requestSubmit()',
  )
  await wait(
    'document.querySelector("[data-support-detail]").textContent.includes("cycle 2")&&' +
      status +
      '==="Support ready."',
  )
  const reopened = await (await send(auth, base + '/cases/' + record.id)).json()
  assert.equal(reopened.record.state, 'open')
  assert.equal(reopened.record.resolution, null)
  assert.ok(reopened.transitions.some((x) => x.previousResolution === 'Original defect corrected'))
  check(
    'Chromium investigates, waits, resumes, resolves, closes and reopens a new cycle while preserving the original resolution and history',
  )
  // Assignee access disappears after global operator unassigns the resource.
  await fill('[data-support-detail] [name=reason]', 'Unassign completed handoff')
  await fill('[data-support-detail] [name=assigneeId]', '')
  await click('Update assignment')
  assert.equal((await send(staffAuth, base + '/cases/' + record.id)).status, 404)
  await evaluate('document.querySelector("#logout").click()')
  await wait('document.querySelector("#support").textContent===""')
  await login(viewer)
  assert.equal(await evaluate('document.querySelector("[data-support-create]").hidden'), true)
  await click('Open case')
  assert.equal(
    await evaluate(
      'document.querySelector("[data-support-detail]").querySelectorAll("form").length===0',
    ),
    true,
  )
  assert.equal(page.console_.filter((x) => x.kind === 'exception').length, 0)
  check(
    'reassignment immediately revokes staff resource access; logout clears private case data and viewer UI hides all write forms without browser exceptions',
  )
  await page.close()
  page = null
  const stoppedBrowser = new Promise((done) => browser.once('exit', done))
  browser.kill('SIGTERM')
  await stoppedBrowser
  browser = null
  await stop()
  await operator(['modules', await privateFile('disabled', { ...modules, support: false })])
  await start()
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/cases')).status, 404)
  await stop()
  await operator(['modules', await privateFile('enabled', modules)])
  await start()
  auth = await signIn(owner)
  assert.equal((await (await send(auth, base + '/cases/' + record.id)).json()).record.cycle, 2)
  assert.equal(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).status,
    'committed',
  )
  check(
    'disable and re-enable remove support routes while retaining assignment, resolution cycles, private notes, grants and receipt recovery',
  )
  await stop()
  const archive = join(home, 'support.dump')
  await operator(['backup', archive])
  await operator(['restore', archive, 'restored_support'])
  env.ARCHAVA_CRM_PROFILE = 'restored-runtime.json'
  const recovered = { username: owner.username, password: randomBytes(24).toString('base64url') }
  await operator(['user', 'recover', await privateFile('recovered', recovered)])
  await start()
  auth = await signIn(recovered)
  const restored = await (await send(auth, base + '/cases/' + record.id)).json()
  assert.equal(restored.record.cycle, 2)
  assert.equal(restored.notes.length, 1)
  assert.ok(restored.transitions.some((x) => x.previousResolution === 'Original defect corrected'))
  assert.equal(
    (await (await send(auth, base + '/commands/' + create.requestId)).json()).status,
    'committed',
  )
  check(
    'actual owner-host backup restore retains case cycles, notes, historical resolutions and original receipts after explicit identity recovery',
  )
  const destination = join(root, 'dist/support-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope: 'bounded internal support cases, assignment and resolution cycles',
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
