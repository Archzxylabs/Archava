/**
 * The deterministic turn pipeline (PRD §16–§21, §23, §25–§27).
 *
 * One function, `runTurn`, walks a single visitor turn end to end in a fixed
 * order. The order is the specification, so it is worth stating plainly:
 *
 *   1. **Tenant scope** (PRD §23). The graph is asserted against the requested
 *      tenant before anything reads it. A cross-tenant graph throws; it never
 *      quietly answers from the wrong tenant's page.
 *   2. **Structured truth** (PRD §17). The utterance is classified *before*
 *      retrieval, because "how much is this" must be answered from the live
 *      authoritative system rather than a chunk that was true last Tuesday.
 *   3. **The masking boundary** (PRD §16). The graph is projected to ids,
 *      labels, counts, and flags, then masked. Not the other way around: the
 *      projection is why no form value can reach a model, and the masker is why
 *      a name that turns out to be sensitive is still withheld.
 *   4. **The policy gate** (PRD §18). Available actions are computed once, and
 *      every action the brain asks for is re-evaluated against that gate. Page
 *      context appears here as a *narrowing* input and nothing else.
 *   5. **The brain**. A pure function of (turn, grounding, permitted actions),
 *      so a turn is reproducible given the same inputs.
 *   6. **Validated generative UI** (PRD §25). Whatever the brain selected is
 *      unvalidated input; what survives the registry's prop schemas is what the
 *      client draws. Nothing here is ever evaluated as code.
 *   7. **Handoff and analytics** (PRD §26, §27).
 *
 * Nothing in this module touches the network or the wall clock. Both arrive as
 * arguments, because a turn that reads either cannot be replayed — and a replay
 * is what makes a bug report answerable.
 */

import type {
  ActionDefinition,
  ActionPolicy,
  DenialReason,
  PolicyResult,
  RoleName,
} from '@archava/acl'
import { maskSensitiveFields, resolveMaskRules } from '@archava/acl'
import type { CapabilityTierName } from '@archava/config'
import type { DecisionOrchestrator } from '@archava/decision'
import type { ContextGraph } from '@archava/core'
import { assertTenant } from '@archava/core'
import type { BrainKnowledgeMode, BrainProvider, BrainReply } from '@archava/adapters'
import type {
  KnowledgeClassification,
  RetrievalContextRequest,
  RetrievalOutcome,
  StructuredTruthSubject,
} from '@archava/knowledge'
import { classifyKnowledgeNeed, STRUCTURED_TRUTH_SUBJECTS } from '@archava/knowledge'
import {
  buildComponents,
  filterComponentsByAllowedActions,
  type GenerativeComponent,
} from './generative-ui.js'
import {
  ACTION_CONFIRMATION_REQUIRED_EVENT,
  ACTION_DENIED_EVENT,
  ACTION_EXECUTION_FAILED_EVENT,
  ACTION_EXECUTION_SUCCEEDED_EVENT,
  buildEvent,
  KNOWLEDGE_GAP_EVENT,
  MEANINGFUL_ANSWER_EVENT,
  type AnalyticsEvent,
} from './analytics.js'
import { buildHandoffContext, type HandoffContext, type HandoffReason } from './handoff.js'
import { evaluateDecisionTurn, type DecisionObservation } from './decision.js'
import {
  buildIdempotencyKey,
  type ActionExecutionResult,
  type ActionExecutor,
  type ExecutionState,
  type GatedAction,
} from './execution.js'
import { inputRejectionMessage, validateActionInputs, type EntityResolver } from './validation.js'

/**
 * Retrieval, narrowed to the one call the pipeline makes.
 *
 * Promise-returning, full stop. A real retrieval port is a network call — a
 * vector store, a search cluster, a hosted index — and the honest shape for that
 * is a promise: it can be slow, and it can fail. Typing it `RetrievalOutcome |
 * Promise<RetrievalOutcome>` would let an implementation return either and every
 * caller guess which, which is exactly the drift this package exists to prevent.
 *
 * A synchronous in-memory implementation still satisfies the contract — it
 * declares `async` and returns its value, or wraps it in `Promise.resolve`. What
 * it may no longer do is *look* synchronous to the caller, because a caller that
 * can treat the result as an already-present object is one refactor away from
 * reading a promise as data.
 */
export interface KnowledgePort {
  readonly retrieve: (request: RetrievalContextRequest) => Promise<RetrievalOutcome>
}

/**
 * The live authoritative systems (PRD §17).
 *
 * Deliberately absent by default: an assistant that cannot reach a price or a
 * stock system must say so, not guess. `runTurn` records a knowledge gap rather
 * than answering from retrieval when this port is missing.
 *
 * Promise-returning for the same reason `KnowledgePort` is, and with rather more
 * at stake: the truth port answers questions about money and availability,
 * which in any real deployment is a call to an inventory or pricing service. A
 * port that may return synchronously is a port whose result can be consumed
 * without anyone noticing it had not arrived yet — which is how a cached price
 * gets quoted as though it were live.
 */
export interface StructuredTruthPort {
  readonly resolve: (
    subjects: readonly StructuredTruthSubject[],
  ) => Promise<Readonly<Record<string, unknown>>>
}

