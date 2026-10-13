/* global process, fetch, setTimeout, Buffer */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { spawn, execFileSync } from 'node:child_process'
import { createServer } from 'node:net'
import { randomBytes, randomUUID, createHmac, hkdfSync } from 'node:crypto'

const root = resolve(import.meta.dirname, '../..'),
  runtime = join(root, 'dist/crm-runtime')
const home = await mkdtemp(join(tmpdir(), 'archava-billing-host-'))
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
  assert.ok(ready, 'isolated billing host did not start')
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

const TENANT = 'studio-demo'
const base = '/api/billing/' + TENANT

/**
 * An independent re-implementation of the provider digest, kept in this proof
 * rather than imported from the domain, so that a change to the framing the
 * host verifies and the framing a real processor would compute are two
 * separate facts rather than one.
 */
const DOMAIN = 'archava/billing/provider-event/v1'
const frameHeader = (size) => {
  const header = Buffer.alloc(4)
  header.writeUInt32BE(size, 0)
  return header
}
const providerKey = (fingerprintKey, tenantId, providerId) =>
  Buffer.from(
    hkdfSync(
      'sha256',
      Buffer.from(fingerprintKey, 'utf8'),
      Buffer.from(tenantId, 'utf8'),
      Buffer.from(DOMAIN + '|' + providerId, 'utf8'),
      32,
    ),
  )
const signEvent = (fingerprintKey, claim) => {
  const parts = [
    Buffer.from(DOMAIN, 'utf8'),
    ...[
      'tenantId',
      'invoiceId',
      'providerId',
      'providerEventId',
      'outcome',
      'amountMinor',
      'currency',
    ].map((key) => Buffer.from(String(claim[key]), 'utf8')),
  ]
  const framed = parts.flatMap((part) => [part, frameHeader(part.length)])
  return createHmac('sha256', providerKey(fingerprintKey, claim.tenantId, claim.providerId))
    .update(Buffer.concat(framed))
    .digest('hex')
}

const createInvoiceCmd = {
  type: 'create_invoice',
  commandId: randomUUID(),
  invoiceCode: 'INV-API-0001',
  customerRef: 'patient-aria',
  currency: 'IDR',
  consumerRef: 'clinics',
  lines: [{ description: 'Specialist consultation', quantity: 2, unitMinor: 250000 }],
}

