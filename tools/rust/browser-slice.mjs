/* global process, URL, fetch, setTimeout */
/**
 * P-RUST-BROWSER producer: the §1.4B slice flow through the REAL browser UI.
 *
 * ## Why a browser, and why over CDP
 *
 * The slice's contract is the visible one: sign in on a rendered page, read a
 * contact table PostgreSQL answered for the session, review a typed command,
 * submit it, and recover its receipt after a refresh, a lost response, or a
 * service restart. HTTP calls alone prove the host's routes — they are already
 * covered by `rust/crates/host/tests/crm.rs` — but not that the SHIPPED
 * hydration entry, the session contract (an HttpOnly cookie, a CSRF record
 * only in storage), the refusal screens, or the SSR shell actually put those
 * routes in front of a person. This producer drives Brave headless over the
 * DevTools protocol the way `tools/e2e/crm.mjs` drives the TS platform's
 * proof, through the `data-testid` handles in the Leptos view tree, and never
 * calls the API from Node directly. The only Node-side HTTP is a `/healthz`
 * liveness poll; every business step happens in the page.
 *
 * ## What each required check maps to
 *
 * The checks ARE the frozen `P-RUST-BROWSER.requiredChecks`; each maps to the
 * page interactions below (`REQUIRED_CHECKS` and `WITNESS_CHECKS` keep the
 * manifest and this file tied together — drift in either direction is named
 * before any proof is claimed). Every negative is a REAL refusal the host's
 * routes render after a REAL page action — nothing is rewritten in flight:
 *
 * - `login SSR/hydration in real browser` — the SSR shell hydrates: the wasm
 *   bundle module-loads, `hydrate()` runs (the page stamps its own hydration
 *   timestamp), the login form accepts input on the prefilled tenant, and the
 *   session marker turns into `principal @ tenant`.
 * - `contacts/pipeline read` — the contact table renders the rows the
 *   session's own view admits (typed search narrows; refresh re-reads), served
 *   by the host's `GET /contacts` from a disposable PostgreSQL.
 * - `reviewed business command executed` — a `followup.create` draft
 *   (summary, due date, revision bind, minted request id) reviewed on the
 *   draft page and submitted through the CSRF-bound route. The same check
 *   carries the duplicate-submission witness: submitting AGAIN under the same
 *   key renders the replayed receipt, and `New attempt` mints a fresh key.
 * - `persisted effect verified in DB` — `psql` against the same fixture
 *   instance finds the followup row, the command row, and the audit row; the
 *   lost-response and restart flows get the same shadow checks, so receipts
 *   named "durable" are named against INNODB rows, not UI copy.
 * - `durable receipt readable` — the recovery page answers `committed` with
 *   the receipt for the minted request id.
 * - `authorized recovery after refresh` — a full document reload restores the
 *   session from cookie + storage facts alone and the same recovery page
 *   answers with the same receipt.
 * - `authorized recovery after lost response` — the CDP Fetch gate fails the
 *   POST's RESPONSE after Chromium has sent it (the server has committed), the
 *   draft panel reports the transport loss and names its request id, and the
 *   recovery page carries that same key to `committed` — whose effect row the
 *   psql check above also saw.
 * - `authorized recovery after restart` — the server process is killed with
 *   SIGKILL (its Drop never runs, so the data directory and postmaster
 *   survive), a second `slice_server` ATTACHES to that instance via
 *   `ARCHAVA_SLICE_ATTACH`, and the recovery page — on the same browser
 *   session — answers with both receipts recorded before the kill.
 * - `CSRF negative` — the signed-in operator's sign-out button clears the
 *   CSRF record while the HttpOnly cookie stays valid; the next submit
 *   through the same UI is refused `csrf_required` by the host's gate.
 * - `stale revision negative` — binding the draft to a revision the contact
 *   never had is refused `revision_changed` (409).
 * - `unauthenticated/forged negative` — three witnesses: a contact read with
 *   no session cookie at all is refused; an assigned-scope principal
 *   URL-forging a draft for a contact they cannot see is refused
 *   `unknown_resource`; a principal logging in against a tenant their grants
 *   do not reach is refused `role_not_allowed`.
 *
 * ## Lifecycle facts the producer owns
 *
 * - It BUILDS the thing it proves: server binary, wasm lib, and the bindgen
 *   `pkg/` bundle (both bound as artifacts — bytes the browser executed).
 * - It RUNS `slice_server` (fixture-only credentials; a disposable PostgreSQL
 *   from `pg-fixture`), and stops the attach server gracefully at the end so
 *   the fixture Drop stops PostgreSQL and removes the data directory: no
 *   leaked instances. The FIRST server is SIGKILLed by design — that is the
 *   restart witness — and on any FAILURE path the ending is graceful so a red
 *   run never leaks an instance either.
 * - The browser profile is fresh per run by `user-data-dir`, so the proof
 *   starts from cleared cookies and empty storage.
 * - On a mid-run failure the evidence file is still written: checks this run
 *   held pass, the rest read `fail: not reached`, and the script exits
 *   non-zero. A red run replaces stale green claims; it never writes less
 *   than what happened.
 */
