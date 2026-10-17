/* global process, URL */
/**
 * P-PLANNING-CONFORMANCE producer: the named-witness runner for the assembly
 * planning suite plus the cold-consumer planning record.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * Two payloads, one proof:
 *
 * 1. **The cargo witnesses.** The workspace step already runs this suite, but
 *    it proves only "every test passed", and the manifest's first two checks —
 *    the conformance cases and the measurement coverage — are each about
 *    specific named tests, so this script parses the harness output for each
 *    witness BY NAME (the RF01 shape of `tools/rust/pg-readiness.mjs`).
 * 2. **The cold-consumer planning record.** The third check names the
 *    cold-consumer planning proof, which is `consumer A`'s record in
 *    `dist/rust-consumer-proof.json`: the shipped `archava-assembly` tarball
 *    materialized inside a scratch consumer, the two planning witness tests it
 *    ran, and the cold/offline/no-product-path flags. That record exists only
 *    after the `two cold consumers` gate step — which is why this step sits
 *    LAST in the gate, right after it — and this script grades against its
 *    own contents, so a record from a run that is not this invocation cannot
 *    be what a green claim rides on.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-PLANNING-CONFORMANCE` in the frozen
 *   manifest must map to a grading branch here (two cargo-checks + one
 *   consumer-record check), and every mapping name must be a manifest check.
 * - `dist/rust-planning-proof.json` is written on every run, pass or fail;
 *   `verify.mjs` binds it as `details`.
 */

import { execFileSync } from 'node:child_process'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from './coverage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = join(root, 'rust')

/** The cargo suites, keyed by the `suite` field the cargo witnesses refer to. */
export const SUITES = {
  assembly: ['-p', 'archava-assembly', '--test', 'assembly'],
}

/**
 * The consumer-A tests that prove the third check, and the record conditions
 * they must ride on.
 *
 * The names are the exact identifiers the consumer's harness prints; the
 * conditions are fields consumer A always records about its own run.
 */
export const PLANNING_CONSUMER_WITNESSES = {
  tests: [
    'plans_an_assembly_from_the_shipped_crate',
    'a_cold_catalogue_mismatch_refuses_with_the_baselines_message',
  ],
  conditions: [
    'coldOffline',
    'productTreeReached',
    'vendoredSourceOnly',
  ],
}

/**
 * Manifest required check → the witness tests that produce it, for the two
 * cargo-graded checks.
 */
export const WITNESSES = {
  'conformance cases pass': [
    // The baseline's own cases, in the port: the base plan, the minimal plan,
    // the serialized wire shape, the messages, and the refusals a caller's
    // first line of defense must produce.
    ['assembly', 'plans_a_minimal_selection'],
    ['assembly', 'plans_base_modules'],
    ['assembly', 'plan_serializes_with_baseline_field_names'],
    ['assembly', 'baseline_messages_hold'],
    ['assembly', 'one_missing_package_refuses_the_whole_plan'],
    ['assembly', 'refuses_unknown_profile'],
    ['assembly', 'refuses_unknown_request_version'],
    // Installation is the catalogue, not the selection — the rule a caller
    // rendering the plan most often gets wrong.
    ['assembly', 'installation_is_the_catalogue_not_the_selection'],
  ],
  'measurement coverage recorded': [
    // The coverage CLAIMS, and the recordings behind them: every module plans
    // against the full catalogue; every contract prerequisite pair has a
    // witness; the module loop reports by contract order; and the legacy name
    // list with its two literal strings is carried verbatim.
    ['assembly', 'all_modules_plan_against_full_catalogue'],
    ['assembly', 'schema_refuses_every_contract_prerequisite_pair'],
    ['assembly', 'the_module_loop_reports_the_earliest_contract_not_the_earliest_name'],
    ['assembly', 'legacy_names_and_literals_are_verbatim'],
  ],
}

/**
 * Run the assembly suite, grade the two cargo checks by their named witnesses,
 * grade the cold-consumer planning check against the consumer proof record,
 * write `dist/rust-planning-proof.json`, and exit non-zero on any failure.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites, consumer, problems }}
 */
