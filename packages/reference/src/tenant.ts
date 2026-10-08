/**
 * The reference tenant: one client configuration, fixed.
 *
 * PRD §32 asks for a canonical showcase that walks the whole arc — land, ask,
 * understand, compare, decide, act, checkout, confirm, receipt, support. Doing
 * that on a live PMS is not a demo, it is a dependency. So the demo tenant is
 * a config like any other, declared here once and read by every slice that
 * needs one.
 *
 * Three things this file is careful about:
 *
 * - The raw literal is the only source. Entities, knowledge, and the tenant id
 *   are read back out of the parsed config rather than restated, so a bundled
 *   unit cannot drift from the config that governs it.
 * - `isReferenceImplementation` stays true. A config that looks like a real
 *   hotel must never be mistaken for one, by the pricing engine or by an
 *   operator.
 * - Nothing here contacts a network. Rates and availability in `rates.ts` are
 *   fixtures with a stated date, read through the same `StructuredTruthPort`
 *   a live PMS would answer through (§17).
 */

import { parseClientConfig, type ClientConfig } from '@archava/config/pure'
import { referenceKnowledgeInputs } from './knowledge.js'
import { referenceUnitInputs } from './units.js'

/** The tenant every reference slice is scoped to. */
export const REFERENCE_TENANT_ID = 'client-xyz'

/**
 * The reservation window the fixtures cover.
 *
 * A stated window, not "next Tuesday": the demo has to answer "is the Deluxe
 * free on the 7th?" the same way twice, and a demo that rolls forward with the
 * calendar cannot.
 */
export const REFERENCE_WINDOW = {
  from: '2026-10-05',
  to: '2026-10-11',
} as const

/** Currency and locale the reference tenant prices and speaks in. */
export const REFERENCE_LOCALE = 'id'
export const REFERENCE_CURRENCY = 'IDR'

/**
 * The raw config, before parsing.
 *
 * Exported so a test can assert the parser still accepts what the demo ships —
 * a schema change that rejects the reference data should fail here, loudly,
 * rather than at the moment someone opens the demo.
 */
export const referenceConfigInput = {
  schema_version: '1.0.0',
  tenantId: REFERENCE_TENANT_ID,
  environment: 'commerce_booking',
  presence: 'chat',
  capability: 'act',
  region: 'ID',
  template: 'hospitality',
  branding: {
    businessName: 'Rumah Aman Resort',
    tagline: 'Panoramic valley stays',
    theme: {
      accent: '#1f6f5c',
      surface: '#f7f5f0',
      ink: '#16201d',
      radius: 'rounded',
      fontFamily: 'Inter',
    },
  },
  languages: [{ code: 'id', label: 'Bahasa Indonesia' }],
  primaryLanguage: REFERENCE_LOCALE,
  knowledgeSources: referenceKnowledgeInputs,
  modules: [
    { name: 'knowledge', enabled: true },
    { name: 'page_awareness', enabled: true },
    { name: 'recommendations', enabled: true },
    { name: 'availability', enabled: true },
    { name: 'booking', enabled: true },
  ],
  entities: referenceUnitInputs,
  support: 'priority',
  updatedAt: '2026-09-22',
  isReferenceImplementation: true,
} as const

/**
 * The parsed reference config.
 *
 * `parseClientConfig` throws on anything invalid, and that is the intended
 * behaviour at import time: a reference dataset that no longer validates is a
 * broken demo, not a degraded one.
 */
export const referenceConfig: ClientConfig = parseClientConfig(referenceConfigInput)

/**
 * The units the demo sells, straight out of the config.
 *
 * `entitySchema.kind` is a closed set (`product | service | resource | offer |
 * location`), so a stay is a `resource` — the thing the visitor occupies.
 */
export const referenceUnits = referenceConfig.entities

/** Knowledge sources, likewise read back rather than duplicated. */
export const referenceKnowledgeSources = referenceConfig.knowledgeSources

/** Look one unit up by id. `undefined` for an id the tenant does not own. */
export function findUnit(unitId: string): (typeof referenceUnits)[number] | undefined {
  return referenceUnits.find((unit) => unit.id === unitId)
}
