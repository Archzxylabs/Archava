import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import {
  EXPECTED_PARITY,
  assertParityExpectations,
  fastGateEvidenceMapping,
  gateSteps,
  run,
  step,
  writeStepProofs,
} from '../verify.mjs'
import { WITNESSES } from '../pg-readiness.mjs'
import { WITNESSES as AUTHORITY_WITNESSES } from '../pg-authority.mjs'
import { WITNESSES as R03_DOMAIN_WITNESSES } from '../r03-domain.mjs'
import { WITNESSES as ASSISTANT_CONTEXT_WITNESSES } from '../assistant-context.mjs'
import { WITNESSES as PURCHASING_WITNESSES } from '../erp/verify-purchasing.mjs'
import { WITNESSES as FOODSERVICE_WITNESSES } from '../erp/verify-foodservice.mjs'
import { WITNESSES as CAPACITY_WITNESSES } from '../capacity-conformance.mjs'
import {
  WITNESSES as PLANNING_WITNESSES,
  PLANNING_CONSUMER_WITNESSES,
} from '../planning-conformance.mjs'
import { loadManifest, repoRoot } from '../coverage.mjs'
import { readEvidence } from '../evidence.mjs'

/**
 * The gate's own tests, run by `node --test`.
 *
 * ## Why every one of these asserts a failure
 *
 * A gate can only be checked by making it fail.
 * `tools/scripts/test/structure.test.ts` builds deliberately broken trees for the
 * same reason, and says so in its own header: a gate tested only against a healthy
 * input "could never do more than assert it is quiet". `rust:verify` has exactly
 * that property — on a healthy tree every step passes, which tells a reader
 * nothing about whether the gate would notice a broken one.
 *
 * The failures here are synthetic, thrown by a step the test suppliers, so the
 * test never needs the tree to be broken. The point is not "the harness can
 * fail"; it is "this gate, with this step order and this propagation, reports the
 * failure it was given."
 */

/** A real passing step: the smallest thing `run()` accepts as green. */
const green = (name) => [name, () => ({ name, seconds: 0 })]

/** Run `run()` with stdout captured, so these assert on the report and not on a console. */
function invoke(steps) {
  let text = ''
  const outcome = run(steps, { stdout: (line) => (text += line) })
  return { outcome, text }
}

test('a green run reports ok and every step as passed', () => {
  const { outcome, text } = invoke([green('first'), green('second')])
  assert.equal(outcome.ok, true)
  assert.equal(outcome.passed.length, 2)
  assert.equal(outcome.total, 2)
  assert.equal(outcome.failure, null)
  assert.match(text, /PASS rust:verify/)
  assert.match(text, /2\/2 steps green/)
})

test('a failing step makes the run not-ok and is named in the failure', () => {
  const { outcome, text } = invoke([
    green('build release artifacts'),
    [
      'cargo clippy --all-targets -- -D warnings',
      () => {
        throw new Error('with exit 2')
      },
    ],
    green('cargo test --workspace'),
  ])
  assert.equal(outcome.ok, false)
  assert.equal(outcome.failure, 'cargo clippy --all-targets -- -D warnings: with exit 2')
  assert.match(text, /✗ cargo clippy/)
  // The count of steps that passed first is in the log on purpose: a failure
  // mid-gate without it reads as a failure of the whole run.
  assert.match(text, /1\/3 steps green before the failure/)
})

test('steps after a failure are not run', () => {
  let reached = false
  const { outcome } = invoke([
    [
      'cargo fmt --all --check',
      () => {
        throw new Error('with exit 1')
      },
    ],
    [
      'must not run',
      () => {
        reached = true
        return {}
      },
    ],
  ])
  assert.equal(reached, false, 'a later step ran after an earlier one failed')
  assert.equal(outcome.ok, false)
  // Pushing on after a failed gate is how a "mostly green" run gets written down
  // as green: nine passing steps after a fatal one is a green-looking log.
})

test('the failure message carries the step name and the exit code', () => {
  const { outcome } = invoke([
    [
      'cargo clippy --all-targets -- -D warnings',
      () => {
        const error = new Error('1 warning emitted')
        error.status = 2
        throw error
      },
    ],
  ])
  assert.match(outcome.failure, /cargo clippy/)
  assert.match(outcome.failure, /1 warning emitted/)
})

