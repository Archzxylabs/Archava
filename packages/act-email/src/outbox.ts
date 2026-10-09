/**
 * The outbox and replay boundary around `EmailGateway`.
 *
 * The executor hands this boundary the same `EmailSendRequest` every time the
 * same turn is replayed, and the same `idempotencyKey` with it. What has to be
 * true, and what this file is for:
 *
 * 1. **One key, one message.** A key, once claimed, never reaches the provider
 *    twice. Not from a concurrent caller, not from a process that restarted
 *    mid-send, not from a visitor pressing the button twice.
 * 2. **The same key and the same payload replay.** The second caller gets the
 *    message id the provider issued, served from the record with no write
 *    anywhere. That id is the authoritative idempotent receipt: it is the one
 *    thing that says the message already went, and it came from the provider.
 * 3. **The same key and a different recipient or template is a conflict.** A key
 *    that arrives carrying a different address, template or sender is not a
 *    replay of anything; it is a bug or a forgery, and it is refused rather than
 *    served. A conflict never reads as a success.
 * 4. **An unknown outcome is never laundered into a success, and never re-sent.**
 *    If nobody knows how the first attempt ended, the next caller is told nobody
 *    knows. A replay may look again — through the provider's status, or through a
 *    receipt the provider already issued — but looking is the only thing it may
 *    do. Silence stays silence.
 * 5. **`accepted` is the provider's word only.** It means a transport accepted a
 *    message. It does not mean an inbox, a read, or a delivery: nothing in this
 *    package can see any of those, and the boundary that claimed otherwise would
 *    be claiming something it has no evidence for.
 *
 * The boundary implements `EmailGateway` itself, which is the point: an executor
 * cannot tell the difference between a raw gateway and a wrapped one, so the
 * existing public executor contract does not change and every caller that
 * already holds an executor keeps working.
 *
 * **This is server-side code.** The attempt store holds tenant-scoped identities
 * and the transport carries tenant credentials. Neither belongs in a browser
 * bundle, and neither is decided here.
 *
 * No per-request state is kept on the instance. Two callers may share one
 * boundary and interleave freely, and which attempt each is is carried in the
 * arguments rather than held where the other could overwrite it.
 */

import { createHmac } from 'node:crypto'
import {
  EMAIL_IDEMPOTENCY_CONFLICT,
  isAcceptedEmail,
  isEmailAddress,
  isNonblank,
  isProviderMessageId,
  isTrustedTemplateShape,
  type EmailGateway,
  type EmailGatewayOutcome,
  type EmailSendRequest,
} from './ports.js'

/** The tenant-scoped identity of one attempt. Every port call carries both. */
export interface EmailAttemptOwnership {
  readonly tenantId: string
  readonly idempotencyKey: string
}

/** Where an attempt stands, as the boundary recorded it. */
export type EmailAttemptState =
  /** Claimed, provider has not answered. A process that died here leaves it here. */
  | 'prepared'
  /** The provider accepted it, and the id is the provider's. */
  | 'accepted'
  /** The provider declined it. */
  | 'rejected'
  /** Nobody knows. The message may or may not have gone out. */
  | 'unknown'

/** One attempt, as the store holds it. */
export interface EmailAttemptRecord extends EmailAttemptOwnership {
  /**
   * A keyed digest of the payload, not the payload.
   *
   * A replay whose fingerprint does not match the stored one is a different
   * request wearing the same key. It is a digest and not a copy because a
   * recipient address is exactly the thing a durable store has no reason to
   * hold: the boundary needs to know that the payload *is the same*, and a
   * keyed digest answers that without storing who the message was for. The
   * fingerprint key must remain secret and stable while attempts can replay.
   */
  readonly fingerprint: string
  readonly state: EmailAttemptState
  /** Present only when `state` is `accepted`, and only ever provider-issued. */
  readonly providerMessageId?: string
}

