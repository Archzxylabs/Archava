/**
 * `BookingAttemptStore`, backed by PostgreSQL.
 *
 * The port it implements is in `packages/act-booking/src/replay.ts` and is
 * unchanged by this file. What this file adds is the durability that port's own
 * docstring says an in-memory map cannot provide: `claim` is one
 * `INSERT ... ON CONFLICT DO NOTHING RETURNING`, so whether this caller owns the
 * key is decided by a unique index and not by a read that could race.
 *
 * ## The three answers, and which one means what
 *
 * `{ status: 'accepted' }` — the insert returned a row, so this caller owns the
 * only reservation this key will ever make.
 *
 * `{ status: 'replayed', attempt }` — the insert returned nothing, because
 * somebody already owns the key. The adapter then reads that owner's record and
 * hands it back. It does not retry the insert, does not overwrite the row, and
 * does not touch the booking system.
 *
 * `throw` — the database would not answer, or answered with a row that does not
 * check out. The boundary above wraps `claim` in a try/catch and turns a throw
 * into `booking_outcome_unknown`. That is the correct outcome: a store that
 * cannot say whether a key is taken cannot promise at most one reservation, so
 * the honest thing is to report that nobody knows and make nothing.
 *
 * The throw is deliberate. An adapter that returned `accepted` on an error
 * would let two processes both believe they owned the key — and the boundary
 * above would then make two reservations. An adapter that returned `replayed`
 * with an invented record would report a booking that was never made. There is
 * no safe third option, so there is no silent path.
 *
 * ## Why `settle` can return `stale` but never throws for it
 *
 * `BookingAttemptSettlementResult` has `settled` and `stale`, and `stale`
 * carries the current record so the caller can resolve from what is durable
 * rather than from what it last saw. Zero rows updated is `stale` — an answer,
 * not an error, because `UPDATE` does not error on matching nothing. A thrown
 * error is the unavailable case, and it means "nobody knows whether that write
 * landed".
 *
 * ## What is never stored
 *
 * No slot, no customer reference, no notes, no booking reference from anywhere
 * but the authoritative system, and no fingerprint key. The fingerprint column
 * holds a keyed digest that this package did not compute and cannot invert.
 * The pipeline's raw idempotency key contains action inputs, so this adapter
 * stores only its separate tenant/action-bound HMAC in the key column.
 *
 * **This is server-side code.** It holds tenant ids, and it is constructed with
 * a database credential by the server composition. It must not reach a bundle.
 */

import type {
  BookingAttemptClaim,
  BookingAttemptRecord,
  BookingAttemptSettlement,
  BookingAttemptSettlementResult,
  BookingAttemptStore,
  BookingAttemptOwnership,
} from '@archava/act-booking'
import { claimAttempt, lookupAttempt, settleAttempt, type AttemptRow } from './core.js'
import type { SqlClient } from './sql.js'
import { attemptStorageKey, copyStorageKeySecret } from './storage-key.js'

/**
 * The action_kind this adapter files under.
 *
 * A separate value from the email adapter's, so one table can hold both without
 * a booking attempt and an email attempt ever sharing a key. Two boundaries
 * that both used `'act'` could make one tenant's booking replay against one
 * tenant's message, which is the sort of thing a table with one shared
 * namespace will happily do if nobody stops it.
 */
export const BOOKING_ACTION_KIND = 'booking'

/** Options the server composition supplies. */
export interface PostgresBookingAttemptStoreOptions {
  /** The injected, parameterized database client. Brings its own driver. */
  readonly client: SqlClient
  /** Stable server secret; rotating it requires migrating existing attempt keys. */
  readonly idempotencyKeyHmacKey: Uint8Array
}

export class PostgresBookingAttemptStore implements BookingAttemptStore {
  /** Composed for the audit trail: names the database, not itself. */
  readonly storeId: string
  readonly #client: SqlClient
  readonly #idempotencyKeyHmacKey: Uint8Array

  constructor(options: PostgresBookingAttemptStoreOptions) {
    this.#client = options.client
    this.#idempotencyKeyHmacKey = copyStorageKeySecret(options.idempotencyKeyHmacKey)
    this.storeId = `postgres(${options.client.storeId})`
  }

