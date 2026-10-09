/**
 * Action input validation (PRD §18, V1.1).
 *
 * PRD §18 requires action inputs to be validated. This module is what closes
 * the gap between "the model named a field" and "the executor received a value
 * it can act on".
 *
 * Three properties define it, and each is a bug that was previously possible:
 *
 * 1. **It fails closed.** A missing field, a wrong type, or an unknown field is
 *    a refusal, never a best guess. A booking request with no slot is a booking
 *    request that did not happen, and saying so is better than filling the gap.
 *
 * 2. **Entity existence is verified against an authoritative resolver, never
 *    against a schema.** This is the trap §9 names: a zod schema describes the
 *    *shape* of an input, and `z.string().uuid()` will happily accept an id for
 *    a slot that does not exist. Whether a slot exists is a question only the
 *    catalog can answer, so it is asked — through {@link EntityResolver} — or the
 *    action is refused. A placeholder, a fixture, or a registry example passed in
 *    as though it were a catalog answer is the failure mode this prevents. "No
 *    resolver was supplied" is a refusal, not permission: an unverified
 *    `slotId` is exactly what §9 exists to stop, and a missing port is not
 *    evidence that the answer would have been yes.
 *
 * 3. **Nothing is inferred from a registered action's example.** `requiredInputs`
 *    on an `ActionDefinition` is the model-facing contract — the list of names a
 *    model may be asked to fill. It is not a value, and no value is ever
 *    substituted for one the visitor did not supply.
 */

import { z } from 'zod'

/**
 * The port through which "does this thing exist" is answered.
 *
 * Implementable against a catalog, an availability system, or a product service.
 * Everything about it is tenant-scoped by the caller's responsibility to pass the
 * right `tenantId`: an id that exists for one tenant must resolve to nothing for
 * another, so an implementation that ignores the argument is broken in the §23
 * sense and not merely incomplete.
 *
 * Promise-returning. "Does this slot exist" is a question for a system, not for
 * this process, and a port typed to allow a bare `boolean` invites an
 * implementation that answers from whatever happens to be in memory — which is
 * the fixture-as-catalog failure §9 exists to catch. A local implementation
 * returns `Promise.resolve(answer)`.
 */
export interface EntityResolver {
  resolveExists(tenantId: string, entityKind: string, entityId: unknown): Promise<boolean>
}

/**
 * What a well-formed entity kind name looks like.
 *
 * Lowercase first letter, then letters, digits, underscores and dots. The first
 * letter is constrained because a kind is a *name* in a message someone reads,
 * and `9` or `_` leading one would be a typo; the rest is deliberately permissive
 * about case, because `pageEntity` is a kind this module declares — see the L1 UI
 * actions below. A pattern that excluded camelCase did not make these kinds
 * invalid, it made them invisible: `fieldDeclaresEntity` returned false, so no
 * missing-resolver refusal, and `collectEntityRejections` returned `[]`, so the id
 * was never asked about at all. The web app's `pageEntityResolver` existed to
 * answer exactly those questions and was never called.
 */
const ENTITY_KIND_PATTERN = /^[a-z][a-zA-Z0-9_.]*$/

interface InputField {
  readonly kind: 'string' | 'number' | 'boolean' | 'object' | 'array'
  /** Set when the field holds an id that must be verified by a resolver. */
  readonly entity?: string
  /** Present for `object` fields: the fields the object itself must carry. */
  readonly fields?: Readonly<Record<string, InputField>>
  /**
   * Present for `array` fields: what every element must be.
   *
   * Without it an array field accepts `['anything', 7, null]` — the shape
   * "a list" and nothing more — which is not a contract an executor can rely
   * on. `ui.compare` needs the two things being compared, and "two things" is
   * only true of an array when every element is the kind of thing being named.
   */
  readonly items?: InputField
  /** True when the field may be omitted. Optional never means unvalidated. */
  readonly optional?: boolean
  /**
   * What the field means, for the refusal message. A model shown "field `x` is
   * missing" cannot act on it; one shown "field `x` (the booking slot) is
   * missing" can ask the right follow-up question.
   */
  readonly description: string
}

