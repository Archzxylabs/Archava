/* global process */
/**
 * P-CLI-OPERATIONS + P-UPGRADE-RESTORE-RUST + P-RELEASE-IDENTITY producer.
 *
 * ## Why a dedicated named-witness producer
 *
 * The three proofs answer three different questions about the same
 * `archava-ops` surface, and none of them can be honestly graded from a step
 * exit alone:
 *
 * 1. **P-CLI-OPERATIONS** — "operator commands replaced with Rust equiva-
 *    lents" is a claim about EXECUTION of the replacement table
 *    (`archava-ops runbook`), not about the table existing. Each row is run
 *    here and its exit recorded.
 * 2. **P-UPGRADE-RESTORE-RUST** — the five MIG rehearsals are a cargo test
 *    suite (`archava-ops --test upgrade_restore`) whose five named witnesses
 *    are graded from the suite's own output, the same pattern
 *    `tools/rust/pg-authority.mjs` uses for the authority suite. A rehearsal
 *    that is not a fixture-backed real run would be the dishonesty §1.4
 *    names for the browser proof, applied to the upgrade story.
 * 3. **P-RELEASE-IDENTITY** — the binding must be RECOMPUTED on this tree:
 *    the config source of truth's version against the dist profile's, every
 *    artifact digest freshly recomputed, every prior archive scanned for
 *    identity and overwrite integrity. `archava-ops verify-release` does the
 *    arithmetic; this producer records its output as evidence details.
 *
 * One producer because they share the same binary build and the same
 * fixture-free CLI runs; the mapping in `tools/rust/verify-release.mjs`
 * binds each proof's checks to this step.
 *
 * ## Exit and output discipline
 *
 * The gate binds the required checks to THIS step's outcome, so a failed
 * internal grade must fail the step: any missing witness, any non-ok CLI
 * row, any refused identity binding → exit 1, stderr names it. The details
 * JSON (`dist/rust-ops-proof.json`) is written even on failure so a red run
 * replaces stale green claims instead of leaving them readable as current.
 */
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '../..')
const workspace = join(root, 'rust')
const binary = join(workspace, 'target', 'debug', 'archava-ops')
const SUITE = 'upgrade_restore'

/** The frozen manifest's required checks → the suite tests that produce them. */
const WITNESSES = {
  'fresh install upgrade passes': [SUITE, 'fresh_install_upgrade_passes'],
  'prior-profile upgrade passes': [SUITE, 'prior_profile_upgrade_passes'],
  'backup/restore rehearsal passes': [SUITE, 'backup_restore_rehearsal_passes'],
  'rollback rehearsal passes': [SUITE, 'rollback_rehearsal_passes'],
  'fenced single-writer handover passes': [SUITE, 'fenced_single_writer_handover_passes'],
}

/**
 * Run the rehearsal suite, capturing output even when it fails (a failing
 * suite is the run this exists to grade; pg-authority carries the same note
 * — a non-zero exit must not swallow the witness lines into a throw).
 */
function runSuite(cargoArgs) {
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
  return { exitCode, tests }
}

/**
 * Run one CLI subcommand with a hard 120s ceiling and capture its JSON line.
 * `ok` is the execution's own verdict; `output` keeps the parsed record (or
 * the failure text) so the details file shows what actually happened.
 */
function runCli(cwd, args) {
  let output = { exit: null, stdout: '', failed: true }
  try {
    const stdout = execFileSync(binary, args, {
      cwd,
      encoding: 'utf8',
      stdio: 'pipe',
      timeout: 120_000,
      maxBuffer: 64 * 1024 * 1024,
      env: { ...process.env, ARCHAVA_OPS_ROOT: cwd },
    })
    output = { exit: 0, stdout, failed: false }
  } catch (error) {
    output = {
      exit: error.status ?? null,
      stdout: error.stdout ?? '',
      failed: true,
      stderr: error.stderr ?? '',
    }
  }
  let parsed = null
  try {
    parsed = JSON.parse(output.stdout.split('\n').find((line) => line.startsWith('{')) ?? '')
  } catch {
    parsed = null
  }
  return { args, exit: output.exit, failed: output.failed, record: parsed, stdout: output.stdout }
}

const cargoBuild = (args) => {
  try {
    execFileSync('cargo', args, { cwd: workspace, encoding: 'utf8', stdio: 'pipe' })
    return { exit: 0, stderr: '' }
  } catch (error) {
    return { exit: error.status ?? 1, stderr: error.stderr ?? '' }
  }
}

const failures = []
const note = (message) => {
  process.stderr.write(`  ✗ ${message}\n`)
  failures.push(message)
}

