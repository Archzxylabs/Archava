import { describe, it, expect } from 'vitest'
import catalog from '../../../config/library-release.v1.json'
import { releaseCatalogSchema } from '../src/release-contracts.js'
import { planAssembly, MINIMAL_MODULES, FULL_MODULES } from '../src/assembly.js'
const trusted = releaseCatalogSchema.parse(catalog)
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
