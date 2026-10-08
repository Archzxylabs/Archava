import { z } from 'zod'
import {
  DESIGN_ADDONS,
  ENVIRONMENTS,
  INTEGRATION_COMPLEXITIES,
  PRESENCE_TIERS,
  REGIONS,
  SUPPORT_TIERS,
} from '@archava/config'

/**
 * PRD §14.1: the configurator's inputs, one per onboarding question.
 *
 * This is the boundary where a human's answers become the platform's own
 * vocabulary, so every field is validated here rather than defaulted later. An
 * answer that is not on the list is refused — a configurator that silently
 * mapped "sort of a booking site" to an environment tier would be guessing at
 * the one input that decides the price.
 *
 * Nothing in this file prices anything. The money lives in the pricebook
 * (`config/pricing.v1.json`) behind `PricingEngine`; the values here are the
 * scope questions, and they are deliberately all enumerations or counts.
 */

const nonEmptyString = z.string().min(1)
const isoDate = z.string().regex(/^\d{4}-\d{2}-\d{2}$/, 'must be a YYYY-MM-DD date')

/**
 * Keeps the first occurrence of each answer.
 *
 * A repeated answer is not a second one. `customDesign` and `integrations` both
 * reach the pricing engine, which charges for every entry it is handed, so an
 * intake that names the same design addon twice — or lists the same system
 * twice — would be quoted for the work twice. Deduplicating here lets every
 * consumer downstream treat these lists as sets.
 */
function distinctBy<T>(values: readonly T[], keyOf: (value: T) => string): T[] {
  const seen = new Set<string>()
  return values.filter((value) => {
    const key = keyOf(value)
    if (seen.has(key)) return false
    seen.add(key)
    return true
  })
}

function distinctStrings<T extends string>(values: readonly T[]): T[] {
  return distinctBy(values, (value) => value)
}

/** The template catalog's own names, so "industry" and "template" cannot drift. */
export const INDUSTRIES = [
  'generic_business',
  'hospitality',
  'restaurant',
  'clinic',
  'real_estate',
  'ecommerce',
  'professional_services',
  'education',
] as const
export type IndustryName = (typeof INDUSTRIES)[number]

/** What the client actually sells through the assistant. */
export const TRANSACTION_MODES = [
  'none',
  'payments_only',
  'booking',
  'checkout_orders',
  'mixed',
] as const
export type TransactionMode = (typeof TRANSACTION_MODES)[number]

/** The state of whatever website they already have. */
export const WEBSITE_STATUSES = ['none', 'live', 'stale', 'under_construction'] as const
export type WebsiteStatus = (typeof WEBSITE_STATUSES)[number]

/** Compliance obligations that change the review path, never the price. */
export const COMPLIANCE_NEEDS = ['none', 'pdp', 'gdpr', 'pci_dss', 'hipaa', 'iso27001'] as const
export type ComplianceNeed = (typeof COMPLIANCE_NEEDS)[number]

export const languageSchema = z.object({
  code: z.string().regex(/^[a-z]{2}$/, 'must be a two-letter ISO 639-1 code'),
  label: nonEmptyString,
})

export const integrationNeedSchema = z.object({
  /** The system's name, as the client calls it. Never a credential. */
  name: nonEmptyString,
  complexity: z.enum(INTEGRATION_COMPLEXITIES),
  /** True when the integration is required for launch, not a nice-to-have. */
  required: z.boolean().default(true),
})

/**
 * The integrations, each named once.
 *
 * Unlike the other lists, a repeated name here is refused rather than
 * deduplicated: two rows both called "Google Calendar" may disagree about
 * complexity and whether the build needs them at all, and silently keeping one
 * would let the price hinge on which row happened to come first.
 */
export const integrationListSchema = z
  .array(integrationNeedSchema)
  .default([])
  .refine(
    (items) => distinctBy(items, (item) => item.name).length === items.length,
    'lists the same integration twice — give each system a distinct name',
  )

export const brandAssetSchema = z.object({
  logo: z.boolean(),
  styleGuide: z.boolean(),
  photography: z.boolean(),
})

export const budgetSchema = z.object({
  /**
   * What the client said they can spend on the one-time build. Optional on
   * purpose (PRD §14.1: "known budget, optional"): absent means no budget
   * verdict is produced, which is different from a budget of zero.
   */
  amount: z.number().nonnegative(),
  currency: z.enum(['USD', 'IDR']),
})

export const intakeSchema = z
  .object({
    /** The tenant slug. Kebab-case, because it becomes the tenant id. */
    clientId: z.string().regex(/^[a-z0-9]+(?:-[a-z0-9]+)*$/, 'must be a lowercase kebab-case slug'),
    clientName: nonEmptyString,
    industry: z.enum(INDUSTRIES),
    region: z.enum(REGIONS),
    existingWebsite: z.object({
      url: z.string().url().optional(),
      status: z.enum(WEBSITE_STATUSES),
    }),
    businessGoals: z.array(nonEmptyString).min(1).transform(distinctStrings),
    /** The client's stated preference. May be overridden, but never silently. */
    presence: z.enum(PRESENCE_TIERS),
    /** Action ids the client needs (§18 vocabulary: `booking.create`, …). */
    actionsRequired: z.array(nonEmptyString).default([]).transform(distinctStrings),
    transactionMode: z.enum(TRANSACTION_MODES),
    expectedConversationsPerMonth: z.number().int().nonnegative().default(0),
    languages: z
      .array(languageSchema)
      .min(1)
      .transform((entries) => distinctBy(entries, (entry) => entry.code)),
    primaryLanguage: nonEmptyString,
    integrations: integrationListSchema,
    brandAssets: brandAssetSchema,
    compliance: z.array(z.enum(COMPLIANCE_NEEDS)).default(['none']).transform(distinctStrings),
    /** Design work beyond the template. Each maps to a design addon. */
    customDesign: z.array(z.enum(DESIGN_ADDONS)).default([]).transform(distinctStrings),
    support: z.enum(SUPPORT_TIERS).default('standard'),
    annualPrepay: z.boolean().default(false),
    targetLaunchDate: isoDate,
    budget: budgetSchema.optional(),
  })
  .strict()

export type ConfiguratorIntake = z.infer<typeof intakeSchema>
export type IntegrationNeed = z.infer<typeof integrationNeedSchema>
export type LanguageNeed = z.infer<typeof languageSchema>
export type BudgetNeed = z.infer<typeof budgetSchema>

export class IntakeError extends Error {
  readonly issues: readonly string[]

  constructor(message: string, issues: readonly string[]) {
    super(message)
    this.name = 'IntakeError'
    this.issues = issues
  }
}

/**
 * Parse onboarding answers into the configurator's own shape.
 *
 * Fails closed with every issue at once: an operator fixing an intake fixes
 * all of it, not one field per round-trip.
 */
export function parseIntake(raw: unknown): ConfiguratorIntake {
  const result = intakeSchema.safeParse(raw)
  if (result.success) return result.data
  throw new IntakeError('The intake could not be read.', issuesOf(result.error))
}

export function safeParseIntake(
  raw: unknown,
): { success: true; data: ConfiguratorIntake } | { success: false; issues: readonly string[] } {
  const result = intakeSchema.safeParse(raw)
  if (result.success) return { success: true, data: result.data }
  return { success: false, issues: issuesOf(result.error) }
}

function issuesOf(error: z.ZodError): readonly string[] {
  return error.issues.map((issue) => `${issue.path.join('.') || '(root)'}: ${issue.message}`)
}

/** Environments the configurator is allowed to choose between. */
export const CONFIGURATOR_ENVIRONMENTS = ENVIRONMENTS
