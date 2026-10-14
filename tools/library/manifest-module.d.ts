/**
 * Ambient types for `manifest.mjs`, which ships as plain JavaScript alongside
 * the other `tools/library` scripts.
 *
 * They exist so the test suite can call the exported builders with real types
 * instead of `any` — an untyped import is what would otherwise turn every
 * `expect(manifest.security.controlsPassed)` into an unchecked member access,
 * and the whole point of those tests is that they check the values.
 *
 * The evidence map is deliberately an index signature: the generator's job is
 * to refuse a malformed source, so the test has to be able to hand it one.
 *
 * This is an ambient `declare module` rather than a sibling `manifest.d.mts`
 * because TypeScript resolves a `.mjs` import only through a `.d.mts`, and a
 * `.d.mts` file trips the repo's ESLint config out of its typed-rule block.
 */

declare module '*/manifest.mjs' {
  /** One loaded JSON proof, as the generator sees it before validating it. */
  export interface ManifestEvidence {
    [field: string]: unknown
  }

  /** Every source the manifest is built from, keyed the way `manifest.mjs` keys them. */
  export interface ManifestSources {
    [source: string]: ManifestEvidence
  }

  /** Limit claims carried verbatim from the security proof, never re-typed. */
  export interface ManifestLimits {
    [limit: string]: unknown
  }

  /**
   * The manifest written to `docs/audit/local-library/acceptance-<profile>.json`.
   * Every field is copied from evidence rather than derived, so the shape is a
   * record of what was read, not of what would be nice.
   */
  export interface ManifestResult {
    schemaVersion: string
    profile: string
    generatedAt: string
    generatedBy: string
    inventory: {
      artifacts: number
      catalogSha256: string
      artifactSha256: string
      catalog: {
        host: { version: string }
        contracts: unknown[]
        externals: number
        databaseLedgers: unknown[]
      }
    }
    unit: { passed: number; failed: number }
    security: {
      controls: number
      controlsPassed: number
      commands: number
      audits: ManifestEvidence
      limits: ManifestLimits
      distributionScope: string
      installPolicy: string
    }
    kits: { candidates: number; operationalKitsAccepted: number }
    upgrade: {
      priorCommit: string
      fromProfile: string
      toProfile: string
      checks: number
      providerCalls: number
      binaryRollbackIsSchemaRollback: boolean
    }
    evidence: {
      source: string
      profile: string
      checks: number
      providerCalls: number
      checkedAt: string
    }[]
    reproduce: string[]
    unfinished: string[]
  }

  /** Read one required source, or throw naming the path it expected. */
  export const loadSource: (source: { file: string }) => Promise<ManifestEvidence>

  /**
   * Build the manifest from an already-loaded source map, throwing on any source
   * that is missing, disagrees about the profile, or did not test this profile's
   * upgrade path.
   */
  export const buildManifest: (
    sources: ManifestSources,
    options: { generatedAt: string; generatedBy: string },
  ) => Promise<ManifestResult>

  /** The root script entry point: read every source, write the manifest. */
  export const main: (options?: { generatedAt?: string }) => Promise<void>
}