import { spawn, spawnSync, execFileSync } from 'node:child_process'
import { createServer } from 'node:net'
import { existsSync, mkdirSync, rmSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

import { loadManifest } from './coverage.mjs'
import { writeEvidence } from './evidence.mjs'

const root = fileURLToPath(new URL('../..', import.meta.url))
const workspace = join(root, 'rust')

/** The hydration bundle both files the browser executes; bound as artifacts. */
const PKG_JS = 'rust/target/site/pkg/archava-slice.js'
const PKG_WASM = 'rust/target/site/pkg/archava-slice_bg.wasm'
const SERVER_BIN = join(workspace, 'target/release/examples/slice_server')

/** The evidence directory the shared writer uses. */
const EVIDENCE_DIR = 'dist/rust-evidence'

/** The proof drives the same port `slice_server` defaults to. */
const PORT = Number(process.env.SLICE_PORT ?? 8347)
const ORIGIN = `http://127.0.0.1:${PORT}`
const APP = `${ORIGIN}/app`

/** Scratch space: browser profile and screenshots, wiped at the start. */
const SCRATCH = '/tmp/opencode/archava-slice-proof'
const PROFILE = join(SCRATCH, 'browser-profile')

/** The fixture DB name the psql fact checks read. */
const DB = 'archava_test'

/** The fixture-only password; every fixture user carries one scrypt hash. */
const FIXTURE_PASSWORD = 'correct horse battery staple'

/**
 * The frozen manifest's `requiredChecks` for P-RUST-BROWSER, verified against
 * the live manifest at run time.
 */
const REQUIRED_CHECKS = [
  'login SSR/hydration in real browser',
  'contacts/pipeline read',
  'reviewed business command executed',
  'persisted effect verified in DB',
  'durable receipt readable',
  'authorized recovery after refresh',
  'authorized recovery after lost response',
  'authorized recovery after restart',
  'CSRF negative',
  'stale revision negative',
  'unauthenticated/forged negative',
]

/** The check each named witness backs. */
const WITNESS_CHECKS = {
  hydrated_login: 'login SSR/hydration in real browser',
  contacts_read: 'contacts/pipeline read',
  review_and_submit: 'reviewed business command executed',
  effect_in_db: 'persisted effect verified in DB',
  receipt_readable: 'durable receipt readable',
  recover_refresh: 'authorized recovery after refresh',
  recover_lost_response: 'authorized recovery after lost response',
  recover_restart: 'authorized recovery after restart',
  csrf_negative: 'CSRF negative',
  stale_revision: 'stale revision negative',
  unauthenticated: 'unauthenticated/forged negative',
  forged_scope: 'unauthenticated/forged negative',
  forged_tenant: 'unauthenticated/forged negative',
  duplicate_submission: 'reviewed business command executed',
}

const steps = []

/** Record one witness's passing line. */
function witness(witnessName, what) {
  const check = WITNESS_CHECKS[witnessName]
  if (check === undefined) throw new Error(`unknown witness ${witnessName}`)
  process.stdout.write(`  ✓ ${witnessName} — ${what} [${check}]\n`)
  steps.push({ witness: witnessName, check, what })
}

// --- Build -------------------------------------------------------------------

function runCommand(command, args, cwd) {
  const started = Date.now()
  const result = spawnSync(command, args, { cwd, encoding: 'utf8' })
  if (result.status !== 0) {
    const tail = (result.stderr ?? '').split('\n').slice(-8).join('\n')
    throw new Error(`${command} ${args.join(' ')} failed (${result.status}):\n${tail}`)
  }
  return Number(((Date.now() - started) / 1000).toFixed(1))
}

/**
 * Build server, wasm lib, and the hydration bundle. Unconditional: the proof
 * binds these artifact bytes, so a cached bundle from a stale tree would bind
 * the proof to nothing. Warm cycles cost seconds; the compile itself is the
 * artifact's provenance.
 */
function buildSliceArtifacts() {
  process.stdout.write('· building release server + wasm hydrate bundle\n')
  const spent = [
    runCommand('cargo', ['build', '--release', '-p', 'archava-slice', '--example', 'slice_server'], workspace),
    runCommand(
      'cargo',
      [
        'build',
        '--release',
        '-p',
        'archava-slice',
        '--lib',
        '--target',
        'wasm32-unknown-unknown',
        '--features',
        'hydrate',
      ],
      workspace,
    ),
    runCommand(
      join(process.env.HOME ?? '', '.cargo', 'bin', 'wasm-bindgen'),
      [
        '--out-dir',
        'target/site/pkg',
        '--out-name',
        'archava-slice',
        '--target',
        'web',
        '--no-typescript',
        'target/wasm32-unknown-unknown/release/archava_slice.wasm',
      ],
      workspace,
    ),
  ]
  return Number(spent.reduce((a, b) => a + b, 0).toFixed(1))
}

/** The bundle must exist before the browser is asked to hydrate it. */
function assertAssetsBuilt() {
  for (const artifact of [PKG_JS, PKG_WASM]) {
    if (!existsSync(join(root, artifact))) {
      throw new Error(`the build did not produce ${artifact}`)
    }
  }
}

// --- Server lifecycle ---------------------------------------------------------

/**
 * Start `slice_server` (provision mode, or attach when facts are given) and
 * return the child plus its captured stdout/stderr lines.
 */
function startServer(attachJson) {
  const child = spawn(SERVER_BIN, [], {
    cwd: workspace,
    env:
      attachJson === undefined
        ? { ...process.env }
        : { ...process.env, ARCHAVA_SLICE_ATTACH: attachJson },
    stdio: ['ignore', 'pipe', 'pipe'],
  })
  const log = []
  child.stdout.on('data', (chunk) => {
    for (const line of String(chunk).split('\n')) {
      if (line.length > 0) log.push(line)
    }
  })
  child.stderr.on('data', (chunk) => {
    for (const line of String(chunk).split('\n')) {
      if (line.length > 0) log.push(`stderr: ${line}`)
    }
  })
  return { child, log: () => log }
}

/** Parse the instance's attach facts out of the startup log, or throw. */
function parseAttachFacts(log) {
  const prefix = 'restart facts: ARCHAVA_SLICE_ATTACH='
  const line = log().find((candidate) => candidate.startsWith(prefix))
  if (line === undefined) {
    throw new Error(`slice_server printed no attach facts:\n${log().join('\n')}`)
  }
  const parsed = JSON.parse(line.slice(prefix.length))
  if (typeof parsed.port !== 'number' || typeof parsed.data !== 'string') {
    throw new Error(`the attach facts are not the documented shape: ${line}`)
  }
  return parsed
}

/** Poll `/healthz` until the server answers. */
async function waitHealthy(label, timeoutMs = 90_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    try {
      const response = await fetch(`${ORIGIN}/healthz`)
      if (response.ok) return
    } catch {
      /* the listener may not be up yet */
    }
    if (Date.now() > deadline) {
      throw new Error(`slice server never became healthy (${label})`)
    }
    await new Promise((resolve) => setTimeout(resolve, 300))
  }
}

