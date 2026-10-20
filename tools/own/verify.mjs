/* global process */
/**
 * The OWN-V1 acceptance driver: `pnpm own:verify` → the ten P-OWN-Txx proofs.
 *
 * ## What changed, and why the old driver was retired
 *
 * The previous driver graded the OWN lane mostly from the TypeScript/`vitest`
 * packages (the pre-Rust owner engine and the old CRM authority producer). The
 * owner engine, its persistence, the `/own/*` routes and the CRM module gate
 * now live in Rust; a proof that still reads the old TS lane is evidence for a
 * program the product no longer ships. This driver grades the SAME frozen
 * checks from the Rust evidence that actually implements them:
 *
 * - `cargo test -p archava-own` — the deterministic engine (`t01`…`t09`);
 * - `cargo test -p archava-host --test own -- --test-threads=1` — the persisted
 *   lifecycle over real HTTP and real `archava_owner`/`archava_crm` ledgers;
 * - `tools/rust/k05-k04-consumers.mjs` — the two packaged business-model
 *   consumers, now running the real CRM read/write/recovery flow;
 * - `dist/rust-evidence/P-RUST-BROWSER.json` — the real headless-browser
 *   owner journey, whose `details.owner` carries the T10 stages.
 *
 * Every named test is required to have actually run. A suite exit of 0 with
 * the named witness absent is a failure, not a pass.
 *
 * ## The both-directions guard
 *
 * The map below must equal each proof's frozen `requiredChecks` in
 * `docs/rust/required-coverage.json` in both directions: a check deleted from
 * the manifest, or added without a producer here, is announced instead of
 * silently passing.
 *
 * ## Evidence discipline
 *
 * Each proof writes `dist/own-evidence/<proof>.json` on EVERY run, including
 * failures, bound to the current git HEAD and the library release version. The
 * release gate's evidence mapping binds the file to the step that produced it.
 * `--browser-only` runs the browser journey itself and grades only P-OWN-T10;
 * the other nine proofs are graded by the full run.
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { loadManifest } from '../rust/coverage.mjs'
import { runK05K04Consumers } from '../rust/k05-k04-consumers.mjs'

const root = resolve(import.meta.dirname, '../..')
const workspace = join(root, 'rust')
const evidenceDir = join(root, 'dist/own-evidence')
const k05k04Evidence = join(root, 'dist/rust-k05-k04-proof.json')
const browserEvidence = join(root, 'dist/rust-evidence/P-RUST-BROWSER.json')
mkdirSync(evidenceDir, { recursive: true })

/**
 * The frozen required checks, per proof id. This is the producer's side of
 * the both-directions guard; it must not be edited without the manifest.
 */
const REQUIRED = {
  'P-OWN-T01': [
    'two business models on identical released core/CRM',
    'owner config changes via UI only',
  ],
  'P-OWN-T02': [
    'chat plan equals dashboard plan',
    'authoritative base/config/catalogue/pricing identity recorded',
  ],
  'P-OWN-T03': [
    'preview shows changes/dependencies/readiness/cost',
    'no hidden activation/payment/provider effect',
  ],
  'P-OWN-T04': [
    'missing grant rejected',
    'public session rejected',
    'forged tenant/role/approval rejected',
    'revoked confirm/status/replay rejected',
  ],
  'P-OWN-T05': [
    'incompatible dependency yields blocked result',
    'blocked result never counted PASS',
  ],
  'P-OWN-T06': ['racing apply converges to one effect', 'refresh/lost response/restart recoverable'],
  'P-OWN-T07': [
    'enable/disable changes UI/API/tool availability',
    'disabled direct API rejected',
    'dependent disable rejected',
  ],
  'P-OWN-T08': ['records/audit/receipts/permissions preserved', 'restore does not rewrite history'],
  'P-OWN-T09': [
    'stale snapshot forces replan',
    'changed targets force reconfirmation',
    'no invented price/readiness',
  ],
  'P-OWN-T10': [
    'inspect→plan→preview→confirm→applied→recovery/history/restore in real browser',
    'no owner code/file/terminal steps',
  ],
}

