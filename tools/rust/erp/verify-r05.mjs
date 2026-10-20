/* global process */
/**
 * P-ERP-R05 producer: the named-witness runner for the R05 ledgers' host
 * suites — the first cohort being scheduling (resource reservations), the
 * second accommodation (room-night stays over the shared occupancy kernel).
 *
 * Same contract as `verify-r04.mjs` (which is `verify-foodservice.mjs` scaled
 * to three suites): this script parses the test harness output for each
 * witness BY NAME and fails when a named witness did not run or did not pass,
 * so the check and its proof are the same object.
 *
 * ## Why the R05 proof grows rather than forks
 *
 * The R05 migration area ships THREE cohorts (scheduling, accommodation,
 * assets), each with its own suite. The proof `P-ERP-R05` is ONE growing
 * evidence surface: the scheduling cohortlands with nine required checks
 * (witnessTotal pinned at 9), and the later cohort packets EXTEND this
 * producer and the frozen manifest additively — never by forking a new
 * proof per cohort. A deleted scheduling witness is therefore a visible
 * gap in THIS run, exactly as the R04 producer's witnesses are.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-ERP-R05` in the frozen manifest must
 *   have a witness mapping here, and every mapping must name a manifest check:
 *   either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in its suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-erp-r05-proof.json` is written on every run, pass or fail;
 *   `verify.mjs` binds it as `details` and as a referenced producing file.
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
  scheduling: ['-p', 'archava-host', '--test', 'erp_scheduling'],
  accommodation: ['-p', 'archava-host', '--test', 'erp_accommodation'],
  assets: ['-p', 'archava-host', '--test', 'erp_assets'],
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
  // --- The scheduling cohort -------------------------------------------------
  // The whole reservation life on ONE writer: create, the set-active cycle,
  // a hold whose expiry the post-lock clock computed, the confirm, the move,
  // the cancel — with the status read answering `committed` and the audit
  // rows naming every operation once.
  'authorized scheduling flow documents its life': [
    ['scheduling', 'authorized_scheduling_flow_documents_its_life'],
  ],
  // The migration's overlap verdict: a second hold over a live interval is a
  // conflict raised by `guard_reservation`, and the refused submission holds
  // nothing — no command row, no audit row, the original untouched.
  'overlapping hold refuses and holds nothing': [
    ['scheduling', 'overlapping_hold_refuses_and_holds_nothing'],
  ],
  // A cancelled reservation is terminal: a fresh-key update conflicts at the
  // state gate, the claim rolls back, and the refused key stays
  // `not_committed`.
  'terminal reservation refuses further updates': [
    ['scheduling', 'terminal_reservation_refuses_updates'],
  ],
  // RN-ERP-01 on the scheduling tables: recovery re-answers the access gate
  // under CURRENT grants — a membership revoked after the receipt was
  // recorded refuses on both the execute path and the status path.
  'revoked scheduling grant refused on recovery': [
    ['scheduling', 'revoked_grant_refuses_recovery'],
  ],
  // RN-ERP-02: the idempotency key is the slot, the fingerprint is the
  // command — the same key with a different operation conflicts and records
  // nothing.
  'same key different command conflicts': [
    ['scheduling', 'same_key_different_command_conflicts_without_recording'],
  ],
  // A caller outside the rows' tenant is refused before any write (the
  // membership read is the first statement), and the same caller acts
  // freely in its own ledger — keys stay per tenant on the
  // `(tenant_id, request_id)` command space.
  'cross-tenant caller refused before any write': [
    ['scheduling', 'cross_tenant_caller_refused_before_any_write'],
  ],
  // Lost-response recovery: a byte-identical resubmit replays the stored
  // receipt with no second resource.
  'lost response replays without a second resource': [
    ['scheduling', 'lost_response_replays_without_a_second_resource'],
  ],
  // The grant ladder on scheduling's own vocabulary: the block arm's extra
  // `resources.write`, and an `audit.read`-only clerk — every refusal leaves
  // no command row.
  'insufficient grants refuse before any statement': [
    ['scheduling', 'insufficient_grants_refuse_before_any_statement'],
  ],
  // The instant boundary: a wall-clock `startsAt` (no offset — the ambiguity
  // the contract refuses) is a 400 that never reaches a statement.
  'malformed instant is a boundary 400': [
    ['scheduling', 'malformed_instant_is_a_boundary_400_never_a_claim'],
  ],

  // --- The accommodation cohort (PACKET-005) -----------------------------------
  // The whole room-night life over the SHARED kernel: property → room type →
  // room (which becomes an accommodation-managed scheduling resource) →
  // hold → confirm → check-in → check-out, the migration's dirty_after_stay
  // bumps on the room row in between, the status read answering the TS
  // commandStatus document, and the audit naming every operation once.
  'authorized accommodation flow documents its life': [
    ['accommodation', 'authorized_accommodation_life_commits_through_the_kernel'],
  ],
  // The calendar resolution (Postgres tzdata authority, the recorded
  // divergence): a local wall time inside the spring-forward gap or the
  // fall-back repeat is the CalendarTimeError verdict — conflict, nothing
  // recorded — while a normal-zone hold resolves the exact instant bytes
  // the property's timezone owns.
  'wall-clock gap or repeat is a conflict': [
    ['accommodation', 'dst_wall_gaps_conflict_and_normal_zones_resolve_bytes_exactly'],
  ],
  // The kernel's overlap verdict on the SAME room: the refused submission
  // holds nothing (one stay, one live slot, no claim row), while a disjoint
  // window on the same room goes through.
  'overlapping stay refuses and holds nothing': [
    ['accommodation', 'overlapping_hold_refuses_and_holds_nothing'],
  ],
  // The maintenance vocabulary's own 409s (illegal transitions), writes
  // under physical occupancy and deactivation under occupancy, and the
  // inactive room refusing holds.
  'housekeeping vocabulary and occupancy refuse the room': [
    ['accommodation', 'housekeeping_vocabulary_and_occupancy_refusals_hold_the_room'],
  ],
  // RN-ERP-03 on the accommodation tables: recovery re-answers the access
  // gate under CURRENT grants — a membership revoked after the receipt was
  // recorded refuses on both the execute path and the status path.
  'revoked accommodation grant refused on recovery': [
    ['accommodation', 'revoked_grant_refuses_the_recovery'],
  ],
  // RN-ERP-04: the idempotency key is the slot, the fingerprint is the
  // command — the same key with a different command conflicts and records
  // nothing on the accommodation ledger.
  'same accommodation key different command conflicts': [
    ['accommodation', 'idempotent_reuse_conflicts_without_recording'],
  ],
  // Lost-response recovery: a byte-identical resubmit replays the stored
  // receipt bytes — no second stay, no second kernel slot.
  'lost response replays without a second stay': [
    ['accommodation', 'replay_answers_the_same_receipt_without_a_second_stay'],
  ],
  // A caller outside the rows' tenant is refused before any write (the
  // membership lock is the first statement), and the same caller acts
  // freely in its own ledger.
  'cross-tenant accommodation caller refused before any write': [
    ['accommodation', 'cross_tenant_callers_refuse_before_any_statement'],
  ],
  // The grant ladder on accommodation's own vocabulary: the missing
  // rooms.write/housekeeping grants refuse exactly their arms while the
  // stay arms walk, and an audit-only clerk is refused before any statement.
  'narrow accommodation grants refuse the arms they lack': [
    ['accommodation', 'narrow_grants_refuse_exactly_the_arms_they_lack'],
  ],
  // The hold's gates: a past arrival, a capacity overflow and the same-day
  // window are the repository's own conflicts; zero/32 nights, the 1800 s
  // hold cap and the checkout-after-checkin order are the wire's 400 s; the
  // unheld space beneath them all still holds, and an unknown request id
  // answers `not_committed`.
  'stay window capacity and boundary gates refuse': [
    ['accommodation', 'window_and_capacity_gates_refuse_while_free_space_holds'],
  ],

  // --- The assets cohort (PACKET-006) -------------------------------------------
  //
  // This cohort's witnesses are on the assets ledger's own shape: the
  // infinite extension of this proof is a fresh set of suite/test pairs that
  // still grade against the same frozen manifest, distinct in name from the
  // two earlier cohorts' families, and the check names are paired with the
  // manifest's requiredChecks exactly.
  //
  // The Rust-side witness identifier is asserted by `verify.test.mjs` to be
  // the `test` name of an EXISTING test in `erp_assets.rs` — the same
  // sync direction as the two earlier cohorts' rows: never derived from the
  // code it checks.
  //
  // The ternary custody life: create → update → dispatch → maintenance
  // → check-in → retire and the receipt kind/revision/identity cells each
  // step binds, with custody_events/maintenance_records/audit readbacks.
  'authorized assets life documents its custody': [
    ['assets', 'authorized_assets_life_documents_its_custody'],
  ],
  // The customer-owned intake: the actor starts in custody BY their own
  // hand, the fixed service_intake row, and the business-owned twin
  // recording none.
  'customer-owned intake records the service intake': [
    ['assets', 'customer_owned_intake_records_the_service_intake'],
  ],
  // The movement vocabulary: the second dispatch, the disembodied check-in
  // and the returned-asset movements are the 409 class, while the
  // damaged-load dispatch and the business-owned return surface the TS
  // 400-class verdicts — and NOTHING records behind any of the refusals.
  'custody vocabulary and state boundaries refuse the movement': [
    ['assets', 'custody_vocabulary_refuses_the_movement'],
  ],
  // RN-ERP-04 on the assets tables: a revoked membership refuses BOTH
  // paths (the fresh-key execution and the status read) after the receipt
  // was recorded.
  'revoked assets grant refused on recovery': [
    ['assets', 'revoked_assets_grant_refuses_the_recovery'],
  ],
  // The same key with a DIFFERENT command conflicts without recording,
  // while a fresh key with the same command commits freely.
  'same assets key different command conflicts': [
    ['assets', 'reuse_conflicts_and_records_nothing'],
  ],
  // A byte-identical resubmit answers the ORIGINAL receipt bytes — still
  // one asset, one command row, one audit row.
  'lost response replays without a second asset': [
    ['assets', 'replay_answers_the_same_receipt_without_a_second_asset'],
  ],
  'cross-tenant assets caller refused before any write': [
    ['assets', 'cross_tenant_assets_callers_refuse_before_any_statement'],
  ],
  // The grant ladder's four write arms each refuse exactly at their own
  // grant gate while the status reads walk (an `all`-scope read answers,
  // and the audit-only clerk refuses its own read).
  'narrow assets grants refuse the arms they lack': [
    ['assets', 'narrow_assets_grants_refuse_exactly_the_arms_they_lack'],
  ],
  // The revision/terminal gates (a stale revision, a retired asset's arm)
  // refuse as conflicts while the wire boundary (bounds, vocabulary, the
  // customer_owned refine, the strict shape) refuses as 400s — none of it
  // recorded, and a correct submission walks through after.
  'stale revision terminal state and wire boundaries refuse': [
    ['assets', 'revision_state_and_boundary_gates_refuse'],
  ],
  // The status read answers the TS commandStatus document verbatim —
  // `committed` with its receipt, bare `not_committed`, the recorded-actor
  // gate under an assigned scope, and the read-grant gate on its own layer.
  'status read answers the assets command status document': [
    ['assets', 'status_read_answers_the_assets_command_status_document'],
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
 * Run the scheduling suite, grade every required check against its named
 * witnesses, write `dist/rust-erp-r05-proof.json`, and exit non-zero on any
 * failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runR05() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-ERP-R05']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-ERP-R05 is not an evidence proof in the frozen manifest')

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

  process.stdout.write('  $ cargo test (erp R05 suites, named witnesses)\n')
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
        `suite ${key} exited ${run.exitCode} — a failing R05 suite fails every check it backs`,
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
  // weakening of the proof. The later R05 cohorts grow this tally additively.
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)
  const details = { suites, checks, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details` and binds its hash as
  // a referenced producing file, and a failing run must record its own truth
  // rather than leave the previous green file to be read as still current.
  const proofPath = join(root, 'dist/rust-erp-r05-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-erp-r05-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runR05()
  if (outcome.ok) {
    process.stdout.write('PASS erp R05 suites — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL erp R05 suites — see the witness lines above\n')
    process.exitCode = 1
  }
}
