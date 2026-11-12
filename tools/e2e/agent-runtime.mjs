/** Current Ava goal journey. Every success assertion requires durable effects/receipts. */
import assert from 'node:assert/strict'
import { randomUUID } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { sourceManifest, sha256 } from '../rust/provenance.mjs'

export async function verifyAgentRuntime({page,origin,facts,until,output,pass}) {
  const checks=[],traces=[],suffix=randomUUID().slice(0,8).toUpperCase(),t=id=>`[data-testid="${id}"]`
  const save=()=>writeFileSync(join(output,'agent-runtime.json'),JSON.stringify({checks,traces},null,2)+'\n')
  const met=(id,details)=>{checks.push({id,status:'passed',details});save();pass(`AVA-${id}`,details)}
  const sql=statement=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:statement,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim()}
  const catalogue=JSON.parse(readFileSync(new URL('../../config/operator-actions.v1.json',import.meta.url),'utf8'))
  // Install explicit synthetic test memberships; production authorization is unchanged.
  for(const m of catalogue.modules){assert.match(m.namespace,/^archava_[a-z_]+$/);sql(`INSERT INTO ${m.namespace}.memberships(tenant_id,principal_id,kind,permissions,scope,active) VALUES('acme-retail','alice','human',ARRAY[${m.permissions.map(p=>{assert.match(p,/^[a-z][a-z_.-]+$/);return `'${p}'`}).join(',')}],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=EXCLUDED.permissions,scope='all',active=true;`)}
  await page.send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false})
  await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await page.navigate(`${origin}/app`)
  await page.fill(t('login-username'),'alice@archava.test');await page.fill(t('login-password'),'correct horse battery staple');await page.fill(t('login-tenant'),'acme-retail');await page.click(t('login-submit'))
  await until(()=>page.evaluate(`location.pathname==='/app/contacts'&&!!document.querySelector('${t('contacts-count')}')`),'Ava acceptance login')
  const call=async(method,path,body)=>{const r=await page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(method)},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body===undefined?'':`,body:${JSON.stringify(JSON.stringify(body))}`}}).then(async r=>({status:r.status,body:await r.json()}))`);traces.push({method,path,input:body??null,...r});save();return r}
  const post=async(path,body)=>{const r=await call('POST',path,body);assert.ok([200,201].includes(r.status),`${path}: ${JSON.stringify(r)}`);return r.body}
  const boundedContext=goal=>{
    const sources=(goal.context?.sources||[]).filter(s=>s.available===true)
    for(const source of sources){
      assert.equal(source.byteBudget.limit,32768)
      assert.ok(source.byteBudget.dataBytes<=32768)
      assert.ok(Buffer.byteLength(JSON.stringify(source.data),'utf8')<=32768)
      for(const[key,coverage]of Object.entries(source.coverage||{})){
        assert.equal(coverage.included,source.data[key].length)
        if(coverage.limitedByBytes){assert.equal(coverage.wholeCollection,false);assert.equal(coverage.moreRowsInSourcePage===true||coverage.moreRowsInLookupResults===true,true)}
      }
    }
    return{sourceByteLimit:32768,maximumDataBytes:Math.max(0,...sources.map(s=>s.byteBudget.dataBytes)),limitedSources:sources.filter(s=>s.byteBudget.limited).map(s=>s.module)}
  }

  const turn=(utterance,extra={})=>post('/api/agent/turns',{tenant:'acme-retail',requestId:randomUUID(),utterance,...extra})
  const approve=g=>post('/api/agent/approve',{tenant:'acme-retail',goalId:g.id,planHash:g.plan_hash,confirmed:true})
  const rows=(namespace,table)=>Number(sql(`SELECT count(*) FROM archava_${namespace}.${table} WHERE tenant_id='acme-retail'`))
  const product=await post('/erp/commerce/commands',{tenant:'acme-retail',requestId:randomUUID(),command:{operation:'product.create',sku:`AVA-STOCK-${suffix}`,title:`Ava stock ${suffix}`,unit:'unit',trackStock:true,price:10000}})
  const supplier=await post('/erp/purchasing/commands',{tenant:'acme-retail',requestId:randomUUID(),command:{operation:'supplier.create',code:`AVA-S-${suffix}`,name:'Owned Ava supplier'}})
  await post('/erp/purchasing/commands',{tenant:'acme-retail',requestId:randomUUID(),command:{operation:'offer.create',supplierId:supplier.receipt.id,productId:product.receipt.id,unitCost:5000}})
  const before=rows('purchasing','orders')
  await page.navigate(`${origin}/app/home`);await page.fill(t('home-agent-goal'),'Review low stock and prepare replenishment for my approval.');await page.click(t('home-agent-start'))
  await until(()=>page.evaluate(`location.pathname==='/app/desk'&&!!document.querySelector('${t('agent-approve')}')&&!document.querySelector('${t('agent-approve')}').disabled`),'Home goal becomes reviewed plan',60000)
  assert.equal(rows('purchasing','orders'),before)
  await page.screenshot('ava-reviewed-stock-plan.png');met('home-goal-review-no-hidden-write',{before,after:rows('purchasing','orders')})
  const goalId=await page.evaluate(`document.querySelector('${t('agent-goal')}').dataset.goal`)
  await page.click(t('agent-approve'))
  await until(()=>page.evaluate(`document.querySelector('[data-goal="${goalId}"]')?.dataset.state==='completed'`),'Ava approved plan completes',60000)
  const effect=Number(sql(`SELECT count(*) FROM archava_purchasing.orders o JOIN archava_purchasing.lines l ON l.order_id=o.id WHERE l.product_id='${product.receipt.id}'::uuid`));assert.equal(effect,1)
  await page.click(`[data-goal="${goalId}"] ${t('agent-goal-report')}`)
  await until(()=>page.text(t('agent-status')).then(s=>s.includes('verified completion')),'Ava receipt-backed report')
  await page.screenshot('ava-completed-receipts.png')
  await page.navigate(`${origin}/app/desk`);await until(()=>page.evaluate(`document.querySelector('[data-goal="${goalId}"]')?.dataset.state==='completed'`),'Ava durable UI reload')
  const g=(await call('GET',`/api/agent/state?tenant=acme-retail&goalId=${goalId}`)).body.goal
  await approve(g);assert.equal(Number(sql(`SELECT count(*) FROM archava_purchasing.orders o JOIN archava_purchasing.lines l ON l.order_id=o.id WHERE l.product_id='${product.receipt.id}'::uuid`)),1)
  met('approval-execution-report-reload-replay',{goalId,verifiedSteps:g.outcome.steps.filter(s=>s.payload_output?.verified).length,productPurchaseOrders:1})
  const memory=await turn('Ingat selalu: minta approval sebelum pembelian.');const recall=await turn('Apa preferensi yang sudah saya simpan?',{threadId:memory.threadId});assert.equal(recall.data.kind,'memory_recall');assert.ok(recall.data.items.some(p=>p.content.includes('minta approval')));met('durable-conversational-memory',{items:recall.data.items.length})
  const schedule=await turn('Cari jadwal resource yang bentrok dan ajukan pemindahan reservasi.');assert.ok(schedule.modules.includes('scheduling'));assert.ok(!schedule.modules.includes('crm'));assert.equal(schedule.goal.state,'clarifying');met('scheduling-clarification-correct-domain',{modules:schedule.modules})
  // Recover a deliberately lost acknowledgement without resubmitting the turn.
  await page.navigate(`${origin}/app/desk`);await until(()=>page.evaluate(`!!document.querySelector('${t('agent-send')}')&&!document.querySelector('${t('agent-send')}').disabled`),'Ava pending fixture ready')
  await page.evaluate(`(()=>{window.__avaFetch=window.fetch.bind(window);window.fetch=async(input,init)=>{const r=await window.__avaFetch(input,init);if(new URL(input instanceof Request?input.url:input,location.origin).pathname==='/api/agent/turns'&&r.ok){window.fetch=window.__avaFetch;throw new TypeError('Owned lost planning acknowledgement')}return r;};})()`)
  await page.fill(t('agent-prompt'),'Review project milestones');await page.click(t('agent-send'));await until(()=>page.evaluate(`!!document.querySelector('${t('agent-recover')}')`),'lost Ava response retained')
  await until(()=>page.text(t('agent-status')).then(s=>s.includes('unconfirmed')),'lost response completed');await page.click(t('agent-recover'));await until(()=>page.evaluate(`!document.querySelector('${t('agent-recover')}')`),'original Ava turn recovered');met('lost-turn-response-recovery',{retainedOriginalRequest:true})
  // Native action inputs share the planner/review/mission path for every domain.
  // Use an existing authorized contact; no synthetic invalid command is needed.
  const contactId=sql("SELECT id::text FROM archava_crm.contacts WHERE tenant_id='acme-retail' ORDER BY created_at LIMIT 1")
  const time=Date.now()+86400000
  const cases=[
    ['crm','crm','contacts','contact.create',{data:{fullName:`Ava contact ${suffix}`,email:`ava-${suffix}@archava.test`}}],
    ['commerce','commerce','products','product.create',{sku:`AVA-${suffix}`,title:`Ava product ${suffix}`,unit:'unit',trackStock:true,price:1000}],
    ['purchasing','purchasing','suppliers','supplier.create',{code:`AVA-NEW-${suffix}`,name:`Ava supplier ${suffix}`}],
    ['billing','billing','invoices','create_invoice',{invoiceCode:`AVA-INV-${suffix}`,customerRef:'owned-customer',currency:'IDR',consumerRef:'owned-consumer',lines:[{description:'Reviewed service',quantity:1,unitMinor:1000}]}],
    ['support','support','cases','case.create',{requesterName:'Ava customer',subject:`Ava case ${suffix}`,description:'Owned reviewed case',priority:'normal'}],
    ['organizations','organizations','organizations','create_organization',{name:`Ava organization ${suffix}`}],
    ['proposals','proposals','proposals','create_proposal',{title:`Ava proposal ${suffix}`,summary:'Reviewed scope',scopeItems:[{title:'Delivery',description:'Owned scope',estimatedDays:1,estimatedValue:1000,deliverableType:'service'}]}],
    ['projects','projects','projects','project.create',{title:`Ava project ${suffix}`,scopeSummary:'Reviewed project scope'}],
    ['foodservice','foodservice','tables','table.create',{code:`A_${suffix}`.toUpperCase(),title:`Ava table ${suffix}`,seats:4}],
    ['scheduling','scheduling','resources','resource.create',{title:`Ava resource ${suffix}`,kind:'staff',timezone:'Asia/Jakarta'}],
    ['accommodation','accommodation','properties','property.create',{title:`Ava property ${suffix}`,timezone:'Asia/Jakarta',checkInTime:'14:00',checkOutTime:'11:00'}],
    ['assets','assets','assets','create_asset',{code:`AVA-ASSET-${suffix}`,name:`Ava asset ${suffix}`,category:'equipment',ownership:'business_owned',condition:'good'}],
    ['subscriptions','subscriptions','subscriptions','create_subscription',{contactId,planCode:`AVA-PLAN-${suffix}`,planName:'Reviewed plan',initialEntitlements:[],billingInterval:'monthly',unitPrice:10000,currency:'IDR',quantity:1}],
    ['attendance','attendance','sessions','create_session',{title:`Ava session ${suffix}`,sessionCode:`AVA-SESSION-${suffix}`,startTime:new Date(time).toISOString(),endTime:new Date(time+3600000).toISOString(),capacity:10}],
    ['laundry','laundry','orders','create_order',{orderCode:`AVA-L-${suffix}`,kind:'item',currency:'IDR',lines:[{itemRef:'shirt',quantity:2,unit:'piece',unitPriceMinor:1000}]}],
    ['field','field','jobs','create_job',{job:{jobId:`ava-job-${suffix}`,customerRef:'owned-customer',siteRef:'site-main',summary:'Reviewed service job',priority:'normal',appointmentRef:`ava-appointment-${suffix}`}}],
    ['fulfillment','fulfillment','jobs','create_job',{jobCode:`AVA-F-${suffix}`,kind:'dispatch',title:`Ava dispatch ${suffix}`}],
    ['education','edu','enrollments','admit_applicant',{enrollment:{enrollmentId:`ava-enrollment-${suffix}`,studentRef:`ava-student-${suffix}`,classRef:'class-main',termRef:'term-current'}}],
  ]
  for(const [module,namespace,table,action,values] of cases){
    const before=rows(namespace,table),input={tenant:'acme-retail',requestId:randomUUID(),utterance:`Prepare reviewed ${module} work`,action:{tool:`${module}.${action}`,values}}
    const p=await post('/api/agent/turns',input);assert.equal(p.goal.state,'planned');assert.equal(rows(namespace,table),before);const contextBudget=boundedContext(p.goal)
    const r=await approve(p.goal);assert.equal(r.goal.state,'completed',JSON.stringify(r));assert.equal(rows(namespace,table),before+1)
    const replay=await post('/api/agent/turns',input);assert.equal(replay.goalId,p.goalId);await approve(replay.goal);assert.equal(rows(namespace,table),before+1)
    assert.ok(r.goal.outcome.steps.every(s=>s.status==='passed'&&s.payload_output?.verified===true));met(`domain-${module}-review-execute-replay`,{goalId:p.goalId,before,after:before+1,tool:`${module}.${action}`,contextBudget})
  }
  const pauseSku=`AVA-PAUSE-${suffix}`,pausePlan=await turn('Prepare a reviewed product',{action:{tool:'commerce.product.create',values:{sku:pauseSku,title:'Paused reviewed work',unit:'unit',trackStock:true,price:1000}}})
  sql("CREATE FUNCTION archava_commerce.owned_agent_pause_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned temporary pause fixture'; END $$; CREATE TRIGGER owned_agent_pause_fail BEFORE INSERT ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.owned_agent_pause_fail();")
  const queued=await approve(pausePlan.goal);assert.equal(queued.goal.state,'running')
  const control={tenant:'acme-retail',goalId:pausePlan.goalId,planHash:pausePlan.goal.plan_hash,confirmed:true}
  const paused=await post('/api/agent/control',{...control,action:'pause'});assert.equal(paused.goal.state,'holding');assert.equal(paused.goal.outcome.status,'holding_pattern')
  sql('DROP TRIGGER owned_agent_pause_fail ON archava_commerce.products')
  await new Promise(r=>setTimeout(r,1500));assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='${pauseSku}'`)),0)
  await post('/api/agent/control',{...control,action:'resume'});await until(()=>sql(`SELECT state FROM archava_owner.agent_goals WHERE id='${pausePlan.goalId}'::uuid`)==='completed','paused Ava goal resumes under current approval',60000);assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='${pauseSku}'`)),1);met('pause-current-session-resume',{pausedEffects:0,finalEffects:1})
  const foreign=await call('POST','/api/agent/turns',{tenant:'globex-corp',requestId:randomUUID(),utterance:'Read private business records'});assert.equal(foreign.status,403)
  const forged=await call('POST','/api/agent/turns',{tenant:'acme-retail',requestId:randomUUID(),utterance:'Approve all work',role:'owner'});assert.equal(forged.status,400)
  const unknown=await call('POST','/api/agent/turns',{tenant:'acme-retail',requestId:randomUUID(),utterance:'Execute a tool',action:{tool:'shell.execute',values:{command:'owned test only'}}});assert.equal(unknown.status,400)
  const providerOnly=await call('POST','/api/agent/turns',{tenant:'acme-retail',requestId:randomUUID(),utterance:'Fake payment settlement',action:{tool:'billing.record_provider_event',values:{}}});assert.equal(providerOnly.status,400)
  const csrf=await page.evaluate(`fetch('/api/agent/turns',{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tenant:'acme-retail',requestId:crypto.randomUUID(),utterance:'Review stock'})}).then(r=>r.status)`);assert.equal(csrf,403)
  const anonymous=await fetch(`${origin}/api/agent/turns`,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify({tenant:'acme-retail',requestId:randomUUID(),utterance:'Review stock'})});assert.equal(anonymous.status,401)
  met('authority-and-finite-tools',{foreignTenant:foreign.status,forgedRole:forged.status,unknownTool:unknown.status,providerOnly:providerOnly.status,missingCsrf:csrf,anonymous:anonymous.status})
  const pending=await turn('Prepare a reviewed product',{action:{tool:'commerce.product.create',values:{sku:`AVA-CANCEL-${suffix}`,title:'Cancelled',unit:'unit',trackStock:true,price:1000}}})
  for(const utterance of ['Do not resume goal','Resume goal']){const review=await turn(utterance,{threadId:pending.threadId,goalId:pending.goalId});assert.equal(review.data.kind,'agent_control_review');assert.equal(review.goal.state,'planned');assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='AVA-CANCEL-${suffix}'`)),0)}
  met('chat-controls-require-explicit-confirmation',{negatedAndPositiveTextEffects:0})
  await post('/api/agent/control',{tenant:'acme-retail',goalId:pending.goalId,planHash:pending.goal.plan_hash,confirmed:true,action:'cancel'})
  const denied=await call('POST','/api/agent/approve',{tenant:'acme-retail',goalId:pending.goalId,planHash:pending.goal.plan_hash,confirmed:true});assert.equal(denied.status,409);assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='AVA-CANCEL-${suffix}'`)),0);met('cancel-no-effect',{approvalStatus:409})
  await page.navigate(`${origin}/app/desk`);await until(()=>page.evaluate(`!!document.querySelector('${t('agent-new-thread')}')`),'Ava action editor ready');await page.click(t('agent-new-thread'))
  await page.fill(t('agent-prompt'),'Prepare a reviewed product');await page.click(t('agent-send'))
  await until(()=>page.evaluate(`!!document.querySelector('.agent-action-details')`),'targeted clarification offers native details')
  await page.evaluate(`document.querySelector('.agent-action-details').open=true`)
  for(const [field,value] of Object.entries({sku:`AVA-UI-${suffix}`,title:`Ava UI product ${suffix}`,unit:'unit',price:'1000'}))await page.fill(t(`operator-commerce-${field}`),value)
  await page.click(t('agent-prepare-action'));await until(()=>page.evaluate(`!!document.querySelector('${t('agent-approve')}')&&!document.querySelector('${t('agent-approve')}').disabled`),'native fields become a new reviewed plan')
  assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='AVA-UI-${suffix}'`)),0)
  await page.click(t('agent-approve'));await until(()=>Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='AVA-UI-${suffix}'`))===1,'native action editor actual effect');met('native-details-to-reviewed-agent-action',{productRows:1})
  await page.navigate(`${origin}/app/desk`);await until(()=>page.evaluate(`!!document.querySelector('${t('agent-thread')} option[value="${g.thread_id}"]')`),'past conversation is listed')
  await page.evaluate(`(()=>{const n=document.querySelector('${t('agent-thread')}');n.value=${JSON.stringify(g.thread_id)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  await until(()=>page.evaluate(`document.querySelector('[data-goal="${goalId}"]')?.dataset.state==='completed'`),'past conversation restored');met('past-conversation-switch',{restoredGoal:goalId})
  await page.navigate(`${origin}/app/desk`);await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await until(()=>page.evaluate('document.documentElement.scrollWidth<=innerWidth'),'Ava mobile overflow');await page.screenshot('ava-mobile.png');assert.deepEqual(page.errors,[]);met('mobile-and-wasm',{overflow:false,runtimeErrors:0})
  const build=JSON.parse(readFileSync('target/site/build-provenance.json','utf8'));assert.equal(sourceManifest(process.cwd()).sha256,build.source.sha256)
  const result={verified:true,sourceSha256:build.source.sha256,scriptSha256:sha256('tools/e2e/agent-runtime.mjs'),checks,traces,scope:'Real Chromium, current goal API, PostgreSQL and actual registered effects across all 18 domains. Explicit forms are not evidence of model reasoning. Model protocol/ID binding and failure/revocation/replan are separately covered by Rust integration tests. No live provider or public deployment acceptance.'};writeFileSync(join(output,'agent-runtime.json'),JSON.stringify(result,null,2)+'\n');return result
}

export async function prepareAgentRestart({page,facts,until,pass}) {
  const sku=`AVA-RESTART-${randomUUID().slice(0,8)}`.toUpperCase()
  const sql=statement=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:statement,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim()}
  const post=async(path,body)=>{const r=await page.evaluate(`fetch(${JSON.stringify(path)},{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''},body:${JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json()}))`);assert.equal(r.status,200,JSON.stringify(r));return r.body}
  const plan=await post('/api/agent/turns',{tenant:'acme-retail',requestId:randomUUID(),utterance:'Prepare a reviewed product',action:{tool:'commerce.product.create',values:{sku,title:'Actual process restart recovery',unit:'unit',trackStock:true,price:1000}}})
  assert.equal(plan.goal.state,'planned')
  sql("CREATE FUNCTION archava_commerce.owned_agent_restart_fail() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned temporary business failure'; END $$; CREATE TRIGGER owned_agent_restart_fail BEFORE INSERT ON archava_commerce.products FOR EACH ROW EXECUTE FUNCTION archava_commerce.owned_agent_restart_fail();")
  const approval={tenant:'acme-retail',goalId:plan.goalId,planHash:plan.goal.plan_hash,confirmed:true}
  const failed=await post('/api/agent/approve',approval);assert.equal(failed.goal.state,'running',JSON.stringify(failed));assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='${sku}'`)),0)
  return async()=>{
    sql('DROP TRIGGER owned_agent_restart_fail ON archava_commerce.products')
    await until(()=>Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='${sku}'`))===1,'fresh production worker recovers original queued goal',60000)
    await until(()=>sql(`SELECT state FROM archava_owner.agent_goals WHERE id='${plan.goalId}'::uuid`)==='completed','fresh production worker persists outcome',60000)
    const replay=await post('/api/agent/approve',approval);assert.equal(replay.goal.state,'completed');assert.equal(Number(sql(`SELECT count(*) FROM archava_commerce.products WHERE sku='${sku}'`)),1)
    pass('AVA-process-kill-restart-continuation',{goalId:plan.goalId,productRows:1,verifiedReceipt:replay.goal.outcome.steps[0].payload_output.verified})
  }
}

