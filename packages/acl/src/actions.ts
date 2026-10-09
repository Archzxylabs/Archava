/**
 * Versioned action registry.
 *
 * PRD §18: every external action must be registered with capability name,
 * tenant, description, required inputs, risk level, confirmation mode, allowed
 * roles, idempotency behaviour, audit behaviour, and a failure/rollback
 * strategy.
 *
 * The registry is versioned so entries can be retired without breaking the
 * client contract: a deprecated action still resolves and still executes, it
 * just reports `deprecatedIn` + `replacedBy` so callers can migrate. New
 * actions never reuse an existing id.
 */
import type { CapabilityTierName } from '@archava/config'
import type { RoleName } from './roles.js'

/** Risk levels from PRD §18. Capability tier does not override risk rules. */
export const ACTION_LEVELS = ['L0', 'L1', 'L2', 'L3', 'L4', 'L5'] as const
export type ActionLevel = (typeof ACTION_LEVELS)[number]

export const ACTION_LEVEL_LABELS: Record<ActionLevel, string> = {
  L0: 'Read — product/availability/order status',
  L1: 'UI — navigate, highlight, filter, compare',
  L2: 'Reversible — fill draft, add/remove cart item',
  L3: 'External state — send form, create booking, send email, CRM write',
  L4: 'Money/account — checkout, payment, cancellation, refund, shipping/account changes',
  L5: 'Admin — never autonomous by default',
}

/** How an action must be gated before it executes. */
export type ConfirmationMode = 'none' | 'user_confirm' | 'human_approval'

export interface ActionDefinition {
  /** Stable, namespaced id. Never reused, never renamed. */
  readonly id: string
  readonly domain: string
  readonly level: ActionLevel
  /** Minimum capability tier required. */
  readonly requiresCapability: CapabilityTierName
  readonly confirmation: ConfirmationMode
  readonly allowedRoles: readonly RoleName[]
  /**
   * Repeating the call with the same key must not repeat the side effect.
   *
   * A promise the executor keeps, not one the pipeline can keep for it: the
   * idempotency key travels as far as `ActionExecutionRequest` and no further,
   * because the pipeline has no store to remember the keys it has already
   * handed out. Every baseline action declares `true`, so every one of them
   * leans on an executor that actually de-duplicates.
   */
  readonly idempotent: boolean
  readonly audited: boolean
  /** Fields that must be masked before any model/provider exposure. */
  readonly sensitiveFields: readonly string[]
  readonly description: string
  /**
   * Input field names the model is told to supply.
   *
   * Names, not a contract. Validation keeps its own {@link INPUT_SCHEMAS} in
   * `packages/assistant/src/validation.ts` and deliberately does not derive from
   * this list: these strings describe what a prompt may name, while that map
   * decides what an executor may accept. Collapsing the two would turn "the
   * registry says the field is called `customer`" into "therefore a booking may
   * be created without a verified customer".
   */
  readonly requiredInputs: readonly string[]
  /** Set when the action is retired; the id keeps working. */
  readonly deprecatedIn?: string
  readonly replacedBy?: string
}

export const ACTION_REGISTRY_VERSION = '1.1.0'

const ASSISTANT_ONLY: readonly RoleName[] = ['archava_assistant']

/**
 * Baseline registry. Only actions a deployed client actually enables should be
 * exposed to the model — the policy gate, not this list, is what restricts a
 * specific client.
 */
