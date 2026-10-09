/**
 * The replay and reconciliation boundary around `BookingGateway`.
 *
 * The executor hands this boundary the same `BookingReservationRequest` every
 * time the same turn is replayed, and the same `idempotencyKey` with it. What
 * has to be true, and what this file is for:
 *
 * 1. **One attempt, one reservation.** A key, once claimed, never issues a
 *    second reservation. Not from a concurrent caller, not from a process that
 *    restarted mid-flight, not from a user pressing the button twice.
 * 2. **The same key and the same payload replay.** A second caller gets the
 *    answer the first one got — the reference the authoritative system issued,
 *    never a reference this package minted.
 * 3. **The same key and a different payload is a conflict.** A key that arrives
 *    carrying a different slot, customer or note is not a replay of anything;
 *    it is a bug or a forgery, and it is refused rather than served.
 * 4. **An unknown outcome is never laundered into a success.** If nobody knows
 *    how the first attempt ended, a later caller is told nobody knows — unless
 *    the authoritative system has since looked the original attempt up and
 *    reconciled it. Only then is a reference reported, and it is still the
 *    system's reference, not ours.
 *
 * The boundary implements `BookingGateway` itself, which is the point: an
 * executor cannot tell the difference between a raw gateway and a wrapped one,
 * so the existing public executor contract is unchanged and every caller that
 * already holds an executor keeps working.
 *
 * **This is server-side code.** The attempt store holds tenant-scoped
 * identifiers and the reconciler speaks with tenant credentials. Neither
 * belongs in a browser bundle, and neither is decided here.
 *
 * No per-request state is kept on the instance. Two callers may share one
 * boundary and interleave freely, and which attempt each is is carried in the
 * arguments rather than held where the other could overwrite it.
 */

import { createHmac } from 'node:crypto'
import { gatewayReady, isConfirmedBooking } from './gateway.js'
import type { BookingGateway, BookingOutcome, BookingReservationRequest } from './gateway.js'

/** The tenant-scoped identity of one attempt. */
export interface BookingAttemptOwnership {
  readonly tenantId: string
  readonly idempotencyKey: string
}

/** Where an attempt stands, as the boundary recorded it. */
export type BookingAttemptState =
  /** Claimed, no answer yet. A process that died here leaves it here forever. */
  | 'in_flight'
  /** The authoritative system issued a reference for it. */
  | 'confirmed'
  /** The authoritative system declined it. */
  | 'rejected'
  /** An answer came back and it did not say. */
  | 'unknown'

/** One attempt, as the store holds it. */
export interface BookingAttemptRecord extends BookingAttemptOwnership {
  /**
   * A keyed digest of the request, never the slot, customer reference or notes.
   * A replay whose fingerprint does not match is a different request wearing
   * the same key. The application must retain its digest key for the lifetime
   * of these records; rotating it requires a migration plan.
   */
  readonly fingerprint: string
  readonly state: BookingAttemptState
  /** Present only when `state` is `confirmed`, and only ever system-issued. */
  readonly bookingReference?: string
}

/** What a store says when asked to claim a key. */
export type BookingAttemptClaim =
  /** Nobody had this key. This caller owns the only reservation it will make. */
  | { readonly status: 'accepted' }
  /** Somebody had it. Nothing further may be written; this is the record. */
  | { readonly status: 'replayed'; readonly attempt: BookingAttemptRecord }

/** The terminal state of an attempt, and the reference that came with it. */
export interface BookingAttemptSettlement extends BookingAttemptOwnership {
  /** The state this writer observed. The store must compare it atomically. */
  readonly from: 'in_flight' | 'unknown'
  readonly state: Exclude<BookingAttemptState, 'in_flight'>
  readonly bookingReference?: string
}

export type BookingAttemptSettlementResult =
  | { readonly status: 'settled' }
  | { readonly status: 'stale'; readonly attempt: BookingAttemptRecord }

