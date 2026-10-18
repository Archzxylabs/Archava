/* global process */
/**
 * P-SDK-WIDGET producer: the public SDK/widget/page-awareness proof.
 *
 * ## Why this proof exists, and why it grades TypeScript code
 *
 * R09's completeness row names the `sdk`/`chat` browser surfaces (PRD_FULL_RUST
 * WEB-01/WEB-02, fabric row "R09 completeness"). `rust/crates/core/src/graph.rs`
 * records the disposition: the graph REDUCER is a Rust port (`archava-core`),
 * but the SDK-side hosting — the barrel `packages/sdk`, the widget shell
 * `packages/chat`, and the hosts that mount them — stays on the TypeScript
 * side of the R09 SDK/widget slice, exactly as it does today. This proof
 * exists to hold that surface to its contract, because a surface nobody
 * executes is exactly the "documented rather than proven" gap the R09 audits
 * rejected on other lanes.
 *
 * Three required checks, graded from this tree, for real:
 *
 * 1. **SDK builds without owner credentials/internal tools** — a structural
 *    scan over `packages/sdk/src` + `packages/chat/src`: every import
 *    specifier must be from the recorded allowlist (the SDK's own modules or
 *    the browser-safe `@archava/core` / `@archava/config(/pure)` /
 *    `@archava/assistant`), and there must be no `node:` import and no
 *    `process.env` credential read in the widget source. §24's rule —
 *    "nothing resembling a provider credential leaves the barrel" — as a
 *    census. Alongside it, the live consumer isolation proof
 *    (`tools/library/verify-consumers.mjs`): both reference hosts are built
 *    from PACKED TARBALLS in a scratch dir, and each host's own `verify.mjs`
 *    asserts its browser bundle carries no cwd `/packages/` input and no
 *    `node:` import — the "builds without internal tools" clause proven by a
 *    real consumer build, not a memo.
 * 2. **widget embed works in two host integrations** — the two consumer
 *    hosts (`hospitality`, `retail`) actually built, verified and reported
 *    DIFFERENT businesses (`check` text/brand/actions must vary — the packer
 *    already refuses identical ones), plus the studio host (`apps/web`) whose
 *    decline/decision suites exercise the real mounted shell.
 * 3. **page awareness tests pass** — named witnesses out of
 *    `packages/sdk/test/page.test.ts` (first observation = full description,
 *    second look = differences only, field names and never a value) and the
 *    host tests that drive observation through consent.
 *
 * Exit and output discipline matches `tools/rust/release-ops.mjs`: a failed
 * grade fails the step (the gate binds this check's records to this outcome),
 * and the details file is written even on failure.
 */
import { execFileSync } from 'node:child_process'
import { readdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '../..')
const failures = []
const note = (message) => {
  process.stderr.write(`  ✗ ${message}\n`)
  failures.push(message)
}

/**
 * Run one command to completion, capturing stdout/stderr even on failure (a
 * failing suite is exactly what a red run exists to show).
 */
function runCapture(command, args, cwd, timeoutMs = 900_000) {
  try {
    const stdout = execFileSync(command, args, {
      cwd,
      encoding: 'utf8',
      stdio: 'pipe',
      timeout: timeoutMs,
      maxBuffer: 256 * 1024 * 1024,
    })
    return { exit: 0, stdout, stderr: '' }
  } catch (error) {
    return {
      exit: error.status ?? 1,
      stdout: error.stdout ?? '',
      stderr: error.stderr ?? '',
    }
  }
}

// ---------------------------------------------------------------------------
// 1. The structural scan over the widget sources (§24 census).
// ---------------------------------------------------------------------------

/** The import specifiers a widget source is ever allowed to name. */
const ALLOWED_SPECIFIER =
  /^(?:\.\/[\w-]+\.js|@archava\/(?:core|assistant|config(?:\/pure)?))$/
/** Packages that hold server authority or owner tooling — never a widget input. */
const BANNED_SPECIFIER = /^@archava\/(?:crm|crm-pg|identity-pg|provider-runtime|providers)/

