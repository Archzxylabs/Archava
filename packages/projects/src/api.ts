import { z } from 'zod'
import { ProjectError, type ProjectPrincipal } from './contracts.js'
import type { ProjectService } from './service.js'
import { createProjectTools } from './tools.js'

async function readBody(request: Request): Promise<string | null> {
  const reader = request.body?.getReader()
  if (!reader) return ''
  const decoder = new TextDecoder()
  let body = '',
    size = 0
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

/** Same-origin bounded API. The trusted host supplies session identity and cookie CSRF protection. */
export function createProjectApi(
  service: ProjectService,
  identity: (request: Request) => Promise<ProjectPrincipal | null>,
) {
  return async (request: Request): Promise<Response> => {
    const json = (body: unknown, status = 200) =>
      Response.json(body, { status, headers: { 'cache-control': 'no-store' } })
    try {
      const url = new URL(request.url)
      const route =
        /^\/api\/projects\/([^/]+)(?:\/(access|records|commands|tools)(?:\/([a-f0-9-]+))?)?$/.exec(
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
      const area = route[2],
        id = route[3]
      if (request.method === 'GET' && area === 'access' && !id)
        return json(await service.access(tenant, principal))
      if (request.method === 'GET' && area === 'records' && !id) {
        const allowed = new Set(['limit', 'after', 'search', 'state'])
        for (const [key] of url.searchParams)
          if (!allowed.has(key)) return json({ error: 'invalid_request' }, 400)
        return json(
          await service.list(tenant, principal, {
            ...(url.searchParams.has('limit')
              ? { limit: Number(url.searchParams.get('limit')) }
              : {}),
            ...(url.searchParams.has('after') ? { after: url.searchParams.get('after') } : {}),
            ...(url.searchParams.has('search') ? { search: url.searchParams.get('search') } : {}),
            ...(url.searchParams.has('state') ? { state: url.searchParams.get('state') } : {}),
          }),
        )
      }
      if (request.method === 'GET' && area === 'records' && id)
        return json(await service.detail(tenant, principal, id))
      if (request.method === 'GET' && area === 'commands' && id)
        return json(await service.commandStatus(tenant, principal, id))
      if (request.method === 'POST' && (area === 'commands' || area === 'tools') && !id) {
        const body = await readBody(request)
        if (body === null) return json({ error: 'invalid_request' }, 413)
        const input: unknown = JSON.parse(body)
        if (area === 'commands') return json(await service.mutate(tenant, principal, input))
        const tool = z
          .object({ name: z.string().max(80), input: z.unknown() })
          .strict()
          .parse(input)
        return json(
          await createProjectTools(service, tenant, principal).invoke(tool.name, tool.input),
        )
      }
      return json({ error: 'not_found' }, 404)
    } catch (error) {
      if (error instanceof ProjectError)
        return json(
          { error: error.code },
          { forbidden: 403, not_found: 404, conflict: 409, unavailable: 503 }[error.code],
        )
      if (error instanceof z.ZodError || error instanceof SyntaxError)
        return json({ error: 'invalid_request' }, 400)
      return json({ error: 'unavailable' }, 503)
    }
  }
}
