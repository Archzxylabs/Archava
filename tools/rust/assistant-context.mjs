/* global process, URL */
/**
 * P-ASSISTANT-CONTEXT producer: the named-witness runner for the R09 context
 * slice — the visitor-assistant turn context the port builds instead of
 * guesses: knowledge classification/retrieval/scope, the consent gate, the
 * masking boundary, the closed decision catalogue and its policy, the rule
 * provider, the validation boundary, the orchestrator judgment, and the turn
 * seam that stitches them into one evaluation.
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The slice's behavior lives in `archava-context --test context` (77
 * integration witnesses in one file, per the port contract) plus the crate's
 * own library tests. The workspace step already runs both but proves only
 * "every test passed", so a witness deleted from the suite would stay
 * invisible. `writeStepProofs` binds a required check to a step's exit; the
 * manifest's three checks bind to THIS step, and this step binds each check
 * to its witness tests by name (the same RF01 shape as
 * `tools/rust/r03-domain.mjs`). Deleting a proving test then fails here, not
 * just somewhere the gate never looks.
 *
 * ## What each required check collects
 *
 * - `context/knowledge/decision parity tests pass` — the surfaces ported from
 *   `packages/knowledge`, `packages/decision` and `packages/assistant` with
 *   their verbatim vocabularies: classification (keywords outrank the
 *   possessive branch), tenant-scoped store reads, retrieval's plan-first
 *   split, the five-task catalogue, per-task floors and directions, the rule
 *   provider's cue tables, the request/response validation boundary, and the
 *   orchestrator's mode/agreement/judgment witnesses.
 * - `provider budget/recovery tests pass` — the provider seam's budget and
 *   the ways a turn recovers while keeping the deterministic answer: one
 *   provider call per turn, none in off mode, none when nothing may be
 *   replaced, the deadline held, a failure context-blind, unready held, a
 *   malformed reply closing the whole reply, a refusal preserving the
 *   baseline. (The R09 provider-contracts packet adds the transport-level
 *   budget witnesses; the map grows then, the checks stay.)
 * - `consent/privacy tests pass` — only explicit `true` grants, withdrawal on
 *   the very next turn, missing consent reported not thrown; the mask walk's
 *   fragment/whole-name/partial classification, the boundary that masks at
 *   the turn seam (client fields and enabled-action fields, whose declared
 *   names the projection carries today), and the redaction that provably
 *   happens before any provider is consulted — utterance scanners and the
 *   evidence projection that carries counts, never entities.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-ASSISTANT-CONTEXT` in the frozen
 *   manifest must have a witness mapping here, and every mapping must name a
 *   manifest check: either direction drifting alone is an error, not a silent
 *   gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in the integration suite's output. A suite that exits non-zero
 *   additionally fails the run even when this script's per-check parse looks
 *   green (an unmapped test failing is still a failing suite).
 * - `dist/rust-assistant-context-proof.json` is written on every run, pass or
 *   fail; `verify.mjs` binds it as `details`. Evidence itself is written by
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
 * refers to. The integration suite carries the named witnesses; the library
 * suite is the crate's own unit tests, run here as a guard so this step fails
 * when the crate's internals regress even though no manifest check maps a
 * unit-test name (the unit tests are not the proof; the witnesses are).
 */
export const SUITES = {
  context: ['-p', 'archava-context', '--test', 'context'],
  contextLib: ['-p', 'archava-context', '--lib'],
}

/**
 * Manifest required check → the `--test context` witnesses that produce it.
 *
 * Each witness is `['context', testName]` where `testName` is the exact
 * identifier the harness prints (`test <name> ... ok`). Deliberately
 * duplicated with the Rust source rather than derived from it: a mapping
 * derived from the code it checks can only ever agree with itself, and the
 * drift is the thing that must be visible. The sync direction is enforced at
 * runtime below and by `verify.test.mjs` against the frozen manifest.
 */