const details = {
  schemaVersion: 'archava-ops-proof/v1',
  producedBy: 'node tools/rust/release-ops.mjs',
  build: null,
  suite: null,
  cli: [],
  runbook: null,
  releaseIdentity: null,
}

// 1. The binary and the suite must BUILD (a replay of stale artifacts would
//    grade nothing). `--locked` keeps the rehearsal on the frozen registry.
const hyperBuild = cargoBuild([
  'build',
  '--offline',
  '--locked',
  '-p',
  'archava-ops',
  '--bin',
  'archava-ops',
])
if (hyperBuild.exit !== 0) {
  note(`archava-ops build failed: ${hyperBuild.stderr.slice(-400)}`)
}
details.build = { exit: hyperBuild.exit }

const suite = runSuite([
  'test',
  '--offline',
  '--locked',
  '-p',
  'archava-ops',
  '--test',
  SUITE,
  '--',
  '--test-threads=1',
])
details.suite = {
  exit: suite.exitCode,
  tests: Object.fromEntries([...suite.tests.entries()].map(([name, status]) => [name, status])),
  witnesses: Object.fromEntries(
    Object.entries(WITNESSES).map(([check, [suiteName, testName]]) => [
      check,
      { suite: suiteName, test: testName, status: suite.tests.get(testName) ?? 'NOT RUN' },
    ]),
  ),
}
if (suite.exitCode !== 0) note(`${SUITE} suite exited ${suite.exitCode}`)
for (const [check, [, testName]] of Object.entries(WITNESSES)) {
  if (suite.tests.get(testName) !== 'ok') note(`witness "${check}" (${testName}) is not ok`)
}

// 2. The runbook registry, driven row by row: five CLI rows through the
//    binary, the rehearsal row already executed above (same suite run).
let runbookRows = []
const runbookRun = runCli(root, ['runbook'])
const runbookRecord = runbookRun.record
details.runbook = {
  exit: runbookRun.exit,
  rows: runbookRecord?.ok ? runbookRecord.commands.map((row) => ({ command: row.command, kind: row.kind, replaces: row.replaces, proves: row.proves })) : null,
}
if (runbookRun.failed || !runbookRun.record?.ok) {
  note('archava-ops runbook failed to print its table')
} else {
  runbookRows = runbookRecord.commands
}

// The plan-assembly request is written from the config's own version — the
// request's profile read from the source of truth so the CLI probe cannot
// drift from what the tree declares.
const configVersion = JSON.parse(
  readFileSync(join(root, 'config/library-release.v1.json'), 'utf8'),
).version
const requestDir = mkdtempSync(join(tmpdir(), 'archava-ops-param-'))
const requestPath = join(requestDir, 'assembly-request.json')
writeFileSync(
  requestPath,
  JSON.stringify({
    version: '8.0.0',
    profile: configVersion,
    modules: {
      contacts: true,
      pipeline: true,
      assistant: true,
      nativeActions: true,
      commerce: true,
      purchasing: true,
    },
  }),
)

const cliCommands = [
  ['status', []],
  ['doctor', []],
  ['verify-release', []],
  ['plan-assembly', ['--request', requestPath]],
].map(([sub, args]) => ({ sub, args, run: runCli(root, [sub, ...args]) }))

for (const { sub, run: { failed, exit, record } } of cliCommands) {
  // The doctor keeps the TS doctor's output contract (`status: 'ready'`,
  // no `ok` key — field names deliberately match so consumers read either
  // implementation's JSON without a translation), so its green row is
  // graded on that shape.
  const ok = !failed && (record?.ok === true || record?.status === 'ready')
  details.cli.push({
    command: `archava-ops ${sub}`,
    exit,
    ok,
    record,
  })
  if (!ok) note(`archava-ops ${sub} did not run green`)
}
// The runbook row itself is a CLI row too — already executed above.
details.cli.push({
  command: 'archava-ops runbook',
  exit: runbookRun.exit,
  ok: !runbookRun.failed && runbookRun.record?.ok === true,
  record: runbookRun.record,
})
rmSync(requestDir, { recursive: true, force: true })