/**
 * The durable attempt record.
 *
 * **The contract no in-memory map satisfies:** `claim` must make
 * "this key is mine, everyone else reads it back" a single atomic step. Two
 * processes calling it at once must not both be told `accepted`. Nothing in
 * this package decides how that is done — a unique index on
 * `(tenantId, idempotencyKey)` and an insert-or-nothing is the usual answer,
 * and it is the deployment's to make.
 *
 * `settle` must also be an atomic compare-and-swap against `from`. Terminal
 * states cannot be overwritten by a late reconciliation. The returned current
 * record lets the caller avoid reporting an obsolete answer after a race.
 *
 * No slot, customer reference or notes are stored. Only their keyed digest is
 * kept, and the reconciler below gets the same treatment.
 */
export interface BookingAttemptStore {
  /** Which store this is, for the audit trail. Not a secret. */
  readonly storeId: string
  /** Read-only lookup; absence is never permission to reserve. Optional for legacy stores. */
  find?(owner: BookingAttemptOwnership): Promise<BookingAttemptRecord | null>
  /**
   * Atomically assert that nobody has claimed this attempt yet.
   *
   * Implementations that answer `replayed` for a key claimed concurrently in
   * the same process have got this wrong, and the failure is a double booking.
   */
  claim(attempt: BookingAttemptRecord): Promise<BookingAttemptClaim>
  /** Atomically settle only if the record is still in `from`. */
  settle(settlement: BookingAttemptSettlement): Promise<BookingAttemptSettlementResult>
}

/**
 * What an authoritative system can be asked about an attempt it already
 * received.
 *
 * The reconciliation lookup deliberately carries no payload beyond the
 * fingerprint. A lookup port that could be handed a slot and a customer is a
 * lookup port that could be talked into making a second reservation.
 */
export interface BookingReconciliationLookup {
  readonly tenantId: string
  readonly idempotencyKey: string
  readonly fingerprint: string
}

/**
 * The authoritative system, asked what became of an attempt.
 *
 * This is *not* `reserve`. It reads; it does not write. A port that can only
 * say "unknown" is a correct port — the honest answer when a system cannot look
 * an attempt up — and the boundary treats it as exactly that.
 */
export interface BookingReconciliationPort {
  /** Which system answers, for the audit trail. Not a secret. */
  readonly reconcilerId: string
  reconcile(lookup: BookingReconciliationLookup): Promise<BookingOutcome>
}

export interface BookingReplayBoundaryOptions {
  /** The authoritative system this boundary issues to at most once per key. */
  readonly gateway: BookingGateway
  /** The durable record of what was attempted and what became of it. */
  readonly store: BookingAttemptStore
  /** The read-only way to ask what became of an attempt. */
  readonly reconciler: BookingReconciliationPort
  /** Server-only secret, at least 32 bytes, stable while attempt records live. */
  readonly fingerprintKey: Uint8Array
}

/** Everything but the identity, when a terminal state is being written. */
type SettlementWithoutOwner = Omit<BookingAttemptSettlement, 'tenantId' | 'idempotencyKey'>

/**
 * A `BookingGateway` that will issue at most one reservation per
 * tenant-scoped idempotency key.
 *
 * Deliberately not a scheduler, not a retry loop and not a queue. There is no
 * automatic retry here on purpose: a blind retry after a timeout is the
 * double-booking path, and the only permitted second look at an attempt is an
 * authoritative reconciliation of the first one.
 */
export class BookingReplayBoundary implements BookingGateway {
  private readonly gateway: BookingGateway
  private readonly store: BookingAttemptStore
  private readonly reconciler: BookingReconciliationPort
  private readonly fingerprintKey: Uint8Array

  constructor(options: BookingReplayBoundaryOptions) {
    if (!(options.fingerprintKey instanceof Uint8Array) || options.fingerprintKey.length < 32) {
      throw new Error('booking replay fingerprint key must contain at least 32 bytes')
    }
    this.gateway = options.gateway
    this.store = options.store
    this.reconciler = options.reconciler
    this.fingerprintKey = Uint8Array.from(options.fingerprintKey)
  }

