/**
 * A minimal Chrome DevTools Protocol driver, because chrome-devtools-mcp only
 * looks for /opt/google/chrome/chrome and this machine ships Brave.
 *
 * Speaks CDP over the global WebSocket that ships in Node 22+. Enough surface
 * for the E2E: navigate, evaluate JS, capture screenshots, and collect the
 * console + network traffic a page produces.
 *
 * Everything machine-specific is an environment variable, so the same harness
 * runs against Chrome on a machine that has it: `E2E_BROWSER` for the binary,
 * `E2E_PROFILE` for the scratch profile, `CDP_PORT` for the debug port.
 */

// Node globals this driver touches. ESLint's `no-undef` knows the language and
// nothing else, so a Node script reads as a page full of undefined names
// until they are named here.
/* global process, fetch, WebSocket, setTimeout, Buffer */

const DEBUG_PORT = Number(process.env.CDP_PORT ?? 9222)
const BROWSER = process.env.E2E_BROWSER ?? '/usr/bin/brave'
const PROFILE = process.env.E2E_PROFILE ?? '/tmp/archava-e2e/profile'

/** HTTP endpoints of every debuggable page target. */
async function targets() {
  const response = await fetch(`http://127.0.0.1:${DEBUG_PORT}/json`)
  if (!response.ok) throw new Error(`devtools http ${response.status}`)
  return /** @type {any[]} */ (await response.json())
}

/**
 * A page to attach to, opening one if the browser has none.
 *
 * A headless launch sometimes ends with no target at all even though the HTTP
 * endpoint answers, so `about:blank` is created rather than assumed.
 */
async function findPage() {
  const listed = await targets()
  const existing = listed.find((t) => t.type === 'page' && t.webSocketDebuggerUrl)
  if (existing !== undefined) return existing
  await fetch(`http://127.0.0.1:${DEBUG_PORT}/json/new?about:blank`, { method: 'PUT' })
  const created = await targets()
  const page = created.find((t) => t.type === 'page' && t.webSocketDebuggerUrl)
  if (page === undefined) throw new Error('no page target')
  return page
}

/** A live session with one page target. */
export class Session {
  /** @param {string} [url] Page to open; omit to attach to an existing tab. */
  static async open(url) {
    const page = await findPage()
    const session = new Session(page.webSocketDebuggerUrl)
    await session.#ready
    if (url !== undefined) await session.navigate(url)
    return session
  }

  #ws
  #next = 1
  #pending = new Map()
  #ready
  #listeners = []
  console_ = []
  network_ = []
  requests_ = new Map()

  /**
   * Subscribe to raw protocol events, additive to every recorded one.
   *
   * The recorded flags cover the traffic the fixed E2E reports. A proof that
   * must INTERCEPT network traffic (the lost-response scenario suspends a
   * response after the server has committed and fails it on the wire) needs
   * the event itself, and a listener is the narrowest surface for that. The
   * returned function unsubscribes.
   *
   * @param {(message: any) => void} listener
   * @returns {() => void}
   */
  on(listener) {
    this.#listeners.push(listener)
    return () => {
      const at = this.#listeners.indexOf(listener)
      if (at !== -1) this.#listeners.splice(at, 1)
    }
  }

