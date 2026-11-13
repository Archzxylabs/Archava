/** Actual native terms, original-source identity and dated AR/AP report. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
import {writeFileSync} from 'node:fs'
import {join} from 'node:path'
import {financialUi} from './financial-ui.mjs'
export async function verifyDueTerms({page,origin,facts,until,pass,suffix,store,output}){
 assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(suffix,/^[0-9a-f]{8}$/)
 const tenant='acme-retail',code=`DUE-${suffix}`.toUpperCase()
 const call=(path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(body?'POST':'GET')},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body?`,body:${JSON.stringify(JSON.stringify(body))}`:''}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const ok=async p=>{const r=await p;assert.equal(r.status,200,JSON.stringify(r));return r.body}
 const org=async(type,data)=>(await ok(call('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}}))).receipt
 const mutate=async(module,command)=>(await ok(call(`/erp/${module}/commands`,{tenant,requestId:randomUUID(),command}))).receipt
 const finance=c=>mutate('accounting',c),trade=c=>mutate('commerce',c),bill=c=>ok(call('/erp/billing/commands',{tenant,mutation:{commandId:randomUUID(),...c}}))
 const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='20s';SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:25000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
 const original=await org('create_organization',{name:'Reviewed due terms business',legalName:'Reviewed Due Terms Business'}),company=(await org('company.create_from_organization',{organizationId:original.organizationId,expectedSourceRevision:1,code,reason:'Separate actual due-date report company'})).companyId
 const book=(await finance({type:'book.create',companyId:company,expectedCompanyRevision:1,code,name:'Original document due-date report',currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic one-operator book'})).bookId
 const accounts={};for(const[c,cls,role]of [['AR','asset','operating'],['REVENUE','revenue','operating'],['CLEARING','asset','none']])accounts[c]=(await finance({type:'account.create',bookId:book,code:c,name:c,class:cls,cashRole:role,reason:'Explicit original invoice roles'})).resourceId
 const ref=c=>({accountId:accounts[c],expectedAccountRevision:1})
 await finance({type:'period.open',bookId:book,code:'DUE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned actual business-date window'})
 await finance({type:'invoice_policy.set',bookId:book,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:[['receivable','AR'],['revenue','REVENUE'],['payment_clearing','CLEARING']].map(([role,c])=>({role,...ref(c)})),reason:'Explicit future original invoice capture'})
 const buyerOrg=await org('create_organization',{name:'Due terms customer',legalName:'Due Terms Customer'}),buyer=await org('party.create_from_organization',{organizationId:buyerOrg.organizationId,expectedSourceRevision:1,customer:true,supplier:false}),invoices=[]
 for(const [n,price]of [[1,10000],[2,2500]]){
  const product=await trade({operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:code+'-'+n,title:'Service with reviewed due terms '+n,unit:'unit',trackStock:false,price}),order=await trade({operation:'order.create_for_company',companyId:company,expectedCompanyRevision:1,partyId:buyer.partyId,expectedPartyRevision:1,lines:[{productId:product.id,quantity:1}]})
  await trade({operation:'order.reserve',id:order.id,expectedRevision:1,holdSeconds:600});await trade({operation:'order.fulfill',id:order.id,expectedRevision:2})
  const invoice=await bill({type:'create_invoice_from_order',invoiceCode:code+'-'+n,orderId:order.id,expectedOrderRevision:3}),issued=await bill({type:'issue_invoice',invoiceId:invoice.invoiceId,expectedRevision:1,reason:'Original reviewed invoice before explicit due agreement'})
  const journal=await finance({type:'invoice_source.draft',sourceId:issued.commandId,code:'INVOICE-'+n,accounts:[ref('AR'),ref('REVENUE')],reason:'Review original source amount and mappings'});await finance({type:'journal.post',id:journal.resourceId,expectedRevision:1,reason:'Separately post the reviewed original invoice'})
  invoices.push({invoice:invoice.invoiceId,source:issued.commandId})
 }
 const today=sql("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text"),due=sql(`SELECT (DATE '${today}'-10)::text`),ui=financialUi({page,origin,until,company}),t=ui.t
 const report=async(side,selectedBook=book)=>ok(call(`/erp/accounting/subledger-report?tenant=${tenant}&bookId=${selectedBook}&side=${side}&asOf=${today}`))
 const before=await report('receivable');assert.equal(before.totals.unknownTermsMinor,'1250000');assert.equal(before.postedSourceControlsAgree,true)
 await ui.open();assert.equal(await page.evaluate(`document.querySelector('${t('ops-tab-accounting')}').classList.contains('active')`),true)
 await ui.review('invoice_terms.set',{sourceId:invoices[0].source,'condition-kind':'due_on','condition-date':due,effectiveFrom:today,reason:'Actual original invoice agreement, ten days before report cutoff'})
 const review=await page.text(t('document-terms-summary'));assert.ok(review.includes(due));assert.ok(review.includes(code+'-1'));assert.ok(review.includes('version'))
 const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
 try{await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true);await page.evaluate(`document.querySelector('${t('document-terms-summary')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('due-terms-mobile-review.png')}finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
 await ui.confirm()
 const dated=await report('receivable');assert.equal(dated.totals.overdue1To30Minor,'1000000');assert.equal(dated.totals.unknownTermsMinor,'250000');assert.equal(dated.totals.netOutstandingMinor,'1250000');assert.equal(dated.postedSourceControlsAgree,true)
 await ui.open();await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current due-date workspace loaded')
 await ui.select('accounting-aging-side','receivable');await page.fill(t('accounting-aging-date'),today);await page.click(t('accounting-aging-run'))
 await until(()=>page.text(t('accounting-aging-documents')).then(s=>s.includes('1–30 days overdue')),'actual domain overdue bucket rendered with its customer label')
 assert.equal((await page.text(t('accounting-aging-documents'))).includes('Unavailable'),false);assert.ok((await page.text(t('accounting-aging-totals'))).includes('10000.00'))
 await page.evaluate(`document.querySelector('${t('accounting-aging-documents')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('due-terms-native-overdue-report.png')
 // The period close check renders what the host reports for the selected period.
 const closePeriod=await page.evaluate(`document.querySelector('${t('accounting-close-period')}').value`);assert.match(closePeriod,/^[0-9a-f-]{36}$/)
 await page.click(t('accounting-close-run'));await until(()=>page.evaluate(`!!document.querySelector('${t('accounting-close-result')}')`),'rendered period close check')
 const closeReport=await ok(call(`/erp/accounting/close-readiness?tenant=${tenant}&periodId=${closePeriod}`))
 const closeShown=await page.evaluate(`({verdict:document.querySelector('${t('accounting-close-verdict')}').textContent,blockers:[...document.querySelectorAll('${t('accounting-close-blockers')} li')].map(n=>n.textContent)})`)
 assert.equal(closeShown.blockers.length,closeReport.blockers.length);closeReport.blockers.forEach((b,i)=>assert.ok(closeShown.blockers[i].startsWith(`${b.count} `)&&!closeShown.blockers[i].includes('other conditions'),JSON.stringify([b,closeShown.blockers[i]])))
 assert.equal(closeShown.verdict.includes('cannot be closed yet'),closeReport.period.state==='open'&&!closeReport.ready,JSON.stringify([closeShown,closeReport]))
 await page.evaluate(`document.querySelector('${t('accounting-close-check')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('accounting-period-close-check.png')
 pass('CLOSE-native-period-close-check',`The Accounting screen renders the host close-readiness report for the selected period: ${closeReport.blockers.length} outstanding condition(s), ready=${closeReport.ready}`)
 await ui.select('operator-action','invoice_terms.set')
 pass('DUE-native-original-source-and-reviewed-date','Accounting is discovered by its actual Operations tab; current document/terms versions bind the native due-date review and exact immutable receipt')
 await ui.select('operator-accounting-sourceId',invoices[0].source);await ui.select('operator-accounting-condition-kind','due_on');await page.fill(t('operator-accounting-condition-date'),due);await page.fill(t('operator-accounting-effectiveFrom'),today)
 await ui.select('operator-accounting-sourceId',invoices[1].source)
 await until(()=>page.evaluate(`document.querySelector('${t('operator-accounting-condition-kind')}').value==='' && document.querySelector('${t('operator-accounting-effectiveFrom')}').value==='' && !document.querySelector('${t('operator-accounting-condition-date')}')`),'new document clears stale proposed date')
 await ui.action('invoice_terms.set',{sourceId:invoices[1].source,'condition-kind':'unknown',effectiveFrom:today,reason:'Explicitly unknown second invoice agreement'})
 const knownReceipt=JSON.parse(sql(`SELECT jsonb_build_object('requestId',request_id,'resourceId',id,'sourceId',source_id,'documentTerms',archava_accounting.terms_snapshot(t)) FROM archava_accounting.document_terms t WHERE book_id='${book}' AND source_id='${invoices[0].source}' AND revision=1`))
 await ui.action('invoice_terms.set',{sourceId:invoices[0].source,'condition-kind':'unknown',effectiveFrom:today,reason:'Explicit reviewed withdrawal of known due date'})
 const recovered=await ok(call(`/erp/accounting/commands/status?tenant=${tenant}&requestId=${knownReceipt.requestId}`));assert.equal(recovered.receipt.documentTerms.condition.date,due);assert.equal(recovered.receipt.revision,1)
 const unknown=await report('receivable');assert.equal(unknown.totals.unknownTermsMinor,'1250000');assert.equal(unknown.totals.netOutstandingMinor,'1250000')
 pass('DUE-native-source-reset-unknown-and-original-replay','Changing source clears both proposed condition and effective date; explicit unknown clears the date while recovery retains the original known-date revision')
 await ui.select('accounting-aging-side','receivable');await page.fill(t('accounting-aging-date'),today);await page.click(t('accounting-aging-run'));await until(()=>page.text(t('accounting-aging-totals')).then(s=>s.includes('12500.00')),'whole-book exact native due-date totals')
 assert.ok((await page.text(t('accounting-aging-control-status'))).includes('agree'));assert.ok((await page.text(t('accounting-aging-documents'))).includes(code+'-1'))
 await page.evaluate(`document.querySelector('${t('accounting-aging-totals')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('due-terms-native-report.png')
 const supplierUi=financialUi({page,origin,until,company:store.company});await supplierUi.open()
 const sources=await ok(call(`/erp/accounting/workspace?tenant=${tenant}&companyId=${store.company}&collection=supplier_term_sources`));assert.equal(sources.supplier_term_sources.length,1);const supplier=sources.supplier_term_sources[0]
 await supplierUi.action('supplier_terms.set',{sourceId:supplier.id,'condition-kind':'due_on','condition-date':due,effectiveFrom:supplier.sourceDate,reason:'Actual settled supplier bill retains separately reviewed dates'})
 const supplierReport=await report('payable',store.book);assert.equal(supplierReport.totals.netOutstandingMinor,'0');assert.equal(supplierReport.positions.length,1);assert.equal(supplierReport.positions[0].terms.condition.date,due);assert.equal(supplierReport.postedSourceControlsAgree,true)
 const proof=JSON.parse(sql(`SELECT jsonb_build_object('termsRows',(SELECT count(*) FROM archava_accounting.document_terms WHERE book_id IN ('${book}','${store.book}')),'invoiceTerms',coalesce((SELECT jsonb_agg(archava_accounting.terms_snapshot(t) ORDER BY t.document_id,t.revision) FROM archava_accounting.document_terms t WHERE t.book_id='${book}'),'[]'::jsonb),'billTerms',coalesce((SELECT jsonb_agg(archava_accounting.terms_snapshot(t)) FROM archava_accounting.document_terms t WHERE t.book_id='${store.book}'),'[]'::jsonb))`));assert.equal(proof.termsRows,4)
 writeFileSync(join(output,'due-terms-sql.json'),JSON.stringify({company,book,knownReceipt,before,dated,unknown,supplierReport,proof},null,2)+'\n')
 pass('DUE-native-whole-book-report-and-supplier-terms','Native AR totals/independent controls and the original supplier document render after actual reviewed date writes; dates change no original money or settled B01 balances')
 return{company,book,asOf:today,knownReceipt,netOutstandingMinor:'1250000',unknownTermsMinor:'1250000',supplierBook:store.book,supplierNetOutstandingMinor:'0',termsRows:4,scope:'Owned actual native/HTTP/SQL original-document due terms and exact as-of report; legacy opening imports, historical knowledge-time reconstruction and complete close remain separate.'}
}
