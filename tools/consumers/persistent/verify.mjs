import assert from 'node:assert/strict'
import {spawn,spawnSync} from 'node:child_process'
import {createHash,randomUUID} from 'node:crypto'
import {mkdtempSync,readFileSync,writeFileSync,rmSync,createWriteStream} from 'node:fs'
import {tmpdir} from 'node:os'
import {join} from 'node:path'
import {createServer} from 'node:net'
export async function verifyPersistentConsumers({root,out,pkg}){
const scratch=mkdtempSync(join(tmpdir(),'archava-persistent-consumers-')),data=join(scratch,'pg'),password='correct horse battery staple',actor='consumer-owner'
const env=Object.fromEntries(['PATH','HOME','TMPDIR','LANG'].filter(k=>process.env[k]).map(k=>[k,process.env[k]]))
const result={scope:'Two independently built public-contract clients connected to separate owned native databases; scoped persistent CRM, reviewed Owner labels and distinct extension commands. Not full K04/K05 industry-kit or real-customer acceptance.',verified:false,checks:[],consumers:[]}
const active=new Set(),logs=[];let started=false,pgPort
const hash=p=>createHash('sha256').update(readFileSync(p)).digest('hex')
const pause=ms=>new Promise(r=>setTimeout(r,ms))
async function port(){const s=createServer();await new Promise(r=>s.listen(0,'127.0.0.1',r));const p=s.address().port;await new Promise(r=>s.close(r));return p}
function run(bin,args,options={}){const r=spawnSync(bin,args,{cwd:root,env,encoding:'utf8',timeout:55000,maxBuffer:4*1024*1024,...options});assert.equal(r.status,0,`${bin}: ${r.stderr}\n${r.stdout}`);return r.stdout.trim()}
function sql(db,input){return run('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(pgPort),'-U','postgres','-d',db],{input:`SET statement_timeout='15s';${input}`})}
async function stop(c){if(!c||c.exitCode!==null||c.signalCode!==null)return;const ended=new Promise(r=>c.once('exit',r));c.kill('SIGTERM');await Promise.race([ended,pause(3000)]);if(c.exitCode===null&&c.signalCode===null){c.kill('SIGKILL');await ended}active.delete(c)}
const met=(id,details)=>{result.checks.push({id,details});console.log('PASS '+id)}
const ops=(db,args)=>JSON.parse(run(join(pkg,'bin/archava-ops'),args,{cwd:pkg,env:{...env,ARCHAVA_OPS_ROOT:pkg,ARCHAVA_MIGRATION_DATABASE_URL:`postgres://archava_owner@127.0.0.1:${pgPort}/${db}`,ARCHAVA_OWNER_PASSWORD:password}}))
async function start(client){
 const p=await port();client.origin=`http://127.0.0.1:${p}`
 const log=createWriteStream(join(out,client.model,`native-${client.starts++}.log`));logs.push(log)
 const c=spawn(join(pkg,'bin/archava-server'),[],{cwd:pkg,env:{...env,DATABASE_URL:`postgres://archava_runtime@127.0.0.1:${pgPort}/${client.db}`,ARCHAVA_IDENTITY_KEY_HEX:(client.model==='agency'?'07':'08').repeat(32),ARCHAVA_BIND_ADDR:`127.0.0.1:${p}`,ARCHAVA_SITE_ROOT:join(pkg,'site')},stdio:['ignore','pipe','pipe']});active.add(c);client.server=c;c.stdout.pipe(log,{end:false});c.stderr.pipe(log,{end:false})
 for(let n=0;;n++){if(n>=300||c.exitCode!==null)throw Error('Native consumer readiness timeout');try{if((await fetch(client.origin+'/readyz')).status===200)break}catch{}await pause(100)}
 const r=await fetch(client.origin+'/login',{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli'},body:JSON.stringify({username:`${client.model}@archava.test`,password})});assert.equal(r.status,200);client.session=await r.json()
}
async function call(c,path,body,session=c.session){
 const r=await fetch(c.origin+path,{method:body?'POST':'GET',headers:{'content-type':'application/json','x-archava-client':'cli',authorization:`Bearer ${session.token}`,'x-archava-csrf':session.csrf},...(body?{body:JSON.stringify(body)}:{})});return{status:r.status,body:await r.json()}
}
const ok=async promise=>{const r=await promise;assert.equal(r.status,200,JSON.stringify(r.body));return r.body}
function wire(c,input){const r=spawnSync(join(out,c.model,'client'),['--config',c.configFile],{env,input:JSON.stringify(input),encoding:'utf8',timeout:10000});assert.equal(r.status,0,r.stdout+r.stderr);return JSON.parse(r.stdout)}
function route(w){return w.module==='crm'?'/crm/commands':`/erp/${w.module}/commands`}
async function send(c,input){const w=wire(c,input);const body=await ok(call(c,route(w),w.request));return{wire:w,body,receipt:body.receipt??body}}
const make=(module,operation,values,extra={})=>({requestId:randomUUID(),module,operation,values,...extra})
try{
 const build=JSON.parse(readFileSync(join(out,'build-result.json'),'utf8'));assert.equal(build.verified,true)
 result.buildResultSha256=hash(join(out,'build-result.json'));result.nativeProfileSha256=hash(join(pkg,'release-profile.json'));result.nativeSourceSha256=JSON.parse(readFileSync(join(pkg,'release-profile.json'),'utf8')).sourceSha256;result.probeSha256=hash(new URL(import.meta.url))
 pgPort=await port();run('initdb',['-D',data,'-U','postgres','-A','trust','--no-locale','--encoding=UTF8']);run('pg_ctl',['-D',data,'-l',join(scratch,'postgres.log'),'-o',`-h 127.0.0.1 -p ${pgPort} -k ${scratch}`,'-w','start']);started=true
 sql('postgres','CREATE ROLE archava_owner LOGIN;CREATE ROLE archava_runtime LOGIN;')
 const clients=[]
 for(const model of ['agency','wholesale']){
  const c={model,db:`consumer_${model}`,tenant:`${model}-client`,starts:0,configFile:join(out,model,'config-reviewed.json')};clients.push(c)
  assert.equal(hash(join(out,model,'client')),build.consumers.find(x=>x.name===model).binarySha256)
  sql('postgres',`CREATE DATABASE ${c.db} OWNER archava_owner`);assert.equal(ops(c.db,['migrate','--runtime-role','archava_runtime']).ok,true);assert.equal(ops(c.db,['bootstrap-owner','--tenant',c.tenant,'--principal',actor,'--username',`${model}@archava.test`]).ok,true)
  await start(c)
  const baseline=await ok(call(c,`/own/config?tenant=${c.tenant}`)),labels=JSON.parse(readFileSync(join(out,model,'config-prototype.json'),'utf8')).pipelineLabels
  const request={baseRevision:baseline.config.revision,terminology:{pipelineLabels:labels}},plan=await ok(call(c,'/own/plan',{tenant:c.tenant,request}));assert.equal(plan.plan.blocked,null)
  const apply={tenant:c.tenant,requestId:randomUUID(),request,confirmation:{planHash:plan.plan.planHash,baseRevision:baseline.config.revision,catalogueVersion:plan.catalogueVersion,pricingVersion:plan.pricingVersion}}
  const applied=await ok(call(c,'/own/apply',apply)),reapplied=await ok(call(c,'/own/apply',apply));assert.equal(applied.replayed,false);assert.equal(reapplied.replayed,true);assert.deepEqual(reapplied.receipt,applied.receipt)
  const reviewed=await ok(call(c,`/own/config?tenant=${c.tenant}`));assert.deepEqual(reviewed.config.terminology.pipelineLabels,labels)
  writeFileSync(c.configFile,JSON.stringify({tenant:c.tenant,model,configRevision:reviewed.config.revision,pipelineLabels:reviewed.config.terminology.pipelineLabels})+'\n')
  met(model+'-reviewed-owner-config',{revision:reviewed.config.revision,labels,applyReplay:true})
  c.input=make('crm','contact.create',{data:{fullName:`Owned ${model} Budi & Sons 日本`,email:`buyer-${model}@archava.test`,organization:'Synthetic persistent consumer'}})
  c.created=await send(c,c.input);c.contact=c.created.receipt.id;assert.match(c.contact,/^[0-9a-f-]{36}$/);assert.equal(c.created.wire.view.stageLabel,labels.new)
  const replay=await send(c,c.input);assert.deepEqual(replay.receipt,c.created.receipt)
  const opportunity=await send(c,make('crm','opportunity.create',{contactId:c.contact,contactRevision:1,title:`${model} canonical pipeline`}));c.opportunity=opportunity.receipt.id
  const moved=await send(c,make('crm','opportunity.move',{id:c.opportunity,expectedRevision:1,stage:'qualified'},{observedStage:'new'}));assert.equal(moved.wire.view.stageLabel,labels.qualified)
  const record=await ok(call(c,`/crm/records?tenant=${c.tenant}&contact=${c.contact}`));assert.equal(record.contact.fullName,`Owned ${model} Budi & Sons 日本`);assert.equal(record.opportunities[0].stage,'qualified');assert.deepEqual(record.pipelineLabels,labels);assert.equal(record.configurationRevision,reviewed.config.revision)
  met(model+'-persistent-crm-replay',{canonicalStage:record.opportunities[0].stage,configuredLabel:moved.wire.view.stageLabel,contactReplay:true})
  const forged=wire(c,make('crm','opportunity.move',{id:c.opportunity,expectedRevision:2,stage:'won'},{observedStage:'proposal'}))
  assert.equal((await call(c,'/crm/commands',forged.request)).status,409)
  const stale=wire(c,make('crm','opportunity.move',{id:c.opportunity,expectedRevision:1,stage:'proposal'},{observedStage:'qualified'}))
  assert.equal((await call(c,'/crm/commands',stale.request)).status,409)
  assert.equal(sql(c.db,`SET archava.tenant_id='${c.tenant}';SELECT stage||':'||revision FROM archava_crm.opportunities WHERE id='${c.opportunity}'`),'qualified:2')
  met(model+'-server-revalidates-client-observations',{forgedObservedStage:409,staleRevision:409,actualStagePreserved:true})
  if(model==='agency'){
   const extension=await send(c,make('projects','project.create',{title:'Owned agency delivery',scopeSummary:'Reviewed project-service extension'}));assert.equal(extension.receipt.kind,'project');c.extension=extension.receipt.id
   assert.equal(sql(c.db,`SET archava.tenant_id='${c.tenant}';SELECT count(*) FROM archava_projects.projects WHERE id='${c.extension}' AND title='Owned agency delivery'`),'1')
  }else{
   const product=(await ok(call(c,'/erp/commerce/commands',{tenant:c.tenant,requestId:randomUUID(),command:{operation:'product.create',sku:'COLD-WHOLESALE',title:'Owned wholesale service',unit:'unit',trackStock:false,price:12000}}))).receipt
   const extension=await send(c,make('commerce','order.create_from_contact',{contactId:c.contact,expectedContactRevision:1,lines:[{productId:product.id,quantity:2}]}));c.extension=extension.receipt.id
   assert.equal(sql(c.db,`SET archava.tenant_id='${c.tenant}';SELECT count(*) FROM archava_commerce.orders WHERE id='${c.extension}' AND customer_contact_id='${c.contact}'`),'1')
  }
  met(model+'-distinct-domain-extension',{extension:model==='agency'?'project.create':'order.create_from_contact',persisted:true})
  // Administrative mutation is deliberate revocation, never fixture permission expansion.
  sql(c.db,`SET archava.tenant_id='${c.tenant}';UPDATE archava_crm.memberships SET active=false WHERE tenant_id='${c.tenant}' AND principal_id='${actor}'`)
  assert.equal((await call(c,'/crm/commands',c.created.wire.request)).status,403)
  sql(c.db,`SET archava.tenant_id='${c.tenant}';UPDATE archava_crm.memberships SET active=true WHERE tenant_id='${c.tenant}' AND principal_id='${actor}'`)
  const before=sql(c.db,`SET archava.tenant_id='${c.tenant}';SELECT md5(string_agg(to_jsonb(r)::text,'' ORDER BY to_jsonb(r)::text)) FROM archava_crm.contacts r`)
  ops(c.db,['drain-writer']);await stop(c.server);await start(c)
  assert.deepEqual((await send(c,c.input)).receipt,c.created.receipt)
  const restored=await ok(call(c,`/crm/records?tenant=${c.tenant}&contact=${c.contact}`));assert.equal(restored.opportunities[0].stage,'qualified');assert.deepEqual(restored.pipelineLabels,labels)
  assert.equal(sql(c.db,`SET archava.tenant_id='${c.tenant}';SELECT md5(string_agg(to_jsonb(r)::text,'' ORDER BY to_jsonb(r)::text)) FROM archava_crm.contacts r`),before)
  met(model+'-revocation-and-native-restart',{revokedReplay:403,restoredReplay:true,originalContactDigest:true,canonicalStage:'qualified'})
  result.consumers.push({model,tenant:c.tenant,reviewedConfigSha256:hash(c.configFile),clientSha256:hash(join(out,model,'client')),persistent:true,separateDatabase:true,serverRestarts:1})
 }
 const [a,b]=clients
 assert.notEqual(a.origin,b.origin);assert.notEqual(a.db,b.db);assert.notEqual(a.created.wire.view.stageLabel,b.created.wire.view.stageLabel)
 assert.equal((await call(a,`/crm/records?tenant=${b.tenant}&contact=${b.contact}`)).status,403)
 assert.equal((await call(a,`/crm/records?tenant=${a.tenant}&contact=${b.contact}`)).status,404)
 assert.equal((await call(a,`/contacts?tenant=${a.tenant}`,undefined,b.session)).status,401)
 for(const c of clients){assert.equal(sql(c.db,`SET archava.tenant_id='${c.tenant}';SELECT count(*) FROM archava_crm.contacts`),'1');assert.equal(sql(c.db,'SELECT (SELECT count(*) FROM archava_accounting.books)+(SELECT count(*) FROM archava_accounting.journals)'),'0')}
 met('separate-installation-isolation',{crossTenant:403,otherDatabaseRecord:404,otherIdentityKeySession:401,contactsEach:1,implicitFinancialBooksOrJournals:0})
 result.verified=true
}catch(e){result.error=e.stack??String(e);console.error(result.error);process.exitCode=1}
finally{
 for(const c of [...active])await stop(c)
 if(started){const r=spawnSync('pg_ctl',['-D',data,'-m','immediate','-w','stop'],{env,encoding:'utf8',timeout:15000});result.postgresCleanupExitCode=r.status;if(r.status!==0){result.cleanupError=r.stderr;process.exitCode=1}}
 for(const log of logs)await new Promise(r=>log.end(r))
 rmSync(scratch,{recursive:true,force:true});result.ownedScratchRemoved=true;writeFileSync(join(out,'persistent-result.json'),JSON.stringify(result,null,2)+'\n');console.log(JSON.stringify({verified:result.verified,checks:result.checks.length,cleanup:result.postgresCleanupExitCode,scratchRemoved:result.ownedScratchRemoved,error:result.error}))
}

return result
}
