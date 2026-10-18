/* global process, URL */
/**
 * P-PROVIDER-CONTRACTS producer: the named-witness runner for the R09
 * provider-contracts slice — the two HTTP seams the assistant turn may
 * cross in production, ported to `archava-providers` over mocked transports:
 * the Gemini plan seam (the free-tier planner's one request/response
 * contract) and the Postmark mail seam (Gateway, attempt label, status
 * port, and the reconciliation that keeps an accepted receipt honest).
 *
 * ## Why a dedicated step instead of riding `cargo test --workspace`
 *
 * The slice's behavior lives in `archava-providers --test
 * provider_contracts` (27 integration witnesses in one file, per the port
 * contract) plus the crate's own library tests. The workspace step already
 * runs both but proves only "every test passed", so a witness deleted from
 * the suite would stay invisible. `writeStepProofs` binds a required check
 * to a step's exit; the manifest's three checks bind to THIS step, and this
 * step binds each check to its witness tests by name (the same RF01 shape
 * as `tools/rust/assistant-context.mjs`). Deleting a proving test then
 * fails here, not just somewhere the gate never looks.
 *
 * ## What each required check collects
 *
 * - `provider contracts preserved` — the wire shapes, ported verbatim from
 *   the TS sources: the Gemini request targets one HTTPS endpoint with a
 *   header-only credential and a body that pins the instruction and the
 *   generation config, the refusal family rides the instruction, the
 *   response schema is strict without retry; the Postmark request targets
 *   the template endpoint with the server token in one header, the payload
 *   carries only the template send fields (TemplateId as a number,
 *   TemplateAlias as a string, never both), and the recipient is the one
 *   address the request carried.
 * - `idempotency/reconciliation tests pass` — the one-call-per-attempt
 *   spine: acceptance needs the MessageID Postmark issued, the ErrorCode is
 *   the discriminator inside a 200, undocumented status refusals stay
 *   refusals, silent statuses stay unknown, a send is never retried,
 *   whatever the answer; the attempt label is a keyed opaque digest that
 *   separates by tenant and by secret, and the status port reconciles the
 *   one matching message while mismatched tenant/fingerprint, unparseable
 *   and unpresent bodies stay silence.
 * - `free/local policy honored` — the free-tier gates: arbitrary personal
 *   text never reaches the free provider, the planner accepts only the
 *   declared safe plan per pilot case, forged or mismatched plans never
 *   become tool requests, the planner bounds simultaneous calls, timeouts
 *   and budget, quota failures do not retry and transport errors stay
 *   quiet, a late body after the deadline is never a successful plan, the
 *   CRM shared policy is the frozen free-tier bucket, an enabled planner
 *   requires a usable key, and oversized or blank utterances refuse before
 *   any call.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-PROVIDER-CONTRACTS` in the frozen
 *   manifest must have a witness mapping here, and every mapping must name a
 *   manifest check: either direction drifting alone is an error, not a silent
 *   gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in the integration suite's output. A suite that exits non-zero
 *   additionally fails the run even when this script's per-check parse looks
 *   green (an unmapped test failing is still a failing suite).
 * - `dist/rust-provider-contracts-proof.json` is written on every run, pass
 *   or fail; `verify.mjs` binds it as `details`. Evidence itself is written
 *   by `writeStepProofs` from the step exit, so a red run replaces any stale
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
 * suite is the crate's own unit tests, run here as a guard so this step
 * fails when the crate's internals regress even though no manifest check
 * maps a unit-test name (the unit tests are not the proof; the witnesses
 * are).
 */
export const SUITES = {
  providerContracts: ['-p', 'archava-providers', '--test', 'provider_contracts'],
  providersLib: ['-p', 'archava-providers', '--lib'],
}

/**
 * Manifest required check → the `--test provider_contracts` witnesses that
 * produce it.
 *
 * Each witness is `['providerContracts', testName]` where `testName` is the
 * exact identifier the harness prints (`test <name> ... ok`). Deliberately
 * duplicated with the Rust source rather than derived from it: a mapping
 * derived from the code it checks can only ever agree with itself, and the
 * drift is the thing that must be visible. The sync direction is enforced at
 * runtime below and by `verify.test.mjs` against the frozen manifest.
 */
