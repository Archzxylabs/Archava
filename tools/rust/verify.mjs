/* global process, URL */
/**
 * The fast Rust regression gate: `pnpm rust:verify`.
 *
 * ## The gap this closes
 *
 * Four Rust proofs existed and none of them was guarded: p1 pricing parity, p2
 * rates parity, the compiled-consumer benchmark, the two cold consumers. Every one
 * was a by-hand invocation, so every one only ran when someone happened to
 * remember. §1.1 item 1 of the execution task names exactly that — *"menambahkan
 * scripts yang tetap hanya diingat untuk dijalankan manual belum cukup"* — and the
 * larger half of the gap is elsewhere: nothing in this repository ran
 * `cargo fmt`, `cargo clippy` or `cargo test` at all. Three checks with no
 * invocation, two proofs with one each, and one test file
 * (`tools/rust/parity/compare.test.mjs`) that was written to be run by
 * `node --test` and then left un-run. All six are steps here.
 *
 * ## Two gates, not one, and the split is the mandate's
 *
 * `pnpm rust:verify` (this file) is the FAST gate: format, clippy, tests, the
 * compare mechanism's own tests, both parity harnesses, both cold consumers. It is
 * wired into `tools/scripts/verify.mjs` and CI, so it runs on every change.
 *
 * `pnpm rust:verify-release` is the FULL candidate gate: everything here plus the
 * benchmark, the 46-command security chain and the documented end-to-end /
 * upgrade / restore proofs. It runs against a candidate identity rather than a
 * commit, and it is slow on purpose.
 *
 * The split is not convenience. A gate that runs the benchmark on every commit
 * measures a moving tree; a gate that rebuilds an archived cold-prior commit on
 * every commit spends minutes answering a question that only matters at release
 * time.
 *
 * ## How a step is judged
 *
 * The real child exit code is propagated, including `null` — a child killed by a
 * signal is not a failed test and saying so beats reporting -1 as though the suite
 * had failed. Nothing is swallowed, because a gate that reported a green step next
 * to a failed one would be worse than no gate.
 *
 * The frozen expectations below are the other half. A parity harness already
 * decides for itself what fails; the harnesses' own choices leave one hole, and
 * the hole is where the open contract difference lives.
 *
 * ## Why this file can be imported
 *
 * `compare.test.mjs` and its siblings run under `node --test`, so the gate's own
 * behaviour has to be testable by `node --test` too. That means the orchestration
 * cannot live at module top level where an import would start a cargo build, so it
 * sits behind `import.meta.main` and `run()` below. The test drives `run()` with
 * synthetic steps; the gate's own contract is then asserted by the same runner that
 * asserts the harness's, which is the only way a gate about exit codes gets a
 * meaningful test rather than a smoke test.
 */

import { execFileSync } from 'node:child_process'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { writeEvidence } from './evidence.mjs'
import { loadManifest } from './coverage.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = join(root, 'rust')

/** Repo-relative paths in the log; an absolute path to the addon is noise. */
const relative = (path) => path.replace(`${root}/`, '')

/**
 * The exact tally both parity harnesses must report, frozen as expectations.
 *
 * These are not aspirations, and they are not derived from the code — a number
 * derived from the code is the code checking that it equals itself. They are the
 * measured state of the two slices, recorded so that a change is a visible event
 * rather than a number that quietly moves. `see` names the disposition, because a
 * frozen number is only honest while the thing it freezes stays written down.
 *
 * Every key is enforced in both directions. A count falling is not automatically
 * good news: if the port genuinely started matching, D01 needs its disposition
 * rewritten in the same review; if the classifier stopped classifying, the run
 * shows a smaller number for the wrong reason. Neither may pass silently.
 */
export const EXPECTED_PARITY = {
  'p1-parity': {
    fixtures: 350,
    match: 338,
    'error-class-only': 12,
    'error-message-only': 0,
    diverge: 0,
    passed: 338,
    failed: 12,
    see: 'KNOWN_DIFFERENCES.md D01',
  },
  'p2-parity': {
    fixtures: 753,
    match: 748,
    'corrected-expectation': 5,
    'error-class-only': 0,
    'error-message-only': 0,
    diverge: 0,
    passed: 753,
    failed: 0,
    see: 'KNOWN_DIFFERENCES.md D02',
  },
}