/**
 * The union zod actually parses, derived from one {@link InputField}.
 *
 * Deriving rather than hand-writing the zod shape means there is exactly one
 * source of truth for a field's type. A second, hand-written schema would be a
 * place for the two to disagree, and the disagreement would be invisible until
 * an action executed with the wrong value in it.
 */
function fieldSchema(field: InputField): z.ZodTypeAny {
  const schema = innerSchema(field)
  return field.optional === true ? schema.optional() : schema
}

function innerSchema(field: InputField): z.ZodTypeAny {
  switch (field.kind) {
    case 'string':
      return z.string()
    case 'number':
      return z.number()
    case 'boolean':
      return z.boolean()
    case 'object':
      return objectSchema(field)
    case 'array': {
      const items = field.items
      // An array with no stated element shape is refused, not treated as
      // `z.array(z.unknown())` — a permissive schema is not a contract, and
      // `ui.compare`'s "the two entities being compared" is only true of an
      // array when every element is the kind of thing being named.
      if (items === undefined) return z.never()
      return z.array(innerSchema(items))
    }
  }
}

/**
 * The zod schema for an object field.
 *
 * With `fields` declared, it is a zod object over exactly those — and nothing else,
 * because a second source of truth for a field's type is a place for the two to
 * disagree. Without them, it is *not* `z.object({})`: zod strips keys an object
 * schema does not name, so `admin.config.update`'s `changes` and
 * `form.submit`'s `fields` — both declared as "an object" with no interior — would
 * arrive at the executor as `{}` whatever the visitor submitted. The action would
 * be allowed and every value in it gone, which is a silent data loss on a path
 * nothing else checks. `z.record(z.unknown())` keeps the payload intact instead.
 */
function objectSchema(field: InputField): z.ZodTypeAny {
  const fields = field.fields
  if (fields === undefined || Object.keys(fields).length === 0)
    return z.record(z.string(), z.unknown())
  return z.object(objectShape(fields))
}

function objectShape(fields: Readonly<Record<string, InputField>>): z.ZodRawShape {
  // Built as a plain record rather than assigned into zod's shape directly: zod 4
  // resolves `ZodRawShape` with a readonly index signature, so `shape[name] = …`
  // is a write to a read-only position. Casting once here keeps a single source of
  // truth for a field's type — `fieldSchema` — instead of a second hand-written
  // schema that could disagree with it.
  const shape: Record<string, z.ZodTypeAny> = {}
  for (const [name, field] of Object.entries(fields)) {
    shape[name] = fieldSchema(field)
  }
  return shape
}

/** One refusal: the field, and what was wrong with it. */
export interface InputRejection {
  readonly actionId: string
  readonly field: string
  readonly reason: string
}

/** Why an action's inputs were refused, or `null` when they were accepted. */
export type ActionInputValidation =
  | { readonly ok: true; readonly inputs: Readonly<Record<string, unknown>> }
  | { readonly ok: false; readonly rejections: readonly InputRejection[] }

/**
 * The registered inputs each action may carry.
 *
 * This is the contract between a model and an executor. It is deliberately
 * narrow — present in this scope — and it is *not* derived from the registry's
 * `requiredInputs` example list, which describes names for the model, not types
 * for the executor. Keeping the two apart is what stops "the registry said the
 * field is called `customer`" from becoming "therefore a booking may be created
 * without a verified customer".
 */