export interface TurnRequest {
  readonly tenantId: string
  readonly sessionId: string
  readonly utterance: string
  /** Page context as the SDK recorded it (PRD §16). */
  readonly graph: ContextGraph
  /** ISO timestamp supplied by the caller. A turn never reads the clock. */
  readonly occurredAt: string
  readonly brain: BrainProvider
  readonly policy: ActionPolicy
  readonly knowledge: KnowledgePort
  readonly truth?: StructuredTruthPort
  /** Trusted host routing may require live data; it can never downgrade lexical routing. */
  readonly authoritativeSubjects?: readonly StructuredTruthSubject[]
  /**
   * Who runs an action the gate allowed. Absent by default, and deliberately so:
   * without an executor an allowed action stays `not_attempted`, which is the
   * truth. An assistant that reported a booking because it *would have* called a
   * booking system would be describing a side effect nobody saw (§18).
   */
  readonly executor?: ActionExecutor
  /**
   * Who answers "does this entity exist" for §9 validation. Absent by default,
   * so an id-bearing action cannot be executed until a real catalog answers —
   * never until the pipeline invents an answer.
   */
  readonly resolver?: EntityResolver
  /** The session's capability tier, not the action's (PRD §18). */
  readonly capability: CapabilityTierName
  readonly role: RoleName
  /** Extra field names the tenant considers sensitive, on top of the action's own. */
  readonly clientSensitiveFields?: readonly string[]
  /** Actions the visitor already confirmed on-screen this turn. */
  readonly confirmedActionIds?: readonly string[]
  /**
   * Actions the visitor declined this session.
   *
   * The counterpart to `confirmedActionIds`, and the reason it is separate is
   * that a decline and a confirmation are not the same fact with a sign flipped:
   * a confirmation is per-turn, because the moment it was given for is gone, while
   * a decline describes the session, because the visitor's answer to "shall I
   * book this?" does not expire between turns.
   *
   * Without this the only place the gate can put a refusal is
   * `confirmation_required`, so a declined action is re-asked on every later turn
   * that happens to mention it. See `declined_by_visitor` in `@archava/acl`.
   */
  readonly declinedActionIds?: readonly string[]
  /** Actions a human operator approved for this session. */
  readonly humanApprovedActionIds?: readonly string[]
  /** True when the visitor asked for a person (PRD §26). */
  readonly handoffRequested?: boolean
  readonly retrievalLimit?: number
  /** Optional bounded judgment. Absent preserves Foundation V1.1 exactly. */
  readonly decisionOrchestrator?: DecisionOrchestrator
  /** Trusted live-system subject candidates for a one-way routing escalation. */
  readonly decisionTruthCandidates?: readonly StructuredTruthSubject[]
}

/**
 * One action request, after the gate has had its say.
 *
 * The shape comes from `execution.ts`, and the reason is §18: a gate decision and
 * a side effect are different facts. `policy` is what the gate permitted;
 * `execution` is what an executor confirmed. An action can be `allowed` and still
 * be `not_attempted` — which is exactly the case that used to be reported as
 * though the booking existed.
 */
export type { GatedAction } from './execution.js'

export type AnswerBasis = 'structured_truth' | 'retrieval' | 'none'

export interface TurnOutcome {
  readonly tenantId: string
  readonly sessionId: string
  readonly occurredAt: string
  readonly basis: AnswerBasis
  readonly text: string
  /** Live values, when the turn needed them and the port produced them. */
  readonly structuredTruth: Readonly<Record<string, unknown>>
  readonly citations: readonly { readonly sourceId: string; readonly sourceTitle: string }[]
  readonly components: readonly GenerativeComponent[]
  /** Why each rejected model selection was rejected; never silently dropped. */
  readonly rejectedComponents: readonly string[]
  readonly permittedActionIds: readonly string[]
  readonly actions: readonly GatedAction[]
  /** §16 masking notes for everything withheld from the provider. */
  readonly notices: readonly string[]
  /** True when the turn needed knowledge neither live data nor the corpus covered. */
  readonly knowledgeGap: boolean
  readonly handoff: HandoffContext | null
  readonly decision?: {
    readonly mode: 'off' | 'shadow' | 'assist'
    readonly intent: string | null
    readonly runtimeChanged: boolean
    readonly traces: readonly DecisionObservation[]
  }
  readonly events: readonly AnalyticsEvent[]
}

const NOTHING: Readonly<Record<string, unknown>> = {}

/**
 * Render an optional section as a plain object or `null`.
 *
 * `null` rather than omission: an absent cart is a fact worth stating ("there is
 * no cart"), and an absent key would force every consumer to distinguish
 * "missing" from "present but empty".
 */
function section<T>(
  value: T | undefined,
  present: (value: T) => Readonly<Record<string, unknown>>,
): Readonly<Record<string, unknown>> | null {
  return value === undefined ? null : present(value)
}

/**
 * Project the graph to what a model may see (PRD §16).
 *
 * This is the reason the masker is a second line of defence rather than the
 * only one: the projection emits ids, labels, counts, and flags — never a form
 * value, because the graph holds field *names* and not field values. A value
 * cannot be projected because it was never carried.
 */
export function projectContextGraph(graph: ContextGraph): Readonly<Record<string, unknown>> {
  const { page, session } = graph
  return {
    route: page.route,
    pageKind: page.kind,
    section: page.section ?? null,
    locale: page.locale,
    entities: graph.entities.map((entity) => ({
      id: entity.id,
      name: entity.name,
      entityKind: entity.kind,
    })),
    comparison: section(graph.comparison, (comparison) => ({
      entityIds: comparison.entityIds,
      metric: comparison.metric ?? null,
    })),
    form: section(graph.form, (form) => ({
      id: form.id,
      step: form.step ?? null,
      completedFields: form.completedFields,
      pendingFields: form.pendingFields,
      withheldFields: form.maskedFields,
    })),
    cart: section(graph.cart, (cart) => ({
      currency: cart.currency ?? null,
      lineCount: cart.lineCount,
      subtotal: cart.subtotal ?? null,
    })),
    checkout: section(graph.checkout, (checkout) => ({
      step: checkout.step,
      index: checkout.index ?? null,
      total: checkout.total ?? null,
    })),
    errors: graph.errors.map((error) => ({ code: error.code, occurredAt: error.occurredAt })),
    availableActions: graph.availableActions.map((action) => ({
      name: action.name,
      enabled: action.enabled,
    })),
    activePanel: graph.activePanel ?? null,
    session: {
      authenticated: session.authenticated,
      role: session.role ?? null,
      customerRef: session.customerRef ?? null,
    },
  }
}

