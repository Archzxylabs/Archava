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
  'session logout revokes server authority',
  'literal search and cursor pagination',
  'pending recovery identity survives reload and resolution',
  'contact record writes through the product UI',
  'assistant desk reads and executes through the host',
  'contact create through the product UI',
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
  logout_revokes_server_session: 'session logout revokes server authority',
  logout_blocks_old_credential: 'session logout revokes server authority',
  literal_search_is_encoded: 'literal search and cursor pagination',
  cursor_pagination_loads_later_rows: 'literal search and cursor pagination',
  record_page_reads_the_record: 'contact record writes through the product UI',
  record_refusal_rendered: 'contact record writes through the product UI',
  record_contact_update: 'contact record writes through the product UI',
  record_activity_add: 'contact record writes through the product UI',
  record_pipeline_create_and_move: 'contact record writes through the product UI',
  record_followup_complete: 'contact record writes through the product UI',
  desk_overview: 'assistant desk reads and executes through the host',
  desk_prepare_requires_confirmation: 'assistant desk reads and executes through the host',
  desk_completion_executes: 'assistant desk reads and executes through the host',
  desk_pipeline: 'assistant desk reads and executes through the host',
  contact_create_ui: 'contact create through the product UI',
  pending_marker_survives_reload: 'pending recovery identity survives reload and resolution',
  record_pending_marker_blocks_new_intent: 'pending recovery identity survives reload and resolution',
  pending_marker_cleared_after_resolution: 'pending recovery identity survives reload and resolution',
  storage_refusal_blocks_submission: 'pending recovery identity survives reload and resolution',
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

