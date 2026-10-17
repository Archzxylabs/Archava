/* global process, URL */
/**
 * The full Rust candidate gate: `pnpm rust:verify-release`.
 *
 * ## What this is for, and why it is not the same gate as `rust:verify`
 *
 * `rust:verify` is the fast gate: format, clippy, tests, both parity harnesses,
 * both cold consumers. It answers "does the Rust side still work" in about a
 * minute, so it belongs in `pnpm verify` and CI on every change.
 *
 * This gate answers a different question: "is this tree a defensible release
 * candidate?" That question is not answerable from the Rust side alone. The
 * candidate ships into the existing 1.17.0 platform, so the candidate's proofs
 * include the platform's: the 46-command security chain, the browser end-to-end
 * proof, the cold-prior upgrade and restore, and the second cold-prior recovery
 * path. §1.1 item 1 requires exactly this split, and names the second gate
 * "complete candidate verification including controlled benchmarks and required
 * end-to-end/upgrade/restore proofs."
 *
 * ## Why the list is compared rather than restated
 *
 * The steps are chosen from `docs/LOCAL_LIBRARY_HANDOFF.md`'s "Reproduce
 * acceptance" list, but the gate no longer *asserts* that it mirrors it — it
 * compares the two at run time (`reconcileWithHandoffList`) and records the gaps
 * with their reasons. The first version of this file wrote
 * `everyStepInHandoffList: true` as a literal, which is precisely the failure
 * §1.1 item 1 forbids: the claim was never checked, and it was false. That draft
 * ran a step the documented list does not contain and omitted one it does.
 *
 * ## What this gate does not do
 *
 * It does not create a release and it does not publish anything. It verifies the
 * tree in front of it and writes one machine-readable report of what it saw.
 * Accepting a candidate is a human decision, so this gate's `verdict` is
 * deliberately not an acceptance: a green run means every listed proof passed on
 * this tree, and nothing beyond that.
 */

import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { run, step, writeStepProofs } from './verify.mjs'
import { validateCoverage } from './coverage.mjs'
import { candidateIdentity, readEvidence } from './evidence.mjs'

const root = fileURLToPath(new URL('../../', import.meta.url))

/**
 * Step name to the command that proves it.
 *
 * A separate map rather than a field on the step tuple, because the command is
 * only used for the handoff reconciliation and `step()` takes its own display
 * name. Keeping them apart is what lets the reconciliation compare commands
 * without changing how a step is named in the log.
 */
export const COMMANDS = {
  'rust fast gate': 'pnpm rust:verify',
  'erp module consumers': 'pnpm erp:verify-consumers',
  'compiled-consumer benchmark': 'pnpm rust:benchmark',
  'rust artifact proofs (vitest)': 'pnpm exec vitest run --config vitest.artifacts.config.ts',
  'security chain (46 commands)': 'pnpm security:verify',
  'CRM browser end-to-end proof': 'pnpm crm:verify-browser',
  'Rust browser slice proof': 'node tools/rust/browser-slice.mjs',
  'cold-prior upgrade and restore': 'pnpm crm:verify-upgrade --cold-prior',
  'cold-prior accommodation recovery': 'pnpm accommodation:verify --cold-prior',
  'CRM benchmark': 'pnpm crm:benchmark',
  'kit plan verification': 'pnpm kits:verify',
  'delivery capacity proof': 'pnpm capacity:verify',
  'K05/K04 two-business-model consumers': 'node tools/rust/k05-k04-consumers.mjs',
}

