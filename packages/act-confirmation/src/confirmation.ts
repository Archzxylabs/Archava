/**
 * The confirmation service: what counts as a confirmation, and what does not.
 *
 * A confirmation is the statement *this tenant, in this session, agreed to this
 * action with exactly these inputs, inside this window*. Everything below is one
 * rule protecting one clause of that sentence, and the file is laid out in that
 * order: the shape, the inputs, the binding, the expiry, the store, and the
 * re-check of what the store said.
 *
 * What this package deliberately does not do is decide whether the underlying
 * action is *allowed* — that is `ActionPolicy`'s job, and a confirmation is not a
 * permission — and it does not run the action. It answers one question and hands
 * back a receipt. What a caller does with the receipt is the caller's business.
 *
 * The guarantee this file exists to hold is: **a verified confirmation cannot be
 * produced by anything but presenting a token this service minted, for the exact
 * thing it was minted for, inside its window, the first time it was presented.**
 * Anything that fails one clause is refused, and a refusal is always a refusal of
 * the same published vocabulary — never a receipt with fewer fields.
 */

import {
  CHALLENGE_TOKEN_BYTES,
  defaultRandomBytes,
  digestBinding,
  digestInputs,
  digestReceipt,
  digestToken,
  digestsEqual,
  isDigestKey,
  newChallengeToken,
  newId,
  type DigestKey,
  type RandomBytes,
} from './digest.js'
import type {
  ConfirmationChallengeConsumeResult,
  ConfirmationChallengeInsert,
  ConfirmationChallengeRecord,
  ConfirmationChallengeStore,
} from './store.js'

/**
 * How long a challenge is good for.
 *
 * Five minutes is long enough to read a prompt and short enough that an abandoned
 * browser tab is not a live token. The bounds exist so a deployment cannot make a
 * confirmation that outlives the session it belongs to.
 */
export const DEFAULT_CONFIRMATION_TTL_MS = 300_000
export const MAX_CONFIRMATION_TTL_MS = 3_600_000
/** A confirmation that expires instantly is not a confirmation a guest can answer. */
const MIN_CONFIRMATION_TTL_MS = 1_000

/**
 * Every refusal this package can produce, and nothing else.
 *
 * Exported so a caller checks *against* this list rather than against whatever
 * string happens to be in the message this week.
 */
export const CONFIRMATION_REFUSALS = [
  'malformed_request',
  'malformed_token',
  'unsupported_inputs',
  'wrong_binding',
  'expired',
  'already_consumed',
  'store_conflict',
  'store_unavailable',
] as const

/** Why a confirmation was not produced. No `unknown`, no catch-all. */
export type ConfirmationRefusal = (typeof CONFIRMATION_REFUSALS)[number]

/** What a caller sends to have a challenge minted. */
export interface ConfirmationChallengeRequest {
  readonly tenantId: string
  readonly sessionId: string
  readonly actionId: string
  readonly inputs: Readonly<Record<string, unknown>>
}

/**
 * A challenge, returned exactly once per `issue`.
 *
 * `token` is the only copy of it this package will ever hold, and it is gone when
 * the caller's reference is. The store receives a digest.
 */
export interface ConfirmationChallenge {
  readonly id: string
  readonly token: string
  readonly tenantId: string
  readonly sessionId: string
  readonly actionId: string
  readonly inputDigest: string
  readonly bindingDigest: string
  readonly issuedAt: number
  readonly expiresAt: number
}

/** What a challenge that could not be minted looks like. */
export type ConfirmationChallengeIssue =
  | { readonly status: 'issued'; readonly challenge: ConfirmationChallenge }
  | { readonly status: 'rejected'; readonly reason: ConfirmationRefusal }

/** What a caller sends to have a confirmation verified. */
export interface ConfirmationPresentation {
  readonly challengeId: string
  readonly token: string
  readonly tenantId: string
  readonly sessionId: string
  readonly actionId: string
  readonly inputs: Readonly<Record<string, unknown>>
}

