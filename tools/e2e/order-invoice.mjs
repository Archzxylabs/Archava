/** Frozen-order billing journey on verify.mjs's owned browser and database. */
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { spawnSync } from 'node:child_process'

export async function verifyOrderInvoice({ page, origin, facts, until, pass, suffix }) {
  assert.equal(new URL(origin).hostname, '127.0.0.1')
  assert.ok(Number.isInteger(facts.port) && facts.port > 1023 && facts.port < 65536)
  const t = id => `[data-testid="${id}"]`
  const call = (path, body) => page.evaluate(`fetch(${JSON.stringify(path)},{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''},body:${JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json()}))`)
  const command = async input => {
    const response = await call('/erp/commerce/commands', { tenant: 'acme-retail', requestId: randomUUID(), command: input })
    assert.equal(response.status, 200, JSON.stringify(response.body))
    return response.body.receipt
  }
  const product = await command({ operation: 'product.create', sku: `SOURCE-${suffix}`.toUpperCase(), title: 'Frozen invoice source item', unit: 'unit', trackStock: false, price: 12000 })
  const order = await command({ operation: 'order.create', customerName: `Source buyer ${suffix}`, lines: [{ productId: product.id, quantity: 3 }] })
  await command({ operation: 'order.reserve', id: order.id, expectedRevision: 1, holdSeconds: 300 })
  await command({ operation: 'order.fulfill', id: order.id, expectedRevision: 2 })
  await command({ operation: 'product.set', id: product.id, expectedRevision: 1, price: 25000, active: true })

  await page.navigate(`${origin}/app/erp?module=invoices`)
  await until(() => page.evaluate(`!!document.querySelector('${t('invoice-order-search')}')`), 'source order search')
  await page.fill(t('invoice-order-search'), `Source buyer ${suffix}`)
  await page.click(t('invoice-order-refresh'))
  await until(() => page.evaluate(`!!document.querySelector('${t('invoice-source-order')}')?.querySelector('option[value="${order.id}"]')`), 'fulfilled source order picker')
  await until(() => page.evaluate(`Array.from(document.querySelector('${t('invoice-source-order')}')?.options??[]).filter(option=>option.value).every(option=>option.value===${JSON.stringify(order.id)})`), 'source customer filter applied')
  await page.evaluate(`(()=>{const n=document.querySelector('${t('invoice-source-order')}');n.value=${JSON.stringify(order.id)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  await page.fill(t('invoice-order-code'), `SOURCE-INV-${suffix}`)
  await until(() => page.text(t('invoice-from-order')).then(text => text.includes('Frozen invoice source item') && text.includes('36.000')), 'frozen line sheet review')
  await page.evaluate(`(()=>{const original=window.fetch;window.fetch=async function(input,init){const request=input instanceof Request?input:new Request(input,init);if(request.method==='POST'&&new URL(request.url).pathname==='/erp/billing/commands'){const body=await request.clone().json();if(body.mutation?.type==='create_invoice_from_order'){window.__sourceInvoiceRequest=body;window.fetch=original;}}return original.call(window,input,init);};})()`)
  await page.click(t('invoice-from-order-save'))
  const invoices = () => page.evaluate("fetch('/erp/billing/workspace?tenant=acme-retail').then(r=>r.json()).then(v=>v.invoices)")
  await until(async () => (await invoices()).some(invoice => invoice.sourceOrderId === order.id), 'source invoice committed')
  const invoice = (await invoices()).find(invoice => invoice.sourceOrderId === order.id)
  assert.equal(invoice.gross, 36000)
  assert.equal(invoice.customer, `Source buyer ${suffix}`)
  assert.equal(invoice.lines[0].unitAmount, 12000)
  assert.equal(invoice.sourceOrderRevision, 3)
  await until(() => page.evaluate(`document.querySelector('${t('invoice-source-order')}')?.value === ''`), 'billed source selection cleared')
  await until(() => page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice.id}`)}')`), 'source invoice row')
  await page.click(t(`ops-open-${invoice.id}`))
  await until(() => page.text(t('invoice-source')).then(text => text.includes('sales order')), 'invoice provenance visible')
  await page.screenshot('operations-order-invoice.png')
  pass('BILLING-order-invoice-UI', 'A fulfilled sales order creates a draft through the native form with its frozen customer, three original-price units and visible provenance')

  const original = await page.evaluate('window.__sourceInvoiceRequest')
  assert.equal(original.mutation.orderId, order.id)
  const replay = await call('/erp/billing/commands', original)
  assert.equal(replay.status, 200)
  assert.equal(replay.body.invoiceId, invoice.id)
  assert.equal(replay.body.idempotencyReplay, true)
  const duplicate = await call('/erp/billing/commands', { ...original, mutation: { ...original.mutation, commandId: randomUUID(), invoiceCode: `DUP-INV-${suffix}` } })
  assert.equal(duplicate.status, 409)
  assert.match(order.id, /^[0-9a-f-]{36}$/)
  const proof = spawnSync('psql', ['-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1', '-p', String(facts.port), '-U', 'postgres', '-d', 'archava_test'], {
    env: { ...process.env, PGPASSWORD: 'archava' }, encoding: 'utf8', timeout: 30000,
    input: `SET archava.tenant_id='acme-retail';SELECT json_build_object('invoices',count(DISTINCT i.id),'gross',min(i.gross_minor),'revision',min(i.source_order_revision),'quantity',min(l.quantity),'price',min(l.unit_minor),'lineMatches',bool_and(l.source_order_line_id=s.id AND l.quantity=s.quantity AND l.unit_minor=s.price)) FROM archava_billing.invoices i JOIN archava_billing.invoice_lines l ON l.tenant_id=i.tenant_id AND l.invoice_id=i.id JOIN archava_commerce.order_lines s ON s.tenant_id=i.tenant_id AND s.id=l.source_order_line_id WHERE i.tenant_id='acme-retail' AND i.source_order_id='${order.id}'::uuid;`,
  })
  assert.equal(proof.status, 0, proof.stderr)
  const stored = JSON.parse(proof.stdout.trim().split('\n').at(-1))
  assert.deepEqual(stored, { invoices: 1, gross: 36000, revision: 3, quantity: 3, price: 12000, lineMatches: true })
  pass('BILLING-order-invoice-SQL-replay', 'Independent SQL verifies one original-price invoice and exact source lines after replay and a competing new conversion')
  await page.navigate(`${origin}/app/erp?module=invoices`)
  await until(() => page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice.id}`)}')`), 'source invoice after reload')
  await page.click(t(`ops-open-${invoice.id}`))
  await until(() => page.text(t('invoice-source')).then(text => text.includes('sales order')), 'provenance after reload')
  pass('BILLING-order-invoice-reload', 'The saved source reference survives a fresh document load')
  return { sourceOrderId: order.id, invoiceId: invoice.id, sql: stored, scope: 'Native form and registered deterministic operation; no new live-model planning acceptance' }
}
