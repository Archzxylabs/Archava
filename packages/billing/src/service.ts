import {
  BillingError,
  billingTenantSchema,
  billingPrincipalSchema,
  billingMutationSchema,
  invoicePageQuerySchema,
  SETTLEMENT_OUTCOMES,
  type BillingRepository,
  type BillingPrincipal,
  type BillingMutation,
  type BillingReceipt,
  type BillingAccess,
  type BillingCommandStatus,
  type InvoicePageQuery,
  type Invoice,
  type InvoiceDetail,
  type InvoiceLine,
  type InvoiceCorrection,
  type PaymentAttempt,
  type PaymentEvent,
} from './contracts.js'
import { isSignatureKey, providerEventDigest, signaturesEqual } from './signature.js'

/**
 * Resolves the secret a provider's notification is verified under.
 *
 * Per tenant and provider rather than global: two tenants settling through the
 * same processor must not share a secret, or either one's configuration could
 * mint events the other would honour. Returning `null` means "no secret is
 * configured for this pair", which the service treats as a refusal — an
 * unconfigured provider settles nothing, rather than everything.
 */
export type SignatureKeySource = (
  tenant: string,
  providerId: string,
) => Promise<SignatureKey | null> | SignatureKey | null

type SignatureKey = Uint8Array

export class BillingService {
  constructor(
    private readonly repository: BillingRepository,
    private readonly signatureKeys: SignatureKeySource,
  ) {}

  /**
   * Every mutation a person can make is refused for a service principal.
   *
   * An automated agent may read, and may be handed a signed provider event, but
   * it cannot issue an invoice, adjust one or attempt a payment on someone's
   * behalf. `record_provider_event` is the single exception, and it is not an
   * exception to authorisation — it is authorised by a signature instead of by a
   * person, which is the whole point of the boundary below.
   */
  private assertHumanOperator(principal: BillingPrincipal, mutation: BillingMutation) {
    if (mutation.type === 'record_provider_event') return
    if (principal.kind !== 'human') {
      throw new BillingError(
        'forbidden',
        'Automated service agents cannot record billing mutations directly',
      )
    }
  }

  /**
   * Verify a provider event's signature against the exact claim it asserts.
   *
   * The claim is rebuilt from the received fields, not from the request body as
   * a whole, so an attacker cannot settle an invoice by adding an unrecognised
   * field or by reordering the ones that matter. A digest that does not match —
   * wrong key, tampered amount, different invoice, forged outcome, or a value
   * minted under another domain entirely — is refused with one code, so nothing
   * about *why* it failed reaches an unauthenticated caller.
   */
  private async verifyProviderEvent(
    tenant: string,
    mutation: Extract<BillingMutation, { type: 'record_provider_event' }>,
  ) {
    const key = await this.signatureKeys(tenant, mutation.providerId)
    if (!isSignatureKey(key))
      throw new BillingError(
        'invalid_signature',
        'No provider signing secret is configured for this provider',
      )
    const expected = providerEventDigest(key, {
      tenantId: tenant,
      invoiceId: mutation.invoiceId,
      providerId: mutation.providerId,
      providerEventId: mutation.providerEventId,
      outcome: mutation.outcome,
      amountMinor: mutation.amountMinor,
      currency: mutation.currency,
    })
    if (!signaturesEqual(expected, mutation.signature))
      throw new BillingError('invalid_signature', 'Provider signature did not verify')
  }

  async mutate(
    tenant: string,
    principal: BillingPrincipal,
    mutation: BillingMutation,
  ): Promise<BillingReceipt> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    const validMutation = billingMutationSchema.parse(mutation)
    this.assertHumanOperator(validPrincipal, validMutation)
    if (validMutation.type === 'record_provider_event')
      await this.verifyProviderEvent(validTenant, validMutation)
    return this.repository.execute(validTenant, validPrincipal, validMutation)
  }

  async execute(
    tenant: string,
    principal: BillingPrincipal,
    mutation: BillingMutation,
  ): Promise<BillingReceipt> {
    return this.mutate(tenant, principal, mutation)
  }

  async getCommandStatus(
    tenant: string,
    principal: BillingPrincipal,
    commandId: string,
  ): Promise<BillingCommandStatus> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    return this.repository.getCommandStatus(validTenant, validPrincipal, commandId)
  }

  async getAccess(tenant: string, principal: BillingPrincipal): Promise<BillingAccess> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    return this.repository.getAccess(validTenant, validPrincipal)
  }

  async listInvoices(
    tenant: string,
    principal: BillingPrincipal,
    query: Partial<InvoicePageQuery> = {},
  ): Promise<readonly Invoice[]> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    const parsedQuery = invoicePageQuerySchema.parse(query)
    return this.repository.listInvoices(validTenant, validPrincipal, parsedQuery)
  }

  async detail(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<InvoiceDetail> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    const detail = await this.repository.getInvoiceDetail(validTenant, validPrincipal, invoiceId)
    if (!detail) throw new BillingError('not_found', `Invoice ${invoiceId} not found`)
    return detail
  }

  async lines(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly InvoiceLine[]> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    return this.repository.listLines(validTenant, validPrincipal, invoiceId)
  }

  async corrections(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly InvoiceCorrection[]> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    return this.repository.listCorrections(validTenant, validPrincipal, invoiceId)
  }

  async attempts(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly PaymentAttempt[]> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    return this.repository.listAttempts(validTenant, validPrincipal, invoiceId)
  }

  /**
   * Every settled payment this invoice has ever seen, newest first.
   *
   * The list is the settlement's own evidence: what is owed is derived from
   * these appended events and nothing else, so there is no counter an operator
   * or a caller could move without an event behind it.
   */
  async events(
    tenant: string,
    principal: BillingPrincipal,
    invoiceId: string,
  ): Promise<readonly PaymentEvent[]> {
    const validTenant = billingTenantSchema.parse(tenant)
    const validPrincipal = billingPrincipalSchema.parse(principal)
    return this.repository.listEvents(validTenant, validPrincipal, invoiceId)
  }
}

/**
 * What a signed settlement is worth against an invoice.
 *
 * `failed` moves nothing — it is history. `succeeded` adds to what has been
 * settled and `refunded` takes it back, so a refund is not a special case
 * bolted onto the side of the ledger but the same arithmetic running the other
 * way, over the same append-only events.
 */
export function settledFromEvents(events: readonly { outcome: string; amountMinor: number }[]) {
  return events.reduce(
    (total, event) =>
      SETTLEMENT_OUTCOMES.includes(event.outcome as (typeof SETTLEMENT_OUTCOMES)[number]) &&
      event.outcome === 'succeeded'
        ? total + event.amountMinor
        : event.outcome === 'refunded'
          ? total - event.amountMinor
          : total,
    0,
  )
}

/** What is still owed: the frozen total, adjusted by corrections, less settlement. */
export function outstandingFrom(invoice: {
  grossMinor: number
  correctionMinor: number
  settledMinor: number
}) {
  return invoice.grossMinor + invoice.correctionMinor - invoice.settledMinor
}