function tsSources(dir, out) {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name)
    if (entry.isDirectory()) tsSources(path, out)
    else if (/\.ts$/.test(entry.name)) out.push(path)
  }
  return out
}

const scanFindings = []
const specifiersObserved = []
const scanFiles = [
  ...tsSources(join(root, 'packages/sdk/src'), []),
  ...tsSources(join(root, 'packages/chat/src'), []),
]
const scan = { filesScanned: scanFiles.length, specifiers: [], findings: scanFindings }
for (const path of scanFiles) {
  // `page.ts` carries deliberate NUL separator constants; utf8 decode keeps
  // them and the regexes below read the real text content.
  const text = readFileSync(path, 'utf8')
  const relative = path.replace(`${root}/`, '')
  if (/node:\w/.test(text)) {
    scanFindings.push({ file: relative, finding: 'node: import in browser source' })
  }
  if (/process\.env(?!\.)/.test(text)) {
    scanFindings.push({ file: relative, finding: 'process.env read in widget source' })
  }
  for (const specifier of text.matchAll(/(?:from|import)\s*'([^']+)'/g)) {
    const name = specifier[1]
    if (!specifiersObserved.includes(name)) specifiersObserved.push(name)
    if (BANNED_SPECIFIER.test(name)) {
      scanFindings.push({ file: relative, finding: `server-authority import: ${name}` })
    } else if (!name.startsWith('.') && !ALLOWED_SPECIFIER.test(name)) {
      scanFindings.push({
        file: relative,
        finding: `specifier outside the recorded allowlist: ${name}`,
      })
    }
  }
}
scan.specifiers = specifiersObserved
let sdkBuildsUnprivileged = scanFiles.length > 0 && scanFindings.length === 0
if (!sdkBuildsUnprivileged) {
  for (const finding of scanFindings) note(`widget source scan: ${finding.file}: ${finding.finding}`)
}

// ---------------------------------------------------------------------------
// 2. The consumer isolation proof (two hosts built from packed tarballs).
// ---------------------------------------------------------------------------

let consumerRun = null
let consumerProof = null
{
  const started = runCapture('node', ['tools/library/verify-consumers.mjs'], root)
  consumerRun = { exit: started.exit, passLine: started.stdout.includes('PASS identical core artifact') }
  const proofPath = join(root, 'dist/library/consumer-proof.json')
  if (started.exit === 0) {
    try {
      consumerProof = JSON.parse(readFileSync(proofPath, 'utf8'))
    } catch (error) {
      note(`consumer proof unreadable: ${error.message}`)
    }
    if (consumerProof) {
      consumerRun.hosts = consumerProof.consumers.map((entry) => ({
        consumer: entry.consumer,
        releaseProfile: entry.releaseProfile,
        presenceBrowserIsolated: entry.presenceBrowserIsolated === true,
        text: entry.checks?.text ?? null,
        brand: entry.checks?.brand ?? null,
      }))
      if (!consumerProof.identicalArtifacts) {
        note('the consumer proof does not record identical core artifacts across hosts')
      }
      if (consumerProof.consumers?.length !== 2) {
        note(`expected two consumer host integrations, got ${consumerProof.consumers?.length}`)
      } else if (consumerProof.consumers[0].consumer === consumerProof.consumers[1].consumer) {
        note('the two consumer entries are the same host')
      }
    }
  } else {
    note(`tools/library/verify-consumers.mjs exited ${started.exit}: ${started.stderr.slice(-400)}`)
  }
}

// ---------------------------------------------------------------------------
// 3. The witness suites (vitest, JSON reporter, names matched to the manifest).
// ---------------------------------------------------------------------------

const suiteRun = runCapture('pnpm', [
  'exec',
  'vitest',
  'run',
  'packages/sdk',
  'packages/chat',
  'packages/core',
  'apps/web',
  '--reporter=json',
])
let suite = null
try {
  suite = JSON.parse(suiteRun.stdout.slice(suiteRun.stdout.indexOf('{')))
} catch {
  note(`vitest JSON output unparseable (exit ${suiteRun.exit}); stderr: ${suiteRun.stderr.slice(-300)}`)
}

