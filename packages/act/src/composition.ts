/**
 * The production composition seam.
 *
 * Everything else in this package dispatches; this file is where a deployment
 * states, once, which tenant's gateways, stores and keys an executor may use —
 * and where an incomplete statement is refused instead of half-honoured.
 *
 * What it deliberately does **not** do: authorize. It does not read a policy, a
 * confirmation, a DecisionProvider score or a capability. The path is still
 *
 *   ActionPolicy → user confirmation → input validation → entity resolution
 *     → ActionExecutor
 *
 * and this factory sits below all four, wiring only the last one. An executor
 * built here tells the truth about a side effect or refuses to claim one; it
 * cannot grant permission, because permission was never asked of it.
 *
 * The one guarantee that is new: **the executor is bound to its tenant.** Every
 * request reaching it is checked against the tenant it was composed for, and a
 * request carrying another tenant's id is refused with no port call at all. The
 * port calls are tenant-scoped downstream too, so this is a second line rather
 * than the only one — but it is the line that catches a misrouted request before
 * it can be served.
 *
 * The wrappers are not optional. `BookingReplayBoundary` and
 * `EmailOutboxBoundary` are constructed here and their outputs are the only
 * gateways the executors are given; a deployment cannot hand this factory a raw
 * gateway and get a working executor, because a raw gateway is an argument to a
 * boundary, not to an executor.
 */

import type {
  ActionExecutionRequest,
  ActionExecutionResult,
  ActionExecutor,
} from '@archava/assistant'
import { BookingActionExecutor, BookingReplayBoundary } from '@archava/act-booking'
import type {
  BookingAttemptStore,
  BookingGateway,
  BookingReconciliationPort,
} from '@archava/act-booking'
import { EmailActionExecutor, EmailOutboxBoundary } from '@archava/act-email'
import type {
  EmailAttemptStore,
  EmailDeliveryStatusPort,
  EmailGateway,
  EmailTemplateResolver,
} from '@archava/act-email'
import { ActActionExecutor } from './dispatcher.js'
import {
  isOwnedTemplate,
  readBookingAttemptStore,
  readBookingGateway,
  readBookingReconciler,
  readEmailAttemptStore,
  readEmailGateway,
  readEmailStatusPort,
  readEmailTemplateResolver,
  readFingerprintKey,
  readTenantId,
} from './guards.js'

/** Every port one tenant's Act execution needs, stated up front. */
export interface TenantActConfiguration {
  /** The tenant this composition belongs to. Nothing else may use it. */
  readonly tenantId: string
  /** The authoritative reservation system, and the seams around it. */
  readonly booking: {
    readonly gateway: BookingGateway
    /** Durable, atomic on `claim`. See the port's own contract. */
    readonly store: BookingAttemptStore
    /** Read-only: what became of an attempt nobody has an answer for. */
    readonly reconciler: BookingReconciliationPort
  }
  /** The transactional mail path, and the outbox around it. */
  readonly email: {
    readonly gateway: EmailGateway
    /** Durable, atomic on `claim`. Same contract as the booking store. */
    readonly store: EmailAttemptStore
    /** Read-only: what the provider already decided. */
    readonly status: EmailDeliveryStatusPort
    /** Server-side templates. Never model text, never browser input. */
    readonly templates: EmailTemplateResolver
  }
  /** Server-only, at least 32 bytes each, stable while records live. */
  readonly fingerprintKeys: {
    readonly booking: Uint8Array
    readonly email: Uint8Array
  }
}

/** A composed executor, and what it may be asked about itself. */
export interface TenantActExecutor extends ActionExecutor {
  /** Recovery of an existing attempt only: no provider send/reservation and no claim. */
  recover(request: ActionExecutionRequest): Promise<ActionExecutionResult>
  /** The tenant this executor is bound to. */
  readonly tenantId: string
  /**
   * Whether this executor may act *as its own business* — that is, whether the
   * id is one of the two it dispatches and the request addresses its tenant.
   * This is not a policy decision and never has been.
   */
  handles(action: string, tenantId: string): boolean
  /** Which wrapped seams this executor was built from. Not a secret. */
  readonly seam: {
    readonly booking: string
    readonly email: string
  }
}

/** The minimum key length both boundaries require. Secure key generation is a deployment duty. */
const FINGERPRINT_MIN_BYTES = 32

/**
 * Compose the tenant-scoped Act executor.
 *
 * Called once per tenant per process, at startup, from server configuration.
 * Refuses — with {@link ActCompositionRefusal} and a machine reason — when the
 * configuration is incomplete, malformed, or missing a port entirely. It never
 * returns a partially wired executor, because a partially wired executor is one
 * that can be talked into reporting a side effect it cannot make.
 *
 * The returned executor refuses, without touching any port, a request whose
 * tenant is not the tenant it was composed for.
 */