try {
  await operator(['setup'])
  const settingsPath = join(home, 'runtime.json'),
    settings = JSON.parse(await readFile(settingsPath, 'utf8'))
  const fingerprintKey = settings.fingerprintKey
  assert.ok(fingerprintKey, 'host settings must carry the master key')
  settings.appPort = 0
  await writeFile(settingsPath, JSON.stringify(settings), { mode: 0o600 })
  const owner = JSON.parse(await readFile(join(home, 'initial-owner.json'), 'utf8')),
    viewer = {
      username: 'viewer@bl.local',
      password: randomBytes(24).toString('base64url'),
      role: 'viewer',
      tenants: [TENANT],
    }
  await operator(['user', 'add', await privateFile('viewer', viewer)])
  const off = {
    contacts: true,
    pipeline: false,
    assistant: false,
    nativeActions: false,
    attendance: false,
    billing: false,
  }
  const on = { ...off, billing: true }

  // Phase 1: the module is off, so the whole route surface is absent.
  await operator(['modules', await privateFile('modules-off', off)])
  await start()
  let auth = await signIn(owner)
  assert.equal((await send(auth, base + '/invoices')).status, 404)
  await stop()
  check('billing routes are absent while the module is disabled')

  // Phase 2: the module is on, but enabling it never auto-enrolls an account.
  await operator(['modules', await privateFile('modules-on', on)])
  await start()
  assert.equal((await fetch(origin + base + '/invoices')).status, 401)
  auth = await signIn(owner)
  assert.equal((await send(auth, base + '/access')).status, 403)
  assert.equal((await send(auth, base + '/invoices')).status, 403)
  await stop()
  check(
    'billing routes require authentication and domain grants; enabling the module never auto-enrolls accounts',
  )

  // Phase 3: enroll two principals and exercise every boundary at the API.
  for (const [file, account, access] of [
    ['bl-owner', owner, 'write'],
    ['bl-viewer', viewer, 'read'],
  ])
    await operator([
      'user',
      'billing',
      await privateFile(file, {
        username: account.username,
        tenants: [TENANT],
        access,
        scope: 'all',
      }),
    ])
  await start()
  auth = await signIn(owner)
  const v = await signIn(viewer)
  assert.equal((await send(auth, base + '/access')).status, 200)
  assert.equal((await send(v, base + '/access')).status, 200)
  assert.equal(
    (await send(auth, base + '/commands', createInvoiceCmd, { 'x-archava-csrf': 'forged' })).status,
    403,
  )
  assert.equal(
    (
      await send(auth, base + '/commands', createInvoiceCmd, {
        origin: 'https://evil.test',
      })
    ).status,
    403,
  )
  assert.equal((await send(v, base + '/commands', createInvoiceCmd)).status, 403)
  assert.equal((await send(auth, '/api/billing/unconfigured/invoices')).status, 404)
  assert.equal(
    (await send(auth, base + '/tools', { tool: 'billing.settleNow', input: {} })).status,
    404,
  )
  assert.equal(
    (
      await send(auth, base + '/tools', {
        tool: 'billing.listInvoices',
        input: { tenant: 'hotel-demo' },
      })
    ).status,
    400,
  )
  const createRes = await send(auth, base + '/commands', createInvoiceCmd)
  assert.equal(createRes.status, 200)
  const receipt = await createRes.json()
  assert.ok(receipt.invoiceId)
  assert.equal(receipt.status, 'draft')
  assert.equal(receipt.revision, 1)
  assert.equal((await send(auth, base + '/invoices/' + receipt.invoiceId)).status, 200)
  for (const area of ['lines', 'corrections', 'attempts', 'events'])
    assert.equal((await send(auth, base + '/' + area + '/' + receipt.invoiceId)).status, 200)
  check(
    'compiled host enforces CSRF, origin check, viewer write refusal, unknown tool refusal and cross-tenant tool input',
  )

  // Phase 4: the settlement boundary itself, at the API surface.
  const issueRes = await send(auth, base + '/commands', {
    type: 'issue_invoice',
    commandId: randomUUID(),
    invoiceId: receipt.invoiceId,
    expectedRevision: 1,
    reason: 'Encounter closed',
  })
  assert.equal(issueRes.status, 200)
  assert.equal((await issueRes.json()).status, 'issued')

  const attemptRes = await send(auth, base + '/commands', {
    type: 'record_payment_attempt',
    commandId: randomUUID(),
    invoiceId: receipt.invoiceId,
    amountMinor: 500000,
    providerRef: 'pi_api_probe',
  })
  assert.equal(attemptRes.status, 200)
  const afterAttempt = await (await send(auth, base + '/invoices/' + receipt.invoiceId)).json()
  assert.equal(afterAttempt.invoice.status, 'issued')
  assert.equal(afterAttempt.invoice.settledMinor, 0)
  assert.equal(afterAttempt.invoice.outstandingMinor, 500000)
  assert.equal(afterAttempt.attempts.length, 1)
  assert.equal(afterAttempt.events.length, 0)

  const claim = {
    tenantId: TENANT,
    invoiceId: receipt.invoiceId,
    providerId: 'acquirer-demo',
    providerEventId: 'evt_api_1',
    outcome: 'succeeded',
    amountMinor: 500000,
    currency: 'IDR',
  }
  // `tenantId` is part of what gets signed, but not part of the request: the
  // tenant comes from the route, so sending it in the body would be a strict
  // schema rejection rather than a signature verdict.
  const eventCommand = (signature) => ({
    type: 'record_provider_event',
    invoiceId: claim.invoiceId,
    providerId: claim.providerId,
    providerEventId: claim.providerEventId,
    outcome: claim.outcome,
    amountMinor: claim.amountMinor,
    currency: claim.currency,
    commandId: randomUUID(),
    signature,
  })
  const forged = await send(auth, base + '/commands', eventCommand('a'.repeat(64)))
  assert.equal(forged.status, 403)
  const afterForged = await (await send(auth, base + '/invoices/' + receipt.invoiceId)).json()
  assert.equal(afterForged.invoice.status, 'issued')
  assert.equal(afterForged.invoice.settledMinor, 0)
  assert.equal(afterForged.events.length, 0)

  const genuine = await send(
    auth,
    base + '/commands',
    eventCommand(signEvent(fingerprintKey, claim)),
  )
  assert.equal(genuine.status, 200)
  assert.equal((await genuine.json()).status, 'settled')
  const settled = await (await send(auth, base + '/invoices/' + receipt.invoiceId)).json()
  assert.equal(settled.invoice.settledMinor, 500000)
  assert.equal(settled.invoice.outstandingMinor, 0)
  assert.equal(settled.invoice.status, 'settled')
  assert.equal(settled.events.length, 1)
  check(
    'a payment attempt settles nothing, a forged provider signature is refused, and only a correctly signed notification settles an issued invoice',
  )

  // Phase 5: a restart must retain state, memberships and the idempotent receipt.
  await stop()
  await start()
  auth = await signIn(owner)
  const statusCheck = await send(auth, base + '/commands/' + createInvoiceCmd.commandId)
  assert.equal(statusCheck.status, 200)
  const statusJson = await statusCheck.json()
  assert.equal(statusJson.status, 'recorded')
  assert.deepEqual(statusJson.receipt, receipt)
  const replayed = await send(auth, base + '/commands', createInvoiceCmd)
  assert.equal(replayed.status, 200)
  assert.equal((await replayed.json()).idempotencyReplay, true)
  const retained = await (await send(auth, base + '/invoices/' + receipt.invoiceId)).json()
  assert.equal(retained.invoice.status, 'settled')
  assert.equal(retained.invoice.settledMinor, 500000)
  const readBack = await signIn(viewer)
  assert.equal((await send(readBack, base + '/invoices/' + receipt.invoiceId)).status, 200)
  check('host restart retains billing state, settled balance and idempotent receipt recovery')

  // Phase 6: isolated Chromium drives the dashboard through the workflow.
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
      `new Promise((done,fail)=>{let tries=0;const tick=()=>{if(${condition})done(true);else if(++tries>180)fail(new Error('UI timed out for [' + ${JSON.stringify(condition)} + '], status: ' + document.querySelector("[data-billing-status]")?.textContent));else setTimeout(tick,100)};tick()})`,
    )
  const idle = () => wait('!document.querySelector("#billing")?.dataset?.busy')
  const fill = (selector, value) =>
    evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
  const clickButton = (scope, label) =>
    evaluate(
      `Array.from(document.querySelectorAll(${JSON.stringify(scope)})).find((b)=>b.textContent===${JSON.stringify(label)}).click()`,
    )
  const selectTenant = async () => {
    await evaluate(
      `document.querySelector("#tenants").value=${JSON.stringify(TENANT)};document.querySelector("#tenants").dispatchEvent(new Event("change"))`,
    )
    await wait('!document.querySelector("#billing").hidden')
    await idle()
  }
  const detailStatus = () =>
    evaluate('document.querySelector("#billing [data-billing-detail] h3").textContent')
  const invoiceIdFor = (code) =>
    evaluate(
      `(()=>{const li=Array.from(document.querySelectorAll('[data-billing-item]')).find((n)=>n.textContent.includes(${JSON.stringify(code)}));return li?li.dataset.billingItem:null})()`,
    )

  await wait('!!document.querySelector("#login")')
  await fill('#login [name=username]', owner.username)
  await fill('#login [name=password]', owner.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!document.querySelector("#workspace").hidden && document.querySelector("#status").textContent===""',
  )
  await selectTenant()
  assert.equal(await evaluate('document.querySelector("[data-billing-recovery]").hidden'), true)

  await fill('#billing [data-billing-create] [name=invoiceCode]', 'INV-UI-0001')
  await fill('#billing [data-billing-create] [name=customerRef]', 'client-ui')
  await fill('#billing [data-billing-create] [name=consumerRef]', 'clinics')
  await fill('#billing [data-billing-create] [name=line1Description]', 'Browser billed item')
  await fill('#billing [data-billing-create] [name=line1Quantity]', '1')
  await fill('#billing [data-billing-create] [name=line1UnitMinor]', '500000')
  await clickButton('#billing [data-billing-create] button', 'Create Invoice')
  await idle()
  assert.equal(
    (await evaluate('document.querySelector("[data-billing-status]").textContent')).startsWith(
      'Command committed: create_invoice',
    ),
    true,
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-billing-list]").textContent.includes("INV-UI-0001")',
    ),
    true,
  )
  await evaluate(`
    Array.from(document.querySelectorAll('[data-billing-list] li'))
      .find((li) => li.textContent.includes('INV-UI-0001'))
      .querySelector('button').click()
  `)
  await wait('!!document.querySelector("#billing [data-billing-detail] h3")')
  await idle()
  assert.equal(await detailStatus(), 'INV-UI-0001 — DRAFT')
  check('browser DOM dashboard creates a priced draft invoice and opens its snapshot')

  // The client holds no control that can assert a payment result.
  assert.equal(
    await evaluate(
      `Array.from(document.querySelectorAll('#billing button')).some((b)=>/mark\\s+paid|settle\\s+now|confirm\\s+payment|mark\\s+as\\s+paid/i.test(b.textContent))`,
    ),
    false,
  )
  assert.equal(
    await evaluate(
      `document.querySelector('#billing [data-billing-event-form] h4').textContent.includes('only a correctly signed notification settles this invoice')`,
    ),
    true,
  )
  assert.equal(
    await evaluate(
      `document.querySelector('#billing [data-billing-attempt-form] h4').textContent.includes('settles nothing')`,
    ),
    true,
  )
  const uiInvoiceId = await invoiceIdFor('INV-UI-0001')
  assert.ok(uiInvoiceId, 'the dashboard must expose the invoice it just created')

  // Recording an attempt from the browser leaves the invoice untouched.
  await fill('#billing [data-billing-attempt-form] [name=providerRef]', 'pi_ui_probe')
  await clickButton('#billing [data-billing-attempt-form] button', 'Record Payment Attempt')
  await idle()
  assert.equal(await detailStatus(), 'INV-UI-0001 — DRAFT')
  let uiRecord = await (await send(auth, base + '/invoices/' + uiInvoiceId)).json()
  assert.equal(uiRecord.invoice.status, 'draft')
  assert.equal(uiRecord.invoice.settledMinor, 0)
  assert.equal(uiRecord.attempts.length, 1)
  assert.equal(uiRecord.events.length, 0)

  await fill('#billing [data-billing-issue-form] [name=reason]', 'Approved and billed')
  await clickButton('#billing [data-billing-issue-form] button', 'Issue Invoice')
  await idle()
  assert.equal(await detailStatus(), 'INV-UI-0001 — ISSUED')

  await fill('#billing [data-billing-attempt-form] [name=providerRef]', 'pi_ui_probe_2')
  await clickButton('#billing [data-billing-attempt-form] button', 'Record Payment Attempt')
  await idle()
  assert.equal(await detailStatus(), 'INV-UI-0001 — ISSUED')
  uiRecord = await (await send(auth, base + '/invoices/' + uiInvoiceId)).json()
  assert.equal(uiRecord.invoice.status, 'issued')
  assert.equal(uiRecord.invoice.settledMinor, 0)
  assert.equal(uiRecord.invoice.outstandingMinor, 500000)
  assert.equal(uiRecord.attempts.length, 2)
  assert.equal(uiRecord.events.length, 0)
  check('the browser can record payment intent but every attempt leaves the invoice unsettled')

  // A forged signature is refused at the host boundary and settles nothing.
  await fill('#billing [data-billing-event-form] [name=providerEventId]', 'evt_ui_forged')
  await fill('#billing [data-billing-event-form] [name=signature]', 'a'.repeat(64))
  await clickButton('#billing [data-billing-event-form] button', 'Record Provider Event')
  await idle()
  assert.match(
    await evaluate('document.querySelector("[data-billing-status]").textContent'),
    /HTTP 403/,
  )
  assert.equal(await evaluate('!document.querySelector("[data-billing-recovery]").hidden'), true)
  uiRecord = await (await send(auth, base + '/invoices/' + uiInvoiceId)).json()
  assert.equal(uiRecord.invoice.status, 'issued')
  assert.equal(uiRecord.invoice.settledMinor, 0)
  assert.equal(uiRecord.events.length, 0)

  // A refusal leaves the request uncertain, so it is reconciled by reloading.
  await page.navigate(origin)
  await wait('!!document.querySelector("#login")')
  await wait(
    '!document.querySelector("#workspace").hidden && document.querySelector("#status").textContent===""',
  )
  await selectTenant()
  assert.equal(await evaluate('document.querySelector("[data-billing-recovery]").hidden'), true)
  await evaluate(`
    Array.from(document.querySelectorAll('[data-billing-list] li'))
      .find((li) => li.textContent.includes('INV-UI-0001'))
      .querySelector('button').click()
  `)
  await wait('!!document.querySelector("#billing [data-billing-detail] h3")')
  await idle()
  assert.equal(await detailStatus(), 'INV-UI-0001 — ISSUED')
  check('an unsigned provider notification is refused by the host and settles nothing')

  // Only a correctly signed notification settles, and the dashboard says so.
  const uiClaim = {
    tenantId: TENANT,
    invoiceId: uiInvoiceId,
    providerId: 'acquirer-demo',
    providerEventId: 'evt_ui_1',
    outcome: 'succeeded',
    amountMinor: 500000,
    currency: 'IDR',
  }
  await fill('#billing [data-billing-event-form] [name=providerId]', uiClaim.providerId)
  await fill('#billing [data-billing-event-form] [name=providerEventId]', uiClaim.providerEventId)
  await fill('#billing [data-billing-event-form] [name=amountMinor]', '500000')
  await fill(
    '#billing [data-billing-event-form] [name=signature]',
    signEvent(fingerprintKey, uiClaim),
  )
  await clickButton('#billing [data-billing-event-form] button', 'Record Provider Event')
  await idle()
  assert.equal(await detailStatus(), 'INV-UI-0001 — SETTLED')
  uiRecord = await (await send(auth, base + '/invoices/' + uiInvoiceId)).json()
  assert.equal(uiRecord.invoice.status, 'settled')
  assert.equal(uiRecord.invoice.settledMinor, 500000)
  assert.equal(uiRecord.invoice.grossMinor, 500000)
  assert.equal(uiRecord.events.length, 1)
  check('the browser settles an issued invoice only by replaying a correctly signed provider event')

  // Phase 7: every S1 failure mode must leave the request recoverable.
  const mode = (value) => evaluate(`window.__blMode=${JSON.stringify(value)}`)
  await evaluate(`
    window.__blMode = 'normal'
    const realFetch = window.fetch.bind(window)
    window.fetch = async (input, init) => {
      const url = String(input)
      const method = (init && init.method) || 'GET'
      const current = window.__blMode
      if (current === 'network') throw new TypeError('simulated network interruption')
      if (current === 'http500' && method === 'POST')
        return new Response('', { status: 500 })
      if (current === 'garbage' && method === 'POST')
        return new Response('not-json{', { status: 200, headers: { 'content-type': 'application/json' } })
      if (current === 'mismatch' && method === 'POST')
        return new Response(JSON.stringify({
          commandId: '00000000-0000-4000-8000-000000000000',
          tenantId: ${JSON.stringify(TENANT)}, action: 'create_invoice',
          invoiceId: '11111111-1111-4111-8111-111111111111',
          status: 'draft', revision: 1, executedAt: new Date().toISOString(),
        }), { status: 200, headers: { 'content-type': 'application/json' } })
      if (current === 'requery_down' && url.includes('/commands/'))
        throw new TypeError('simulated outage during recovery')
      const result = await realFetch(input, init)
      if (method === 'POST') {
        try {
          window.__blLastReq = String((init && init.body) || '')
          window.__blLastPost = result.status + ' ' + (await result.clone().text())
        } catch (error) {
          window.__blLastPost = String(error)
        }
      }
      return result
    }
  `)
  const recoveryVisible = () =>
    evaluate('!document.querySelector("[data-billing-recovery]").hidden')
  const statusText = () => evaluate('document.querySelector("[data-billing-status]").textContent')
  const recovery = '#billing [data-billing-recovery] button'

  // The reload above reset the create form to its defaults, so every required
  // field is filled again rather than relying on what a previous page held.
  await fill('#billing [data-billing-create] [name=invoiceCode]', 'INV-UI-S1')
  await fill('#billing [data-billing-create] [name=customerRef]', 'client-ui')
  await fill('#billing [data-billing-create] [name=consumerRef]', 'clinics')
  await fill('#billing [data-billing-create] [name=line1Description]', 'Browser billed item')
  await fill('#billing [data-billing-create] [name=line1Quantity]', '1')
  await fill('#billing [data-billing-create] [name=line1UnitMinor]', '500000')
  await mode('network')
  await clickButton('#billing [data-billing-create] button', 'Create Invoice')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Network interrupted while executing command/)

  await mode('http500')
  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Command failed: HTTP 500/)

  await mode('garbage')
  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Invalid or truncated command receipt/)

  await mode('mismatch')
  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Command receipt ID mismatch/)

  await mode('requery_down')
  await clickButton(recovery, 'Re-query status')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Network interrupted while executing recovery query/)

  await mode('normal')
  await clickButton(recovery, 'Re-query status')
  await idle()
  assert.equal(await recoveryVisible(), true)
  assert.match(await statusText(), /Command status is still unknown on the server/)

  await clickButton(recovery, 'Retry same request')
  await idle()
  assert.equal(await evaluate('document.querySelector("[data-billing-recovery]").hidden'), true)
  const diagnostics = await evaluate(
    'JSON.stringify({ req: window.__blLastReq, res: window.__blLastPost })',
  )
  assert.match(
    await statusText(),
    /Command committed: create_invoice/,
    'last request/response: ' + diagnostics,
  )
  assert.equal(
    await evaluate(
      'document.querySelector("[data-billing-list]").textContent.includes("INV-UI-S1")',
    ),
    true,
  )
  check('browser DOM dashboard survives all 5 S1 failure modes through idempotent receipt recovery')

  const destination = join(root, 'dist/billing-host-proof')
  await mkdir(destination, { recursive: true })
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'immutable invoice intent and snapshot, append-only correction chains and provider-authoritative settlement proven in the compiled host',
        checks,
        browser: 'isolated Chromium',
        providerCalls: 0,
        operationalKitsAccepted: [],
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(`\nBilling host proof verified with ${checks.length} checks\n`)
} finally {
  await page?.close()
  if (browser && browser.exitCode === null) browser.kill('SIGTERM')
  await stop()
  await rm(home, { recursive: true, force: true })
}