  /**
   * Claim this key, or find out who owns it.
   *
   * Throws only when the database cannot be trusted to answer — an error, or a
   * row that does not validate. Both leave the caller knowing nothing, which is
   * exactly where it started.
   */
  async find(owner: BookingAttemptOwnership): Promise<BookingAttemptRecord | null> {
    const reading = await lookupAttempt(this.#client, {
      tenantId: owner.tenantId,
      actionKind: BOOKING_ACTION_KIND,
      idempotencyKey: this.storageKey(owner.tenantId, owner.idempotencyKey),
    })
    if (reading.status === 'empty') return null
    if (reading.status === 'replayed') return bookingRecord(reading.row, owner.idempotencyKey)
    throw new Error(storeFailure(reading))
  }

  async claim(attempt: BookingAttemptRecord): Promise<BookingAttemptClaim> {
    const storageKey = this.storageKey(attempt.tenantId, attempt.idempotencyKey)
    const reading = await claimAttempt(this.#client, {
      tenantId: attempt.tenantId,
      actionKind: BOOKING_ACTION_KIND,
      idempotencyKey: storageKey,
      fingerprint: attempt.fingerprint,
      openingState: 'in_flight',
    })
    if (reading.status === 'claimed') return { status: 'accepted' }
    if (reading.status === 'replayed') {
      return { status: 'replayed', attempt: bookingRecord(reading.row, attempt.idempotencyKey) }
    }
    throw new Error(storeFailure(reading))
  }

  /**
   * Settle, but only if the record is still in the state this caller left it in.
   *
   * `stale` returns the durable record, which is what lets the boundary resolve
   * a race from what is actually stored rather than from its own read. A
   * confirmed record that came back stale is answered from the durable
   * reference; a rejected one is answered as a previous refusal.
   */
  async settle(settlement: BookingAttemptSettlement): Promise<BookingAttemptSettlementResult> {
    const reading = await settleAttempt(this.#client, {
      tenantId: settlement.tenantId,
      actionKind: BOOKING_ACTION_KIND,
      idempotencyKey: this.storageKey(settlement.tenantId, settlement.idempotencyKey),
      fromState: settlement.from,
      toState: settlement.state,
      providerId: settlement.bookingReference ?? null,
    })
    if (reading.status === 'settled') return { status: 'settled' }
    if (reading.status === 'stale') {
      // Read what is there now rather than handing back a stale assumption.
      // A settlement is allowed to lose, but it is not allowed to answer with
      // a record nobody read.
      const current = await this.current(settlement.tenantId, settlement.idempotencyKey)
      if (current === null) throw new Error('booking_attempt_settlement_unresolvable')
      return { status: 'stale', attempt: current }
    }
    throw new Error(storeFailure(reading))
  }

  /**
   * Re-read the record, for a `stale` settlement that needs the durable truth.
   *
   * There is no claim here and no write: this is the record as it stands, or
   * nothing. A failure or a malformed row is reported by throwing, because
   * answering a CAS loss with a fabricated record is worse than an error.
   */
  private async current(
    tenantId: string,
    idempotencyKey: string,
  ): Promise<BookingAttemptRecord | null> {
    const reading = await lookupAttempt(this.#client, {
      tenantId,
      actionKind: BOOKING_ACTION_KIND,
      idempotencyKey: this.storageKey(tenantId, idempotencyKey),
    })
    if (reading.status !== 'replayed') return null
    return bookingRecord(reading.row, idempotencyKey)
  }

  private storageKey(tenantId: string, rawIdempotencyKey: string): string {
    return attemptStorageKey(
      this.#idempotencyKeyHmacKey,
      tenantId,
      BOOKING_ACTION_KIND,
      rawIdempotencyKey,
    )
  }
}

/**
 * The state as the booking port spells it, or a throw.
 *
 * The row's `action_kind` has already been checked to be `booking`, so a row
 * arriving here carrying an email state is the schema or the query being wrong.
 * Guessing between `prepared` and `in_flight` would be inventing state, so this
 * refuses instead.
 */
function bookingRecord(row: AttemptRow, rawIdempotencyKey: string): BookingAttemptRecord {
  const state = row.state
  if (
    state !== 'in_flight' &&
    state !== 'confirmed' &&
    state !== 'rejected' &&
    state !== 'unknown'
  ) {
    throw new Error('booking_attempt_row_state_unusable')
  }
  const record: BookingAttemptRecord = {
    tenantId: row.tenantId,
    idempotencyKey: rawIdempotencyKey,
    fingerprint: row.fingerprint,
    state,
  }
  // The reference is only present when the row is confirmed, and only ever came
  // from the authoritative system.
  if (state === 'confirmed' && row.providerId !== null) {
    return { ...record, bookingReference: row.providerId }
  }
  return record
}

/** What the boundary above will see in its log. Never carries tenant data. */
function storeFailure(reading: { readonly status: string; readonly reason?: string }): string {
  if (reading.status === 'malformed') {
    return `booking_attempt_row_${reading.reason ?? 'unusable'}`
  }
  if (reading.status === 'unavailable') return 'booking_attempt_store_unavailable'
  return `booking_attempt_store_${reading.status}`
}
