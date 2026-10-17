/* global process, URL */
/**
 * P-CAPACITY-CONFORMANCE producer: the named-witness runner for the capacity
 * suites.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The workspace step already runs this suite — but it proves only "every test
 * passed", and `writeStepProofs` binds a required check to a step's exit. The
 * six required checks of P-CAPACITY-CONFORMANCE are each about a SPECIFIC
 * behavior (refusals, bounds, the iteration cap, retained parity), so this
 * script parses the test harness output for each witness BY NAME and fails
 * when a named witness did not run or did not pass — the same RF01 shape the
 * two pg producers use (`tools/rust/pg-readiness.mjs`).
 *
 * ## What each witness proves
 *
 * One suite, no PostgreSQL: `archava-capacity --test capacity` is the
 * differential suite over `tests/reference.json`, plus the boundary and
 * RA07 witnesses. The witnesses per check are deliberately duplicated here
 * rather than derived from Rust, per the pg producers' rule: a mapping derived
 * from the code it checks can only ever agree with itself.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-CAPACITY-CONFORMANCE` in the frozen
 *   manifest must have a witness mapping here, and every mapping must name a
 *   manifest check: either direction drifting alone is an error, not a silent
 *   gap (the runtime check below, and `verify.test.mjs` against the frozen
 *   manifest).
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in the suite's output. A suite that exits non-zero additionally
 *   fails the run even when this script's per-check parse looks green.
 * - `dist/rust-capacity-proof.json` is written on every run, pass or fail;
 *   `verify.mjs` binds it as `details`. Evidence itself is written by
 *   `writeStepProofs` from the step exit, so a red run replaces any stale
 *   green claim instead of leaving it readable.
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
  capacity: ['-p', 'archava-capacity', '--test', 'capacity'],
}

/**
 * Manifest required check → the witness tests that produce it.
 *
 * Each witness is `[suite, testName]` where `testName` is the exact
 * identifier the harness prints (`test <name> ... ok`).
 */
export const WITNESSES = {
  'non-finite input rejected (invalid_request)': [
    ['capacity', 'non_finite_input_is_refused_at_entry'],
  ],
  'calendar overflow returns controlled error (no hang)': [
    ['capacity', 'extreme_calendar_inputs_are_refused_with_invalid_request_not_a_hang'],
  ],
  'loop bounds advance (iteration cap honored)': [
    // The cap value itself is a pinned baseline literal, not a chosen one.
    ['capacity', 'the_two_literals_are_the_baselines_literals'],
    // The two ends of the bound: the permanently unsatisfiable search ends in
    // the recorded refusal, and the big-but-satisfiable job still advances to
    // its schedule. The recorded rows underneath both ends live in the
    // differential suite.
    ['capacity', 'the_capped_loop_refuses_the_unsatisfiable_and_advances_to_schedule'],
    ['capacity', 'reproduces_every_recorded_baseline_result'],
  ],
  'horizon and allocation bounds enforced': [
    ['capacity', 'a_span_beyond_the_horizon_is_refused_before_the_walk'],
    ['capacity', 'a_pool_that_cannot_burn_is_refused_before_the_search'],
  ],
  'extreme overflow returns controlled error': [
    // The extreme `Instant` values that once hung the walk refuse up front as
    // a controlled `invalid_request`; a beyond-horizon span refuses the same
    // way before any step. Together they are the RA07 overflow family.
    ['capacity', 'extreme_calendar_inputs_are_refused_with_invalid_request_not_a_hang'],
    ['capacity', 'a_span_beyond_the_horizon_is_refused_before_the_walk'],
  ],
  'corrected parity retained': [
    // The scoreboard story of this proof: every recorded baseline row, the
    // fixture set that carries them, the two literals, the day step, the
    // comparable refusal, the exactly-one wall-clock row and the plumbing
    // conversion — all still hold.
    ['capacity', 'reproduces_every_recorded_baseline_result'],
    ['capacity', 'the_fixture_set_is_intact'],
    ['capacity', 'the_two_literals_are_the_baselines_literals'],
    ['capacity', 'the_day_step_is_the_baselines_day_step'],
    ['capacity', 'the_refusal_is_comparable_and_its_message_is_its_code'],
    ['capacity', 'the_wall_clock_case_is_recorded_and_not_faked'],
    ['capacity', 'the_reference_rows_deserialize_into_the_ports_inputs'],
  ],
}

/**
 * Run one suite, capturing output even when it fails, then grade every
 * required check against its named witnesses.
 *
 * Mirrors `pg-readiness.mjs` one for one; see that file for the reasoning this
 * shape exists for. Differences: one suite instead of two, and the details
 * file is `dist/rust-capacity-proof.json`.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runCapacityConformance() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-CAPACITY-CONFORMANCE']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-CAPACITY-CONFORMANCE is not an evidence proof in the frozen manifest')

  const problems = []

  // Either direction of drift is an error.
  const required = spec.requiredChecks ?? []
  for (const check of required)
    if (!WITNESSES[check])
      problems.push(`no witness wired for required check ${JSON.stringify(check)}`)
  for (const check of Object.keys(WITNESSES))
    if (!required.includes(check))
      problems.push(
        `witness map names check ${JSON.stringify(check)} the manifest does not require`,
      )

  process.stdout.write('  $ cargo test (capacity conformance, named witnesses)\n')
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
        `suite ${key} exited ${run.exitCode} — a failing conformance suite fails every check it backs`,
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
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)
  const details = { suites, checks, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details` and binds its hash as
  // a referenced producing file, and a failing run must record its own truth
  // rather than leave the previous green file to be read as still current.
  const proofPath = join(root, 'dist/rust-capacity-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write('  witness proof → dist/rust-capacity-proof.json\n')

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

/**
 * Run one suite, capturing output even when it fails.
 *
 * Duplication with `pg-readiness.mjs` is deliberate: the two producers are
 * reviewed as units, and sharing a helper would couple their grading edits.
 *
 * @param {string[]} cargoArgs arguments after `cargo`
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

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runCapacityConformance()
  if (outcome.ok) {
    process.stdout.write('PASS capacity conformance — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL capacity conformance — see the witness lines above\n')
    process.exitCode = 1
  }
}