export async function verifyAgentNavigation({page,origin,until,pass}) {
  await page.send('Emulation.setDeviceMetricsOverride',{width:1440,height:1000,deviceScaleFactor:1,mobile:false})
  await page.navigate(`${origin}/app/desk`)
  await until(()=>page.evaluate("!!document.querySelector('[data-testid=agent-send]')&&!document.querySelector('[data-testid=agent-send]').disabled"),'agent navigation fixture ready')
  await page.evaluate(`(()=>{window.__avaNavReady=false;window.__avaNavFetch=window.fetch.bind(window);window.fetch=async(input,init)=>{const r=await window.__avaNavFetch(input,init);if(new URL(input instanceof Request?input.url:input,location.origin).pathname==='/api/agent/turns'&&r.ok){window.fetch=window.__avaNavFetch;window.__avaNavReady=true;await new Promise(resolve=>setTimeout(resolve,2000));}return r;};})()`)
  await page.fill('[data-testid=agent-prompt]','Review project milestones')
  await page.click('[data-testid=agent-send]')
  await until(()=>page.evaluate('window.__avaNavReady===true'),'owned delayed agent response')
  await page.evaluate(`document.querySelector('a[href="/app/contacts"]').click()`)
  await until(()=>page.evaluate("location.pathname==='/app/contacts'&&!!document.querySelector('[data-testid=contacts-count]')"),'client navigation disposes the agent panel')
  await new Promise(resolve=>setTimeout(resolve,2500))
  assert.deepEqual(page.errors,[],'Late agent response must not read disposed UI signals')
  pass('AVA-late-response-after-navigation',{delayedResponseMs:2000,runtimeErrors:0})
  return {verified:true,delayedResponseMs:2000,runtimeErrors:0}
}
