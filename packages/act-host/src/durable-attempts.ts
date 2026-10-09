import type { ActHostAttemptLedger, ActHostAttemptRecord } from './attempts.js'

export const ACT_HOST_ATTEMPT_PHASES = [
  'pending',
  'executing',
  'succeeded',
  'unknown',
  'cancelled',
] as const
export type ActHostAttemptPhase = (typeof ACT_HOST_ATTEMPT_PHASES)[number]

/** Safe metadata only. No payload, token, model text or raw idempotency key. */
export interface DurableActHostAttemptRecord extends ActHostAttemptRecord {
  readonly intentId: string
  readonly generation: number
  readonly phase: ActHostAttemptPhase
  readonly previousIntentId: string | null
}

/** Server-owned port. All transitions are atomic and scoped to its tenant/session. */
export interface AtomicActHostAttemptLedger extends ActHostAttemptLedger {
  /** Retry opens the existing intent; a supplied predecessor advances only a safe terminal intent. */
  open(
    record: ActHostAttemptRecord,
    previousIntentId?: string,
  ): Promise<DurableActHostAttemptRecord>
  findByIntent(intentId: string): Promise<DurableActHostAttemptRecord | null>
  /** Only one pending -> executing winner may reach an executor. */
  claim(intentId: string, claimId: string): Promise<boolean>
  /** A stale claimant cannot overwrite reconciled or terminal truth. */
  settle(intentId: string, claimId: string, phase: 'succeeded' | 'unknown'): Promise<boolean>
  /** Cancellation applies only before any executor was admitted. */
  cancel(intentId: string): Promise<boolean>
  /** Positive, server-verified evidence only. Absence/timeout never proves rejection. */
  reconcileSuccess(intentId: string, proofDigest: string): Promise<boolean>
}

export class ActHostAttemptError extends Error {
  constructor(readonly code: 'unavailable' | 'transition_refused' | 'malformed') {
    super('The action attempt could not be verified.')
    this.name = 'ActHostAttemptError'
  }
}

export function isAttemptReference(value: unknown): value is string {
  return typeof value === 'string' && /^[a-zA-Z0-9_-]{16,128}$/.test(value)
}

export function readDurableAttempt(value: unknown): DurableActHostAttemptRecord {
  if (value === null || typeof value !== 'object') throw new ActHostAttemptError('malformed')
  const row = value as Partial<DurableActHostAttemptRecord>
  if (
    typeof row.tenantId !== 'string' ||
    typeof row.sessionId !== 'string' ||
    typeof row.actionId !== 'string' ||
    ![row.tenantId, row.sessionId, row.actionId].every(
      (id) => typeof id === 'string' && id.length > 0 && id.length <= 256,
    ) ||
    typeof row.inputDigest !== 'string' ||
    !/^[a-f0-9]{64}$/.test(row.inputDigest) ||
    !isAttemptReference(row.intentId) ||
    !isAttemptReference(row.challengeId) ||
    (row.previousIntentId !== null && !isAttemptReference(row.previousIntentId)) ||
    typeof row.generation !== 'number' ||
    !Number.isSafeInteger(row.generation) ||
    (row.generation ?? 0) < 1 ||
    !(ACT_HOST_ATTEMPT_PHASES as readonly unknown[]).includes(row.phase) ||
    typeof row.occurredAt !== 'string' ||
    !Number.isSafeInteger(Date.parse(row.occurredAt)) ||
    Date.parse(row.occurredAt) < 0 ||
    new Date(row.occurredAt).toISOString() !== row.occurredAt ||
    typeof row.expiresAt !== 'number' ||
    !Number.isSafeInteger(row.expiresAt) ||
    (row.expiresAt ?? 0) < 0 ||
    (row.generation === 1) !== (row.previousIntentId === null)
  )
    throw new ActHostAttemptError('malformed')
  return {
    tenantId: row.tenantId,
    sessionId: row.sessionId,
    actionId: row.actionId,
    inputDigest: row.inputDigest,
    intentId: row.intentId,
    generation: row.generation,
    phase: row.phase as ActHostAttemptPhase,
    previousIntentId: row.previousIntentId,
    occurredAt: row.occurredAt,
    challengeId: row.challengeId,
    expiresAt: row.expiresAt,
  }
}
