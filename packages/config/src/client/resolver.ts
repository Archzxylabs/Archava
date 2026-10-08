import { TemplateRegistry } from '../templates/loader.js'
import type { ClientConfig, Entity, Integration, ModuleToggle } from './schema.js'
import { buildDeploymentPlan, type DeploymentPlan, type IntegrationEvidence } from './plan.js'
import { ModuleRegistry } from './modules.js'

export class ClientConfigError extends Error {
  constructor(message: string) {
    super(message)
    this.name = 'ClientConfigError'
  }
}

/**
 * Resolves a client configuration against the template catalog.
 *
 * Order of precedence is fixed and one-directional:
 *   client config  >  template defaults  >  no default at all
 *
 * A template can never override an explicit client decision, and an unknown
 * template is an error rather than a silent fallback.
 */
export class ClientConfigResolver {
  constructor(private readonly registry: TemplateRegistry = new TemplateRegistry()) {}

  /** @deprecated V1 defaults are planning suggestions, not deployment readiness.
   * Retained for existing callers. New consumers must use plan(). */
  resolve(config: ClientConfig): ClientConfig {
    return this.resolveLegacy(config)
  }

  /** Explicit compatibility path. Do not provision from its enabled flags. */
  resolveLegacy(config: ClientConfig): ClientConfig {
    const template = this.registry.tryGet(config.template)
    if (!template) {
      throw new ClientConfigError(
        `Template "${config.template}" is not in template catalog version ` +
          `${this.registry.schemaVersion}. Available: ${this.registry.listNames().join(', ')}.`,
      )
    }

    return {
      ...config,
      // Template supplies the *default* environment when the client has not
      // expressed one; because `environment` is required in the config schema,
      // a mismatch here is surfaced instead of silently adopted.
      modules: this.mergeModules(config.modules, template.modules),
      integrations: this.mergeIntegrations(config.integrations, template.common_integrations),
    }
  }

  plan(
    config: ClientConfig,
    modules: ModuleRegistry = new ModuleRegistry(),
    hostEvidence: readonly IntegrationEvidence[] = [],
  ): DeploymentPlan {
    return buildDeploymentPlan(config, this.registry, modules, hostEvidence)
  }

  /** Effective module set: template modules filtered by client toggles, plus client-only modules. */
  mergeModules(
    clientModules: readonly ModuleToggle[],
    templateModules: readonly string[],
  ): ModuleToggle[] {
    const clientByName = new Map(clientModules.map((module) => [module.name, module]))
    const merged: ModuleToggle[] = templateModules.map((name) => {
      const override = clientByName.get(name)
      if (!override) return { name, enabled: true }
      return override
    })
    for (const module of clientModules) {
      if (!templateModules.includes(module.name)) merged.push(module)
    }
    return merged
  }

  /** Template common integrations are Standard unless the client declares otherwise. */
  mergeIntegrations(
    clientIntegrations: readonly Integration[],
    templateIntegrations: readonly string[],
  ): Integration[] {
    const clientByName = new Map(
      clientIntegrations.map((integration) => [integration.name, integration]),
    )
    const merged: Integration[] = templateIntegrations
      .filter((name) => !clientByName.has(name))
      .map((name) => ({ name, complexity: 'standard' as const, enabled: true }))
    return [...merged, ...clientIntegrations]
  }

  /** Modules that are both in the template vocabulary and enabled by the client. */
  enabledModules(config: ClientConfig): string[] {
    return config.modules.filter((module) => module.enabled).map((module) => module.name)
  }

  enabledIntegrations(config: ClientConfig): Integration[] {
    return config.integrations.filter((integration) => integration.enabled)
  }

  enabledEntities(config: ClientConfig): Entity[] {
    return config.entities.filter((entity) => entity.visible)
  }
}
