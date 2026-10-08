import { z } from 'zod'
import type { Template } from '../templates/schema.js'
import { ModuleRegistry } from './modules.js'
import { parseClientConfig, type ClientConfig, type Integration } from './schema.js'

/** Passed by a trusted host after checking its adapter, credentials and health.
 * Never deserialize this from a visitor/client configuration or quote request.
 * This snapshot is not authorization or a permanent health guarantee. */
export const integrationEvidenceSchema = z
  .object({
    tenantId: z.string().min(1),
    name: z.string().min(1),
    supported: z.boolean(),
    configured: z.boolean(),
    connected: z.boolean(),
    healthy: z.boolean(),
    checkedAt: z.iso.datetime(),
  })
  .strict()
  .refine(
    (item) =>
      (!item.configured || item.supported) &&
      (!item.connected || item.configured) &&
      (!item.healthy || item.connected),
    'healthy requires connected, connected requires configured, configured requires supported',
  )

export type IntegrationEvidence = z.infer<typeof integrationEvidenceSchema>

/** Host-supplied catalog lookup; can be memory-backed without filesystem imports. */
export interface TemplateLookup {
  get(name: string): Template
}

export interface DeploymentPlan {
  readonly schema_version: '2.0.0'
  readonly tenantId: string
  /** Exactly the declared scope; template suggestions are kept separately. */
  readonly config: ClientConfig
  readonly suggestions: {
    readonly modules: readonly string[]
    readonly integrations: readonly string[]
  }
  readonly modules: readonly {
    readonly name: string
    readonly version: string
    readonly requested: boolean
    readonly supported: boolean
  }[]
  readonly integrations: readonly (Integration & {
    readonly requested: boolean
    readonly supported: boolean
    readonly configured: boolean
    readonly connected: boolean
    readonly healthy: boolean
    readonly checkedAt?: string
  })[]
  readonly blockers: readonly string[]
  /** Planning readiness only; never permission to execute an action. */
  readonly ready: boolean
}

function requireUnique(names: readonly string[], kind: string): void {
  if (new Set(names).size !== names.length) throw new Error(`Duplicate ${kind} in deployment scope`)
}

/** V2 planning does not inherit enabled integrations or modules from templates.
 * Existing V1 configs may be supplied directly: their explicit requests survive,
 * while implicit legacy defaults require operator scope reconciliation first. */
export function buildDeploymentPlan(
  raw: ClientConfig,
  templates: TemplateLookup,
  registry: ModuleRegistry = new ModuleRegistry(),
  hostEvidence: readonly IntegrationEvidence[] = [],
): DeploymentPlan {
  const config = parseClientConfig(raw)
  if (config.schema_version !== '1.0.0')
    throw new Error(`Unsupported client config version: ${config.schema_version}`)
  const template = templates.get(config.template)
  requireUnique(
    config.modules.map((item) => item.name),
    'module',
  )
  requireUnique(
    config.integrations.map((item) => item.name),
    'integration',
  )
  requireUnique(
    hostEvidence.map((item) => item.name),
    'integration evidence',
  )
  const evidence = new Map(
    hostEvidence.map((rawEvidence) => {
      const item = integrationEvidenceSchema.parse(rawEvidence)
      if (item.tenantId !== config.tenantId)
        throw new Error('Cross-tenant integration evidence refused')
      return [item.name, item] as const
    }),
  )
  const toggles = new Map(config.modules.map((item) => [item.name, item.enabled]))
  // Unknown disabled names are errors too: a typo must not hide an opt-out.
  for (const name of toggles.keys()) registry.get(name)
  const selected = new Map<string, DeploymentPlan['modules'][number]>()
  const include = (name: string): void => {
    if (toggles.get(name) === false) throw new Error(`Required module explicitly disabled: ${name}`)
    if (selected.has(name)) return
    const manifest = registry.get(name)
    for (const dependency of manifest.requires) include(dependency)
    selected.set(name, {
      name,
      version: manifest.version,
      requested: toggles.get(name) === true,
      supported: manifest.status === 'implemented',
    })
  }
  for (const [name, enabled] of toggles) if (enabled) include(name)
  const integrations = config.integrations.map((item): DeploymentPlan['integrations'][number] => {
    const proof = item.enabled ? evidence.get(item.name) : undefined
    return {
      ...item,
      requested: item.enabled,
      supported: proof?.supported ?? false,
      configured: proof?.configured ?? false,
      connected: proof?.connected ?? false,
      healthy: proof?.healthy ?? false,
      ...(proof ? { checkedAt: proof.checkedAt } : {}),
    }
  })
  const modules = [...selected.values()]
  const blockers = [
    ...modules
      .filter((item) => !item.supported)
      .map((item) => `Module ${item.name} is planned; no supported implementation registered`),
    ...integrations
      .filter((item) => item.requested && !item.healthy)
      .map((item) => `Integration ${item.name} needs host connection/health verification`),
  ]
  return {
    schema_version: '2.0.0',
    tenantId: config.tenantId,
    config,
    suggestions: {
      modules: [...template.modules],
      integrations: [...template.common_integrations],
    },
    modules,
    integrations,
    blockers,
    ready: blockers.length === 0,
  }
}
