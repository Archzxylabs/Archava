/* global process, URL */
/**
 * P-ERP-PURCHASING producer: the named-witness runner for the R06 purchasing
 * cohort's host suite.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The workspace step already runs the suite this script targets — but it
 * proves only "every test passed", and `writeStepProofs` binds a required check
 * to a step's exit. A step that passes without the proving test is exactly the
 * hole RF01 closed: delete
 * `authorized_flow_changes_stock_exactly_once` from the ERP purchasing suite
 * and the workspace step stays green while the manifest's "authorized flow
 * changes stock exactly once" has no producer left. This script parses the
 * test harness output for each witness BY NAME and fails when a named witness
 * did not run or did not pass, so the check and its proof are the same object.
 *
 * ## What each witness proves
 *
 * One suite, one fixture per test (`archava-host --test erp_purchasing`):
 *
 * The nine required checks from the manifest map to the witnesses in
 * `WITNESSES` below. The suite also carries the receipts-ledger read-backs and
 * the commerce movements counts inside those witnesses, which are additional
 * structural facts, not required checks — they guard the same flow from the
 * sides the required checks do not cover.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-ERP-PURCHASING` in the frozen manifest
 *   must have a witness mapping here, and every mapping must name a manifest
 *   check: either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in the suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-erp-purchasing-proof.json` is written on every run, pass or
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
  purchasing: ['-p', 'archava-host', '--test', 'erp_purchasing'],
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
  'authorized flow changes stock exactly once': [
    ['purchasing', 'authorized_flow_changes_stock_exactly_once'],
  ],
  'over-acceptance refuses without stock': [
    ['purchasing', 'over_acceptance_refuses_without_stock'],
  ],
  'rejected-only delivery adds no stock': [
    ['purchasing', 'rejected_only_delivery_adds_no_stock'],
  ],
  'non-tracked offer refuses the order': [
    ['purchasing', 'non_tracked_offers_refuse_the_order'],
  ],
  // RN-ERP-01: recovery re-answers the policy gate under CURRENT grants, so a
  // purchasing membership revoked after a receipt was recorded gets a refusal
  // on both the execute path and the recovery path — not the stored payload.
  'revoked grant refused on receive and recovery': [
    ['purchasing', 'revoked_grant_refuses_receive_and_recovery'],
  ],
  // RN-ERP-02: the idempotency key identifies the slot; the fingerprint
  // identifies the command. Same key + different bytes conflicts before any
  // statement runs, and nothing of the rejected attempt commits.
  'same key different command conflicts': [
    ['purchasing', 'same_key_different_command_conflicts'],
  ],
  'cross-tenant caller refused, keys per tenant': [
    ['purchasing', 'cross_tenant_caller_is_refused_and_keys_stay_per_tenant'],
  ],
  'missing purchasing grant refused': [
    ['purchasing', 'missing_purchasing_grant_refuses_the_ladder'],
  ],
  // Lost-response recovery: the status read answers `committed` from the
  // stored receipt while a byte-identical resubmit replays with no second
  // effect — the receipt survives, the caller does not invent a duplicate.
  'lost response recovers by replay and status': [
    ['purchasing', 'lost_response_recovers_by_replay_and_status'],
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
 * Run the purchasing suite, grade every required check against its named
 * witnesses, write `dist/rust-erp-purchasing-proof.json`, and exit non-zero on
 * any failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runPurchasing() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-ERP-PURCHASING']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-ERP-PURCHASING is not an evidence proof in the frozen manifest')

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

  process.stdout.write('  $ cargo test (erp purchasing suite, named witnesses)\n')
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
        `suite ${key} exited ${run.exitCode} — a failing purchasing suite fails every check it backs`,
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
  // purchasing grant that still receives or recovers receipts, and an
  // idempotency key answered twice for different commands. The strings are
  // the check names the run must have graded, so the tally is derived from
  // `checks` rather than written blind: a regression name that is not a
  // graded check is a wiring drift the producer refuses, and a check that
  // failed has already pushed a problem — the record then differs from the
  // frozen expectation and the coverage validator says so.
  const regressionChecks = [
    'revoked grant refused on receive and recovery',
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
  const proofPath = join(root, 'dist/rust-erp-purchasing-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-erp-purchasing-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runPurchasing()
  if (outcome.ok) {
    process.stdout.write('PASS erp purchasing suite — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL erp purchasing suite — see the witness lines above\n')
    process.exitCode = 1
  }
}