/**
 * Apply §16 at the boundary: mask the projected context.
 *
 * The rules combine what the enabled actions declare as sensitive with whatever
 * the tenant adds. Given the projection above this is belt-and-braces — it is
 * what catches a host that later learns a field it has been echoing is
 * sensitive, without that host having to remember to stop echoing it.
 */
function maskAtBoundary(request: TurnRequest): {
  readonly value: Readonly<Record<string, unknown>>
  readonly notices: readonly string[]
} {
  const declared = enabledPageActions(request.graph).flatMap(
    (actionId) => definitionFor(actionId, request.policy)?.sensitiveFields ?? [],
  )
  const rules = resolveMaskRules(declared, request.clientSensitiveFields ?? [])
  const masked = maskSensitiveFields(projectContextGraph(request.graph), { rules })

  return { value: masked.value as Readonly<Record<string, unknown>>, notices: masked.notices }
}

/** The enabled page actions, as identities the policy gate understands. */
function enabledPageActions(graph: ContextGraph): readonly string[] {
  return graph.availableActions.filter((action) => action.enabled).map((action) => action.name)
}

/**
 * Ask the policy gate which actions this session may be shown (PRD §18).
 *
 * Two questions are asked here, and the order is the point. The gate answers the
 * first: what do the capability tier, the role, and the risk rules permit? The
 * page then answers the second: what does this page actually offer? Neither can
 * widen the other, which is what §16 means by page context being a narrowing
 * signal only. Doing the intersection in the caller rather than inside the gate
 * is deliberate — `ActionPolicy.availableActionIds` answers about a session and
 * has no per-action page answer to pass along, so a page offer that arrived there
 * as an action id would be indistinguishable from a client configuration.
 */
function permittedActions(request: TurnRequest): readonly string[] {
  const permitted = request.policy.availableActionIds({
    capability: request.capability,
    role: request.role,
    humanApproved: request.humanApprovedActionIds !== undefined,
  })
  const onPage = new Set(enabledPageActions(request.graph))
  return permitted.filter((actionId) => onPage.has(actionId))
}

/**
 * Read one action request through the gate.
 *
 * The definition is read to establish that the id is registered — an unregistered
 * id is a denial, not a crash, and the turn survives with the denial on the
 * record. What is *not* read from the definition is the capability: the client's
 * own tier is passed, because that is what `ActionPolicy.evaluate` weighs against
 * the action's requirement. Passing the requirement instead compares it with
 * itself, which can only ever pass, and a model that names an action then claims
 * whatever tier the naming earned it. `permittedActionIds` is the offer; this is
 * the gate, and it answers the same question a second time against the same
 * client.
 */
function gateAction(
  actionId: string,
  inputs: Readonly<Record<string, unknown>>,
  request: TurnRequest,
): GatedAction {
  if (definitionFor(actionId, request.policy) === null) {
    return {
      actionId,
      policy: 'denied',
      execution: 'not_attempted',
      reason: `No action "${actionId}" is registered.`,
      // The gate's answer to the same question, carried here so the record says
      // *why* it was denied rather than only that it was. The alternative — an
      // early return with no `denialReason` — is how a caller downstream has to
      // fall back on "was it denied at all", and that is the question which
      // mistakes an invented id for an entitlement problem.
      denialReason: 'unknown_action',
      inputs,
    }
  }

  const result = request.policy.evaluate({
    actionId,
    capability: request.capability,
    role: request.role,
    availableOnPage: pageAvailability(actionId, request.graph),
    confirmed: request.confirmedActionIds?.includes(actionId) === true,
    // Deliberately not reconciled here. A confirmation that arrives in the same
    // turn as the decline overrides it inside the gate, which is where the
    // ordering rule lives and where it can be tested; deriving `declined` from
    // `confirmed` at this layer would encode the same rule a second time and let
    // the two copies drift.
    declined: request.declinedActionIds?.includes(actionId) === true,
    humanApproved: request.humanApprovedActionIds?.includes(actionId) === true,
  })

  return toGatedAction(actionId, inputs, result)
}

/**
 * A registered action definition, or `null` when the id is not registered.
 *
 * An unregistered id is a denial and not a crash: `describe` throws for an
 * unknown action, and turning that into `null` lets the gate answer "no such
 * action" in the outcome rather than taking the turn down.
 */
function definitionFor(actionId: string, policy: ActionPolicy): ActionDefinition | null {
  try {
    return policy.describe(actionId)
  } catch {
    return null
  }
}

/**
 * What the page said about an action.
 *
 * Two answers, and the list that gives them is authoritative: an action the page
 * did not enable is not available, whether the page said so by disabling it or by
 * omitting it. That is §16 read correctly — a page can only narrow, so a page that
 * offers nothing narrows to nothing, and a browser assistant cannot reach an
 * action the page never offered. Nothing here is read *before* the policy gate
 * has weighed the capability: the ordering inside `ActionPolicy.evaluate` puts
 * the client's own limits first, so an action above the tier is denied as such
 * rather than as a page complaint. Confusing the two was a real defect: passing
 * this list as `enabledActionIds` made every page-silent action a
 * `action_disabled_for_client`, and the capability denial it should have been
 * never fired at all.
 */
