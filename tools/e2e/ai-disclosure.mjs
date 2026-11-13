/** Owned planner transport and native consent lifecycle. No external AI destination. */
import assert from 'node:assert/strict'
import {createServer} from 'node:http'
import {randomUUID,createHash} from 'node:crypto'

export async function startPlannerFixture(){
 const requests=[]
 const server=createServer(async(req,res)=>{
  try{
   assert.equal(req.method,'POST');assert.match(req.url,/^\/v1beta\/models\/owned-browser-planner:generateContent$/)
   assert.equal(req.headers['x-goog-api-key'],'owned-browser-planner-key')
   const chunks=[];let bytes=0
   for await(const chunk of req){bytes+=chunk.length;if(bytes>1048576)throw Error('Owned planner request exceeded its transport allowance');chunks.push(chunk)}
   const body=JSON.parse(Buffer.concat(chunks).toString('utf8')),text=body.contents?.[0]?.parts?.[0]?.text
   assert.equal(typeof text,'string');assert.ok(Buffer.byteLength(text)<=131072)
   const input=JSON.parse(text);requests.push(input)
   const answer=input.phase==='route'?{modules:['commerce']}:{summary:'Reviewed the approved bounded records.',questions:[],steps:[]}
   res.writeHead(200,{'content-type':'application/json'});res.end(JSON.stringify({candidates:[{finishReason:'STOP',content:{parts:[{text:JSON.stringify(answer)}]}}]}))
  }catch{res.writeHead(400,{'content-type':'application/json'});res.end('{"error":"owned_planner_contract_refused"}')}
 })
 await new Promise((resolve,reject)=>{server.once('error',reject);server.listen(0,'127.0.0.1',resolve)})
 return{requests,environment:{GEMINI_AGENT_MODEL:'owned-browser-planner',GEMINI_AGENT_API_KEY:'owned-browser-planner-key',GEMINI_AGENT_ENDPOINT:`http://127.0.0.1:${server.address().port}`,GEMINI_AGENT_THINKING_LEVEL:''},async close(){await new Promise((resolve,reject)=>server.close(e=>e?reject(e):resolve()))}}
}

