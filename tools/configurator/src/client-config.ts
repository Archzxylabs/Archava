import {
  safeParseClientConfig,
  type Branding,
  type ClientConfig,
  type Entity,
  type KnowledgeSource,
  type ModuleToggle,
} from '@archava/config'
import type { NormalizedScope } from './normalized.js'

/**
 * The last step: a normalised scope becomes a `ClientConfig` the platform can
 * load (§14, §23).
 *
 * Two boundaries are deliberate and worth stating, because both look like gaps
 * until you read the reason:
 *
 * 1. **The configurator never invents a brand.** The template catalog carries
 *    no palette, and the intake asks for no colours, so `branding` and
 *    `updatedAt` arrive from the caller. An operator owns how a client looks and
 *    which day this snapshot is stamped; a configurator that made up a hex code
 *    would be writing brand decisions no client ever made. The core also stays
 *    clock-free, which is what keeps two runs on one intake identical.
 * 2. **Only explicitly requested integrations are emitted.** New deployment
 *    consumers use ClientConfigResolver.plan(): common integrations stay
 *    suggestions and host verification supplies connection/health evidence.
 *    resolveLegacy() preserves V1 demo defaults but is not a provisioning input.
 *    A quote and its deployment scope must never silently gain unpriced work.
 */

/** Matches `config/templates.v1.json`; bumped there, changed here, never silently. */
export const CLIENT_CONFIG_SCHEMA_VERSION = '1.0.0'

/** What the configurator cannot derive and will not guess. */
export interface ClientConfigDraft {
  /** Palette, business name, logo. Caller-supplied; see boundary 1 above. */
  readonly branding: Branding
  /** `YYYY-MM-DD`. Caller-supplied so the core never reads a clock. */
  readonly updatedAt: string
  /** Origins allowed to embed the Archava SDK. `[]` means none yet declared. */
  readonly allowedOrigins?: readonly string[]
  /**
   * Template modules to switch off. Modules named here and absent from the
   * template vocabulary are kept as-is — the resolver merges, and an explicit
   * opt-out is the client's to make.
   */
  readonly modules?: readonly ModuleToggle[]
  readonly entities?: readonly Entity[]
  readonly knowledgeSources?: readonly KnowledgeSource[]
  readonly isReferenceImplementation?: boolean
}

export class ClientConfigEmitError extends Error {
  readonly issues: readonly string[]

  constructor(message: string, issues: readonly string[]) {
    super(message)
    this.name = 'ClientConfigEmitError'
    this.issues = issues
  }
}

/**
 * Build the config for one scope, validated by the platform's own schema.
 *
 * Throws rather than falling back: a configurator that quietly repaired an
 * invalid field would hand the runtime a tenant that no longer matches what the
 * client was quoted. `tryEmitClientConfig` is the non-throwing form for
 * callers that report issues instead.
 */
export function emitClientConfig(scope: NormalizedScope, draft: ClientConfigDraft): ClientConfig {
  const parsed = tryEmitClientConfig(scope, draft)
  if (!parsed.success) {
    throw new ClientConfigEmitError(
      'The generated client configuration is not valid.',
      parsed.issues,
    )
  }
  return parsed.data
}

/** The same build, with validation issues returned rather than raised. */
export function tryEmitClientConfig(
  scope: NormalizedScope,
  draft: ClientConfigDraft,
): { success: true; data: ClientConfig } | { success: false; issues: readonly string[] } {
  return safeParseClientConfig({
    schema_version: CLIENT_CONFIG_SCHEMA_VERSION,
    tenantId: scope.tenantId,
    environment: scope.environment,
    presence: scope.presence,
    capability: scope.capability,
    region: scope.region,
    template: scope.template,
    branding: draft.branding,
    languages: scope.languages,
    primaryLanguage: scope.primaryLanguage,
    integrations: scope.integrations,
    support: scope.support,
    designAddons: scope.designAddons,
    modules: draft.modules ?? [],
    entities: draft.entities ?? [],
    knowledgeSources: draft.knowledgeSources ?? [],
    allowedOrigins: draft.allowedOrigins ?? [],
    updatedAt: draft.updatedAt,
    isReferenceImplementation: draft.isReferenceImplementation ?? false,
  })
}
