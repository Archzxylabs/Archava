/**
 * `apps/web`'s server: pages in, bundles out, one API route that prices.
 *
 * The shape is a single tsx-run process that compiles the browser bundles with
 * esbuild on the way past. Three modes, because "serve the source" and "serve a
 * build" are different jobs with different failure modes:
 *
 * - `dev` — compile on demand, keep the bundles in memory, log where to look.
 *   A dev server that writes `dist/` makes a directory nobody meant to keep.
 * - `build` — write `dist/` and exit. Nothing runs, because a build that starts
 *   serving is a deploy nobody asked for.
 * - `prod` — serve `dist/` and the API. Refuses to serve anything else.
 *
 * Browser consumers use explicit pure configuration exports. The build needs
 * no workspace aliases or private source paths.
 */
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'node:http'
import { readFile } from 'node:fs/promises'
import { dirname, join, resolve, sep } from 'node:path'
import { fileURLToPath } from 'node:url'
import { build, type BuildOptions } from 'esbuild'
import { answerQuote } from './api.js'

const appDir = dirname(fileURLToPath(import.meta.url))
/**
 * The app root, which every path in this file is measured from.
 *
 * This file is two directories below the app it belongs to — `src/server`, not
 * `src` — so a path built off `appDir` lands one or two levels adrift. Naming
 * the root once is the difference between one place to get it right and three.
 */
const webDir = join(appDir, '..', '..')
const distDir = join(webDir, 'dist')
const pagesDir = join(webDir, 'pages')
const publicDir = join(webDir, 'public')

const PORT = Number(process.env['PORT'] ?? 4173)
const HOST = process.env['HOST'] ?? '127.0.0.1'

type Mode = 'dev' | 'prod' | 'build'

const MODES: readonly Mode[] = ['dev', 'prod', 'build']

function parseMode(argv: readonly string[]): Mode {
  const flag = argv.find((arg) => arg.startsWith('--mode='))
  if (flag === undefined) return 'dev'
  const value = flag.slice('--mode='.length)
  const found = MODES.find((mode) => mode === value)
  if (found === undefined) throw new Error(`unknown mode: ${value} (expected ${MODES.join(', ')})`)
  return found
}

/** Browser entries, and the names they are served under. */
const ENTRIES: readonly { readonly from: string; readonly name: string }[] = [
  { from: '../main.ts', name: 'app' },
  { from: '../studio.ts', name: 'studio' },
  { from: '../frontend/marketing.ts', name: 'marketing' },
  { from: '../frontend/workspace.ts', name: 'workspace' },
]

/** Every page this app serves, and the file behind it. */
const PAGES: Readonly<Record<string, string>> = {
  '/': 'index.html',
  '/studio': 'studio.html',
  '/archava': 'archava.html',
  '/workspace': 'workspace.html',
}

const TYPES: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.woff2': 'font/woff2',
  '.woff': 'font/woff',
  '.ttf': 'font/ttf',
  '.webp': 'image/webp',
  '.txt': 'text/plain; charset=utf-8',
}

/**
 * Options every browser build shares.
 *
 * The entry is a parameter because it is the thing being built. A defaulted
 * entry — `ENTRIES[0]` — would compile the first bundle twice and quietly never
 * build the rest, and `dev`'s spread-then-override would hide it: the same
 * function answering for both paths is the only way one of them can fail alone.
 */
function buildOptions(from: string, outfile: string): BuildOptions {
  return {
    entryPoints: [join(appDir, from)],
    bundle: true,
    format: 'esm',
    platform: 'browser',
    target: 'es2022',
    outfile,
    logLevel: 'warning',
  }
}

/** What `dev` keeps in memory, keyed by the bundle's served name. */
const memory = new Map<string, string>()

