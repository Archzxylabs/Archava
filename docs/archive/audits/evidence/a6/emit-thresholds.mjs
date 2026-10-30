#!/usr/bin/env node
/**
 * Emit the `measuredPackageThresholds` literal for vitest.config.ts from a
 * measured `coverage-final.json`, so the gate table is transcribed by the same
 * arithmetic that measures it rather than by hand.
 *
 * Usage: node emit-thresholds.mjs <coverage-final.json>
 */
import { readdirSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(here, '..', '..', '..', '..')
const requireRoot = createRequire(path.join(ROOT, 'package.json'))

function loadIstanbul() {
  try {
    return requireRoot('istanbul-lib-coverage')
  } catch {
    const store = path.join(ROOT, 'node_modules', '.pnpm')
    const dir = readdirSync(store).find((entry) => entry.startsWith('istanbul-lib-coverage@'))
    if (!dir) throw new Error('istanbul-lib-coverage not found in node_modules/.pnpm')
    return requireRoot(path.join(store, dir, 'node_modules', 'istanbul-lib-coverage'))
  }
}

const lib = loadIstanbul()
const coverageMap = lib.createCoverageMap(JSON.parse(readFileSync(process.argv[2], 'utf8')))
const groups = new Map()

for (const abs of coverageMap.files()) {
  const rel = path.relative(ROOT, abs).split(path.sep).join('/')
  const [head, second] = rel.split('/')
  const key = second && (head === 'packages' || head === 'apps') ? `${head}/${second}` : head
  if (!groups.has(key)) groups.set(key, lib.createCoverageMap({}))
  groups.get(key).addFileCoverage(coverageMap.fileCoverageFor(abs))
}

const KEYS = ['statements', 'branches', 'functions', 'lines']
const zero = []

for (const [key, map] of [...groups.entries()].sort(([a], [b]) => a.localeCompare(b))) {
  const summary = map.getCoverageSummary()
  const values = KEYS.map((k) => summary.data[k].pct)
  if (values.every((v) => v === 0)) {
    zero.push(key)
    continue
  }
  const body = KEYS.map((k) => `${k}: ${summary.data[k].pct}`).join(', ')
  console.log(`  '${key}/src/**/*.ts': { ${body} },`)
}

console.error(
  `zero-coverage scopes (${zero.length}): ${zero.map((z) => z.replace(/^(packages|apps)\//, '')).join(', ')}`,
)
