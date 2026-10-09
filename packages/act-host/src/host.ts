/**
 * The Act host — the only surface a visitor's envelope may reach.
 *
 * The seam this module holds is narrow and worth stating plainly. A browser is
 * not a peer: it is a caller that cannot be authenticated, cannot be believed
 * about who it is, and will eventually be operated by someone with devtools
 * open. So the host takes *two* kinds of input and treats them completely
 * differently.
 *
 * Server-owned inputs arrive in the constructor and cannot be changed per
 * request: the tenant, the session, the `ActionPolicy`, the capability tier, the
 * role, the entity resolver, the brain, the knowledge and structured-truth
 * ports, the composed tenant executor, the confirmation service, and the
 * attempt ledger. A browser asking to be served as another tenant, at another
 * tier, under another role, is not asking — it is misusing, and there is no
 * argument shape in which the answer is yes.
 *
 * Visitor-owned input arrives per request and is bounded by
 * {@link readVisitorEnvelope}: an utterance, exactly one action candidate, and
 * optionally the two strings a challenge minted. Everything else the body
 * carried is dropped and reported with a safe known name or generic marker.
 *
 * What the host does with them, in two calls:
 *
 * - `review` asks the gate whether a candidate action is eligible, validates
 *   its inputs against the registered contract, and — only for an action the
 *   policy answered `confirmation_required` for — mints exactly one challenge
 *   bound to the server-owned tenant, session and action id plus the digest of
 *   exactly the validated inputs, and records the attempt with the `occurredAt`
 *   the challenge was issued under.
 * - `present` spends that challenge. It re-reads the envelope, looks the
 *   attempt up by challenge id rather than by anything the visitor named,
 *   re-validates the presented inputs, verifies the challenge against the
 *   server-owned identity, and only then hands `runTurn` a request whose
 *   `requestedActions` is the one bound action and nothing else.
 *
 * `runTurn` remains the final gate. The host does not replace the policy, the
 * §9 validation, or entity resolution — it makes sure they are asked with the
 * right question. An action that survives to the executor has been asked three
 * times: once here, once in the service, once in the turn.
 *
 * The `occurredAt` guarantee lives or dies in one line, in
 * {@link ActHost.present}: the attempt's own recorded timestamp, read back from
 * the ledger. The host never calls its clock on the bound path. The clock it is
 * constructed with exists for `availableActions`, which runs a read-only probe
 * turn and produces no attempt at all.
 */

import type { ContextGraph } from '@archava/core'
import { assertTenant } from '@archava/core'
import type { ActionPolicy, ConfirmationMode, RoleName } from '@archava/acl'
import type { CapabilityTierName } from '@archava/config'
import type {
  ActionExecutor,
  EntityResolver,
  KnowledgePort,
  StructuredTruthPort,
  ExecutionState,
  TurnOutcome,
} from '@archava/assistant'
import { runTurn, validateActionInputs } from '@archava/assistant'
import type { BrainActionRequest, BrainProvider, BrainReply, BrainTurn } from '@archava/adapters'
import type {
  ConfirmationChallenge,
  ConfirmationRefusal,
  ConfirmationService,
} from '@archava/act-confirmation'

import {
  attemptIdentityKey,
  type ActHostAttemptIdentity,
  type ActHostAttemptLedger,
  type ActHostAttemptRecord,
} from './attempts.js'
import {
  VisitorEnvelopeError,
  readVisitorEnvelope,
  type VisitorEnvelope,
  type VisitorEnvelopeReading,
} from './envelope.js'

/**
 * The one thing a visitor is asked to confirm, reduced to the page.
 *
 * `token` is here because the visitor is the only party who can carry it back:
 * a host that kept it would have nothing to present, and a host that logged it
 * would have written a live credential into a place backups outlive. It is
 * returned once, to the caller, and this package never sees it again.
 */
