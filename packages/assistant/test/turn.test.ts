import { describe, expect, it } from 'vitest'
import { ActionPolicy } from '@archava/acl'
import type {
  BrainProvider,
  BrainReply,
  BrainTurn,
  DecisionProvider,
  Decision,
} from '@archava/adapters'
import { DecisionOrchestrator, type DecisionTask } from '@archava/decision'
import { parseClientConfig, type ClientConfig } from '@archava/config'
import {
  assertTenant,
  foldContextEvents,
  seedContextGraph,
  TenantScopeError,
  type ContextEvent,
  type ContextGraph,
} from '@archava/core'
import type {
  KnowledgeContextChunk,
  RetrievalOutcome,
  StructuredTruthSubject,
} from '@archava/knowledge'
import {
  actionExecuted,
  buildIdempotencyKey,
  runTurn,
  type KnowledgePort,
  type StructuredTruthPort,
  type TurnRequest,
} from '../src/index.js'
import type {
  ActionExecutionRequest,
  ActionExecutionResult,
  ActionExecutor,
} from '../src/execution.js'
import type { EntityResolver } from '../src/validation.js'

/**
 * The turn pipeline (PRD §16–§21, §23, §25–§27) as a black box.
 *
 * Everything here goes through `runTurn`, because the guarantees are properties
 * of the pipeline and not of one module: the graph is masked before the brain
 * ever sees it, an action a brain asks for is a request the gate re-decides, and
 * a component the brain invents never renders. Each test names its clause.
 */

const TENANT = 'acme-hotels'
const OCCURRED_AT = '2026-04-01T09:00:00.000Z'

function configFor(tenantId = TENANT): ClientConfig {
  return parseClientConfig({
    schema_version: '1.0.0',
    tenantId,
    environment: 'commerce_booking',
    presence: 'chat',
    capability: 'transact',
    region: 'ID',
    template: 'hospitality',
    branding: {
      businessName: 'Acme Hotels',
      theme: {
        accent: '#123456',
        surface: '#ffffff',
        ink: '#101010',
        radius: 'rounded',
        fontFamily: 'Inter',
      },
    },
    languages: [{ code: 'id', label: 'Bahasa Indonesia' }],
    primaryLanguage: 'id',
    updatedAt: '2026-04-01',
  })
}

/** The graph the SDK hands over, built by the real reducer rather than cast. */
function graph(events: readonly ContextEvent[] = [], route = '/rooms'): ContextGraph {
  return foldContextEvents(seedContextGraph(configFor(), route), events)
}

const ROOMS: ContextGraph['entities'] = [
  { id: 'room-12', name: 'Room 12', kind: 'product' },
  { id: 'room-14', name: 'Room 14', kind: 'product' },
]

const BOOK_FORM: NonNullable<ContextGraph['form']> = {
  id: 'booking_request',
  step: 'guest_details',
  completedFields: ['check_in'],
  pendingFields: ['guest_phone'],
  maskedFields: ['guest_phone'],
}

const enabled = (...names: string[]): ContextGraph['availableActions'] =>
  names.map((name) => ({ name, enabled: true }))

function chunk(sourceId: string, text = 'Acme Hotels sits two minutes from Senggigi beach.') {
  const prepared: KnowledgeContextChunk = {
    sourceId,
    sourceTitle: `Doc ${sourceId}`,
    sourceKind: 'documentation',
    text,
  }
  return prepared
}

const EMPTY_RETRIEVAL: KnowledgePort = {
  retrieve: (): Promise<RetrievalOutcome> =>
    Promise.resolve({
      plan: { need: 'retrieval_knowledge', subjects: [], query: '' },
      context: [],
      deferToStructuredTruth: false,
    }),
}

function retrieve(...chunks: readonly { sourceId: string; text?: string }[]): KnowledgePort {
  return {
    retrieve: (): Promise<RetrievalOutcome> =>
      Promise.resolve({
        plan: { need: 'retrieval_knowledge', subjects: [], query: '' },
        context: chunks.map((entry) => chunk(entry.sourceId, entry.text)),
        deferToStructuredTruth: false,
      }),
  }
}

/** A truth port that answers immediately, as a fixture store would. */
function truth(values: Readonly<Record<string, unknown>>): StructuredTruthPort {
  return {
    resolve: (): Promise<Readonly<Record<string, unknown>>> => Promise.resolve(values),
  }
}

/**
 * A truth port that answers on a later microtask.
 *
 * Not decorative. These regressions only hold if the port genuinely hands back
 * control before it answers: a port typed `T | Promise<T>` lets a caller read a
 * plain record without awaiting, and the only test that can tell the difference
 * is one whose answer is not already there when the call returns.
 */
function deferredTruth(
  values: Readonly<Record<string, unknown>>,
): StructuredTruthPort & { readonly settled: number } {
  const port = {
    settled: 0,
    async resolve(): Promise<Readonly<Record<string, unknown>>> {
      await Promise.resolve()
      port.settled += 1
      return values
    },
  }
  return port
}

const EMPTY_TRUTH: StructuredTruthPort = {
  resolve: (): Promise<Readonly<Record<string, unknown>>> => Promise.resolve({}),
}

/** The inputs §9 accepts, keyed by action id. */
const VALID_INPUTS: Readonly<Record<string, Readonly<Record<string, unknown>>>> = {
  'product.read': { productId: 'room-12' },
  'availability.read': { subjectId: 'slot_1' },
  'order.status.read': { orderReference: 'ORD-1' },
  'cart.item.add': { productId: 'room-12', quantity: 2 },
  'cart.item.remove': { productId: 'room-12' },
  'booking.create': { slotId: 'slot_1', customer: { customerRef: 'cust_9' } },
  'booking.reschedule': { bookingId: 'book_1', slotId: 'slot_2' },
  'email.send': { templateId: 'tpl_1', to: 'guest@example.com' },
  'form.submit': { formId: 'booking_request', fields: { check_in: '2026-05-01' } },
  'admin.config.update': { changes: { presence: 'chat' } },
}

/**
 * A brain that records the turn it was handed.
 *
 * `ScriptedBrain` always returns no actions and no components, which cannot
 * exercise the gate or the registry. This one keeps the turn so a test can
 * assert on the projected context and the permitted set it received, and never
 * invents a component: what the pipeline does with an invented one is tested
 * with its own brain below.
 */
function probe(reply: Partial<BrainReply> = {}): BrainProvider & {
  readonly turns: readonly BrainTurn[]
} {
  const turns: BrainTurn[] = []
  return {
    providerId: 'probe-brain',
    model: 'reference-model',
    health: { ready: true, reason: null },
    reply(turn) {
      turns.push(turn)
      return Promise.resolve({
        text: 'Here are the two rooms closest to the beach.',
        requestedActions: [],
        citations: [],
        deferToStructuredTruth: false,
        ...reply,
      })
    },
    get turns() {
      return turns
    },
  }
}

/**
 * A brain that asks for actions with inputs that pass §9's contracts.
 *
 * Override the inputs only when a test is about a contract refusal; a test that
 * is about the gate should not be refused for an unrelated reason.
 */
function brainFor(...actions: readonly string[]): BrainProvider {
  return {
    providerId: 'probe-brain',
    model: 'reference-model',
    health: { ready: true, reason: null },
    reply() {
      return Promise.resolve({
        text: 'On it.',
        requestedActions: actions.map((actionId) => ({
          actionId,
          inputs: VALID_INPUTS[actionId] ?? {},
        })),
        citations: [],
        deferToStructuredTruth: false,
      })
    },
  }
}

/** A brain that selects generative components. Unvalidated by construction. */
function brainWithComponents(components: readonly unknown[]): BrainProvider {
  return {
    providerId: 'probe-brain',
    model: 'reference-model',
    health: { ready: true, reason: null },
    reply() {
      return Promise.resolve({
        text: 'Two rooms match.',
        requestedActions: [],
        citations: [],
        deferToStructuredTruth: false,
        components,
      })
    },
  }
}

/**
 * An executor that reports success for everything, recording what it got.
 *
 * Used by the tests that assert an execution actually happened: without one the
 * pipeline is honest about `not_attempted`, which is the right answer but cannot
 * demonstrate §18's executor path.
 *
 * It keeps two records, because the two answer different questions. `calls` is
 * what a test asserting "the executor ran this" wants: the action and its
 * inputs, and nothing that could drift if the request shape grew. `requests` is
 * what the de-duplication tests want: the whole request, including the
 * idempotency key the pipeline derived and the tenant and session it derived it
 * from — facts that live nowhere else, because the pipeline keeps no store of
 * the keys it has handed out.
 */
function executor(
  handler?: (request: { action: string; inputs: Readonly<Record<string, unknown>> }) => void,
): ActionExecutor & {
  readonly calls: readonly { action: string; inputs: Readonly<Record<string, unknown>> }[]
  readonly requests: readonly ActionExecutionRequest[]
} {
  const calls: { action: string; inputs: Readonly<Record<string, unknown>> }[] = []
  const requests: ActionExecutionRequest[] = []
  return {
    executorId: 'probe-executor',
    execute(request) {
      const entry = { action: request.action, inputs: request.inputs }
      calls.push(entry)
      requests.push(request)
      handler?.(entry)
      return Promise.resolve({ status: 'succeeded', output: { ok: true } })
    },
    get calls() {
      return calls
    },
    get requests() {
      return requests
    },
  }
}

/**
 * The key the executor was handed for one action, read back off its record.
 *
 * Read off the request rather than off the action, because the request is the
 * thing an executor would actually de-duplicate on: a key recorded on the action
 * and a key sent over the port could disagree, and only one of them is the
 * promise the pipeline is making.
 */
function keyOf(requests: readonly ActionExecutionRequest[], actionId: string): string | undefined {
  return requests.find((request) => request.action === actionId)?.idempotencyKey
}

/** A resolver that answers "exists" for a fixed set of kinds and ids. */
function resolverFor(existing: Readonly<Record<string, readonly string[]>>): EntityResolver {
  return {
    resolveExists: (_tenantId, entityKind, entityId): Promise<boolean> =>
      Promise.resolve((existing[entityKind] ?? []).includes(String(entityId))),
  }
}
/**
 * A resolver that confirms whatever it is asked about.
 *
 * Only for tests that reach the gate, the executor or the event boundary with an
 * entity-bearing action and are *not* about §9 — supplying this is what lets them
 * stay on their subject now that an action naming an entity without a resolver is
 * refused. It is deliberately not the default in `request()`: a resolver nobody
 * had to name was exactly how the validation gate came to be fail-open, and a
 * fixture that answers yes to everything should stay as explicit in the file as it
 * is untrustworthy as a catalog.
 */
