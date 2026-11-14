/** Tax on documents for one IDR company, from the company's tax codes to the tax report, on the native screens. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
export async function verifyTax({page,origin,facts,until,pass,suffix,emailProvider}){
 assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(suffix,/^[0-9a-f]{8}$/)
 const tenant='acme-retail',t=id=>`[data-testid="${id}"]`,q=JSON.stringify,upper=s=>s.toUpperCase()
 const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='30s';SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:35000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
 const call=(path,body)=>page.evaluate(`fetch(${q(path)},{method:${q(body?'POST':'GET')},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body?`,body:${q(q(body))}`:''}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const ok=async(path,body)=>{const r=await call(path,body);assert.equal(r.status,200,q(r));return r.body.receipt??r.body}
 const org=(type,data)=>ok('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}})
 const command=(domain,command)=>ok(`/erp/${domain}/commands`,{tenant,requestId:randomUUID(),command})
 const view=(domain,company)=>page.evaluate(`fetch('/erp/${domain}/workspace?tenant=${tenant}&companyId=${company}').then(r=>r.json())`)
 const has=selector=>page.evaluate(`!!document.querySelector(${q(selector)})`)
 const texts=selector=>page.evaluate(`[...document.querySelectorAll(${q(selector)})].map(n=>n.textContent)`)
 const select=async(id,wanted)=>{
  await until(()=>has(t(id)),id);await page.evaluate(`document.querySelector(${q(t(id))}).focus()`)
  await until(()=>page.evaluate(`!![...document.querySelector(${q(t(id))}).options].find(o=>o.value===${q(String(wanted))})`),`${id} offers ${wanted}`)
  await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});n.value=${q(String(wanted))};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 }
 const optionText=(id,wanted)=>page.evaluate(`[...document.querySelector(${q(t(id))}).options].find(o=>o.value===${q(wanted)})?.textContent??''`)
 const tick=(id,wanted)=>page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});n.checked=${!!wanted};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 const click=async id=>{await until(()=>page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});return !!n&&!n.disabled&&n.getClientRects().length>0&&!n.closest('fieldset')?.disabled;})()`),'enabled native '+id);await page.click(t(id))}
 const submitOf=id=>page.evaluate(`document.querySelector(${q(t(id))}).closest('form').querySelector('button[type="submit"]').click()`)
 const open=async(module,company)=>{await page.navigate(`${origin}/app/erp?module=${module}`);await scope(company)}
 // The picker ignores a change while its workspace is loading, so choose until the reviewed scope names the company.
 const scope=async company=>{await until(()=>has(`${t('trade-company-select')} option[value="${company}"]`),'company scope option');await until(async()=>{if((await page.text(t('trade-company-selected'))).includes(company))return true;await page.evaluate(`(()=>{const n=document.querySelector(${q(t('trade-company-select'))});n.value=${q(company)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`);return false},'selected company scope')}
 const openRow=async id=>{await until(()=>has(t('ops-open-'+id)),'record row '+id);await page.click(t('ops-open-'+id))}
 const shot=async(file,id)=>{await page.evaluate(`document.querySelector(${q(t(id))}).scrollIntoView({block:'center',behavior:'instant'})`);await page.evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');await page.screenshot(file)}
 // Native Accounting action form: fill, review, confirm.
 const field=async(name,wanted)=>{
  const id=`operator-accounting-${name}`;await until(()=>has(t(id)),id+' rendered')
  const kind=await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});return n.tagName==='SELECT'?'select':n.type})()`)
  if(kind==='select')await select(id,wanted)
  else if(kind==='checkbox')await tick(id,wanted)
  else await page.fill(t(id),String(wanted))
 }
 const choose=async action=>{await select('operator-action',action);await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current financial grant')}
 const review=async(action,fields,{extraAccount=false}={})=>{await choose(action);if(extraAccount)await page.click(t('operator-accounting-accounts')+' > button');for(const[k,v]of Object.entries(fields))await field(k,v);await page.click(t('module-accounting-create'));await until(async()=>{if(await has(t('ops-refused')))throw Error(await page.text(t('ops-feedback')));return has(t('operator-review'))},action+' review')}
 const confirm=async()=>{await page.click(t('operator-confirm'));await until(async()=>{const text=await page.text(t('ops-feedback'));if(await has(t('ops-refused'))||await has(t('ops-uncertain')))throw Error(text);return text.includes('Saved.')},'matching financial receipt')}
 const act=async(action,fields)=>{await review(action,fields);await confirm()}
 const one=query=>sql(query),json=query=>JSON.parse(sql(query))

 // Standalone browser fixture only, as in the other financial journeys: the operator may manage organizations, companies and parties.
 sql(`INSERT INTO archava_organizations.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write','parties.read','parties.write'],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY(SELECT DISTINCT p FROM unnest(m.permissions||EXCLUDED.permissions) p ORDER BY p)`)
 // Setup the screens under test do not own, through the command API: the company, its trading partner, the book with
 // single-operator posting, the ledger accounts, the period, the two posting policies and two products.
 const o=await org('create_organization',{name:`Tax company ${suffix}`,legalName:`Tax Company Official ${suffix}`})
 const company=(await org('company.create_from_organization',{code:upper(`TAX-${suffix}`),organizationId:o.organizationId,expectedSourceRevision:1,reason:'Owned tax acceptance company'})).companyId
 const partnerName=`Tax partner ${suffix}`,partner=await org('create_organization',{name:partnerName,legalName:`Tax Partner Official ${suffix}`})
 const party=(await org('party.create_from_organization',{organizationId:partner.organizationId,expectedSourceRevision:1,customer:true,supplier:true})).partyId
 const supplier=await command('purchasing',{operation:'supplier.create-from-party',code:upper(`TAX-SUP-${suffix}`),partyId:party,expectedPartyRevision:1})
 const bookCode=upper(`TAX-BOOK-${suffix}`)
 const book=(await command('accounting',{type:'book.create',companyId:company,expectedCompanyRevision:1,code:bookCode,name:`Tax book ${suffix}`,currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator IDR book for the tax acceptance'})).bookId
 assert.match(book,/^[0-9a-f-]{36}$/)
 const accounts={};for(const[code,cls,cash]of [['AR','asset','operating'],['REVENUE','revenue','operating'],['CLEARING','asset','none'],['INVENTORY','asset','operating'],['GRNI','liability','operating'],['PAYABLE','liability','operating'],['LANDED_FEE','liability','operating'],['VAT-OUT','liability','operating'],['VAT-IN','asset','operating']])accounts[code]=(await command('accounting',{type:'account.create',bookId:book,code,name:code==='VAT-OUT'?'Output tax':code==='VAT-IN'?'Input tax':code,class:cls,cashRole:cash,reason:'Reviewed tax acceptance account'})).resourceId
 await command('accounting',{type:'period.open',bookId:book,code:'TAX-SOURCE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned synthetic source period'})
 const mapped=pairs=>pairs.map(([role,code])=>({role,accountId:accounts[code],expectedAccountRevision:1}))
 await command('accounting',{type:'invoice_policy.set',bookId:book,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:mapped([['receivable','AR'],['revenue','REVENUE'],['payment_clearing','CLEARING']]),reason:'Whole Rupiah documents post as IDR book units'})
 await command('accounting',{type:'purchase_policy.set',bookId:book,expectedRevision:0,currencyCatalogue:'SIX-2026-09-17',sourceUnit:'whole_idr',mappings:mapped([['inventory','INVENTORY'],['goods_received_not_invoiced','GRNI'],['payable','PAYABLE'],['landed_fee_accrual','LANDED_FEE']]),reason:'Whole Rupiah documents post as IDR book units'})
 const teaTitle=`Taxed tea ${suffix}`
 const tea=await command('commerce',{operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:upper(`TAX-TEA-${suffix}`),title:teaTitle,unit:'unit',trackStock:false,price:15000})
 const leaf=await command('commerce',{operation:'product.create_for_company',companyId:company,expectedCompanyRevision:1,sku:upper(`TAX-LEAF-${suffix}`),title:`Bought leaf ${suffix}`,unit:'unit',trackStock:true,price:9000})
 const today=sql("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
 const setup='the company, parties, supplier, single-operator IDR book, ledger accounts, period, the invoice and purchase posting policies and the two products were created through the command API'

 // 1. Tax codes: entered and confirmed on the native Accounting forms; the panel lists them.
 const codeRow=code=>json(`SELECT coalesce((SELECT jsonb_build_object('id',id,'state',state,'kind',kind,'rate',rate_numerator||'/'||rate_denominator,'base',base_numerator||'/'||base_denominator,'account',control_account_id,'revision',revision) FROM archava_accounting.tax_codes WHERE book_id='${book}' AND code='${code}'),'null'::jsonb)`)
 const enter=async(code,kind,rate,base,account)=>{
  await act('tax_code.create',{bookId:book,code,kind,'rate-numerator':rate[0],'rate-denominator':rate[1],'base-numerator':base[0],'base-denominator':base[1],'controlAccount-accountId':accounts[account],effectiveFrom:'2020-01-01',reason:'Entered by the company from its own tax advice'})
  const row=codeRow(code);assert.deepEqual({state:row.state,kind:row.kind,rate:row.rate,base:row.base,account:row.account},{state:'entered',kind,rate:rate.join('/'),base:base.join('/'),account:accounts[account]});return row
 }
 const confirmCode=async code=>{await act('tax_code.confirm',{id:codeRow(code).id,reason:'Checked against the company tax advice'});assert.equal(codeRow(code).state,'confirmed')}
 // The panel row of one code: its cells by column heading.
 const panelRow=code=>page.evaluate(`(()=>{const table=document.querySelector(${q(t('accounting-tax-table'))});if(!table)return null;const heads=[...table.querySelectorAll('thead th')].map(c=>c.textContent);const tr=[...table.querySelectorAll('tbody tr')].find(r=>r.cells[0].textContent===${q(code+' · version 1')});return tr?Object.fromEntries([...tr.cells].map((c,i)=>[heads[i],c.textContent])):null})()`)
 const shown=async(code,wanted)=>until(async()=>{const row=await panelRow(code);return row&&Object.entries(wanted).every(([k,v])=>row[k]===v)?row:false},`tax code ${code} shown as ${q(wanted)}`)
 await open('accounting',company)
 await enter('VAT11','output',[11,100],[1,1],'VAT-OUT')
 await shown('VAT11',{Kind:'Output (sales)',Rate:'11%','Applies to':'Whole price','Control account':'VAT-OUT · Output tax',State:'Entered — not confirmed, does not apply','Confirmed by':'Not yet'})
 await confirmCode('VAT11');await enter('VATIN','input',[11,100],[1,1],'VAT-IN');await confirmCode('VATIN')
 const out=await shown('VAT11',{Rate:'11%',State:'Confirmed','Entered by':'alice','Confirmed by':'alice'}),inn=await shown('VATIN',{Kind:'Input (purchases)',Rate:'11%','Control account':'VAT-IN · Input tax',State:'Confirmed'})
 assert.ok(Object.values(out).includes('Applies')&&Object.values(inn).includes('Applies'),q([out,inn]))
 const notice=await page.text(t('accounting-tax-notice'));assert.ok(notice.includes('Entered by your company; Archava does not verify tax rates or rules.'),notice);assert.ok(!/not posted to the ledger yet/.test(notice),notice)
 await shot('tax-codes.png','accounting-tax-codes')
 pass('TAX-native-tax-codes',`The native Accounting forms entered and confirmed an output code VAT11 (11/100, control account VAT-OUT) and an input code VATIN (11/100, VAT-IN); the Tax codes panel shows each first as entered and not applying, then as Confirmed with 11% on the whole price, with the notice that the company entered them and Archava does not verify them. Setup by in-page fetch: ${setup}`)

 // 2. A second output code whose rate applies to eleven twelfths of the price.
 await enter('VAT12','output',[12,100],[11,12],'VAT-OUT')
 await shown('VAT12',{Rate:'12%','Applies to':'11/12 of the price',State:'Entered — not confirmed, does not apply'})
 await confirmCode('VAT12');await shown('VAT12',{Rate:'12%','Applies to':'11/12 of the price',State:'Confirmed'})
 pass('TAX-native-base-fraction-code','The native form entered and confirmed VAT12 as 12/100 on a base of 11/12; the panel shows "12%" applying to "11/12 of the price", stored as 12/100 and 11/12')

 // 3. Sales: a company order with the output code on the native Sales form, exclusive basis.
 const orders=()=>view('commerce',company).then(w=>w.orders??[])
 const sell=async(code,inclusive,quantity)=>{
  const before=new Set((await orders()).map(o=>o.id))
  await open('sales',company);await until(()=>has(`${t('order-product')} option[value="${tea.id}"]`),'sales product')
  await page.fill(t('customer-party-search'),partnerName);await click('customer-party-find');await select('customer-party-select',party)
  await select('order-product',tea.id);await page.fill(t('order-quantity'),String(quantity));await click('order-add-line')
  await select('order-tax-code',code);assert.equal(await page.evaluate(`document.querySelector(${q(t('order-price-includes-tax'))}).checked`),false,'the price basis starts as exclusive')
  if(inclusive)await tick('order-price-includes-tax',true)
  await click('order-save')
  const order=await until(async()=>(await orders()).find(o=>!before.has(o.id)),'taxed sales order');assert.deepEqual({taxCode:order.taxCode,priceIncludesTax:order.priceIncludesTax,total:order.total},{taxCode:code,priceIncludesTax:inclusive,total:quantity*15000})
  await openRow(order.id);const hint=await until(()=>has(t('order-tax')).then(found=>found&&page.text(t('order-tax'))),'order tax hint')
  assert.ok(hint.startsWith(`Tax code ${code} · ${inclusive?'prices include the tax':'tax is added to the prices on the invoice'} · the rate is taken when the order is invoiced.`),hint)
  await click('order-reserve');await click('order-fulfill');await until(async()=>(await orders()).find(x=>x.id===order.id)?.state==='fulfilled','fulfilled taxed order')
  return order
 }
 const invoiceRow=code=>json(`SELECT coalesce((SELECT jsonb_build_object('id',id,'gross',gross_minor,'tax',tax_minor,'correction',correction_minor,'correctionTax',correction_tax_minor,'outstanding',outstanding_minor,'status',status,'code',tax_code,'version',tax_code_version,'inclusive',price_includes_tax,'revision',revision) FROM archava_billing.invoices WHERE invoice_code='${code}'),'null'::jsonb)`)
 // The invoice-from-order preview and the draft invoice made from it.
 const invoiceFrom=async(order,code,preview)=>{
  await open('invoices',company);await select('invoice-source-order',order.id)
  await until(()=>texts(`${t('invoice-order-tax')} dd`).then(v=>v.join('|')===preview.join('|')),`invoice preview ${preview.join(' / ')}`)
  const label=await page.evaluate(`document.querySelectorAll(${q(t('invoice-order-tax')+' dt')})[1].textContent`)
  await page.fill(t('invoice-order-code'),code);await click('invoice-from-order-save')
  const invoice=await until(()=>invoiceRow(code),'taxed invoice '+code);return{invoice,label}
 }
 const taxBlock=()=>Promise.all(['invoice-tax-net','invoice-tax-amount','invoice-tax-total'].map(id=>page.text(t(id))))
 const first=await sell('VAT11',false,2);await shot('tax-order.png','sales-detail')
 const code1=upper(`TAX-INV-A-${suffix}`),made=await invoiceFrom(first,code1,['Rp 30.000','Rp 3.300','Rp 33.300'])
 assert.equal(made.label,'Tax · VAT11 11% · added to the prices');const inv1=made.invoice
 assert.deepEqual({gross:inv1.gross,tax:inv1.tax,code:inv1.code,version:inv1.version,inclusive:inv1.inclusive,status:inv1.status},{gross:33300,tax:3300,code:'VAT11',version:1,inclusive:false,status:'draft'})
 await openRow(inv1.id);await until(()=>taxBlock().then(v=>v.join('|')==='Rp 30.000|Rp 3.300|Rp 33.300'),'invoice tax block')
 assert.ok((await page.text(t('invoice-tax-notice'))).includes('Archava does not verify tax rates or rules'));await shot('tax-invoice.png','invoice-detail')
 pass('TAX-native-exclusive-sale','The native Sales form offered VAT11 with the price basis unticked; the order of 2 × 15,000 stores the code and an exclusive basis at 30,000 and shows its tax hint; after reserve and fulfil the invoice-from-order preview shows net Rp 30.000, tax Rp 3.300, total Rp 33.300 and the draft invoice stores gross 33300 with tax 3300 under VAT11 version 1, shown in its tax block')

 // 4. Issue: the bridge captures gross and tax; the source review shows the split and the third leg; draft and post natively.
 const invoiceSource=(invoice,effect)=>until(()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'amount',amount_minor,'tax',tax_minor,'account',tax_account_id,'accountRevision',tax_account_revision)::text FROM archava_accounting.invoice_sources WHERE book_id='${book}' AND invoice_id='${invoice}' AND effect='${effect}'),'')`),`captured invoice ${effect}`).then(JSON.parse)
 const issueInvoice=async(invoice,reason)=>{await page.fill(t('invoice-reason'),reason);await click('invoice-issue')}
 await issueInvoice(inv1,'Issue the reviewed taxed invoice');const issue=await invoiceSource(inv1.id,'issue')
 assert.deepEqual({amount:issue.amount,tax:issue.tax,account:issue.account,accountRevision:issue.accountRevision},{amount:3330000,tax:330000,account:accounts['VAT-OUT'],accountRevision:1})
 await until(()=>has(t('invoice-correction')),'issued taxed invoice')
 // The owned mail of the issued invoice prints net, tax and total.
 const sentBefore=emailProvider.state.sends;await page.fill(t('email-recipient'),`tax-${suffix}@archava.test`);await page.fill(t('email-subject'),`Taxed invoice ${suffix}`);await click('email-send-button')
 await until(()=>page.text(t('email-notice')).then(s=>/Email (status|outcome confirmed): accepted/.test(s)),'taxed invoice mail accepted');assert.equal(emailProvider.state.sends,sentBefore+1)
 const mail=emailProvider.state.bodies.at(-1)
 assert.ok(mail.text.includes('\nNet: 30000 IDR\nTax (VAT11 11%, added to the prices): 3300 IDR\nTotal: 33300 IDR\nTax codes are entered by the company; Archava does not verify tax rates or rules.\n'),mail.text)
 assert.ok(mail.html.includes('<li>Net: 30000 IDR</li><li>Tax (VAT11 11%, added to the prices): 3300 IDR</li><li>Total: <strong>33300 IDR</strong></li>'),mail.html)
 pass('TAX-native-invoice-mail','The owned mail of the issued taxed invoice, sent from the native invoice screen, prints "Net: 30000 IDR", "Tax (VAT11 11%, added to the prices): 3300 IDR" and "Total: 33300 IDR" with the notice that Archava does not verify tax rates or rules, in text and HTML')
 const journals={}
 const lines=journal=>json(`SELECT jsonb_agg(jsonb_build_array(account_code,debit_minor,credit_minor) ORDER BY ordinal) FROM archava_accounting.journal_lines WHERE journal_id='${journal}'`)
 // One native source review: the summary amounts by test id, the proposed journal naming every account, then draft and post.
 const post=async(action,source,code,roles,summary,expected,posted)=>{
  await open('accounting',company);const fields={sourceId:source,code,reason:'Review the captured amount, its tax and the account roles'};roles.forEach((role,i)=>fields[`accounts-${i}-accountId`]=accounts[role]);await review(action,fields,{extraAccount:roles.length===3})
  for(const[id,amount]of Object.entries(expected))assert.equal(await page.text(t(id)),amount,`${code} ${id}`)
  const reviewText=await page.text(t('operator-review'));for(const role of roles)assert.ok(reviewText.includes(role),`${code}: ${role} missing in the proposed journal: ${reviewText}`)
  if(summary)await shot(summary,'operator-review')
  await confirm();const journal=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book}' AND code='${code}'`);assert.match(journal,/^[0-9a-f-]{36}$/)
  await act('journal.post',{id:journal,reason:'Post the separately reviewed taxed source'});assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`),'posted');journals[code]=journal
  assert.deepEqual(lines(journal),posted,code);return journal
 }
 await post('invoice_source.draft',issue.id,'TAX-ISSUE',['AR','REVENUE','VAT-OUT'],'tax-invoice-source-review.png',{'invoice-source-net':'IDR 30000.00','invoice-source-tax':'IDR 3300.00'},[['AR',3330000,0],['REVENUE',0,3000000],['VAT-OUT',0,330000]])
 pass('TAX-native-issue-and-source-posting','Issuing on the native invoice screen captured a source of 3,330,000 with tax 330,000 on VAT-OUT; the native source review shows net IDR 30000.00 and tax IDR 3300.00 and names AR, REVENUE and VAT-OUT; drafted and posted natively as debit AR 3,330,000, credit REVENUE 3,000,000, credit VAT-OUT 330,000')

 // 5. An inclusive order: the total is the entered price and the tax is extracted.
 const second=await sell('VAT11',true,2)
 const code2=upper(`TAX-INV-B-${suffix}`),inclusive=await invoiceFrom(second,code2,['Rp 27.027','Rp 2.973','Rp 30.000'])
 assert.equal(inclusive.label,'Tax · VAT11 11% · included in the prices');const inv2=inclusive.invoice
 assert.deepEqual({gross:inv2.gross,tax:inv2.tax,inclusive:inv2.inclusive,status:inv2.status},{gross:30000,tax:2973,inclusive:true,status:'draft'})
 await openRow(inv2.id);await until(()=>taxBlock().then(v=>v.join('|')==='Rp 27.027|Rp 2.973|Rp 30.000'),'inclusive invoice tax block')
 pass('TAX-native-inclusive-order','With "Product prices already include this tax" ticked on the native Sales form, the order of 2 × 15,000 invoices at a total of Rp 30.000, the entered price, with net Rp 27.027 and extracted tax Rp 2.973; the draft invoice stores gross 30000 and tax 2973')

 // 6. A correction of the taxed invoice is split with the invoice's rate; its source posts three legs.
 await openRow(inv1.id);await until(()=>has(t('invoice-correction')),'correction form of the issued invoice')
 await page.fill(t('invoice-reason'),'Reviewed customer credit');await page.fill(t('invoice-correction'),'-11100');await submitOf('invoice-correction')
 const correction=await invoiceSource(inv1.id,'correction');assert.deepEqual({amount:correction.amount,tax:correction.tax,account:correction.account},{amount:-1110000,tax:-110000,account:accounts['VAT-OUT']})
 await until(()=>taxBlock().then(v=>v.join('|')==='Rp 20.000|Rp 2.200|Rp 22.200'),'tax block after the credit')
 const corrected=invoiceRow(code1);assert.deepEqual({correction:corrected.correction,correctionTax:corrected.correctionTax,outstanding:corrected.outstanding},{correction:-11100,correctionTax:-1100,outstanding:22200})
 await shot('tax-invoice-credit.png','invoice-detail')
 await post('invoice_source.draft',correction.id,'TAX-CREDIT',['AR','REVENUE','VAT-OUT'],null,{'invoice-source-net':'IDR -10000.00','invoice-source-tax':'IDR -1100.00'},[['REVENUE',1000000,0],['VAT-OUT',110000,0],['AR',0,1110000]])
 pass('TAX-native-correction-split','The native correction of -11100 on the taxed invoice is stored with a tax part of -1100; the invoice shows net Rp 20.000, tax Rp 2.200, total Rp 22.200; its source review shows net IDR -10000.00 and tax IDR -1100.00 and posts debit REVENUE 1,000,000 and debit VAT-OUT 110,000 against credit AR 1,110,000')

 // 7. Purchases: supplier price, order, receipt and a supplier claim with the input code and the supplier's printed tax.
 await open('purchasing',company);await until(()=>has(`${t('offer-product')} option[value="${leaf.id}"]`),'purchasing catalog');await until(()=>has(`${t('offer-supplier')} option[value="${supplier.id}"]`),'supplier projection')
 await page.evaluate(`document.querySelector(${q(t('offer-save'))}).closest('details').open=true`);await select('offer-supplier',supplier.id);await select('offer-product',leaf.id)
 await page.fill(t('offer-cost'),'7000');await click('offer-save')
 const offer=await until(async()=>(await view('purchasing',company)).offers?.find(x=>x.productId===leaf.id),'supplier price');assert.equal(offer.unitCost,7000)
 await select('purchase-supplier',supplier.id);await until(()=>has(`${t('purchase-offer')} option[value="${offer.id}"]`),'offer in the picker')
 await select('purchase-offer',offer.id);await page.fill(t('purchase-quantity'),'20');await click('purchase-add-line');await click('purchase-save')
 const po=await until(async()=>(await view('purchasing',company)).orders?.find(x=>x.supplierId===supplier.id),'purchase order');assert.equal(po.total,140000)
 await openRow(po.id);await page.fill(t('purchase-reason'),'Reviewed delivery under the purchase policy');await click('purchase-confirm');await until(()=>has(t('purchase-receive')),'confirmed purchase')
 await page.fill(t('receive-accepted'),'20');await page.fill(t('receive-landed-fee'),'0');await click('purchase-receive')
 await until(async()=>(await view('purchasing',company)).orders.find(x=>x.id===po.id)?.state==='received','goods receipt')
 const purchaseSource=effect=>until(()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'goods',goods_minor,'fee',landed_fee_minor,'tax',tax_minor,'account',tax_account_id)::text FROM archava_accounting.purchase_sources WHERE book_id='${book}' AND order_id='${po.id}' AND effect='${effect}'),'')`),`captured purchase ${effect}`).then(JSON.parse)
 const receipt=await purchaseSource('receipt');assert.deepEqual({goods:receipt.goods,fee:receipt.fee,tax:receipt.tax},{goods:14000000,fee:0,tax:0})
 await until(()=>has(t('supplier-doc-capture')),'native claim capture section');await page.evaluate(`document.querySelector(${q(t('supplier-doc-capture'))}).open=true`)
 const reference=upper(`TAX-BILL-${suffix}`);await page.fill(t('supplier-doc-reference'),reference);await page.fill(t('supplier-doc-date'),today);await page.fill(t('supplier-doc-total'),'140000');await page.fill(t('supplier-doc-quantity'),'20');await page.fill(t('supplier-doc-price'),'7000')
 await select('supplier-doc-tax-code','VATIN')
 await until(()=>page.text(t('supplier-doc-tax-preview')).then(s=>s===`VATIN 11% · version 1 applies on ${today} · computed tax Rp 15.400. Enter the tax amount printed on the supplier's document.`).catch(()=>false),'computed tax before the printed amount is entered')
 await page.fill(t('supplier-doc-tax'),'15401')
 const composer=`VATIN 11% · version 1 applies on ${today} · computed tax Rp 15.400 · entered tax Rp 15.401 · difference Rp 1 · owed to the supplier Rp 155.401`
 await until(()=>page.text(t('supplier-doc-tax-preview')).then(s=>s===composer).catch(()=>false),'composer shows computed, entered, difference and amount owed')
 assert.ok((await page.text(t('supplier-doc-tax-notice'))).includes('Archava does not verify tax rates or rules'));await shot('tax-supplier-claim-composer.png','supplier-doc-tax-preview')
 await click('supplier-doc-save')
 const claim=()=>view('purchasing',company).then(w=>w.documents?.find(d=>d.vendorReference===reference))
 const captured=await until(claim,'captured taxed claim');assert.equal(captured.declaredTotal,140000)
 assert.deepEqual({code:captured.tax.code,version:captured.tax.version,entered:captured.tax.enteredMinor,computed:captured.tax.computedMinor,difference:captured.tax.differenceMinor,total:captured.tax.totalMinor},{code:'VATIN',version:1,entered:15401,computed:15400,difference:1,total:155401})
 assert.equal(sql(`SELECT tax_minor||'/'||tax_computed_minor||'/'||tax_code||'/'||tax_code_version FROM archava_purchasing.supplier_documents WHERE id='${captured.id}'`),'15401/15400/VATIN/1')
 await openRow(captured.id)
 const claimTax=()=>Promise.all(['net','entered','computed','difference','total'].map(part=>page.text(t('supplier-document-tax-'+part))))
 await until(()=>claimTax().then(v=>v.join('|')==='Rp 140.000|Rp 15.401|Rp 15.400|Rp 1|Rp 155.401').catch(()=>false),'claim tax block')
 await page.fill(t('supplier-doc-reason'),'Exact accepted goods at the order price');await click('supplier-doc-match')
 await until(()=>claim().then(d=>d.state==='matched'),'matched taxed claim');await shot('tax-supplier-claim.png','supplier-document-detail')
 const bill=await purchaseSource('bill');assert.deepEqual({goods:bill.goods,fee:bill.fee,tax:bill.tax,account:bill.account},{goods:14000000,fee:0,tax:1540100,account:accounts['VAT-IN']})
 pass('TAX-native-supplier-claim-tax','On the native purchasing screens an offer of 7000, an order of 20, its receipt and a supplier claim for goods of 140000 with input code VATIN and a printed tax of 15401: the composer shows computed Rp 15.400, entered Rp 15.401, difference Rp 1 and Rp 155.401 owed; the claim stores entered 15401 beside computed 15400, matches on goods alone and captures a bill source of goods 14,000,000 with tax 1,540,100 on VAT-IN')

 // 8. The receipt and the taxed bill are reviewed, drafted and posted natively.
 await post('purchase_source.draft',receipt.id,'TAX-RECEIPT',['INVENTORY','GRNI'],null,{},[['INVENTORY',14000000,0],['GRNI',0,14000000]])
 await post('purchase_source.draft',bill.id,'TAX-BILL',['GRNI','VAT-IN','PAYABLE'],'tax-bill-source-review.png',{'purchase-source-net':'IDR 140000.00','purchase-source-tax':'IDR 15401.00','purchase-source-total':'IDR 155401.00'},[['GRNI',14000000,0],['VAT-IN',1540100,0],['PAYABLE',0,15540100]])
 pass('TAX-native-bill-source-posting','The native bill source review shows goods IDR 140000.00, input tax IDR 15401.00 and IDR 155401.00 owed and names GRNI, VAT-IN and PAYABLE; drafted and posted natively as debit GRNI 14,000,000, debit VAT-IN 1,540,100, credit PAYABLE 15,540,100')

 // 9. The tax report of the day on the native panel, against the ledger and the host.
 const balances=()=>json(`SELECT jsonb_object_agg(a.code,n.balance) FROM(SELECT l.account_id,sum(l.debit_minor-l.credit_minor)::text AS balance FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE j.book_id='${book}' AND j.state='posted' GROUP BY l.account_id)n JOIN archava_accounting.accounts a ON a.id=n.account_id`)
 const ledger=balances();assert.deepEqual(ledger,{AR:'2220000',REVENUE:'-2000000','VAT-OUT':'-220000',INVENTORY:'14000000',GRNI:'0','VAT-IN':'1540100',PAYABLE:'-15540100'})
 const table=id=>page.evaluate(`[...document.querySelectorAll(${q(t(id)+' tbody tr')})].map(tr=>[...tr.cells].map(c=>c.textContent))`)
 const runReport=async(from,to)=>{
  await open('accounting',company);await until(()=>page.evaluate(`document.querySelector(${q(t('accounting-taxreport-run'))})?.disabled===false`),'tax report controls')
  await page.fill(t('accounting-taxreport-from'),from);await page.fill(t('accounting-taxreport-to'),to);await click('accounting-taxreport-run');await until(()=>has(t('accounting-taxreport-result')),'tax report result')
 }
 await open('accounting',company);await until(()=>page.evaluate(`document.querySelector(${q(t('accounting-taxreport-from'))})?.value`).then(v=>v==='2000-01-01'),'tax report dates start as the open period')
 assert.equal(await page.evaluate(`document.querySelector(${q(t('accounting-taxreport-to'))}).value`),'2100-12-31')
 await runReport(today,today)
 assert.equal(await page.text(t('accounting-taxreport-verdict')),'Posted sources explain the movement of every tax control account in this period.')
 const reportCodes=await table('accounting-taxreport-codes')
 assert.deepEqual(reportCodes,[['VAT11 · version 1','Output (sales)','11%','2','IDR 20000.00','IDR 2200.00'],['VATIN · version 1','Input (purchases)','11%','1','IDR 140000.00','IDR 15401.00']])
 const totals={output:await page.text(t('accounting-taxreport-output')),input:await page.text(t('accounting-taxreport-input')),difference:await page.text(t('accounting-taxreport-difference'))}
 assert.deepEqual(totals,{output:'IDR 2200.00',input:'IDR 15401.00',difference:'IDR -13201.00'})
 const reportControls=await table('accounting-taxreport-controls')
 assert.deepEqual(reportControls,[['VAT-IN · Input tax','Input (purchases)','IDR 15401.00','IDR 15401.00','IDR 0.00'],['VAT-OUT · Output tax','Output (sales)','IDR 2200.00','IDR 2200.00','IDR 0.00']])
 assert.equal(await has(t('accounting-taxreport-control-differs')),false)
 assert.equal(await page.text(t('accounting-taxreport-pending')),'Captured but not posted, not in the totals: invoice sources 0 (taxable IDR 0.00, tax IDR 0.00); supplier bill sources 0 (taxable IDR 0.00, tax IDR 0.00).')
 assert.ok((await page.text(t('accounting-taxreport-outside'))).includes('taxed invoices issued without a posting policy 0 (tax IDR 0); taxed supplier documents matched without one 0 (tax IDR 0)'))
 const reportNotice=await page.text(t('accounting-taxreport-notice'));assert.ok(reportNotice.includes('Archava does not verify tax rates or rules')&&reportNotice.includes('not a tax return'),reportNotice)
 const api=await ok(`/erp/accounting/tax-report?tenant=${tenant}&bookId=${book}&from=${today}&to=${today}`)
 assert.deepEqual(api.totals,{outputTaxMinor:'220000',inputTaxMinor:'1540100',differenceMinor:'-1320100'});assert.equal(api.postedSourceControlsAgree,true)
 assert.deepEqual(api.controls.map(c=>[c.code,c.reportedTaxMinor,c.movementMinor,c.differenceMinor]),[['VAT-IN','1540100','1540100','0'],['VAT-OUT','220000','220000','0']])
 assert.equal(String(-Number(ledger['VAT-OUT'])),api.totals.outputTaxMinor);assert.equal(ledger['VAT-IN'],api.totals.inputTaxMinor)
 await shot('tax-report.png','accounting-taxreport-result')
 pass('TAX-native-tax-report',`The native Tax report panel starts on the open period, and for ${today} shows VAT11 with a taxable amount of IDR 20000.00 and tax IDR 2200.00 from 2 sources and VATIN with IDR 140000.00 and IDR 15401.00; output tax IDR 2200.00, input tax IDR 15401.00, output less input IDR -13201.00; both control accounts move by exactly the reported tax with a difference of IDR 0.00; nothing pending or outside the ledger; the host read and the posted ledger balances (VAT-OUT ${ledger['VAT-OUT']}, VAT-IN ${ledger['VAT-IN']}) agree`)

 // 10. A refusal that matters: while the output tax account is inactive, a taxed invoice is not issued.
 const account=()=>json(`SELECT jsonb_build_object('active',active,'revision',revision) FROM archava_accounting.accounts WHERE id='${accounts['VAT-OUT']}'`)
 await act('account.set_active',{id:accounts['VAT-OUT'],active:false,reason:'Deactivate the output tax account to see the refusal'});assert.deepEqual(account(),{active:false,revision:2})
 await open('invoices',company);await openRow(inv2.id);await until(()=>has(t('invoice-issue')),'draft inclusive invoice')
 await issueInvoice(inv2,'Issue while the tax account is inactive')
 await until(()=>has(t('ops-refused')),'visible refusal of the taxed invoice');const refusal=(await page.text(t('ops-refused'))).trim()
 const sentence="The control account of this invoice's tax code is inactive, so the invoice's tax cannot be posted and the invoice was not issued. Reactivate the account in Accounting and issue the invoice again."
 assert.ok(refusal.includes(sentence),refusal)
 assert.deepEqual({status:invoiceRow(code2).status,revision:invoiceRow(code2).revision},{status:'draft',revision:inv2.revision});assert.equal(sql(`SELECT count(*) FROM archava_accounting.invoice_sources WHERE invoice_id='${inv2.id}'`),'0')
 await shot('tax-inactive-account-refused.png','ops-refused')
 await open('accounting',company);await act('account.set_active',{id:accounts['VAT-OUT'],active:true,reason:'Reactivate the output tax account'});assert.deepEqual(account(),{active:true,revision:3})
 await open('invoices',company);await openRow(inv2.id);await until(()=>has(t('invoice-issue')),'draft inclusive invoice after reactivation')
 await issueInvoice(inv2,'Issue after the tax account was reactivated');const released=await invoiceSource(inv2.id,'issue')
 assert.deepEqual({amount:released.amount,tax:released.tax,account:released.account,accountRevision:released.accountRevision},{amount:3000000,tax:297300,account:accounts['VAT-OUT'],accountRevision:3});assert.equal(invoiceRow(code2).status,'issued')
 // The released source is captured and not posted: the report lists it as pending and leaves the totals alone.
 await runReport(today,today)
 await until(()=>page.text(t('accounting-taxreport-pending')).then(s=>s.includes('invoice sources 1 (taxable IDR 27027.00, tax IDR 2973.00)')),'released invoice pending in the tax report')
 assert.equal(await page.text(t('accounting-taxreport-output')),'IDR 2200.00')
 pass('TAX-native-inactive-tax-account-refusal',`With VAT-OUT deactivated on the native Accounting form, issuing the taxed invoice on the native invoice screen shows "${sentence}" and writes nothing (invoice still draft at its revision, no source); after reactivating the account natively the same invoice issues and its source carries tax 297,300 on VAT-OUT at account revision 3; the Tax report then lists it as 1 pending invoice source (taxable IDR 27027.00, tax IDR 2973.00) outside the unchanged output total`)
 return{scope:'Owned synthetic IDR company with a single-operator book on the native screens: tax codes, taxed sales order, invoice, mail, correction, supplier claim with printed tax, source reviews and postings, the tax report and one issue refusal. The company, parties, supplier, book, ledger accounts, period, the two posting policies and the products are created through the command API. Not covered in the browser: a book with independent posting (a second signed-in person), a 0% code, a supplier document correction or void, payments, tax code retirement, outside-ledger documents with amounts, a manual journal difference, and non-IDR tax (host tests cover these).',company,book,invoices:{exclusive:inv1.id,inclusive:inv2.id},purchase:po.id,document:captured.id,journals,ledger,report:{codes:reportCodes,totals,controls:reportControls},refusal}
}
