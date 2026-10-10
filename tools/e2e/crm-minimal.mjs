/* global process, console */
import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { URL } from 'node:url'
const { Session } = await import('./cdp.mjs')
const { origin, credentials } = JSON.parse(
  await readFile(process.env.ARCHAVA_CRM_BROWSER_TARGET, 'utf8'),
)
assert.equal(new URL(origin).hostname, '127.0.0.1')
assert.ok(credentials.startsWith('/tmp/archava-crm-runtime-'))
const accounts = JSON.parse(await readFile(credentials, 'utf8'))
const page = await Session.open()
try {
  await page.navigate(origin)
  await page.evaluate(
    `document.querySelector('#login [name=username]').value=${JSON.stringify(accounts.owner.username)};document.querySelector('#login [name=password]').value=${JSON.stringify(accounts.owner.password)};document.querySelector('#login').requestSubmit()`,
  )
  await page.evaluate(
    `new Promise((done,fail)=>{let attempts=0;const tick=()=>{if(!document.querySelector('#workspace').hidden&&document.querySelector('[data-crm-contacts]')&&document.querySelector('#status').textContent==='')done(true);else if(++attempts>100)fail(new Error('Minimal UI startup timeout'));else setTimeout(tick,100)};tick()})`,
  )
  assert.equal(
    await page.evaluate(
      `['pipeline','conversation','overview'].every(id=>document.querySelector('#'+id).hidden&&document.querySelector('#'+id).childElementCount===0)`,
    ),
    true,
  )
  console.log(
    'PASS minimal composition exposes contacts while optional pipeline, assistant and overview DOM stay empty and hidden',
  )
  const statuses = await page.evaluate(
    `(async()=>{const session=await fetch('/api/session').then(r=>r.json());return Promise.all(['/api/crm/hotel-demo/opportunities','/api/crm/hotel-demo/conversation','/api/crm/hotel-demo/actions/followup/review'].map((path,i)=>fetch(path,i?{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':session.csrf},body:'{}'}:{}).then(r=>r.status)))})()`,
  )
  assert.deepEqual(statuses, [404, 404, 404])
  console.log(
    'PASS authenticated browser cannot bypass disabled pipeline, conversation or native mutation routes',
  )
  assert.equal(await page.evaluate(`fetch('/api/crm/hotel-demo/contacts').then(r=>r.status)`), 200)
  console.log('PASS mandatory contacts remain accessible with current credential identity')
} finally {
  await page.close()
}
