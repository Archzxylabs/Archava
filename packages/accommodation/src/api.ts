import { z } from 'zod'
import { AccommodationError, type AccommodationPrincipal } from './contracts.js'
import type { AccommodationService } from './service.js'
import { createAccommodationTools } from './tools.js'

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
/** Session identity and cookie CSRF belong to the trusted host. This API never accepts principal fields. */
export function createAccommodationApi(
  service: AccommodationService,
  identity: (request: Request) => Promise<AccommodationPrincipal | null>,
) {
  return async (request: Request): Promise<Response> => {
    const json = (body: unknown, status = 200) =>
      Response.json(body, { status, headers: { 'cache-control': 'no-store' } })
    try {
      const url = new URL(request.url)
      const route =
        /^\/api\/accommodation\/([^/]+)\/(access|properties|room-types|rooms|stays|availability|commands|tools)(?:\/([a-f0-9-]+))?$/.exec(
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
        id = route[3]
      if (request.method === 'GET') {
        const allowed =
          area === 'properties' && !id
            ? ['limit', 'after']
            : ['room-types', 'rooms', 'stays'].includes(area ?? '') && !id
              ? ['propertyId', 'limit', 'after']
              : area === 'availability' && !id
                ? ['propertyId', 'roomId', 'arrival', 'departure', 'guests']
                : []
        const query: Record<string, unknown> = {}
        for (const [key, value] of url.searchParams) {
          if (!allowed.includes(key) || Object.hasOwn(query, key))
            return json({ error: 'invalid_request' }, 400)
          query[key] = key === 'limit' || key === 'guests' ? Number(value) : value
        }
        if (area === 'access' && !id) return json(await service.access(tenant, principal))
        if (area === 'properties')
          return json(
            id
              ? await service.property(tenant, principal, id)
              : await service.properties(tenant, principal, query),
          )
        if (area === 'room-types' && !id)
          return json(await service.roomTypes(tenant, principal, query))
        if (area === 'rooms')
          return json(
            id
              ? await service.room(tenant, principal, id)
              : await service.rooms(tenant, principal, query),
          )
        if (area === 'stays')
          return json(
            id
              ? await service.detail(tenant, principal, id)
              : await service.stays(tenant, principal, query),
          )
        if (area === 'availability' && !id)
          return json(await service.availability(tenant, principal, query))
        if (area === 'commands' && id)
          return json(await service.commandStatus(tenant, principal, id))
      }

      if (
        request.method === 'POST' &&
        (area === 'commands' || area === 'tools') &&
        !id &&
        url.search === ''
      ) {
        const body = await readBody(request)
        if (body === null) return json({ error: 'invalid_request' }, 413)
        const input: unknown = JSON.parse(body)
        if (area === 'commands') return json(await service.mutate(tenant, principal, input))
        const tool = z
          .object({ name: z.string().max(80), input: z.unknown() })
          .strict()
          .parse(input)
        return json(
          await createAccommodationTools(service, tenant, principal).invoke(tool.name, tool.input),
        )
      }
      return json({ error: 'not_found' }, 404)
    } catch (error) {
      if (error instanceof AccommodationError)
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
