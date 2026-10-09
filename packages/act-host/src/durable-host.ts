import { createHash } from 'node:crypto'
import type { ConfirmationReceipt } from '@archava/act-confirmation'
import {
  buildIdempotencyKey,
  validateActionInputs,
  type ActionExecutionRequest,
  type ActionExecutionResult,
  type ActionExecutor,
} from '@archava/assistant'
import type { ContextGraph } from '@archava/core'
import { assertTenant } from '@archava/core'
import {
  ActHost,
  ActHostRefusal,
  type ActHostOptions,
  type ActHostRequest,
  type ActHostReview,
  type ActHostPresentation,
} from './host.js'
import { readVisitorEnvelope } from './envelope.js'
import {
  ActHostAttemptError,
  isAttemptReference,
  readDurableAttempt,
  type AtomicActHostAttemptLedger,
  type DurableActHostAttemptRecord,
} from './durable-attempts.js'

/** Intent ids are correlation only: policy, consent, validation and resource auth still run. */
export interface ActHostIntentExecutor {
  readonly executorId: string
  execute(
    request: ActionExecutionRequest,
    intentId: string,
    receipt: ConfirmationReceipt,
  ): Promise<ActionExecutionResult>
}
export interface ActHostOutcomeResolver {
  /** Read-only authoritative positive evidence. An absent or uncertain outcome stays unknown. */
  find(
    attempt: Readonly<
      Pick<
        DurableActHostAttemptRecord,
        'intentId' | 'tenantId' | 'sessionId' | 'actionId' | 'inputDigest' | 'occurredAt'
      >
    >,
    /** Validated original inputs, when supplied. Never grants execution or issues consent. */
    context?: ActionExecutionRequest,
  ): Promise<
    { readonly status: 'confirmed'; readonly receiptId: string } | { readonly status: 'unknown' }
  >
}
export type DurableActHostOptions = Omit<ActHostOptions, 'attempts' | 'executor'> & {
  readonly attempts: AtomicActHostAttemptLedger
  readonly executor: ActHostIntentExecutor
  readonly resolverOutcome?: ActHostOutcomeResolver
  readonly executionTimeoutMs?: number
}
export type DurableActHostReview = ActHostReview & { readonly intentId?: string }
export interface DurableActHostStatus {
  readonly intentId: string
  readonly actionId: string
  readonly phase: DurableActHostAttemptRecord['phase']
}
const unknown = (): ActionExecutionResult => ({
  status: 'failed',
  errorCode: 'provider_outcome_unknown',
  retryable: false,
  message:
    'The outcome remains unknown. Check its authoritative receipt before starting a new intent.',
})