/** What a store says when asked to claim a key. */
export type EmailAttemptClaim =
  /** Nobody had this key. This caller owns the only send it will make. */
  | { readonly status: 'accepted' }
  /** Somebody had it. Nothing further may be written; this is the record. */
  | { readonly status: 'replayed'; readonly attempt: EmailAttemptRecord }

/** The terminal state of an attempt, and the receipt that came with it. */
export interface EmailAttemptSettlement extends EmailAttemptOwnership {
  readonly state: Exclude<EmailAttemptState, 'prepared'>
  readonly providerMessageId?: string
  /**
   * The state the caller believes the record is in.
   *
   * This is what makes a settlement a compare-and-swap instead of a blind
   * write. A caller that gave up on a slow transport settles `unknown` from
   * `prepared`; another caller that got the provider's answer meanwhile
   * settles `accepted`, also from `prepared`. If the first write lands last and
   * the store takes it, the receipt is gone and the next replay is told nobody
   * knows about a message that did go out. A store that honours `from` refuses
   * that write, because the record is no longer in the state the write assumed.
   */
  readonly from: Exclude<EmailAttemptState, 'accepted' | 'rejected'>
}

/**
 * The durable outbox.
 *
 * **The contract no in-memory map satisfies:** `claim` must make "this key is
 * mine, everyone else reads it back" a single atomic step. Two processes calling
 * it at once must not both be told `accepted`. Nothing in this package decides
 * how that is done — a unique index on `(tenantId, idempotencyKey)` with an
 * insert-or-nothing is the usual answer, and it is the deployment's to make.
 *
 * And `settle` must be a compare-and-swap against `from`, reporting `true` only
 * after its write is durable. `false` means the state changed or the record was
 * absent. A settled `accepted` that a restart forgets is a receipt this
 * package cannot replay, which puts the next replay back on the status port or
 * into `unknown` for a message that really went out.
 *
 * Note what is *not* stored: no recipient, no template content, no provider
 * payload. The fingerprint covers the payload without copying it.
 */
export interface EmailAttemptStore {
  /** Which store this is, for the audit trail. Not a secret. */
  readonly storeId: string
  /** Read-only lookup; absence is never permission to send. Optional for legacy stores. */
  find?(owner: EmailAttemptOwnership): Promise<EmailAttemptRecord | null>
  /** Atomically assert that this tenant-scoped key is the caller's, or hand back the record that owns it. */
  claim(attempt: EmailAttemptRecord): Promise<EmailAttemptClaim>
  /** Return true only when the compare-and-swap was applied durably. */
  settle(settlement: EmailAttemptSettlement): Promise<boolean>
}

/** What the status port is asked. Nothing it could act on. */
export interface EmailDeliveryStatusLookup {
  readonly tenantId: string
  readonly idempotencyKey: string
  readonly fingerprint: string
}

/**
 * The read-only look at what a provider already decided.
 *
 * Deliberately narrow: the tenant, the key, and the digest of the attempt it
 * should be answering about. Not the recipient, not the template body, not the
 * sender. A port that could be handed a recipient could be talked into sending
 * one, and this port's whole job is to be the *reason* not to send. If it cannot
 * answer, the attempt stays `unknown` — which is the correct report, not a
 * failure of the port.
 */
export interface EmailDeliveryStatusPort {
  /** Which status port this is, for the audit trail. Not a secret. */
  readonly statusPortId: string
  status(lookup: EmailDeliveryStatusLookup): Promise<EmailGatewayOutcome>
}

export interface EmailOutboxBoundaryOptions {
  readonly gateway: EmailGateway
  readonly store: EmailAttemptStore
  readonly status: EmailDeliveryStatusPort
  /** Server-only secret of at least 32 bytes; use the same key across processes and restarts. */
  readonly fingerprintKey: Uint8Array
}

/**
 * Why a reply from a provider or a status port does not answer the question.
 * Machine reasons only, so that no provider text reaches a public payload.
 */
