import { randomUUID } from 'node:crypto'
import { z } from 'zod'
import {
  PRESENCE_VERSION,
  presenceRequestSchema,
  presenceContextSchema,
  presenceAnswerSchema,
  presenceResultSchema,
  type PresenceRequest,
  type PresenceAnswer,
  type PresenceTool,
  type PresenceSession,
} from './contracts.js'
export * from './contracts.js'
/** Only a verified host identity enters here; never parse identity from request JSON. */
export type PresenceIdentity =
  | { kind: 'guest'; binding: string; tenantId: string }
  | { kind: 'owner'; binding: string; tenantId: string; principalId: string }
type Scope = {
  identity: PresenceIdentity
  expiresAt: number
  section: string
  revision: number
  sections: readonly string[]
  requests: Set<string>
  busy: boolean
}
export class PresenceRefusal extends Error {
  constructor(
    readonly code:
      | 'invalid'
      | 'forbidden'
      | 'expired'
      | 'revision'
      | 'replay'
      | 'busy'
      | 'timeout'
      | 'unavailable',
  ) {
    super('Presence request refused: ' + code)
  }
}
export interface PresenceExecutor {
  project(identity: PresenceIdentity): Promise<readonly PresenceTool[]>
  answer(
    request: PresenceRequest,
    context: { identity: PresenceIdentity; section: string; revision: number; signal: AbortSignal },
  ): Promise<PresenceAnswer>
}
/** Bounded ephemeral transport scope. Business truth and exact action consent stay in the domain. */
export class PresenceGateway {
  private readonly sessions = new Map<string, Scope>()
  constructor(
    private readonly executor: PresenceExecutor,
    private readonly clock = Date.now,
    private readonly timeoutMs = 2000,
  ) {
    if (!Number.isInteger(timeoutMs) || timeoutMs < 1 || timeoutMs > 10000)
      throw new Error('Invalid presence timeout')
  }
  issue(identity: PresenceIdentity, sections: readonly string[], seconds = 120): PresenceSession {
    const now = this.clock()
    for (const [id, scope] of this.sessions) if (scope.expiresAt <= now) this.sessions.delete(id)
    if (this.sessions.size >= 1024) throw new PresenceRefusal('busy')
    if (
      !identity.binding ||
      !identity.tenantId ||
      !Number.isInteger(seconds) ||
      seconds < 1 ||
      seconds > 300 ||
      !sections.length ||
      sections.length > 32 ||
      sections.some((s) => !/^([a-z][a-z0-9-]{0,63})$/.test(s))
    )
      throw new PresenceRefusal('invalid')
    const sessionId = randomUUID(),
      section = sections[0] ?? 'top',
      expiresAt = now + seconds * 1000
    this.sessions.set(sessionId, {
      identity: { ...identity },
      expiresAt,
      section,
      revision: 0,
      sections: [...sections],
      requests: new Set(),
      busy: false,
    })
    return {
      version: PRESENCE_VERSION,
      sessionId,
      expiresAt,
      section,
      revision: 0,
      kind: identity.kind,
    }
  }
  private scope(id: string, identity: PresenceIdentity) {
    const scope = this.sessions.get(id)
    if (!scope) throw new PresenceRefusal('forbidden')
    if (scope.expiresAt <= this.clock()) {
      this.sessions.delete(id)
      throw new PresenceRefusal('expired')
    }
    if (
      scope.identity.kind !== identity.kind ||
      scope.identity.binding !== identity.binding ||
      scope.identity.tenantId !== identity.tenantId ||
      (scope.identity.kind === 'owner' &&
        (identity.kind !== 'owner' || scope.identity.principalId !== identity.principalId))
    )
      throw new PresenceRefusal('forbidden')
    return scope
  }
  context(raw: unknown, identity: PresenceIdentity) {
    const parsed = presenceContextSchema.safeParse(raw)
    if (!parsed.success) throw new PresenceRefusal('invalid')
    const input = parsed.data,
      scope = this.scope(input.sessionId, identity)
    if (!scope.sections.includes(input.section) || input.revision <= scope.revision || scope.busy)
      throw new PresenceRefusal('revision')
    scope.section = input.section
    scope.revision = input.revision
    return { section: scope.section, revision: scope.revision }
  }
  async turn(
    raw: unknown,
    identity: PresenceIdentity,
  ): Promise<ReturnType<typeof presenceResultSchema.parse>> {
    const parsed = presenceRequestSchema.safeParse(raw)
    if (!parsed.success) throw new PresenceRefusal('invalid')
    const input = parsed.data,
      scope = this.scope(input.sessionId, identity)
    if (input.pageRevision !== scope.revision) throw new PresenceRefusal('revision')
    if (scope.requests.has(input.requestId)) throw new PresenceRefusal('replay')
    if (scope.busy || scope.requests.size >= 256) throw new PresenceRefusal('busy')
    if (identity.kind === 'guest' && input.selection) throw new PresenceRefusal('forbidden')
    scope.requests.add(input.requestId)
    scope.busy = true
    const controller = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const work = async () => {
        const allowedTools = z
          .array(
            z.enum([
              'public.help',
              'crm.contacts.list',
              'crm.contact.summary',
              'crm.followups.list',
              'crm.overview',
              'crm.opportunities.list',
              'crm.followup.prepare',
              'crm.followup.complete.prepare',
            ]),
          )
          .max(8)
          .parse(await this.executor.project(identity))
        if (identity.kind === 'guest' && allowedTools.some((t) => t !== 'public.help'))
          throw new PresenceRefusal('forbidden')
        if (input.requestedTool && !allowedTools.includes(input.requestedTool))
          throw new PresenceRefusal('forbidden')
        const answer = presenceAnswerSchema.parse(
          await this.executor.answer(input, {
            identity,
            section: scope.section,
            revision: scope.revision,
            signal: controller.signal,
          }),
        )
        if (JSON.stringify(answer).length > 65536) throw new PresenceRefusal('unavailable')
        this.scope(input.sessionId, identity)
        if (identity.kind === 'guest' && answer.source === 'crm')
          throw new PresenceRefusal('forbidden')
        return presenceResultSchema.parse({
          ...answer,
          version: PRESENCE_VERSION,
          sessionId: input.sessionId,
          requestId: input.requestId,
          pageRevision: scope.revision,
          transport: input.transport,
          allowedTools,
        })
      }
      return await Promise.race([
        work().finally(() => {
          scope.busy = false
        }),
        new Promise<never>((_, reject) => {
          timer = setTimeout(() => {
            controller.abort()
            reject(new PresenceRefusal('timeout'))
          }, this.timeoutMs)
        }),
      ])
    } finally {
      if (timer) clearTimeout(timer)
    }
  }
}
