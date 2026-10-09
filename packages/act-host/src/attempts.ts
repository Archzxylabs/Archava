/**
 * The attempt ledger — the only thing that remembers a turn's time.
 *
 * `runTurn` takes an `occurredAt`, and an idempotency key is derived from it.
 * That makes the timestamp a *decision*, not a reading: derive it twice and the
 * same visitor action produces two idempotency keys, which is precisely how a
 * timeout after a booking turns into a second booking. The ledger exists so the
 * decision is made once, written down, and read back unchanged on every retry.
 *
 * What it stores is deliberately tiny, and the omissions are the design:
 *
 * - the identity — tenant, session, action id, and the **digest** of the exact
 *   validated inputs, which the confirmation service already computed and which
 *   this host never needs a digest key to compare;
 * - the `occurredAt` the attempt was opened under, in ISO-8601 UTC;
 * - the challenge id currently live for that attempt, so a presentation can be
 *   routed back to the timestamp it belongs to.
 *
 * What it does not store: the inputs, the token, the utterance, the policy
 * verdict, or the executor's output. Nothing here is a place a value the visitor
 * typed can end up, which is what makes an attempt record safe to log and safe
 * to hand to a store another subsystem reads.
 *
 * The port is async and the host requires an explicit implementation. The
 * in-memory implementation below is for tests and offline reference use; a
 * deployment needs a durable store to preserve retry identity across restarts.
 */

/** Which attempt a record is about. Two attempts differ in any part. */
export interface ActHostAttemptIdentity {
  readonly tenantId: string
  readonly sessionId: string
  readonly actionId: string
  /** The digest of the exact validated inputs. Never the inputs themselves. */
  readonly inputDigest: string
}

/**
 * One recorded attempt.
 *
 * `occurredAt` is written once, when the attempt is first opened, and never
 * rewritten — not by a re-issue, not by a retry, not by a timeout. Everything
 * downstream (the idempotency key, the record the visitor sees) is a function
 * of it.
 */
export interface ActHostAttemptRecord {
  readonly tenantId: string
  readonly sessionId: string
  readonly actionId: string
  /** The digest of the exact validated inputs. Never the inputs themselves. */
  readonly inputDigest: string
  /** ISO-8601 UTC. Stable for the life of the attempt. */
  readonly occurredAt: string
  /** The challenge currently live for this attempt. */
  readonly challengeId: string
  /** When that challenge stops binding, as the service reported it. */
  readonly expiresAt: number
}

/** Where attempt records live. Implement durably for a deployment. */
export interface ActHostAttemptLedger {
  readonly ledgerId: string
  /** Atomic canonical open for durable hosts. Legacy find/save is an offline compatibility path. */
  open?(record: ActHostAttemptRecord, previousIntentId?: string): Promise<ActHostAttemptRecord>
  save(record: ActHostAttemptRecord): Promise<void>
  find(identity: ActHostAttemptIdentity): Promise<ActHostAttemptRecord | null>
  findByChallenge(challengeId: string): Promise<ActHostAttemptRecord | null>
}

/**
 * The identity as a single comparable key.
 *
 * A JSON tuple retains field boundaries even when an identifier contains spaces
 * or separator characters. Distinct tenant/session pairs must never share an
 * attempt merely because their printable keys happen to look alike.
 */
export function attemptIdentityKey(identity: ActHostAttemptIdentity): string {
  return JSON.stringify([
    identity.tenantId,
    identity.sessionId,
    identity.actionId,
    identity.inputDigest,
  ])
}

/**
 * A ledger that holds records for the life of the process.
 *
 * It loses records on restart, so a same-attempt retry after a restart can open
 * a new `occurredAt`. Inject a durable implementation for production use.
 */
export class InMemoryActHostAttemptLedger implements ActHostAttemptLedger {
  readonly ledgerId = 'act-host-attempts-memory@1'
  private readonly byIdentity = new Map<string, ActHostAttemptRecord>()
  private readonly byChallenge = new Map<string, string>()

  save(record: ActHostAttemptRecord): Promise<void> {
    // A record is never mutated in place. The previous challenge id is retired
    // from the challenge index and the new one takes its place, so a
    // presentation of a spent challenge still resolves to the attempt it
    // belonged to.
    const identity = attemptIdentityKey(record)
    const previous = this.byIdentity.get(identity)
    if (previous !== undefined) this.byChallenge.delete(previous.challengeId)
    this.byIdentity.set(identity, record)
    this.byChallenge.set(record.challengeId, identity)
    return Promise.resolve()
  }

  find(identity: ActHostAttemptIdentity): Promise<ActHostAttemptRecord | null> {
    return Promise.resolve(this.byIdentity.get(attemptIdentityKey(identity)) ?? null)
  }

  findByChallenge(challengeId: string): Promise<ActHostAttemptRecord | null> {
    const identity = this.byChallenge.get(challengeId)
    if (identity === undefined) return Promise.resolve(null)
    return Promise.resolve(this.byIdentity.get(identity) ?? null)
  }
}
