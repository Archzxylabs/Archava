/* global process, URL */
/**
 * P-R03-DOMAIN producer: the named-witness runner for the R03 CRM domain
 * lane — the contacts/pipeline repository reads, the command receipt state
 * machine, and the assistant planning/refusal surface.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * Three suites carry the R03 domain behavior — `archava-host --test crm`
 * (the command lane and its repository), `archava-host --test assistant`
 * (the conversation route over real HTTP and real PostgreSQL), and the
 * `archava-assistant` crate's own library tests (the flow over the canned
 * host, where refusal ORDER and the unverifiable-plan branch live — reachable
 * only behind an injected host, never through the public HTTP surface). The
 * workspace step already runs all of them but proves only "every test
 * passed", so a witness deleted from a suite would stay invisible.
 * `writeStepProofs` binds a required check to a step's exit; the manifest's
 * three checks bind to THIS step, and this step binds each check to its
 * witness tests by name (the RF01 shape of `tools/rust/pg-authority.mjs`).
 * Deleting a proving test then fails here, not just somewhere the gate never
 * looks.
 *
 * ## What each required check collects
 *
 * - `contacts/pipeline repository tests pass` — CRM reads scoped to verified
 *   membership: real rows for a granted reader, the assigned scope seeing
 *   only its own contacts, literal-escaped search, page bounds, and the
 *   conversation route's overview/search/summary/worklist/pipeline witnesses.
 * - `command receipt state machine tests pass` — reviewed command → durable
 *   receipt → authorized recovery: the reviewed write persists, a stale
 *   revision conflicts and records nothing, key replay returns its own
 *   receipt (sequential and racing), a lost response recovers through
 *   replay/status, receipt reads keep their grants and scope, the CSRF
 *   boundary and the service-assembly rules hold.
 * - `assistant planning refusal tests pass` — planning proposes, the server
 *   authorizes and validates in the flow before any execution, and what
 *   could not be verified is refused rather than executed. The crate-side
 *   witnesses cover the two branches the HTTP surface cannot reach honestly:
 *   the mid-turn revision conflict (injected between two transactions) and
 *   the unverifiable-plan branch (no honest HTTP input makes the flow lie
 *   about its own parse).
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-R03-DOMAIN` in the frozen manifest
 *   must have a witness mapping here, and every mapping must name a manifest
 *   check: either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in its suite's output. A suite that exits non-zero additionally
 *   fails the run even when this script's per-check parse looks green (an
 *   unmapped test failing is still a failing suite).
 * - `dist/rust-r03-domain-proof.json` is written on every run, pass or fail;
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

/**
 * The suites this producer targets, keyed by the `suite` field every witness
 * refers to. The two HTTP suites each rebuild a disposable PostgreSQL per
 * test; the crate library tests are the flow behind an injected host and
 * have no database.
 */
export const SUITES = {
  crm: ['-p', 'archava-host', '--test', 'crm'],
  assistant: ['-p', 'archava-host', '--test', 'assistant'],
  assistantCrate: ['-p', 'archava-assistant', '--lib'],
}

/**
 * Manifest required check → the suite tests that produce it.
 *
 * Each witness is `[suite, testName]` where `testName` is the exact
 * identifier the harness prints (`test <name> ... ok`; for the crate library
 * tests that identifier is module-qualified, e.g.
 * `conversation::tests::an_unverifiable_plan_is_unavailable_not_executed`).
 * Deliberately duplicated with the Rust source rather than derived from it: a
 * mapping derived from the code it checks can only ever agree with itself,
 * and the drift is the thing that must be visible. The sync direction is
 * enforced at runtime below and by `verify.test.mjs` against the frozen
 * manifest.
 */
