import { describe, expect, it } from 'vitest'
import { ClientConfigResolver, TemplateRegistry, loadTemplates } from '@archava/config'
import { PricingEngine } from '@archava/pricing'
import { INDUSTRIES, parseIntake, buildConfigSet, emitClientConfig } from '../src/index.js'
import { makeIntake } from './fixtures.js'

const draft = {
  branding: {
    businessName: 'Offline client',
    theme: {
      accent: '#123456',
      surface: '#ffffff',
      ink: '#000000',
      radius: 'soft' as const,
      fontFamily: 'Inter',
    },
  },
  updatedAt: '2026-10-08',
}

describe('quoted scope and deployment plan', () => {
  it('includes every catalog industry, including education, in the quote/config path', () => {
    expect([...INDUSTRIES].sort()).toEqual(Object.keys(loadTemplates().templates).sort())
    for (const industry of INDUSTRIES) {
      const intake = parseIntake(makeIntake({ industry }))
      const set = buildConfigSet(intake, new PricingEngine())
      const selected = set.variants.find((item) => item.variant === set.selected)
      if (!selected) throw new Error('Missing selected variant')
      const config = emitClientConfig(selected.scope, draft)
      expect(config.template).toBe(industry)
      expect(new ClientConfigResolver().plan(config).config).toEqual(config)
    }
  })

  it('never adds unpriced template integrations to the resolved scope', () => {
    for (const integrations of [
      [],
      [{ name: 'Postmark', complexity: 'standard' as const, required: true }],
    ]) {
      const set = buildConfigSet(makeIntake({ integrations }), new PricingEngine())
      const selected = set.variants.find((item) => item.variant === set.selected)
      if (!selected) throw new Error('Missing selected variant')
      const plan = new ClientConfigResolver(new TemplateRegistry()).plan(
        emitClientConfig(selected.scope, draft),
      )
      expect(plan.integrations.map((item) => item.name)).toEqual(
        selected.scope.integrations.map((item) => item.name),
      )
      expect(plan.integrations.every((item) => !item.connected && !item.healthy)).toBe(true)
    }
  })
})