  /** Composed, so a trace names the store the record lives in as well. */
  get gatewayId(): string {
    return `replay(${this.store.storeId}→${this.gateway.gatewayId})`
  }

  /**
   * Passed through, with the throw absorbed.
   *
   * The executor already guards a health read, but a boundary that throws from
   * its own property is a boundary that can be used to crash a turn, and that
   * is not a thing worth leaving available.
   */
  get health(): { readonly ready: boolean; readonly reason: string } {
    try {
      return this.gateway.health
    } catch {
      return { ready: false, reason: 'booking_system_health_unreadable' }
    }
  }

  /** Recover an existing attempt only. Never claims or calls the reservation gateway. */
  async recover(request: BookingReservationRequest): Promise<BookingOutcome> {
    try {
      if (!this.store.find) return { outcome: 'unknown', reason: 'attempt_not_reconciled' }
      const row = await this.store.find({
        tenantId: request.tenantId,
        idempotencyKey: request.idempotencyKey,
      })
      if (
        !row ||
        row.tenantId !== request.tenantId ||
        row.idempotencyKey !== request.idempotencyKey
      )
        return { outcome: 'unknown', reason: 'attempt_not_reconciled' }
      return await this.replay(row, attemptFingerprint(request, this.fingerprintKey))
    } catch {
      return { outcome: 'unknown', reason: 'attempt_not_reconciled' }
    }
  }

  async reserve(request: BookingReservationRequest): Promise<BookingOutcome> {
    // Before claiming: an unreachable system must not consume a key, or the
    // caller who tried during the outage would be left holding a record that
    // nothing ever made.
    if (!this.isReady()) {
      return { outcome: 'unknown', reason: 'booking_system_unavailable' }
    }

    const owner: BookingAttemptOwnership = {
      tenantId: request.tenantId,
      idempotencyKey: request.idempotencyKey,
    }
    const fingerprint = attemptFingerprint(request, this.fingerprintKey)
    const claim = await this.claim(owner, fingerprint)
    if (claim === null) {
      // The store could not answer. Nothing was reserved, and nothing can be
      // claimed to have been — this is an unknown, not a result.
      return { outcome: 'unknown', reason: 'attempt_store_unavailable' }
    }
    if (claim.status === 'replayed') {
      return await this.replay(claim.attempt, fingerprint)
    }
    return await this.issue(owner, request, fingerprint)
  }

  /** Ask the store to make this attempt this caller's. */
  private async claim(
    owner: BookingAttemptOwnership,
    fingerprint: string,
  ): Promise<BookingAttemptClaim | null> {
    try {
      return await this.store.claim({ ...owner, fingerprint, state: 'in_flight' })
    } catch {
      return null
    }
  }

  /**
   * This caller owns the attempt: make the one reservation and record how it
   * ended.
   *
   * Every branch settles the record, including the ones that end badly. An
   * unsettled record is a record that looks in-flight to the next process,
   * which is the state that can only be recovered by reconciliation.
   */
  private async issue(
    owner: BookingAttemptOwnership,
    request: BookingReservationRequest,
    fingerprint: string,
  ): Promise<BookingOutcome> {
    let reply: unknown
    try {
      reply = await this.gateway.reserve(request)
    } catch {
      const current = await this.settle(owner, fingerprint, {
        from: 'in_flight',
        state: 'unknown',
      })
      if (current !== null) return outcomeAfterStaleSettlement(current)
      return { outcome: 'unknown', reason: 'booking_system_did_not_answer' }
    }

    const answer = readOutcome(reply)
    if (answer?.outcome === 'confirmed') {
      // The reference is the one the system issued. Recording it is what makes
      // the next caller's replay answerable without touching the system.
      const current = await this.settle(owner, fingerprint, {
        from: 'in_flight',
        state: 'confirmed',
        bookingReference: answer.bookingReference,
      })
      if (current !== null) return outcomeAfterStaleSettlement(current, answer)
      return answer
    }
    if (answer?.outcome === 'rejected') {
      const current = await this.settle(owner, fingerprint, {
        from: 'in_flight',
        state: 'rejected',
      })
      if (current !== null) return outcomeAfterStaleSettlement(current, answer)
      return answer
    }
    // A reply that names no reference, or is not shaped like an outcome at all,
    // has not said no — it has said nothing, and it is recorded as nothing.
    const current = await this.settle(owner, fingerprint, {
      from: 'in_flight',
      state: 'unknown',
    })
    if (current !== null) return outcomeAfterStaleSettlement(current)
    if (answer === null) return { outcome: 'unknown', reason: 'booking_reply_unrecognised' }
    return { outcome: 'unknown', reason: statedReason(answer.reason) }
  }

