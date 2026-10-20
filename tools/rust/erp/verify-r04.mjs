/* global process, URL */
/**
 * P-ERP-R04 producer: the named-witness runner for the R04 ledgers' host
 * suites — organizations, proposals, projects.
 *
 * Same contract as `verify-foodservice.mjs` (the R06 kitchen slice), scaled to
 * THREE suites: the R04 migration carries three ledgers, each with its own
 * grammar, receipts and gates, and each cohort's nine witnesses live in its own
 * integration file:
 *
 * - `rust/crates/host/tests/erp_organizations.rs` — the human-only membership
 *   ledger: affiliations, primary-conflict, archived lifecycles;
 * - `rust/crates/host/tests/erp_proposals.rs` — the scope-amendment ledger:
 *   snapshots frozen beside decisions, change-request amendments;
 * - `rust/crates/host/tests/erp_projects.rs` — the delivery ledger: the frozen
 *   state table, the required-work delivered gate, milestone evidence.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The workspace step already runs these suites — but it proves only "every
 * test passed", and `writeStepProofs` binds a required check to a step's exit.
 * A step that passes without the proving test is exactly the hole RF01 closed:
 * delete `authorized_org_flow_documents_its_life` from the ERP organizations
 * suite and the workspace step stays green while the manifest's "authorized
 * organization life documents its chain" has no producer left. This script
 * parses the test harness output for each witness BY NAME and fails when a
 * named witness did not run or did not pass, so the check and its proof are
 * the same object.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-ERP-R04` in the frozen manifest must
 *   have a witness mapping here, and every mapping must name a manifest check:
 *   either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in its suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-erp-r04-proof.json` is written on every run, pass or fail;
 *   `verify.mjs` binds it as `details` and as a referenced producing file.
 *   Evidence itself is written by `writeStepProofs` from the step exit, so a
 *   red run replaces any stale green claim instead of leaving it readable.
 */