function pageAvailability(actionId: string, graph: ContextGraph): boolean {
  const entry = graph.availableActions.find((action) => action.name === actionId)
  return entry !== undefined && entry.enabled
}

/**
 * Translate one gate decision into the outcome's action record.
 *
 * `allowed` here means *the gate permitted it* and nothing more. Whether the
 * action then ran is a separate fact, recorded by
 * {@link gateActions} once an executor has confirmed it — so a record with
 * `policy: 'allowed'` and `execution: 'not_attempted'` is the honest shape of an
 * action that was permitted and never attempted.
 */
function toGatedAction(
  actionId: string,
  inputs: Readonly<Record<string, unknown>>,
  result: PolicyResult,
): GatedAction {
  if (result.decision === 'allow') {
    return { actionId, policy: 'allowed', execution: 'not_attempted', inputs }
  }
  if (result.decision === 'confirmation_required') {
    return {
      actionId,
      policy: 'confirmation_required',
      execution: 'not_attempted',
      // The mode names *who* must confirm, which is the useful part to record.
      reason: `Confirmation required via ${result.mode}.`,
      prompt: result.prompt,
      inputs,
    }
  }
  return {
    actionId,
    policy: 'denied',
    execution: 'not_attempted',
    reason: result.message,
    // Which denial, separately from the sentence. A caller that needs to tell
    // "the visitor said no" from "the page never offered this" reads this; the
    // sentence below it is for a reader.
    denialReason: result.reason,
    inputs,
  }
}

/**
 * Run every requested action through the gate, then §9 validation, then the
 * executor — in that order, and never out of it.
 *
 * The ordering is the whole point of the three fields this returns. The gate
 * (§18) answers "may this run". Validation (§9) answers "is this a request an
 * executor could act on". The executor answers "did it happen". An action that
 * fails the second question is denied with the reason on the record; an action
 * that passes it but has no executor configured is `allowed` and `not_attempted`,
 * because a booking that was never requested of a booking system is not a booking.
 *
 * Validation runs *before* execution rather than inside it: an executor must
 * never be handed an input it has to re-check, because then there are two places
 * to trust and only one of them is visible here.
 */
async function gateActions(
  reply: BrainReply,
  request: TurnRequest,
): Promise<readonly GatedAction[]> {
  const gated: GatedAction[] = []
  for (const requested of reply.requestedActions) {
    const action = gateAction(requested.actionId, requested.inputs, request)
    if (action.policy !== 'allowed') {
      gated.push(action)
      continue
    }

    const validation = await validateActionInputs({
      actionId: requested.actionId,
      inputs: requested.inputs,
      tenantId: request.tenantId,
      ...(request.resolver === undefined ? {} : { resolver: request.resolver }),
    })
    if (!validation.ok) {
      // A refusal, not a crash: the gate permitted it, but nobody could act on
      // what was submitted. Recording it as denied keeps `allowed` meaning "an
      // execution was attempted" everywhere downstream.
      gated.push({
        ...action,
        policy: 'denied',
        reason: inputRejectionMessage(validation.rejections),
      })
      continue
    }

    gated.push(await executeAllowed(action, validation.inputs, request))
  }
  return gated
}

/**
 * Hand an allowed action to the executor, and record what came back.
 *
 * `not_attempted` is the no-executor answer, and it is the correct one. A
 * pipeline with a brain and no executor has not performed any side effects, and
 * saying so is the difference between a working assistant and one that reports
 * bookings it never made.
 */
async function executeAllowed(
  action: GatedAction,
  inputs: Readonly<Record<string, unknown>>,
  request: TurnRequest,
): Promise<GatedAction> {
  if (request.executor === undefined) {
    return {
      ...action,
      execution: 'not_attempted',
      reason: 'No action executor is configured, so the action was not attempted.',
    }
  }

  // Derived from the identifying record, not from the clock, so replaying this
  // turn produces the same key. Not a store: the pipeline remembers no key it
  // has issued, so "the same turn twice" collapses onto one attempt and
  // "the same ask an hour later" is a second, which is what a visitor asking
  // for something again actually did.
  const idempotencyKey = buildIdempotencyKey({
    tenantId: request.tenantId,
    sessionId: request.sessionId,
    occurredAt: request.occurredAt,
    actionId: action.actionId,
    inputs,
  })

  let result: ActionExecutionResult
  try {
    result = await request.executor.execute({
      tenantId: request.tenantId,
      sessionId: request.sessionId,
      action: action.actionId,
      inputs,
      idempotencyKey,
    })
  } catch (error) {
    // An executor that threw is a failed execution, not a denied action. The two
    // need different responses — one is a broken integration, the other is a
    // configuration problem — and §8's events exist precisely to keep them apart.
    return recordExecution(
      action,
      {
        status: 'failed',
        errorCode: 'executor_threw',
        retryable: true,
        message: errorText(error),
      },
      idempotencyKey,
    )
  }

  return recordExecution(action, result, idempotencyKey)
}

