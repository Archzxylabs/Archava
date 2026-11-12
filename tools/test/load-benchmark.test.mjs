import test from 'node:test'
import assert from 'node:assert/strict'
import http from 'node:http'
import { runBenchmark } from '../rust/load-benchmark.mjs'

test('load-benchmark executes concurrent requests and computes percentile distribution', async () => {
  let hitCount = 0
  const server = http.createServer((req, res) => {
    hitCount++
    // Simulate brief 2ms processing delay
    setTimeout(() => {
      res.writeHead(200, { 'Content-Type': 'application/json' })
      res.end(JSON.stringify({ status: 'ok' }))
    }, 2)
  })

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  const port = server.address().port

  try {
    const result = await runBenchmark({
      target: `http://127.0.0.1:${port}`,
      path: '/healthz',
      concurrency: 5,
      requests: 25,
      timeoutMs: 3000,
    })

    assert.equal(result.totalRequests, 25)
    assert.equal(result.completed, 25)
    assert.equal(result.errors, 0)
    assert.equal(result.statusCodes['200'], 25)
    assert.ok(result.rps > 0)
    assert.ok(result.p50Ms >= 1)
    assert.ok(result.p95Ms >= result.p50Ms)
    assert.ok(result.maxMs >= result.p95Ms)
    assert.equal(hitCount, 25)
  } finally {
    await new Promise((resolve) => server.close(resolve))
  }
})

test('failed HTTP responses are not a passing latency proof',async()=>{
 const server=http.createServer((req,res)=>{res.writeHead(503);res.end('unavailable')})
 await new Promise(r=>server.listen(0,'127.0.0.1',r))
 try{
  const result=await runBenchmark({target:`http://127.0.0.1:${server.address().port}`,requests:4,concurrency:2})
  assert.equal(result.unexpectedStatuses,4)
  assert.equal(result.statusCodes['503'],4)
 }finally{await new Promise(r=>server.close(r))}
})

test('mutation load uses separate request bodies and bounded valid options',async()=>{
 const seen=new Set()
 const server=http.createServer(async(req,res)=>{let text='';for await(const chunk of req)text+=chunk;seen.add(JSON.parse(text).requestId);res.end('{}')})
 await new Promise(r=>server.listen(0,'127.0.0.1',r))
 try{
  let next=0
  const target=`http://127.0.0.1:${server.address().port}`
  const result=await runBenchmark({target,method:'POST',requests:12,concurrency:3,body:()=>JSON.stringify({requestId:++next})})
  assert.equal(seen.size,12)
  assert.equal(result.unexpectedStatuses,0)
  for(const invalid of [{requests:0},{requests:NaN},{concurrency:-1},{concurrency:Infinity},{concurrency:257},{timeoutMs:0},{path:'https://example.invalid/'}]) {
   await assert.rejects(runBenchmark({target,...invalid}),TypeError)
  }
 }finally{await new Promise(r=>server.close(r))}
})