import { execFileSync } from 'node:child_process'
import { mkdirSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { loadManifest } from '../coverage.mjs'

const root = fileURLToPath(new URL('../../../', import.meta.url))
const workspace = join(root, 'rust')

/** The suites this producer targets, keyed by the `suite` field every witness refers to. */
export const SUITES = {
  organizations: ['-p', 'archava-host', '--test', 'erp_organizations'],
  proposals: ['-p', 'archava-host', '--test', 'erp_proposals'],
  projects: ['-p', 'archava-host', '--test', 'erp_projects'],
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
  // --- The organizations cohort -------------------------------------------------
  // The whole membership life on ONE writer: create with affiliations, the
  // draft→active acceptance, an update whose absent keys keep the old values,
  // and the status read answering `committed` with the stored receipt.
  'authorized organization life documents its chain': [
    ['organizations', 'authorized_org_flow_documents_its_life'],
  ],
  // An update reaching an archived organization conflicts at the state gate
  // and holds nothing: no revision move, no audit row, no command row.
  'update on archived refuses and holds nothing': [
    ['organizations', 'update_on_archived_refuses_and_holds_nothing'],
  ],
  // The unique active-membership index and the second-primary rule both
  // refuse inside the same transaction as their effect — the org and its
  // affiliations land together or not at all.
  'affiliation gates refuse without holding': [
    ['organizations', 'affiliation_gates_refuse_without_holding'],
  ],
  // RN-ERP-01: recovery re-answers the access gate under CURRENT grants, so
  // a membership revoked after a receipt was recorded gets a refusal on both
  // the execute path and the status path — not the stored payload.
  'revoked organizations grant refused on recovery': [
    ['organizations', 'revoked_org_grant_refuses_recovery'],
  ],
  // RN-ERP-02: the idempotency key identifies the slot; the fingerprint
  // identifies the command. Same key + different bytes conflicts before any
  // statement runs, and nothing of the rejected attempt commits.
  'same key different command conflicts': [
    ['organizations', 'same_key_different_command_conflicts_without_recording'],
    ['proposals', 'same_key_different_command_conflicts_without_recording'],
    ['projects', 'same_key_different_command_conflicts_without_recording'],
  ],
  // A caller outside the rows' tenant is refused before any write, and the
  // same caller acting in its own tenant succeeds — keys stay per tenant.
  'cross-tenant caller refused before any write': [
    ['organizations', 'cross_tenant_caller_refused_before_any_write'],
    ['proposals', 'cross_tenant_caller_refused_before_any_write'],
    ['projects', 'cross_tenant_caller_refused_before_any_write'],
  ],
  // Lost-response recovery: a byte-identical resubmit replays with the
  // stored receipt and no second organization exists.
  'lost response replays without a second organization': [
    ['organizations', 'lost_response_replays_without_a_second_organization'],
  ],
  // The archived organization releases its affiliation — the one write that
  // skips the archived gate, so departure survives archiving.
  'archived organization still releases its affiliation': [
    ['organizations', 'archived_org_still_releases_its_affiliation'],
  ],
  // The `audit.read`-only clerk cannot write any more than a stranger can,
  // and a malformed arm is a boundary 400 that never reaches a statement.
  'missing organizations grant refused': [
    ['organizations', 'grants_missing_actor_refused_before_any_write'],
  ],

  // --- The proposals cohort -------------------------------------------------------
  // The whole proposal life: create with embedded draft items, the scope swap
  // that rewrites v1 in place, the submit, the accept that freezes the v1
  // snapshot beside the decision — and the status read on the stored receipt.
  'authorized proposal flow documents its life': [
    ['proposals', 'authorized_proposal_flow_documents_its_life'],
  ],
  // An accept against a proposal that never reached `submitted` conflicts at
  // the state gate, the whole submission rolls back, the refused request's
  // command row never lands.
  'accept before submit refuses and holds nothing': [
    ['proposals', 'accept_before_submit_refuses_and_holds_nothing'],
  ],
  // The reject branch moves `submitted → rejected` exactly once; a second
  // reject under a fresh key conflicts without recording.
  'reject records rejected exactly once': [
    ['proposals', 'reject_records_rejected_exactly_once'],
  ],
  // A receipt the CURRENT access no longer authorizes does not replay —
  // neither the execute path nor the status read.
  'revoked proposals grant refused on recovery': [
    ['proposals', 'revoked_proposals_grant_refuses_recovery'],
  ],
  // Lost-response recovery for the snapshot ledger: the replay branch
  // answers `replayed` with no second proposal and no snapshot.
  'lost response replays without a second snapshot': [
    ['proposals', 'lost_response_replays_without_a_second_snapshot'],
  ],
  // The change-request surface: a draft over an accepted proposal, a second
  // draft conflicts, the amendment updates, and the approved snapshot
  // carries the `CR-1: <title>` amendment line with the frozen deltas.
  'change request lifecycle amends the scope': [
    ['proposals', 'change_request_lifecycle_amends_the_scope'],
  ],
  // The `audit.read`-only clerk and an arm the grammar never admits — the
  // grant gate and the boundary 400 that never reaches a statement.
  'missing proposals grant refused': [
    ['proposals', 'grants_missing_actor_refused_before_any_write'],
  ],

  // --- The projects cohort ---------------------------------------------------------
  // The whole delivery chain on the frozen state table: create, milestone,
  // tasks, activation (scope confirmation), the work, the milestone's
  // evidence — settled in review and delivered with zero pending required
  // work and the audit rows on the ledger.
  'authorized delivery chain walks the state table': [
    ['projects', 'authorized_delivery_chain_walks_the_state_table'],
  ],
  // The delivered gate counts required work: pending tasks conflict, an
  // unreached required milestone conflicts, zero required work conflicts —
  // every refusal records no delivered audit row.
  'delivered gate requires the work': [
    ['projects', 'delivered_gate_requires_the_work'],
  ],
  // Out-of-table states and no-op transitions conflict and hold nothing:
  // draft→delivered, active→active — every refusal rolls back its claim.
  'out-of-table transitions refuse and hold nothing': [
    ['projects', 'out_of_table_transitions_refuse_and_hold_nothing'],
  ],
  // A receipt the CURRENT access no longer authorizes does not replay —
  // neither the execute path nor the status read.
  'revoked projects grant refused on recovery': [
    ['projects', 'revoked_projects_grant_refuses_recovery'],
  ],
  // Lost-response recovery for the delivery ledger: the replay branch
  // answers `replayed` with no second project.
  'lost response replays without a second project': [
    ['projects', 'lost_response_replays_without_a_second_project'],
  ],
  // A task living under a REACHED milestone cannot reopen: the parent gate
  // conflicts on every non-done move and the task stays put.
  'task under a reached milestone cannot reopen': [
    ['projects', 'task_under_reached_milestone_cannot_reopen'],
  ],
  // The `audit.read`-only clerk and an arm the grammar never admits — the
  // grant gate and the boundary 400 that never reaches a statement.
  'missing projects grant refused': [
    ['projects', 'grants_missing_actor_refused_before_any_write'],
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
 * Run the three R04 suites, grade every required check against its named
 * witnesses, write `dist/rust-erp-r04-proof.json`, and exit non-zero on any
 * failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runR04() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-ERP-R04']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-ERP-R04 is not an evidence proof in the frozen manifest')

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

  process.stdout.write('  $ cargo test (erp R04 suites, named witnesses)\n')
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
        `suite ${key} exited ${run.exitCode} — a failing R04 suite fails every check it backs`,
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
  const proofPath = join(root, 'dist/rust-erp-r04-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-erp-r04-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runR04()
  if (outcome.ok) {
    process.stdout.write('PASS erp R04 suites — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL erp R04 suites — see the witness lines above\n')
    process.exitCode = 1
  }
}
