/** Agentic product diagnostic. Completed probes can expose readiness gaps. */
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { sourceManifest, sha256 } from '../rust/provenance.mjs'

export async function verifyAgentic({ page, origin, facts, until, output }) {
  assert.equal(new URL(origin).hostname, '127.0.0.1')
  assert.ok(Number.isInteger(facts.port) && facts.port > 1023 && facts.port < 65536)
  const requirements = [], traces = [], suffix = randomUUID().slice(0, 8)
  const t = id => `[data-testid="${id}"]`
  const sql = statement => {
    const result = spawnSync('psql', ['-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1', '-p', String(facts.port), '-U', 'postgres', '-d', 'archava_test'], { env: { ...process.env, PGPASSWORD: 'archava' }, input: statement, encoding: 'utf8', timeout: 30000 })
    assert.equal(result.status, 0, result.stderr)
    return result.stdout.trim()
  }
  const snapshot = () => JSON.parse(sql(`SELECT json_build_object(
    'missions',(SELECT count(*) FROM archava_cockpit.missions WHERE tenant_id='acme-retail'),
    'purchaseOrders',(SELECT count(*) FROM archava_purchasing.orders WHERE tenant_id='acme-retail'),
    'crmCommands',(SELECT count(*) FROM archava_crm.commands WHERE tenant_id='acme-retail'),
    'nodes',(SELECT count(*) FROM archava_orbz.nodes WHERE tenant_id='acme-retail'),
    'episodes',(SELECT count(*) FROM archava_orbz.episodes WHERE tenant_id='acme-retail'));`))
  const observe = (id, expected, actual, met) => {
    requirements.push({ id, expected, actual, status: met ? 'met' : 'gap' })
    writeFileSync(join(output, 'agentic-observations.json'), JSON.stringify({ auditCompleted: false, requirements, traces }, null, 2) + '\n')
    process.stdout.write(`AGENTIC ${met ? 'MET' : 'GAP'} ${id}\n`)
  }
  await page.send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 1000, deviceScaleFactor: 1, mobile: false })
  await page.send('Network.clearBrowserCookies'); await page.evaluate('localStorage.clear()')
  await page.navigate(`${origin}/app`)
  await page.fill(t('login-username'), 'alice@archava.test'); await page.fill(t('login-password'), 'correct horse battery staple'); await page.fill(t('login-tenant'), 'acme-retail'); await page.click(t('login-submit'))
  await until(() => page.evaluate(`location.pathname==='/app/contacts'&&!!document.querySelector('${t('contacts-count')}')`), 'agentic diagnostic login')
  const call = async (method, path, body) => {
    const answer = await page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(method)},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body === undefined ? '' : `,body:${JSON.stringify(JSON.stringify(body))}`}}).then(async r=>({status:r.status,body:await r.json().catch(()=>null)}))`)
    traces.push({ method, path, input: body ?? null, ...answer })
    return answer
  }
  const post = async (path, body) => { const answer = await call('POST', path, body); assert.ok([200, 201].includes(answer.status), `${path}: ${JSON.stringify(answer)}`); return answer.body }
  const ask = utterance => post('/api/crm/acme-retail/conversation', { utterance })
  await page.navigate(`${origin}/app/home`)
  const home = await page.evaluate(`({heading:document.querySelector('h1')?.textContent,prompts:document.querySelectorAll('textarea,input:not([type=hidden])').length,assistantLink:!!document.querySelector('a[href="/app/desk"]'),text:document.querySelector('main')?.textContent?.slice(0,900)})`)
  observe('AG-01-agent-entry', 'The primary entry invites a business goal and shows agent work', home, home.prompts > 0)
  await page.screenshot('agentic-home.png')
  const overview = await ask('show crm overview')
  const contacts = Number(sql("SELECT count(*) FROM archava_crm.contacts WHERE tenant_id='acme-retail'"))
  assert.equal(overview.source, 'crm'); assert.equal(overview.data.snapshot.contacts, contacts)
  observe('AG-02-grounded-crm', 'Supported assistant commands use real authorized business data', { planner: overview.planner, reportedContacts: overview.data.snapshot.contacts, sqlContacts: contacts }, true)
  const beforeGoals = snapshot()
  const stockGoal = 'Ava, urus kebutuhan stok minggu depan. Periksa stok dan pemasok, buat rencana pembelian, lalu minta persetujuan saya sebelum menjalankannya.'
  const stock = await ask(stockGoal)
  observe('AG-03-goal-to-plan', 'A business goal yields a grounded plan or targeted clarification', stock, stock.data?.kind !== 'help' && stock.source !== 'none')
  const invoices = await ask('Invoice mana yang sudah jatuh tempo dan apa tindakan yang perlu saya setujui?')
  observe('AG-04-cross-domain-context', 'The main assistant can use Billing context in a business conversation', invoices, invoices.source !== 'none' && invoices.data?.kind !== 'help')
  const preference = await ask('Ingat untuk bisnis saya: pembelian di atas Rp 100.000 harus saya tinjau lebih dulu.')
  const recall = await ask('Apa aturan persetujuan pembelian yang baru saya sampaikan?')
  const afterGoals = snapshot()
  assert.equal(afterGoals.purchaseOrders, beforeGoals.purchaseOrders)
  assert.equal(afterGoals.crmCommands, beforeGoals.crmCommands)
  observe('AG-05-conversation-memory', 'A later turn recalls the stated business preference under the same principal', { preference, recall, before: beforeGoals, after: afterGoals }, recall.source !== 'none' && afterGoals.episodes > beforeGoals.episodes)
  observe('AG-06-goal-mission-bridge', 'A reviewed goal can become a durable mission through the conversation', { missionsBefore: beforeGoals.missions, missionsAfter: afterGoals.missions, reply: stock }, afterGoals.missions > beforeGoals.missions)
  await page.navigate(`${origin}/app/desk`)
  await until(() => page.evaluate(`!!document.querySelector('${t('desk-reply')}')`), 'assistant initial grounded answer')
  await page.fill(t('desk-prompt'), stockGoal); await page.click(t('desk-send'))
  await until(() => page.evaluate(`!!document.querySelector('${t('desk-reply')}')&&!document.querySelector('${t('desk-status')}')`), 'customer goal answer')
  const visibleGoalReply = await page.text(t('desk-reply'))
  await page.screenshot('agentic-customer-goal.png')
  observe('AG-07-native-goal-experience', 'The native assistant takes the customer toward the requested goal', { prompt: stockGoal, visibleReply: visibleGoalReply }, stock.source !== 'none')

  const skus = [`AGENTIC-A-${suffix}`, `AGENTIC-B-${suffix}`].map(s => s.toUpperCase())
  const countProducts = () => Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku IN ('${skus[0]}','${skus[1]}')`))
  const mission = await post('/api/v1/cockpit/missions', { tenant_id: 'acme-retail', mission_code: `agentic-${suffix}`, title: 'Explicit two-step execution diagnostic', description: 'Test supplied the steps; no claim of model planning', auto_launch: true, steps: skus.map((sku, step_index) => ({ step_index, step_name: `Create product ${step_index + 1}`, tool_name: 'commerce.product.create', payload_input: { sku, title: `Agentic diagnostic ${step_index + 1}`, unit: 'unit', trackStock: true, price: 1000 }, depends_on: step_index === 0 ? [] : [0] })) })
  const id = mission.id, path = `/api/v1/cockpit/missions/${id}`, run = stepIndex => call('POST', `${path}/run-next?tenant=acme-retail`, { stepIndex }), get = () => postRead(`${path}?tenant=acme-retail`)
  async function postRead(url) { const answer = await call('GET', url); assert.equal(answer.status, 200); return answer.body }
  const unapproved = await run(0)
  assert.equal(unapproved.status, 409); assert.equal(countProducts(), 0)
  observe('AG-08-approval-enforced', 'Business execution waits for approval with zero effects', { status: unapproved.status, productRows: countProducts() }, true)
  for (let i = 0; i < 2; i++) await post(`${path}/steps/${i}/approve?tenant=acme-retail`, {})
  const outOfOrder = await run(1)
  assert.equal(outOfOrder.status, 409); assert.equal(countProducts(), 0)
  observe('AG-09-dependency-enforced', 'A dependent step cannot run before its prerequisite', { status: outOfOrder.status, productRows: countProducts() }, true)
  const first = await run(0)
  assert.equal(first.status, 200); assert.equal(countProducts(), 1)
  const replay = await run(0)
  assert.equal(replay.status, 200); assert.equal(countProducts(), 1)
  observe('AG-10-verified-executor', 'Explicit approved step commits once and replays a verified receipt', { first: first.body, replay: replay.body, productRows: countProducts() }, first.body.steps[0].payload_output.verified === true)
  await new Promise(resolve => setTimeout(resolve, 1500))
  const waiting = await get()
  observe('AG-11-unattended-continuation', 'An approved mission advances eligible steps without another operator execution request', { observationMs: 1500, missionStatus: waiting.status, steps: waiting.steps.map(s => ({ index: s.step_index, status: s.status })), productRows: countProducts(), limit: 'Short observation plus source inspection, not a scheduling SLA test' }, countProducts() === 2)
  const permissions = JSON.parse(sql("SELECT to_json(permissions)::text FROM archava_commerce.memberships WHERE tenant_id='acme-retail' AND principal_id='alice'"))
  assert.ok(permissions.every(p => /^[a-z][a-z.-]+$/.test(p)))
  try {
    sql("UPDATE archava_commerce.memberships SET permissions=array_remove(permissions,'catalog.write') WHERE tenant_id='acme-retail' AND principal_id='alice'")
    const revoked = await run(1)
    assert.equal(revoked.status, 403); assert.equal(countProducts(), 1)
    observe('AG-12-current-domain-authority', 'Approval does not bypass a later revoked domain grant', { status: revoked.status, productRows: countProducts() }, true)
  } finally {
    sql(`UPDATE archava_commerce.memberships SET permissions=ARRAY[${permissions.map(p => `'${p}'`).join(',')}] WHERE tenant_id='acme-retail' AND principal_id='alice'`)
  }
  const second = await run(1)
  assert.equal(second.status, 200); assert.equal(second.body.status, 'touchdown'); assert.equal(countProducts(), 2)
  observe('AG-13-manual-mission-completion', 'The explicit executor can complete a two-step mission under restored current grants', { missionId: id, status: second.body.status, productRows: countProducts(), verified: second.body.steps.every(s => s.payload_output?.verified === true) }, true)
  const report = await ask(`Laporkan hasil misi ${id} dan tunjukkan bukti pekerjaan yang sudah selesai.`)
  observe('AG-14-conversational-outcome-report', 'The agent reports the actual mission outcome and its verified receipts', report, report.source !== 'none' && report.data?.kind !== 'help')
  await page.navigate(`${origin}/app/contacts`)
  const counts = { met: requirements.filter(r => r.status === 'met').length, gaps: requirements.filter(r => r.status === 'gap').length }
  const build = JSON.parse(readFileSync('target/site/build-provenance.json', 'utf8')), source = sourceManifest(process.cwd())
  assert.equal(source.sha256, build.source.sha256)
  const result = { auditCompleted: true, agenticReady: counts.gaps === 0, scope: 'Goal-driven agentic product diagnostic. Foundation passes do not certify the complete agent journey. No live model/provider acceptance.', sourceSha256: source.sha256, scriptSha256: sha256('tools/e2e/agentic.mjs'), counts, requirements, traces, unverified: ['Live model reasoning and planner quality: no model planner is wired to the main chat', 'Voice-to-business tool execution: media transport proof is separate', 'Automated replanning and mission output-to-input references: no runtime proof in this audit'] }
  writeFileSync(join(output, 'agentic-observations.json'), JSON.stringify(result, null, 2) + '\n')
  return result
}
