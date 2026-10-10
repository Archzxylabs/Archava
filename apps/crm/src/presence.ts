import { randomBytes, createHash } from 'node:crypto'
import { z } from 'zod'
import {
  PresenceGateway,
  PresenceRefusal,
  PRESENCE_VERSION,
  presenceErrorSchema,
  presenceRequestSchema,
  presenceContextSchema,
  type PresenceIdentity,
  type PresenceTool,
} from '@archava/presence'
import { runTurn } from '@archava/assistant'
import { ActionPolicy } from '@archava/acl'
import { contextGraphSchema } from '@archava/core'
import type { BrainProvider } from '@archava/adapters'
import { CrmError, type CrmService } from '@archava/crm'
import type { CrmPipelineService } from '@archava/crm/pipeline'
import {
  createCrmConversation,
  LocalCrmPlanner,
  conversationPlanSchema,
} from '@archava/crm-assistant'
import { FULL_MODULES } from '@archava/config/assembly'
import type { RuntimeSettings } from './settings.js'

const publicText =
  'Archava is a reusable business assistant. This local public demo uses offline text and synthetic voice transcripts. Private CRM requires owner sign-in; actions require a separate exact confirmation. Live voice and avatar are not connected here.'
const brain: BrainProvider = {
  providerId: 'archava-public-offline',
  model: 'verified-public-help-v1',
  health: { ready: true },
  reply: () =>
    Promise.resolve({
      text: publicText,
      citations: [],
      requestedActions: [],
      deferToStructuredTruth: false,
    }),
}
/** Same runTurn policy as CRM; this guest projection has no private or write adapter. */
export function createHostPresence(
  service: CrmService,
  settings: RuntimeSettings,
  pipeline?: CrmPipelineService,
) {
  const modules = settings.modules ?? FULL_MODULES
  const guestBindings = new Map<string, number>()
  const identities = new Map<
    string,
    { tenantId: string; kind: 'guest' | 'owner'; expiresAt: number }
  >()
  const gateway = new PresenceGateway({
    project: async (identity) => {
      if (identity.kind === 'guest') return ['public.help']
      const access = await service.access(identity.tenantId, {
        kind: 'human',
        id: identity.principalId,
      })
      if (!modules.assistant || !access.permissions.includes('contacts.read'))
        throw new CrmError('forbidden', 'Assistant access refused.')
      const tools: PresenceTool[] = ['crm.contacts.list', 'crm.contact.summary']
      if (access.permissions.includes('followups.read')) tools.push('crm.followups.list')
      if (modules.pipeline && access.permissions.includes('pipeline.read'))
        tools.push('crm.overview', 'crm.opportunities.list')
      if (modules.nativeActions && access.permissions.includes('followups.write'))
        tools.push('crm.followup.prepare', 'crm.followup.complete.prepare')
      return tools
    },
    answer: async (request, context) => {
      if (context.signal.aborted) throw new PresenceRefusal('timeout')
      if (context.identity.kind === 'guest') {
        const outcome = await runTurn({
          tenantId: context.identity.tenantId,
          sessionId: request.sessionId,
          utterance: request.utterance,
          occurredAt: new Date().toISOString(),
          graph: contextGraphSchema.parse({
            tenantId: context.identity.tenantId,
            page: { route: '/' + context.section, kind: 'public', locale: 'id' },
            session: { authenticated: false },
            availableActions: [],
            entities: [],
            errors: [],
          }),
          brain,
          policy: new ActionPolicy(),
          capability: 'assist',
          role: 'archava_assistant',
          knowledge: {
            retrieve: () =>
              Promise.resolve({
                plan: { need: 'retrieval_knowledge', query: request.utterance, subjects: [] },
                context: [],
                deferToStructuredTruth: false,
              }),
          },
        })
        return {
          status: 'ok',
          text: outcome.text || publicText,
          source: outcome.text === publicText ? 'public' : 'none',
          mode: 'offline',
        }
      }
      const local = new LocalCrmPlanner()
      const planner = {
        providerId: local.providerId,
        model: local.model,
        plan: async (utterance: string) => {
          const plan = conversationPlanSchema.parse(await local.plan(utterance))
          if (
            !modules.pipeline &&
            ['crm.overview', 'opportunities.list', 'opportunity.summary'].includes(plan.operation)
          )
            throw new CrmError('not_found', 'Pipeline unavailable.')
          if (
            !modules.nativeActions &&
            ['followup.prepare', 'followup.complete.prepare'].includes(plan.operation)
          )
            throw new CrmError('not_found', 'Native preparation unavailable.')
          return plan
        },
      }
      const reply = await createCrmConversation(
        service,
        planner,
        modules.pipeline ? pipeline : undefined,
      ).ask(
        {
          tenantId: context.identity.tenantId,
          principal: { kind: 'human', id: context.identity.principalId },
          turnId: request.requestId,
          occurredAt: new Date().toISOString(),
        },
        { utterance: request.utterance, ...request.selection },
      )
      return {
        status: 'ok',
        text: reply.text,
        source: reply.source === 'crm' ? 'crm' : 'none',
        mode: 'offline',
        data: reply.data,
      }
    },
  })
  const key = (token: string) => createHash('sha256').update(token).digest('hex')
  return {
    guest(token: string | null) {
      return token && (guestBindings.get(key(token)) ?? 0) > Date.now() ? token : null
    },
    issueGuest() {
      for (const [id, expiry] of guestBindings) if (expiry <= Date.now()) guestBindings.delete(id)
      if (guestBindings.size >= 1024) throw new PresenceRefusal('busy')
      const token = randomBytes(32).toString('base64url')
      guestBindings.set(key(token), Date.now() + 300000)
      return token
    },
    async issue(raw: unknown, token: string, ownerId?: string) {
      const input = z
        .object({ tenantId: z.string().max(100) })
        .strict()
        .parse(raw)
      if (!settings.tenants.some((t) => t.id === input.tenantId))
        throw new PresenceRefusal('forbidden')
      const identity: PresenceIdentity = ownerId
        ? { kind: 'owner', binding: key(token), tenantId: input.tenantId, principalId: ownerId }
        : { kind: 'guest', binding: key(token), tenantId: input.tenantId }
      if (ownerId) await service.access(input.tenantId, { kind: 'human', id: ownerId })
      // Bound the companion index to exactly the gateway's live-session limit.
      for (const [id, value] of identities) if (value.expiresAt <= Date.now()) identities.delete(id)
      if (identities.size >= 1024) throw new PresenceRefusal('busy')
      const session = gateway.issue(
        identity,
        ownerId ? ['crm'] : ['top', 'catalog-section', 'protocol-section', 'business-section'],
      )
      identities.set(session.sessionId, {
        tenantId: input.tenantId,
        kind: identity.kind,
        expiresAt: session.expiresAt,
      })
      return session
    },
    async handle(raw: unknown, token: string, ownerId: string | undefined, context = false) {
      const input = context ? presenceContextSchema.parse(raw) : presenceRequestSchema.parse(raw)
      const issued = identities.get(input.sessionId)
      if (!issued || issued.kind !== (ownerId ? 'owner' : 'guest'))
        throw new PresenceRefusal('forbidden')
      const identity: PresenceIdentity = ownerId
        ? { kind: 'owner', binding: key(token), tenantId: issued.tenantId, principalId: ownerId }
        : { kind: 'guest', binding: key(token), tenantId: issued.tenantId }
      return context ? gateway.context(input, identity) : await gateway.turn(input, identity)
    },
  }
}
export function presenceRefusalResponse(error: PresenceRefusal) {
  const status = {
    invalid: 400,
    forbidden: 403,
    expired: 410,
    revision: 409,
    replay: 409,
    busy: 429,
    timeout: 504,
    unavailable: 503,
  }[error.code]
  return Response.json(
    presenceErrorSchema.parse({
      version: PRESENCE_VERSION,
      error: { code: error.code, message: 'Presence request could not be completed.' },
    }),
    { status, headers: { 'cache-control': 'no-store' } },
  )
}