/**
 * Compare one harness's evidence file against its frozen expectation.
 *
 * Why this exists at all. Both harnesses set `process.exitCode = 1` when
 * `diverge` or `error-message-only` is non-zero, which means p1 is green at 12
 * error-class-only and *would still be green at 100*. That is a defensible choice
 * for the harness — error-class-only is the recorded contract difference, not a
 * defect — but it leaves the one number the gate most needs to watch unenforced.
 * The gate owns it here, deliberately, in both directions.
 *
 * @param {string} name evidence file stem, a key of {@link EXPECTED_PARITY}
 * @param {Record<string, number>} actual the parsed evidence file
 */
export function assertParityExpectations(name, actual) {
  const expected = EXPECTED_PARITY[name]
  if (expected === undefined) throw new Error(`no frozen expectation recorded for ${name}`)

  const problems = []
  for (const [key, value] of Object.entries(expected)) {
    if (key === 'see') continue
    const seen = actual[key]
    if (seen !== value) problems.push(`${key}: expected ${value}, got ${seen}`)
  }
  if (problems.length > 0) {
    throw new Error(
      `${name} does not match its frozen expectation (${expected.see})\n` +
        problems.map((line) => `    ${line}`).join('\n') +
        '\n  Update the expectation and its disposition together, in one review.\n' +
        '  A smaller count is not a smaller finding: if the port started matching,\n' +
        '  the disposition must be rewritten to say so.',
    )
  }
  process.stdout.write(`     ✓ ${name} frozen expectation (${expected.see})\n`)
}

/**
 * Read one parity evidence file after its run.
 *
 * Read from disk rather than parsed out of stdout, because the file is what the
 * gate leaves behind as evidence and stdout is not.
 *
 * @param {string} name evidence file stem
 */
export function parityEvidence(name) {
  const path = join(root, `tools/rust/parity/evidence/${name}.json`)
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch (error) {
    throw new Error(`parity evidence unreadable at ${relative(path)}: ${error.message}`)
  }
}

/**
 * Run one step, stream its output, propagate its exit code.
 *
 * `stdio: 'inherit'` is deliberate: a gate that buffers its steps can only report
 * a summary, and the summary of `cargo clippy` is worthless.
 *
 * @param {string} name what the log calls this step
 * @param {string[]} command command and arguments, split — no shell, so no
 * quoting rules and no injected arguments
 * @param {string} cwd the directory it runs in
 */
export function step(name, command, cwd = root) {
  process.stdout.write(
    `\n  $ ${command.join(' ')}${cwd === root ? '' : `   (in ${relative(cwd)})`}\n`,
  )
  const started = Date.now()
  try {
    execFileSync(command[0], command.slice(1), { cwd, stdio: 'inherit' })
  } catch (error) {
    // A signal reports as a null status. That is not a test failure, and
    // pretending it is `exit -1` would send whoever reads the log looking for a
    // failing assertion that never existed.
    const status = error.status ?? null
    throw new Error(
      `${name} failed${status === null ? ' (killed by signal)' : ` with exit ${status}`}\n` +
        `    ran for ${((Date.now() - started) / 1000).toFixed(1)}s`,
    )
  }
  const seconds = ((Date.now() - started) / 1000).toFixed(1)
  process.stdout.write(`     ✓ ${name} (${seconds}s)\n`)
  return { name, seconds: Number(seconds) }
}

/**
 * The ordered steps of the fast gate.
 *
 * The order is load-bearing twice over. The artifacts come first because p1
 * `require`s the addon by name and the cold-consumer proof hashes the same file,
 * so building after them would leave two proofs reporting two different artifact
 * hashes for one build. The consumers come last because they are the slowest step
 * and they consume what the first step produced; if anything above them failed,
 * the staleness question is moot.
 *
 * `cargo fmt --all` and `cargo clippy --all-targets` are not decoration.
 * `rust/Cargo.toml` is a virtual workspace manifest with no `[package]`, so the
 * bare `cargo fmt --check` prints "Failed to find targets" and exits 1 without
 * reading a single file — the same exit code as a formatting failure and the
 * opposite meaning, which is how a false "format passes" row got into a gate
 * table before it was caught.
 *
 * @returns {Array<[string, () => Record<string, unknown>]>} name and runner
 */
