/**
 * `EmailAttemptStore`, backed by PostgreSQL.
 *
 * The port is `packages/act-email/src/outbox.ts` and is unchanged by this file.
 * The email port's `settle` returns a `boolean` rather than a record-and-status
 * pair, so this adapter's shape is slightly different from the booking one's —
 * but the policy underneath is the same `core.ts` for a reason that matters more
 * than the shape difference: both adapters must fail the same way.
 *
 * ## `settle` returns `false` in three situations, and only two are the same
 *
 * `true` — the conditional `UPDATE` matched and moved a row.
 *
 * `false` — one of:
 *
 *   1. **The CAS lost.** Zero rows updated because the record is no longer
 *      `prepared`. This is the ordinary case: another caller settled it as
 *      `accepted` first, or a reconciliation moved it to `unknown`. The record
 *      is perfectly healthy; this writer simply lost. `false` is the correct
 *      answer and the outbox leaves the record alone.
 *   2. **The database would not answer.** Same `false`, because the port has no
 *      third value. This is the case that deserves attention: it is a `false`
 *      that means "nobody knows", and the outbox treats it as exactly that —
 *      the record stays where it is, and recovery is a status read, never a
 *      second send.
 *   3. **A row came back and does not check out.** Also `false`.
 *
 * The port's boolean cannot distinguish these, and that is the port's own
 * choice, so this adapter does not pretend otherwise. What it must never do is
 * return `true` for any of them: a `true` on an error would leave a record
 * claiming a state nobody wrote, and a `true` on a lost CAS would tell the
 * outbox that its write won when it did not — which, combined with the outbox's
 * rule that a settlement implies the send happened, is a fabricated receipt.
 *
 * ## What is never stored
 *
 * No recipient, no template id or provider template id, no sender address, no
 * message body, no provider credential, and no fingerprint key. The recipient is
 * exactly what the port's own docstring says a durable store has no reason to
 * hold; this adapter holds the digest and nothing else. The pipeline's raw
 * idempotency key may contain inputs, so the SQL key is its separate HMAC.
 */

import type {
  EmailAttemptClaim,
  EmailAttemptRecord,
  EmailAttemptSettlement,
  EmailAttemptStore,
  EmailAttemptOwnership,
} from '@archava/act-email'
import { claimAttempt, lookupAttempt, settleAttempt, type AttemptRow } from './core.js'
import type { SqlClient } from './sql.js'
import { attemptStorageKey, copyStorageKeySecret } from './storage-key.js'

/**
 * The action_kind this adapter files under, distinct from the booking adapter's
 * so that a `(tenant, booking, key)` row and a `(tenant, email, key)` row are
 * two attempts rather than one collision.
 */
export const EMAIL_ACTION_KIND = 'email'

/** Options the server composition supplies. */
export interface PostgresEmailAttemptStoreOptions {
  /** The injected, parameterized database client. Brings its own driver. */
  readonly client: SqlClient
  /** Stable server secret; rotating it requires migrating existing attempt keys. */
  readonly idempotencyKeyHmacKey: Uint8Array
}

export class PostgresEmailAttemptStore implements EmailAttemptStore {
  /** Composed for the audit trail: names the database, not itself. */
  readonly storeId: string
  readonly #client: SqlClient
  readonly #idempotencyKeyHmacKey: Uint8Array

  constructor(options: PostgresEmailAttemptStoreOptions) {
    this.#client = options.client
    this.#idempotencyKeyHmacKey = copyStorageKeySecret(options.idempotencyKeyHmacKey)
    this.storeId = `postgres(${options.client.storeId})`
  }

  async find(owner: EmailAttemptOwnership): Promise<EmailAttemptRecord | null> {
    const reading = await lookupAttempt(this.#client, {
      tenantId: owner.tenantId,
      actionKind: EMAIL_ACTION_KIND,
      idempotencyKey: this.storageKey(owner.tenantId, owner.idempotencyKey),
    })
    if (reading.status === 'empty') return null
    if (reading.status === 'replayed') return emailRecord(reading.row, owner.idempotencyKey)
    throw new Error(storeFailure(reading))
  }

  async claim(attempt: EmailAttemptRecord): Promise<EmailAttemptClaim> {
    const reading = await claimAttempt(this.#client, {
      tenantId: attempt.tenantId,
      actionKind: EMAIL_ACTION_KIND,
      idempotencyKey: this.storageKey(attempt.tenantId, attempt.idempotencyKey),
      fingerprint: attempt.fingerprint,
      openingState: 'prepared',
    })
    if (reading.status === 'claimed') return { status: 'accepted' }
    if (reading.status === 'replayed') {
      return { status: 'replayed', attempt: emailRecord(reading.row, attempt.idempotencyKey) }
    }
    // A claim that could not be answered is a claim this caller does not have.
    // The outbox wraps `claim` in a try/catch and reports `unknown`, which is
    // the only honest answer: a store that cannot say whether a key is taken
    // cannot promise at most one message.
    throw new Error(storeFailure(reading))
  }

  /**
   * Settle only if the record is still `prepared`.
   *
   * `true` for a write that landed; `false` for a CAS loss, an unavailable
   * database, or a row that does not check out. See the header for why those
   * three share a return value and why none of them is allowed to be `true`.
   */
  async settle(settlement: EmailAttemptSettlement): Promise<boolean> {
    const reading = await settleAttempt(this.#client, {
      tenantId: settlement.tenantId,
      actionKind: EMAIL_ACTION_KIND,
      idempotencyKey: this.storageKey(settlement.tenantId, settlement.idempotencyKey),
      fromState: settlement.from,
      toState: settlement.state,
      providerId: settlement.providerMessageId ?? null,
    })
    if (reading.status === 'settled') return true
    return false
  }

  private storageKey(tenantId: string, rawIdempotencyKey: string): string {
    return attemptStorageKey(
      this.#idempotencyKeyHmacKey,
      tenantId,
      EMAIL_ACTION_KIND,
      rawIdempotencyKey,
    )
  }
}

/**
 * The state as the email port spells it, or a throw.
 *
 * The row's `action_kind` has already been checked to be `email`, so a row
 * arriving here carrying a booking state is the schema or the query being wrong.
 * `prepared` and `in_flight` are not interchangeable names for the same thing,
 * and this does not translate between them.
 */
function emailRecord(row: AttemptRow, rawIdempotencyKey: string): EmailAttemptRecord {
  const state = row.state
  if (state !== 'prepared' && state !== 'accepted' && state !== 'rejected' && state !== 'unknown') {
    throw new Error('email_attempt_row_state_unusable')
  }
  const record: EmailAttemptRecord = {
    tenantId: row.tenantId,
    idempotencyKey: rawIdempotencyKey,
    fingerprint: row.fingerprint,
    state,
  }
  // The provider's message id is only present when the provider accepted, and
  // it is the authoritative idempotent receipt. A store that invented one here
  // would be claiming a send nobody made.
  if (state === 'accepted' && row.providerId !== null) {
    return { ...record, providerMessageId: row.providerId }
  }
  return record
}

/** What the outbox sees in its log. Never carries tenant data. */
function storeFailure(reading: { readonly status: string; readonly reason?: string }): string {
  if (reading.status === 'malformed') {
    return `email_attempt_row_${reading.reason ?? 'unusable'}`
  }
  if (reading.status === 'unavailable') return 'email_attempt_store_unavailable'
  return `email_attempt_store_${reading.status}`
}
