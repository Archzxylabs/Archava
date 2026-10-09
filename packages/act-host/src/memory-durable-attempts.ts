import { randomUUID } from 'node:crypto'
import {
  attemptIdentityKey,
  type ActHostAttemptIdentity,
  type ActHostAttemptRecord,
} from './attempts.js'
import {
  ActHostAttemptError,
  isAttemptReference,
  readDurableAttempt,
  type AtomicActHostAttemptLedger,
  type DurableActHostAttemptRecord,
} from './durable-attempts.js'

/** Offline reference only. Its synchronous transitions are atomic within this process. */
export class InMemoryAtomicActHostAttemptLedger implements AtomicActHostAttemptLedger {
  readonly ledgerId = 'act-host-attempts-memory@2-offline'
  readonly #scope: { tenantId: string; sessionId: string }
  readonly #intents = new Map<string, DurableActHostAttemptRecord>()
  readonly #heads = new Map<string, string>()
  readonly #challenges = new Map<string, { intentId: string; expiresAt: number }>()
  readonly #claims = new Map<string, string>()
  constructor(scope: { tenantId: string; sessionId: string }) {
    this.#scope = { ...scope }
  }
  #bound(identity: ActHostAttemptIdentity) {
    if (identity.tenantId !== this.#scope.tenantId || identity.sessionId !== this.#scope.sessionId)
      throw new ActHostAttemptError('transition_refused')
  }
  async open(
    record: ActHostAttemptRecord,
    previousIntentId?: string,
  ): Promise<DurableActHostAttemptRecord> {
    // Simulate async entry, keeping the following read/transition/write indivisible.
    await Promise.resolve()
    this.#bound(record)
    const key = attemptIdentityKey(record)
    const id = this.#heads.get(key)
    const previous = id ? this.#intents.get(id) : undefined
    let current: DurableActHostAttemptRecord
    if (previousIntentId !== undefined && !isAttemptReference(previousIntentId))
      throw new ActHostAttemptError('transition_refused')
    if (previousIntentId === undefined && previous)
      current = { ...previous, challengeId: record.challengeId, expiresAt: record.expiresAt }
    else if (previous && previous.previousIntentId === previousIntentId)
      current = { ...previous, challengeId: record.challengeId, expiresAt: record.expiresAt }
    else {
      if (
        previousIntentId !== undefined &&
        (!previous ||
          previous.intentId !== previousIntentId ||
          !['succeeded', 'cancelled'].includes(previous.phase))
      )
        throw new ActHostAttemptError('transition_refused')
      const at = previous
        ? Math.max(Date.parse(record.occurredAt), Date.parse(previous.occurredAt) + 1)
        : Date.parse(record.occurredAt)
      current = {
        ...record,
        intentId: randomUUID(),
        generation: (previous?.generation ?? 0) + 1,
        phase: 'pending',
        previousIntentId: previous?.intentId ?? null,
        occurredAt: new Date(at).toISOString(),
      }
    }
    current = readDurableAttempt(current)
    const mapping = this.#challenges.get(record.challengeId)
    if (
      mapping &&
      (mapping.intentId !== current.intentId || mapping.expiresAt !== record.expiresAt)
    )
      throw new ActHostAttemptError('transition_refused')
    this.#intents.set(current.intentId, current)
    this.#heads.set(key, current.intentId)
    this.#challenges.set(record.challengeId, {
      intentId: current.intentId,
      expiresAt: record.expiresAt,
    })
    return { ...current }
  }
  async save(record: ActHostAttemptRecord): Promise<void> {
    await this.open(record)
  }
  find(identity: ActHostAttemptIdentity): Promise<DurableActHostAttemptRecord | null> {
    this.#bound(identity)
    const id = this.#heads.get(attemptIdentityKey(identity))
    return this.findByIntent(id ?? '')
  }
  findByChallenge(challengeId: string): Promise<DurableActHostAttemptRecord | null> {
    const mapping = this.#challenges.get(challengeId)
    const record = mapping ? this.#intents.get(mapping.intentId) : undefined
    return Promise.resolve(
      record && mapping ? { ...record, challengeId, expiresAt: mapping.expiresAt } : null,
    )
  }
  findByIntent(intentId: string): Promise<DurableActHostAttemptRecord | null> {
    const record = this.#intents.get(intentId)
    return Promise.resolve(record ? { ...record } : null)
  }
  claim(intentId: string, claimId: string): Promise<boolean> {
    const record = this.#intents.get(intentId)
    const mapping = this.#challenges.get(claimId)
    if (!record || record.phase !== 'pending' || mapping?.intentId !== intentId)
      return Promise.resolve(false)
    this.#intents.set(intentId, { ...record, phase: 'executing' })
    this.#claims.set(intentId, claimId)
    return Promise.resolve(true)
  }
  settle(intentId: string, claimId: string, phase: 'succeeded' | 'unknown'): Promise<boolean> {
    const record = this.#intents.get(intentId)
    if (!['succeeded', 'unknown'].includes(phase)) throw new ActHostAttemptError('malformed')
    if (!record || record.phase !== 'executing' || this.#claims.get(intentId) !== claimId)
      return Promise.resolve(false)
    this.#intents.set(intentId, { ...record, phase })
    return Promise.resolve(true)
  }
  cancel(intentId: string): Promise<boolean> {
    const record = this.#intents.get(intentId)
    if (!record || record.phase !== 'pending') return Promise.resolve(false)
    this.#intents.set(intentId, { ...record, phase: 'cancelled' })
    return Promise.resolve(true)
  }
  reconcileSuccess(intentId: string, proofDigest: string): Promise<boolean> {
    if (!/^[a-f0-9]{64}$/.test(proofDigest)) throw new ActHostAttemptError('malformed')
    const record = this.#intents.get(intentId)
    if (!record || !['executing', 'unknown'].includes(record.phase)) return Promise.resolve(false)
    this.#intents.set(intentId, { ...record, phase: 'succeeded' })
    return Promise.resolve(true)
  }
}