const CONFIRMING: EntityResolver = {
  resolveExists: (): Promise<boolean> => Promise.resolve(true),
}

/**
 * A turn request with the defaults every test shares.
 *
 * The graph and brain default too: most tests care about one clause and should
 * not have to spell out the other nine fields to reach it.
 */
function request(overrides: Partial<TurnRequest> = {}): TurnRequest {
  return {
    tenantId: TENANT,
    sessionId: 'sess_1',
    utterance: 'Which rooms are near the beach?',
    occurredAt: OCCURRED_AT,
    policy: new ActionPolicy(),
    capability: 'transact',
    role: 'archava_assistant',
    knowledge: retrieve({ sourceId: 'k1' }),
    graph: graph(),
    brain: probe(),
    ...overrides,
  }
}

describe('trusted host authoritative routing', () => {
  it('routes non-keyword business queries to live truth and withholds stale retrieval', async () => {
    const brain = probe()
    const outcome = await runTurn(
      request({
        utterance: 'tampilkan kontak',
        authoritativeSubjects: ['customer_or_order'],
        brain,
        truth: truth({ crm: { count: 1 } }),
      }),
    )
    expect(outcome.basis).toBe('structured_truth')
    expect(brain.turns[0]?.grounding).toEqual([])
    expect(brain.turns[0]?.structuredTruth).toEqual({ crm: { count: 1 } })
    expect(outcome.knowledgeGap).toBe(false)
  })
  it('does not fall back to retrieval when host-routed live data is unavailable', async () => {
    const brain = probe()
    const outcome = await runTurn(
      request({
        utterance: 'tampilkan kontak',
        authoritativeSubjects: ['customer_or_order'],
        brain,
        truth: EMPTY_TRUTH,
      }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
    expect(brain.turns[0]?.grounding).toEqual([])
  })
  it('retains lexical live subjects when a host supplies an empty or additional route', async () => {
    for (const authoritativeSubjects of [[], ['customer_or_order']] as const) {
      const subjects: StructuredTruthSubject[][] = []
      const outcome = await runTurn(
        request({
          utterance: 'payment status',
          authoritativeSubjects,
          truth: {
            resolve: (requested) => {
              subjects.push([...requested])
              return Promise.resolve({ verified: true })
            },
          },
        }),
      )
      expect(subjects[0]).toContain('payment_status')
      expect(outcome.basis).toBe('structured_truth')
    }
  })
})

/** The projected context the brain was actually handed. */
function seen(
  brain: BrainProvider & { readonly turns: readonly BrainTurn[] },
): Readonly<Record<string, unknown>> {
  const turn = brain.turns[0]
  if (turn === undefined) throw new Error('the brain was never invoked')
  return turn.context
}

/** The permitted action ids the brain was offered, in the order it got them. */
function permitted(
  brain: BrainProvider & { readonly turns: readonly BrainTurn[] },
): readonly string[] {
  const turn = brain.turns[0]
  if (turn === undefined) throw new Error('the brain was never invoked')
  return turn.permittedActionIds
}

/** The grounding the brain was offered, and nothing else. */
function grounding(
  brain: BrainProvider & { readonly turns: readonly BrainTurn[] },
): readonly unknown[] {
  const turn = brain.turns[0]
  if (turn === undefined) throw new Error('the brain was never invoked')
  return turn.grounding
}

describe('tenant scope (§23)', () => {
  it('throws on a cross-tenant graph rather than answering from the wrong page', async () => {
    // §23: cross-tenant access is a bug, not a recoverable error. It throws — it
    // does not return empty, partial, or the other tenant's rooms.
    const foreign = seedContextGraph(configFor('other-hotels'), '/rooms')
    const brain = probe()
    await expect(runTurn(request({ graph: foreign, brain }))).rejects.toThrow(TenantScopeError)
    expect(brain.turns).toHaveLength(0)
  })

  it('accepts the tenant it was given', () => {
    expect(assertTenant(graph(), TENANT).tenantId).toBe(TENANT)
  })

  it('scopes retrieval to the requested tenant', async () => {
    const asked: string[] = []
    const brain = probe()
    await runTurn(
      request({
        graph: graph(),
        brain,
        knowledge: {
          retrieve: (query): Promise<RetrievalOutcome> => {
            asked.push(query.tenantId)
            return Promise.resolve({
              plan: { need: 'retrieval_knowledge', subjects: [], query: query.question },
              context: [chunk('k1')],
              deferToStructuredTruth: false,
            })
          },
        },
      }),
    )
    expect(asked).toEqual([TENANT])
    // The tenant's own chunk is what the brain was grounded on — never another
    // tenant's, and never a merged pile of both.
    expect(grounding(brain)).toEqual([chunk('k1')])
  })
})

function decisionFor(
  task: DecisionTask,
  answer: string | boolean | number,
  mode: 'shadow' | 'assist' = 'assist',
  confidence = 1,
): DecisionOrchestrator {
  const provider: DecisionProvider = {
    providerId: 'test-decision',
    model: 'bounded-test',
    health: { ready: true },
    decide: (query) =>
      Promise.resolve({
        providerId: 'test-decision',
        model: 'bounded-test',
        decisions: query.questions.map(
          (question): Decision =>
            ({
              id: question.id,
              kind: question.kind,
              answer,
              confidence,
            }) as Decision,
        ),
        refused: [],
      }),
  }
  return new DecisionOrchestrator({ mode, provider, enabledTasks: [task] })
}

describe('Decision Intelligence turn boundary', () => {
  it('shadow records a disagreement and preserves visible Foundation behavior', async () => {
    const base = request({
      utterance: 'Is my booking confirmed?',
      truth: truth({ confirmed: true }),
    })
    const foundation = await runTurn(base)
    const shadow = await runTurn({
      ...base,
      decisionOrchestrator: decisionFor('knowledge_routing', 'retrieval_knowledge', 'shadow'),
    })
    expect({
      text: shadow.text,
      basis: shadow.basis,
      actions: shadow.actions,
      handoff: shadow.handoff,
    }).toEqual({
      text: foundation.text,
      basis: foundation.basis,
      actions: foundation.actions,
      handoff: foundation.handoff,
    })
    expect(shadow.decision?.traces[0]).toMatchObject({
      agreement: 'downgraded',
      applied: false,
      runtimeChanged: false,
    })
  })

  it('cannot downgrade structured truth, even at confidence 1', async () => {
    const brain = probe()
    const outcome = await runTurn(
      request({
        utterance: 'Is my booking confirmed?',
        brain,
        truth: truth({ confirmed: true }),
        decisionOrchestrator: decisionFor('knowledge_routing', 'retrieval_knowledge'),
      }),
    )
    expect(outcome.basis).toBe('structured_truth')
    expect(grounding(brain)).toEqual([])
    expect(outcome.decision?.traces[0]?.applied).toBe(false)
  })

  it('escalates only to a trusted subject and never treats retrieval as transactional truth', async () => {
    const brain = probe()
    const outcome = await runTurn(
      request({
        utterance: 'Can I take that one?',
        brain,
        knowledge: retrieve({ sourceId: 'stale', text: 'Old price: 10 dollars' }),
        decisionOrchestrator: decisionFor('knowledge_routing', 'structured_truth'),
        decisionTruthCandidates: ['availability'],
      }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
    expect(grounding(brain)).toEqual([])
    const noSubject = await runTurn(
      request({
        utterance: 'Can I take that one?',
        decisionOrchestrator: decisionFor('knowledge_routing', 'structured_truth'),
      }),
    )
    expect(noSubject.basis).toBe('retrieval')
    expect(noSubject.decision?.runtimeChanged).toBe(false)
  })

  it('can ask clarification and veto an unsupported retrieval claim', async () => {
    const clarify = await runTurn(
      request({
        utterance: 'Book that one',
        decisionOrchestrator: decisionFor('clarification', true),
      }),
    )
    expect(clarify.text).toContain('Yang mana')
    expect(clarify.actions).toEqual([])
    const evidence = await runTurn(
      request({
        utterance: 'Is breakfast halal certified?',
        knowledge: retrieve({ sourceId: 'breakfast', text: 'Breakfast is served daily.' }),
        decisionOrchestrator: decisionFor('evidence_sufficiency', 0),
      }),
    )
    expect(evidence.basis).toBe('none')
    expect(evidence.knowledgeGap).toBe(true)
    expect(evidence.text).toContain('belum punya bukti yang cukup')
  })

  it('cannot suppress mandatory handoff or bypass ActionPolicy', async () => {
    const handoff = await runTurn(
      request({
        handoffRequested: true,
        decisionOrchestrator: decisionFor('handoff_recommendation', false),
      }),
    )
    expect(handoff.handoff?.reason).toBe('visitor_requested')
    const action = await runTurn(
      request({
        capability: 'assist',
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainFor('booking.create'),
        decisionOrchestrator: decisionFor('intent_classification', 'purchase_request'),
        resolver: CONFIRMING,
        executor: executor(),
      }),
    )
    expect(action.actions[0]).toMatchObject({
      policy: 'denied',
      execution: 'not_attempted',
      denialReason: 'capability_insufficient',
    })
  })

  it('confidence 1 cannot bypass transaction tier, confirmation, input validation, or execution', async () => {
    const decisionOrchestrator = decisionFor('intent_classification', 'purchase_request')
    const payment = await runTurn(
      request({
        capability: 'act',
        graph: graph([{ type: 'action/set', actions: enabled('payment.initiate') }]),
        brain: brainFor('payment.initiate'),
        decisionOrchestrator,
      }),
    )
    expect(payment.actions[0]).toMatchObject({
      policy: 'denied',
      execution: 'not_attempted',
      denialReason: 'capability_insufficient',
    })

    const bookingGraph = graph([{ type: 'action/set', actions: enabled('booking.create') }])
    const pending = await runTurn(
      request({
        graph: bookingGraph,
        brain: brainFor('booking.create'),
        resolver: CONFIRMING,
        decisionOrchestrator,
      }),
    )
    expect(pending.actions[0]).toMatchObject({
      policy: 'confirmation_required',
      execution: 'not_attempted',
    })

    const invalid = await runTurn(
      request({
        graph: bookingGraph,
        brain: probe({
          requestedActions: [{ actionId: 'booking.create', inputs: { slotId: 42 } }],
        }),
        confirmedActionIds: ['booking.create'],
        resolver: CONFIRMING,
        executor: executor(),
        decisionOrchestrator,
      }),
    )
    expect(invalid.actions[0]?.execution).toBe('not_attempted')
    expect(invalid.actions[0]?.policy).toBe('denied')

    const unresolved = await runTurn(
      request({
        graph: bookingGraph,
        brain: brainFor('booking.create'),
        confirmedActionIds: ['booking.create'],
        executor: executor(),
        decisionOrchestrator,
      }),
    )
    expect(unresolved.actions[0]?.execution).toBe('not_attempted')

    const noExecutor = await runTurn(
      request({
        graph: bookingGraph,
        brain: brainFor('booking.create'),
        confirmedActionIds: ['booking.create'],
        resolver: CONFIRMING,
        decisionOrchestrator,
      }),
    )
    expect(noExecutor.actions[0]).toMatchObject({ policy: 'allowed', execution: 'not_attempted' })
  })

  it('projects and redacts data before a provider receives it', async () => {
    let sent = ''
    const provider: DecisionProvider = {
      providerId: 'inspector',
      model: 'test',
      health: { ready: true },
      decide: (query) => {
        sent = JSON.stringify(query)
        return Promise.resolve({
          providerId: 'inspector',
          model: 'test',
          decisions: [],
          refused: query.questions.map((question) => ({ id: question.id, reason: 'skip' })),
        })
      },
    }
    await runTurn(
      request({
        utterance: 'Email me at guest@example.com or call +6281234567890',
        knowledge: retrieve({
          sourceId: 'published',
          text: 'Contact private@example.com for details.',
        }),
        decisionOrchestrator: new DecisionOrchestrator({
          mode: 'shadow',
          provider,
          enabledTasks: ['intent_classification'],
        }),
      }),
    )
    expect(sent).not.toContain('guest@example.com')
    expect(sent).not.toContain('private@example.com')
    expect(sent).not.toContain('6281234567890')
    expect(sent).not.toContain('customerRef')
  })
})

describe('the masking boundary (§16)', () => {
  it('projects field names and never a form value', async () => {
    // The graph carries field *names*; a value cannot be projected because it
    // was never carried. A pending field reaches the brain as a name only.
    const brain = probe()
    await runTurn(
      request({
        graph: graph([
          { type: 'entities/set', entities: ROOMS },
          { type: 'form/set', form: BOOK_FORM },
        ]),
        brain,
      }),
    )

    const form = seen(brain).form as Record<string, unknown>
    expect(form.pendingFields).toEqual(['guest_phone'])
    expect(form.withheldFields).toEqual(['guest_phone'])
    expect(form).not.toHaveProperty('value')
    expect(JSON.stringify(seen(brain))).not.toMatch(/\+62|0812/)
  })

  it('redacts a projected key the tenant declares sensitive, and says so', async () => {
    // Masking is loud when it fires: a redacted answer is distinguishable from
    // a silent one, so nobody mistakes a withheld field for a wrong one.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'page/section', section: 'beachfront' }]),
        clientSensitiveFields: ['section'],
        brain: probe(),
      }),
    )
    expect(outcome.notices.some((notice) => notice.includes('section'))).toBe(true)
  })

  it('records no notice when nothing was withheld', async () => {
    const outcome = await runTurn(request({ graph: graph(), brain: probe() }))
    expect(outcome.notices).toEqual([])
  })
})

describe('async ports (V1.1)', () => {
  const PRICE_QUESTION = 'How much is Room 12 per night?'

  it('reads a truth answer that arrives after the call returns', async () => {
    // The regression that matters. A port typed `T | Promise<T>` lets a caller
    // consume the record without awaiting, and with a synchronous double that
    // mistake is invisible: the value is already there. This port does not
    // answer until a microtask later, so any pipeline that skipped the `await`
    // would read `undefined` from the record and — worse — treat `Object.keys()`
    // of a promise as an answered subject set.
    const port = deferredTruth({ price: { amountMinor: 1_200_000, currency: 'IDR' } })
    const outcome = await runTurn(
      request({
        utterance: PRICE_QUESTION,
        graph: graph(),
        brain: probe(),
        knowledge: retrieve({ sourceId: 'stale-price', text: 'Room 12 costs 900000 IDR.' }),
        truth: port,
      }),
    )
    expect(port.settled).toBe(1)
    expect(outcome.basis).toBe('structured_truth')
    expect(outcome.structuredTruth).toEqual({ price: { amountMinor: 1_200_000, currency: 'IDR' } })
    // The stale chunk was fetched but never became the answer.
    expect(outcome.knowledgeGap).toBe(false)
    expect(outcome.knowledgeGap).toBe(outcome.basis === 'none')
  })

  it('reads retrieval context that arrives after the call returns', async () => {
    const knowledge: KnowledgePort = {
      async retrieve(): Promise<RetrievalOutcome> {
        await Promise.resolve()
        return {
          plan: { need: 'retrieval_knowledge', subjects: [], query: '' },
          context: [chunk('k1')],
          deferToStructuredTruth: false,
        }
      },
    }
    const brain = probe()
    const outcome = await runTurn(
      request({ utterance: 'What is the cancellation policy?', graph: graph(), brain, knowledge }),
    )
    expect(outcome.basis).toBe('retrieval')
    expect(grounding(brain)).toEqual([chunk('k1')])
  })

  it('records a knowledge gap when the truth port rejects, not a fallback answer', async () => {
    // An unreachable live system is the §17 case, not a licence to answer from
    // retrieval. The rejection has to be caught by the pipeline, which means it
    // has to be a real rejected promise rather than a synchronous throw.
    const outcome = await runTurn(
      request({
        utterance: PRICE_QUESTION,
        graph: graph(),
        brain: probe(),
        knowledge: retrieve({ sourceId: 'stale-price', text: 'Room 12 costs 900000 IDR.' }),
        truth: {
          async resolve(): Promise<Readonly<Record<string, unknown>>> {
            await Promise.resolve()
            throw new Error('pricing service unreachable')
          },
        },
      }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
    // Empty, not absent: the field is always a record, so "no live answer" is
    // expressed as no keys rather than a hole a caller has to check for.
    expect(outcome.structuredTruth).toEqual({})
    expect(outcome.text).not.toContain('900000')
  })

  it('withholds retrieval from the brain on an unanswered live-value turn', async () => {
    // The guard above passes for the wrong reason with a brain that cannot read
    // what it was handed: `probe()` returns a fixed string, so a pipeline that
    // passed the stale chunk along would still produce an answer with no price
    // in it. This brain is shaped like the `ScriptedBrain` fallback — it quotes
    // `turn.grounding[0].text` verbatim whenever it is handed one — so the only
    // thing standing between a rejected truth port and a quoted stale price is
    // whether §17's withholding held at the call site.
    const turns: BrainTurn[] = []
    const quoting: BrainProvider & { readonly turns: readonly BrainTurn[] } = {
      providerId: 'quoting-brain',
      model: 'reference-model',
      health: { ready: true, reason: null },
      reply(turn) {
        turns.push(turn)
        const first = turn.grounding[0]
        return Promise.resolve({
          text: first === undefined ? '' : first.text,
          requestedActions: [],
          citations: [],
          deferToStructuredTruth: false,
        })
      },
      get turns() {
        return turns
      },
    }
    const outcome = await runTurn(
      request({
        utterance: PRICE_QUESTION,
        graph: graph(),
        brain: quoting,
        knowledge: retrieve({ sourceId: 'stale-price', text: 'Room 12 costs 900000 IDR.' }),
        truth: {
          async resolve(): Promise<Readonly<Record<string, unknown>>> {
            await Promise.resolve()
            throw new Error('pricing service unreachable')
          },
        },
      }),
    )
    // Retrieval ran and found the chunk; the brain was never shown it.
    expect(grounding(quoting)).toEqual([])
    // And it reports the same mode, so a provider that does check is told the
    // truth rather than left to infer it from an empty array.
    expect(quoting.turns[0]?.knowledgeMode).toBe('none')
    // So there is nothing to quote, and no citation to lend it authority.
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
    expect(outcome.text).toBe('')
    expect(outcome.citations).toEqual([])
  })

  it('asks an async resolver before running an entity-bearing action', async () => {
    const asked: [string, string][] = []
    const outcome = await runTurn(
      request({
        utterance: 'book Room 12',
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainFor('booking.create'),
        knowledge: EMPTY_RETRIEVAL,
        resolver: {
          async resolveExists(_tenantId, entityKind, entityId): Promise<boolean> {
            await Promise.resolve()
            asked.push([entityKind, String(entityId)])
            return entityKind === 'slot' && String(entityId) === 'slot_1'
          },
        },
        confirmedActionIds: ['booking.create'],
      }),
    )
    // §9 asked both entity questions — the slot and the nested customer ref —
    // before the gate answered either, which only works if the port's answer is
    // awaited rather than read as a synchronously-returned value.
    expect(asked).toEqual([
      ['slot', 'slot_1'],
      ['customer', 'cust_9'],
    ])
    // The slot exists, so that reference resolved; the customer does not, so the
    // nested refusal fires. The refusal names the path and the id that failed.
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.reason).toContain('customer.customerRef')
    expect(outcome.actions[0]?.reason).toContain('cust_9')
  })
})

describe('structured truth (§17)', () => {
  const PRICE_QUESTION = 'How much is Room 12 per night?'
  const priceTurn = (overrides: Partial<TurnRequest> = {}): TurnRequest =>
    request({ utterance: PRICE_QUESTION, ...overrides })

  it('answers a live question from the live system, not from retrieval', async () => {
    const outcome = await runTurn(
      priceTurn({
        graph: graph(),
        brain: probe(),
        knowledge: retrieve({ sourceId: 'stale-price', text: 'Room 12 costs 900000 IDR.' }),
        truth: truth({ price: { amountMinor: 1_200_000, currency: 'IDR' } }),
      }),
    )
    expect(outcome.basis).toBe('structured_truth')
    expect(outcome.structuredTruth).toEqual({ price: { amountMinor: 1_200_000, currency: 'IDR' } })
    expect(outcome.knowledgeGap).toBe(false)
  })

  it('reports no gap once the live system has answered, whatever retrieval did', async () => {
    // §17 outranks retrieval, so the gap and the basis have to agree. The port
    // answered `price`, yet retrieval for the same utterance came back empty —
    // which used to be recorded as a knowledge gap on a turn that carries the
    // answer, telling the operator the visitor was left without one. A gap is
    // about the visitor, not about whether a second system also had a match.
    const outcome = await runTurn(
      priceTurn({
        graph: graph(),
        brain: probe(),
        knowledge: EMPTY_RETRIEVAL,
        truth: deferredTruth({ price: { amountMinor: 1_200_000, currency: 'IDR' } }),
      }),
    )
    expect(outcome.basis).toBe('structured_truth')
    expect(outcome.knowledgeGap).toBe(false)
    // The invariant, stated once: the two fields never contradict each other.
    expect(outcome.knowledgeGap).toBe(outcome.basis === 'none')
  })

  it('does not let retrieval become the basis when the live system is unreachable', async () => {
    // §17's absolute: a price question with no live answer is not answered by a
    // chunk, however confident the chunk looks. A stale price quoted as fact is
    // the failure this clause exists to prevent.
    const outcome = await runTurn(
      priceTurn({
        graph: graph(),
        brain: probe(),
        knowledge: retrieve({ sourceId: 'stale-price', text: 'Room 12 costs 900000 IDR.' }),
      }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
  })

  it('treats a live system that answered nothing as unresolved', async () => {
    const outcome = await runTurn(
      priceTurn({
        graph: graph(),
        brain: probe(),
        knowledge: retrieve({ sourceId: 'k1' }),
        truth: EMPTY_TRUTH,
      }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
  })

  it('classifies before retrieving, so retrieval cannot reclassify the question', async () => {
    // The classifier runs first and hands the live subjects onward; retrieval
    // sees the same utterance regardless of what it decided.
    const resolved: unknown[] = []
    const outcome = await runTurn(
      priceTurn({
        graph: graph(),
        brain: probe(),
        knowledge: retrieve({ sourceId: 'k1' }),
        truth: {
          resolve: (
            subjects: readonly StructuredTruthSubject[],
          ): Promise<Readonly<Record<string, unknown>>> => {
            resolved.push([...subjects])
            return Promise.resolve({ price: { amountMinor: 1_200_000, currency: 'IDR' } })
          },
        },
      }),
    )
    expect(resolved).toEqual([['price']])
    expect(outcome.basis).toBe('structured_truth')
    expect(outcome.actions).toHaveLength(0)
  })

  it('answers a retrieval question from retrieval when no live system is needed', async () => {
    const outcome = await runTurn(
      request({
        utterance: 'Which rooms are near the beach?',
        graph: graph(),
        brain: probe({ citations: [{ sourceId: 'k1', sourceTitle: 'Doc k1' }] }),
        truth: truth({ price: { amountMinor: 999_999 } }),
      }),
    )
    expect(outcome.basis).toBe('retrieval')
    expect(outcome.structuredTruth).toEqual({})
    expect(outcome.knowledgeGap).toBe(false)
  })

  it('records a knowledge gap when retrieval came back empty', async () => {
    const outcome = await runTurn(
      request({ graph: graph(), brain: probe(), knowledge: EMPTY_RETRIEVAL }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
  })

  it('survives a retrieval system that throws', async () => {
    // An unreachable knowledge system is a reported gap, not a crashed turn and
    // not a confident guess. The visitor still gets an answer.
    const outcome = await runTurn(
      request({
        graph: graph(),
        brain: probe({ text: 'I could not find that.' }),
        knowledge: {
          retrieve: (): Promise<RetrievalOutcome> => Promise.reject(new Error('store offline')),
        },
      }),
    )
    expect(outcome.basis).toBe('none')
    expect(outcome.knowledgeGap).toBe(true)
    expect(outcome.text).toBe('I could not find that.')
  })
})

describe('the policy gate (§18)', () => {
  it('gives the brain only what the capability, role, and page allow', async () => {
    const brain = probe()
    await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: enabled('order.status.read', 'cart.item.add') },
        ]),
        brain,
      }),
    )
    expect(permitted(brain)).toEqual(['order.status.read', 'cart.item.add'])
  })

  it('narrows what the page offers without ever widening it', async () => {
    // `enabled: false` removes an action. The page cannot add one that the
    // capability tier did not already permit.
    const brain = probe()
    const outcome = await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: [{ name: 'order.status.read', enabled: false }] },
        ]),
        brain,
      }),
    )
    expect(permitted(brain)).toEqual([])
    expect(outcome.permittedActionIds).toEqual([])
  })

  it('lets a page that says nothing narrow nothing further', async () => {
    // A page that lists an action has offered it, and the answer is "yes" without
    // qualification: `permittedActionIds` is the whole offer, and one entry in it
    // is one entry in the outcome.
    const brain = probe()
    await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain,
      }),
    )
    expect(permitted(brain)).toEqual(['order.status.read'])
  })

  it('denies an above-capability action as above capability, not as a page complaint', async () => {
    // The reference tenant runs at `act`; `payment.initiate` needs `transact`. The
    // page never offered it — the reference page deliberately omits it — and the
    // two facts used to collapse into one denial. Passing the page's list as the
    // gate's client allow-list made this read "not enabled for this client
    // configuration", which is also what an unlisted page action reads, so the
    // denial explained nothing about the tier. Weighed in the right order the
    // answer names the capability, because the capability is the reason there is
    // no page offer to consider.
    const outcome = await runTurn(
      request({
        capability: 'act',
        graph: graph([{ type: 'action/set', actions: enabled('ui.compare', 'booking.create') }]),
        brain: brainFor('payment.initiate'),
      }),
    )
    const payment = outcome.actions[0]
    expect(payment?.policy).toBe('denied')
    expect(payment?.execution).toBe('not_attempted')
    expect(payment?.reason).toBe(
      '"payment.initiate" needs the transact capability; this client is act.',
    )
  })

  it('denies an in-capability action the page does not offer', async () => {
    // §16 at the gate, and the reason is why: the client is Transact, so the tier
    // is not what refused it — the page is. A denial that named the tier here
    // would be a lie about which boundary did the narrowing.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('ui.compare'),
      }),
    )
    const compare = outcome.actions[0]
    expect(compare?.policy).toBe('denied')
    expect(compare?.execution).toBe('not_attempted')
    expect(compare?.reason).toBe('The current page does not offer "ui.compare".')
  })

  it('treats an action the page disabled as one it does not offer', async () => {
    // Disabled and absent are the same answer from the visitor's side: neither is
    // offered. The executor never hears about it.
    const run = executor()
    const outcome = await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: [{ name: 'booking.create', enabled: false }] },
        ]),
        brain: brainFor('booking.create'),
        executor: run,
        confirmedActionIds: ['booking.create'],
      }),
    )
    expect(outcome.actions[0]?.reason).toBe('The current page does not offer "booking.create".')
    expect(run.calls).toEqual([])
  })

  it('offers the brain nothing a page offering nothing can carry out', async () => {
    // Fail closed, and the reason is the offer rather than the tier: a page that
    // declared no affordances offers nothing, so an action it never listed cannot
    // be confirmed into existence by a visitor saying yes.
    const run = executor()
    const outcome = await runTurn(
      request({
        brain: brainFor('booking.create'),
        executor: run,
        confirmedActionIds: ['booking.create'],
      }),
    )
    expect(outcome.permittedActionIds).toEqual([])
    expect(outcome.actions[0]?.reason).toBe('The current page does not offer "booking.create".')
    expect(run.calls).toEqual([])
  })

  it('denies an action the brain invented, without taking the turn down', async () => {
    // A model that names an action has not thereby claimed the tier it needs.
    // The denial is an unknown action id — a defect in the caller, not an
    // entitlement this client lacks — so it is not a capability handoff either:
    // no tier change makes an action the registry does not have exist.
    const outcome = await runTurn(
      request({ graph: graph(), brain: brainFor('booking.cancel_with_refund') }),
    )
    expect(outcome.actions).toHaveLength(1)
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.execution).toBe('not_attempted')
    expect(outcome.actions[0]?.denialReason).toBe('unknown_action')
    expect(outcome.actions[0]?.reason).toContain('No action "booking.cancel_with_refund"')
    expect(outcome.handoff).toBeNull()
  })

  it('holds an unconfirmed L3 action at confirmation_required and does not run it', async () => {
    // §18: capability tier does not override risk rules. A Transact client still
    // needs to confirm an L3 action — while the L0 read beside it runs free.
    const outcome = await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: enabled('booking.create', 'order.status.read') },
        ]),
        brain: brainFor('booking.create', 'order.status.read'),
        resolver: CONFIRMING,
      }),
    )
    const booking = outcome.actions.find((action) => action.actionId === 'booking.create')
    expect(booking?.policy).toBe('confirmation_required')
    expect(booking?.execution).toBe('not_attempted')
    expect(booking?.reason).toBe('Confirmation required via user_confirm.')
    expect(booking?.prompt).toContain('booking.create')

    const read = outcome.actions.find((action) => action.actionId === 'order.status.read')
    expect(read?.policy).toBe('allowed')
    expect(read?.execution).toBe('not_attempted')
  })

  it('runs the same action once the visitor confirmed it', async () => {
    const run = executor()
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainFor('booking.create'),
        executor: run,
        confirmedActionIds: ['booking.create'],
        resolver: CONFIRMING,
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('allowed')
    expect(outcome.actions[0]?.execution).toBe('succeeded')
    expect(run.calls).toEqual([
      { action: 'booking.create', inputs: VALID_INPUTS['booking.create'] },
    ])
  })

  it('refuses an L5 admin action unless a human approved it', async () => {
    // "Never autonomous" means the confirmation mode is not enough: a human must
    // have actually approved it, by id, for this session. The capability is
    // Enterprise because `admin.config.update` requires it, so the gate reaches
    // the L5 refusal instead of stopping earlier at the tier.
    const graphWithAdmin = () =>
      graph([{ type: 'action/set', actions: enabled('admin.config.update') }])

    const denied = await runTurn(
      request({
        graph: graphWithAdmin(),
        brain: brainFor('admin.config.update'),
        role: 'admin',
        capability: 'enterprise',
      }),
    )
    expect(denied.actions[0]?.policy).toBe('denied')
    expect(denied.actions[0]?.execution).toBe('not_attempted')
    expect(denied.actions[0]?.reason).toContain('never runs autonomously')

    const run = executor()
    const approved = await runTurn(
      request({
        graph: graphWithAdmin(),
        brain: brainFor('admin.config.update'),
        role: 'admin',
        capability: 'enterprise',
        executor: run,
        confirmedActionIds: ['admin.config.update'],
        humanApprovedActionIds: ['admin.config.update'],
      }),
    )
    expect(approved.actions[0]?.policy).toBe('allowed')
    expect(approved.actions[0]?.execution).toBe('succeeded')
  })

  it('reads the action definition tier for the gate, not the client tier', async () => {
    // The client is Transact; `order.status.read` needs Assist. The permitted
    // set is derived from the client's tier, and the per-action decision from
    // the action's own requirement — so a tier can never quietly widen a check.
    const brain = probe()
    await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        capability: 'assist',
        brain,
      }),
    )
    expect(permitted(brain)).toEqual(['order.status.read'])
  })

  it('keeps a denied action out of the permitted set it offered the brain', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        capability: 'assist',
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
      }),
    )
    expect(outcome.permittedActionIds).toEqual(['order.status.read'])
    expect(outcome.actions[0]?.policy).toBe('allowed')
  })
})

