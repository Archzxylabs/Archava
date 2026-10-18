/* global process */
/**
 * P-REALTIME-VOICE producer: the voice/avatar realtime lifecycle proof.
 *
 * ## Why this proof exists, and what it does NOT claim
 *
 * R09's realtime row (`required-coverage.json` R09-REALTIME-VOICE-AVATAR) does
 * NOT claim a live voice stack in this repository. The live stack — Gemini
 * realtime voice, LiveKit, the Spatius renderer — is the STANDALONE template
 * (`Archava Template/`), kept deliberately separate by the security gate
 * (`tools/security/verify.mjs`'s `realtimeTemplate` limit, restated verbatim
 * in every PASS line: "realtime template remains separate"). What THIS tree
 * owns is the presence LIFECYCLE those stacks plug into:
 *
 * - `packages/presence` — the shared policy boundary: `transport: 'chat' |
 *   'synthetic-voice-transcript'`, the 1024-session gateway, replay/revision
 *   refusals, bounded timeouts, owner/guest projection. The vendor bridge's
 *   contract is "no live media in the lifecycle" — a synthetic transcript is
 *   the only voice-shaped input, which is exactly what keeps a third-party
 *   media stack OUT of reached code.
 * - `packages/adapters` — the vendor-adapter SEAM: `PRESENCE_MODES`
 *   ('human','voice','chat'), the published fallback order, the four
 *   documented preflight checks, and the renderer rule that the provider id
 *   ("Spatius") must not be hardcoded outside the adapter.
 *
 * Three required checks, graded from this tree, for real:
 *
 * 1. **realtime lifecycle tests pass** — named witnesses out of
 *    `packages/presence/test/gateway.test.ts`: same implementation for chat
 *    and synthetic voice, authority/schema/replay/revision refusals,
 *    cross-tenant projection, honest timeout with a retained busy lane, and
 *    no fabricated data on unavailability.
 * 2. **documented vendor bridge recorded** — the bridge is RECORDED in four
 *    places and this scan quotes them byte-level: PRD_FULL_RUST AI-04
 *    ("Vendor JS SDK bridge boleh menjadi documented exception yang diuji"
 *    with the session-establishment/mute/cleanup/fallback inventory), the
 *    security gate's `realtimeTemplate` limit, the template's own operator
 *    README (`AVATAR_PROVIDER=voice`, "Tidak ada fallback diam-diam" — no
 *    silent mid-session provider changes), and the migration inventory's
 *    Python-template disposition. If someone rewords any of these, the
 *    witness fails loudly rather than the bridge drifting silent.
 * 3. **Chat fallback works** — named witnesses out of
 *    `packages/adapters/test/presence.test.ts`: the fallback order is real
 *    (voice before chat, never upward, chat as the floor), a failed
 *    embodiment is skipped in favour of the next candidate, and the
 *    requested mode's own disqualification details are carried through.
 *
 * Exit and output discipline matches the sibling producers (`release-ops.mjs`,
 * `sdk-widget.mjs`): a failed grade fails the step, and the details file is
 * written even on failure.
 */
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '../..')
const failures = []
const note = (message) => {
  process.stderr.write(`  ✗ ${message}\n`)
  failures.push(message)
}

function runCapture(command, args, cwd, timeoutMs = 600_000) {
  try {
    const stdout = execFileSync(command, args, {
      cwd,
      encoding: 'utf8',
      stdio: 'pipe',
      timeout: timeoutMs,
      maxBuffer: 128 * 1024 * 1024,
    })
    return { exit: 0, stdout, stderr: '' }
  } catch (error) {
    return { exit: error.status ?? 1, stdout: error.stdout ?? '', stderr: error.stderr ?? '' }
  }
}

// ---------------------------------------------------------------------------
// 1. The lifecycle + fallback witness suites (one vitest run, JSON reporter).
// ---------------------------------------------------------------------------

const suiteRun = runCapture('pnpm', [
  'exec',
  'vitest',
  'run',
  'packages/presence',
  'packages/adapters',
  '--reporter=json',
])
let suite = null
try {
  suite = JSON.parse(suiteRun.stdout.slice(suiteRun.stdout.indexOf('{')))
} catch {
  note(`vitest JSON output unparseable (exit ${suiteRun.exit}); stderr: ${suiteRun.stderr.slice(-300)}`)
}

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