export function composeTenantActExecutor(configuration: TenantActConfiguration): TenantActExecutor {
  // The tenant is read first because every later check is scoped by it.
  const tenantId = readConfigurationTenant(configuration)
  const bookingKey = readFingerprintKey(
    configuration.fingerprintKeys?.booking,
    FINGERPRINT_MIN_BYTES,
  )
  const emailKey = readFingerprintKey(configuration.fingerprintKeys?.email, FINGERPRINT_MIN_BYTES)

  // The wrappers are built first, and their outputs are what the executors are
  // handed. There is no path through this function that gives an executor an
  // unwrapped gateway, which is the whole point of constructing them here
  // rather than leaving the wiring to each deployment.
  const bookingReplay = new BookingReplayBoundary({
    gateway: readBookingGateway(configuration.booking?.gateway),
    store: readBookingAttemptStore(configuration.booking?.store),
    reconciler: readBookingReconciler(configuration.booking?.reconciler),
    fingerprintKey: bookingKey,
  })
  const emailOutbox = new EmailOutboxBoundary({
    gateway: readEmailGateway(configuration.email?.gateway),
    store: readEmailAttemptStore(configuration.email?.store),
    status: readEmailStatusPort(configuration.email?.status),
    fingerprintKey: emailKey,
  })

  // The template resolver is bounded to this tenant before the executor ever
  // sees it. A resolver that answered for another tenant would let one tenant's
  // template id pick up another's sender, and the only place that can be said
  // "no" without editing the resolver is here.
  const templates = readEmailTemplateResolver(configuration.email?.templates)
  const tenantBoundTemplates: EmailTemplateResolver = {
    async resolve(resolvedTenantId, templateId) {
      if (resolvedTenantId !== tenantId) return null
      const template = await templates.resolve(resolvedTenantId, templateId)
      return isOwnedTemplate(template, tenantId) ? template : null
    },
  }

  const booking = new BookingActionExecutor({ gateway: bookingReplay })
  const email = new EmailActionExecutor({
    templates: tenantBoundTemplates,
    gateway: emailOutbox,
  })
  const dispatch = new ActActionExecutor(booking, email)
  const recovery = new ActActionExecutor(
    new BookingActionExecutor({
      gateway: {
        gatewayId: bookingReplay.gatewayId + '-read-only',
        health: { ready: true, reason: 'recovery reads only' },
        reserve: (request) => bookingReplay.recover(request),
      },
    }),
    new EmailActionExecutor({
      templates: tenantBoundTemplates,
      gateway: {
        send: (request) => emailOutbox.recover(request),
      },
    }),
  )

  return {
    executorId: `tenant-act@1(${tenantId})`,
    tenantId,
    seam: { booking: bookingReplay.gatewayId, email: emailOutbox.outboxId },
    async recover(request) {
      if (request.tenantId !== tenantId)
        return {
          status: 'failed',
          errorCode: 'act_executor_tenant_mismatch',
          retryable: false,
          message: 'This executor is not configured for the requesting tenant.',
        }
      return recovery.execute(request)
    },
    handles(action, requestTenantId) {
      return (
        requestTenantId === tenantId && (action === 'booking.create' || action === 'email.send')
      )
    },
    async execute(request: ActionExecutionRequest): Promise<ActionExecutionResult> {
      // This refusal is the composition's own, made before any port is reached.
      // It is deliberately not a policy: it does not deny an action, it
      // declines a request addressed to someone else's deployment.
      if (request.tenantId !== tenantId) {
        return {
          status: 'failed',
          errorCode: 'act_executor_tenant_mismatch',
          retryable: false,
          message: 'This executor is not configured for the requesting tenant.',
        }
      }
      if (request.action !== 'booking.create' && request.action !== 'email.send') {
        return {
          status: 'failed',
          errorCode: 'act_action_not_dispatched',
          retryable: false,
          message: 'This Act executor cannot run the requested action.',
        }
      }
      return await dispatch.execute(request)
    },
  }
}

/**
 * Read the tenant, refusing a configuration that does not state one.
 *
 * Kept as its own step so the reason is about the tenant and not about a port:
 * a missing tenant is the one case where the rest of the configuration cannot
 * be interpreted at all, and reporting "booking incomplete" for it would send
 * an operator looking in the wrong place.
 */
function readConfigurationTenant(configuration: TenantActConfiguration): string {
  return readTenantId(configuration.tenantId)
}
