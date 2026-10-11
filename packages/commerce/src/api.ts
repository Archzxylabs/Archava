import { z } from 'zod'
import { CommerceError, type CommercePrincipal } from './contracts.js'
import type { CommerceService } from './service.js'
import { createCommerceTools } from './tools.js'

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
/** Authenticated host supplies identity and cookie CSRF; requests never supply permission or principal. */
export function createCommerceApi(
  service: CommerceService,
  identity: (request: Request) => Promise<CommercePrincipal | null>,
) {
  return async (request: Request): Promise<Response> => {
    const json = (body: unknown, status = 200) =>
      Response.json(body, { status, headers: { 'cache-control': 'no-store' } })
    try {
      const url = new URL(request.url),
        route =
          /^\/api\/commerce\/([^/]+)\/(access|products|stock|orders|commands|tools)(?:\/([a-f0-9-]+))?(?:\/(movements))?$/.exec(
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
      const tenant = route[1] ?? '',
        area = route[2],
        id = route[3],
        history = route[4]
      if (history && !(area === 'stock' && id)) return json({ error: 'not_found' }, 404)
      if (request.method === 'GET') {
        const allowed =
          area === 'products' && !id
            ? ['limit', 'after', 'active']
            : area === 'orders' && !id
              ? ['limit', 'after', 'state']
              : history
                ? ['limit']
                : []
        const query: Record<string, unknown> = {}
        for (const [key, value] of url.searchParams) {
          if (!allowed.includes(key) || Object.hasOwn(query, key))
            return json({ error: 'invalid_request' }, 400)
          if (key === 'active') {
            if (value !== 'true' && value !== 'false')
              return json({ error: 'invalid_request' }, 400)
            query[key] = value === 'true'
          } else query[key] = key === 'limit' ? Number(value) : value
        }
        if (area === 'access' && !id) return json(await service.access(tenant, principal))
        if (area === 'products')
          return json(
            id
              ? await service.product(tenant, principal, id)
              : await service.products(tenant, principal, query),
          )
        if (area === 'stock' && id)
          return json(
            history
              ? await service.movements(tenant, principal, id, query)
              : await service.stock(tenant, principal, id),
          )
        if (area === 'orders')
          return json(
            id
              ? await service.detail(tenant, principal, id)
              : await service.orders(tenant, principal, query),
          )
        if (area === 'commands' && id)
          return json(await service.commandStatus(tenant, principal, id))
      }
      if (request.method === 'POST' && !id && !url.search) {
        const body = await readBody(request)
        if (body === null) return json({ error: 'payload_too_large' }, 413)
        const input: unknown = JSON.parse(body)
        if (area === 'commands') return json(await service.mutate(tenant, principal, input))
        if (area === 'tools') {
          const tool = z
            .object({ name: z.string().min(1).max(100), input: z.unknown() })
            .strict()
            .parse(input)
          return json(
            await createCommerceTools(service, tenant, principal).invoke(tool.name, tool.input),
          )
        }
      }
      return json({ error: 'not_found' }, 404)
    } catch (error) {
      if (error instanceof z.ZodError || error instanceof SyntaxError)
        return json({ error: 'invalid_request' }, 400)
      if (error instanceof CommerceError)
        return json(
          { error: error.code },
          error.code === 'forbidden'
            ? 403
            : error.code === 'not_found'
              ? 404
              : error.code === 'conflict'
                ? 409
                : 503,
        )
      return json({ error: 'unavailable' }, 503)
    }
  }
}
