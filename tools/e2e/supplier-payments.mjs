/** Real native form -> reviewed supplier bank settlement -> exact independent SQL. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
import {writeFileSync} from 'node:fs'
import {join} from 'node:path'
export async function verifySupplierPayments({page,origin,facts,until,pass,suffix,output}) {
  assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(suffix,/^[0-9a-f]{8}$/)
  const tenant='acme-retail',t=id=>`[data-testid="${id}"]`,code=`PAY-${suffix}`.toUpperCase()
  const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='30s';SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:35000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
  // Explicit fixture-only setup permits this journey to run alone. Installed
  // bootstrap/runtime grants are checked separately by native-payables.mjs.
  sql(`INSERT INTO archava_organizations.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write'],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY(SELECT DISTINCT p FROM unnest(m.permissions||EXCLUDED.permissions) p ORDER BY p)`)
  const call=(path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(body?'POST':'GET')},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body?`,body:${JSON.stringify(JSON.stringify(body))}`:''}}).then(async r=>({status:r.status,body:await r.json()}))`)
  const ok=async promise=>{const r=await promise;assert.equal(r.status,200,JSON.stringify(r));return r.body.receipt??r.body}
  const mutate=(module,command)=>ok(call(`/erp/${module}/commands`,{tenant,requestId:randomUUID(),command}))
  const finance=command=>mutate('accounting',command),org=(type,data)=>ok(call('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}}))
  const organization=await org('create_organization',{name:'Owned supplier bank acceptance',legalName:'Owned Supplier Bank Acceptance Company'})
  const company=(await org('company.create_from_organization',{organizationId:organization.organizationId,expectedSourceRevision:1,code,reason:'Synthetic source-backed supplier payment acceptance'})).companyId
  const book=(await finance({type:'book.create',companyId:company,expectedCompanyRevision:1,code,name:'Reviewed supplier bank book',currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic one-operator book; separate HTTP tests prove independent posting'})).bookId
  const accounts={};for(const[c,cls,role]of [['INV','asset','operating'],['GRNI','liability','operating'],['AP','liability','operating'],['FEE','liability','operating'],['CASH','asset','cash'],['EQUITY','equity','financing']])accounts[c]=(await finance({type:'account.create',bookId:book,code:c,name:c,class:cls,cashRole:role,reason:'Reviewed original supplier source account'})).resourceId
  const ref=c=>({accountId:accounts[c],expectedAccountRevision:1});let date=sql("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
  await finance({type:'period.open',bookId:book,code:'SOURCE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned source window'})
  await finance({type:'purchase_policy.set',bookId:book,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:[['inventory','INV'],['goods_received_not_invoiced','GRNI'],['payable','AP'],['landed_fee_accrual','FEE']].map(([role,c])=>({role,...ref(c)})),reason:'Explicit future source capture; no historical import'})
  const supplier=await mutate('purchasing',{operation:'supplier.create',code,name:'Synthetic actual supplier'})
  const product=await mutate('commerce',{operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:code,title:'Actual accepted supplier goods',unit:'unit',trackStock:true,price:15000})
  const offer=await mutate('purchasing',{operation:'offer.create',supplierId:supplier.id,productId:product.id,unitCost:7000})
  const order=await mutate('purchasing',{operation:'order.create-for-company',companyId:company,expectedCompanyRevision:1,supplierId:supplier.id,lines:[{offerId:offer.id,quantity:2}]})
  await mutate('purchasing',{operation:'order.confirm',id:order.id,expectedRevision:1,reason:'Pin actual original source mapping'})
  const line=sql(`SELECT id FROM archava_purchasing.lines WHERE order_id='${order.id}'`)
  const received=await mutate('purchasing',{operation:'order.receive',id:order.id,expectedRevision:2,lines:[{lineId:line,accepted:2,rejected:0}],reason:'Actual accepted original inventory'})
  const document=await mutate('purchasing',{operation:'document.capture',orderId:order.id,expectedOrderRevision:3,supplierId:supplier.id,vendorReference:code,documentDate:date,currency:'IDR',declaredTotal:14000,lines:[{lineId:line,quantity:2,unitCost:7000}]})
  const bill=await mutate('purchasing',{operation:'document.match',id:document.id,expectedRevision:1,expectedOrderRevision:3,reason:'Review exact accepted goods claim'})
  date=sql(`SELECT source_date::text FROM archava_accounting.purchase_sources WHERE id='${bill.requestId}'`)
  for(const[source,c,roles]of [[received.requestId,'RECEIPT',['INV','GRNI']],[bill.requestId,'BILL',['GRNI','AP']]]){
    const j=await finance({type:'purchase_source.draft',sourceId:source,code:c,accounts:roles.map(ref),reason:'Review actual source and original accounts'})
    await finance({type:'journal.post',id:j.resourceId,expectedRevision:1,reason:'Separately post reviewed original supplier source'})
  }
  const prior=new Date(`${date}T00:00:00Z`);prior.setUTCDate(prior.getUTCDate()-1)
  const opening=await finance({type:'journal.draft',bookId:book,code:'OPENING',date:prior.toISOString().slice(0,10),sourceRef:'Explicit synthetic opening source',memo:'Reviewed actual opening bank equity',lines:[{...ref('CASH'),debitMinor:'2000000',creditMinor:'0',memo:'Opening bank'},{...ref('EQUITY'),debitMinor:'0',creditMinor:'2000000',memo:'Opening equity'}],reason:'Separate reviewed prior-day opening balance'})
  await finance({type:'journal.post',id:opening.resourceId,expectedRevision:1,reason:'Post explicit prior opening before current bank movements'})
  const bank=await finance({type:'bank_account.create',bookId:book,code:'MANUAL',name:'Owned supplier bank',account:ref('CASH'),reason:'Explicit manual bank evidence profile'})
  await finance({type:'bank_statement.import',bankAccountId:bank.resourceId,account:ref('CASH'),statement:{reference:code,fromDate:date,toDate:date,currency:'IDR',openingMinor:'2000000',closingMinor:'600000',rows:[
    {externalRef:'PAY-A',date,amountMinor:'-1000000',description:'Actual initial withdrawal'},
    {externalRef:'RETURN',date,amountMinor:'200000',description:'Actual returned payment'},
    {externalRef:'PAY-B',date,amountMinor:'-600000',description:'Actual remaining supplier payment'}]},reason:'Actual synthetic movements; do not infer a transfer'})
  const rows=JSON.parse(sql(`SELECT jsonb_object_agg(external_ref,id) FROM archava_accounting.bank_rows WHERE book_id='${book}'`))
  const select=async(id,value)=>{await until(()=>page.evaluate(`!![...document.querySelector(${JSON.stringify(t(id))})?.options||[]].find(o=>o.value===${JSON.stringify(value)})`),id+' authorized option');await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)}
  const open=async()=>{await page.navigate(`${origin}/app/erp?module=accounting`);await select('trade-company-select',company);await until(()=>page.evaluate(`!!document.querySelector('${t('operator-action')}')`),'supplier bank company workspace')}
  const review=async(type,fields)=>{await select('operator-action',type);await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current action grant');for(const[k,value]of Object.entries(fields)){
    const id=`operator-accounting-${k}`,isSelect=await page.evaluate(`document.querySelector(${JSON.stringify(t(id))})?.tagName==='SELECT'`)
    if(isSelect)await select(id,String(value));else await page.fill(t(id),String(value))
  }await page.click(t('module-accounting-create'));await until(()=>page.evaluate(`!!document.querySelector('${t('operator-review')}')`),'supplier payment review')}
  const confirm=async()=>{await page.click(t('operator-confirm'));await until(async()=>{if(await page.evaluate(`!!document.querySelector('${t('ops-refused')}')||!!document.querySelector('${t('ops-uncertain')}')`))throw Error(await page.text(t('ops-feedback')));return(await page.text(t('ops-feedback'))).includes('Saved.')},'matching supplier payment receipt')}
  const action=async(type,fields)=>{await review(type,fields);await confirm()}
  const fields=(row,amount,code)=>({bankRowId:row,code,'payableAccount-accountId':accounts.AP,'cashAccount-accountId':accounts.CASH,amountMinor:amount,reason:'Review original supplier bill, actual bank evidence and exact money'})
  await open();await review('payable_payment.draft',{sourceId:bill.requestId,...fields(rows['PAY-A'],'10000.00','PAYMENT-A')})
  const summary=await page.text(t('payable-review-summary'));assert.ok(summary.includes('IDR 10000.00'));assert.ok(summary.includes('PAY-A'));assert.ok(summary.includes(code))
  assert.equal(await page.evaluate(`document.querySelector('${t('operator-confirm')}').disabled`),false)
  await page.evaluate(`document.querySelector('${t('operator-review')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('supplier-payment-review.png')
  const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
  try{await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))');assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true);await page.screenshot('supplier-payment-mobile-review.png');await page.evaluate(`document.querySelector('${t('payable-source-journal')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('supplier-payment-mobile-journal.png')}
  finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.payable_settlements WHERE book_id='${book}'`),'0')
  await confirm();const first=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='PAYMENT-A'`)
  assert.equal(sql(`SELECT archava_accounting.payable_balance('${tenant}','${bill.requestId}')->>'reservedPaymentMinor'`),'1000000')
  pass('PAYABLE-native-payment-review',{source:bill.requestId,bankEvidence:rows['PAY-A'],exactMinor:'1000000',moneySent:false,draftReserved:true})
  await action('journal.post',{id:first,reason:'Separate explicit posting of reviewed supplier bank payment'})
  await action('bank_allocation.record',{bankRowId:rows['PAY-A'],journalId:first,journalLineOrdinal:2,'account-accountId':accounts.CASH,amountMinor:'10000.00',reason:'Reconcile the exact actual withdrawal with its posted cash leg'})
  await open();await review('payable_return.draft',{originalPaymentId:first,...fields(rows.RETURN,'2000.00','RETURN')})
  assert.ok((await page.text(t('payable-review-summary'))).includes('IDR 2000.00'));await confirm()
  const returned=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='RETURN'`)
  assert.equal(sql(`SELECT archava_accounting.payable_balance('${tenant}','${bill.requestId}')->>'availableMinor'`),'400000')
  await action('journal.post',{id:returned,reason:'Recognize the actual returned payment only after separate review'})
  await action('bank_allocation.record',{bankRowId:rows.RETURN,journalId:returned,journalLineOrdinal:2,'account-accountId':accounts.CASH,amountMinor:'2000.00',reason:'Reconcile the actual positive returned payment'})
  await open();await action('payable_payment.draft',{sourceId:bill.requestId,...fields(rows['PAY-B'],'6000.00','PAYMENT-B')})
  const last=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='PAYMENT-B'`)
  await action('journal.post',{id:last,reason:'Post the exact remaining supplier payable'})
  await action('bank_allocation.record',{bankRowId:rows['PAY-B'],journalId:last,journalLineOrdinal:2,'account-accountId':accounts.CASH,amountMinor:'6000.00',reason:'Match final actual supplier withdrawal'})
  const proof=JSON.parse(sql(`SELECT jsonb_build_object('settlements',(SELECT count(*) FROM archava_accounting.payable_settlements WHERE book_id='${book}'),'balance',archava_accounting.payable_balance('${tenant}','${bill.requestId}'),'ap',(SELECT sum(l.credit_minor-l.debit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE l.book_id='${book}' AND j.state='posted' AND l.account_id='${accounts.AP}'),'cash',(SELECT sum(l.debit_minor-l.credit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE l.book_id='${book}' AND j.state='posted' AND l.account_id='${accounts.CASH}'),'unmatched',(SELECT sum(abs(archava_accounting.bank_row_remaining(tenant_id,id)))::text FROM archava_accounting.bank_rows WHERE book_id='${book}'))`))
  assert.equal(proof.settlements,3);assert.equal(proof.balance.postedPaymentMinor,'1400000');assert.equal(proof.balance.outstandingMinor,'0');assert.equal(proof.ap,'0');assert.equal(proof.cash,'600000');assert.equal(proof.unmatched,'0')
  writeFileSync(join(output,'supplier-payment-sql.json'),JSON.stringify(proof,null,2)+'\n')
  await open()
  const expected={"Recognized bills after posted corrections":"IDR 14000.00","Posted payments less posted returns":"IDR 14000.00","Outstanding":"IDR 0.00","Reserved in payment drafts":"IDR 0.00","Available for a new payment draft":"IDR 0.00"}
  let actual={}
  try{await until(async()=>{actual=await page.evaluate(`(()=>{const n=document.querySelector('${t('payable-current-balances')}');return n?Object.fromEntries([...n.querySelectorAll('dt')].map(d=>[d.textContent.trim(),d.nextElementSibling?.textContent.trim()])):{};})()`);return Object.entries(expected).every(([k,v])=>actual[k]===v)},'current supplier balances after the company workspace finishes loading')}
  catch(error){throw Error(`${error.message}; observed ${JSON.stringify(actual)}`)}
  await page.evaluate(`document.querySelector('${t('payable-current-balances')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('supplier-payable-final-balances.png')
  pass('PAYABLE-native-return-final-payment-and-SQL',proof)
  return{company,book,scope:'Owned actual HTTP source capture, native supplier payment/return review, separate one-operator posting and bank matching; not a bank transfer, full financial close, aging, opening import or live supplier acceptance.',proof}
}