const manifest = loadManifest(root)
for (const [proofId, checks] of Object.entries(REQUIRED)) {
  const spec = manifest.proofs?.[proofId]
  if (!spec) throw new Error(`${proofId} missing from the frozen manifest`)
  const frozen = [...(spec.requiredChecks ?? [])].sort()
  const mine = [...checks].sort()
  if (JSON.stringify(frozen) !== JSON.stringify(mine)) {
    throw new Error(
      `${proofId} drifted between the frozen manifest and this producer:\n` +
        `  manifest : ${JSON.stringify(spec.requiredChecks)}\n` +
        `  producer : ${JSON.stringify(checks)}`,
    )
  }
}

/** The library release identity, carried in every evidence record. */
function configVersion() {
  const path = join(root, 'config/library-release.v1.json')
  if (!existsSync(path)) return null
  return JSON.parse(readFileSync(path, 'utf8')).version ?? null
}

function head() {
  const git = (args) => {
    const result = spawnSync('git', ['rev-parse', ...args], { cwd: root, encoding: 'utf8' })
    return result.status === 0 ? result.stdout.trim() : null
  }
  return {
    head: git(['HEAD']),
    branch: git(['--abbrev-ref', 'HEAD']),
    node: process.version,
  }
}

function writeProof(proofId, details, grades, problems) {
  const record = {
    schemaVersion: 'archava-own-proof/v1',
    producedBy: 'pnpm own:verify',
    proof: proofId,
    requiredChecks: REQUIRED[proofId],
    result: Object.values(grades).every(Boolean) ? 'pass' : 'fail',
    grades,
    details,
    problems,
    configVersion: configVersion(),
    candidate: head(),
    producedAt: new Date().toISOString(),
  }
  writeFileSync(join(evidenceDir, `${proofId}.json`), `${JSON.stringify(record, null, 2)}\n`)
  process.stdout.write(`  evidence → dist/own-evidence/${proofId}.json\n`)
}

function runCapture(command, args, cwd, timeoutMs = 1_800_000) {
  const result = spawnSync(command, args, {
    cwd,
    encoding: 'utf8',
    timeout: timeoutMs,
    maxBuffer: 512 * 1024 * 1024,
    env: { ...process.env, CARGO_TERM_COLOR: 'never' },
  })
  if (result.status === 0) return { exit: 0, stdout: result.stdout ?? '', stderr: '' }
  if (result.error) {
    return { exit: result.status ?? 1, stdout: result.stdout ?? '', stderr: result.error.message }
  }
  return { exit: result.status ?? 1, stdout: result.stdout ?? '', stderr: result.stderr ?? '' }
}

/** Run one cargo test invocation and index the named tests that actually passed. */
function cargoTests(args, label) {
  const run = runCapture('cargo', ['test', '--offline', ...args], workspace)
  const tests = new Set()
  for (const line of run.stdout.split('\n')) {
    const match = /^test (\S+) \.\.\. ok\b/.exec(line)
    if (match) tests.add(match[1])
  }
  const summary = /test result: ok\. (\d+) passed;/.exec(run.stdout)
  return {
    label,
    exit: run.exit,
    passed: run.exit === 0 && summary !== null,
    passedCount: summary === null ? null : Number(summary[1]),
    tests,
    stderr: run.stderr,
  }
}

let engineRun = null
let hostRun = null
const engine = () => (engineRun ??= cargoTests(['-p', 'archava-own'], 'archava-own engine'))
const host = () =>
  (hostRun ??= cargoTests(
    ['-p', 'archava-host', '--test', 'own', '--', '--test-threads=1'],
    'archava-host own suite',
  ))

/**
 * Require every named witness to be present AND the suite to have exited 0.
 * The returned names map is the details evidence; a missing name pushes a
 * problem so a green suite with a deleted witness cannot pass.
 */
