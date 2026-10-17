/* global process, URL */
/**
 * P-AUTHORITY-PG producer: the named-witness runner for the authority suite.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The workspace step already runs the suite this script targets — but it
 * proves only "every test passed", and `writeStepProofs` binds a required check
 * to a step's exit. A step that passes without the proving test is exactly the
 * hole RF01 closed: delete `a_forged_role_and_approval_in_the_body_change_nothing`
 * from the authority suite and the workspace step stays green while the
 * manifest's "forged tenant/role/approval refused" has no producer left.
 * This script parses the test harness output for each witness BY NAME and fails
 * when a named witness did not run or did not pass, so the check and its proof
 * are the same object.
 *
 * ## What each witness proves
 *
 * One suite, one fixture per test (`archava-host --test authority`):
 *
 * The eight required checks from the manifest map to the witnesses in
 * `WITNESSES` below. The suite also carries CSRF, approval-single-use,
 * service-approver, self-approval, unknown-action, scope-rule and
 * declined-confirmation negatives, which are additional structural refusals,
 * not required checks — they guard the same gate from the sides the required
 * checks do not cover.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-AUTHORITY-PG` in the frozen manifest
 *   must have a witness mapping here, and every mapping must name a manifest
 *   check: either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in the suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-authority-proof.json` is written on every run, pass or fail;
 *   `verify.mjs` binds it as `details` and as a referenced producing file.
 *   Evidence itself is written by `writeStepProofs` from the step exit, so a
 *   red run replaces any stale green claim instead of leaving it readable.
 */

import { execFileSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from './coverage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = join(root, 'rust')

/** The suite this producer targets, keyed by the `suite` field every witness refers to. */
export const SUITES = {
  authority: ['-p', 'archava-host', '--test', 'authority'],
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
  'unauthenticated command refused': [['authority', 'an_unauthenticated_command_is_refused']],
  'forged tenant/role/approval refused': [
    ['authority', 'a_forged_role_and_approval_in_the_body_change_nothing'],
  ],
  'revoked grant refused on execute': [['authority', 'a_revoked_grant_is_refused_on_execute']],
  'revoked grant refused on status': [['authority', 'a_revoked_grant_is_refused_on_status']],
  'revoked grant refused on replay': [['authority', 'a_revoked_grant_is_refused_on_replay']],
  'revoked grant refused on recovery': [
    ['authority', 'a_revoked_grant_is_refused_on_recovery_after_restart'],
  ],
  // RN01: recovery re-answers the policy gate under CURRENT grants, so an
  // active membership whose role/scope no longer reaches the recorded action
  // gets a refusal — not the payload it used to be able to produce.
  'downgraded grant refused on recovery': [
    ['authority', 'a_downgraded_grant_cannot_recover_a_receipt_it_may_no_longer_act_on'],
  ],
  // RN02: an idempotency key returns its OWN command's receipt, sequentially
  // and under concurrent same-key/different-input collision. The key
  // identifies the slot; the fingerprint identifies the command.
  'reused key for another command conflicts': [
    ['authority', 'a_key_reused_for_a_different_command_conflicts_on_execute_and_in_race'],
  ],
  'concurrent same-key different-input conflicts': [
    ['authority', 'concurrent_same_key_different_input_yields_conflict_not_the_other_receipt'],
  ],
  'authorized operation succeeds': [
    ['authority', 'an_authorized_operation_succeeds_with_a_receipt'],
  ],
  'cross-tenant read refused at DB': [
    ['authority', 'a_receipt_of_another_tenant_is_invisible_at_the_db_boundary'],
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
 * Run the authority suite, grade every required check against its named
 * witnesses, write `dist/rust-authority-proof.json`, and exit non-zero on any
 * failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runAuthority() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-AUTHORITY-PG']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-AUTHORITY-PG is not an evidence proof in the frozen manifest')

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

  process.stdout.write('  $ cargo test (authority suite, named witnesses)\n')
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
    if (run.exitCode !== 0) {
      // A named-witness line saying "FAILED" is not enough to act on — whoever
      // reads the gate log needs the assertion that tripped. The harness
      // formats each failed test as a `---- <name> stdout ----` block with the
      // panic inside; print those blocks, bounded, so the log carries the real
      // failing point for each failed test and no more.
      const blocks = []
      let current = null
      for (const line of run.stdout.split('\n')) {
        if (line.startsWith('---- ') && line.endsWith(' stdout ----')) {
          if (current) blocks.push(current)
          current = [line]
        } else if (current && line.startsWith('failures:')) {
          blocks.push(current)
          current = null
        } else if (current && current.length < 60) {
          current.push(line)
        }
      }
      if (current) blocks.push(current)
      for (const block of blocks) {
        process.stdout.write(block.map((entry) => `       ${entry}`).join('\n') + '\n')
      }
      if (blocks.length === 0)
        process.stdout.write('       (no per-test failure blocks in the harness output)\n')
      problems.push(
        `suite ${key} exited ${run.exitCode} — a failing authority suite fails every check it backs`,
      )
    }
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
  // `namedRegressions` is the second frozen tally. RN01 and RN02 are the two
  // regression notes this suite exists to answer — a downgrade granted past
  // its authority record (RN01), and an idempotency key answered twice —
  // reused for another input, or reused concurrently (RN02). The strings here
  // are the check names the run must have graded, so the tally is derived
  // from `checks` below rather than written blind: a regression name that is
  // not a graded check is a wiring drift the producer refuses, and a graded
  // check that failed has already pushed a problem, so a green details blob
  // whose named regression did not run cannot be produced here.
  const rn01 = 'downgraded grant refused on recovery'
  const rn02 = [
    'reused key for another command conflicts',
    'concurrent same-key different-input conflicts',
  ]
  for (const name of [rn01, ...rn02]) {
    if (!(name in checks)) problems.push(`named regression check ${name} was not graded`)
  }
  const namedRegressions = { RN01: rn01, RN02: rn02 }
  const details = { suites, checks, namedRegressions, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details` and binds its hash as
  // a referenced producing file, and a failing run must record its own truth
  // rather than leave the previous green file to be read as still current.
  const proofPath = join(root, 'dist/rust-authority-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-authority-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runAuthority()
  if (outcome.ok) {
    process.stdout.write('PASS pg authority suite — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL pg authority suite — see the witness lines above\n')
    process.exitCode = 1
  }
}
