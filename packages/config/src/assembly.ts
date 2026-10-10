import { z } from 'zod'
import { releaseCatalogSchema, type ReleaseCatalog } from './release-contracts.js'

export const moduleSelectionSchema = z
  .object({
    contacts: z.literal(true),
    pipeline: z.boolean(),
    assistant: z.boolean(),
    nativeActions: z.boolean(),
    projects: z.boolean().optional(),
    scheduling: z.boolean().optional(),
  })
  .strict()
  .superRefine((modules, ctx) => {
    if (modules.nativeActions && !modules.assistant)
      ctx.addIssue({ code: 'custom', message: 'nativeActions requires assistant' })
  })
export type ModuleSelection = z.infer<typeof moduleSelectionSchema>
export const FULL_MODULES: ModuleSelection = Object.freeze({
  contacts: true,
  pipeline: true,
  assistant: true,
  nativeActions: true,
})
export const MINIMAL_MODULES: ModuleSelection = Object.freeze({
  contacts: true,
  pipeline: false,
  assistant: false,
  nativeActions: false,
})
/** Prospective assembly inventory. Existing four-module runtime defaults remain unchanged. */
export const COMPOSABLE_MODULES: ModuleSelection = Object.freeze({
  ...FULL_MODULES,
  projects: true,
  scheduling: true,
})
export const MODULE_CONTRACTS = [
  {
    id: 'contacts',
    version: '1.0.0',
    requires: [],
    packages: ['@archava/crm', '@archava/crm-pg', '@archava/identity-pg'],
    permissions: ['contacts.read', 'contacts.write'],
    entry: '@archava/crm',
  },
  {
    id: 'pipeline',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/crm'],
    permissions: ['pipeline.read', 'pipeline.write'],
    entry: '@archava/crm/pipeline',
  },
  {
    id: 'assistant',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/crm-assistant'],
    permissions: ['contacts.read'],
    entry: '@archava/crm-assistant',
  },
  {
    id: 'projects',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/projects', '@archava/projects-pg', '@archava/identity-pg'],
    permissions: ['projects.read', 'projects.write'],
    entry: '@archava/projects',
  },
  {
    id: 'scheduling',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/scheduling', '@archava/scheduling-pg', '@archava/identity-pg'],
    permissions: ['resources.read', 'resources.write', 'reservations.read', 'reservations.write'],
    entry: '@archava/scheduling',
  },
  {
    id: 'nativeActions',
    version: '1.0.0',
    requires: ['contacts', 'assistant'],
    packages: ['@archava/crm-act', '@archava/act-host-pg'],
    permissions: ['followups.write'],
    entry: '@archava/crm-act',
  },
] as const
const legacyModules = moduleSelectionSchema.refine(
  (modules) => modules.projects === undefined && modules.scheduling === undefined,
  'Project composition requires assembly contract 2.0.0',
)
export const assemblyRequestSchema = z.discriminatedUnion('version', [
  z
    .object({
      version: z.literal('1.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: legacyModules,
    })
    .strict(),
  z
    .object({
      version: z.literal('2.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) => modules.scheduling === undefined,
        'Scheduling requires assembly contract 3.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('3.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema,
    })
    .strict(),
])
export function planAssembly(raw: unknown, trustedCatalog: ReleaseCatalog) {
  const catalog = releaseCatalogSchema.parse(trustedCatalog)
  const request = assemblyRequestSchema.parse(raw)
  if (request.profile !== catalog.version) throw new Error('Unsupported assembly profile')
  if (
    (request.version === '2.0.0' &&
      !['2.0.0', '3.0.0'].includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '3.0.0' && catalog.contracts.assembly !== '3.0.0')
  )
    throw new Error('Unsupported assembly contract')
  const enabled = MODULE_CONTRACTS.filter((module) => request.modules[module.id])
  for (const module of enabled) {
    if (module.requires.some((id) => !request.modules[id]))
      throw new Error('Disabled module prerequisite')
    if (module.packages.some((name) => !catalog.packages.some((pkg) => pkg.name === name)))
      throw new Error('Module implementation unavailable')
  }
  return {
    version: request.version,
    profile: catalog.version,
    modules: request.modules,
    entries: enabled.map((module) => ({
      id: module.id,
      entry: module.entry,
      version: module.version,
    })),
    installation: catalog.packages.map((pkg) => ({ name: pkg.name, version: pkg.version })),
    unresolvedLegacyNames: ['crm_core', 'payment_gateway', 'inventory', 'booking', 'erp'],
    authority: 'configuration-only' as const,
    dataPolicy:
      'retain-data-audit-receipts; refuse disable with unresolved native effects' as const,
  }
}
