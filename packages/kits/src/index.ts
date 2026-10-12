import { KIT_CATALOGUE_DATA, KIT_CATALOGUE_VERSION, CAPABILITY_IDS } from './catalogue.js'
import {
  kitManifestSchema,
  kitRequestSchema,
  KitPlanningError,
  type KitManifest,
  type KitPlan,
  type KitRequirement,
} from './contracts.js'
import { moduleSelectionSchema, planAssembly, type ModuleSelection } from '@archava/config/assembly'
import { releaseCatalogSchema, type ReleaseCatalog } from '@archava/config/release-contracts'

export * from './contracts.js'
export { KIT_CATALOGUE_VERSION, CAPABILITY_IDS } from './catalogue.js'

/** Freeze nested host-owned material; a returned reference cannot add capabilities. */
function freeze<T>(value: T): T {
  if (value !== null && typeof value === 'object') {
    for (const child of Object.values(value)) freeze(child)
    Object.freeze(value)
  }
  return value
}
const manifests = freeze(KIT_CATALOGUE_DATA.map((raw) => kitManifestSchema.parse(raw)))
const byId = new Map(manifests.map((manifest) => [manifest.id, manifest]))
if (byId.size !== 40 || new Set(manifests.map((m) => m.slug)).size !== 40)
  throw new KitPlanningError('invalid_builtin_catalogue')

export function listKits(): readonly KitManifest[] {
  return manifests
}
export function getKit(id: string): KitManifest {
  const manifest = byId.get(id)
  if (!manifest) throw new KitPlanningError('unknown_kit')
  return manifest
}