export function gateSteps() {
  return [
    [
      'build release artifacts',
      () => step('rust:build-artifacts', ['node', 'tools/rust/build-artifacts.mjs']),
    ],
    [
      'cargo fmt --all --check',
      () => step('cargo fmt --all --check', ['cargo', 'fmt', '--all', '--check'], workspace),
    ],
    [
      'cargo clippy --all-targets -- -D warnings',
      () =>
        step(
          'cargo clippy --all-targets -- -D warnings',
          ['cargo', 'clippy', '--all-targets', '--locked', '--offline', '--', '-D', 'warnings'],
          workspace,
        ),
    ],
    // The browser/server split, both directions, every run. It was previously
    // a by-hand row in the EXECUTION_STATE gate table — the same unguarded
    // invocation the other six proofs were. The script owns its own evidence
    // (three checks come from three subprocess results, not one exit code), so
    // this step's only job here is to run it and propagate its exit.
    [
      'wasm portable boundary',
      () => step('wasm portable boundary', ['node', 'tools/rust/wasm-boundary.mjs']),
    ],
    [
      'cargo test --workspace',
      () =>
        step(
          'cargo test --workspace',
          ['cargo', 'test', '--workspace', '--locked', '--offline'],
          workspace,
        ),
    ],
    // The workspace step above runs the readiness suites but only proves "all
    // tests passed" — which a deleted proving test would still satisfy. This
    // step re-runs the two suites and grades P-READINESS-PG's five required
    // checks against witness tests it looks up BY NAME in the harness output,
    // so the manifest check and the test that produces it cannot drift apart.
    [
      'pg readiness suites',
      () => step('pg readiness suites', ['node', 'tools/rust/pg-readiness.mjs']),
    ],
    // Same pattern for P-AUTHORITY-PG: the workspace step above runs the
    // authority suite but only proves "all tests passed". This step re-runs it
    // and grades the eight required checks against witness tests it looks up
    // BY NAME, so a deleted proving test is a visible gap rather than a green
    // step with no producer.
    [
      'pg authority suite',
      () => step('pg authority suite', ['node', 'tools/rust/pg-authority.mjs']),
    ],
    // Same pattern for P-R03-DOMAIN: the R03 CRM domain lane spans three
    // suites (the command lane repository, the conversation route over HTTP,
    // and the assistant crate's injected-host flow tests). The workspace step
    // runs all three but proves only "all tests passed"; this step re-runs
    // them and grades the three required checks against witness tests it
    // looks up BY NAME, so a deleted proving test is a visible gap. It sits
    // after the authority step because both rebuild disposable PostgreSQL
    // fixtures one suite at a time.
    [
      'R03 domain suite',
      () => step('R03 domain suite', ['node', 'tools/rust/r03-domain.mjs']),
    ],
    // P-CAPACITY-CONFORMANCE: same RF01 shape, one Rust suite, no PostgreSQL.
    // The six required checks grade named witnesses in the differential
    // suite — refusals, both bound families, the iteration cap, retained
    // parity — so a deleted proving test is a visible gap.
    [
      'capacity conformance suite',
      () => step('capacity conformance suite', ['node', 'tools/rust/capacity-conformance.mjs']),
    ],
    // The tests written for `node --test` and then left unwired: a run nobody
    // invokes proves nothing. The files are named explicitly, not `node --test
    // tools/rust` — that form also sweeps up the three vitest `*.test.ts` files in
    // `tools/rust/benchmark/test/`, which need vitest's globals and fail with
    // `Cannot read properties of undefined (reading 'config')`. Vitest owns those
    // three through the `{packages,apps,tools}/**/test/**/*.test.ts` include in
    // `vitest.config.ts` (37 tests, green), so naming the `.mjs` files here covers
    // exactly the gap this gate is closing. The third one is the release gate's
    // own tests: it is the file that carries the `verdict`, and a gate whose
    // output decides an exit code with no tests of its own is the same omission.
    [
      'rust unit tests (node --test)',
      () =>
        step('rust unit tests (node --test)', [
          'node',
          '--test',
          'tools/rust/parity/compare.test.mjs',
          'tools/rust/test/verify.test.mjs',
          'tools/rust/test/verify-release.test.mjs',
          // ADR-R10's derivation: the consumer-closure fetch's `--locked`
          // pinning is only as good as the lock derivation feeding it, and a
          // derivation that keeps the wrong blocks still exits 0. Pure string
          // checks over the committed consumer lock — cold-safe, no cargo.
          'tools/rust/test/bootstrap.test.mjs',
          // D01's disposition, checked rather than merely written down. The
          // counts in `EXPECTED_PARITY` say there are 12 `error-class-only`
          // differences; this file is the only thing that says what they are —
          // that all 12 are engine refusals whose messages match `engine.ts`
          // character for character, that none is a deserialization error, and
          // that the boundary cannot carry `error.name`. It runs after the
          // addon build above, which it needs, and before the parity harness
          // below, whose evidence it pins the meaning of.
          'tools/rust/test/error-contract.test.mjs',
        ]),
    ],
    // Neither harness reads RUST_PILOT_RUNTIME to choose an engine —
    // `run-parity.mjs:284` records that both engines always run — so passing it
    // here would imply a selector that does not exist.
    [
      'p1 pricing parity',
      () => {
        // The step result is returned, not discarded: `run()` prints the seconds
        // each step took, and a step that reports no duration prints `undefined`
        // in the summary — which reads like a step the gate could not measure.
        const ran = step('p1 pricing parity', ['node', 'tools/rust/parity/run-parity.mjs'])
        assertParityExpectations('p1-parity', parityEvidence('p1-parity'))
        return ran
      },
    ],
    [
      'p2 rates parity',
      () => {
        const ran = step('p2 rates parity', ['node', 'tools/rust/parity/run-parity-rates.mjs'])
        assertParityExpectations('p2-parity', parityEvidence('p2-parity'))
        return ran
      },
    ],
    [
      'two cold consumers',
      () => step('two cold consumers', ['node', 'tools/library/verify-rust-consumers.mjs']),
    ],
    // P-PLANNING-CONFORMANCE, LAST on purpose: its third check grades the
    // cold-consumer planning record that the step above writes
    // (`dist/rust-consumer-proof.json`), so the record it reads is the one
    // this invocation produced, not a stale one. The two cargo checks ride
    // the assembly suite with named witnesses like the capacity producer.
    [
      'planning conformance suite',
      () => step('planning conformance suite', ['node', 'tools/rust/planning-conformance.mjs']),
    ],
  ]
}