const INPUT_SCHEMAS: Readonly<Record<string, Readonly<Record<string, InputField>>>> = {
  'product.read': {
    productId: { kind: 'string', entity: 'product', description: 'the product' },
  },
  'availability.read': {
    subjectId: { kind: 'string', entity: 'unit', description: 'the unit or slot' },
  },
  'order.status.read': {
    orderReference: { kind: 'string', entity: 'order', description: 'the order' },
  },
  'cart.item.add': {
    productId: { kind: 'string', entity: 'product', description: 'the product' },
    quantity: { kind: 'number', description: 'how many to add' },
  },
  'cart.item.remove': {
    productId: { kind: 'string', entity: 'product', description: 'the product' },
  },
  'booking.create': {
    slotId: { kind: 'string', entity: 'slot', description: 'the booking slot' },
    customer: {
      kind: 'object',
      description: 'who the booking is for',
      fields: {
        customerRef: { kind: 'string', entity: 'customer', description: 'the customer' },
        notes: { kind: 'string', optional: true, description: 'notes for the operator' },
      },
    },
  },
  'admin.config.update': {
    changes: { kind: 'object', description: 'the configuration changes to apply' },
  },
  'email.send': {
    templateId: { kind: 'string', entity: 'template', description: 'the template' },
    to: { kind: 'string', description: 'the recipient address' },
  },
  'crm.followup.create': {
    contactId: { kind: 'string', entity: 'contact', description: 'the authorized CRM contact' },
    contactRevision: { kind: 'number', description: 'the reviewed contact revision' },
    summary: { kind: 'string', description: 'the reviewed follow-up text' },
    dueAt: { kind: 'string', description: 'the reviewed due date' },
  },
  'crm.followup.complete': {
    followupId: { kind: 'string', entity: 'followup', description: 'the authorized CRM task' },
    expectedRevision: { kind: 'number', description: 'the reviewed task revision' },
  },
  'form.submit': {
    formId: { kind: 'string', entity: 'form', description: 'the form' },
    fields: { kind: 'object', description: 'the submitted values' },
  },
  // The three L1 UI actions. They are the only actions a page can run, and the
  // only ones whose inputs name something the *page* is showing rather than
  // something the tenant sells — which is why each asks for an id the page
  // awareness pass put in the graph (`page.entities`), verified against that
  // graph's resolver rather than against the tenant's catalog.
  'navigation.go': {
    path: { kind: 'string', description: 'the path to navigate to' },
  },
  'ui.highlight': {
    entityId: { kind: 'string', entity: 'pageEntity', description: 'the entity to highlight' },
  },
  'ui.compare': {
    entityIds: {
      kind: 'array',
      description: 'the entities being compared',
      items: {
        kind: 'string',
        entity: 'pageEntity',
        description: 'an entity being compared',
      },
    },
  },
}

/**
 * Fields an action must never receive from a model in the first place.
 *
 * A tenant id in an input is a §23 accident waiting to happen: an executor that
 * trusts it has been handed a cross-tenant write. The tenant is a property of the
 * turn (see `ActionExecutionRequest`), never of the payload.
 */
const FORBIDDEN_FIELDS: readonly string[] = ['tenantId', 'tenant_id']

/**
 * Whether a field asks for an id that has to be resolved, at any depth.
 *
 * Recursive for the reason the walk in {@link collectEntityRejections} is: a field
 * three levels down still names a real thing, and a check that stopped at the top
 * would let `booking.create`'s `customer.customerRef` — an entity-bearing field
 * nobody at the top level can see — run unresolved. An entity *kind* that does not
 * match {@link ENTITY_KIND_PATTERN} is not treated as a declaration, because the
 * walk below skips one too and the two must agree.
 */
function fieldDeclaresEntity(field: InputField): boolean {
  if (field.entity !== undefined && ENTITY_KIND_PATTERN.test(field.entity)) return true
  if (field.items !== undefined && fieldDeclaresEntity(field.items)) return true
  if (field.fields !== undefined) {
    for (const child of Object.values(field.fields)) {
      if (fieldDeclaresEntity(child)) return true
    }
  }
  return false
}

/**
 * Whether a contract, as a whole, asks for an id that has to be resolved.
 *
 * A `true` here is what makes a missing resolver a refusal rather than a shrug.
 */
function contractDeclaresEntity(contract: Readonly<Record<string, InputField>>): boolean {
  return Object.values(contract).some(fieldDeclaresEntity)
}

/**
 * Validate one action's inputs against its registered contract.
 *
 * Returns the parsed inputs, or the reasons it was refused. It does not throw:
 * a refusal is an ordinary turn outcome, reported on the action, not a crash.
 */
