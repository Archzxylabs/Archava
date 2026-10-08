import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import type { Pool } from 'pg'
import { z } from 'zod'
import { CrmError, createCrmApi, type CrmService } from '@archava/crm'
import { IdentityError, type PostgresIdentity } from '@archava/identity-pg'
import type { RuntimeSettings } from './settings.js'

const json = (value: unknown, status = 200, headers: Record<string, string> = {}) =>
  Response.json(value, { status, headers: { 'cache-control': 'no-store', ...headers } })
export function cookieToken(header: string | null, name: string): string | null {
  const candidates = (header ?? '')
    .split(';')
    .map((item) => item.trim())
    .filter((item) => item.startsWith(name + '='))
  if (candidates.length !== 1) return null
  const token = candidates[0]?.slice(name.length + 1) ?? ''
  return /^[A-Za-z0-9_-]{43}$/.test(token) ? token : null
}
export function sessionCookie(name: string, token: string, secure: boolean, clear = false) {
  return `${name}=${token}; HttpOnly; SameSite=Strict; Path=/${secure ? '; Secure' : ''}${clear ? '; Max-Age=0' : ''}`
}
/** Host identity and CSRF checks wrap every CRM route, including tools. */
export function createHostHandler(
  service: CrmService,
  identity: PostgresIdentity,
  pool: Pool,
  settings: RuntimeSettings,
  origin: string,
) {
  const secure = new URL(origin).protocol === 'https:'
  const cookieName = secure ? '__Host-archava_session' : 'archava_local_session'
  return async (request: Request, source: string): Promise<Response> => {
    try {
      const url = new URL(request.url)
      if (url.origin !== origin) return json({ error: { message: 'Request origin refused.' } }, 403)
      if (request.method === 'POST' && request.headers.get('origin') !== origin)
        return json({ error: { message: 'Request origin refused.' } }, 403)
      if (request.method === 'GET' && url.pathname === '/healthz') {
        await pool.query('SELECT 1')
        return json({ status: 'ok' })
      }
      const token = cookieToken(request.headers.get('cookie'), cookieName)
      if (request.method === 'POST' && url.pathname === '/api/login') {
        const input = z
          .object({ username: z.string().max(254), password: z.string().max(128) })
          .strict()
          .parse(await request.json())
        const result = await identity.login(input.username, input.password, source)
        return json({ authenticated: true }, 200, {
          'set-cookie': sessionCookie(cookieName, result.token, secure),
        })
      }
      const session = await identity.authenticate(token)
      if (request.method === 'GET' && url.pathname === '/api/session') {
        if (!session) return json({ authenticated: false })
        const tenants = []
        for (const tenant of settings.tenants) {
          try {
            await service.access(tenant.id, { kind: 'human', id: session.principalId })
            tenants.push(tenant)
          } catch (error) {
            if (!(error instanceof CrmError) || error.code !== 'forbidden') throw error
          }
        }
        return json({
          authenticated: true,
          username: session.username,
          csrf: session.csrf,
          tenants,
        })
      }
      if (!session || !token) return json({ error: { message: 'Sign in to continue.' } }, 401)
      if (
        request.method === 'POST' &&
        !identity.validCsrf(token, request.headers.get('x-archava-csrf'))
      )
        return json({ error: { message: 'Request verification failed.' } }, 403)
      if (request.method === 'POST' && url.pathname === '/api/logout') {
        await identity.logout(token)
        return json({ authenticated: false }, 200, {
          'set-cookie': sessionCookie(cookieName, '', secure, true),
        })
      }
      if (request.method === 'POST' && url.pathname === '/api/password') {
        const input = z
          .object({ current: z.string().max(128), next: z.string().min(15).max(128) })
          .strict()
          .parse(await request.json())
        await identity.changePassword(token, input.current, input.next)
        return json({ authenticated: false }, 200, {
          'set-cookie': sessionCookie(cookieName, '', secure, true),
        })
      }
      if (url.pathname.startsWith('/api/crm/'))
        return await createCrmApi(service, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      return json({ error: { message: 'Unknown route.' } }, 404)
    } catch (error) {
      if (error instanceof z.ZodError || error instanceof SyntaxError)
        return json({ error: { message: 'Invalid request.' } }, 400)
      if (error instanceof IdentityError) {
        const limited = error.code === 'rate_limited' || error.code === 'busy'
        return json(
          {
            error: {
              message: limited
                ? 'Too many requests. Try again later.'
                : 'Sign-in details could not be verified.',
            },
          },
          limited ? 429 : 401,
          limited ? { 'retry-after': '900' } : {},
        )
      }
      // Do not expose SQL, credentials or personal records through errors.
      process.stderr.write('CRM request temporarily unavailable.\n')
      return json({ error: { message: 'Service temporarily unavailable.' } }, 503)
    }
  }
}
export async function startCrmHost(
  service: CrmService,
  identity: PostgresIdentity,
  pool: Pool,
  settings: RuntimeSettings,
  publicDirectory: string,
) {
  const handle = (request: Request, source: string) =>
    createHostHandler(
      service,
      identity,
      pool,
      settings,
      settings.publicOrigin ?? `http://127.0.0.1:${(server.address() as { port: number }).port}`,
    )(request, source)
  const server = createServer(
    { maxHeaderSize: 8192, requestTimeout: 10000, headersTimeout: 10000 },
    (incoming, outgoing) => {
      void (async () => {
        const origin =
          settings.publicOrigin ?? `http://127.0.0.1:${(server.address() as { port: number }).port}`
        const url = new URL(incoming.url ?? '/', origin)
        const headers = new Headers()
        for (const [name, value] of Object.entries(incoming.headers))
          if (value !== undefined) headers.set(name, Array.isArray(value) ? value.join(',') : value)
        outgoing.setHeader('x-content-type-options', 'nosniff')
        outgoing.setHeader('referrer-policy', 'no-referrer')
        outgoing.setHeader('cache-control', 'no-store')
        outgoing.setHeader(
          'content-security-policy',
          "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'self'",
        )
        if (settings.publicOrigin)
          outgoing.setHeader('strict-transport-security', 'max-age=31536000')
        if (url.pathname === '/healthz' || url.pathname.startsWith('/api/')) {
          const chunks: Buffer[] = []
          let bytes = 0
          for await (const chunk of incoming) {
            const buffer = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as string)
            bytes += buffer.byteLength
            if (bytes > 16384) {
              outgoing
                .writeHead(413, { 'content-type': 'application/json' })
                .end('{"error":{"message":"Request too large."}}')
              return
            }
            chunks.push(buffer)
          }
          const response = await handle(
            new Request(url, {
              method: incoming.method ?? 'GET',
              headers,
              ...(incoming.method === 'GET' || incoming.method === 'HEAD'
                ? {}
                : { body: Buffer.concat(chunks) }),
            }),
            incoming.socket.remoteAddress ?? 'local',
          )
          outgoing.writeHead(response.status, Object.fromEntries(response.headers.entries()))
          outgoing.end(await response.text())
          return
        }
        if (incoming.method !== 'GET' && incoming.method !== 'HEAD') {
          outgoing.writeHead(405).end()
          return
        }
        const asset = new Map([
          ['/', 'index.html'],
          ['/app.js', 'app.js'],
          ['/styles.css', 'styles.css'],
        ]).get(url.pathname)
        if (!asset) {
          outgoing.writeHead(404).end()
          return
        }
        outgoing.setHeader(
          'content-type',
          asset.endsWith('.js')
            ? 'text/javascript; charset=utf-8'
            : asset.endsWith('.css')
              ? 'text/css; charset=utf-8'
              : 'text/html; charset=utf-8',
        )
        const content = await readFile(join(publicDirectory, asset))
        outgoing.end(incoming.method === 'HEAD' ? undefined : content)
      })().catch(() => {
        if (!outgoing.headersSent) outgoing.writeHead(503)
        outgoing.end('Service temporarily unavailable.')
      })
    },
  )
  server.maxConnections = 64
  server.maxRequestsPerSocket = 100
  await new Promise<void>((done, fail) => {
    server.once('error', fail)
    server.listen(settings.appPort, '127.0.0.1', done)
  })
  const origin =
    settings.publicOrigin ?? `http://127.0.0.1:${(server.address() as { port: number }).port}`
  return {
    origin,
    async close() {
      await new Promise<void>((done, fail) =>
        server.close((error) => (error ? fail(error) : done())),
      )
    },
  }
}
