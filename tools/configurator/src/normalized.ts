import type {
  CapabilityTierName,
  DesignAddonName,
  EnvironmentName,
  IntegrationComplexity,
  PresenceTierName,
  RegionCode,
  SupportTierName,
} from '@archava/config'
import type { ComplianceNeed, ConfiguratorIntake, IndustryName, LanguageNeed } from './intake.js'
import type { ScopeOverride } from './recommend.js'
import type { VariantRequest } from './variants.js'

/**
 * The one canonical scope record a launch is built from (§14, §23).
 *
 * Everything downstream of this point reads the same shape: the client-config
 * emitter does not re-derive anything from the intake, and the report does not
 * re-derive anything from the variant. A scope that goes through here comes out
 * the same for everyone who receives it, which is what makes two configurator
 * runs on one intake comparable rather than merely plausible.
 *
 * The rule that governs every field: **a value absent from the intake stays
 * absent.** The usage forecast is `null` when the client never stated one, not
 * `0` — pricing those two differently is the whole point. The same discipline
 * applies to integrations: only the ones the intake marked required for launch
 * are counted, and the rest are recorded in `provenance.omitted` with the reason
 * they were dropped, so nothing disappears without a trace.
 */

export interface NormalizedScope {
  readonly tenantId: string
  readonly businessName: string
  readonly template: IndustryName
  readonly region: RegionCode
  readonly presence: PresenceTierName
  readonly capability: CapabilityTierName
  readonly environment: EnvironmentName
  readonly support: SupportTierName
  readonly designAddons: readonly DesignAddonName[]
  readonly integrations: readonly NormalizedIntegration[]
  readonly languages: readonly LanguageNeed[]
  readonly primaryLanguage: string
  readonly compliance: readonly ComplianceNeed[]
  readonly annualPrepay: boolean
  /** `null` when the client never stated a forecast. Never a fabricated zero. */
  readonly usageForecast: number | null
  readonly targetLaunchDate: string
  /** How this scope came to be, so a launch can explain itself (§19, §23). */
  readonly provenance: ScopeProvenance
}

/** An integration requested for launch, with its quoted complexity. Not connection evidence. */
export interface NormalizedIntegration {
  readonly name: string
  readonly complexity: IntegrationComplexity
  readonly enabled: true
}

/** Everything that makes this scope more than a list of tiers. */
export interface ScopeProvenance {
  /** The industry template this scope was normalised onto. */
  readonly industry: IndustryName
  readonly transactionMode: ConfiguratorIntake['transactionMode']
  readonly actionsRequired: readonly string[]
  /** Every place the scope differs from what the client asked for. */
  readonly overrides: readonly ScopeOverride[]
  /** Required work this variant declined, with the reason. */
  readonly omitted: readonly { readonly name: string; readonly reason: string }[]
  readonly variant: VariantRequest['variant']
}

/** Freeze an intake plus one variant into the record everything else reads. */
export function normalizeScope(
  intake: ConfiguratorIntake,
  variant: VariantRequest,
): NormalizedScope {
  return {
    tenantId: intake.clientId,
    businessName: intake.clientName,
    template: intake.industry,
    region: intake.region,
    presence: variant.presence,
    capability: variant.capability,
    environment: variant.environment,
    support: variant.support,
    designAddons: [...variant.designAddons],
    integrations: normalizeIntegrations(intake),
    languages: [...intake.languages],
    primaryLanguage: intake.primaryLanguage,
    compliance: [...intake.compliance],
    annualPrepay: variant.annualPrepay,
    usageForecast: variant.usageForecast ?? null,
    targetLaunchDate: intake.targetLaunchDate,
    provenance: {
      industry: intake.industry,
      transactionMode: intake.transactionMode,
      actionsRequired: [...intake.actionsRequired],
      overrides: [...variant.overrides],
      omitted: [...variant.omittedIntegrations],
      variant: variant.variant,
    },
  }
}

/**
 * Only the integrations required for launch survive, and each keeps the
 * complexity it was quoted at — dropping one here without dropping it upstream
 * would leave a quote priced for work that ships nowhere.
 */
function normalizeIntegrations(intake: ConfiguratorIntake): readonly NormalizedIntegration[] {
  return intake.integrations
    .filter((item) => item.required)
    .map((item) => ({ name: item.name, complexity: item.complexity, enabled: true as const }))
}