export async function verifyAiDisclosure({page,origin,until,pass,fixture,suffix}){
 assert.equal(new URL(origin).hostname,'127.0.0.1');const tenant='acme-retail',t=id=>`[data-testid="${id}"]`
 const call=(method,path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(method)},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''},credentials:'same-origin',body:${body===undefined?'undefined':JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json()}))`)
 const ok=async(method,path,body)=>{const r=await call(method,path,body);assert.equal(r.status,200,`${path}: ${JSON.stringify(r.body)}`);return r.body}
 const owner=()=>ok('GET',`/own/config?tenant=${tenant}`)
 const open=async()=>{await page.navigate(`${origin}/app/owner`);await until(()=>page.evaluate(`!!document.querySelector('${t('owner-ai-enabled')}')&&!document.querySelector('${t('owner-ai-enabled')}').disabled`),'native current AI provider settings')}
 const review=()=>until(()=>page.evaluate(`!!document.querySelector('${t('owner-ai-review')}')&&!document.querySelector('${t('owner-confirm')}')?.disabled`),'signed native sharing review')
 const apply=async revision=>{await page.click(t('owner-confirm'));await until(()=>page.text(t('owner-status')).then(s=>s.includes(`committed revision ${revision}`)),'matching native sharing receipt')}
 const before=await owner(),base=before.config.revision
 const providerBeforeOwner=fixture.requests.length
 assert.equal(before.config.aiDisclosure??null,null,'The standard owned fixture starts without business-data consent')
 const companyPage=await ok('GET',`/erp/organizations/workspace?tenant=${tenant}&collection=companies&limit=100`)
 const company=companyPage.companies[0];assert.ok(company?.id,'Previous company acceptance supplies a real reviewed company')
 await open();assert.ok((await page.text(t('owner-ai-provider'))).includes('owned-browser-planner'))
 await page.click(t('owner-ai-enabled'))
 for(const id of ['owner-ai-area-commerce','owner-ai-area-organizations','owner-ai-class-conversation_text','owner-ai-class-record_references','owner-ai-class-business_facts','owner-ai-workspace'])await page.click(t(id))
 await until(()=>page.evaluate(`!!document.querySelector('${t(`owner-ai-company-${company.id}`)}')`),'visible company choice')
 await page.click(t(`owner-ai-company-${company.id}`));await page.click(t('owner-ai-preview'));await review()
 const display=await page.text(t('owner-ai-review'))
 assert.ok(display.includes(company.legalName??company.name));assert.ok(display.includes(company.code))
 assert.ok(display.includes('Current messages you send Ava'));assert.ok(display.includes('Statuses, quantities and dates'))
 assert.ok(!display.includes('owned-browser-planner-key'))
 await apply(base+1)
 const saved=await owner();assert.deepEqual(saved.config.aiDisclosure.modules,['commerce','organizations']);assert.deepEqual(saved.config.aiDisclosure.companies,[company.id]);assert.equal(saved.aiProvider.approvedForCurrentProvider,true)
 await page.evaluate(`document.querySelector('${t('owner-ai-settings')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('owner-ai-sharing-desktop.png')
 const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
 try{
  await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true})
  await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
  assert.equal(await page.evaluate('innerWidth'),390,'This must exercise the actual 390px layout')
  assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true)
  await page.evaluate(`document.querySelector('${t('owner-ai-settings')}').scrollIntoView({block:'start',behavior:'instant'})`)
  await page.screenshot('owner-ai-sharing-mobile.png')
 }finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
 // OWN-T03: reviewing and applying an owner configuration is not a provider effect.
 assert.equal(fixture.requests.length,providerBeforeOwner,'Owner preview and confirmation send nothing to the provider')
 pass('OWNER-native-preview-and-apply-send-no-provider-request','The owned provider endpoint receives no request while the owner previews, reviews and confirms the AI-sharing configuration')
 pass('AI-native-sharing-selection','Native settings selects the exact provider, categories, workspace and a named company; preview and signed confirmation persist one explicit policy without granting record access')
 const privateSku=`PRIVATE-AI-${suffix}`.toUpperCase(),privateTitle=`PRIVATE-AI-TITLE-${suffix}`
 const product=await ok('POST','/erp/commerce/commands',{tenant,requestId:randomUUID(),command:{operation:'product.create',sku:privateSku,title:privateTitle,unit:'unit',trackStock:false,price:77777}})
 const callsBefore=fixture.requests.length
 await page.navigate(`${origin}/app/desk`);await until(()=>page.evaluate(`!!document.querySelector('${t('agent-prompt')}')`),'native Ava goal input')
 await page.click(t('agent-new-thread'));await page.fill(t('agent-prompt'),'Review products for a bounded read.');await page.click(t('agent-send'))
 await until(()=>fixture.requests.length===callsBefore+1,'actual owned planner request')
 await until(()=>page.text(t('agent-conversation')).then(s=>s.includes('Reviewed the approved bounded records.')),'native Ava approved-data reply')
 const sent=fixture.requests.at(-1),wire=JSON.stringify(sent)
 assert.equal(sent.phase,'plan');assert.ok(!wire.includes(privateTitle));assert.ok(!wire.includes(privateSku));assert.ok(!wire.includes('77777'));assert.ok(!wire.includes('owned-browser-planner-key'))
 const source=sent.authorizedContext.sources.find(s=>s.module==='commerce')
 assert.ok(source.data.products.some(p=>p.id===product.receipt.id));assert.ok(source.data.products.every(p=>p.title===undefined&&p.price===undefined));assert.deepEqual(sent.history,[]);assert.deepEqual(sent.preferences,[])
 pass('AI-native-actual-egress-projection','A goal submitted through the native Ava UI reaches the owned HTTP provider with approved references/facts; actual private names, prices and raw history are absent from its request')
 await open();await page.click(t('owner-ai-revoke'));await review();assert.ok((await page.text(t('owner-ai-review'))).includes('AI sharing will stop'));await apply(base+2)
 assert.equal((await owner()).config.aiDisclosure??null,null)
 const atRevoke=fixture.requests.length
 const local=await ok('POST','/api/agent/turns',{tenant,requestId:randomUUID(),utterance:'Review products after sharing stopped.'})
 assert.equal(local.planner.providerId,'archava-agent-local');assert.equal(fixture.requests.length,atRevoke)
 pass('AI-native-revoke-local-fallback','Native reviewed revocation persists and the next business goal uses local planning with zero further provider requests')
 const final=await owner();assert.deepEqual(final.config.enabled,before.config.enabled);assert.deepEqual(final.config.terminology,before.config.terminology)
 return{scope:'Owned loopback planner, real native Owner/Ava UI, HTTP and PostgreSQL. External provider terms, safe historical memory, all financial agent goals and production operations remain separate.',providerRequests:fixture.requests.length-callsBefore,actualProviderInput:sent,providerInputSha256:createHash('sha256').update(wire).digest('hex'),providerInputBytes:Buffer.byteLength(wire),exactSelectedCompany:company.id,historyWithheld:true,unselectedNamesAndMoneyWithheld:true,revocationStoppedRequests:true,baselineRevision:base,finalRevision:base+2}
}
