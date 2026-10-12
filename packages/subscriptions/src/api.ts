import { z } from 'zod'
import {
  SubscriptionError,
  subscriptionPageQuerySchema,
  type SubscriptionPrincipal,
  type SubscriptionMutation,
} from './contracts.js'
import type { SubscriptionService } from './service.js'
import { executeSubscriptionTool } from './tools.js'

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

export function createSubscriptionApi(
  service: SubscriptionService,
  identity: (request: Request) => Promise<SubscriptionPrincipal | null>,
) {
  return async (request: Request): Promise<Response> => {
    const json = (body: unknown, status = 200) =>
      Response.json(body, { status, headers: { 'cache-control': 'no-store' } })
    try {
      const url = new URL(request.url)
      const route =
        /^\/api\/subscriptions\/([^/]+)\/(access|records|commands|tools)(?:\/([a-f0-9-]+)(?:\/(entitlements|usage))?)?$/.exec(
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
          return json(await service.getAccess(tenant, principal))
        }
        if (area === 'records') {
          if (!id) {
            const query = subscriptionPageQuerySchema.parse({
              limit: url.searchParams.get('limit')
                ? Number(url.searchParams.get('limit'))
                : undefined,
              offset: url.searchParams.get('offset')
                ? Number(url.searchParams.get('offset'))
                : undefined,
              status: url.searchParams.get('status') ?? undefined,
              contactId: url.searchParams.get('contactId') ?? undefined,
            })
            return json(await service.listSubscriptions(tenant, principal, query))
          }
          if (sub === 'entitlements') {
            return json(await service.listEntitlements(tenant, principal, id))
          }
          if (sub === 'usage') {
            return json(await service.listUsageEvents(tenant, principal, id))
          }
          return json(await service.getSubscriptionDetail(tenant, principal, id))
        }
        if (area === 'commands' && id) {
          return json(await service.getCommandStatus(tenant, principal, id))
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
          return json(await service.execute(tenant, principal, body as SubscriptionMutation))
        }
        if (area === 'tools') {
          const { tool, input } = z
            .object({ tool: z.string().max(80), input: z.unknown().optional() })
            .strict()
            .parse(body)
          return json(await executeSubscriptionTool(service, tenant, principal, tool, input))
        }
        return json({ error: 'not_found' }, 404)
      }

      return json({ error: 'method_not_allowed' }, 405)
    } catch (error) {
      if (error instanceof SubscriptionError) {
        const codeMap: Record<string, number> = {
          forbidden: 403,
          unauthorized: 401,
          not_found: 404,
          conflict: 409,
          already_active: 409,
          past_due: 402,
          exhausted: 409,
          suspended: 409,
          invalid_request: 400,
        }
        return json({ error: error.code, message: error.message }, codeMap[error.code] ?? 400)
      }
      if (error instanceof z.ZodError) {
        return json({ error: 'invalid_request', details: error.issues.map((i) => i.message) }, 400)
      }
      return json({ error: 'server_error' }, 500)
    }
  }
}
