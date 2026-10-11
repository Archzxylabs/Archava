import { describe, it, expect } from 'vitest'
import { releaseCatalogSchema } from '../src/release-contracts.js'
import { planAssembly, MINIMAL_MODULES, FULL_MODULES, COMPOSABLE_MODULES } from '../src/assembly.js'
const trusted = releaseCatalogSchema.parse({
  schemaVersion: '1.0.0',
  id: 'archava-local-library',
  version: '1.3.0',
  host: { name: '@archava/crm-host', version: '0.6.0' },
  contracts: { clientConfig: '1.0.0', deploymentPlan: '2.0.0', hostSettings: 1 },
  packages: ['crm', 'crm-pg', 'identity-pg', 'crm-assistant', 'crm-act', 'act-host-pg'].map(
    (name) => ({
      name: '@archava/' + name,
      version: '0.1.0',
      migrations: name === 'crm-pg' ? [{ file: '001_fixture.sql', checksum: 'a'.repeat(64) }] : [],
    }),
  ),
  databaseLedgers: [
    { schema: 'archava_crm', package: '@archava/crm-pg', migrations: ['001_fixture'] },
  ],
  requiredAdapters: ['@archava/crm-pg'],
  operatorChecks: ['fixture ledger'],
})
const request = (modules: unknown) => ({ version: '1.0.0', profile: trusted.version, modules })
describe('untrusted composition requests', () => {
  it('requires v7 and current support implementation while preserving earlier contracts and defaults', () => {
    const available = {
      ...trusted,
      contracts: { ...trusted.contracts, assembly: '7.0.0' as const },
      packages: [
        ...trusted.packages,
        ...['support', 'support-pg'].map((name) => ({
          name: '@archava/' + name,
          version: '0.1.0',
          migrations: [],
        })),
      ],
    }
    const input = {
      version: '7.0.0',
      profile: trusted.version,
      modules: { ...MINIMAL_MODULES, support: true },
    }
    expect(planAssembly(input, available).entries.map((x) => x.id)).toEqual(['contacts', 'support'])
    for (const version of ['1.0.0', '2.0.0', '3.0.0', '4.0.0', '5.0.0', '6.0.0'])
      expect(() => planAssembly({ ...input, version }, available)).toThrow()
    expect(() =>
      planAssembly(input, {
        ...available,
        packages: available.packages.filter((x) => x.name !== '@archava/support-pg'),
      }),
    ).toThrow()
    expect(FULL_MODULES.support).toBeUndefined()
  })
  it('requires v6 and shared commerce for food-service while refusing unavailable domain implementations', () => {
    const available = {
      ...trusted,
      contracts: { ...trusted.contracts, assembly: '6.0.0' as const },
      packages: [
        ...trusted.packages,
        ...['commerce', 'commerce-pg', 'foodservice', 'foodservice-pg'].map((name) => ({
          name: '@archava/' + name,
          version: '0.1.0',
          migrations: [],
        })),
      ],
    }
    const input = {
      version: '6.0.0',
      profile: trusted.version,
      modules: { ...MINIMAL_MODULES, commerce: true, foodservice: true },
    }
    expect(planAssembly(input, available).entries.map((x) => x.id)).toEqual([
      'contacts',
      'commerce',
      'foodservice',
    ])
    for (const version of ['1.0.0', '2.0.0', '3.0.0', '4.0.0', '5.0.0'])
      expect(() => planAssembly({ ...input, version }, available)).toThrow()
    expect(() =>
      planAssembly({ ...input, modules: { ...input.modules, commerce: false } }, available),
    ).toThrow()
    expect(() =>
      planAssembly(input, {
        ...available,
        packages: available.packages.filter((x) => x.name !== '@archava/foodservice-pg'),
      }),
    ).toThrow()
    expect(FULL_MODULES.foodservice).toBeUndefined()
  })
  it('requires v5 for commerce while preserving old defaults and refusing missing stock adapters', () => {
    const available = {
      ...trusted,
      contracts: { ...trusted.contracts, assembly: '5.0.0' as const },
      packages: [
        ...trusted.packages,
        ...['commerce', 'commerce-pg'].map((name) => ({
          name: '@archava/' + name,
          version: '0.1.0',
          migrations: [],
        })),
      ],
    }
    const input = {
      version: '5.0.0',
      profile: trusted.version,
      modules: { ...MINIMAL_MODULES, commerce: true },
    }
    expect(planAssembly(input, available).entries.map((x) => x.id)).toEqual([
      'contacts',
      'commerce',
    ])
    for (const version of ['1.0.0', '2.0.0', '3.0.0', '4.0.0'])
      expect(() => planAssembly({ ...input, version }, available)).toThrow()
    expect(() =>
      planAssembly(input, {
        ...available,
        packages: available.packages.filter((x) => x.name !== '@archava/commerce-pg'),
      }),
    ).toThrow('unavailable')
    expect(planAssembly(request(FULL_MODULES), available).entries).toHaveLength(4)
    expect(COMPOSABLE_MODULES.commerce).toBe(true)
  })
  it('requires v4 and both occupancy and accommodation implementations without changing earlier contracts', () => {
    const available = {
      ...trusted,
      contracts: { ...trusted.contracts, assembly: '4.0.0' as const },
      packages: [
        ...trusted.packages,
        ...[
          'projects',
          'projects-pg',
          'scheduling',
          'scheduling-pg',
          'accommodation',
          'accommodation-pg',
        ].map((name) => ({ name: '@archava/' + name, version: '0.1.0', migrations: [] })),
      ],
    }
    const input = {
      version: '4.0.0',
      profile: trusted.version,
      modules: { ...FULL_MODULES, projects: true, scheduling: true, accommodation: true },
    }
    expect(planAssembly(input, available).entries.map((x) => x.id)).toContain('accommodation')
    for (const version of ['1.0.0', '2.0.0', '3.0.0'])
      expect(() => planAssembly({ ...input, version }, available)).toThrow()
    expect(() =>
      planAssembly({ ...input, modules: { ...input.modules, scheduling: false } }, available),
    ).toThrow()
    expect(() =>
      planAssembly(input, {
        ...available,
        packages: available.packages.filter((p) => p.name !== '@archava/accommodation-pg'),
      }),
    ).toThrow('unavailable')
    expect(planAssembly(request(FULL_MODULES), available).entries).toHaveLength(4)
  })
  it('requires v3 and installed scheduling packages while retaining v1/v2 compatibility', () => {
    const available = {
      ...trusted,
      contracts: { ...trusted.contracts, assembly: '3.0.0' as const },
      packages: [
        ...trusted.packages,
        ...['projects', 'projects-pg', 'scheduling', 'scheduling-pg'].map((name) => ({
          name: '@archava/' + name,
          version: '0.1.0',
          migrations: [],
        })),
      ],
    }
    const input = {
      version: '3.0.0',
      profile: trusted.version,
      modules: { ...FULL_MODULES, projects: true, scheduling: true },
    }
    expect(planAssembly(input, available).entries.map((x) => x.id)).toContain('scheduling')
    expect(planAssembly(request(FULL_MODULES), available).entries).toHaveLength(4)
    expect(
      planAssembly(
        {
          version: '2.0.0',
          profile: trusted.version,
          modules: { ...FULL_MODULES, projects: true },
        },
        available,
      ).entries,
    ).toHaveLength(5)
    expect(() => planAssembly({ ...input, version: '2.0.0' }, available)).toThrow()
    expect(() => planAssembly(input, trusted)).toThrow('contract')
    expect(() =>
      planAssembly(input, {
        ...available,
        packages: available.packages.filter((x) => x.name !== '@archava/scheduling-pg'),
      }),
    ).toThrow('unavailable')
  })
  it('composes projects only under the supported v2 contract and never silently changes legacy defaults', () => {
    const available = {
      ...trusted,
      contracts: { ...trusted.contracts, assembly: '2.0.0' as const },
      packages: [
        ...trusted.packages,
        { name: '@archava/projects', version: '0.1.0', migrations: [] },
        { name: '@archava/projects-pg', version: '0.1.0', migrations: [] },
      ],
    }
    const input = {
      version: '2.0.0',
      profile: trusted.version,
      modules: { ...FULL_MODULES, projects: true },
    }
    expect(planAssembly(input, available).entries.map((x) => x.id)).toContain('projects')
    expect(planAssembly(request(FULL_MODULES), available).entries).toHaveLength(4)
    expect(() => planAssembly({ ...input, version: '1.0.0' }, available)).toThrow()
    expect(() => planAssembly(input, trusted)).toThrow('contract')
    expect(() =>
      planAssembly(input, {
        ...available,
        packages: available.packages.filter((x) => x.name !== '@archava/projects-pg'),
      }),
    ).toThrow('unavailable')
  })
  it('plans minimal and full configurations using shared installed entries without authority', () => {
    expect(planAssembly(request(MINIMAL_MODULES), trusted).entries.map((x) => x.id)).toEqual([
      'contacts',
    ])
    const full = planAssembly(request(FULL_MODULES), trusted)
    expect(full.entries).toHaveLength(4)
    expect(full.authority).toBe('configuration-only')
    expect(full.unresolvedLegacyNames).toContain('erp')
  })
  it('refuses unknown modules, missing mandatory core, disabled prerequisites, unavailable implementations and forged authority', () => {
    for (const modules of [
      { ...FULL_MODULES, inventory: true },
      { ...FULL_MODULES, contacts: false },
      { ...FULL_MODULES, assistant: false },
    ])
      expect(() => planAssembly(request(modules), trusted)).toThrow()
    expect(() => planAssembly({ ...request(FULL_MODULES), role: 'owner' }, trusted)).toThrow()
    expect(() => planAssembly({ ...request(FULL_MODULES), profile: '99.0.0' }, trusted)).toThrow(
      'Unsupported',
    )
    expect(() =>
      planAssembly(request(FULL_MODULES), {
        ...trusted,
        packages: trusted.packages.filter((p) => p.name !== '@archava/crm-assistant'),
      }),
    ).toThrow('unavailable')
  })
})
