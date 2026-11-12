/** Native backup/restore proof, restricted to this gate's disposable cluster. */
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { randomBytes, randomUUID } from 'node:crypto'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'

export async function verifyOps({ packageRoot, facts, scratch, productionEnvironment, startServer, terminate, until, freePort, pass, dueTerms }) {
  assert.ok(Number.isInteger(facts.port) && facts.port > 1023 && facts.port < 65536)
  const distribution = resolve(packageRoot), binary = join(distribution, 'bin/archava-ops')
  const backup = join(scratch, 'native-backup'), target = `archava_restore_${randomUUID().slice(0,8)}`
  assert.match(target, /^archava_restore_[0-9a-f]{8}$/)
  const env = { ...process.env, PGPASSWORD: 'archava', ARCHAVA_OPS_ROOT: distribution, ARCHAVA_BACKUP_KEY: randomBytes(32).toString('hex') }
  const sql = (database, statement) => {
    const result = spawnSync('psql', ['-X', '-qAt', '-v', 'ON_ERROR_STOP=1', '-h', '127.0.0.1', '-p', String(facts.port), '-U', 'postgres', '-d', database], { env, input: statement, encoding: 'utf8', timeout: 30000 })
    assert.equal(result.status, 0, result.stderr)
    return result.stdout.trim()
  }
  const bankTables=['bank_accounts','bank_statements','bank_rows','bank_allocations','bank_unmatches','bank_policies','bank_reconciliations']
  const financialTables=['memberships','books','accounts','periods','journals','journal_lines','commands','audit','invoice_policies','invoice_sources','journal_sources','purchase_policies','purchase_origins','purchase_sources','purchase_journal_sources','stock_layer_origins','stock_policies','stock_sources','stock_journal_sources','payable_settlements','clearing_settlements','document_terms',...bankTables]
  const financialProof=`json_build_object(${financialTables.map(table=>`'${table}',(SELECT json_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(r)::text,'' ORDER BY to_jsonb(r)::text),''))) FROM archava_accounting.${table} r)`).join(',')})`
  const businessProof = database => JSON.parse(sql(database, `SELECT json_build_object(
    'contacts',(SELECT json_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(c)::text,'' ORDER BY tenant_id,id),''))) FROM archava_crm.contacts c),
    'products',(SELECT json_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(p)::text,'' ORDER BY tenant_id,id),''))) FROM archava_commerce.products p),
    'commands',(SELECT json_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(c)::text,'' ORDER BY tenant_id,request_id),''))) FROM archava_crm.commands c),
    'email',(SELECT json_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(e)::text,'' ORDER BY tenant_id,id),''))) FROM archava_billing.email_outbox e),
    'accounting',${financialProof});`))
  const ops = args => {
    const result = spawnSync(binary, args, { cwd: distribution, env, encoding: 'utf8', timeout: 55000, maxBuffer: 4*1024*1024 })
    assert.equal(result.status, 0, `${result.stderr}\n${result.stdout}`)
    const body = JSON.parse(result.stdout); assert.equal(body.ok, true); return body
  }
  const before = businessProof('archava_test'), captureStarted = performance.now()
  assert.equal(financialTables.length,29)
  assert.ok(before.accounting.payable_settlements.rows>0,'Recovery must preserve actual supplier payments and returns')
  assert.ok(before.accounting.clearing_settlements.rows>0,'Recovery must preserve actual gateway bank payouts and refunds')
  assert.ok(before.accounting.document_terms.rows>=4,'Recovery must preserve nonempty reviewed invoice and supplier due terms')
  for(const table of bankTables)assert.ok(before.accounting[table].rows>0,`Recovery must exercise actual ${table} history, not an empty-table digest`)
  const captured = ops(['backup', '--port', String(facts.port), '--db', 'archava_test', '--user', 'postgres', '--out', backup])
  const captureMs = performance.now() - captureStarted, manifest = join(backup, 'archava-backup.manifest.json')
  assert.equal(captured.backup.credentialValuesPrinted, false)
  const archive = join(backup, 'archava-backup.dump')
  const magic = Buffer.from('ARCHAVA-BACKUP-AESGCM-1\n')
  assert.ok(readFileSync(archive).subarray(0,magic.length).equals(magic))
  assert.equal(statSync(archive).mode & 0o777, 0o600)
  assert.ok(readdirSync(backup).every(name => !name.startsWith('.staging-')))
  sql('postgres', `CREATE DATABASE ${target} OWNER archava_owner;`)
  const restoreStarted = performance.now()
  ops(['restore', '--manifest', manifest, '--port', String(facts.port), '--db', target, '--user', 'postgres'])
  const restoreMs = performance.now() - restoreStarted
  const after = businessProof(target); assert.deepEqual(after, before)
  assert.equal(sql(target, 'SELECT held FROM archava_billing.gateway_runtime WHERE singleton=true'), 't')
  assert.ok(readdirSync(backup).every(name => !name.startsWith('.staging-')))
  const port = await freePort(), origin = `http://127.0.0.1:${port}`
  const child = startServer(join(distribution, 'bin/archava-server'), { ...productionEnvironment, DATABASE_URL: `postgres://archava_runtime:archava@127.0.0.1:${facts.port}/${target}`, ARCHAVA_BIND_ADDR: `127.0.0.1:${port}`, ARCHAVA_SITE_ROOT: join(distribution, 'site'), ARCHAVA_WRITER_TAKEOVER: 'false' }, 'restored-production.log')
  try {
    await until(async () => (await fetch(`${origin}/readyz`)).status === 200, 'restored native service readiness')
    const login = await fetch(`${origin}/login`, { method: 'POST', headers: { 'content-type': 'application/json', 'x-archava-client': 'cli' }, body: JSON.stringify({ username: 'alice@archava.test', password: 'correct horse battery staple' }) })
    assert.equal(login.status, 200)
    const session = await login.json()
    const contacts = await fetch(`${origin}/contacts?tenant=acme-retail&limit=50`, { headers: { authorization: `Bearer ${session.token}` } })
    assert.equal(contacts.status, 200)
    assert.ok(after.accounting.books.rows>0,'Recovery includes a real configured book')
    const book=sql(target,"SELECT b.id FROM archava_accounting.books b LEFT JOIN archava_accounting.invoice_sources s ON s.tenant_id=b.tenant_id AND s.book_id=b.id WHERE b.source_authority='native' GROUP BY b.tenant_id,b.id ORDER BY count(s.id) DESC,b.code LIMIT 1")
    assert.match(book,/^[0-9a-f-]{36}$/)
    const financial=await fetch(`${origin}/erp/accounting/reports?tenant=acme-retail&bookId=${book}&from=2026-11-01&to=2026-11-30`,{headers:{authorization:`Bearer ${session.token}`}})
    assert.equal(financial.status,200)
    const report=await financial.json();assert.equal(report.trialBalance.balanced,true);assert.equal(report.balanceSheet.balanced,true)
    if(after.accounting.invoice_sources.rows>0){assert.ok(report.sourceCoverage.capturedCount>0);assert.equal(report.sourceCoverage.unpostedCount,0)}
    let restoredStockReport=null
    if(after.accounting.stock_sources.rows>0){
      const stockBook=sql(target,"SELECT s.book_id FROM archava_accounting.stock_sources s WHERE s.tenant_id='acme-retail' GROUP BY s.book_id ORDER BY count(*) DESC,s.book_id LIMIT 1")
      const expectedCogs=sql(target,`SELECT coalesce(sum(l.debit_minor::numeric-l.credit_minor::numeric),0)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE l.tenant_id='acme-retail' AND l.book_id='${stockBook}' AND j.state='posted' AND l.class='cost_of_goods_sold'`)
      const restored=await fetch(`${origin}/erp/accounting/reports?tenant=acme-retail&bookId=${stockBook}&from=2000-01-01&to=2100-12-31`,{headers:{authorization:`Bearer ${session.token}`}});assert.equal(restored.status,200)
      const stockReport=await restored.json();assert.equal(stockReport.profitAndLoss.costOfGoodsSoldMinor,expectedCogs);assert.equal(stockReport.sourceCoverage.unpostedCount,0);assert.equal(stockReport.trialBalance.balanced,true)
      restoredStockReport={status:200,book:stockBook,costOfGoodsSoldMinor:expectedCogs,unpostedSources:0,balanced:true}
    }
    const bankReview=JSON.parse(sql(target,"SELECT jsonb_build_object('policy',bank_policy_id,'period',period_id,'hash',check_snapshot->>'evidenceSha256') FROM archava_accounting.bank_reconciliations WHERE tenant_id='acme-retail' ORDER BY created_at DESC LIMIT 1"))
    const bankRead=await fetch(`${origin}/erp/accounting/bank-reconciliation?tenant=acme-retail&bankPolicyId=${bankReview.policy}&periodId=${bankReview.period}`,{headers:{authorization:`Bearer ${session.token}`}});assert.equal(bankRead.status,200)
    const restoredBank=await bankRead.json();assert.equal(restoredBank.check.evidenceSha256,bankReview.hash);assert.equal(restoredBank.check.ready,true);assert.equal(restoredBank.reviewed,true);assert.equal(restoredBank.periodState,'closed')
    const restoredBankReview={status:200,periodState:'closed',currentEvidenceReviewed:true,evidenceSha256:bankReview.hash,nonemptyBankTables:bankTables.length}
    let restoredDueTerms=null
    if(dueTerms){
      const response=await fetch(`${origin}/erp/accounting/subledger-report?tenant=acme-retail&bookId=${dueTerms.book}&side=receivable&asOf=${dueTerms.asOf}`,{headers:{authorization:`Bearer ${session.token}`}});assert.equal(response.status,200)
      const asOf=await response.json();assert.equal(asOf.totals.netOutstandingMinor,'1250000');assert.equal(asOf.totals.unknownTermsMinor,'1250000');assert.equal(asOf.postedSourceControlsAgree,true)
      const original=await fetch(`${origin}/erp/accounting/commands/status?tenant=acme-retail&requestId=${dueTerms.knownReceipt.requestId}`,{headers:{authorization:`Bearer ${session.token}`}});assert.equal(original.status,200);const receipt=(await original.json()).receipt;assert.equal(receipt.revision,1);assert.deepEqual(receipt.documentTerms,dueTerms.knownReceipt.documentTerms)
      restoredDueTerms={status:200,exactNetOutstandingMinor:'1250000',latestUnknownTermsPreserved:true,originalKnownTermsReceiptRevision:1,nonemptyTermsRows:after.accounting.document_terms.rows}
    }
    const result = { scope: 'Local encrypted recovery of the owned synthetic database; production RTO/RPO and offsite recovery require separate acceptance', captureMs, restoreMs, restoreThroughLoginMs: performance.now()-restoreStarted, archiveBytes: statSync(archive).size, archiveMode: '0600', encrypted: true, businessRowsAndDigestsMatch: true, proof: after, restoredReadyStatus: 200, restoredLoginStatus: 200, restoredContactsStatus: 200, restoredFinancialReportsStatus: 200, financialTableDigestsMatch: true, financialTablesVerified:financialTables.length, restoredDueTerms, restoredStockReport, restoredBankReview, restoredExternalEffectsHeld: true, plaintextStagingRemaining: false }
    pass('OPS-native-encrypted-restore-and-login', result)
    return result
  } finally { await terminate(child) }
}