describe('action input validation (§9)', () => {
  /** A brain whose inputs are whatever the test wrote, unvalidated in advance. */
  function brainWithInputs(
    actionId: string,
    inputs: Readonly<Record<string, unknown>>,
  ): BrainProvider {
    return {
      providerId: 'probe-brain',
      model: 'reference-model',
      health: { ready: true, reason: null },
      reply() {
        return Promise.resolve({
          text: 'On it.',
          requestedActions: [{ actionId, inputs }],
          citations: [],
          deferToStructuredTruth: false,
        })
      },
    }
  }

  const readingTurn = (brain: BrainProvider): TurnRequest =>
    request({
      graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
      brain,
    })

  it('refuses a missing required field instead of filling it in', async () => {
    // Fail closed: a booking with no slot is a booking that did not happen. The
    // refusal names the field and what it is for, so the model can ask.
    const outcome = await runTurn(readingTurn(brainWithInputs('order.status.read', {})))
    expect(outcome.actions[0]?.policy).toBe('denied')
    // The message names the field it refused and what the field is for, so the
    // model can ask the visitor for it instead of guessing at a value.
    expect(outcome.actions[0]?.reason).toContain('orderReference')
    expect(outcome.actions[0]?.reason).toContain('the order')
  })

  it('refuses a wrong type rather than coercing it', async () => {
    const outcome = await runTurn(
      readingTurn(brainWithInputs('order.status.read', { orderReference: 17 })),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.reason).toContain('must be a string')
  })

  it('refuses an unknown field rather than dropping it', async () => {
    // Refused twice over: dropping would hide a model reaching for a field the
    // contract does not have, and a tenant id in an input is a §23 accident.
    const outcome = await runTurn(
      readingTurn(
        brainWithInputs('order.status.read', { orderReference: 'ORD-1', tenantId: 'other' }),
      ),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.reason).toContain('tenantId')
  })

  it('refuses an action that has no registered input contract', async () => {
    // The fail-closed default: a new action is inert until its contract exists,
    // rather than accepting whatever a model happens to send.
    const outcome = await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: enabled('order.status.read', 'payment.status.read') },
        ]),
        brain: brainWithInputs('payment.status.read', { paymentReference: 'PAY-1' }),
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.reason).toContain('No input contract')
  })

  it('refuses an entity id no resolver can confirm', async () => {
    // Existence is a question only the catalog can answer. A shape that parses
    // is not an id that exists, and pretending otherwise is the trap §9 names.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: resolverFor({ order: ['ORD-9'] }),
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.reason).toContain('ORD-1')
    expect(outcome.actions[0]?.reason).toContain('no order matching')
  })

  it('allows an entity id the resolver confirms', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: resolverFor({ order: ['ORD-1'] }),
        executor: executor(),
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('allowed')
    expect(outcome.actions[0]?.execution).toBe('succeeded')
  })

  it('verifies a nested entity id, not only the top-level ones', async () => {
    // `customer.customerRef` is a reference to a real customer one level down.
    // Checking the parent and not the child would let a booking be created
    // against a customer who does not exist.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainFor('booking.create'),
        resolver: resolverFor({ slot: ['slot_1'], customer: ['cust_other'] }),
        confirmedActionIds: ['booking.create'],
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    // The refusal names the nested path and the id that failed to resolve, so the
    // model knows which of the two it may retry.
    expect(outcome.actions[0]?.reason).toContain('customer.customerRef')
    expect(outcome.actions[0]?.reason).toContain('cust_9')
  })

  it('leaves an optional nested field out rather than inventing it', async () => {
    const run = executor()
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainWithInputs('booking.create', {
          slotId: 'slot_1',
          customer: { customerRef: 'cust_9' },
        }),
        resolver: resolverFor({ slot: ['slot_1'], customer: ['cust_9'] }),
        executor: run,
        confirmedActionIds: ['booking.create'],
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('allowed')
    expect(outcome.actions[0]?.execution).toBe('succeeded')
    expect(run.calls[0]?.inputs).toEqual({
      slotId: 'slot_1',
      customer: { customerRef: 'cust_9' },
    })
  })

  it('reports a failed execution separately from a denied one', async () => {
    // §8's separation: the gate allowed it, the executor tried, the store was
    // down. An operator must be able to tell that from a permission problem.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
        executor: {
          executorId: 'broken',
          execute() {
            return Promise.reject(new Error('store offline'))
          },
        },
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('allowed')
    expect(outcome.actions[0]?.execution).toBe('failed')
    expect(outcome.actions[0]?.errorCode).toBe('executor_threw')
    expect(outcome.actions[0]?.retryable).toBe(true)
    expect(outcome.actions[0]?.reason).toBe('store offline')
  })
})

