import { describe, expect, it } from 'vitest'
import { resolve } from 'node:path'
import {
  ClientConfigResolver,
  ModuleRegistry,
  TemplateRegistry,
  PLATFORM_MODULES,
  parseClientConfig,
  loadTemplates,
  type IntegrationEvidence,
  type ModuleManifest,
} from '../src/index.js'

const config = (overrides: Record<string, unknown> = {}) =>
  parseClientConfig({
    schema_version: '1.0.0',
    tenantId: 'clinic-demo',
    template: 'clinic',
    presence: 'chat',
    capability: 'assist',
    region: 'ID',
    environment: 'business',
    branding: {
      businessName: 'Offline clinic',
      theme: {
        accent: '#123456',
        surface: '#ffffff',
        ink: '#000000',
        radius: 'soft',
        fontFamily: 'Inter',
      },
    },
    languages: [{ code: 'en', label: 'English' }],
    primaryLanguage: 'en',
    updatedAt: '2026-10-08',
    ...overrides,
  })
const resolver = new ClientConfigResolver()
const proof: IntegrationEvidence = {
  tenantId: 'clinic-demo',
  name: 'Postmark',
  supported: true,
  configured: true,
  connected: true,
  healthy: true,
  checkedAt: '2026-10-08T00:00:00Z',
}

describe('deployment plan V2', () => {
  it('loads host templates from a deeply nested server working directory', () => {
    const templates = new TemplateRegistry(loadTemplates(resolve('apps/web/src/server')))
    expect(new ClientConfigResolver(templates).plan(config()).suggestions.integrations).toContain(
      'Postmark',
    )
  })
  it('keeps a zero-integration Assist scope empty; template suggestions never become deployment requests', () => {
    const input = config()
    const plan = resolver.plan(input)
    expect(plan.config).toEqual(input)
    expect(plan.integrations).toEqual([])
    expect(plan.modules).toEqual([])
    expect(plan.suggestions.integrations).toContain('Postmark')
    expect(plan.ready).toBe(true)
    expect(input.integrations).toEqual([])
  })

  it('preserves explicitly labelled legacy behavior for existing configs', () => {
    expect(resolver.resolveLegacy(config())).toEqual(resolver.resolve(config()))
    expect(resolver.resolveLegacy(config()).integrations.some((item) => item.enabled)).toBe(true)
  })

  it('preserves quoted complexity without pretending a requested integration is connected', () => {
    const plan = resolver.plan(
      config({ integrations: [{ name: 'Postmark', complexity: 'custom' }] }),
    )
    expect(plan.integrations).toEqual([
      {
        name: 'Postmark',
        complexity: 'custom',
        enabled: true,
        requested: true,
        supported: false,
        configured: false,
        connected: false,
        healthy: false,
      },
    ])
    expect(plan.ready).toBe(false)
  })

  it('uses only tenant-bound trusted host evidence and preserves disabled requests', () => {
    const input = config({
      integrations: [{ name: 'Postmark', complexity: 'standard', enabled: true }],
    })
    expect(resolver.plan(input, undefined, [proof]).ready).toBe(true)
    expect(resolver.plan(input, undefined, [{ ...proof, healthy: false }]).ready).toBe(false)
    expect(() => resolver.plan(input, undefined, [{ ...proof, tenantId: 'other-tenant' }])).toThrow(
      /Cross-tenant/,
    )
    const disabled = resolver.plan(
      config({ integrations: [{ name: 'Postmark', complexity: 'standard', enabled: false }] }),
      undefined,
      [proof],
    )
    expect(disabled.integrations[0]).toMatchObject({
      requested: false,
      connected: false,
      healthy: false,
    })
  })

  it('refuses impossible or duplicated evidence and tenant-authored health fields', () => {
    expect(() => resolver.plan(config(), undefined, [{ ...proof, configured: false }])).toThrow()
    expect(() => resolver.plan(config(), undefined, [proof, proof])).toThrow(/Duplicate/)
    expect(() =>
      config({ integrations: [{ name: 'Postmark', complexity: 'standard', connected: true }] }),
    ).not.toThrow()
    // Legacy nested objects strip unknown fields; stripped claims cannot reach readiness.
    expect(
      resolver.plan(
        config({ integrations: [{ name: 'Postmark', complexity: 'standard', connected: true }] }),
      ).integrations[0]?.connected,
    ).toBe(false)
  })

  it('refuses unknown names, conflicting duplicates and unregistered client extensions', () => {
    expect(() => resolver.plan(config({ schema_version: '99.0.0' }))).toThrow(
      /Unsupported client config version/,
    )
    expect(() =>
      resolver.plan(config({ modules: [{ name: 'not-a-real-module', enabled: true }] })),
    ).toThrow(/Unknown module/)
    expect(() =>
      resolver.plan(config({ modules: [{ name: 'knowlege', enabled: false }] })),
    ).toThrow(/Unknown module/)
    expect(() =>
      resolver.plan(
        config({
          modules: [
            { name: 'knowledge', enabled: true },
            { name: 'knowledge', enabled: false },
          ],
        }),
      ),
    ).toThrow(/Duplicate/)
    expect(() =>
      resolver.plan(
        config({
          integrations: [
            { name: 'Postmark', complexity: 'standard' },
            { name: 'Postmark', complexity: 'custom' },
          ],
        }),
      ),
    ).toThrow(/Duplicate/)
  })

  it('keeps planned native CRM unavailable even when requested', () => {
    const plan = resolver.plan(config({ modules: [{ name: 'crm', enabled: true }] }))
    expect(plan.modules[0]).toMatchObject({ name: 'crm', supported: false })
    expect(plan.ready).toBe(false)
  })

  it('pins dependency versions, resolves them once and refuses an explicit dependency opt-out', () => {
    const manifests: ModuleManifest[] = [
      { id: 'contacts', version: '0.1.0', status: 'implemented', requires: [], permissions: [] },
      {
        id: 'followup',
        version: '0.2.0',
        status: 'implemented',
        requires: ['contacts'],
        permissions: [],
      },
    ]
    const registry = new ModuleRegistry(manifests)
    const plan = resolver.plan(config({ modules: [{ name: 'followup', enabled: true }] }), registry)
    expect(plan.modules).toEqual([
      { name: 'contacts', version: '0.1.0', requested: false, supported: true },
      { name: 'followup', version: '0.2.0', requested: true, supported: true },
    ])
    expect(() =>
      resolver.plan(
        config({
          modules: [
            { name: 'followup', enabled: true },
            { name: 'contacts', enabled: false },
          ],
        }),
        registry,
      ),
    ).toThrow(/explicitly disabled/)
    expect(
      () =>
        new ModuleRegistry([
          { ...(manifests.at(0) as ModuleManifest), requires: ['followup'] },
          manifests.at(1) as ModuleManifest,
        ]),
    ).toThrow(/Circular/)
    expect(
      () => new ModuleRegistry([{ ...(manifests.at(0) as ModuleManifest), requires: ['missing'] }]),
    ).toThrow(/unknown module/)
    expect(
      () =>
        new ModuleRegistry([manifests.at(0) as ModuleManifest, manifests.at(0) as ModuleManifest]),
    ).toThrow(/Duplicate/)
  })

  it('covers every existing template module without calling future capabilities implemented', () => {
    const registry = new ModuleRegistry()
    const templates = new TemplateRegistry()
    for (const template of templates.listNames()) {
      for (const name of templates.get(template).modules)
        expect(() => registry.get(name)).not.toThrow()
    }
    expect(PLATFORM_MODULES.find((item) => item.id === 'crm')?.status).toBe('planned')
  })
})