/**
 * Write the fast gate's proof evidence from what actually ran (RF01).
 *
 * The mapping below decides which step backs which required check of which
 * proof. A check whose step did not pass — or never ran, because an earlier
 * step aborted the gate — is written as `fail: …`, never omitted: the frozen
 * validator treats a missing required check exactly like a failed one, and the
 * producer should say so in the file rather than leave the gap for it.
 *
 * Evidence is written even when the gate fails. A failing run that recorded
 * nothing would leave the previous green evidence on disk, and a validator
 * comparing that stale pass against the current tree would be reading a claim
 * about a run that no longer describes this tree.
 *
 * @param {{ ok: boolean, passed: Array<{name: string}>, failure: string | null }} outcome
 *   the result of {@link run}
 * @param {Array<object>} mapping proof entries: `{ proof, checks?, step?, details? }`
 *   where `checks` maps a required check name to `{ step }` (or `{ step: null }`
 *   for the whole-gate outcome), `step` alone maps every required check to one
 *   step, and `details` is an object or a function returning one
 * @param {string} producer the command that produced these results
 * @param {{ root?: string }} [options] test seam: a fixture root to write
 *   against instead of this repository (the frozen manifest must exist under it)
 */
export function writeStepProofs(outcome, mapping, producer, options = {}) {
  const target = options.root ?? root
  const manifest = loadManifest(target)
  for (const entry of mapping) {
    const spec = manifest.proofs[entry.proof]
    if (!spec) {
      process.stdout.write(
        `  ! evidence ${entry.proof} NOT written — not a proof in the frozen manifest\n`,
      )
      continue
    }
    const bindings =
      entry.checks ??
      Object.fromEntries(spec.requiredChecks.map((check) => [check, { step: entry.step }]))
    const checks = {}
    for (const check of spec.requiredChecks) {
      const binding = bindings[check]
      if (!binding) {
        checks[check] = 'fail: no producer wired for this required check'
        continue
      }
      const passed =
        binding.step === null ? outcome.ok : outcome.passed.some((ran) => ran.name === binding.step)
      checks[check] = passed
        ? 'pass'
        : `fail: step ${JSON.stringify(binding.step ?? producer)} did not pass in this invocation` +
          ` (${outcome.failure ?? 'not reached'})`
    }
    let details
    try {
      details = typeof entry.details === 'function' ? entry.details() : entry.details
    } catch (error) {
      process.stdout.write(
        `  ! evidence ${entry.proof} NOT written — details unavailable: ${error.message}\n`,
      )
      continue
    }
    try {
      const evidence = writeEvidence(target, {
        proof: entry.proof,
        checks,
        ...(details !== undefined ? { details } : {}),
        sourcePaths: spec.sourcePaths,
        artifacts: spec.artifacts ?? [],
        referenced: spec.referenced ?? [],
        producer,
      })
      process.stdout.write(
        `  evidence → dist/rust-evidence/${entry.proof}.json (${evidence.result})\n`,
      )
    } catch (error) {
      process.stdout.write(`  ! evidence ${entry.proof} NOT written — ${error.message}\n`)
    }
  }
}

