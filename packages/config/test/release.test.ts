import { afterEach, describe, expect, it } from 'vitest'
import { createHash } from 'node:crypto'
import { mkdtemp, mkdir, writeFile, readFile, rm, symlink } from 'node:fs/promises'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import {
  releaseCatalogSchema,
  releaseImplementationHash,
  validateResolvedRelease,
  verifyInstalledRelease,
  verifyReleaseDatabase,
  type ResolvedRelease,
} from '../src/release.js'

const scratch: string[] = []
afterEach(async () => {
  await Promise.all(scratch.splice(0).map((path) => rm(path, { recursive: true, force: true })))
})
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), 'archava-profile-test-'))
  scratch.push(root)
  const directory = join(root, '@archava', 'fixture')
  await mkdir(join(directory, 'dist'), { recursive: true })
  await mkdir(join(directory, 'migrations'))
  const sql = 'SELECT 1;\n'
  await writeFile(
    join(directory, 'package.json'),
    JSON.stringify({ name: '@archava/fixture', version: '1.0.0', dependencies: {} }),
  )
  await writeFile(join(directory, 'dist/index.js'), 'export const value = 1\n')
  await writeFile(join(directory, 'migrations/001_fixture.sql'), sql)
  const catalog = releaseCatalogSchema.parse({
    schemaVersion: '1.0.0',
    id: 'archava-local-library',
    version: '1.0.0',
    host: { name: '@archava/crm-host', version: '0.3.0' },
    contracts: { clientConfig: '1.0.0', deploymentPlan: '2.0.0', hostSettings: 1 },
    packages: [
      {
        name: '@archava/fixture',
        version: '1.0.0',
        migrations: [
          { file: '001_fixture.sql', checksum: createHash('sha256').update(sql).digest('hex') },
        ],
      },
    ],
    databaseLedgers: [
      { schema: 'archava_fixture', package: '@archava/fixture', migrations: ['001_fixture'] },
    ],
    requiredAdapters: ['@archava/fixture'],
    operatorChecks: ['verify fixture'],
  })
  const profile: ResolvedRelease = {
    catalog,
    artifacts: [
      {
        name: '@archava/fixture',
        version: '1.0.0',
        tarball: 'tarballs/archava-fixture-1.0.0.tgz',
        sha256: 'a'.repeat(64),
        implementationSha256: await releaseImplementationHash(directory),
      },
    ],
  }
  return { root, directory, catalog, profile }
}
describe('supported installed release boundary', () => {
  it('accepts the exact cold-installed implementation and a matching authoritative database ledger', async () => {
    const { root, catalog, profile } = await fixture()
    await expect(verifyInstalledRelease(profile, catalog, root)).resolves.toEqual(profile)
    await expect(
      verifyReleaseDatabase(profile, {
        query: () =>
          Promise.resolve({
            rows: [
              { id: '001_fixture', checksum: catalog.packages[0]?.migrations[0]?.checksum ?? '' },
            ],
          }),
      }),
    ).resolves.toBeUndefined()
  })
  it('refuses replacing expected contract/version/schema metadata through the supplied profile', async () => {
    const { profile, catalog } = await fixture()
    const replaced = structuredClone(profile)
    replaced.catalog.host.version = '999.0.0'
    expect(() => validateResolvedRelease(replaced, catalog)).toThrow('catalog_mismatch')
    expect(() => validateResolvedRelease({ ...profile, role: 'owner' }, catalog)).toThrow(
      'invalid_profile',
    )
  })
  it('rejects mismatched artifact identities, duplicates and path traversal before filesystem access', async () => {
    const { profile, catalog } = await fixture()
    const artifact = profile.artifacts[0]
    expect(() =>
      validateResolvedRelease({ ...profile, artifacts: [artifact, artifact] }, catalog),
    ).toThrow('artifact_set_mismatch')
    expect(() =>
      validateResolvedRelease(
        { ...profile, artifacts: [{ ...artifact, version: '2.0.0' }] },
        catalog,
      ),
    ).toThrow('package_version_mismatch')
    expect(() =>
      validateResolvedRelease(
        { ...profile, artifacts: [{ ...artifact, tarball: '../../secret' }] },
        catalog,
      ),
    ).toThrow('artifact_filename_mismatch')
  })
  it('detects altered package bytes even if the declared name and version are unchanged', async () => {
    const { root, directory, profile, catalog } = await fixture()
    await writeFile(join(directory, 'dist/index.js'), 'export const value = 999\n')
    await expect(verifyInstalledRelease(profile, catalog, root)).rejects.toThrow(
      'installed_hash_mismatch',
    )
  })
  it('rejects mismatched dependency versions, changed migrations and unowned migration files', async () => {
    const { root, directory, profile, catalog } = await fixture()
    const path = join(directory, 'package.json'),
      original = await readFile(path, 'utf8')
    await writeFile(
      path,
      JSON.stringify({
        ...(JSON.parse(original) as object),
        dependencies: { '@archava/fixture': '2.0.0' },
      }),
    )
    await expect(verifyInstalledRelease(profile, catalog, root)).rejects.toThrow(
      'dependency_version_mismatch',
    )
    await writeFile(path, original)
    await writeFile(join(directory, 'migrations/002_unowned.sql'), 'SELECT 2;')
    await expect(verifyInstalledRelease(profile, catalog, root)).rejects.toThrow(
      'migration_set_mismatch',
    )
    await rm(join(directory, 'migrations/002_unowned.sql'))
    await writeFile(join(directory, 'migrations/001_fixture.sql'), 'SELECT 999;')
    await expect(verifyInstalledRelease(profile, catalog, root)).rejects.toThrow(
      'migration_file_mismatch',
    )
  })
  it('refuses symlinks instead of following package code outside the installation', async () => {
    const { root, directory, profile, catalog } = await fixture()
    await rm(join(directory, 'dist/index.js'))
    await symlink(join(directory, 'package.json'), join(directory, 'dist/index.js'))
    await expect(verifyInstalledRelease(profile, catalog, root)).rejects.toThrow(
      'unsafe_package_path',
    )
  })
  it('refuses missing, changed or additional database migrations without running DDL', async () => {
    const { profile } = await fixture()
    for (const rows of [
      [],
      [{ id: '001_fixture', checksum: 'b'.repeat(64) }],
      [
        { id: '001_fixture', checksum: profile.catalog.packages[0]?.migrations[0]?.checksum ?? '' },
        { id: '002_unowned', checksum: 'a'.repeat(64) },
      ],
    ]) {
      const seen: string[] = []
      await expect(
        verifyReleaseDatabase(profile, {
          query: (sql) => {
            seen.push(sql)
            return Promise.resolve({ rows })
          },
        }),
      ).rejects.toThrow('database_schema_mismatch')
      expect(seen).toEqual([
        'SELECT id,checksum FROM archava_fixture.schema_migrations ORDER BY id',
      ])
    }
  })
})
