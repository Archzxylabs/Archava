import { composeTenantActExecutor } from '@archava/act'
import {
  DurableActHost,
  InMemoryAtomicActHostAttemptLedger,
  type DurableActHostOptions,
  type AtomicActHostAttemptLedger,
} from '@archava/act-host'
import {
  ConfirmationService,
  type ConfirmationChallengeStore,
  type ConfirmationChallengeRecord,
} from '@archava/act-confirmation'
import { ActionPolicy } from '@archava/acl'
import { parseClientConfig, type CapabilityTierName } from '@archava/config'
import { seedContextGraph, foldContextEvents } from '@archava/core'
import {
  PostmarkEmailGateway,
  PostmarkStatusPort,
  type PostmarkHTTPRequest,
  type PostmarkReadRequest,
} from '@archava/act-postmark'
import type {
  BookingAttemptStore,
  BookingAttemptRecord,
  BookingOutcome,
  BookingReservationRequest,
} from '@archava/act-booking'
import type { EmailAttemptStore, EmailAttemptRecord } from '@archava/act-email'
import type { ActionExecutionRequest } from '@archava/assistant'

/** Own offline doubles. Only authoritative external ports/stores are simulated. */
export function actFixture(
  tenantId = 'hospitality-example',
  capability: CapabilityTierName = 'act',
  ports: {
    attempts?: AtomicActHostAttemptLedger
    challengeStore?: ConfirmationChallengeStore
    bookingStore?: BookingAttemptStore
    emailStore?: EmailAttemptStore
  } = {},
) {
  let now = 1791536400000
  const clock = () => now
  const challenges = new Map<string, ConfirmationChallengeRecord>()
  const challengeStore: ConfirmationChallengeStore = {
    storeId: 'offline-confirmation',
    issue: (challenge) => {
      if (challenges.has(challenge.id)) return false
      challenges.set(challenge.id, { ...challenge, state: 'pending' })
      return true
    },
    consume: async (request) => {
      await Promise.resolve()
      const row = challenges.get(request.id)
      if (!row) return { status: 'not_found' }
      if (
        row.tenantId !== request.tenantId ||
        row.sessionId !== request.sessionId ||
        row.actionId !== request.actionId ||
        row.tokenDigest !== request.tokenDigest ||
        row.bindingDigest !== request.bindingDigest
      )
        return { status: 'mismatch' }
      if (row.state === 'consumed') return { status: 'already_consumed', challenge: row }
      if (request.now >= row.expiresAt)
        return { status: 'expired', challenge: { ...row, state: 'expired' } }
      const spent = { ...row, state: 'consumed' as const, consumedAt: request.now }
      challenges.set(row.id, spent)
      return { status: 'spent', challenge: spent }
    },
  }
  const bookingRows = new Map<string, BookingAttemptRecord>(),
    emailRows = new Map<string, EmailAttemptRecord>()
  const identity = (tenant: string, key: string) => JSON.stringify([tenant, key])
  const bookingStore: BookingAttemptStore = {
    storeId: 'offline-booking-attempts',
    find: (owner) =>
      Promise.resolve(bookingRows.get(identity(owner.tenantId, owner.idempotencyKey)) ?? null),
    claim: async (row) => {
      await Promise.resolve()
      const key = identity(row.tenantId, row.idempotencyKey),
        old = bookingRows.get(key)
      if (old) return { status: 'replayed', attempt: old }
      bookingRows.set(key, { ...row })
      return { status: 'accepted' }
    },
    settle: async (change) => {
      await Promise.resolve()
      const key = identity(change.tenantId, change.idempotencyKey),
        old = bookingRows.get(key)
      if (!old) throw new Error('Missing fixture attempt')
      if (old.state !== change.from) return { status: 'stale', attempt: old }
      bookingRows.set(key, {
        ...old,
        state: change.state,
        bookingReference: change.bookingReference,
      })
      return { status: 'settled' }
    },
  }
  const emailStore: EmailAttemptStore = {
    storeId: 'offline-email-attempts',
    find: (owner) =>
      Promise.resolve(emailRows.get(identity(owner.tenantId, owner.idempotencyKey)) ?? null),
    claim: async (row) => {
      await Promise.resolve()
      const key = identity(row.tenantId, row.idempotencyKey),
        old = emailRows.get(key)
      if (old) return { status: 'replayed', attempt: old }
      emailRows.set(key, { ...row })
      return { status: 'accepted' }
    },
    settle: async (change) => {
      await Promise.resolve()
      const key = identity(change.tenantId, change.idempotencyKey),
        old = emailRows.get(key)
      if (!old || old.state !== change.from) return false
      emailRows.set(key, {
        ...old,
        state: change.state,
        providerMessageId: change.providerMessageId,
      })
      return true
    },
  }
  let bookingResult: BookingOutcome = {
    outcome: 'confirmed',
    bookingReference: 'authoritative_booking_123',
  }
  let templateApproved = true
  let mailUnknown = false
  let mailAccepted = false
  const bookingCalls: BookingReservationRequest[] = [],
    mailCalls: PostmarkHTTPRequest[] = [],
    executionCalls: ActionExecutionRequest[] = []
  const statusCalls: PostmarkReadRequest[] = []
  const templateId = 'confirmation'
  const key = new Uint8Array(32).fill(19)
  const executor = composeTenantActExecutor({
    tenantId,
    fingerprintKeys: { booking: key, email: key },
    booking: {
      store: ports.bookingStore ?? bookingStore,
      gateway: {
        gatewayId: 'offline-authoritative-booking',
        health: { ready: true, reason: 'offline fixture' },
        reserve: async (request) => {
          await Promise.resolve()
          bookingCalls.push(request)
          return bookingResult
        },
      },
      reconciler: {
        reconcilerId: 'offline-authoritative-status',
        reconcile: () => Promise.resolve({ outcome: 'unknown' as const, reason: 'unresolved' }),
      },
    },
    email: {
      store: ports.emailStore ?? emailStore,
      status: new PostmarkStatusPort({
        recovery: {
          tenantId,
          serverToken: 'offline-not-a-real-token',
          attemptLabelKey: key,
          transport: {
            get: async (request) => {
              await Promise.resolve()
              statusCalls.push(request)
              const label = new URL(request.url).searchParams.get('metadata_archava_attempt')
              return {
                status: 200,
                body: JSON.stringify(
                  mailAccepted
                    ? {
                        TotalCount: 1,
                        Messages: [
                          {
                            MessageID: '00000000-0000-4000-8000-000000000001',
                            MessageStream: 'outbound',
                            Sandboxed: false,
                            Status: 'Sent',
                            Metadata: { archava_attempt: label },
                          },
                        ],
                      }
                    : { TotalCount: 0, Messages: [] },
                ),
              }
            },
          },
        },
      }),
      templates: {
        resolve: (t, id) =>
          Promise.resolve(
            t === tenantId && id === templateId && templateApproved
              ? {
                  tenantId,
                  templateId,
                  providerTemplateId: 'approved-confirmation',
                  senderAddress: 'sender@example.test',
                  approved: true,
                }
              : null,
          ),
      },
      gateway: new PostmarkEmailGateway({
        serverToken: 'offline-not-a-real-token',
        attemptLabelKey: key,
        transport: {
          post: async (request) => {
            await Promise.resolve()
            mailCalls.push(request)
            return mailUnknown
              ? { status: 503, body: 'private upstream body' }
              : {
                  status: 200,
                  body: JSON.stringify({
                    ErrorCode: 0,
                    MessageID: '00000000-0000-4000-8000-000000000001',
                    SubmittedAt: '2026-10-09T10:00:00Z',
                    Message: 'OK',
                  }),
                }
          },
        },
      }),
    },
  })
  const config = parseClientConfig({
    schema_version: '1.0.0',
    tenantId,
    environment: 'commerce_booking',
    presence: 'chat',
    capability: 'act',
    region: 'ID',
    template: 'hospitality',
    branding: {
      businessName: tenantId,
      theme: {
        accent: '#123456',
        surface: '#ffffff',
        ink: '#101010',
        radius: 'rounded',
        fontFamily: 'Inter',
      },
    },
    languages: [{ code: 'id', label: 'Bahasa Indonesia' }],
    primaryLanguage: 'id',
    updatedAt: '2026-10-09',
  })
  const actions = ['booking.create', 'email.send']
  const graph = foldContextEvents(seedContextGraph(config, '/booking'), [
    { type: 'action/set', actions: actions.map((name) => ({ name, enabled: true })) },
  ])
  const sessionId = 'trusted-session-' + tenantId
  const ledger = ports.attempts ?? new InMemoryAtomicActHostAttemptLedger({ tenantId, sessionId })
  const confirmations = new ConfirmationService({
    store: ports.challengeStore ?? challengeStore,
    digestKey: key,
    clock,
  })
  const options: DurableActHostOptions = {
    tenantId,
    sessionId,
    policy: new ActionPolicy(),
    capability,
    role: 'archava_assistant',
    clock,
    brain: {
      providerId: 'offline-model',
      model: 'offline',
      health: { ready: true, reason: null },
      reply: () =>
        Promise.resolve({
          text: 'The model does not decide success.',
          requestedActions: [],
          citations: [],
          deferToStructuredTruth: false,
        }),
    },
    knowledge: {
      retrieve: () =>
        Promise.resolve({
          plan: { need: 'retrieval_knowledge', subjects: [], query: '' },
          context: [],
          deferToStructuredTruth: false,
        }),
    },
    resolver: {
      resolveExists: (t, kind, id) =>
        Promise.resolve(
          t === tenantId &&
            ((kind === 'slot' && id === 'slot_1') ||
              (kind === 'customer' && id === 'customer_1') ||
              (kind === 'template' && id === templateId)),
        ),
    },
    confirmations,
    attempts: ledger,
    resolverOutcome: {
      find: async (_attempt, context) => {
        if (!context) return { status: 'unknown' }
        const result = await executor.recover(context)
        if (result.status !== 'succeeded') return { status: 'unknown' }
        const receiptId = result.output['providerMessageId'] ?? result.output['bookingReference']
        return typeof receiptId === 'string'
          ? { status: 'confirmed', receiptId }
          : { status: 'unknown' }
      },
    },
    executor: {
      executorId: 'composed-with-intent',
      execute: async (request) => {
        executionCalls.push(request)
        return executor.execute(request)
      },
    },
  }
  return {
    host: new DurableActHost(options),
    options,
    executor,
    ledger,
    graph,
    bookingCalls,
    mailCalls,
    statusCalls,
    challengeCount: () => challenges.size,
    executionCalls,
    bookingRows,
    emailRows,
    advance: (ms: number) => {
      now += ms
    },
    unknownBooking: () => {
      bookingResult = { outcome: 'unknown', reason: 'timeout' }
    },
    unknownMail: () => {
      mailUnknown = true
    },
    confirmMailAcceptance: () => {
      mailAccepted = true
    },
    revokeTemplate: () => {
      templateApproved = false
    },
    bookingInput: { slotId: 'slot_1', customer: { customerRef: 'customer_1' } },
    emailInput: { templateId, to: 'synthetic-recipient@example.test' },
  }
}
