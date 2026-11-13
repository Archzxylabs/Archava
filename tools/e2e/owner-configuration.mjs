/** Native Owner lifecycle with actual commit loss/recovery and immutable business digests. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
export async function verifyOwnerConfiguration({page,origin,facts,contact,until,pass}){
 const tenant='acme-retail',t=id=>`[data-testid="${id}"]`
 assert.equal(new URL(origin).hostname,'127.0.0.1');assert.match(contact,/^[0-9a-f-]{36}$/)
 const call=(method,path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(method)},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body===undefined?'':`,body:${JSON.stringify(JSON.stringify(body))}`}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const ok=async(method,path,body)=>{const r=await call(method,path,body);assert.equal(r.status,200,JSON.stringify(r));return r.body}
 const sql=statement=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET archava.tenant_id='${tenant}';${statement}`,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim()}
 const tables=['archava_crm.contacts','archava_crm.opportunities','archava_crm.opportunity_transitions','archava_crm.commands','archava_accounting.memberships','archava_accounting.books','archava_accounting.accounts','archava_accounting.periods','archava_accounting.journals','archava_accounting.journal_lines','archava_accounting.invoice_policies','archava_accounting.invoice_sources','archava_accounting.journal_sources','archava_accounting.commands','archava_accounting.audit','archava_accounting.purchase_policies','archava_accounting.purchase_origins','archava_accounting.purchase_sources','archava_accounting.purchase_journal_sources','archava_accounting.stock_layer_origins','archava_accounting.stock_policies','archava_accounting.stock_sources','archava_accounting.stock_journal_sources']
 const digests=()=>JSON.parse(sql(`SELECT jsonb_build_object(${tables.map(table=>`'${table}',(SELECT jsonb_build_object('rows',count(*),'digest',md5(coalesce(string_agg(to_jsonb(r)::text,'' ORDER BY to_jsonb(r)::text),''))) FROM ${table} r WHERE tenant_id='${tenant}')`).join(',')})`))
 const owner=()=>ok('GET',`/own/config?tenant=${tenant}`)
 const open=async()=>{await page.navigate(`${origin}/app/owner`);await until(()=>page.evaluate(`document.querySelector('${t('owner-preview')}')?.disabled===false`),'current Owner setup')}
 const readyPlan=()=>until(()=>page.evaluate(`document.querySelector('${t('owner-confirm')}')?.disabled===false`),'confirmable Owner plan')
 const applied=revision=>until(()=>page.text(t('owner-status')).then(s=>s.includes(`committed revision ${revision}`)),'matching Owner committed revision')
 const beforeConfig=await owner(),base=beforeConfig.config.revision
 assert.ok(base>0,'The standard native gate records a restorable baseline revision')
 const record=await ok('GET',`/crm/records?tenant=${tenant}&contact=${contact}`)
 await ok('POST','/crm/commands',{tenant,requestId:randomUUID(),command:{operation:'opportunity.create',contactId:contact,contactRevision:record.contact.revision,title:'Owner configuration acceptance opportunity'}})
 const before=digests()
 await open();assert.match(await page.text(t('owner-module-crm-pipeline')),/optional/)
 await page.click(t('owner-toggle-crm-pipeline'));await page.click(t('owner-preview'));await readyPlan()
 const disabledPreview=await ok('POST','/own/plan',{tenant,request:{baseRevision:base,disable:['crm-pipeline']}})
 assert.equal(disabledPreview.plan.blocked,null);assert.equal(disabledPreview.plan.changes.some(c=>c.module==='crm-pipeline'||c.id==='crm-pipeline'),true)
 const moduleDescription=await page.text(t('owner-module-crm-pipeline')),moduleSnapshot=beforeConfig.modules.find(m=>m.id==='crm-pipeline')
 for(const key of ['readiness','cost']){assert.equal(typeof moduleSnapshot[key],'string');assert.ok(moduleSnapshot[key].length>0);assert.ok(moduleDescription.includes(moduleSnapshot[key]))}
 assert.equal(disabledPreview.plan.effects.length,0,'This optional disable declares no extra activation/provider effects')
 await page.evaluate(`(()=>{const original=window.fetch.bind(window);window.__ownerApplyBodies=[];window.__ownerDrop=true;window.fetch=async(input,init)=>{const req=input instanceof Request?new Request(input.clone(),init):new Request(input,init);const apply=new URL(req.url).pathname==='/own/apply'&&req.method==='POST';if(apply)window.__ownerApplyBodies.push(await req.clone().json());const response=await original(input,init);if(apply&&response.ok&&window.__ownerDrop){window.__ownerDrop=false;throw new TypeError('Owned lost Owner acknowledgement after real commit')}return response;};})()`)
 await page.click(t('owner-confirm'));await until(()=>page.evaluate(`!!document.querySelector('${t('owner-pending-id')}')`),'retained Owner request after lost acknowledgement')
 const pending=(await page.text(t('owner-pending-id'))).trim(),sent=await page.evaluate('window.__ownerApplyBodies')
 assert.match(pending,/^[a-z0-9-]{1,128}$/);assert.equal(sent.length,1);assert.equal(sent[0].requestId,pending);assert.equal(sent[0].confirmation.planHash,disabledPreview.plan.planHash)
 await until(async()=> (await owner()).config.revision===base+1,'actual Owner commit before recovery')
 assert.equal(sql(`SELECT count(*) FROM archava_owner.apply_receipts WHERE tenant_id='${tenant}' AND request_id='${pending}'`),'1')
 await open();await until(()=>page.text(t('owner-status')).then(s=>s.includes(`recovered apply committed revision ${base+1}`)),'reload recovers original Owner receipt')
 assert.equal(await page.evaluate(`!!document.querySelector('${t('owner-pending-id')}')`),false)
 assert.equal((await owner()).modules.find(m=>m.id==='crm-pipeline').enabled,false)
 const denied=await call('POST','/crm/commands',{tenant,requestId:randomUUID(),command:{operation:'opportunity.create',contactId:contact,contactRevision:record.contact.revision,title:'Disabled module must refuse'}})
 assert.equal(denied.status,403)
 const disabledRecord=await ok('GET',`/crm/records?tenant=${tenant}&contact=${contact}`);assert.equal(disabledRecord.pipelineEnabled,false);assert.ok(disabledRecord.opportunities.some(o=>o.title==='Owner configuration acceptance opportunity'),'Authorized historical records are retained while the module is disabled')
 await page.navigate(`${origin}/app/contact?id=${contact}`);await until(()=>page.evaluate(`!!document.querySelector('${t('record-pipeline-disabled')}')&&!document.querySelector('${t('record-opportunity')}')`),'disabled native pipeline state')
 await open()
 assert.deepEqual(digests(),before)
 pass('OWNER-native-disable-lost-response-recovery','The native dashboard disables an optional module after review; the real commit acknowledgement is dropped, reload recovers the same receipt, direct CRM write is refused, and business/financial digests stay unchanged')
 await until(()=>page.evaluate(`!!document.querySelector('${t(`owner-restore-${base}`)}')`),'baseline Owner history')
 await page.click(t(`owner-restore-${base}`));await readyPlan();await page.click(t('owner-confirm'));await applied(base+2)
 assert.equal((await owner()).modules.find(m=>m.id==='crm-pipeline').enabled,true);assert.deepEqual(digests(),before)
 pass('OWNER-native-compatible-restore','Native history preview/confirmation restores the prior enabled set as a new revision without rolling back business or financial history')
 const label='Discovery <&>',expected=await ok('POST','/own/plan',{tenant,request:{baseRevision:base+2,terminology:{pipelineLabels:{new:label}}}})
 await page.evaluate(`(()=>{const original=window.fetch.bind(window);window.__ownerChat=[];window.fetch=async(input,init)=>{const req=input instanceof Request?new Request(input.clone(),init):new Request(input,init);const response=await original(input,init);if(new URL(req.url).pathname==='/own/chat'&&response.ok)window.__ownerChat.push(await response.clone().json());return response;};})()`)
 await page.fill(t('owner-chat-input'),`rename new to ${label}`);await page.click(t('owner-chat-preview'));await readyPlan()
 const chat=await until(()=>page.evaluate('window.__ownerChat[0]'),'native chat plan capture')
 for(const key of ['plan','baseConfig','catalogueVersion','pricingVersion'])assert.deepEqual(chat[key],expected[key],`Chat/manual ${key} identity`)
 assert.equal((await owner()).config.revision,base+2,'Preview is read-only')
 await page.click(t('owner-confirm'));await applied(base+3);assert.equal((await owner()).config.terminology.pipelineLabels.new,label);assert.deepEqual(digests(),before)
 pass('OWNER-native-chat-manual-plan-identity','The actual chat UI and manual contract share the exact plan, base configuration, catalogue and pricing identity; confirmation persists the supported label without changing canonical business rows')
 await page.navigate(`${origin}/app/contact?id=${contact}`);await until(()=>page.evaluate(`!!document.querySelector('${t('record-opportunity')}')`),'current CRM opportunity view')
 const labels=await page.evaluate(`[...document.querySelector('${t('record-move-stage')}').options].map(o=>({value:o.value,label:o.textContent}))`)
 assert.ok(labels.some(o=>o.value==='new'))
 const configuredLabelRendered=labels.some(o=>o.value==='new'&&o.label===label)
 assert.equal(configuredLabelRendered,true,'The current native CRM view must render its configured caption while preserving the canonical option value')
 assert.equal(await page.evaluate(`document.querySelector('${t('record-opportunity')}').querySelector('script,img')===null`),true,'Configured captions render as plain text')
 await page.evaluate(`document.querySelector('${t('record-opportunity')}').scrollIntoView({block:'center',behavior:'instant'})`)
 await page.screenshot('owner-configured-pipeline-label.png')
 await open();await until(()=>page.evaluate(`!!document.querySelector('${t(`owner-restore-${base}`)}')`),'Owner original history after label review');await page.click(t(`owner-restore-${base}`));await readyPlan();await page.click(t('owner-confirm'));await applied(base+4)
 const final=await owner();assert.deepEqual(final.config.terminology,beforeConfig.config.terminology);assert.deepEqual(final.config.enabled,beforeConfig.config.enabled);assert.deepEqual(digests(),before)
 pass('OWNER-native-label-restore-preserves-history',`Restoring the original supported configuration removes later labels as a new revision and preserves ${tables.length} independent business/financial table digests`)
 const gaps=configuredLabelRendered?[]:['The Owner label persists but the contact pipeline still renders canonical stage names. OWN-T01/T07 remain incomplete.']
 return{scope:'Owned native Owner configuration lifecycle. Contract consumers, live model discovery, all business profiles and real-user acceptance remain separate.',baselineRevision:base,finalRevision:base+4,lostResponseRequestId:pending,oneReceipt:true,chatManualPlanIdentity:true,tableDigestsVerified:tables.length,configuredLabelRendered,allObservedRequirementsMet:gaps.length===0,gaps}
}