export async function validateActionInputs(request: {
  readonly actionId: string
  readonly inputs: Readonly<Record<string, unknown>>
  readonly tenantId: string
  readonly resolver?: EntityResolver
}): Promise<ActionInputValidation> {
  const contract = INPUT_SCHEMAS[request.actionId]

  // An action with no registered input contract accepts nothing from a model.
  // This is the fail-closed default: a new action is inert until someone writes
  // its contract, rather than accepting whatever a model happens to send.
  if (contract === undefined) {
    return {
      ok: false,
      rejections: [
        {
          actionId: request.actionId,
          field: '*',
          reason: `No input contract is registered for "${request.actionId}", so nothing may be submitted for it.`,
        },
      ],
    }
  }

  const rejections: InputRejection[] = []

  for (const forbidden of FORBIDDEN_FIELDS) {
    if (Object.hasOwn(request.inputs, forbidden)) {
      rejections.push({
        actionId: request.actionId,
        field: forbidden,
        reason: `A tenant is a property of the turn, not of an input; "${forbidden}" is never accepted here.`,
      })
    }
  }

  const parsed: Record<string, unknown> = {}

  for (const [name, field] of Object.entries(contract)) {
    if (!Object.hasOwn(request.inputs, name)) {
      rejections.push({
        actionId: request.actionId,
        field: name,
        reason: `Missing required input "${name}" (${field.description}).`,
      })
      continue
    }
    const result = fieldSchema(field).safeParse(request.inputs[name])
    if (!result.success) {
      rejections.push({
        actionId: request.actionId,
        field: name,
        reason: `Input "${name}" must be a ${field.kind} (${field.description}).`,
      })
      continue
    }
    parsed[name] = result.data
  }

  // Unknown fields are refused rather than dropped. Dropping would hide a model
  // that reached for a field the contract does not have — which is exactly the
  // signal an operator needs to see when a prompt regresses.
  for (const name of Object.keys(request.inputs)) {
    if (!Object.hasOwn(contract, name) && !FORBIDDEN_FIELDS.includes(name)) {
      rejections.push({
        actionId: request.actionId,
        field: name,
        reason: `Unknown input "${name}"; this action does not accept it.`,
      })
    }
  }

  if (rejections.length > 0) {
    return { ok: false, rejections }
  }

  // Shape is not existence. A contract with an entity-bearing field somewhere in
  // it — however deeply nested — declares that some id in the input must be
  // answered for by an authoritative resolver, and without one there is nothing
  // to ask. Returning `ok` here is what fail-open looked like: every shape checked
  // out, so the action ran with an unverified `slotId` and a booking was created
  // against a slot that may not exist.
  //
  // A contract with no entity-bearing field is different, and is allowed through.
  // `admin.config.update` and `navigation.go` name no outside thing, so there is
  // nothing to resolve and refusing them would be a rule that never had a reason.
  if (request.resolver === undefined) {
    if (contractDeclaresEntity(contract)) {
      return {
        ok: false,
        rejections: [
          {
            actionId: request.actionId,
            field: '*',
            reason: `"${request.actionId}" takes an id that must be verified against an authoritative entity resolver, and none was supplied, so nothing may be submitted for it.`,
          },
        ],
      }
    }
    return { ok: true, inputs: parsed }
  }

  const existence = await verifyEntities(request, contract, parsed)
  if (existence.length > 0) {
    return { ok: false, rejections: existence }
  }
  return { ok: true, inputs: parsed }
}

/**
 * Ask the resolver whether each entity-bearing id exists for this tenant.
 *
 * Recurses into nested objects, because an id a model may hallucinate is just as
 * dangerous one level down: `booking.create`'s `customer.customerRef` is a
 * reference to a real customer, and refusing the parent field while leaving the
 * child unchecked would let a booking be created against a customer that does
 * not exist. The path in a rejection is dotted so the message names the field
 * the model actually filled (`customer.customerRef`, not `customer`).
 *
 * A resolver that throws is treated as "not verified" for the field, because an
 * unreachable catalog is not permission to guess. The values are still shaped
 * correctly; what is missing is the authoritative answer, and that is a refusal
 * rather than an execution.
 */