export function runPlanningConformance() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-PLANNING-CONFORMANCE']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-PLANNING-CONFORMANCE is not an evidence proof in the frozen manifest')

  const required = spec.requiredChecks ?? []
  const problems = []

  const cargoChecks = new Set(Object.keys(WITNESSES))
  for (const check of required) {
    if (cargoChecks.has(check)) continue
    if (check !== 'cold-consumer planning proof passes')
      problems.push(`no grading branch wired for required check ${JSON.stringify(check)}`)
  }
  for (const check of Object.keys(WITNESSES))
    if (!required.includes(check))
      problems.push(
        `witness map names check ${JSON.stringify(check)} the manifest does not require`,
      )

  process.stdout.write('  $ cargo test (assembly planning conformance, named witnesses)\n')
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
  // Graded once; the same object carries the check's grade and the details.
  const consumerGrade =
    required.includes('cold-consumer planning proof passes') ? gradeConsumerRecord() : null
  for (const check of required) {
    if (check === 'cold-consumer planning proof passes') {
      const { witnesses, problems: consumerProblems } = consumerGrade
      checks[check] = { consumerRecord: witnesses, passed: consumerProblems.length === 0 }
      for (const problem of consumerProblems) problems.push(problem)
      process.stdout.write(
        `     ${checks[check].passed ? '✓' : '✗'} ${check} (cold-consumer planning record)\n`,
      )
      continue
    }
    const witnesses = WITNESSES[check]
    if (!witnesses) continue
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
  const details = {
    suites,
    checks,
    consumer: consumerGrade?.witnesses ?? null,
    problems,
    witnessTotal,
    allPassed,
  }

  const proofPath = join(root, 'dist/rust-planning-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write('  witness proof → dist/rust-planning-proof.json\n')

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

/**
 * Grade the cold-consumer planning proof from `dist/rust-consumer-proof.json`.
 *
 * The step ordering is the freshness guarantee: this producer is wired in the
 * gate immediately after `two cold consumers`, so the record on disk is the
 * one the same invocation produced. Grading reads the record's own facts —
 * the named planning tests consumer A ran, the assembly tarball it consumed,
 * and its cold/offline flags.
 *
 * @returns {{ witnesses: object, problems: string[] }}
 */
export function gradeConsumerRecord() {
  const problems = []
  let record
  try {
    record = JSON.parse(readFileSync(join(root, 'dist/rust-consumer-proof.json'), 'utf8'))
  } catch (error) {
    problems.push(
      `the cold-consumer planning record is unreadable — the ${JSON.stringify('two cold consumers')} ` +
        `step must run before this one in the same invocation (${error.message})`,
    )
    return { witnesses: { readable: false }, problems }
  }

  const consumerA = (record.consumers ?? []).find((entry) => entry.consumer === 'crate')
  if (!consumerA) {
    problems.push("the consumer proof has no 'crate' consumer record")
    return { witnesses: { readable: true }, problems }
  }

  const named = new Set(consumerA.namedTests ?? [])
  const tests = PLANNING_CONSUMER_WITNESSES.tests.map((test) => ({
    test,
    status: named.has(test) ? 'ok' : 'not-run',
  }))
  for (const { test, status } of tests)
    if (status !== 'ok')
      problems.push(
        `the cold consumer did not run the planning witness ${JSON.stringify(test)} — ` +
          're-run `node tools/library/verify-rust-consumers.mjs`, not a stale record',
      )

  const assemblyTarball = (consumerA.artifacts ?? []).find(
    (artifact) => artifact.crate === 'archava-assembly',
  )
  if (!assemblyTarball)
    problems.push("the cold consumer's artifacts do not include the shipped archava-assembly tarball")

  const unmetConditions = PLANNING_CONSUMER_WITNESSES.conditions.filter((condition) => {
    if (condition === 'productTreeReached') return consumerA[condition] !== false
    return consumerA[condition] !== true
  })
  for (const condition of unmetConditions)
    problems.push(
      `consumer A's planning record does not show ${JSON.stringify(condition)} as required`,
    )

  return {
    witnesses: {
      runAtStep: 'two cold consumers',
      tests,
      assemblyTarball: assemblyTarball ?? null,
      conditions: PLANNING_CONSUMER_WITNESSES.conditions.reduce((acc, condition) => {
        acc[condition] = condition === 'productTreeReached' ? consumerA[condition] : consumerA[condition] === true
        return acc
      }, {}),
    },
    problems,
  }
}

/**
 * Run one suite, capturing output even when it fails.
 *
 * Duplication with `pg-readiness.mjs` is deliberate: the producers are
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
  const outcome = runPlanningConformance()
  if (outcome.ok) {
    process.stdout.write('PASS planning conformance — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL planning conformance — see the witness lines above\n')
    process.exitCode = 1
  }
}
