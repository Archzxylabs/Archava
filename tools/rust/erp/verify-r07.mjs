/* global process */
/**
 * P-ERP-R07 producer: the named-witness runner for the R07 ledgers' host
 * suites — billing (invoices, corrections, settlements and voids over the
 * payment-claim grain) landed whole with PACKET-007, subscriptions (plans,
 * entitlements, metered usage and the corrected paid-activation rule) with
 * PACKET-008, attendance (sessions, rosters and the corrected
 * roster-binding check-in ledger) with PACKET-009.
 * Later R07 cohorts extend this map and the frozen manifest additively.
 *
 * Same contract as `verify-r04.mjs` and `verify-r05.mjs` (one suite here,
 * not three): this script parses the test harness output for each witness
 * BY NAME and fails when a named witness did not run or did not pass, so
 * the check and its proof are the same object.
 *
 * ## Why R07 opens a NEW proof rather than growing R05
 *
 * The R05 migration area closed with PACKET-006 — three cohorts, 29 checks,
 * the `P-ERP-R05` surface finished at its frozen tally. Billing is a
 * separate migration area (its own ledger, its own migration package), so
 * shoehorning it into the closed R05 surface would freeze a mixed story;
 * instead R07 opens its OWN growing evidence surface, `P-ERP-R07`: the
 * billing cohort lands with thirteen required checks (witnessTotal pinned
 * at 13), and any later R07 cohort packet EXTENDS this producer and the
 * frozen manifest additively — never by forking a new proof per cohort. A
 * deleted billing witness is therefore a visible gap in THIS run, exactly
 * as the R04 producer's witnesses are.
 *
 * ## Contract
 *
 * - Every `requiredChecks` entry of `P-ERP-R07` in the frozen manifest must
 *   have a witness mapping here, and every mapping must name a manifest check:
 *   either direction drifting alone is an error, not a silent gap.
 * - A check passes only if every one of its witnesses is present with status
 *   `ok` in its suite's output. A suite that exits non-zero additionally fails
 *   the run even when this script's per-check parse looks green (an unmapped
 *   test failing is still a failing suite).
 * - `dist/rust-erp-r07-proof.json` is written on every run, pass or fail;
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
  billing: ['-p', 'archava-host', '--test', 'erp_billing'],
  subscriptions: ['-p', 'archava-host', '--test', 'erp_subscriptions'],
  attendance: ['-p', 'archava-host', '--test', 'erp_attendance'],
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
  // --- The billing cohort (PACKET-007) ------------------------------------------
  //
  // This cohort's witnesses are on the billing ledger's own shape: the
  // migration's first cohort lands whole (13 checks, the R07 surface's sole
  // cohort so far), and the check names are paired with the manifest's
  // requiredChecks exactly. Same fiduciary grain as the ERP kernel — one
  // command per `requestId`, one invoice per `invoiceCode`, one provider
  // event per `(tenant, providerEventId)` — with the settlement money
  // moving ONLY on claims the derived provider key signed.
  //
  // The whole settlement life walks the ledger: the priced invoice starts
  // draft, issues, records an attempt (a receipt with no outcome), and
  // settles ONLY through a claim the derived key signed — the payment
  // event row rides its own idempotent identity, the balance arithmetic
  // rides the frozen fact, and the audit names every operation once; the
  // status read answers the recorded receipt and the bare unknown.
  'authorized billing life documents its settlement': [
    ['billing', 'authorized_billing_life_documents_its_settlement'],
  ],
  // The signature check runs BEFORE any SQL: a tampered amount, a claim
  // signed under another provider's derived key and a digest minted under
  // another domain all refuse the 403 carrier, and NOTHING is left — no
  // event row, no extra command row, no balance movement, no audit row.
  'a forged claim settles nothing': [
    ['billing', 'a_forged_claim_settles_nothing'],
  ],
  // The correction chain is append-only and respects its OWN invoice: a
  // zero correction refuses at the parse boundary, a real correction
  // rebalances through the frozen arithmetic, a missing reversal target is
  // 404, a cross-invoice reversal target is 409.
  'correction reversal respects its invoice': [
    ['billing', 'correction_reversal_respects_its_invoice'],
  ],
  // Voiding holds on recorded settlement: a partially settled invoice
  // refuses (the `unbalanced_settlement` verdict), a fully settled one
  // refuses until a refund empties it — the refund REOPENS `issued` — then
  // the void walks, and void is terminal.
  'voiding holds on recorded settlement': [
    ['billing', 'voiding_holds_on_recorded_settlement'],
  ],
  // Settlement beyond the balance refuses and holds nothing: a succeeded
  // claim over the outstanding balance answers the conflict carrier with
  // NO event row and NO command row (the claim's own insertion rides the
  // same rollback), nor any audit row.
  'settlement beyond the balance holds nothing': [
    ['billing', 'settlement_beyond_the_balance_refuses_and_holds_nothing'],
  ],
  // A FAILED outcome is history: the signed claim lands, its row is
  // recorded, its receipt is the `failed` label — and the invoice's
  // revision, settled amount and status move not one increment.
  'a failed outcome moves no money and no revision': [
    ['billing', 'a_failed_outcome_moves_no_money_and_no_revision'],
  ],
  // The attempt arm asserts no result: a providerRef rides the intent
  // (present or absent), the receipt stays `attempted` with the invoice's
  // own revision, the balance never moves — and a VOIDED invoice takes no
  // attempt at all (the refusal recorded none).
  'the attempt arm asserts no result': [
    ['billing', 'the_attempt_arm_asserts_no_result'],
  ],
  // RN-ERP-01 on the billing tables: recovery re-answers the access gate
  // under CURRENT grants — even a FRESH submission from a dead membership
  // refuses at the ladder (both the execute path and the status path),
  // and once the grant is restored the byte-identical replay answers the
  // stored receipt while a fresh command walks again.
  'revoked billing grant refused on recovery': [
    ['billing', 'revoked_billing_grant_refused_on_recovery'],
  ],
  // A caller outside the rows' tenant is refused before any statement
  // runs: the GLOBEX member submitting ACME-tenant commands gets the
  // ladder's 403 on BOTH the write arm and the status read — and acts
  // freely in its own ledger, keys staying per tenant on the
  // `(tenant_id, request_id)` command space.
  'cross-tenant billing caller refused before any write': [
    ['billing', 'cross_tenant_caller_refused_before_any_write'],
  ],
  // RN-ERP-02: the idempotency key is the slot, the fingerprint is the
  // command — the same `commandId` with a DIFFERENT payload conflicts
  // without recording, even across arm types (the mismatch is the
  // fingerprint's, not the arm's).
  'same billing key different command conflicts': [
    ['billing', 'same_key_different_command_conflicts'],
  ],
  // Lost-response recovery: a byte-identical resubmit answers the stored
  // receipt with `idempotencyReplay: true` — still one invoice, one line
  // row, one command row, one audit row. Nothing commits twice.
  'lost response replays without a second row': [
    ['billing', 'lost_response_replays_the_recorded_receipt'],
  ],
  // The grant ladder on billing's own vocabulary: the read+audit member
  // refuses every write arm while its OWN status reads walk (the status
  // read grants NOTHING beyond enrollment — the TS status read never
  // checks a permission; this gate keeps exactly that), and an
  // audit-only clerk is refused before any statement.
  'narrow billing grants refuse the arms they lack': [
    ['billing', 'insufficient_grants_refuse_before_any_statement'],
  ],
  // The transition/state gates and the boundary refuse at the right
  // layer: issuing an issued invoice, correcting a draft and settling a
  // draft are the 409 class; a malformed document (unknown member,
  // lowercase currency, uppercase signature) is the parse's 400; and no
  // refusal records ANYTHING.
  'transition state and wire boundaries refuse the movement': [
    ['billing', 'the_transition_gates_and_the_wire_boundary_refuse_at_the_right_layer'],
  ],

  // --- The subscriptions cohort (PACKET-008) ------------------------------------
  //
  // The R07 entitlements/usage ledger: five command arms, explicit quotas and
  // the corrected paid-activation rule (audit A05). The witnesses ride the
  // same fiduciary grain — one command per `requestId`, one usage event per
  // `(entitlement, idempotencyKey)` — and add the READ projections the TS api
  // serves (list, detail, entitlements, usage history), because a ledger whose
  // rows can only be written is not a usable domain.
  //
  // The authorized life walks create (draft) → record_payment (active) →
  // consume_usage, and the audit names every operation once; the reads answer
  // the same rows the writes committed.
  'authorized subscription life documents its entitlements': [
    ['subscriptions', 'authorized_subscriptions_life_documents_its_entitlements'],
  ],
  // The A05 correction: the baseline lets a PAID plan's entitlement be
  // consumed while the subscription is still `draft` (never paid). The port
  // refuses with the 402 `not_paid` class and records nothing.
  'an unpaid draft grants no consumption right': [
    ['subscriptions', 'an_unpaid_draft_grants_no_consumption_right'],
  ],
  // The quota is the allocation: beyond it the arm refuses (409) and the
  // meter does not move; exactly to the allocation it records and the
  // entitlement exhausts; one more unit refuses.
  'the quota boundary refuses and records nothing': [
    ['subscriptions', 'the_quota_boundary_refuses_and_records_nothing'],
  ],
  // The usage idempotency key is a slot: the same key under a different
  // command id is a CONFLICT (the baseline lets the UNIQUE constraint raise
  // after the counter moved), and the meter counts once.
  'a duplicate usage key conflicts and records nothing': [
    ['subscriptions', 'a_duplicate_usage_key_conflicts_and_records_nothing'],
  ],
  // The TS's own 402 class: a past-due subscription refuses usage.
  'a past due subscription refuses usage': [
    ['subscriptions', 'a_past_due_subscription_refuses_usage'],
  ],
  // The second half of the A05 correction: `change_status` may not push a
  // never-paid draft to `active` (402 `not_paid`), while a draft may still be
  // canceled — and canceled is terminal.
  'activation without a payment record refuses': [
    ['subscriptions', 'activation_without_a_payment_record_refuses'],
  ],
  // Canceling the subscription revokes its entitlements; a revoked
  // entitlement refuses usage with the TS's `forbidden` (403).
  'a revoked entitlement refuses usage': [
    ['subscriptions', 'a_revoked_entitlement_refuses_usage'],
  ],
  // The optimistic revision boundary: a stale `expectedRevision` conflicts
  // and moves nothing.
  'the stale revision conflicts and records nothing': [
    ['subscriptions', 'the_stale_revision_conflicts_and_records_nothing'],
  ],
  // The idempotency key is the slot, the fingerprint is the command: the same
  // `commandId` with a different payload conflicts and records nothing.
  'the same subscription key a different command conflicts': [
    ['subscriptions', 'the_same_key_a_different_command_conflicts'],
  ],
  // Lost-response recovery: the byte-identical resubmit answers the stored
  // receipt with `idempotencyReplay: true` and the status read answers the
  // TS commandStatus document.
  'lost response replays the recorded subscription receipt': [
    ['subscriptions', 'lost_response_replays_the_recorded_receipt'],
  ],
  // A caller outside the rows' tenant is refused before any statement runs,
  // while the same caller acts freely in its own ledger.
  'cross-tenant subscription callers refuse before any write': [
    ['subscriptions', 'cross_tenant_callers_refuse_before_any_write'],
  ],
  // The grant ladder: read+audit members refuse every write arm while their
  // own reads walk; an audit-only clerk refuses; a revoked membership refuses.
  'narrow subscription grants refuse exactly the arms they lack': [
    ['subscriptions', 'narrow_grants_refuse_exactly_the_arms_they_lack'],
  ],
  // The wire boundary (unknown arm, extra member, lowercase currency, zero
  // units) and the read query bounds (limit 0/101, unknown status filter)
  // refuse at the parse layer with 400, and the detail read answers `null`
  // for an unknown subscription rather than a fabricated page.
  'the subscription wire boundary and read projections hold': [
    ['subscriptions', 'the_wire_boundary_and_the_read_projections_hold'],
  ],

  // --- The attendance cohort (PACKET-009) --------------------------------------
  //
  // The R07 session/roster/check-in ledger: five command arms, one open
  // session window, and the corrected roster binding (audit A04). The
  // witnesses ride the same fiduciary grain — one command per `requestId`,
  // one roster per `(session, contact)`, one ticket per tenant, one event per
  // `(roster, idempotencyKey)` — and add the READ projections the TS api
  // serves (access, session list/detail, roster, check-in ledger), because a
  // ledger whose rows can only be written is not a usable domain.
  //
  // The authorized life walks create → start → register (twice) → confirm →
  // check in, and the audit names every operation once; the reads answer the
  // same rows the writes committed, and the migration's immutability triggers
  // refuse tampering even for the schema owner.
  'authorized attendance life documents its sessions and check-ins': [
    ['attendance', 'authorized_attendance_life_documents_its_sessions_and_check_ins'],
  ],
  // RN-ERP-04 / audit A04: the baseline loads the roster by `(tenant, id)`
  // alone, so a valid roster FROM ANOTHER SESSION checks in against the
  // supplied session and moves the WRONG counters. The port binds the roster
  // to the supplied session in the row SELECT and in the pure policy: the
  // cross-session roster is 404, no event lands, no counter moves, and the
  // other session still exposes only its own row.
  'a roster from another session cannot check in': [
    ['attendance', 'a_roster_from_another_session_cannot_check_in'],
  ],
  // The seat allocation is the boundary: the last seat refuses the next
  // registration (409 `capacity_exceeded`), and two CONCURRENT registrations
  // for the last seat produce exactly one roster — the loser sees the
  // capacity class, the counter never overbooks.
  'the seat boundary refuses and holds nothing': [
    ['attendance', 'the_seat_boundary_refuses_and_holds_nothing'],
  ],
  // `(tenant, session, contact)`, `(tenant, ticket_code)` and
  // `(tenant, session_code)` are identities: a duplicate is a CONFLICT (the
  // baseline lets the UNIQUE constraint raise after the counter moved), and
  // the rolled-back transaction records no roster, no session, no claim.
  'a duplicate registration conflicts and records nothing': [
    ['attendance', 'a_duplicate_registration_conflicts_and_records_nothing'],
  ],
  // The check-in idempotency key is a slot per roster: a second command with
  // the same key — or a different key for an already-attended roster — is
  // `already_checked_in` (409), never a raw SQLSTATE, with one event and one
  // attendance count.
  'the same check in key a different command conflicts': [
    ['attendance', 'the_same_check_in_key_a_different_command_conflicts'],
  ],
  // Two scanners race for one roster: exactly one event commits, the loser
  // waits on the roster lock and refuses with `already_checked_in`; the
  // attended counter advances exactly once.
  'concurrent check ins record exactly one attendance': [
    ['attendance', 'concurrent_check_ins_record_exactly_one_attendance'],
  ],
  // A canceled session closes registration (400 `bad_request`) and refuses
  // check-in; the cancellation cascades over the non-attended roster, and
  // both late arms record nothing.
  'a closed session refuses registration and check in': [
    ['attendance', 'a_closed_session_refuses_registration_and_check_in'],
  ],
  // The optimistic revision boundary: a stale `expectedRevision` on the
  // roster or the session conflicts and moves nothing.
  'the stale attendance revision conflicts and records nothing': [
    ['attendance', 'the_stale_revision_conflicts_and_records_nothing'],
  ],
  // Lost-response recovery: the byte-identical resubmit answers the stored
  // receipt with `idempotencyReplay: true`, the status read answers the TS
  // commandStatus document, and the unknown id is the bare `unknown`.
  'lost response replays the recorded attendance receipt': [
    ['attendance', 'lost_response_replays_the_recorded_attendance_receipt'],
  ],
  // The idempotency key is the slot, the fingerprint is the command: the same
  // `commandId` with a different payload — even across arm types — conflicts
  // and records nothing.
  'the same attendance key a different command conflicts': [
    ['attendance', 'the_same_attendance_key_a_different_command_conflicts'],
  ],
  // A caller outside the rows' tenant is refused before any statement runs on
  // both the write and the status read, while the same caller acts freely in
  // its own ledger.
  'cross tenant attendance callers refuse before any write': [
    ['attendance', 'cross_tenant_attendance_callers_refuse_before_any_write'],
  ],
  // RN-ERP-01 on the attendance tables: a revoked membership refuses the
  // replay and the status read, and the restored grant replays the stored
  // receipt.
  'revoked attendance grant refused on recovery': [
    ['attendance', 'revoked_attendance_grant_refused_on_recovery'],
  ],
  // The grant ladder: a read+audit member refuses the write and check-in
  // arms while its reads walk; an audit-only clerk refuses every read gate;
  // the status and access reads grant nothing beyond enrollment.
  'narrow attendance grants refuse exactly the arms they lack': [
    ['attendance', 'narrow_attendance_grants_refuse_exactly_the_arms_they_lack'],
  ],
  // The wire boundary (unknown arm, extra member, reversed window, zero
  // capacity) and the read query bounds (limit 0/101, negative offset,
  // unknown status) refuse at the parse layer with 400; unknown detail ids
  // answer `null`/empty, and an assigned scope lists only its own sessions.
  'the attendance wire boundary and read projections hold': [
    ['attendance', 'the_attendance_wire_boundary_and_read_projections_hold'],
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
 * Run every R07 suite, grade every required check against its named
 * witnesses, write `dist/rust-erp-r07-proof.json`, and exit non-zero on any
 * failure.
 *
 * Exported so `verify.test.mjs` can assert the witness map against the frozen
 * manifest without launching cargo.
 *
 * @returns {{ ok: boolean, checks: Record<string, object>, suites: Record<string, object>, problems: string[] }}
 */
