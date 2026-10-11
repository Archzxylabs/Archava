import { createServer } from 'node:http'
import { readFile } from 'node:fs/promises'
import { join } from 'node:path'
import type { Pool } from 'pg'
import { z } from 'zod'
import { CrmError, createCrmApi, type CrmService } from '@archava/crm'
import type { CrmPipelineService } from '@archava/crm/pipeline'
import {
  createCrmConversation,
  LocalCrmPlanner,
  conversationInputSchema,
  conversationPlanSchema,
  type CrmConversationPlanner,
} from '@archava/crm-assistant'
import { randomUUID } from 'node:crypto'
import { ProviderAdmissionError } from '@archava/provider-runtime'
import { IdentityError, type PostgresIdentity } from '@archava/identity-pg'
import type { RuntimeSettings } from './settings.js'
import type { NativeCrmActHost } from '@archava/crm-act'
import { ActHostRefusal } from '@archava/act-host'
import { FULL_MODULES } from '@archava/config/assembly'
import { PresenceRefusal } from '@archava/presence'
import { createHostPresence, presenceRefusalResponse } from './presence.js'
import { createProjectApi, type ProjectService } from '@archava/projects'
import { createSchedulingApi, type SchedulingService } from '@archava/scheduling'
import { createAccommodationApi, type AccommodationService } from '@archava/accommodation'
import { createPurchasingApi, type PurchasingService } from '@archava/purchasing'
import { createSupportApi, type SupportService } from '@archava/support'
import { createFoodApi, type FoodService } from '@archava/foodservice'
import { createCommerceApi, type CommerceService } from '@archava/commerce'

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
  pipeline?: CrmPipelineService,
  plannerFor: (utterance: string, attemptId: string) => CrmConversationPlanner = () =>
    new LocalCrmPlanner(),
  plannerMode: 'local' | 'gemini-synthetic' = 'local',
  nativeFor?: (tenantId: string, humanId: string) => Promise<NativeCrmActHost>,
  projects?: ProjectService,
  scheduling?: SchedulingService,
  accommodation?: AccommodationService,
  commerce?: CommerceService,
  foodservice?: FoodService,
  support?: SupportService,
  purchasing?: PurchasingService,
) {
  const modules = settings.modules ?? FULL_MODULES
  if (!modules.pipeline) pipeline = undefined
  const presence = createHostPresence(service, settings, pipeline)
  const secure = new URL(origin).protocol === 'https:'
  const cookieName = secure ? '__Host-archava_session' : 'archava_local_session'
  return async (request: Request, source: string): Promise<Response> => {
    const attemptId = randomUUID()
    const respond = (value: unknown, status = 200, headers: Record<string, string> = {}) =>
      json(value, status, { ...headers, 'x-archava-request-id': attemptId })
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
      if (
        request.method === 'POST' &&
        /^\/api\/presence\/(session|context|turn)$/.test(url.pathname)
      ) {
        if (
          session &&
          (!token || !identity.validCsrf(token, request.headers.get('x-archava-csrf')))
        )
          return json({ error: { message: 'Request verification failed.' } }, 403)
        const body: unknown = await request.json()
        const ownerId = session?.principalId
        const guestName = secure ? '__Host-archava_presence_guest' : 'archava_presence_guest'
        let guest = presence.guest(cookieToken(request.headers.get('cookie'), guestName))
        let setCookie: string | undefined
        if (!ownerId && !guest && url.pathname.endsWith('/session')) {
          guest = presence.issueGuest()
          setCookie = sessionCookie(guestName, guest, secure)
        }
        const binding = ownerId ? token : guest
        if (!binding) return json({ error: { message: 'Presence session required.' } }, 401)
        const result = url.pathname.endsWith('/session')
          ? await presence.issue(body, binding, ownerId)
          : await presence.handle(body, binding, ownerId, url.pathname.endsWith('/context'))
        return respond(result, 200, setCookie ? { 'set-cookie': setCookie } : {})
      }
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
          plannerMode,
          modules,
          nativeActions: modules.nativeActions && nativeFor !== undefined,
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
      if (url.pathname.startsWith('/api/purchasing/')) {
        const tenant = /^\/api\/purchasing\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.purchasing ||
          !purchasing ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createPurchasingApi(purchasing, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      }
      if (url.pathname.startsWith('/api/support/')) {
        const tenant = /^\/api\/support\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.support ||
          !support ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createSupportApi(support, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      }
      if (url.pathname.startsWith('/api/foodservice/')) {
        const tenant = /^\/api\/foodservice\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.foodservice ||
          !foodservice ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createFoodApi(foodservice, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      }
      if (url.pathname.startsWith('/api/commerce/')) {
        const tenant = /^\/api\/commerce\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.commerce ||
          !commerce ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createCommerceApi(commerce, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      }
      if (url.pathname.startsWith('/api/accommodation/')) {
        const tenant = /^\/api\/accommodation\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.accommodation ||
          !accommodation ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createAccommodationApi(accommodation, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      }
      if (url.pathname.startsWith('/api/scheduling/')) {
        const tenant = /^\/api\/scheduling\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.scheduling ||
          !scheduling ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createSchedulingApi(scheduling, () =>
          Promise.resolve({ kind: 'human', id: session.principalId }),
        )(request)
      }
      if (url.pathname.startsWith('/api/projects/')) {
        const tenant = /^\/api\/projects\/([^/]+)/.exec(url.pathname)?.[1]
        if (
          !tenant ||
          !modules.projects ||
          !projects ||
          !settings.tenants.some((item) => item.id === tenant)
        )
          return json({ error: 'not_found' }, 404)
        await service.access(tenant, { kind: 'human', id: session.principalId })
        return await createProjectApi(projects, () =>
          Promise.resolve({
            kind: 'human',
            id: session.principalId,
          }),
        )(request)
      }
      if (url.pathname.startsWith('/api/crm/')) {
        const unavailable = () =>
          json({ error: { message: 'Module unavailable in this composition.' } }, 404)
        if (
          !modules.assistant &&
          (url.pathname.endsWith('/conversation') || url.pathname.endsWith('/tools'))
        )
          return unavailable()
        if (
          !modules.nativeActions &&
          url.pathname.includes('/actions/') &&
          !url.pathname.endsWith('/cancel')
        )
          return unavailable()
        if (!modules.pipeline) {
          if (/\/(opportunities|overview)(\/|$)/.test(url.pathname)) return unavailable()
          if (request.method === 'POST' && /\/(commands|tools)$/.test(url.pathname)) {
            const body = z
              .object({
                command: z.object({ operation: z.string() }).passthrough().optional(),
                name: z.string().optional(),
              })
              .passthrough()
              .parse(await request.clone().json())
            if (
              (typeof body.command?.operation === 'string' &&
                body.command.operation.startsWith('opportunity.')) ||
              (typeof body.name === 'string' && /^crm\.opportunit/.test(body.name))
            )
              return unavailable()
          }
        }
      }
      const conversation = /^\/api\/crm\/([^/]+)\/conversation$/.exec(url.pathname)
      const native = /^\/api\/crm\/([^/]+)\/actions\/followup\/(review|present)$/.exec(url.pathname)
      const completeNative =
        /^\/api\/crm\/([^/]+)\/actions\/followup\/complete\/(review|present)$/.exec(url.pathname)
      const cancelNative = /^\/api\/crm\/([^/]+)\/actions\/followup\/([a-f0-9-]+)\/cancel$/.exec(
        url.pathname,
      )
      if (cancelNative && request.method === 'POST' && nativeFor) {
        const tenant = settings.tenants.find((item) => item.id === cancelNative[1])
        if (!tenant) return json({ error: { message: 'Workspace access refused.' } }, 403)
        const id = z.uuid().parse(cancelNative[2])
        return respond({
          cancelled: await (await nativeFor(tenant.id, session.principalId)).cancelPending(id),
        })
      }
      if (native && request.method === 'POST' && nativeFor) {
        const tenant = settings.tenants.find((item) => item.id === native[1])
        if (!tenant) return json({ error: { message: 'Workspace access refused.' } }, 403)
        const action = await nativeFor(tenant.id, session.principalId)
        const input: unknown = await request.json()
        return respond(
          native[2] === 'review' ? await action.review(input) : await action.present(input),
        )
      }
      if (completeNative && request.method === 'POST' && nativeFor) {
        const tenant = settings.tenants.find((item) => item.id === completeNative[1])
        if (!tenant) return json({ error: { message: 'Workspace access refused.' } }, 403)
        const action = await nativeFor(tenant.id, session.principalId),
          input: unknown = await request.json()
        return respond(
          completeNative[2] === 'review'
            ? await action.reviewCompletion(input)
            : await action.presentCompletion(input),
        )
      }
      const receiptRoute = /^\/api\/crm\/([^/]+)\/commands\/([a-f0-9-]+)$/.exec(url.pathname)
      if (receiptRoute && request.method === 'GET' && nativeFor) {
        const tenant = settings.tenants.find((item) => item.id === receiptRoute[1])
        if (!tenant) return json({ error: { message: 'Workspace access refused.' } }, 403)
        const requestId = z.uuid().parse(receiptRoute[2])
        const human = { kind: 'human' as const, id: session.principalId }
        const existing = await service.commandStatus(tenant.id, human, requestId)
        if (existing.status === 'committed') return respond(existing)
        return respond(await (await nativeFor(tenant.id, session.principalId)).status(requestId))
      }
      if (conversation && request.method === 'POST') {
        const tenant = settings.tenants.find((item) => item.id === conversation[1])
        if (!tenant) return json({ error: { message: 'Workspace access refused.' } }, 403)
        const input = conversationInputSchema.parse(await request.json())
        const selectedPlanner = plannerFor(input.utterance, attemptId)
        const boundedPlanner: CrmConversationPlanner = {
          providerId: selectedPlanner.providerId,
          model: selectedPlanner.model,
          plan: async (utterance) => {
            const plan = conversationPlanSchema.parse(await selectedPlanner.plan(utterance))
            if (
              !modules.pipeline &&
              ['crm.overview', 'opportunities.list', 'opportunity.summary'].includes(plan.operation)
            )
              throw new CrmError('not_found', 'Pipeline module is disabled.')
            if (
              !modules.nativeActions &&
              ['followup.prepare', 'followup.complete.prepare'].includes(plan.operation)
            )
              throw new CrmError('not_found', 'Native actions module is disabled.')
            return plan
          },
        }
        return respond(
          await createCrmConversation(service, boundedPlanner, pipeline).ask(
            {
              tenantId: tenant.id,
              principal: { kind: 'human', id: session.principalId },
              turnId: attemptId,
              occurredAt: new Date().toISOString(),
            },
            input,
          ),
        )
      }
      if (url.pathname.startsWith('/api/crm/'))
        return await createCrmApi(
          service,
          () => Promise.resolve({ kind: 'human', id: session.principalId }),
          pipeline,
        )(request)
      return json({ error: { message: 'Unknown route.' } }, 404)
    } catch (error) {
      if (error instanceof PresenceRefusal) return presenceRefusalResponse(error)
      if (error instanceof ActHostRefusal)
        return respond(
          {
            error: {
              message: 'Action approval could not be verified. Review the current draft again.',
            },
          },
          403,
        )
      if (error instanceof ProviderAdmissionError)
        return respond({ error: { message: 'AI request limit reached. Try again later.' } }, 429, {
          'retry-after': String(Math.max(1, Math.ceil(error.retryAfterMs / 1000))),
        })
      if (error instanceof z.ZodError || error instanceof SyntaxError)
        return json({ error: { message: 'Invalid request.' } }, 400)
      if (error instanceof CrmError) {
        const status = {
          unauthenticated: 401,
          forbidden: 403,
          not_found: 404,
          conflict: 409,
          unavailable: 503,
        }[error.code]
        return json(
          {
            error: {
              message: status === 503 ? 'Service temporarily unavailable.' : 'CRM request refused.',
            },
          },
          status,
        )
      }
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
      return respond({ error: { message: 'Service temporarily unavailable.' } }, 503)
    }
  }
}
export async function startCrmHost(
  service: CrmService,
  identity: PostgresIdentity,
  pool: Pool,
  settings: RuntimeSettings,
  publicDirectory: string,
  pipeline?: CrmPipelineService,
  plannerFor?: (utterance: string, attemptId: string) => CrmConversationPlanner,
  plannerMode?: 'local' | 'gemini-synthetic',
  nativeFor?: (tenantId: string, humanId: string) => Promise<NativeCrmActHost>,
  projects?: ProjectService,
  scheduling?: SchedulingService,
  accommodation?: AccommodationService,
  commerce?: CommerceService,
  foodservice?: FoodService,
  support?: SupportService,
  purchasing?: PurchasingService,
) {
  let handler: ReturnType<typeof createHostHandler> | undefined
  const handle = (request: Request, source: string) => {
    handler ??= createHostHandler(
      service,
      identity,
      pool,
      settings,
      settings.publicOrigin ?? `http://127.0.0.1:${(server.address() as { port: number }).port}`,
      pipeline,
      plannerFor,
      plannerMode,
      nativeFor,
      projects,
      scheduling,
      accommodation,
      commerce,
      foodservice,
      support,
      purchasing,
    )
    return handler(request, source)
  }
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