/**
 * The port must be FREE before the proof starts: a leftover server answering
 * /healthz would let every later step pass against the WRONG process (and
 * its receipts would belong to a stranger's instance). Refuse with the one
 * command that clears it — never serve the proof silently next to a rival.
 */
async function assertPortFree() {
  try {
    const response = await fetch(`${ORIGIN}/healthz`)
    if (response.ok) {
      throw new Error(
        `something already answers ${ORIGIN}/healthz — a foreign or leftover ` +
          'slice server owns the port; end it before the proof runs (its ' +
          'receipts would otherwise be attributed to a stranger)',
      )
    }
  } catch (error) {
    if (error instanceof TypeError) return /* connection refused: free */
    throw error
  }
}

/**
 * Wait for OUR child's own ready line (the startup log is the child's
 * identity — a healthy /healthz can only be trusted once the child has
 * claimed the port in its own words), then the listener.
 */
async function waitServerReady(server, label, timeoutMs = 120_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    if (server.log().some((line) => line.startsWith('slice server ready'))) return
    if (Date.now() > deadline) {
      throw new Error(`the slice server never printed its ready line (${label}):\n${server.log().join('\n')}`)
    }
    await new Promise((resolve) => setTimeout(resolve, 300))
  }
}

/** Poll `/healthz` until the server stops answering (a kill or a stop). */
async function waitDead(label, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    let ok = false
    try {
      ok = (await fetch(`${ORIGIN}/healthz`)).ok
    } catch {
      /* refused — exactly what a stop means */
    }
    if (!ok) return
    if (Date.now() > deadline) throw new Error(`the stop never took effect (${label})`)
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
}

/** SIGTERM: the graceful end the fixture's Drop needs. */
function stopServer(server) {
  try {
    server.child.kill('SIGTERM')
  } catch {
    /* already gone */
  }
}

/** Where the instance's PostgreSQL answers (psql checks run against it). */
function instancePort(attachFacts) {
  if (attachFacts === null) throw new Error('instance facts not known yet')
  return attachFacts.port
}

// --- DB fact checks (psql, read-only, against the serving instance) -----------

function psql(sql, port) {
  return execFileSync(
    'psql',
    ['-h', '127.0.0.1', '-p', String(port), '-U', 'postgres', '-d', DB, '-Atc', sql],
    { encoding: 'utf8' },
  ).trim()
}

// --- Browser helpers ----------------------------------------------------------

/** Fill a `data-testid` input through the DOM the way typing lands. */
function fill(page, testid, value) {
  return page.evaluate(
    `(() => { const el = document.querySelector('[data-testid="${testid}"]');\n` +
      ` if (!el) throw new Error('missing [data-testid=${testid}]');\n` +
      ` el.value = ${JSON.stringify(value)};\n` +
      ` el.dispatchEvent(new Event('input', { bubbles: true }));\n` +
      ` return el.value; })()`,
  )
}

function click(page, testid) {
  return page.evaluate(
    `(() => { const el = document.querySelector('[data-testid="${testid}"]');\n` +
      ` if (!el) throw new Error('missing [data-testid=${testid}]');\n` +
      ` el.click(); })()`,
  )
}

function textOf(page, testid) {
  return page.evaluate(
    `document.querySelector('[data-testid="${testid}"]')?.textContent ?? null`,
  )
}

function valueOf(page, testid) {
  return page.evaluate(
    `document.querySelector('[data-testid="${testid}"]')?.value ?? null`,
  )
}

function count(page, testid) {
  return page.evaluate(
    `document.querySelectorAll('[data-testid="${testid}"]').length`,
  )
}

