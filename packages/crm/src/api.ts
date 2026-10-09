import { z } from 'zod'
import { CrmError, type CrmPrincipal } from './contracts.js'
import { type CrmService, createCrmTools } from './service.js'
import { type CrmPipelineService, createPipelineTools } from './pipeline.js'

export type CrmAuthenticator = (request: Request) => Promise<CrmPrincipal | null>
const json = (value: unknown, status = 200) =>
  Response.json(value, { status, headers: { 'cache-control': 'no-store' } })

async function readBoundedBody(request: Request): Promise<string | null> {
  const reader = request.body?.getReader()
  if (!reader) return ''
  const decoder = new TextDecoder()
  let text = ''
  let size = 0
  try {
    while (true) {
      const chunk = await reader.read()
      if (chunk.done) return text + decoder.decode()
      size += chunk.value.byteLength
      if (size > 16_384) {
        await reader.cancel()
        return null
      }
      text += decoder.decode(chunk.value, { stream: true })
    }
  } finally {
    reader.releaseLock()
  }
}

/** Same-origin JSON API. Authenticate using a host session, not body roles/ids.
 * Host must enforce CSRF protection for cookie-based deployments. */
export function createCrmApi(
  service: CrmService,
  authenticate: CrmAuthenticator,
  pipeline?: CrmPipelineService,
) {
  return async (request: Request): Promise<Response> => {
    try {
      const url = new URL(request.url)
      const parts = url.pathname.split('/').filter(Boolean)
      if (parts[0] !== 'api' || parts[1] !== 'crm' || !parts[2])
        return json({ error: { code: 'not_found', message: 'Unknown CRM route.' } }, 404)
      const tenant = parts[2]
      const principal = await authenticate(request)
      if (!principal) throw new CrmError('unauthenticated', 'Sign in to access CRM.')
      if (request.method === 'POST') {
        const origin = request.headers.get('origin')
        if (
          (origin !== null && origin !== url.origin) ||
          (origin === null && request.headers.has('cookie'))
        )
          throw new CrmError('forbidden', 'CRM request origin refused.')
      }
      if (request.method === 'GET' && parts.length === 4) {
        if (parts[3] === 'access') return json(await service.access(tenant, principal))
        if (parts[3] === 'overview')
          return json(
            await service.overview(tenant, principal, Object.fromEntries(url.searchParams)),
          )
        if (parts[3] === 'contacts')
          return json(
            await service.list(tenant, principal, {
              ...(url.searchParams.has('after') ? { after: url.searchParams.get('after') } : {}),
              limit: url.searchParams.has('limit') ? Number(url.searchParams.get('limit')) : 20,
              search: url.searchParams.get('search') ?? '',
            }),
          )
        if (parts[3] === 'followups')
          return json(
            await service.listFollowups(tenant, principal, {
              ...(url.searchParams.has('bucket') ? { bucket: url.searchParams.get('bucket') } : {}),
              ...(url.searchParams.has('asOf') ? { asOf: url.searchParams.get('asOf') } : {}),
              ...(url.searchParams.has('afterDueAt') || url.searchParams.has('afterId')
                ? {
                    after: {
                      dueAt: url.searchParams.get('afterDueAt'),
                      id: url.searchParams.get('afterId'),
                    },
                  }
                : {}),
              limit: url.searchParams.has('limit') ? Number(url.searchParams.get('limit')) : 20,
              search: url.searchParams.get('search') ?? '',
            }),
          )
        if (parts[3] === 'opportunities' && pipeline)
          return json(
            await pipeline.list(tenant, principal, {
              ...(url.searchParams.has('after') ? { after: url.searchParams.get('after') } : {}),
              ...(url.searchParams.has('stage') ? { stage: url.searchParams.get('stage') } : {}),
              ...(url.searchParams.has('contactId')
                ? { contactId: url.searchParams.get('contactId') }
                : {}),
              limit: url.searchParams.has('limit') ? Number(url.searchParams.get('limit')) : 20,
              search: url.searchParams.get('search') ?? '',
            }),
          )
      }
      if (request.method === 'GET' && parts.length === 5 && parts[3] === 'contacts')
        return json(await service.detail(tenant, principal, parts[4]))
      if (request.method === 'GET' && parts.length === 5 && parts[3] === 'followups')
        return json(await service.followup(tenant, principal, parts[4]))
      if (request.method === 'GET' && parts.length === 5 && parts[3] === 'commands')
        return json(await service.commandStatus(tenant, principal, parts[4]))
      if (
        request.method === 'GET' &&
        parts.length === 5 &&
        parts[3] === 'opportunities' &&
        pipeline
      )
        return json(await pipeline.detail(tenant, principal, parts[4]))
      if (
        request.method === 'POST' &&
        parts.length === 4 &&
        ['commands', 'tools'].includes(parts[3] ?? '')
      ) {
        const text = await readBoundedBody(request)
        if (text === null)
          return json(
            { error: { code: 'invalid', message: 'Request exceeds CRM size limit.' } },
            413,
          )
        let input: unknown
        try {
          input = JSON.parse(text)
        } catch {
          return json({ error: { code: 'invalid', message: 'Invalid JSON.' } }, 400)
        }
        if (parts[3] === 'commands') return json(await service.mutate(tenant, principal, input))
        const tool = z
          .object({ name: z.string().max(80), input: z.unknown() })
          .strict()
          .parse(input)
        if (pipeline && ['crm.opportunities.list', 'crm.opportunity.summary'].includes(tool.name))
          return json(
            await createPipelineTools(pipeline, tenant, principal).invoke(tool.name, tool.input),
          )
        return json(await createCrmTools(service, tenant, principal).invoke(tool.name, tool.input))
      }
      return json({ error: { code: 'not_found', message: 'Unknown CRM route.' } }, 404)
    } catch (error) {
      if (error instanceof z.ZodError)
        return json({ error: { code: 'invalid', message: 'Invalid CRM input.' } }, 400)
      if (error instanceof CrmError) {
        const status = {
          unauthenticated: 401,
          forbidden: 403,
          not_found: 404,
          conflict: 409,
          unavailable: 503,
        }[error.code]
        return json({ error: { code: error.code, message: error.message } }, status)
      }
      return json({ error: { code: 'unavailable', message: 'CRM temporarily unavailable.' } }, 503)
    }
  }
}