/** Fold an executor's answer into the action record, without losing the verdict. */
function recordExecution(
  action: GatedAction,
  result: ActionExecutionResult,
  idempotencyKey: string,
): GatedAction {
  const execution: ExecutionState = result.status === 'succeeded' ? 'succeeded' : 'failed'

  if (result.status === 'succeeded') {
    return { ...action, execution, idempotencyKey, output: result.output }
  }
  return {
    ...action,
    execution,
    idempotencyKey,
    errorCode: result.errorCode,
    retryable: result.retryable,
    // The executor's message is documented as safe to surface and free of tenant
    // data, which is what makes it the right thing to put on a visitor's record.
    reason: result.message,
  }
}

/** A thrown value as one sentence, without pretending it was an Error. */
function errorText(error: unknown): string {
  if (error instanceof Error) return error.message
  return 'The action executor failed.'
}

/** Validate the brain's component selections against the registry (PRD §25). */
function validatedComponents(
  reply: BrainReply,
  permittedActionIds: readonly string[],
): { readonly components: readonly GenerativeComponent[]; readonly rejected: readonly string[] } {
  const built = buildComponents(reply.components ?? [])
  return {
    components: filterComponentsByAllowedActions(built.components, permittedActionIds),
    rejected: built.rejected,
  }
}

/** The structured-truth answer the turn has, and whether it is authoritative. */
interface TruthResolution {
  readonly values: Readonly<Record<string, unknown>>
  readonly resolved: boolean
}

/** What retrieval returned, or why it could not. */
interface RetrievalResult {
  readonly outcome: RetrievalOutcome | null
  readonly failed: boolean
}

async function resolveTruth(
  classification: KnowledgeClassification,
  request: TurnRequest,
): Promise<TruthResolution> {
  if (classification.need !== 'structured_truth' || request.truth === undefined) {
    return { values: NOTHING, resolved: false }
  }
  let values: Readonly<Record<string, unknown>>
  try {
    values = await request.truth.resolve(classification.subjects)
  } catch {
    // A truth port that could not be reached has not answered, and an
    // unreachable source of live money and availability is the exact case where
    // falling back to retrieval would be most tempting and least defensible.
    return { values: NOTHING, resolved: false }
  }
  // A port that came back empty for the subjects it was asked about has not
  // answered. Reporting that record as authoritative truth would be §17's guess
  // wearing a uniform, so it counts as unresolved and the turn records a gap.
  return { values, resolved: Object.keys(values).length > 0 }
}

/**
 * What the turn's answer rests on (PRD §17).
 *
 * Retrieval is never the basis of a structured-truth answer. When the live
 * system did not answer — no port configured, or a port that returned nothing —
 * the basis is `none`: the caller is told nothing authoritative backed the
 * turn, which is the same thing `knowledgeGap` says.
 */
function basisOf(
  need: KnowledgeClassification['need'],
  truthResolved: boolean,
  retrieved: boolean,
): AnswerBasis {
  if (truthResolved) {
    return 'structured_truth'
  }
  if (need === 'structured_truth') {
    return 'none'
  }
  return retrieved ? 'retrieval' : 'none'
}

/**
 * Retrieve the tenant's own content, or record why it could not be.
 *
 * A retrieval failure is a knowledge gap the operator should see (§27
 * `knowledge_gap`), not a silent empty answer — the visitor is told nothing was
 * found rather than being handed a confident guess.
 */
async function retrieve(request: TurnRequest): Promise<RetrievalResult> {
  const query: RetrievalContextRequest = {
    question: request.utterance,
    tenantId: request.tenantId,
    ...(request.retrievalLimit === undefined ? {} : { limit: request.retrievalLimit }),
  }
  try {
    const outcome = await request.knowledge.retrieve(query)
    return { outcome, failed: outcome.context.length === 0 }
  } catch {
    return { outcome: null, failed: true }
  }
}

/**
 * §26's three-state summary of what an action actually did.
 *
 * The gate's three outcomes do not map one-to-one onto it, and the mismatch is
 * where a handoff either helps an operator or misleads them. An action waiting
 * on a human is folded into `denied`, because a human reading the summary needs
 * to know the thing did not happen; whether that was a permission or a pending
 * confirmation is on the action record itself.
 */
function handoffOutcome(action: GatedAction): 'allowed' | 'denied' | 'failed' {
  if (action.policy !== 'allowed') {
    return 'denied'
  }
  return action.execution === 'failed' ? 'failed' : 'allowed'
}

/**
 * Which mode the brain is being asked to answer from.
 *
 * This is a fold of two independent facts — what the utterance needed (§17) and
 * what the pipeline actually managed to supply — because the brain has to know
 * the difference between "there is no structured truth for this visitor" and
 * "the structured truth system is down". The first is an ordinary answer; the
 * second is a `knowledge_gap` (§27), and a brain that conflates them will answer
 * confidently from retrieval during an outage.
 */
function brainKnowledgeMode(
  classification: KnowledgeClassification,
  truth: TruthResolution,
  retrieval: RetrievalResult,
): BrainKnowledgeMode {
  if (classification.need === 'structured_truth') {
    // An utterance that needed structured truth, and got it, is answered from
    // it even if retrieval would also have had something to say (§17).
    return truth.resolved ? 'structured_truth' : 'none'
  }
  if (retrieval.failed || retrieval.outcome === null) {
    return 'none'
  }
  return 'retrieval'
}