/** Set a `<select>`'s value and fire the events Leptos listens for. */
function selectOption(page, testid, value) {
  return page.evaluate(
    `(() => { const el = document.querySelector('[data-testid="${testid}"]');\n` +
      ` if (!el) throw new Error('missing [data-testid=${testid}]');\n` +
      ` el.value = ${JSON.stringify(value)};\n` +
      ` el.dispatchEvent(new Event('input', { bubbles: true }));\n` +
      ` el.dispatchEvent(new Event('change', { bubbles: true }));\n` +
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
/** Open a named contact's record screen through the row's own button. */
async function openNamedRecord(page, fullName) {
  const probe =
    `(() => { const rows = [...document.querySelectorAll('[data-testid="contact-row"]')];\n` +
    ` const row = rows.find((r) => r.textContent.includes(${JSON.stringify(fullName)}));\n` +
    ` if (!row) throw new Error('no contact row named '+${JSON.stringify(fullName)});\n` +
    ` const button = row.querySelector('[data-testid="contact-open"]');\n` +
    ` if (!button) throw new Error('the row carries no open button');\n` +
    ` button.click(); })()`
  await page.evaluate(probe)
  await waitText(page, 'record-name', (text) => text.includes(fullName))
}

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

    // 7. CSRF negative — a VALID session that submits without the header (and
    //    with a forged one) is refused by the host's gate, and the session
    //    SURVIVES both refusals.
    //
    //    The old witness signed out to drop the CSRF header and relied on
    //    the cookie staying valid. Signing out now really ends the session
    //    (C02: the server revokes it), so that shape is gone — and it was
    //    the shape the audit called out. The negative is now taken against a
    //    LIVE session with a missing or forged header, which is what the
    //    mandate asks for, and the session's survival is asserted.
    const csrfOutcomeRaw = await page.evaluate(
      `(async () => {
         const body = JSON.stringify({
           tenant: 'acme-retail',
           requestId: crypto.randomUUID(),
           command: {
             operation: 'followup.create',
             contactId: '${adaId}',
             contactRevision: 1,
             summary: 'Browser proof — csrf skip',
             dueAt: '2027-06-02T09:00:00Z',
           },
         });
         const without = await fetch('/crm/commands', {
           method: 'POST',
           credentials: 'include',
           headers: { 'content-type': 'application/json' },
           body,
         });
         const withoutBody = await without.json();
         const forged = await fetch('/crm/commands', {
           method: 'POST',
           credentials: 'include',
           headers: { 'content-type': 'application/json', 'x-archava-csrf': 'forged-csrf-value' },
           body,
         });
         const forgedBody = await forged.json();
         return JSON.stringify({
           without: without.status,
           withoutReason: withoutBody.reason ?? null,
           forged: forged.status,
           forgedReason: forgedBody.reason ?? null,
         });
       })()`,
    )
    const csrfOutcome = JSON.parse(csrfOutcomeRaw)
    if (csrfOutcome.without !== 403 || csrfOutcome.withoutReason !== 'csrf_required') {
      throw new Error(
        `the header-less submit was not refused by the CSRF gate: ${JSON.stringify(csrfOutcome)}`,
      )
    }
    if (csrfOutcome.forged !== 403 || csrfOutcome.forgedReason !== 'csrf_required') {
      throw new Error(
        `the forged-CSRF submit was not refused by the CSRF gate: ${JSON.stringify(csrfOutcome)}`,
      )
    }
    // The session is still a session: a refused mutation did not end it.
    const sessionSurvived = await page.evaluate(
      `(async () => { const response = await fetch('/contacts?tenant=acme-retail', { credentials: 'include' }); return String(response.status); })()`,
    )
    if (sessionSurvived !== '200') {
      throw new Error(
        `the session did not survive the refused submissions: HTTP ${sessionSurvived}`,
      )
    }
    witness(
      'csrf_negative',
      `the header-less and forged-header submits were both refused (csrf_required) and the session survived`,
    )
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

    // 11b. The pending marker SURVIVES a reload (C05). ────────────────────
    //      The audit reloaded the draft right here and watched the request
    //      id change to a fresh one, with the original nowhere in the tab,
    //      the URL or storage. The application now keeps an
    //      account/workspace-bound marker: the reload comes back with the
    //      ORIGINAL key, and a fresh key is refused while the outcome is
    //      unknown.
    const draftHref = await page.evaluate('location.href')
    await openPage(page, draftHref)
    const restoredKey = (
      await waitText(page, 'review-request-id', (text) => text.trim().length > 0)
    ).trim()
    if (restoredKey !== lostKey) {
      throw new Error(`the reload minted a new key: ${restoredKey} != ${lostKey}`)
    }
    const pendingBanner = await textOf(page, 'draft-pending-id')
    if (typeof pendingBanner !== 'string' || pendingBanner.trim() !== lostKey) {
      throw new Error(`the page did not show the pending marker: ${pendingBanner}`)
    }
    await click(page, 'draft-new-key')
    await waitText(page, 'draft-status', (text) => text.includes('unresolved'))
    const stillLost = (await textOf(page, 'review-request-id')).trim()
    if (stillLost !== lostKey) {
      throw new Error(`a fresh key was minted while the outcome was unknown: ${stillLost}`)
    }
    witness(
      'pending_marker_survives_reload',
      `the reload restored ${lostKey}; a fresh key was refused while the outcome was unknown`,
    )
    await shot(page, '13b-pending-restored')

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

    // 11c. Resolution clears the marker, so the workspace can move on. ─────
    await openPage(page, draftHref)
    const freshKey = (
      await waitText(page, 'review-request-id', (text) => text.trim().length > 0)
    ).trim()
    if (freshKey === lostKey || freshKey.length === 0) {
      throw new Error(`the resolved marker still owns the draft: ${freshKey}`)
    }
    const pendingGone = await page.evaluate(
      `document.querySelector('[data-testid="draft-pending"]') === null`,
    )
    if (!pendingGone) {
      throw new Error('the resolved marker still blocks the workspace')
    }
    witness(
      'pending_marker_cleared_after_resolution',
      `the committed receipt cleared the marker; the next draft minted ${freshKey.slice(0, 8)}…`,
    )

    // 11d. A browser that cannot record the marker refuses the send. ────────
    await page.evaluate(
      `(() => { window.__archavaSetItem = Storage.prototype.setItem;
                Storage.prototype.setItem = function () { throw new Error('storage refused'); }; })()`,
    )
    await fillDraft(page, 'Browser proof — storage refused', '2027-06-07T09:00:00Z')
    await click(page, 'draft-submit')
    await waitText(page, 'draft-status', (text) => text.includes('submission was not made'))
    await page.evaluate(
      `(() => { Storage.prototype.setItem = window.__archavaSetItem; })()`,
    )
    if (
      psql(
        `SELECT count(*) FROM archava_crm.followups WHERE tenant_id='acme-retail' AND summary='Browser proof — storage refused'`,
        port,
      ) !== '0'
    ) {
      throw new Error('a submission the browser could not record still reached the server')
    }
    witness(
      'storage_refusal_blocks_submission',
      'the refused storage write blocked the send: the host never saw the command',
    )

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

    // --- C02: the UI sign-out closes the SERVER session --------------------
    // The 19 Oktober audit reproduced the opposite of what a sign-out must
    // do: the marker said "signed out" while the same browser still read
    // twenty authorized contact rows. This journey asserts the real thing,
    // in the browser and against PostgreSQL:
    //   - the session ROW is gone — the server revoked it, which a page that
    //     only forgets its own state could never prove;
    //   - the same browser can no longer read the contacts.
    await signIn(page, 'alice', 'acme-retail')
    // The count, not a fixed number: every sign-in of this journey added a
    // row (only the sign-out removes one), so what matters is that the
    // presented session's row is GONE — exactly one fewer, not "none".
    const sessionsBefore = psql(
      "SELECT count(*) FROM archava_identity.sessions WHERE user_id = 'alice'",
      port,
    )
    if (Number(sessionsBefore) < 1) {
      throw new Error('the signed-in browser holds no session row at all')
    }
    await click(page, 'session-sign-out')
    const marker = await waitText(page, 'session-marker', (text) => text.includes('signed out'))
    const sessionsAfter = psql(
      "SELECT count(*) FROM archava_identity.sessions WHERE user_id = 'alice'",
      port,
    )
    if (Number(sessionsAfter) !== Number(sessionsBefore) - 1) {
      throw new Error(
        `the UI sign-out removed ${Number(sessionsBefore) - Number(sessionsAfter)} session row(s) instead of the presented one`,
      )
    }
    witness(
      'logout_revokes_server_session',
      `the UI sign-out deleted the presented session row (${sessionsBefore} → ${sessionsAfter}; marker: ${marker.trim()})`,
    )
    await shot(page, '15-signed-out')


    const afterSignOut = await page.evaluate(
      `(async () => { const response = await fetch('/contacts?tenant=acme-retail', ` +
        `{ credentials: 'include' }); return String(response.status); })()`,
    )
    if (afterSignOut !== '401') {
      throw new Error(
        `the signed-out browser still reached the contacts: HTTP ${afterSignOut} — the credential was not revoked`,
      )
    }
    witness(
      'logout_blocks_old_credential',
      `the signed-out browser's contact read was refused (HTTP ${afterSignOut})`,
    )

    // --- C06: literal search and real cursor pagination --------------------
    // The 19 Oktober audit typed `Ada&limit=1` into the filter: the page
    // concatenated it raw, so it became a different query and the screen
    // showed a row the literal search never returns. And with 26 rows in
    // the tenant the page could not move: it named a cursor in a sentence.
    // Both are asserted here, on screen.
    await signIn(page, 'alice', 'bulk-demo')

    // The audit's exact repro: a literal that is not a name matches nothing.
    await fill(page, 'contacts-search', 'Ada&limit=1')
    await click(page, 'contacts-refresh')
    await waitText(page, 'contacts-count', (text) => text.includes('0 contact(s)'))
    const literalRows = await count(page, 'contact-row')
    if (literalRows !== 0) {
      throw new Error(
        `the literal search "Ada&limit=1" returned ${literalRows} row(s) — the query string was not encoded`,
      )
    }

    // The positive case: the row whose name really contains `&` is found by
    // its literal name, and the plain "Ada Lovelace" row is not.
    await fill(page, 'contacts-search', 'Ada &')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, 1)
    const literalName = await textOf(page, 'contact-row')
    if (!literalName.includes('Ada & Lovelace Ltd')) {
      throw new Error(`the literal search found the wrong row: ${literalName}`)
    }
    witness(
      'literal_search_is_encoded',
      'the literal "Ada &" search returned only "Ada & Lovelace Ltd"; "Ada&limit=1" returned nothing',
    )
    await shot(page, '16-literal-search')

    // The pages move: 26 rows, a page of 20, and a next page that is
    // really the next rows.
    await fill(page, 'contacts-search', '')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, 20)
    const firstRow = await textOf(page, 'contact-row')
    const pageNote = await textOf(page, 'contacts-page-note')
    if (typeof pageNote !== 'string' || !pageNote.includes('a further page exists')) {
      throw new Error(`the page did not name a further page: ${pageNote}`)
    }
    await click(page, 'contacts-next-page')
    await waitRowCount(page, 6)
    const secondRow = await textOf(page, 'contact-row')
    if (secondRow === firstRow) {
      throw new Error('the next page showed the same first row — the cursor did not move')
    }
    await click(page, 'contacts-first-page')
    await waitRowCount(page, 20)
    const backToFirst = await textOf(page, 'contact-row')
    if (backToFirst !== firstRow) {
      throw new Error(
        `the first-page action did not return to the first page: ${backToFirst} != ${firstRow}`,
      )
    }
    witness(
      'cursor_pagination_loads_later_rows',
      'page 1 (20 rows) → next page (6 rows) → back to page 1, same first row',
    )
    await shot(page, '17-paginated')

    // --- C01: the record screen's real writes ------------------------------
    // The audit's C01: six of the seven baseline writes had no command AND no
    // product screen. The commands now exist; this journey drives the
    // product's own record page and asks PostgreSQL what each click did —
    // a rendered "Recorded" is not the proof.
    // "Ada Lovelace" is the 22nd bulk row — the SECOND page, which is
    // exactly why the filter (not the page position) opens it.
    const ADA_BULK_ID = '2c3e5a10-3333-4333-8333-000000000016'
    await fill(page, 'contacts-search', 'Ada Lovelace')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, 1)
    await openNamedRecord(page, 'Ada Lovelace')
    const recordName = await textOf(page, 'record-name')
    const recordId = (await textOf(page, 'record-id')).trim()
    if (!recordName.includes('Ada Lovelace') || recordId !== ADA_BULK_ID) {
      throw new Error(`the record page opened the wrong record: ${recordName} / ${recordId}`)
    }
    const openingRevision = Number(await textOf(page, 'record-revision'))
    if (openingRevision !== 1) {
      throw new Error(`the fresh record started at revision ${openingRevision}, not 1`)
    }
    witness(
      'record_page_reads_the_record',
      `the record screen read ${ADA_BULK_ID} from the host (revision 1)`,
    )

    // A refused write renders the HOST's reason, not a generic error.
    await fill(page, 'record-activity-summary', '   ')
    await click(page, 'record-activity-add')
    const refusal = await waitText(page, 'record-action', (text) => text.includes('Refused:'))
    if (!refusal.includes('invalid_submission')) {
      throw new Error(`the refused write did not carry the host reason: ${refusal}`)
    }
    witness(
      'record_refusal_rendered',
      'the empty activity was refused with the host’s invalid_submission reason',
    )

    // contact.update — the name and the revision move together.
    await fill(page, 'record-edit-name', 'Ada Lovelace (edited)')
    await click(page, 'record-save')
    await waitText(page, 'record-revision', (text) => text.trim() === '2')
    const editedName = await textOf(page, 'record-name')
    if (!editedName.includes('(edited)')) {
      throw new Error(`the record did not show the edited name: ${editedName}`)
    }
    const storedName = psql(
      `SELECT full_name FROM archava_crm.contacts WHERE tenant_id='bulk-demo' AND id='${ADA_BULK_ID}'`,
      port,
    )
    if (storedName !== 'Ada Lovelace (edited)') {
      throw new Error(`contact.update did not persist the name: ${storedName}`)
    }
    witness(
      'record_contact_update',
      `contact.update persisted the edited name and moved the revision to 2 (psql: ${storedName})`,
    )

    // activity.add — the row AND the contact's revision bump.
    await fill(page, 'record-activity-summary', 'Browser proof — record call')
    await selectOption(page, 'record-activity-kind', 'call')
    await click(page, 'record-activity-add')
    await waitText(page, 'record-activities', (text) => text.includes('record call'))
    await waitText(page, 'record-revision', (text) => text.trim() === '3')
    const activityRows = psql(
      `SELECT count(*) FROM archava_crm.activities WHERE tenant_id='bulk-demo' AND contact_id='${ADA_BULK_ID}' AND summary='Browser proof — record call'`,
      port,
    )
    if (activityRows !== '1') {
      throw new Error(`activity.add did not persist exactly one activity: ${activityRows}`)
    }
    witness(
      'record_activity_add',
      'activity.add persisted the call and bumped the contact to revision 3',
    )

    // opportunity.create — the pipeline row exists at `new`, revision 1.
    await fill(page, 'record-opportunity-title', 'Browser proof — deal')
    await click(page, 'record-opportunity-create')
    await waitText(page, 'record-opportunities', (text) => text.includes('Browser proof — deal'))
    const createdStage = psql(
      `SELECT stage || ':' || revision FROM archava_crm.opportunities WHERE tenant_id='bulk-demo' AND contact_id='${ADA_BULK_ID}' AND title='Browser proof — deal'`,
      port,
    )
    if (createdStage !== 'new:1') {
      throw new Error(`opportunity.create answered ${createdStage}, not new:1`)
    }

    // opportunity.move — qualified is a legal edge from new; the page shows
    // the new stage and the history table carries the edge.
    await selectOption(page, 'record-move-stage', 'qualified')
    await click(page, 'record-move')
    await waitText(page, 'record-opportunities', (text) => text.includes('stage qualified'))
    const movedStage = psql(
      `SELECT stage || ':' || revision FROM archava_crm.opportunities WHERE tenant_id='bulk-demo' AND title='Browser proof — deal'`,
      port,
    )
    if (movedStage !== 'qualified:2') {
      throw new Error(`opportunity.move answered ${movedStage}, not qualified:2`)
    }
    const edge = psql(
      `SELECT from_stage || '>' || to_stage || ':' || revision FROM archava_crm.opportunity_transitions WHERE tenant_id='bulk-demo' AND to_stage='qualified'`,
      port,
    )
    if (edge !== 'new>qualified:2') {
      throw new Error(`the pipeline history did not record the edge: ${edge}`)
    }
    // The record screen itself shows that history, read from the host — the
    // initial `new` row and the move, oldest first.
    const shownHistory = await textOf(page, 'record-opportunity-history')
    if (
      typeof shownHistory !== 'string' ||
      !shownHistory.includes('new') ||
      !shownHistory.includes('qualified') ||
      !shownHistory.includes('alice')
    ) {
      throw new Error(`the record screen did not render the stage history: ${shownHistory}`)
    }
    witness(
      'record_pipeline_create_and_move',
      'opportunity.create opened new:1, opportunity.move recorded new>qualified:2, and the screen rendered both history rows',
    )

    // followup.complete — a follow-up created through the draft screen is
    // closed from the record screen.
    await openPage(page, `${APP}/contacts`)
    await waitText(page, 'contacts-count', (text) => text.includes('contact(s)'))
    await fill(page, 'contacts-search', 'Ada Lovelace')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, 1)
    await draftContactNamed(page, 'Ada Lovelace')
    await fillDraft(page, 'Browser proof — record followup', '2027-08-01T09:00:00Z')
    await click(page, 'draft-submit')
    await waitText(page, 'draft-status', (text) => text.includes('Recorded. Receipt:'))
    await openPage(page, `${APP}/contact?id=${ADA_BULK_ID}`)
    await waitText(page, 'record-followup-complete', (text) => text.length >= 0)
    await click(page, 'record-followup-complete')
    await waitText(page, 'record-followups', (text) => text.includes('completed'))
    const completed = psql(
      `SELECT status || ':' || revision FROM archava_crm.followups WHERE tenant_id='bulk-demo' AND contact_id='${ADA_BULK_ID}' AND summary='Browser proof — record followup'`,
      port,
    )
    if (completed !== 'completed:2') {
      throw new Error(`followup.complete answered ${completed}, not completed:2`)
    }
    witness(
      'record_followup_complete',
      'followup.complete closed the drafted follow-up (psql: completed:2)',
    )
    await shot(page, '18-record-screen')

    // --- C03: the shipped owner-configuration journey ----------------------
    // These steps are NOT a coverage witness (the frozen P-RUST-BROWSER
    // checks do not include owner configuration); they are a real UI smoke
    // that aborts this proof if the shipped owner surface regresses. The
    // lifecycle's semantic evidence lives in `cargo test -p archava-host
    // --test own`.
    process.stdout.write('  · owner: disable crm-pipeline through /app/owner\n')
    await openPage(page, `${APP}/owner`)
    await waitText(page, 'owner-state', (value) => value.includes('Configuration revision'))
    await click(page, 'owner-toggle-crm-pipeline')
    await click(page, 'owner-preview')
    await waitText(page, 'owner-plan', (value) => value.includes('Disable crm-pipeline'))
    await click(page, 'owner-confirm')
    await waitText(page, 'owner-status', (value) => value.includes('apply committed revision 1'))
    await openPage(page, `${APP}/contact?id=${ADA_BULK_ID}`)
    await waitText(page, 'record-pipeline-disabled', (value) => value.length > 0)
    process.stdout.write('  · owner: disabled module hides the record pipeline\n')

    process.stdout.write('  · owner: re-enable and restore r1, then leave it enabled\n')
    await openPage(page, `${APP}/owner`)
    await waitText(page, 'owner-state', (value) => value.includes('Configuration revision 1'))
    await click(page, 'owner-toggle-crm-pipeline')
    await click(page, 'owner-preview')
    await waitText(page, 'owner-plan', (value) => value.includes('Enable crm-pipeline'))
    await click(page, 'owner-confirm')
    await waitText(page, 'owner-status', (value) => value.includes('apply committed revision 2'))
    await openPage(page, `${APP}/contact?id=${ADA_BULK_ID}`)
    await waitText(page, 'record-opportunities', (value) => value.includes('Browser proof — deal'))

    await openPage(page, `${APP}/owner`)
    await waitText(page, 'owner-state', (value) => value.includes('Configuration revision 2'))
    await waitText(page, 'owner-revision-1', () => true)
    await click(page, 'owner-restore-1')
    await waitText(page, 'owner-plan', (value) => value.includes('Restore revision 1'))
    await click(page, 'owner-confirm')
    await waitText(page, 'owner-status', (value) => value.includes('restore committed revision 3'))
    await openPage(page, `${APP}/contact?id=${ADA_BULK_ID}`)
    await waitText(page, 'record-pipeline-disabled', (value) => value.length > 0)

    await openPage(page, `${APP}/owner`)
    await waitText(page, 'owner-state', (value) => value.includes('Configuration revision 3'))
    await click(page, 'owner-toggle-crm-pipeline')
    await click(page, 'owner-preview')
    await waitText(page, 'owner-plan', (value) => value.includes('Enable crm-pipeline'))
    await click(page, 'owner-confirm')
    await waitText(page, 'owner-status', (value) => value.includes('apply committed revision 4'))
    const ownerRevisions = psql(
      "SELECT count(*) FROM archava_owner.config_revisions WHERE tenant_id='bulk-demo'",
      port,
    )
    if (ownerRevisions !== '4') {
      throw new Error(`the owner journey wrote ${ownerRevisions} revisions, not 4`)
    }
    process.stdout.write('  · owner: four persisted revisions, pipeline enabled again\n')
    await shot(page, '18b-owner-configuration')

    // --- C01: the assistant desk — prepare, confirm, execute ---------------
    // The audit said the assistant could only PREPARE: execution did not
    // exist. The desk asks the real conversation route, shows the typed
    // completion draft with `requiresApproval`, and writes only after an
    // explicit human confirmation — asserted against PostgreSQL on both sides
    // of the confirmation.
    await openPage(page, `${APP}/contacts`)
    await waitText(page, 'contacts-count', (text) => text.includes('contact(s)'))
    await fill(page, 'contacts-search', 'Ada Lovelace')
    await click(page, 'contacts-refresh')
    await waitRowCount(page, 1)
    await draftContactNamed(page, 'Ada Lovelace')
    await fillDraft(page, 'Browser proof — desk followup', '2027-09-01T09:00:00Z')
    await click(page, 'draft-submit')
    await waitText(page, 'draft-status', (text) => text.includes('Recorded. Receipt:'))

    await openPage(page, `${APP}/desk`)
    await waitText(page, 'desk-overview', (text) => text.length > 0)
    const overviewContacts = (await textOf(page, 'desk-overview-contacts')).trim()
    const storedContacts = psql(
      "SELECT count(*) FROM archava_crm.contacts WHERE tenant_id='bulk-demo'",
      port,
    )
    if (overviewContacts !== storedContacts) {
      throw new Error(
        `the desk overview said ${overviewContacts} contacts, PostgreSQL holds ${storedContacts}`,
      )
    }
    const deskSource = await textOf(page, 'desk-source')
    if (deskSource.trim() !== 'crm') {
      throw new Error(`the overview was not served from the CRM: ${deskSource}`)
    }
    const deskPlanner = (await textOf(page, 'desk-planner')).trim()
    if (deskPlanner !== 'deterministic-commands-v1 / archava-crm-local') {
      throw new Error(`the desk did not show the planner identity: ${deskPlanner}`)
    }
    witness(
      'desk_overview',
      `the desk read ${overviewContacts} contacts from the CRM (source crm, matches psql)`,
    )

    await click(page, 'desk-ask-worklist')
    await waitText(page, 'desk-worklist', (text) => text.includes('desk followup'))
    const preparedId = await page.evaluate(
      `document.querySelector('[data-testid="desk-prepare-complete"]').getAttribute('data-followup')`,
    )
    const preparedRow = psql(
      `SELECT status || ':' || revision FROM archava_crm.followups WHERE id='${preparedId}'`,
      port,
    )
    if (preparedRow !== 'open:1') {
      throw new Error(`the worklist offered a follow-up that is not open:1 — ${preparedRow}`)
    }
    await click(page, 'desk-prepare-complete')
    let prepared = null
    for (let waited = 0; waited < 80; waited += 1) {
      prepared = await textOf(page, 'desk-completion')
      if (typeof prepared === 'string') break
      await new Promise((resolve) => setTimeout(resolve, 250))
    }
    if (typeof prepared !== 'string') {
      // The panel's own words, not a bare timeout: a refusal, a different
      // data kind, or a request that never resolved are three different
      // bugs and must not look alike.
      const status = await textOf(page, 'desk-status')
      const reply = await textOf(page, 'desk-reply')
      const kind = await textOf(page, 'desk-data-kind')
      throw new Error(
        `the desk rendered no completion draft for ${preparedId} (db ${preparedRow}); ` +
          `status=${JSON.stringify(status)} kind=${JSON.stringify(kind)} reply=${JSON.stringify(reply)}`,
      )
    }
    if (!prepared.includes('requiresApproval')) {
      throw new Error(`the prepared completion did not ask for confirmation: ${prepared}`)
    }
    const deskFollowupId = await page.evaluate(
      `document.querySelector('[data-testid="desk-confirm-complete"]').getAttribute('data-followup')`,
    )
    const beforeConfirm = psql(
      `SELECT status || ':' || revision FROM archava_crm.followups WHERE id='${deskFollowupId}'`,
      port,
    )
    if (beforeConfirm !== 'open:1') {
      throw new Error(`preparing the completion already wrote: ${beforeConfirm}`)
    }
    witness(
      'desk_prepare_requires_confirmation',
      'the prepared completion carried requiresApproval and the follow-up stayed open:1',
    )

    await click(page, 'desk-confirm-complete')
    await waitText(page, 'desk-action', (text) => text.includes('Recorded. Receipt:'))
    await waitText(page, 'desk-worklist', (text) => !text.includes('desk followup'))
    const afterConfirm = psql(
      `SELECT status || ':' || revision FROM archava_crm.followups WHERE id='${deskFollowupId}'`,
      port,
    )
    if (afterConfirm !== 'completed:2') {
      throw new Error(`the confirmed completion did not commit: ${afterConfirm}`)
    }
    // One follow-up carries its CREATE and COMPLETE audit rows; the completion
    // is the row this step is about.
    const deskAudit = psql(
      `SELECT actor_id || ':' || operation FROM archava_crm.audit WHERE resource_id='${deskFollowupId}' AND operation='followup.complete'`,
      port,
    )
    if (deskAudit !== 'alice:followup.complete') {
      throw new Error(`the confirmed completion's audit row is ${deskAudit}`)
    }
    witness(
      'desk_completion_executes',
      'the human confirmation executed followup.complete (psql: completed:2, audit alice:followup.complete)',
    )

    await click(page, 'desk-ask-pipeline')
    await waitText(page, 'desk-pipeline', (text) => text.includes('Browser proof — deal'))
    const pipelineText = await textOf(page, 'desk-pipeline')
    if (!pipelineText.includes('qualified')) {
      throw new Error(`the pipeline reply did not carry the moved stage: ${pipelineText}`)
    }
    witness(
      'desk_pipeline',
      'the desk listed the pipeline with the opportunity moved to qualified',
    )
    await shot(page, '19-assistant-desk')

    // --- C01: contact.create through the visible form ----------------------
    // The command was already proven over HTTP; the audit's gap was the
    // PRODUCT surface. The form writes the same typed command, renders the
    // host's refusal for an invalid one, persists the row, and opens the new
    // record.
    await openPage(page, `${APP}/contacts`)
    await waitText(page, 'contacts-count', (text) => text.includes('contact(s)'))
    await fill(page, 'contact-new-email', 'new-contact@proof.test')
    await fill(page, 'contact-new-org', 'Proof Org')
    await click(page, 'contact-create')
    const createRefusal = await waitText(page, 'contact-create-status', (text) =>
      text.includes('Refused:'),
    )
    if (!createRefusal.includes('invalid_submission')) {
      throw new Error(`the empty contact was refused with the wrong reason: ${createRefusal}`)
    }
    await fill(page, 'contact-new-name', 'Browser proof — new contact')
    await click(page, 'contact-create')
    await waitText(page, 'contact-create-status', (text) =>
      text.includes('Recorded. Receipt:'),
    )
    const createdRow = psql(
      "SELECT full_name || ':' || revision || ':' || owner_id FROM archava_crm.contacts WHERE tenant_id='bulk-demo' AND full_name='Browser proof — new contact'",
      port,
    )
    if (createdRow !== 'Browser proof — new contact:1:alice') {
      throw new Error(`contact.create did not persist the row as alice's revision 1: ${createdRow}`)
    }
    await click(page, 'contact-create-open')
    await waitText(page, 'record-name', (text) => text.includes('Browser proof — new contact'))
    await waitText(page, 'record-revision', (text) => text.trim() === '1')
    witness(
      'contact_create_ui',
      'the visible form refused an invalid contact, then created "Browser proof — new contact" (psql: revision 1, owner alice) and the record screen read it',
    )
    await shot(page, '20-contact-create')

    // --- C05 extension: the record screen honours the app-owned marker -----
    // Every committing form writes the marker before the send; while one is
    // unresolved (here: the exact stored shape an unknown outcome leaves
    // behind) a NEW intent is refused and the agent is pointed at recovery.
    // Reads stay available, because a pending write is not a reason to hide
    // the record.
    const beforeBlocked = psql(
      `SELECT count(*) FROM archava_crm.activities WHERE tenant_id='bulk-demo' AND contact_id='${ADA_BULK_ID}'`,
      port,
    )
    await page.evaluate(
      `localStorage.setItem('archava.pending.bulk-demo', JSON.stringify({requestId:'00000000-0000-4000-8000-00000000c105', tenant:'bulk-demo', principal:'alice@archava.test', createdAt:new Date().toISOString()}))`,
    )
    await openPage(page, `${APP}/contact?id=${ADA_BULK_ID}`)
    await waitText(page, 'record-name', (text) => text.includes('Ada Lovelace'))
    const pendingNotice = await waitText(page, 'record-pending', (text) =>
      text.includes('00000000-0000-4000-8000-00000000c105'),
    )
    await fill(page, 'record-activity-summary', 'must not be written while pending')
    await click(page, 'record-activity-add')
    const blockedRefusal = await waitText(page, 'record-action', (text) =>
      text.includes('Refused:'),
    )
    if (!blockedRefusal.includes('pending_unresolved')) {
      throw new Error(`a new intent was not refused while a marker existed: ${blockedRefusal}`)
    }
    const afterBlocked = psql(
      `SELECT count(*) FROM archava_crm.activities WHERE tenant_id='bulk-demo' AND contact_id='${ADA_BULK_ID}'`,
      port,
    )
    if (afterBlocked !== beforeBlocked) {
      throw new Error(`the blocked write still reached PostgreSQL: ${beforeBlocked} → ${afterBlocked}`)
    }
    await page.evaluate(`localStorage.removeItem('archava.pending.bulk-demo')`)
    await openPage(page, `${APP}/contact?id=${ADA_BULK_ID}`)
    await waitText(page, 'record-name', (text) => text.includes('Ada Lovelace'))
    const clearedNotice = await textOf(page, 'record-pending')
    if (clearedNotice !== null) {
      throw new Error(`the record still claims an unresolved marker: ${clearedNotice}`)
    }
    witness(
      'record_pending_marker_blocks_new_intent',
      'the record screen read the app-owned marker, refused a new write with pending_unresolved (no PostgreSQL row), and unblocked after resolution',
    )

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