function named(run, names, label, problems) {
  const seen = {}
  for (const name of names) {
    seen[name] = run.tests.has(name)
    if (!seen[name]) problems.push(`${label} did not run ${name}`)
  }
  if (!run.passed) {
    problems.push(
      `${label} exited ${run.exit} without a green test summary: ${run.stderr.slice(-400)}`,
    )
  }
  return { passed: run.passed && names.every((name) => seen[name]), seen }
}

const HOST = {
  pipeline: 'disabling_the_pipeline_refuses_the_backend_and_enabling_restores_it',
  duplicate: 'a_duplicate_submission_replays_one_receipt_and_one_effect',
  stale: 'a_stale_base_and_a_foreign_confirmation_refuse_without_writing',
  grant: 'a_missing_or_revoked_grant_refuses_the_operation',
  restore: 'restore_lands_as_a_new_revision_and_keeps_history_and_business_rows',
  mandatory: 'a_mandatory_module_is_blocked_and_nothing_is_written',
  racing: 'racing_applies_converge_to_one_revision_and_one_receipt',
  rebuild: 'a_rebuilt_host_recovers_the_same_receipt_and_history_from_postgres',
  chat: 'the_chat_plan_equals_the_dashboard_plan_and_applies',
  public: 'the_public_owner_surface_rejects_missing_and_forged_credentials',
  foreign: 'a_foreign_tenant_role_or_approval_is_refused_without_writing',
}

const ENGINE = {
  t01: 't01_relabelling_changes_the_label_but_not_the_canonical_stage_key',
  t02: 't02_the_same_typed_request_has_the_same_plan_identity',
  t03: 't03_the_plan_carries_readiness_and_cost_for_every_change',
  t05Dependency: 't05_a_missing_dependency_is_an_explicit_block_and_never_a_partial_apply',
  t05Entitlement: 't05_entitlement_install_and_readiness_are_distinct_refusals',
  t06Stale: 't06_a_stale_base_is_refused_and_demands_a_replan',
  t06Confirm: 't06_a_confirmation_for_another_plan_is_refused',
  t06Id: 't06_the_same_request_id_yields_one_effect_and_the_same_receipt',
  t07Dependency: 't07_disabling_a_dependency_of_an_enabled_module_is_blocked',
  t07Mandatory: 't07_a_mandatory_module_cannot_be_disabled',
  t08Restore: 't08_restore_creates_a_new_revision_and_never_rewrites_history',
  t08Revalidate: 't08_a_restore_is_revalidated_against_the_current_catalogue',
  t09: 't09_a_request_cannot_invent_readiness_cost_or_authority',
}

// --- Evidence sources ---------------------------------------------------------

/** Reuse a fresh K05/K04 proof from the release gate; run the producer if stale. */
async function ensureK05K04() {
  const current = head().head
  if (existsSync(k05k04Evidence)) {
    try {
      const proof = JSON.parse(readFileSync(k05k04Evidence, 'utf8'))
      if (proof.ok === true && proof.candidate?.head === current) return proof
    } catch {
      // A malformed proof is not evidence; fall through to a real run.
    }
  }
  const outcome = await runK05K04Consumers()
  mkdirSync(join(root, 'dist'), { recursive: true })
  writeFileSync(k05k04Evidence, `${JSON.stringify(outcome, null, 2)}\n`)
  return outcome
}

