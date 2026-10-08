/* global process, console */
import assert from 'node:assert/strict'
import { readFile, mkdir, writeFile } from 'node:fs/promises'
import { URL } from 'node:url'

process.env.CDP_PORT ??= '9459'
process.env.E2E_PROFILE ??= '/tmp/archava-crm-runtime-browser/profile'
const { launchDebugBrowser, Session } = await import('./cdp.mjs')
const { origin, credentials } = JSON.parse(
  await readFile('/tmp/archava-crm-runtime-browser-target.json', 'utf8'),
)
if (
  new URL(origin).hostname !== '127.0.0.1' ||
  !credentials.startsWith('/tmp/archava-crm-runtime-')
)
  throw new Error('Browser verifier accepts only its private runtime fixture')
const accounts = JSON.parse(await readFile(credentials, 'utf8'))
await mkdir('/tmp/archava-crm-runtime-browser/shots', { recursive: true })
await launchDebugBrowser()
const page = await Session.open()
const checks = []
const check = (label, condition) => {
  assert.ok(condition, label)
  checks.push(label)
  console.log('PASS ' + label)
}
const evaluate = (expr) => page.evaluate(expr)
const wait = (condition) =>
  evaluate(
    `new Promise((done,fail)=>{let attempts=0;const tick=()=>{if(${condition})done(true);else if(++attempts>100)fail(new Error('UI condition timed out'));else setTimeout(tick,100)};tick()})`,
  )
const fill = (selector, value) =>
  evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
async function signIn(account) {
  await fill('#login [name=username]', account.username)
  await fill('#login [name=password]', account.password)
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait(
    '!document.querySelector("#workspace").hidden && !!document.querySelector("[data-crm-contacts]") && document.querySelector("#status").textContent===""',
  )
}
async function logout() {
  await evaluate('document.querySelector("#logout").click()')
  await wait(
    '!document.querySelector("#login").hidden && document.querySelector("#workspace").hidden',
  )
}
try {
  await page.send('Network.deleteCookies', { name: 'archava_local_session', url: origin })
  await page.navigate(origin)
  await page.send('Log.clear')
  page.console_ = []
  check(
    'unauthenticated visitor sees sign-in and no business records',
    await evaluate(
      '!document.querySelector("#login").hidden && document.querySelector("#workspace").hidden && document.querySelector("#crm").textContent===""',
    ),
  )
  await fill('#login [name=username]', accounts.owner.username)
  await fill('#login [name=password]', 'this password is incorrect')
  await evaluate('document.querySelector("#login").requestSubmit()')
  await wait('document.querySelector("#status").textContent.includes("could not be verified")')
  check(
    'invalid credentials remain outside the workspace',
    await evaluate('document.querySelector("#workspace").hidden'),
  )
  await signIn(accounts.owner)
  await wait('document.querySelector("[data-crm-contacts]").textContent.includes("Owner contact")')
  check(
    'owner credentials open the persisted shared CRM dashboard',
    await evaluate(
      'document.querySelector("[data-crm-contacts]").textContent.includes("Staff contact") && document.querySelector("#tenants").options.length===2',
    ),
  )
  await page.navigate(origin)
  await wait('document.querySelector("[data-crm-contacts]")?.textContent.includes("Owner contact")')
  check(
    'reload keeps the authenticated session and database records',
    await evaluate('document.querySelector("#login").hidden'),
  )
  await fill('[data-crm-create] [name=fullName]', 'Browser permanent guest')
  await evaluate('document.querySelector("[data-crm-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-crm-detail]").textContent.includes("Browser permanent guest")',
  )
  check('shared dashboard writes through the session and CSRF-protected API', true)
  const noCsrf = await evaluate(
    "fetch('/api/crm/hotel-demo/commands',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({requestId:crypto.randomUUID(),command:{operation:'contact.create',data:{fullName:'Forbidden browser command'}}})}).then(response=>response.status)",
  )
  check('direct browser mutation without CSRF is refused', noCsrf === 403)
  await evaluate(
    'document.querySelector("#tenants").value="studio-demo";document.querySelector("#tenants").dispatchEvent(new Event("change"))',
  )
  await wait(
    'document.querySelector("#crm h1")?.textContent==="Professional services" && document.querySelector("#status").textContent===""',
  )
  check(
    'business selection switches configuration and isolated records',
    await evaluate(
      'document.querySelector("[data-crm-contacts]").textContent.includes("Studio client") && !document.querySelector("#crm").textContent.includes("Browser permanent guest")',
    ),
  )
  await logout()
  check(
    'sign-out clears the visible workspace',
    await evaluate('document.querySelector("#crm").textContent===""'),
  )
  await page.navigate(origin)
  check(
    'a logged-out session stays logged out after reload',
    await evaluate('!document.querySelector("#login").hidden'),
  )
  await signIn(accounts.staff)
  await wait('document.querySelector("[data-crm-contacts]").textContent.includes("Staff contact")')
  check(
    'staff sees only its assigned contacts and allowed business',
    await evaluate(
      'document.querySelector("#tenants").options.length===1 && !document.querySelector("[data-crm-contacts]").textContent.includes("Owner contact") && !document.querySelector("[data-crm-contacts]").textContent.includes("Browser permanent guest")',
    ),
  )
  const crossTenant = await evaluate(
    "fetch('/api/crm/studio-demo/contacts').then(response=>response.status)",
  )
  check('staff cannot bypass tenant selection by calling the API directly', crossTenant === 403)
  await fill('[data-crm-create] [name=fullName]', 'Browser staff contact')
  await evaluate('document.querySelector("[data-crm-create]").requestSubmit()')
  await wait(
    'document.querySelector("[data-crm-detail]").textContent.includes("Browser staff contact")',
  )
  await logout()
  await signIn(accounts.owner)
  await wait(
    'document.querySelector("[data-crm-contacts]").textContent.includes("Browser staff contact")',
  )
  check('owner and staff operate through the same module and database', true)
  check(
    'session credential stays HttpOnly and business data is absent from localStorage',
    await evaluate('!document.cookie.includes("archava_local_session") && localStorage.length===0'),
  )
  await page.send('Emulation.setDeviceMetricsOverride', {
    width: 360,
    height: 900,
    deviceScaleFactor: 1,
    mobile: true,
  })
  check(
    'authenticated CRM remains usable at mobile width',
    await evaluate('document.documentElement.scrollWidth<=innerWidth'),
  )
  await page.screenshot('/tmp/archava-crm-runtime-browser/shots/authenticated-mobile.png')
  const expected = page.console_.filter(
    (item) => item.kind === 'error' && /status of (401|403)/.test(item.text),
  )
  await writeFile(
    '/tmp/archava-runtime-browser-diagnostics.json',
    JSON.stringify(
      { console: page.console_, failed: page.network_.filter((item) => item.phase === 'failed') },
      null,
      2,
    ),
    { mode: 0o600 },
  )
  check(
    'no unexpected browser errors, CSP violations or failed resources',
    expected.length <= 3 &&
      page.console_.length === expected.length &&
      page.network_.every((item) => item.phase !== 'failed'),
  )
  console.log(`${checks.length}/${checks.length} authenticated CRM browser checks passed.`)
} finally {
  await page.close()
}
