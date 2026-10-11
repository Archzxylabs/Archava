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
    accommodation: z.boolean().optional(),
    commerce: z.boolean().optional(),
  })
  .strict()
  .superRefine((modules, ctx) => {
    if (modules.nativeActions && !modules.assistant)
      ctx.addIssue({ code: 'custom', message: 'nativeActions requires assistant' })
    if (modules.accommodation && !modules.scheduling)
      ctx.addIssue({ code: 'custom', message: 'accommodation requires scheduling' })
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
  accommodation: true,
  commerce: true,
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
    id: 'accommodation',
    version: '1.0.0',
    requires: ['contacts', 'scheduling'],
    packages: [
      '@archava/accommodation',
      '@archava/accommodation-pg',
      '@archava/scheduling-pg',
      '@archava/identity-pg',
    ],
    permissions: ['properties.read', 'rooms.read', 'stays.read', 'stays.write'],
    entry: '@archava/accommodation',
  },
  {
    id: 'nativeActions',
    version: '1.0.0',
    requires: ['contacts', 'assistant'],
    packages: ['@archava/crm-act', '@archava/act-host-pg'],
    permissions: ['followups.write'],
    entry: '@archava/crm-act',
  },
  {
    id: 'commerce',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/commerce', '@archava/commerce-pg', '@archava/identity-pg'],
    permissions: ['catalog.read', 'stock.read', 'orders.read', 'orders.write'],
    entry: '@archava/commerce',
  },
] as const
const legacyModules = moduleSelectionSchema.refine(
  (modules) =>
    modules.projects === undefined &&
    modules.scheduling === undefined &&
    modules.accommodation === undefined &&
    modules.commerce === undefined,
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
        (modules) =>
          modules.scheduling === undefined &&
          modules.accommodation === undefined &&
          modules.commerce === undefined,
        'Scheduling requires assembly contract 3.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('3.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) => modules.accommodation === undefined && modules.commerce === undefined,
        'Accommodation requires assembly contract 4.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('4.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) => modules.commerce === undefined,
        'Commerce requires assembly contract 5.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('5.0.0'),
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
      !['2.0.0', '3.0.0', '4.0.0', '5.0.0'].includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '3.0.0' &&
      !['3.0.0', '4.0.0', '5.0.0'].includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '4.0.0' &&
      !['4.0.0', '5.0.0'].includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '5.0.0' && catalog.contracts.assembly !== '5.0.0')
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
