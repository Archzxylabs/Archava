/** Real-browser acceptance against a task-owned fixture, followed by the production binary. */
import assert from 'node:assert/strict'
import { startPaymentFixture, verifyPayments } from './payments.mjs'
import { startEmailFixture, verifyEmail } from './email.mjs'
import { verifyPresence, verifyPresenceIsolation } from './presence.mjs'
import { verifyOperations } from './operations.mjs'
import { verifyPendingRecovery } from './recovery.mjs'
import { readWriterLease, verifyStartupRefusals, verifyWriterLifecycle } from './writer.mjs'
import { spawn, spawnSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { createWriteStream, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { createServer } from 'node:net'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const args = process.argv.slice(2)
if (args.includes('--help')) {
  process.stdout.write('node tools/e2e/verify.mjs --output DIRECTORY\nUses freshly built release artifacts. Run tools/rust/build-slice.mjs first.\nRequires installed Chromium (E2E_BROWSER overrides discovery) and PostgreSQL tools.\nOnly synthetic fixture credentials/data are used; all owned processes are closed.\n')
  process.exit(0)
}
assert.equal(args.length, 2, 'Expected --output DIRECTORY')
assert.equal(args[0], '--output')
const output = resolve(root, args[1]); mkdirSync(output, { recursive: true })
const preview = join(root, 'target/release/examples/slice_server')
const production = join(root, 'target/release/archava-server')
for (const p of [preview, production, join(root, 'target/site/pkg/archava-slice_bg.wasm')]) assert.ok(existsSync(p), `Build required: ${p}`)
const scratch = mkdtempSync(join(tmpdir(), 'archava-browser-proof-'))
const pause = (ms) => new Promise((r) => setTimeout(r, ms))
async function until(predicate, label, timeout = 30000) {
  const deadline = Date.now() + timeout; let last
  while (Date.now() < deadline) {
    try { const result = await predicate(); if (result) return result } catch (error) { last = error.message }
    await pause(100)
  }
  throw new Error(`Timed out: ${label}${last ? ` (${last})` : ''}`)
}
function browserPath() {
  if (process.env.E2E_BROWSER) { assert.ok(existsSync(process.env.E2E_BROWSER)); return process.env.E2E_BROWSER }
  for (const path of ['/usr/bin/chromium', '/usr/bin/chromium-browser', '/usr/bin/google-chrome', '/usr/bin/brave']) if (existsSync(path)) return path
  const cache = join(homedir(), '.cache/ms-playwright')
  if (existsSync(cache)) {
    const dirs = readdirSync(cache).filter((p) => /^chromium-\d+$/.test(p)).sort((a,b) => Number(b.split('-')[1]) - Number(a.split('-')[1]))
    for (const dir of dirs) for (const suffix of ['chrome-linux64/chrome', 'chrome-linux/chrome']) {
      const path = join(cache, dir, suffix); if (existsSync(path)) return path
    }
  }
  throw new Error('No installed Chromium. Set E2E_BROWSER; this tool never downloads one.')
}
async function freePort() {
  const server = createServer(); await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const port = server.address().port; await new Promise((r) => server.close(r)); return port
}
async function terminate(child) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return
  child.kill('SIGTERM')
  const ended = await Promise.race([new Promise((r) => child.once('exit', () => r(true))), pause(10000).then(() => false)])
  if (!ended) { child.kill('SIGKILL'); await new Promise((r) => child.once('exit', r)) }
}
class Page {
  pending = new Map(); next = 0; errors = []
  constructor(ws) {
    this.ws = ws
    ws.addEventListener('message', ({ data }) => {
      const m = JSON.parse(String(data))
      if (m.id) { const p = this.pending.get(m.id); if (p) { clearTimeout(p.timer); this.pending.delete(m.id); m.error ? p.reject(new Error(m.error.message)) : p.resolve(m.result) } }
      else if (m.method === 'Runtime.exceptionThrown') this.errors.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text)
      else if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error') {
        const text = m.params.args.map((x) => x.value ?? x.description ?? '').join(' ')
        if (text.includes('slice panic:')) this.errors.push(text)
      }
    })
    ws.addEventListener('close', () => { for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(new Error('CDP closed')) }; this.pending.clear() })
  }
  static async open(port, targetId) {
    const targets = await (await fetch(`http://127.0.0.1:${port}/json`)).json()
    const target = targets.find((t) => t.type === 'page' && (!targetId || t.id === targetId))
    assert.ok(target?.webSocketDebuggerUrl)
    const ws = new WebSocket(target.webSocketDebuggerUrl)
    await new Promise((r,j) => { ws.addEventListener('open', r, { once:true }); ws.addEventListener('error', j, { once:true }) })
    const page = new Page(ws); await page.send('Runtime.enable'); await page.send('Page.enable'); await page.send('Network.enable'); return page
  }
  send(method, params = {}) {
    const id = ++this.next
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`CDP timeout: ${method}`)) }, 15000)
      this.pending.set(id, { resolve, reject, timer }); this.ws.send(JSON.stringify({ id, method, params }))
    })
  }
  async evaluate(expression) {
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise:true, returnByValue:true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  async navigate(url) {
    const prior = await this.evaluate('Number(window.archavaSliceHydratedAt || 0)').catch(() => 0)
    await this.send('Page.navigate', { url })
    await until(() => this.evaluate(`Number(window.archavaSliceHydratedAt || 0) > ${prior}`), 'SSR document hydration', 45000)
  }
  async fill(selector, value) {
    await this.evaluate(`(() => { const n=document.querySelector(${JSON.stringify(selector)}); if(!n)throw Error('Input missing'); n.value=${JSON.stringify(value)}; n.dispatchEvent(new Event('input',{bubbles:true})); })()`)
  }
  async click(selector) {
    await this.evaluate(`(() => { const n=document.querySelector(${JSON.stringify(selector)}); if(!n || n.disabled)throw Error('Control unavailable'); n.click(); })()`)
  }
  async text(selector) { return this.evaluate(`document.querySelector(${JSON.stringify(selector)})?.textContent ?? ''`) }
  async screenshot(name) { const r=await this.send('Page.captureScreenshot', { format:'png', captureBeyondViewport:false }); writeFileSync(join(output,name),Buffer.from(r.data,'base64')) }
}
const evidence = { startedAt:new Date().toISOString(), scope:'Synthetic local fixture, real Chromium/SSR/WASM/HTTP/PostgreSQL; production binary restart; owned loopback email/presence transports. No live provider or public/TLS deployment acceptance.', checks:[], errors:[] }
let server, browser, page, facts, serverText = ''
const logs = []
const ownedServers = []
function startServer(binary, env, filename) {
  serverText = ''; const log = createWriteStream(join(output, filename)); logs.push(log)
  const child = spawn(binary, [], { cwd:root, env:{ ...process.env, ...env }, stdio:['ignore','pipe','pipe'] })
  ownedServers.push(child)
  child.stdout.on('data', (chunk) => { serverText += chunk; log.write(chunk) }); child.stderr.on('data', (chunk) => log.write(chunk))
  child.on('error', (e) => { serverText += `\nSPAWN ERROR: ${e.message}` }); return child
}
function pass(id, details) { if(page?.errors.length)throw Error(`Unexpected browser runtime errors before ${id}: ${page.errors[0]}`);evidence.checks.push({ id, status:'passed', details }); process.stdout.write(`PASS ${id}\n`) }
const testId = (name) => `[data-testid="${name}"]`
function databaseCounts(port, contact, requestId) {
  for (const id of [contact, requestId]) assert.match(id, /^[0-9a-f-]{36}$/i)
  const sql = `SET archava.tenant_id='acme-retail'; SELECT json_build_object('same_email_rows',(SELECT count(*) FROM archava_crm.contacts WHERE email=(SELECT email FROM archava_crm.contacts WHERE id='${contact}'::uuid)),'contact_rows',(SELECT count(*) FROM archava_crm.contacts WHERE id='${contact}'::uuid),'followups',(SELECT count(*) FROM archava_crm.followups WHERE contact_id='${contact}'::uuid),'receipt_rows',(SELECT count(*) FROM archava_crm.commands WHERE request_id='${requestId}'::uuid AND receipt IS NOT NULL));`
  const r=spawnSync('psql',['-X','-q','-A','-t','-v','ON_ERROR_STOP=1'], { input:sql, encoding:'utf8', env:{...process.env,PGHOST:'127.0.0.1',PGPORT:String(port),PGDATABASE:'archava_test',PGUSER:'archava_runtime',PGPASSWORD:'archava'} })
  assert.equal(r.status,0,r.stderr); return JSON.parse(r.stdout.trim())
}
let paymentFixture, emailFixture
try {
  paymentFixture=await startPaymentFixture()
  emailFixture=await startEmailFixture()
  const port = await freePort(); const origin=`http://127.0.0.1:${port}`
  const pidfile=join(scratch,'preview.pid')
  server=startServer(preview,{SLICE_PORT:String(port),ARCHAVA_SLICE_PIDFILE:pidfile,ARCHAVA_SLICE_PAYMENT_ORIGIN:paymentFixture.origin,ARCHAVA_SLICE_EMAIL_ORIGIN:emailFixture.origin,ARCHAVA_SLICE_PRESENCE_LOOPBACK:'1'},'preview.log')
  await until(() => { const m=/restart facts: ARCHAVA_SLICE_ATTACH=(\{[^\n]+\})/.exec(serverText); if (m) return facts=JSON.parse(m[1]); if(server.exitCode!==null) throw Error('Preview exited'); return false },'fixture startup',60000)
  await until(async()=> (await fetch(`${origin}/readyz`)).ok,'preview readiness')
  const executable=browserPath(); evidence.browserExecutable=executable; evidence.browserSandboxDisabled=process.env.E2E_BROWSER_NO_SANDBOX==='1'
  const profile=join(scratch,'browser'); mkdirSync(profile)
  const launchArgs=['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-address=127.0.0.1','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank']
  if(process.env.E2E_BROWSER_NO_SANDBOX==='1') launchArgs.unshift('--no-sandbox')
  const browserLog=createWriteStream(join(output,'chromium.log')); logs.push(browserLog)
  browser=spawn(executable,launchArgs,{stdio:['ignore','ignore','pipe']}); browser.stderr.pipe(browserLog)
  const cdpPort=await until(()=> { const f=join(profile,'DevToolsActivePort');return existsSync(f)&&Number(readFileSync(f,'utf8').split('\n')[0]) },'Chromium DevTools endpoint')
  page=await Page.open(cdpPort)
  await page.send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false})
  async function login(user) {
    await page.navigate(`${origin}/app`)
    await page.fill(testId('login-username'),`${user}@archava.test`);await page.fill(testId('login-password'),'correct horse battery staple');await page.fill(testId('login-tenant'),'acme-retail');await page.click(testId('login-submit'))
    await until(()=>page.evaluate("location.pathname==='/app/contacts' && !!document.querySelector('[data-testid=contacts-count]')"),'authenticated contact list')
  }
  async function selectDraft(name) {
    await page.navigate(`${origin}/app/contacts`);await page.fill(testId('contacts-search'),name);await page.click(testId('contacts-refresh'))
    await until(()=>page.evaluate("document.querySelectorAll('[data-testid=contact-row]').length===1"),'unique contact search')
    await page.click(testId('contact-draft'));await until(()=>page.evaluate("location.pathname==='/app/draft' && !!document.querySelector('[data-testid=draft-submit]')"),'review draft')
  }
  async function observeMutation() {
    await page.evaluate(`(() => { if(window.__auditFetch)return;window.__auditFetch=window.fetch.bind(window);window.__auditBodies=[];window.fetch=async function(input,init){const req=input instanceof Request?new Request(input.clone(),init):new Request(input,init);const mutation=new URL(req.url).pathname==='/crm/commands'&&req.method==='POST';if(mutation)window.__auditBodies.push(await req.clone().json());const response=await window.__auditFetch(input,init);if(mutation&&response.ok&&window.__auditFaultMode){const mode=window.__auditFaultMode;window.__auditFaultMode=null;if(mode==='http503')return new Response(JSON.stringify({decision:'unavailable',message:'Synthetic lost acknowledgement'}),{status:503,headers:{'content-type':'application/json'}});const body=await response.clone().json();body.requestId='a-different-request';return new Response(JSON.stringify(body),{status:200,headers:{'content-type':'application/json'}});}if(mutation&&response.ok&&window.__auditDropNext){window.__auditDropNext=false;throw new TypeError('Synthetic lost response after server acknowledgement');}return response;};})()`)
  }
  await login('alice');pass('F01-authenticated-hydration','Real login, session cookie, WASM handlers and contact query')
  const suffix=randomUUID().slice(0,8), name=`Launch Audit ${suffix}`
  await page.fill(testId('contact-new-name'),name);await page.fill(testId('contact-new-email'),`audit-${suffix}@archava.test`);await page.fill(testId('contact-new-org'),'Synthetic launch fixture');await page.click(testId('contact-create'))
  await until(()=>page.evaluate("!!document.querySelector('[data-testid=contact-create-open]')"),'durable contact create')
  await page.click(testId('contact-create-open'));await until(()=>page.text(testId('record-name')).then((t)=>t===name),'created record view')
  const contact=await page.evaluate("new URL(location.href).searchParams.get('id')");assert.match(contact,/^[0-9a-f-]{36}$/i)
  const createdCounts=databaseCounts(facts.port,contact,contact)
  assert.equal(createdCounts.contact_rows,1);assert.equal(createdCounts.same_email_rows,1);assert.equal(createdCounts.followups,0)
  pass('F02-contact-navigation-idempotency','Opening the new record creates no second contact or other mutation')
  await page.screenshot('contact-created.png')
  await selectDraft(name);await observeMutation();await page.fill(testId('draft-summary'),`Normal follow-up ${suffix}`);await page.fill(testId('draft-due'),new Date(Date.now()+86400000).toISOString())
  const firstId=(await page.text(testId('review-request-id'))).trim()
  await page.evaluate("document.querySelector('[data-testid=draft-submit]').click();document.querySelector('[data-testid=draft-submit]').click()")
  await until(()=>page.text(testId('draft-status')).then((t)=>/Recorded|Replayed/.test(t)),'durable follow-up receipt')
  assert.deepEqual(databaseCounts(facts.port,contact,firstId),{same_email_rows:1,contact_rows:1,followups:1,receipt_rows:1})
  pass('F02-browser-to-database',{contact,requestId:firstId,proof:'Created via UI, reviewed/applied twice, exactly one follow-up and durable receipt via non-owner SQL'})
  await page.screenshot('followup-receipt.png')
  await selectDraft(name);await observeMutation();await page.fill(testId('draft-summary'),`Lost response ${suffix}`);await page.fill(testId('draft-due'),new Date(Date.now()+172800000).toISOString())
  const lostId=(await page.text(testId('review-request-id'))).trim();assert.notEqual(lostId,firstId)
  await page.evaluate('window.__auditDropNext=true');await page.click(testId('draft-submit'))
  await until(()=>page.evaluate("!!document.querySelector('[data-testid=unreachable-recover]')"),'uncertain outcome after lost response')
  const validBody=await page.evaluate('window.__auditBodies.at(-1)')
  await page.click(testId('unreachable-recover'))
  await until(()=>page.text(testId('recover-status')).then((t)=>t.includes('Committed')),'read-only recovery receipt')
  assert.deepEqual(databaseCounts(facts.port,contact,lostId),{same_email_rows:1,contact_rows:1,followups:2,receipt_rows:1})
  pass('F03-lost-response-recovery',{requestId:lostId,proof:'Response deliberately lost after real commit; UI shows uncertainty and recovers without resubmitting'})
  const csrfStatus=await page.evaluate(`fetch('/crm/commands',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(${JSON.stringify(validBody)})}).then(r=>r.status)`)
  assert.equal(csrfStatus,403);pass('F04-csrf-refusal','Valid mutation body without the CSRF header is refused')
  evidence.payments=await verifyPayments({page,origin,until,pass,suffix,provider:paymentFixture})
  evidence.email=await verifyEmail({page,origin,until,pass,suffix,provider:emailFixture})
  const presence=await verifyPresence({page,pass,suffix})
  evidence.presence=presence
  await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await login('bob')
  await verifyPresenceIsolation({page,pass,sessionId:presence.sessionId})
  await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await login('alice')
  // Restart the actual production executable on the same migrated fixture and session.
  server.kill('SIGKILL');await new Promise((r)=>server.once('exit',r))
  const productionEnvironment={DATABASE_URL:`postgres://archava_runtime:archava@127.0.0.1:${facts.port}/archava_test`,ARCHAVA_IDENTITY_KEY_HEX:'07'.repeat(32),ARCHAVA_BIND_ADDR:`127.0.0.1:${port}`,ARCHAVA_SITE_ROOT:join(root,'target/site')}
  server=startServer(production,{...productionEnvironment,ARCHAVA_WRITER_TAKEOVER:'true'},'production-restart.log')
  await until(async()=> (await fetch(`${origin}/readyz`)).ok,'production binary readiness after restart',60000)
  const readLease=()=>readWriterLease(facts.port)
  await verifyStartupRefusals({production,env:productionEnvironment,root,output,origin,freePort,readLease,pass})
  await page.navigate(`${origin}/app/recover?id=${lostId}`);await until(()=>page.text(testId('recover-status')).then((t)=>t.includes('Committed')),'receipt survives process restart')
  assert.deepEqual(databaseCounts(facts.port,contact,lostId),{same_email_rows:1,contact_rows:1,followups:2,receipt_rows:1})
  pass('F03-production-restart','Fresh production process, same PostgreSQL and browser session, committed receipt survives')
  await page.screenshot('recovery-after-restart.png')

  let expectedFollowups=2
  for(const mode of ['http503','mismatched200']){
    await selectDraft(name);await observeMutation();await page.fill(testId('draft-summary'),`Acknowledgement fault ${mode} ${suffix}`);await page.fill(testId('draft-due'),new Date(Date.now()+259200000).toISOString())
    const key=(await page.text(testId('review-request-id'))).trim();await page.evaluate(`window.__auditFaultMode=${JSON.stringify(mode)}`);await page.click(testId('draft-submit'))
    await until(()=>databaseCounts(facts.port,contact,key).followups===expectedFollowups+1,`CRM ${mode} actual commit`)
    evidence.acknowledgementFaults??=[];evidence.acknowledgementFaults.push({mode,requestId:key,sql:databaseCounts(facts.port,contact,key)})
    await until(()=>page.evaluate("!!document.querySelector('[data-testid=unreachable-recover]')"),`CRM ${mode} retains uncertain outcome`)
    await page.click(testId('unreachable-recover'));await until(()=>page.text(testId('recover-status')).then(t=>t.includes('Committed')),`CRM ${mode} recovers original receipt`)
    expectedFollowups++;assert.deepEqual(databaseCounts(facts.port,contact,key),{same_email_rows:1,contact_rows:1,followups:expectedFollowups,receipt_rows:1})
    pass(`CRM-${mode}-receipt-recovery`,'A real committed write receives an unusable acknowledgement; UI retains the key and recovers exactly one effect')
  }
  await verifyPendingRecovery({page,origin,until,pass,suffix,login,openTab:async()=>{
    const {targetId}=await page.send('Target.createTarget',{url:'about:blank'})
    const tab=await Page.open(cdpPort,targetId)
    return {page:tab,close:async()=>{tab.ws.close();await page.send('Target.closeTarget',{targetId})}}
  }})
  evidence.operations=await verifyOperations({page,origin,until,pass,suffix})
  for(const id of Object.values(evidence.operations))assert.match(id,/^[0-9a-f-]{36}$/i)
  const op=evidence.operations
  const erpSql=`SET archava.tenant_id='acme-retail';SELECT json_build_object('inventoryValue',(SELECT sum(archava_commerce.cost_portion(total_cost_minor,original_quantity,original_quantity-remaining_quantity,remaining_quantity))::text FROM archava_commerce.inventory_layers WHERE product_id='${op.product}'::uuid AND remaining_quantity>0),'fulfillmentCost',(SELECT sum(total_cost_minor)::text FROM archava_commerce.inventory_uses WHERE order_id='${op.order}'::uuid),'stock',(SELECT quantity FROM archava_commerce.stock_balances WHERE product_id='${op.product}'::uuid),'sales',(SELECT state FROM archava_commerce.orders WHERE id='${op.order}'::uuid),'purchase',(SELECT state FROM archava_purchasing.orders WHERE id='${op.purchase}'::uuid),'invoice',(SELECT status FROM archava_billing.invoices WHERE id='${op.invoice}'::uuid),'settled',(SELECT settled_minor FROM archava_billing.invoices WHERE id='${op.invoice}'::uuid),'support',(SELECT state FROM archava_support.cases WHERE id='${op.support}'::uuid));`
  const erpProof=spawnSync('psql',['-X','-At','-h','127.0.0.1','-p',String(facts.port),'-U','archava_runtime','-d','archava_test','-c',erpSql],{env:{...process.env,PGPASSWORD:'archava'},encoding:'utf8'})
  assert.equal(erpProof.status,0,erpProof.stderr)
  const erpRows=JSON.parse(erpProof.stdout.trim().split('\n').at(-1));assert.deepEqual(erpRows,{inventoryValue:'100000',fulfillmentCost:'20000',stock:10,sales:'fulfilled',purchase:'received',invoice:'issued',settled:0,support:'resolved'})
  pass('ERP-independent-SQL',erpRows)

  await page.navigate(`${origin}/app/owner`);await until(()=>page.evaluate("!!document.querySelector('[data-testid=owner-preview]') && !document.querySelector('[data-testid=owner-preview]').disabled"),'owner configuration read')
  await page.click(testId('owner-preview'));await until(()=>page.evaluate("!!document.querySelector('[data-testid=owner-confirm]') && !document.querySelector('[data-testid=owner-confirm]').disabled"),'owner reviewed plan')
  await page.click(testId('owner-confirm'));await until(()=>page.text(testId('owner-status')).then(t=>t.includes('committed revision')),'matching owner receipt')
  const ownerConfig=await page.evaluate("fetch('/own/config?tenant=acme-retail').then(r=>r.json())");assert.ok(ownerConfig.config.revision>=1)
  await page.screenshot('owner-configuration-receipt.png');pass('F05-owner-confirmation','Owner reviews and confirms a configuration revision; a matching receipt and persisted revision are present')
  await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await login('bob')
  await selectDraft(name);await page.fill(testId('draft-summary'),'Must be refused');await page.click(testId('draft-submit'))
  await until(()=>page.text(testId('draft-status')).then((t)=>t.includes('Refused')),'read-only actor write refusal')
  assert.deepEqual(databaseCounts(facts.port,contact,lostId),{same_email_rows:1,contact_rows:1,followups:expectedFollowups,receipt_rows:1})
  const crossTenant=await page.evaluate("fetch('/contacts?tenant=globex-corp').then(r=>r.status)");assert.ok([403,404].includes(crossTenant),`Cross-tenant HTTP ${crossTenant}`)
  pass('F04-role-and-tenant-refusal','Read-only UI submission creates no effect; foreign tenant read refused')
  await page.navigate(`${origin}/app/contacts`);await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true})
  await until(()=>page.evaluate('document.documentElement.scrollWidth<=innerWidth'),'mobile overflow check')
  await page.screenshot('contacts-mobile.png');pass('UI-mobile-layout','390px viewport has no horizontal page overflow')
  assert.deepEqual(page.errors,[],'Unexpected JavaScript/WASM errors')
  await verifyWriterLifecycle({production,env:productionEnvironment,origin,original:server,startServer,terminate,until,freePort,readLease,pass})
  evidence.status='passed'
} catch(error) {
  evidence.status='failed';evidence.errors.push(error.stack ?? error.message);process.stderr.write(`${error.stack ?? error}\n`);process.exitCode=1
  if(page) await page.screenshot('failure.png').catch(()=>{})
} finally {
  evidence.pageErrors=page?.errors ?? []
  if(page) { await page.send('Browser.close').catch(()=>{});page.ws.close() }
  await terminate(browser);for(const child of ownedServers.reverse())await terminate(child)
  if(facts) {
    const cleanup=spawnSync(preview,['cleanup'],{cwd:root,env:{...process.env,ARCHAVA_SLICE_ATTACH:JSON.stringify(facts)},encoding:'utf8',timeout:30000})
    evidence.fixtureCleanup={exitCode:cleanup.status,signal:cleanup.signal};writeFileSync(join(output,'cleanup.log'),`${cleanup.stdout ?? ''}${cleanup.stderr ?? ''}`)
    if(cleanup.status!==0){evidence.status='failed';process.exitCode=1}
  }
  if(paymentFixture)await paymentFixture.close()
  if(emailFixture)await emailFixture.close()
  for(const log of logs) await new Promise((r)=>log.end(r))
  rmSync(scratch,{recursive:true,force:true})
  evidence.finishedAt=new Date().toISOString()
  evidence.artifacts=[preview,production,join(root,'target/site/pkg/archava-slice_bg.wasm')].map((path)=>({path:path.slice(root.length+1),bytes:readFileSync(path).length,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')}))
  writeFileSync(join(output,'result.json'),`${JSON.stringify(evidence,null,2)}\n`)
}