/**
 * The full gate's steps: the fast Rust gate, then the platform proofs.
 *
 * The fast gate runs first so a Rust failure costs a minute instead of ten, and
 * because nothing after it means much on a broken Rust side. Its own header
 * explains why this is one step rather than a re-implementation of each sub-step:
 * `rust:verify` owns the artifact-build ordering both parity harnesses and the
 * cold consumers depend on, and duplicating that ordering here is exactly what
 * produces the two different artifact hashes for one build that ordering prevents.
 *
 * Two steps were removed after the first draft, both measured rather than
 * assumed: `kits:verify` and `capacity:verify` are both already inside the
 * 46-command security chain (`tools/security/verify.mjs:45` and `:64-65`), so
 * running them again added minutes and no proof. The `crm:verify-browser` and
 * `crm:verify-upgrade --cold-prior` steps stay because the chain does **not** run
 * them — `docs/LOCAL_LIBRARY_HANDOFF.md:198-200` says so explicitly.
 *
 * `pnpm accommodation:verify --cold-prior` was missing from the first draft
 * entirely, which the reconciliation below now detects by comparing against the
 * documented list instead of trusting a hardcoded `true`.
 *
 * @returns {Array<[string, () => Record<string, unknown>]>}
 */
export function releaseSteps() {
  return [
    ['rust fast gate', () => step('rust fast gate', ['node', 'tools/rust/verify.mjs'])],

    // The two business-model consumers (K05 agency, K04 wholesale): one
    // artifact cut from the shipped core/CRM crates, two scratch consumers
    // materialized from one template with only a config difference, each
    // verified offline with an empty CARGO_HOME. Right after the fast gate —
    // it needs no fixtures and its artifacts must be cut from the same tree
    // the fast gate just proved.
    [
      'K05/K04 two-business-model consumers',
      () => step('K05/K04 two-business-model consumers', ['node', 'tools/rust/k05-k04-consumers.mjs']),
    ],

    // R06's two independently installed ERP module consumers — the
    // P-MODULE-CONSUMERS proof: both product verticals install the compiled
    // library from tarballs, hash identically across the pair, and run their
    // verify batteries against the installed modules with no workspace path
    // on any resolved entry. Added by the Archxy 2 ERP namespace in
    // PACKET-002 (scope edit for review). Their packet pinned this step
    // directly after the fast gate; main consumed that slot with the K05/K04
    // step before integration, so it rides first after that — both steps are
    // fast, fixtureless and independent of each other.
    [
      'erp module consumers',
      () => step('erp module consumers', ['pnpm', 'erp:verify-consumers']),
    ],

    // Controlled benchmark. Separate from the fast gate because it measures a
    // compiled consumer against a compiled baseline on the current tree, and
    // PRD §3.4 requires both sides compiled with a registered steady state.
    [
      'compiled-consumer benchmark',
      () => step('compiled-consumer benchmark', ['pnpm', 'rust:benchmark']),
    ],

    // RA03's other half, and the reason it lives HERE rather than in the fast
    // gate: every file `vitest.artifacts.config.ts` reads (`build/pricing-engine.mjs`,
    // the workload JSONs, the compiled `archava-bench` consumer) is produced by
    // the `pnpm rust:benchmark` step directly above — not by
    // `rust:build-artifacts`. In the cold unit suite these assertions were
    // either a hard failure on a fresh tree (the `existsSync` case, reaudit
    // 2026-10-16) or a silent early return (`if (!existsSync(...)) return`,
    // two tests). Both are gone: the unit suite never reads a build artifact,
    // and HERE — after the build that produces them — every assertion runs with
    // no skip and no early return, failing hard if a file is missing. A proof
    // that quietly stopped executing would be the same defect as the silent
    // return, one checkpoint later.
    [
      'rust artifact proofs (vitest)',
      () =>
        step('rust artifact proofs (vitest)', [
          'pnpm',
          'exec',
          'vitest',
          'run',
          '--config',
          'vitest.artifacts.config.ts',
        ]),
    ],

    // The 46-command security chain — the core of the candidate's evidence.
    [
      'security chain (46 commands)',
      () => step('security chain (46 commands)', ['pnpm', 'security:verify']),
    ],

    // The documented end-to-end and recovery proofs. Each is slow, rebuilds
    // something, or needs a fixture up, and none of that belongs per-commit.
    [
      'CRM browser end-to-end proof',
      () => step('CRM browser end-to-end proof', ['pnpm', 'crm:verify-browser']),
    ],
    // The Leptos slice's browser proof: the §1.4B vertical slice driven in a
    // REAL browser — SSR + hydration, the visible refusals, and the restart
    // contract (receipts survive a `kill -9` of the server because the data
    // is in the database, not in the process). Builds its own artifacts; its
    // module doc maps each required check to the page action proving it.
    [
      'Rust browser slice proof',
      () => step('Rust browser slice proof', ['node', 'tools/rust/browser-slice.mjs']),
    ],
    [
      'cold-prior upgrade and restore',
      () =>
        step('cold-prior upgrade and restore', [
          'node',
          'apps/crm/scripts/upgrade.mjs',
          '--cold-prior',
        ]),
    ],
    // A second recovery path from a different codebase, on the cold-prior branch
    // the security chain does not take. It was missing from this gate's first
    // draft; the reconciliation with the documented list is what found it.
    [
      'cold-prior accommodation recovery',
      () =>
        step('cold-prior accommodation recovery', ['pnpm', 'accommodation:verify', '--cold-prior']),
    ],
    ['CRM benchmark', () => step('CRM benchmark', ['pnpm', 'crm:benchmark'])],
  ]
}