/** Read the browser proof and decide whether its owner journey is usable. */
function readBrowser() {
  if (!existsSync(browserEvidence)) {
    return { ok: false, fresh: false, proof: null, owner: null, problems: ['no browser proof'] }
  }
  let proof
  try {
    proof = JSON.parse(readFileSync(browserEvidence, 'utf8'))
  } catch (error) {
    return {
      ok: false,
      fresh: false,
      proof: null,
      owner: null,
      problems: [`the browser proof does not parse: ${error.message}`],
    }
  }
  const current = head().head
  const fresh = proof.identity?.head === current
  const owner = proof.details?.owner ?? null
  const stages = ['inspect', 'plan', 'preview', 'confirm', 'applied', 'refresh-recovery', 'history', 'restore']
  const problems = []
  if (!fresh)
    problems.push(
      `the browser proof is bound to ${proof.identity?.head ?? 'no head'}, not the candidate ${current}`,
    )
  if (proof.result !== 'pass') problems.push(`the browser proof result is ${proof.result ?? 'absent'}`)
  if (owner === null) problems.push('the browser proof carries no owner journey details')
  else {
    const missing = stages.filter((stage) => !(owner.stages ?? []).includes(stage))
    if (missing.length > 0) problems.push(`the owner journey did not reach: ${missing.join(', ')}`)
    if ((owner.appliedRevisions ?? []).join(',') !== '1,2,3,4')
      problems.push(`the owner journey applied ${(owner.appliedRevisions ?? []).join(',')}`)
    for (const flag of [
      'refreshRecovery',
      'historyRevision1',
      'pipelineDisabledRefused',
      'pipelineEnabledRestored',
    ]) {
      if (owner[flag] !== true) problems.push(`the owner journey did not record ${flag}`)
    }
    if (owner.restoredRevision !== 3) problems.push('the owner journey did not restore revision 1 as revision 3')
    if (owner.finalRevision !== 4) problems.push('the owner journey did not finish at revision 4')
    if (owner.auditActor !== 'alice' || owner.auditCount !== 4)
      problems.push('the owner journey did not record four applies by the browser owner')
  }
  return {
    ok: problems.length === 0,
    fresh,
    proof,
    owner,
    problems,
  }
}

let browserCache = null
/** Use a fresh browser proof when one exists; otherwise drive the browser once. */
function ensureBrowser() {
  if (browserCache !== null) return browserCache
  let evidence = readBrowser()
  if (!evidence.fresh) {
    const run = runCapture('node', ['tools/rust/browser-slice.mjs'], root)
    evidence = readBrowser()
    evidence.runExit = run.exit
    if (run.exit !== 0) evidence.problems.push(`the browser producer exited ${run.exit}`)
  }
  browserCache = evidence
  return evidence
}

// --- The ten proofs -----------------------------------------------------------