const LIFECYCLE_WITNESSES = [
  'uses the same implementation for chat and synthetic voice with exact request identities and bounded tools',
  'refuses caller authority, extra fields, malformed schema, private tools and forged guest bindings before execution',
  'refuses replay, expired sessions, unknown sections and stale/context-injected revisions',
  'rechecks owner projection and refuses other owner sessions and cross-tenant scope',
  'times out honestly, aborts the executor and retains a busy lane until late work settles',
  'reports unavailable without fabricated data or success and refuses unsafe public projection',
]
const FALLBACK_WITNESSES = [
  'falls back to chat when human has no embodiment installed',
  'falls back to voice before chat',
  'skips an embodiment that failed and tries the next candidate',
  'never falls back upward',
  'lands on chat when the requested and every richer mode fail',
  'always ends on a mode from the published order',
]
const rendererContract = ['never carries a credential in its session handle']

const witnesses = Object.fromEntries(
  [...LIFECYCLE_WITNESSES, ...FALLBACK_WITNESSES, ...rendererContract].map((title) => [
    title,
    witnessStatus(title),
  ]),
)
for (const [title, status] of Object.entries(witnesses)) {
  if (status !== 'ok') note(`witness "${title}" is ${status}`)
}

// ---------------------------------------------------------------------------
// 2. The documented vendor bridge, quoted byte-level.
// ---------------------------------------------------------------------------

const bridge = { prd: null, gate: null, template: null, inventory: null }
let documentedBridge = true

{
  const prd = readFileSync(join(root, 'docs/PRD_FULL_RUST.md'), 'utf8')
  const ai04Line = prd
    .split('\n')
    .find((line) => line.startsWith('**AI-04:** Voice/avatar migration'))
  const tokens = [
    'session establishment',
    'media permission handling',
    'mute/interrupt/disconnect',
    'vendor timeouts',
    'Chat fallback',
    'Vendor JS SDK bridge',
    'documented exception yang diuji',
    'speech codec/avatar renderer',
  ]
  bridge.prd = { line: ai04Line ? 'PRD_FULL_RUST.md' : null, tokens: {} }
  if (!ai04Line) {
    note('the AI-04 vendor-bridge row is missing from docs/PRD_FULL_RUST.md')
    documentedBridge = false
  } else {
    for (const token of tokens) {
      const present = ai04Line.includes(token)
      bridge.prd.tokens[token] = present
      if (!present) {
        note(`the AI-04 bridge row no longer names "${token}"`)
        documentedBridge = false
      }
    }
  }
}

{
  const gate = readFileSync(join(root, 'tools/security/verify.mjs'), 'utf8')
  const realtimeLimit = gate.includes('realtimeTemplate:')
  const separateLine = gate.includes('realtime template remains separate.')
  bridge.gate = { realtimeLimit, separateLine }
  if (!realtimeLimit) note("the security gate no longer carries the 'realtimeTemplate' limit")
  if (!separateLine) {
    note("the security gate no longer states 'realtime template remains separate'")
  }
  if (!realtimeLimit || !separateLine) documentedBridge = false
}

{
  const readmePath = join(root, 'Archava Template/README.md')
  const readmeExists = existsSync(readmePath)
  const readme = readmeExists ? readFileSync(readmePath, 'utf8') : ''
  const tokens = {
    voiceProviderSetWithoutRenderer: readme.includes('AVATAR_PROVIDER=voice'),
    livekitNamed: readme.includes('LiveKit'),
    spatiusNamed: readme.includes('Spatius'),
    noSilentProviderChange: readme.includes('Tidak ada fallback diam-diam'),
  }
  const docs = ['INTEGRATION.md', 'LATENCY.md', 'GUARDRAILS.md'].map((name) =>
    join(root, 'Archava Template/docs', name),
  )
  const docsPresent = docs.every((path) => existsSync(path))
  bridge.template = { readmePresent: readmeExists, tokens: {}, operatorDocsPresent: docsPresent }
  if (!readmeExists) {
    note('the Archava Template README is missing — the vendor bridge surface is unrecorded')
    documentedBridge = false
  } else {
    for (const [name, present] of Object.entries(tokens)) {
      bridge.template.tokens[name] = present
      if (!present) {
        note(`the template README no longer documents "${name}"`)
        documentedBridge = false
      }
    }
    if (!docsPresent) {
      note(
        'the template operator docs ' +
          docs.map((path) => path.replace(`${root}/`, '')).join(', ') +
          ' are missing',
      )
      documentedBridge = false
    }
  }
}

