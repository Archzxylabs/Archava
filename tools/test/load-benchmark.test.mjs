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
