#!/usr/bin/env node
/**
 * Concurrency & Latency Benchmark for Archava Host.
 * Uses only Node.js built-ins (http, https, performance).
 *
 * Measures throughput and p50, p95, p99 latencies under concurrent client loads
 * to empirically verify non-functional performance targets (e.g. p95 < 50ms for internal queries).
 */
import http from 'node:http'
import https from 'node:https'
import { performance } from 'node:perf_hooks'
import { fileURLToPath } from 'node:url'
import { resolve } from 'node:path'

export async function runBenchmark(options = {}) {
  const {
    target = process.env.ARCHAVA_TARGET || 'http://127.0.0.1:8347',
    path = '/healthz',
    concurrency = 10,
    requests = 100,
    headers = {},
    method = 'GET',
    body = null,
    timeoutMs = 10000,
    expectedStatus = 200,
  } = options

  for (const [name,value,maximum] of [['concurrency',concurrency,256],['requests',requests,1000000],['timeoutMs',timeoutMs,300000]]) {
    if (!Number.isSafeInteger(value) || value<1 || value>maximum) throw new TypeError(`${name} must be an integer from 1 to ${maximum}`)
  }
  if (!Number.isInteger(expectedStatus) || expectedStatus<100 || expectedStatus>599) throw new TypeError('expectedStatus must be an HTTP status')

  const parsedUrl = new URL(path, target)
  if (!['http:','https:'].includes(parsedUrl.protocol) || parsedUrl.origin!==new URL(target).origin) throw new TypeError('Benchmark path must stay on the configured HTTP(S) origin')
  const isHttps = parsedUrl.protocol === 'https:'
  const client = isHttps ? https : http

  const durations = []
  const statusCodes = new Map()
  let errorCount = 0
  let remainingRequests = requests

  const agent = new client.Agent({
    keepAlive: true,
    maxSockets: concurrency * 2,
  })

  const startTime = performance.now()

  async function worker() {
    while (remainingRequests > 0) {
      remainingRequests--
      const reqStart = performance.now()
      try {
        const payload=typeof body==='function'?body():body
        const statusCode = await new Promise((resolvePromise, rejectPromise) => {
          const req = client.request(
            parsedUrl,
            {
              method,
              headers: {
                Connection: 'keep-alive',
                ...headers,
              },
              agent,
              timeout: timeoutMs,
            },
            (res) => {
              res.resume()
              res.on('end', () => resolvePromise(res.statusCode))
            },
          )
          req.on('error', rejectPromise)
          req.on('timeout', () => {
            req.destroy(new Error('Request timed out'))
          })
          if (payload) req.write(payload)
          req.end()
        })
        const reqDuration = performance.now() - reqStart
        durations.push(reqDuration)
        statusCodes.set(statusCode, (statusCodes.get(statusCode) ?? 0) + 1)
      } catch {
        errorCount++
      }
    }
  }

  const workers = Array.from({ length: Math.min(concurrency, requests) }, () => worker())
  await Promise.all(workers)
  agent.destroy()

  const totalTime = (performance.now() - startTime) / 1000
  durations.sort((a, b) => a - b)

  const count = durations.length
  const p50 = count ? durations[Math.floor(count * 0.5)] : 0
  const p95 = count ? durations[Math.floor(count * 0.95)] : 0
  const p99 = count ? durations[Math.floor(count * 0.99)] : 0
  const min = count ? durations[0] : 0
  const max = count ? durations[count - 1] : 0
  const sum = durations.reduce((acc, d) => acc + d, 0)
  const mean = count ? sum / count : 0
  const rps = totalTime > 0 ? count / totalTime : 0

  return {
    target: parsedUrl.origin,
    path: parsedUrl.pathname + parsedUrl.search,
    concurrency,
    totalRequests: requests,
    completed: count,
    errors: errorCount,
    rps: Number(rps.toFixed(2)),
    minMs: Number(min.toFixed(2)),
    meanMs: Number(mean.toFixed(2)),
    p50Ms: Number(p50.toFixed(2)),
    p95Ms: Number(p95.toFixed(2)),
    p99Ms: Number(p99.toFixed(2)),
    maxMs: Number(max.toFixed(2)),
    statusCodes: Object.fromEntries(statusCodes.entries()),
    expectedStatus,
    unexpectedStatuses: [...statusCodes].filter(([status])=>status!==expectedStatus).reduce((sum,[,n])=>sum+n,0),
  }
}

function parseArgs(args) {
  const options = {}
  for (let i = 0; i < args.length; i++) {
    const arg = args[i]
    if (arg === '--help' || arg === '-h') options.help = true
    else if (arg === '--target' && i + 1 < args.length) options.target = args[++i]
    else if (arg === '--path' && i + 1 < args.length) options.path = args[++i]
    else if (arg === '--concurrency' && i + 1 < args.length) options.concurrency = Number(args[++i])
    else if (arg === '--requests' && i + 1 < args.length) options.requests = Number(args[++i])
    else if (arg === '--max-p95' && i + 1 < args.length) options.maxP95 = Number(args[++i])
    else if (arg === '--json') options.json = true
    else if (arg === '--session' && i + 1 < args.length) {
      options.headers = options.headers || {}
      options.headers.cookie = `archava_session=${args[++i]}`
    }
  }
  return options
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const options = parseArgs(process.argv.slice(2))
  if (options.help) {
    process.stdout.write(`Usage: node tools/rust/load-benchmark.mjs [options]

Options:
  --target <url>       Target host URL (default: http://127.0.0.1:8347 or $ARCHAVA_TARGET)
  --path <path>        Request path (default: /healthz)
  --concurrency <num>  Number of concurrent workers (default: 10)
  --requests <num>     Total requests to complete (default: 100)
  --max-p95 <ms>       Assert p95 latency is at most this threshold; exits 1 if exceeded
  --session <token>    Pass session token in archava_session cookie
  --json               Output raw JSON instead of table
  --help               Show this help message
`)
    process.exit(0)
  }

  runBenchmark(options)
    .then((result) => {
      if (options.json) {
        process.stdout.write(JSON.stringify(result, null, 2) + '\n')
      } else {
        process.stdout.write(`
Archava Load Benchmark Results
==============================
Target:         ${result.target}${result.path}
Concurrency:    ${result.concurrency} workers
Total requests: ${result.totalRequests} (Completed: ${result.completed}, Errors: ${result.errors})
Throughput:     ${result.rps} req/sec

Latency (ms):
  Min:          ${result.minMs} ms
  Mean:         ${result.meanMs} ms
  p50 (median): ${result.p50Ms} ms
  p95:          ${result.p95Ms} ms
  p99:          ${result.p99Ms} ms
  Max:          ${result.maxMs} ms

HTTP Statuses:  ${JSON.stringify(result.statusCodes)}
==============================
`)
      }

      if (options.maxP95 !== undefined && result.p95Ms > options.maxP95) {
        process.stderr.write(`FAIL: p95 latency (${result.p95Ms} ms) exceeded maximum threshold (${options.maxP95} ms)\n`)
        process.exit(1)
      }
      if (result.errors || result.unexpectedStatuses || result.completed!==result.totalRequests) {
        process.stderr.write('FAIL: benchmark requests failed or returned unexpected HTTP statuses\n')
        process.exitCode=1
      }
    })
    .catch((err) => {
      process.stderr.write(`Benchmark failed: ${err.message}\n`)
      process.exit(1)
    })
}