const MACHINE_REASON_FALLBACK = 'email_outcome_unexplained'
const SAFE_PROVIDER_REASONS = new Set([
  'provider_declined',
  'status_not_available',
  'still_waiting',
])
const PROVIDER_SILENT = 'email_provider_did_not_answer'
const RECEIPT_UNRECOGNISED = 'email_provider_receipt_unrecognised'
const NOT_RECONCILED = 'email_attempt_not_reconciled'
const STORE_UNAVAILABLE = 'email_attempt_store_unavailable'
const REQUEST_UNUSABLE = 'email_request_unusable'
const PREVIOUSLY_DECLINED = 'email_previously_declined'
const STORED_RECEIPT_UNUSABLE = 'email_stored_receipt_unusable'
const SETTLEMENT_UNCONFIRMED = 'email_attempt_settlement_unconfirmed'

export class EmailOutboxBoundary implements EmailGateway {
  private readonly gateway: EmailGateway
  private readonly store: EmailAttemptStore
  private readonly status: EmailDeliveryStatusPort
  private readonly fingerprintKey: Uint8Array

  constructor(options: EmailOutboxBoundaryOptions) {
    if (!(options.fingerprintKey instanceof Uint8Array) || options.fingerprintKey.byteLength < 32) {
      throw new TypeError('An email fingerprint key of at least 32 bytes is required.')
    }
    this.gateway = options.gateway
    this.store = options.store
    this.status = options.status
    this.fingerprintKey = Uint8Array.from(options.fingerprintKey)
  }

  /** Which ports this boundary is built from, for the audit trail. Not a secret. */
  get outboxId(): string {
    return `outbox(${this.store.storeId}->${this.status.statusPortId})`
  }

  /** Recover an existing attempt only. Never calls claim or the send gateway. */
  async recover(request: EmailSendRequest): Promise<EmailGatewayOutcome> {
    try {
      if (!sendable(request) || !this.store.find)
        return { outcome: 'unknown', reason: NOT_RECONCILED }
      const row = await this.store.find({
        tenantId: request.tenantId,
        idempotencyKey: request.idempotencyKey,
      })
      if (
        !row ||
        row.tenantId !== request.tenantId ||
        row.idempotencyKey !== request.idempotencyKey
      )
        return { outcome: 'unknown', reason: NOT_RECONCILED }
      return await this.replay(row, this.attemptFingerprint(request))
    } catch {
      return { outcome: 'unknown', reason: NOT_RECONCILED }
    }
  }

  async send(request: EmailSendRequest): Promise<EmailGatewayOutcome> {
    if (!sendable(request)) {
      // The provider is never consulted about a request this package cannot
      // vouch for, and an unapproved sender is one of them.
      return { outcome: 'rejected', reason: REQUEST_UNUSABLE }
    }
    const owner: EmailAttemptOwnership = {
      tenantId: request.tenantId,
      idempotencyKey: request.idempotencyKey,
    }
    const fingerprint = this.attemptFingerprint(request)

    let claim: EmailAttemptClaim | null
    try {
      claim = await this.store.claim({ ...owner, fingerprint, state: 'prepared' })
    } catch {
      claim = null
    }
    if (claim === null) {
      // A store that cannot answer whether the key is taken is a store that
      // cannot promise at most one message. Reporting a failure here would be a
      // lie about a send nobody made; reporting a success would be the double
      // send. Unknown is the honest answer, and nothing was sent.
      return { outcome: 'unknown', reason: STORE_UNAVAILABLE }
    }
    if (claim.status === 'replayed') return this.replay(claim.attempt, fingerprint)
    return this.issue(owner, request)
  }