/**
 * The fast gate's evidence mapping: which step backs which required check.
 *
 * The P-FAST-GATE entry maps each of its required checks to its own step — the
 * check names in the frozen manifest ARE the step names, deliberately, so
 * adding a gate step without adding its manifest check (or the reverse) is a
 * visible mismatch rather than a silent gap.
 */
export function fastGateEvidenceMapping() {
  return [
    { proof: 'P-FAST-GATE', step: null, checks: null }, // resolved below against gateSteps()
    {
      proof: 'P-P1-PARITY',
      checks: {
        'p1 parity harness exits 0': { step: 'p1 pricing parity' },
        'frozen expectation reconciled': { step: 'p1 pricing parity' },
      },
      details: () => parityEvidence('p1-parity'),
    },
    {
      proof: 'P-P2-PARITY',
      checks: {
        'p2 parity harness exits 0': { step: 'p2 rates parity' },
        'frozen expectation reconciled': { step: 'p2 rates parity' },
      },
      details: () => parityEvidence('p2-parity'),
    },
    {
      proof: 'P-READINESS-PG',
      checks: {
        'startup readiness blocks on missing forced policy': { step: 'pg readiness suites' },
        'startup readiness blocks on modified migration checksum': { step: 'pg readiness suites' },
        'startup readiness blocks on owner runtime': { step: 'pg readiness suites' },
        'host refuses to serve before readiness passes': { step: 'pg readiness suites' },
        'pooled tenant context does not leak across begin': { step: 'pg readiness suites' },
      },
      // Read from disk, not from the step's exit: the per-check witness detail
      // is what the step produced, and the file is hashed as a referenced
      // producing file so a proof claiming pass cannot cite different bytes
      // than the ones that carried its witnesses.
      details: () => {
        try {
          return JSON.parse(readFileSync(join(root, 'dist/rust-readiness-proof.json'), 'utf8'))
        } catch (error) {
          throw new Error(`readiness witness proof unreadable: ${error.message}`)
        }
      },
    },
    {
      proof: 'P-AUTHORITY-PG',
      checks: {
        'unauthenticated command refused': { step: 'pg authority suite' },
        'forged tenant/role/approval refused': { step: 'pg authority suite' },
        'revoked grant refused on execute': { step: 'pg authority suite' },
        'revoked grant refused on status': { step: 'pg authority suite' },
        'revoked grant refused on replay': { step: 'pg authority suite' },
        'revoked grant refused on recovery': { step: 'pg authority suite' },
        'downgraded grant refused on recovery': { step: 'pg authority suite' },
        'reused key for another command conflicts': { step: 'pg authority suite' },
        'concurrent same-key different-input conflicts': { step: 'pg authority suite' },
        'authorized operation succeeds': { step: 'pg authority suite' },
        'cross-tenant read refused at DB': { step: 'pg authority suite' },
      },
      details: () => {
        try {
          return JSON.parse(readFileSync(join(root, 'dist/rust-authority-proof.json'), 'utf8'))
        } catch (error) {
          throw new Error(`authority witness proof unreadable: ${error.message}`)
        }
      },
    },
    {
      proof: 'P-R03-DOMAIN',
      checks: {
        'contacts/pipeline repository tests pass': { step: 'R03 domain suite' },
        'command receipt state machine tests pass': { step: 'R03 domain suite' },
        'assistant planning refusal tests pass': { step: 'R03 domain suite' },
      },
      details: () => {
        try {
          return JSON.parse(readFileSync(join(root, 'dist/rust-r03-domain-proof.json'), 'utf8'))
        } catch (error) {
          throw new Error(`R03 domain witness proof unreadable: ${error.message}`)
        }
      },
    },
    {
      proof: 'P-CAPACITY-CONFORMANCE',
      checks: {
        'non-finite input rejected (invalid_request)': { step: 'capacity conformance suite' },
        'calendar overflow returns controlled error (no hang)': {
          step: 'capacity conformance suite',
        },
        'loop bounds advance (iteration cap honored)': {
          step: 'capacity conformance suite',
        },
        'horizon and allocation bounds enforced': { step: 'capacity conformance suite' },
        'extreme overflow returns controlled error': { step: 'capacity conformance suite' },
        'corrected parity retained': { step: 'capacity conformance suite' },
      },
      details: () => {
        try {
          return JSON.parse(readFileSync(join(root, 'dist/rust-capacity-proof.json'), 'utf8'))
        } catch (error) {
          throw new Error(`capacity conformance witness proof unreadable: ${error.message}`)
        }
      },
    },
    {
      proof: 'P-PLANNING-CONFORMANCE',
      checks: {
        'conformance cases pass': { step: 'planning conformance suite' },
        'measurement coverage recorded': { step: 'planning conformance suite' },
        // The cold-consumer planning proof rides the consumer record; the
        // grading lives in the planning producer, which the same invocation
        // ran immediately after the cold consumers.
        'cold-consumer planning proof passes': { step: 'planning conformance suite' },
      },
      details: () => {
        try {
          return JSON.parse(readFileSync(join(root, 'dist/rust-planning-proof.json'), 'utf8'))
        } catch (error) {
          throw new Error(`planning conformance witness proof unreadable: ${error.message}`)
        }
      },
    },
    {
      proof: 'P-COLD-CONSUMERS',
      checks: {
        'consumer A (crate tarballs) compiles and tests offline': { step: 'two cold consumers' },
        'consumer B (copied addon) checks pass offline': { step: 'two cold consumers' },
        'artifact freshness enforced': { step: 'two cold consumers' },
        'no product path in consumer manifests': { step: 'two cold consumers' },
      },
      details: () => {
        const proof = JSON.parse(readFileSync(join(root, 'dist/rust-consumer-proof.json'), 'utf8'))
        return { consumers: proof.consumers.length, offlineOnly: proof.offlineOnly }
      },
    },
  ].map((entry) =>
    entry.proof === 'P-FAST-GATE'
      ? {
          proof: 'P-FAST-GATE',
          // Every gate step is a required check; `step: null` would mean the
          // whole-gate exit, but per-step mapping keeps a partially-failed run
          // granular: the green steps still say pass, the rest say why not.
          checks: Object.fromEntries(gateSteps().map(([name]) => [name, { step: name }])),
        }
      : entry,
  )
}