/**
 * Compose the handoff payload, when the turn warrants one.
 *
 * The summary is mechanical on purpose: what was tried, what broke, where the
 * visitor is. It does not characterise their mood — see `handoff.ts`.
 */ function buildHandoff(
  request: TurnRequest,
  actions: readonly GatedAction[],
  decisionRecommended = false,
): HandoffContext | null {
  const reason = handoffReason(request, actions, decisionRecommended)
  if (reason === null) {
    return null
  }

  const graph = request.graph
  const focus = focusedEntity(graph)
  const attempted = actions.map((action) => ({
    actionId: action.actionId,
    outcome: handoffOutcome(action),
    ...(action.reason === undefined ? {} : { detail: action.reason }),
  }))

  return buildHandoffContext({
    tenantId: request.tenantId,
    sessionId: request.sessionId,
    reason,
    route: graph.page.route,
    locale: graph.page.locale,
    // Shared only when the session is authenticated and carries a reference.
    // An anonymous visitor has no identity to hand over, and inventing one is
    // worse than leaving the field absent (§26).
    ...(graph.session.authenticated && graph.session.customerRef !== undefined
      ? { customerRef: graph.session.customerRef }
      : {}),
    attemptedActions: attempted,
    errors: graph.errors.map((error) => ({
      code: error.code,
      message: `Recorded at ${error.occurredAt}.`,
    })),
    ...(focus === undefined ? {} : { entityName: focus.name }),
    summary:
      `Visitor at ${graph.page.route} (${graph.page.kind}) after ${attempted.length} attempted action(s); ` +
      `${graph.errors.length} recorded error(s)` +
      `${focus === undefined ? '' : `; viewing ${focus.name} (${focus.kind})`}.`,
  })
}

/**
 * The entity the visitor is currently looking at, as the reducer recorded it.
 *
 * `entities/select` pushes and `entities/deselect` clears the whole list, so the
 * list behaves as a selection the page has been narrowing: the last entry is what
 * the visitor last looked at. §26 asks the handover to carry the story so the
 * human does not make the visitor repeat it, and the entity in focus is the first
 * half of that story — a handoff that relocates the visitor to the home page
 * loses the one thing they were asking about.
 */
function focusedEntity(graph: ContextGraph): ContextGraph['entities'][number] | undefined {
  return graph.entities[graph.entities.length - 1]
}

/**
 * The denials that are about the client's own authorization envelope.
 *
 * `capability_exceeded` is a claim made to an operator, and an operator acts on
 * it — by raising a tier, enabling an action, or correcting a role. So the set of
 * denials allowed to produce it has to be exactly the set where those are the
 * right moves, and "was this action denied" cannot be that set: of the seven
 * codes in `DENIAL_REASONS`, only three are a statement about the envelope at all.
 *
 * - `capability_insufficient` — the action needs a tier this client does not hold.
 * - `role_not_allowed` — this client's role is not permitted to run it.
 * - `action_disabled_for_client` — this client's configuration turned it off.
 *
 * Each of those is the same shape of sentence: *the entitlements said no, and the
 * fix is to change the entitlements.* What they are not is any of the following.
 *
 * `declined_by_visitor` is a person's decision. Escalating it tells an operator to
 * fix an entitlement problem when a visitor said no, and the only move it could
 * prompt is asking the visitor again.
 *
 * `action_unavailable_on_page` is the page's own scope. The browser was told it
 * may highlight and not book; no tier change alters what a page offered.
 *
 * `unknown_action` is a defect in the caller — an action id the registry does not
 * have — and there is no envelope to widen that would make a nonexistent action
 * exist.
 *
 * `admin_action_requires_human` is about the *action's* level, not the client's
 * envelope. A client at `enterprise` is fully entitled and an `L5` action still
 * does not run autonomously; calling that `capability_exceeded` would send someone
 * to raise a tier that was never too low. It stays where it already is — on the
 * action, beside a sentence that tells the visitor a human must approve.
 *
 * A §9 validation refusal arrives by a third route, which is why it needs no name
 * here: it does not come from the policy gate at all, so it carries no
 * `denialReason` and matches nothing in this set. That is deliberate. The
 * alternative — recognising it by parsing the human-readable `reason` — is how a
 * refusal gets reclassified by someone reworded a sentence months later.
 */
const CAPABILITY_DENIALS: ReadonlySet<DenialReason> = new Set<DenialReason>([
  'capability_insufficient',
  'role_not_allowed',
  'action_disabled_for_client',
])

/**
 * Why this turn escalates, or `null` when it does not.
 *
 * Repeated failure counts failures only, and it counts them in two scopes for
 * the same reason: a visitor whose booking failed a third time just now, and a
 * visitor who arrives with three failed attempts already on the session, both
 * need a human.
 *
 * What it does not count is everything that used to land here. Three successful
 * actions in one turn is a visitor getting things done, and escalating that to a
 * human is both wrong and corrosive — it teaches the operator that handoff means
 * noise. An action held at `confirmation_required` is a visitor being asked, not
 * an attempt that went wrong, so it is not a failure either. An action the gate
 * never ran (`not_attempted`) is the absence of an attempt, which the event layer
 * already says by recording nothing at all. The only thing left that counts is an
 * execution that actually returned `failed`.
 */
function handoffReason(
  request: TurnRequest,
  actions: readonly GatedAction[],
  decisionRecommended = false,
): HandoffReason | null {
  if (request.handoffRequested === true) {
    return 'visitor_requested'
  }
  // The gate's own classification, read as data. A human-readable `reason` is for a
  // reader and can be reworded without changing what happened; `denialReason` is
  // what the gate committed to, and it is absent exactly where the gate did not
  // deny — so a §9 refusal, which is a different component's judgement, cannot
  // match by accident.
  if (
    actions.some(
      (action) => action.denialReason !== undefined && CAPABILITY_DENIALS.has(action.denialReason),
    )
  ) {
    return 'capability_exceeded'
  }
  if (
    failedExecutions(actions) >= REPEATED_FAILURE_THRESHOLD ||
    request.graph.errors.length >= REPEATED_FAILURE_THRESHOLD
  ) {
    return 'repeated_failure'
  }
  if (decisionRecommended) return 'decision_recommended'
  return null
}

