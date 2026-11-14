/** Native Ava review of an existing journal; amounts come from the real ledger. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'

export async function verifyAgentFinance({page,origin,facts,until,pass,suffix}){
 assert.equal(new URL(origin).hostname,'127.0.0.1')
 const tenant='acme-retail',t=id=>`[data-testid="${id}"]`
 const sql=statement=>{const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET archava.tenant_id='${tenant}';${statement}`,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim()}
 const post=async(path,body)=>{const r=await page.evaluate(`fetch(${JSON.stringify(path)},{method:'POST',headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''},body:${JSON.stringify(JSON.stringify(body))}}).then(async r=>({status:r.status,body:await r.json()}))`);assert.equal(r.status,200,JSON.stringify(r));return r.body}
 const org=async(type,data)=>(await post('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}})).receipt
 const finance=async command=>(await post('/erp/accounting/commands',{tenant,requestId:randomUUID(),command})).receipt
 const o=await org('create_organization',{name:`Ava finance ${suffix}`,legalName:`Ava Finance ${suffix} Official`})
 const c=await org('company.create_from_organization',{organizationId:o.organizationId,expectedSourceRevision:1,code:`AVA-FIN-${suffix}`.toUpperCase(),reason:'Owned native financial review acceptance'})
 const company=c.companyId
 const b=await finance({type:'book.create',companyId:company,expectedCompanyRevision:1,code:`AVA-FIN-${suffix}`.toUpperCase(),name:'Ava financial review book',currency:'IDR',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator policy'})
 const book=b.bookId
 const account=async(code,name,cls)=>(await finance({type:'account.create',bookId:book,code,name,class:cls,cashRole:'none',reason:'Explicit original reviewed account'})).resourceId
 const debit=await account('INVENTORY','Original Ava inventory','asset'),credit=await account('CAPITAL','Original Ava capital','equity')
 await finance({type:'period.open',bookId:book,code:'2026-11',startDate:'2026-11-01',endDate:'2026-11-30',reason:'Explicit owned reporting period'})
 const draft=async code=>(await finance({type:'journal.draft',bookId:book,code,date:'2026-11-09',sourceRef:`Owned ${code}`,memo:'Reviewed money from actual journal',reason:'Require explicit posting review',lines:[{accountId:debit,expectedAccountRevision:1,debitMinor:'705100',creditMinor:'0',memo:'Original debit'},{accountId:credit,expectedAccountRevision:1,debitMinor:'0',creditMinor:'705100',memo:'Original credit'}]})).resourceId
 const planned=async id=>post('/api/agent/turns',{tenant,companyId:company,requestId:randomUUID(),utterance:'Prepare this exact journal posting for my review.',action:{tool:'accounting.journal.post',values:{id,expectedRevision:1,reason:'Review original exact amounts before posting'}}})
 const open=async p=>{
  await page.navigate(`${origin}/app/desk`)
  await until(()=>page.evaluate(`!!document.querySelector('${t('agent-thread')} option[value="${p.threadId}"]')`),'financial conversation listed')
  await page.evaluate(`(()=>{const n=document.querySelector('${t('agent-thread')}');n.value=${JSON.stringify(p.threadId)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  await until(()=>page.evaluate(`!!document.querySelector('[data-goal="${p.goalId}"] ${t('agent-financial-journal')}')`),'native exact journal review')
  const text=await page.text(`[data-goal="${p.goalId}"] ${t('agent-financial-review')}`)
  for(const value of ['IDR 7051.00','Original Ava inventory','Original Ava capital','Ava financial review book'])assert.ok(text.includes(value),`Missing financial review ${value}: ${text}`)
 }
 const journal=await draft('AVA-NATIVE-REVIEW'),p=await planned(journal)
 assert.equal(p.planner.providerId,'archava-agent-local','Explicit typed business actions stay local')
 await open(p)
 assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`),'draft')
 await page.evaluate(`document.querySelector('[data-goal="${p.goalId}"] ${t('agent-financial-review')}').scrollIntoView({block:'start',behavior:'instant'})`)
 await page.screenshot('ava-financial-journal-desktop.png')
 const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
 try{
  await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true})
  await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
  assert.equal(await page.evaluate('innerWidth'),390,'This must exercise the actual 390px layout')
  assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true)
  assert.equal(await page.evaluate(`(()=>{const n=document.querySelector('[data-goal="${p.goalId}"] ${t('agent-financial-journal')}').parentElement;return n.scrollWidth<=n.clientWidth+1})()`),true,'All original account and money columns must fit the mobile review')
  await page.evaluate(`document.querySelector('[data-goal="${p.goalId}"] ${t('agent-financial-review')}').scrollIntoView({block:'start',behavior:'instant'})`)
  await page.screenshot('ava-financial-journal-mobile.png')
 }finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
 await page.click(`[data-goal="${p.goalId}"] ${t('agent-approve')}`)
 await until(()=>page.evaluate(`document.querySelector('[data-goal="${p.goalId}"]')?.dataset.state==='completed'`),'native approved journal committed',60000)
 assert.equal(sql(`SELECT state FROM archava_accounting.journals WHERE id='${journal}'`),'posted')
 assert.equal(sql(`SELECT sum(debit_minor)::text FROM archava_accounting.journal_lines WHERE journal_id='${journal}'`),'705100')
 pass('AVA-native-exact-financial-review','Native Ava shows IDR 7,051.00 and both original account names on desktop/mobile; no journal posts until explicit approval and its exact ledger amount is preserved')
 const staleJournal=await draft('AVA-NATIVE-STALE'),stale=await planned(staleJournal)
 await open(stale)
 await finance({type:'journal.post',id:staleJournal,expectedRevision:1,reason:'An explicit intervening authorized posting'})
 await page.click(`[data-goal="${stale.goalId}"] ${t('agent-approve')}`)
 await until(()=>page.evaluate(`document.querySelector('[data-goal="${stale.goalId}"]')?.dataset.state==='needs_review'`),'changed journal held for review')
 assert.equal(sql(`SELECT count(*) FROM archava_accounting.journals WHERE book_id='${book}' AND state='posted'`),'2')
 assert.equal(sql(`SELECT count(*) FROM archava_accounting.audit WHERE resource_kind='journal' AND resource_id='${staleJournal}'`),'2')
 pass('AVA-native-stale-financial-hold','An intervening authorized posting invalidates the stored journal review; native Ava requires a new review without a duplicate posting or audit effect')
 return{company,book,journal,staleJournal,amountMinor:'705100',currency:'IDR',postedJournals:2,staleGoalState:'needs_review',scope:'Owned native GL single-operator review. Independent posting is covered separately by HTTP tests; full financial agent workflows remain open.'}
}