test('the gate step name wins over the runner inner label', () => {
  // The bug this pins: `step()` is often called with a label that differs from
  // the gate's own step name (`step('rust:build-artifacts', …)` under the gate
  // name `build release artifacts`). `writeStepProofs` keys every required
  // check on the GATE name, so if the runner's label overwrote it, a passed
  // step was recorded as "not reached" in P-FAST-GATE.json — evidence
  // contradicting the run it claims to describe.
  const { outcome } = invoke([
    ['build release artifacts', () => step('rust:build-artifacts', ['node', '-e', ''])],
  ])
  assert.equal(outcome.ok, true)
  assert.equal(outcome.passed[0].name, 'build release artifacts')
})

test('a step killed by a signal is reported as a signal, not exit -1', () => {
  // `execFileSync` reports a signal death as a null status. Turning that into
  // "exit -1" would send whoever reads the log hunting a failing assertion that
  // never existed, when the real cause is a killed child. So this kills a real
  // child rather than faking the error: the point is the propagation, not the
  // shape of a hand-built object.
  const { outcome, text } = invoke([
    ['signal step', () => step('signal step', ['node', '-e', 'process.kill(process.pid, 9)'])],
  ])
  assert.match(outcome.failure, /signal step failed \(killed by signal\)/)
  assert.equal(text.includes('-1'), false, 'a signal death was reported as exit -1')
})

for (const name of Object.keys(EXPECTED_PARITY)) {
  test(`the frozen expectation for ${name} decomposes the whole workload`, () => {
    const expected = EXPECTED_PARITY[name]
    // The invariant that makes a frozen number checkable at all: every fixture is
    // accounted for exactly once by the four tally buckets. Written this generic it
    // caught a real defect while being written — the first spelling omitted
    // `corrected-expectation`, so p2's 748 + 0 + 0 came to 748 against 753 and the
    // assertion failed on the live, green, correct expectation. That is the good
    // failure: it means the check had teeth.
    const buckets = [
      expected.match,
      expected['corrected-expectation'] ?? 0,
      expected['error-class-only'],
      expected['error-message-only'],
      expected.diverge,
    ]
    const decomposed = buckets.reduce((sum, count) => sum + count, 0)
    assert.equal(
      decomposed,
      expected.fixtures,
      `${name}: match + corrected-expectation + error-class-only + error-message-only + diverge must equal fixtures`,
    )
  })
}

test('a p1 run matching its frozen expectation passes', () => {
  // `see` is the disposition reference, not a number the assertion checks, so it
  // is dropped rather than destructured into an unused binding the linter rejects.
  const expected = { ...EXPECTED_PARITY['p1-parity'] }
  delete expected.see
  assertParityExpectations('p1-parity', expected) // must not throw
})

test('a p1 error-class count that rises is a failure, even though the harness exits 0', () => {
  // The live hole this gate closes. `run-parity.mjs:328` sets a non-zero exit only
  // for `diverge` and `error-message-only`, so a count climbing from 12 to 100
  // leaves a green p1 run. The harness's choice is defensible — that difference is
  // a recorded contract difference, not a defect — which is exactly why the gate
  // has to be the thing that notices.
  assert.throws(
    () =>
      assertParityExpectations('p1-parity', {
        fixtures: 350,
        match: 338,
        'error-class-only': 100,
        'error-message-only': 0,
        diverge: 0,
        passed: 338,
        failed: 12,
      }),
    /does not match its frozen expectation/,
  )
})

test('a p1 error-class count that falls is a failure, because a smaller D01 needs a rewritten D01', () => {
  // A falling count is not automatically good news. Either the port started
  // matching — in which case the D01 disposition must be rewritten in the same
  // review — or the classifier stopped classifying and the run reports a smaller
  // number for the wrong reason. Neither may pass silently.
  assert.throws(
    () =>
      assertParityExpectations('p1-parity', {
        fixtures: 349,
        match: 338,
        'error-class-only': 11,
        'error-message-only': 0,
        diverge: 0,
        passed: 338,
        failed: 11,
      }),
    /does not match its frozen expectation/,
  )
})

test('a p1 error-message-only count of one is a failure', () => {
  // The harness would catch this one itself; the gate asserts it too. It means a
  // message identical across both engines is not identical any more, and no
  // recorded disposition covers that.
  assert.throws(
    () =>
      assertParityExpectations('p1-parity', {
        fixtures: 350,
        match: 338,
        'error-class-only': 12,
        'error-message-only': 1,
        diverge: 0,
        passed: 338,
        failed: 12,
      }),
    /error-message-only: expected 0, got 1/,
  )
})