/**
 * How many actions in this turn were attempted and did not work.
 *
 * Deliberately narrow: an action that was never attempted, and an action waiting
 * on a human to confirm it, are both counted as zero. Only an executor that came
 * back `failed` adds to the total, which is what makes the threshold mean "this
 * visitor has hit the same wall three times" rather than "this visitor pressed
 * three buttons".
 */
function failedExecutions(actions: readonly GatedAction[]): number {
  return actions.filter((action) => action.execution === 'failed').length
}

const REPEATED_FAILURE_THRESHOLD = 3

/** Per-turn §27 events. Session-scoped events belong to whoever owns the session. */
function turnEvents(
  request: TurnRequest,
  outcome: Omit<TurnOutcome, 'events'>,
): readonly AnalyticsEvent[] {
  const envelope = {
    tenantId: request.tenantId,
    sessionId: request.sessionId,
    occurredAt: request.occurredAt,
  }
  const events: AnalyticsEvent[] = []

  if (outcome.text.length > 0 || outcome.components.length > 0) {
    events.push(
      buildEvent({
        ...envelope,
        name: MEANINGFUL_ANSWER_EVENT,
        outcome: outcome.basis,
      }),
    )
  }
  if (outcome.knowledgeGap) {
    events.push(buildEvent({ ...envelope, name: KNOWLEDGE_GAP_EVENT }))
  }

  for (const trace of outcome.decision?.traces ?? []) {
    if (trace.outcome === 'not_called') continue
    const detail = `mode=${trace.mode}; provider=${trace.providerId ?? 'none'}; confidence=${trace.confidence ?? 'none'}; changed=${trace.runtimeChanged}; disagreement=${trace.agreement ?? 'none'}; latencyMs=${trace.latencyMs ?? 'none'}`
    events.push(
      buildEvent({
        ...envelope,
        name: 'decision_evaluated',
        subjectId: trace.task,
        outcome: trace.outcome,
        note: detail,
      }),
    )
    if (
      trace.outcome === 'failed' ||
      trace.outcome === 'timeout' ||
      trace.outcome === 'malformed'
    ) {
      events.push(
        buildEvent({
          ...envelope,
          name: 'decision_provider_failure',
          subjectId: trace.task,
          outcome: trace.outcome,
        }),
      )
    }
    if (trace.reason.includes('below the floor')) {
      events.push(
        buildEvent({ ...envelope, name: 'decision_low_confidence', subjectId: trace.task }),
      )
    }
    if (!trace.applied && trace.mode === 'assist') {
      events.push(
        buildEvent({
          ...envelope,
          name: 'decision_fallback_used',
          subjectId: trace.task,
          outcome: trace.outcome,
        }),
      )
    }
  }

  // One event per action, chosen by which of the three layers the action stopped
  // at. Collapsing these into a single failure event is what made an operator
  // reading a dashboard unable to tell a misconfigured capability from a broken
  // integration, so they are separate events now.
  for (const action of outcome.actions) {
    const subjectId = action.actionId
    if (action.policy === 'denied') {
      events.push(
        buildEvent({
          ...envelope,
          name: ACTION_DENIED_EVENT,
          subjectId,
          outcome: 'denied',
          note: action.reason,
        }),
      )
      continue
    }
    if (action.policy === 'confirmation_required') {
      events.push(
        buildEvent({
          ...envelope,
          name: ACTION_CONFIRMATION_REQUIRED_EVENT,
          subjectId,
          outcome: 'confirmation_required',
        }),
      )
      continue
    }
    if (action.execution === 'succeeded') {
      events.push(
        buildEvent({
          ...envelope,
          name: ACTION_EXECUTION_SUCCEEDED_EVENT,
          subjectId,
          outcome: 'allowed',
        }),
      )
      continue
    }
    if (action.execution === 'failed') {
      events.push(
        buildEvent({
          ...envelope,
          name: ACTION_EXECUTION_FAILED_EVENT,
          subjectId,
          outcome: 'failed',
          note: action.errorCode,
        }),
      )
    }
  }

  return events
}

/**
 * Run one turn.
 *
 * Throws only on a §23 tenant-scope violation, which is a bug and not a
 * condition to recover from. Everything else — an unreachable knowledge system,
 * a brain that invents a component kind, an action the page no longer offers —
 * is reported in the outcome so the visitor still gets an answer.
 *
 * It is `async` because the honest boundary it crosses is asynchronous: §9 asks
 * an authoritative resolver whether the ids in a request exist, and an executor
 * reports side effects that have not happened yet. Awaiting those here is what
 * keeps a caller from having to guess whether the action it was handed is a
 * promise of a booking or a booking.
 */
