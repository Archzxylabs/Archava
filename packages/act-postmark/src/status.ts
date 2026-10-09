import type {
  EmailDeliveryStatusLookup,
  EmailDeliveryStatusPort,
  EmailGatewayOutcome,
} from '@archava/act-email'
import { postmarkAttemptLabel } from './label.js'
import { POSTMARK_ATTEMPT_KEY } from './gateway.js'
import { defaultPostmarkReadTransport, type PostmarkReadTransport } from './transport.js'

export const POSTMARK_STATUS_PORT_ID = 'postmark:status@1'
export const POSTMARK_STATUS_URL = 'https://api.postmarkapp.com/messages/outbound'
const unknown = (): EmailGatewayOutcome => ({ outcome: 'unknown', reason: 'status_not_available' })
export interface PostmarkStatusRecovery {
  readonly tenantId: string
  readonly serverToken: string
  readonly attemptLabelKey: Uint8Array
  readonly messageStream?: string
  readonly sandboxed?: boolean
  readonly timeoutMs?: number
  readonly transport?: PostmarkReadTransport
}
export interface PostmarkStatusPortOptions {
  readonly deployment?: string
  /** Explicit opt-in. Unconfigured construction stays credential-free and unknown. */
  readonly recovery?: PostmarkStatusRecovery
}
const safeText = (value: unknown): value is string =>
  typeof value === 'string' && value.length > 0 && value.length <= 1000 && !/[\r\n\0]/.test(value)
const object = (value: unknown): value is Record<string, unknown> =>
  value !== null && typeof value === 'object' && !Array.isArray(value)

/** Tenant-bound positive acceptance lookup; never sends or reports rejection/delivery. */
export class PostmarkStatusPort implements EmailDeliveryStatusPort {
  readonly statusPortId: string
  readonly #recovery:
    | (PostmarkStatusRecovery & { readonly transport: PostmarkReadTransport })
    | undefined
  constructor(options: PostmarkStatusPortOptions = {}) {
    this.statusPortId =
      options.deployment === undefined
        ? POSTMARK_STATUS_PORT_ID
        : `${POSTMARK_STATUS_PORT_ID}(${options.deployment})`
    const recovery = options.recovery
    if (recovery) {
      if (
        !safeText(recovery.tenantId) ||
        !safeText(recovery.serverToken) ||
        !(recovery.attemptLabelKey instanceof Uint8Array) ||
        recovery.attemptLabelKey.length < 32 ||
        (recovery.messageStream !== undefined &&
          !/^[a-zA-Z0-9_-]{1,100}$/.test(recovery.messageStream)) ||
        (recovery.sandboxed !== undefined && typeof recovery.sandboxed !== 'boolean') ||
        (recovery.timeoutMs !== undefined &&
          (!Number.isInteger(recovery.timeoutMs) ||
            recovery.timeoutMs < 1 ||
            recovery.timeoutMs > 30000)) ||
        (recovery.transport !== undefined && typeof recovery.transport.get !== 'function')
      )
        throw new TypeError('Invalid tenant-bound Postmark recovery configuration.')
      this.#recovery = {
        ...recovery,
        attemptLabelKey: Uint8Array.from(recovery.attemptLabelKey),
        transport: recovery.transport ?? defaultPostmarkReadTransport(),
      }
    }
  }
  async status(lookup: EmailDeliveryStatusLookup): Promise<EmailGatewayOutcome> {
    const config = this.#recovery
    if (
      !config ||
      !lookup ||
      lookup.tenantId !== config.tenantId ||
      typeof lookup.idempotencyKey !== 'string' ||
      lookup.idempotencyKey.length < 1 ||
      lookup.idempotencyKey.length > 65536 ||
      !/^[a-f0-9]{64}$/.test(lookup.fingerprint)
    )
      return unknown()
    const label = postmarkAttemptLabel(
      config.attemptLabelKey,
      lookup.tenantId,
      lookup.idempotencyKey,
    )
    const url = new URL(POSTMARK_STATUS_URL)
    url.searchParams.set('count', '2')
    url.searchParams.set('offset', '0')
    url.searchParams.set('messagestream', config.messageStream ?? 'outbound')
    url.searchParams.set('metadata_' + POSTMARK_ATTEMPT_KEY, label)
    const controller = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    try {
      const response = await Promise.race([
        config.transport.get({
          url: url.href,
          headers: { accept: 'application/json', 'x-postmark-server-token': config.serverToken },
          signal: controller.signal,
        }),
        new Promise<never>((_resolve, reject) => {
          timer = setTimeout(() => {
            controller.abort()
            reject(new Error('Status deadline'))
          }, config.timeoutMs ?? 15000)
        }),
      ])
      if (
        response.status !== 200 ||
        typeof response.body !== 'string' ||
        Buffer.byteLength(response.body) > 65536
      )
        return unknown()
      const reply: unknown = JSON.parse(response.body)
      if (
        !object(reply) ||
        reply.TotalCount !== 1 ||
        !Array.isArray(reply.Messages) ||
        reply.Messages.length !== 1
      )
        return unknown()
      const message: unknown = reply.Messages[0]
      if (
        !object(message) ||
        typeof message.MessageID !== 'string' ||
        !/^[a-fA-F0-9]{8}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{4}-[a-fA-F0-9]{12}$/.test(
          message.MessageID,
        ) ||
        !object(message.Metadata) ||
        message.Metadata[POSTMARK_ATTEMPT_KEY] !== label ||
        message.MessageStream !== (config.messageStream ?? 'outbound') ||
        message.Sandboxed !== (config.sandboxed ?? false) ||
        !['Sent', 'Processed', 'Queued'].includes(String(message.Status))
      )
        return unknown()
      return { outcome: 'accepted', providerMessageId: message.MessageID }
    } catch {
      return unknown()
    } finally {
      if (timer) clearTimeout(timer)
    }
  }
}