/**
 * RF01 (closed): the required-inventory binding is a real validator over the
 * frozen `docs/rust/required-coverage.json` manifest, executed by
 * `tools/rust/coverage.mjs`. Every scope item names the proofs that satisfy it;
 * every evidence-kind proof must exist in `dist/rust-evidence/` with ALL of its
 * required checks passing, a recomputed passing `result`, the CURRENT candidate
 * identity (head/branch/toolchain/release profile), a fresh source digest over
 * the manifest's frozen source paths, a fresh artifact digest, and every
 * declared reference hash matching the file it names. A document/json proof
 * must exist at its frozen path and satisfy its contract. Anything absent or
 * failing leaves the gate incomplete with named, item-level reasons — there are
 * no placeholder rows, no self-declared ledger, and no "pending" bypass. The
 * report records the validated coverage document (item/prove counts, error
 * list, manifest digest, validation timestamp) so an auditor can re-run the
 * validator against the same frozen manifest and compare results.
 *
 * @param {object} [options]
 * @param {string} [options.root] Repository root to validate against.
 * @returns {object} The binding record: completeness, blocked-by reasons, the
 *   validated coverage summary, and the live candidate identity.
 */
export function inventoryBinding(options = {}) {
  const validationRoot = options.root ?? root
  const baseline = candidateIdentity(validationRoot)
  const { requiredCoverage, errors } = validateCoverage({
    root: validationRoot,
    identity: baseline,
  })

  const failedProofIds = new Set(
    errors
      .map((message) => /^proof ([A-Z][A-Z0-9-]*)/.exec(message))
      .filter(Boolean)
      .map((match) => match[1]),
  )
  const blockedItems = [
    ...new Set(
      requiredCoverage.items
        .filter((item) => item.proofs.some((proofId) => failedProofIds.has(proofId)))
        .map((item) => item.id),
    ),
  ]
  const missingEvidence = requiredCoverage.proofs
    .filter((proof) => proof.kind === 'evidence' && !readEvidence(validationRoot, proof.id))
    .map((proof) => proof.id)

  const blockedBy = [
    ...blockedItems.map(
      (id) =>
        `scope item ${id} is blocked by failing or missing required proofs (see coverage.errors)`,
    ),
    ...missingEvidence.map(
      (id) =>
        `proof ${id} has no evidence document in dist/rust-evidence/ (step not wired or not run)`,
    ),
    ...(errors.length === 0
      ? []
      : [`coverage validation failed: ${errors.length} error(s) (first: ${errors[0]})`]),
  ]

  return {
    source: 'docs/rust/required-coverage.json',
    requiredInventoryComplete: errors.length === 0,
    blockedBy: errors.length === 0 ? null : blockedBy.join('; '),
    coverage: {
      manifest: requiredCoverage.manifestDigest,
      items: requiredCoverage.items.length,
      proofs: requiredCoverage.proofs.length,
      errors,
      validationTimestamp: new Date().toISOString(),
    },
    baseline,
    releaseProfileVersion: baseline.releaseProfileVersion,
    artifacts: null,
  }
}