/** Operational host. The old ActHost remains an offline compatibility seam. */
export class DurableActHost {
  readonly #options: DurableActHostOptions
  readonly #timeout: number
  constructor(options: DurableActHostOptions) {
    if (
      !options.attempts ||
      ![
        'open',
        'findByChallenge',
        'findByIntent',
        'claim',
        'settle',
        'cancel',
        'reconcileSuccess',
      ].every(
        (method) =>
          typeof (options.attempts as unknown as Record<string, unknown>)[method] === 'function',
      ) ||
      !options.executor
    )
      throw new TypeError('A durable host needs an atomic ledger and an intent executor.')
    const timeout = options.executionTimeoutMs ?? 45000
    if (!Number.isInteger(timeout) || timeout < 1 || timeout > 120000)
      throw new TypeError('Invalid execution deadline')
    this.#options = Object.freeze({ ...options })
    this.#timeout = timeout
  }
  #host(executor?: ActionExecutor) {
    return new ActHost({ ...this.#options, executor })
  }
  #scope(record: unknown) {
    const attempt = readDurableAttempt(record)
    if (
      attempt.tenantId !== this.#options.tenantId ||
      attempt.sessionId !== this.#options.sessionId
    )
      throw new ActHostRefusal(
        'visitor_challenge_mismatch',
        'The intent belongs to a different session.',
      )
    return attempt
  }
  #failure(error: unknown): never {
    if (error instanceof ActHostRefusal) throw error
    throw new ActHostRefusal(
      error instanceof ActHostAttemptError && error.code === 'transition_refused'
        ? 'attempt_transition_refused'
        : 'attempt_record_unavailable',
      'The durable action state could not be verified.',
    )
  }
  async review(request: ActHostRequest): Promise<DurableActHostReview> {
    try {
      if (request.previousIntentId !== undefined && !isAttemptReference(request.previousIntentId))
        throw new ActHostAttemptError('transition_refused')
      const result = await this.#host().review(request)
      if (result.status !== 'challenge_issued') return result
      const attempt = this.#scope(
        await this.#options.attempts.findByChallenge(result.challenge.challengeId),
      )
      if (
        attempt.challengeId !== result.challenge.challengeId ||
        attempt.occurredAt !== result.occurredAt
      )
        throw new ActHostAttemptError('malformed')
      return { ...result, intentId: attempt.intentId }
    } catch (error) {
      this.#failure(error)
    }
  }
  async present(request: ActHostRequest): Promise<ActHostPresentation> {
    try {
      const reading = readVisitorEnvelope(request.envelope)
      if (!reading.envelope.confirmation) return await this.#host().present(request)
      const stored = await this.#options.attempts.findByChallenge(
        reading.envelope.confirmation.challengeId,
      )
      if (stored === null)
        throw new ActHostRefusal(
          'attempt_record_missing',
          'No intent is recorded for that challenge.',
        )
      const attempt = this.#scope(stored)
      const claimId = reading.envelope.confirmation.challengeId
      const host = new ActHost({
        ...this.#options,
        executor: undefined,
        executorForConfirmation: (receipt) => ({
          executorId: 'durable-' + this.#options.executor.executorId,
          execute: (execution) => this.#execute(attempt, claimId, execution, receipt),
        }),
      })
      return await host.present(request)
    } catch (error) {
      this.#failure(error)
    }
  }
  async #execute(
    attempt: DurableActHostAttemptRecord,
    claimId: string,
    request: ActionExecutionRequest,
    receipt: ConfirmationReceipt,
  ): Promise<ActionExecutionResult> {
    const ledger = this.#options.attempts
    let claimed = false
    try {
      const expectedKey = buildIdempotencyKey({
        tenantId: attempt.tenantId,
        sessionId: attempt.sessionId,
        occurredAt: attempt.occurredAt,
        actionId: attempt.actionId,
        inputs: request.inputs,
      })
      if (
        request.tenantId !== attempt.tenantId ||
        request.sessionId !== attempt.sessionId ||
        request.action !== attempt.actionId ||
        request.idempotencyKey !== expectedKey
      )
        return unknown()
      claimed = await ledger.claim(attempt.intentId, claimId)
      if (typeof claimed !== 'boolean') return unknown()
      if (claimed !== true) {
        const current = this.#scope(await ledger.findByIntent(attempt.intentId))
        if (
          current.intentId !== attempt.intentId ||
          current.inputDigest !== attempt.inputDigest ||
          current.actionId !== attempt.actionId ||
          current.occurredAt !== attempt.occurredAt
        )
          return unknown()
        return current.phase === 'succeeded'
          ? { status: 'succeeded', output: { replayed: true } }
          : unknown()
      }
      let timer: ReturnType<typeof setTimeout> | undefined
      let result: ActionExecutionResult
      try {
        result = await Promise.race([
          this.#options.executor.execute(request, attempt.intentId, receipt),
          new Promise<ActionExecutionResult>((resolve) => {
            timer = setTimeout(() => resolve(unknown()), this.#timeout)
          }),
        ])
      } catch {
        result = unknown()
      } finally {
        if (timer) clearTimeout(timer)
      }
      // All failures are conservatively unknown. Only an actual success settles success.
      const phase =
        result?.status === 'succeeded' &&
        result.output !== null &&
        typeof result.output === 'object' &&
        !Array.isArray(result.output)
          ? 'succeeded'
          : 'unknown'
      if ((await ledger.settle(attempt.intentId, claimId, phase)) !== true) return unknown()
      return phase === 'succeeded' ? result : unknown()
    } catch {
      if (claimed) await ledger.settle(attempt.intentId, claimId, 'unknown').catch(() => false)
      return unknown()
    }
  }
  async #record(intentId: string, graph: ContextGraph) {
    if (!isAttemptReference(intentId))
      throw new ActHostRefusal('attempt_record_missing', 'The intent reference is invalid.')
    assertTenant(graph, this.#options.tenantId)
    const record = await this.#options.attempts.findByIntent(intentId)
    if (record === null)
      throw new ActHostRefusal(
        'attempt_record_missing',
        'No intent is recorded for that reference.',
      )
    const attempt = this.#scope(record)
    if (attempt.intentId !== intentId) throw new ActHostAttemptError('malformed')
    if (!(await this.#host().availableActions(graph)).includes(attempt.actionId))
      throw new ActHostRefusal(
        'visitor_action_not_permitted',
        'Current policy does not permit access to this action.',
      )
    return attempt
  }
  async status(intentId: string, graph: ContextGraph): Promise<DurableActHostStatus> {
    try {
      const record = await this.#record(intentId, graph)
      return { intentId: record.intentId, actionId: record.actionId, phase: record.phase }
    } catch (error) {
      this.#failure(error)
    }
  }
  async cancel(intentId: string, graph: ContextGraph): Promise<boolean> {
    try {
      await this.#record(intentId, graph)
      return (await this.#options.attempts.cancel(intentId)) === true
    } catch (error) {
      this.#failure(error)
    }
  }
  async reconcile(
    intentId: string,
    graph: ContextGraph,
    inputs?: Readonly<Record<string, unknown>>,
  ): Promise<DurableActHostStatus> {
    try {
      const record = await this.#record(intentId, graph)
      let context: ActionExecutionRequest | undefined
      if (inputs !== undefined) {
        const validated = await validateActionInputs({
          actionId: record.actionId,
          inputs,
          tenantId: record.tenantId,
          ...(this.#options.resolver ? { resolver: this.#options.resolver } : {}),
        })
        if (!validated.ok)
          throw new ActHostRefusal('visitor_inputs_rejected', 'Recovery inputs were refused.')
        const description = this.#options.confirmations.describeBinding({
          tenantId: record.tenantId,
          sessionId: record.sessionId,
          actionId: record.actionId,
          inputs: validated.inputs,
        })
        if (description.status !== 'bound' || description.inputDigest !== record.inputDigest)
          throw new ActHostRefusal(
            'visitor_challenge_mismatch',
            'Recovery inputs do not match the intent.',
          )
        context = {
          tenantId: record.tenantId,
          sessionId: record.sessionId,
          action: record.actionId,
          inputs: validated.inputs,
          idempotencyKey: buildIdempotencyKey({
            tenantId: record.tenantId,
            sessionId: record.sessionId,
            actionId: record.actionId,
            occurredAt: record.occurredAt,
            inputs: validated.inputs,
          }),
        }
      }
      if (['unknown', 'executing'].includes(record.phase) && this.#options.resolverOutcome) {
        const result = await this.#options.resolverOutcome.find(
          {
            intentId: record.intentId,
            tenantId: record.tenantId,
            sessionId: record.sessionId,
            actionId: record.actionId,
            inputDigest: record.inputDigest,
            occurredAt: record.occurredAt,
          },
          context,
        )
        if (result?.status === 'confirmed' && isAttemptReference(result.receiptId)) {
          const proof = createHash('sha256').update(result.receiptId).digest('hex')
          await this.#options.attempts.reconcileSuccess(intentId, proof)
        }
      }
      return await this.status(intentId, graph)
    } catch (error) {
      this.#failure(error)
    }
  }
}
