import { describe, it, expect } from 'vitest'
import { releaseCatalogSchema } from '../src/release-contracts.js'
import { planAssembly, MINIMAL_MODULES, FULL_MODULES } from '../src/assembly.js'
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