/**
 * The executor's key, and what it is derived from (PRD §18).
 *
 * The key is a promise the pipeline makes and an executor keeps: repeat the call
 * with the same key, and the side effect is not repeated. That only holds if the
 * key is derived from the intent and from nothing else — not from a clock, not
 * from state an executor instance holds, and not from the order a model happened
 * to write its JSON in. A key that read the wall clock would differ on every
 * retry, so a double submit would double-book; a key that read only the action
 * id would be the same key for the second booking in a session, so a legitimate
 * one would be swallowed as a duplicate of the first.
 */
describe('idempotency key derivation (§18)', () => {
  /** A brain that asks for exactly the actions given, with exactly those inputs. */
  function brainAsking(
    ...asked: readonly {
      readonly actionId: string
      readonly inputs: Readonly<Record<string, unknown>>
    }[]
  ): BrainProvider {
    return {
      providerId: 'probe-brain',
      model: 'reference-model',
      health: { ready: true, reason: null },
      reply() {
        return Promise.resolve({
          text: 'On it.',
          requestedActions: asked,
          citations: [],
          deferToStructuredTruth: false,
        })
      },
    }
  }

  /** A page offering one L0 read, which is enough to reach the executor. */
  const readingOffer = (): ContextEvent[] => [
    { type: 'action/set', actions: enabled('order.status.read') },
  ]

  it('hands the executor a key at all, and records the same one on the action', async () => {
    // The key exists so that a port can recognise a repeat. An empty string
    // satisfies any check that only asks whether a key is present, and two empty
    // keys are equal — so an executor that kept its promise would collapse every
    // retry of every action into the first one's side effect.
    const run = executor()
    const outcome = await runTurn(
      request({
        graph: graph(readingOffer()),
        brain: brainFor('order.status.read'),
        executor: run,
        resolver: CONFIRMING,
      }),
    )
    expect(run.requests).toHaveLength(1)
    const key = keyOf(run.requests, 'order.status.read')
    expect(typeof key).toBe('string')
    expect(key).not.toBe('')
    // The record an operator reads carries the key that was actually sent.
    expect(outcome.actions[0]?.idempotencyKey).toBe(key)
  })

  it('derives the same key when the same turn is replayed', async () => {
    // The replay is the ordinary case, not the exotic one: the visitor tapped
    // twice, or the network retried. So the key has to come from the request —
    // which tenant, which session, which moment, which action, which inputs — and
    // from nothing an executor holds, which is why the second run here goes to a
    // second executor and still produces the first run's key.
    const asked = {
      graph: graph(readingOffer()),
      brain: brainFor('order.status.read'),
      resolver: CONFIRMING,
    }
    const first = executor()
    const second = executor()
    await runTurn(request({ ...asked, executor: first }))
    await runTurn(request({ ...asked, executor: second }))

    expect(keyOf(first.requests, 'order.status.read')).toBeDefined()
    expect(keyOf(second.requests, 'order.status.read')).toBe(
      keyOf(first.requests, 'order.status.read'),
    )
  })

  it('derives a different key for the same action at a different moment', async () => {
    // `occurredAt` is in the key because the same words an hour later are a
    // different intent. Without it the second ask would arrive carrying the first
    // one's key, and an executor doing exactly as promised would refuse the
    // booking the visitor had just made.
    const keyAt = async (occurredAt: string): Promise<string | undefined> => {
      const run = executor()
      await runTurn(
        request({
          graph: graph(readingOffer()),
          brain: brainFor('order.status.read'),
          executor: run,
          resolver: CONFIRMING,
          occurredAt,
        }),
      )
      return keyOf(run.requests, 'order.status.read')
    }
    const morning = await keyAt(OCCURRED_AT)
    expect(await keyAt('2026-04-01T10:00:00.000Z')).not.toBe(morning)
  })

  it('derives a different key for the same action in a different session', async () => {
    // Two guests asking for the same slot are two bookings, and the session is
    // what tells them apart. A key that stopped at the action id would make the
    // second look like a duplicate of the first.
    const keyFor = async (sessionId: string): Promise<string | undefined> => {
      const run = executor()
      await runTurn(
        request({
          graph: graph(readingOffer()),
          brain: brainFor('order.status.read'),
          executor: run,
          resolver: CONFIRMING,
          sessionId,
        }),
      )
      return keyOf(run.requests, 'order.status.read')
    }
    const mine = await keyFor('sess_1')
    expect(await keyFor('sess_2')).not.toBe(mine)
  })

  it('derives a different key for the same action in a different tenant (§23)', async () => {
    // One tenant's submit must never be recognised as another's, in either
    // direction: a key that omitted the tenant would let an executor that stores
    // keys per tenant — the shape every real one takes — hand one guest's
    // booking attempt to another guest's slot.
    const keyIn = async (tenantId: string): Promise<string | undefined> => {
      const run = executor()
      await runTurn(
        request({
          tenantId,
          graph:
            tenantId === TENANT
              ? graph(readingOffer())
              : foldContextEvents(seedContextGraph(configFor(tenantId), '/rooms'), readingOffer()),
          brain: brainFor('order.status.read'),
          executor: run,
          resolver: CONFIRMING,
        }),
      )
      return keyOf(run.requests, 'order.status.read')
    }
    const ours = await keyIn(TENANT)
    expect(await keyIn('other-hotels')).not.toBe(ours)
  })

  it('derives a different key for the same action carrying different inputs', async () => {
    // The inputs are the substance of the intent. Same action, same session, same
    // moment, a different customer: a different booking, and a key that could not
    // tell the two apart would let an executor de-duplicate it away.
    const asked = {
      graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
      resolver: CONFIRMING,
      confirmedActionIds: ['booking.create'],
    }
    const ours = executor()
    await runTurn(request({ ...asked, brain: brainFor('booking.create'), executor: ours }))
    const theirs = executor()
    await runTurn(
      request({
        ...asked,
        brain: brainAsking({
          actionId: 'booking.create',
          inputs: { slotId: 'slot_1', customer: { customerRef: 'cust_other' } },
        }),
        executor: theirs,
      }),
    )
    expect(keyOf(theirs.requests, 'booking.create')).not.toBe(
      keyOf(ours.requests, 'booking.create'),
    )
  })

  it('derives a different key for each of two actions in the same turn', async () => {
    // One key for the whole reply is the simplest thing to get wrong here. The
    // second action would arrive carrying the first's key, and an executor that
    // de-duplicates correctly — the thing the key is asking it to do — would
    // silently skip a side effect the visitor was promised.
    const run = executor()
    await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: enabled('order.status.read', 'product.read') },
        ]),
        brain: brainFor('order.status.read', 'product.read'),
        executor: run,
        resolver: CONFIRMING,
      }),
    )
    expect(run.calls.map((call) => call.action)).toEqual(['order.status.read', 'product.read'])
    expect(keyOf(run.requests, 'product.read')).not.toBe(keyOf(run.requests, 'order.status.read'))
  })

  it('derives the same key from inputs written in a different order', async () => {
    // Key order is an artefact of how a value was built, not part of the data:
    // `{a:1,b:2}` and `{b:2,a:1}` are the same booking. A key that treated them
    // as two would double-book on any client that reorders its JSON, which is a
    // client doing nothing wrong.
    const asked = {
      graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
      resolver: CONFIRMING,
      confirmedActionIds: ['booking.create'],
    }
    const forwards = executor()
    await runTurn(request({ ...asked, brain: brainFor('booking.create'), executor: forwards }))
    const backwards = executor()
    await runTurn(
      request({
        ...asked,
        brain: brainAsking({
          actionId: 'booking.create',
          inputs: { customer: { customerRef: 'cust_9' }, slotId: 'slot_1' },
        }),
        executor: backwards,
      }),
    )
    expect(keyOf(backwards.requests, 'booking.create')).toBe(
      keyOf(forwards.requests, 'booking.create'),
    )
  })

  it('cannot be made to collide by moving the separator across two fields (§23)', () => {
    // The key used to be a `'|'`-joined string, and `'|'` is a character a tenant
    // id is allowed to contain: `createSession` trims `tenantId` and validates
    // nothing else, and a host page passes whatever `sessionId` it likes. So
    // `tenantId: 'acme|x'` + `sessionId: 's1'` and `tenantId: 'acme'` +
    // `sessionId: 'x|s1'` produced byte-identical keys — and an executor keeping
    // its promise would have recognised the second as the first, applying one
    // tenant's attempt to another tenant's tenant-scoped side effect.
    //
    // Asserted through `buildIdempotencyKey` directly, because the pair of keys
    // is the whole claim. A test that ran these through `runTurn` would be
    // asserting a tenant mismatch as well, and a tenant that fails to resolve
    // never reaches the executor at all.
    const body = {
      occurredAt: OCCURRED_AT,
      actionId: 'order.status.read',
      inputs: { orderReference: 'R-1' },
    }
    const ours = buildIdempotencyKey({ ...body, tenantId: 'acme|x', sessionId: 's1' })
    const theirs = buildIdempotencyKey({ ...body, tenantId: 'acme', sessionId: 'x|s1' })
    expect(theirs).not.toBe(ours)
  })

  it('escapes a delimiter a caller smuggled in, so it cannot impersonate a field (§23)', () => {
    // Not a different separator, which would only move the boundary — the shape
    // means no field's content can match a boundary at all. A tenant id carrying
    // its own quotes and a pipe arrives as one tenant name, not two, so a reader
    // that parses the key back gets the tenant that asked and never the tenant
    // the id was dressed up to look like.
    const smuggled = buildIdempotencyKey({
      tenantId: 'acme"|"other',
      sessionId: 's1',
      occurredAt: OCCURRED_AT,
      actionId: 'order.status.read',
      inputs: { orderReference: 'R-1' },
    })
    // The frame is parsed back rather than matched as text, which is the claim:
    // the key is one structure, so a reader gets fields and never a phrase a
    // delimiter could have cut somewhere else. `toStrictEqual` over the whole
    // record also refuses a key that quietly dropped a field to make the poke
    // pass — the tenant arriving intact means every part of it arrived.
    const parsed = JSON.parse(smuggled) as {
      tenantId: string
      sessionId: string
      occurredAt: string
      actionId: string
      inputs: { orderReference: string }
    }
    expect(parsed).toStrictEqual({
      tenantId: 'acme"|"other',
      sessionId: 's1',
      occurredAt: OCCURRED_AT,
      actionId: 'order.status.read',
      inputs: { orderReference: 'R-1' },
    })

    // And a key is never a valid rendering of a field it does not carry: the
    // attempt the smuggled id would have impersonated has a different key,
    // because the tenant is one value and not a phrase a delimiter can cut.
    const impersonated = buildIdempotencyKey({
      tenantId: 'acme',
      sessionId: 's1',
      occurredAt: OCCURRED_AT,
      actionId: 'order.status.read',
      inputs: { orderReference: 'R-1' },
    })
    expect(smuggled).not.toBe(impersonated)
  })

  it('stays stable across a replay of the same turn and only that (§18)', async () => {
    // The promise, stated with its boundary attached, because the shape of the
    // key invites a stronger reading than either half of it supports. Same
    // request, same key — a retry is recognisable as the attempt it retries.
    // Same words at a later moment is a different key — a visitor asking again
    // tomorrow has made a new request, and a key that refused to notice would
    // have an executor answer a new booking with the old one's side effect.
    const ask = async (occurredAt: string): Promise<string | undefined> => {
      const run = executor()
      await runTurn(
        request({
          graph: graph(readingOffer()),
          brain: brainFor('order.status.read'),
          executor: run,
          resolver: CONFIRMING,
          occurredAt,
        }),
      )
      return keyOf(run.requests, 'order.status.read')
    }
    const first = await ask(OCCURRED_AT)
    expect(await ask(OCCURRED_AT)).toBe(first)
    expect(await ask('2026-04-02T09:00:00.000Z')).not.toBe(first)
  })
})