/**
 * The release gate's evidence mapping: which step backs which required check.
 *
 * P-FAST-GATE is deliberately absent: the fast gate is run as a subprocess
 * here, and that subprocess already wrote its own per-step evidence
 * (`writeStepProofs` in `verify.mjs`). Rewriting it from this gate's single
 * step exit would replace granular evidence with a coarser claim about the
 * same run.
 *
 * The remaining proofs each bind to the step that produces them. A step that
 * fails — or never runs, because an earlier one aborted the gate — writes
 * `fail: …` evidence rather than nothing, so a red run replaces stale green
 * claims instead of leaving them readable as current.
 */
export function releaseEvidenceMapping() {
  return [
    {
      proof: 'P-ARTIFACT-PROOFS',
      step: 'rust artifact proofs (vitest)',
    },
    {
      proof: 'P-MODULE-CONSUMERS',
      step: 'erp module consumers',
      details: () =>
        JSON.parse(
          readFileSync(join(root, 'dist/rust-erp-module-consumers-proof.json'), 'utf8'),
        ),
    },
    {
      proof: 'P-BENCHMARK',
      step: 'compiled-consumer benchmark',
      details: () =>
        JSON.parse(
          readFileSync(join(root, 'tools/rust/benchmark/evidence/benchmark.json'), 'utf8'),
        ),
    },
    {
      proof: 'P-K05-K04',
      step: 'K05/K04 two-business-model consumers',
      details: () =>
        JSON.parse(readFileSync(join(root, 'dist/rust-k05-k04-proof.json'), 'utf8')),
    },
    {
      proof: 'P-SECURITY-CHAIN',
      step: 'security chain (46 commands)',
      details: () => {
        const results = JSON.parse(
          readFileSync(join(root, 'dist/security-proof/results.json'), 'utf8'),
        )
        return { commands: Array.isArray(results.commands) ? results.commands.length : undefined }
      },
    },
    { proof: 'P-CRM-BROWSER', step: 'CRM browser end-to-end proof' },
    { proof: 'P-RUST-BROWSER', step: 'Rust browser slice proof' },
    { proof: 'P-COLD-PRIOR-UPGRADE', step: 'cold-prior upgrade and restore' },
    { proof: 'P-COLD-PRIOR-RECOVERY', step: 'cold-prior accommodation recovery' },
    { proof: 'P-CRM-BENCHMARK', step: 'CRM benchmark' },
  ]
}

/**
 * The handoff list, verbatim, as the documented reproduce path.
 *
 * Parsed from `docs/LOCAL_LIBRARY_HANDOFF.md` rather than re-typed, because a
 * re-typed copy is the failure this whole file exists to avoid: the first draft
 * of this gate hardcoded `everyStepInHandoffList: true`, asserted a fact it had
 * never checked, and the assertion was false — the gate ran a step the list does
 * not contain (`rust:benchmark`) and omitted one it does
 * (`accommodation:verify --cold-prior`). Computing the comparison is the only
 * version of that field worth writing.
 *
 * Only the fenced code block is read. The surrounding prose mentions command
 * names in explanations and would make the list wrong.
 *
 * @returns {string[]} the documented commands, in documented order
 */
export function handoffCommands() {
  const doc = readFileSync(join(root, 'docs/LOCAL_LIBRARY_HANDOFF.md'), 'utf8')
  const start = doc.indexOf('## Reproduce acceptance\n')
  if (start === -1) throw new Error('handoff list not found: no "Reproduce acceptance" heading')
  const fence = doc.indexOf('```sh', start)
  if (fence === -1) throw new Error('handoff list not found: no code fence after the heading')
  const end = doc.indexOf('```', fence + 5)
  if (end === -1) throw new Error('handoff list not found: unterminated code fence')
  const commands = doc
    .slice(doc.indexOf('\n', fence) + 1, end)
    .split('\n')
    .map((line) => line.trim())
    .filter((line) => line.length > 0)
  if (commands.length === 0) throw new Error('handoff list is empty')
  return commands
}

/**
 * The gate's own steps as commands, so they can be compared to the handoff list.
 *
 * The fast gate is included, not excluded: this gate really does run
 * `pnpm rust:verify`, and it is now in the documented list, so it counts as
 * covered. The one command excluded is `pnpm rust:verify-release` itself — a gate
 * cannot cover itself, and the reconciliation records it in
 * DELIBERATELY_ABSENT.
 */
