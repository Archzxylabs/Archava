/** Owned Resend-shaped transport plus real-browser invoice email acceptance. */
import assert from 'node:assert/strict'
import { createServer } from 'node:http'

const key = 'synthetic-browser-email-key'

async function mutate(page, path, body) {
  const csrf = await page.evaluate("localStorage.getItem('archava_csrf') || ''")
  assert.ok(csrf, 'CSRF token present')
  return page.evaluate(`fetch(${JSON.stringify(path)},{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':${JSON.stringify(csrf)}},body:${JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json().catch(()=>null)}))`)
}

export async function startEmailFixture() {
  const state = { sends: 0, keys: [], ids: [], bodies: [], failNext: false }
  const server = createServer(async (req, res) => {
    try {
      assert.equal(req.headers.authorization, `Bearer ${key}`)
      let body = ''
      for await (const chunk of req) { body += chunk; assert.ok(body.length < 65536) }
      if (req.method !== 'POST' || req.url !== '/emails') { res.writeHead(404); res.end('{}'); return }
      const payload = JSON.parse(body)
      state.sends += 1
      state.keys.push(req.headers['idempotency-key'])
      state.bodies.push(payload)
      if (state.failNext) {
        state.failNext = false
        res.writeHead(500, { 'content-type': 'application/json' })
        res.end('{"message":"synthetic provider failure"}')
        return
      }
      const id = `owned-email-${state.ids.length + 1}`
      state.ids.push(id)
      res.writeHead(200, { 'content-type': 'application/json' })
      res.end(JSON.stringify({ id }))
    } catch {
      res.writeHead(500)
      res.end('{}')
    }
  })
  await new Promise((r) => server.listen(0, '127.0.0.1', r))
  const origin = `http://127.0.0.1:${server.address().port}`
  return { origin, state, close: async () => { server.closeAllConnections(); await new Promise((r) => server.close(r)) } }
}

export async function verifyEmail({ page, origin, until, pass, suffix, provider }) {
  const t = (id) => `[data-testid="${id}"]`
  await page.navigate(`${origin}/app/erp?module=invoices`)
  await until(() => page.evaluate(`!!document.querySelector('${t('invoice-save')}')`), 'invoice workspace for email')
  const code = `MAIL-${suffix}`
  await page.fill(t('invoice-code'), code)
  await page.fill(t('invoice-customer'), 'Synthetic email customer')
  await page.fill(t('invoice-reference'), 'email-proof')
  await page.fill(t('invoice-description'), 'Service')
  await page.fill(t('invoice-quantity'), '1')
  await page.fill(t('invoice-price'), '100')
  await page.click(t('invoice-add-line'))
  await page.click(t('invoice-save'))
  const invoice = await until(async () => {
    const value = await page.evaluate("fetch('/erp/billing/workspace?tenant=acme-retail').then(r=>r.json())")
    return value.invoices?.find((i) => i.code === code)
  }, 'email invoice committed')
  await until(() => page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice.id}`)}')`), 'email invoice row')
  await page.click(t(`ops-open-${invoice.id}`))
  await page.fill(t('invoice-reason'), 'Issued for email proof')
  await page.click(t('invoice-issue'))
  await until(() => page.evaluate(`document.querySelector('${t('invoice-email-panel')}')?.hidden===false`), 'email panel after issue')

  const list = () => page.evaluate(`fetch('/erp/billing/invoices/${invoice.id}/emails?tenant=acme-retail').then(r=>r.json())`)

  await page.fill(t('email-recipient'), `customer-${suffix}@archava.test`)
  await page.fill(t('email-subject'), `Invoice ${suffix}`)
  await page.click(t('email-send-button'))
  await until(() => page.text(t('email-notice')).then((s) => /Email (status|outcome confirmed): accepted/.test(s)), 'email accepted')
  const accepted = await until(async () => {
    const rows = await list()
    return rows.length === 1 && rows[0].state === 'accepted' ? rows : null
  }, 'accepted outbox row')
  assert.equal(provider.state.ids.length, 1)
  assert.equal(provider.state.keys[0], accepted[0].requestId)
  assert.ok(provider.state.bodies[0].to.includes(`customer-${suffix}@archava.test`))
  pass('EMAIL-outbox-accepted', { invoice: invoice.id, requestId: accepted[0].requestId, providerMessageId: accepted[0].providerMessageId })

  provider.state.failNext = true
  await page.fill(t('email-subject'), `Retry ${suffix}`)
  await page.click(t('email-send-button'))
  await until(() => page.evaluate(`!!document.querySelector('${t('email-pending-recovery')}')`), 'email recovery marker after provider failure')
  const unknown = await until(async () => {
    const rows = await list()
    return rows.length === 2 ? rows.find((row) => row.state === 'unknown') : null
  }, 'unknown outbox row')
  const retryKey = provider.state.keys[1]
  assert.equal(retryKey, unknown.requestId)

  await page.navigate(`${origin}/app/erp?module=invoices`)
  await until(() => page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice.id}`)}')`), 'email invoice after reload')
  await page.click(t(`ops-open-${invoice.id}`))
  await until(() => page.evaluate(`!!document.querySelector('${t('email-pending-recovery')}')`), 'email recovery marker survives reload')
  await until(() => page.evaluate(`document.querySelector('${t('email-recipient')}')?.value==='customer-${suffix}@archava.test' && document.querySelector('${t('email-subject')}')?.value==='Retry ${suffix}'`), 'email recovery restores recipient and subject')
  await page.click(t('email-retry'))
  await until(() => page.text(t('email-notice')).then((s) => /Email (status|outcome confirmed): accepted/.test(s)), 'email retry accepted')
  await until(() => page.evaluate(`!document.querySelector('${t('email-pending-recovery')}')`), 'email recovery marker cleared')
  const retried = await until(async () => {
    const rows = await list()
    return rows.length === 2 && rows.every((row) => row.state === 'accepted') ? rows : null
  }, 'retried outbox accepted')
  assert.equal(provider.state.ids.length, 2)
  assert.equal(provider.state.keys[2], retryKey, 'retry reuses the original idempotency key')
  const recovered = retried.find((row) => row.requestId === retryKey)
  assert.ok(recovered.providerMessageId)
  pass('EMAIL-unknown-retry-same-key', { requestId: retryKey, providerKeys: provider.state.keys.slice(1) })

  const replay = await mutate(page, `/erp/billing/invoices/${invoice.id}/send-email?tenant=acme-retail`, {
    requestId: recovered.requestId,
    recipient: recovered.recipient,
    subject: recovered.subject,
    message: 'Please review your invoice details below.',
  })
  assert.equal(replay.status, 200, JSON.stringify(replay.body))
  assert.equal(replay.body.state, 'accepted')
  assert.equal(provider.state.ids.length, 2, 'terminal replay never invokes the provider again')
  pass('EMAIL-replay-no-provider-call', { requestId: recovered.requestId, providerSends: provider.state.sends })

  const foreign = await page.evaluate(`fetch('/erp/billing/invoices/${invoice.id}/emails?tenant=globex-corp').then(r=>r.status)`)
  assert.ok([403, 404].includes(foreign), `Foreign tenant email list status ${foreign}`)
  pass('EMAIL-tenant-refusal', { status: foreign })
  return { invoice: invoice.id, requestId: accepted[0].requestId, retryRequestId: retryKey, providerSends: provider.state.sends, providerSuccesses: provider.state.ids.length }
}
