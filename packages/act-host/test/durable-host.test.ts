import { describe, it, expect } from 'vitest'
import type { BrainProvider } from '@archava/adapters'
import type { ActionExecutionRequest, ActionExecutionResult } from '@archava/assistant'
import {
  DurableActHost,
  InMemoryAtomicActHostAttemptLedger,
  ActHostRefusal,
  type DurableActHostOptions,
} from '../src/index.js'
import {
  ACTION,
  INPUTS,
  SESSION,
  TENANT,
  ROLE,
  CAPABILITY,
  CONFIRMING,
  SILENT_KNOWLEDGE,
  confirmations,
  graph,
  policy,
} from './fakes.js'

const brain: BrainProvider = {
  providerId: 'offline-durable-test',
  model: 'offline',
  health: { ready: true, reason: null },
  reply: () =>
    Promise.resolve({
      text: 'Model prose cannot grant execution.',
      requestedActions: [],
      citations: [],
      deferToStructuredTruth: false,
    }),
}
const envelope = { utterance: 'Synthetic booking', action: { actionId: ACTION, inputs: INPUTS } }
const request = { envelope, graph: graph(ACTION) }
function fixture(extra: Partial<DurableActHostOptions> = {}) {
  const confirmation = confirmations()
  const ledger = new InMemoryAtomicActHostAttemptLedger({ tenantId: TENANT, sessionId: SESSION })
  const calls: { request: ActionExecutionRequest; intentId: string }[] = []
  const options: DurableActHostOptions = {
    tenantId: TENANT,
    sessionId: SESSION,
    policy: policy(),
    capability: CAPABILITY,
    role: ROLE,
    brain,
    knowledge: SILENT_KNOWLEDGE,
    resolver: CONFIRMING,
    confirmations: confirmation.service,
    attempts: ledger,
    clock: confirmation.clock.now,
    executor: {
      executorId: 'offline-intent-executor',
      execute: (request, intentId) => {
        calls.push({ request, intentId })
        return Promise.resolve({ status: 'succeeded', output: { receipt: 'opaque' } })
      },
    },
    ...extra,
  }
  return {
    host: new DurableActHost(options),
    options,
    ledger: options.attempts,
    confirmation,
    calls,
  }
}
async function review(host: DurableActHost, previousIntentId?: string) {
  const result = await host.review({ ...request, previousIntentId })
  if (result.status !== 'challenge_issued' || !result.intentId)
    throw new Error('Expected a bound challenge')
  return { ...result, intentId: result.intentId }
}
function present(host: DurableActHost, result: Awaited<ReturnType<typeof review>>) {
  return host.present({
    graph: request.graph,
    envelope: {
      ...envelope,
      confirmation: { challengeId: result.challenge.challengeId, token: result.challenge.token },
    },
  })
}
describe('DurableActHost intent lifecycle', () => {
  it('concurrent reviews bind all challenges to one canonical intent/time; executions admit once', async () => {
    const f = fixture()
    const second = new DurableActHost(f.options)
    const reviews = await Promise.all(
      Array.from({ length: 8 }, (_, i) => review(i % 2 ? second : f.host)),
    )
    expect(new Set(reviews.map((r) => r.intentId)).size).toBe(1)
    expect(new Set(reviews.map((r) => r.occurredAt)).size).toBe(1)
    expect(new Set(reviews.map((r) => r.challenge.challengeId)).size).toBe(8)
    const outcomes = await Promise.all(reviews.map((r) => present(f.host, r)))
    expect(outcomes.some((result) => result.execution === 'succeeded')).toBe(true)
    expect(f.calls).toHaveLength(1)
    expect(await f.ledger.findByIntent(required(reviews[0]).intentId)).toMatchObject({
      phase: 'succeeded',
      generation: 1,
    })
    for (const r of reviews)
      expect(await f.ledger.findByChallenge(r.challenge.challengeId)).toMatchObject({
        intentId: r.intentId,
        occurredAt: r.occurredAt,
      })
  })
  it('recreated hosts and challenge reissue use the original outcome without another executor call', async () => {
    const f = fixture()
    const first = await review(f.host)
    expect((await present(f.host, first)).execution).toBe('succeeded')
    f.confirmation.clock.advance(120000)
    const restarted = new DurableActHost(f.options)
    const retry = await review(restarted)
    expect(retry.intentId).toBe(first.intentId)
    expect(retry.occurredAt).toBe(first.occurredAt)
    expect((await present(restarted, retry)).execution).toBe('succeeded')
    expect(f.calls).toHaveLength(1)
    expect(JSON.stringify(await restarted.status(first.intentId, request.graph))).not.toContain(
      first.challenge.token,
    )
    expect(JSON.stringify(restarted)).toBe('{}')
  })
  it('a genuine same-payload new intent needs a safe predecessor transition, whose retries cannot create a third intent', async () => {
    const f = fixture()
    const first = await review(f.host)
    await expect(review(f.host, first.intentId)).rejects.toMatchObject({
      code: 'attempt_transition_refused',
    })
    await present(f.host, first)
    const next = await review(f.host, first.intentId)
    expect(next.intentId).not.toBe(first.intentId)
    expect(Date.parse(next.occurredAt)).toBeGreaterThan(Date.parse(first.occurredAt))
    await present(f.host, next)
    const transitionRetry = await review(f.host, first.intentId)
    expect(transitionRetry.intentId).toBe(next.intentId)
    await present(f.host, transitionRetry)
    expect(f.calls).toHaveLength(2)
    expect(new Set(f.calls.map((call) => call.request.idempotencyKey)).size).toBe(2)
  })
  it('unknown never permits a new intent or another executor call, even after reissue/restart', async () => {
    let calls = 0
    const f = fixture({
      executor: {
        executorId: 'uncertain',
        execute: () => {
          calls++
          return Promise.resolve({
            status: 'failed',
            errorCode: 'provider_outcome_unknown',
            message: 'private upstream body',
            retryable: true,
          })
        },
      },
    })
    const first = await review(f.host)
    const result = await present(f.host, first)
    expect(result).toMatchObject({ execution: 'failed', errorCode: 'provider_outcome_unknown' })
    expect(JSON.stringify(result)).not.toContain('private upstream')
    await expect(review(f.host, first.intentId)).rejects.toMatchObject({
      code: 'attempt_transition_refused',
    })
    const restarted = new DurableActHost(f.options)
    const retry = await review(restarted)
    expect((await present(restarted, retry)).errorCode).toBe('provider_outcome_unknown')
    expect(calls).toBe(1)
    expect(await restarted.reconcile(first.intentId, request.graph)).toMatchObject({
      phase: 'unknown',
    })
  })
  it('cancel is safe only before claim; old tokens cannot execute the cancelled intent', async () => {
    const f = fixture()
    const first = await review(f.host)
    expect(await f.host.cancel(first.intentId, request.graph)).toBe(true)
    expect((await present(f.host, first)).execution).toBe('failed')
    expect(f.calls).toHaveLength(0)
    const next = await review(f.host, first.intentId)
    await present(f.host, next)
    expect(await f.host.cancel(next.intentId, request.graph)).toBe(false)
    expect(f.calls).toHaveLength(1)
  })
  it('refuses cross-session references before consuming consent and applies current policy to reads', async () => {
    const f = fixture()
    const first = await review(f.host)
    const other = new DurableActHost({ ...f.options, sessionId: 'other-session' })
    await expect(present(other, first)).rejects.toMatchObject({
      code: 'visitor_challenge_mismatch',
    })
    expect(f.confirmation.store.stateOf(first.challenge.challengeId)?.state).toBe('pending')
    await expect(other.status(first.intentId, request.graph)).rejects.toMatchObject({
      code: 'visitor_challenge_mismatch',
    })
    const disabled = new DurableActHost({ ...f.options, capability: 'assist' })
    await expect(disabled.status(first.intentId, request.graph)).rejects.toMatchObject({
      code: 'visitor_action_not_permitted',
    })
    expect(f.calls).toHaveLength(0)
  })
  it('a deadline leaves unknown; late executor completion cannot settle it and only positive read evidence recovers it', async () => {
    let complete: (result: ActionExecutionResult) => void = () => {}
    let calls = 0
    let evidence = false
    const f = fixture({
      executionTimeoutMs: 5,
      executor: {
        executorId: 'slow',
        execute: () => {
          calls++
          return new Promise((resolve) => {
            complete = resolve
          })
        },
      },
      resolverOutcome: {
        find: () =>
          Promise.resolve(
            evidence
              ? { status: 'confirmed', receiptId: 'authoritative_receipt_12345' }
              : { status: 'unknown' },
          ),
      },
    })
    const first = await review(f.host)
    expect((await present(f.host, first)).errorCode).toBe('provider_outcome_unknown')
    complete({ status: 'succeeded', output: { receipt: 'late' } })
    await Promise.resolve()
    expect(await f.host.reconcile(first.intentId, request.graph)).toMatchObject({
      phase: 'unknown',
    })
    expect(await f.ledger.settle(first.intentId, first.challenge.challengeId, 'succeeded')).toBe(
      false,
    )
    evidence = true
    expect(await f.host.reconcile(first.intentId, request.graph)).toMatchObject({
      phase: 'succeeded',
    })
    expect(calls).toBe(1)
    const retry = await review(f.host)
    expect((await present(f.host, retry)).execution).toBe('succeeded')
    expect(calls).toBe(1)
  })
  it('ambiguous settlement reports unknown and cannot release another executor', async () => {
    class OutageLedger extends InMemoryAtomicActHostAttemptLedger {
      override settle(): Promise<boolean> {
        return Promise.reject(new Error('private database connection'))
      }
    }
    const f = fixture({ attempts: new OutageLedger({ tenantId: TENANT, sessionId: SESSION }) })
    const first = await review(f.host)
    expect((await present(f.host, first)).errorCode).toBe('provider_outcome_unknown')
    const retry = await review(f.host)
    expect((await present(f.host, retry)).execution).toBe('failed')
    expect(f.calls).toHaveLength(1)
    await expect(review(f.host, first.intentId)).rejects.toBeInstanceOf(ActHostRefusal)
  })
})

function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error('Missing fixture result')
  return value
}