export interface ActHostChallenge {
  readonly challengeId: string
  readonly token: string
  /** When the challenge stops binding, as the service reported it. */
  readonly expiresAt: number
  /** The gate's own wording for what is being confirmed, verbatim. */
  readonly prompt: string
  /** Which kind of consent the gate asked for. */
  readonly mode: ConfirmationMode
}

/** The result of asking whether an action may be confirmed. */
export type ActHostReview =
  | {
      readonly status: 'challenge_issued'
      readonly challenge: ActHostChallenge
      /** The `occurredAt` this attempt is bound to, for good. */
      readonly occurredAt: string
    }
  | { readonly status: 'refused'; readonly refusal: ActHostRefusal }

/** The only result a visitor-facing transport may serialize after presentation. */
export interface ActHostPresentation {
  readonly actionId: string
  readonly execution: ExecutionState
  readonly errorCode?: 'provider_outcome_unknown' | 'action_not_completed'
  /** Derived from the executor's result, never from pre-execution Brain prose. */
  readonly text: string
}

/** Why the host would not proceed. Every case is a distinct sentence. */
export const ACT_HOST_REFUSAL_CODES = [
  // The visitor's envelope was not one of the four known shapes.
  'visitor_envelope_invalid',
  // The gate answered something other than `confirmation_required`.
  'visitor_action_not_permitted',
  // The gate wants a person, not a visitor. A browser click is not an approval.
  'visitor_human_approval_required',
  // The gate would allow this action outright; there is nothing to confirm.
  'visitor_confirmation_not_required',
  // The inputs do not satisfy the action's registered contract, or name an id
  // an authoritative resolver could not answer for.
  'visitor_inputs_rejected',
  // A presentation arrived with no confirmation to spend.
  'visitor_confirmation_required',
  // A challenge was presented for something other than the attempt it belongs
  // to: another action, another session, or inputs that changed.
  'visitor_challenge_mismatch',
  // A challenge that had already been spent was presented again.
  'visitor_challenge_already_spent',
  // A challenge past its expiry.
  'visitor_challenge_expired',
  // Every other refusal the confirmation service itself reported.
  'confirmation_refused',
  // No executor is wired, so no effect may be claimed.
  'executor_unavailable',
  // A presentation named a challenge this host has no record of.
  'attempt_record_missing',
  'attempt_record_unavailable',
  'attempt_transition_refused',
] as const
export type ActHostRefusalCode = (typeof ACT_HOST_REFUSAL_CODES)[number]

/**
 * A refusal. Carries a code, a sentence an operator can read, and — when the
 * refusal came from somewhere deeper — that layer's own code.
 *
 * Input validation details are deliberately projected to one generic reason.
 * The validator can quote an unknown field name or an unresolved entity value,
 * both of which are visitor-controlled and must stay server-side.
 */
export class ActHostRefusal extends Error {
  readonly code: ActHostRefusalCode
  /** The confirmation service's own reason, when that is where it came from. */
  readonly rejection?: ConfirmationRefusal
  /** Public-safe validation detail; never a raw validator reason. */
  readonly fields?: readonly ActHostFieldRejection[]

  constructor(
    code: ActHostRefusalCode,
    message: string,
    detail?: {
      readonly rejection?: ConfirmationRefusal
      readonly fields?: readonly ActHostFieldRejection[]
    },
  ) {
    super(message)
    this.name = 'ActHostRefusal'
    this.code = code
    if (detail?.rejection !== undefined) this.rejection = detail.rejection
    if (detail?.fields !== undefined) this.fields = detail.fields
  }
}

