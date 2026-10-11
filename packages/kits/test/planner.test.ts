import { describe, expect, it } from 'vitest'
import { FULL_MODULES, MINIMAL_MODULES, COMPOSABLE_MODULES } from '@archava/config/assembly'
import { releaseCatalogSchema } from '@archava/config/release-contracts'
import { KitPlanner, listKits, getKit, kitRequestSchema } from '../src/index.js'

function release() {
  return releaseCatalogSchema.parse({
    schemaVersion: '1.0.0',
    id: 'archava-local-library',
    version: '1.3.0',
    host: { name: '@archava/crm-host', version: '0.6.0' },
    contracts: { clientConfig: '1.0.0', deploymentPlan: '2.0.0', hostSettings: 1 },
    packages: [
      { name: '@archava/crm', version: '0.7.0', migrations: [] },
      {
        name: '@archava/crm-pg',
        version: '0.7.0',
        migrations: [{ file: '001_fixture.sql', checksum: 'a'.repeat(64) }],
      },
      { name: '@archava/identity-pg', version: '0.1.0', migrations: [] },
      ...['crm-assistant', 'crm-act', 'act-host-pg'].map((name) => ({
        name: '@archava/' + name,
        version: '0.1.0',
        migrations: [],
      })),
    ],
    databaseLedgers: [
      { schema: 'archava_crm', package: '@archava/crm-pg', migrations: ['001_fixture'] },
    ],
    requiredAdapters: ['@archava/crm-pg'],
    operatorChecks: ['fixture ledger'],
  })
}
const request = (id = 'K01', scope = 'operations') => ({
  schemaVersion: '1.0.0',
  catalogueVersion: '1.0.0',
  profile: '1.3.0',
  selections: [{ id, scope }],
})