test('an unknown slice name is refused rather than silently passing', () => {
  // A frozen-expectation check that passes on a name it has no expectation for is
  // a check that passes on everything. `pricing.json`-shaped path mistakes are
  // exactly this failure.
  assert.throws(
    () => assertParityExpectations('p3-parity', { fixtures: 0 }),
    /no frozen expectation recorded for p3-parity/,
  )
})

// ---------------------------------------------------------------------------
// RF01: the gate, its evidence mapping and the frozen manifest cannot drift.
//
// Three lists must stay equal: the gate's steps, the P-FAST-GATE check names
// the evidence mapping writes, and the requiredChecks the frozen manifest
// demands. Any of the three moving alone is the silent-gap failure — a step
// nobody's evidence records, or a check no step ever runs.
// ---------------------------------------------------------------------------

test('the fast gate steps, the evidence mapping and the frozen manifest cannot drift apart', () => {
  const stepNames = gateSteps()
    .map(([name]) => name)
    .sort()
  const mapped = fastGateEvidenceMapping().find((entry) => entry.proof === 'P-FAST-GATE')
  assert.deepEqual(Object.keys(mapped.checks).sort(), stepNames)
  const manifest = loadManifest(repoRoot)
  assert.deepEqual([...manifest.proofs['P-FAST-GATE'].requiredChecks].sort(), stepNames)
})