/**
 * Where "allowed" becomes "done" (PRD §18).
 *
 * The gate decides whether an action may run; the executor is what makes it have
 * run. These tests hold the two apart by watching the boundary itself. The
 * executor is the only thing in the file that can turn `not_attempted` into
 * anything else, so "was the executor called, and with what" is the question that
 * separates a pipeline that asked permission from one that reports side effects
 * nobody performed.
 */
describe('the executor boundary (§18)', () => {
  /** A brain whose inputs are whatever the test wrote, unvalidated in advance. */
  function brainWithInputs(
    actionId: string,
    inputs: Readonly<Record<string, unknown>>,
  ): BrainProvider {
    return {
      providerId: 'probe-brain',
      model: 'reference-model',
      health: { ready: true, reason: null },
      reply() {
        return Promise.resolve({
          text: 'On it.',
          requestedActions: [{ actionId, inputs }],
          citations: [],
          deferToStructuredTruth: false,
        })
      },
    }
  }

  it('never hands the executor an action the gate denied, while running the one it allowed', async () => {
    // One reply, two boundaries. The visitor asked to see an order and to pay for
    // it; the client is Act, so the payment is above its tier. The read runs and
    // the payment never reaches the executor — a denial forwarded anyway would
    // describe an attempt the tenant never permitted, and an executor would
    // perform it.
    const run = executor()
    const outcome = await runTurn(
      request({
        capability: 'act',
        graph: graph([
          { type: 'action/set', actions: enabled('order.status.read', 'payment.initiate') },
        ]),
        brain: brainFor('order.status.read', 'payment.initiate'),
        executor: run,
        resolver: CONFIRMING,
      }),
    )
    expect(run.calls).toEqual([
      { action: 'order.status.read', inputs: VALID_INPUTS['order.status.read'] },
    ])

    const payment = outcome.actions.find((action) => action.actionId === 'payment.initiate')
    expect(payment?.policy).toBe('denied')
    expect(payment?.execution).toBe('not_attempted')
    expect(payment?.idempotencyKey).toBeUndefined()
  })

  it('never hands the executor an action the brain invented', async () => {
    // The registry is the boundary on what may be asked for, and an executor is a
    // port to a real system — a booking store, a payment provider. Handing it an
    // id nobody registered would be asking that system to perform an act no one
    // has described, priced, or audited.
    const run = executor()
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainFor('booking.cancel_with_refund'),
        executor: run,
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.execution).toBe('not_attempted')
    expect(run.calls).toEqual([])
    expect(run.requests).toEqual([])
  })

  it('holds an unconfirmed action out of the executor, and hands it over once it is confirmed', async () => {
    // §18: capability tier does not override risk rules. `user_confirm` is a gate
    // in front of the executor, not a note beside it — while the action is
    // unconfirmed there is nothing for an executor to do, and nothing here may
    // pretend otherwise.
    const asked = {
      graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
      resolver: CONFIRMING,
    }
    const unconfirmed = executor()
    await runTurn(request({ ...asked, brain: brainFor('booking.create'), executor: unconfirmed }))
    expect(unconfirmed.calls).toEqual([])

    const confirmed = executor()
    await runTurn(
      request({
        ...asked,
        brain: brainFor('booking.create'),
        executor: confirmed,
        confirmedActionIds: ['booking.create'],
      }),
    )
    expect(confirmed.calls).toEqual([
      { action: 'booking.create', inputs: VALID_INPUTS['booking.create'] },
    ])

    // The confirmation itself is not an ingredient of the key: the coordinates
    // above are the ones the unconfirmed turn would have used, so a visitor who
    // confirms twice produces one key and one booking. What changes the key is
    // the booking — the same slot for a different customer is a different intent,
    // and a key that could not tell them apart would de-duplicate it away.
    const again = executor()
    await runTurn(
      request({
        ...asked,
        brain: brainFor('booking.create'),
        executor: again,
        confirmedActionIds: ['booking.create'],
      }),
    )
    const otherCustomer = executor()
    await runTurn(
      request({
        ...asked,
        brain: brainWithInputs('booking.create', {
          slotId: 'slot_1',
          customer: { customerRef: 'cust_other' },
        }),
        executor: otherCustomer,
        confirmedActionIds: ['booking.create'],
      }),
    )
    expect(keyOf(confirmed.requests, 'booking.create')).toBe(
      keyOf(again.requests, 'booking.create'),
    )
    expect(keyOf(confirmed.requests, 'booking.create')).not.toBe(
      keyOf(otherCustomer.requests, 'booking.create'),
    )
  })

  it('never hands the executor an action whose inputs §9 refused', async () => {
    // §9 sits between the gate and the executor for the same reason: an order
    // reference typed as a number is not an order reference, and an executor that
    // received it would either guess or fail against a real system. The executor
    // is not called at all — not called and then ignored.
    const run = executor()
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainWithInputs('order.status.read', { orderReference: 17 }),
        executor: run,
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('denied')
    expect(outcome.actions[0]?.execution).toBe('not_attempted')
    expect(run.calls).toEqual([])
    expect(run.requests).toEqual([])
  })

  it('records an executor that threw as an allowed action that failed to run', async () => {
    // §8's separation, seen from the executor's side: the gate said yes, the call
    // threw, and neither fact may be written over the other. The policy verdict
    // stays `allowed` — the tenant is not misconfigured, the integration is
    // broken — while the execution is `failed` with a code and a retryability an
    // operator can act on. The record keeps the key the failed call was sent
    // with, so a retry of that request is recognisable as the same attempt.
    const handed: ActionExecutionRequest[] = []
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
        executor: {
          executorId: 'broken',
          execute(actionRequest) {
            handed.push(actionRequest)
            return Promise.reject(new Error('store offline'))
          },
        },
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('allowed')
    expect(outcome.actions[0]?.execution).toBe('failed')
    expect(outcome.actions[0]?.errorCode).toBe('executor_threw')
    expect(outcome.actions[0]?.retryable).toBe(true)
    expect(outcome.actions[0]?.reason).toBe('store offline')
    expect(outcome.actions[0]?.idempotencyKey).toBe(handed[0]?.idempotencyKey)
  })

  it('does not take the turn down when the executor throws', async () => {
    // A broken integration is an ordinary turn outcome. The visitor asked a
    // question and is owed an answer; throwing here would turn a booking store
    // that is down into a blank page, which is a worse failure than the one that
    // started it. The answer arrives, and the rest of the turn's work stands.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
        executor: {
          executorId: 'broken',
          execute() {
            throw new Error('store offline')
          },
        },
      }),
    )
    expect(outcome.text).toBe('On it.')
    expect(outcome.actions).toHaveLength(1)
    expect(outcome.permittedActionIds).toEqual(['order.status.read'])
  })

  it('treats a thrown value that is not an Error as a failed execution, not a crash', async () => {
    // Nothing obliges an executor to throw an Error — a rejected promise carrying
    // a string is a legal port. `error instanceof Error` is false there, and the
    // catch has to have an answer for it that is not "the turn dies": the failure
    // is still recorded, with the sentence the pipeline can safely surface.
    const notAnError = 'the store said nothing useful'
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
        executor: {
          executorId: 'rude',
          execute() {
            // The pipeline has to answer a port that rejects with something that is not an
            // Error, so this test cannot reject with an Error to cover it.
            // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors -- the subject under test
            return Promise.reject(notAnError)
          },
        },
      }),
    )
    expect(outcome.actions[0]?.policy).toBe('allowed')
    expect(outcome.actions[0]?.execution).toBe('failed')
    expect(outcome.actions[0]?.errorCode).toBe('executor_threw')
    expect(outcome.actions[0]?.retryable).toBe(true)
    expect(outcome.actions[0]?.reason).toBe('The action executor failed.')
    expect(outcome.text).toBe('On it.')
  })

  it('folds a success in with its output and its key, and counts nothing else as executed', async () => {
    // `actionExecuted` is the one question a client can ask about a side effect:
    // did this happen? It is true only when the gate allowed it and an executor
    // confirmed it, so the three other shapes in this reply — an action still
    // waiting for its confirmation, an action the gate refused outright — must
    // not all read as done. The output is the executor's, and the key on the
    // record is the key the request carried.
    const handed: ActionExecutionRequest[] = []
    const outcome = await runTurn(
      request({
        capability: 'act',
        graph: graph([
          {
            type: 'action/set',
            actions: enabled('booking.create', 'booking.reschedule', 'payment.initiate'),
          },
        ]),
        brain: brainFor('booking.create', 'booking.reschedule', 'payment.initiate'),
        resolver: CONFIRMING,
        confirmedActionIds: ['booking.create'],
        executor: {
          executorId: 'booking-store',
          execute(actionRequest) {
            handed.push(actionRequest)
            return Promise.resolve({ status: 'succeeded', output: { bookingId: 'book_7' } })
          },
        },
      }),
    )

    const booking = outcome.actions.find((action) => action.actionId === 'booking.create')
    expect(booking?.policy).toBe('allowed')
    expect(booking?.execution).toBe('succeeded')
    expect(booking?.output).toEqual({ bookingId: 'book_7' })
    expect(booking?.idempotencyKey).toBe(handed[0]?.idempotencyKey)

    const reschedule = outcome.actions.find((action) => action.actionId === 'booking.reschedule')
    expect(reschedule?.policy).toBe('confirmation_required')
    expect(reschedule?.execution).toBe('not_attempted')
    expect(reschedule?.idempotencyKey).toBeUndefined()

    const payment = outcome.actions.find((action) => action.actionId === 'payment.initiate')
    expect(payment?.policy).toBe('denied')
    expect(payment?.execution).toBe('not_attempted')

    expect(
      outcome.actions.filter((action) => actionExecuted(action)).map((a) => a.actionId),
    ).toEqual(['booking.create'])
  })
})

