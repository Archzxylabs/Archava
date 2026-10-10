/**
 * `@archava/configurator` — one entry point per consumer.
 *
 * The surface is deliberately flat. A caller wants one of three things: to turn
 * onboarding answers into intake (`parseIntake`), to price the variants those
 * answers imply (`buildConfigSet`), or to write the chosen one out as a tenant
 * config (`emitClientConfig`). Everything else is exported so a caller that
 * needs a stage on its own — a recommendation without a pricebook, a budget
 * verdict against a quote it already has — can take it without reaching past
 * the package.
 */

// Intake: the boundary where a human's answers become the platform's vocabulary.
export {
  parseIntake,
  safeParseIntake,
  IntakeError,
  intakeSchema,
  INDUSTRIES,
  TRANSACTION_MODES,
  WEBSITE_STATUSES,
  COMPLIANCE_NEEDS,
  CONFIGURATOR_ENVIRONMENTS,
  type ConfiguratorIntake,
  type IndustryName,
  type TransactionMode,
  type WebsiteStatus,
  type ComplianceNeed,
  type IntegrationNeed,
  type LanguageNeed,
  type BudgetNeed,
} from './intake.js'

// Recommendation: answers in, a scope out, with every override explained.
export { recommendScope, type RecommendedScope, type ScopeOverride } from './recommend.js'

// Variants: the same scope at three levels of optional work (§14.1).
export {
  recommendedVariant,
  budgetVariant,
  premiumVariant,
  buildVariants,
  type VariantRequest,
  type VariantName,
} from './variants.js'

// The pricebook bridge: a variant in the pricebook's own request shape.
export { quoteRequestFor } from './quote.js'

// §19: does what they asked for fit what they said they can spend.
export {
  assessBudget,
  type BudgetAssessment,
  type BudgetVerdict,
  type BudgetBasis,
} from './budget.js'

// The one canonical scope record a config set and a client config share.
export {
  normalizeScope,
  type NormalizedScope,
  type NormalizedIntegration,
  type ScopeProvenance,
} from './normalized.js'

// Emission: a normalized scope as a ClientConfig the platform can load.
export {
  emitClientConfig,
  tryEmitClientConfig,
  ClientConfigEmitError,
  CLIENT_CONFIG_SCHEMA_VERSION,
  type ClientConfigDraft,
} from './client-config.js'

// Orchestration: intake in, priced variants and a selection out.
export { buildConfigSet, type ConfigSet, type PricedVariant } from './build.js'

// Host-owned business scope planning; no runtime implementation or price authority.
export { KitPlanner, listKits, getKit, type KitPlan, type KitRequest } from '@archava/kits'