async function runT01() {
  const outcome = await ensureK05K04()
  const checks = outcome.checks ?? {}
  const browser = ensureBrowser()
  const problems = [...(outcome.problems ?? []), ...browser.problems]
  const grades = {
    'two business models on identical released core/CRM': Boolean(
      outcome.ok &&
        checks['K05 consumer assembled and verified']?.passed &&
        checks['K04 consumer assembled and verified']?.passed &&
        checks['identical core/CRM artifact hashes']?.passed,
    ),
    'owner config changes via UI only': Boolean(browser.ok && browser.owner?.uiOnly === true),
  }
  writeProof(
    'P-OWN-T01',
    {
      k05k04: {
        ok: outcome.ok,
        checks,
        crm: {
          K05: checks['K05 consumer assembled and verified']?.crm ?? null,
          K04: checks['K04 consumer assembled and verified']?.crm ?? null,
        },
      },
      browser: {
        head: browser.proof?.identity?.head ?? null,
        stages: browser.owner?.stages ?? [],
        uiOnly: browser.owner?.uiOnly ?? null,
      },
    },
    grades,
    problems,
  )
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT02() {
  const problems = []
  const hostResult = named(host(), [HOST.chat], 'host', problems)
  const engineResult = named(engine(), [ENGINE.t02], 'engine', problems)
  const grades = {
    'chat plan equals dashboard plan': hostResult.passed,
    'authoritative base/config/catalogue/pricing identity recorded':
      hostResult.passed && engineResult.passed,
  }
  writeProof(
    'P-OWN-T02',
    { host: hostResult.seen, engine: engineResult.seen },
    grades,
    problems,
  )
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT03() {
  const problems = []
  const hostResult = named(host(), [HOST.pipeline, HOST.chat], 'host', problems)
  const engineResult = named(engine(), [ENGINE.t03, ENGINE.t09], 'engine', problems)
  const grades = {
    'preview shows changes/dependencies/readiness/cost': hostResult.passed && engineResult.passed,
    'no hidden activation/payment/provider effect': hostResult.passed && engineResult.passed,
  }
  writeProof('P-OWN-T03', { host: hostResult.seen, engine: engineResult.seen }, grades, problems)
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT04() {
  const problems = []
  const hostResult = named(
    host(),
    [HOST.grant, HOST.public, HOST.foreign],
    'host',
    problems,
  )
  const grades = {
    'missing grant rejected': hostResult.passed,
    'public session rejected': hostResult.passed,
    'forged tenant/role/approval rejected': hostResult.passed,
    'revoked confirm/status/replay rejected': hostResult.passed,
  }
  writeProof('P-OWN-T04', { host: hostResult.seen }, grades, problems)
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT05() {
  const problems = []
  const hostResult = named(host(), [HOST.mandatory], 'host', problems)
  const engineResult = named(
    engine(),
    [ENGINE.t05Dependency, ENGINE.t05Entitlement, ENGINE.t07Mandatory],
    'engine',
    problems,
  )
  const grades = {
    'incompatible dependency yields blocked result': hostResult.passed && engineResult.passed,
    'blocked result never counted PASS': hostResult.passed && engineResult.passed,
  }
  writeProof('P-OWN-T05', { host: hostResult.seen, engine: engineResult.seen }, grades, problems)
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT06() {
  const problems = []
  const browser = ensureBrowser()
  const hostResult = named(host(), [HOST.duplicate, HOST.racing, HOST.rebuild], 'host', problems)
  const engineResult = named(engine(), [ENGINE.t06Id], 'engine', problems)
  problems.push(...browser.problems)
  const refreshRecovery = Boolean(browser.ok && browser.owner?.refreshRecovery === true)
  const grades = {
    'racing apply converges to one effect': hostResult.passed,
    // All three recovery paths named by the check: the duplicate request is
    // the lost response, the rebuilt host is the restart, and the browser's
    // owner journey is the refresh that recovers the committed revision.
    'refresh/lost response/restart recoverable':
      hostResult.passed && engineResult.passed && refreshRecovery,
  }
  writeProof(
    'P-OWN-T06',
    {
      host: hostResult.seen,
      engine: engineResult.seen,
      browser: { refreshRecovery, stages: browser.owner?.stages ?? [] },
    },
    grades,
    problems,
  )
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT07() {
  const problems = []
  const browser = ensureBrowser()
  const hostResult = named(host(), [HOST.pipeline], 'host', problems)
  const engineResult = named(
    engine(),
    [ENGINE.t07Dependency, ENGINE.t07Mandatory],
    'engine',
    problems,
  )
  const browserOk = Boolean(
    browser.ok &&
      browser.owner?.pipelineDisabledRefused === true &&
      browser.owner?.pipelineEnabledRestored === true,
  )
  const grades = {
    'enable/disable changes UI/API/tool availability': hostResult.passed && browserOk,
    'disabled direct API rejected': hostResult.passed,
    'dependent disable rejected': engineResult.passed,
  }
  writeProof(
    'P-OWN-T07',
    {
      host: hostResult.seen,
      engine: engineResult.seen,
      browser: {
        stages: browser.owner?.stages ?? [],
        pipelineDisabledRefused: browser.owner?.pipelineDisabledRefused ?? null,
        pipelineEnabledRestored: browser.owner?.pipelineEnabledRestored ?? null,
      },
    },
    grades,
    problems,
  )
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT08() {
  const problems = []
  const hostResult = named(host(), [HOST.restore, HOST.rebuild], 'host', problems)
  const engineResult = named(
    engine(),
    [ENGINE.t08Restore, ENGINE.t08Revalidate],
    'engine',
    problems,
  )
  const grades = {
    'records/audit/receipts/permissions preserved': hostResult.passed,
    'restore does not rewrite history': hostResult.passed && engineResult.passed,
  }
  writeProof('P-OWN-T08', { host: hostResult.seen, engine: engineResult.seen }, grades, problems)
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT09() {
  const problems = []
  const hostResult = named(host(), [HOST.stale, HOST.chat], 'host', problems)
  const engineResult = named(
    engine(),
    [ENGINE.t06Stale, ENGINE.t06Confirm, ENGINE.t09],
    'engine',
    problems,
  )
  const grades = {
    'stale snapshot forces replan': hostResult.passed && engineResult.passed,
    'changed targets force reconfirmation': hostResult.passed && engineResult.passed,
    'no invented price/readiness': hostResult.passed && engineResult.passed,
  }
  writeProof('P-OWN-T09', { host: hostResult.seen, engine: engineResult.seen }, grades, problems)
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

function runT10() {
  const problems = []
  // The step named "Owner browser journey" drives the browser itself. Reading
  // an earlier step's proof would make this a file check, not a journey.
  const run = runCapture('node', ['tools/rust/browser-slice.mjs'], root)
  if (run.exit !== 0) problems.push(`the browser producer exited ${run.exit}`)
  const browser = readBrowser()
  problems.push(...browser.problems)
  const journey = Boolean(browser.ok && run.exit === 0)
  const grades = {
    'inspect→plan→preview→confirm→applied→recovery/history/restore in real browser': journey,
    'no owner code/file/terminal steps': journey && browser.owner?.uiOnly === true,
  }
  writeProof(
    'P-OWN-T10',
    {
      browserExit: run.exit,
      head: browser.proof?.identity?.head ?? null,
      owner: browser.owner,
      stderrTail: run.stderr.slice(-400),
    },
    grades,
    problems,
  )
  return { ok: Object.values(grades).every(Boolean), grades, problems }
}

// ---------------------------------------------------------------------------

const RUNNERS = {
  'P-OWN-T01': runT01,
  'P-OWN-T02': runT02,
  'P-OWN-T03': runT03,
  'P-OWN-T04': runT04,
  'P-OWN-T05': runT05,
  'P-OWN-T06': runT06,
  'P-OWN-T07': runT07,
  'P-OWN-T08': runT08,
  'P-OWN-T09': runT09,
  'P-OWN-T10': runT10,
}

async function main() {
  const argv = process.argv.slice(2)
  const browserOnly = argv.includes('--browser-only')
  const only = argv.includes('--only') ? argv[argv.indexOf('--only') + 1] : null
  const ids = only !== null ? [only] : browserOnly ? ['P-OWN-T10'] : Object.keys(RUNNERS).slice(0, 9)
  const failures = []
  for (const proofId of ids) {
    if (!RUNNERS[proofId]) throw new Error(`unknown proof ${proofId}`)
    process.stdout.write(`\nOWN lane · ${proofId}\n`)
    const outcome = await RUNNERS[proofId]()
    if (outcome.ok) {
      process.stdout.write(
        `PASS ${proofId} — ${Object.keys(outcome.grades).length} check(s) graded true from named Rust/browser witnesses\n`,
      )
    } else {
      const failed = Object.entries(outcome.grades)
        .filter(([, value]) => !value)
        .map(([name]) => name)
      process.stderr.write(`FAIL ${proofId} — ${failed.join(', ')}\n`)
      failures.push(proofId)
    }
  }
  if (failures.length > 0) {
    process.stderr.write(`OWN lane: ${failures.length} failing proof(s)\n`)
    process.exit(1)
  }
  process.stdout.write(
    browserOnly ? 'PASS OWN-T10 browser journey\n' : 'PASS OWN-V1 acceptance family\n',
  )
}

if (import.meta.main) {
  await main()
}
