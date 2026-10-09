import { createHmac } from 'node:crypto'
import { CrmError, type CrmService, type CrmPrincipal, type CrmMutation } from '@archava/crm'
import {
  DurableActHost,
  type AtomicActHostAttemptLedger,
  type ActHostIntentExecutor,
} from '@archava/act-host'
import type { ConfirmationService } from '@archava/act-confirmation'
import type { EntityResolver } from '@archava/assistant'
import { ActionPolicy } from '@archava/acl'
import { parseClientConfig } from '@archava/config'
import { seedContextGraph, foldContextEvents } from '@archava/core'
import {
  followupActionInputsSchema,
  nativePresentSchema,
  nativeReviewSchema,
  followupCompletionInputsSchema,
  nativeCompletionPresentSchema,
  type ActionControl,
} from './contracts.js'

export const NATIVE_FOLLOWUP_ACTION = 'crm.followup.create'
export const NATIVE_COMPLETE_ACTION = 'crm.followup.complete'
export function nativeSessionId(
  key: Uint8Array,
  tenant: string,
  human: string,
  serviceId: string,
  epoch: string,
) {
  return (
    'crm-owner-' +
    createHmac('sha256', key)
      .update(JSON.stringify(['archava/native-crm/session/v1', tenant, human, serviceId, epoch]))
      .digest('hex')
  )
}
export interface NativeCrmActOptions {
  tenantId: string
  /** Verified by the HTTP host; no browser field can choose this identity. */
  humanId: string
  serviceId: string
  confirmationKey: Uint8Array
  control: () => Promise<ActionControl>
  service: CrmService
  attempts: (sessionId: string) => Promise<AtomicActHostAttemptLedger>
  confirmations: (sessionId: string) => Promise<ConfirmationService>
  clock: () => number
}
/** Two bounded native commands. Read/prepare tools and ordinary machine mutations stay separate. */
export class NativeCrmActHost {
  readonly #options: NativeCrmActOptions
  readonly #principal: CrmPrincipal
  constructor(options: NativeCrmActOptions) {
    if (!(options.confirmationKey instanceof Uint8Array) || options.confirmationKey.byteLength < 32)
      throw new TypeError('Native approval requires a stable server key of at least 32 bytes.')
    this.#options = { ...options, confirmationKey: Uint8Array.from(options.confirmationKey) }
    this.#principal = { kind: 'service', id: options.serviceId, onBehalfOf: options.humanId }
  }
  async #context(write: boolean) {
    const o = this.#options,
      control = await o.control()
    if (write && control.held)
      throw new CrmError('unavailable', 'Native actions are held for recovery.')
    const access = await o.service.access(o.tenantId, this.#principal)
    if (
      !access.permissions.includes('contacts.read') ||
      !access.permissions.includes('followups.read') ||
      (write && !access.permissions.includes('followups.write'))
    )
      throw new CrmError('forbidden', 'Native follow-up permission is required.')
    const sessionId = nativeSessionId(
      o.confirmationKey,
      o.tenantId,
      o.humanId,
      o.serviceId,
      control.epoch,
    )
    const attempts = await o.attempts(sessionId),
      confirmations = await o.confirmations(sessionId)
    const resolver: EntityResolver = {
      resolveExists: async (tenant, kind, id) => {
        if (
          tenant !== o.tenantId ||
          !['contact', 'followup'].includes(kind) ||
          typeof id !== 'string'
        )
          return false
        try {
          if (kind === 'contact') await o.service.detail(tenant, this.#principal, id)
          else await o.service.followup(tenant, this.#principal, id)
          return true
        } catch {
          return false
        }
      },
    }
    const graph = foldContextEvents(
      seedContextGraph(
        parseClientConfig({
          schema_version: '1.0.0',
          tenantId: o.tenantId,
          environment: 'commerce_booking',
          presence: 'chat',
          capability: 'act',
          region: 'ID',
          template: 'hospitality',
          branding: {
            businessName: o.tenantId,
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
        }),
        '/crm',
      ),
      [
        {
          type: 'action/set',
          actions: [NATIVE_FOLLOWUP_ACTION, NATIVE_COMPLETE_ACTION].map((name) => ({
            name,
            enabled: true,
          })),
        },
      ],
    )
    const executor: ActHostIntentExecutor = {
      executorId: 'native-crm-followup-v1',
      execute: async (request, intentId, receipt) => {
        if (
          request.tenantId !== o.tenantId ||
          ![NATIVE_FOLLOWUP_ACTION, NATIVE_COMPLETE_ACTION].includes(request.action) ||
          request.sessionId !== sessionId
        )
          return {
            status: 'failed',
            errorCode: 'native_scope_refused',
            retryable: false,
            message: 'Native scope refused.',
          }
        let command: CrmMutation['command']
        if (request.action === NATIVE_FOLLOWUP_ACTION)
          command = {
            operation: 'followup.create',
            ...followupActionInputsSchema.parse(request.inputs),
          }
        else {
          const inputs = followupCompletionInputsSchema.parse(request.inputs)
          command = {
            operation: 'followup.complete',
            id: inputs.followupId,
            expectedRevision: inputs.expectedRevision,
          }
        }
        const result = await o.service.mutateApproved(
          o.tenantId,
          this.#principal,
          { requestId: intentId, command },
          { intentId, receipt },
        )
        return {
          status: 'succeeded',
          output: { recordId: result.id, kind: result.kind, revision: result.revision },
        }
      },
    }
    const host = new DurableActHost({
      tenantId: o.tenantId,
      sessionId,
      policy: new ActionPolicy(),
      capability: 'act',
      role: 'archava_assistant',
      clock: o.clock,
      attempts,
      confirmations,
      resolver,
      executor,
      brain: {
        providerId: 'native-reviewed-action',
        model: 'deterministic',
        health: { ready: true, reason: null },
        reply: () =>
          Promise.resolve({
            text: '',
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
      resolverOutcome: {
        find: async (attempt) => {
          const result = await o.service.approvedCommandStatus(
            o.tenantId,
            this.#principal,
            attempt.intentId,
          )
          return result.status === 'committed'
            ? { status: 'confirmed', receiptId: result.receipt.id }
            : { status: 'unknown' }
        },
      },
    })
    return { host, graph, sessionId, attempts }
  }
  async #preflight(raw: unknown) {
    const inputs = followupActionInputsSchema.parse(raw)
    const detail = await this.#options.service.detail(
      this.#options.tenantId,
      this.#principal,
      inputs.contactId,
    )
    if (detail.contact.revision !== inputs.contactRevision)
      throw new CrmError('conflict', 'Contact changed. Prepare and review a fresh draft.')
    return inputs
  }
  async #review(actionId: string, inputs: Record<string, unknown>) {
    const { host, graph } = await this.#context(true)
    const result = await host.review({
      graph,
      envelope: {
        utterance: 'Owner reviewed a CRM follow-up.',
        action: { actionId, inputs },
      },
    })
    if (result.status !== 'challenge_issued' || !result.intentId)
      throw new CrmError('forbidden', 'Native review refused.')
    return nativeReviewSchema.parse({
      intentId: result.intentId,
      challenge: {
        challengeId: result.challenge.challengeId,
        token: result.challenge.token,
        expiresAt: result.challenge.expiresAt,
      },
    })
  }
  async review(raw: unknown) {
    return this.#review(NATIVE_FOLLOWUP_ACTION, await this.#preflight(raw))
  }
  async #preflightCompletion(raw: unknown) {
    const inputs = followupCompletionInputsSchema.parse(raw)
    const detail = await this.#options.service.followup(
      this.#options.tenantId,
      this.#principal,
      inputs.followupId,
    )
    if (detail.followup.status !== 'open' || detail.followup.revision !== inputs.expectedRevision)
      throw new CrmError('conflict', 'Follow-up changed. Review the current task again.')
    return inputs
  }
  async reviewCompletion(raw: unknown) {
    return this.#review(NATIVE_COMPLETE_ACTION, await this.#preflightCompletion(raw))
  }
  async presentCompletion(raw: unknown) {
    const input = nativeCompletionPresentSchema.parse(raw),
      { host, graph } = await this.#context(true)
    const inputs = await this.#preflightCompletion(input.inputs)
    return host.present({
      graph,
      envelope: {
        utterance: 'Owner confirmed completion of one reviewed CRM task.',
        action: { actionId: NATIVE_COMPLETE_ACTION, inputs },
        confirmation: input.confirmation,
      },
    })
  }
  async present(raw: unknown) {
    const input = nativePresentSchema.parse(raw),
      { host, graph } = await this.#context(true)
    const inputs = await this.#preflight(input.inputs)
    const result = await host.present({
      graph,
      envelope: {
        utterance: 'Owner confirmed a reviewed CRM follow-up.',
        action: { actionId: NATIVE_FOLLOWUP_ACTION, inputs },
        confirmation: input.confirmation,
      },
    })
    return result
  }
  async status(requestId: string) {
    const { host, graph, attempts } = await this.#context(false)
    const result = await this.#options.service.approvedCommandStatus(
      this.#options.tenantId,
      this.#principal,
      requestId,
    )
    if (result.status === 'committed' && (await attempts.findByIntent(requestId)))
      await host.reconcile(requestId, graph)
    return result
  }
  async cancelPending(requestId: string) {
    const { host, graph } = await this.#context(false)
    return host.cancel(requestId, graph)
  }
}