/** What a host can act on, log, or forward. */
export interface ConfirmationReceipt {
  readonly receiptId: string
  readonly challengeId: string
  readonly tenantId: string
  readonly sessionId: string
  readonly actionId: string
  /** The digest of the inputs — never the inputs themselves. */
  readonly inputDigest: string
  readonly bindingDigest: string
  readonly issuedAt: number
  readonly expiresAt: number
  readonly confirmedAt: number
  /** What a trace is allowed to carry: every fact, and none of the content. */
  readonly receiptDigest: string
}

export type ConfirmationVerification =
  | { readonly status: 'confirmed'; readonly receipt: ConfirmationReceipt }
  | { readonly status: 'rejected'; readonly reason: ConfirmationRefusal }

/** Correlation only. A binding description is neither consent nor an execution receipt. */
export type ConfirmationBindingDescription =
  | { readonly status: 'bound'; readonly inputDigest: string; readonly bindingDigest: string }
  | { readonly status: 'rejected'; readonly reason: ConfirmationRefusal }

/** The collaborators this service needs, all of them injectable. */
export interface ConfirmationServiceOptions {
  readonly store: ConfirmationChallengeStore
  /** The HMAC-SHA256 key every digest here is computed under. */
  readonly digestKey: DigestKey
  /** The time, as epoch milliseconds. Never read from the environment. */
  readonly clock: () => number
  /** Where a challenge token's randomness comes from. Defaults to `node:crypto`. */
  readonly random?: RandomBytes
  /** How long a challenge is good for, in milliseconds. */
  readonly ttlMs?: number
}

export class ConfirmationService {
  readonly #store: ConfirmationChallengeStore
  readonly #digestKey: DigestKey
  readonly #clock: () => number
  readonly #random: RandomBytes
  readonly #ttlMs: number

  constructor(options: ConfirmationServiceOptions) {
    if (!isDigestKey(options.digestKey)) {
      throw new TypeError('a confirmation needs a digest key of at least 32 bytes')
    }
    const ttlMs = options.ttlMs ?? DEFAULT_CONFIRMATION_TTL_MS
    if (
      !Number.isInteger(ttlMs) ||
      ttlMs < MIN_CONFIRMATION_TTL_MS ||
      ttlMs > MAX_CONFIRMATION_TTL_MS
    ) {
      throw new TypeError(
        `ttlMs must be an integer from ${MIN_CONFIRMATION_TTL_MS} to ${MAX_CONFIRMATION_TTL_MS}`,
      )
    }
    if (typeof options.clock !== 'function') {
      throw new TypeError('a confirmation needs a clock')
    }
    if (options.store === undefined || options.store === null) {
      throw new TypeError('a confirmation needs a store')
    }
    this.#store = options.store
    // A caller must not be able to rotate this service's key by mutating the
    // Uint8Array it passed to the constructor after a challenge was issued.
    this.#digestKey = new Uint8Array(options.digestKey)
    this.#clock = options.clock
    this.#random = options.random ?? defaultRandomBytes
    this.#ttlMs = ttlMs
  }