/**
 * Witness matching: an `it` title is unique within its suite, so the fullName
 * containment is the same [suite, testName] pairing the Rust producers use.
 */
const gradedTests = suite
  ? suite.testResults.flatMap((entry) =>
      entry.assertionResults.map((assertion) => ({
        fullName: assertion.fullName,
        status: assertion.status,
        file: entry.name.replace(`${root}/`, ''),
      })),
    )
  : []
const witnessStatus = (title) => {
  const matches = gradedTests.filter((entry) => entry.fullName.includes(title))
  if (matches.length === 0) return 'NOT RUN'
  return matches.every((entry) => entry.status === 'passed') ? 'ok' : 'FAILED'
}

const WITNESSES = {
  // Page awareness: the translator's contract.
  'describes the page rather than reporting nothing changed': true,
  'reports nothing when nothing changed': true,
  'reports a route change once, with both halves of the move': true,
  'carries field names and never a value': true,
  // One-line install + observation discipline (the widget's door).
  'refuses a blank client, because there is nowhere to scope the read to': true,
  'refuses rather than guess when the host gave it no document': true,
  'reads the host document and applies what changed': true,
  'says nothing when the visitor said no': true,
  'records the graph shape and never a field value, when the visitor said yes': true,
  // The studio host (apps/web): the shell's refusals and shadow behaviour.
  'hears the refusal through the button the shell actually drew': true,
  'never executes what it declined': true,
  'off and shadow keep the visible answer while shadow records a candidat': true,
}
const witnesses = Object.fromEntries(
  Object.keys(WITNESSES).map((title) => [title, witnessStatus(title)]),
)
const pageWitnessTitles = [
  'describes the page rather than reporting nothing changed',
  'reports nothing when nothing changed',
  'reports a route change once, with both halves of the move',
  'carries field names and never a value',
]
for (const [title, status] of Object.entries(witnesses)) {
  if (status !== 'ok') {
    note(`witness "${title}" is ${status}${suite ? '' : ' (suite output missing)'}`)
  }
}

// Count the empty-suite failure mode as a failure too: 14 files / 208 tests
// ran when last measured for these four packages; a suite that silently
// collected nothing would read as green.
const filesRun = suite ? suite.testResults.length : 0
const totalTests = suite ? suite.numTotalTests : 0
if (suite && (suite.numFailedTests > 0 || totalTests < 208 || filesRun < 14)) {
  note(`widget suite incomplete: ${suite.numFailedTests} failed, ${filesRun} files, ${totalTests} tests`)
}

// ---------------------------------------------------------------------------
// The required-check grading.
// ---------------------------------------------------------------------------

const cliChecks = suite
  ? {
      sdkBuildsUnprivileged,
      widgetEmbedTwoHosts:
        consumerRun.exit === 0 &&
        consumerRun.hosts?.length === 2 &&
        consumerRun.hosts.every((host) => host.presenceBrowserIsolated) &&
        witnesses['hears the refusal through the button the shell actually drew'] === 'ok',
      pageAwareness: pageWitnessTitles.every((title) => witnesses[title] === 'ok'),
    }
  : { sdkBuildsUnprivileged, widgetEmbedTwoHosts: false, pageAwareness: false }

const details = {
  schemaVersion: 'archava-sdk-widget-proof/v1',
  producedBy: 'node tools/rust/sdk-widget.mjs',
  scan,
  consumers: consumerRun,
  suite: {
    exit: suiteRun.exit,
    files: filesRun,
    totalTests,
    failedTests: suite?.numFailedTests ?? null,
    witnesses,
  },
  grades: cliChecks,
}
const detailsPath = join(root, 'dist/rust-sdk-widget-proof.json')
writeFileSync(detailsPath, `${JSON.stringify(details, null, 2)}\n`)
process.stdout.write('  details → dist/rust-sdk-widget-proof.json\n')

if (failures.length > 0) {
  process.stderr.write(`PASS/FAIL summary: ${failures.length} failing grade(s)\n`)
  process.exit(1)
}
process.stdout.write(
  'PASS public SDK builds unprivileged; widget embed verified in two packed-tarball hosts plus the studio host; page-awareness witnesses ok\n',
)