export const WITNESSES = {
  'consent/privacy tests pass': [
    // The consent gate: only explicit `true`, a state value not an exception.
    ['context', 'consent_gate_reads_only_explicit_true'],
    ['context', 'consent_withdrawal_takes_effect_on_the_next_turn'],
    ['context', 'consent_missing_is_reported_not_thrown'],
    // The mask walk: fragments, whole names, partial views, and the values a
    // name cannot weaponise.
    ['context', 'mask_walk_redacts_structured_values_over_classified_names'],
    ['context', 'mask_walk_classifies_fragments_whole_names_and_partials'],
    ['context', 'mask_walk_keeps_null_and_boolean_but_not_numbers'],
    ['context', 'partial_masking_keeps_whole_characters_and_its_notice'],
    ['context', 'mask_string_defaults_to_redaction_for_unknown_fields'],
    ['context', 'resolve_mask_rules_narrows_partials_and_redactions'],
    // The boundary itself: the projection masks at the seam, not in storage,
    // and the value and its notice move together.
    ['context', 'the_boundary_masks_client_fields_at_the_turn_seam'],
    ['context', 'the_boundary_masks_enabled_action_fields'],
    ['context', 'the_projection_withholds_everything_but_shape'],
    ['context', 'the_decision_evidence_names_only_the_page_and_three_publisheds'],
    // Redaction provably happens before a provider is consulted: what the
    // provider receives carries the labels, never the secrets.
    ['context', 'the_turn_seam_redacts_before_a_provider_is_consulted'],
    ['context', 'utterance_masks_emails'],
    ['context', 'utterance_masks_phone_runs'],
    ['context', 'utterance_masks_cards_with_separators'],
    ['context', 'utterance_masks_credentials'],
    ['context', 'utterance_handles_mixed_secrets'],
  ],
  'provider budget/recovery tests pass': [
    // One provider call a turn is allowed; every other settle keeps the
    // deterministic answer and says why.
    ['context', 'run_decision_turn_runs_the_catalogue_once_per_turn'],
    ['context', 'off_mode_makes_no_provider_call'],
    ['context', 'a_provider_is_not_consulted_when_nothing_may_be_replaced'],
    // The deadline: the timer fires first, and the latency is still told.
    ['context', 'the_provider_call_respects_its_deadline'],
    // Failure recovery: context-blind reasons, no partial acceptance, the
    // baseline preserved in every settle the provider cannot complete.
    ['context', 'provider_failures_never_become_context'],
    ['context', 'an_unready_provider_holds_the_turn'],
    ['context', 'a_malformed_reply_closes_the_whole_reply'],
    ['context', 'a_refused_task_preserves_the_baseline'],
  ],
  'context/knowledge/decision parity tests pass': [
    // Classification (packages/knowledge structured-truth §17).
    ['context', 'classification_keeps_live_values_in_the_live_system'],
    ['context', 'classification_reads_a_possessive_as_customer_data'],
    ['context', 'classification_leaves_published_copy_to_retrieval'],
    // The knowledge store: tenant scope on every read, name-not-uuid errors.
    ['context', 'store_document_refuses_unknown_keys_and_reads_known_ones'],
    ['context', 'store_chunks_refuse_cross_tenant_ids_with_distinct_facts'],
    ['context', 'store_search_names_unknown_tenants'],
    ['context', 'store_search_never_reads_another_partition'],
    ['context', 'store_stats_count_the_tenant_they_name'],
    // Retrieval: plan first, defaults, provenance without scores.
    ['context', 'retrieval_plan_puts_price_behind_structured_truth'],
    ['context', 'retrieval_limit_defaults_to_eight'],
    ['context', 'retrieval_chunks_carry_provenance_not_scores'],
    // The catalogue (packages/decision tasks.ts): five tasks, one ladder.
    ['context', 'catalog_declares_the_five_tasks'],
    ['context', 'the_routing_ladder_ascends_toward_structured_truth'],
    // The policy floors and directions, each with a reviewable reason.
    ['context', 'floors_and_directions_are_pinned_per_task'],
    ['context', 'every_policy_carries_a_reason_and_a_floor_in_range'],
    ['context', 'confidence_problem_reads_the_effective_floor'],
    ['context', 'escalation_moves_only_up_the_declared_ladder'],
    ['context', 'disagreement_policy_blocks_only_the_unsafe_direction'],
    ['context', 'a_baseline_answer_may_not_be_wrong_twice'],
    ['context', 'task_answer_problem_covers_the_three_kinds'],
    // The rule provider (packages/decision rule-provider.ts).
    ['context', 'the_rule_provider_counts_cues_never_asserts_certainty'],
    ['context', 'coverage_scores_only_the_supplied_facts'],
    ['context', 'rule_reports_need_a_basis_and_honest_confidence'],
    ['context', 'cue_provider_refuses_when_nothing_or_a_tie_decides'],
    ['context', 'the_provider_answers_intent_from_its_cues'],
    ['context', 'the_provider_answers_coverage_from_the_facts'],
    ['context', 'cue_matching_treats_punctuation_as_word_boundaries'],
    ['context', 'normalisation_folds_case_and_separators'],
    // The validation boundary (packages/decision validate.ts, at the seam).
    ['context', 'the_request_boundary_checks_shape_uniqueness_and_kinds'],
    ['context', 'question_shape_problems_name_the_unusable_question'],
    ['context', 'the_response_boundary_refuses_orphans_and_blanks'],
    ['context', 'the_response_boundary_narrows_answers_to_their_tasks'],
    // The orchestrator judgment and the turn seam's reading of it.
    ['context', 'assist_mode_refuses_to_construct_without_a_provider'],
    ['context', 'assist_applies_the_answer_when_the_gates_open'],
    ['context', 'shadow_mode_records_what_would_have_applied'],
    ['context', 'a_floor_gates_the_answer_it_holds'],
    ['context', 'the_escalation_policy_blocks_a_descent'],
    ['context', 'the_escalation_policy_permits_the_ascent'],
    ['context', 'intent_is_readable_from_any_source'],
    ['context', 'evaluate_turn_with_no_orchestrator_changes_nothing'],
    ['context', 'an_escalation_replaces_the_classification'],
    ['context', 'an_escalation_dedupes_the_candidates_first_seen_wins'],
    ['context', 'an_escalation_needs_the_provider_route'],
    ['context', 'an_escalation_waits_for_candidates'],
    ['context', 'provider_true_flags_are_one_way'],
    ['context', 'low_evidence_scores_are_held_by_the_floor'],
    ['context', 'strong_evidence_scores_flow_but_change_nothing'],
    ['context', 'duplicate_baselines_are_caller_bugs'],
    ['context', 'baselines_outside_the_enabled_set_are_dropped_unexamined'],
    // The projection's page-awareness surface (turn.ts §16).
    ['context', 'enabled_page_actions_are_the_enabled_ones'],
  ],
}

/**
 * The `tests` map is the libtest summary (`test <identifier> ... <status>`).
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
 * Run the two archava-context suites, grade every required check against its
 * named witnesses, write `dist/rust-assistant-context-proof.json`, and exit
 * non-zero on any failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runAssistantContext() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-ASSISTANT-CONTEXT']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-ASSISTANT-CONTEXT is not an evidence proof in the frozen manifest')

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
    const passed = [...run.tests.values()].filter((status) => status === 'ok').length
    const failed = [...run.tests.values()].filter((status) => status === 'FAILED').length
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
  const proofPath = join(root, 'dist/rust-assistant-context-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-assistant-context-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runAssistantContext()
  if (outcome.ok) {
    process.stdout.write('PASS R09 context suite — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL R09 context suite — see the witness lines above\n')
    process.exitCode = 1
  }
}
