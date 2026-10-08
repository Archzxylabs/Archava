import assert from 'node:assert/strict'
import { URL } from 'node:url'
import { readFile, mkdir } from 'node:fs/promises'
await mkdir('/tmp/archava-crm-browser/shots', { recursive: true })
/* global process, console */
process.env.CDP_PORT ??= '9458'
process.env.E2E_PROFILE ??= '/tmp/archava-crm-browser/profile'
const { launchDebugBrowser, Session } = await import('./cdp.mjs')
const { origin } = process.env.CRM_BASE
  ? { origin: process.env.CRM_BASE }
  : JSON.parse(await readFile('/tmp/archava-crm-browser-target.json', 'utf8'))
if (new URL(origin).hostname !== '127.0.0.1')
  throw new Error('CRM browser checks require the local disposable reference server')
await launchDebugBrowser()
const page = await Session.open()
await page.send('Log.enable')
await page.send('Log.clear')
page.console_ = []
await page.navigate(origin + '/hospitality/')
const evaluate = (expr) => page.evaluate(expr)
const pause = () => evaluate('new Promise(r=>setTimeout(r,300))')
const fill = async (selector, value) =>
  evaluate(`document.querySelector(${JSON.stringify(selector)}).value=${JSON.stringify(value)}`)
const submit = async (selector) => {
  await evaluate(`document.querySelector(${JSON.stringify(selector)}).requestSubmit()`)
  await pause()
}
const check = (label, condition) => {
  assert.ok(condition, label)
  console.log('PASS ' + label)
}
try {
  await evaluate('document.querySelector("[data-crm-signin=hotel-owner]").click()')
  await pause()
  check(
    'hotel owner dashboard loads with authorized controls',
    await evaluate(
      '!!document.querySelector("[data-crm-contacts]") && !document.querySelector("[data-crm-create]").hidden',
    ),
  )
  const hostile = '<img src=x onerror=alert(1)> Browser guest'
  await fill('[data-crm-create] [name=fullName]', hostile)
  await fill('[data-crm-create] [name=email]', 'browser@example.test')
  await submit('[data-crm-create]')
  check(
    'create contact through API and safely render hostile name',
    await evaluate(
      'document.querySelector("[data-crm-detail]").textContent.includes(' +
        JSON.stringify(hostile) +
        ') && !document.querySelector("#crm img")',
    ),
  )
  await fill('[data-crm-edit] [name=fullName]', 'Browser guest updated')
  await submit('[data-crm-edit]')
  check(
    'edit preserves email and refreshes persisted detail',
    await evaluate(
      'document.querySelector("[data-crm-detail]").textContent.includes("browser@example.test") && document.querySelector("[data-crm-detail]").textContent.includes("Browser guest updated")',
    ),
  )
  await fill('[data-crm-activity] [name=summary]', 'Browser inquiry note')
  await submit('[data-crm-activity]')
  check(
    'activity note is saved through shared module',
    await evaluate(
      'document.querySelector("[data-crm-detail]").textContent.includes("Browser inquiry note")',
    ),
  )
  await fill('[data-crm-followup] [name=summary]', 'Browser follow-up')
  await fill('[data-crm-followup] [name=dueAt]', '2026-10-10T10:00')
  await submit('[data-crm-followup]')
  check(
    'follow-up is persisted',
    await evaluate(
      '!!document.querySelector("[data-crm-complete]") && document.querySelector("[data-crm-detail]").textContent.includes("Browser follow-up")',
    ),
  )
  await evaluate('document.querySelector("[data-crm-complete]").click()')
  await pause()
  check(
    'follow-up completion persists',
    await evaluate('document.querySelector("[data-crm-detail]").textContent.includes("completed")'),
  )
  await page.navigate(origin + '/hospitality/')
  await evaluate('document.querySelector("[data-crm-signin=hotel-owner]").click()')
  await pause()
  check(
    'reload restores database records without localStorage',
    await evaluate(
      'document.querySelector("[data-crm-contacts]").textContent.includes("Browser guest updated") && localStorage.length===0',
    ),
  )
  await evaluate('document.querySelector("[data-crm-signin=hotel-viewer]").click()')
  await pause()
  check(
    'viewer has no write controls',
    await evaluate('document.querySelector("[data-crm-create]").hidden'),
  )
  const denied = await evaluate(
    `fetch('/api/crm/hotel-demo/commands',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({requestId:crypto.randomUUID(),command:{operation:'contact.create',data:{fullName:'Forbidden'}}})}).then(r=>r.status)`,
  )
  check('viewer cannot bypass UI by issuing a direct command', denied === 403)
  await page.screenshot('/tmp/archava-crm-browser/shots/hotel.png')
  await page.navigate(origin + '/professional-services/')
  await evaluate('document.querySelector("[data-crm-signin=studio-owner]").click()')
  await pause()
  check(
    'professional-service context has its own persisted contacts and labels',
    await evaluate(
      '!document.querySelector("#crm").textContent.includes("Browser guest updated") && document.querySelector("h1").textContent.includes("Professional services")',
    ),
  )
  await fill('[data-crm-create] [name=fullName]', 'Browser consulting client')
  await submit('[data-crm-create]')
  check(
    'same dashboard component writes professional-service contacts',
    await evaluate(
      'document.querySelector("[data-crm-detail]").textContent.includes("Browser consulting client")',
    ),
  )
  await page.send('Emulation.setDeviceMetricsOverride', {
    width: 360,
    height: 900,
    deviceScaleFactor: 1,
    mobile: true,
  })
  check(
    'mobile dashboard has no horizontal overflow',
    await evaluate('document.documentElement.scrollWidth<=innerWidth'),
  )
  const expectedDenials = page.console_.filter(
    (item) => item.kind === 'error' && item.text.includes('status of 403'),
  )
  check(
    'no unexpected runtime exceptions or failed resources',
    expectedDenials.length <= 1 &&
      page.console_.length === expectedDenials.length &&
      page.network_.every((item) => item.phase !== 'failed'),
  )
  await page.screenshot('/tmp/archava-crm-browser/shots/professional-services.png')
} finally {
  await page.close()
}