/** Everything the host cannot accept from a request. */
export interface ActHostOptions {
  readonly tenantId: string
  readonly sessionId: string
  readonly policy: ActionPolicy
  readonly capability: CapabilityTierName
  readonly role: RoleName
  readonly brain: BrainProvider
  readonly knowledge: KnowledgePort
  readonly truth?: StructuredTruthPort
  readonly resolver?: EntityResolver
  /** Absent means no claimed effect. The host refuses rather than improvising. */
  readonly executor?: ActionExecutor
  readonly confirmations: ConfirmationService
  /** Required: the host cannot infer a safe retry identity after a restart. */
  readonly attempts: ActHostAttemptLedger
  /** Extra field names the tenant considers sensitive, on top of the action's own. */
  readonly clientSensitiveFields?: readonly string[]
  readonly retrievalLimit?: number
  /**
   * The wall clock, owned by the deployment.
   *
   * Used by {@link ActHost.availableActions} and nowhere else. The bound path —
   * `review` and `present` — reads `occurredAt` off the challenge, because a
   * retry that re-derived it would mint a second idempotency key from a first
   * booking. See the module header.
   */
  readonly clock: () => number
}

/** A page context plus an envelope: what `review` and `present` are handed. */
export interface ActHostRequest {
  /** Untrusted, bounded by {@link readVisitorEnvelope}. */
  readonly envelope: unknown
  /** The page context, as the SDK recorded it. Server-owned per call. */
  readonly graph: ContextGraph
  /** Opaque predecessor, parsed by the trusted host. Never permission or consent. */
  readonly previousIntentId?: string
}

/** A public-safe validation reason; `*` avoids echoing visitor field names. */
export interface ActHostFieldRejection {
  readonly field: string
  readonly reason: string
}

/**
 * The gate request for one action on this session's page.
 *
 * `availableOnPage` is the host's own reading of the graph, so the gate — not
 * the browser — decides whether the page really offers the action. `confirmed`
 * and `humanApproved` are both false on purpose: this is the question "would
 * this action need consent", and answering it with consent already in hand
 * would make `review` unable to tell an action that needs confirming from one
 * that does not.
 */
export function configFor(
  policy: ActionPolicy,
  request: {
    readonly actionId: string
    readonly capability: CapabilityTierName
    readonly role: RoleName
    readonly availableOnPage: boolean
  },
) {
  return policy.evaluate({
    actionId: request.actionId,
    capability: request.capability,
    role: request.role,
    availableOnPage: request.availableOnPage,
    confirmed: false,
    humanApproved: false,
  })
}

/**
 * A brain that asks for the bound action and nothing else.
 *
 * The tenant's real brain still runs, but its `requestedActions` list is
 * replaced wholesale by the one validated visitor candidate. This can add that
 * candidate even when the brain requested none; it also removes every other
 * model candidate. The policy and validator in `runTurn` remain final gates.
 */
function boundBrain(inner: BrainProvider, bound: readonly BrainActionRequest[]): BrainProvider {
  return {
    providerId: `${inner.providerId}:act-host-bound`,
    model: inner.model,
    health: inner.health,
    reply: async (turn: BrainTurn): Promise<BrainReply> => {
      const reply = await inner.reply(turn)
      return { ...reply, requestedActions: bound }
    },
  }
}

/** A brain that asks for nothing. Used only by the read-only catalog probe. */
const SILENT_BRAIN: BrainProvider = {
  providerId: 'act-host-probe',
  model: 'act-host-probe@1',
  health: { ready: true, reason: null },
  reply: (): Promise<BrainReply> =>
    Promise.resolve({
      text: '',
      requestedActions: [],
      citations: [],
      deferToStructuredTruth: false,
    }),
}

/** The host. One instance per tenant session, built from server-owned input. */
export class ActHost {
  private readonly options: ActHostOptions
  private readonly attempts: ActHostAttemptLedger

  constructor(options: ActHostOptions) {
    if (options.attempts === undefined || options.attempts === null) {
      throw new TypeError('an Act host needs an attempt ledger')
    }
    this.options = options
    this.attempts = options.attempts
  }

  /** The tenant this host serves. Never negotiable. */
  get tenantId(): string {
    return this.options.tenantId
  }