/** Poll until the table (or list) carries exactly `expected` rows. */
async function waitRowCount(page, expected, timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs
  let last = -1
  for (;;) {
    last = await count(page, 'contact-row')
    if (last === expected) return
    if (Date.now() > deadline) {
      throw new Error(`the row count never reached ${expected} — stuck at ${last}`)
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
}

/** Poll until the element's text draws the probe, then return the text. */
async function waitText(page, testid, probe, timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs
  let last = null
  for (;;) {
    last = await textOf(page, testid)
    if (typeof last === 'string' && probe(last)) return last
    if (Date.now() > deadline) {
      throw new Error(
        `[data-testid=${testid}] never matched within ${timeoutMs}ms — last text: ${JSON.stringify(last)}`,
      )
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
}

/** Poll an INPUT's value until the probe hits. */
async function waitValue(page, testid, probe, timeoutMs = 20_000) {
  const deadline = Date.now() + timeoutMs
  let last = null
  for (;;) {
    last = await valueOf(page, testid)
    if (typeof last === 'string' && probe(last)) return last
    if (Date.now() > deadline) {
      throw new Error(`[input data-testid=${testid}] never matched — last value: ${JSON.stringify(last)}`)
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
}

async function shot(page, name) {
  const dir = join(SCRATCH, 'screenshots')
  mkdirSync(dir, { recursive: true })
  return page.screenshot(join(dir, `${name}.png`))
}

/**
 * Navigate a FULL document load and wait for THIS load's hydration: each
 * document stamps `window.archavaSliceHydratedAt` with its own timestamp, so
 * a stale true cannot read as the new document's hydration. `page.navigate`
 * resolves on the load event; the wasm module is still booting after that.
 */
async function openPage(page, url) {
  const before = await page.evaluate(`window.archavaSliceHydratedAt ?? 0`)
  await page.navigate(url)
  await waitText(page, 'session-marker', () => true)
  const deadline = Date.now() + 30_000
  for (;;) {
    const stamp = await page.evaluate(`window.archavaSliceHydratedAt ?? 0`)
    if (typeof stamp === 'number' && stamp > 0 && stamp >= before) return
    if (Date.now() > deadline) {
      throw new Error(`hydration never completed for ${url} (stamp ${JSON.stringify(stamp)})`)
    }
    await new Promise((resolve) => setTimeout(resolve, 150))
  }
}

/** Complete the login form; resolve once the contacts page it opens is live.
 * A still-held previous session is signed out first through the marker's
 * native button (the reload restored its facts and the login screen
 * redirects held operators into the flow). */
async function signIn(page, principal, tenantName, { expectRows = true } = {}) {
  await openPage(page, `${APP}`)
  const marker = await textOf(page, 'session-marker')
  if (typeof marker === 'string' && !marker.includes('signed out')) {
    await click(page, 'session-sign-out')
    await waitText(page, 'session-marker', (text) => text.includes('signed out'))
    await openPage(page, `${APP}`)
  }  await waitText(page, 'session-marker', (text) => text.includes('signed out'))
  await fill(page, 'login-username', `${principal}@archava.test`)
  await fill(page, 'login-password', FIXTURE_PASSWORD)
  await fill(page, 'login-tenant', tenantName)
  await click(page, 'login-submit')
  // The SPA navigates to /app/contacts in the SAME document — same stamp —
  // and the contacts page's own effect loads the list for the new session.
  await waitText(page, 'contacts-heading', (text) => text.includes('Contacts'))
  const settled = await waitContactsSettled(page)
  if (expectRows && settled.rowCount === 0) {
    // A refusal that satisfied the settle (the row/waiter read the status)
    // must not masquerade as a signed-in list the next step can click.
    throw new Error(`sign-in landed on a refused contacts page: ${firstLine(settled?.status ?? '')}`)
  }
  return settled
}

/**
 * The contacts page settles in ONE of its states; wait until the status/count
 * pair names a row count or a refusal, whichever this session earns.
 */
async function waitContactsSettled(page, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs
  for (;;) {
    const status = await textOf(page, 'contacts-status')
    const rowCount = await count(page, 'contact-row')
    // `Loading…` is the page's initial render, not an answer: settle means
    // the list arrived or the host refused on the record. Anything else the
    // status shows while waiting keeps the wait going.
    const refused = typeof status === 'string' && status.startsWith('Refused:')
    if (rowCount > 0 || refused) {
      return { status, rowCount }
    }
    if (Date.now() > deadline) {
      throw new Error(`the contacts page never settled (status ${JSON.stringify(status)})`)
    }
    await new Promise((resolve) => setTimeout(resolve, 200))
  }
}

/** Click a named row's Draft button; resolve on the bound draft page with the
 * minted request id (`key`) and the revision the row is currently bound to
 * (`bound` — a colleague's write may have bumped it; the caller reads the
 * fact, it does not assume). */
async function draftContactNamed(page, fullName) {
  await clickNamedRow(page, fullName)
  const key = await waitText(page, 'review-request-id', (text) => text.trim().length > 0)
  const bound = await waitValue(page, 'draft-revision', (text) => text.trim().length > 0)
  return { key: key.trim(), bound }
}

/** Click a named row's Draft button. The lookup polls: the rows arrive from
 * the page's own post-hydrate effect, and a mid-reactive re-render must not
 * read as "no such contact" from a single probe. */
async function clickNamedRow(page, fullName) {
  const probe =
    `(() => { const rows = [...document.querySelectorAll('[data-testid="contact-row"]')];\n` +
    ` const row = rows.find((r) => r.textContent.includes(${JSON.stringify(fullName)}));\n` +
    ` if (!row) throw new Error('no contact row named '+${JSON.stringify(fullName)});\n` +
    ` const button = row.querySelector('[data-testid="contact-draft"]');\n` +
    ` if (!button) throw new Error('the row carries no draft button');\n` +
    ` button.click(); })()`
  const deadline = Date.now() + 15_000
  for (;;) {
    try {
      return await page.evaluate(probe)
    } catch (error) {
      if (Date.now() > deadline) throw error
      await new Promise((resolve) => setTimeout(resolve, 250))
    }
  }
}

/** Fill the followup fields (summary comes from the caller — it is the fact). */
async function fillDraft(page, summary, dueAt) {
  await fill(page, 'draft-summary', summary)
  await fill(page, 'draft-due', dueAt)
}

/**
 * Drive the recovery page for a request id and read the answer: the request
 * id echoed in its span, and the receipt out of the status panel (the span
 * names the REQUEST it recovered; the panel carries the receipt JSON).
 */
async function recoverVisible(page, requestId) {
  await openPage(page, `${APP}/recover?id=${requestId}`)
  let status
  try {
    status = await waitText(page, 'recover-status', (text) => text.includes('Committed'))
  } catch (error) {
    const url = await page.evaluate('location.href').catch(() => 'unknown-url')
    await shot(page, `debug-recover-fail-${requestId.slice(0, 8)}`)
    throw new Error(`recovery for ${requestId} did not commit — at ${url}: ${error.message}`)
  }
  const echoed = await textOf(page, 'recover-receipt-request-id')
  if (!echoed.includes(requestId)) {
    throw new Error(`the recovery panel did not name its request id: ${JSON.stringify(echoed)}`)
  }
  return { receiptId: extractReceiptId(status), status, echoed }
}

/** Receipt id out of `… {"id": "…", "kind": …}` panel text. */
function extractReceiptId(text) {
  const match = /"id"\s*:\s*"([^"]+)"/.exec(text)
  if (match === null) throw new Error(`no receipt id in: ${text}`)
  return match[1]
}

function firstLine(text) {
  return text.replace(/\s+/g, ' ').trim().slice(0, 110)
}

// --- Evidence -----------------------------------------------------------------

/**
 * Compose the evidence from what THIS run did. Witnessed checks pass; every
 * required check without a witness this run reads a named fail.
 */
function composeEvidenceSpec(error, facts) {
  const spec = loadManifest(root).proofs['P-RUST-BROWSER']
  const messages = error === undefined ? [] : [error.message]
  const checks = {}
  for (const check of REQUIRED_CHECKS) {
    if (steps.some((step) => step.check === check)) checks[check] = 'pass'
    else checks[check] = `fail: not reached${messages.at(-1) ?? ''}`
  }
  const witnesses = {}
  for (const step of steps) witnesses[step.witness] = (witnesses[step.witness] ?? 0) + 1
  return {
    proof: 'P-RUST-BROWSER',
    checks,
    sourcePaths: spec.sourcePaths,
    artifacts: [PKG_JS, PKG_WASM],
    details: {
      buildSeconds: facts.buildSeconds,
      serverPort: facts.port,
      instancePort: facts.instancePort,
      instanceDirectory: facts.instanceDir,
      witnesses,
      receipts: facts.receipts,
      screenshotDir: `${SCRATCH}/screenshots`,
      startedAt: facts.startedAt,
      failure: error === undefined ? null : error.message,
    },
    producer: 'tools/rust/browser-slice.mjs',
  }
}

// --- Main ---------------------------------------------------------------------

async function main() {
  // Fail the fast way if the producer and the manifest ever drift apart.
  const browserSpec = loadManifest(root).proofs['P-RUST-BROWSER']
  for (const check of REQUIRED_CHECKS) {
    if (!browserSpec.requiredChecks.includes(check)) {
      throw new Error(`manifest drift: "${check}" is no longer a P-RUST-BROWSER required check`)
    }
  }

  const startedAt = new Date().toISOString()
  const buildSeconds = buildSliceArtifacts()
  assertAssetsBuilt()

  rmSync(SCRATCH, { recursive: true, force: true })
  mkdirSync(SCRATCH, { recursive: true })

  // Nothing else may own the port: a leftover server would let every later
  // step pass against the WRONG process.
  await assertPortFree()
  const prime = startServer(undefined)
  let attachFacts = null
  let attached = null
  let page = null
  const receipts = []
  try {
    // The child's own ready line first (its claim on the port), then the
    // listener — so a slow provision can never be mistaken for an up server
    // this run did not start.
    await waitServerReady(prime, 'first boot')
    await waitHealthy('first boot')
    attachFacts = parseAttachFacts(prime.log)
    process.stdout.write(
      `· server up — fixture instance on port ${instancePort(attachFacts)} (${attachFacts.data})\n`,
    )

    // A reserved debug port so a leftover browser on 9222 cannot steal the run.
    const reservation = createServer()
    await new Promise((done, fail) => {
      reservation.once('error', fail)
      reservation.listen(0, '127.0.0.1', done)
    })
    const cdpPort = reservation.address().port
    await new Promise((done) => reservation.close(done))

    // Set BEFORE the dynamic import: cdp.mjs reads the env at module scope.
    process.env.CDP_PORT = String(cdpPort)
    process.env.E2E_PROFILE = PROFILE
    const { Session, launchDebugBrowser } = await import(join('..', 'e2e', 'cdp.mjs'))
    const launched = await launchDebugBrowser()
    if (launched === null) {
      process.stdout.write('· reusing the browser the environment already exposes\n')
    }
    page = await Session.open()
    await openPage(page, `${APP}`)
    const port = instancePort(attachFacts)

    // 1. Hydrated login. ──────────────────────────────────────────────────────
    const tenantPrefill = await valueOf(page, 'login-tenant')
    if (tenantPrefill !== 'acme-retail') {
      throw new Error(`the tenant field should prefill the checked choice: ${tenantPrefill}`)
    }
    await fill(page, 'login-username', 'alice@archava.test')
    await fill(page, 'login-password', FIXTURE_PASSWORD)
    await click(page, 'login-submit')
    await waitText(page, 'session-marker', (text) => text.includes('alice@archava.test @ acme-retail'))
    await waitText(page, 'contacts-heading', () => true)
    witness('hydrated_login', 'login form interactive after hydration; the SPA opened the contacts page')
    await shot(page, '01-logged-in')

    // 2. The contact read is the session's own view. ──────────────────────────
    const { rowCount: aliceRows } = await waitContactsSettled(page)
    if (aliceRows === 0) throw new Error('the contact table rendered no rows for alice')
    await fill(page, 'contacts-search', 'Ada')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, 1)
    const filtered = await count(page, 'contact-row')
    await fill(page, 'contacts-search', '')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, aliceRows)
    witness('contacts_read', `GET /contacts answered ${aliceRows} row(s); the typed filter narrowed to ${filtered}`)
    await shot(page, '02-contacts')

    // 3. Draft Ada's row → review → submit → recorded receipt. ────────────────
    const adaId = psql(
      `SELECT id FROM archava_crm.contacts WHERE tenant_id='acme-retail' AND full_name='Ada Lovelace'`,
      port,
    ).split('\n')[0]
    const { key: reviewKeyA } = await draftContactNamed(page, 'Ada Lovelace')
    await fillDraft(page, 'Browser proof — first flow', '2027-06-01T09:00:00Z')
    await shot(page, '03-review')
    await click(page, 'draft-submit')
    const recordedA = await waitText(page, 'draft-status', (text) => text.includes('Recorded. Receipt:'))
    const receiptA = { requestId: reviewKeyA.trim(), receiptId: extractReceiptId(recordedA) }
    receipts.push(receiptA)
    witness('review_and_submit', `followup.create recorded — ${receiptA.receiptId}`)
    await shot(page, '04-recorded')

    // 4. The effect is in the database, not just in the HTTP answer. ──────────
    const followupId = psql(
      `SELECT f.id FROM archava_crm.followups f ` +
        `JOIN archava_crm.contacts c ON c.tenant_id=f.tenant_id AND c.id=f.contact_id ` +
        `WHERE c.tenant_id='acme-retail' AND f.summary='Browser proof — first flow'`,
      port,
    )
    if (followupId.length === 0) throw new Error('the followup row never reached PostgreSQL')
    if (
      psql(
        `SELECT actor_id FROM archava_crm.commands ` +
          `WHERE tenant_id='acme-retail' AND request_id='${receiptA.requestId}'`,
        port,
      ) !== 'alice'
    ) {
      throw new Error(`no command row for ${receiptA.requestId}`)
    }
    if (
      psql(
        `SELECT count(*) FROM archava_crm.audit ` +
          `WHERE tenant_id='acme-retail' AND request_id='${receiptA.requestId}' AND operation='followup.create'`,
        port,
      ) !== '1'
    ) {
      throw new Error('no audit row for the recorded command')
    }
    witness('effect_in_db', `followup ${followupId} + command + audit rows verified by psql`)

    // 5. The receipt is readable through the recovery page. ──────────────────
    const recoverA = await recoverVisible(page, receiptA.requestId)
    if (recoverA.receiptId !== receiptA.receiptId) {
      throw new Error(
        `recovery named a different receipt: ${recoverA.receiptId} != ${receiptA.receiptId}`,
      )
    }
    witness('receipt_readable', 'recover answered Committed with the recorded receipt')
    await shot(page, '05-recovered')

    // 6. A full reload keeps the session; the same key reads the same receipt.
    await openPage(page, `${APP}`)
    await waitText(page, 'session-marker', (text) => text.includes('alice@archava.test @ acme-retail'))
    if ((await waitContactsSettled(page)).rowCount === 0) {
      throw new Error('post-refresh contacts page rendered no rows')
    }
    const recoverA2 = await recoverVisible(page, receiptA.requestId)
    if (recoverA2.receiptId !== receiptA.receiptId) {
      throw new Error(
        `post-refresh recovery named a different receipt: ${recoverA2.receiptId} != ${receiptA.receiptId}`,
      )
    }
    witness('recover_refresh', 'session and receipt both survive a full page reload')
    await shot(page, '06-refresh-recovered')

    // 7. CSRF negative — sign out keeps the cookie valid while the CSRF record
    //    dies; the next UI submit hits the host's gate. ──────────────────────
    await openPage(page, `${APP}/draft?contact=${adaId}&revision=1&name=Ada%20Lovelace`)
    await waitValue(page, 'draft-revision', (text) => text.trim() === '1')
    await click(page, 'session-sign-out')
    await waitText(page, 'session-marker', (text) => text.includes('signed out'))
    if ((await page.evaluate(`localStorage.getItem('archava_csrf')`)) !== null) {
      throw new Error('sign-out left a CSRF record behind')
    }
    await fillDraft(page, 'Browser proof — csrf skip', '2027-06-02T09:00:00Z')
    await click(page, 'draft-submit')
    const csrfRefusal = await waitText(page, 'draft-status', (text) => text.includes('csrf_required'))
    witness('csrf_negative', `the header-less submit was refused: ${firstLine(csrfRefusal)}`)
    await shot(page, '07-csrf-refusal')

    // 8. Stale revision negative — bind a revision the contact never had. ─────
    await signIn(page, 'alice', 'acme-retail')
    await draftContactNamed(page, 'Ada Lovelace')
    await fillDraft(page, 'Browser proof — stale bind', '2027-06-03T09:00:00Z')
    await fill(page, 'draft-revision', '9')
    await click(page, 'draft-submit')
    const staleRefusal = await waitText(
      page,
      'draft-status',
      (text) => text.includes('Conflict: the revision this draft bound to moved'),
    )
    witness('stale_revision', `the moved bind was refused: ${firstLine(staleRefusal)}`)
    await shot(page, '08-stale')

    // 9. Unauthenticated + forged negatives. ──────────────────────────────────
    // 9a. No session cookie at all — the storage record alone authorizes
    //     nothing, and the host decides.
    await page.send('Network.clearBrowserCookies')
    await openPage(page, `${APP}/contacts`)
    const unauthSettled = await waitContactsSettled(page)
    if (!unauthSettled.status.includes('Refused:')) {
      throw new Error(`the cookie-less read did not refuse: ${JSON.stringify(unauthSettled.status)}`)
    }
    witness('unauthenticated', `cookie-less contact read refused: ${firstLine(unauthSettled.status)}`)
    await shot(page, '09-unauthenticated')

    // 9b. Assigned-scope principal forging a URL the list never offered.
    await signIn(page, 'carol', 'acme-retail')
    if ((await count(page, 'contact-row')) === 0) {
      throw new Error('carol saw no rows — her own contacts should list')
    }
    await openPage(page, `${APP}/draft?contact=${adaId}&revision=1&name=Ada%20Lovelace`)
    await fillDraft(page, 'Browser proof — forged scope', '2027-06-04T09:00:00Z')
    await click(page, 'draft-submit')
    const scopeRefusal = await waitText(page, 'draft-status', (text) => text.includes('unknown_resource'))
    witness('forged_scope', `an invisible contact stays unfetchable: ${firstLine(scopeRefusal)}`)
    await shot(page, '10-forged-scope')

    // 9c. A principal whose grants do not reach the tenant they named.
    await signIn(page, 'another', 'acme-retail', { expectRows: false })
    const tenantSettled = await waitContactsSettled(page)
    if (!tenantSettled.status.includes('Refused:')) {
      throw new Error(`the cross-tenant read did not refuse: ${JSON.stringify(tenantSettled.status)}`)
    }
    witness('forged_tenant', `the cross-tenant read was refused: ${firstLine(tenantSettled.status)}`)
    await shot(page, '11-forged-tenant')

    // 10. Duplicate submission: same key replays; New attempt mints. ──────────
    await signIn(page, 'alice', 'acme-retail')
    const { key: reviewKeyB } = await draftContactNamed(page, 'Ada Lovelace')
    await fillDraft(page, 'Browser proof — duplicate', '2027-06-05T09:00:00Z')
    await click(page, 'draft-submit')
    const duplicateRecorded = await waitText(page, 'draft-status', (text) => text.includes('Recorded. Receipt:'))
    const receiptB = { requestId: reviewKeyB.trim(), receiptId: extractReceiptId(duplicateRecorded) }
    receipts.push(receiptB)
    await click(page, 'draft-submit') // the SAME key again: the wire replays
    const replayed = await waitText(
      page,
      'draft-status',
      (text) => text.includes('Replayed (the draft was already recorded)'),
    )
    if (!replayed.includes(receiptB.receiptId)) {
      throw new Error(`the replay did not carry the same receipt: ${replayed}`)
    }
    const keyBefore = await textOf(page, 'review-request-id')
    await putStateIdleByKey(page)
    const keyAfter = await textOf(page, 'review-request-id')
    if (keyAfter === keyBefore || keyAfter.trim().length === 0) {
      throw new Error(`New attempt did not mint a fresh key: ${keyAfter}`)
    }
    witness(
      'duplicate_submission',
      `the same-key resubmit replayed ${receiptB.receiptId}; New attempt minted a fresh key`,
    )
    await shot(page, '12-replay')

    // 11. Lost response — CDP fails the response after Chromium sent it; the
    //     server commits, the page sees the transport die. ────────────────────
    //     A FRESH bind first: step 10's commit bumped the contact's
    //     revision, and reusing the mounted draft would submit a stale bind
    //     the host refuses as revision_changed — a refusal would arrive and
    //     this scenario would prove the gate, not a lost commit.
    await openPage(page, `${APP}`)
    await waitText(page, 'session-marker', (text) => text.includes('alice@archava.test @ acme-retail'))
    const lostSettled = await waitContactsSettled(page)
    if (lostSettled.rowCount === 0) {
      throw new Error(`the lost-response flow needs the contacts list: ${firstLine(lostSettled.status ?? '')}`)
    }
    await draftContactNamed(page, 'Ada Lovelace')
    let paused = 0
    const unlisten = page.on((message) => {
      if (message.method === 'Fetch.requestPaused' && paused === 0) {
        paused += 1
        page
          .send('Fetch.failRequest', { requestId: message.params.requestId, errorReason: 'Aborted' })
          .catch(() => {})
      }
    })
    await page.send('Fetch.enable', {
      patterns: [{ urlPattern: `${ORIGIN}/crm/commands*`, requestStage: 'Response' }],
    })
    await fillDraft(page, 'Browser proof — lost response', '2027-06-06T09:00:00Z')
    const lostKey = (await textOf(page, 'review-request-id')).trim()
    await click(page, 'draft-submit')
    await waitText(page, 'unreachable-recover', () => true, 40_000)
    if (paused === 0) throw new Error('the response was never intercepted — the gate did not fire')
    await page.send('Fetch.disable')
    unlisten()
    const lostHref = await page.evaluate(
      `document.querySelector('[data-testid="unreachable-recover"]')?.getAttribute('href') ?? null`,
    )
    if (lostHref === null || !lostHref.includes(lostKey)) {
      throw new Error(`the unreachable panel does not name its request id: ${lostHref}`)
    }
    witness('recover_lost_response', 'the committed answer died on the wire — the panel names the recovery key')
    await shot(page, '13-lost-response')
    const lostRecover = await recoverVisible(page, lostKey)
    const lostReceiptId = lostRecover.receiptId
    receipts.push({ requestId: lostKey, receiptId: lostReceiptId, note: 'lost response' })
    if (
      psql(
        `SELECT count(*) FROM archava_crm.followups WHERE tenant_id='acme-retail' AND summary='Browser proof — lost response'`,
        port,
      ) !== '1'
    ) {
      throw new Error('the lost-response commit never persisted its followup')
    }

    // 12. Restart — SIGKILL the server (its Drop never runs; the data dir and
    //     the postmaster both survive), attach a new one, recover BOTH.
    prime.child.kill('SIGKILL')
    await waitDead('the restart kill')
    attached = startServer(joinAttach(attachFacts))
    await waitServerReady(attached, 'restart-attach', 30_000)
    await waitHealthy('restart-attach')
    for (const line of attached.log()) {
      if (line.startsWith('restart facts:')) {
        throw new Error('the attached server re-printed attach facts — it re-provisioned, which is wrong')
      }
    }
    process.stdout.write('· the attached restart is serving the surviving instance\n')
    await openPage(page, `${APP}`)
    await waitText(page, 'session-marker', (text) => text.includes('alice@archava.test @ acme-retail'))
    if ((await waitContactsSettled(page)).rowCount === 0) {
      throw new Error('post-restart contacts page rendered no rows')
    }
    const restartA = await recoverVisible(page, receiptA.requestId)
    if (restartA.receiptId !== receiptA.receiptId) {
      throw new Error(
        `post-restart recovery named a different receipt for A: ${restartA.receiptId} != ${receiptA.receiptId}`,
      )
    }
    const restartLost = await recoverVisible(page, lostKey)
    if (restartLost.receiptId !== lostReceiptId) {
      throw new Error(
        `post-restart recovery named a different receipt for the lost-response key: ${restartLost.receiptId} != ${lostReceiptId}`,
      )
    }
    witness(
      'recover_restart',
      'receipts recorded before the SIGKILL answer identically after the attach',
    )
    await shot(page, '14-restart-recovered')

    // Graceful stop of the attached server: its fixture owns the surviving
    // instance now; its Drop stops PostgreSQL and removes the directory.
    stopServer(attached)
    await waitDead('the attached stop')
    attached = null

    writeEvidence(
      root,
      composeEvidenceSpec(undefined, {
        buildSeconds,
        port: PORT,
        instancePort: attachFacts.port,
        instanceDir: attachFacts.data,
        receipts,
        startedAt,
      }),
    )
    process.stdout.write(`  evidence → ${EVIDENCE_DIR}/P-RUST-BROWSER.json (pass)\n`)
  } catch (error) {
    writeEvidence(
      root,
      composeEvidenceSpec(error, {
        buildSeconds,
        port: PORT,
        instancePort: attachFacts?.port ?? null,
        instanceDir: attachFacts?.data ?? null,
        receipts,
        startedAt,
      }),
    )
    process.stdout.write(`  evidence → ${EVIDENCE_DIR}/P-RUST-BROWSER.json (fail: ${error.message})\n`)
    throw error
  } finally {
    if (page !== null) await page.close().catch(() => {})
    if (attached !== null) {
      stopServer(attached)
      attached = null
    }
    // The prime died by SIGKILL in the restart flow, or is still up on a
    // failure path — either way end it GRACEFULLY so the fixture Drop stops
    // the disposable PostgreSQL (kill -9 in the restart step is the one
    // deliberate exception, and the attach server takes over its instance).
    stopServer(prime)
    try {
      await waitDead('the final stop', 30_000)
    } catch (stopError) {
      process.stdout.write(`  ! the final stop did not complete: ${stopError.message}\n`)
    }
    killBrowser()
  }
}

/** Clear the on-screen receipt/panel state and mint a fresh key in one go. */
function putStateIdleByKey(page) {
  return click(page, 'draft-new-key')
}

function joinAttach(attachFacts) {
  return JSON.stringify({ port: attachFacts.port, data: attachFacts.data })
}

function killBrowser() {
  const profile = process.env.E2E_PROFILE ?? PROFILE
  spawnSync('pkill', ['-f', `-- --user-data-dir=${profile.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`])
}

if (import.meta.main) {
  try {
    await main()
  } catch (error) {
    process.stdout.write(`✗ P-RUST-BROWSER — ${error.message}\n`)
    process.exitCode = 1
  }
}