export const BASELINE_ACTIONS: readonly ActionDefinition[] = [
  // ---- L0 read ----------------------------------------------------------
  {
    id: 'product.read',
    domain: 'catalog',
    level: 'L0',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: false,
    sensitiveFields: [],
    description: 'Read product or service detail from the live authoritative source.',
    requiredInputs: ['productId'],
  },
  {
    id: 'availability.read',
    domain: 'availability',
    level: 'L0',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: false,
    sensitiveFields: [],
    description: 'Read real availability or stock for a unit, slot, or product.',
    requiredInputs: ['subjectId'],
  },
  {
    id: 'order.status.read',
    domain: 'order',
    level: 'L0',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['customerEmail', 'customerPhone'],
    description: 'Read order or booking status for an identified customer order.',
    requiredInputs: ['orderReference'],
  },
  {
    id: 'payment.status.read',
    domain: 'payment',
    level: 'L0',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['customerEmail'],
    description: 'Read payment status from the payment provider.',
    requiredInputs: ['paymentReference'],
  },

  // ---- L1 UI ------------------------------------------------------------
  {
    id: 'navigation.go',
    domain: 'navigation',
    level: 'L1',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: false,
    sensitiveFields: [],
    description: 'Navigate the visitor to a page or route.',
    requiredInputs: ['path'],
  },
  {
    id: 'ui.highlight',
    domain: 'navigation',
    level: 'L1',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: false,
    sensitiveFields: [],
    description: 'Highlight or scroll to a section or entity on the current page.',
    requiredInputs: ['entityId'],
  },
  {
    id: 'ui.compare',
    domain: 'catalog',
    level: 'L1',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: false,
    sensitiveFields: [],
    description: 'Open the comparison view for two or more entities.',
    requiredInputs: ['entityIds'],
  },

  // ---- L2 reversible ----------------------------------------------------
  {
    id: 'lead.capture',
    domain: 'lead',
    level: 'L2',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['email', 'phone', 'message'],
    description:
      'Store a captured lead in the Archava lead store. Writing it to an external CRM requires Act.',
    requiredInputs: ['contact'],
  },
  {
    id: 'form.fill_draft',
    domain: 'form',
    level: 'L2',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: false,
    sensitiveFields: [],
    description: 'Pre-fill a host-page form for the visitor to review and submit.',
    requiredInputs: ['formId', 'fields'],
  },
  {
    id: 'handoff.request',
    domain: 'support',
    level: 'L2',
    requiresCapability: 'assist',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['contact'],
    description: 'Escalate to a human agent queue. The human decides; Archava does not.',
    requiredInputs: ['reason'],
  },
  {
    id: 'cart.item.add',
    domain: 'cart',
    level: 'L2',
    requiresCapability: 'transact',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: [],
    description: 'Add an item to the cart. Reversible.',
    requiredInputs: ['productId', 'quantity'],
  },
  {
    id: 'cart.item.remove',
    domain: 'cart',
    level: 'L2',
    requiresCapability: 'transact',
    confirmation: 'none',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: [],
    description: 'Remove an item from the cart. Reversible.',
    requiredInputs: ['productId'],
  },

  // ---- L3 external state ------------------------------------------------
  {
    id: 'form.submit',
    domain: 'form',
    level: 'L3',
    requiresCapability: 'act',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['fields'],
    description: 'Submit a form to an external system.',
    requiredInputs: ['formId', 'fields'],
  },
  {
    id: 'crm.lead.create',
    domain: 'crm',
    level: 'L3',
    requiresCapability: 'act',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['email', 'phone', 'notes'],
    description: 'Write or update a lead record in an external CRM.',
    requiredInputs: ['contact', 'source'],
  },
  {
    id: 'crm.followup.create',
    domain: 'crm',
    level: 'L2',
    requiresCapability: 'act',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['summary'],
    description: 'Create an owner-approved CRM follow-up task.',
    requiredInputs: ['contactId', 'contactRevision', 'summary', 'dueAt'],
  },
  {
    id: 'email.send',
    domain: 'email',
    level: 'L3',
    requiresCapability: 'act',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['to', 'body'],
    description: 'Send a transactional email as part of a workflow.',
    requiredInputs: ['templateId', 'to'],
  },
  {
    id: 'booking.create',
    domain: 'booking',
    level: 'L3',
    requiresCapability: 'act',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['customerPhone', 'customerEmail', 'notes'],
    description: 'Create an appointment or reservation in the booking system.',
    requiredInputs: ['slotId', 'customer'],
  },
  {
    id: 'booking.reschedule',
    domain: 'booking',
    level: 'L3',
    requiresCapability: 'act',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['customerPhone'],
    description: 'Reschedule a non-financial appointment where policy allows.',
    requiredInputs: ['bookingId', 'slotId'],
  },

  // ---- L4 money / account ----------------------------------------------
  {
    id: 'checkout.start',
    domain: 'checkout',
    level: 'L4',
    requiresCapability: 'transact',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: [],
    description: 'Start the checkout flow from the current cart.',
    requiredInputs: [],
  },
  {
    id: 'payment.initiate',
    domain: 'payment',
    level: 'L4',
    requiresCapability: 'transact',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['cardNumber', 'cvv', 'expiry', 'cardholderName', 'authorization', 'otp'],
    description:
      'Initiate payment. Archava never accepts a client-supplied price as authoritative.',
    requiredInputs: ['paymentMethodId'],
  },
  {
    id: 'order.confirm',
    domain: 'order',
    level: 'L4',
    requiresCapability: 'transact',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: [],
    description: 'Confirm a paid order or booking using the backend identifier.',
    requiredInputs: ['orderId'],
  },
  {
    id: 'order.cancel_refund',
    domain: 'order',
    level: 'L4',
    requiresCapability: 'transact',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['customerEmail'],
    description: 'Cancel or refund an order tied to a financial transaction.',
    requiredInputs: ['orderId', 'reason'],
  },
  {
    id: 'account.update',
    domain: 'account',
    level: 'L4',
    requiresCapability: 'transact',
    confirmation: 'user_confirm',
    allowedRoles: ASSISTANT_ONLY,
    idempotent: true,
    audited: true,
    sensitiveFields: ['email', 'phone', 'address'],
    description: 'Change shipping details or account data for an authenticated customer.',
    requiredInputs: ['changes'],
  },

  // ---- L5 admin ---------------------------------------------------------
  {
    id: 'admin.config.update',
    domain: 'admin',
    level: 'L5',
    requiresCapability: 'enterprise',
    confirmation: 'human_approval',
    allowedRoles: ['owner', 'admin'],
    idempotent: true,
    audited: true,
    sensitiveFields: [],
    description: 'Change tenant solution configuration. Never autonomous.',
    requiredInputs: ['changes'],
  },
  {
    id: 'admin.knowledge.reindex',
    domain: 'admin',
    level: 'L5',
    requiresCapability: 'enterprise',
    confirmation: 'human_approval',
    allowedRoles: ['owner', 'admin'],
    idempotent: true,
    audited: true,
    sensitiveFields: [],
    description: 'Trigger a tenant knowledge reindex. Never autonomous.',
    requiredInputs: [],
  },
]

