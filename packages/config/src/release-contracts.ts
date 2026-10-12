import { z } from 'zod'

const version = z.string().regex(/^\d+\.\d+\.\d+$/)
const digest = z.string().regex(/^[a-f0-9]{64}$/)
const packageName = z.string().regex(/^@archava\/[a-z][a-z0-9-]*$/)
const migrationFile = z.string().regex(/^\d{3}_[a-z0-9_]+\.sql$/)
const migration = z.object({ file: migrationFile, checksum: digest }).strict()
const entry = z.object({ name: packageName, version, migrations: z.array(migration) }).strict()
const ledger = z
  .object({
    schema: z.string().regex(/^archava_[a-z_]+$/),
    package: packageName,
    migrations: z.array(z.string().regex(/^\d{3}_[a-z0-9_]+$/)).min(1),
  })
  .strict()

/** Host-owned metadata, never a permission, price or tenant-authentication token. */
export const releaseCatalogSchema = z
  .object({
    schemaVersion: z.literal('1.0.0'),
    id: z.literal('archava-local-library'),
    version,
    host: z.object({ name: z.literal('@archava/crm-host'), version }).strict(),
    contracts: z
      .object({
        clientConfig: z.literal('1.0.0'),
        deploymentPlan: z.literal('2.0.0'),
        hostSettings: z.literal(1),
        assembly: z
          .enum([
            '1.0.0',
            '2.0.0',
            '3.0.0',
            '4.0.0',
            '5.0.0',
            '6.0.0',
            '7.0.0',
            '8.0.0',
            '9.0.0',
            '10.0.0',
            '11.0.0',
            '12.0.0',
          ])
          .optional(),
        presence: z.literal('1.0.0').optional(),
        kitPlan: z.literal('1.0.0').optional(),
        kitCatalogue: z.literal('1.0.0').optional(),
      })
      .strict(),
    packages: z.array(entry).min(1).max(64),
    externals: z
      .array(
        z.object({ name: z.string().regex(/^(?:@[a-z0-9-]+\/)?[a-z0-9-]+$/), version }).strict(),
      )
      .min(1)
      .max(128)
      .optional(),
    databaseLedgers: z.array(ledger).min(1).max(32),
    requiredAdapters: z.array(packageName).min(1),
    operatorChecks: z.array(z.string().min(1).max(120)).min(1),
  })
  .strict()
  .superRefine((catalog, context) => {
    const names = catalog.packages.map((item) => item.name)
    if (
      catalog.externals &&
      new Set(catalog.externals.map((item) => item.name)).size !== catalog.externals.length
    )
      context.addIssue({ code: 'custom', message: 'Duplicate external package' })
    if (new Set(names).size !== names.length)
      context.addIssue({ code: 'custom', message: 'Duplicate release package' })
    for (const item of catalog.packages)
      if (new Set(item.migrations.map((m) => m.file)).size !== item.migrations.length)
        context.addIssue({ code: 'custom', message: 'Duplicate release migration' })
    if (
      new Set(catalog.databaseLedgers.map((item) => item.schema)).size !==
      catalog.databaseLedgers.length
    )
      context.addIssue({ code: 'custom', message: 'Duplicate database ledger' })
    for (const item of catalog.databaseLedgers) {
      if (new Set(item.migrations).size !== item.migrations.length)
        context.addIssue({ code: 'custom', message: 'Duplicate ledger migration' })
      const owner = catalog.packages.find((p) => p.name === item.package)
      if (
        !owner ||
        item.migrations.some((id) => !owner.migrations.some((m) => m.file === id + '.sql'))
      )
        context.addIssue({ code: 'custom', message: 'Unowned database migration' })
    }
    if (catalog.requiredAdapters.some((name) => !names.includes(name)))
      context.addIssue({ code: 'custom', message: 'Missing required release adapter' })
    if (new Set(catalog.requiredAdapters).size !== catalog.requiredAdapters.length)
      context.addIssue({ code: 'custom', message: 'Duplicate release adapter' })
  })

export const resolvedReleaseSchema = z
  .object({
    catalog: releaseCatalogSchema,
    artifacts: z
      .array(
        z
          .object({
            name: packageName,
            version,
            tarball: z.string().min(1),
            sha256: digest,
            implementationSha256: digest,
          })
          .strict(),
      )
      .min(1)
      .max(64),
  })
  .strict()

export type ReleaseCatalog = z.infer<typeof releaseCatalogSchema>
export type ResolvedRelease = z.infer<typeof resolvedReleaseSchema>
export class ReleaseProfileError extends Error {
  readonly code: string
  constructor(code: string) {
    super(`Unsupported local release: ${code}`)
    this.code = code
  }
}

/** Compare untrusted material to a separately supplied, host-owned catalog. */
export function validateResolvedRelease(raw: unknown, expected: ReleaseCatalog): ResolvedRelease {
  const parsed = resolvedReleaseSchema.safeParse(raw)
  if (!parsed.success) throw new ReleaseProfileError('invalid_profile')
  const profile = parsed.data
  if (JSON.stringify(profile.catalog) !== JSON.stringify(releaseCatalogSchema.parse(expected)))
    throw new ReleaseProfileError('catalog_mismatch')
  const names = profile.artifacts.map((item) => item.name)
  if (names.length !== expected.packages.length || new Set(names).size !== names.length)
    throw new ReleaseProfileError('artifact_set_mismatch')
  for (const pkg of expected.packages) {
    const artifact = profile.artifacts.find((item) => item.name === pkg.name)
    if (!artifact || artifact.version !== pkg.version)
      throw new ReleaseProfileError('package_version_mismatch')
    if (
      artifact.tarball !==
      `tarballs/archava-${pkg.name.slice('@archava/'.length)}-${pkg.version}.tgz`
    )
      throw new ReleaseProfileError('artifact_filename_mismatch')
  }
  return profile
}
