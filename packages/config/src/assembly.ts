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
    foodservice: z.boolean().optional(),
    support: z.boolean().optional(),
    purchasing: z.boolean().optional(),
    organizations: z.boolean().optional(),
    proposals: z.boolean().optional(),
    capacity: z.boolean().optional(),
    assets: z.boolean().optional(),
    subscriptions: z.boolean().optional(),
    attendance: z.boolean().optional(),
  })
  .strict()
  .superRefine((modules, ctx) => {
    if (modules.nativeActions && !modules.assistant)
      ctx.addIssue({ code: 'custom', message: 'nativeActions requires assistant' })
    if (modules.purchasing && !modules.commerce)
      ctx.addIssue({ code: 'custom', message: 'purchasing requires commerce' })
    if (modules.foodservice && !modules.commerce)
      ctx.addIssue({ code: 'custom', message: 'foodservice requires commerce' })
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
  foodservice: true,
  support: true,
  purchasing: true,
  organizations: true,
  proposals: true,
  capacity: true,
  assets: true,
  subscriptions: true,
  attendance: true,
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
  {
    id: 'foodservice',
    version: '1.0.0',
    requires: ['contacts', 'commerce'],
    packages: [
      '@archava/foodservice',
      '@archava/foodservice-pg',
      '@archava/commerce-pg',
      '@archava/identity-pg',
    ],
    permissions: ['tables.read', 'menu.read', 'tickets.read', 'tickets.write', 'kitchen.write'],
    entry: '@archava/foodservice',
  },
  {
    id: 'support',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/support', '@archava/support-pg', '@archava/identity-pg'],
    permissions: ['cases.read', 'cases.write', 'cases.assign', 'notes.read', 'notes.write'],
    entry: '@archava/support',
  },
  {
    id: 'purchasing',
    version: '1.0.0',
    requires: ['contacts', 'commerce'],
    packages: [
      '@archava/purchasing',
      '@archava/purchasing-pg',
      '@archava/commerce-pg',
      '@archava/identity-pg',
    ],
    permissions: [
      'suppliers.read',
      'offers.read',
      'orders.read',
      'orders.write',
      'receipts.read',
      'receipts.write',
    ],
    entry: '@archava/purchasing',
  },
  {
    id: 'organizations',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/organizations', '@archava/organizations-pg', '@archava/identity-pg'],
    permissions: [
      'organizations.read',
      'organizations.write',
      'affiliations.read',
      'affiliations.write',
    ],
    entry: '@archava/organizations',
  },
  {
    id: 'proposals',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/proposals', '@archava/proposals-pg', '@archava/identity-pg'],
    permissions: [
      'proposals.read',
      'proposals.write',
      'proposals.decide',
      'changerequests.read',
      'changerequests.write',
      'changerequests.decide',
    ],
    entry: '@archava/proposals',
  },
  {
    id: 'capacity',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/capacity', '@archava/capacity-pg', '@archava/identity-pg'],
    permissions: ['capacity.read', 'capacity.write', 'capacity.allocate'],
    entry: '@archava/capacity',
  },
  {
    id: 'assets',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/assets', '@archava/assets-pg', '@archava/identity-pg'],
    permissions: ['assets.read', 'assets.write', 'assets.handover', 'assets.maintenance'],
    entry: '@archava/assets',
  },
  {
    id: 'subscriptions',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/subscriptions', '@archava/subscriptions-pg', '@archava/identity-pg'],
    permissions: [
      'subscriptions.read',
      'subscriptions.write',
      'subscriptions.payment',
      'subscriptions.usage',
    ],
    entry: '@archava/subscriptions',
  },
  {
    id: 'attendance',
    version: '1.0.0',
    requires: ['contacts'],
    packages: ['@archava/attendance', '@archava/attendance-pg', '@archava/identity-pg'],
    permissions: ['attendance.read', 'attendance.write', 'attendance.checkin'],
    entry: '@archava/attendance',
  },
] as const
const legacyModules = moduleSelectionSchema.refine(
  (modules) =>
    modules.projects === undefined &&
    modules.scheduling === undefined &&
    modules.accommodation === undefined &&
    modules.commerce === undefined &&
    modules.foodservice === undefined &&
    modules.support === undefined &&
    modules.purchasing === undefined &&
    modules.organizations === undefined &&
    modules.proposals === undefined &&
    modules.capacity === undefined &&
    modules.assets === undefined &&
    modules.subscriptions === undefined &&
    modules.attendance === undefined,
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
          modules.commerce === undefined &&
          modules.foodservice === undefined &&
          modules.support === undefined &&
          modules.purchasing === undefined &&
          modules.organizations === undefined &&
          modules.proposals === undefined &&
          modules.capacity === undefined &&
          modules.assets === undefined &&
          modules.subscriptions === undefined &&
          modules.attendance === undefined,
        'Scheduling requires assembly contract 3.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('3.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) =>
          modules.accommodation === undefined &&
          modules.commerce === undefined &&
          modules.foodservice === undefined &&
          modules.support === undefined &&
          modules.purchasing === undefined &&
          modules.organizations === undefined &&
          modules.proposals === undefined &&
          modules.capacity === undefined &&
          modules.assets === undefined &&
          modules.subscriptions === undefined &&
          modules.attendance === undefined,
        'Accommodation requires assembly contract 4.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('4.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) =>
          modules.commerce === undefined &&
          modules.foodservice === undefined &&
          modules.support === undefined &&
          modules.purchasing === undefined &&
          modules.organizations === undefined &&
          modules.proposals === undefined &&
          modules.capacity === undefined &&
          modules.assets === undefined &&
          modules.subscriptions === undefined &&
          modules.attendance === undefined,
        'Commerce requires assembly contract 5.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('5.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) =>
          modules.foodservice === undefined &&
          modules.support === undefined &&
          modules.purchasing === undefined &&
          modules.organizations === undefined &&
          modules.proposals === undefined &&
          modules.capacity === undefined &&
          modules.assets === undefined &&
          modules.subscriptions === undefined &&
          modules.attendance === undefined,
        'Food service requires assembly contract 6.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('6.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (modules) =>
          modules.support === undefined &&
          modules.purchasing === undefined &&
          modules.organizations === undefined &&
          modules.proposals === undefined &&
          modules.capacity === undefined &&
          modules.assets === undefined &&
          modules.subscriptions === undefined &&
          modules.attendance === undefined,
        'Support cases require assembly contract 7.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('7.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (m) =>
          m.purchasing === undefined &&
          m.organizations === undefined &&
          m.proposals === undefined &&
          m.capacity === undefined &&
          m.assets === undefined &&
          m.subscriptions === undefined &&
          m.attendance === undefined,
        'Purchasing requires assembly contract 8.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('8.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (m) =>
          m.organizations === undefined &&
          m.proposals === undefined &&
          m.capacity === undefined &&
          m.assets === undefined &&
          m.subscriptions === undefined &&
          m.attendance === undefined,
        'Organizations, proposals and capacity require assembly contract 9.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('9.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (m) =>
          m.assets === undefined && m.subscriptions === undefined && m.attendance === undefined,
        'Asset register requires assembly contract 10.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('10.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (m) => m.subscriptions === undefined && m.attendance === undefined,
        'Subscriptions require assembly contract 11.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('11.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema.refine(
        (m) => m.attendance === undefined,
        'Attendance requires assembly contract 12.0.0',
      ),
    })
    .strict(),
  z
    .object({
      version: z.literal('12.0.0'),
      profile: z.string().regex(/^\d+\.\d+\.\d+$/),
      modules: moduleSelectionSchema,
    })
    .strict(),
])
export function planAssembly(raw: unknown, trustedCatalog: ReleaseCatalog) {
  const catalog = releaseCatalogSchema.parse(trustedCatalog)
  const request = assemblyRequestSchema.parse(raw)
  if (request.profile !== catalog.version) throw new Error('Unsupported assembly profile')
  const validVersions = [
    '2.0.0',
    '3.0.0',
    '4.0.0',
    '5.0.0',
    '6.0.0',
    '7.0.0',
    '8.0.0',
    '9.0.0',
    '10.0.0',
    '11.0.0',
    '12.0.0',
  ]
  if (
    (request.version === '2.0.0' &&
      !validVersions.slice(0).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '3.0.0' &&
      !validVersions.slice(1).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '4.0.0' &&
      !validVersions.slice(2).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '5.0.0' &&
      !validVersions.slice(3).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '6.0.0' &&
      !validVersions.slice(4).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '7.0.0' &&
      !validVersions.slice(5).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '8.0.0' &&
      !validVersions.slice(6).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '9.0.0' &&
      !validVersions.slice(7).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '10.0.0' &&
      !validVersions.slice(8).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '11.0.0' &&
      !validVersions.slice(9).includes(catalog.contracts.assembly ?? '')) ||
    (request.version === '12.0.0' && catalog.contracts.assembly !== '12.0.0')
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
