#!/usr/bin/env node
/**
 * Aggregate a vitest v8 `coverage-final.json` per workspace package, using the
 * exact istanbul summary arithmetic that the coverage gate itself uses
 * (`percent()` floors to two decimals), so the numbers printed here can be
 * pasted into `test.coverage.thresholds` without a rounding argument.
 *
 * Usage: node per-package-coverage.mjs <coverage-final.json> [--md]
 */
import { readdirSync, readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const ROOT = path.resolve(here, '..', '..', '..', '..')
const requireRoot = createRequire(path.join(ROOT, 'package.json'))

/**
 * `istanbul-lib-coverage` is a transitive dependency of `@vitest/coverage-v8`;
 * pnpm's strict layout does not expose it at the repo root, so fall back to the
 * .pnpm store directory when the plain resolve fails.
 */
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
const KEYS = ['lines', 'functions', 'branches', 'statements']

const file = process.argv[2]
if (!file) {
  console.error('usage: node per-package-coverage.mjs <coverage-final.json> [--md]')
  process.exit(2)
}
const asMd = process.argv.includes('--md')

const coverageMap = lib.createCoverageMap(JSON.parse(readFileSync(file, 'utf8')))
const groups = new Map()

for (const abs of coverageMap.files()) {
  const rel = path.relative(ROOT, abs).split(path.sep).join('/')
  const [head, second] = rel.split('/')
  const key =
    second && (head === 'packages' || head === 'apps' || head === 'tools')
      ? `${head}/${second}`
      : head
  if (!groups.has(key)) groups.set(key, lib.createCoverageMap({}))
  groups.get(key).addFileCoverage(coverageMap.fileCoverageFor(abs))
}

const rows = [...groups.entries()]
  .sort(([a], [b]) => a.localeCompare(b))
  .map(([key, map]) => ({ key, files: map.files().length, summary: map.getCoverageSummary() }))

const pct = (summary, key) => summary.data[key].pct

if (asMd) {
  console.log('| scope | files | lines % | functions % | branches % | statements % |')
  console.log('| --- | ---: | ---: | ---: | ---: | ---: |')
  for (const r of rows) {
    console.log(
      `| \`${r.key}\` | ${r.files} | ${KEYS.map((k) => pct(r.summary, k).toFixed(2)).join(' | ')} |`,
    )
  }
  const total = coverageMap.getCoverageSummary()
  console.log(
    `| **TOTAL** | ${coverageMap.files().length} | ${KEYS.map((k) => pct(total, k).toFixed(2)).join(' | ')} |`,
  )
} else {
  console.log(
    [
      'scope'.padEnd(34),
      'files'.padStart(6),
      ...KEYS.map((k) => `${k}%`.padStart(12)),
      ...KEYS.map(() => '-total'.padStart(8)),
      ...KEYS.map(() => '-hit'.padStart(7)),
    ].join(''),
  )
  for (const r of rows) {
    const nums = KEYS.map((k) => pct(r.summary, k).toFixed(2).padStart(12)).join('')
    const totals = KEYS.map((k) => String(r.summary.data[k].total).padStart(8)).join('')
    const hits = KEYS.map((k) => String(r.summary.data[k].covered).padStart(7)).join('')
    console.log(`${r.key.padEnd(34)}${String(r.files).padStart(6)}${nums}${totals}${hits}`)
  }
  const total = coverageMap.getCoverageSummary()
  console.log(
    `\nTOTAL files=${coverageMap.files().length} ` +
      KEYS.map(
        (k) => `${k}=${pct(total, k).toFixed(2)} (${total.data[k].covered}/${total.data[k].total})`,
      ).join(' '),
  )
}