{
  let inventory = null
  try {
    inventory = JSON.parse(readFileSync(join(root, 'docs/rust/migration-inventory.json'), 'utf8'))
  } catch (error) {
    note(`migration inventory unreadable for the template disposition: ${error.message}`)
  }
  if (inventory) {
    const languageIsPython = inventory.template?.language === 'python'
    const entrypoints = Array.isArray(inventory.template?.backendEntrypoints)
      ? inventory.template.backendEntrypoints.length
      : 0
    bridge.inventory = { language: inventory.template?.language ?? null, backendEntrypoints: entrypoints }
    if (!languageIsPython || entrypoints === 0) {
      note(
        `the migration inventory's template disposition is incomplete ` +
          `(language=${inventory.template?.language}, entrypoints=${entrypoints})`,
      )
      documentedBridge = false
    }
  } else {
    documentedBridge = false
  }
}

// ---------------------------------------------------------------------------
// 3. The fallback seam itself must still exist in the adapter source.
// ---------------------------------------------------------------------------

const adaptersSource = readFileSync(join(root, 'packages/adapters/src/presence.ts'), 'utf8')
const seam = {
  modes: adaptersSource.includes("PRESENCE_MODES = ['human', 'voice', 'chat']"),
  fallbackOrder: adaptersSource.includes('PRESENCE_FALLBACK_ORDER'),
  preflightChecks: adaptersSource.includes('PREFLIGHT_CHECKS'),
}
{
  const rendererSource = readFileSync(
    join(root, 'packages/adapters/src/renderer.ts'),
    'utf8',
  )
    .replace(/[\n\r] */g, ' ')
    .replace(/\*\s+/g, '')
  seam.providerIdNotHardcoded =
    rendererSource.includes('Spatius must not be hardcoded outside the provider adapter')
  if (!seam.providerIdNotHardcoded) {
    note('the renderer no longer carries the provider-adapter isolation rule')
    documentedBridge = false
  }
  for (const [name, present] of Object.entries(seam)) {
    if (name === 'providerIdNotHardcoded') continue
    if (!present) {
      note(`the adapters presence seam no longer records ${name}`)
      documentedBridge = false
    }
  }
}

// ---------------------------------------------------------------------------

const allWitnessesOk = Object.values(witnesses).every((status) => status === 'ok')
const details = {
  schemaVersion: 'archava-realtime-voice-proof/v1',
  producedBy: 'node tools/rust/realtime-voice.mjs',
  suite: {
    exit: suiteRun.exit,
    files: suite?.testResults.length ?? 0,
    totalTests: suite?.numTotalTests ?? 0,
    failedTests: suite?.numFailedTests ?? null,
    witnesses,
  },
  bridge,
  seam,
  grades: {
    realtimeLifecycle: allWitnessesOk &&
      LIFECYCLE_WITNESSES.every((title) => witnesses[title] === 'ok'),
    documentedVendorBridge: documentedBridge,
    chatFallbackWorks: FALLBACK_WITNESSES.every((title) => witnesses[title] === 'ok') &&
      seam.modes &&
      seam.fallbackOrder,
  },
}
details.grades.realtimeLifecycle =
  LIFECYCLE_WITNESSES.every((title) => witnesses[title] === 'ok') && details.grades.realtimeLifecycle
const detailsPath = join(root, 'dist/rust-realtime-voice-proof.json')
writeFileSync(detailsPath, `${JSON.stringify(details, null, 2)}\n`)
process.stdout.write('  details → dist/rust-realtime-voice-proof.json\n')

if (failures.length > 0) {
  process.stderr.write(`PASS/FAIL summary: ${failures.length} failing grade(s)\n`)
  process.exit(1)
}
process.stdout.write(
  'PASS presence lifecycle witnesses ok; vendor bridge recorded verbatim in four places; chat fallback witnesses ok\n',
)
