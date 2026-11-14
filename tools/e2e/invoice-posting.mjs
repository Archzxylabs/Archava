/** Native invoice -> exact source journal, with a task-owned HTTP payment gateway. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'

export async function verifyInvoicePosting({page,origin,facts,until,pass,suffix,paymentProvider}){
 assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(suffix,/^[0-9a-f]{8}$/)
 const tenant='acme-retail',t=id=>`[data-testid="${id}"]`
 const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
 const call=(method,path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(method)},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body===undefined?'':`,body:${JSON.stringify(JSON.stringify(body))}`}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const post=async(path,body)=>{const r=await call('POST',path,body);assert.equal(r.status,200,JSON.stringify(r));return r.body}
 const org=async(type,data)=>(await post('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}})).receipt
 const commerce=async command=>(await post('/erp/commerce/commands',{tenant,requestId:randomUUID(),command})).receipt
 const select=async(id,value)=>{
  await until(()=>page.evaluate(`!!document.querySelector(${JSON.stringify(t(id))})`),id)
  await page.evaluate(`document.querySelector(${JSON.stringify(t(id))}).focus()`)
  await until(()=>page.evaluate(`!![...document.querySelector(${JSON.stringify(t(id))}).options].find(o=>o.value===${JSON.stringify(value)})`),`${id} authorized option`)
  await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 }
 const open=async module=>{await page.navigate(`${origin}/app/erp?module=${module}`);await select('trade-company-select',company)}
 const fill=async(name,value)=>{
  const id=`operator-accounting-${name}`,kind=await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});if(!n)throw Error('Missing financial field '+${JSON.stringify(id)});return n.tagName==='SELECT'?'select':n.type})()`)
  if(kind==='select')await select(id,String(value))
  else if(kind==='checkbox')await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.checked=${!!value};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  else await page.fill(t(id),String(value))
 }
 const review=async(action,fields)=>{await select('operator-action',action);await until(()=>page.evaluate(`document.querySelector('.operator-actions form fieldset')?.disabled===false`),'current financial permission');for(const[key,value]of Object.entries(fields))await fill(key,value);await page.click(t('module-accounting-create'));await until(()=>page.evaluate(`!!document.querySelector('${t('operator-review')}')`),'explicit financial review')}
 const confirm=async()=>{await page.click(t('operator-confirm'));await until(async()=>{const text=await page.text(t('ops-feedback'));if(await page.evaluate(`!!document.querySelector('${t('ops-refused')}')||!!document.querySelector('${t('ops-uncertain')}')`))throw Error(text);return text.includes('Saved.')},'matching financial source receipt')}
 const action=async(op,fields)=>{await review(op,fields);await confirm()}
 const organization=await org('create_organization',{name:`Invoice source ${suffix}`,legalName:`Invoice Source Company ${suffix}`})
 const company=(await org('company.create_from_organization',{organizationId:organization.organizationId,expectedSourceRevision:1,code:`SRC-${suffix}`.toUpperCase(),reason:'Explicit owned invoice-source acceptance company'})).companyId
 const partner=await org('create_organization',{name:`Source customer ${suffix}`,legalName:`Source Customer Official ${suffix}`})
 const party=(await org('party.create_from_organization',{organizationId:partner.organizationId,expectedSourceRevision:1,customer:true,supplier:false})).partyId
 await open('accounting')
 const bookCode=`SRC-BOOK-${suffix}`.toUpperCase()
 await action('book.create',{code:bookCode,name:`Invoice posting ${suffix}`,currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator policy; no legacy opening import'})
 const book=sql(`SELECT id FROM archava_accounting.books WHERE code='${bookCode}'`);assert.match(book,/^[0-9a-f-]{36}$/)
 const account=async(code,cls,cash)=>{await action('account.create',{bookId:book,code,name:code,class:cls,cashRole:cash,reason:'Reviewed invoice posting classification'});return sql(`SELECT id FROM archava_accounting.accounts WHERE book_id='${book}' AND code='${code}'`)}
 const ar=await account('AR','asset','operating'),revenue=await account('REVENUE','revenue','operating'),clearing=await account('CLEARING','asset','none')
 await action('period.open',{bookId:book,code:'SOURCE-TEST',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned synthetic source period spanning the source clock'})
 const period=sql(`SELECT id FROM archava_accounting.periods WHERE book_id='${book}'`)
 await action('invoice_policy.set',{bookId:book,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr','mappings-0-role':'receivable','mappings-0-accountId':ar,'mappings-1-role':'revenue','mappings-1-accountId':revenue,'mappings-2-role':'payment_clearing','mappings-2-accountId':clearing,reason:'Future whole Rupiah invoice values convert by 100 into IDR book units; provider money remains in clearing'})
 assert.equal(sql(`SELECT count(*) FROM archava_accounting.invoice_policies WHERE book_id='${book}'`),'1')
 const policyRevision=()=>sql(`SELECT revision FROM archava_accounting.invoice_policies WHERE book_id='${book}'`)
 assert.equal(policyRevision(),'1')
 await until(()=>page.text(t('operator-accounting-expectedRevision')).then(s=>s.includes('Current posting policy')),'current saved policy loaded for review')
 await action('invoice_policy.set',{bookId:book,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr','mappings-0-role':'receivable','mappings-0-accountId':ar,'mappings-1-role':'revenue','mappings-1-accountId':revenue,'mappings-2-role':'payment_clearing','mappings-2-accountId':clearing,reason:'Review the existing posting policy without manually entering an internal revision'})
 assert.equal(policyRevision(),'2')
 assert.equal(await page.evaluate(`document.querySelector('${t('operator-accounting-expectedRevision')}')?.tagName`),'P')
 pass('INVOICEPOST-current-policy-review','Both first setup and subsequent policy review load the current book policy automatically; internal revisions are not a manual customer input')

 assert.equal(sql(`SELECT count(*) FROM archava_accounting.invoice_sources WHERE book_id='${book}'`),'0')
 pass('INVOICEPOST-native-explicit-policy','Native policy reviews whole-IDR units and receivable/revenue/clearing account roles without importing historic invoices')
 const product=await commerce({operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:`SRC-P-${suffix}`.toUpperCase(),title:'Non-stock source acceptance service',unit:'unit',trackStock:false,price:15000})
 const order=await commerce({operation:'order.create_for_company',companyId:company,expectedCompanyRevision:1,partyId:party,expectedPartyRevision:1,lines:[{productId:product.id,quantity:2}]})
 await commerce({operation:'order.reserve',id:order.id,expectedRevision:1,holdSeconds:300});await commerce({operation:'order.fulfill',id:order.id,expectedRevision:2})
 await open('invoices')
 await select('invoice-source-order',order.id);const invoiceCode=`SRC-INV-${suffix}`.toUpperCase();await page.fill(t('invoice-order-code'),invoiceCode);await page.click(t('invoice-from-order-save'))
 const invoice=await until(()=>sql(`SELECT id FROM archava_billing.invoices WHERE invoice_code='${invoiceCode}'`),'native source invoice')
 await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice}`)}')`),'native source invoice row');await page.click(t(`ops-open-${invoice}`));await page.fill(t('invoice-reason'),'Issue reviewed company source invoice');await page.click(t('invoice-issue'))
 const source=async effect=>until(()=>sql(`SELECT id FROM archava_accounting.invoice_sources WHERE book_id='${book}' AND invoice_id='${invoice}' AND effect='${effect}'`),`captured ${effect} source`)
 const issue=await source('issue');assert.equal(sql(`SELECT amount_minor FROM archava_accounting.invoice_sources WHERE id='${issue}'`),'3000000')
 assert.equal(sql(`SELECT count(*) FROM archava_accounting.journals WHERE book_id='${book}'`),'0')
 pass('INVOICEPOST-native-issuance-source','Native order invoicing and issuance capture the actual frozen 30000 whole Rupiah as 3000000 IDR book units; no journal posts automatically')
 const sourceDraft=async(id,code,counter,amount)=>{
  await open('accounting');await review('invoice_source.draft',{sourceId:id,code,'accounts-0-accountId':ar,'accounts-1-accountId':counter,reason:'Review captured invoice amount, source date and policy accounts'})
  assert.match(await page.text(t('operator-review')),new RegExp(`IDR ${amount.replace('.','\\.')}`));await confirm()
  const journal=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='${code}'`);assert.match(journal,/^[0-9a-f-]{36}$/)
  await action('journal.post',{id:journal,reason:'Post the separately reviewed source journal'})
  assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`),'posted');return journal
 }
 await open('accounting');await review('period.close',{id:period,reason:'Pending sources must prevent finalization'});await page.click(t('operator-confirm'));await until(()=>page.evaluate(`!!document.querySelector('${t('ops-refused')}')`),'pending invoice source close refusal')
 const journals=[await sourceDraft(issue,'ISSUE',revenue,'30000.00')]
 pass('INVOICEPOST-native-review-source-and-close-refusal','Pending captured sources refuse close; native review shows the exact captured money and posts a source-bound journal with a matching receipt')
 await open('invoices');await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice}`)}')`),'source invoice for credit');await page.click(t(`ops-open-${invoice}`));await page.fill(t('invoice-reason'),'Reviewed customer credit');await page.fill(t('invoice-correction'),'-5000');await page.evaluate(`document.querySelector('${t('invoice-correction')}').closest('form').querySelector('button[type="submit"]').click()`)
 const correction=await source('correction');journals.push(await sourceDraft(correction,'CREDIT',revenue,'-5000.00'))
 pass('INVOICEPOST-native-credit-source','A real native invoice correction keeps the original issued source and posts the signed source delta')
 await open('invoices');await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice}`)}')`),'source invoice payment');await page.click(t(`ops-open-${invoice}`))
 paymentProvider.state.status='pending';paymentProvider.state.refund=0
 await until(()=>page.evaluate(`document.querySelector('${t('payment-create')}')?.disabled===false`),'owned gateway creation available');await page.click(t('payment-create'));await until(()=>page.evaluate(`!!document.querySelector('${t('payment-checkout')}')`),'owned source checkout')
 await until(()=>page.evaluate(`document.querySelector('${t('payment-check')}')?.disabled===false`),'owned source verification available');paymentProvider.state.status='settlement';await page.click(t('payment-check'))
 const collection=await source('collection');journals.push(await sourceDraft(collection,'COLLECTION',clearing,'25000.00'))
 const net=accountId=>sql(`SELECT coalesce(sum(l.debit_minor-l.credit_minor),0) FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE j.book_id='${book}' AND j.state='posted' AND l.account_id='${accountId}'`)
 assert.equal(net(ar),'0');assert.equal(net(clearing),'2500000')
 pass('INVOICEPOST-owned-verified-collection-clearing','The authenticated owned gateway settles 25000 Rupiah; the native source journal clears AR into provider clearing without claiming a bank settlement')
 await open('invoices');await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice}`)}')`),'source invoice refund');await page.click(t(`ops-open-${invoice}`));paymentProvider.state.status='refund';paymentProvider.state.refund=25000
 await new Promise(resolve=>setTimeout(resolve,2100));await until(()=>page.evaluate(`document.querySelector('${t('payment-check')}')?.disabled===false`),'owned source refund verification available');await page.click(t('payment-check'))
 const refund=await source('refund');journals.push(await sourceDraft(refund,'REFUND',clearing,'-25000.00'));assert.equal(net(clearing),'0');assert.equal(net(ar),'2500000')
 await open('invoices');await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${invoice}`)}')`),'source invoice void');await page.click(t(`ops-open-${invoice}`));await page.fill(t('invoice-reason'),'Void after the verified full refund');await page.click(t('invoice-void'))
 const voidSource=await source('void');journals.push(await sourceDraft(voidSource,'VOID',revenue,'-25000.00'));assert.equal(net(ar),'0');assert.equal(net(revenue),'0')
 pass('INVOICEPOST-owned-refund-and-native-void','Verified refund and native invoice void create separate signed sources/journals; original histories remain and AR, revenue and clearing return to zero')
 await action('period.close',{id:period,reason:'All five captured invoice effects are posted'})
 await page.fill(t('accounting-report-from'),'2000-01-01');await page.fill(t('accounting-report-to'),'2100-12-31');await page.click(t('accounting-report-run'));await until(()=>page.text(t('accounting-source-coverage')).then(s=>s.includes('5 posted, 0 awaiting posting')),'complete captured-source report coverage')
 await page.evaluate(`document.querySelector('${t('accounting-report-result')}').scrollIntoView({block:'center',behavior:'instant'})`);await page.screenshot('invoice-posting-reports.png')
 const current=await call('GET',`/own/config?tenant=${tenant}`)
 for(const module of ['billing','accounting']){const r=await post('/own/plan',{tenant,request:{baseRevision:current.body.config.revision,disable:[module]}});assert.equal(r.plan.blocked.reason,'mandatory');assert.equal(r.plan.blocked.module,module)}
 pass('INVOICEPOST-native-source-close-and-owner-dependency','All five sources are posted before native close; reports expose complete captured-source coverage and Owner refuses to disable its live accounting capture modules')
 const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
 try{await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await page.evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true);await page.screenshot('invoice-posting-mobile.png')}finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
 pass('INVOICEPOST-native-source-mobile','Native invoice posting reports and source coverage remain within a 390px page')
 return{company,book,invoice,period,sources:[issue,correction,collection,refund,voidSource],journals,expectedSources:5,scope:'Owned native whole-IDR invoice source journey. AP, stock/COGS, terms/aging, formal credit documents, bank reconciliation, localization and real-user acceptance remain separate.'}
}
