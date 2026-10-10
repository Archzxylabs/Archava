import { createHash } from 'node:crypto'
import { lstat, readFile, readdir } from 'node:fs/promises'
import { join } from 'node:path'
import {
  ReleaseProfileError,
  validateResolvedRelease,
  type ReleaseCatalog,
  type ResolvedRelease,
} from './release-contracts.js'

export * from './release-contracts.js'

/** Hash only packaged implementation; refuse symlinks and unexpected material. */
export async function releaseImplementationHash(directory: string): Promise<string> {
  const digest = createHash('sha256')
  const visit = async (current: string, prefix: string) => {
    const info = await lstat(current)
    if (!info.isDirectory() || info.isSymbolicLink())
      throw new ReleaseProfileError('unsafe_package_path')
    const entries = (await readdir(current, { withFileTypes: true })).sort((a, b) =>
      a.name.localeCompare(b.name),
    )
    for (const entry of entries) {
      const name = prefix + entry.name
      if (entry.isSymbolicLink()) throw new ReleaseProfileError('unsafe_package_path')
      if (prefix === '' && !['package.json', 'dist', 'migrations'].includes(entry.name))
        throw new ReleaseProfileError('unexpected_package_file')
      const file = join(current, entry.name)
      if (entry.isDirectory()) await visit(file, name + '/')
      else if (entry.isFile()) {
        const bytes = await readFile(file)
        // npm normalizes package.json whitespace when installing a tarball.
        const content =
          name === 'package.json'
            ? JSON.stringify(JSON.parse(bytes.toString('utf8')) as unknown)
            : bytes
        digest.update(name).update('\0').update(content).update('\0')
      } else throw new ReleaseProfileError('unsafe_package_path')
    }
  }
  await visit(directory, '')
  return digest.digest('hex')
}

export async function verifyReleasePackage(
  directory: string,
  pkg: ReleaseCatalog['packages'][number],
) {
  const manifest = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8')) as {
    name?: string
    version?: string
    dependencies?: Record<string, string>
  }
  if (manifest.name !== pkg.name || manifest.version !== pkg.version)
    throw new ReleaseProfileError('installed_version_mismatch')
  let actual: string[] = []
  try {
    actual = (await readdir(join(directory, 'migrations'))).sort()
  } catch (error) {
    if (!(error instanceof Error && 'code' in error && error.code === 'ENOENT')) throw error
  }
  if (JSON.stringify(actual) !== JSON.stringify(pkg.migrations.map((item) => item.file).sort()))
    throw new ReleaseProfileError('migration_set_mismatch')
  for (const migration of pkg.migrations) {
    const info = await lstat(join(directory, 'migrations', migration.file))
    if (!info.isFile() || info.isSymbolicLink())
      throw new ReleaseProfileError('unsafe_migration_path')
    const checksum = createHash('sha256')
      .update(await readFile(join(directory, 'migrations', migration.file)))
      .digest('hex')
    if (checksum !== migration.checksum) throw new ReleaseProfileError('migration_file_mismatch')
  }
  return manifest
}

export async function verifyInstalledRelease(
  raw: unknown,
  expected: ReleaseCatalog,
  nodeModules: string,
): Promise<ResolvedRelease> {
  const profile = validateResolvedRelease(raw, expected)
  for (const pkg of profile.catalog.packages) {
    const directory = join(nodeModules, pkg.name)
    const manifest = await verifyReleasePackage(directory, pkg)
    for (const [name, version] of Object.entries(manifest.dependencies ?? {})) {
      if (!name.startsWith('@archava/')) continue
      if (profile.catalog.packages.find((item) => item.name === name)?.version !== version)
        throw new ReleaseProfileError('dependency_version_mismatch')
    }
    if (
      (await releaseImplementationHash(directory)) !==
      profile.artifacts.find((item) => item.name === pkg.name)?.implementationSha256
    )
      throw new ReleaseProfileError('installed_hash_mismatch')
  }
  return profile
}

export async function verifyReleaseDatabase(
  profile: ResolvedRelease,
  database: { query(sql: string): Promise<{ rows: { id: string; checksum: string }[] }> },
) {
  // Reparse even for callers compiled without TypeScript. Identifiers are closed and host-owned.
  const validated = validateResolvedRelease(profile, profile.catalog)
  for (const ledger of validated.catalog.databaseLedgers) {
    const rows = (
      await database.query(`SELECT id,checksum FROM ${ledger.schema}.schema_migrations ORDER BY id`)
    ).rows
    const owner = validated.catalog.packages.find((pkg) => pkg.name === ledger.package)
    const expected = ledger.migrations.map((id) => ({
      id,
      checksum: owner?.migrations.find((item) => item.file === id + '.sql')?.checksum,
    }))
    if (
      rows.length !== expected.length ||
      rows.some(
        (row, index) =>
          row.id !== expected[index]?.id || row.checksum !== expected[index]?.checksum,
      )
    )
      throw new ReleaseProfileError('database_schema_mismatch')
  }
}
