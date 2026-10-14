import { describe, expect, it } from 'vitest'
import type { ManifestEvidence, ManifestSources } from '../manifest.mjs'
import { buildManifest, loadSource } from '../manifest.mjs'

/**
 * The acceptance manifest generator, tested against evidence that should refuse it.
 *
 * A manifest is only honest if it refuses to be produced from incomplete evidence, and
 * this repository's own `dist/` is healthy — so testing the generator against itself
 * could never do more than assert it is quiet. Each case here supplies one defect and
 * asserts the generator names it, which is the only evidence that its truth rules protect
 * anything at all.
 */

const PROFILE = '1.17.0'

/**
 * A complete, self-consistent evidence set. Defect cases derive from this by one mutation.
 *
 * Typed as the loose `ManifestSources` rather than its own inferred shape on purpose:
 * the generator's whole job is refusing malformed input, so the test must be able to hand
 * it malformed input. A precisely-inferred fixture would reject `kits.operationalKitsAccepted
 * = 'none'` at compile time and that case could never run.
 */
const healthy = (): ManifestSources => ({
  trusted: { version: PROFILE },
  security: {
    profile: PROFILE,
    catalogSha256: 'c'.repeat(64),
    artifactSha256: 'a'.repeat(64),
    checkedAt: '2026-10-14T00:00:00.000Z',
    node: 'v22.22.3',
    commands: [{ name: 'unit' }, { name: 'crm' }],
    controls: [
      { id: 'a', result: 'passed' },
      { id: 'b', result: 'passed' },
      { id: 'c', result: 'failed' },
    ],
    unit: { passed: 10, failed: 0 },
    audits: { platform: 0, runtime: 0, template: { counts: { moderate: 13, total: 13 } } },
    distributionScope: 'Private internal artifacts only.',
    installPolicy: 'frozen-lockfile',
    limits: { certification: false, penetrationTest: false, realtimeTemplate: 'template gate' },
  },
  artifacts: { schema_version: '1.0.0', artifacts: [{ name: 'n', version: '1.0.0' }] },
  releaseProfile: {
    catalog: { host: { version: '0.13.0' }, contracts: [], externals: [], databaseLedgers: [] },
  },
  kits: { profile: PROFILE, candidates: 40, operationalKitsAccepted: 0, checks: [1, 2, 3] },
  browser: { profile: PROFILE, checks: new Array(70).fill(0), providerCalls: 0 },
  browserMinimal: { profile: PROFILE, checks: new Array(3).fill(0), providerCalls: 0 },
  upgrade: {
    priorCommit: '345d039',
    fromProfile: '1.10.0',
    toProfile: PROFILE,
    checks: [1, 2, 3, 4],
    providerCalls: 0,
    binaryRollbackIsSchemaRollback: false,
  },
  capacity: {
    profile: PROFILE,
    checks: new Array(29).fill(0),
    providerCalls: 0,
    operationalKitsAccepted: [],
  },
  capacityHost: { profile: PROFILE, checks: new Array(6).fill(0), providerCalls: 0 },
})

const FIXED = '2026-10-14T12:00:00.000Z'

/**
 * The section of a source set that a defect case mutates.
 *
 * `noUncheckedIndexedAccess` makes an indexed read possibly-undefined, and the generator is
 * entirely about refusing malformed sources, so this asserts the fixture really carries the
 * section once and lets each case mutate it without repeating that check.
 */
const section = (sources: ManifestSources, name: string): ManifestEvidence => {
  const found = sources[name]
  if (found === undefined) throw new Error(`fixture has no ${name} section`)
  return found
}

