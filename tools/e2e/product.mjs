/** Requirement probes: missing features are findings, never passing acceptance. */
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { modules } from '../product/catalogue.mjs'
import { verifyNativeModules } from './native-modules.mjs'

export async function verifyProduct({page,origin,facts,until,output,operations,contact,media}) {
  assert.equal(new URL(origin).hostname,'127.0.0.1')
  assert.ok(Number.isInteger(facts.port)&&facts.port>1023&&facts.port<65536)
  const requirements=[]
  const observe=(id,expected,actual,met)=>{
    const item={id,expected,actual,status:met?'met':'gap'}
    requirements.push(item);writeFileSync(join(output,'product-observations.json'),JSON.stringify({auditCompleted:false,requirements},null,2)+'\n');process.stdout.write(`PRODUCT ${item.status.toUpperCase()} ${id}\n`);return item
  }
  const t=id=>`[data-testid="${id}"]`
  const sql=statement=>{
    const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:statement,encoding:'utf8',timeout:30000})
    assert.equal(r.status,0,r.stderr);return r.stdout.trim()
  }
  // All writes here target only the gate-owned database. Read probes need real
  // domain grants, so a fixture's missing grant is not mistaken for a feature gap.
  for(const module of modules) {
    const namespace=`archava_${module.id==='field'?'field':module.id==='education'?'edu':module.id}`
    const schemas={field:'archava_field',education:'archava_edu'}
    const schema=schemas[module.id]||namespace
    assert.match(schema,/^archava_[a-z_]+$/)
    const definitions=JSON.parse(sql(`SELECT coalesce(json_agg(pg_get_constraintdef(c.oid)),'[]') FROM pg_constraint c JOIN pg_class t ON t.oid=c.conrelid JOIN pg_namespace n ON n.oid=t.relnamespace WHERE n.nspname='${schema}' AND t.relname='memberships' AND c.contype='c' AND pg_get_constraintdef(c.oid) LIKE '%permissions%';`))
    const permissions=[...new Set(definitions.flatMap(d=>[...d.matchAll(/'([a-z][a-z.-]+)'/g)].map(m=>m[1])).filter(p=>p.includes('.')))]
    if(module.id==='field')permissions.push('field.read','field.write','field.dispatch','field.signoff','audit.read')
    if(module.id==='education')permissions.push('education.read','education.enroll','education.record_session','education.record_result','education.review','education.retain','audit.read')
    assert.ok(permissions.length,`Fixture permission vocabulary missing: ${schema}`)
    sql(`INSERT INTO ${schema}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail','alice','human',ARRAY[${permissions.map(p=>`'${p}'`).join(',')}],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=EXCLUDED.permissions,scope='all',active=true;`)
  }
  await page.send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false})
  await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()')
  await page.navigate(`${origin}/app`)
  await page.fill(t('login-username'),'alice@archava.test');await page.fill(t('login-password'),'correct horse battery staple');await page.fill(t('login-tenant'),'acme-retail');await page.click(t('login-submit'))
  await until(()=>page.evaluate(`location.pathname==='/app/contacts'&&!!document.querySelector('${t('contacts-count')}')`),'product-audit login')
  const call=(method,path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(method)},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body===undefined?'':`,body:${JSON.stringify(JSON.stringify(body))}`}}).then(async r=>({status:r.status,body:await r.json().catch(()=>null)}))`)
  const domains=[]
  for(const module of modules) {
    const path=module.uiPath||`/app/erp?module=${module.id}`
    await page.navigate(`${origin}${path}`)
    if(module.control)await until(()=>page.evaluate(`!!document.querySelector(${JSON.stringify(t(module.control))})`),`${module.id} operator controls`)
    const screen=await page.evaluate(`({heading:document.querySelector('h1')?.textContent,tabs:[...document.querySelectorAll('.ops-tabs button')].map(n=>n.textContent),control:${module.control?`!!document.querySelector(${JSON.stringify(t(module.control))})`:'false'},catalogFallback:!!document.querySelector('[data-testid=product-save]')})`)
    const read=module.readPath?await call('GET',`${module.readPath}?tenant=acme-retail`):null
    domains.push({id:module.id,label:module.label,url:path,screen,read:read?{path:module.readPath,status:read.status}:null})
    observe(`MODULE-${module.id}-operator`, 'Dedicated native screen exposes this domain primary create control',screen,!!module.uiPath&&screen.control)
    observe(`MODULE-${module.id}-read`, 'Mounted authenticated business collection read',read?{path:module.readPath,status:read.status}:{path:null,reason:'No collection read route mounted; command status is not a business list'},read?.status===200)
  }
  const open=async(module,id,detail)=>{
    await page.navigate(`${origin}/app/erp?module=${module}`)
    await until(()=>page.evaluate(`!!document.querySelector(${JSON.stringify(t(`ops-open-${id}`))})`),`${module} target row`)
    await page.click(t(`ops-open-${id}`))
    await until(()=>page.evaluate(`!!document.querySelector(${JSON.stringify(t(detail))})`),`${module} detail`)
  }
  const commerce=async()=>(await call('GET','/erp/commerce/workspace?tenant=acme-retail')).body
  await open('sales',operations.order,'sales-detail')
  await page.fill(t('return-quantity'),'1');await page.fill(t('return-reason'),'Product acceptance restock')
  await page.click(t('order-return'))
  await until(()=>page.text(t('sales-detail')).then(s=>s.includes('returned 1')),'partial restock reflected in UI')
  let workspace=await commerce(), product=workspace.products.find(p=>p.id===operations.product), order=workspace.orders.find(o=>o.id===operations.order)
  observe('COMMERCE-partial-restock','Returning one fulfilled item restores one unit and its original acquisition basis',{quantity:product.quantity,inventoryValueMinor:product.inventoryValueMinor,restockedCostMinor:order.restockedCostMinor},product.quantity===11&&product.inventoryValueMinor==='110000'&&order.restockedCostMinor==='10000')
  await page.evaluate(`(()=>{const n=document.querySelector('${t('return-disposition')}');n.value='discard';n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  await page.fill(t('return-reason'),'Product acceptance discard');await page.click(t('order-return'))
  await until(async()=>((await commerce()).orders.find(o=>o.id===operations.order)?.state==='returned'),'full return after discard')
  workspace=await commerce();product=workspace.products.find(p=>p.id===operations.product);order=workspace.orders.find(o=>o.id===operations.order)
  observe('COMMERCE-discard-return','Discarding the other item completes the return without adding inventory',{quantity:product.quantity,inventoryValueMinor:product.inventoryValueMinor,state:order.state},product.quantity===11&&product.inventoryValueMinor==='110000'&&order.state==='returned')
  await open('invoices',operations.invoice,'invoice-detail')
  await page.fill(t('invoice-reason'),'Product acceptance correction');await page.fill(t('invoice-correction'),'1000')
  await page.evaluate(`document.querySelector('${t('invoice-correction')}').closest('form').querySelector('button[type=submit]').click()`)
  await until(()=>page.text(t('invoice-detail')).then(s=>s.includes('31.000')),'invoice correction visible')
  let invoice=(await call('GET','/erp/billing/workspace?tenant=acme-retail')).body.invoices.find(i=>i.id===operations.invoice)
  observe('BILLING-correction','Appending a correction updates outstanding without claiming payment',{correction:invoice.correction,outstanding:invoice.outstanding,settled:invoice.settled},invoice.correction===1000&&invoice.outstanding===31000&&invoice.settled===0)
  await page.fill(t('invoice-reason'),'Product acceptance void');await page.click(t('invoice-void'))
  await until(async()=>((await call('GET','/erp/billing/workspace?tenant=acme-retail')).body.invoices.find(i=>i.id===operations.invoice)?.status==='void'),'invoice void committed')
  invoice=(await call('GET','/erp/billing/workspace?tenant=acme-retail')).body.invoices.find(i=>i.id===operations.invoice)
  observe('BILLING-void','Voiding an eligible invoice preserves the absence of settlement',{status:invoice.status,settled:invoice.settled},invoice.status==='void'&&invoice.settled===0)
  await open('support',operations.support,'case-detail')
  await page.fill(t('case-reason'),'Product acceptance lifecycle');await page.click(t('case-close'))
  const caseState=async()=>(await call('GET','/erp/support/cases?tenant=acme-retail')).body.items.find(c=>c.id===operations.support)?.state
  await until(async()=>(await caseState())==='closed','case close')
  await until(()=>page.evaluate(`!!document.querySelector('${t('case-reopen')}')&&!document.querySelector('${t('case-close')}')`),'closed case controls')
  await page.click(t('case-reopen'));await until(()=>page.evaluate(`!!document.querySelector('${t('case-start')}')`),'reopened case controls')
  observe('SUPPORT-close-reopen','Closed case can reopen with a new lifecycle cycle',{state:await caseState(),cycle:sql(`SELECT cycle FROM archava_support.cases WHERE id='${operations.support}'::uuid`)},(await caseState())==='open')
  await page.click(t('case-start'))
  const labelled=async label=>{
    await until(()=>page.evaluate(`!![...document.querySelectorAll('[data-testid=case-detail] button')].find(n=>n.textContent.trim()===${JSON.stringify(label)}&&!n.disabled)`),`support ${label} control`)
    await page.evaluate(`[...document.querySelectorAll('[data-testid=case-detail] button')].find(n=>n.textContent.trim()===${JSON.stringify(label)}&&!n.disabled).click()`)
  }
  await labelled('Waiting for reply');await until(async()=>(await caseState())==='waiting','case waiting')
  await labelled('Resume work');await until(async()=>(await caseState())==='in_progress','case resumed')
  observe('SUPPORT-wait-resume','Active case waits for a reply and resumes',{state:await caseState()},(await caseState())==='in_progress')
  const note=`Product acceptance ${randomUUID()}`
  await page.fill(t('case-note'),note);await page.click(t('case-note-save'))
  await until(()=>page.text(t('case-detail')).then(s=>s.includes(note)),'durable support note visible')
  const notes=Number(sql(`SELECT count(*) FROM archava_support.notes WHERE case_id='${operations.support}'::uuid AND body='${note}'`))
  observe('SUPPORT-note','Saved internal note appears once in the ledger and case history',{notes},notes===1)
  assert.match(contact,/^[0-9a-f-]{36}$/i)
  await page.navigate(`${origin}/app/contact?id=${contact}`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('record-save')}')`),'CRM contact editor')
  const organization=`Product audit ${randomUUID()}`
  await page.fill(t('record-edit-organization'),organization);await page.click(t('record-save'))
  await until(()=>page.text(t('record-organization')).then(s=>s===organization),'CRM edit persisted and rendered')
  observe('CRM-contact-edit','Contact edits persist through the operator screen',{organization:sql(`SELECT organization FROM archava_crm.contacts WHERE id='${contact}'::uuid`)},sql(`SELECT organization FROM archava_crm.contacts WHERE id='${contact}'::uuid`)===organization)
  const summary=`Product activity ${randomUUID()}`
  await page.fill(t('record-activity-summary'),summary);await page.click(t('record-activity-add'))
  await until(()=>page.text(t('record-activities')).then(s=>s.includes(summary)),'CRM activity recorded')
  const activities=Number(sql(`SELECT count(*) FROM archava_crm.activities WHERE contact_id='${contact}'::uuid AND summary='${summary}'`))
  observe('CRM-activity','Operator can record an activity once',{activities},activities===1)
  const title=`Product opportunity ${randomUUID()}`
  await page.fill(t('record-opportunity-title'),title);await page.click(t('record-opportunity-create'))
  await until(()=>page.text(t('record-opportunities')).then(s=>s.includes(title)),'pipeline opportunity created')
  const opportunity=sql(`SELECT id FROM archava_crm.opportunities WHERE contact_id='${contact}'::uuid AND title='${title}'`)
  observe('CRM-opportunity-create','Operator creates a durable opportunity',{id:opportunity},/^[0-9a-f-]{36}$/.test(opportunity))
  await page.evaluate(`(()=>{const n=document.querySelector('${t('record-move-stage')}');n.value='qualified';n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  await page.click(t('record-move'))
  await until(()=>page.text(t('record-opportunities')).then(s=>s.includes('qualified')),'pipeline stage moved')
  const stage=sql(`SELECT stage FROM archava_crm.opportunities WHERE id='${opportunity}'::uuid`)
  observe('CRM-opportunity-move','Operator advances the opportunity and sees persisted history',{stage},stage==='qualified')
  const config=await call('GET','/own/config?tenant=acme-retail')
  assert.equal(config.status,200)
  const revision=config.body.config.revision
  const chat=await call('POST','/own/chat',{tenant:'acme-retail',message:'disable crm-pipeline'})
  const dashboard=await call('POST','/own/plan',{tenant:'acme-retail',request:{baseRevision:revision,disable:['crm-pipeline']}})
  observe('OWN-bounded-chat-parity','Supported chat and dashboard produce one plan identity',{chatStatus:chat.status,dashboardStatus:dashboard.status,chatHash:chat.body?.plan?.planHash,dashboardHash:dashboard.body?.plan?.planHash},chat.status===200&&dashboard.status===200&&!!chat.body?.plan?.planHash&&chat.body.plan.planHash===dashboard.body.plan?.planHash)
  const discovery=await call('POST','/own/chat',{tenant:'acme-retail',message:'Bisnis saya hotel, tambahkan laundry tanpa restaurant'})
  observe('OWN-business-discovery','Business-language request yields a structured supported plan or capability-gap conversation',{status:discovery.status,body:discovery.body},discovery.status===200&&!!discovery.body?.plan)
  observe('OWN-business-modules','Owner catalogue exposes the supported business modules, beyond CRM configuration',{modules:config.body.modules.map(m=>m.id)},config.body.modules.some(m=>m.id.includes('laundry')))
  await page.navigate(`${origin}/app/owner`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('owner-modules')}')`),'owner product screen')
  const ownerScreen=await page.evaluate(`({preview:!!document.querySelector('${t('owner-preview')}'),restoreControls:document.querySelectorAll('[data-testid^=owner-restore-]').length,chatInput:!!document.querySelector('input[data-testid*=chat],textarea[data-testid*=chat],input[name=message],textarea[name=message]')})`)
  observe('OWN-chat-ui','Owner can submit a configuration conversation from the first-party UI',ownerScreen,ownerScreen.chatInput)
  const missionSku=`MISSION-${randomUUID().slice(0,8)}`.toUpperCase()
  const mission=await call('POST','/api/v1/cockpit/missions',{tenant_id:'acme-retail',mission_code:`product-write-${randomUUID()}`,title:'Verified product action',description:'Synthetic authorized native business action',auto_launch:true,steps:[{step_index:0,step_name:'Create product',tool_name:'commerce.product.create',payload_input:{sku:missionSku,title:'Verified mission product',unit:'unit',trackStock:true,price:1000},depends_on:[]}]})
  assert.ok([200,201].includes(mission.status),JSON.stringify(mission.body))
  const approve=await call('POST',`/api/v1/cockpit/missions/${mission.body.id}/steps/0/approve?tenant=acme-retail`,{})
  assert.equal(approve.status,200,JSON.stringify(approve.body))
  const run=await call('POST',`/api/v1/cockpit/missions/${mission.body.id}/run-next?tenant=acme-retail`,{stepIndex:0})
  const replay=await call('POST',`/api/v1/cockpit/missions/${mission.body.id}/run-next?tenant=acme-retail`,{stepIndex:0})
  const missionRows=Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='${missionSku}'`))
  observe('COCKPIT-business-write','Approved mission executes a real business command once and records its verified receipt',{missionStatus:mission.status,executionStatus:run.status,replayStatus:replay.status,missionRows,body:run.body},run.status===200&&replay.status===200&&run.body.status==='touchdown'&&missionRows===1)
  // Make one stock observation actionable using a real current supplier offer.
  const feedSku=`FEED-${randomUUID().slice(0,8)}`.toUpperCase()
  const feedProduct=await call('POST','/erp/commerce/commands',{tenant:'acme-retail',requestId:randomUUID(),command:{operation:'product.create',sku:feedSku,title:'Synthetic feed stock',unit:'unit',trackStock:true,price:10000}})
  const purchaseView=(await call('GET','/erp/purchasing/workspace?tenant=acme-retail')).body
  const supplier=purchaseView.suppliers.find(s=>s.active)
  assert.ok(supplier)
  const offer=await call('POST','/erp/purchasing/commands',{tenant:'acme-retail',requestId:randomUUID(),command:{operation:'offer.create',supplierId:supplier.id,productId:feedProduct.body.receipt.id,unitCost:5000}})
  assert.equal(offer.status,200,JSON.stringify(offer.body))
  await page.navigate(`${origin}/app/erp?module=feed`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('feed-review')}')`),'real native feed card')
  const feed=await call('GET','/api/v1/feed/action-cards?tenant=acme-retail&profile=retail_commerce')
  const card=feed.body.feed.cards.find(c=>c.title.includes('Synthetic feed stock'));assert.ok(card)
  await page.click(t('feed-review'));await until(()=>page.evaluate(`!!document.querySelector('${t('workflow-confirm')}')`),'native feed review')
  await page.click(t('workflow-confirm'));await until(()=>page.evaluate(`!!document.querySelector('${t('workflow-receipt')}')`),'durable native feed receipt')
  const executed=await call('POST',`/api/v1/feed/action-cards/${card.id}/execute`,{tenant:'acme-retail',plan_hash:card.plan_hash,confirmed:true})
  const feedRows=Number(sql(`SELECT count(*) FROM archava_purchasing.commands WHERE request_id='${card.id}'::uuid AND receipt IS NOT NULL`))
  observe('FEED-dispatch','Real native stock card creates one authorized purchase draft and replays its durable receipt',{feedStatus:feed.status,executeStatus:executed.status,receiptRows:feedRows,body:executed.body},executed.status===200&&feedRows===1)
  await page.navigate(`${origin}/app/erp?module=import`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('import-preview')}')`),'native import form')
  const importName=`Native import ${randomUUID()}`
  await page.fill(t('import-csv'),`name,email\n${importName},import@archava.test`);await page.click(t('import-preview'))
  await until(()=>page.evaluate(`!!document.querySelector('${t('workflow-confirm')}')`),'native import review')
  await page.click(t('workflow-confirm'));await until(()=>page.evaluate(`!!document.querySelector('${t('workflow-receipt')}')`),'atomic native import receipt')
  const imported=Number(sql(`SELECT count(*) FROM archava_crm.contacts WHERE full_name='${importName}'`))
  observe('IMPORT-durable-commit','Native CSV review commits an authorized durable record and its batch receipt',{imported,display:await page.text(t('workflow-receipt'))},imported===1)
  observe('VOICE-production-media','Actual native mic/camera, LiveKit and model media bridge are exercised',{media:media??null},media?.verified===true)
  await verifyNativeModules({page,origin,until,call,sql,observe,contact,operations})
  // Validate real assistant reads and human-confirmed writes, then deliberately
  // lose/mangle a response after the database commits. No provider is contacted.
  await page.navigate(`${origin}/app/desk`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('desk-overview')}')`),'assistant overview')
  const desk=await page.evaluate(`({source:document.querySelector('${t('desk-source')}')?.textContent,planner:document.querySelector('${t('desk-planner')}')?.textContent,contacts:document.querySelector('${t('desk-overview-contacts')}')?.textContent})`)
  observe('ASSISTANT-sql-read','Assistant overview derives from current CRM SQL',desk,desk.source==='crm'&&Number(desk.contacts)>0)
  await page.click(t('desk-ask-worklist'))
  await until(()=>page.evaluate(`document.querySelectorAll('${t('desk-prepare-complete')}').length>=2`),'assistant actionable worklist')
  const firstFollowup=await page.evaluate(`document.querySelector('${t('desk-prepare-complete')}').dataset.followup`)
  assert.match(firstFollowup,/^[0-9a-f-]{36}$/i)
  await page.click(t('desk-prepare-complete'))
  await until(()=>page.evaluate(`!!document.querySelector('${t('desk-confirm-complete')}')`),'assistant review')
  await page.click(t('desk-confirm-complete'))
  await until(()=>page.text(t('desk-action')).then(s=>s.includes('Recorded')),'assistant committed write')
  const firstStatus=sql(`SELECT status FROM archava_crm.followups WHERE id='${firstFollowup}'::uuid AND tenant_id='acme-retail'`)
  observe('ASSISTANT-confirmed-write','Human confirmation commits a real follow-up completion',{action:await page.text(t('desk-action')),followupId:firstFollowup,databaseStatus:firstStatus},firstStatus==='completed')
  await page.click(t('desk-ask-worklist'))
  await until(()=>page.evaluate(`!!document.querySelector('${t('desk-prepare-complete')}')`),'next assistant follow-up')
  await page.click(t('desk-prepare-complete'));await until(()=>page.evaluate(`!!document.querySelector('${t('desk-confirm-complete')}')`),'second assistant review')
  await page.evaluate(`(() => {window.__productFetch=window.fetch.bind(window);window.__productSubmitted=null;window.__productResponseDone=false;window.fetch=async function(input,init){const req=input instanceof Request?new Request(input.clone(),init):new Request(input,init);if(req.method==='POST'&&new URL(req.url).pathname==='/crm/commands'){window.__productSubmitted=await req.clone().json();const answer=await window.__productFetch(input,init);window.__productResponseDone=true;if(answer.ok)return new Response(JSON.stringify({decision:'receipt',requestId:'wrong-request',receipt:{id:'wrong-resource',revision:1}}),{status:200,headers:{'content-type':'application/json'}});return answer;}return window.__productFetch(input,init);};})()`)
  await page.click(t('desk-confirm-complete'))
  await until(()=>page.evaluate('window.__productResponseDone'),'assistant response to mismatched receipt')
  await until(()=>page.text(t('desk-action')).then(s=>!s.includes('Submitting')),'assistant acknowledgement classified')
  const wrong=await page.evaluate(`({action:document.querySelector('${t('desk-action')}')?.textContent,request:window.__productSubmitted})`)
  await page.evaluate('window.fetch=window.__productFetch')
  assert.ok(wrong.request?.requestId)
  const receipt=await call('GET',`/crm/commands/status?tenant=acme-retail&requestId=${encodeURIComponent(wrong.request.requestId)}`)
  observe('ASSISTANT-exact-receipt','Mismatched acknowledgement remains uncertain and preserves the submitted reference',{display:wrong.action,submittedRequestId:wrong.request.requestId,serverStatus:receipt.body?.status,serverReceipt:receipt.body?.receipt,acknowledgement:{requestId:'wrong-request',resourceId:'wrong-resource'}},!wrong.action.includes('Recorded'))
  await page.navigate(`${origin}/app/desk`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('desk-recover')}')`),'Assistant original reference survives reload')
  await page.click(t('desk-recover'));await until(()=>page.text(t('desk-action')).then(s=>s.includes('Recorded')),'Assistant recovers its exact original receipt')
  const recoveredCount=Number(sql(`SELECT count(*) FROM archava_crm.commands WHERE request_id='${wrong.request.requestId}'::uuid AND receipt IS NOT NULL`))
  observe('ASSISTANT-durable-recovery','Reload preserves original completion identity and recovery observes exactly one effect',{recoveredCount,display:await page.text(t('desk-action'))},recoveredCount===1)
  await page.screenshot('product-assistant-receipt.png')
  await page.navigate(`${origin}/app/contacts`)
  const result={auditCompleted:true,allObservedRequirementsMet:requirements.every(r=>r.status==='met'),scope:'Current first-party product journey probes; future backlog and live providers are separate',requirements,domains,counts:{met:requirements.filter(r=>r.status==='met').length,gaps:requirements.filter(r=>r.status==='gap').length}}
  writeFileSync(join(output,'product-observations.json'),JSON.stringify(result,null,2)+'\n');return result
}