  /**
   * Somebody else owns this attempt. Serve what is already true.
   *
   * `confirmed` and `rejected` are answered from the record with no write
   * anywhere. `in_flight` and `unknown` are the recoverable cases, and the only
   * thing allowed to speak for them is the authoritative system.
   */
  private async replay(
    attempt: BookingAttemptRecord,
    fingerprint: string,
  ): Promise<BookingOutcome> {
    // Same key, different payload. This is not a replay of the attempt that
    // owns the key, so neither its answer nor a second write is available to
    // it. The check lives here and not only in the store: a store that answered
    // loosely cannot turn a payload change into a replay.
    if (fingerprint !== attempt.fingerprint) {
      return { outcome: 'rejected', reason: 'idempotency_key_conflict' }
    }
    if (attempt.state === 'confirmed') {
      // Defensive on purpose. A confirmed record whose reference no longer
      // passes the same guard the live reply passed is a record nobody should
      // trust, and inventing a reference is never the repair.
      const stored = attempt.bookingReference
      if (
        typeof stored === 'string' &&
        isConfirmedBooking({ outcome: 'confirmed', bookingReference: stored })
      ) {
        return { outcome: 'confirmed', bookingReference: stored }
      }
      return { outcome: 'unknown', reason: 'stored_reference_unusable' }
    }
    if (attempt.state === 'rejected') {
      return { outcome: 'rejected', reason: 'reservation_previously_declined' }
    }
    return await this.reconcile(attempt)
  }

  /**
   * Ask the authoritative system what became of an attempt nobody has an
   * answer for.
   *
   * This is the one path a replay can take out of an ambiguous outcome, and it
   * is a read. When the answer is unknown — or the reconciler cannot look the
   * attempt up, or throws, or says something unrecognisable — the boundary
   * returns unknown and writes nothing.
   */
  private async reconcile(attempt: BookingAttemptRecord): Promise<BookingOutcome> {
    let answer: BookingOutcome | null = null
    try {
      answer = readOutcome(
        await this.reconciler.reconcile({
          tenantId: attempt.tenantId,
          idempotencyKey: attempt.idempotencyKey,
          fingerprint: attempt.fingerprint,
        }),
      )
    } catch {
      answer = null
    }
    if (answer?.outcome === 'confirmed') {
      const current = await this.settle(attempt, attempt.fingerprint, {
        from: attempt.state === 'unknown' ? 'unknown' : 'in_flight',
        state: 'confirmed',
        bookingReference: answer.bookingReference,
      })
      if (current !== null) return outcomeAfterStaleSettlement(current, answer)
      return answer
    }
    if (answer?.outcome === 'rejected') {
      // An in-flight issuer may not have reached the PMS yet. A negative read
      // at this instant cannot prove that the still-running write will fail.
      if (attempt.state === 'in_flight') {
        return { outcome: 'unknown', reason: 'attempt_still_in_flight' }
      }
      const current = await this.settle(attempt, attempt.fingerprint, {
        from: 'unknown',
        state: 'rejected',
      })
      if (current !== null) return outcomeAfterStaleSettlement(current, answer)
      return answer
    }
    if (answer === null) return { outcome: 'unknown', reason: 'attempt_not_reconciled' }
    return { outcome: 'unknown', reason: statedReason(answer.reason) }
  }