export const WITNESSES = {
  'contacts/pipeline repository tests pass': [
    // The command lane's repository surface — real rows, real scopes.
    ['crm', 'the_contact_list_reads_real_rows_for_a_granted_reader'],
    ['crm', 'an_assigned_scope_sees_only_its_own_contacts'],
    ['crm', 'a_reader_without_a_contact_read_grant_is_refused'],
    ['crm', 'an_unauthenticated_contact_read_is_refused'],
    ['crm', 'a_principal_without_a_crm_membership_is_refused'],
    ['crm', 'a_search_is_escaped_so_the_caller_percent_stays_a_literal'],
    ['crm', 'a_limit_outside_the_page_bounds_is_refused'],
    // The conversation route's reads — the same repository through the rank
    // the milestone adds: overview counting in-scope rows, escaped search
    // over HTTP, exact/choose/not-found name resolution for summary and
    // detail, bucketed worklists with the boundary equality, and the
    // pipeline lane behind its own grant.
    ['assistant', 'an_overview_counts_the_records_the_scope_allows'],
    ['assistant', 'the_contact_search_answers_items_pagination_and_escaped_search'],
    ['assistant', 'the_summary_walks_exact_choose_and_notfound_over_real_rows'],
    ['assistant', 'the_worklist_reads_buckets_boundaries_and_scope'],
    ['assistant', 'the_pipeline_lane_gates_reads_and_serves_real_rows'],
  ],
  'command receipt state machine tests pass': [
    // The reviewed write persists its real effect and carries an approval
    // that this lane cannot send: contact revision, the DB stamp, the audit.
    ['crm', 'a_reviewed_followup_create_persists_the_real_effect'],
    // The conflict arc: a stale revision records nothing new.
    ['crm', 'a_stale_revision_conflicts_and_records_nothing_new'],
    // Replays: the same key returns its own receipt.
    ['crm', 'the_same_key_replays_the_same_receipt'],
    // RN02 (this lane's copy): the key identifies the slot; the fingerprint
    // identifies the command — sequentially and under a race.
    ['crm', 'the_same_key_with_a_different_command_conflicts_without_a_receipt'],
    ['crm', 'another_principals_key_is_unknown_with_no_leak'],
    ['crm', 'the_racing_same_key_yields_conflict_not_the_other_receipt'],
    // Recovery: a lost response re-answered through replay and status.
    ['crm', 'a_lost_response_recovers_through_replay_and_status'],
    ['crm', 'an_unknown_keys_status_is_unconfirmed_not_an_error'],
    // Receipt reads are authorized reads: grants first, scope always.
    ['crm', 'a_receipt_is_refused_without_the_read_grants'],
    ['crm', 'a_receipt_hides_when_its_contact_leaves_the_scope'],
    // The dispatcher's POST rule on the command lane, and the recovery-side
    // refusal when the membership is revoked.
    ['crm', 'a_mutation_without_csrf_is_refused'],
    ['crm', 'a_revoked_membership_is_refused_on_execute_and_on_recovery'],
    // The write grant is checked before any claim is recorded at all.
    ['crm', 'a_missing_write_grant_is_refused_before_any_claim'],
    // The service-on-behalf assembly rules: initiator recording, the
    // subset check, and the scope rule.
    ['crm', 'the_service_path_records_the_human_initiator_when_the_grants_intersect'],
    ['crm', 'the_service_path_refuses_when_the_human_grant_is_not_a_subset'],
    ['crm', 'the_service_scope_never_widens_the_human_scope'],
  ],
  'assistant planning refusal tests pass': [
    // Over HTTP: identity first, then grants, then validation, then the
    // flow's own refusals — with the planner's prompts and envelopes riding
    // the real records it proposes to touch.
    ['assistant', 'an_unmapped_utterance_gets_the_recorded_help'],
    ['assistant', 'a_turn_without_a_session_or_with_a_forged_one_is_refused'],
    ['assistant', 'a_member_without_the_contact_read_grant_is_refused_before_any_tool'],
    ['assistant', 'preparing_a_followup_needs_then_envelops_a_real_draft'],
    ['assistant', 'completing_a_followup_prepares_from_a_real_open_task'],
    ['assistant', 'without_the_followup_write_grant_both_preparations_refuse'],
    ['assistant', 'the_tenant_segment_and_body_are_validated_before_any_database_work'],
    ['assistant', 'an_inactive_or_foreign_membership_is_refused_like_a_missing_one'],
    // In the flow, behind a canned host: the two branches no honest HTTP
    // request can reach — a plan that fails its own parse is unavailable,
    // never executed, and the flow's resolve-order refusals (contact read,
    // follow-up write, pipeline read, completion lifecycle) hold before any
    // tool would run.
    ['assistantCrate', 'conversation::tests::an_unverifiable_plan_is_unavailable_not_executed'],
    ['assistantCrate', 'conversation::tests::without_the_contact_read_grant_the_whole_turn_is_refused'],
    ['assistantCrate', 'conversation::tests::without_the_followup_write_grant_preparation_is_refused'],
    ['assistantCrate', 'conversation::tests::pipeline_reads_refuse_without_the_pipeline_grant'],
    ['assistantCrate', 'conversation::tests::preparing_completion_gates_on_the_task_choice_and_lifecycle'],
    // The planner side of the contract: deterministic values parse into the
    // plan contract, anything outside the recorded vocabulary (including an
    // over-bound query) is unsupported — help, not a guess.
    ['assistantCrate', 'planner::tests::planner_values_parse_into_plan_contract'],
    ['assistantCrate', 'planner::tests::anything_outside_the_vocabulary_is_unsupported'],
    ['assistantCrate', 'planner::tests::an_over_bound_query_is_unsupported'],
  ],
}

/**
 * The `tests` map is the libtest summary (`test <identifier> ... <status>`).
 * Crate-qualified identifiers share the same syntax as the integration
 * suites', so one parser serves both.
 *
 * @param {string[]} cargoArgs arguments after `cargo test`
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
 * Run the three R03 suites, grade every required check against its named
 * witnesses, write `dist/rust-r03-domain-proof.json`, and exit non-zero on
 * any failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runR03Domain() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-R03-DOMAIN']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-R03-DOMAIN is not an evidence proof in the frozen manifest')

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

  const suites = {}
  const runs = {}
  for (const [key, cargoArgs] of Object.entries(SUITES)) {
    const argv = ['test', ...cargoArgs, '--locked', '--offline']
    process.stdout.write(`  $ ${argv.join(' ')}\n`)
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
      // reads the gate log needs the assertion that tripped. Print each
      // `---- <name> stdout ----` block, bounded, so the log carries the real
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
        `suite ${key} exited ${run.exitCode} — a failing suite fails every check it backs`,
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
  // `witnessTotal` pins the SIZE of the witness map in the details blob, so
  // removing a witness from the script while its check still has others is a
  // visible tally change rather than a silent weakening of the proof.
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)
  const details = { suites, checks, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details`, and a failing run
  // must record its own truth rather than leave the previous green file to be
  // read as still current.
  const proofPath = join(root, 'dist/rust-r03-domain-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-r03-domain-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runR03Domain()
  if (outcome.ok) {
    process.stdout.write('PASS R03 domain suite — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL R03 domain suite — see the witness lines above\n')
    process.exitCode = 1
  }
}