  /** @param {string} endpoint */
  constructor(endpoint) {
    this.#ws = new WebSocket(endpoint)
    this.#ready = new Promise((resolve, reject) => {
      this.#ws.addEventListener('open', () => resolve(), { once: true })
      this.#ws.addEventListener('error', () => reject(new Error(`ws ${endpoint}`)), { once: true })
    })
    this.#ws.addEventListener('message', (event) => this.#onMessage(event))
  }

  #onMessage(event) {
    const message = JSON.parse(String(event.data))
    if (typeof message.id === 'number') {
      const settle = this.#pending.get(message.id)
      if (settle !== undefined) {
        this.#pending.delete(message.id)
        settle(message)
      }
      return
    }
    this.#record(message)
  }

  #record(message) {
    for (const listener of [...this.#listeners]) listener(message)
    const { method, params } = message
    if (method === 'Runtime.consoleAPICalled') {
      const text = (params.args ?? [])
        .map((arg) => arg.value ?? arg.description ?? arg.unserializableValue ?? '')
        .join(' ')
      this.console_.push({ kind: params.type, text })
      return
    }
    if (method === 'Runtime.exceptionThrown') {
      const detail = params.exceptionDetails
      this.console_.push({
        kind: 'exception',
        text: detail.exception?.description ?? detail.text,
      })
      return
    }
    if (method === 'Log.entryAdded') {
      this.console_.push({ kind: params.entry.level, text: params.entry.text })
      return
    }
    if (method === 'Network.requestWillBeSent') {
      this.requests_.set(params.requestId, params.request.url)
      this.network_.push({ phase: 'request', url: params.request.url })
      return
    }
    if (method === 'Network.responseReceived') {
      this.network_.push({
        phase: 'response',
        url: params.response.url,
        status: params.response.status,
      })
      return
    }
    if (method === 'Network.loadingFailed') {
      this.network_.push({
        phase: 'failed',
        url: this.requests_.get(params.requestId) ?? '?',
        error: params.errorText,
      })
    }
  }

  /** @param {string} method @param {any} [params] */
  send(method, params = {}) {
    const id = this.#next++
    return new Promise((resolve, reject) => {
      this.#pending.set(id, (message) => {
        if (message.error !== undefined) reject(new Error(`${method}: ${message.error.message}`))
        else resolve(message.result)
      })
      this.#ws.send(JSON.stringify({ id, method, params }))
    })
  }

  async navigate(url, timeout = 15000) {
    await this.send('Page.enable')
    await this.send('Runtime.enable')
    await this.send('Log.enable')
    await this.send('Network.enable')
    const loaded = new Promise((resolve) => {
      const listener = (event) => {
        const message = JSON.parse(String(event.data))
        if (message.method === 'Page.loadEventFired') {
          this.#ws.removeEventListener('message', listener)
          resolve()
        }
      }
      this.#ws.addEventListener('message', listener)
    })
    await this.send('Page.navigate', { url })
    await Promise.race([loaded, new Promise((r) => setTimeout(r, timeout))])
    // One macrotask, so a synchronous post-load boot can finish before we look.
    await this.evaluate('new Promise(r => setTimeout(r, 250))')
  }

  async evaluate(expression) {
    const result = await this.send('Runtime.evaluate', {
      expression,
      returnByValue: true,
      awaitPromise: true,
    })
    if (result.exceptionDetails !== undefined) {
      throw new Error(
        result.exceptionDetails.exception?.description ?? result.exceptionDetails.text,
      )
    }
    return result.result.value
  }

  async screenshot(path) {
    const shot = await this.send('Page.captureScreenshot', { format: 'png' })
    const { writeFile } = await import('node:fs/promises')
    await writeFile(path, Buffer.from(shot.data, 'base64'))
    return path
  }

  async close() {
    try {
      this.#ws.close()
    } catch {
      /* already gone */
    }
  }
}

/**
 * Launch a debuggable browser, or return `null` when one is already up.
 *
 * The argument is kept so a caller can name a binary per run; the environment
 * variable is read before it so a machine with no Brave can run the same
 * harness without editing it.
 */
export async function launchDebugBrowser(brave = BROWSER) {
  const { spawn } = await import('node:child_process')
  const existing = await targets().catch(() => [])
  if (existing.length > 0) return null
  const child = spawn(
    brave,
    [
      '--headless=new',
      '--no-sandbox',
      '--disable-gpu',
      '--remote-debugging-port=' + String(DEBUG_PORT),
      '--user-data-dir=' + PROFILE,
      'about:blank',
    ],
    { stdio: 'ignore', detached: true },
  )
  child.unref()
  for (let attempt = 0; attempt < 60; attempt += 1) {
    await new Promise((r) => setTimeout(r, 250))
    const list = await targets().catch(() => [])
    if (list.length > 0) return list
  }
  throw new Error('browser never exposed a devtools endpoint')
}
