/* global fetch, performance, AbortSignal, process */
import assert from 'node:assert/strict'
import {randomUUID} from 'node:crypto'
import {cpus,totalmem,freemem,platform,arch} from 'node:os'
import {execFileSync} from 'node:child_process'
import {mkdir,writeFile,readFile} from 'node:fs/promises'
import {join} from 'node:path'

/** Invoked only by the disposable runtime fixture, never by the user's running host. */
export async function runCapacityBaseline({root,home,origin,session,pool,hostPid}) {
  assert.ok(home.startsWith('/tmp/archava-crm-runtime-'))
  assert.equal(new URL(origin).hostname,'127.0.0.1')
  const database=await pool.query("SELECT current_setting('unix_socket_directories') AS socket,current_database() AS database,version() AS version")
  assert.ok(database.rows[0].socket.startsWith(home+'/socket'))
  const tenants=['hotel-demo','studio-demo'],ids={}
  const post=async(tenant,command,requestId=randomUUID())=>{
    const response=await fetch(origin+'/api/crm/'+tenant+'/commands',{method:'POST',headers:{origin,cookie:session.cookie,'x-archava-csrf':session.csrf,'content-type':'application/json'},body:JSON.stringify({requestId,command}),signal:AbortSignal.timeout(10000)})
    assert.equal(response.status,200,'synthetic fixture mutation failed')
    return response.json()
  }
  for(const tenant of tenants) {
    ids[tenant]=[]
    for(let offset=0;offset<200;offset+=5) {
      const chunk=await Promise.all(Array.from({length:5},(_,i)=>post(tenant,{operation:'contact.create',data:{fullName:`Synthetic baseline ${tenant} ${offset+i}`,email:null}})))
      ids[tenant].push(...chunk.map(receipt=>receipt.id))
    }
    for(let i=0;i<20;i++) await post(tenant,{operation:'followup.create',contactId:ids[tenant][i],contactRevision:1,summary:'Synthetic baseline task '+i,dueAt:new Date(Date.now()-i*3600000).toISOString()})
    for(let i=0;i<10;i++) await post(tenant,{operation:'opportunity.create',contactId:ids[tenant][i],contactRevision:1,title:'Synthetic baseline opportunity '+i})
  }
  const fixture={}
  for(const tenant of tenants) {
    const counts=await pool.query('SELECT (SELECT count(*)::int FROM archava_crm.contacts WHERE tenant_id=$1) AS contacts,(SELECT count(*)::int FROM archava_crm.followups WHERE tenant_id=$1) AS followups,(SELECT count(*)::int FROM archava_crm.opportunities WHERE tenant_id=$1) AS opportunities',[tenant])
    fixture[tenant]=counts.rows[0];assert.ok(fixture[tenant].contacts>=200)
  }
  const workloads=tenants.flatMap(tenant=>['contacts?limit=20','followups?limit=20','opportunities?limit=20','overview'].map(route=>({tenant,path:'/api/crm/'+tenant+'/'+route})))
  let sequence=0
  const read=async(workload)=>{
    const response=await fetch(origin+workload.path,{headers:{cookie:session.cookie},signal:AbortSignal.timeout(5000)})
    if(response.status!==200) return {failure:'http-'+response.status}
    const body=await response.json()
    if(workload.path.endsWith('/overview')) assert.equal(body.tenantId,workload.tenant)
    if(workload.path.includes('/contacts?')||workload.path.includes('/opportunities?')) for(const item of body.items) assert.equal(item.tenantId,workload.tenant)
    if(workload.path.includes('/followups?')) for(const item of body.items) assert.ok(ids[workload.tenant].includes(item.contactId)||!item.summary.startsWith('Synthetic baseline'),'cross-tenant synthetic task')
    return {failure:null}
  }
  for(let i=0;i<32;i++) assert.equal((await read(workloads[i%workloads.length])).failure,null)
  const hostResources=()=>{
    const [rssKiB,cpuPercent]=execFileSync('ps',['-p',String(hostPid),'-o','rss=,pcpu='],{encoding:'utf8'}).trim().split(/\s+/).map(Number)
    return {rssKiB,cpuPercent}
  }
  const resources={platform:platform(),arch:arch(),cpu:cpus()[0]?.model,logicalCpus:cpus().length,totalMemoryBytes:totalmem(),freeMemoryBefore:freemem(),hostBefore:hostResources()}
  const measurements=[]
  for(const concurrency of [1,5,20]) {
    const latencies=[],failures={},started=performance.now(),deadline=started+15000
    const worker=async()=>{
      while(performance.now()<deadline) {
        const workload=workloads[sequence++%workloads.length],begin=performance.now()
        let failure
        try {failure=(await read(workload)).failure} catch(error) {
          if(error instanceof assert.AssertionError) throw error
          failure=error.name==='TimeoutError'||error.name==='AbortError'?'timeout':'transport-error'
        }
        if(failure) failures[failure]=(failures[failure]??0)+1
        else latencies.push(performance.now()-begin)
      }
    }
    await Promise.all(Array.from({length:concurrency},worker))
    const elapsedSeconds=(performance.now()-started)/1000
    latencies.sort((a,b)=>a-b)
    const percentile=p=>latencies[Math.min(latencies.length-1,Math.ceil(latencies.length*p)-1)]??null
    const failed=Object.values(failures).reduce((a,b)=>a+b,0)
    assert.ok(latencies.length>0,'no completed samples')
    measurements.push({concurrency,durationRequestedSeconds:15,elapsedSeconds,successfulSamples:latencies.length,failedSamples:failed,totalSamples:latencies.length+failed,successfulRequestsPerSecond:latencies.length/elapsedSeconds,p50Ms:percentile(.5),p95Ms:percentile(.95),failures})
    process.stdout.write(`BASELINE concurrency=${concurrency} samples=${latencies.length} failures=${failed}\n`)
  }
  const requestId=randomUUID(),command={operation:'contact.create',data:{fullName:'Synthetic contention contact',email:null}}
  const receipts=await Promise.all(Array.from({length:20},()=>post(tenants[0],command,requestId)))
  assert.ok(receipts.every(receipt=>JSON.stringify(receipt)===JSON.stringify(receipts[0])))
  const persisted=await pool.query('SELECT (SELECT count(*)::int FROM archava_crm.commands WHERE tenant_id=$1 AND request_id=$2) AS commands,(SELECT count(*)::int FROM archava_crm.audit WHERE tenant_id=$1 AND request_id=$2) AS audits,(SELECT count(*)::int FROM archava_crm.contacts WHERE tenant_id=$1 AND id=$3) AS effects',[tenants[0],requestId,receipts[0].id])
  assert.deepEqual(persisted.rows[0],{commands:1,audits:1,effects:1})
  const recovery=await fetch(origin+'/api/crm/'+tenants[0]+'/commands/'+requestId,{headers:{cookie:session.cookie}}).then(r=>r.json())
  assert.deepEqual(recovery,{status:'committed',requestId,receipt:receipts[0]})
  const cross=await fetch(origin+'/api/crm/'+tenants[1]+'/commands/'+requestId,{headers:{cookie:session.cookie}}).then(r=>r.json())
  assert.notEqual(cross.status,'committed')
  const after=await pool.query('SELECT count(*)::int AS audits FROM archava_crm.audit WHERE tenant_id=$1 AND request_id=$2',[tenants[0],requestId]);assert.equal(after.rows[0].audits,1)
  const profile=JSON.parse(await readFile(join(root,'dist/library/release-profile.json'),'utf8'))
  const report={schemaVersion:'1.0.0',profile:profile.catalog.version,node:process.version,postgres:database.rows[0].version,checkedAt:new Date().toISOString(),syntheticOnly:true,providerCalls:0,loginOutsideTimedWork:true,warmupReads:32,workloads:workloads.map(w=>w.path),fixture,resources:{...resources,freeMemoryAfter:freemem(),hostAfter:hostResources()},measurements,contention:{concurrency:20,identicalReceipts:true,committedEffects:1,audits:1,commands:1,readOnlyRecovery:true,crossTenantReceiptRefused:true},limits:'One local machine, one bounded synthetic fixture, app pool max=8; not a production capacity/SLA claim. p50/p95 describe successful reads; failures and timeouts are recorded separately.'}
  const output=join(root,'dist/capacity-proof');await mkdir(output,{recursive:true});await writeFile(join(output,'results.json'),JSON.stringify(report,null,2)+'\n')
  process.stdout.write('PASS bounded isolated capacity baseline and mutation/recovery contention.\n')
}
