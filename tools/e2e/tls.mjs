/** Task-owned HTTPS/proxy rehearsal. Never installs a CA in user/system trust. */
import assert from 'node:assert/strict'
import { spawn, spawnSync } from 'node:child_process'
import { createWriteStream, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'

export async function verifyTls({origin,scratch,output,freePort,until,pass,terminate}) {
  assert.equal(new URL(origin).hostname,'127.0.0.1')
  const port=await freePort(), directory=join(scratch,'tls-proxy'), storage=join(directory,'storage')
  mkdirSync(storage,{recursive:true})
  const config=join(directory,'Caddyfile'), certificate=join(storage,'pki/authorities/local/root.crt')
  writeFileSync(config,`{
 admin off
 persist_config off
 skip_install_trust
 auto_https disable_redirects
 storage file_system {
  root ${JSON.stringify(storage)}
 }
}
https://localhost:${port} {
 bind 127.0.0.1
 tls internal
 @metrics path /metrics
 respond @metrics 404
 reverse_proxy ${new URL(origin).host}
}
`)
  const log=createWriteStream(join(output,'tls-proxy.log'))
  const child=spawn('caddy',['run','--config',config],{env:{...process.env,XDG_CONFIG_HOME:join(directory,'config'),XDG_DATA_HOME:storage},stdio:['ignore','pipe','pipe']})
  let startError
  child.on('error',error=>{startError=error})
  child.stdout.pipe(log,{end:false});child.stderr.pipe(log,{end:false})
  let request=0
  const call=(path,{source='127.0.0.3',password,forwarded}={})=>{
    const id=++request, head=join(directory,`headers-${id}`), body=join(directory,`body-${id}`)
    const args=['--silent','--show-error','--max-time','10','--noproxy','*','--cacert',certificate,'--resolve',`localhost:${port}:127.0.0.1`,'--interface',source,'--dump-header',head,'--output',body,'--write-out','%{http_code}']
    if(password!==undefined)args.push('--request','POST','--header','content-type: application/json','--header','x-archava-client: cli','--data-binary',JSON.stringify({username:'alice@archava.test',password}))
    if(forwarded)args.push('--header',`x-forwarded-for: ${forwarded}`)
    args.push(`https://localhost:${port}${path}`)
    const result=spawnSync('curl',args,{encoding:'utf8',timeout:12000})
    assert.equal(result.status,0,result.stderr)
    return {status:Number(result.stdout),headers:readFileSync(head,'utf8'),body:readFileSync(body,'utf8')}
  }
  try {
    await until(()=>{if(startError)throw startError;if(child.exitCode!==null)throw Error('Owned TLS proxy exited');return existsSync(certificate)},'owned TLS certificate')
    await until(()=>{try{return call('/readyz').status===200}catch{return false}},'verified HTTPS readiness')
    const blocked=[]
    for(let i=0;i<5;i++)blocked.push(call('/login',{source:'127.0.0.2',password:'wrong password',forwarded:`203.0.113.${10+i}`}).status)
    assert.deepEqual(blocked,[401,401,401,401,401])
    const noisy=call('/login',{source:'127.0.0.2',password:'correct horse battery staple',forwarded:'198.51.100.10'})
    assert.equal(noisy.status,429,'spoofed forwarded header cannot escape the real client budget')
    const quiet=call('/login',{password:'correct horse battery staple'})
    assert.equal(quiet.status,200,'a separate client behind Caddy keeps its own budget')
    assert.match(quiet.headers,/set-cookie: __Host-archava_session=[^\r\n]*HttpOnly[^\r\n]*Secure/i)
    assert.match(quiet.headers,/strict-transport-security: max-age=31536000/i)
    assert.match(quiet.headers,/cache-control: no-store/i)
    const document=call('/app')
    assert.equal(document.status,200)
    assert.match(document.headers,/content-security-policy:[^\r\n]*wasm-unsafe-eval/i)
    assert.match(document.headers,/x-content-type-options: nosniff/i)
    assert.equal(call('/metrics').status,404,'public proxy blocks the monitoring endpoint')
    const result={scope:'Loopback HTTPS with an explicit task CA; public DNS/ACME not exercised',caInstalled:false,certificateVerified:true,secureHostCookie:true,hsts:true,documentCsp:true,publicMetricsStatus:404,attackerAttempts:blocked,attackerCorrectPasswordStatus:noisy.status,independentClientStatus:quiet.status}
    pass('TLS-real-proxy-and-client-isolation',result)
    return result
  } finally {
    await terminate(child)
    await new Promise(resolve=>log.end(resolve))
  }
}