  /** The session this host serves. Never negotiable. */
  get sessionId(): string {
    return this.options.sessionId
  }

  /**
   * Which actions this session may currently confirm, as one turn sees them.
   *
   * A probe turn, on a brain that asks for nothing: the policy ∩ page
   * intersection stays in `turn.ts`, where the rest of the pipeline reads it, so
   * this answer cannot drift from the answer a real turn gives. It produces no
   * attempt, no challenge and no execution, which is why it is the one call here
   * allowed to look at a clock.
   */
  async availableActions(graph: ContextGraph): Promise<readonly string[]> {
    const outcome = await this.runProbe(graph)
    return outcome.permittedActionIds
  }

  /**
   * Whether this candidate has a claim on a confirmation, and the challenge if
   * so. Never executes anything.
   */
  async review({ envelope, graph, previousIntentId }: ActHostRequest): Promise<ActHostReview> {
    let reading: VisitorEnvelopeReading
    try {
      reading = readVisitorEnvelope(envelope)
    } catch (error) {
      if (error instanceof VisitorEnvelopeError) {
        throw new ActHostRefusal(
          'visitor_envelope_invalid',
          `The envelope is not one of the shapes a visitor may send (${error.reason}).`,
        )
      }
      throw error
    }
    assertTenant(graph, this.options.tenantId)
    const candidate = reading.envelope.action

    const gate = configFor(this.options.policy, {
      actionId: candidate.actionId,
      capability: this.options.capability,
      role: this.options.role,
      availableOnPage: pageOffers(graph, candidate.actionId),
    })
    // An administrative action is not refused because this visitor lacks a
    // role. It is refused because no browser may ever be the approving party,
    // and the only reason it is a refusal rather than a confirmation prompt is
    // that the ladder denies L5 before it ever offers the `human_approval` mode
    // — both arrive here on the same path, and both mean: an operator decides.
    if (gate.decision === 'denied' && gate.reason === 'admin_action_requires_human') {
      throw new ActHostRefusal(
        'visitor_human_approval_required',
        `The requested action needs an operator's approval: ${gate.message}`,
      )
    }
    if (gate.decision === 'denied') {
      throw new ActHostRefusal(
        'visitor_action_not_permitted',
        `The gate refused the requested action for this session (${gate.reason}).`,
      )
    }
    if (gate.decision === 'allow') {
      throw new ActHostRefusal(
        'visitor_confirmation_not_required',
        'The requested action is permitted without confirmation, so there is nothing to confirm.',
      )
    }
    // A visitor's click is not an operator's approval, and no amount of wishing
    // makes it one. The frozen authority separates the two: consent is a
    // server-verified one-time confirmation, and an L3+ action is a human
    // decision. Refusing here is the only place that distinction holds.
    if (gate.mode === 'human_approval') {
      throw new ActHostRefusal(
        'visitor_human_approval_required',
        `The requested action needs an operator's approval: ${gate.prompt}`,
      )
    }

    const validated = await validateActionInputs({
      actionId: candidate.actionId,
      inputs: candidate.inputs,
      tenantId: this.options.tenantId,
      ...(this.options.resolver === undefined ? {} : { resolver: this.options.resolver }),
    })
    if (!validated.ok) {
      throw new ActHostRefusal('visitor_inputs_rejected', 'The submitted inputs were refused.', {
        fields: SAFE_INPUT_REJECTION,
      })
    }

    // The attempt is bound to the moment it was first opened and never again
    // touched. That is the guarantee: a timeout, a page reload, a re-submitted
    // form — all of them read the same `occurredAt`, and therefore mint the same
    // idempotency key for the same booking.
    //
    // Operational ports open atomically and return the persisted identity;
    // the legacy read/save path below remains offline compatibility. A challenge
    // minted 90 seconds after the original is a new challenge for the *same*
    // attempt, not a new attempt — only its id and expiry change. Derived from
    // the clock instead, that same retry would mint a second key, which is
    // exactly the shape of a timeout turning one booking into two.
    const challenge = await this.mint(candidate.actionId, validated.inputs)
    const identity: ActHostAttemptIdentity = {
      tenantId: this.options.tenantId,
      sessionId: this.options.sessionId,
      actionId: candidate.actionId,
      inputDigest: challenge.inputDigest,
    }
    let occurredAt: string
    if (this.attempts.open) {
      const canonical = await this.attempts.open(
        {
          ...identity,
          occurredAt: new Date(challenge.issuedAt).toISOString(),
          challengeId: challenge.id,
          expiresAt: challenge.expiresAt,
        },
        previousIntentId,
      )
      if (
        canonical.tenantId !== identity.tenantId ||
        canonical.sessionId !== identity.sessionId ||
        canonical.actionId !== identity.actionId ||
        canonical.inputDigest !== identity.inputDigest ||
        canonical.challengeId !== challenge.id ||
        canonical.expiresAt !== challenge.expiresAt ||
        !Number.isSafeInteger(Date.parse(canonical.occurredAt))
      )
        throw new ActHostRefusal(
          'attempt_record_missing',
          'The canonical attempt could not be verified.',
        )
      occurredAt = canonical.occurredAt
    } else {
      if (previousIntentId !== undefined)
        throw new ActHostRefusal(
          'attempt_record_missing',
          'A durable attempt ledger is required for a new intent.',
        )
      const existing = await this.attempts.find(identity)
      occurredAt = existing?.occurredAt ?? new Date(challenge.issuedAt).toISOString()
      await this.attempts.save({
        ...identity,
        occurredAt,
        challengeId: challenge.id,
        expiresAt: challenge.expiresAt,
      })
    }

    return {
      status: 'challenge_issued',
      challenge: {
        challengeId: challenge.id,
        token: challenge.token,
        expiresAt: challenge.expiresAt,
        prompt: gate.prompt,
        mode: gate.mode,
      },
      occurredAt,
    }
  }

