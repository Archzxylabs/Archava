/* global process, URL */
/**
 * P-READINESS-PG producer: the named-witness runner for the readiness suites.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The workspace step already runs both suites this script targets — but it
 * proves only "every test passed", and `writeStepProofs` binds a required check
 * to a step's exit. A step that passes without the proving test is exactly the
 * hole RF01 closed: delete `missing_forced_policy_readiness_check_blocks_construction`
 * from the host suite and the workspace step stays green while the manifest's
 * "startup readiness blocks on missing forced policy" has no producer left.
 * This script parses the test harness output for each witness BY NAME and fails
 * when a named witness did not run or did not pass, so the check and its proof
 * are the same object.
 *
 * ## What each witness proves
 *
 * Two suites, one fixture each:
 *
 * - `archava-host --test readiness` — `AppState::try_new`, the only constructor
 *   reachable outside the host crate, refuses to build serving state on a
 *   fixture whose forced policies, migration checksum or runtime role are
 *   wrong, and serves `/healthz` on a ready one. No state means no router, so
 *   "host refuses to serve before readiness passes" is structural, not asserted.
 * - `archava-adapters --test tenant_context` — the same three refusals at the
 *   `assert_startup_readiness` layer with their named error variants, plus the
 *   pooled tenant setting that must not survive commit, rollback, or the next
 *   transaction on one connection.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-READINESS-PG` in the frozen manifest
 *   must have a witness mapping here, and every mapping must name a manifest
 *   check: either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in its suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-readiness-proof.json` is written on every run, pass or fail;
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

/** The suites, keyed by the `suite` field every witness refers to. */
export const SUITES = {
  host: ['-p', 'archava-host', '--test', 'readiness'],
  adapters: ['-p', 'archava-adapters', '--test', 'tenant_context'],
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
  'startup readiness blocks on missing forced policy': [
    ['host', 'missing_forced_policy_readiness_check_blocks_construction'],
    ['adapters', 'an_unforced_policy_is_refused'],
  ],
  'startup readiness blocks on modified migration checksum': [
    ['host', 'modified_checksum_readiness_check_blocks_construction'],
    ['adapters', 'a_modified_migration_checksum_is_refused'],
  ],
  'startup readiness blocks on owner runtime': [
    ['host', 'owner_runtime_readiness_check_blocks_construction'],
    ['adapters', 'owner_and_superuser_runtimes_are_refused'],
  ],
  'host refuses to serve before readiness passes': [
    // Positive side: a ready fixture constructs state and the router answers.
    ['host', 'ready_fixture_constructs_state_and_router_serves'],
    // Negative side: the refusal is the readiness gate by name, not an
    // incidental failure such as a typo'd table that would also refuse.
    ['host', 'readiness_refusal_is_the_named_gate'],
    ['host', 'missing_forced_policy_readiness_check_blocks_construction'],
    ['host', 'modified_checksum_readiness_check_blocks_construction'],
    ['host', 'owner_runtime_readiness_check_blocks_construction'],
    ['adapters', 'startup_readiness_passes_on_a_configured_ledger'],
  ],
  'pooled tenant context does not leak across begin': [
    ['adapters', 'tenant_context_does_not_leak_after_commit'],
    ['adapters', 'tenant_context_does_not_leak_after_rollback'],
    ['adapters', 'the_setting_is_gone_before_the_next_transaction_on_one_connection'],
  ],
}

/**
 * Run one suite, capturing output even when it fails.
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
 * Run both suites, grade every required check against its named witnesses,
 * write `dist/rust-readiness-proof.json`, and exit non-zero on any failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runReadiness() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-READINESS-PG']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-READINESS-PG is not an evidence proof in the frozen manifest')

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

  process.stdout.write('  $ cargo test (readiness suites, named witnesses)\n')
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
        `suite ${key} exited ${run.exitCode} — a failing readiness suite fails every check it backs`,
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
  const details = { suites, checks, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details` and binds its hash as
  // a referenced producing file, and a failing run must record its own truth
  // rather than leave the previous green file to be read as still current.
  const proofPath = join(root, 'dist/rust-readiness-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-readiness-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runReadiness()
  if (outcome.ok) {
    process.stdout.write('PASS pg readiness suites — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL pg readiness suites — see the witness lines above\n')
    process.exitCode = 1
  }
}
