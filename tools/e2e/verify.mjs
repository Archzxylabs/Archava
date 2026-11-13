/** Real-browser acceptance against a task-owned fixture, followed by the production binary. */
import assert from 'node:assert/strict'
import { startPaymentFixture, verifyPayments } from './payments.mjs'
import { startEmailFixture, verifyEmail } from './email.mjs'
import { verifyPresence, verifyPresenceIsolation } from './presence.mjs'
import { verifyOperations } from './operations.mjs'
import { verifyOrderInvoice } from './order-invoice.mjs'
import { verifyCustomerIdentity } from './customer-identity.mjs'
import { verifySupplierIdentity } from './supplier-identity.mjs'
import { verifyOrganizationIdentity } from './organization-identity.mjs'
import { verifyParties } from './parties.mjs'
import { verifyCompanies } from './companies.mjs'
import { verifyCompanyTrade } from './company-trade.mjs'
import {verifySupplierDocuments} from './supplier-documents.mjs'
import {verifyAccounting} from './accounting.mjs'
import {verifyInvoicePosting} from './invoice-posting.mjs'
import {verifyPurchasePosting} from './purchase-posting.mjs'
import {verifyStockPosting} from './stock-posting.mjs'
import {verifyStoreFinanceCycle} from './store-finance-cycle.mjs'
import {verifyStoreBankCycle} from './store-bank-cycle.mjs'
import {verifyDueTerms} from './due-terms.mjs'
import {verifyGatewayClearing} from './gateway-clearing.mjs'
import {verifyBankReconciliation} from './bank-reconciliation.mjs'
import {verifySupplierPayments} from './supplier-payments.mjs'
import {verifyLegacyBankReceipts} from './legacy-bank-receipts.mjs'
import {verifyPriorNative} from './prior-native.mjs'
import {verifyOwnerConfiguration} from './owner-configuration.mjs'
import {startPlannerFixture,verifyAiDisclosure} from './ai-disclosure.mjs'
import {verifyAgentFinance} from './agent-finance.mjs'
import { verifyPartyTransactions } from './party-transactions.mjs'
import { verifyPendingRecovery } from './recovery.mjs'
import { verifyLoad } from './load.mjs'
import { verifyTls } from './tls.mjs'
import { verifyOps } from './ops.mjs'
import { verifyProduct } from './product.mjs'
import { verifyAgentLive } from './agent-live.mjs'
import { verifyAgentRuntime, prepareAgentRestart, verifyAgentNavigation } from './agent-runtime.mjs'
import { verifyAgentic } from './agentic.mjs'
import { verifySimulations } from './simulations.mjs'
import { startMediaFixture, verifyMedia } from './media.mjs'
import { readWriterLease, verifyStartupRefusals, verifyWriterLifecycle } from './writer.mjs'
import { spawn, spawnSync } from 'node:child_process'
import { createHash, randomUUID } from 'node:crypto'
import { createWriteStream, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, resolve } from 'node:path'
import { createServer } from 'node:net'
import { fileURLToPath } from 'node:url'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const liveAgent = process.env.ARCHAVA_E2E_AGENT_LIVE === '1'
const bankOnly = process.env.ARCHAVA_E2E_BANK_ONLY === '1'
const payablesOnly = process.env.ARCHAVA_E2E_PAYABLES_ONLY === '1'
const clearingOnly = process.env.ARCHAVA_E2E_CLEARING_ONLY === '1'
const termsOnly = process.env.ARCHAVA_E2E_TERMS_ONLY === '1'
assert.ok([bankOnly,payablesOnly,clearingOnly,termsOnly].filter(Boolean).length<=1,'Choose one focused financial journey')
if(bankOnly||payablesOnly||clearingOnly||termsOnly){
  assert.equal(liveAgent,false,'Run live planning separately from the focused bank proof')
  for(const flag of ['ARCHAVA_E2E_PRODUCT','ARCHAVA_E2E_AGENT_RUNTIME','ARCHAVA_E2E_AGENTIC','ARCHAVA_E2E_SIMULATIONS','ARCHAVA_E2E_MEDIA','ARCHAVA_E2E_TLS','ARCHAVA_E2E_LOAD'])assert.notEqual(process.env[flag],'1','Focused bank proof must not imply optional full-gate acceptance')
  for(const key of ['ARCHAVA_E2E_OPS_ROOT','ARCHAVA_E2E_PREVIOUS_NATIVE','ARCHAVA_E2E_DISCLOSURE_PREVIOUS_NATIVE','ARCHAVA_E2E_BANK_PREVIOUS_NATIVE'])assert.ok(!process.env[key],'Run native Ops/upgrade acceptance in the complete browser gate')
}
if (liveAgent) {
  assert.ok(process.env.GEMINI_AGENT_API_KEY && process.env.GEMINI_AGENT_MODEL, 'Live acceptance requires an explicit provider key and GenerateContent model')
  assert.ok(process.env.GEMINI_AGENT_ENDPOINT === 'https://generativelanguage.googleapis.com', 'Live acceptance cannot use a fixture endpoint')
  for (const flag of ['ARCHAVA_E2E_AGENT_RUNTIME','ARCHAVA_E2E_AGENTIC','ARCHAVA_E2E_SIMULATIONS']) assert.notEqual(process.env[flag], '1', 'Run live planning separately from deterministic planner probes')
}
const args = process.argv.slice(2)
if (args.includes('--help')) {
  process.stdout.write('node tools/e2e/verify.mjs --output DIRECTORY\nUses freshly built release artifacts. Run tools/rust/build-slice.mjs first.\nRequires installed Chromium (E2E_BROWSER overrides discovery) and PostgreSQL tools.\nSynthetic business data are used; all owned processes are closed.\nOptional: ARCHAVA_E2E_TLS=1 (Caddy/curl), ARCHAVA_E2E_LOAD=1, ARCHAVA_E2E_OPS_ROOT=/absolute/native/package.\nARCHAVA_E2E_AGENT_RUNTIME=1 tests the current goal/review/worker/receipt journey across 18 domains.\nFor explicit live Google inference use tools/e2e/run-agent-live.mjs --help.\nARCHAVA_E2E_PRODUCT=1 audits product requirements; inspect product.allObservedRequirementsMet separately from runner status.\n')
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
      const timer = setTimeout(() => { this.pending.delete(id); reject(new Error(`CDP timeout: ${method}`)) }, liveAgent && method === 'Runtime.evaluate' ? 45000 : 15000)
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
const evidence = { startedAt:new Date().toISOString(), scope:'Synthetic local fixture, real Chromium/SSR/WASM/HTTP/PostgreSQL; production binary restart; owned loopback email/presence transports. Optional local TLS/load/native recovery are recorded separately. No live provider or public deployment acceptance.', checks:[], errors:[] }
if(liveAgent)evidence.scope='Real Google Gemini GenerateContent and local native/Chromium/PostgreSQL acceptance with synthetic business data; no public deployment or real customer/counterparty effects.'
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
let paymentFixture, emailFixture, mediaFixture, plannerFixture
try {
  if(!bankOnly){
    paymentFixture=await startPaymentFixture()
    if(!liveAgent)plannerFixture=await startPlannerFixture()
    emailFixture=await startEmailFixture()
  }
  if(process.env.ARCHAVA_E2E_MEDIA==='1')mediaFixture=await startMediaFixture({scratch,output,freePort,until,terminate})
  const port = await freePort(); const origin=`http://127.0.0.1:${port}`
  const pidfile=join(scratch,'preview.pid')
  server=startServer(preview,{ARCHAVA_SLICE_ATTACH:undefined,SLICE_PORT:String(port),ARCHAVA_SLICE_PIDFILE:pidfile,ARCHAVA_SLICE_PAYMENT_ORIGIN:paymentFixture?.origin,ARCHAVA_SLICE_EMAIL_ORIGIN:emailFixture?.origin,ARCHAVA_SLICE_PRESENCE_LOOPBACK:'1',...(plannerFixture?.environment??{})},'preview.log')
  await until(() => { const m=/restart facts: ARCHAVA_SLICE_ATTACH=(\{[^\n]+\})/.exec(serverText); if (m) return facts=JSON.parse(m[1]); if(server.exitCode!==null) throw Error('Preview exited'); return false },'fixture startup',60000)
  await until(async()=> (await fetch(`${origin}/readyz`)).ok,'preview readiness')
  assert.match(facts.data,/(?:^|\/)archava-pg-fixture-[^/]+\/data$/,'The browser gate must own a disposable fixture database')
  evidence.fixtureCreation={freshDisposableDatabase:true,inheritedAttachmentIgnored:process.env.ARCHAVA_SLICE_ATTACH!==undefined}
  pass('FIXTURE-fresh-disposable-database',evidence.fixtureCreation)

  const executable=browserPath(); evidence.browserExecutable=executable; evidence.browserSandboxDisabled=process.env.E2E_BROWSER_NO_SANDBOX==='1'
  const profile=join(scratch,'browser'); mkdirSync(profile)
  const launchArgs=['--headless=new','--disable-gpu','--no-first-run','--no-default-browser-check','--remote-debugging-address=127.0.0.1','--remote-debugging-port=0',`--user-data-dir=${profile}`,'about:blank']
  if(mediaFixture)launchArgs.unshift('--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream')
  if(process.env.E2E_BROWSER_NO_SANDBOX==='1') launchArgs.unshift('--no-sandbox')
  const browserLog=createWriteStream(join(output,'chromium.log')); logs.push(browserLog)
  browser=spawn(executable,launchArgs,{stdio:['ignore','ignore','pipe']}); browser.stderr.pipe(browserLog)
  const cdpPort=await until(()=> { const f=join(profile,'DevToolsActivePort');return existsSync(f)&&Number(readFileSync(f,'utf8').split('\n')[0]) },'Chromium DevTools endpoint',120000)
  page=await Page.open(cdpPort)
  await page.send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false})
  // Reference pickers load their options on a real `focus` event, which a
  // headless page only fires while it holds focus. Without this the page was
  // focused only as a side effect of the media segment's mouse input, so the
  // product probes passed with ARCHAVA_E2E_MEDIA=1 and timed out without it.
  await page.send('Emulation.setFocusEmulationEnabled',{enabled:true})
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
  if(termsOnly){
    evidence.scope='Focused original-document terms and B01 journey: actual Chromium/SSR/WASM/HTTP/PostgreSQL and owned authenticated gateway. No full product, restart, native Ops, upgrade, TLS, load or live-model acceptance.'
    evidence.storeBankCycle=await verifyStoreBankCycle({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture,output})
    evidence.dueTerms=await verifyDueTerms({page,origin,facts,until,pass,suffix,store:evidence.storeBankCycle,output})
  }else if(clearingOnly){
    evidence.scope='Focused synthetic B01 and gateway-fee journeys only: actual Chromium/SSR/WASM/HTTP/PostgreSQL and owned authenticated synthetic gateway. No general product, restart, native Ops, upgrade, TLS, load or live provider acceptance.'
    evidence.storeBankCycle=await verifyStoreBankCycle({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture,output})
    evidence.gatewayClearing=await verifyGatewayClearing({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture,output})
  }else if(payablesOnly){
    evidence.scope='Focused synthetic supplier-payment journey only: actual Chromium/SSR/WASM/HTTP/PostgreSQL. No general product, restart, native Ops, upgrade, TLS, load or live provider acceptance.'
    evidence.supplierPayments=await verifySupplierPayments({page,origin,facts,until,pass,suffix,output})
  }else if(bankOnly){
    evidence.scope='Focused synthetic manual-bank journey only: actual Chromium/SSR/WASM/HTTP/PostgreSQL. No general product, restart, native Ops, upgrade, TLS, load or live provider acceptance.'
    evidence.bankReconciliation=await verifyBankReconciliation({page,origin,facts,until,pass,suffix,output})
  }else{
  // Hold the create-triggered list refresh until its route has been disposed.
  await page.evaluate(`(()=>{const original=window.fetch.bind(window);window.__contactRefreshHeld=false;window.__contactRefreshReleased=false;window.__holdNextContactRefresh=false;window.fetch=async(input,init)=>{const request=input instanceof Request?new Request(input.clone(),init):new Request(input,init);const path=new URL(request.url).pathname;const body=request.method==='POST'&&path==='/crm/commands'?await request.clone().json():null;const watched=body?.command?.operation==='contact.create'&&body.command.data.fullName===${JSON.stringify(name)};const response=await original(input,init);if(watched&&response.ok)window.__holdNextContactRefresh=true;if(request.method==='GET'&&path==='/contacts'&&window.__holdNextContactRefresh){window.__holdNextContactRefresh=false;window.__contactRefreshHeld=true;await new Promise(resolve=>window.__releaseContactRefresh=resolve);window.__contactRefreshReleased=true;}return response;};})()`)
  await page.fill(testId('contact-new-name'),name);await page.fill(testId('contact-new-email'),`audit-${suffix}@archava.test`);await page.fill(testId('contact-new-org'),'Synthetic launch fixture');await page.click(testId('contact-create'))
  await until(()=>page.evaluate("!!document.querySelector('[data-testid=contact-create-open]')"),'durable contact create')
  await until(()=>page.evaluate('window.__contactRefreshHeld===true'),'create-triggered list refresh held before navigation')
  await page.click(testId('contact-create-open'));await until(()=>page.text(testId('record-name')).then((t)=>t===name),'created record view')
  await page.evaluate('window.__releaseContactRefresh()');await until(()=>page.evaluate('window.__contactRefreshReleased===true'),'old contacts response released after route disposal');await page.evaluate('new Promise(resolve=>setTimeout(resolve,50))')
  const contact=await page.evaluate("new URL(location.href).searchParams.get('id')");assert.match(contact,/^[0-9a-f-]{36}$/i)
  const createdCounts=databaseCounts(facts.port,contact,contact)
  assert.equal(createdCounts.contact_rows,1);assert.equal(createdCounts.same_email_rows,1);assert.equal(createdCounts.followups,0)
  pass('F02-contact-navigation-idempotency','Opening the new record creates no second mutation; an old list response arriving after route disposal does not corrupt the record page or panic')
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
  evidence.orderInvoice=await verifyOrderInvoice({page,origin,facts,until,pass,suffix})
  evidence.customerIdentity=await verifyCustomerIdentity({page,origin,facts,until,pass,suffix})
  evidence.supplierIdentity=await verifySupplierIdentity({page,origin,facts,until,pass,suffix})
  evidence.organizationIdentity=await verifyOrganizationIdentity({page,origin,facts,until,pass,suffix})
  evidence.parties=await verifyParties({page,origin,facts,until,pass,suffix})
  evidence.partyTransactions=await verifyPartyTransactions({page,origin,facts,until,pass,suffix,parties:evidence.parties})
  evidence.companies=await verifyCompanies({page,origin,facts,until,pass,suffix})
  evidence.companyTrade=await verifyCompanyTrade({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture,emailProvider:emailFixture})
  evidence.supplierDocuments=await verifySupplierDocuments({page,origin,facts,until,pass,suffix,trade:evidence.companyTrade})
  evidence.accounting=await verifyAccounting({page,origin,facts,until,pass,suffix})
  evidence.invoicePosting=await verifyInvoicePosting({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture})
  evidence.purchasePosting=await verifyPurchasePosting({page,origin,facts,until,pass,suffix})
  evidence.stockPosting=await verifyStockPosting({page,origin,facts,until,pass,suffix,purchase:evidence.purchasePosting})
  evidence.storeFinanceCycle=await verifyStoreFinanceCycle({page,origin,facts,pass,suffix,paymentProvider:paymentFixture})
  evidence.supplierPayments=await verifySupplierPayments({page,origin,facts,until,pass,suffix,output})
  evidence.storeBankCycle=await verifyStoreBankCycle({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture,output})
  evidence.dueTerms=await verifyDueTerms({page,origin,facts,until,pass,suffix,store:evidence.storeBankCycle,output})
  evidence.gatewayClearing=await verifyGatewayClearing({page,origin,facts,until,pass,suffix,paymentProvider:paymentFixture,output})
  evidence.bankReconciliation=await verifyBankReconciliation({page,origin,facts,until,pass,suffix,output})
  // Owned transport endpoints are preview-only; the production parser keeps its canonical HTTPS rules.
  const resumeAgent=process.env.ARCHAVA_E2E_AGENT_RUNTIME==='1'?await prepareAgentRestart({page,facts,until,pass}):null
  // Restart the actual production executable on the same migrated fixture and session.
  server.kill('SIGKILL');await new Promise((r)=>server.once('exit',r))
  const productionEnvironment={DATABASE_URL:`postgres://archava_runtime:archava@127.0.0.1:${facts.port}/archava_test`,ARCHAVA_IDENTITY_KEY_HEX:'07'.repeat(32),ARCHAVA_BIND_ADDR:`127.0.0.1:${port}`,ARCHAVA_SITE_ROOT:join(root,'target/site'),ARCHAVA_TRUSTED_PROXIES:'127.0.0.1,::1',...(mediaFixture?.environment??{})}
  server=startServer(production,{...productionEnvironment,...(plannerFixture?.environment??{}),ARCHAVA_WRITER_TAKEOVER:'true'},'production-restart.log')
  await until(async()=> (await fetch(`${origin}/readyz`)).ok,'production binary readiness after restart',60000)
  if(resumeAgent)await resumeAgent()
  if(plannerFixture)evidence.aiDisclosure=await verifyAiDisclosure({page,origin,until,pass,fixture:plannerFixture,suffix})
  evidence.agentFinance=await verifyAgentFinance({page,origin,facts,until,pass,suffix})
  const readLease=()=>readWriterLease(facts.port)
  await verifyStartupRefusals({production,env:productionEnvironment,root,output,origin,freePort,readLease,pass})
  await page.navigate(`${origin}/app/recover?id=${lostId}`);await until(()=>page.text(testId('recover-status')).then((t)=>t.includes('Committed')),'receipt survives process restart')
  assert.deepEqual(databaseCounts(facts.port,contact,lostId),{same_email_rows:1,contact_rows:1,followups:expectedFollowups,receipt_rows:1})
  pass('F03-production-restart','Fresh production process, same PostgreSQL and browser session, committed receipt survives')
  await page.screenshot('recovery-after-restart.png')


  const companyRecovery=await page.evaluate(`(async()=>{const out={};for(const domain of ['commerce','purchasing','billing']){const r=await fetch('/erp/'+domain+'/workspace?tenant=acme-retail&companyId=${evidence.companyTrade.company}');out[domain]={httpStatus:r.status,...await r.json()};}const history=await fetch('/erp/billing/commands/status?tenant=acme-retail&requestId=${evidence.companyTrade.invoiceRequestId}');out.receipt={httpStatus:history.status,data:await history.json()};return out;})()`)
  for(const domain of ['commerce','purchasing','billing'])assert.equal(companyRecovery[domain].httpStatus,200)
  const recoveredProduct=companyRecovery.commerce.products.find(p=>p.id===evidence.companyTrade.product),recoveredSale=companyRecovery.commerce.orders.find(o=>o.id===evidence.companyTrade.sale),recoveredPurchase=companyRecovery.purchasing.orders.find(o=>o.id===evidence.companyTrade.purchase),recoveredInvoice=companyRecovery.billing.invoices.find(i=>i.id===evidence.companyTrade.invoice)
  assert.equal(recoveredProduct.quantity,6);assert.equal(recoveredSale.state,'fulfilled');assert.equal(recoveredPurchase.state,'received');assert.equal(recoveredInvoice.status,'settled');assert.equal(recoveredInvoice.settled,30000);assert.equal(recoveredInvoice.companyRevision,3);assert.equal(companyRecovery.receipt.httpStatus,200);assert.equal(companyRecovery.receipt.data.status,'recorded');assert.equal(companyRecovery.receipt.data.receipt.invoiceId,evidence.companyTrade.invoice)
  evidence.companyTrade.productionRecovery={verified:true,companyId:recoveredInvoice.companyId,stock:recoveredProduct.quantity,saleState:recoveredSale.state,purchaseState:recoveredPurchase.state,invoiceState:recoveredInvoice.status,settled:recoveredInvoice.settled,originalInvoiceReceipt:true}
  pass('COMPANYTRADE-production-restart',evidence.companyTrade.productionRecovery)
  const supplierRecovery=await page.evaluate(`(async()=>{const r=await fetch('/erp/purchasing/workspace?tenant=acme-retail&companyId=${evidence.supplierDocuments.company}');const w=await r.json();const old=await fetch('/erp/purchasing/commands/status?tenant=acme-retail&requestId=${evidence.supplierDocuments.originalRequestId}');return {httpStatus:r.status,document:w.documents.find(d=>d.id==='${evidence.supplierDocuments.document}'),receiptStatus:old.status,receipt:await old.json()};})()`)
  assert.equal(supplierRecovery.httpStatus,200);assert.equal(supplierRecovery.document.state,'matched');assert.equal(supplierRecovery.document.revision,7);assert.equal(supplierRecovery.document.activeAllocatedQuantity,3);assert.equal(supplierRecovery.receiptStatus,200);assert.equal(supplierRecovery.receipt.receipt.revision,1)
  evidence.supplierDocuments.productionRecovery={verified:true,state:'matched',revision:7,activeAllocatedQuantity:3,originalCaptureRevision:1}
  pass('SUPPLIERDOC-production-restart',evidence.supplierDocuments.productionRecovery)
  const sourceRecovery=await page.evaluate(`fetch('/erp/accounting/reports?tenant=acme-retail&bookId=${evidence.invoicePosting.book}&from=2000-01-01&to=2100-12-31').then(async r=>({status:r.status,body:await r.json()}))`)
  assert.equal(sourceRecovery.status,200);assert.equal(sourceRecovery.body.sourceCoverage.capturedCount,5);assert.equal(sourceRecovery.body.sourceCoverage.unpostedCount,0);assert.equal(sourceRecovery.body.balanceSheet.assetsMinor,'0');assert.equal(sourceRecovery.body.profitAndLoss.revenueMinor,'0')
  evidence.invoicePosting.productionRecovery={status:200,capturedSources:5,unpostedSources:0,assetsMinor:'0',revenueMinor:'0'}
  pass('INVOICEPOST-production-restart',evidence.invoicePosting.productionRecovery)
  for(const [name,journey,expectedSources,expectedAssets,expectedProfit] of [['B01',evidence.storeBankCycle,8,'115000000','15000000'],['CLEARING',evidence.gatewayClearing,4,'2970000','1970000']]){
    const report=await page.evaluate(`fetch('/erp/accounting/reports?tenant=acme-retail&bookId=${journey.book}&from=2000-01-01&to=2100-12-31').then(async r=>({status:r.status,body:await r.json()}))`)
    assert.equal(report.status,200);assert.equal(report.body.balanceSheet.assetsMinor,expectedAssets);assert.equal(report.body.profitAndLoss.netProfitMinor,expectedProfit);assert.equal(report.body.trialBalance.balanced,true);assert.equal(report.body.sourceCoverage.postedCount,expectedSources);assert.equal(report.body.sourceCoverage.unpostedCount,0)
    const original=journey.settlementReceipt,status=await page.evaluate(`fetch('/erp/accounting/commands/status?tenant=acme-retail&requestId=${original.requestId}').then(async r=>({status:r.status,body:await r.json()}))`)
    assert.equal(status.status,200);for(const key of ['sourceId','bankRowId','resourceId'])assert.equal(status.body.receipt[key],original[key]);assert.equal(status.body.receipt.revision,1)
    journey.productionRecovery={actualNativeReports:true,sourceCount:expectedSources,assetsMinor:expectedAssets,profitMinor:expectedProfit,originalDraftReceiptRevision:1,originalSourceAndBankReferencesPreserved:true}
    pass(name+'-production-restart',journey.productionRecovery)
  }
  const dueRecovery=await page.evaluate(`(async()=>{const r=await fetch('/erp/accounting/subledger-report?tenant=acme-retail&bookId=${evidence.dueTerms.book}&side=receivable&asOf=${evidence.dueTerms.asOf}');const old=await fetch('/erp/accounting/commands/status?tenant=acme-retail&requestId=${evidence.dueTerms.knownReceipt.requestId}');return{status:r.status,report:await r.json(),receiptStatus:old.status,original:await old.json()};})()`)
  assert.equal(dueRecovery.status,200);assert.equal(dueRecovery.report.totals.netOutstandingMinor,'1250000');assert.equal(dueRecovery.report.totals.unknownTermsMinor,'1250000');assert.equal(dueRecovery.report.postedSourceControlsAgree,true);assert.equal(dueRecovery.receiptStatus,200);assert.equal(dueRecovery.original.receipt.revision,1);assert.deepEqual(dueRecovery.original.receipt.documentTerms,evidence.dueTerms.knownReceipt.documentTerms)
  evidence.dueTerms.productionRecovery={actualNativeReport:true,netOutstandingMinor:'1250000',originalKnownTermsReceiptRevision:1,latestUnknownTermsPreserved:true}
  pass('DUE-production-restart',evidence.dueTerms.productionRecovery)
  const stockRecovery=await page.evaluate(`(async()=>{const w=await fetch('/erp/accounting/workspace?tenant=acme-retail&companyId=${evidence.stockPosting.company}&collection=stock_sources');const r=await fetch('/erp/accounting/reports?tenant=acme-retail&bookId=${evidence.stockPosting.book}&from=2000-01-01&to=2100-12-31');return{workspaceStatus:w.status,workspace:await w.json(),reportStatus:r.status,report:await r.json()};})()`)
  assert.equal(stockRecovery.workspaceStatus,200);assert.equal(stockRecovery.reportStatus,200);assert.equal(stockRecovery.workspace.stockPolicy.revision,2);assert.equal(stockRecovery.workspace.stock_sources.length,4);assert.equal(stockRecovery.workspace.stock_sources.filter(s=>s.sourceState==='posted').length,3);assert.equal(stockRecovery.report.sourceCoverage.capturedCount,7);assert.equal(stockRecovery.report.sourceCoverage.unpostedCount,0);assert.equal(stockRecovery.report.profitAndLoss.costOfGoodsSoldMinor,'1410100');assert.equal(stockRecovery.report.trialBalance.balanced,true)
  evidence.stockPosting.productionRecovery={verified:true,workspaceStatus:200,reportStatus:200,stockPolicyRevision:2,capturedStockSources:4,postedStockSources:3,capturedCombinedSources:7,unpostedCombinedSources:0,costOfGoodsSoldMinor:'1410100'}
  pass('STOCKPOST-production-restart',evidence.stockPosting.productionRecovery)
  const storeRecovery=await page.evaluate(`fetch('/erp/accounting/reports?tenant=acme-retail&bookId=${evidence.storeFinanceCycle.book}&from=2000-01-01&to=2100-12-31').then(async r=>({status:r.status,body:await r.json()}))`)
  assert.equal(storeRecovery.status,200);assert.equal(storeRecovery.body.sourceCoverage.capturedCount,8);assert.equal(storeRecovery.body.sourceCoverage.postedCount,8);assert.equal(storeRecovery.body.sourceCoverage.unpostedCount,0);assert.deepEqual(storeRecovery.body.profitAndLoss,evidence.storeFinanceCycle.profitAndLoss);assert.deepEqual(storeRecovery.body.balanceSheet,evidence.storeFinanceCycle.balanceSheet);assert.equal(storeRecovery.body.trialBalance.balanced,true)
  evidence.storeFinanceCycle.productionRecovery={verified:true,status:200,postedSources:8,netProfitMinor:storeRecovery.body.profitAndLoss.netProfitMinor,assetsMinor:storeRecovery.body.balanceSheet.assetsMinor,liabilitiesMinor:storeRecovery.body.balanceSheet.liabilitiesMinor}
  pass('STORECYCLE-production-restart',evidence.storeFinanceCycle.productionRecovery)
  const bankRecovery=await page.evaluate(`fetch('/erp/accounting/bank-reconciliation?tenant=acme-retail&bankPolicyId=${evidence.bankReconciliation.policy}&periodId=${evidence.bankReconciliation.period}').then(async r=>({status:r.status,body:await r.json()}))`)
  assert.equal(bankRecovery.status,200);assert.equal(bankRecovery.body.periodState,'closed');assert.equal(bankRecovery.body.check.ready,true);assert.equal(bankRecovery.body.reviewed,true);assert.equal(bankRecovery.body.check.evidenceSha256,evidence.bankReconciliation.evidenceSha256)
  evidence.bankReconciliation.productionRecovery={verified:true,status:200,periodState:'closed',currentEvidenceReviewed:true,evidenceSha256:bankRecovery.body.check.evidenceSha256}
  pass('BANK-production-restart',evidence.bankReconciliation.productionRecovery)





  await page.navigate(`${origin}/app/owner`);await until(()=>page.evaluate("!!document.querySelector('[data-testid=owner-preview]') && !document.querySelector('[data-testid=owner-preview]').disabled"),'owner configuration read')
  await page.click(testId('owner-preview'));await until(()=>page.evaluate("!!document.querySelector('[data-testid=owner-confirm]') && !document.querySelector('[data-testid=owner-confirm]').disabled"),'owner reviewed plan')
  await page.click(testId('owner-confirm'));await until(()=>page.text(testId('owner-status')).then(t=>t.includes('committed revision')),'matching owner receipt')
  const ownerConfig=await page.evaluate("fetch('/own/config?tenant=acme-retail').then(r=>r.json())");assert.ok(ownerConfig.config.revision>=1)
  await page.screenshot('owner-configuration-receipt.png');pass('F05-owner-confirmation','Owner reviews and confirms a configuration revision; a matching receipt and persisted revision are present')
  evidence.ownerConfiguration=await verifyOwnerConfiguration({page,origin,facts,contact,until,pass,openTab:async()=>{
    const {targetId}=await page.send('Target.createTarget',{url:'about:blank'})
    const tab=await Page.open(cdpPort,targetId)
    return {page:tab,close:async()=>{tab.ws.close();await page.send('Target.closeTarget',{targetId})}}
  }})
  await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await login('bob')
  await selectDraft(name);await page.fill(testId('draft-summary'),'Must be refused');await page.click(testId('draft-submit'))
  await until(()=>page.text(testId('draft-status')).then((t)=>t.includes('Refused')),'read-only actor write refusal')
  assert.deepEqual(databaseCounts(facts.port,contact,lostId),{same_email_rows:1,contact_rows:1,followups:expectedFollowups,receipt_rows:1})
  const crossTenant=await page.evaluate("fetch('/contacts?tenant=globex-corp').then(r=>r.status)");assert.ok([403,404].includes(crossTenant),`Cross-tenant HTTP ${crossTenant}`)
  pass('F04-role-and-tenant-refusal','Read-only UI submission creates no effect; foreign tenant read refused')
  // OWN-T04: a signed-in principal without a management grant gets a refusal, not the settings.
  const ownerBefore=evidence.ownerConfiguration.finalRevision
  await page.navigate(`${origin}/app/owner`);await until(()=>page.text(testId('owner-refusal')).then((t)=>t.includes('missing_grant')),'non-owner settings refusal')
  assert.equal(await page.evaluate("!!document.querySelector('[data-testid=owner-modules]')||!!document.querySelector('[data-testid=owner-preview]')||!!document.querySelector('[data-testid=owner-history] li')"),false,'A refused principal sees no modules, preview control or history')
  const nonOwner=await page.evaluate(`(async()=>{const h={'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''};const plan=await fetch('/own/plan',{method:'POST',headers:h,body:JSON.stringify({tenant:'acme-retail',request:{baseRevision:${ownerBefore},disable:['crm-pipeline']}})});const chat=await fetch('/own/chat',{method:'POST',headers:h,body:JSON.stringify({tenant:'acme-retail',message:'disable crm-pipeline'})});const history=await fetch('/own/history?tenant=acme-retail');return{plan:[plan.status,(await plan.json()).reason],chat:[chat.status,(await chat.json()).reason],history:[history.status,(await history.json()).reason]};})()`)
  assert.deepEqual(nonOwner,{plan:[403,'missing_grant'],chat:[403,'missing_grant'],history:[403,'missing_grant']})
  await page.screenshot('owner-non-owner-refused.png');pass('OWNER-native-non-owner-refused','A signed-in principal without a management grant sees the refusal at the settings page with no modules, preview or history, and plan, chat and history are refused by the host')
  await page.navigate(`${origin}/app/contacts`);await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true})
  await until(()=>page.evaluate('document.documentElement.scrollWidth<=innerWidth'),'mobile overflow check')
  await page.screenshot('contacts-mobile.png');pass('UI-mobile-layout','390px viewport has no horizontal page overflow')
  assert.deepEqual(page.errors,[],'Unexpected JavaScript/WASM errors')
  if(mediaFixture)evidence.media=await verifyMedia({page,origin,fixture:mediaFixture,until,pass})
  if(process.env.ARCHAVA_E2E_PRODUCT==='1') evidence.product=await verifyProduct({page,origin,facts,until,output,operations:evidence.operations,contact,media:evidence.media})
  if(liveAgent) evidence.agentLive=await verifyAgentLive({page,origin,facts,until,output,pass})
  if(process.env.ARCHAVA_E2E_AGENT_RUNTIME==='1') evidence.agentRuntime=await verifyAgentRuntime({page,origin,facts,until,output,pass})
  if(process.env.ARCHAVA_E2E_AGENT_RUNTIME==='1') evidence.agentNavigation=await verifyAgentNavigation({page,origin,until,pass})
  if(process.env.ARCHAVA_E2E_AGENTIC==='1') evidence.agentic=await verifyAgentic({page,origin,facts,until,output})
  if(process.env.ARCHAVA_E2E_SIMULATIONS==='1') evidence.simulations=await verifySimulations({page,origin,facts,until,output})
  if(process.env.ARCHAVA_E2E_TLS==='1') evidence.tls=await verifyTls({origin,scratch,output,freePort,until,pass,terminate})
  if(process.env.ARCHAVA_E2E_LOAD==='1') evidence.load=await verifyLoad({origin,facts,pass})
  if(process.env.ARCHAVA_E2E_OPS_ROOT) evidence.ops=await verifyOps({packageRoot:process.env.ARCHAVA_E2E_OPS_ROOT,facts,scratch,productionEnvironment,startServer,terminate,until,freePort,pass,dueTerms:evidence.dueTerms})
  if(process.env.ARCHAVA_E2E_PREVIOUS_NATIVE){assert.ok(process.env.ARCHAVA_E2E_OPS_ROOT,'Previous native upgrade proof also needs the current verified package');evidence.nativeUpgrade=await verifyPriorNative({previousRoot:process.env.ARCHAVA_E2E_PREVIOUS_NATIVE,packageRoot:process.env.ARCHAVA_E2E_OPS_ROOT,facts,scratch,productionEnvironment,startServer,terminate,until,freePort,pass})}
  if(process.env.ARCHAVA_E2E_DISCLOSURE_PREVIOUS_NATIVE){assert.ok(process.env.ARCHAVA_E2E_OPS_ROOT,'Disclosure downgrade proof needs the current verified package');evidence.nativeDisclosureUpgrade=await verifyPriorNative({previousRoot:process.env.ARCHAVA_E2E_DISCLOSURE_PREVIOUS_NATIVE,packageRoot:process.env.ARCHAVA_E2E_OPS_ROOT,facts,scratch,productionEnvironment,startServer:(binary,env,name)=>startServer(binary,env,`disclosure-${name}`),terminate,until,freePort,pass:(id,details)=>pass(`DISCLOSURE-${id}`,details),requireDisclosureMigration:true})}
  if(process.env.ARCHAVA_E2E_BANK_PREVIOUS_NATIVE){assert.ok(process.env.ARCHAVA_E2E_OPS_ROOT,'Legacy bank receipt recovery needs the current verified package');evidence.legacyBankReceipts=await verifyLegacyBankReceipts({previousRoot:process.env.ARCHAVA_E2E_BANK_PREVIOUS_NATIVE,packageRoot:process.env.ARCHAVA_E2E_OPS_ROOT,facts,productionEnvironment,startServer,terminate,until,freePort,pass})}
  await verifyWriterLifecycle({production,env:productionEnvironment,origin,original:server,startServer,terminate,until,freePort,readLease,pass})
  }
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
  if(plannerFixture)await plannerFixture.close()
  if(emailFixture)await emailFixture.close()
  if(mediaFixture)await mediaFixture.close()
  for(const log of logs) await new Promise((r)=>log.end(r))
  rmSync(scratch,{recursive:true,force:true})
  evidence.finishedAt=new Date().toISOString()
  evidence.artifacts=[preview,production,join(root,'target/site/pkg/archava-slice_bg.wasm')].map((path)=>({path:path.slice(root.length+1),bytes:readFileSync(path).length,sha256:createHash('sha256').update(readFileSync(path)).digest('hex')}))
  writeFileSync(join(output,'result.json'),`${JSON.stringify(evidence,null,2)}\n`)
}