  /**
   * This caller owns the attempt: make the one send and record how it ended.
   *
   * Every branch settles the record, including the ones that end badly. An
   * unsettled record is a record that looks in-flight to the next process, which
   * is the state that can only be recovered by a read.
   */
  private async issue(
    owner: EmailAttemptOwnership,
    request: EmailSendRequest,
  ): Promise<EmailGatewayOutcome> {
    let reply: unknown
    try {
      reply = await this.gateway.send(request)
    } catch {
      // The transport may have taken the message. Nothing may be said about it,
      // and nothing may be sent again.
      await this.settle(owner, { from: 'prepared', state: 'unknown' })
      return { outcome: 'unknown', reason: PROVIDER_SILENT }
    }

    const answer = readOutcome(reply)
    if (answer?.outcome === 'accepted') {
      // The id is the one the provider issued. Recording it is what makes the
      // next caller's replay answerable without asking the provider anything.
      // This direct receipt remains true even if the store cannot preserve it;
      // a later replay then has to reconcile instead of sending again.
      await this.settle(owner, {
        from: 'prepared',
        state: 'accepted',
        providerMessageId: answer.providerMessageId,
      })
      return answer
    }
    if (answer?.outcome === 'rejected') {
      const settled = await this.settle(owner, { from: 'prepared', state: 'rejected' })
      return settled ? answer : { outcome: 'unknown', reason: SETTLEMENT_UNCONFIRMED }
    }
    // A reply that names no usable message id has not said no — it has said
    // nothing, and it is recorded as nothing.
    await this.settle(owner, { from: 'prepared', state: 'unknown' })
    if (answer === null) return { outcome: 'unknown', reason: RECEIPT_UNRECOGNISED }
    return { outcome: 'unknown', reason: answer.reason }
  }

  /**
   * Somebody else owns this attempt. Serve what is already true.
   *
   * `accepted` and `rejected` are answered from the record with no write
   * anywhere. `prepared` and `unknown` are the recoverable cases, and the only
   * thing allowed to speak for them is a read.
   */
  private async replay(
    attempt: EmailAttemptRecord,
    fingerprint: string,
  ): Promise<EmailGatewayOutcome> {
    // Same key, different payload. This is not a replay of the attempt that owns
    // the key, so neither its answer nor a second send is available to it. The
    // check lives in the boundary and not only in the store, so a store that
    // answered loosely cannot turn a payload change into a replay.
    if (fingerprint !== attempt.fingerprint) {
      return { outcome: 'rejected', reason: EMAIL_IDEMPOTENCY_CONFLICT }
    }
    if (attempt.state === 'accepted') {
      // The authoritative idempotent receipt. If it is unusable the record is
      // not evidence of anything, so the attempt is not an acceptance.
      return isProviderMessageId(attempt.providerMessageId)
        ? { outcome: 'accepted', providerMessageId: attempt.providerMessageId }
        : { outcome: 'unknown', reason: STORED_RECEIPT_UNUSABLE }
    }
    if (attempt.state === 'rejected') {
      return { outcome: 'rejected', reason: PREVIOUSLY_DECLINED }
    }
    return this.resolve(attempt)
  }