/**
 * Run the gate.
 *
 * A caller supplies the steps, which is what lets the test drive a failure it
 * controls instead of breaking real code to prove the gate notices.
 *
 * @param {Array<[string, () => Record<string, unknown>]>} steps ordered steps
 * @param {{ stdout?: (text: string) => void }} [io] output sink, injectable
 * @returns {{ ok: boolean, passed: Array<Record<string, unknown>>, total: number, failure: string | null }}
 */
export function run(steps, io = {}) {
  const write = io.stdout ?? ((text) => process.stdout.write(text))
  const passed = []

  for (const [name, execute] of steps) {
    try {
      const outcome = execute()
      // The tuple's name is authoritative, not the runner's own label. A step
      // may run an inner helper that names itself differently (`step('rust:
      // build-artifacts', …)` under the gate name `build release artifacts`),
      // and `writeStepProofs` keys every required check on THIS name — so
      // letting the inner label win recorded a passed step as "not reached"
      // in P-FAST-GATE.json. Evidence must describe the step the gate ran.
      passed.push({ ...outcome, name })
    } catch (error) {
      write(`\n✗ ${name} FAILED — ${error.message}\n`)
      write(`  ${passed.length}/${steps.length} steps green before the failure\n`)
      return {
        ok: false,
        passed,
        total: steps.length,
        failure: `${name}: ${error.message}`,
      }
    }
  }

  write(`\nPASS rust:verify — ${steps.length}/${steps.length} steps green\n`)
  for (const { name, seconds } of passed) write(`  ✓ ${name} (${seconds}s)\n`)
  return { ok: true, passed, total: steps.length, failure: null }
}

// Only here does the gate actually build anything. Guarded so `import` in a test
// does not start a 40-second cargo build before the first assertion runs.
if (import.meta.main) {
  const steps = gateSteps()
  const outcome = run(steps)
  // Evidence is written from what ran — on success AND on failure, so a red
  // run replaces any stale green claim instead of leaving it to be read as
  // still true. This is the RF01 producer half for the fast-gate proofs.
  writeStepProofs(outcome, fastGateEvidenceMapping(), 'pnpm rust:verify')
  process.exitCode = outcome.ok ? 0 : 1
}