describe('generative UI (§25)', () => {
  const cta = (actionId: string): { kind: string; props: Record<string, unknown> } => ({
    kind: 'cta',
    props: { label: 'Book', actionId },
  })

  it('keeps a component the registry accepts', async () => {
    const outcome = await runTurn(
      request({
        graph: graph(),
        brain: brainWithComponents([
          {
            kind: 'recommendation_list',
            props: {
              items: [
                { entityId: 'room-12', entityName: 'Room 12', reason: 'closest', sourceId: null },
              ],
            },
          },
        ]),
      }),
    )
    expect(outcome.components).toHaveLength(1)
    expect(outcome.components[0]?.kind).toBe('recommendation_list')
    expect(outcome.rejectedComponents).toEqual([])
  })

  it('rejects an invented kind and reports why instead of rendering it', async () => {
    // Model output selects from a registry. It does not execute arbitrary
    // client-side code, so an unknown kind never becomes a component.
    const outcome = await runTurn(
      request({
        graph: graph(),
        brain: brainWithComponents([{ kind: 'arbitrary_widget', props: {} }]),
      }),
    )
    expect(outcome.components).toEqual([])
    expect(outcome.rejectedComponents).toHaveLength(1)
    expect(outcome.rejectedComponents[0]).toContain('arbitrary_widget')
  })

  it('drops a CTA whose action the gate did not permit, keeping its siblings', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainWithComponents([
          cta('admin.config.update'),
          {
            kind: 'faq_source_card',
            props: { sourceId: 's1', sourceTitle: 'Policy', excerpt: 'Check-in is 14:00.' },
          },
        ]),
      }),
    )
    // The card runs nothing, so the gate does not touch it.
    expect(outcome.components.map((component) => component.kind)).toEqual(['faq_source_card'])
  })
})