  /**
   * The only path that may look again, and it is a read.
   *
   * A `prepared` or `unknown` attempt is one where the outcome is genuinely
   * open, so the question "did it go out?" is asked of the one party that could
   * know. When that party does not know either, the attempt stays `unknown`:
   * an unanswered lookup is not permission to send, and the boundary would
   * rather report "nobody knows" than a visitor receiving the same receipt twice.
   */
  private async resolve(attempt: EmailAttemptRecord): Promise<EmailGatewayOutcome> {
    // The only two states a recovery lookup is allowed to speak for. The other
    // two are already answered above, so a record reaching this line in one of
    // them is a store telling a story the boundary does not believe.
    const from: 'prepared' | 'unknown' = attempt.state === 'unknown' ? 'unknown' : 'prepared'
    let answer: EmailGatewayOutcome | null = null
    try {
      answer = readOutcome(
        await this.status.status({
          tenantId: attempt.tenantId,
          idempotencyKey: attempt.idempotencyKey,
          fingerprint: attempt.fingerprint,
        }),
      )
    } catch {
      answer = null
    }
    if (answer?.outcome === 'accepted') {
      const settled = await this.settle(attempt, {
        from,
        state: 'accepted',
        providerMessageId: answer.providerMessageId,
      })
      return settled ? answer : { outcome: 'unknown', reason: SETTLEMENT_UNCONFIRMED }
    }
    if (answer?.outcome === 'rejected') {
      if (from === 'prepared') {
        // The original send may still be in flight. A status-port refusal must
        // not freeze the record as rejected before its direct receipt arrives.
        // A crashed prepared attempt remains conservatively unknown until it
        // can be reconciled through a stronger provider receipt.
        return { outcome: 'unknown', reason: NOT_RECONCILED }
      }
      const settled = await this.settle(attempt, { from, state: 'rejected' })
      return settled ? answer : { outcome: 'unknown', reason: SETTLEMENT_UNCONFIRMED }
    }
    if (answer === null) return { outcome: 'unknown', reason: NOT_RECONCILED }
    return { outcome: 'unknown', reason: answer.reason }
  }

  /**
   * Write a settlement, reporting whether it became the durable record.
   *
   * The send is not undone by a store that would not take the write: the record
   * simply stays where it was, and the next replay asks the status port instead
   * of asking the provider again. The current call remains unknown rather than
   * contradicting a record another caller may have settled.
   */
  private async settle(
    owner: EmailAttemptOwnership,
    settlement: Omit<EmailAttemptSettlement, 'tenantId' | 'idempotencyKey'>,
  ): Promise<boolean> {
    try {
      return (await this.store.settle({ ...owner, ...settlement })) === true
    } catch {
      // Left where it was. Recovery is the status port's job, never a re-send.
      return false
    }
  }

  /** Stable only under the same server-side key, which a restart must retain. */
  private attemptFingerprint(request: EmailSendRequest): string {
    const template = request.template
    return createHmac('sha256', this.fingerprintKey)
      .update(
        JSON.stringify([
          request.tenantId,
          request.recipient,
          template.templateId,
          template.providerTemplateId,
          template.senderAddress,
        ]),
      )
      .digest('hex')
  }
}

/** The defensive half of the executor's own checks, for direct callers. */
function sendable(request: EmailSendRequest): boolean {
  return (
    isNonblank(request.tenantId) &&
    isNonblank(request.idempotencyKey) &&
    isEmailAddress(request.recipient) &&
    isTrustedTemplateShape(request.template) &&
    request.template.tenantId === request.tenantId
  )
}

/**
 * Read a reply that crossed a boundary. Anything this package cannot name is
 * not an answer, and a reply that does not answer is `unknown`.
 *
 * The provider's own `reason` text is flattened to a machine code or dropped:
 * a transport is free to put the recipient address in an error string, and this
 * function is where that stops being true of anything this package returns.
 */
function readOutcome(reply: unknown): EmailGatewayOutcome | null {
  if (isAcceptedEmail(reply)) return reply
  if (typeof reply !== 'object' || reply === null || Array.isArray(reply)) return null
  const candidate = reply as Record<string, unknown>
  if (candidate['outcome'] === 'rejected') {
    return { outcome: 'rejected', reason: statedReason(candidate['reason']) }
  }
  if (candidate['outcome'] === 'unknown') {
    return { outcome: 'unknown', reason: statedReason(candidate['reason']) }
  }
  return null
}

/** A known safe reason, or a fixed fallback. Never arbitrary provider text. */
function statedReason(reason: unknown): string {
  if (typeof reason !== 'string') return MACHINE_REASON_FALLBACK
  return SAFE_PROVIDER_REASONS.has(reason) ? reason : MACHINE_REASON_FALLBACK
}