export function gateCommands() {
  return releaseSteps().map(([name]) => COMMANDS[name])
}

/**
 * Commands this gate omits, each with a recorded reason. An unexplained omission
 * is indistinguishable from an oversight, so every command the gate omits is
 * accounted for here. The reconciliation then asserts that the set of unexplained
 * omissions is empty — that assertion is the reason a future omission cannot be
 * added silently.
 *
 * @type {Array<{ command: string, because: string }>}
 */
const DELIBERATELY_ABSENT = [
  {
    command: 'pnpm rust:verify-release',
    because:
      'this gate itself. The documented list names it because an operator needs ' +
      'the full candidate verification, but a command cannot cover itself, so it ' +
      'is not counted as one of the proofs this gate ran.',
  },
  {
    command: 'pnpm verify',
    because:
      'invoking it from here would recurse: the root gate’s `rust` step calls ' +
      '`pnpm rust:verify`, whose own fast gate would then re-enter this one. It ' +
      'is the root gate and runs independently.',
  },
  {
    command: 'pnpm kits:verify',
    because:
      'already inside the 46-command security chain — `tools/security/verify.mjs:45` ' +
      'runs `kits` — so a second invocation adds minutes and no proof.',
  },
  {
    command: 'pnpm capacity:verify',
    because:
      'already inside the 46-command security chain — `tools/security/verify.mjs:64` ' +
      'runs `capacity` — so a second invocation adds minutes and no proof.',
  },
  {
    command: 'pnpm library:manifest',
    because:
      'a documentation writer, not a proof, and it consumes the chain’s own output. ' +
      'Adding it to a gate would make the security chain’s artefact depend on the ' +
      'chain’s documentation. It runs standalone, after this gate, as it does today.',
  },
]

/**
 * Which handoff-list commands this gate does and does not run, and why.
 *
 * The gaps are recorded rather than hidden so an operator can see exactly what
 * the documented list asks for and what this gate delivers instead, with reasons.
 */
export function reconcileWithHandoffList(
  documented = handoffCommands(),
  executed = gateCommands(),
) {
  const covered = documented.filter((command) => executed.includes(command))
  const missing = documented.filter((command) => !executed.includes(command))
  const explained = DELIBERATELY_ABSENT.map((entry) => entry.command)
  const unexplained = missing.filter((command) => !explained.includes(command))
  // A stale explanation is one for a command the gate now runs — the reason it
  // was omitted stopped being true. Only commands the documented list actually
  // mentions are checked, so a shorter documented list does not make every
  // unmentioned explanation look stale.
  const staleExplanations = explained.filter(
    (command) => documented.includes(command) && executed.includes(command),
  )
  if (unexplained.length > 0) {
    throw new Error(
      `handoff list commands this gate omits without a recorded reason: ${unexplained.join(', ')}`,
    )
  }
  if (staleExplanations.length > 0) {
    throw new Error(
      `recorded omissions the gate now runs anyway: ${staleExplanations.join(', ')} — ` +
        'the reason is no longer true, remove it',
    )
  }
  return {
    covered,
    missingFromGate: missing,
    unexplained,
    notInHandoffList: executed.filter((command) => !documented.includes(command)),
    // Computed, not asserted: a gate that claims to mirror a list while omitting
    // an entry would be reporting a fact nobody checked.
    everyStepInHandoffList: executed.every((command) => documented.includes(command)),
    deliberatelyAbsent: DELIBERATELY_ABSENT,
  }
}

/**
 * Compose the report from a given outcome, reconciliation and binding.
 *
 * Split from `writeReport` because the verdict is the one field in this file that
 * decides an exit code, and it was never testable: `writeReport` writes to
 * `dist/rust-release-report.json`, so a test could only exercise it by overwriting
 * the real evidence. The composition is pure, so both branches — `candidate-verified`
 * and `incomplete` — can be driven with the inputs that produce them.
 *
 * @param {{ ok: boolean, passed: Array<Record<string, unknown>>, total: number, failure: string | null }} outcome
 * @param {object} reconciliation the handoff comparison
 * @param {object} binding the inventory binding
 */