  /** Read-only binding for recovery: no challenge, token, store access or authority. */
  describeBinding(request: ConfirmationChallengeRequest): ConfirmationBindingDescription {
    try {
      if (request === null || typeof request !== 'object')
        return { status: 'rejected', reason: 'malformed_request' }
      const shape = requireShape(
        request.tenantId,
        request.sessionId,
        request.actionId,
        request.inputs,
      )
      if (shape !== null) return { status: 'rejected', reason: shape }
      const inputDigest = digestInputs(this.#digestKey, request.inputs)
      if (inputDigest === null) return { status: 'rejected', reason: 'unsupported_inputs' }
      return {
        status: 'bound',
        inputDigest,
        bindingDigest: digestBinding(
          this.#digestKey,
          request.tenantId,
          request.sessionId,
          request.actionId,
          inputDigest,
        ),
      }
    } catch {
      return { status: 'rejected', reason: 'unsupported_inputs' }
    }
  }

  /** Server verification of a receipt MAC. Database consumption must still be checked by the adapter. */
  verifyReceipt(receipt: ConfirmationReceipt): boolean {
    try {
      return (
        typeof receipt.receiptDigest === 'string' &&
        /^[a-f0-9]{64}$/.test(receipt.receiptDigest) &&
        Number.isSafeInteger(receipt.confirmedAt) &&
        receipt.confirmedAt >= receipt.issuedAt &&
        receipt.confirmedAt < receipt.expiresAt &&
        digestsEqual(digestReceipt(this.#digestKey, receipt), receipt.receiptDigest)
      )
    } catch {
      return false
    }
  }

  /**
   * Mint a challenge.
   *
   * The bindings are taken here, at the moment of the *request*, so a confirmation
   * and a later action agree on the same payload. An action that has drifted
   * cannot be confirmed: a confirmation for drifted inputs is a confirmation for
   * something that is no longer happening.
   *
   * The token never reaches the store, and it never comes back to this file.
   */
  async issue(request: ConfirmationChallengeRequest): Promise<ConfirmationChallengeIssue> {
    if (request === null || typeof request !== 'object') {
      return { status: 'rejected', reason: 'malformed_request' }
    }
    const shape = requireShape(
      request.tenantId,
      request.sessionId,
      request.actionId,
      request.inputs,
    )
    if (shape !== null) return { status: 'rejected', reason: shape }

    let inputDigest: string | null
    try {
      inputDigest = digestInputs(this.#digestKey, request.inputs)
    } catch {
      return { status: 'rejected', reason: 'unsupported_inputs' }
    }
    if (inputDigest === null) {
      // Refused rather than coerced: two different inputs that shared a digest
      // would both be confirmable with whichever was minted.
      return { status: 'rejected', reason: 'unsupported_inputs' }
    }

    const bindingDigest = digestBinding(
      this.#digestKey,
      request.tenantId,
      request.sessionId,
      request.actionId,
      inputDigest,
    )

    let id: string
    let token: string
    let issuedAt: number
    try {
      id = newId(this.#random)
      token = newChallengeToken(this.#random)
      issuedAt = this.#clock()
    } catch {
      return { status: 'rejected', reason: 'store_unavailable' }
    }
    const expiresAt = issuedAt + this.#ttlMs
    if (!Number.isSafeInteger(issuedAt) || !Number.isSafeInteger(expiresAt)) {
      return { status: 'rejected', reason: 'store_unavailable' }
    }
    const insert: ConfirmationChallengeInsert = {
      id,
      tenantId: request.tenantId,
      sessionId: request.sessionId,
      actionId: request.actionId,
      // The digest, never the token. A store row is a durable artefact, a backup
      // is a durable artefact, and neither is entitled to the thing that spends
      // a confirmation.
      tokenDigest: digestToken(this.#digestKey, token),
      bindingDigest,
      issuedAt,
      expiresAt,
    }

    let recorded: boolean
    try {
      recorded = (await this.#store.issue(insert)) === true
    } catch {
      // A store outage is a deployment fact, not a caller fact: the refusal says
      // the store was unavailable, and no token escapes.
      return { status: 'rejected', reason: 'store_unavailable' }
    }
    if (!recorded) {
      // No token exists where no record does. A challenge a store would not take
      // is not a challenge, and a caller learns that instead of holding a token
      // nothing could ever spend.
      return { status: 'rejected', reason: 'store_conflict' }
    }

    return {
      status: 'issued',
      challenge: {
        id,
        token,
        tenantId: request.tenantId,
        sessionId: request.sessionId,
        actionId: request.actionId,
        inputDigest,
        bindingDigest,
        issuedAt,
        expiresAt,
      },
    }
  }

  /**
   * Confirm a presentation, or refuse it.
   *
   * The order of the checks is the order of the clauses in the definition, and it
   * is not arbitrary:
   *
   * 1. Shape first, because there is nothing to talk about yet — a plain `yes`
   *    is not a malformed token that reached a store, it is a refusal decided
   *    before the store is asked anything.
   * 2. The inputs next: something without a canonical form is refused rather than
   *    digested, so it cannot borrow a digest that was never minted for it.
   * 3. The window after, enforced by the store as part of the same transition —
   *    so an expired presentation spends nothing, the record it cannot spend is
   *    left alone, and the refusal says `expired` rather than blaming the tokens.
   * 4. Exactly one store transition, last, because the store is the only thing
   *    that can make this state change — and it is asked once, never twice, so a
   *    store which *is* atomic is given something it can be atomic about.
   */
  async verify(presentation: ConfirmationPresentation): Promise<ConfirmationVerification> {
    const shape = requirePresentation(presentation)
    if (shape !== null) return { status: 'rejected', reason: shape }

    let inputDigest: string | null
    try {
      inputDigest = digestInputs(this.#digestKey, presentation.inputs)
    } catch {
      return { status: 'rejected', reason: 'unsupported_inputs' }
    }
    if (inputDigest === null) return { status: 'rejected', reason: 'unsupported_inputs' }

    const bindingDigest = digestBinding(
      this.#digestKey,
      presentation.tenantId,
      presentation.sessionId,
      presentation.actionId,
      inputDigest,
    )
    let now: number
    try {
      now = this.#clock()
    } catch {
      return { status: 'rejected', reason: 'store_unavailable' }
    }
    if (!Number.isSafeInteger(now)) return { status: 'rejected', reason: 'store_unavailable' }
    let receiptId: string
    try {
      receiptId = newId(this.#random)
    } catch {
      return { status: 'rejected', reason: 'store_unavailable' }
    }

    let consumed: ConfirmationChallengeConsumeResult
    try {
      consumed = await this.#store.consume({
        id: presentation.challengeId,
        tenantId: presentation.tenantId,
        sessionId: presentation.sessionId,
        actionId: presentation.actionId,
        tokenDigest: digestToken(this.#digestKey, presentation.token),
        bindingDigest,
        now,
      })
    } catch {
      return { status: 'rejected', reason: 'store_unavailable' }
    }

    // The store's answer is evidence, not authority. Everything past this line is
    // re-derived from the presentation and from the record, because a store that
    // answers `spent` for something it was not asked about is a store whose claim
    // cannot be believed — and one whose record cannot be trusted either.
    if (consumed === null || typeof consumed !== 'object' || !('status' in consumed)) {
      return { status: 'rejected', reason: 'store_unavailable' }
    }
    if (consumed.status === 'unavailable') {
      return { status: 'rejected', reason: 'store_unavailable' }
    }
    if (consumed.status === 'not_found') return { status: 'rejected', reason: 'wrong_binding' }
    if (consumed.status === 'already_consumed') {
      // A replay. Reported as one, because a challenge that was already spent is
      // a known fact about the presentation and hiding it behind a generic
      // refusal would leave a caller unable to tell a replay from a typo.
      return { status: 'rejected', reason: 'already_consumed' }
    }
    if (consumed.status === 'expired') {
      // The store applied the window and declined to spend. This is reported as
      // what it is, and it is safe to say: the window belongs to a challenge the
      // caller already presented, so nothing is revealed by admitting it. Unlike
      // `mismatch`, whose answer would confirm which challenge ids exist.
      return { status: 'rejected', reason: 'expired' }
    }
    if (consumed.status === 'mismatch') {
      return { status: 'rejected', reason: 'wrong_binding' }
    }
    if (consumed.status !== 'spent' || !('challenge' in consumed)) {
      return { status: 'rejected', reason: 'store_unavailable' }
    }

    const refusal = reconcile(
      this.#digestKey,
      presentation,
      consumed.challenge,
      bindingDigest,
      now,
      this.#ttlMs,
    )
    if (refusal !== null) return { status: 'rejected', reason: refusal }

    const spent = consumed.challenge
    const confirmedAt = spent.consumedAt ?? now
    return {
      status: 'confirmed',
      receipt: {
        receiptId,
        challengeId: spent.id,
        tenantId: spent.tenantId,
        sessionId: spent.sessionId,
        actionId: spent.actionId,
        inputDigest,
        bindingDigest,
        issuedAt: spent.issuedAt,
        expiresAt: spent.expiresAt,
        confirmedAt,
        receiptDigest: digestReceipt(this.#digestKey, {
          receiptId,
          challengeId: spent.id,
          tenantId: spent.tenantId,
          sessionId: spent.sessionId,
          actionId: spent.actionId,
          inputDigest,
          bindingDigest,
          issuedAt: spent.issuedAt,
          expiresAt: spent.expiresAt,
          confirmedAt,
        }),
      },
    }
  }
}

/**
 * Whether the record the store claims to have spent really is the one that was
 * presented, about what it was about.
 *
 * The identifiers, the credentials and the window are all re-derived. This is what
 * makes an overconfident store harmless: it can report a spend about a challenge
 * nobody presented, and the receipt that would have come out of it is still
 * refused, because the record contradicts the presentation in the first clause.
 */
function reconcile(
  key: DigestKey,
  presentation: ConfirmationPresentation,
  record: ConfirmationChallengeRecord,
  bindingDigest: string,
  now: number,
  ttlMs: number,
): ConfirmationRefusal | null {
  if (record === null || typeof record !== 'object') return 'store_unavailable'
  if (record.id !== presentation.challengeId) return 'wrong_binding'
  if (record.tenantId !== presentation.tenantId) return 'wrong_binding'
  if (record.sessionId !== presentation.sessionId) return 'wrong_binding'
  if (record.actionId !== presentation.actionId) return 'wrong_binding'

  // Credentials, compared only after both sides have been hashed, so a mismatch
  // cannot be timed into a guess about where the difference was. The token is
  // never compared raw: the store holds a digest, and the presentation's digest is
  // computed here under the service's own key so the two are the same thing.
  if (!digestsEqual(record.tokenDigest, digestToken(key, presentation.token))) {
    return 'wrong_binding'
  }
  if (!digestsEqual(record.bindingDigest, bindingDigest)) return 'wrong_binding'

  // State: consumed is the only state a spend is. A store that reports having
  // spent something still pending has not done the thing the contract asks of it.
  if (record.state !== 'consumed') return 'wrong_binding'

  // The window, checked from the record's own facts. A record that claims to have
  // been consumed before it was issued, or after it expired, describes something
  // impossible and is refused as expired rather than honoured.
  //
  // The upper bound is exclusive, and it has to be: `expiresAt` is `issuedAt` plus
  // the ttl, so the instant a challenge reaches its expiry is the instant it stops
  // being spendable. An inclusive bound here would let through a record claiming a
  // spend at exactly `expiresAt` — a record the store was asked to refuse. The
  // service refusing on its own clock is the same rule from a second source.
  if (
    !Number.isSafeInteger(record.issuedAt) ||
    !Number.isSafeInteger(record.expiresAt) ||
    typeof record.consumedAt !== 'number' ||
    !Number.isSafeInteger(record.consumedAt) ||
    record.expiresAt - record.issuedAt !== ttlMs
  ) {
    return 'store_unavailable'
  }
  if (record.consumedAt < record.issuedAt || record.consumedAt >= record.expiresAt) {
    return 'expired'
  }
  if (record.consumedAt > now) return 'store_unavailable'
  if (now < record.issuedAt || now >= record.expiresAt) return 'expired'

  return null
}

/**
 * Whether a value is a usable challenge token.
 *
 * The one place the raw token's shape is decided. 64 lowercase hex characters is
 * 256 bits, and a shorter string is not a partially-valid token — it is not a
 * token at all, which is why this is checked before the store is consulted.
 */
export function isChallengeToken(value: unknown): value is string {
  return (
    typeof value === 'string' &&
    value.length === CHALLENGE_TOKEN_BYTES * 2 &&
    /^[0-9a-f]+$/.test(value)
  )
}

function requireShape(
  tenantId: string,
  sessionId: string,
  actionId: string,
  inputs: unknown,
): ConfirmationRefusal | null {
  for (const field of [tenantId, sessionId, actionId]) {
    if (typeof field !== 'string' || field.trim().length === 0 || field.length > 256) {
      return 'malformed_request'
    }
  }
  // Inputs are a plain record. An array, a string, a class instance or a value
  // with no canonical form is a refusal, not something to normalise later.
  if (typeof inputs !== 'object' || inputs === null || Array.isArray(inputs)) {
    return 'malformed_request'
  }
  return null
}

function requirePresentation(presentation: ConfirmationPresentation): ConfirmationRefusal | null {
  if (typeof presentation !== 'object' || presentation === null) return 'malformed_request'
  if (
    typeof presentation.challengeId !== 'string' ||
    presentation.challengeId.length === 0 ||
    presentation.challengeId.length > 256
  ) {
    return 'malformed_request'
  }
  if (!isChallengeToken(presentation.token)) return 'malformed_token'
  return requireShape(
    presentation.tenantId,
    presentation.sessionId,
    presentation.actionId,
    presentation.inputs,
  )
}
