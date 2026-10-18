/* global process, URL */
/**
 * P-ERP-FOODSERVICE producer: the named-witness runner for the R06 foodservice
 * cohort's host suite.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The workspace step already runs the suite this script targets — but it
 * proves only "every test passed", and `writeStepProofs` binds a required check
 * to a step's exit. A step that passes without the proving test is exactly the
 * hole RF01 closed: delete
 * `authorized_food_flow_consumes_the_shared_pool_exactly_once` from the ERP
 * foodservice suite and the workspace step stays green while the manifest's
 * "authorized food flow consumes the shared pool exactly once" has no
 * producer left. This script parses the
 * test harness output for each witness BY NAME and fails when a named witness
 * did not run or did not pass, so the check and its proof are the same object.
 *
 * ## What each witness proves
 *
 * One suite, one fixture per test (`archava-host --test erp_foodservice`):
 *
 * The nine required checks from the manifest map to the witnesses in
 * `WITNESSES` below. The suite also carries the receipts-ledger read-backs and
 * the commerce movements counts inside those witnesses, which are additional
 * structural facts, not required checks — they guard the same flow from the
 * sides the required checks do not cover.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-ERP-FOODSERVICE` in the frozen manifest
 *   must have a witness mapping here, and every mapping must name a manifest
 *   check: either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in the suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-erp-foodservice-proof.json` is written on every run, pass or
 *   fail; `verify.mjs` binds it as `details` and as a referenced producing
 *   file. Evidence itself is written by `writeStepProofs` from the step exit,
 *   so a red run replaces any stale green claim instead of leaving it
 *   readable.
 */

import { execFileSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from '../coverage.mjs'

const root = fileURLToPath(new URL('../../../', import.meta.url))
const workspace = join(root, 'rust')

/** The suite this producer targets, keyed by the `suite` field every witness refers to. */
export const SUITES = {
  foodservice: ['-p', 'archava-host', '--test', 'erp_foodservice'],
}

/**
 * Manifest required check → the suite tests that produce it.
 *
 * Each witness is `[suite, testName]` where `testName` is the exact identifier
 * the harness prints (`test <name> ... ok`). Deliberately duplicated with the
 * Rust source rather than derived from it: a mapping derived from the code it
 * checks can only ever agree with itself, and the drift is the thing that must
 * be visible. The sync direction is enforced at runtime below and by
 * `verify.test.mjs` against the frozen manifest.
 */
export const WITNESSES = {
  // The whole kitchen slice on ONE writer: the commerce stock pool's second
  // consumer mints the draft, reserves with recipe×line allocations, fulfills
  // with consume movements, and the movement count proves the pool moved
  // exactly once (stock 100 → 98 for a −2 fulfill) with receipts bound in
  // BOTH ledgers under the same request id.
  'authorized food flow consumes the shared pool exactly once': [
    ['foodservice', 'authorized_food_flow_consumes_the_shared_pool_exactly_once'],
  ],
  // Over-reservation (120 > 100) refuses with a conflict BEFORE any hold or
  // command row: nothing is reserved, nothing is recorded, the pool is
  // untouched.
  'over-reservation refuses without holding stock': [
    ['foodservice', 'over_reservation_refuses_without_holding_stock'],
  ],
  // A recipe ingredient that is not tracked (or managed, or inactive)
  // refuses the whole menu.create — no menu row, no recipe rows, no
  // half-written kernel state.
  'recipe refuses a non-tracked ingredient': [
    ['foodservice', 'recipe_refuses_non_tracked_ingredient'],
  ],
  // RN-ERP-01: recovery re-answers the commerce policy gate under CURRENT
  // grants, so a membership revoked after a receipt was recorded gets a
  // refusal on both the execute path and the status path — not the stored
  // payload.
  'revoked food grant refused on recovery': [
    ['foodservice', 'revoked_food_grant_refuses_recovery'],
  ],
  // RN-ERP-02: the idempotency key identifies the slot; the fingerprint
  // identifies the command. Same key + different bytes conflicts before any
  // statement runs, and nothing of the rejected attempt commits.
  'same key different command conflicts': [
    ['foodservice', 'same_key_different_command_conflicts_without_recording'],
  ],
  // A caller outside the rows' tenant is refused before any write, and the
  // same caller acting as a GLOBEX member succeeds — keys stay per tenant.
  'cross-tenant caller refused before any write': [
    ['foodservice', 'cross_tenant_caller_refused_before_any_write'],
  ],
  // Lost-response recovery: the status read answers `committed` from the
  // stored receipt while a byte-identical resubmit replays with no second
  // held allocation or second effect.
  'lost response replays without a second reservation': [
    ['foodservice', 'lost_response_replays_without_a_second_reservation'],
  ],
  // Cancel releases the reservation (order cancelled, allocation released,
  // zero movements) — a ticket that never prepared leaves the pool untouched.
  'cancel releases the reservation untouched': [
    ['foodservice', 'cancel_releases_the_reservation_before_any_consumption'],
  ],
  // The commerce membership ladder gates the whole namespace before any
  // command is claimed or recorded.
  'missing foodservice grant refused': [
    ['foodservice', 'grants_missing_actor_refused_before_any_write'],
  ],
}

/**
 * Run the suite, capturing output even when it fails.
 *
 * `execFileSync` throws on a non-zero exit but carries the captured stdout on
 * the error, which is where the witness lines are — a failing suite is the run
 * this script exists to grade, so its output must not be lost to the throw.
 *
 * @param {string[]} cargoArgs arguments after `cargo`
 * @returns {{ exitCode: number|null, seconds: number, stdout: string, tests: Map<string, string> }}
 */
function runSuite(cargoArgs) {
  const started = Date.now()
  let stdout = ''
  let exitCode = 0
  try {
    stdout = execFileSync('cargo', cargoArgs, {
      cwd: workspace,
      encoding: 'utf8',
      stdio: 'pipe',
      maxBuffer: 32 * 1024 * 1024,
    })
  } catch (error) {
    stdout = error.stdout ?? ''
    exitCode = error.status ?? null
  }
  // `test <identifier> ... ok|FAILED|ignored` — one line per test, printed by
  // libtest regardless of parallel scheduling, so names cannot interleave.
  const tests = new Map()
  for (const line of stdout.split('\n')) {
    const match = /^test (\S+) \.\.\. (ok|FAILED|ignored)/.exec(line)
    if (match) tests.set(match[1], match[2])
  }
  return {
    exitCode,
    seconds: Number(((Date.now() - started) / 1000).toFixed(1)),
    stdout,
    tests,
  }
}

/**
 * Run the foodservice suite, grade every required check against its named
 * witnesses, write `dist/rust-erp-foodservice-proof.json`, and exit non-zero on
 * any failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runFoodservice() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-ERP-FOODSERVICE']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-ERP-FOODSERVICE is not an evidence proof in the frozen manifest')

  const problems = []

  // Either direction of drift is an error: a manifest check nobody produces,
  // or a mapping naming a check the manifest does not require.
  const required = spec.requiredChecks ?? []
  for (const check of required)
    if (!WITNESSES[check])
      problems.push(`no witness wired for required check ${JSON.stringify(check)}`)
  for (const check of Object.keys(WITNESSES))
    if (!required.includes(check))
      problems.push(
        `witness map names check ${JSON.stringify(check)} the manifest does not require`,
      )

  process.stdout.write('  $ cargo test (erp foodservice suite, named witnesses)\n')
  const suites = {}
  const runs = {}
  for (const [key, cargoArgs] of Object.entries(SUITES)) {
    const argv = ['test', ...cargoArgs, '--locked', '--offline']
    const run = runSuite(argv)
    runs[key] = run
    const passed = [...run.tests.values()].filter((s) => s === 'ok').length
    const failed = [...run.tests.values()].filter((s) => s === 'FAILED').length
    suites[key] = {
      command: `cargo ${argv.join(' ')}`,
      exitCode: run.exitCode,
      seconds: run.seconds,
      testsDiscovered: run.tests.size,
      testsPassed: passed,
      testsFailed: failed,
    }
    process.stdout.write(
      `     ${run.exitCode === 0 ? '✓' : '✗'} suite ${key}: ${passed} passed, ${failed} failed` +
        ` (discovered ${run.tests.size}, exit ${run.exitCode}, ${run.seconds}s)\n`,
    )
    if (run.exitCode !== 0)
      problems.push(
        `suite ${key} exited ${run.exitCode} — a failing foodservice suite fails every check it backs`,
      )
  }

  const checks = {}
  for (const check of required) {
    const witnesses = WITNESSES[check]
    if (!witnesses) {
      checks[check] = { witnesses: [], passed: false }
      continue
    }
    const observed = witnesses.map(([suite, test]) => {
      const status = runs[suite]?.tests.get(test)
      if (status === undefined) {
        problems.push(
          `witness ${JSON.stringify(`${suite}:${test}`)} for ${JSON.stringify(check)} did not run`,
        )
        return { suite, test, status: 'not-run' }
      }
      if (status !== 'ok')
        problems.push(
          `witness ${JSON.stringify(`${suite}:${test}`)} for ${JSON.stringify(check)} is ${status}`,
        )
      return { suite, test, status }
    })
    const passed = observed.every((w) => w.status === 'ok')
    checks[check] = { witnesses: observed, passed }
    process.stdout.write(`     ${passed ? '✓' : '✗'} ${check} (${observed.length} witness(es))\n`)
  }

  const allPassed = problems.length === 0 && Object.values(checks).every((c) => c.passed)
  // `witnessTotal` is the frozen tally: it pins the SIZE of the witness map in
  // the manifest's `expect`, so removing a witness from the script while its
  // check still has others is a visible mismatch rather than a silent
  // weakening of the proof.
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)

  // `namedRegressions` is the second frozen tally. RN-ERP-01 and RN-ERP-02
  // are the two regression notes this suite exists to answer — a revoked
  // foodservice grant that still replays receipts through recovery, and an
  // idempotency key answered twice for different commands. The strings are
  // the check names the run must have graded, so the tally is derived from
  // `checks` rather than written blind: a regression name that is not a
  // graded check is a wiring drift the producer refuses, and a check that
  // failed has already pushed a problem — the record then differs from the
  // frozen expectation and the coverage validator says so.
  const regressionChecks = [
    'revoked food grant refused on recovery',
    'same key different command conflicts',
  ]
  for (const name of regressionChecks)
    if (!(name in checks)) problems.push(`named regression check ${name} was not graded`)
  const namedRegressions = Object.fromEntries(
    regressionChecks.map((name, index) => [
      `RN-ERP-0${index + 1}`,
      checks[name]?.passed === true
        ? name
        : `not satisfied: the witness for ${name} did not reach pass`,
    ]),
  )

  const details = { suites, checks, problems, witnessTotal, namedRegressions, allPassed }

  // Written on every run: the gate reads it as `details` and binds its hash as
  // a referenced producing file, and a failing run must record its own truth
  // rather than leave the previous green file to be read as still current.
  const proofPath = join(root, 'dist/rust-erp-foodservice-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-erp-foodservice-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runFoodservice()
  if (outcome.ok) {
    process.stdout.write('PASS erp foodservice suite — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL erp foodservice suite — see the witness lines above\n')
    process.exitCode = 1
  }
}
