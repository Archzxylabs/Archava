/** Bounded authenticated load on the browser gate's owned database only. */
import assert from 'node:assert/strict'
import { spawnSync } from 'node:child_process'
import { randomUUID } from 'node:crypto'
import { cpus, totalmem, platform, arch } from 'node:os'
import { runBenchmark } from '../rust/load-benchmark.mjs'

export async function verifyLoad({origin, facts, pass}) {
  const repetitions=Number(process.env.ARCHAVA_E2E_LOAD_REPETITIONS||1)
  assert.ok(Number.isSafeInteger(repetitions)&&repetitions>=1&&repetitions<=20,'Load repetitions must be an integer from 1 to 20')
  assert.ok(Number.isInteger(facts.port) && facts.port>1023 && facts.port<65536)
  assert.equal(new URL(origin).hostname,'127.0.0.1')
  const prefix=`LOAD-${randomUUID().slice(0,8).toUpperCase()}`
  const sql=(statement)=>{
    const result=spawnSync('psql',['-X','-qAt','-v','ON_ERROR_STOP=1','-h','127.0.0.1','-p',String(facts.port),'-U','postgres','-d','archava_test'],{
      env:{...process.env,PGPASSWORD:'archava'},input:`SET statement_timeout='30s';SET archava.tenant_id='acme-retail';${statement}`,encoding:'utf8',timeout:35000,
    })
    assert.equal(result.status,0,result.stderr)
    return result.stdout.trim()
  }
  // Fixed synthetic scale, no connection to DATABASE_URL or a user database.
  sql(`INSERT INTO archava_crm.contacts(tenant_id,id,full_name,email,organization,owner_id)
    SELECT 'acme-retail',gen_random_uuid(),'${prefix}-seed-'||n,'load-'||n||'@archava.test','Synthetic load dataset','alice' FROM generate_series(1,10000) n;
    INSERT INTO archava_commerce.products(tenant_id,id,sku,title,unit,track_stock,price,currency)
    SELECT 'acme-retail',gen_random_uuid(),'${prefix}-SKU-'||n,'Synthetic load product '||n,'unit',false,100,'IDR' FROM generate_series(1,2000) n;
    ANALYZE archava_crm.contacts;ANALYZE archava_commerce.products;`)
  const login=await fetch(`${origin}/login`,{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli'},body:JSON.stringify({username:'alice@archava.test',password:'correct horse battery staple'})})
  assert.equal(login.status,200)
  const session=await login.json()
  const headers={authorization:`Bearer ${session.token}`,'x-archava-csrf':session.csrf,'content-type':'application/json'}
  const phases=[]
  for(const [name,path] of [['contacts','/contacts?tenant=acme-retail&limit=50'],['commerce','/erp/commerce/workspace?tenant=acme-retail']]) {
    for(let repetition=1;repetition<=repetitions;repetition++) {
      await runBenchmark({target:origin,path,headers,requests:20,concurrency:2})
      const result=await runBenchmark({target:origin,path,headers,requests:500,concurrency:8})
      phases.push({name,repetition,...result,targetP95Ms:50})
      assert.equal(result.errors,0);assert.equal(result.unexpectedStatuses,0);assert.equal(result.completed,500)
    }
  }
  const ids=[]
  const mutations=await runBenchmark({target:origin,path:'/crm/commands',method:'POST',headers,requests:200,concurrency:8,body:()=>{
    const requestId=randomUUID();ids.push(requestId)
    return JSON.stringify({tenant:'acme-retail',requestId,command:{operation:'contact.create',data:{fullName:`${prefix}-write-${ids.length}`,email:null,organization:'Synthetic load mutation'}}})
  }})
  phases.push({name:'contact-create',...mutations,targetP95Ms:250})
  assert.equal(mutations.errors,0);assert.equal(mutations.unexpectedStatuses,0);assert.equal(mutations.completed,200)
  for(const id of ids)assert.match(id,/^[0-9a-f-]{36}$/)
  const proof=JSON.parse(sql(`SELECT json_build_object('writes',(SELECT count(*) FROM archava_crm.contacts WHERE full_name LIKE '${prefix}-write-%'),'receipts',(SELECT count(*) FROM archava_crm.commands WHERE request_id=ANY(ARRAY[${ids.map(id=>`'${id}'`).join(',')}]::uuid[]) AND receipt IS NOT NULL));`))
  assert.deepEqual(proof,{writes:200,receipts:200})
  const result={scope:'Local synthetic scale proof; production sizing and hardware require separate acceptance',readRepetitions:repetitions,warmupPerReadRepetition:20,dataset:{contacts:10000,products:2000,uniqueMutations:200},hardware:{platform:platform(),arch:arch(),cpu:cpus()[0]?.model,logicalCpus:cpus().length,memoryBytes:totalmem(),node:process.version},phases,proof,latencyTargetsMet:phases.every(p=>p.p95Ms<=p.targetP95Ms)}
  pass('LOAD-authenticated-data-and-mutations',result)
  // Latency is reported separately from correctness. A missed target remains
  // visible in result.json and cannot be described as production acceptance.
  return result
}
