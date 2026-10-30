/** Native process lifecycle on the browser gate's isolated PostgreSQL fixture. */
import assert from 'node:assert/strict'
import {spawnSync} from 'node:child_process'
import {writeFileSync} from 'node:fs'
import {join} from 'node:path'

export function readWriterLease(port) {
 const query="SELECT json_build_object('epoch',epoch,'drained',drained) FROM archava_control.writer_lease WHERE lease_name='archava-serving';"
 const result=spawnSync('psql',['-X','-q','-A','-t','-v','ON_ERROR_STOP=1'],{input:query,encoding:'utf8',env:{...process.env,PGHOST:'127.0.0.1',PGPORT:String(port),PGDATABASE:'archava_test',PGUSER:'postgres',PGPASSWORD:'archava'}})
 assert.equal(result.status,0,result.stderr)
 const row=JSON.parse(result.stdout.trim());assert.ok(Number.isSafeInteger(row.epoch));assert.equal(typeof row.drained,'boolean');return row
}
export async function verifyStartupRefusals({production,env,root,output,origin,freePort,readLease,pass}) {
 const before=readLease()
 for(const [name,override] of [
  ['duplicate-port',{ARCHAVA_WRITER_TAKEOVER:'true'}],
  ['active-writer',{ARCHAVA_WRITER_TAKEOVER:'false',ARCHAVA_BIND_ADDR:`127.0.0.1:${await freePort()}`}],
 ]) {
  const result=spawnSync(production,[],{cwd:root,env:{...process.env,...env,...override},encoding:'utf8',timeout:15000})
  writeFileSync(join(output,`native-${name}-refusal.log`),`${result.stdout??''}${result.stderr??''}`)
  assert.equal(result.signal,null,`${name} must return a refusal, not time out`)
  assert.ok(Number.isInteger(result.status)&&result.status!==0,`${name} unexpectedly started`)
  assert.deepEqual(readLease(),before,`${name} must not replace the healthy writer`)
  assert.ok((await fetch(`${origin}/readyz`)).ok)
 }
 pass('Ops-native-startup-refusals','Duplicate bind fails before takeover; a free port with default takeover=false refuses the active lease; the original process stays ready with its epoch unchanged')
}
export async function verifyWriterLifecycle({production,env,origin,original,startServer,terminate,until,freePort,readLease,pass}) {
 const before=readLease()
 const port=await freePort();const replacementOrigin=`http://127.0.0.1:${port}`
 const replacement=startServer(production,{...env,ARCHAVA_BIND_ADDR:`127.0.0.1:${port}`,ARCHAVA_WRITER_TAKEOVER:'true'},'native-takeover.log')
 await until(async()=> (await fetch(`${replacementOrigin}/readyz`)).ok,'replacement writer native readiness',60000)
 const current=readLease();assert.ok(current.epoch>before.epoch);assert.equal(current.drained,false)
 assert.equal((await fetch(`${origin}/readyz`)).status,503)
 const login=address=>fetch(`${address}/login`,{method:'POST',headers:{'content-type':'application/json','x-archava-client':'cli'},body:JSON.stringify({username:'alice@archava.test',password:'correct horse battery staple'})})
 const oldWrite=await login(origin);assert.ok(oldWrite.status>=500,'stale native process must not create a session')
 assert.equal((await login(replacementOrigin)).status,200,'replacement process can perform a real Identity write')
 await terminate(original)
 assert.deepEqual(readLease(),current,'old-holder shutdown cannot drain the successor')
 assert.ok((await fetch(`${replacementOrigin}/readyz`)).ok)
 await terminate(replacement)
 assert.deepEqual(readLease(),{epoch:current.epoch,drained:true})
 const restarted=startServer(production,{...env,ARCHAVA_WRITER_TAKEOVER:'false'},'native-after-drain.log')
 await until(async()=> (await fetch(`${origin}/readyz`)).ok,'normal startup after graceful drain',60000)
 const next=readLease();assert.ok(next.epoch>current.epoch);assert.equal(next.drained,false)
 await terminate(restarted)
 assert.deepEqual(readLease(),{epoch:next.epoch,drained:true})
 pass('Ops-native-writer-lifecycle',{beforeEpoch:before.epoch,takeoverEpoch:current.epoch,restartedEpoch:next.epoch,staleLoginStatus:oldWrite.status,proof:'Two actual native processes: old readiness/write refused, stale shutdown preserves successor, graceful drain permits default=false restart'})
}