describe('buildManifest', () => {
  it('reports only what the evidence carries', async () => {
    const manifest = await buildManifest(healthy(), { generatedAt: FIXED, generatedBy: 'test' })

    expect(manifest.profile).toBe(PROFILE)
    expect(manifest.unit).toEqual({ passed: 10, failed: 0 })
    // One control failed in the fixture, so the passed count must not round up to the total.
    expect(manifest.security.controls).toBe(3)
    expect(manifest.security.controlsPassed).toBe(2)
    expect(manifest.security.commands).toBe(2)
    expect(manifest.kits.operationalKitsAccepted).toBe(0)
    expect(manifest.inventory.artifacts).toBe(1)
    expect(manifest.upgrade).toMatchObject({ priorCommit: '345d039', toProfile: PROFILE })
    // Limits are copied, never re-typed: a dropped limit is a claim the manifest cannot support.
    expect(manifest.security.limits.certification).toBe(false)
    expect(manifest.security.limits.penetrationTest).toBe(false)
    expect(manifest.evidence.length).toBe(7)
  })

  it('is deterministic for identical evidence', async () => {
    const sources = healthy()
    const options = { generatedAt: FIXED, generatedBy: 'test' }
    expect(await buildManifest(sources, options)).toEqual(await buildManifest(sources, options))
  })

  it('refuses a source declaring a different profile', async () => {
    // The frozen 1.3.0 manifest was exactly this: real evidence, wrong profile.
    const sources = healthy()
    section(sources, 'kits').profile = '1.3.0'

    await expect(
      buildManifest(sources, { generatedAt: FIXED, generatedBy: 'test' }),
    ).rejects.toThrow(/kit-proof declares profile 1\.3\.0 but the trusted catalogue is 1\.17\.0/)
  })

  it('refuses an upgrade proof that did not target this profile', async () => {
    const sources = healthy()
    section(sources, 'upgrade').toProfile = '1.16.0'

    await expect(
      buildManifest(sources, { generatedAt: FIXED, generatedBy: 'test' }),
    ).rejects.toThrow(/upgrade-proof targets 1\.16\.0 but the accepted profile is 1\.17\.0/)
  })

  it('refuses a non-integer operational kit count rather than deriving one', async () => {
    const sources = healthy()
    section(sources, 'kits').operationalKitsAccepted = 'none'

    await expect(
      buildManifest(sources, { generatedAt: FIXED, generatedBy: 'test' }),
    ).rejects.toThrow(/operationalKitsAccepted is not an integer/)
  })

  it('refuses evidence with no commands array', async () => {
    const sources = healthy()
    delete section(sources, 'security').commands

    await expect(
      buildManifest(sources, { generatedAt: FIXED, generatedBy: 'test' }),
    ).rejects.toThrow(/security-proof has no commands array/)
  })

  it('refuses a trusted catalogue with no version', async () => {
    const sources = healthy()
    sources.trusted = {}

    await expect(
      buildManifest(sources, { generatedAt: FIXED, generatedBy: 'test' }),
    ).rejects.toThrow(/Trusted catalogue has no version/)
  })

  it('copies a nonzero operational kit count instead of trusting it blindly', async () => {
    // Copied, never derived — the manifest carries the number as recorded rather than
    // silently reinterpreting an acceptance that was decided somewhere else.
    const sources = healthy()
    section(sources, 'kits').operationalKitsAccepted = 2
    const manifest = await buildManifest(sources, { generatedAt: FIXED, generatedBy: 'test' })
    expect(manifest.kits.operationalKitsAccepted).toBe(2)
    expect(manifest.unfinished.join(' ')).toContain('40 operating kit families remain incomplete')
  })
})

describe('loadSource', () => {
  it('refuses missing evidence and names the path it expected', async () => {
    // loadSource resolves against the repository root, so the missing-source case uses a
    // path that is genuinely absent from a clean tree: a proof nobody has produced yet.
    // The message must name it rather than dropping the section.
    await expect(
      loadSource({ file: 'dist/proof-that-nobody-produced/results.json' }),
    ).rejects.toThrow(/Missing acceptance evidence dist\/proof-that-nobody-produced\/results\.json/)
  })

  it('reads a present source', async () => {
    const catalogue = await loadSource({ file: 'config/library-release.v1.json' })
    expect(catalogue.version).toBe(PROFILE)
  })
})
