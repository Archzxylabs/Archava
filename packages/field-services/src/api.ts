import { z } from 'zod'
import { FieldServicesError, fieldJobPageQuerySchema } from './contracts.js'
import type { FieldPrincipal } from './contracts.js'
import type { FieldServicesService } from './service.js'
import { runFieldTool } from './tools.js'

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
  } catch {
    return null
  }
}

export function createFieldApi(
  service: FieldServicesService,
  identity?: (request: Request) => Promise<FieldPrincipal | null>,
) {
  return async function fieldApiHandler(request: Request): Promise<Response> {
    const url = new URL(request.url)
    const match = url.pathname.match(
      /^\/api\/field\/([^/]+)\/(access|jobs|commands|tools)(?:\/([^/?]+))?(?:\/([^/?]+))?/,
    )
    if (!match) return json({ error: 'not_found' }, 404)

    const tenant = match[1]
    const area = match[2]
    const jobId = match[3]
    const leaf = match[4]
    if (!tenant) return json({ error: 'not_found' }, 404)

    let principal: FieldPrincipal | null = null
    if (identity) {
      principal = await identity(request)
      if (!principal) return json({ error: 'unauthenticated' }, 401)
    } else {
      principal = {
        kind: 'human',
        id: request.headers.get('x-user-id') ?? 'anonymous',
      }
    }

    try {
      if (request.method !== 'GET' && request.method !== 'POST') {
        return json({ error: 'method_not_allowed' }, 405)
      }

      if (request.method === 'POST') {
        const origin = request.headers.get('origin')
        const secFetchSite = request.headers.get('sec-fetch-site')
        if (secFetchSite && !['same-origin', 'same-site', 'none'].includes(secFetchSite)) {
          return json({ error: 'forbidden' }, 403)
        }
        if (origin) {
          const originHost = new URL(origin).host
          const reqHost = request.headers.get('host')
          if (reqHost && originHost !== reqHost) {
            return json({ error: 'forbidden' }, 403)
          }
        }
      }

      if (request.method === 'GET') {
        if (area === 'access') {
          return json(await service.getAccess(tenant, principal))
        }
        if (area === 'jobs') {
          if (leaf === 'audit' && jobId) {
            return json(await service.audit(tenant, principal, jobId))
          }
          if (jobId) {
            return json(await service.detail(tenant, principal, jobId))
          }
          const query = fieldJobPageQuerySchema.parse({
            status: url.searchParams.get('status') ?? undefined,
            limit: url.searchParams.get('limit')
              ? Number(url.searchParams.get('limit'))
              : undefined,
            cursor: url.searchParams.get('cursor') ?? undefined,
          })
          return json(await service.listJobs(tenant, principal, query))
        }
        if (area === 'commands' && jobId) {
          return json(await service.getCommandStatus(tenant, principal, jobId))
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
          return json(await service.execute(tenant, principal, body as never))
        }
        if (area === 'tools') {
          const { tool, input } = z
            .object({ tool: z.string().max(80), input: z.unknown().optional() })
            .strict()
            .parse(body)
          return json(await runFieldTool(service, tenant, principal, tool, input))
        }
        return json({ error: 'not_found' }, 404)
      }

      return json({ error: 'method_not_allowed' }, 405)
    } catch (error) {
      if (error instanceof FieldServicesError) {
        const codeMap: Record<string, number> = {
          validation_failed: 400,
          bad_request: 400,
          forbidden: 403,
          unauthorized: 401,
          not_found: 404,
          conflict: 409,
          illegal_transition: 409,
        }
        return json({ error: error.code, message: error.message }, codeMap[error.code] ?? 400)
      }
      if (error instanceof z.ZodError) {
        return json({ error: 'validation_error', issues: error.issues }, 400)
      }
      return json({ error: 'internal_error' }, 500)
    }
  }
}

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  })
}