  /**
   * Ask the service for one challenge, bound by the server and not by the page.
   *
   * Everything the binding is made of — tenant, session, action id, inputs — is
   * assembled here from server-owned state and already-validated content. The
   * only thing the visitor contributed is the inputs, and those were refused
   * wholesale by {@link validateActionInputs} moments ago.
   *
   * The service computes the input digest, so this host never holds a digest key
   * and never sees a digest it could be tempted to compare. It takes the digest
   * off the returned challenge purely to file the attempt under it.
   */
  private async mint(
    actionId: string,
    inputs: Readonly<Record<string, unknown>>,
  ): Promise<ConfirmationChallenge> {
    const issued = await this.options.confirmations.issue({
      tenantId: this.options.tenantId,
      sessionId: this.options.sessionId,
      actionId,
      inputs,
    })
    if (issued.status === 'rejected') {
      throw new ActHostRefusal(
        'confirmation_refused',
        'The confirmation service would not mint a challenge for this action.',
        { rejection: issued.reason },
      )
    }
    return issued.challenge
  }

  /**
   * Spend a challenge and run the one action it binds, through the final gates.
   *
   * Throws {@link ActHostRefusal} for every refusal, including the ones the
   * gate and the validator raise — because a caller that cannot tell "the
   * visitor may not do this" from "it worked" will report the second as the
   * first. A returned outcome is an outcome the gates were actually asked,
   * whatever it says.
   */
  async present({ envelope, graph }: ActHostRequest): Promise<ActHostPresentation> {
    let reading: VisitorEnvelopeReading
    try {
      reading = readVisitorEnvelope(envelope)
    } catch (error) {
      if (error instanceof VisitorEnvelopeError) {
        throw new ActHostRefusal(
          'visitor_envelope_invalid',
          `The envelope is not one of the shapes a visitor may send (${error.reason}).`,
        )
      }
      throw error
    }
    assertTenant(graph, this.options.tenantId)

    if (reading.envelope.confirmation === undefined) {
      throw new ActHostRefusal(
        'visitor_confirmation_required',
        'An execution needs a confirmation challenge the visitor has presented.',
      )
    }

    // The attempt is found by challenge id — the one thing in this request that
    // came from the service — and never by the action id the visitor named. A
    // candidate that arrives under the right challenge and the wrong action is
    // caught two lines down; a candidate that arrives under a challenge this
    // host never minted has nothing to catch it here but this.
    const attempt = await this.attempts.findByChallenge(reading.envelope.confirmation.challengeId)
    if (attempt === null) {
      throw new ActHostRefusal(
        'attempt_record_missing',
        'No attempt is recorded for that challenge.',
      )
    }
    if (
      attempt.tenantId !== this.options.tenantId ||
      attempt.sessionId !== this.options.sessionId
    ) {
      throw new ActHostRefusal(
        'visitor_challenge_mismatch',
        'That challenge belongs to a different tenant or session.',
      )
    }
    if (attempt.actionId !== reading.envelope.action.actionId) {
      throw new ActHostRefusal(
        'visitor_challenge_mismatch',
        'That challenge was issued for a different action.',
      )
    }

    // Re-validated, not trusted from `review`: the two calls are separated by a
    // network round trip in which anything can happen, and a payload that no
    // longer satisfies the contract must not be executed on a receipt minted
    // for one that did.
    const validated = await validateActionInputs({
      actionId: attempt.actionId,
      inputs: reading.envelope.action.inputs,
      tenantId: this.options.tenantId,
      ...(this.options.resolver === undefined ? {} : { resolver: this.options.resolver }),
    })
    if (!validated.ok) {
      throw new ActHostRefusal('visitor_inputs_rejected', 'The submitted inputs were refused.', {
        fields: SAFE_INPUT_REJECTION,
      })
    }

    const executor = this.options.executor
    if (executor === undefined) {
      throw new ActHostRefusal(
        'executor_unavailable',
        'No ActionExecutor is wired into this host, so no action may be run.',
      )
    }

    const verification = await this.options.confirmations.verify({
      challengeId: reading.envelope.confirmation.challengeId,
      token: reading.envelope.confirmation.token,
      tenantId: attempt.tenantId,
      sessionId: attempt.sessionId,
      actionId: attempt.actionId,
      inputs: validated.inputs,
    })
    if (verification.status === 'rejected') {
      throw new ActHostRefusal(
        confirmationCodeFor(verification.reason),
        `The confirmation service refused the presentation: ${verification.reason}`,
        { rejection: verification.reason },
      )
    }
    if (verification.receipt.inputDigest !== attempt.inputDigest)
      throw new ActHostRefusal(
        'visitor_challenge_mismatch',
        'The confirmation does not match its recorded intent.',
      )

    // The one line the whole module exists for. `attempt.occurredAt` was written
    // when the challenge was minted and has not been rewritten since — not by a
    // re-issue, not by a retry, not by the timeout that prompted one.
    const occurredAt = attempt.occurredAt

    const outcome = await runTurn({
      tenantId: attempt.tenantId,
      sessionId: attempt.sessionId,
      utterance: reading.envelope.utterance,
      graph,
      occurredAt,
      brain: boundBrain(this.options.brain, [
        { actionId: attempt.actionId, inputs: validated.inputs },
      ]),
      policy: this.options.policy,
      knowledge: this.options.knowledge,
      ...(this.options.truth === undefined ? {} : { truth: this.options.truth }),
      executor,
      ...(this.options.resolver === undefined ? {} : { resolver: this.options.resolver }),
      capability: this.options.capability,
      role: this.options.role,
      // The receipt is the confirmation. A browser that sent
      // `confirmedActionIds` in its envelope was dropped for it — this list is
      // built here, from a service-verified receipt, and nowhere else.
      confirmedActionIds: [verification.receipt.actionId],
      ...(this.options.clientSensitiveFields === undefined
        ? {}
        : { clientSensitiveFields: this.options.clientSensitiveFields }),
      ...(this.options.retrievalLimit === undefined
        ? {}
        : { retrievalLimit: this.options.retrievalLimit }),
    })
    return publicPresentation(outcome, attempt.actionId, graph.page.locale)
  }

