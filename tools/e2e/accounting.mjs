/** Native company books, exact decimal entry, real reports and immutable corrections. */
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { spawnSync } from 'node:child_process'

export async function verifyAccounting({ page, origin, facts, until, pass, suffix }) {
  assert.equal(new URL(origin).hostname, '127.0.0.1')
  assert.match(suffix, /^[0-9a-f]{8}$/)
  const tenant = 'acme-retail', t = id => `[data-testid="${id}"]`
  const sql = input => {
    const r = spawnSync('psql', ['-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1', '-p', String(facts.port), '-U', 'postgres', '-d', 'archava_test'], {
      env: { ...process.env, PGPASSWORD: 'archava' }, input: `SET archava.tenant_id='${tenant}';${input}`, encoding: 'utf8', timeout: 30000,
    })
    assert.equal(r.status, 0, r.stderr)
    return r.stdout.trim().split('\n').at(-1)
  }
  const post = (path, body) => page.evaluate(`fetch(${JSON.stringify(path)},{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''},body:${JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json()}))`)
  const org = async (type, data) => {
    const r = await post('/erp/organizations/commands', { tenant, mutation: { requestId: randomUUID(), type, data } })
    assert.equal(r.status, 200, JSON.stringify(r.body)); return r.body.receipt
  }
  const company = async code => {
    const o = await org('create_organization', { name: code, legalName: `${code} Official` })
    return (await org('company.create_from_organization', { code, organizationId: o.organizationId, expectedSourceRevision: 1, reason: 'Explicit synthetic financial acceptance company' })).companyId
  }
  const select = async (id, value) => {
    await until(() => page.evaluate(`!!document.querySelector(${JSON.stringify(t(id))})`), id)
    await page.evaluate(`document.querySelector(${JSON.stringify(t(id))}).focus()`)
    await until(() => page.evaluate(`!![...document.querySelector(${JSON.stringify(t(id))}).options].find(o=>o.value===${JSON.stringify(value)})`), `${id} authorized option`)
    await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  }
  const fill = async (id, value) => {
    const input = `operator-accounting-${id}`
    const type = await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(input))});if(!n)throw Error('Missing financial field '+${JSON.stringify(input)});return n.tagName==='SELECT'?'select':n.type;})()`)
    if (type === 'select') await select(input, String(value))
    else if (type === 'checkbox') await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(input))});n.checked=${!!value};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
    else await page.fill(t(input), String(value))
  }
  const review = async (action, fields) => {
    await select('operator-action', action)
    await until(() => page.evaluate(`document.querySelector('.operator-actions form fieldset')?.disabled===false`), `${action} current financial grant`)
    for (const [key, value] of Object.entries(fields)) await fill(key, value)
    await page.click(t('module-accounting-create'))
    await until(() => page.evaluate(`!!document.querySelector('${t('operator-review')}')`), `${action} explicit review`)
  }
  const confirm = async () => {
    await page.click(t('operator-confirm'))
    await until(async () => {
      const text = await page.text(t('ops-feedback'))
      if (await page.evaluate(`!!document.querySelector('${t('ops-refused')}')||!!document.querySelector('${t('ops-uncertain')}')`)) throw Error(text)
      return text.includes('Saved.')
    }, 'financial action matching receipt')
  }
  const action = async (kind, fields) => { await review(kind, fields); await confirm() }
  const a = await company(`FIN-${suffix}`.toUpperCase())
  await page.navigate(`${origin}/app/erp?module=accounting`)
  await select('trade-company-select', a)
  await until(() => page.evaluate(`document.querySelector('${t('module-accounting-create')}')?.closest('fieldset').disabled===false`), 'financial setup available')
  const bookCode = `BOOK-${suffix}`.toUpperCase()
  await action('book.create', { code: bookCode, name: `Financial acceptance ${suffix}`, currency: 'USD', sourceAuthority: 'native', independentPosting: false, reason: 'Explicit single-operator synthetic policy; no opening import' })
  const book = await until(() => sql(`SELECT id FROM archava_accounting.books WHERE code='${bookCode}'`), 'one explicit company book')
  assert.match(book, /^[0-9a-f-]{36}$/)
  assert.equal(sql(`SELECT minor_units FROM archava_accounting.books WHERE id='${book}'`), '2')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.journals WHERE book_id='${book}'`), '0')
  pass('ACCOUNTING-native-explicit-book', 'Native setup binds a managed company, USD units and an explicit single-operator policy; setup posts no historical balances')

  const account = async (code, name, cls, role) => {
    await action('account.create', { bookId: book, code, name, class: cls, cashRole: role, reason: 'Reviewed account and cash flow classification' })
    return until(() => sql(`SELECT id FROM archava_accounting.accounts WHERE book_id='${book}' AND code='${code}'`), `native ${code} account`)
  }
  const bank = await account('BANK', 'Bank', 'asset', 'cash')
  const equity = await account('EQUITY', 'Opening equity', 'equity', 'financing')
  await action('period.open', { bookId: book, code: '2026-11', startDate: '2026-11-01', endDate: '2026-11-30', reason: 'Open reviewed reporting period' })
  const period = sql(`SELECT id FROM archava_accounting.periods WHERE book_id='${book}' AND code='2026-11'`)
  const journalFields = code => ({ bookId: book, code, date: '2026-11-07', sourceRef: `Explicit UI source ${code}`, memo: 'Reviewed opening cash', 'lines-0-accountId': bank, 'lines-0-debitMinor': '123.45', 'lines-0-creditMinor': '0.00', 'lines-1-accountId': equity, 'lines-1-debitMinor': '0.00', 'lines-1-creditMinor': '123.45', reason: 'Review exact source amount without calculating minor units' })
  await review('journal.draft', journalFields('OPENING'))
  assert.match(await page.text(t('operator-review')), /USD 123\.45/)
  await confirm()
  const journal = await until(() => sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='OPENING'`), 'native reviewed opening draft')
  assert.equal(sql(`SELECT sum(debit_minor) FROM archava_accounting.journal_lines WHERE journal_id='${journal}'`), '12345')
  assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`), 'draft')
  await action('journal.post', { id: journal, reason: 'Post reviewed journal under the explicit one-person book policy' })
  await until(() => sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`) === 'posted', 'native posting committed')
  pass('ACCOUNTING-native-decimal-review-and-post', 'USD 123.45 enters as a normal amount, previews identically, stores exactly 12345 units and posts once with reviewed account versions')

  const reports = async expected => {
    await page.fill(t('accounting-report-from'), '2026-11-01'); await page.fill(t('accounting-report-to'), '2026-11-30')
    await page.click(t('accounting-report-run'))
    await until(() => page.text(t('accounting-report-result')).then(s => s.includes(expected)), 'painted exact financial reports')
  }
  await reports('USD 123.45')
  assert.equal(sql(`SELECT sum(l.debit_minor-l.credit_minor) FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE j.book_id='${book}' AND j.state='posted'`), '0')
  await page.evaluate(`document.querySelector('${t('accounting-report-result')}').scrollIntoView({block:'center',behavior:'instant'})`)
  await page.screenshot('accounting-financial-reports.png')
  pass('ACCOUNTING-native-exact-reports', 'Native reports show the posted trial balance, assets, equity and financing cash movement against an independent balanced SQL oracle')

  await action('period.close', { id: period, reason: 'Reviewed report period close' })
  await until(() => sql(`SELECT state FROM archava_accounting.periods WHERE id='${period}'`) === 'closed', 'native period close')
  await review('journal.draft', journalFields('CLOSED-REFUSAL')); await page.click(t('operator-confirm'))
  await until(() => page.evaluate(`!!document.querySelector('${t('ops-refused')}')`), 'closed period visible business refusal')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.journals WHERE book_id='${book}'`), '1')
  await action('period.reopen', { id: period, reason: 'Authorized controlled reopening for an explicit correction' })
  await until(() => sql(`SELECT revision FROM archava_accounting.periods WHERE id='${period}'`) === '3', 'controlled reopen revision')
  pass('ACCOUNTING-native-closed-period-refusal', 'A closed period refuses a real native journal without claiming a write; explicit authorized reopening advances the existing period history')

  await action('journal.reverse', { id: journal, code: 'REV-OPENING', date: '2026-11-08', sourceRef: 'Explicit reversal source', reason: 'Correct reviewed opening while retaining its original posting' })
  const reverse = await until(() => sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='REV-OPENING'`), 'new reversal draft')
  assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`), 'posted')
  assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${reverse}'`), 'draft')
  await action('journal.post', { id: reverse, reason: 'Post separately reviewed inverse journal' })
  await until(() => sql(`SELECT state FROM archava_accounting.journals WHERE id='${reverse}'`) === 'posted', 'reversal posting')
  await reports('USD 0.00')
  assert.equal(sql(`SELECT sum(l.debit_minor-l.credit_minor) FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id JOIN archava_accounting.accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id WHERE j.book_id='${book}' AND j.state='posted' AND a.code='BANK'`), '0')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.audit WHERE book_id='${book}' AND resource_kind='journal'`), '4')
  pass('ACCOUNTING-native-reversal-receipt', 'A reversal returns its new draft receipt bound to the original journal; a second explicit posting restores zero cash while preserving both posted originals and four journal audit events')

  await action('account.set_active', { id: bank, active: false, reason: 'Stop future use while retaining posted account history' })
  await until(() => sql(`SELECT active FROM archava_accounting.accounts WHERE id='${bank}'`) === 'f', 'native account activity change')
  await reports('USD 0.00')
  const viewport = await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
  try {
    await page.send('Emulation.setDeviceMetricsOverride', { width: 390, height: 844, deviceScaleFactor: 1, mobile: true })
    await page.evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))')
    await page.evaluate(`document.querySelector('${t('accounting-reports')}').scrollIntoView({block:'start',behavior:'instant'})`)
    assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'), true)
    await page.screenshot('accounting-mobile-reports.png')
  } finally {
    await page.send('Emulation.setDeviceMetricsOverride', viewport)
  }
  pass('ACCOUNTING-native-history-and-mobile', 'Deactivating an account keeps its posted reports readable; the financial screen remains within a 390px viewport')

  await org('company.set_active',{id:a,expectedRevision:1,active:false,reason:'Stop new operating business while completing financial close'})
  await page.click(t('trade-company-refresh'))
  await until(()=>page.text(t('trade-company-selected')).then(s=>s.includes('inactive')),'current inactive company context')
  await until(()=>page.text(t('accounting-reports')).then(s=>s.includes('inactive for new business')),'financial completion policy visible')
  await action('period.close',{id:period,reason:'Complete the existing book after operating deactivation'})
  await until(()=>sql(`SELECT state FROM archava_accounting.periods WHERE id='${period}'`)==='closed','inactive company financial close')
  await reports('USD 0.00')
  pass('ACCOUNTING-native-inactive-company-close','Operating deactivation preserves authorized financial closing and report access; it does not abandon the existing book')

  const b = await company(`FIN-B-${suffix}`.toUpperCase())
  await page.navigate(`${origin}/app/erp?module=accounting`); await select('trade-company-select', b)
  await until(() => page.evaluate(`!document.querySelector('${t('accounting-report-result')}')&&document.querySelector('${t('operator-collection')}')`), 'company switch removes old report')
  const view = await page.evaluate(`fetch('/erp/accounting/workspace?tenant=acme-retail&companyId=${b}').then(r=>r.json())`)
  assert.deepEqual(view.books, []); assert.deepEqual(view.journals, [])
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.books WHERE company_id='${b}'`), '0')
  pass('ACCOUNTING-native-company-isolation', 'Another managed company starts without a book or balances and never inherits the previous company report or setup')
  return { company: a, otherCompany: b, book, bank, equity, period, originalJournal: journal, reversalJournal: reverse, policy: 'explicit_single_operator_synthetic_book', scope: 'Native GL controls; operational AP/AR source bridges, bank reconciliation, localization and external accounting remain separate whole-PRD work' }
}
