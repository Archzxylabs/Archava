/**
 * Refusal reason catalogue against the database guards.
 *
 * `config/refusal-reasons.v1.json` is keyed by the exact message of a guard
 * (`RAISE EXCEPTION '<message>'` in `migrations/`). This check fails when a
 * key matches no guard message (a typo, or a guard that no longer exists) and
 * reports, without failing, how many `23514` guard messages of each ledger
 * have no entry. See docs/architecture/REFUSAL_REASONS.md.
 */
import { readdirSync, readFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const CATALOGUE = 'config/refusal-reasons.v1.json'
/** The SQLSTATE the adapter classifier reads (crates/adapters/src/erp/mod.rs). */
export const GUARD_STATE = '23514'

/** Every `RAISE EXCEPTION '<literal>'` of one SQL text: message, SQLSTATE and whether it takes format arguments. */
export function raisedMessages(sql) {
  // Line comments are dropped so a sentence quoted in prose is not a guard.
  const code = sql.replace(/^\s*--.*$/gm, '')
  const found = []
  const pattern = /RAISE\s+EXCEPTION\s+'((?:[^']|'')*)'([^;]*);/gi
  for (let match; (match = pattern.exec(code));) {
    const state = /ERRCODE\s*=\s*'([0-9A-Z]{5})'/i.exec(match[2])
    const message = match[1].replaceAll("''", "'")
    found.push({ message, state: state ? state[1].toUpperCase() : 'P0001', dynamic: message.includes('%') })
  }
  return found
}

/** All guard messages under `migrations/`, each with its ledger directory and file. */
export function guardMessages(root) {
  const base = join(root, 'migrations')
  const found = []
  for (const ledger of readdirSync(base, { withFileTypes: true }).filter((entry) => entry.isDirectory()).map((entry) => entry.name).sort()) {
    for (const file of readdirSync(join(base, ledger)).filter((name) => name.endsWith('.sql')).sort()) {
      for (const raised of raisedMessages(readFileSync(join(base, ledger, file), 'utf8'))) found.push({ ledger, file, ...raised })
    }
  }
  return found
}

const SNAKE = /^[a-z][a-z0-9]*(_[a-z0-9]+)*$/

/** Problems (stale keys, malformed entries) and per-ledger coverage of the `23514` guard messages. */
export function checkRefusalReasons(root) {
  const document = JSON.parse(readFileSync(join(root, CATALOGUE), 'utf8'))
  const reasons = document.reasons ?? {}
  const guards = guardMessages(root)
  const raised = new Map()
  for (const guard of guards) if (!guard.dynamic) raised.set(guard.message, [...(raised.get(guard.message) ?? []), guard])
  const problems = []
  const codes = new Map()
  for (const [message, entry] of Object.entries(reasons)) {
    if (!raised.has(message)) problems.push(`${CATALOGUE}: "${message}" matches no RAISE EXCEPTION message in migrations/ (stale key or typo)`)
    if (typeof entry.code !== 'string' || !SNAKE.test(entry.code)) problems.push(`${CATALOGUE}: "${message}" needs a snake_case code`)
    else if (codes.has(entry.code)) problems.push(`${CATALOGUE}: code ${entry.code} is used by both "${codes.get(entry.code)}" and "${message}"`)
    else codes.set(entry.code, message)
    for (const field of ['message', 'message_id']) if (typeof entry[field] !== 'string' || entry[field].trim().length < 20) problems.push(`${CATALOGUE}: "${message}" needs a ${field} sentence`)
    const extra = Object.keys(entry).filter((key) => !['code', 'message', 'message_id'].includes(key))
    if (extra.length) problems.push(`${CATALOGUE}: "${message}" has unknown members: ${extra.join(', ')}`)
  }
  const coverage = new Map()
  for (const guard of guards) {
    if (guard.state !== GUARD_STATE) continue
    const row = coverage.get(guard.ledger) ?? { ledger: guard.ledger, messages: new Set(), catalogued: new Set(), dynamic: new Set() }
    row.messages.add(guard.message)
    if (guard.dynamic) row.dynamic.add(guard.message)
    else if (Object.hasOwn(reasons, guard.message)) row.catalogued.add(guard.message)
    coverage.set(guard.ledger, row)
  }
  const ledgers = [...coverage.values()].map((row) => ({
    ledger: row.ledger,
    guards: row.messages.size,
    catalogued: row.catalogued.size,
    uncatalogued: row.messages.size - row.catalogued.size,
    dynamic: row.dynamic.size,
    missing: [...row.messages].filter((message) => !row.catalogued.has(message)).sort(),
  }))
  // Catalogue keys whose guard raises with another SQLSTATE: the classifier does not read them from the database.
  const outsideGuardState = Object.keys(reasons).filter((message) => raised.has(message) && !raised.get(message).some((guard) => guard.state === GUARD_STATE))
  return { problems, ledgers, entries: Object.keys(reasons).length, outsideGuardState }
}

export function coverageLines(result) {
  const lines = result.ledgers.map((row) => `${row.ledger}: ${row.catalogued}/${row.guards} ${GUARD_STATE} guard messages catalogued, ${row.uncatalogued} uncatalogued`)
  const total = result.ledgers.reduce((sum, row) => sum + row.uncatalogued, 0)
  lines.push(`total: ${result.entries} catalogue entries, ${total} uncatalogued ${GUARD_STATE} guard messages`)
  if (result.outsideGuardState.length) lines.push(`${result.outsideGuardState.length} entries name a guard raised with another SQLSTATE (reached only where an adapter names the code itself)`)
  return lines
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const root = resolve(dirname(fileURLToPath(import.meta.url)), '..')
  const result = checkRefusalReasons(root)
  if (process.argv.includes('--missing')) for (const row of result.ledgers) for (const message of row.missing) process.stdout.write(`${row.ledger}\t${message}\n`)
  for (const line of coverageLines(result)) process.stdout.write(`${line}\n`)
  for (const problem of result.problems) process.stderr.write(`${problem}\n`)
  if (result.problems.length) process.exitCode = 1
  else process.stdout.write('PASS refusal reasons: every catalogue key is a guard message\n')
}