describe('trusted kit contracts and support boundary', () => {
  it('composes shared internal support cases across service models without accepting specialized case systems', () => {
    const supported = release()
    supported.contracts.assembly = '7.0.0'
    supported.packages.push(
      ...['support', 'support-pg'].map((name) => ({
        name: '@archava/' + name,
        version: '0.1.0',
        migrations: [],
      })),
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES)
    for (const id of ['K05', 'K06', 'K12', 'K18', 'K21']) {
      const plan = planner.plan(request(id, 'support_cases'))
      expect(plan.readyToAssemble).toBe(true)
      expect(plan.assembly).toEqual({ ...MINIMAL_MODULES, support: true })
      expect(planner.plan(request(id)).readyToAssemble).toBe(false)
    }
    expect(planner.plan(request('K26', 'support_cases')).readyToAssemble).toBe(false)
    expect(
      new KitPlanner(supported, FULL_MODULES).plan(request('K06', 'support_cases')).readyToAssemble,
    ).toBe(false)
  })
  it('composes bounded restaurant tickets with shared commerce and retains all full kit gaps', () => {
    const supported = release()
    supported.contracts.assembly = '6.0.0'
    supported.packages.push(
      ...['commerce', 'commerce-pg', 'foodservice', 'foodservice-pg'].map((name) => ({
        name: '@archava/' + name,
        version: name.startsWith('commerce') ? '0.3.0' : '0.1.0',
        migrations: [],
      })),
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES),
      plan = planner.plan(request('K02', 'foodservice_tickets'))
    expect(plan.readyToAssemble).toBe(true)
    expect(plan.assembly).toEqual({ ...MINIMAL_MODULES, commerce: true, foodservice: true })
    expect(plan.requirements.some((x) => x.id === 'inventory_quantities')).toBe(true)
    for (const kit of listKits()) expect(planner.plan(request(kit.id)).readyToAssemble).toBe(false)
    expect(planner.plan(request('K03', 'foodservice_tickets')).readyToAssemble).toBe(false)
    expect(
      new KitPlanner(supported, FULL_MODULES).plan(request('K02', 'foodservice_tickets'))
        .readyToAssemble,
    ).toBe(false)
  })
  it('composes bounded purchasing in wholesale and food contexts while full kits remain blocked', () => {
    const supported = release()
    supported.contracts.assembly = '8.0.0'
    supported.packages.push(
      ...['commerce', 'commerce-pg', 'purchasing', 'purchasing-pg'].map((name) => ({
        name: '@archava/' + name,
        version: name.startsWith('commerce') ? '0.3.0' : '0.1.0',
        migrations: [],
      })),
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES)
    for (const id of ['K02', 'K04', 'K18', 'K23']) {
      const plan = planner.plan(request(id, 'purchase_receipts'))
      expect(plan.readyToAssemble).toBe(true)
      expect(plan.assembly).toEqual({ ...MINIMAL_MODULES, commerce: true, purchasing: true })
      expect(plan.operationalKitsAccepted).toEqual([])
      expect(planner.plan(request(id)).readyToAssemble).toBe(false)
    }
    expect(planner.plan(request('K01', 'purchase_receipts')).readyToAssemble).toBe(false)
    expect(
      new KitPlanner(supported, FULL_MODULES).plan(request('K04', 'purchase_receipts'))
        .readyToAssemble,
    ).toBe(false)
  })
  it('accepts bounded shared orders and stock for retail and wholesale while retaining full kit gaps', () => {
    const supported = release()
    supported.contracts.assembly = '5.0.0'
    supported.packages.push(
      ...['commerce', 'commerce-pg'].map((name) => ({
        name: '@archava/' + name,
        version: '0.3.0',
        migrations: [],
      })),
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES)
    for (const id of ['K03', 'K04']) {
      const plan = planner.plan(request(id, 'commerce_orders'))
      expect(plan.readyToAssemble).toBe(true)
      expect(plan.assembly).toEqual({ ...MINIMAL_MODULES, commerce: true })
      expect(plan.operationalKitsAccepted).toEqual([])
      expect(planner.plan(request(id)).readyToAssemble).toBe(false)
    }
    expect(planner.plan(request('K02', 'commerce_orders')).readyToAssemble).toBe(false)
    expect(
      new KitPlanner(supported, FULL_MODULES).plan(request('K03', 'commerce_orders'))
        .readyToAssemble,
    ).toBe(false)
    expect(
      new KitPlanner(release(), COMPOSABLE_MODULES).plan(request('K03', 'commerce_orders'))
        .readyToAssemble,
    ).toBe(false)
  })
  it('accepts only bounded room-night stays for K01 with exact hotel and shared kernel packages', () => {
    const supported = release()
    supported.contracts.assembly = '4.0.0'
    supported.packages.push(
      ...['scheduling', 'scheduling-pg', 'accommodation', 'accommodation-pg'].map((name) => ({
        name: '@archava/' + name,
        version: name.startsWith('scheduling') ? '0.2.0' : '0.1.0',
        migrations: [],
      })),
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES)
    const plan = planner.plan(request('K01', 'accommodation_stays'))
    expect(plan.readyToAssemble).toBe(true)
    expect(plan.assembly).toEqual({ ...MINIMAL_MODULES, scheduling: true, accommodation: true })
    expect(plan.operationalKitsAccepted).toEqual([])
    expect(planner.plan(request('K01')).readyToAssemble).toBe(false)
    expect(planner.plan(request('K02', 'accommodation_stays')).readyToAssemble).toBe(false)
    expect(
      new KitPlanner(supported, FULL_MODULES).plan(request('K01', 'accommodation_stays'))
        .readyToAssemble,
    ).toBe(false)
    expect(
      new KitPlanner(release(), COMPOSABLE_MODULES).plan(request('K01', 'accommodation_stays'))
        .readyToAssemble,
    ).toBe(false)
  })
  it('accepts bounded exclusive reservations in appointment and asset rental contexts while full kits remain blocked', () => {
    const supported = release()
    supported.contracts.assembly = '3.0.0'
    supported.packages.push(
      { name: '@archava/scheduling', version: '0.2.0', migrations: [] },
      { name: '@archava/scheduling-pg', version: '0.2.0', migrations: [] },
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES)
    for (const id of ['K07', 'K10']) {
      expect(planner.plan(request(id, 'resource_reservations')).assembly).toEqual({
        ...MINIMAL_MODULES,
        scheduling: true,
      })
      expect(planner.plan(request(id)).readyToAssemble).toBe(false)
      expect(planner.plan(request(id, 'resource_reservations')).operationalKitsAccepted).toEqual([])
    }
    expect(planner.plan(request('K01', 'resource_reservations')).readyToAssemble).toBe(false)
    expect(
      new KitPlanner(supported, FULL_MODULES)
        .plan(request('K07', 'resource_reservations'))
        .blockers.some((x) => x.disposition === 'needs_configuration'),
    ).toBe(true)
    expect(
      new KitPlanner(release(), COMPOSABLE_MODULES)
        .plan(request('K07', 'resource_reservations'))
        .blockers.some((x) => x.disposition === 'needs_implementation'),
    ).toBe(true)
  })
  it('accepts explicit internal project tracking in two contexts while full project/EPC kits remain blocked', () => {
    const supported = release()
    supported.contracts.assembly = '2.0.0'
    supported.packages.push(
      { name: '@archava/projects', version: '0.1.0', migrations: [] },
      { name: '@archava/projects-pg', version: '0.1.0', migrations: [] },
    )
    const planner = new KitPlanner(supported, COMPOSABLE_MODULES)
    for (const id of ['K05', 'K31']) {
      const plan = planner.plan(request(id, 'project_tracking'))
      expect(plan.readyToAssemble).toBe(true)
      expect(plan.assembly).toEqual({ ...MINIMAL_MODULES, projects: true })
      expect(plan.operationalKitsAccepted).toEqual([])
      expect(planner.plan(request(id)).readyToAssemble).toBe(false)
    }
    expect(
      planner
        .plan(request('K01', 'project_tracking'))
        .blockers.some((x) => x.disposition === 'unsupported'),
    ).toBe(true)
    expect(
      new KitPlanner(supported, FULL_MODULES)
        .plan(request('K05', 'project_tracking'))
        .blockers.some((x) => x.disposition === 'needs_configuration'),
    ).toBe(true)
    expect(
      new KitPlanner(release(), COMPOSABLE_MODULES)
        .plan(request('K05', 'project_tracking'))
        .blockers.some(
          (x) => x.id === 'project_delivery' && x.disposition === 'needs_implementation',
        ),
    ).toBe(true)
  })
  it('has all 40 immutable families with distinct owners and explicit contract requirements', () => {
    expect(listKits()).toHaveLength(40)
    expect(listKits().filter((kit) => kit.group === 'general_business')).toHaveLength(24)
    expect(new Set(listKits().map((kit) => kit.specification.recordOwner)).size).toBe(40)
    for (const kit of listKits()) {
      expect(kit.specification.apiContract).toContain(kit.slug)
      expect(kit.invariants.length).toBeGreaterThanOrEqual(2)
      expect(kit.status).toBe('candidate')
    }
    expect(() => {
      getKit('K01').capabilities.push('payment_processing')
    }).toThrow()
    expect(() => {
      getKit('K01').specification.recordOwner = 'caller'
    }).toThrow()
    expect(() => getKit('K00')).toThrow('unknown_kit')
  })
  it('does not turn research manifests or existing website templates into operational support', () => {
    const planner = new KitPlanner(release(), FULL_MODULES)
    for (const kit of listKits()) {
      const plan = planner.plan(request(kit.id))
      expect(plan.readyToAssemble).toBe(false)
      expect(plan.assembly).toBeNull()
      expect(plan.operationalKitsAccepted).toEqual([])
      expect(plan.blockers.some((item) => item.id === kit.domainRequirement)).toBe(true)
    }
    expect(
      planner.plan(request('K30')).blockers.find((r) => r.id === 'domain.animal-aquaculture')
        ?.disposition,
    ).toBe('needs_domain_review')
  })
  it('permits only the two accepted narrow CRM inquiry contexts and retains their narrow scope', () => {
    const planner = new KitPlanner(release(), FULL_MODULES)
    for (const id of ['K01', 'K05']) {
      const plan = planner.plan(request(id, 'crm_inquiry'))
      expect(plan.readyToAssemble).toBe(true)
      expect(plan.selections).toEqual([{ id, scope: 'crm_inquiry' }])
      expect(plan.assembly).toEqual(MINIMAL_MODULES)
      expect(plan.operationalKitsAccepted).toEqual([])
      expect(plan.authority).toBe('configuration-only')
    }
    expect(planner.plan(request('K02', 'crm_inquiry')).readyToAssemble).toBe(false)
  })
  it('deduplicates shared dependencies in a hybrid without omitting either domain', () => {
    const plan = new KitPlanner(release(), FULL_MODULES).plan({
      ...request(),
      selections: [
        { id: 'K01', scope: 'operations' },
        { id: 'K02', scope: 'operations' },
      ],
    })
    expect(plan.requirements.filter((r) => r.id === 'payment_processing')).toHaveLength(1)
    expect(plan.requirements.find((r) => r.id === 'payment_processing')?.requiredBy).toEqual([
      'K01',
      'K02',
    ])
    expect(plan.blockers.map((r) => r.id)).toEqual(
      expect.arrayContaining(['domain.accommodation', 'domain.foodservice']),
    )
    expect(plan.readyToAssemble).toBe(false)
  })
  it('keeps exact-package availability and host-disabled surfaces separate from client flags', () => {
    const planner = new KitPlanner(release(), MINIMAL_MODULES)
    const plan = planner.plan({ ...request('K05', 'crm_inquiry'), pipeline: true, assistant: true })
    expect(plan.blockers.map((r) => r.disposition)).toEqual([
      'needs_configuration',
      'needs_configuration',
    ])
    const missing = release()
    missing.packages = missing.packages.filter((p) => p.name !== '@archava/identity-pg')
    expect(
      new KitPlanner(missing, FULL_MODULES).plan(request('K01', 'crm_inquiry')).blockers[0]
        ?.disposition,
    ).toBe('needs_implementation')
    const incompatible = release()
    const crm = incompatible.packages.find((p) => p.name === '@archava/crm')
    if (!crm) throw new Error('Missing fixture')
    crm.version = '99.0.0'
    expect(
      new KitPlanner(incompatible, FULL_MODULES).plan(request('K01', 'crm_inquiry'))
        .readyToAssemble,
    ).toBe(false)
  })
  it('blocks unregistered requirements/channels without silently falling back to generic business', () => {
    const plan = new KitPlanner(release(), FULL_MODULES).plan({
      ...request('K01', 'crm_inquiry'),
      requirements: ['patient_record', 'surface.deployment'],
      channels: ['web', 'pos'],
    })
    expect(plan.blockers.map((r) => r.id)).toEqual([
      'patient_record',
      'surface.deployment',
      'channel.pos',
    ])
    expect(plan.blockers.every((r) => r.disposition === 'unsupported')).toBe(true)
    expect(plan.assembly).toBeNull()
  })
  it('rejects caller-supplied support, grants, provider evidence, scripts and arbitrary model tools', () => {
    const planner = new KitPlanner(release(), FULL_MODULES)
    for (const forged of [
      { supported: true },
      { role: 'owner' },
      { readiness: { healthy: true } },
      { catalogue: [] },
      { apiKey: 'fixture-secret' },
      { execute: 'anything' },
      { script: 'DROP TABLE' },
    ])
      expect(() => planner.plan({ ...request(), ...forged })).toThrow()
    expect(() => planner.plan({ ...request(), profile: '99.0.0' })).toThrow('unsupported_profile')
    expect(() => planner.plan({ ...request(), catalogueVersion: '99.0.0' })).toThrow()
    expect(() =>
      planner.plan({ ...request(), selections: [{ id: 'K99', scope: 'operations' }] }),
    ).toThrow()
    expect(() => planner.plan({ ...request(), requirements: ['../../private'] })).toThrow()
  })
  it('snapshots trusted host metadata and enforces native-action assistant dependencies', () => {
    const trusted = release()
    const modules = { ...FULL_MODULES }
    const planner = new KitPlanner(trusted, modules)
    trusted.packages.length = 0
    modules.pipeline = false
    const plan = planner.plan({
      ...request('K01', 'crm_inquiry'),
      requirements: ['surface.native_actions'],
      pipeline: true,
    })
    expect(plan.readyToAssemble).toBe(true)
    expect(plan.assembly).toEqual(FULL_MODULES)
    expect(() => {
      Object.defineProperty(plan.requirements, 'length', { value: 0 })
    }).toThrow()
    expect(kitRequestSchema.safeParse({ ...request(), nativeActions: true }).success).toBe(false)
  })
  it('bounds malformed and duplicate selections rather than silently dropping scope', () => {
    const raw = request()
    expect(
      kitRequestSchema.safeParse({ ...raw, selections: [raw.selections[0], raw.selections[0]] })
        .success,
    ).toBe(false)
    expect(kitRequestSchema.safeParse({ ...raw, selections: [] }).success).toBe(false)
    expect(
      kitRequestSchema.safeParse({
        ...raw,
        selections: Array.from({ length: 9 }, (_, i) => ({
          id: 'K' + String(i + 1).padStart(2, '0'),
          scope: 'operations',
        })),
      }).success,
    ).toBe(false)
  })
})
