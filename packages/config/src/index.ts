export {
  clientConfigSchema,
  safeParseClientConfig,
  parseClientConfig,
  type ClientConfig,
  type Integration,
  type ModuleToggle,
  type Entity,
  type KnowledgeSource,
  type Language,
  type Branding,
  type Presence,
  type Capability,
  type Environment,
  type Region,
  DECISION_TASK_IDS,
  DECISION_MODES,
  decisionTaskPolicySchema,
  decisionTasksSchema,
  decisionConfigSchema,
  type DecisionMode,
  type DecisionTaskId,
  type DecisionTaskPolicy,
  type DecisionTasks,
  type DecisionConfig,
} from './client/schema.js'

export { ClientConfigResolver, ClientConfigError } from './client/resolver.js'
export {
  buildDeploymentPlan,
  integrationEvidenceSchema,
  type DeploymentPlan,
  type TemplateLookup,
  type IntegrationEvidence,
} from './client/plan.js'
export {
  ModuleRegistry,
  PLATFORM_MODULES,
  moduleManifestSchema,
  type ModuleManifest,
} from './client/modules.js'

export {
  pricebookSchema,
  type Pricebook,
  type Region as PricebookRegion,
} from './pricing/schema.js'

export { loadPricebook, parsePricebook, PricebookError } from './pricing/loader.js'

export { templatesSchema, type Template, type TemplateCatalog } from './templates/schema.js'

export { loadTemplates, TemplateRegistry, TemplateError } from './templates/loader.js'

export {
  SUPPORTED_CURRENCIES,
  CURRENCY_MINOR_UNIT_EXPONENTS,
  UnknownCurrencyError,
  isSupportedCurrency,
  minorUnitExponent,
  minorUnitDivisor,
  type CurrencyCode,
} from './currency.js'

export {
  REGIONS,
  PRESENCE_TIERS,
  CAPABILITY_TIERS,
  ENVIRONMENTS,
  INTEGRATION_COMPLEXITIES,
  DESIGN_ADDONS,
  SUPPORT_TIERS,
  type RegionCode,
  type PresenceTierName,
  type CapabilityTierName,
  type EnvironmentName,
  type IntegrationComplexity,
  type DesignAddonName,
  type SupportTierName,
  type PaidCoreComponent,
  classifyLine,
  type PriceLine,
} from './pricing/types.js'