describe('handoff and events (§26, §27)', () => {
  it('hands over with the story the visitor should not have to repeat', async () => {
    // The handoff exists because this visitor asked, and the payload is built
    // from the graph and the turn record — `room-12` from the entity in focus, the
    // recorded error with its timestamp. §26: no form value ever reaches it.
    const outcome = await runTurn(
      request({
        graph: graph([
          { type: 'entities/select', entityId: 'room-12' },
          { type: 'session/authenticated', role: 'guest', customerRef: 'cust_9' },
          { type: 'error/recorded', code: 'booking_slot_unavailable', occurredAt: OCCURRED_AT },
        ]),
        brain: brainFor('order.status.read'),
        handoffRequested: true,
      }),
    )
    expect(outcome.handoff?.reason).toBe('visitor_requested')
    expect(outcome.handoff?.summary).toContain('room-12')
    expect(outcome.handoff?.errors).toEqual([
      {
        code: 'booking_slot_unavailable',
        message: `Recorded at ${OCCURRED_AT}.`,
      },
    ])
  })

  it('escalates after repeated failure without being asked', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([
          { type: 'error/recorded', code: 'booking_slot_unavailable', occurredAt: OCCURRED_AT },
          { type: 'error/recorded', code: 'payment_declined', occurredAt: OCCURRED_AT },
          { type: 'error/recorded', code: 'checkout_timeout', occurredAt: OCCURRED_AT },
        ]),
        brain: probe(),
      }),
    )
    expect(outcome.handoff?.reason).toBe('repeated_failure')
  })

  it('does not escalate on two recorded errors — the threshold is three (§26)', async () => {
    // The boundary, from below. A threshold lowered to two passes every test that
    // only ever exercises three, so the absence needs its own assertion: two
    // recorded errors is a visitor with a problem, not a visitor the platform has
    // already failed three times.
    const two = await runTurn(
      request({
        graph: graph([
          { type: 'error/recorded', code: 'booking_slot_unavailable', occurredAt: OCCURRED_AT },
          { type: 'error/recorded', code: 'payment_declined', occurredAt: OCCURRED_AT },
        ]),
        brain: probe(),
      }),
    )
    expect(two.handoff).toBeNull()
  })

  it('counts failed executions to the same threshold (§26)', async () => {
    // The second scope, at its own boundary. Two attempts that returned `failed`
    // is not three, and three is — the two halves of `repeated_failure` fail
    // together, so both need the assertion from below and above.
    const broken: ActionExecutor = {
      executorId: 'broken',
      execute: (request): Promise<ActionExecutionResult> =>
        Promise.resolve({
          status: 'failed',
          errorCode: 'store_offline',
          retryable: true,
          message: `Store offline for ${request.action}.`,
        }),
    }

    const two = await runTurn(
      request({
        graph: graph([
          { type: 'action/set', actions: enabled('product.read', 'availability.read') },
        ]),
        brain: brainFor('product.read', 'availability.read'),
        resolver: CONFIRMING,
        executor: broken,
      }),
    )
    expect(two.actions.map((action) => action.execution)).toEqual(['failed', 'failed'])
    expect(two.handoff).toBeNull()

    const three = await runTurn(
      request({
        graph: graph([
          {
            type: 'action/set',
            actions: enabled('product.read', 'availability.read', 'order.status.read'),
          },
        ]),
        brain: brainFor('product.read', 'availability.read', 'order.status.read'),
        resolver: CONFIRMING,
        executor: broken,
      }),
    )
    expect(three.actions.map((action) => action.execution)).toEqual(['failed', 'failed', 'failed'])
    expect(three.handoff?.reason).toBe('repeated_failure')
    // No graph error was recorded, so this escalation came from the failed
    // executions and not from the session arriving with three of its own.
    expect(three.handoff?.errors).toEqual([])
  })

  it('does not count successes or pending confirmations as failures (§26)', async () => {
    // Three reads that worked is a visitor getting things done. Counting them
    // would escalate exactly the turns that went well, and a confirmation held
    // open is a visitor being asked — neither is an attempt that broke.
    const succeeded = await runTurn(
      request({
        graph: graph([
          {
            type: 'action/set',
            actions: enabled('product.read', 'availability.read', 'order.status.read'),
          },
        ]),
        brain: brainFor('product.read', 'availability.read', 'order.status.read'),
        resolver: CONFIRMING,
        executor: executor(),
      }),
    )
    expect(succeeded.actions.map((action) => action.execution)).toEqual([
      'succeeded',
      'succeeded',
      'succeeded',
    ])
    expect(succeeded.handoff).toBeNull()

    const confirming = await runTurn(
      request({
        graph: graph([
          {
            type: 'action/set',
            actions: enabled('form.submit', 'email.send', 'booking.reschedule'),
          },
        ]),
        brain: brainFor('form.submit', 'email.send', 'booking.reschedule'),
        resolver: CONFIRMING,
      }),
    )
    // Assert the policy too: an action §9 or the gate refused for another reason
    // would come back `denied` and take the handoff down the `capability_exceeded`
    // branch, which would let this pass for the wrong reason.
    expect(confirming.actions.map((action) => action.policy)).toEqual([
      'confirmation_required',
      'confirmation_required',
      'confirmation_required',
    ])
    expect(confirming.actions.map((action) => action.execution)).toEqual([
      'not_attempted',
      'not_attempted',
      'not_attempted',
    ])
    expect(confirming.handoff).toBeNull()
  })

  it('separates a denied action from a failed execution (§8)', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('admin.config.update'),
        utterance: 'Which rooms are near the beach?',
      }),
    )
    const names = outcome.events.map((event) => event.name)
    expect(names).toContain('meaningful_question_answered')
    // An action the gate refused is `action_denied`. It is not `tool_failure`,
    // which is reserved for a tool the platform depends on breaking, and not
    // `action_execution_failed`, which is an attempt that did not work out.
    expect(names).toContain('action_denied')
    expect(names).not.toContain('action_execution_failed')
    expect(names).not.toContain('tool_failure')
    expect(names).not.toContain('knowledge_gap')
    expect(outcome.events.find((event) => event.name === 'action_denied')?.subjectId).toBe(
      'admin.config.update',
    )
  })

  it('records a confirmation hold as its own event (§8)', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('booking.create') }]),
        brain: brainFor('booking.create'),
      }),
    )
    const names = outcome.events.map((event) => event.name)
    expect(names).toContain('action_confirmation_required')
    expect(names).not.toContain('action_denied')
    expect(names).not.toContain('action_execution_succeeded')
  })

  it('records a confirmed side effect as its own event (§8)', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
        executor: executor(),
      }),
    )
    const names = outcome.events.map((event) => event.name)
    expect(names).toContain('action_execution_succeeded')
    expect(names).not.toContain('action_denied')
  })

  it('records an attempt that failed as its own event (§8)', async () => {
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
        executor: {
          executorId: 'broken',
          execute() {
            return Promise.resolve({
              status: 'failed',
              errorCode: 'store_offline',
              retryable: true,
              message: 'Store offline.',
            })
          },
        },
      }),
    )
    const names = outcome.events.map((event) => event.name)
    expect(names).toContain('action_execution_failed')
    expect(names).not.toContain('action_denied')
    expect(outcome.events.find((event) => event.name === 'action_execution_failed')?.note).toBe(
      'store_offline',
    )
  })
  it('emits nothing for an allowed action that was never attempted', async () => {
    // `allowed` with no executor means no side effect happened. Emitting a
    // success event here is how an assistant comes to report bookings it never
    // made.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        resolver: CONFIRMING,
      }),
    )
    const names = outcome.events.map((event) => event.name)
    expect(names).not.toContain('action_execution_succeeded')
    expect(names).not.toContain('action_execution_failed')
    expect(names).not.toContain('action_denied')
  })

  it('emits a knowledge gap and stays quiet when there is no answer', async () => {
    const outcome = await runTurn(
      request({ graph: graph(), brain: probe({ text: '' }), knowledge: EMPTY_RETRIEVAL }),
    )
    const names = outcome.events.map((event) => event.name)
    expect(names).toEqual(['knowledge_gap'])
  })
})