  /**
   * Write the terminal state, and survive a store that refuses to take it.
   *
   * A settle failure after a confirmed write does not turn that write into a
   * failure to report: the reference is verified by the authoritative system,
   * and withdrawing it would tell a visitor nothing was booked while a booking
   * stands. What it does leave is an `in_flight` record, and an `in_flight`
   * record is reconciled rather than re-issued — so the guarantee that matters,
   * one reservation per key, still holds.
   */
  private async settle(
    owner: BookingAttemptOwnership,
    fingerprint: string,
    settlement: SettlementWithoutOwner,
  ): Promise<BookingAttemptRecord | null> {
    try {
      const result = await this.store.settle({ ...owner, ...settlement })
      if (result.status !== 'stale') return null
      const current = result.attempt
      return current.tenantId === owner.tenantId &&
        current.idempotencyKey === owner.idempotencyKey &&
        current.fingerprint === fingerprint
        ? current
        : null
    } catch {
      // Swallowed deliberately, and only here. See above.
      return null
    }
  }

  private isReady(): boolean {
    try {
      return gatewayReady(this.gateway)
    } catch {
      return false
    }
  }
}

/** Read whatever a port said as a well-formed outcome, or nothing. */
function readOutcome(reply: unknown): BookingOutcome | null {
  if (typeof reply !== 'object' || reply === null) return null
  const candidate = reply as Record<string, unknown>
  if (candidate['outcome'] === 'confirmed') {
    // The one guard that matters: `confirmed` with no usable reference is not a
    // confirmation, wherever it came from.
    return isConfirmedBooking(reply) ? reply : null
  }
  if (candidate['outcome'] === 'rejected') {
    return { outcome: 'rejected', reason: statedReason(candidate['reason']) }
  }
  if (candidate['outcome'] === 'unknown') {
    return { outcome: 'unknown', reason: statedReason(candidate['reason']) }
  }
  return null
}

/**
 * Keep only a known machine reason.
 *
 * A prose reason from a booking system is dropped, not rewritten. The class it
 * belonged to survives; the words do not.
 */
function statedReason(reason: unknown): string {
  if (typeof reason !== 'string') return MACHINE_REASON_FALLBACK
  return SAFE_PROVIDER_REASONS.has(reason) ? reason : MACHINE_REASON_FALLBACK
}

const MACHINE_REASON_FALLBACK = 'booking_outcome_unexplained'
const SAFE_PROVIDER_REASONS = new Set([
  'attempt_not_found',
  'idempotency_key_conflict',
  'slot_already_taken',
  'timeout',
])

/**
 * Everything in the request except the key, as one stable string.
 *
 * The tenant is in it even though the claim is already tenant-scoped. A store
 * that ignored the tenant and answered another tenant's replay would still be
 * caught here.
 */
function attemptFingerprint(request: BookingReservationRequest, key: Uint8Array): string {
  return createHmac('sha256', key)
    .update(
      JSON.stringify([
        request.tenantId,
        request.slotId,
        request.customerRef,
        request.notes ?? null,
      ]),
    )
    .digest('hex')
}

/** Resolve a CAS loss from the current durable state, never from a stale read. */
function outcomeAfterStaleSettlement(
  attempt: BookingAttemptRecord,
  candidate?: BookingOutcome,
): BookingOutcome {
  if (attempt.state === 'confirmed') {
    const reference = attempt.bookingReference
    if (
      typeof reference === 'string' &&
      isConfirmedBooking({ outcome: 'confirmed', bookingReference: reference })
    ) {
      if (candidate?.outcome === 'confirmed' && candidate.bookingReference !== reference) {
        return { outcome: 'unknown', reason: 'conflicting_authoritative_outcomes' }
      }
      return { outcome: 'confirmed', bookingReference: reference }
    }
  }
  if (attempt.state === 'rejected') {
    if (candidate?.outcome === 'confirmed') {
      return { outcome: 'unknown', reason: 'conflicting_authoritative_outcomes' }
    }
    return { outcome: 'rejected', reason: 'reservation_previously_declined' }
  }
  return { outcome: 'unknown', reason: 'attempt_state_changed' }
}
