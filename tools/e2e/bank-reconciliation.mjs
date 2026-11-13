/** Owned manual bank evidence, native human review, independent review and exact SQL proof. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {spawnSync} from 'node:child_process'
import {writeFileSync} from 'node:fs'
import {join} from 'node:path'

export async function verifyBankReconciliation({page,origin,facts,until,pass,suffix,output}) {
  assert.equal(new URL(origin).hostname,'127.0.0.1'); assert.match(suffix,/^[0-9a-f]{8}$/)
  const tenant='acme-retail',t=id=>`[data-testid="${id}"]`,accountRef=id=>({accountId:id,expectedAccountRevision:1})
  const sql=input=>{
    const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='30s';SET archava.tenant_id='${tenant}';${input}`,encoding:'utf8',timeout:35000})
    assert.equal(r.status,0,r.stderr);return r.stdout.trim().split('\n').at(-1)
  }
  // Explicit test-only organization grants also make this journey runnable in isolation.
  // Existing memberships retain all their prior grants; production bootstrap is proved separately.
  for(const[actor,permissions]of [['alice',['organizations.read','organizations.write','companies.read','companies.write']],['bob',['organizations.read','companies.read']]]){
    sql(`INSERT INTO archava_organizations.memberships AS m(tenant_id,principal_id,kind,permissions,scope,active) VALUES('${tenant}','${actor}','human',ARRAY[${permissions.map(p=>`'${p}'`).join(',')}],'all',true) ON CONFLICT(tenant_id,principal_id) DO UPDATE SET permissions=ARRAY(SELECT DISTINCT p FROM unnest(m.permissions||EXCLUDED.permissions) p ORDER BY p)`)
  }
  const call=(path,body)=>page.evaluate(`fetch(${JSON.stringify(path)},{method:${JSON.stringify(body?'POST':'GET')},headers:{'content-type':'application/json','x-archava-csrf':localStorage.getItem('archava_csrf')||''}${body?`,body:${JSON.stringify(JSON.stringify(body))}`:''}}).then(async r=>({status:r.status,body:await r.json()}))`)
  const post=async(path,body)=>{const r=await call(path,body);assert.equal(r.status,200,JSON.stringify(r));return r.body.receipt??r.body}
  const command=command=>post('/erp/accounting/commands',{tenant,requestId:randomUUID(),command})
  const org=(type,data)=>post('/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}})
  const select=async(id,value)=>{
    await until(()=>page.evaluate(`!![...document.querySelector(${JSON.stringify(t(id))})?.options||[]].find(o=>o.value===${JSON.stringify(value)})`),id+' authorized option')
    await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.value=${JSON.stringify(value)};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
  }
  const fill=async(name,value)=>{
    const id=`operator-accounting-${name}`,kind=await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});if(!n)throw Error('Missing bank field '+${JSON.stringify(id)});return n.tagName==='SELECT'?'select':n.type;})()`)
    if(kind==='select')await select(id,String(value))
    else if(kind==='checkbox')await page.evaluate(`(()=>{const n=document.querySelector(${JSON.stringify(t(id))});n.checked=${!!value};n.dispatchEvent(new Event('change',{bubbles:true}));})()`)
    else await page.fill(t(id),String(value))
  }
  const begin=async(kind,fields,rows=0)=>{
    await select('operator-action',kind)
    await until(()=>page.evaluate("document.querySelector('.operator-actions form fieldset')?.disabled===false"),'current bank action grant')
    for(let i=0;i<rows;i++)await page.click(t('operator-accounting-statement-rows')+' > button')
    for(const[k,v]of Object.entries(fields))await fill(k,v)
  }
  const review=async()=>{await page.click(t('module-accounting-create'));await until(()=>page.evaluate(`!!document.querySelector('${t('operator-review')}')`),'native bank review')}
  const confirm=async()=>{
    await page.click(t('operator-confirm'))
    try{await until(async()=>{
      const message=await page.text(t('ops-feedback'))
      if(await page.evaluate(`!!document.querySelector('${t('ops-refused')}')||!!document.querySelector('${t('ops-uncertain')}')`))throw Error(message)
      return message.includes('Saved.')
    },'native bank committed receipt')}catch(error){
      const wire=await page.evaluate('({command:window.__bankCommands.at(-1),reply:window.__bankReplies.at(-1)})')
      const request=wire.command?.requestId;assert.match(request,/^[0-9a-f-]{36}$/)
      const stored=JSON.parse(sql(`SELECT jsonb_build_object('command',(SELECT to_jsonb(c) FROM archava_accounting.commands c WHERE tenant_id='${tenant}' AND request_id='${request}'),'audit',(SELECT to_jsonb(a) FROM archava_accounting.audit a WHERE tenant_id='${tenant}' AND request_id='${request}'))`))
      writeFileSync(join(output,'bank-failure-evidence.json'),JSON.stringify({wire,stored,message:error.message},null,2)+'\n')
      throw error
    }
  }
  const action=async(kind,fields,rows=0)=>{await begin(kind,fields,rows);await review();await confirm()}
  const observe=()=>page.evaluate(`(()=>{const original=window.fetch.bind(window);window.__bankCommands=[];window.__bankReplies=[];window.fetch=async(input,init)=>{const request=input instanceof Request?new Request(input.clone(),init):new Request(input,init);const watched=request.method==='POST'&&new URL(request.url).pathname==='/erp/accounting/commands';if(watched)window.__bankCommands.push(await request.clone().json());const reply=await original(input,init);if(watched)window.__bankReplies.push({status:reply.status,body:await reply.clone().json()});return reply;};})()`)
  const o=await org('create_organization',{name:`Bank acceptance ${suffix}`,legalName:`Owned Manual Bank ${suffix}`})
  const company=(await org('company.create_from_organization',{organizationId:o.organizationId,expectedSourceRevision:1,code:`BANK-${suffix}`.toUpperCase(),reason:'Owned synthetic bank controls acceptance company'})).companyId
  const book=(await command({type:'book.create',companyId:company,expectedCompanyRevision:1,code:`BANK-${suffix}`.toUpperCase(),name:'Owned exact bank controls',currency:'USD',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator journal posting; independent bank review remains required'})).bookId
  const cash=(await command({type:'account.create',bookId:book,code:'CASH',name:'Reviewed bank cash',class:'asset',cashRole:'cash',reason:'Actual cash asset; not payment clearing'})).resourceId
  const equity=(await command({type:'account.create',bookId:book,code:'EQUITY',name:'Reviewed equity',class:'equity',cashRole:'financing',reason:'Explicit synthetic counteraccount'})).resourceId
  const period=(await command({type:'period.open',bookId:book,code:'2026-11',startDate:'2026-11-01',endDate:'2026-11-30',reason:'Complete thirty-day financial period'})).resourceId
  const open=async()=>{
    await page.navigate(`${origin}/app/erp?module=accounting`);await select('trade-company-select',company)
    await until(()=>page.evaluate(`!!document.querySelector('${t('operator-action')}')`),'bank company workspace');await observe()
  }
  await open()
  await action('bank_account.create',{bookId:book,code:'MANUAL',name:'Owned November bank', 'account-accountId':cash,reason:'Explicit reviewed native manual bank registration'})
  const bank=sql(`SELECT id FROM archava_accounting.bank_accounts WHERE book_id='${book}'`);assert.match(bank,/^[0-9a-f-]{36}$/)
  await action('bank_policy.set',{bankAccountId:bank,fromDate:'2026-11-01',independentReview:true,reason:'Explicit full-month basis; importer cannot approve their own bank evidence'})
  const policy=sql(`SELECT id FROM archava_accounting.bank_policies WHERE bank_account_id='${bank}'`)
  assert.equal(sql(`SELECT independent_review FROM archava_accounting.bank_policies WHERE id='${policy}'`),'t')
  pass('BANK-native-registration-and-policy','Native setup binds a reviewed cash asset, explicit financial-period opening basis and independent bank reviewer; no historical cash or permission is invented')

  const statementFields={bankAccountId:bank,'account-accountId':cash,'statement-reference':`NOV-${suffix}`,'statement-fromDate':'2026-11-01','statement-toDate':'2026-11-30','statement-currency':'USD','statement-openingMinor':'0.00','statement-closingMinor':'100.00','statement-rows-0-externalRef':`IN-${suffix}`,'statement-rows-0-date':'2026-11-07','statement-rows-0-amountMinor':'150.00','statement-rows-0-description':'Owned positive bank movement','statement-rows-1-externalRef':`OUT-${suffix}`,'statement-rows-1-date':'2026-11-08','statement-rows-1-amountMinor':'-50.00',reason:'Review complete original statement and both signed movements'}
  await begin('bank_statement.import',statementFields,2);await review()
  const importText=await page.text(t('bank-import-review'))
  for(const amount of ['USD 150.00','USD 50.00','USD 100.00','USD -50.00'])assert.ok(importText.includes(amount),importText)
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_statements WHERE book_id='${book}'`),'0')
  await page.evaluate(`document.querySelector('${t('operator-review')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('bank-import-native-review.png')
  const viewport=await page.evaluate('({width:innerWidth,height:innerHeight,deviceScaleFactor:devicePixelRatio,mobile:false})')
  try{
    await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true})
    await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
    assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true,'Signed bank review must fit a mobile viewport')
    await page.evaluate(`document.querySelector('${t('operator-review')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('bank-import-mobile-review.png')
  }finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
  await confirm()
  const imported=await page.evaluate('window.__bankCommands.find(c=>c.command.type==="bank_statement.import")')
  assert.equal(imported.command.statement.rows[1].amountMinor,'-5000');assert.equal(imported.command.statement.closingMinor,'10000')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.journals WHERE book_id='${book}'`),'0')
  assert.equal((await post('/erp/accounting/commands',imported)).replayed,true)
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_statements WHERE book_id='${book}'`),'1')
  pass('BANK-native-signed-import-and-replay','Actual native review preserves signed human decimals and optional row descriptions; exact wire/database amounts and one replayed immutable import agree, with no payment or journal created')

  const journals=[]
  for(const[code,incoming,amount]of [['IN',true,'15000'],['OUT',false,'5000']]){
    const j=await command({type:'journal.draft',bookId:book,code,date:incoming?'2026-11-07':'2026-11-08',sourceRef:`Owned bank cash ${code}`,memo:'Explicit synthetic posted cash source',lines:incoming?[{...accountRef(cash),debitMinor:amount,creditMinor:'0',memo:'Cash receipt'},{...accountRef(equity),debitMinor:'0',creditMinor:amount,memo:'Equity'}]:[{...accountRef(equity),debitMinor:amount,creditMinor:'0',memo:'Equity'},{...accountRef(cash),debitMinor:'0',creditMinor:amount,memo:'Cash withdrawal'}],reason:'Review actual cash posting before any bank match'})
    await command({type:'journal.post',id:j.resourceId,expectedRevision:1,reason:'Post reviewed synthetic cash with the explicitly chosen journal policy'});journals.push(j.resourceId)
  }
  const rows=[sql(`SELECT id FROM archava_accounting.bank_rows WHERE bank_account_id='${bank}' AND external_ref='IN-${suffix}'`),sql(`SELECT id FROM archava_accounting.bank_rows WHERE bank_account_id='${bank}' AND external_ref='OUT-${suffix}'`)]
  const readinessPath=`/erp/accounting/bank-reconciliation?tenant=${tenant}&bankPolicyId=${policy}&periodId=${period}`
  const incomplete=await call(readinessPath);assert.equal(incomplete.status,200);assert.equal(incomplete.body.check.ready,false);assert.equal(incomplete.body.check.coveredDays,30)
  const closeHeld=await call('/erp/accounting/commands',{tenant,requestId:randomUUID(),command:{type:'period.close',id:period,expectedRevision:1,reason:'Complete evidence without matches cannot close'}});assert.equal(closeHeld.status,409)
  await open()
  for(let i=0;i<2;i++)await action('bank_allocation.record',{bankRowId:rows[i],journalId:journals[i],journalLineOrdinal:i===0?1:2,'account-accountId':cash,amountMinor:i===0?'150.00':'50.00',reason:'Review the original signed row and actual posted bank cash leg'})
  const allocation=sql(`SELECT id FROM archava_accounting.bank_allocations WHERE book_id='${book}' AND bank_row_id='${rows[0]}'`)
  await begin('bank_allocation.reverse',{id:allocation,reason:'Correct the match while preserving its immutable original evidence'});await review()
  await page.evaluate(`(()=>{const original=window.fetch.bind(window);let pending=true;window.fetch=async(input,init)=>{const request=input instanceof Request?new Request(input.clone(),init):new Request(input,init);const watched=pending&&request.method==='POST'&&new URL(request.url).pathname==='/erp/accounting/commands'&&(await request.clone().json()).command.type==='bank_allocation.reverse';const reply=await original(input,init);if(watched&&reply.ok){pending=false;const body=await reply.clone().json();body.receipt.originalBankAllocationId='00000000-0000-4000-8000-000000000999';return new Response(JSON.stringify(body),{status:200,headers:{'content-type':'application/json'}});}return reply;};})()`)
  await page.click(t('operator-confirm'))
  await until(()=>page.evaluate(`!!document.querySelector('${t('ops-uncertain')}')`),'wrong original allocation acknowledgement held as uncertain')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_unmatches WHERE book_id='${book}'`),'1')
  await page.click(t('ops-recover'))
  await until(()=>page.text(t('ops-feedback')).then(s=>s.includes('Saved.')),'authoritative original bank command recovered without a second write')
  assert.equal(await page.evaluate('window.__bankCommands.filter(c=>c.command.type==="bank_allocation.reverse").length'),1)
  const unmatch=await page.evaluate('window.__bankReplies.find(r=>r.body.receipt?.type==="bank_allocation.reverse").body.receipt')
  assert.equal(unmatch.originalBankAllocationId,allocation)
  assert.notEqual(unmatch.resourceId,allocation)
  pass('BANK-native-unmatch-acknowledgement-recovery','A real committed unmatch receives a mangled original-allocation reference; UI keeps the original request uncertain, then recovers its authoritative receipt without resubmitting or duplicating history')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_unmatches WHERE book_id='${book}'`),'1')
  await action('bank_allocation.record',{bankRowId:rows[0],journalId:journals[0],journalLineOrdinal:1,'account-accountId':cash,amountMinor:'150.00',reason:'Explicitly review a replacement after releasing the original match'})
  const ready=await call(readinessPath);assert.equal(ready.body.check.ready,true);assert.equal(ready.body.canConfirm,false);assert.equal(ready.body.importerCannotReview,true)
  await begin('bank_reconciliation.confirm',{bankPolicyId:policy,periodId:period,reason:'Importer must see their separate bank reviewer requirement'})
  await until(()=>page.text(t('bank-period-controls')).then(s=>s.includes('reviewer who did not import')),'native independent reviewer hold')
  await page.click(t('module-accounting-create'))
  await until(()=>page.text(t('operator-review')).then(s=>s.includes('cannot be confirmed')),'native importer review displays its current restriction')
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_reconciliations WHERE book_id='${book}'`),'0')
  await page.click(t('operator-confirm'))
  await until(()=>page.evaluate(`!!document.querySelector('${t('ops-refused')}')`),'importer confirmation refused by current server authority')
  assert.equal(await page.evaluate('window.__bankReplies.at(-1).status'),403)
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_reconciliations WHERE book_id='${book}'`),'0')
  pass('BANK-native-match-correction-and-reviewer-hold','Native exact cash matches, immutable unmatching and reviewed replacement resolve both signed rows; importer sees the review restriction, attempts save, receives403 and creates no attestation')

  // Explicit synthetic grant, local to this disposable fixture. Existing production grants stay unchanged.
  const previousBob=JSON.parse(sql("SELECT to_json(permissions) FROM archava_accounting.memberships WHERE tenant_id='acme-retail' AND principal_id='bob'"))
  sql("UPDATE archava_accounting.memberships SET permissions=array_append(permissions,'bank.reconcile') WHERE tenant_id='acme-retail' AND principal_id='bob' AND NOT('bank.reconcile'=ANY(permissions))")
  const login=async user=>{
    await page.send('Network.clearBrowserCookies');await page.evaluate('localStorage.clear()');await page.navigate(`${origin}/app`)
    await page.fill(t('login-username'),`${user}@archava.test`);await page.fill(t('login-password'),'correct horse battery staple');await page.fill(t('login-tenant'),tenant);await page.click(t('login-submit'))
    await until(()=>page.evaluate(`location.pathname==='/app/contacts'&&!!document.querySelector('${t('contacts-count')}')`),'bank reviewer login')
  }
  const aliceLogin=await fetch(`${origin}/login`,{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli'},body:JSON.stringify({username:'alice@archava.test',password:'correct horse battery staple'})});assert.equal(aliceLogin.status,200)
  const alice=await aliceLogin.json()
  const asAlice=async command=>{
    const r=await fetch(`${origin}/erp/accounting/commands`,{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli',authorization:`Bearer ${alice.token}`,'x-archava-csrf':alice.csrf},body:JSON.stringify({tenant,requestId:randomUUID(),command})});const body=await r.json();assert.equal(r.status,200,JSON.stringify(body));return body.receipt
  }
  try{
    await login('bob');await open()
    const reviewControls=async()=>{
      await begin('bank_reconciliation.confirm',{bankPolicyId:policy,periodId:period,reason:'Independent human reviews the exact complete current bank evidence'})
      await until(()=>page.text(t('bank-controls-loader')).then(s=>s.includes('Complete bank controls are loaded')),'complete native bank control read')
      const summary=await page.text(t('bank-period-controls'));assert.ok(summary.includes('30 of 30 days'));assert.ok(summary.includes('USD 100.00'));assert.ok(!summary.includes('Needs attention'),summary)
      await review()
    }
    await reviewControls()
    const originalHash=(await call(readinessPath)).body.check.evidenceSha256
    await page.evaluate(`document.querySelector('${t('operator-review')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('bank-independent-period-review.png')
    try{
      await page.send('Emulation.setDeviceMetricsOverride',{width:390,height:844,deviceScaleFactor:1,mobile:true});await page.evaluate('new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)))')
      assert.equal(await page.evaluate('document.documentElement.scrollWidth<=innerWidth+1'),true)
      await page.evaluate(`document.querySelector('${t('operator-review')}').scrollIntoView({block:'start',behavior:'instant'})`);await page.screenshot('bank-independent-period-mobile-review.png')
    }finally{await page.send('Emulation.setDeviceMetricsOverride',viewport)}
    const replacement=sql(`SELECT id FROM archava_accounting.bank_allocations a WHERE a.book_id='${book}' AND a.bank_row_id='${rows[0]}' AND NOT EXISTS(SELECT 1 FROM archava_accounting.bank_unmatches u WHERE u.tenant_id=a.tenant_id AND u.allocation_id=a.id)`)
    await asAlice({type:'bank_allocation.reverse',id:replacement,expectedRevision:1,reason:'Concurrent explicit correction after Bob reviewed the prior evidence'})
    await asAlice({type:'bank_allocation.record',bankRowId:rows[0],journalId:journals[0],journalLineOrdinal:1,expectedJournalRevision:2,account:accountRef(cash),amountMinor:'15000',reason:'Same net cash, newly reviewed replacement evidence'})
    const changed=(await call(readinessPath)).body;assert.equal(changed.check.ready,true);assert.notEqual(changed.check.evidenceSha256,originalHash)
    await page.click(t('operator-confirm'));await until(()=>page.evaluate(`!!document.querySelector('${t('ops-refused')}')`),'stale complete-control hash refused on save')
    assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_reconciliations WHERE book_id='${book}'`),'0')
    pass('BANK-native-stale-review-refused','After the human reviews, a real concurrent unmatch/rematch changes the evidence without changing net money or policy/period revisions; saving the stale native review is refused atomically')
    await open();await reviewControls();await confirm()
    const confirmed=await page.evaluate('window.__bankCommands.find(c=>c.command.type==="bank_reconciliation.confirm")');assert.equal(confirmed.command.expectedCheckHash,changed.check.evidenceSha256)
    assert.equal((await post('/erp/accounting/commands',confirmed)).replayed,true)
    assert.equal(sql(`SELECT created_by FROM archava_accounting.bank_reconciliations WHERE book_id='${book}'`),'bob')
    assert.equal(sql(`SELECT count(*) FROM archava_accounting.bank_reconciliations WHERE book_id='${book}'`),'1')
    pass('BANK-native-independent-review-and-replay','Bob confirms the current human-readable controls with exact source hash and observed versions; SQL binds the independent actor, and command replay creates no second attestation')
  }finally{
    assert.ok(previousBob.every(p=>/^[a-z][a-z_.-]+$/.test(p)));sql(`UPDATE archava_accounting.memberships SET permissions=ARRAY[${previousBob.map(p=>`'${p}'`).join(',')}]::text[] WHERE tenant_id='acme-retail' AND principal_id='bob'`)
    await login('alice')
  }
  await open();await action('period.close',{id:period,reason:'Complete statements and exact cash matches have a current independent bank attestation'})
  const closed=await call(readinessPath);assert.equal(closed.body.periodState,'closed');assert.equal(closed.body.reviewed,true);assert.equal(closed.body.check.ready,true)
  const proof=JSON.parse(sql(`SELECT jsonb_build_object('statements',(SELECT count(*) FROM archava_accounting.bank_statements WHERE book_id='${book}'),'rows',(SELECT count(*) FROM archava_accounting.bank_rows WHERE book_id='${book}'),'allocations',(SELECT count(*) FROM archava_accounting.bank_allocations WHERE book_id='${book}'),'unmatches',(SELECT count(*) FROM archava_accounting.bank_unmatches WHERE book_id='${book}'),'reviews',(SELECT count(*) FROM archava_accounting.bank_reconciliations WHERE book_id='${book}'),'cashMinor',(SELECT sum(l.debit_minor-l.credit_minor)::text FROM archava_accounting.journal_lines l JOIN archava_accounting.journals j ON j.tenant_id=l.tenant_id AND j.id=l.journal_id WHERE l.book_id='${book}' AND l.account_id='${cash}' AND j.state='posted'),'periodState',(SELECT state FROM archava_accounting.periods WHERE id='${period}'))`))
  assert.deepEqual(proof,{statements:1,rows:2,allocations:4,unmatches:2,reviews:1,cashMinor:'10000',periodState:'closed'})
  pass('BANK-native-close-independent-SQL',proof)
  await command({type:'period.open',bookId:book,code:'2026-12',startDate:'2026-12-01',endDate:'2026-12-31',reason:'Owned future statement import, separate from the reviewed closed November period'})
  await open()
  await begin('bank_statement.import',{bankAccountId:bank,'account-accountId':cash,'statement-reference':`DEC-BULK-${suffix}`,'statement-fromDate':'2026-12-01','statement-toDate':'2026-12-31','statement-currency':'USD','statement-openingMinor':'100.00','statement-closingMinor':'101.01',reason:'Review every one of 101 original transactions without silently dropping the final row'},101)
  for(let i=0;i<101;i++){
    await fill(`statement-rows-${i}-externalRef`,`DEC-${suffix}-${i}`)
    await fill(`statement-rows-${i}-date`,'2026-12-02')
    await fill(`statement-rows-${i}-amountMinor`,'0.01')
  }
  await review()
  assert.equal(await page.evaluate(`document.querySelectorAll('${t('bank-import-review')} tbody tr').length`),101)
  assert.ok((await page.text(t('bank-import-review'))).includes('USD 1.01'))
  await confirm()
  const largeImport=JSON.parse(sql(`SELECT jsonb_build_object('declaredRows',s.row_count,'actualRows',(SELECT count(*) FROM archava_accounting.bank_rows r WHERE r.tenant_id=s.tenant_id AND r.statement_id=s.id),'movementMinor',(SELECT sum(amount_minor)::text FROM archava_accounting.bank_rows r WHERE r.tenant_id=s.tenant_id AND r.statement_id=s.id),'closingMinor',s.closing_minor::text) FROM archava_accounting.bank_statements s WHERE s.bank_account_id='${bank}' AND reference='DEC-BULK-${suffix}'`))
  assert.deepEqual(largeImport,{declaredRows:101,actualRows:101,movementMinor:'101',closingMinor:'10101'})
  assert.equal(sql(`SELECT count(*) FROM archava_accounting.journals WHERE book_id='${book}'`),'2','Large statement entry records evidence without inventing cash journals')
  const priorPeriod=await call(readinessPath);assert.equal(priorPeriod.body.reviewed,true);assert.equal(priorPeriod.body.check.evidenceSha256,closed.body.check.evidenceSha256)
  pass('BANK-native-101-row-import',largeImport)
  await begin('bank_statement.import',{})
  await page.evaluate(`(()=>{const add=document.querySelector('${t('operator-accounting-statement-rows')} > button');for(let i=0;i<1001;i++)add.click();})()`)
  await until(()=>page.evaluate(`document.querySelectorAll('${t('operator-accounting-statement-rows')} > .operator-array-item').length===1000&&document.querySelector('${t('operator-accounting-statement-rows')} > button').disabled`),'visible configured thousand-row form limit')
  await page.evaluate(`document.querySelector('${t('operator-accounting-statement-rows')} > button').click()`)
  assert.equal(await page.evaluate(`document.querySelectorAll('${t('operator-accounting-statement-rows')} > .operator-array-item').length`),1000)
  await select('operator-action','bank_reconciliation.confirm')
  pass('BANK-native-array-limit','Actual Add controls allow 1000 draft rows, visibly disable at the declared bound and refuse a 1001st item; this boundary check does not submit 1000 empty rows')
  return{scope:'Owned manual bank import, cash matching, independent review and bank-period close; 101-row future statement import is evidence-only and remains unmatched. No external transfer, automatic bank feed or complete subledger close acceptance',company,book,bank,policy,period,evidenceSha256:closed.body.check.evidenceSha256,proof,largeImport}
}
