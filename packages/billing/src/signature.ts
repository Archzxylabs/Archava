/**
 * Provider signatures: how a payment result earns the right to be believed.
 *
 * A payment provider notifies this domain that money moved. That notification
 * arrives over HTTP, in a body a caller controls, from a source that is not
 * authenticated by anything this domain can see. Treating it as an instruction
 * would mean any client that could reach the endpoint could mark an invoice
 * settled. So the notification is not trusted — only its signature is.
 *
 * The digest is `HMAC(key, domain ‖ length-framed parts)` over the canonical
 * rendering of exactly the fields the provider says moved: the tenant, the
 * invoice, the provider's own event id, the outcome, the amount in minor units
 * and the currency. Three properties are load-bearing here, and each has a test:
 *
 * 1. **A digest covers one claim.** The domain separates this from every other
 *    HMAC this platform computes. A digest minted under another domain — an
 *    act-confirmation token, say — verifies against nothing here, so a value
 *    that is meaningful elsewhere cannot be replayed as a settlement.
 *
 * 2. **Framing makes the parts unambiguous.** Each part is length-prefixed
 *    before concatenation, so `['ab','c']` and `['a','bc']` digest differently.
 *    Without it a provider could move a character between fields and re-point a
 *    signature at a different invoice without invalidating it.
 *
 * 3. **A comparison cannot be observed.** `signaturesEqual` hashes both sides
 *    before comparing, so the bytes compared are the same length whatever
 *    arrives, and a mismatch cannot be timed into a guess about which prefix was
 *    right. This is the one comparison in the domain against a value an
 *    unauthenticated caller supplies.
 *
 * The key is a deployment secret, at least 256 bits, read per call rather than
 * cached: rotating it invalidates outstanding notifications, which is the correct
 * failure — a stale key must stop settling invoices, not keep settling them.
 */

import { createHmac, timingSafeEqual } from 'node:crypto'

/** The shortest key a provider digest may be computed under. */
export const SIGNATURE_KEY_MIN_BYTES = 32

/**
 * The one namespace in this domain.
 *
 * Nothing outside billing computes a digest under this tag, and nothing inside
 * billing computes one under any other, so a signature is only ever a statement
 * about a payment event.
 */
const PROVIDER_EVENT_DOMAIN = 'archava/billing/provider-event/v1'

/**
 * A server-side key a provider notification is verified under.
 *
 * Bytes, not a string, so that a deployment cannot accidentally verify against
 * the text of a secret rather than the secret itself.
 */
export type SignatureKey = Uint8Array

export function isSignatureKey(value: unknown): value is SignatureKey {
  return (
    value instanceof Uint8Array &&
    value.byteLength >= SIGNATURE_KEY_MIN_BYTES &&
    Object.prototype.toString.call(value) === '[object Uint8Array]'
  )
}

/** Exactly the fields a provider signs, in the order they are framed. */
export interface PaymentEventClaim {
  tenantId: string
  invoiceId: string
  providerId: string
  providerEventId: string
  outcome: string
  amountMinor: number
  currency: string
}

/**
 * A ceiling on what one claim will frame.
 *
 * Claims reach this code already validated, so this is not a size check on
 * well-formed data — it is a ceiling that pays for itself the first time a
 * malformed payload arrives with a field wired to something unbounded.
 */
const MAX_FRAMED_BYTES = 1_048_576

function lengthHeader(size: number): Uint8Array {
  const header = new Uint8Array(4)
  new DataView(header.buffer).setUint32(0, size, false)
  return header
}

function concat(chunks: readonly Uint8Array[], size: number): Uint8Array {
  const out = new Uint8Array(size)
  let offset = 0
  for (const chunk of chunks) {
    out.set(chunk, offset)
    offset += chunk.byteLength
  }
  return out
}

/**
 * The claim as a sequence of parts, each length-framed before concatenation.
 *
 * The amount is rendered as an exact integer string rather than through JSON so
 * that no formatting step — a trailing `.0`, a locale separator — can produce two
 * renderings of the same money, or one rendering of two different amounts.
 */
function claimParts(claim: PaymentEventClaim): string[] {
  return [
    claim.tenantId,
    claim.invoiceId,
    claim.providerId,
    claim.providerEventId,
    claim.outcome,
    String(claim.amountMinor),
    claim.currency,
  ]
}

export function providerEventDigest(key: SignatureKey, claim: PaymentEventClaim): string {
  if (!isSignatureKey(key)) throw new BillingSignatureError('key')
  const parts = claimParts(claim)
  const domain = new TextEncoder().encode(PROVIDER_EVENT_DOMAIN)
  const framed: Uint8Array[] = [domain, lengthHeader(domain.byteLength)]
  let total = domain.byteLength + 4
  for (const part of parts) {
    const bytes = new TextEncoder().encode(part)
    framed.push(bytes, lengthHeader(bytes.byteLength))
    total += bytes.byteLength + 4
  }
  if (total > MAX_FRAMED_BYTES) throw new BillingSignatureError('size')
  const digest = createHmac('sha256', key).update(concat(framed, total)).digest('hex')
  if (digest.length !== 64) throw new BillingSignatureError('format')
  return digest
}

export type BillingSignatureErrorCode = 'key' | 'format' | 'size'

export class BillingSignatureError extends Error {
  constructor(readonly code: BillingSignatureErrorCode) {
    super('Provider signature could not be computed.')
    this.name = 'BillingSignatureError'
  }
}

/**
 * Compare two signatures without leaking where they differ.
 *
 * Both sides are re-hashed under a fixed digest before the comparison, so the
 * `timingSafeEqual` call always sees two 32-byte values regardless of what
 * arrived. A caller who sends a short or malformed signature therefore cannot
 * distinguish "wrong length" from "right length, wrong value" by timing, and
 * cannot walk the correct signature one character at a time.
 */
export function signaturesEqual(left: unknown, right: unknown): boolean {
  const expected = digestForComparison(left)
  const actual = digestForComparison(right)
  return timingSafeEqual(expected, actual)
}

function digestForComparison(value: unknown): Uint8Array {
  const text = typeof value === 'string' ? value : ''
  return new Uint8Array(createHmac('sha256', 'archava/billing/compare/v1').update(text).digest())
}