export function runR07() {
  const manifest = loadManifest(root)
  const spec = manifest.proofs['P-ERP-R07']
  if (!spec || spec.kind !== 'evidence')
    throw new Error('P-ERP-R07 is not an evidence proof in the frozen manifest')

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

  process.stdout.write('  $ cargo test (erp R07 suites, named witnesses)\n')
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
        `suite ${key} exited ${run.exitCode} — a failing R07 suite fails every check it backs`,
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
  // weakening of the proof. Any later R07 cohort grows this tally additively.
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)
  const details = { suites, checks, problems, witnessTotal, allPassed }

  // Written on every run: the gate reads it as `details` and binds its hash as
  // a referenced producing file, and a failing run must record its own truth
  // rather than leave the previous green file to be read as still current.
  const proofPath = join(root, 'dist/rust-erp-r07-proof.json')
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(proofPath, `${JSON.stringify(details, null, 2)}\n`)
  process.stdout.write(`  witness proof → dist/rust-erp-r07-proof.json\n`)

  for (const problem of problems) process.stdout.write(`     ✗ ${problem}\n`)
  return { ok: allPassed, checks, suites, problems }
}

// Guarded so importing `WITNESSES` in a test cannot start cargo.
if (import.meta.main) {
  const outcome = runR07()
  if (outcome.ok) {
    process.stdout.write('PASS erp R07 suites — every required check has its witnesses\n')
  } else {
    process.stdout.write('FAIL erp R07 suites — see the witness lines above\n')
    process.exitCode = 1
  }
}