async function main(): Promise<void> {
  const mode = parseMode(process.argv.slice(2))

  if (mode === 'build') {
    for (const entry of ENTRIES) {
      await build(buildOptions(entry.from, join(distDir, `${entry.name}.js`)))
      process.stdout.write(`built ${entry.name}.js\n`)
    }
    return
  }

  if (mode === 'dev') {
    for (const entry of ENTRIES) {
      const result = await build({
        ...buildOptions(entry.from, '/dev/null'),
        write: false,
      })
      const file = result.outputFiles[0]
      if (file !== undefined) memory.set(entry.name, file.text)
    }
    process.stdout.write(`compiled ${ENTRIES.length} bundles into memory\n`)
  }

  await serve()
}

async function serve(): Promise<void> {
  const server: Server = createServer((request, response) => {
    route(request, response).catch((error: unknown) => {
      response.statusCode = 500
      response.end(error instanceof Error ? error.message : String(error))
    })
  })

  await new Promise<void>((resolve, reject) => {
    server.once('error', reject)
    server.listen(PORT, HOST, () => {
      resolve()
    })
  })

  process.stdout.write(
    `listening on http://${HOST}:${PORT} (studio: http://${HOST}:${PORT}/studio)\n`,
  )

  for (const signal of ['SIGINT', 'SIGTERM'] as const) {
    process.once(signal, () => {
      server.close(() => {
        process.exit(0)
      })
    })
  }
}

/**
 * Start, and say so if starting failed.
 *
 * `main` is the last thing the module does rather than something exported, so
 * that running the file is running the server — a server file with an uninvoked
 * `main` is a file that starts nothing. The rejection handler is here rather
 * than a top-level `await` because a failed bind should print the reason and
 * exit non-zero, not throw past the handler and lose it.
 */
void main().catch((error: unknown) => {
  process.stderr.write(`${error instanceof Error ? error.stack : String(error)}\n`)
  process.exitCode = 1
})

async function route(request: IncomingMessage, response: ServerResponse): Promise<void> {
  const url = new URL(request.url ?? '/', `http://${request.headers.host ?? 'localhost'}`)

  if (url.pathname === '/api/quote') {
    await answerQuote(request, response)
    return
  }

  const page = PAGES[url.pathname]
  if (page !== undefined) {
    await sendFile(join(pagesDir, page), response)
    return
  }

  if (
    url.pathname.startsWith('/assets/') ||
    url.pathname === '/archava.css' ||
    url.pathname === '/workspace.css'
  ) {
    const asset = resolve(publicDir, `.${decodeURIComponent(url.pathname)}`)
    if (asset.startsWith(`${publicDir}${sep}`)) {
      await sendFile(asset, response)
      return
    }
  }

  // The path a bundle is served at is `<name>.js`, which is what the pages ask
  // for. Comparing the bare name would match nothing: `/app.js` is not `/app`,
  // and a page whose script 404'd is a page that renders an empty shell.
  const entry = ENTRIES.find((candidate) => `${candidate.name}.js` === url.pathname.slice(1))
  if (entry !== undefined) {
    const js = memory.get(entry.name)
    if (js !== undefined) {
      respond(response, 200, TYPES['.js'] ?? 'text/plain', js)
      return
    }
    await sendFile(join(distDir, `${entry.name}.js`), response)
    return
  }

  respond(response, 404, TYPES['.html'] ?? 'text/plain', 'not found')
}

async function sendFile(path: string, response: ServerResponse): Promise<void> {
  let content: Uint8Array
  try {
    content = await readFile(path)
  } catch {
    respond(response, 404, TYPES['.html'] ?? 'text/plain', 'not found')
    return
  }
  respond(response, 200, TYPES[extension(path)] ?? 'application/octet-stream', content)
}

function extension(path: string): string {
  const dot = path.lastIndexOf('.')
  return dot < 0 ? '' : path.slice(dot)
}

function respond(
  response: ServerResponse,
  status: number,
  type: string,
  body: string | Uint8Array,
): void {
  response.statusCode = status
  response.setHeader('content-type', type)
  response.end(body)
}
