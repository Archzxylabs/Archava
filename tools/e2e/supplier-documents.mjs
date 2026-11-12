/** Native unposted supplier claims, accepted-receipt capacity and recovery. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
export async function verifySupplierDocuments({page,origin,facts,until,pass,suffix,trade}){
 const t=id=>`[data-testid="${id}"]`,tenant='acme-retail',a=trade.company
 const view=()=>page.evaluate(`fetch('/erp/purchasing/workspace?tenant=acme-retail&companyId=${a}').then(async r=>({httpStatus:r.status,...await r.json()}))`)
 const post=command=>page.evaluate(`fetch('/erp/purchasing/commands',{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''},body:${JSON.stringify(JSON.stringify({tenant,requestId:randomUUID(),command}))}}).then(async r=>({httpStatus:r.status,body:await r.json()}))`)
 const sql=input=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)}
 const click=async id=>{await until(()=>page.evaluate(`(()=>{const n=document.querySelector('${t(id)}');return !!n&&!n.disabled&&n.getClientRects().length>0&&!n.closest('fieldset')?.disabled;})()`),'enabled native '+id);await page.click(t(id))}
 const shot=async(name,id)=>{await page.evaluate(`document.querySelector('${t(id)}').scrollIntoView({block:'center',behavior:'instant'})`);await page.screenshot(name)}
 await page.navigate(`${origin}/app/erp?module=purchasing`)
 await until(()=>page.evaluate(`!!document.querySelector('${t('trade-company-select')} option[value="${a}"]')`),'supplier document company option')
 await page.evaluate(`(()=>{const n=document.querySelector('${t('trade-company-select')}');n.value='${a}';n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${trade.purchase}`)}')`),'supplier document source PO');await page.click(t(`ops-open-${trade.purchase}`))
 await until(()=>page.evaluate(`!!document.querySelector('${t('supplier-doc-capture')}')`),'native claim capture section');await page.evaluate(`document.querySelector('${t('supplier-doc-capture')}').open=true`)
 await page.fill(t('supplier-doc-reference'),`VENDOR-${suffix}`);await page.fill(t('supplier-doc-date'),'2026-11-07');await page.fill(t('supplier-doc-total'),'21001');await page.fill(t('supplier-doc-quantity'),'3');await page.fill(t('supplier-doc-price'),'7000');await click('supplier-doc-save')
 const d=await until(async()=> (await view()).documents?.find(d=>d.vendorReference===`VENDOR-${suffix}`.toUpperCase()),'captured vendor document');assert.equal(d.state,'captured');assert.equal(d.accountingStatus,'not_inferred');assert.equal(d.declaredTotal,21001)
 await until(()=>page.evaluate(`!!document.querySelector('${t(`ops-open-${d.id}`)}')`),'native supplier document row');await page.click(t(`ops-open-${d.id}`));await page.fill(t('supplier-doc-reason'),'Check actual supplier amount');await click('supplier-doc-match')
 await until(async()=> (await view()).documents?.find(x=>x.id===d.id)?.state==='held','held vendor total discrepancy');await until(()=>page.text(t('supplier-document-findings')).then(s=>s.includes('declared total mismatch')),'painted held discrepancy')
 await shot('supplier-document-held.png','supplier-document-detail')
 pass('SUPPLIERDOC-native-held-claim','Explicit vendor reference/date/quantity/price/total captured via native form; a one-Rupiah total discrepancy remains held with visible findings and no receipt allocation')
 await click('supplier-doc-edit');await page.fill(t('supplier-doc-edit-total'),'21000');await page.fill(t('supplier-doc-edit-reason'),'Supplier confirmed goods total');await click('supplier-doc-revise')
 await until(async()=> (await view()).documents?.find(x=>x.id===d.id)?.revision===3,'corrected supplier claim version');await until(()=>page.text(t('supplier-document-state')).then(s=>s.startsWith('captured')),'painted corrected claim');await page.fill(t('supplier-doc-reason'),'Review corrected amount and accepted goods');await click('supplier-doc-match')
 await until(async()=> (await view()).documents?.find(x=>x.id===d.id)?.state==='matched','exact accepted goods match');await until(()=>page.text(t('supplier-document-state')).then(s=>s.includes('matched')),'native matched display');assert.equal(await page.evaluate(`!!document.querySelector('${t('supplier-doc-revise-form')}')`),false);await shot('supplier-document-matched.png','supplier-document-detail')
 pass('SUPPLIERDOC-native-correct-and-match','Explicit correction keeps vendor/source identity and history; exact price/total/accepted quantity allocates three accepted goods without accounting posting')
 await page.evaluate(`document.querySelector('${t('operator-action')}').closest('details').open=true`)
 await page.evaluate(`(()=>{const n=document.querySelector('${t('operator-action')}');n.value='document.void';n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
 await until(()=>page.evaluate(`!!document.querySelector('${t('operator-purchasing-id')}')`),'registered supplier document reference')
 let referenceResponse
 const referenceEvents=({data})=>{const e=JSON.parse(String(data));if(e.method==='Network.responseReceived'){const r=e.params.response;if(new URL(r.url).pathname==='/erp/purchasing/workspace'&&new URL(r.url).searchParams.has('search'))referenceResponse={url:r.url,status:r.status}}}
 page.ws.addEventListener('message',referenceEvents)
 try{
  assert.equal(await page.evaluate(`!document.querySelector('${t('operator-purchasing-id')}').closest('fieldset').disabled`),true)
  await page.evaluate(`document.querySelector('${t('operator-purchasing-id')}').closest('.operator-reference').querySelector('.ops-search button').click()`)
  await until(()=>referenceResponse?.status===200,'scoped reference search response')
  assert.equal(new URL(referenceResponse.url).searchParams.get('companyId'),a)
  await until(()=>page.evaluate(`!!document.querySelector('${t('operator-purchasing-id')} option[value="${d.id}"]')`),'company supplier document in registered action picker')
 }finally{page.ws.removeEventListener('message',referenceEvents)}
 pass('SUPPLIERDOC-scoped-registered-action-reference','Registered action record search retains selected company and offers the same authorized supplier document')

 const order=(await view()).orders.find(o=>o.id===trade.purchase),otherInput={operation:'document.capture',orderId:order.id,expectedOrderRevision:order.revision,supplierId:order.supplierId,vendorReference:`OTHER-${suffix}`,documentDate:'2026-11-07',currency:'IDR',declaredTotal:7000,lines:[{lineId:order.lines[0].id,quantity:1,unitCost:7000}]}
 const other=await post(otherInput);assert.equal(other.httpStatus,200,JSON.stringify(other.body));const otherId=(other.body.receipt??other.body).id
 assert.equal((await post({operation:'document.match',id:otherId,expectedRevision:1,expectedOrderRevision:order.revision,reason:'Cannot bill already allocated receipt'})).httpStatus,200)
 const held=(await view()).documents.find(x=>x.id===otherId);assert.equal(held.state,'held');assert.equal(held.activeAllocatedQuantity,0);assert.equal(held.matchEvidence.reasons[0].code,'insufficient_unallocated_accepted_goods')
 pass('SUPPLIERDOC-no-double-billing','A second claim cannot reuse accepted quantities already allocated to a matched supplier document')
 await page.fill(t('supplier-doc-reason'),'Withdraw claim for explicit review');await click('supplier-doc-void');await until(async()=> (await view()).documents?.find(x=>x.id===d.id)?.state==='void','void and receipt capacity release')
 await click('supplier-doc-edit');await page.fill(t('supplier-doc-edit-reason'),'Reopen same vendor reference after review');await click('supplier-doc-revise');await until(async()=> (await view()).documents?.find(x=>x.id===d.id)?.revision===6,'explicit reopen version');await page.fill(t('supplier-doc-reason'),'Recheck released accepted goods');await click('supplier-doc-match');await until(async()=> (await view()).documents?.find(x=>x.id===d.id)?.revision===7,'reconciled version seven');await until(()=>page.text(t('supplier-document-state')).then(s=>s.startsWith('matched')),'painted final matched claim')
 assert.equal((await post({...otherInput,vendorReference:` vendor-${suffix} `})).httpStatus,409)
 pass('SUPPLIERDOC-native-void-and-correction','Voiding releases current capacity and retains allocations/history; reopening is an explicit correction version, and duplicate vendor references stay reserved')
 const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio})')
 try{await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await page.evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true);await shot('supplier-document-mobile.png','supplier-document-detail')}finally{await page.send('Emulation.setDeviceMetricsOverride',{...viewport,mobile:false})}
 pass('SUPPLIERDOC-mobile','The current matched supplier document remains usable at 390px without document-level horizontal overflow')
 const request=sql(`SELECT request_id FROM archava_purchasing.supplier_document_versions WHERE document_id='${d.id}' AND revision=1;`);assert.match(request,/^[0-9a-f-]{36}$/)
 const original=await page.evaluate(`fetch('/erp/purchasing/commands/status?tenant=acme-retail&requestId=${request}').then(async r=>({httpStatus:r.status,...await r.json()}))`);assert.equal(original.httpStatus,200);assert.equal(original.receipt.revision,1)
 const grants=JSON.parse(sql("SELECT to_json(permissions) FROM archava_purchasing.memberships WHERE tenant_id='acme-retail' AND principal_id='alice';"))
 try{
  sql("UPDATE archava_purchasing.memberships SET permissions=array_remove(permissions,'documents.read') WHERE tenant_id='acme-retail' AND principal_id='alice';")
  await click('ops-refresh');await until(()=>page.evaluate(`!document.querySelector('${t('supplier-document-detail')}')`),'revoked document grant clears detail');assert.equal((await view()).documents.length,0)
  const status=await page.evaluate(`fetch('/erp/purchasing/commands/status?tenant=acme-retail&requestId=${request}').then(r=>r.status)`);assert.equal(status,403)
  pass('SUPPLIERDOC-revocation-and-historical-receipt','Removing document read clears native details and collection, and original command recovery rechecks current authority')
 }finally{assert.ok(grants.every(g=>/^[a-z.]+$/.test(g)));sql(`UPDATE archava_purchasing.memberships SET permissions=ARRAY[${grants.map(g=>`'${g}'`).join(',')}]::text[] WHERE tenant_id='acme-retail' AND principal_id='alice';`)}
 const proof=JSON.parse(sql(`SELECT json_build_object('company',d.company_id,'state',d.state,'revision',d.revision,'declaredTotal',d.declared_total,'versions',(SELECT count(*) FROM archava_purchasing.supplier_document_versions v WHERE v.document_id=d.id),'active',(SELECT sum(quantity) FROM archava_purchasing.supplier_document_allocations x WHERE x.document_id=d.id AND released_revision IS NULL),'released',(SELECT sum(quantity) FROM archava_purchasing.supplier_document_allocations x WHERE x.document_id=d.id AND released_revision IS NOT NULL),'heldHistory',(SELECT snapshot->>'state' FROM archava_purchasing.supplier_document_versions v WHERE v.document_id=d.id AND revision=2),'stock',(SELECT quantity FROM archava_commerce.stock_balances WHERE product_id='${trade.product}'),'inventoryValue',(SELECT sum(archava_commerce.cost_portion(total_cost_minor,original_quantity,original_quantity-remaining_quantity,remaining_quantity))::text FROM archava_commerce.inventory_layers WHERE product_id='${trade.product}' AND remaining_quantity>0),'invoiceState',(SELECT status FROM archava_billing.invoices WHERE id='${trade.invoice}')) FROM archava_purchasing.supplier_documents d WHERE id='${d.id}';`))
 assert.deepEqual(proof,{company:a,state:'matched',revision:7,declaredTotal:21000,versions:7,active:3,released:3,heldHistory:'held',stock:6,inventoryValue:'36000',invoiceState:'settled'})
 pass('SUPPLIERDOC-independent-SQL',proof)
 return {document:d.id,otherDocument:otherId,originalRequestId:request,company:a,proof}
}