const ACTIONS_BY_ID = new Map<string, ActionDefinition>(
  BASELINE_ACTIONS.map((action) => [action.id, action]),
)

export class ActionRegistryError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'ActionRegistryError'
  }
}

/**
 * A read-only, versioned view over the registered actions.
 *
 * Additive-only by contract: entries may be deprecated (and still resolve), but
 * an existing id is never removed or re-pointed, so a stored client contract
 * from an older version keeps executing.
 */
export class ActionRegistry {
  readonly version = ACTION_REGISTRY_VERSION

  list(): readonly ActionDefinition[] {
    return BASELINE_ACTIONS
  }

  has(id: string): boolean {
    return ACTIONS_BY_ID.has(id)
  }

  /** Throws when the id is unknown. Never returns `undefined`. */
  get(id: string): ActionDefinition {
    const action = ACTIONS_BY_ID.get(id)
    if (!action) {
      throw new ActionRegistryError(
        `Unknown action "${id}". Register it before use; do not invent an action id.`,
      )
    }
    return action
  }

  tryGet(id: string): ActionDefinition | undefined {
    return ACTIONS_BY_ID.get(id)
  }

  byDomain(domain: string): readonly ActionDefinition[] {
    return BASELINE_ACTIONS.filter((action) => action.domain === domain)
  }

  /** Actions that are retired but still executable, with their replacement. */
  deprecations(): readonly { id: string; deprecatedIn: string; replacedBy?: string }[] {
    return BASELINE_ACTIONS.filter(
      (action): action is ActionDefinition & { deprecatedIn: string } =>
        action.deprecatedIn !== undefined,
    ).map((action) => ({
      id: action.id,
      deprecatedIn: action.deprecatedIn,
      ...(action.replacedBy === undefined ? {} : { replacedBy: action.replacedBy }),
    }))
  }
}
