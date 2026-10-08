import { createServer, type Server } from 'node:http'
import { readFile } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { randomUUID } from 'node:crypto'
import type { createCrmApi, CrmService, CrmPrincipal } from '@archava/crm'
import type { CrmPipelineService } from '@archava/crm/pipeline'

/** Fictional identities only, on loopback. Replace this host auth adapter for release. */
export async function startReferenceServer(
  service: CrmService,
  publicDirectory: string,
  apiFactory: typeof createCrmApi,
  pipeline?: CrmPipelineService,
) {
  const sessions = new Map<string, CrmPrincipal>()
  const api = apiFactory(
    service,
    (request) => {
      const bearer = request.headers.get('authorization')?.replace(/^Bearer /, '')
      const cookie = request.headers
        .get('cookie')
        ?.split(';')
        .map((item) => item.trim())
        .find((item) => item.startsWith('crm_reference='))
        ?.slice('crm_reference='.length)
      return Promise.resolve(sessions.get(bearer ?? cookie ?? '') ?? null)
    },
    pipeline,
  )
  const server: Server = createServer((incoming, outgoing) => {
    void (async () => {
      const origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`
      const url = new URL(incoming.url ?? '/', origin)
      if (url.pathname === '/reference-session' && incoming.method === 'POST') {
        if (incoming.headers.origin !== origin) {
          outgoing.writeHead(403).end()
          return
        }
        const principal = url.searchParams.get('principal')
        if (
          !['hotel-owner', 'hotel-viewer', 'hotel-editor', 'studio-owner'].includes(principal ?? '')
        ) {
          outgoing.writeHead(400).end()
          return
        }
        const token = randomUUID()
        sessions.set(token, { kind: 'human', id: principal ?? '' })
        outgoing.setHeader(
          'set-cookie',
          `crm_reference=${token}; HttpOnly; SameSite=Strict; Path=/`,
        )
        outgoing.writeHead(200, { 'content-type': 'application/json' }).end('{}')
        return
      }
      if (url.pathname.startsWith('/api/crm/')) {
        const chunks: Buffer[] = []
        let bytes = 0
        for await (const chunk of incoming) {
          const value = Buffer.isBuffer(chunk) ? chunk : Buffer.from(chunk as string)
          bytes += value.length
          if (bytes > 16_384) {
            outgoing.writeHead(413).end()
            return
          }
          chunks.push(value)
        }
        const headers = new Headers()
        for (const [name, value] of Object.entries(incoming.headers))
          if (value !== undefined) headers.set(name, Array.isArray(value) ? value.join(',') : value)
        const response = await api(
          new Request(url, {
            method: incoming.method ?? 'GET',
            headers,
            ...(incoming.method === 'GET' || incoming.method === 'HEAD'
              ? {}
              : { body: Buffer.concat(chunks) }),
          }),
        )
        outgoing.writeHead(response.status, Object.fromEntries(response.headers.entries()))
        outgoing.end(await response.text())
        return
      }
      const route =
        url.pathname === '/'
          ? '/hospitality/index.html'
          : url.pathname.endsWith('/')
            ? url.pathname + 'index.html'
            : url.pathname
      const file = resolve(publicDirectory, '.' + route)
      if (!file.startsWith(resolve(publicDirectory) + '/')) {
        outgoing.writeHead(404).end()
        return
      }
      try {
        outgoing.setHeader(
          'content-type',
          file.endsWith('.js') ? 'text/javascript; charset=utf-8' : 'text/html; charset=utf-8',
        )
        outgoing.setHeader('cache-control', 'no-store')
        outgoing.end(await readFile(file))
      } catch {
        outgoing.writeHead(404).end()
      }
    })().catch(() => outgoing.writeHead(500).end('Reference server unavailable'))
  })
  await new Promise<void>((done) => server.listen(0, '127.0.0.1', done))
  const origin = `http://127.0.0.1:${(server.address() as { port: number }).port}`
  return {
    origin,
    token(principal: CrmPrincipal) {
      const token = randomUUID()
      sessions.set(token, principal)
      return token
    },
    async close() {
      await new Promise<void>((done, error) =>
        server.close((failure) => (failure ? error(failure) : done())),
      )
    },
    publicDirectory: join(publicDirectory),
  }
}
