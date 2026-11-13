/** Real previous bank executable -> repaired executable, with immutable legacy receipts. */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {readFileSync} from 'node:fs'
import {join,resolve} from 'node:path'
import {spawnSync} from 'node:child_process'
import {sha256} from '../rust/provenance.mjs'

export async function verifyLegacyBankReceipts({previousRoot,packageRoot,facts,productionEnvironment,startServer,terminate,until,freePort,pass}) {
  const previous=resolve(previousRoot),current=resolve(packageRoot)
  assert.notEqual(previous,current)
  const oldProfile=JSON.parse(readFileSync(join(previous,'release-profile.json'),'utf8'))
  const profile=JSON.parse(readFileSync(join(current,'release-profile.json'),'utf8'))
  assert.equal(oldProfile.sourceSha256,'ce17f01bb23979bea2091808ea9cabd1ac355d49ae839b6a2fbeb3057740f33a','This regression needs the actual bank candidate with the observed receipt defect')
  assert.notEqual(profile.sourceSha256,oldProfile.sourceSha256)
  for(const[root,p]of [[previous,oldProfile],[current,profile]]){
    for(const name of ['bin/archava-server','bin/archava-ops']){
      const artifact=p.artifacts.find(a=>a.name===name);assert.ok(artifact);assert.equal(sha256(join(root,name)),artifact.sha256)
    }
  }
  const oldManifest=JSON.parse(readFileSync(join(previous,'migrations/manifest.v1.json'),'utf8')),manifest=JSON.parse(readFileSync(join(current,'migrations/manifest.v1.json'),'utf8'))
  for(const ledger of oldManifest.ledgers){
    const next=manifest.ledgers.find(l=>l.namespace===ledger.namespace);assert.ok(next)
    assert.deepEqual(next.migrations.slice(0,ledger.migrations.length),ledger.migrations,'Every already applied migration identity and byte must remain unchanged')
  }
  for(const grant of oldManifest.runtimeGrants)assert.ok(manifest.runtimeGrants.includes(grant),'Previously declared runtime grants must remain available')
  const addedMigrations=manifest.ledgers.reduce((n,l)=>n+l.migrations.length-(oldManifest.ledgers.find(x=>x.namespace===l.namespace)?.migrations.length??0),0)
  const database=`archava_bank_legacy_${randomUUID().slice(0,8)}`,tenant='bank-legacy',principal='native-owner',username='native-owner@archava.test',password='correct horse battery staple'
  assert.match(database,/^archava_bank_legacy_[0-9a-f]{8}$/)
  const sql=input=>{
    const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d',database],{env:{...process.env,PGPASSWORD:'archava'},input,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr);return r.stdout.trim()
  }
  const ops=(root,args)=>{
    const r=spawnSync(join(root,'bin/archava-ops'),args,{cwd:root,env:{...process.env,ARCHAVA_OPS_ROOT:root,ARCHAVA_MIGRATION_DATABASE_URL:`postgres://archava_owner:archava@127.0.0.1:${facts.port}/${database}`,ARCHAVA_OWNER_PASSWORD:password},encoding:'utf8',timeout:55000,maxBuffer:4*1024*1024});assert.equal(r.status,0,`${r.stderr}\n${r.stdout}`);const body=JSON.parse(r.stdout);assert.equal(body.ok,true);return body
  }
  const start=async(root,name)=>{
    const port=await freePort(),origin=`http://127.0.0.1:${port}`
    const environment={...productionEnvironment,DATABASE_URL:`postgres://archava_runtime:archava@127.0.0.1:${facts.port}/${database}`,ARCHAVA_BIND_ADDR:`127.0.0.1:${port}`,ARCHAVA_SITE_ROOT:join(root,'site'),ARCHAVA_WRITER_TAKEOVER:'false',GEMINI_AGENT_API_KEY:'',GEMINI_API_KEY:'',GEMINI_AGENT_MODEL:'',GEMINI_AGENT_ENDPOINT:''}
    for(const key of ['ARCHAVA_PAYMENT_CONFIG','ARCHAVA_EMAIL_CONFIG','ARCHAVA_AGENT_CONFIG'])environment[key]=undefined
    const child=startServer(join(root,'bin/archava-server'),environment,name)
    try{await until(async()=>{try{return(await fetch(`${origin}/readyz`)).status===200}catch{return false}},name+' readiness');return{child,origin}}catch(error){await terminate(child);throw error}
  }
  const login=async server=>{
    const r=await fetch(`${server.origin}/login`,{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli'},body:JSON.stringify({username,password})});assert.equal(r.status,200);return r.json()
  }
  const call=async(server,session,path,body)=>{
    const r=await fetch(`${server.origin}${path}`,{method:body?'POST':'GET',headers:{'content-type':'application/json','x-archava-client':'cli',authorization:`Bearer ${session.token}`,'x-archava-csrf':session.csrf},...(body?{body:JSON.stringify(body)}:{})});return{status:r.status,body:await r.json()}
  }
  const ok=async response=>{const r=await response;assert.equal(r.status,200,JSON.stringify(r));return r.body.receipt??r.body}
  let beforeServer,afterServer,created=false
  try{
    const create=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','postgres'],{env:{...process.env,PGPASSWORD:'archava'},input:`CREATE DATABASE ${database} OWNER archava_owner`,encoding:'utf8',timeout:30000});assert.equal(create.status,0,create.stderr);created=true
    ops(previous,['migrate','--runtime-role','archava_runtime'])
    ops(previous,['bootstrap-owner','--tenant',tenant,'--principal',principal,'--username',username])
    beforeServer=await start(previous,'bank-legacy-before.log');const oldSession=await login(beforeServer)
    const org=(type,data)=>ok(call(beforeServer,oldSession,'/erp/organizations/commands',{tenant,mutation:{requestId:randomUUID(),type,data}}))
    const finance=command=>ok(call(beforeServer,oldSession,'/erp/accounting/commands',{tenant,requestId:randomUUID(),command}))
    const o=await org('create_organization',{name:'Legacy bank receipt',legalName:'Legacy Bank Receipt Synthetic Company'})
    const company=await org('company.create_from_organization',{organizationId:o.organizationId,expectedSourceRevision:1,code:'LEGACY-BANK',reason:'Actual prior native bank command acceptance fixture'})
    const book=await finance({type:'book.create',companyId:company.companyId,expectedCompanyRevision:1,code:'LEGACY-BANK',name:'Legacy bank receipt book',currency:'USD',sourceAuthority:'native',independentPosting:false,reason:'Explicit synthetic single-operator journal posting'})
    const cash=await finance({type:'account.create',bookId:book.bookId,code:'BANK',name:'Actual legacy native cash',class:'asset',cashRole:'cash',reason:'Explicit cash asset'})
    const equity=await finance({type:'account.create',bookId:book.bookId,code:'EQUITY',name:'Synthetic source equity',class:'equity',cashRole:'financing',reason:'Explicit synthetic counterpart'})
    await finance({type:'period.open',bookId:book.bookId,code:'2026-11',startDate:'2026-11-01',endDate:'2026-11-30',reason:'Actual prior bank evidence period'})
    const account={accountId:cash.resourceId,expectedAccountRevision:1}
    const bank=await finance({type:'bank_account.create',bookId:book.bookId,code:'LEGACY',name:'Legacy manual bank',account,reason:'Actual prior bank account without administrator grants'})
    await finance({type:'bank_statement.import',bankAccountId:bank.resourceId,account,statement:{reference:'LEGACY-NOV',fromDate:'2026-11-01',toDate:'2026-11-30',currency:'USD',openingMinor:'0',closingMinor:'10000',rows:[{externalRef:'LEGACY-IN',date:'2026-11-07',amountMinor:'10000',description:'Actual prior native statement'}]},reason:'Actual prior import; no transfer or auto posting'})
    const journal=await finance({type:'journal.draft',bookId:book.bookId,code:'LEGACY-IN',date:'2026-11-07',sourceRef:'Owned prior cash',memo:'Actual prior native posting',lines:[{...account,debitMinor:'10000',creditMinor:'0',memo:'Cash'},{accountId:equity.resourceId,expectedAccountRevision:1,debitMinor:'0',creditMinor:'10000',memo:'Equity'}],reason:'Actual reviewed prior cash journal'})
    await finance({type:'journal.post',id:journal.resourceId,expectedRevision:1,reason:'Separate prior native posting under explicit synthetic policy'})
    const workspace=await ok(call(beforeServer,oldSession,`/erp/accounting/workspace?tenant=${tenant}&companyId=${company.companyId}&collection=bank_rows`))
    const row=workspace.bank_rows.find(r=>r.externalRef==='LEGACY-IN');assert.ok(row)
    const allocation=await finance({type:'bank_allocation.record',bankRowId:row.id,journalId:journal.resourceId,journalLineOrdinal:1,expectedJournalRevision:2,account,amountMinor:'10000',reason:'Actual prior native match'})
    const input={tenant,requestId:randomUUID(),command:{type:'bank_allocation.reverse',id:allocation.resourceId,expectedRevision:1,reason:'Actual prior committed unmatch with the original receipt format'}}
    const original=await ok(call(beforeServer,oldSession,'/erp/accounting/commands',input))
    assert.equal(original.resourceKind,'bank_unmatch');assert.equal(original.originalBankAllocationId,undefined)
    const proof=()=>JSON.parse(sql(`SELECT jsonb_build_object('command',(SELECT to_jsonb(c) FROM archava_accounting.commands c WHERE tenant_id='${tenant}' AND request_id='${input.requestId}'),'audit',(SELECT to_jsonb(a) FROM archava_accounting.audit a WHERE tenant_id='${tenant}' AND request_id='${input.requestId}'),'unmatches',(SELECT count(*) FROM archava_accounting.bank_unmatches),'allocations',(SELECT count(*) FROM archava_accounting.bank_allocations))`))
    const before=proof();assert.equal(before.unmatches,1);assert.equal(before.audit.snapshot.record.allocation_id,allocation.resourceId)
    ops(previous,['drain-writer']);await terminate(beforeServer.child);beforeServer=null
    assert.equal(ops(current,['migrate','--runtime-role','archava_runtime']).installation.migration.applied,addedMigrations)
    afterServer=await start(current,'bank-legacy-after.log');const session=await login(afterServer)
    const status=await call(afterServer,session,`/erp/accounting/commands/status?tenant=${tenant}&requestId=${input.requestId}`);assert.equal(status.status,200);assert.equal(status.body.decision,'committed')
    for(const receipt of [status.body.receipt,await ok(call(afterServer,session,'/erp/accounting/commands',input))]){
      assert.equal(receipt.originalBankAllocationId,allocation.resourceId);assert.equal(receipt.resourceId,original.resourceId);assert.equal(receipt.replayed,true)
    }
    assert.deepEqual(proof(),before,'Historical receipt/audit bytes and exact row counts must remain unchanged during metadata recovery and command replay')
    const result={previousSourceSha256:oldProfile.sourceSha256,currentSourceSha256:profile.sourceSha256,sameSqlProfile:addedMigrations===0,migrationsApplied:addedMigrations,priorMigrationBytesPreserved:true,oldReceiptHadOriginalReference:false,referenceDerivedFromImmutableEvidence:true,statusRecovered:true,sameKeyReplayed:true,historicalReceiptAndAuditUnchanged:true,unmatchRows:1,extraAdministratorGrants:false,scope:'Actual owned prior/current native executables with a synthetic historical bank command; no application database or external transfer'}
    pass('BANK-actual-legacy-receipt-upgrade',result);return result
  }finally{
    for(const server of [beforeServer,afterServer])if(server)await terminate(server.child)
    if(created){const r=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','postgres'],{env:{...process.env,PGPASSWORD:'archava'},input:`DROP DATABASE ${database} WITH (FORCE)`,encoding:'utf8',timeout:30000});assert.equal(r.status,0,r.stderr)}
  }
}