async function verifyEntities(
  request: {
    readonly actionId: string
    readonly tenantId: string
    readonly resolver?: EntityResolver
  },
  contract: Readonly<Record<string, InputField>>,
  parsed: Readonly<Record<string, unknown>>,
): Promise<readonly InputRejection[]> {
  const rejections: InputRejection[] = []

  for (const [name, field] of Object.entries(contract)) {
    rejections.push(...(await collectEntityRejections(request, name, field, parsed[name])))
  }

  return rejections
}

async function collectEntityRejections(
  request: {
    readonly actionId: string
    readonly tenantId: string
    readonly resolver?: EntityResolver
  },
  path: string,
  field: InputField,
  value: unknown,
): Promise<readonly InputRejection[]> {
  if (field.kind === 'array') {
    if (field.items === undefined) return []
    // An absent optional array has nothing to verify, and an array that
    // validated is guaranteed to be one — so a non-array here is a schema
    // problem the caller already reported, not a second one to duplicate.
    if (!Array.isArray(value)) return []

    const nested: InputRejection[] = []
    for (const [index, element] of value.entries()) {
      nested.push(
        ...(await collectEntityRejections(request, `${path}[${index}]`, field.items, element)),
      )
    }
    return nested
  }

  if (field.kind === 'object') {
    if (field.fields === undefined) return []
    // An absent optional object has nothing to verify, and an object that
    // validated is guaranteed to be a plain record.
    if (typeof value !== 'object' || value === null) return []

    const nested: InputRejection[] = []
    const elements = value as Readonly<Record<string, unknown>>
    for (const [name, child] of Object.entries(field.fields)) {
      if (!Object.hasOwn(elements, name)) continue
      nested.push(
        ...(await collectEntityRejections(request, `${path}.${name}`, child, elements[name])),
      )
    }
    return nested
  }

  if (field.entity === undefined) return []
  if (!ENTITY_KIND_PATTERN.test(field.entity)) return []

  // No resolver present.
  //
  // `validateActionInputs` refuses an entity-bearing contract before it gets here,
  // so a caller reaching this line is one that reached it via a path it did not
  // anticipate — most easily an *optional* nested object the input never supplied,
  // where the walk still runs over a declared field nobody filled. Returning `[]`
  // ("nothing to verify here") is what a port nobody asked looks like, and a
  // refusal is the only honest answer, so the same refusal is produced here rather
  // than trusting every caller to have filtered first.
  const resolver = request.resolver
  if (resolver === undefined) {
    return [
      {
        actionId: request.actionId,
        field: path,
        reason: `"${path}" names an id that must be verified against an authoritative entity resolver, and none was supplied.`,
      },
    ]
  }

  let exists: boolean
  try {
    exists = await resolver.resolveExists(request.tenantId, field.entity, value)
  } catch {
    exists = false
  }
  if (exists) return []

  return [
    {
      actionId: request.actionId,
      field: path,
      reason: `No ${field.entity} matching "${String(value)}" exists for this tenant (${field.description}).`,
    },
  ]
}

/** Whether a registered action declares an input contract. */
export function hasInputContract(actionId: string): boolean {
  return Object.hasOwn(INPUT_SCHEMAS, actionId)
}

/**
 * The refusal message a turn surfaces for an unvalidatable action.
 *
 * One sentence, no tenant data, safe to show a visitor. Two properties:
 *
 * 1. It names the field, using the dotted path from the rejection, so the model
 *    sees `customer.customerRef` and not just `customer`. That path is the whole
 *    reason {@link verifyEntities} builds one, and a message that dropped it
 *    would throw the information away.
 * 2. Only the first character is decapitalised, not the whole sentence. Ids are
 *    case-sensitive — `ORD-1` lowercased to `ord-1` is a different-looking string
 *    than the one the visitor typed — and the grammar does not require it.
 */
export function inputRejectionMessage(rejections: readonly InputRejection[]): string {
  const first = rejections[0]
  if (first === undefined) return ''
  if (first.field === '*') return first.reason

  const sentence = first.reason.charAt(0).toLowerCase() + first.reason.slice(1)
  return `Could not complete that: input "${first.field}" — ${sentence}`
}
