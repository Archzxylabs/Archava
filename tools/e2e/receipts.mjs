/** Manual customer receipts and the Indonesia tax pack for one IDR company, on the native screens. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
import {financialUi} from './financial-ui.mjs'
export async function verifyReceipts({page,origin,facts,until,pass,suffix}){
 assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(suffix,/^[0-9a-f]{8}$/)
 const tenant='acme-retail',t=id=>`[data-testid="${id}"]`,q=JSON.stringify,upper=s=>s.toUpperCase()
 const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='30s';SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:35000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
 const json=query=>JSON.parse(sql(query))
 const call=(path,body)=>page.evaluate(`fetch(${q(path)},{method:${q(body?'POST':'GET')},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body?`,body:${q(q(body))}`:''}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const ok=async(path,body)=>{const r=await call(path,body);assert.equal(r.status,200,q(r));return r.body.receipt??r.body}
 const org=(type,data)=>ok('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}})
 const command=(domain,command)=>ok(`/erp/${domain}/commands`,{tenant,requestId:randomUUID(),command})
 const bill=mutation=>ok('/erp/billing/commands',{tenant,mutation:{commandId:randomUUID(),...mutation}})
 const view=(domain,company)=>page.evaluate(`fetch('/erp/${domain}/workspace?tenant=${tenant}&companyId=${company}').then(r=>r.json())`)
 const has=selector=>page.evaluate(`!!document.querySelector(${q(selector)})`)
 const value=selector=>page.evaluate(`document.querySelector(${q(selector)}).value`)
 const select=async(id,wanted)=>{
  await until(()=>has(t(id)),id);await page.evaluate(`document.querySelector(${q(t(id))}).focus()`)
  await until(()=>page.evaluate(`!![...document.querySelector(${q(t(id))}).options].find(o=>o.value===${q(String(wanted))})`),`${id} offers ${wanted}`)
  await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});n.value=${q(String(wanted))};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 }
 const tick=(id,wanted)=>page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});n.checked=${!!wanted};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 const click=async id=>{await until(()=>page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});return !!n&&!n.disabled&&n.getClientRects().length>0&&!n.closest('fieldset')?.disabled;})()`),'enabled native '+id);await page.click(t(id))}
 const submitOf=id=>page.evaluate(`document.querySelector(${q(t(id))}).closest('form').querySelector('button[type="submit"]').click()`)
 const open=async(module,company)=>{await page.navigate(`${origin}/app/erp?module=${module}`);await scope(company)}
 const scope=async company=>{await until(()=>has(`${t('trade-company-select')} option[value="${company}"]`),'company scope option');await until(async()=>{if((await page.text(t('trade-company-selected'))).includes(company))return true;await page.evaluate(`(()=>{const n=document.querySelector(${q(t('trade-company-select'))});n.value=${q(company)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`);return false},'selected company scope')}
 const openRow=async id=>{await until(()=>has(t('ops-open-'+id)),'record row '+id);await page.click(t('ops-open-'+id))}
 const shot=async(file,id)=>{await page.evaluate(`document.querySelector(${q(t(id))}).scrollIntoView({block:'center',behavior:'instant'})`);await page.evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');await page.screenshot(file)}
 const rows=id=>page.evaluate(`[...document.querySelectorAll(${q(t(id)+' tbody tr')})].map(tr=>[...tr.cells].map(c=>c.textContent))`)
 const pairs=selector=>page.evaluate(`(()=>{const out={};for(const dt of document.querySelectorAll(${q(selector)}+' dt'))out[dt.textContent]=dt.nextElementSibling.textContent;return out})()`)
 // Native Accounting action form: fill, review, confirm.
 const field=async(name,wanted)=>{
  const id=`operator-accounting-${name}`;await until(()=>has(t(id)),id+' rendered')
  const kind=await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});return n.tagName==='SELECT'?'select':n.type})()`)
  if(kind==='select')await select(id,wanted)
  else if(kind==='checkbox')await tick(id,wanted)
  else await page.fill(t(id),String(wanted))
 }
 const choose=async action=>{await select('operator-action',action);await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current financial grant')}
 const reviewed=async action=>{await page.click(t('module-accounting-create'));await until(async()=>{if(await has(t('ops-refused')))throw Error(await page.text(t('ops-feedback')));return has(t('operator-review'))},action+' review')}
 const review=async(action,fields,{extraAccount=false}={})=>{await choose(action);if(extraAccount)await page.click(t('operator-accounting-accounts')+' > button');for(const[k,v]of Object.entries(fields))await field(k,v);await reviewed(action)}
 const confirm=async()=>{await page.click(t('operator-confirm'));await until(async()=>{const text=await page.text(t('ops-feedback'));if(await has(t('ops-refused'))||await has(t('ops-uncertain')))throw Error(text);return text.includes('Saved.')},'matching financial receipt')}
 const act=async(action,fields)=>{await review(action,fields);await confirm()}

 // Standalone browser fixture only, as in the other financial journeys: the operator may manage organizations, companies and parties.
 sql(`INSERT INTO archava_organizations.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write','parties.read','parties.write'],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY(SELECT DISTINCT p FROM unnest(m.permissions||EXCLUDED.permissions) p ORDER BY p)`)
 // Setup the screens under test do not own, through the command API.
 const sellerTaxId='01.234.567.8-999.000',buyerTaxId='02.345.678.9-012.000',badTaxId='AB-123'
 const o=await org('create_organization',{name:`Receipt company ${suffix}`,legalName:`Receipt Company Official ${suffix}`,taxId:sellerTaxId})
 const company=(await org('company.create_from_organization',{code:upper(`RCT-${suffix}`),organizationId:o.organizationId,expectedSourceRevision:1,reason:'Owned receipt and tax pack acceptance company'})).companyId
 const odd=await org('create_organization',{name:`Odd tax id company ${suffix}`,legalName:`Odd Tax Id Company Official ${suffix}`,taxId:badTaxId})
 const oddCompany=(await org('company.create_from_organization',{code:upper(`ODD-${suffix}`),organizationId:odd.organizationId,expectedSourceRevision:1,reason:'Owned company whose tax id is not digits'})).companyId
 const buyerName=`Receipt buyer ${suffix}`,buyerOrg=await org('create_organization',{name:buyerName,legalName:`Receipt Buyer Official ${suffix}`,taxId:buyerTaxId})
 const party=(await org('party.create_from_organization',{organizationId:buyerOrg.organizationId,expectedSourceRevision:1,customer:true,supplier:false})).partyId
 const bookCode=upper(`RCT-BOOK-${suffix}`)
 const book=(await command('accounting',{type:'book.create',companyId:company,expectedCompanyRevision:1,code:bookCode,name:`Receipt book ${suffix}`,currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator IDR book for the receipt and tax pack acceptance'})).bookId
 assert.match(book,/^[0-9a-f-]{36}$/)
 const accounts={};for(const[code,cls,cash]of [['AR','asset','operating'],['REVENUE','revenue','operating'],['CLEARING','asset','none'],['BANK','asset','cash'],['VAT-OUT','liability','operating']])accounts[code]=(await command('accounting',{type:'account.create',bookId:book,code,name:code==='VAT-OUT'?'Output tax':code,class:cls,cashRole:cash,reason:'Reviewed receipt acceptance account'})).resourceId
 await command('accounting',{type:'period.open',bookId:book,code:'RCT-SOURCE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned synthetic source period'})
 const ref=c=>({accountId:accounts[c],expectedAccountRevision:1})
 await command('accounting',{type:'invoice_policy.set',bookId:book,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:[['receivable','AR'],['revenue','REVENUE'],['payment_clearing','CLEARING']].map(([role,c])=>({role,...ref(c)})),reason:'Whole Rupiah documents post as IDR book units'})
 const plain=await command('commerce',{operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:upper(`RCT-SVC-${suffix}`),title:`Receipt service ${suffix}`,unit:'unit',trackStock:false,price:50000})
 const tea=await command('commerce',{operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:upper(`RCT-TEA-${suffix}`),title:`Taxed tea ${suffix}`,unit:'unit',trackStock:false,price:40000})
 const today=sql("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
 const bank=(await command('accounting',{type:'bank_account.create',bookId:book,code:'RCT-BANK',name:'Receipt bank evidence',account:ref('BANK'),reason:'Explicit actual cash account binding'})).resourceId
 await command('accounting',{type:'bank_statement.import',bankAccountId:bank,account:ref('BANK'),statement:{reference:`RCT-${suffix}`,fromDate:today,toDate:today,currency:'IDR',openingMinor:'0',closingMinor:'2000000',rows:[{externalRef:'TRANSFER-IN',date:today,amountMinor:'2000000',description:'IDR 20000 transfer from the buyer'}]},reason:'Immutable actual bank evidence of the transfer'})
 const bankRow=sql(`SELECT id FROM archava_accounting.bank_rows WHERE book_id='${book}' AND external_ref='TRANSFER-IN'`);assert.match(bankRow,/^[0-9a-f-]{36}$/)
 const setup='the company with its tax id, a second company with a tax id that is not digits, the buyer with its tax id, the single-operator IDR book, ledger accounts, period, invoice posting policy, products, bank account and its statement row were created through the command API'

 // A fulfilled company order and its draft invoice, made through the command API.
 const draftInvoice=async(product,quantity,code)=>{
  const order=await command('commerce',{operation:'order.create_for_company',companyId:company,expectedCompanyRevision:1,partyId:party,expectedPartyRevision:1,lines:[{productId:product.id,quantity}]})
  await command('commerce',{operation:'order.reserve',id:order.id,expectedRevision:1,holdSeconds:600});await command('commerce',{operation:'order.fulfill',id:order.id,expectedRevision:2})
  const made=await bill({type:'create_invoice_from_order',invoiceCode:code,orderId:order.id,expectedOrderRevision:3});return made.invoiceId
 }
 const invoiceRow=code=>json(`SELECT coalesce((SELECT jsonb_build_object('id',id,'gross',gross_minor,'settled',settled_minor,'outstanding',outstanding_minor,'status',status,'revision',revision,'tax',tax_minor,'reference',tax_reference,'transactionCode',tax_transaction_code,'referenceVersion',tax_reference_version,'seller',seller_tax_id,'buyer',buyer_tax_id) FROM archava_billing.invoices WHERE invoice_code='${code}'),'null'::jsonb)`)
 const events=invoiceId=>json(`SELECT coalesce(jsonb_agg(jsonb_build_object('id',id,'outcome',outcome,'amount',amount_minor,'provider',provider_id,'method',manual_method,'receivedOn',received_on,'reference',reference,'reason',reason,'by',recorded_by,'reverses',reverses_event_id) ORDER BY verified_at,id),'[]'::jsonb) FROM archava_billing.payment_events WHERE invoice_id='${invoiceId}'`)
 const eventCount=()=>sql(`SELECT count(*) FROM archava_billing.payment_events WHERE tenant_id='${tenant}' AND invoice_id IN (SELECT id FROM archava_billing.invoices WHERE (to_jsonb(invoices)->>'company_id')='${company}')`)
 const issue=async(invoice,reason)=>{await until(()=>has(t('invoice-issue')),'draft invoice detail');await page.fill(t('invoice-reason'),reason);await click('invoice-issue')}
 const invoiceSource=(invoice,effect)=>until(()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'amount',amount_minor,'tax',tax_minor,'date',source_date::text,'bookId',book_id)::text FROM archava_accounting.invoice_sources WHERE book_id='${book}' AND invoice_id='${invoice}' AND effect='${effect}' ORDER BY occurred_at DESC LIMIT 1),'')`),`captured invoice ${effect}`).then(JSON.parse)
 const lines=journal=>json(`SELECT jsonb_agg(jsonb_build_array(account_code,debit_minor,credit_minor) ORDER BY ordinal) FROM archava_accounting.journal_lines WHERE journal_id='${journal}'`)
 // One native source review: the proposed journal names every account, then draft and post.
 const post=async(action,source,code,roles,posted)=>{
  await open('accounting',company);const fields={sourceId:source,code,reason:'Review the captured amount and the account roles'};roles.forEach((role,i)=>fields[`accounts-${i}-accountId`]=accounts[role]);await review(action,fields,{extraAccount:roles.length===3})
  const reviewText=await page.text(t('operator-review'));for(const role of roles)assert.ok(reviewText.includes(role),`${code}: ${role} missing in the proposed journal: ${reviewText}`)
  await confirm();const journal=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='${code}'`);assert.match(journal,/^[0-9a-f-]{36}$/)
  await act('journal.post',{id:journal,reason:'Post the separately reviewed source'});assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`),'posted')
  assert.deepEqual(lines(journal),posted,code);return journal
 }
 const openInvoice=async invoiceId=>{await open('invoices',company);await openRow(invoiceId);await until(()=>has(t('invoice-payments-panel')),'payments panel of the invoice')}
 const paymentRows=()=>rows('invoice-payments')
 const noInternalWords=async scopeName=>{const body=await page.evaluate('document.body.textContent');for(const[word,pattern]of [['NPWP',/\bNPWP\b/],['PKP',/\bPKP\b/],['PPN',/\bPPN\b/],['e-Faktur',/e-?faktur/i],['Coretax',/coretax/i]])assert.ok(!pattern.test(body),`${scopeName} names ${word}`)}

 // ===== Receipts =====
 const code1=upper(`RCT-INV-${suffix}`),inv1=await draftInvoice(plain,1,code1)
 await open('invoices',company);await openRow(inv1);await issue(inv1,'Issue the reviewed invoice that the receipts are recorded on')
 await until(()=>invoiceRow(code1).status==='issued','issued invoice '+code1)
 assert.deepEqual(events(inv1),[]);await until(()=>has(t('invoice-receipt-form')),'receipt form on the issued invoice')
 assert.equal(await page.text(t('invoice-payments-empty')),'No payment has been recorded on this invoice.')

 // 1. The form defaults, then a partial bank-transfer receipt.
 assert.equal(await value(t('invoice-receipt-amount')),'50000','the amount defaults to the outstanding amount')
 assert.equal(await value(t('invoice-receipt-date')),today,'the date defaults to today (UTC)')
 assert.equal(await value(t('invoice-receipt-method')),'bank_transfer')
 assert.ok((await page.text(t('invoice-receipt-notice'))).includes('A payment recorded here is manual'));assert.ok(!/verified/i.test(await page.text(t('invoice-receipt-form'))),'a manual receipt is never worded as verified')
 const reference1=upper(`TRF-${suffix}`),reason1='Seen on the bank statement of the day'
 await page.fill(t('invoice-receipt-amount'),'20000');await page.fill(t('invoice-receipt-reference'),reference1);await page.fill(t('invoice-receipt-reason'),reason1)
 await click('invoice-receipt-save');await until(()=>invoiceRow(code1).settled===20000,'partial receipt stored')
 const afterFirst=invoiceRow(code1);assert.deepEqual({settled:afterFirst.settled,outstanding:afterFirst.outstanding,status:afterFirst.status},{settled:20000,outstanding:30000,status:'issued'})
 const first=events(inv1);assert.equal(first.length,1)
 assert.deepEqual({outcome:first[0].outcome,amount:first[0].amount,provider:first[0].provider,method:first[0].method,receivedOn:first[0].receivedOn,reference:first[0].reference,reason:first[0].reason,by:first[0].by,reverses:first[0].reverses},{outcome:'succeeded',amount:20000,provider:'manual:bank_transfer',method:'bank_transfer',receivedOn:today,reference:reference1,reason:reason1,by:'alice',reverses:null})
 const who=sql(`SELECT coalesce(nullif(display_name,''),principal_id) FROM archava_billing.memberships WHERE tenant_id='${tenant}' AND principal_id='alice'`)
 await until(()=>paymentRows().then(r=>r.length===1),'one payment row')
 assert.deepEqual(await paymentRows(),[[today,'Manual · bank transfer','Payment received','Rp 20.000',`${reference1} · ${reason1}`,who]])
 assert.equal(await page.evaluate(`document.querySelector(${q(t('invoice-payment-row'))}).dataset.origin`),'manual')
 await until(()=>pairs('[data-testid="invoice-detail"] dl.ops-totals').then(v=>v['Settled']==='Rp 20.000'&&v['Outstanding']==='Rp 30.000'),'invoice totals after the part payment')
 await until(()=>value(t('invoice-receipt-amount')).then(v=>v==='30000'),'the form offers the new outstanding amount')
 await shot('receipt-partial.png','invoice-payments-panel')
 pass('RECEIPT-native-partial-bank-transfer',`The issued invoice ${code1} (Rp 50.000) showed the native "Record a payment received" form with the amount defaulting to 50000 (the outstanding amount, in IDR) and the date to today UTC (${today}); a partial bank transfer of 20000 with reference ${reference1} and a reason is listed as one "Manual · bank transfer / Payment received" row with its date, reference and recorder (${who}), the invoice shows Settled Rp 20.000 and Outstanding Rp 30.000, and the payment_events row holds manual_method bank_transfer, received_on ${today}, the reference, provider id manual:bank_transfer and recorded_by alice. Setup by in-page fetch: ${setup}; the invoice was made from a fulfilled order through the command API and issued on the native invoice screen`)

 // 3. A receipt above what is outstanding is refused with its sentence, and nothing is stored.
 const storedBefore=eventCount(),revisionBefore=invoiceRow(code1).revision
 await page.fill(t('invoice-receipt-amount'),'30001');await page.fill(t('invoice-receipt-reference'),'TOO-MUCH');await page.fill(t('invoice-receipt-reason'),'More than the invoice owes')
 await click('invoice-receipt-save');await until(()=>has(t('ops-refused')),'visible refusal of the overpayment')
 const overpay=(await page.text(t('ops-refused'))).trim();assert.equal(overpay,'Enter at most the outstanding amount of this invoice.')
 assert.equal(eventCount(),storedBefore);assert.equal(invoiceRow(code1).revision,revisionBefore);assert.equal(invoiceRow(code1).outstanding,30000);assert.equal((await paymentRows()).length,1)
 await shot('receipt-overpayment-refused.png','ops-refused')
 // The host holds the same line when the form's own check is bypassed: the same command through the API.
 const direct=await call('/erp/billing/commands',{tenant,mutation:{commandId:randomUUID(),type:'record_manual_receipt',invoiceId:inv1,expectedRevision:revisionBefore,amountMinor:30001,method:'bank_transfer',receivedOn:today,reference:'DIRECT-TOO-MUCH',reason:'More than the invoice owes'}})
 assert.notEqual(direct.status,200,q(direct));assert.ok(q(direct.body).includes('manual_receipt_exceeds_outstanding'),q(direct));assert.equal(eventCount(),storedBefore)
 pass('RECEIPT-native-overpayment-refused',`On the partially paid invoice an amount of 30001 against 30000 outstanding is refused on the native form with "${overpay}" and nothing is stored (still one payment event, the invoice at revision ${revisionBefore} with 30000 outstanding); the same command sent through the API is refused by the host with manual_receipt_exceeds_outstanding (HTTP ${direct.status}) and stores nothing`)

 // 2. The remainder in cash settles the invoice and the form goes away.
 const reference2=upper(`KW-${suffix}`),reason2='Counted at the counter'
 await select('invoice-receipt-method','cash');await page.fill(t('invoice-receipt-amount'),'30000');await page.fill(t('invoice-receipt-date'),today);await page.fill(t('invoice-receipt-reference'),reference2);await page.fill(t('invoice-receipt-reason'),reason2)
 await click('invoice-receipt-save');await until(()=>invoiceRow(code1).status==='settled','cash receipt settles the invoice')
 const settled=invoiceRow(code1);assert.deepEqual({settled:settled.settled,outstanding:settled.outstanding,status:settled.status},{settled:50000,outstanding:0,status:'settled'})
 await until(()=>has(t('invoice-receipt-form')).then(f=>!f),'receipt form gone from the settled invoice')
 const second=events(inv1)[1];assert.deepEqual({outcome:second.outcome,amount:second.amount,provider:second.provider,method:second.method,receivedOn:second.receivedOn,reference:second.reference,reason:second.reason},{outcome:'succeeded',amount:30000,provider:'manual:cash',method:'cash',receivedOn:today,reference:reference2,reason:reason2})
 await until(()=>paymentRows().then(r=>r.length===2),'two payment rows')
 assert.deepEqual((await paymentRows())[1],[today,'Manual · cash','Payment received','Rp 30.000',`${reference2} · ${reason2}`,who])
 await until(()=>pairs('[data-testid="invoice-detail"] dl.ops-totals').then(v=>v['Settled']==='Rp 50.000'&&v['Outstanding']==='Rp 0'),'settled totals');await shot('receipt-settled.png','invoice-payments-panel')
 pass('RECEIPT-native-cash-remainder-settles',`The remaining 30000 recorded as cash (reference ${reference2}) settles the invoice: status settled, settled 50000, outstanding 0, a second row "Manual · cash" listed, payment_events holds manual_method cash and provider id manual:cash, and the native receipt form is no longer rendered`)

 // 4. Reverse part of the first receipt.
 const reverse=async(needle,amount,reason)=>{
  await page.evaluate(`(()=>{const d=[...document.querySelectorAll('[data-testid="invoice-receipt-reversal"]')].find(n=>n.querySelector('summary').textContent.includes(${q(needle)}));if(!d)throw Error('no reversal form for '+${q(needle)});d.open=true;const a=d.querySelector('[data-testid="invoice-reversal-amount"]');a.value=${q(amount)};a.dispatchEvent(new Event('input',{bubbles:true}));const r=d.querySelector('[data-testid="invoice-reversal-reason"]');r.value=${q(reason)};r.dispatchEvent(new Event('input',{bubbles:true}));d.querySelector('[data-testid="invoice-reversal-save"]').click()})()`)
 }
 const summaries=await page.evaluate(`[...document.querySelectorAll('[data-testid="invoice-receipt-reversal"] summary')].map(n=>n.textContent)`)
 assert.deepEqual(summaries,[`Reverse the manual · bank transfer of ${today} · ${reference1}`,`Reverse the manual · cash of ${today} · ${reference2}`])
 assert.equal(await page.evaluate(`document.querySelector('[data-testid="invoice-receipt-reversal"] [data-testid="invoice-reversal-amount"]').value`),'20000','the reversal amount defaults to what is left of the receipt')
 const reasonR='Part of the transfer was returned by the bank'
 await reverse(reference1,'5000',reasonR);await until(()=>invoiceRow(code1).status==='issued','invoice reopened by the reversal')
 const reopened=invoiceRow(code1);assert.deepEqual({settled:reopened.settled,outstanding:reopened.outstanding,status:reopened.status},{settled:45000,outstanding:5000,status:'issued'})
 const all=events(inv1);assert.equal(all.length,3)
 assert.deepEqual({outcome:all[2].outcome,amount:all[2].amount,provider:all[2].provider,method:all[2].method,reason:all[2].reason,reverses:all[2].reverses,by:all[2].by},{outcome:'refunded',amount:5000,provider:'manual:bank_transfer',method:'bank_transfer',reason:reasonR,reverses:all[0].id,by:'alice'})
 await until(()=>paymentRows().then(r=>r.length===3),'three payment rows')
 const listed=await paymentRows();assert.deepEqual(listed.map(r=>[r[1],r[2],r[3]]),[['Manual · bank transfer','Payment received','Rp 20.000 · Rp 5.000 reversed'],['Manual · cash','Payment received','Rp 30.000'],['Manual · bank transfer','Reversal','Rp 5.000']])
 assert.deepEqual([listed[2][0],listed[2][4],listed[2][5]],[today,reasonR,who])
 await until(()=>pairs('[data-testid="invoice-detail"] dl.ops-totals').then(v=>v['Settled']==='Rp 45.000'&&v['Outstanding']==='Rp 5.000'),'reopened totals')
 await until(()=>has(t('invoice-receipt-form')),'receipt form back on the reopened invoice');assert.equal(await value(t('invoice-receipt-amount')),'5000')
 assert.deepEqual(await page.evaluate(`[...document.querySelectorAll('[data-testid="invoice-receipt-reversal"] summary')].map(n=>n.textContent)`),summaries,'both receipts still have something to reverse')
 await shot('receipt-reversed.png','invoice-payments-panel')
 pass('RECEIPT-native-partial-reversal-reopens',`Reversing 5000 of the bank transfer on the native reversal form (default 20000, the part left) reopens the settled invoice: status issued, settled 45000, outstanding 5000; the list shows the transfer as "Rp 20.000 · Rp 5.000 reversed" and a "Reversal" row of Rp 5.000 with the reason and recorder; payment_events holds an outcome refunded event of 5000 with reverses_event_id of the transfer; the receipt form is back with 5000 as the amount`)

 // 5. Accounting: the sources are captured, reviewed, drafted and posted natively; a clearing settlement matches the transfer to its bank row.
 const issueSource=await invoiceSource(inv1,'issue'),bankSource=await until(()=>sql(`SELECT s.id FROM archava_accounting.invoice_sources s WHERE s.book_id='${book}' AND s.invoice_id='${inv1}' AND s.effect='collection' AND s.amount_minor=2000000`),'captured transfer collection source'),cashSource=await until(()=>sql(`SELECT s.id FROM archava_accounting.invoice_sources s WHERE s.book_id='${book}' AND s.invoice_id='${inv1}' AND s.effect='collection' AND s.amount_minor=3000000`),'captured cash collection source'),reversalSource=await invoiceSource(inv1,'refund')
 assert.equal(issueSource.amount,5000000);assert.equal(reversalSource.amount,-500000)
 const sources=json(`SELECT jsonb_agg(jsonb_build_object('effect',effect,'amount',amount_minor,'date',source_date::text) ORDER BY amount_minor) FROM archava_accounting.invoice_sources WHERE book_id='${book}' AND invoice_id='${inv1}'`)
 assert.deepEqual(sources,[{effect:'refund',amount:-500000,date:today},{effect:'collection',amount:2000000,date:today},{effect:'collection',amount:3000000,date:today},{effect:'issue',amount:5000000,date:today}])
 assert.equal(sql(`SELECT (s.source_date::text=e.received_on::text)::text FROM archava_accounting.invoice_sources s JOIN archava_billing.commands c ON c.tenant_id=s.tenant_id AND c.request_id=s.id JOIN archava_billing.payment_events e ON e.id=(c.receipt->>'eventId')::uuid WHERE s.id='${bankSource}'`),'true','a collection source is dated on the receipt received-on date')
 const issueJournal=await post('invoice_source.draft',issueSource.id,'RCT-ISSUE',['AR','REVENUE'],[['AR',5000000,0],['REVENUE',0,5000000]])
 const bankJournal=await post('invoice_source.draft',bankSource,'RCT-BANK-COLLECTION',['AR','CLEARING'],[['CLEARING',2000000,0],['AR',0,2000000]])
 await post('invoice_source.draft',cashSource,'RCT-CASH-COLLECTION',['AR','CLEARING'],[['CLEARING',3000000,0],['AR',0,3000000]])
 await post('invoice_source.draft',reversalSource.id,'RCT-REVERSAL',['AR','CLEARING'],[['AR',500000,0],['CLEARING',0,500000]])
 pass('RECEIPT-native-sources-reviewed-and-posted',`The billing bridge captured four sources for the invoice (issue 5,000,000, collections of 2,000,000 and 3,000,000 dated ${today}, the received-on date of their receipts, and a refund of -500,000 for the reversal); each was reviewed, drafted and posted on the native source review and posts as debit CLEARING / credit AR for each collection, debit AR / credit CLEARING for the reversal and debit AR / credit REVENUE for the issue`)

 const ui=financialUi({page,origin,until,company})
 await ui.open();await ui.review('clearing_settlement.draft',{sourceId:bankSource,bankRowId:bankRow,code:'RCT-CLEARING','clearingAccount-accountId':accounts.CLEARING,'cashAccount-accountId':accounts.BANK,grossMinor:'20000.00',feeMinor:'0.00',reason:'Match the manual transfer to the bank statement row of the same amount'})
 assert.equal(await has(ui.t('operator-accounting-feeAccount-accountId')),false,'a zero fee needs no expense account')
 const summary=await page.text(ui.t('clearing-review-summary'));for(const part of ['Record the bank settlement of a manual receipt','Original manual receipt source','IDR 20000.00','IDR 0.00','TRANSFER-IN'])assert.ok(summary.includes(part),`${part} in ${summary}`)
 assert.ok((await page.text(ui.t('clearing-source-journal'))).includes('Original payment clearing of the manual receipt'))
 assert.equal(sql(`SELECT count(*) FROM archava_accounting.clearing_settlements WHERE book_id='${book}'`),'0','the review writes nothing');await shot('receipt-clearing-review.png','clearing-review-summary')
 await ui.confirm();const settlement=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='RCT-CLEARING'`);assert.match(settlement,/^[0-9a-f-]{36}$/)
 await ui.action('journal.post',{id:settlement,reason:'Post the reviewed manual receipt to the bank without repeating receivable or revenue'});await ui.action('bank_allocation.record',{bankRowId:bankRow,journalId:settlement,journalLineOrdinal:2,'account-accountId':accounts.BANK,amountMinor:'20000.00',reason:'Match the settlement cash leg to the actual bank row'})
 assert.deepEqual(lines(settlement),[['CLEARING',0,2000000],['BANK',2000000,0]])
 const proof=json(`SELECT jsonb_build_object('clearing',archava_accounting.clearing_balance('${tenant}','${bankSource}'),'unmatched',(SELECT sum(abs(archava_accounting.bank_row_remaining(tenant_id,id)))::text FROM archava_accounting.bank_rows WHERE book_id='${book}'),'settlements',(SELECT count(*) FROM archava_accounting.clearing_settlements WHERE book_id='${book}'))`)
 assert.equal(proof.unmatched,'0');assert.equal(proof.settlements,1);assert.ok([proof.clearing.availableGrossMinor,proof.clearing.unsettledGrossMinor].every(v=>v===undefined||v==='0'),q(proof.clearing))
 const balances=()=>json(`SELECT jsonb_object_agg(a.code,n.balance) FROM(SELECT l.account_id,sum(l.debit_minor-l.credit_minor)::text AS balance FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE j.book_id='${book}' AND j.state='posted' GROUP BY l.account_id)n JOIN archava_accounting.accounts a ON a.id=n.account_id`)
 assert.deepEqual(balances(),{AR:'500000',REVENUE:'-5000000',CLEARING:'2500000',BANK:'2000000'})
 pass('RECEIPT-native-clearing-settlement-to-bank',`On the native clearing review the transfer's collection source (worded as a manual receipt, gross IDR 20000.00, fee IDR 0.00) was settled against the imported bank row TRANSFER-IN: drafted, posted as credit CLEARING 2,000,000 / debit BANK 2,000,000 and matched on the cash leg; the transfer's clearing balance is 0 and the bank row has nothing unmatched; the ledger holds AR 500000, CLEARING 2500000 (the cash receipt less the reversal, not yet banked), BANK 2000000 and REVENUE -5000000`)

 await open('accounting',company);await until(()=>page.evaluate(`document.querySelector(${q(t('accounting-aging-run'))})?.disabled===false`),'receivable report controls')
 await select('accounting-aging-side','receivable');await page.fill(t('accounting-aging-date'),today);await click('accounting-aging-run')
 await until(()=>pairs(t('accounting-aging-totals')).then(v=>v['Net outstanding']==='IDR 5000.00'),'receivable net outstanding IDR 5000.00')
 assert.ok((await page.text(t('accounting-aging-control-status'))).includes('agree'));const controlRows=await rows('accounting-aging-controls')
 assert.deepEqual(controlRows,[['AR · AR','IDR 5000.00','IDR 5000.00','IDR 0.00']])
 const subledger=await ok(`/erp/accounting/subledger-report?tenant=${tenant}&bookId=${book}&side=receivable&asOf=${today}`);assert.equal(subledger.postedSourceControlsAgree,true);assert.ok(subledger.controls.every(c=>c.differenceMinor==='0'),q(subledger.controls));assert.equal(subledger.totals.netOutstandingMinor,'500000')
 await shot('receipt-receivable-control.png','accounting-aging-controls')
 pass('RECEIPT-native-receivable-control-zero',`The native receivable report as of ${today} shows Net outstanding IDR 5000.00 (the 5000 left on the invoice after the reversal) and its independent control comparison for AR reads original-source IDR 5000.00, ledger IDR 5000.00, difference IDR 0.00; the host report agrees (postedSourceControlsAgree, net 500000)`)

 // ===== Tax pack =====
 // 7. Tax codes from a template.
 await open('accounting',company);await until(()=>has(t('accounting-tax-templates')),'tax code template picker')
 const picker=t('accounting-tax-templates');await page.evaluate(`document.querySelector(${q(picker)}).open=true`)
 const templateOptions=await page.evaluate(`[...document.querySelector(${q(t('accounting-tax-template'))}).options].map(o=>o.value)`)
 assert.deepEqual(templateOptions,['','id-output-11','id-input-11','id-output-12-base-11-12','id-input-12-base-11-12'])
 const templateNotice=await page.text(t('accounting-tax-template-notice'));assert.ok(templateNotice.includes('Archava does not verify tax rates or rules')&&templateNotice.includes('Check every value with your own tax adviser')&&templateNotice.includes('only pre-fills the tax code form'),templateNotice)
 assert.equal(await page.evaluate(`document.querySelector(${q(t('accounting-tax-template-use'))}).disabled`),true,'nothing to apply before a template is chosen')
 assert.equal(json(`SELECT to_jsonb(count(*)) FROM archava_accounting.tax_codes WHERE book_id='${book}'`),0)
 await select('accounting-tax-template','id-output-11');await click('accounting-tax-template-use');await until(()=>has(t('accounting-tax-template-applied')),'template applied to the form')
 assert.ok((await page.text(t('accounting-tax-template-applied'))).includes('(not verified)'))
 await until(()=>has(t('operator-accounting-code')),'tax code form')
 const prefilled=await page.evaluate(`(()=>{const v=n=>document.querySelector('[data-testid="operator-accounting-'+n+'"]')?.value;return{action:document.querySelector('[data-testid="operator-action"]').value,code:v('code'),kind:v('kind'),rateNumerator:v('rate-numerator'),rateDenominator:v('rate-denominator'),baseNumerator:v('base-numerator'),baseDenominator:v('base-denominator'),book:v('bookId'),account:v('controlAccount-accountId'),from:v('effectiveFrom'),reason:v('reason')}})()`)
 assert.deepEqual(prefilled,{action:'tax_code.create',code:'VAT-OUT-11',kind:'output',rateNumerator:'11',rateDenominator:'100',baseNumerator:'1',baseDenominator:'1',book:'',account:'',from:'',reason:''})
 assert.equal(json(`SELECT to_jsonb(count(*)) FROM archava_accounting.tax_codes WHERE book_id='${book}'`),0,'choosing a template stores nothing')
 await shot('tax-template-prefilled.png','operator-accounting-code')
 await field('controlAccount-accountId',accounts['VAT-OUT']);await field('effectiveFrom','2020-01-01');await field('reason','Entered by the company from its own tax advice, starting from the template');await field('bookId',book)
 await reviewed('tax_code.create');await confirm()
 const codeRow=code=>json(`SELECT coalesce((SELECT jsonb_build_object('id',id,'state',state,'kind',kind,'rate',rate_numerator||'/'||rate_denominator,'base',base_numerator||'/'||base_denominator,'account',control_account_id,'from',effective_from::text) FROM archava_accounting.tax_codes WHERE book_id='${book}' AND code='${code}'),'null'::jsonb)`)
 const entered=codeRow('VAT-OUT-11');assert.deepEqual({state:entered.state,kind:entered.kind,rate:entered.rate,base:entered.base,account:entered.account,from:entered.from},{state:'entered',kind:'output',rate:'11/100',base:'1/1',account:accounts['VAT-OUT'],from:'2020-01-01'})
 await act('tax_code.confirm',{id:entered.id,reason:'Checked against the company tax advice'});assert.equal(codeRow('VAT-OUT-11').state,'confirmed')
 const panelRow=code=>page.evaluate(`(()=>{const table=document.querySelector(${q(t('accounting-tax-table'))});if(!table)return null;const heads=[...table.querySelectorAll('thead th')].map(c=>c.textContent);const tr=[...table.querySelectorAll('tbody tr')].find(r=>r.cells[0].textContent===${q(code+' · version 1')});return tr?Object.fromEntries([...tr.cells].map((c,i)=>[heads[i],c.textContent])):null})()`)
 const panel=await until(async()=>{const r=await panelRow('VAT-OUT-11');return r&&r.State==='Confirmed'?r:false},'the confirmed template code in the panel')
 assert.deepEqual({kind:panel.Kind,rate:panel.Rate,base:panel['Applies to'],account:panel['Control account'],by:panel['Entered by']},{kind:'Output (sales)',rate:'11%',base:'Whole price',account:'VAT-OUT · Output tax',by:'alice'})
 const standing='Entered by your company; Archava does not verify tax rates or rules.'
 assert.ok((await page.text(t('accounting-tax-notice'))).includes(standing));assert.ok((await page.text(t('accounting-tax-template-notice'))).includes('Archava does not verify tax rates or rules'))
 await noInternalWords('the Accounting screen with the tax code panel and the template picker')
 await shot('tax-template-code.png','accounting-tax-codes')
 pass('TAXPACK-native-template-tax-code','Choosing "Output tax on sales, 11% on the full price" in the Tax codes template picker (four Indonesian templates offered, apply disabled until one is chosen) pre-filled the native tax code form with code VAT-OUT-11, kind output, rate 11/100 and base 1/1 while the control account, first day and reason stayed empty and nothing was stored; the company picked the control account VAT-OUT, the first day 2020-01-01 and a reason, reviewed and saved it (stored as entered, 11/100 on 1/1), and confirmed it separately; the panel shows it as Confirmed, 11%, Whole price, with the standing notice "Entered by your company; Archava does not verify tax rates or rules." in the panel and the picker, and the page text names none of NPWP, PKP, PPN, e-Faktur or Coretax')

 // 8. The company states it is registered for value-added tax; a tax id that is not digits is refused.
 const registration=id=>json(`SELECT jsonb_build_object('vat',vat_registered,'revision',revision,'taxId',tax_id) FROM archava_organizations.companies WHERE id='${id}'`)
 await page.navigate(`${origin}/app/erp?module=organizations`);await until(()=>has(`${t('company-profile')} option[value="${company}"]`)&&has(`${t('company-profile')} option[value="${oddCompany}"]`),'both managed companies offered');await page.evaluate(`document.querySelector(${q(t('company-profile'))}).closest('details').open=true;document.querySelector(${q(t('company-tax-registration-panel'))}).open=true`)
 await select('company-profile',company);await until(()=>page.text(t('company-profile-current')).then(s=>s.includes(company)),'selected company profile')
 const before=registration(company);assert.deepEqual(before,{vat:false,revision:1,taxId:sellerTaxId})
 await until(()=>page.text(t('company-tax-registration')).then(s=>s===`Tax registration: tax id ${sellerTaxId} · no statement that the company is registered for value-added tax.`),'stored registration line before the statement')
 assert.ok((await page.text(t('company-tax-registration-notice'))).includes('Entered by your company; Archava does not verify tax ids or tax registration.'))
 assert.equal(await page.evaluate(`document.querySelector(${q(t('company-tax-registration-save'))}).disabled`),true,'nothing to save before the statement changes')
 await tick('company-vat-registered',true);await page.fill(t('company-tax-registration-reason'),'The company states its own value-added tax registration');await click('company-tax-registration-save')
 await until(()=>registration(company).vat===true,'registration statement stored');assert.deepEqual(registration(company),{vat:true,revision:2,taxId:sellerTaxId})
 await until(()=>page.text(t('company-tax-registration')).then(s=>s===`Tax registration: tax id ${sellerTaxId} · the company states it is registered for value-added tax.`),'stored registration line after the statement')
 await noInternalWords('the company screen');await shot('company-tax-registration.png','company-tax-registration-panel')
 await select('company-profile',oddCompany);await until(()=>page.text(t('company-tax-registration')).then(s=>s.includes(badTaxId)),'the second company registration line')
 const oddBefore=registration(oddCompany);assert.deepEqual(oddBefore,{vat:false,revision:1,taxId:badTaxId})
 await tick('company-vat-registered',true);await page.fill(t('company-tax-registration-reason'),'Try to state a registration with a tax id that is not digits');await click('company-tax-registration-save')
 await until(()=>has(t('ops-refused')),'visible refusal of the registration');const sentence='A company can be marked as registered for tax only while its tax id is digits with spaces, dots, slashes or hyphens. Correct the tax id on the source organization, refresh the company, then set the registration.'
 assert.equal((await page.text(t('ops-refused'))).trim(),sentence);assert.deepEqual(registration(oddCompany),oddBefore);await shot('company-tax-registration-refused.png','ops-refused')
 pass('TAXPACK-native-company-vat-statement',`On the native company profile the company whose source organization has tax id ${sellerTaxId} states it is registered for value-added tax (stored vat_registered true at revision 2, the registration line and notice read "Entered by your company; Archava does not verify tax ids or tax registration."); the second company with tax id ${badTaxId} is refused with "${sentence}" and stays unchanged at revision 1. Setup: both tax ids come from the source organizations created through the command API`)

 // 9. A taxed invoice for a buyer with a tax id; the reference is recorded, then replaced with a reason.
 const sell=async()=>{
  const before=new Set((await view('commerce',company).then(w=>w.orders??[])).map(x=>x.id))
  await open('sales',company);await until(()=>has(`${t('order-product')} option[value="${tea.id}"]`),'sales product')
  await page.fill(t('customer-party-search'),buyerName);await click('customer-party-find');await select('customer-party-select',party)
  await select('order-product',tea.id);await page.fill(t('order-quantity'),'1');await click('order-add-line');await select('order-tax-code','VAT-OUT-11');await click('order-save')
  const order=await until(async()=>(await view('commerce',company).then(w=>w.orders??[])).find(x=>!before.has(x.id)),'taxed sales order');assert.deepEqual({taxCode:order.taxCode,total:order.total},{taxCode:'VAT-OUT-11',total:40000})
  await openRow(order.id);await click('order-reserve');await click('order-fulfill');await until(async()=>(await view('commerce',company).then(w=>w.orders??[])).find(x=>x.id===order.id)?.state==='fulfilled','fulfilled taxed order');return order
 }
 const order=await sell(),code2=upper(`RCT-TAX-${suffix}`)
 await open('invoices',company);await select('invoice-source-order',order.id)
 await until(()=>page.evaluate(`[...document.querySelectorAll(${q(t('invoice-order-tax')+' dd')})].map(n=>n.textContent).join('|')`).then(v=>v==='Rp 40.000|Rp 4.400|Rp 44.400'),'invoice preview')
 await page.fill(t('invoice-order-code'),code2);await click('invoice-from-order-save');const inv2row=await until(()=>invoiceRow(code2),'taxed invoice '+code2);const inv2=inv2row.id
 assert.deepEqual({gross:inv2row.gross,tax:inv2row.tax,seller:inv2row.seller,buyer:inv2row.buyer,status:inv2row.status},{gross:44400,tax:4400,seller:sellerTaxId,buyer:buyerTaxId,status:'draft'})
 await openRow(inv2);await until(()=>has(t('invoice-tax-identifiers')),'tax ids on the draft invoice')
 assert.equal(await has(t('invoice-tax-reference-form')),false,'a draft invoice takes no tax invoice reference')
 await issue(inv2,'Issue the reviewed taxed invoice');await until(()=>invoiceRow(code2).status==='issued','taxed invoice issued')
 await until(()=>has(t('invoice-tax-reference-form')),'reference form on the issued taxed invoice')
 assert.equal(await page.text(t('invoice-seller-tax-id')),sellerTaxId);assert.equal(await page.text(t('invoice-buyer-tax-id')),buyerTaxId);assert.equal(await page.text(t('invoice-tax-reference')),'Not recorded')
 assert.ok((await page.text(t('invoice-tax-identifiers-notice'))).includes('Archava does not verify tax ids or tax invoice references'))
 const recordFirst={reference:'010.000-26.00000001',code:'01'},replaceWith={reference:'010.000-26.00000002',code:'02'},replaceReason='The first number was issued for the wrong period'
 await page.fill(t('invoice-tax-reference-input'),recordFirst.reference);await page.fill(t('invoice-tax-transaction-code'),recordFirst.code);await click('invoice-tax-reference-save')
 await until(()=>invoiceRow(code2).reference===recordFirst.reference,'tax invoice reference stored')
 await until(()=>page.text(t('invoice-tax-reference')).then(s=>s===`${recordFirst.reference} · transaction code ${recordFirst.code} · entry 1`),'shown reference after recording')
 let current=invoiceRow(code2);assert.deepEqual({reference:current.reference,transactionCode:current.transactionCode,version:current.referenceVersion,status:current.status,gross:current.gross},{reference:recordFirst.reference,transactionCode:recordFirst.code,version:1,status:'issued',gross:44400})
 await until(()=>page.text(t('invoice-tax-reference-save')).then(s=>s==='Replace tax invoice reference'),'the form now replaces')
 assert.equal(await has(t('invoice-tax-reference-reason')),true)
 await page.fill(t('invoice-tax-reference-input'),replaceWith.reference);await page.fill(t('invoice-tax-transaction-code'),replaceWith.code);await page.fill(t('invoice-tax-reference-reason'),replaceReason);await click('invoice-tax-reference-save')
 await until(()=>invoiceRow(code2).reference===replaceWith.reference,'tax invoice reference replaced')
 await until(()=>page.text(t('invoice-tax-reference')).then(s=>s===`${replaceWith.reference} · transaction code ${replaceWith.code} · entry 2`),'shown reference after replacing')
 current=invoiceRow(code2);assert.deepEqual({reference:current.reference,transactionCode:current.transactionCode,version:current.referenceVersion},{reference:replaceWith.reference,transactionCode:replaceWith.code,version:2})
 const audit=json(`SELECT jsonb_agg(jsonb_build_array(operation,reason) ORDER BY revision) FROM archava_billing.audit WHERE invoice_id='${inv2}' AND operation IN ('record_tax_reference','replace_tax_reference')`)
 assert.deepEqual(audit,[['record_tax_reference',`Recorded tax invoice reference "${recordFirst.reference}" (transaction code "${recordFirst.code}")`],['replace_tax_reference',`Replaced tax invoice reference "${recordFirst.reference}" (transaction code "${recordFirst.code}") with "${replaceWith.reference}" (transaction code "${replaceWith.code}"): ${replaceReason}`]])
 await noInternalWords('the taxed invoice');await shot('tax-invoice-identifiers.png','invoice-tax-identifiers')
 pass('TAXPACK-native-invoice-tax-ids-and-reference',`The taxed invoice ${code2} (net Rp 40.000, tax Rp 4.400) for the buyer with a tax id shows "Seller tax id ${sellerTaxId}" and "Buyer tax id ${buyerTaxId}" (stored on the invoice at creation) and no reference form while a draft; once issued, the native form recorded "${recordFirst.reference}" with transaction code ${recordFirst.code} (shown as entry 1, version 1, amounts and status unchanged) and then replaced it with "${replaceWith.reference}" / ${replaceWith.code} and a reason (entry 2, version 2); the billing audit trail holds the recorded value, and on the replacement the old value, the new value and the reason`)

 // 10. The tax report documents listing and its CSV download.
 const taxIssue=await invoiceSource(inv2,'issue');assert.deepEqual({amount:taxIssue.amount,tax:taxIssue.tax},{amount:4440000,tax:440000})
 await post('invoice_source.draft',taxIssue.id,'RCT-TAX-ISSUE',['AR','REVENUE','VAT-OUT'],[['AR',4440000,0],['REVENUE',0,4000000],['VAT-OUT',0,440000]])
 await open('accounting',company);await until(()=>page.evaluate(`document.querySelector(${q(t('accounting-taxreport-run'))})?.disabled===false`),'tax report controls')
 await page.fill(t('accounting-taxreport-from'),today);await page.fill(t('accounting-taxreport-to'),today);await click('accounting-taxdocs-run');await until(()=>has(t('accounting-taxdocs-result')),'documents listing')
 const documents=await rows('accounting-taxdocs-table')
 assert.deepEqual(documents,[[today,'Sales invoice',code2,buyerName,buyerTaxId,'VAT-OUT-11 · version 1 · 11%','IDR 40000.00','IDR 4400.00',`${replaceWith.reference} · code ${replaceWith.code}`]])
 const listingNotice=await page.text(t('accounting-taxdocs-notice'));assert.ok(listingNotice.length>0)
 const href=await page.evaluate(`document.querySelector(${q(t('accounting-taxdocs-csv'))}).getAttribute('href')`);assert.equal(href,`/erp/accounting/tax-report/documents.csv?tenant=${tenant}&bookId=${book}&from=${today}&to=${today}`)
 const csv=await page.evaluate(`fetch(${q(href)}).then(async r=>({status:r.status,type:r.headers.get('content-type'),disposition:r.headers.get('content-disposition'),text:await r.text()}))`)
 assert.equal(csv.status,200);assert.equal(csv.type,'text/csv; charset=utf-8');assert.ok(csv.disposition.includes(`filename="tax-documents-${bookCode}-${today}-to-${today}.csv"`),csv.disposition)
 const disclaimer="\"Draft export of data your company entered in Archava. It is not a tax return and is not in any tax authority's import format. Archava does not verify tax codes, rates, tax ids or tax invoice references.\""
 const header='Side,Document type,Effect,Document code,Date,Counterparty,Counterparty tax id,Company tax id,Tax code,Tax code version,Rate,Base,Taxable base,Tax,Currency,Tax invoice reference,Transaction code'
 const dataRow=['output','invoice','issue',code2,today,buyerName,buyerTaxId,sellerTaxId,'VAT-OUT-11','1','11/100','1/1','40000.00','4400.00','IDR',replaceWith.reference,replaceWith.code].join(',')
 assert.equal(csv.text,`${disclaimer}\r\n${header}\r\n${dataRow}\r\n`)
 const api=await ok(`/erp/accounting/tax-report/documents?tenant=${tenant}&bookId=${book}&from=${today}&to=${today}`);assert.equal(api.documents.length,1);assert.deepEqual({tax:api.documents[0].taxMinor,base:api.documents[0].taxableBaseMinor,reference:api.documents[0].taxReference},{tax:'440000',base:'4000000',reference:replaceWith.reference})
 await shot('tax-documents.png','accounting-taxdocs-result')
 pass('TAXPACK-native-documents-listing-and-csv',`After the taxed invoice's source was reviewed, drafted and posted natively (debit AR 4,440,000, credit REVENUE 4,000,000, credit VAT-OUT 440,000), the Tax report documents listing for ${today} shows exactly one row: Sales invoice ${code2}, ${buyerName}, buyer tax id ${buyerTaxId}, VAT-OUT-11 version 1 at 11%, taxable IDR 40000.00, tax IDR 4400.00 and the current reference ${replaceWith.reference} · code ${replaceWith.code}; the CSV link's href, fetched from the page, answers 200 as text/csv; charset=utf-8 with the file name tax-documents-${bookCode}-${today}-to-${today}.csv and holds exactly the draft/disclaimer line, the header row and the data row of that invoice (CRLF line ends), and the host listing agrees`)

 // 6. A person without billing.settle sees the payments but no receipt or reversal form.
 const bobGrant=json(`SELECT coalesce((SELECT to_jsonb(permissions) FROM archava_billing.memberships WHERE tenant_id='${tenant}' AND principal_id='bob'),'null'::jsonb)`)
 const login=async user=>{await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await page.navigate(`${origin}/app`);await page.fill(t('login-username'),`${user}@archava.test`);await page.fill(t('login-password'),'correct horse battery staple');await page.fill(t('login-tenant'),tenant);await page.click(t('login-submit'));await until(()=>page.evaluate(`location.pathname==='/app/contacts'&&!!document.querySelector('${t('contacts-count')}')`),`${user} login`)}
 sql(`INSERT INTO archava_billing.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','bob','human',ARRAY['billing.read'],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY['billing.read'],active=true`)
 for(const[actor,permissions]of [['bob',['organizations.read','companies.read']]])sql(`INSERT INTO archava_organizations.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','${actor}','human',ARRAY[${permissions.map(p=>`'${p}'`).join(',')}],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY(SELECT DISTINCT p FROM unnest(m.permissions||EXCLUDED.permissions) p ORDER BY p)`)
 let reader
 try{
  await login('bob');await open('invoices',company);await openRow(inv1);await until(()=>has(t('invoice-payments-panel')),'payments panel for the reader')
  await until(async()=>(await paymentRows()).length===3,'the reader sees all three payments')
  reader={rows:(await paymentRows()).length,forms:await page.evaluate(`['invoice-receipt-form','invoice-receipt-save','invoice-receipt-reversal','invoice-reversal-save'].filter(id=>document.querySelector('[data-testid="'+id+'"]')).length`)}
  assert.deepEqual(reader,{rows:3,forms:0});await shot('receipt-reader-no-forms.png','invoice-payments-panel')
  pass('RECEIPT-native-reader-sees-no-forms','A signed-in person holding billing.read without billing.settle opens the invoice and sees the payments list (three rows: two receipts and the reversal) but no "Record a payment received" form and no reversal form, although the invoice is issued with 5000 outstanding and both receipts have something left to reverse')
 }finally{
  if(bobGrant===null)sql(`DELETE FROM archava_billing.memberships WHERE tenant_id='${tenant}' AND principal_id='bob'`);else sql(`UPDATE archava_billing.memberships SET permissions=ARRAY[${bobGrant.map(p=>`'${p}'`).join(',')}]::text[] WHERE tenant_id='${tenant}' AND principal_id='bob'`)
  await login('alice')
 }
 assert.deepEqual(page.errors,[],'Unexpected JavaScript/WASM errors in the receipts and tax pack journey')
 return{scope:'Owned synthetic IDR company with a single-operator book on the native screens: manual bank-transfer and cash receipts on an invoice, an overpayment refusal, a partial reversal, the collection and refund sources reviewed and posted, a clearing settlement to an imported bank row and the receivable control comparison; the tax code template, the company VAT statement and its refusal, the tax ids and the tax invoice reference on a taxed invoice, and the tax report documents listing with its CSV. The company, parties, book, accounts, period, policy, products, bank account and statement row, and the orders and draft invoices, are created through the command API. Not covered in the browser: a book with independent posting, a receipt dated before today (an invoice cannot be issued earlier), a receipt in a closed period, a manual receipt on an invoice with an open payment link, a non-IDR receipt, and the CSV of a period with several documents or quoting (host tests cover these).',company,book,invoice:inv1,taxedInvoice:inv2,bankSource,settlement,reader}
}