export const WITNESSES = {
  // The wire shapes: what the transport carries, pinned byte-for-byte where
  // the TS source pinned it.
  'provider contracts preserved': [
    ['providerContracts', 'the_gemini_request_targets_one_https_endpoint_with_header_only_credentials'],
    ['providerContracts', 'the_gemini_request_body_pins_the_instruction_and_generation_config'],
    ['providerContracts', 'the_gemini_instruction_carries_the_refusal_family'],
    ['providerContracts', 'the_gemini_response_schema_is_strict_without_retry'],
    ['providerContracts', 'the_postmark_request_targets_the_template_endpoint_with_the_token_in_one_header'],
    ['providerContracts', 'the_postmark_payload_carries_only_the_template_send_fields'],
    ['providerContracts', 'a_numeric_template_id_travels_as_templateid_and_an_alias_as_templatealias'],
    ['providerContracts', 'the_recipient_is_the_one_address_the_request_carried'],
  ],
  // The one-call-per-attempt spine: receipt, label, and the reads that
  // recover an uncertain state without ever lying about it.
  'idempotency/reconciliation tests pass': [
    ['providerContracts', 'acceptance_needs_the_message_id_postmark_issued'],
    ['providerContracts', 'the_error_code_is_the_discriminator_inside_a_200'],
    ['providerContracts', 'status_refusals_without_a_body_code_stay_refusals'],
    ['providerContracts', 'silent_statuses_stay_unknown'],
    ['providerContracts', 'a_send_is_never_retried_whatever_the_answer'],
    ['providerContracts', 'the_attempt_label_is_a_keyed_opaque_digest'],
    ['providerContracts', 'identical_keys_separate_by_tenant_and_by_secret'],
    ['providerContracts', 'the_status_port_reconciles_the_one_matching_message'],
    ['providerContracts', 'the_status_port_refuses_a_mismatched_tenant_or_fingerprint'],
    ['providerContracts', 'reconciliation_silence_stays_unknown'],
  ],
  // The free-tier gates: what never reaches the provider, what never comes
  // back as a plan, and what the planner refuses before any cost at all.
  'free/local policy honored': [
    ['providerContracts', 'arbitrary_personal_text_never_reaches_the_free_provider'],
    ['providerContracts', 'the_planner_accepts_only_the_declared_safe_plan_per_pilot_case'],
    ['providerContracts', 'forged_or_mismatched_plans_never_become_tool_requests'],
    ['providerContracts', 'the_planner_bounds_simultaneous_calls_timeouts_and_budget'],
    ['providerContracts', 'quota_failures_do_not_retry_and_transport_errors_stay_quiet'],
    ['providerContracts', 'a_late_body_after_the_deadline_is_never_a_successful_plan'],
    ['providerContracts', 'the_crm_shared_policy_is_the_frozen_free_tier_bucket'],
    ['providerContracts', 'an_enabled_planner_requires_a_usable_key'],
    ['providerContracts', 'oversized_or_blank_utterances_refuse_before_any_call'],
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
 * Run the two archava-providers suites, grade every required check against
 * its named witnesses, write `dist/rust-provider-contracts-proof.json`, and
 * exit non-zero on any failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the
 * frozen manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runProviderContracts() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-PROVIDER-CONTRACTS']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-PROVIDER-CONTRACTS is not an evidence proof in the frozen manifest')

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
  // visible tally change rather than a silent weakening of the proof. (The
  // manifest freezes no `expect` for this proof — the tally rides here.)
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)
  const details = { suites, checks, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details`, and a failing run
  // must record its own truth rather than leave the previous green file to be
  // read as still current.
  const proofPath = join(root, 'dist/rust-provider-contracts-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-provider-contracts-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runProviderContracts()
  if (outcome.ok) {
    process.stdout.write(
      'PASS R09 provider contracts — every required check has its witnesses\n',
    )
  } else {
    process.stdout.write('FAIL R09 provider contracts — see the witness lines above\n')
    process.exitCode = 1
  }
}