export function composeReport(outcome, reconciliation, binding) {
  return {
    schemaVersion: 1,
    gate: 'rust:verify-release',
    purpose:
      'Complete candidate verification on this tree. Not a release, not an acceptance, ' +
      'not an independent audit.',
    candidate: {
      head: binding.baseline.head,
      branch: binding.baseline.branch,
      rustc: binding.baseline.rustc ?? null,
      cargo: binding.baseline.cargo ?? null,
      node: binding.baseline.node ?? null,
      pnpm: binding.baseline.pnpm ?? null,
    },
    local_verification: {
      ok: outcome.ok,
      stepsPassed: outcome.passed.length,
      stepsTotal: outcome.total,
      steps: outcome.passed.map(({ name, seconds }) => ({ name, seconds })),
      failure: outcome.failure,
      handoffSource: 'docs/LOCAL_LIBRARY_HANDOFF.md "Reproduce acceptance"',
      handoffReconciliation: reconciliation,
    },
    inventoryBinding: binding,
    // A gate that cannot reach this value is not a completion gate, so it is
    // computed once and both the report and the exit code read it.
    verdict: outcome.ok && binding.requiredInventoryComplete ? 'candidate-verified' : 'incomplete',
    independent_review: {
      status: 'pending',
      note:
        'A local gate run is not an independent audit. This field stays "pending" ' +
        'until a reviewer outside this workspace reviews the evidence.',
    },
  }
}

/**
 * Write the machine-readable gate report.
 *
 * One report, with the facts that must never be conflated kept apart:
 * `local_verification` — what this machine observed — `inventory_binding`, which
 * records whether the required inventory evidence exists to bind at all — and
 * `independent_review`, which is `"pending"` and stays that way. A gate that ran
 * locally is not an independent audit, and a report that merged them would let a
 * green run get quoted as an approval. §1.1 item 1 requires the separation
 * explicitly.
 *
 * The exit code follows the verdict, not the step count. §1.1 item 1's rule is
 * that the completion gate must not be green while required inventory or proof
 * is incomplete, so a run that is 8/8 green on its own steps still exits non-zero
 * when the inventory binding is unavailable. That is the whole reason the verdict
 * exists as a separate field from `local_verification.ok`.
 *
 * @param {{ ok: boolean, passed: Array<Record<string, unknown>>, total: number, failure: string | null }} outcome
 */
export function writeReport(outcome) {
  const report = composeReport(outcome, reconcileWithHandoffList(), inventoryBinding())

  const path = join(root, 'dist/rust-release-report.json')
  writeFileSync(path, `${JSON.stringify(report, null, 2)}\n`)
  process.stdout.write(`  report → ${path.replace(`${root}/`, '')}\n`)
  return report
}

if (import.meta.main) {
  const outcome = run(releaseSteps())
  // Evidence from what actually ran, written BEFORE the report so this run's
  // coverage reflects this run's steps (RF01 producer half for release proofs).
  writeStepProofs(outcome, releaseEvidenceMapping(), 'pnpm rust:verify-release')
  const report = writeReport(outcome)
  const { stepsPassed, stepsTotal } = report.local_verification
  process.stdout.write(
    outcome.ok
      ? `\nPASS rust:verify-release — ${stepsPassed}/${stepsTotal} proofs green\n` +
          `  independent_review: ${report.independent_review.status} — unchanged by any local run\n`
      : `\n✗ rust:verify-release FAILED after ${stepsPassed}/${stepsTotal} proofs: ${outcome.failure}\n`,
  )
  // The verdict, not the step count. A green run on an incomplete inventory is
  // reported as what it is and exits non-zero, because that is the fact an
  // operator downstream needs most and the one a step count cannot carry.
  if (report.verdict === 'incomplete') {
    process.stdout.write(
      `  verdict: incomplete — ${report.inventoryBinding.blockedBy ?? 'a required proof did not run'}\n`,
    )
    process.exitCode = 1
  } else {
    process.exitCode = 0
  }
}