/**
 * §26 escalation, case by case, because the whole value of `capability_exceeded`
 * is that an operator can act on it.
 *
 * The rule this table pins: only denials about the client's *entitlements* map to
 * `capability_exceeded`. A visitor's "no", a page's "not offered here", an invented
 * action id, and a §9 input refusal each mean something a tier change cannot fix,
 * and each of them used to arrive here as `capability_exceeded` anyway — because
 * the code asked "was this denied?" instead of "why was this denied?".
 *
 * Each case is its own `it` and asserts the handoff reason outright, not merely
 * that the field is absent. A test that says `toBeUndefined()` on the reason would
 * still pass if the whole handoff were dropped for a different bug.
 */
describe('handoff escalation is keyed to the denial, not to the fact of it (§26)', () => {
  const escalated = (outcome: Awaited<ReturnType<typeof runTurn>>): void => {
    expect(outcome.handoff?.reason).toBe('capability_exceeded')
  }
  const notEscalated = (outcome: Awaited<ReturnType<typeof runTurn>>): void => {
    // No handoff at all — not a handoff with some other reason. The distinction
    // matters: a turn nobody must act on is a turn with nothing on the queue.
    expect(outcome.handoff).toBeNull()
  }

  it('escalates an insufficient capability, and says which tier was short', async () => {
    // The reference case, and the one operator action follows from: the action
    // needs `transact`, the client is `act`, so the answer is to raise the tier.
    const outcome = await runTurn(
      request({
        capability: 'act',
        graph: graph([{ type: 'action/set', actions: enabled('payment.initiate') }]),
        brain: brainFor('payment.initiate'),
      }),
    )
    expect(outcome.actions[0]?.denialReason).toBe('capability_insufficient')
    escalated(outcome)
  })

  it('does not escalate a visitor’s own decline — no entitlement can undo it', async () => {
    // A decline is a person's decision, and it survives the turn that produced it.
    // `capability_exceeded` here tells an operator to raise a tier that was never
    // too low; the only thing it could prompt is asking the visitor again, which
    // is precisely what the decline gate exists to stop.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        declinedActionIds: ['order.status.read'],
      }),
    )
    expect(outcome.actions[0]?.denialReason).toBe('declined_by_visitor')
    expect(outcome.actions[0]?.policy).toBe('denied')
    notEscalated(outcome)
  })

  it('does not escalate an action the page never offered — that is the page’s scope', async () => {
    // The browser was told it may highlight and not read orders. No tier grants
    // a tool the host page withheld, so an operator has nothing to change. The
    // page's offer and the brain's request are deliberately different actions:
    // name the same one on both sides and the gate has nothing left to refuse,
    // which turns the case into a §9 refusal and proves nothing about the page.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('ui.highlight') }]),
        brain: brainFor('order.status.read'),
      }),
    )
    expect(outcome.actions[0]?.denialReason).toBe('action_unavailable_on_page')
    notEscalated(outcome)
  })

  it('does not escalate an invented action id — there is no tier that would make it exist', async () => {
    const outcome = await runTurn(
      request({ graph: graph(), brain: brainFor('booking.cancel_with_refund') }),
    )
    expect(outcome.actions[0]?.denialReason).toBe('unknown_action')
    notEscalated(outcome)
  })

  it('does not escalate a §9 input refusal — the gate permitted it, the payload did not', async () => {
    // Two different verdicts, deliberately: the gate says the client may run this
    // action, and validation says nobody could act on *these inputs*. The second
    // is not an entitlement statement, and it is also the case where
    // `denialReason` is absent entirely — which is how the code tells a policy
    // denial from an input refusal without reading a sentence.
    //
    // No resolver, which is the point and not an omission: `order.status.read`
    // carries an entity id, and with no catalog to confirm it the refusal is
    // §9's own. Supply `CONFIRMING` here and the same action executes, so what
    // is being held apart is the gate's yes from validation's no — not a tier.
    const outcome = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions: enabled('order.status.read') }]),
        brain: brainFor('order.status.read'),
        executor: executor(),
      }),
    )
    const read = outcome.actions.find((action) => action.actionId === 'order.status.read')
    expect(read?.policy).toBe('denied')
    expect(read?.execution).toBe('not_attempted')
    expect(read?.denialReason).toBeUndefined()
    notEscalated(outcome)
  })

  it('does not escalate an L5 action held for a human — the tier was never the problem', async () => {
    // The one remaining denial code, and the reason it is not in the escalation
    // set: `admin_action_requires_human` is about the action's level. This client
    // is on `enterprise`, the top tier, and the action still does not run alone.
    // Calling that `capability_exceeded` would send an operator to raise a tier
    // that is already the highest one there is.
    const outcome = await runTurn(
      request({
        capability: 'enterprise',
        role: 'admin',
        graph: graph([{ type: 'action/set', actions: enabled('admin.config.update') }]),
        brain: brainFor('admin.config.update'),
      }),
    )
    expect(outcome.actions[0]?.denialReason).toBe('admin_action_requires_human')
    notEscalated(outcome)
  })

  it('keeps repeated_failure at three; two failed executions are not a handoff', async () => {
    // The threshold still holds after the escalation set narrowed, and the two
    // halves are asserted together because a narrowed set could easily have
    // started catching records it was never meant to: a *failed execution* is
    // not a denial, so reclassifying it as one would put `capability_exceeded`
    // on a turn where the client was entitled all along and the store was down.
    const broken: ActionExecutor = {
      executorId: 'broken',
      execute: (request): Promise<ActionExecutionResult> =>
        Promise.resolve({
          status: 'failed',
          errorCode: 'store_offline',
          retryable: true,
          message: `Store offline for ${request.action}.`,
        }),
    }
    const actions = enabled('product.read', 'availability.read')

    const two = await runTurn(
      request({
        graph: graph([{ type: 'action/set', actions }]),
        brain: brainFor('product.read', 'availability.read'),
        resolver: CONFIRMING,
        executor: broken,
      }),
    )
    expect(two.actions.map((action) => action.execution)).toEqual(['failed', 'failed'])
    expect(two.actions.every((action) => action.denialReason === undefined)).toBe(true)
    expect(two.handoff).toBeNull()

    const three = await runTurn(
      request({
        graph: graph([
          {
            type: 'action/set',
            actions: enabled('product.read', 'availability.read', 'order.status.read'),
          },
        ]),
        brain: brainFor('product.read', 'availability.read', 'order.status.read'),
        resolver: CONFIRMING,
        executor: broken,
      }),
    )
    expect(three.actions.map((action) => action.execution)).toEqual(['failed', 'failed', 'failed'])
    expect(three.actions.every((action) => action.denialReason === undefined)).toBe(true)
    expect(three.handoff?.reason).toBe('repeated_failure')
  })
})