export async function runTurn(request: TurnRequest): Promise<TurnOutcome> {
  const graph = assertTenant(request.graph, request.tenantId)
  const lexicalClassification = classifyKnowledgeNeed(request.utterance)
  const hostSubjects = request.authoritativeSubjects ?? []
  if (hostSubjects.some((subject) => !STRUCTURED_TRUTH_SUBJECTS.includes(subject)))
    throw new Error('Unknown authoritative subject')
  const deterministicClassification =
    hostSubjects.length === 0
      ? lexicalClassification
      : {
          need: 'structured_truth' as const,
          subjects: [...new Set([...lexicalClassification.subjects, ...hostSubjects])],
          matched: [...lexicalClassification.matched, 'host_authoritative_route'],
        }
  const retrieval = await retrieve(request)

  const permitted = permittedActions(request)
  const masked = maskAtBoundary(request)
  const grounded = retrieval.outcome?.context ?? []
  const assessment = await evaluateDecisionTurn({
    tenantId: request.tenantId,
    utterance: request.utterance,
    locale: graph.page.locale,
    context: masked.value,
    classification: deterministicClassification,
    grounding: grounded,
    handoffRequested: request.handoffRequested === true,
    ...(request.decisionOrchestrator === undefined
      ? {}
      : { orchestrator: request.decisionOrchestrator }),
    ...(request.decisionTruthCandidates === undefined
      ? {}
      : { structuredTruthCandidates: request.decisionTruthCandidates }),
  })
  const classification = assessment.classification
  const truth = await resolveTruth(classification, request)
  // The brain step is documented as a function of (turn, grounding, permitted
  // actions). This is where `grounding` is chosen — not at line 925, which is
  // now a pass-through.
  //
  // A turn that needed live values is never handed retrieval to fall back on.
  // `brainKnowledgeMode` below already reports such a turn as `none` when the
  // live system could not answer, but a *label* of `none` does not remove the
  // competing chunk: the brain is still handed `grounded`, and a provider that
  // prefers it will quote a stale price under a "no source" badge. §17's rule is
  // that a live-value question is answered from the live system or not at all,
  // so the source is withheld rather than merely discouraged. The
  // `ScriptedBrain` fallback branch (adapters/src/brain.ts) is what makes this
  // concrete: it returns `turn.grounding[0].text` verbatim whenever that array
  // is non-empty, so until this line chose, an unresolved price turn was
  // answered from a stale chunk.
  //
  // `basisOf` still receives `grounded.length > 0`, not this value's: the fact
  // "retrieval ran and found content" is true regardless of what the brain was
  // shown, and for a structured-truth turn `basisOf` never reads it anyway.
  const retrievalForBrain = classification.need === 'structured_truth' ? [] : grounded

  const reply: BrainReply = assessment.clarify
    ? {
        text: graph.page.locale.startsWith('id')
          ? 'Yang mana yang Anda maksud? Mohon sebutkan nama pilihannya.'
          : 'Which one do you mean? Please name the option.',
        requestedActions: [],
        citations: [],
        deferToStructuredTruth: false,
      }
    : assessment.evidenceInsufficient
      ? {
          text: graph.page.locale.startsWith('id')
            ? 'Saya belum punya bukti yang cukup untuk memastikan klaim itu.'
            : 'I do not have enough evidence to verify that claim.',
          requestedActions: [],
          citations: [],
          deferToStructuredTruth: false,
        }
      : await request.brain.reply({
          tenantId: request.tenantId,
          utterance: request.utterance,
          locale: graph.page.locale,
          context: masked.value,
          grounding: retrievalForBrain,
          permittedActionIds: permitted,
          // §5: the brain sees the structured truth it is being asked to answer from,
          // and which mode that is. §17 requires that structured truth outranks
          // retrieval, so a brain that cannot see which one it was handed cannot
          // prefer it.
          structuredTruth: truth.values,
          knowledgeMode: brainKnowledgeMode(classification, truth, retrieval),
        })

  const actions = await gateActions(reply, request)
  const { components, rejected } = validatedComponents(reply, permitted)

  // A gap means the visitor was reached without an answer, which is not the
  // same thing as "retrieval came up empty". §17 makes structured truth
  // outrank retrieval, so a price question the port answered has its answer
  // whether or not a stale chunk happened to match too. Claiming a gap in the
  // same record that carries `basis: 'structured_truth'` and the answer would
  // tell the operator the opposite of what happened, so truth that resolved
  // suppresses the gap outright. The two cases left are both real: the port was
  // asked for something it does not own, and the knowledge system had nothing.
  const knowledgeGap =
    assessment.evidenceInsufficient ||
    (truth.resolved ? false : retrieval.failed || classification.need === 'structured_truth')

  const decision =
    assessment.run === null
      ? undefined
      : {
          mode: assessment.run.mode,
          intent: assessment.intent,
          runtimeChanged: assessment.runtimeChanged,
          traces: assessment.run.traces.map(
            (trace): DecisionObservation => ({
              ...trace,
              runtimeChanged:
                trace.task === 'knowledge_routing'
                  ? classification.need !== deterministicClassification.need
                  : trace.task === 'clarification'
                    ? assessment.clarify
                    : trace.task === 'handoff_recommendation'
                      ? assessment.recommendHandoff
                      : trace.task === 'evidence_sufficiency'
                        ? assessment.evidenceInsufficient
                        : false,
            }),
          ),
        }

  const outcome: Omit<TurnOutcome, 'events'> = {
    tenantId: request.tenantId,
    sessionId: request.sessionId,
    occurredAt: request.occurredAt,
    basis:
      assessment.clarify || assessment.evidenceInsufficient
        ? 'none'
        : basisOf(classification.need, truth.resolved, grounded.length > 0),
    text: reply.text,
    structuredTruth: truth.values,
    citations: reply.citations,
    components,
    rejectedComponents: rejected,
    permittedActionIds: permitted,
    actions,
    notices: masked.notices,
    knowledgeGap,
    handoff: buildHandoff(request, actions, assessment.recommendHandoff),
    ...(decision === undefined ? {} : { decision }),
  }

  return { ...outcome, events: turnEvents(request, outcome) }
}