  /** The identity key an attempt is stored under, for an audit trail. */
  identityOf(actionId: string, inputDigest: string): string {
    return attemptIdentityKey({
      tenantId: this.options.tenantId,
      sessionId: this.options.sessionId,
      actionId,
      inputDigest,
    })
  }

  /**
   * One read-only turn, to answer a question about the session and not the
   * visitor. See {@link ActHost.availableActions}.
   */
  private async runProbe(graph: ContextGraph): Promise<TurnOutcome> {
    return runTurn({
      tenantId: this.options.tenantId,
      sessionId: this.options.sessionId,
      utterance: '',
      graph,
      occurredAt: new Date(this.options.clock()).toISOString(),
      brain: SILENT_BRAIN,
      policy: this.options.policy,
      knowledge: this.options.knowledge,
      ...(this.options.truth === undefined ? {} : { truth: this.options.truth }),
      capability: this.options.capability,
      role: this.options.role,
    })
  }
}

function publicPresentation(
  outcome: TurnOutcome,
  actionId: string,
  locale: string,
): ActHostPresentation {
  const action =
    outcome.actions.length === 1 && outcome.actions[0]?.actionId === actionId
      ? outcome.actions[0]
      : undefined
  if (action?.policy === 'allowed' && action.execution === 'succeeded') {
    return {
      actionId,
      execution: 'succeeded',
      text: locale.startsWith('id')
        ? 'Permintaan Anda berhasil diproses.'
        : 'Your request was completed.',
    }
  }
  return {
    actionId,
    execution: action?.execution ?? 'not_attempted',
    errorCode:
      action?.errorCode === 'provider_outcome_unknown'
        ? 'provider_outcome_unknown'
        : 'action_not_completed',
    text: locale.startsWith('id')
      ? 'Saya belum bisa memastikan permintaan ini berhasil. Periksa statusnya sebelum mencoba lagi.'
      : 'I cannot confirm this request completed. Check its status before trying again.',
  }
}