test('the P-READINESS-PG witness map and the frozen manifest cannot drift apart', () => {
  // Same three-lists-equal property as the gate drift above, for the proof whose
  // producers are named Rust test functions: a manifest check with no witness
  // is a check nothing produces, and a witness naming a check the manifest does
  // not require is a proof of something nobody asked for. Both directions, and
  // the frozen `witnessTotal` too — a witness silently dropped from the script
  // would otherwise leave `expect` describing a map that no longer exists.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-READINESS-PG']
  assert.deepEqual([...Object.keys(WITNESSES)].sort(), [...spec.requiredChecks].sort())
  const witnessTotal = Object.values(WITNESSES).reduce((sum, list) => sum + list.length, 0)
  assert.equal(witnessTotal, spec.expect.witnessTotal)
  for (const [check, witnesses] of Object.entries(WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.ok(
        ['host', 'adapters'].includes(suite),
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-AUTHORITY-PG witness map and the frozen manifest cannot drift apart', () => {
  // Same three-lists-equal property as P-READINESS-PG, for the proof whose
  // producers are the authority suite's named Rust test functions: a manifest
  // check with no witness is a check nothing produces, and a witness naming a
  // check the manifest does not require is a proof of something nobody asked
  // for. Both directions, and the frozen `witnessTotal` too — a witness
  // silently dropped from the script would otherwise leave `expect`
  // describing a map that no longer exists.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-AUTHORITY-PG']
  assert.deepEqual([...Object.keys(AUTHORITY_WITNESSES)].sort(), [...spec.requiredChecks].sort())
  const witnessTotal = Object.values(AUTHORITY_WITNESSES).reduce(
    (sum, list) => sum + list.length,
    0,
  )
  assert.equal(witnessTotal, spec.expect.witnessTotal)
  for (const [check, witnesses] of Object.entries(AUTHORITY_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.equal(
        suite,
        'authority',
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-ERP-PURCHASING witness map and the frozen manifest cannot drift apart', () => {
  // Same three-lists-equal property as P-READINESS-PG and P-AUTHORITY-PG, for
  // the ERP namespace's purchasing cohort: a manifest check with no witness is
  // a check nothing produces, and a witness naming a check the manifest does
  // not require is a proof of something nobody asked for. Both directions, and
  // the frozen `witnessTotal` too.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-ERP-PURCHASING']
  assert.deepEqual([...Object.keys(PURCHASING_WITNESSES)].sort(), [...spec.requiredChecks].sort())
  const witnessTotal = Object.values(PURCHASING_WITNESSES).reduce(
    (sum, list) => sum + list.length,
    0,
  )
  assert.equal(witnessTotal, spec.expect.witnessTotal)
  for (const [check, witnesses] of Object.entries(PURCHASING_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.equal(
        suite,
        'purchasing',
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-ERP-FOODSERVICE witness map and the frozen manifest cannot drift apart', () => {
  // Same three-lists-equal property as P-ERP-PURCHASING, for the ERP
  // namespace's kitchen slice: a manifest check with no witness is a check
  // nothing produces, and a witness naming a check the manifest does not
  // require is a proof of something nobody asked for. Both directions, and
  // the frozen `witnessTotal` too.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-ERP-FOODSERVICE']
  assert.deepEqual([...Object.keys(FOODSERVICE_WITNESSES)].sort(), [...spec.requiredChecks].sort())
  const witnessTotal = Object.values(FOODSERVICE_WITNESSES).reduce(
    (sum, list) => sum + list.length,
    0,
  )
  assert.equal(witnessTotal, spec.expect.witnessTotal)
  for (const [check, witnesses] of Object.entries(FOODSERVICE_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.equal(
        suite,
        'foodservice',
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-R03-DOMAIN witness map and the frozen manifest cannot drift apart', () => {
  // Same three-lists-equal property as P-AUTHORITY-PG, for the proof whose
  // producers are the three R03 suites' named Rust test functions: a manifest
  // check with no witness is a check nothing produces, and a witness naming a
  // check the manifest does not require is a proof of something nobody asked
  // for. Both directions. Unlike the single-suite proofs, crate-qualified
  // identifiers (assistantCrate's library tests) carry `::`, so the
  // identifier shape allows it; and the witness totals are not frozen in the
  // manifest's `expect` for this proof — the tally rides the details blob the
  // producer writes, where a removed witness still moves a visible number.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-R03-DOMAIN']
  assert.deepEqual([...Object.keys(R03_DOMAIN_WITNESSES)].sort(), [...spec.requiredChecks].sort())
  for (const [check, witnesses] of Object.entries(R03_DOMAIN_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_:]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.ok(
        ['crm', 'assistant', 'assistantCrate'].includes(suite),
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-ASSISTANT-CONTEXT witness map and the frozen manifest cannot drift apart', () => {
  // The RF01 shape for the R09 context slice: the three manifest checks are
  // graded by the named witnesses of ONE integration suite
  // (`archava-context --test context`) plus the crate's library tests, so a
  // manifest check with no witness, or a witness naming a check the manifest
  // does not require, is a visible gap in either direction. No frozen
  // `witnessTotal` on this proof — the tally rides the details blob the
  // producer writes, exactly like P-R03-DOMAIN: when the provider-contracts
  // packet lands, appending its witnesses to the budget check moves a visible
  // number instead of tripping a frozen one.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-ASSISTANT-CONTEXT']
  assert.deepEqual(
    [...Object.keys(ASSISTANT_CONTEXT_WITNESSES)].sort(),
    [...spec.requiredChecks].sort(),
  )
  for (const [check, witnesses] of Object.entries(ASSISTANT_CONTEXT_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.ok(
        ['context'].includes(suite),
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-CAPACITY-CONFORMANCE witness map and the frozen manifest cannot drift apart', () => {
  // The RF01 shape for the suite with no database: the six required checks
  // each grade named Rust witnesses, so a manifest check with no witness, or a
  // witness naming a check the manifest does not require, is a visible gap in
  // either direction. This one has no frozen `witnessTotal` — the capacity
  // entry in the manifest carries no `expect` block — so a witness dropped
  // from the producer is caught by the coverage validator's structural
  // equality, not by a tally.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-CAPACITY-CONFORMANCE']
  assert.deepEqual([...Object.keys(CAPACITY_WITNESSES)].sort(), [...spec.requiredChecks].sort())
  for (const [check, witnesses] of Object.entries(CAPACITY_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.equal(
        suite,
        'capacity',
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
})

test('the P-PLANNING-CONFORMANCE witness map and the frozen manifest cannot drift apart', () => {
  // Same property, plus the third check's shape: it is not a cargo witness —
  // it grades the cold-consumer planning record — so this test pins that the
  // producer's maps still name exactly the two cargo checks, that both name
  // the assembly suite, and that the consumer witnesses are the concretely
  // named tests and conditions the record must carry.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-PLANNING-CONFORMANCE']
  // The map carries the two cargo-graded checks only; the third check grades
  // the cold-consumer record and is asserted about below. Exactly one check
  // must sit outside the map, and it must be that one: two maps grading one
  // check, or a check in neither, is the silent gap this test exists for.
  const cargoChecks = [...Object.keys(PLANNING_WITNESSES)].sort()
  const required = [...spec.requiredChecks].sort()
  const outsideCargo = required.filter((check) => !cargoChecks.includes(check))
  const expectedCargo = required.filter(
    (check) => check !== 'cold-consumer planning proof passes',
  )
  assert.deepEqual(cargoChecks, expectedCargo)
  assert.equal(
    outsideCargo.length,
    1,
    'exactly one required check grades outside the cargo maps',
  )
  assert.ok(
    outsideCargo.includes('cold-consumer planning proof passes'),
    'the one check outside the cargo maps is the cold-consumer planning proof',
  )
  for (const [check, witnesses] of Object.entries(PLANNING_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witness tests`)
    for (const [suite, name] of witnesses) {
      assert.match(
        name,
        /^[a-z0-9_]+$/,
        `${check}: witness name ${JSON.stringify(name)} is not a test identifier`,
      )
      assert.equal(
        suite,
        'assembly',
        `${check}: witness ${JSON.stringify(name)} names unknown suite ${JSON.stringify(suite)}`,
      )
    }
  }
  // The consumer witnesses: two named tests the shipped-planning consumer
  // must run, graded against a record only the cold-consumer step writes.
  assert.deepEqual(
    PLANNING_CONSUMER_WITNESSES.tests.sort(),
    [
      'a_cold_catalogue_mismatch_refuses_with_the_baselines_message',
      'plans_an_assembly_from_the_shipped_crate',
    ].sort(),
  )
  assert.deepEqual([...PLANNING_CONSUMER_WITNESSES.conditions].sort(), [
    'coldOffline',
    'productTreeReached',
    'vendoredSourceOnly',
  ])
})

test('every fast-gate evidence mapping names a proof the frozen manifest defines', () => {
  const manifest = loadManifest(repoRoot)
  for (const entry of fastGateEvidenceMapping()) {
    const spec = manifest.proofs[entry.proof]
    assert.ok(spec, `mapping names proof ${entry.proof} that the manifest does not define`)
    assert.equal(spec.kind, 'evidence')
    for (const check of Object.keys(entry.checks ?? {})) {
      assert.ok(
        spec.requiredChecks.includes(check),
        `${entry.proof}: mapping writes check ${JSON.stringify(check)} the manifest does not require`,
      )
    }
  }
})

test('a step that never ran is written as a failed check, not left as a gap', () => {
  // The producer half of RF01, driven against a fixture root so this test
  // cannot touch the repository's real evidence. The run failed at "step two",
  // so "step one" passed and "step two" is a named failure — and crucially it
  // HAS an entry: the validator treats a missing required check exactly like a
  // failed one, but the evidence file should say which it was.
  const root = mkdtempSync(join('/tmp/opencode', 'archava-stepproofs-'))
  mkdirSync(join(root, 'docs/rust'), { recursive: true })
  mkdirSync(join(root, 'src'), { recursive: true })
  writeFileSync(join(root, 'src/a.txt'), 'x\n')
  writeFileSync(
    join(root, 'docs/rust/required-coverage.json'),
    `${JSON.stringify({
      schemaVersion: 1,
      frozen: 'fixture',
      items: [{ id: 'X-1', milestone: 'X', behavior: 'b', proofs: ['P-ONE'] }],
      proofs: {
        'P-ONE': {
          kind: 'evidence',
          requiredChecks: ['step one', 'step two'],
          sourcePaths: ['src/a.txt'],
          artifacts: [],
          referenced: [],
        },
      },
    })}\n`,
  )
  const outcome = {
    ok: false,
    passed: [{ name: 'step one', seconds: 0 }],
    failure: 'step two: with exit 1',
  }
  writeStepProofs(
    outcome,
    [
      {
        proof: 'P-ONE',
        checks: { 'step one': { step: 'step one' }, 'step two': { step: 'step two' } },
      },
    ],
    'fixture gate',
    { root },
  )
  const evidence = readEvidence(root, 'P-ONE')
  assert.equal(evidence.checks['step one'], 'pass')
  assert.match(
    evidence.checks['step two'],
    /^fail: step "step two" did not pass in this invocation/,
  )
  assert.equal(evidence.result, 'fail')
  assert.equal(evidence.producer, 'fixture gate')
})
