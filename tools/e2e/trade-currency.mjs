/** One company whose book is USD, from the first product to the posted ledger, on the native screens. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
export async function verifyTradeCurrency({page,origin,facts,until,pass,suffix,emailProvider}){
 assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(suffix,/^[0-9a-f]{8}$/)
 const tenant='acme-retail',t=id=>`[data-testid="${id}"]`,q=JSON.stringify,upper=s=>s.toUpperCase()
 const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='30s';SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:35000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
 const call=(path,body)=>page.evaluate(`fetch(${q(path)},{method:${q(body?'POST':'GET')},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body?`,body:${q(q(body))}`:''}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const ok=async(path,body)=>{const r=await call(path,body);assert.equal(r.status,200,q(r));return r.body.receipt??r.body}
 const org=(type,data)=>ok('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}})
 const command=(domain,command)=>ok(`/erp/${domain}/commands`,{tenant,requestId:randomUUID(),command})
 const view=(domain,company)=>page.evaluate(`fetch('/erp/${domain}/workspace?tenant=${tenant}&companyId=${company}').then(r=>r.json())`)
 const has=selector=>page.evaluate(`!!document.querySelector(${q(selector)})`)
 const value=id=>page.evaluate(`document.querySelector(${q(t(id))})?.value`)
 // The caption of a labelled input, e.g. "Unit price · USD".
 const caption=id=>page.evaluate(`document.querySelector(${q(t(id))})?.closest('label')?.querySelector('span')?.textContent??''`)
 const step=id=>page.evaluate(`document.querySelector(${q(t(id))}).getAttribute('step')`)
 const select=async(id,wanted)=>{
  await until(()=>has(t(id)),id);await page.evaluate(`document.querySelector(${q(t(id))}).focus()`)
  await until(()=>page.evaluate(`!![...document.querySelector(${q(t(id))}).options].find(o=>o.value===${q(String(wanted))})`),`${id} offers ${wanted}`)
  await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});n.value=${q(String(wanted))};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 }
 const optionText=(id,wanted)=>page.evaluate(`[...document.querySelector(${q(t(id))}).options].find(o=>o.value===${q(wanted)})?.textContent??''`)
 const click=async id=>{await until(()=>page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});return !!n&&!n.disabled&&n.getClientRects().length>0&&!n.closest('fieldset')?.disabled;})()`),'enabled native '+id);await page.click(t(id))}
 const submitOf=id=>page.evaluate(`document.querySelector(${q(t(id))}).closest('form').querySelector('button[type="submit"]').click()`)
 const open=async(module,company)=>{await page.navigate(`${origin}/app/erp?module=${module}`);await scope(company)}
 // The picker ignores a change while its workspace is loading, so choose until the reviewed scope names the company.
 const scope=async company=>{await until(()=>has(`${t('trade-company-select')} option[value="${company}"]`),'company scope option');await until(async()=>{if((await page.text(t('trade-company-selected'))).includes(company))return true;await page.evaluate(`(()=>{const n=document.querySelector(${q(t('trade-company-select'))});n.value=${q(company)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`);return false},'selected company scope')}
 // The list row of a record and the heading of its table.
 const row=id=>page.evaluate(`(()=>{const n=document.querySelector(${q(t('ops-open-'+id))});return n?{cells:[...n.closest('tr').cells].map(c=>c.textContent),headings:[...n.closest('table').querySelectorAll('thead th')].map(c=>c.textContent)}:null})()`)
 const listed=id=>until(()=>row(id),'listed record '+id)
 const openRow=async id=>{await until(()=>has(t('ops-open-'+id)),'record row '+id);await page.click(t('ops-open-'+id))}
 const shot=async(file,id)=>{await page.evaluate(`document.querySelector(${q(t(id))}).scrollIntoView({block:'center',behavior:'instant'})`);await page.evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');await page.screenshot(file)}
 // Native Accounting action form: fill, review, confirm.
 const field=async(name,wanted)=>{
  const id=`operator-accounting-${name}`;await until(()=>has(t(id)),id+' rendered')
  const kind=await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});return n.tagName==='SELECT'?'select':n.type})()`)
  if(kind==='select')await select(id,wanted)
  else if(kind==='checkbox')await page.evaluate(`(()=>{const n=document.querySelector(${q(t(id))});n.checked=${!!wanted};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  else await page.fill(t(id),String(wanted))
 }
 const choose=async action=>{await select('operator-action',action);await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current financial grant')}
 const review=async(action,fields,{extraAccount=false}={})=>{await choose(action);if(extraAccount)await page.click(t('operator-accounting-accounts')+' > button');for(const[k,v]of Object.entries(fields))await field(k,v);await page.click(t('module-accounting-create'));await until(async()=>{if(await has(t('ops-refused')))throw Error(await page.text(t('ops-feedback')));return has(t('operator-review'))},action+' review')}
 const confirm=async()=>{await page.click(t('operator-confirm'));await until(async()=>{const text=await page.text(t('ops-feedback'));if(await has(t('ops-refused'))||await has(t('ops-uncertain')))throw Error(text);return text.includes('Saved.')},'matching financial receipt')}
 const act=async(action,fields)=>{await review(action,fields);await confirm()}

 // Standalone browser fixture only, as in the other financial journeys: the operator may manage organizations, companies and parties.
 sql(`INSERT INTO archava_organizations.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','alice','human',ARRAY['organizations.read','organizations.write','companies.read','companies.write','parties.read','parties.write'],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY(SELECT DISTINCT p FROM unnest(m.permissions||EXCLUDED.permissions) p ORDER BY p)`)
 // Two companies and their trading partners. No native company or party step is under test here: created through the command API.
 const company=async code=>{const o=await org('create_organization',{name:`Currency ${code} ${suffix}`,legalName:`Currency ${code} Official ${suffix}`});return(await org('company.create_from_organization',{code:upper(`CUR-${code}-${suffix}`),organizationId:o.organizationId,expectedSourceRevision:1,reason:'Owned trade-currency acceptance company'})).companyId}
 const usd=await company('USD'),idr=await company('IDR')
 const partnerName=`Currency partner ${suffix}`,partner=await org('create_organization',{name:partnerName,legalName:`Currency Partner Official ${suffix}`})
 const party=(await org('party.create_from_organization',{organizationId:partner.organizationId,expectedSourceRevision:1,customer:true,supplier:true})).partyId
 const supplier=await command('purchasing',{operation:'supplier.create-from-party',code:upper(`CUR-SUP-${suffix}`),partyId:party,expectedPartyRevision:1})

 // 1. The book comes first: the native form creates a USD book for a company without any trade record.
 const bookFields=code=>({code,name:`Trade currency ${suffix}`,currency:'USD',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator USD book before the first trade record'})
 const bookCode=upper(`CUR-BOOK-${suffix}`)
 await open('accounting',usd);await act('book.create',bookFields(bookCode))
 const book=JSON.parse(sql(`SELECT jsonb_build_object('id',id,'currency',currency,'minorUnits',minor_units,'company',company_id) FROM archava_accounting.books WHERE code='${bookCode}'`))
 assert.deepEqual({...book,id:undefined},{id:undefined,currency:'USD',minorUnits:2,company:usd});assert.match(book.id,/^[0-9a-f-]{36}$/)
 pass('CURRENCY-native-usd-book-first',`The native Accounting form created book ${bookCode} in USD with 2 minor units for a company that has no product, order, purchase order or invoice`)

 // 2. Catalog: the new-product form follows the company's trade currency.
 await open('catalog',usd);await until(()=>caption('product-price').then(s=>s==='Unit price · USD'),'new-product price labelled in the book currency')
 assert.equal(await step('product-price'),'0.01')
 const skuA=upper(`CUR-A-${suffix}`),titleA=`Dollar goods ${suffix}`,productRow=sku=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'price',price,'currency',currency)::text FROM archava_commerce.products WHERE sku='${sku}'),'')`)
 await page.fill(t('product-sku'),skuA);await page.fill(t('product-title'),titleA);await page.fill(t('product-price'),'19.999');await click('product-save')
 await page.evaluate('new Promise(resolve=>setTimeout(resolve,600))')
 const blocked=await page.evaluate(`({stepMismatch:document.querySelector(${q(t('product-price'))}).validity.stepMismatch,refused:!!document.querySelector(${q(t('ops-refused'))})})`)
 assert.ok(blocked.stepMismatch||blocked.refused,q(blocked));assert.equal(productRow(skuA),'','A price with three decimals must not create a USD product')
 await page.fill(t('product-price'),'19.99');await click('product-save')
 const a=JSON.parse(await until(()=>productRow(skuA),'USD product saved'));assert.deepEqual({price:a.price,currency:a.currency},{price:1999,currency:'USD'})
 // A second stock product, through the command API in minor units, so the list holds more than one USD row.
 const b=await command('commerce',{operation:'product.create_for_company',companyId:usd,expectedCompanyRevision:1,sku:upper(`CUR-B-${suffix}`),title:`Dollar spare ${suffix}`,unit:'unit',trackStock:true,price:500})
 await click('ops-refresh');const listedA=await listed(a.id);await listed(b.id)
 assert.ok(listedA.headings.includes('Price · USD'),q(listedA));assert.ok(listedA.cells.includes('USD 19.99'),q(listedA));assert.ok((await row(b.id)).cells.includes('USD 5.00'))
 await openRow(a.id);await until(()=>page.text(t('product-detail')).then(s=>s.includes('USD 19.99 · revision 1')),'USD product detail')
 assert.equal(await caption('product-new-price'),'New price · USD');assert.equal(await step('product-new-price'),'0.01');assert.equal(await caption('stock-unit-cost'),'Unit acquisition cost · USD (optional for additions)');assert.equal(await step('stock-unit-cost'),'0.01')
 await shot('trade-currency-catalog.png','product-detail')
 pass('CURRENCY-native-product-price',`The Catalog form of a USD company is labelled "Unit price · USD" with step 0.01; 19.999 is blocked (${blocked.stepMismatch?'browser step validation':'form refusal'}) and creates nothing; 19.99 creates a product stored as 1999 USD minor units, listed as "USD 19.99" under "Price · USD"`)
 const price=()=>sql(`SELECT price FROM archava_commerce.products WHERE id='${a.id}'`)
 await page.fill(t('product-new-price'),'24.50');await submitOf('product-new-price');await until(()=>price()==='2450','decimal price update stored')
 await until(()=>page.text(t('product-detail')).then(s=>s.includes('USD 24.50 · revision 2')),'updated USD price shown')
 await page.fill(t('product-new-price'),'19.99');await submitOf('product-new-price');await until(()=>price()==='1999','price restored');await until(()=>page.text(t('product-detail')).then(s=>s.includes('USD 19.99 · revision 3')),'restored USD price shown')
 await openRow(b.id);await until(()=>page.text(t('product-detail')).then(s=>s.includes('USD 5.00')),'spare product detail')
 await page.fill(t('stock-quantity'),'3');await page.fill(t('stock-unit-cost'),'2.75');await page.fill(t('stock-reason'),'Owned USD opening count');await click('stock-save')
 await until(()=>sql(`SELECT coalesce((SELECT quantity FROM archava_commerce.stock_balances WHERE product_id='${b.id}'),0)`)==='3','USD opening count stored')
 assert.equal(sql(`SELECT sum(total_cost_minor)::text FROM archava_commerce.inventory_layers WHERE product_id='${b.id}'`),'825')
 await until(()=>page.text(t('inventory-value')).then(s=>s==='USD 8.25'),'exact USD inventory value shown')
 pass('CURRENCY-native-price-update-and-stock-cost','The native price form stores 24.50 as 2450 and 19.99 as 1999 and shows each as USD; the native stock form stores 3 units at 2.75 as an 825 minor-unit layer, shown as "USD 8.25"')

 // 3. Switching company: the same form follows the selected company; a company without a book is IDR.
 await scope(idr);await until(()=>caption('product-price').then(s=>s==='Unit price · IDR'),'IDR company label');assert.equal(await step('product-price'),null)
 const skuR=upper(`CUR-R-${suffix}`);await page.fill(t('product-sku'),skuR);await page.fill(t('product-title'),`Rupiah goods ${suffix}`);await page.fill(t('product-price'),'15000');await click('product-save')
 const r=JSON.parse(await until(()=>productRow(skuR),'IDR product saved'));assert.deepEqual({price:r.price,currency:r.currency},{price:15000,currency:'IDR'})
 const listedR=await listed(r.id);assert.ok(listedR.headings.includes('Price · IDR'),q(listedR));assert.ok(listedR.cells.includes('15000'),q(listedR))
 await scope(usd);await until(()=>caption('product-price').then(s=>s==='Unit price · USD'),'USD company label after switching back');assert.equal(await step('product-price'),'0.01');await listed(a.id)
 pass('CURRENCY-native-company-switch','Switching the Catalog between a company without a book and the USD company changes the price label and step (IDR, whole; USD, 0.01); the native form stores 15000 whole Rupiah for the first and the list shows it under "Price · IDR"')

 // 4. The guard: a company that already has a product is refused a USD book on the native form.
 const refusedCode=upper(`CUR-LATE-${suffix}`);await open('accounting',idr);await review('book.create',bookFields(refusedCode));await page.click(t('operator-confirm'))
 await until(()=>has(t('ops-refused')),'visible refusal of a late USD book');const refusal=(await page.text(t('ops-refused'))).trim();assert.ok(refusal.length>0)
 assert.equal(sql(`SELECT count(*) FROM archava_accounting.books WHERE company_id='${idr}' OR code='${refusedCode}'`),'0')
 await shot('trade-currency-late-book-refused.png','ops-refused')
 pass('CURRENCY-native-late-book-refused',`The native Accounting form shows a refusal ("${refusal.slice(0,120)}") for a USD book of a company that already has a product, and no book is written`)

 // 5. Posting policies: accounts and the period through the command API, the three policy forms natively.
 const accounts={};for(const[code,cls,cash]of [['AR','asset','operating'],['REVENUE','revenue','operating'],['CLEARING','asset','none'],['INVENTORY','asset','operating'],['GRNI','liability','operating'],['PAYABLE','liability','operating'],['LANDED_FEE','liability','operating'],['COGS','cost_of_goods_sold','operating']])accounts[code]=(await command('accounting',{type:'account.create',bookId:book.id,code,name:code,class:cls,cashRole:cash,reason:'Reviewed USD posting account'})).resourceId
 await command('accounting',{type:'period.open',bookId:book.id,code:'USD-SOURCE',startDate:'2000-01-01',endDate:'2100-12-31',reason:'Owned synthetic source period'})
 const policy=async(action,table,fields)=>{
  await open('accounting',usd);await choose(action);await until(()=>value('operator-accounting-sourceUnit').then(v=>v==='minor'),action+' settles on the minor unit')
  for(const[k,v]of Object.entries({bookId:book.id,currencyCatalogue:'SIX-2026-09-17',...fields,reason:'USD amounts are minor units and post unchanged'}))await field(k,v)
  assert.equal(await value('operator-accounting-sourceUnit'),'minor');await page.click(t('module-accounting-create'));await until(()=>has(t('operator-review')),action+' review');await confirm()
  assert.equal(sql(`SELECT source_unit||':'||revision FROM archava_accounting.${table} WHERE book_id='${book.id}'`),'minor:1')
 }
 const mappings=pairs=>Object.fromEntries(pairs.flatMap(([role,code],i)=>[[`mappings-${i}-role`,role],[`mappings-${i}-accountId`,accounts[code]]]))
 await policy('invoice_policy.set','invoice_policies',mappings([['receivable','AR'],['revenue','REVENUE'],['payment_clearing','CLEARING']]))
 await policy('purchase_policy.set','purchase_policies',mappings([['inventory','INVENTORY'],['goods_received_not_invoiced','GRNI'],['payable','PAYABLE'],['landed_fee_accrual','LANDED_FEE']]))
 await policy('stock_policy.set','stock_policies',{'cogsAccount-accountId':accounts.COGS})
 pass('CURRENCY-native-policy-forms-minor','On a USD book the native invoice, purchase and stock policy forms set the source unit to "minor" without operator input, and each save is stored with source_unit minor')

 // 6. Purchasing: supplier price, purchase order, receipt and supplier claim in USD.
 await open('purchasing',usd);await until(()=>has(`${t('offer-product')} option[value="${a.id}"]`),'USD purchasing catalog');await until(()=>has(`${t('offer-supplier')} option[value="${supplier.id}"]`),'supplier projection')
 await page.evaluate(`document.querySelector(${q(t('offer-save'))}).closest('details').open=true`);await select('offer-supplier',supplier.id);await select('offer-product',a.id)
 await until(()=>caption('offer-cost').then(s=>s==='Unit cost · USD'),'offer cost labelled by its product');assert.equal(await step('offer-cost'),'0.01')
 await page.fill(t('offer-cost'),'12.50');await click('offer-save')
 const offer=await until(async()=>(await view('purchasing',usd)).offers?.find(o=>o.productId===a.id),'USD supplier price');assert.equal(offer.unitCost,1250);assert.equal(offer.currency,'USD')
 await select('purchase-supplier',supplier.id);await until(()=>has(`${t('purchase-offer')} option[value="${offer.id}"]`),'USD offer in the picker')
 const offerLabel=await optionText('purchase-offer',offer.id);assert.ok(offerLabel.endsWith('USD 12.50'),offerLabel)
 await select('purchase-offer',offer.id);await page.fill(t('purchase-quantity'),'4');await click('purchase-add-line');await click('purchase-save')
 const po=await until(async()=>(await view('purchasing',usd)).orders?.find(o=>o.supplierId===supplier.id),'USD purchase order');assert.equal(po.total,5000);assert.equal(po.currency,'USD')
 const listedPo=await listed(po.id);assert.ok(listedPo.headings.includes('Total · USD'),q(listedPo));assert.ok(listedPo.cells.includes('USD 50.00'),q(listedPo))
 await openRow(po.id);await until(()=>page.text(t('purchase-detail')).then(s=>s.includes('draft · USD 50.00')),'USD purchase detail')
 // The generic action form takes the currency of the selected purchase order.
 await page.evaluate(`document.querySelector(${q(t('operator-action'))}).closest('details').open=true`);await select('operator-action','document.capture')
 await until(()=>value('operator-purchasing-currency').then(v=>v==='USD'),'generic supplier-document form prefilled with the order currency')
 pass('CURRENCY-native-generic-capture-prefill','With a USD purchase order selected, "All purchasing actions" → document.capture shows currency USD without operator input')
 await page.fill(t('purchase-reason'),'Reviewed USD delivery');await click('purchase-confirm');await until(()=>has(t('purchase-receive')),'confirmed USD purchase')
 assert.equal(await caption('receive-landed-fee'),'Delivery / landed fee allocated to accepted goods · USD');assert.equal(await step('receive-landed-fee'),'0.01')
 await page.fill(t('receive-accepted'),'4');await page.fill(t('receive-landed-fee'),'1.20');await click('purchase-receive')
 await until(async()=>(await view('purchasing',usd)).orders.find(o=>o.id===po.id)?.state==='received','USD goods receipt')
 const purchaseSource=effect=>until(()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'goods',goods_minor,'fee',landed_fee_minor)::text FROM archava_accounting.purchase_sources WHERE book_id='${book.id}' AND order_id='${po.id}' AND effect='${effect}'),'')`),`captured purchase ${effect}`).then(JSON.parse)
 const receipt=await purchaseSource('receipt');assert.deepEqual({goods:receipt.goods,fee:receipt.fee},{goods:5000,fee:120})
 assert.equal(sql(`SELECT sum(total_cost_minor)::text FROM archava_commerce.inventory_layers WHERE product_id='${a.id}'`),'5120')
 pass('CURRENCY-native-purchase-receipt','Native supplier price 12.50 is stored as 1250 and offered as "USD 12.50"; the purchase order of 4 lists "USD 50.00"; the receipt with a 1.20 landed fee captures goods 5000 and fee 120 for the ledger and a 5120 stock layer')
 // Supplier claim: capture with a one-cent total discrepancy, correct it, match.
 await page.evaluate(`(()=>{const original=window.fetch;window.__currencyClaims=[];window.fetch=async function(input,init){const request=input instanceof Request?input:new Request(input,init);if(request.method==='POST'&&new URL(request.url).pathname==='/erp/purchasing/commands'){const body=await request.clone().json();if(String(body?.command?.operation).startsWith('document.'))window.__currencyClaims.push(body.command)}return original.call(window,input,init)};window.__restoreCurrencyFetch=()=>{window.fetch=original}})()`)
 await until(()=>has(t('supplier-doc-capture')),'native claim capture section');await page.evaluate(`document.querySelector(${q(t('supplier-doc-capture'))}).open=true`)
 assert.equal(await caption('supplier-doc-total'),'Declared goods total · USD');assert.equal(await step('supplier-doc-total'),'0.01')
 const claimForm=await page.text(t('supplier-doc-capture-form'));assert.ok(claimForm.includes('PO unit price USD 12.50'),claimForm);assert.ok(claimForm.includes('Claimed unit price · USD'),claimForm);assert.equal(await step('supplier-doc-price'),'0.01')
 const reference=upper(`CUR-BILL-${suffix}`);await page.fill(t('supplier-doc-reference'),reference);await page.fill(t('supplier-doc-date'),'2026-11-07');await page.fill(t('supplier-doc-total'),'50.01');await page.fill(t('supplier-doc-quantity'),'4');await page.fill(t('supplier-doc-price'),'12.50');await click('supplier-doc-save')
 const claim=()=>view('purchasing',usd).then(w=>w.documents?.find(d=>d.vendorReference===reference))
 const captured=await until(claim,'captured USD claim');assert.equal(captured.declaredTotal,5001);assert.equal(captured.currency,'USD')
 const sentCapture=await page.evaluate('window.__currencyClaims.at(-1)');assert.deepEqual({operation:sentCapture.operation,currency:sentCapture.currency,declaredTotal:sentCapture.declaredTotal,unitCost:sentCapture.lines[0].unitCost},{operation:'document.capture',currency:'USD',declaredTotal:5001,unitCost:1250})
 const listedClaim=await listed(captured.id);assert.ok(listedClaim.headings.includes('Claim · USD'),q(listedClaim));assert.ok(listedClaim.cells.includes('USD 50.01'),q(listedClaim))
 await openRow(captured.id);await page.fill(t('supplier-doc-reason'),'Check the supplier total');await click('supplier-doc-match');await until(()=>claim().then(d=>d.state==='held'),'held one-cent discrepancy')
 await until(()=>page.text(t('supplier-document-state')).then(s=>s.startsWith('held')&&s.includes('USD 50.01')),'held USD claim shown')
 await click('supplier-doc-edit');await until(()=>has(t('supplier-doc-edit-total')),'correction form')
 assert.equal(await caption('supplier-doc-edit-total'),'Declared goods total · USD');assert.equal(await value('supplier-doc-edit-total'),'50.01');assert.equal(await value('supplier-doc-edit-price'),'12.50')
 await page.fill(t('supplier-doc-edit-total'),'50.00');await page.fill(t('supplier-doc-edit-reason'),'Supplier confirmed the goods total');await click('supplier-doc-revise')
 await until(()=>claim().then(d=>d.revision===3&&d.declaredTotal===5000),'corrected USD claim version')
 const sentRevise=await page.evaluate('window.__currencyClaims.at(-1)');assert.deepEqual({operation:sentRevise.operation,currency:sentRevise.currency,declaredTotal:sentRevise.declaredTotal,unitCost:sentRevise.lines[0].unitCost},{operation:'document.revise',currency:'USD',declaredTotal:5000,unitCost:1250})
 await until(()=>page.text(t('supplier-document-state')).then(s=>s.startsWith('captured')),'corrected claim shown');await page.fill(t('supplier-doc-reason'),'Exact accepted goods at the order price');await click('supplier-doc-match')
 await until(()=>claim().then(d=>d.state==='matched'),'matched USD claim');await until(()=>page.text(t('supplier-document-state')).then(s=>s.startsWith('matched')&&s.includes('USD 50.00')),'matched USD claim shown')
 await page.evaluate('window.__restoreCurrencyFetch()');await shot('trade-currency-supplier-claim.png','supplier-document-detail')
 const bill=await purchaseSource('bill');assert.deepEqual({goods:bill.goods,fee:bill.fee},{goods:5000,fee:0})
 pass('CURRENCY-native-supplier-claim','The native claim form is labelled in USD, sends currency USD with 50.01 as 5001 and 12.50 as 1250; the correction form shows the stored amounts as 50.01 and 12.50 and sends 5000; the matched claim captures a 5000 bill source')

 // 7. Sale: order, reservation and fulfillment in USD.
 await open('sales',usd);await until(()=>has(`${t('order-product')} option[value="${a.id}"]`),'USD sales product')
 const productLabel=await optionText('order-product',a.id);assert.ok(productLabel.endsWith('USD 19.99'),productLabel)
 await page.fill(t('customer-party-search'),partnerName);await click('customer-party-find');await select('customer-party-select',party)
 await select('order-product',a.id);await page.fill(t('order-quantity'),'2');await click('order-add-line');await click('order-save')
 const sale=await until(async()=>(await view('commerce',usd)).orders?.find(o=>o.customerPartyId===party),'USD sales order');assert.equal(sale.total,3998);assert.equal(sale.currency,'USD')
 const listedSale=await listed(sale.id);assert.ok(listedSale.headings.includes('Total · USD'),q(listedSale));assert.ok(listedSale.cells.includes('USD 39.98'),q(listedSale))
 await openRow(sale.id);await click('order-reserve');await click('order-fulfill');await until(async()=>(await view('commerce',usd)).orders.find(o=>o.id===sale.id)?.state==='fulfilled','USD fulfillment')
 await until(()=>page.text(t('sales-detail')).then(s=>s.includes('fulfilled · USD 39.98')),'fulfilled USD order shown');await until(()=>page.text(t('order-fulfillment-cost')).then(s=>s==='USD 25.60'),'USD fulfillment cost shown')
 const stockSource=JSON.parse(await until(()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'amount',amount_minor)::text FROM archava_accounting.stock_sources WHERE book_id='${book.id}' AND order_id='${sale.id}' AND effect='fulfillment'),'')`),'captured fulfillment cost'));assert.equal(stockSource.amount,2560)
 pass('CURRENCY-native-sale','The native order form offers the product as "USD 19.99"; 2 units total 3998 minor units, listed and shown as "USD 39.98"; fulfillment shows cost "USD 25.60" and captures a 2560 stock source')

 // 8. Invoice from the order, issuance, mail and a negative decimal correction.
 await open('invoices',usd);await until(()=>has(`${t('invoice-source-order')} option[value="${sale.id}"]`),'USD order in the invoice picker')
 const orderLabel=await optionText('invoice-source-order',sale.id);assert.ok(orderLabel.includes('· USD 39.98 ·'),orderLabel)
 await select('invoice-source-order',sale.id);await until(()=>page.text(`${t('invoice-from-order')} ul.ops-lines`).then(s=>s===`2 × ${titleA} · USD 39.98`),'USD invoice preview line')
 const invoiceCode=upper(`CUR-INV-${suffix}`);await page.fill(t('invoice-order-code'),invoiceCode);await click('invoice-from-order-save')
 const invoiceRow=()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'currency',currency,'gross',gross_minor,'outstanding',outstanding_minor,'status',status)::text FROM archava_billing.invoices WHERE invoice_code='${invoiceCode}'),'')`)
 const invoice=JSON.parse(await until(invoiceRow,'USD invoice'));assert.deepEqual({currency:invoice.currency,gross:invoice.gross},{currency:'USD',gross:3998})
 await openRow(invoice.id);const totals=()=>page.evaluate(`[...document.querySelectorAll(${q(t('invoice-detail')+' dl.ops-totals dd')})].map(n=>n.textContent)`)
 await until(()=>totals().then(v=>v.join('|')==='USD 39.98|USD 0.00|USD 0.00|USD 39.98'),'USD invoice totals');assert.equal(await page.text(`${t('invoice-detail')} ul.ops-lines`),`2 × ${titleA} · USD 39.98`)
 await page.fill(t('invoice-reason'),'Issue the reviewed USD invoice');await click('invoice-issue')
 const invoiceSource=effect=>until(()=>sql(`SELECT coalesce((SELECT jsonb_build_object('id',id,'amount',amount_minor)::text FROM archava_accounting.invoice_sources WHERE book_id='${book.id}' AND invoice_id='${invoice.id}' AND effect='${effect}'),'')`),`captured invoice ${effect}`).then(JSON.parse)
 const issue=await invoiceSource('issue');assert.equal(issue.amount,3998)
 await until(()=>has(t('invoice-correction')),'issued USD invoice');assert.equal(await caption('invoice-correction'),'Correction amount · USD');assert.equal(await step('invoice-correction'),'0.01')
 const sentBefore=emailProvider.state.sends;await page.fill(t('email-recipient'),`currency-${suffix}@archava.test`);await page.fill(t('email-subject'),`USD invoice ${suffix}`);await click('email-send-button')
 await until(()=>page.text(t('email-notice')).then(s=>/Email (status|outcome confirmed): accepted/.test(s)),'USD invoice mail accepted');assert.equal(emailProvider.state.sends,sentBefore+1)
 const mail=emailProvider.state.bodies.at(-1);assert.match(mail.text,/Total Due: 39\.98 USD\n/);assert.ok(mail.html.includes('Total Due: <strong>39.98 USD</strong>'),mail.html)
 await page.fill(t('invoice-reason'),'Reviewed customer credit');await page.fill(t('invoice-correction'),'-1.25');await submitOf('invoice-correction')
 const correction=await invoiceSource('correction');assert.equal(correction.amount,-125)
 await until(()=>totals().then(v=>v.join('|')==='USD 39.98|USD -1.25|USD 0.00|USD 38.73'),'USD invoice totals after the credit')
 const listedInvoice=await listed(invoice.id);assert.ok(listedInvoice.cells.includes('3873')&&listedInvoice.cells.includes('USD'),q(listedInvoice));assert.equal(JSON.parse(invoiceRow()).outstanding,3873)
 await shot('trade-currency-invoice.png','invoice-detail')
 pass('CURRENCY-native-invoice-and-credit','The invoice picker and preview show "USD 39.98"; the invoice is stored in USD with gross 3998 and issues a 3998 source; the owned mail prints "Total Due: 39.98 USD"; the native correction -1.25 is stored as -125 and the detail shows outstanding "USD 38.73"')

 // 9. Ledger: every captured source is reviewed, drafted and posted on the native screen at the entered amount.
 const journals={}
 const post=async(action,source,code,roles,shown,summary)=>{
  await open('accounting',usd);const fields={sourceId:source,code,reason:'Review the exact USD amount and its policy accounts'};roles.forEach((role,i)=>fields[`accounts-${i}-accountId`]=accounts[role]);await review(action,fields,{extraAccount:roles.length===3})
  const text=await page.text(t(summary??'operator-review'));for(const amount of shown)assert.ok(text.includes(amount),`${code}: ${amount} missing in ${text}`)
  if(action==='invoice_source.draft')assert.ok((await page.text(t('operator-review'))).includes('USD minor units posted unchanged as USD book units'),code+' source unit caption')
  if(code==='USD-RECEIPT')await shot('trade-currency-source-review.png','operator-review')
  await confirm();const journal=sql(`SELECT id FROM archava_accounting.journals WHERE book_id='${book.id}' AND code='${code}'`);assert.match(journal,/^[0-9a-f-]{36}$/)
  await act('journal.post',{id:journal,reason:'Post the separately reviewed USD source'});assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`),'posted');journals[code]=journal
 }
 await post('invoice_source.draft',issue.id,'USD-ISSUE',['AR','REVENUE'],['USD 39.98'])
 await post('invoice_source.draft',correction.id,'USD-CREDIT',['AR','REVENUE'],['USD -1.25'])
 await post('purchase_source.draft',receipt.id,'USD-RECEIPT',['INVENTORY','GRNI','LANDED_FEE'],['USD 50.00','USD 1.20'],'purchase-source-summary')
 await post('purchase_source.draft',bill.id,'USD-BILL',['GRNI','PAYABLE'],['USD 50.00','USD 0.00'],'purchase-source-summary')
 await post('stock_source.draft',stockSource.id,'USD-COGS',['INVENTORY','COGS'],['USD 25.60'],'stock-source-summary')
 const balances=JSON.parse(sql(`SELECT jsonb_object_agg(a.code,n.balance) FROM(SELECT l.account_id,sum(l.debit_minor-l.credit_minor)::text AS balance FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE j.book_id='${book.id}' AND j.state='posted' GROUP BY l.account_id)n JOIN archava_accounting.accounts a ON a.id=n.account_id`))
 assert.deepEqual(balances,{AR:'3873',REVENUE:'-3873',INVENTORY:'2560',GRNI:'0',PAYABLE:'-5000',LANDED_FEE:'-120',COGS:'2560'})
 pass('CURRENCY-native-source-posting',`Five native source reviews show the entered USD amounts (39.98, -1.25, 50.00 with fee 1.20, 50.00, 25.60) and are drafted and posted; posted balances in minor units are ${q(balances)}`)

 // 10. Reports and the three control comparisons on the native screen.
 const today=sql("SELECT (clock_timestamp() AT TIME ZONE 'UTC')::date::text")
 await open('accounting',usd);await until(()=>page.evaluate(`document.querySelector(${q(t('accounting-report-run'))})?.disabled===false`),'report controls')
 await page.fill(t('accounting-report-from'),'2000-01-01');await page.fill(t('accounting-report-to'),'2100-12-31');await click('accounting-report-run')
 await until(()=>page.text(t('accounting-source-coverage')).then(s=>s.includes('5 posted, 0 awaiting posting')),'USD source coverage')
 const pairs=selector=>page.evaluate(`(()=>{const out={};for(const dt of document.querySelectorAll(${q(selector)}+' dt'))out[dt.textContent]=dt.nextElementSibling.textContent;return out})()`)
 const reported=await pairs(`${t('accounting-report-result')} dl.operator-details`)
 assert.deepEqual({revenue:reported['Revenue'],cogs:reported['Cost of goods sold'],profit:reported['Net profit'],assets:reported['Assets'],liabilities:reported['Liabilities']},{revenue:'USD 38.73',cogs:'USD 25.60',profit:'USD 13.13',assets:'USD 64.33',liabilities:'USD 51.20'})
 await shot('trade-currency-reports.png','accounting-report-result')
 const controls=id=>page.evaluate(`[...document.querySelectorAll(${q(t(id)+' tbody tr')})].map(tr=>[...tr.cells].map(c=>c.textContent))`)
 const aging=async(side,net,account)=>{
  assert.equal(await page.evaluate(`[...document.querySelector(${q(t('accounting-aging-side'))}).options].find(o=>o.value===${q(side)}).disabled`),false,side+' report available to this operator')
  await select('accounting-aging-side',side);await page.fill(t('accounting-aging-date'),today);await click('accounting-aging-run')
  await until(()=>pairs(t('accounting-aging-totals')).then(v=>v['Net outstanding']===net),`${side} net outstanding ${net}`)
  assert.ok((await page.text(t('accounting-aging-control-status'))).includes('agree'));const rows=await controls('accounting-aging-controls')
  assert.ok(rows.length>0&&rows.every(c=>c.at(-1)==='USD 0.00'),q(rows));assert.ok(rows.some(c=>c[0].startsWith(account+' ·')&&c[1]===net&&c[2]===net),q(rows))
  const api=await ok(`/erp/accounting/subledger-report?tenant=${tenant}&bookId=${book.id}&side=${side}&asOf=${today}`);assert.equal(api.postedSourceControlsAgree,true);assert.ok(api.controls.every(c=>c.differenceMinor==='0'),q(api.controls));return rows
 }
 const receivable=await aging('receivable','USD 38.73','AR');await shot('trade-currency-receivable-control.png','accounting-aging-controls')
 const payable=await aging('payable','USD 50.00','PAYABLE')
 await page.fill(t('accounting-inventory-date'),'2100-12-31');await click('accounting-inventory-run');await until(()=>has(t('accounting-inventory-result')),'inventory control comparison')
 assert.ok((await page.text(t('accounting-inventory-verdict'))).includes('explain every inventory account balance'));assert.ok((await page.text(t('accounting-inventory-pending'))).includes(': 0.'))
 const inventory=await controls('accounting-inventory-controls');assert.deepEqual(inventory.filter(c=>c[0].startsWith('INVENTORY ·')).map(c=>c.slice(1)),[['USD 25.60','USD 25.60','USD 0.00']],q(inventory));assert.ok(inventory.every(c=>c.at(-1)==='USD 0.00'),q(inventory))
 const inventoryApi=await ok(`/erp/accounting/inventory-control?tenant=${tenant}&bookId=${book.id}&asOf=2100-12-31`);assert.equal(inventoryApi.postedSourceControlsAgree,true);assert.ok(inventoryApi.controls.every(c=>c.differenceMinor==='0'))
 await shot('trade-currency-inventory-control.png','accounting-inventory-control')
 pass('CURRENCY-native-reports-and-controls','The native reports show revenue USD 38.73, cost of goods sold USD 25.60 and net profit USD 13.13 with 5 sources posted and none pending; the receivable (USD 38.73), payable (USD 50.00) and inventory (USD 25.60) control comparisons each show a difference of USD 0.00, as does the host')
 return{scope:'Owned synthetic USD company on the native screens: book, catalog, purchasing, supplier claim, sale, invoice, mail, source posting, reports and control comparisons. Companies, parties, the supplier, ledger accounts, the period and one spare product are created through the command API. No exchange rate, settlement, payment gateway, checkout document, return, void or period close; other non-IDR currencies and exponents are covered by unit and host tests only.',company:usd,idrCompany:idr,book:book.id,product:a.id,purchase:po.id,document:captured.id,sale:sale.id,invoice:invoice.id,journals,balances,lateBookRefusal:refusal,controls:{receivable,payable,inventory}}
}