/** The validator's field names and entity messages can contain visitor input. */
const SAFE_INPUT_REJECTION: readonly ActHostFieldRejection[] = [
  { field: '*', reason: 'The submitted inputs did not pass validation.' },
]

/**
 * Where a challenge id came from, as a refusal the host is willing to say.
 *
 * `wrong_binding` is the one a changed payload arrives as, and it gets the
 * mismatch code rather than the generic one, because the operator reading the
 * log needs to know the confirmation was fine and the request was not.
 */
function confirmationCodeFor(reason: ConfirmationRefusal): ActHostRefusalCode {
  if (reason === 'wrong_binding') return 'visitor_challenge_mismatch'
  if (reason === 'expired') return 'visitor_challenge_expired'
  if (reason === 'already_consumed') return 'visitor_challenge_already_spent'
  return 'confirmation_refused'
}

/**
 * Whether the page context offers an action, read off the graph.
 *
 * Deliberately not a client field: the host asks the graph, so a visitor who
 * claims the page offers `booking.create` has to have been served a page that
 * offers it.
 */
function pageOffers(graph: ContextGraph, actionId: string): boolean {
  const entry = graph.availableActions.find((action) => action.name === actionId)
  return entry !== undefined && entry.enabled
}

/** Re-exported so a caller catching one does not need the envelope module. */
export { VisitorEnvelopeError, readVisitorEnvelope }
export type { VisitorEnvelope, ActHostAttemptLedger, ActHostAttemptRecord }