const LOCAL_IMPLEMENTATIONS = freeze({
  purchasing: [
    { name: '@archava/purchasing', version: '0.1.0' },
    { name: '@archava/purchasing-pg', version: '0.1.0' },
    { name: '@archava/commerce', version: '0.3.0' },
    { name: '@archava/commerce-pg', version: '0.3.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  organization_accounts: [
    { name: '@archava/organizations', version: '0.1.0' },
    { name: '@archava/organizations-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  proposals_scope: [
    { name: '@archava/proposals', version: '0.1.0' },
    { name: '@archava/proposals-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  delivery_capacity: [
    { name: '@archava/capacity', version: '0.1.0' },
    { name: '@archava/capacity-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  asset_register: [
    { name: '@archava/assets', version: '0.1.0' },
    { name: '@archava/assets-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  subscriptions_entitlements: [
    { name: '@archava/subscriptions', version: '0.1.0' },
    { name: '@archava/subscriptions-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  attendance_roster: [
    { name: '@archava/attendance', version: '0.1.0' },
    { name: '@archava/attendance-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  attendance: [
    { name: '@archava/attendance', version: '0.1.0' },
    { name: '@archava/attendance-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  support_cases: [
    { name: '@archava/support', version: '0.1.0' },
    { name: '@archava/support-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  foodservice_ops: [
    { name: '@archava/foodservice', version: '0.1.0' },
    { name: '@archava/foodservice-pg', version: '0.1.0' },
    { name: '@archava/commerce', version: '0.3.0' },
    { name: '@archava/commerce-pg', version: '0.3.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  crm_contacts: [
    { name: '@archava/crm', version: '0.7.0' },
    { name: '@archava/crm-pg', version: '0.7.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  crm_sales_pipeline: [
    { name: '@archava/crm', version: '0.7.0' },
    { name: '@archava/crm-pg', version: '0.7.0' },
  ],
  project_delivery: [
    { name: '@archava/projects', version: '0.1.0' },
    { name: '@archava/projects-pg', version: '0.1.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  appointment_scheduling: [
    { name: '@archava/scheduling', version: '0.2.0' },
    { name: '@archava/scheduling-pg', version: '0.2.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  accommodation_stays: [
    { name: '@archava/accommodation', version: '0.1.0' },
    { name: '@archava/accommodation-pg', version: '0.1.0' },
    { name: '@archava/scheduling', version: '0.2.0' },
    { name: '@archava/scheduling-pg', version: '0.2.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  catalog_products: [
    { name: '@archava/commerce', version: '0.3.0' },
    { name: '@archava/commerce-pg', version: '0.3.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  commerce_orders: [
    { name: '@archava/commerce', version: '0.3.0' },
    { name: '@archava/commerce-pg', version: '0.3.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
  inventory_quantities: [
    { name: '@archava/commerce', version: '0.3.0' },
    { name: '@archava/commerce-pg', version: '0.3.0' },
    { name: '@archava/identity-pg', version: '0.1.0' },
  ],
})

/**
 * Construct only from the trusted host after installed-profile verification.
 * Request JSON never supplies the catalogue, module policy, implementation or
 * provider evidence. The plan does not replace startup health or authorization.
 */
export class KitPlanner {
  private readonly release: ReleaseCatalog
  private readonly modules: ModuleSelection
  constructor(trustedRelease: ReleaseCatalog, trustedModules: ModuleSelection) {
    this.release = freeze(releaseCatalogSchema.parse(trustedRelease))
    this.modules = freeze(moduleSelectionSchema.parse(trustedModules))
  }

  plan(raw: unknown): KitPlan {
    const request = kitRequestSchema.parse(raw)
    if (request.profile !== this.release.version) throw new KitPlanningError('unsupported_profile')
    const required = new Map<string, Set<string>>()
    const add = (id: string, by: string) => {
      const owners = required.get(id) ?? new Set<string>()
      owners.add(by)
      required.set(id, owners)
    }
    for (const selected of request.selections) {
      const kit = getKit(selected.id)
      if (selected.scope === 'crm_inquiry') {
        // Existing reuse evidence covers these two inquiry contexts only.
        if (kit.id !== 'K01' && kit.id !== 'K05') add(`scope.${kit.id}.crm_inquiry`, kit.id)
        add('crm_contacts', kit.id)
      } else if (selected.scope === 'project_tracking') {
        if (kit.id !== 'K05' && kit.id !== 'K31') add(`scope.${kit.id}.project_tracking`, kit.id)
        add('crm_contacts', kit.id)
        add('project_delivery', kit.id)
      } else if (selected.scope === 'resource_reservations') {
        if (kit.id !== 'K07' && kit.id !== 'K10')
          add(`scope.${kit.id}.resource_reservations`, kit.id)
        add('crm_contacts', kit.id)
        add('appointment_scheduling', kit.id)
      } else if (selected.scope === 'accommodation_stays') {
        if (kit.id !== 'K01') add(`scope.${kit.id}.accommodation_stays`, kit.id)
        add('crm_contacts', kit.id)
        add('appointment_scheduling', kit.id)
        add('accommodation_stays', kit.id)
      } else if (selected.scope === 'purchase_receipts') {
        if (!['K02', 'K04', 'K18', 'K23'].includes(kit.id))
          add(`scope.${kit.id}.purchase_receipts`, kit.id)
        add('purchasing', kit.id)
      } else if (selected.scope === 'organization_accounts') {
        if (!['K04', 'K21'].includes(kit.id)) add(`scope.${kit.id}.organization_accounts`, kit.id)
        add('crm_contacts', kit.id)
        add('organization_accounts', kit.id)
      } else if (selected.scope === 'proposals_scope') {
        if (!['K05', 'K31'].includes(kit.id)) add(`scope.${kit.id}.proposals_scope`, kit.id)
        add('crm_contacts', kit.id)
        add('proposals_scope', kit.id)
      } else if (selected.scope === 'delivery_capacity') {
        if (!['K05', 'K31'].includes(kit.id)) add(`scope.${kit.id}.delivery_capacity`, kit.id)
        add('crm_contacts', kit.id)
        add('delivery_capacity', kit.id)
      } else if (selected.scope === 'asset_register') {
        if (!['K10', 'K18'].includes(kit.id)) add(`scope.${kit.id}.asset_register`, kit.id)
        add('crm_contacts', kit.id)
        add('asset_register', kit.id)
      } else if (selected.scope === 'subscriptions_entitlements') {
        if (!['K08', 'K09'].includes(kit.id))
          add(`scope.${kit.id}.subscriptions_entitlements`, kit.id)
        add('crm_contacts', kit.id)
        add('subscriptions_entitlements', kit.id)
      } else if (selected.scope === 'attendance_roster') {
        if (!['K13', 'K15'].includes(kit.id)) add(`scope.${kit.id}.attendance_roster`, kit.id)
        add('crm_contacts', kit.id)
        add('attendance_roster', kit.id)
      } else if (selected.scope === 'support_cases') {
        if (!['K05', 'K06', 'K12', 'K18', 'K21'].includes(kit.id))
          add(`scope.${kit.id}.support_cases`, kit.id)
        add('crm_contacts', kit.id)
        add('support_cases', kit.id)
      } else if (selected.scope === 'foodservice_tickets') {
        if (kit.id !== 'K02') add(`scope.${kit.id}.foodservice_tickets`, kit.id)
        add('foodservice_ops', kit.id)
      } else if (selected.scope === 'commerce_orders') {
        if (kit.id !== 'K03' && kit.id !== 'K04') add(`scope.${kit.id}.commerce_orders`, kit.id)
        for (const id of [
          'crm_contacts',
          'catalog_products',
          'commerce_orders',
          'inventory_quantities',
        ])
          add(id, kit.id)
      } else {
        for (const capability of kit.capabilities) add(capability, kit.id)
        add(kit.domainRequirement, kit.id)
      }
    }
    for (const id of request.requirements) add(id, 'explicit-requirement')
    if (required.has('purchasing'))
      for (const id of ['crm_contacts', 'catalog_products', 'inventory_quantities'])
        add(id, 'purchasing')
    if (required.has('support_cases')) add('crm_contacts', 'support_cases')
    if (required.has('foodservice_ops'))
      for (const id of [
        'crm_contacts',
        'catalog_products',
        'commerce_orders',
        'inventory_quantities',
      ])
        add(id, 'foodservice_ops')
    if (required.has('accommodation_stays')) {
      add('appointment_scheduling', 'accommodation_stays')
      add('crm_contacts', 'accommodation_stays')
    }
    if (
      ['catalog_products', 'commerce_orders', 'inventory_quantities'].some((id) => required.has(id))
    )
      add('crm_contacts', 'commerce')
    if (request.pipeline) add('crm_sales_pipeline', 'pipeline')
    if (required.has('crm_sales_pipeline')) add('crm_contacts', 'crm_sales_pipeline')
    if (request.assistant) add('surface.assistant', 'assistant')
    if (request.nativeActions) add('surface.native_actions', 'nativeActions')
    if (required.has('surface.native_actions')) add('surface.assistant', 'surface.native_actions')
    for (const channel of new Set(request.channels))
      if (channel !== 'web' && channel !== 'api') add(`channel.${channel}`, 'channel')

    const requirements: KitRequirement[] = [...required].map(([id, owners]) => {
      const base = { id, requiredBy: [...owners].sort(), packages: [] }
      const implementation = Object.hasOwn(LOCAL_IMPLEMENTATIONS, id)
        ? LOCAL_IMPLEMENTATIONS[id as keyof typeof LOCAL_IMPLEMENTATIONS]
        : undefined
      if (implementation) {
        if (
          implementation.some(
            (pkg) =>
              !this.release.packages.some(
                (installed) => installed.name === pkg.name && installed.version === pkg.version,
              ),
          )
        )
          return {
            ...base,
            disposition: 'needs_implementation',
            reason: 'Required exact local implementation is absent from the trusted profile.',
          }
        if (
          (id === 'crm_sales_pipeline' && !this.modules.pipeline) ||
          (id === 'project_delivery' && !this.modules.projects) ||
          (id === 'appointment_scheduling' && !this.modules.scheduling) ||
          (id === 'purchasing' && !this.modules.purchasing) ||
          (id === 'organization_accounts' && !this.modules.organizations) ||
          (id === 'proposals_scope' && !this.modules.proposals) ||
          (id === 'delivery_capacity' && !this.modules.capacity) ||
          (id === 'asset_register' && !this.modules.assets) ||
          (id === 'subscriptions_entitlements' && !this.modules.subscriptions) ||
          ((id === 'attendance_roster' || id === 'attendance') && !this.modules.attendance) ||
          (id === 'support_cases' && !this.modules.support) ||
          (id === 'foodservice_ops' && !this.modules.foodservice) ||
          (id === 'accommodation_stays' && !this.modules.accommodation) ||
          (['catalog_products', 'commerce_orders', 'inventory_quantities'].includes(id) &&
            !this.modules.commerce)
        )
          return {
            ...base,
            packages: implementation,
            disposition: 'needs_configuration',
            reason: 'Requested domain is disabled by the trusted host module policy.',
          }
        return {
          ...base,
          packages: implementation,
          disposition: 'implemented_local_contract',
          reason:
            'Exact local domain implementation is in the trusted verified profile; this does not accept the full kit.',
        }
      }
      if (id === 'surface.assistant' || id === 'surface.native_actions') {
        const enabled =
          id === 'surface.assistant' ? this.modules.assistant : this.modules.nativeActions
        const selected = {
          contacts: true as const,
          pipeline: false,
          assistant: true,
          nativeActions: id === 'surface.native_actions',
        }
        try {
          planAssembly(
            { version: '1.0.0', profile: this.release.version, modules: selected },
            this.release,
          )
        } catch {
          return {
            ...base,
            disposition: 'needs_implementation',
            reason: 'The requested assistant/action implementation is absent from the profile.',
          }
        }
        return {
          ...base,
          disposition: enabled ? 'implemented_local_contract' : 'needs_configuration',
          reason: enabled
            ? 'Local surface exists; current domain grants and exact consent remain required.'
            : 'Requested surface is disabled by trusted host policy.',
        }
      }
      const domain = manifests.find((kit) => kit.domainRequirement === id)
      if (domain)
        return {
          ...base,
          disposition:
            domain.evidenceDepth === 'taxonomy_only'
              ? 'needs_domain_review'
              : 'needs_implementation',
          reason:
            domain.evidenceDepth === 'taxonomy_only'
              ? 'Taxonomy evidence is insufficient to accept this domain workflow.'
              : 'Domain manifest is a candidate; no accepted implementation/adapter is registered.',
        }
      if ((CAPABILITY_IDS as readonly string[]).includes(id))
        return {
          ...base,
          disposition: 'needs_implementation',
          reason:
            'Capability is a research candidate, not a registered native service or live adapter.',
        }
      return {
        ...base,
        disposition: 'unsupported',
        reason:
          'No accepted scope, channel or capability contract is registered for this requirement.',
      }
    })
    const blockers = requirements.filter(
      (item) => item.disposition !== 'implemented_local_contract',
    )
    const modules: ModuleSelection = {
      contacts: true,
      ...(required.has('purchasing') ? { purchasing: true } : {}),
      ...(required.has('organization_accounts') ? { organizations: true } : {}),
      ...(required.has('proposals_scope') ? { proposals: true } : {}),
      ...(required.has('delivery_capacity') ? { capacity: true } : {}),
      ...(required.has('asset_register') ? { assets: true } : {}),
      ...(required.has('subscriptions_entitlements') ? { subscriptions: true } : {}),
      ...(required.has('attendance_roster') || required.has('attendance')
        ? { attendance: true }
        : {}),
      ...(required.has('support_cases') ? { support: true } : {}),
      ...(required.has('foodservice_ops') ? { foodservice: true } : {}),
      pipeline: required.has('crm_sales_pipeline'),
      assistant: required.has('surface.assistant'),
      nativeActions: required.has('surface.native_actions'),
      ...(required.has('project_delivery') ? { projects: true } : {}),
      ...(required.has('appointment_scheduling') ? { scheduling: true } : {}),
      ...(required.has('accommodation_stays') ? { accommodation: true } : {}),
      ...(['catalog_products', 'commerce_orders', 'inventory_quantities'].some((id) =>
        required.has(id),
      )
        ? { commerce: true }
        : {}),
    }
    // Reuse the existing assembly validator; planning metadata cannot bypass it.
    let assembly: ModuleSelection | null = null
    if (blockers.length === 0) {
      planAssembly(
        {
          version:
            modules.attendance !== undefined
              ? '12.0.0'
              : modules.subscriptions !== undefined
                ? '11.0.0'
                : modules.assets !== undefined
                  ? '10.0.0'
                  : modules.purchasing !== undefined ||
                      modules.organizations !== undefined ||
                      modules.proposals !== undefined ||
                      modules.capacity !== undefined
                    ? '8.0.0'
                    : modules.support !== undefined
                      ? '7.0.0'
                      : modules.foodservice !== undefined
                        ? '6.0.0'
                        : modules.commerce !== undefined
                          ? '5.0.0'
                          : modules.accommodation !== undefined
                            ? '4.0.0'
                            : modules.scheduling !== undefined
                              ? '3.0.0'
                              : modules.projects !== undefined
                                ? '2.0.0'
                                : '1.0.0',
          profile: this.release.version,
          modules,
        },
        this.release,
      )
      assembly = modules
    }
    return freeze({
      schemaVersion: '1.0.0',
      catalogueVersion: KIT_CATALOGUE_VERSION,
      profile: this.release.version,
      selections: request.selections,
      requirements,
      blockers,
      readyToAssemble: blockers.length === 0,
      assembly,
      operationalKitsAccepted: [],
      authority: 'configuration-only',
      pricing: 'manual-review-required-for-new-kit-scope',
    })
  }
}
