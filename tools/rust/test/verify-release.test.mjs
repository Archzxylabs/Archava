import test from 'node:test'
import assert from 'node:assert/strict'
import { appendFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import {
  COMMANDS,
  composeReport,
  gateCommands,
  handoffCommands,
  inventoryBinding,
  reapOrphanPostmasters,
  releaseEvidenceMapping,
  releaseSteps,
  reconcileWithHandoffList,
} from '../verify-release.mjs'
import { loadManifest, repoRoot, validateCoverage } from '../coverage.mjs'
import { candidateIdentity, readEvidence, writeEvidence } from '../evidence.mjs'
import {
  CONSUMERS,
  WITNESSES as MODULE_CONSUMER_WITNESSES,
} from '../erp/module-consumers.witnesses.mjs'

/**
 * The full candidate gate's own tests, run by `node --test`.
 *
 * `test/verify.test.mjs` explains why a gate is tested by making it fail. This
 * file has a second reason on top of that: `rust:verify-release` is the gate whose
 * report carries a `verdict`, and the first version of it wrote
 * `everyStepInHandoffList: true` as a literal — a field asserting a fact nobody
 * had checked, and the fact was false. So most of what follows tests the claims
 * the report makes rather than the steps it runs.
 *
 * The largest block below is RF01's regression suite. The 2026-10-16 reaudit
 * demonstrated that five self-declared JSON rows (`exists: true`, a non-empty
 * `sha256`, a hardcoded profile) could drive `requiredInventoryComplete` to
 * true with `dist/` absent and one invented proof. The replacement is the
 * frozen manifest plus its validator, and a validator with no negatives is the
 * same one-row ledger wearing a lab coat: each demonstrated attack is replayed
 * here against a synthetic fixture root so the suite proves the refusal, not
 * just the code path.
 *
 * Nothing here invokes the gate. The steps build cargo artifacts and run a
 * 46-command chain; the assertions below are about the report's composition,
 * the reconciliation, and the coverage validator.
 */

/** The gate's own steps, minus the fast gate, as the commands they run. */
const ownSteps = () =>
  releaseSteps()
    .filter(([name]) => name !== 'rust fast gate')
    .map(([name]) => name)

test('every step resolves to a command, so the comparison cannot silently skip one', () => {
  // A step missing from COMMANDS reconciles as `undefined`, which belongs to no
  // documented list. `everyStepInHandoffList` would then read false forever
  // without naming the step at fault.
  for (const name of ownSteps()) {
    assert.equal(typeof COMMANDS[name], 'string', `no command recorded for step "${name}"`)
  }
})

test('the documented list is parsed from the handoff document, not mirrored here', () => {
  const commands = handoffCommands()
  assert.ok(commands.length > 0, 'the documented list came back empty')
  // The commands this gate runs must be a subset of the documented ones plus the
  // three recorded additions, or the document and the gate describe different
  // products. Each exception is named so adding a fourth one is a visible edit.
  // `node tools/rust/browser-slice.mjs` is the real-browser CRM slice proof: it
  // has its own operator entry (`pnpm rust:browser-slice`) and, like the
  // benchmark, is deliberately not inside the handoff fence — the reconciliation
  // reports it under `notInHandoffList` instead.
  const additions = [
    'pnpm rust:benchmark',
    'pnpm exec vitest run --config vitest.artifacts.config.ts',
    'node tools/rust/browser-slice.mjs',
    // R10's operational lane and R11's legacy scan: both named-witness
    // producers with their own operator entries in the release gate's step
    // comments — deliberate additions beyond the platform fence, like the
    // benchmark above.
    'node tools/rust/release-ops.mjs',
    'node tools/rust/legacy-replacement.mjs',
    // R09's remaining browser-lane proofs: SDK/widget contract + realtime
    // lifecycle over the documented vendor bridge (see
    // tools/rust/{sdk-widget,realtime-voice}.mjs headers for the dispositions).
    'node tools/rust/sdk-widget.mjs',
    'node tools/rust/realtime-voice.mjs',
    // OWN-V1's acceptance family (OWNER_CONFIGURATION_PRD §9): the ten
    // P-OWN-Txx proofs run by tools/own/verify.mjs — the suite as the
    // configuration step, the real-browser journey as its own step.
    'node tools/own/verify.mjs',
    'node tools/own/verify.mjs --browser-only',
  ]
  const executed = gateCommands()
  for (const command of executed) {
    assert.ok(
      commands.includes(command) || additions.includes(command),
      `"${command}" is neither documented nor explained`,
    )
  }
})

test('the live gate omits nothing from the handoff list without a recorded reason', () => {
  // The gate-level statement of the invariant `reconcileWithHandoffList` enforces.
  const { unexplained } = reconcileWithHandoffList()
  assert.equal(unexplained.length, 0)
})

test('a gate that omits an unexplained command is refused', () => {
  // The guard, driven with a defective gate. This is the test that would have
  // caught the hardcoded `true`: the assertion now has something to reject.
  assert.throws(
    () =>
      reconcileWithHandoffList(
        ['pnpm verify', 'pnpm security:verify', 'pnpm crm:verify-browser'],
        ['pnpm rust:benchmark'],
      ),
    /omits without a recorded reason/,
  )
})

test('a recorded omission the gate now runs is refused, because its reason expired', () => {
  // The other direction. A gate that starts running a command it documented as
  // skipped must update the record, not keep the stale excuse next to a green run.
  assert.throws(
    () =>
      reconcileWithHandoffList(
        ['pnpm verify', 'pnpm kits:verify'],
        ['pnpm verify', 'pnpm kits:verify'],
      ),
    /runs anyway/,
  )
})

test('a gate running only documented commands reports them all covered', () => {
  // The positive branch of the field the first draft hardcoded. Reaching it by
  // construction, rather than by asserting it, is what makes it a check.
  const { everyStepInHandoffList, covered } = reconcileWithHandoffList(
    ['pnpm security:verify', 'pnpm crm:benchmark'],
    ['pnpm security:verify', 'pnpm crm:benchmark'],
  )
  assert.equal(everyStepInHandoffList, true)
  assert.deepEqual(covered, ['pnpm security:verify', 'pnpm crm:benchmark'])
})

test('the reconciliation reports the commands beyond the handoff list', () => {
  // None of the nine is in the documented list: the list documents platform
  // acceptance and predates the Rust gates. `pnpm rust:benchmark` is the
  // controlled benchmark, the vitest artifact-proofs step is RA03's split of
  // artifact-bound assertions out of the cold unit suite, the browser-slice
  // command is the real-browser proof with its own operator entry, the two
  // R10/R11 producers are the ops-lane and legacy-scan named witnesses, the
  // two R09 producers are the SDK/widget and realtime/voice contract
  // proofs, and the two OWN commands are the owner-configuration acceptance
  // family (tools/own/verify.mjs, suite + browser journey). All nine must
  // be reported as additions rather than quietly counted as covered.
  const { notInHandoffList, covered } = reconcileWithHandoffList()
  assert.deepEqual(notInHandoffList, [
    'pnpm rust:benchmark',
    'pnpm exec vitest run --config vitest.artifacts.config.ts',
    'node tools/rust/browser-slice.mjs',
    'node tools/rust/release-ops.mjs',
    'node tools/rust/legacy-replacement.mjs',
    'node tools/rust/sdk-widget.mjs',
    'node tools/rust/realtime-voice.mjs',
    'node tools/own/verify.mjs',
    'node tools/own/verify.mjs --browser-only',
  ])
  assert.equal(covered.includes('pnpm rust:benchmark'), false)
})

test('the P-MODULE-CONSUMERS witness map and the frozen manifest cannot drift apart', () => {
  // The witness map is carried separately from the driver (so `node --test`
  // can grade it without launching tsx, the pack, or npm) and the three
  // lists-equal property applies anyway: each manifest check is witnessed by
  // one stage per consumer, and the frozen `witnessTotal` pins the map size.
  const manifest = loadManifest(repoRoot)
  const spec = manifest.proofs['P-MODULE-CONSUMERS']
  assert.deepEqual(
    [...Object.keys(MODULE_CONSUMER_WITNESSES)].sort(),
    [...spec.requiredChecks].sort(),
  )
  const witnessTotal = Object.values(MODULE_CONSUMER_WITNESSES).reduce(
    (sum, list) => sum + list.length,
    0,
  )
  assert.equal(witnessTotal, spec.expect.witnessTotal)
  for (const [check, witnesses] of Object.entries(MODULE_CONSUMER_WITNESSES)) {
    assert.ok(witnesses.length > 0, `${check} has no witnesses`)
    assert.equal(witnesses.length, CONSUMERS.length, `${check} is graded per consumer`)
    for (const [consumer, stage] of witnesses)
      assert.ok(
        CONSUMERS.includes(consumer) && stage.length > 0,
        `${check}: unknown consumer/stage ${consumer}:${stage}`,
      )
  }
})

// ---------------------------------------------------------------------------
// RF01: the coverage validator, driven against synthetic fixture roots.
//
// Each test builds the smallest tree the validator grades, then removes or
// corrupts one thing. The point is the refusal: a validator that only ever
// returns errors is indistinguishable from one that always returns them, so
// the valid-fixture test below is load-bearing too.
// ---------------------------------------------------------------------------

/** A minimal valid tree: one item, one evidence proof, one source file. */
function fixtureRoot() {
  const root = mkdtempSync(join('/tmp/opencode', 'archava-coverage-'))
  mkdirSync(join(root, 'docs/rust'), { recursive: true })
  mkdirSync(join(root, 'src'), { recursive: true })
  mkdirSync(join(root, 'dist/rust-evidence'), { recursive: true })
  writeFileSync(join(root, 'src/a.txt'), 'alpha\n')
  writeFileSync(
    join(root, 'docs/rust/required-coverage.json'),
    `${JSON.stringify(
      {
        schemaVersion: 1,
        frozen: 'fixture',
        items: [{ id: 'X-1', milestone: 'X', behavior: 'fixture behavior', proofs: ['P-ONE'] }],
        proofs: {
          'P-ONE': {
            kind: 'evidence',
            requiredChecks: ['one check'],
            expect: { tally: 7 },
            sourcePaths: ['src/a.txt'],
            artifacts: [],
            referenced: [],
          },
        },
      },
      null,
      2,
    )}\n`,
  )
  return root
}

/** Produce valid evidence for the fixture, exactly as a gate step would. */
function produceValidEvidence(root) {
  return writeEvidence(root, {
    proof: 'P-ONE',
    checks: { 'one check': 'pass' },
    details: { tally: 7 },
    sourcePaths: ['src/a.txt'],
    producer: 'fixture producer',
  })
}

/** Read one fixture evidence file, mutate it, write it back. */
function mutateEvidence(root, mutate) {
  const path = join(root, 'dist/rust-evidence/P-ONE.json')
  const evidence = JSON.parse(readFileSync(path, 'utf8'))
  mutate(evidence)
  writeFileSync(path, `${JSON.stringify(evidence, null, 2)}\n`)
}

const validate = (root, identity = candidateIdentity(root)) => validateCoverage({ root, identity })

test('RF01: an empty evidence directory is incomplete, and every proof is named as missing', () => {
  const { errors, requiredCoverage } = validate(fixtureRoot())
  assert.ok(errors.some((e) => /proof P-ONE: no evidence document/.test(e)))
  assert.equal(requiredCoverage.items.length, 1)
  assert.equal(requiredCoverage.proofs.length, 1)
})

test('RF01: a valid evidence document makes the fixture complete — the pass is reachable', () => {
  const root = fixtureRoot()
  produceValidEvidence(root)
  const { errors } = validate(root)
  assert.deepEqual(errors, [], `expected a clean validation, got: ${errors.join('; ')}`)
})

test('RF01: invented proof — evidence the frozen manifest does not name — is refused', () => {
  // The exact reaudit probe: one proof nobody produced. The manifest is closed,
  // so a file it does not name cannot count toward anything.
  const root = fixtureRoot()
  produceValidEvidence(root)
  writeFileSync(
    join(root, 'dist/rust-evidence/P-INVENTED.json'),
    `${JSON.stringify({ proof: 'P-INVENTED', checks: { anything: 'pass' }, result: 'pass' })}\n`,
  )
  const { errors } = validate(root)
  assert.ok(errors.some((e) => /proof P-INVENTED: .*invented proof/.test(e)))
})

test('RF01: stale evidence — covered sources changed after production — is refused', () => {
  const root = fixtureRoot()
  produceValidEvidence(root)
  appendFileSync(join(root, 'src/a.txt'), 'changed after the proof ran\n')
  const { errors } = validate(root)
  assert.ok(errors.some((e) => /proof P-ONE: sourceDigest mismatch/.test(e)))
})

test('RF01: evidence from a different candidate identity is refused', () => {
  const root = fixtureRoot()
  produceValidEvidence(root)
  const foreign = { ...candidateIdentity(root), head: 'f'.repeat(40) }
  const { errors } = validate(root, foreign)
  assert.ok(errors.some((e) => /proof P-ONE: identity\.head/.test(e)))
})

test('RF01: a result contradicting its own checks is refused', () => {
  const root = fixtureRoot()
  produceValidEvidence(root)
  mutateEvidence(root, (evidence) => {
    evidence.result = 'fail'
  })
  const { errors } = validate(root)
  assert.ok(errors.some((e) => /proof P-ONE: result "fail" contradicts its checks/.test(e)))
})

test('RF01: a required check that never ran is treated as a failed check, not a gap', () => {
  // The silent-gap failure the old gate had by construction: no entry at all.
  const root = fixtureRoot()
  produceValidEvidence(root)
  mutateEvidence(root, (evidence) => {
    evidence.checks = {}
    evidence.result = 'fail'
  })
  const { errors } = validate(root)
  assert.ok(errors.some((e) => /proof P-ONE: required check "one check" never ran/.test(e)))
})

test('RF01: a failing check and an undeclared check are both refused', () => {
  const root = fixtureRoot()
  produceValidEvidence(root)
  mutateEvidence(root, (evidence) => {
    evidence.checks = { 'one check': 'fail: because', 'not in manifest': 'pass' }
    evidence.result = 'fail'
  })
  const { errors } = validate(root)
  assert.ok(
    errors.some((e) => /proof P-ONE: required check "one check" is "fail: because"/.test(e)),
  )
  assert.ok(errors.some((e) => /checks the frozen manifest does not declare/.test(e)))
})

test('RF01: a frozen tally that moved is refused', () => {
  const root = fixtureRoot()
  produceValidEvidence(root)
  mutateEvidence(root, (evidence) => {
    evidence.details.tally = 6
  })
  const { errors } = validate(root)
  assert.ok(errors.some((e) => /proof P-ONE: details\.tally is 6, frozen expectation is 7/.test(e)))
})

test('RF01: a structurally invalid manifest is a loud throw, not a scored run', () => {
  const root = fixtureRoot()
  const manifestPath = join(root, 'docs/rust/required-coverage.json')
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
  manifest.items.push({ id: 'X-1', milestone: 'X', behavior: 'duplicate', proofs: ['P-ONE'] })
  manifest.proofs['P-ONE'].requiredChecks = []
  writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`)
  assert.throws(() => validate(root), /structurally invalid/)
})

// ---------------------------------------------------------------------------
// The live binding: identity is real, completeness is the validator's verdict,
// and the report's verdict needs both halves.
// ---------------------------------------------------------------------------

test('the inventory binding carries live identity and delegates completeness to the validator', () => {
  const binding = inventoryBinding()
  // Real identity, read at validation time: git head/branch, toolchain, the
  // release profile from config/. None of it is hand-typed, and none of it is
  // a cached field of some inventory file.
  assert.match(binding.baseline.head, /^[0-9a-f]{40}$/)
  assert.equal(typeof binding.baseline.branch, 'string')
  assert.equal(typeof binding.releaseProfileVersion, 'string')

  // The frozen scope is the whole migration, not five crate labels.
  assert.ok(binding.coverage.items > 80, `expected the full scope, got ${binding.coverage.items}`)
  assert.ok(
    binding.coverage.proofs > 40,
    `expected the full proof set, got ${binding.coverage.proofs}`,
  )
  assert.match(binding.coverage.manifest, /^sha256:[0-9a-f]{64}$/)

  // Completeness is exactly "the validator found nothing": no other input, no
  // override, no cached ledger.
  const { errors } = validateCoverage({
    root: repoRoot,
    identity: candidateIdentity(repoRoot),
  })
  assert.equal(binding.requiredInventoryComplete, errors.length === 0)
  if (binding.requiredInventoryComplete) {
    assert.equal(binding.blockedBy, null)
  } else {
    assert.ok(binding.blockedBy.length > 0, 'incomplete coverage must carry named reasons')
    assert.ok(binding.coverage.errors.length > 0)
  }
})

test('every release step beyond the fast gate backs a proof in the frozen manifest', () => {
  // Drift check: a release step whose evidence mapping names a proof the
  // manifest does not define would write nothing and silently drop the claim.
  const manifest = loadManifest(repoRoot)
  const mapped = releaseEvidenceMapping()
  const mappedSteps = new Set(mapped.map((entry) => entry.step))
  for (const stepName of ownSteps()) {
    assert.ok(mappedSteps.has(stepName), `release step "${stepName}" backs no evidence proof`)
  }
  for (const entry of mapped) {
    const spec = manifest.proofs[entry.proof]
    assert.ok(spec, `mapping names proof ${entry.proof} that the manifest does not define`)
    assert.equal(spec.kind, 'evidence', `${entry.proof} must be an evidence-kind proof`)
    assert.ok(ownSteps().includes(entry.step), `${entry.proof} maps to unknown step ${entry.step}`)
  }
})

/** A green outcome on all of the gate's real steps, for the report tests. */
const greenOutcome = () => ({
  ok: true,
  passed: ownSteps().map((name) => ({ name, seconds: 0 })),
  total: ownSteps().length,
  failure: null,
})

test('a fully green run still reports incomplete while required coverage is incomplete', () => {
  // The rule §1.1 item 1 states outright: the completion gate must not be green
  // while required inventory or proof is incomplete. Every step green is not
  // enough, and this is the branch that runs on this tree today.
  const binding = inventoryBinding()
  const report = composeReport(greenOutcome(), reconcileWithHandoffList(), binding)
  assert.equal(report.local_verification.ok, true)
  assert.equal(report.local_verification.stepsPassed, report.local_verification.stepsTotal)
  if (binding.requiredInventoryComplete) {
    assert.equal(report.verdict, 'candidate-verified')
  } else {
    assert.equal(report.verdict, 'incomplete')
    assert.ok(report.inventoryBinding.blockedBy.length > 0)
    // The blocked-by reasons must be the validator's, not a hand-written string.
    assert.equal(report.inventoryBinding.coverage.errors, binding.coverage.errors)
  }
})

test('the candidate-verified verdict is reachable, and needs both conditions', () => {
  // The positive branch never runs on this tree, because coverage is incomplete.
  // Left unexercised it would be code nobody had executed — so it is driven with
  // the input that reaches it, and with each half of that input withdrawn.
  const real = inventoryBinding()
  const binding = { ...real, requiredInventoryComplete: true, blockedBy: null }
  assert.equal(
    composeReport(greenOutcome(), reconcileWithHandoffList(), binding).verdict,
    'candidate-verified',
  )
  // Green steps but the inventory still incomplete.
  assert.equal(
    composeReport(greenOutcome(), reconcileWithHandoffList(), real).verdict,
    real.requiredInventoryComplete ? 'candidate-verified' : 'incomplete',
  )
  // Inventory complete but a step failed.
  const failed = { ...greenOutcome(), ok: false, failure: 'CRM benchmark: exit 1' }
  assert.equal(composeReport(failed, reconcileWithHandoffList(), binding).verdict, 'incomplete')
})

test('a green local run never flips independent_review', () => {
  // The claim most likely to be quoted out of context. `local_verification.ok` is
  // a fact about this machine; an independent review is a fact about a reviewer.
  // No outcome — green, failing, or with a complete inventory — may change it.
  const binding = { ...inventoryBinding(), requiredInventoryComplete: true }
  for (const outcome of [greenOutcome(), { ...greenOutcome(), ok: false, failure: 'x' }]) {
    const report = composeReport(outcome, reconcileWithHandoffList(), binding)
    assert.equal(report.independent_review.status, 'pending')
    assert.match(report.independent_review.note, /not an independent audit/)
    assert.notEqual(report.local_verification.ok, report.independent_review.status)
  }
})

test('evidence the validator grades is exactly what readEvidence returns — no second copy', () => {
  // The binding and the tests above both read evidence through the producer
  // module's reader; a validator that parsed its own private copy could pass
  // against bytes nobody wrote. This asserts the file on disk is the object
  // both halves share.
  const root = fixtureRoot()
  const written = produceValidEvidence(root)
  assert.deepEqual(readEvidence(root, 'P-ONE'), written)
})

test('the gate start reaps orphan postmasters whose data dir no longer exists', () => {
  // An aborted earlier run can leave private postgres processes alive after
  // their battery's tempdir was removed (twice in one day this left the next
  // run's security chain failing at a different battery each time: with the
  // corpses' io workers churning, `pg_ctl -w` missed its start deadline and
  // the operator surfaced a generic database-state error). The reaper's
  // contract is narrow: kill only whose `-D` data directory does not exist;
  // leave live fixtures and children strictly alone.
  const live = mkdtempSync(join('/tmp', 'reap-live-'))
  const liveData = join(live, 'data')
  mkdirSync(liveData, { recursive: true })
  try {
    const killed = []
    const reaped = reapOrphanPostmasters({
      psLines: [
        '   11 /usr/bin/postgres -D /tmp/archava-corpse-gone/data -p 44187',
        `   12 /usr/bin/postgres -D ${liveData}`,
        '   13 postgres: io worker 0',
        '   14 pg_ctl -D /tmp/archava-corpse-gone/data status',
        '   15 /usr/bin/postgres -D',
        '',
      ],
      kill: (pid, signal) => killed.push({ pid, signal }),
      out: { write: () => {} },
    })
    assert.equal(reaped, 1, 'only the corpse is reaped')
    assert.deepEqual(killed, [{ pid: 11, signal: 'SIGTERM' }])
  } finally {
    rmSync(live, { recursive: true, force: true })
  }
})
