import { z } from 'zod'
import { OrganizationError, type OrganizationPrincipal } from './contracts.js'
import type { OrganizationService } from './service.js'
import { createOrganizationTools } from './tools.js'

async function readBody(request: Request): Promise<string | null> {
  const reader = request.body?.getReader()
  if (!reader) return ''
  const decoder = new TextDecoder()
  let body = ''
  let size = 0
  try {
    while (true) {
      const chunk = await reader.read()
      if (chunk.done) return body + decoder.decode()
      size += chunk.value.byteLength
      if (size > 16_384) {
        await reader.cancel()
        return null
      }
      body += decoder.decode(chunk.value, { stream: true })
    }
  } finally {
    reader.releaseLock()
  }
}

export function createOrganizationApi(
  service: OrganizationService,
  identity: (request: Request) => Promise<OrganizationPrincipal | null>,
) {
  return async (request: Request): Promise<Response> => {
    const json = (body: unknown, status = 200) =>
      Response.json(body, { status, headers: { 'cache-control': 'no-store' } })
    try {
      const url = new URL(request.url)
      const route =
        /^\/api\/organizations\/([^/]+)\/(access|records|commands|tools)(?:\/([a-f0-9-]+)(?:\/(affiliations))?)?$/.exec(
          url.pathname,
        )
      if (!route) return json({ error: 'not_found' }, 404)
      const principal = await identity(request)
      if (!principal) return json({ error: 'unauthenticated' }, 401)

      if (request.method === 'POST') {
        const origin = request.headers.get('origin')
        if (
          (origin !== null && origin !== url.origin) ||
          (origin === null && request.headers.has('cookie'))
        )
          return json({ error: 'forbidden' }, 403)
      }

      const tenant = route[1] ?? ''
      const area = route[2]
      const id = route[3]
      const sub = route[4]

      if (request.method === 'GET') {
        if (area === 'access') {
          return json(await service.access(tenant, principal))
        }
        if (area === 'records') {
          if (!id) {
            const allowed = ['limit', 'after', 'state', 'search']
            for (const key of url.searchParams.keys()) {
              if (!allowed.includes(key)) return json({ error: 'invalid_request' }, 400)
            }
            const query: Record<string, string> = {}
            for (const key of allowed) {
              const val = url.searchParams.get(key)
              if (val !== null) query[key] = val
            }
            return json(await service.listOrganizations(tenant, principal, query))
          }
          if (sub === 'affiliations') {
            const allowed = ['limit', 'after', 'status', 'role']
            for (const key of url.searchParams.keys()) {
              if (!allowed.includes(key)) return json({ error: 'invalid_request' }, 400)
            }
            const query: Record<string, string> = {}
            for (const key of allowed) {
              const val = url.searchParams.get(key)
              if (val !== null) query[key] = val
            }
            return json(await service.listAffiliations(tenant, principal, id, query))
          }
          return json(await service.detail(tenant, principal, id))
        }
        if (area === 'commands' && id) {
          return json(await service.commandStatus(tenant, principal, id))
        }
        return json({ error: 'not_found' }, 404)
      }

      if (request.method === 'POST') {
        const raw = await readBody(request)
        if (raw === null) return json({ error: 'payload_too_large' }, 413)
        let body: unknown = {}
        if (raw.trim()) {
          try {
            body = JSON.parse(raw)
          } catch {
            return json({ error: 'invalid_request' }, 400)
          }
        }
        if (area === 'commands') {
          return json(await service.mutate(tenant, principal, body))
        }
        if (area === 'tools') {
          const { tool, input } = z
            .object({ tool: z.string().max(80), input: z.unknown().optional() })
            .strict()
            .parse(body)
          const tools = createOrganizationTools(service, tenant, principal)
          return json(await tools.invoke(tool, input ?? {}))
        }
        return json({ error: 'not_found' }, 404)
      }

      return json({ error: 'method_not_allowed' }, 405)
    } catch (error) {
      if (error instanceof OrganizationError) {
        const codeMap: Record<string, number> = {
          forbidden: 403,
          unauthorized: 401,
          not_found: 404,
          conflict: 409,
          primary_conflict: 409,
          archived: 409,
          invalid_request: 400,
        }
        return json({ error: error.code }, codeMap[error.code] ?? 400)
      }
      if (error instanceof z.ZodError) {
        return json({ error: 'invalid_request', details: error.issues.map((i) => i.message) }, 400)
      }
      return json({ error: 'server_error' }, 500)
    }
  }
}
