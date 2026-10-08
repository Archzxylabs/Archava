import { minorUnitExponent } from '@archava/config/pure'

/**
 * Money, formatted for a visitor without restating what a live system said.
 *
 * A shell that draws an `order_summary` holds exactly one amount, paired by
 * schema with the currency it is in (§25's `amountMinor` + `currency` pair
 * exists so a number can never appear bare). That pairing is the whole problem:
 * "1250000" is meaningless, "Rp 1.438.000" is not — but which of those it is
 * depends on the currency's *minor unit*, not on the field name.
 *
 * IDR is the interesting case. Its minor unit is 1, so the reference tenant's
 * `amountMinor: 1_438_000` (`packages/reference/src/rates.ts`) is 1,438,000
 * whole rupiah, and a formatter that assumed two decimal places would print
 * `IDR 14,380` — off by a factor of a hundred, in the direction that under-quotes
 * a guest.
 *
 * So the divisor comes from `minorUnitExponent`, an explicit platform contract,
 * and never from `Intl`. That is the one thing this module refuses to do, and it
 * is worth saying in terms of the failure rather than the preference: the divisor
 * used to be read from
 * `new Intl.NumberFormat(locale, { style: 'currency', currency }).formatToParts(1)`,
 * which makes the value of a *stored* price depend on the ICU dataset of the
 * machine printing it. CI and the laptop that wrote the code disagreed — IDR
 * gained two fraction digits on the runner and lost them locally — and the same
 * `amountMinor` meant two different prices. A stored amount is a fact about the
 * tenant's data; a locale's preference for symbol placement and digit grouping is
 * not, and letting the second decide the first is how a price under-quotes by a
 * hundred on one operating system and not another.
 *
 * `Intl` is still here, and still doing the half it is actually authoritative
 * for: taking the already-scaled major-unit number and presenting it in the
 * tenant's language (`Rp` grouping, `$` placement). Presentation may vary by ICU
 * dataset; the number being presented may not — and that includes how many
 * fraction digits it has, because `Rp 1.438.000,00` is not a different rendering
 * of the same price, it is a different price with the same digits in it. So the
 * digit count comes from the exponent alongside the divisor, and `Intl` keeps
 * only the choices a visitor's language genuinely makes for them.
 */

/** Raised when an amount reaches the formatter that cannot be money at all. */
export class MoneyFormatError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'MoneyFormatError'
  }
}

export { minorUnitExponent }

/**
 * How many decimal places `currency` uses, as the platform declares it.
 *
 * Re-exported from `@archava/config` so a shell that needs the divisor has one
 * import rather than two definitions. It lost its `locale` parameter, and that is
 * the change rather than a simplification: whether a currency has a minor unit is
 * not a question a locale can answer, and an argument that can only be ignored is
 * an argument that was always lying.
 */
export function currencyFractionDigits(currency: string): number {
  return minorUnitExponent(currency)
}

/**
 * Minor units as the string a visitor reads: `Rp 1.438.000`, `$143.80`.
 *
 * `locale` is the tenant's language, not the browser's — a resort in Yogyakarta
 * should show `Rp` to an Indonesian visitor regardless of where the browser is
 * set. An amount that is not a finite number throws rather than rendering as a
 * dash: a decided turn carrying a non-numeric total is a defect in the producer,
 * and a dash in the total would hide it behind a plausible-looking empty cell. A
 * currency the platform declares no exponent for throws by the same standard —
 * an amount whose scale is unknown is not an amount, and printing it anyway is
 * the failure this module exists to prevent.
 */
export function formatMoney(amountMinor: number, currency: string, locale: string): string {
  if (!Number.isFinite(amountMinor)) {
    throw new MoneyFormatError(`amount must be a finite number, received ${String(amountMinor)}`)
  }
  const digits = minorUnitExponent(currency)
  const major = amountMinor / 10 ** digits
  const formatted = new Intl.NumberFormat(locale, {
    style: 'currency',
    currency,
    // The exponent decides the scale, so it decides the fraction digit count as
    // well. That is one rule applied twice, not a second rule: the divisor and
    // the rendered decimals are the same contract, and a formatter that asked
    // `Intl` for the second was putting a different authority in charge than the
    // one it had already trusted for the first. The GitHub runner's ICU dataset
    // reports two fraction digits for IDR, so the unpinned call printed
    // `Rp 1.438.000,00` there and `Rp 1.438.000` on the laptop that wrote it —
    // one stored price, two numbers on screen, decided by the host.
    //
    // What stays `Intl`'s is presentation and only presentation: `Rp` versus the
    // ISO code, dot versus comma grouping, non-breaking versus ordinary space.
    // Those vary by design. The fraction digit count may not, and pinning both
    // ends of the range is what makes "zero decimals for IDR" mean zero.
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  }).format(major)
  // ICU joins a currency symbol to its amount with a non-breaking space, so the
  // string a shell renders and the string a caller compares against differ by an
  // invisible character. Normalising to a plain space keeps the separator out of
  // the page's text content and out of every assertion that reads it.
  //
  // The two characters are written as escapes rather than typed literally: both
  // are irregular whitespace, one of them is invisible in an editor, and a literal
  // inside the class is a character no reviewer can see in a diff. Behaviour is
  // identical to spelling them out.
  return formatted.replace(/[\u00a0\u202f]/g, ' ')
}
