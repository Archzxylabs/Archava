/** Real native forms: review, matching receipt, SQL persistence and missing Tier 1 actions. */
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { readFileSync } from 'node:fs'

export async function verifyNativeModules({ page, origin, until, call, sql, observe, contact, operations }) {
  const t = id => `[data-testid="${id}"]`, suffix = randomUUID().slice(0, 8)
  const select = async (id, value) => {
    const selector = t(id)
    await page.evaluate(`document.querySelector(${JSON.stringify(selector)}).focus()`)
    if (id.endsWith('-contactId') && value) {
      const detail = await call('GET', `/crm/records?tenant=acme-retail&contact=${encodeURIComponent(value)}`)
      assert.equal(detail.status, 200, JSON.stringify(detail.body))
      const name = detail.body.contact.fullName
      assert.ok(name, 'Reference search requires the authorized contact name')
      await page.evaluate(`(()=>{const group=document.querySelector(${JSON.stringify(selector)}).closest('.operator-reference');const input=group.querySelector('input');input.value=${JSON.stringify(name)};input.dispatchEvent(new Event('input',{bubbles:true}));group.querySelector('.ops-search button').click();})()`)
    }
    await until(() => page.evaluate(`!![...document.querySelector(${JSON.stringify(selector)}).options].find(o=>o.value===${JSON.stringify(value)})`), `${id} available record`)
    await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(selector)});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  }
  async function fill(domain, values, path = []) {
    for (const [key, value] of Object.entries(values)) {
      const next = [...path, key], id = `operator-${domain}-${next.join('-')}`
      if (Array.isArray(value)) {
        for (let index = 0; index < value.length; index++) {
          await until(async () => {
            const count = await page.evaluate(`document.querySelector(${JSON.stringify(t(id))})?.querySelectorAll(':scope > .operator-array-item').length||0`)
            if (count > index) return true
            await page.evaluate(`document.querySelector(${JSON.stringify(t(id))}).querySelector(':scope > button').click()`)
            return false
          }, `${id} row ${index}`)
          await fill(domain, value[index], [...next, String(index)])
        }
      } else if (value && typeof value === 'object') await fill(domain, value, next)
      else {
        const type = await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});if(!n)throw Error('Missing native field: '+${JSON.stringify(id)});return n.tagName==='SELECT'?'select':n.type;})()`)
        if (type === 'select') await select(id, value === null ? '' : String(value))
        else if (type === 'checkbox') await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.checked=${!!value};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
        else {
          let text = value === null ? '' : String(value)
          if (type === 'datetime-local') text = await page.evaluate(`(()=>{const d=new Date(${JSON.stringify(text)});return new Date(d.getTime()-d.getTimezoneOffset()*60000).toISOString().slice(0,16)})()`)
          await page.fill(t(id), text)
        }
      }
    }
  }
  async function action(domain, action, values, { navigate = true } = {}) {
    if (navigate) await page.navigate(`${origin}/app/erp?module=${domain}`)
    await until(() => page.evaluate(`!!document.querySelector('${t('operator-action')}')`), `${domain} action form`)
    await page.evaluate(`(()=>{const d=document.querySelector('.operator-actions').closest('details');if(d)d.open=true;})()`)
    await select('operator-action', action)
    await until(() => page.evaluate(`!document.querySelector(${JSON.stringify(t(`module-${domain}-create`))}).closest('fieldset').disabled`), `${domain} current grant`)
    await fill(domain, values)
    await page.click(t(`module-${domain}-create`))
    await until(() => page.evaluate(`!!document.querySelector('${t('operator-review')}')`), `${domain} reviewed command`)
    await page.click(t('operator-confirm'))
    await until(async () => {
      const state = await page.text(t('ops-feedback'))
      if (state.includes('Saved.')) return true
      if (await page.evaluate(`!!document.querySelector('${t('ops-refused')}')||!!document.querySelector('${t('ops-uncertain')}')`)) throw Error(`${domain}.${action}: ${state}`)
      return false
    }, `${domain}.${action} matching receipt`)
  }
  const time = Date.now() + 86400000
  const cases = [
    ['organizations', 'organizations', 'organizations', 'create_organization', { name: `Native organization ${suffix}` }],
    ['proposals', 'proposals', 'proposals', 'create_proposal', { title: `Native proposal ${suffix}`, summary: 'Synthetic reviewed scope', scopeItems: [{ title: 'Delivery', description: 'Synthetic scope item', estimatedDays: 1, estimatedValue: 1000, deliverableType: 'service' }] }],
    ['projects', 'projects', 'projects', 'project.create', { title: `Native project ${suffix}`, scopeSummary: 'Synthetic delivery scope' }],
    ['foodservice', 'foodservice', 'tables', 'table.create', { code: `T_${suffix}`.toUpperCase(), title: `Native table ${suffix}`, seats: 4 }],
    ['scheduling', 'scheduling', 'resources', 'resource.create', { title: `Native resource ${suffix}`, kind: 'staff', timezone: 'Asia/Jakarta' }],
    ['accommodation', 'accommodation', 'properties', 'property.create', { title: `Native property ${suffix}`, timezone: 'Asia/Jakarta', checkInTime: '14:00', checkOutTime: '11:00' }],
    ['assets', 'assets', 'assets', 'create_asset', { code: `ASSET-${suffix}`.toUpperCase(), name: `Native asset ${suffix}`, category: 'equipment', ownership: 'business_owned', condition: 'good' }],
    ['subscriptions', 'subscriptions', 'subscriptions', 'create_subscription', { contactId: contact, planCode: `PLAN-${suffix}`, planName: `Native membership ${suffix}`, billingInterval: 'monthly', unitPrice: 10000, currency: 'IDR', quantity: 1 }],
    ['attendance', 'attendance', 'sessions', 'create_session', { title: `Native session ${suffix}`, sessionCode: `SESSION-${suffix}`, startTime: new Date(time).toISOString(), endTime: new Date(time + 3600000).toISOString(), capacity: 10 }],
    ['laundry', 'laundry', 'orders', 'create_order', { orderCode: `L-${suffix}`, kind: 'item', currency: 'IDR', lines: [{ itemRef: 'shirt', quantity: 2, unit: 'piece', unitPriceMinor: 1000 }] }],
    ['field', 'field', 'jobs', 'create_job', { job: { jobId: `native-job-${suffix}`, customerRef: `customer-${suffix}`, siteRef: 'site-main', summary: 'Synthetic native service job', priority: 'normal', appointmentRef: `appointment-${suffix}` } }],
    ['fulfillment', 'fulfillment', 'jobs', 'create_job', { jobCode: `F-${suffix}`, kind: 'dispatch', title: `Native dispatch ${suffix}` }],
    ['education', 'edu', 'enrollments', 'admit_applicant', { enrollment: { enrollmentId: `native-enrollment-${suffix}`, studentRef: `student-${suffix}`, classRef: 'class-main', termRef: 'term-current' } }],
  ]
  const created = {}
  for (const [domain, namespace, table, operation, values] of cases) {
    const before = Number(sql(`SELECT count(*) FROM archava_${namespace}.${table} WHERE tenant_id='acme-retail'`))
    await action(domain, operation, values)
    const after = Number(sql(`SELECT count(*) FROM archava_${namespace}.${table} WHERE tenant_id='acme-retail'`))
    assert.equal(after, before + 1, `${domain} must commit exactly one real record`)
    const workspace = await call('GET', `/erp/${domain}/workspace?tenant=acme-retail`)
    assert.equal(workspace.status, 200)
    observe(`MODULE-${domain}-native-write`, 'Native form -> explicit review -> exact receipt -> one tenant-scoped durable record', { before, after, operation, readStatus: workspace.status }, after === before + 1)
    created[domain] = workspace.body
  }
  const contracts = JSON.parse(readFileSync(new URL('../../config/operator-actions.v1.json', import.meta.url), 'utf8'))
  let renderedActions = 0
  const sharedModules=contracts.modules.filter(m => !['crm', 'commerce', 'billing'].includes(m.id))
  for (const module of sharedModules) {
    await page.navigate(`${origin}/app/erp?module=${module.id}`)
    await until(() => page.evaluate(`!!document.querySelector('${t('operator-action')}')`), `${module.id} action catalogue`)
    await page.evaluate(`(()=>{const d=document.querySelector('.operator-actions').closest('details');if(d)d.open=true;})()`)
    for (const action of module.actions.filter(a => !a.providerOnly)) {
      await select('operator-action', action.id)
      await until(() => page.evaluate(`document.querySelector('.operator-actions form fieldset > legend')?.textContent===${JSON.stringify(action.label)}&&!document.querySelector('.operator-actions form fieldset').disabled`), `${module.id}.${action.id} rendered under current grants`)
      assert.equal(await page.evaluate(`!!document.querySelector('${t(`module-${module.id}-create`)}')`), true)
      renderedActions++
    }
  }
  observe('NATIVE-form-catalogue-rendering', 'Every registered shared-form action renders its native inputs and review control under current grants', { modules: sharedModules.length, renderedActions, scope: 'Rendering only; writes are covered by the explicit journeys and Rust integration suites' }, true)
  await page.screenshot('installed-native-modules.png')
  let purchasing = (await call('GET', '/erp/purchasing/workspace?tenant=acme-retail')).body
  const offer = purchasing.offers.find(o => o.productId === operations.product), supplier = purchasing.suppliers.find(s => s.id === offer.supplierId)
  assert.ok(offer && supplier)
  for (const [operation, values] of [
    ['supplier.set-active', { id: supplier.id, active: false, reason: 'Native supplier control' }],
    ['supplier.set-active', { id: supplier.id, active: true, reason: 'Native supplier reactivation' }],
    ['offer.set-cost', { id: offer.id, unitCost: 9500, reason: 'Native supplier cost review' }],
    ['offer.set-active', { id: offer.id, active: false, reason: 'Native offer control' }],
    ['offer.set-active', { id: offer.id, active: true, reason: 'Native offer reactivation' }],
    ['order.create', { supplierId: supplier.id, lines: [{ offerId: offer.id, quantity: 1 }] }],
  ]) await action('purchasing', operation, values)
  purchasing = (await call('GET', '/erp/purchasing/workspace?tenant=acme-retail')).body
  const draft = purchasing.orders.find(o => o.supplierName === supplier.name && o.state === 'draft')
  assert.ok(draft)
  await action('purchasing', 'order.replace-lines', { id: draft.id, reason: 'Native line replacement', lines: [{ offerId: offer.id, quantity: 3 }] })
  purchasing = (await call('GET', '/erp/purchasing/workspace?tenant=acme-retail')).body
  assert.equal(purchasing.orders.find(o => o.id === draft.id).lines[0].quantity, 3)
  assert.equal(purchasing.offers.find(o => o.id === offer.id).unitCost, 9500)
  observe('PURCHASING-native-maintenance', 'Supplier activation, offer cost/activation and draft line replacement are usable through native forms', { supplierId: supplier.id, offerId: offer.id, orderId: draft.id, quantity: 3, unitCost: 9500 }, true)
  for (const [operation, values] of [
    ['case.assign', { id: operations.support, assigneeId: 'alice', reason: 'Native assignment' }],
    ['case.set-priority', { id: operations.support, priority: 'high', reason: 'Native priority' }],
    ['case.set-target', { id: operations.support, dueAt: new Date(time).toISOString(), reason: 'Native target' }],
    ['case.cancel', { id: operations.support, reason: 'Native cancellation' }],
  ]) await action('support', operation, values)
  const support = (await call('GET', '/erp/support/cases?tenant=acme-retail')).body.items.find(c => c.id === operations.support)
  assert.equal(support.state, 'cancelled'); assert.equal(support.assigneeId, 'alice'); assert.equal(support.priority, 'high'); assert.ok(support.dueAt)
  observe('SUPPORT-native-maintenance', 'Assignment, priority, target and cancellation persist through native forms', support, true)
  return { domains: cases.length, renderedActions, missingTier1Controls: 8, created }
}