// 3. The release-identity binding from the CLI's own verify-release record.
const verifyRelease = details.cli.find((row) => row.command === 'archava-ops verify-release')?.record
const verification = verifyRelease?.verification
details.releaseIdentity = verification
  ? {
      configVersion: verification.config_version,
      profileVersion: verification.profile_version,
      versionMatches: verification.version_matches,
      artifactsVerified: verification.artifacts_verified,
      artifactDigests: (verification.artifacts ?? []).map((artifact) => ({
        name: artifact.name,
        version: artifact.version,
        bytes: artifact.bytes,
        sha256: artifact.digest_recomputed,
        recorded: artifact.digest_recorded,
        verified: artifact.verified,
      })),
      priorArchives: (verification.prior_archives ?? []).map((archive) => ({
        dir: archive.dir,
        version: archive.version,
        intact: archive.intact,
        profileDigest: archive.profile_digest,
        bytes: archive.bytes,
      })),
      priorsPreserved: verification.priors_preserved,
    }
  : null

const identity = details.releaseIdentity
let versionMatched = false
let digestsRecorded = false
let priorsIntact = false
if (identity) {
  versionMatched = identity.versionMatches === true && identity.configVersion === identity.profileVersion
  digestsRecorded = identity.artifactsVerified === true && identity.artifactDigests.length > 0
    && identity.artifactDigests.every((artifact) => artifact.verified === true && artifact.sha256.length === 64 && artifact.sha256 === artifact.recorded)
  priorsIntact = identity.priorsPreserved === true
    && identity.priorArchives.length > 0
    && identity.priorArchives.every((archive) => archive.intact === true && archive.version === archive.dir)
    && !identity.priorArchives.some((archive) => archive.dir === identity.configVersion)
}
if (!versionMatched) note('release profile version does not match the config source of truth')
if (!digestsRecorded) {
  note('artifact digests were not all recomputed and verified against the recorded values')
}
if (!priorsIntact) note('prior archives are not all intact (or a slot claims the current identity)')

// 4. The CLI-OPERATIONS grades.
const runbookKind = (kind) => runbookRows.filter((row) => row.kind === kind)
let commandsReplaced = false
let runbookExecuted = false
if (runbookRows.length > 0) {
  // "operator commands replaced": every row's replacement must name a real
  // first-party surface (a file that exists on this tree, or a documented
  // surface phrase) — a row replacing a fantasy path replaced nothing.
  const replacedPathsReal = runbookRows.every((row) => {
    const paths = row.replaces.match(/(?:apps|packages|tools|docs)\/[\w/.-]+/g) ?? []
    return paths.length === 0 || paths.every((path) => existsSync(join(root, path)))
  })
  // "runbook commands executed": every kind-'cli' row has a recorded exit-0
  // execution with ok:true in details.cli, and the kind-'cargo-test' rows
  // are covered by the suite run above.
  const cliRowCommands = runbookKind('cli').map((row) => row.command)
  const executedCommands = new Set(details.cli.filter((row) => row.ok).map((row) => row.command))
  const cargoRows = runbookKind('cargo-test')
  runbookExecuted =
    cliRowCommands.length > 0 &&
    cliRowCommands.every((command) => executedCommands.has(command)) &&
    cargoRows.every((row) => row.command.startsWith('cargo test -p archava-ops --test upgrade_restore')) &&
    suite.exitCode === 0
  commandsReplaced = replacedPathsReal && cliRowCommands.length > 0 && cargoRows.length > 0
}
if (!commandsReplaced) note('not every runbook row names a real first-party surface it replaces')
if (!runbookExecuted) note('not every runbook row was executed to exit 0 (CLI rows and the rehearsal suite)')
// "assembly/pack/verify CLI path passes": plan-assembly planned a full
// selection against the real catalogue; verify-release verified identity.
const planRecord = details.cli.find((row) => row.command === 'archava-ops plan-assembly')?.record
let cliPathPasses =
  planRecord?.ok === true &&
  planRecord?.planned === true &&
  planRecord?.authority === 'configuration-only' &&
  typeof planRecord?.installationCount === 'number' &&
  planRecord.installationCount > 0 &&
  versionMatched &&
  digestsRecorded
if (!cliPathPasses) note('assembly/pack/verify CLI path did not pass (plan and/or verify-release)')

// The details file: written even on failure (a red run replaces stale green).
mkdirSync(join(root, 'dist'), { recursive: true })
const detailsPath = join(root, 'dist/rust-ops-proof.json')
writeFileSync(detailsPath, `${JSON.stringify({ ...details, grades: { versionMatched, digestsRecorded, priorsIntact, commandsReplaced, runbookExecuted, cliPathPasses } }, null, 2)}\n`)

process.stdout.write(`  details → dist/rust-ops-proof.json\n`)
if (failures.length > 0) {
  process.stderr.write(`PASS/FAIL summary: ${failures.length} failing grade(s)\n`)
  process.exit(1)
}
process.stdout.write(
  'PASS Rust operational CLI path, upgrade/restore/rollback/fencing rehearsals and release-identity binding\n',
)
