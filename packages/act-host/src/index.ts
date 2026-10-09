/**
 * The trusted server host that turns a bounded visitor envelope into a
 * confirmed, executed Act action.
 *
 * Everything a browser can contribute fits in one small, boring object, and
 * this package is the code that keeps it that way. What it does is narrow by
 * design, and each module owns one wall:
 *
 * - `envelope.ts` reads an untrusted value as exactly four known shapes and
 *   drops — with a safe known name or generic marker — anything a visitor might have sent that
 *   only the server owns: the tenant, the session, the time, the permission, the
 *   capability, the role, a policy verdict, a `DecisionProvider`'s confidence, or
 *   a tool-call payload.
 * - `host.ts` takes the server-owned substitutes as constructor arguments, asks
 *   the policy whether the action would even need consent, runs the tenant's own
 *   registered-input validation, and only then asks
 *   `@archava/act-confirmation` for one challenge bound to the server-owned
 *   tenant, session, action id and exactly those validated inputs. Spending it
 *   goes through the same service, and the receipt — not the browser — is what
 *   reaches `runTurn` as `confirmedActionIds`.
 * - `attempts.ts` remembers the `occurredAt` an attempt was opened under, so a
 *   retry after a timeout derives the same idempotency key rather than a second
 *   one. The port is async and injection is required; a deployment needs a
 *   durable ledger to keep this identity across a restart.
 *
 * `runTurn` still runs in full on the execution path. `ActionPolicy`,
 * `validateActionInputs` and the entity resolver are the last gates, not
 * optimizations this host replaces. What this package adds is that a request
 * arriving from a browser cannot skip them.
 *
 * What it is not: a route, a framework, a credential store, or a PMS. It holds
 * no provider keys, records no raw form values, and puts neither in its refusals
 * or its attempt records.
 */

export {
  ActHost,
  ActHostRefusal,
  ACT_HOST_REFUSAL_CODES,
  configFor,
  type ActHostChallenge,
  type ActHostFieldRejection,
  type ActHostOptions,
  type ActHostPresentation,
  type ActHostRefusalCode,
  type ActHostRequest,
  type ActHostReview,
} from './host.js'

export {
  SERVER_OWNED_ENVELOPE_FIELDS,
  VISITOR_ENVELOPE_REASONS,
  VisitorEnvelopeError,
  readVisitorEnvelope,
  type VisitorActionCandidate,
  type VisitorConfirmationPresentation,
  type VisitorEnvelope,
  type VisitorEnvelopeReading,
  type VisitorEnvelopeReason,
} from './envelope.js'

export {
  InMemoryActHostAttemptLedger,
  attemptIdentityKey,
  type ActHostAttemptIdentity,
  type ActHostAttemptLedger,
  type ActHostAttemptRecord,
} from './attempts.js'

export * from './durable-attempts.js'
export * from './durable-host.js'
export * from './memory-durable-attempts.js'
