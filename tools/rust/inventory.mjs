/* global process, console */
/**
 * Generates the R00 migration inventory from the working tree.
 *
 * The counts are read from the filesystem rather than transcribed from any
 * report: the PRD snapshot claims 64 package directories against a candidate
 * that has since gained two, and a hand-typed figure would silently inherit
 * that error. Every number emitted here is measured at run time so the
 * inventory can be regenerated on any future tree and stay true.
 *
 * §11 item 3 requires the inventory to carry per-behavior item IDs and the
 * proof IDs that bind each item. The first attempt at that emitted a five-row
 * `proofs[]` ledger whose `exists`/`sha256`/`profile` fields were computed once
 * and read as truth by the release gate — the RF01 finding: an invented
 * binding that turned five self-declared booleans into a
 * `candidate-verified` verdict. That ledger is gone. The inventory now points
 * at the frozen coverage manifest (`docs/rust/required-coverage.json`) and its
 * validator (`tools/rust/coverage.mjs`), which re-opens every producing file
 * and re-hashes every byte; this file measures the tree, and the validator
 * decides coverage. Neither role is allowed to stand in for the other.
 */
import { existsSync, readFileSync, readdirSync, writeFileSync } from 'node:fs'
import { createHash } from 'node:crypto'
import { execSync } from 'node:child_process'
import { join } from 'node:path'

const root = process.cwd()
const read = (relative) => JSON.parse(readFileSync(join(root, relative), 'utf8'))
const dirs = (relative) =>
  readdirSync(join(root, relative), { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort()

/** A directory is a package when it declares one; a bare directory is noted, not counted. */
const packagesOf = (glob) =>
  dirs(glob)
    .filter((name) => existsSync(join(root, glob, name, 'package.json')))
    .map((name) => ({ root: glob, name, manifest: read(join(glob, name, 'package.json')) }))

const packages = packagesOf('packages')
const apps = packagesOf('apps')
const tools = packagesOf('tools')
const withManifest = new Set(
  [...packages, ...apps, ...tools].map((pkg) => `${pkg.root}/${pkg.name}`),
)
const bare = ['packages', 'apps', 'tools']
  .flatMap((glob) => dirs(glob).map((name) => `${glob}/${name}`))
  .filter((entry) => !withManifest.has(entry))

/**
 * The SQL ledger owners, identified by the migrations they actually own.
 *
 * A `-pg` suffix filter is *not* used as the test, because the convention has
 * already drifted: `act-storage`, `crm-act` and `provider-runtime` each own a
 * `migrations/` directory with real schema, while the `-pg` suffix would miss
 * all three. Both the true owner count and the drift are emitted so neither
 * number silently stands in for the other.
 */
const ledgerOwners = packages
  .filter((pkg) => existsSync(join(root, 'packages', pkg.name, 'migrations')))
  .map((pkg) => {
    const migrations = readdirSync(join(root, 'packages', pkg.name, 'migrations'), {
      withFileTypes: true,
    })
      .filter((entry) => entry.isFile() && entry.name.endsWith('.sql'))
      .map((entry) => entry.name)
      .sort()
    return { package: pkg.name, version: pkg.manifest.version, migrations }
  })
  .filter((ledger) => ledger.migrations.length > 0)
  .sort((left, right) => left.package.localeCompare(right.package))

/** Browser panels are the UI surface the migration must reproduce, not just APIs. */
const crmSrc = join(root, 'apps/crm/src')
const browserFiles = existsSync(crmSrc)
  ? readdirSync(crmSrc)
      .filter((name) => name.endsWith('.ts'))
      .sort()
  : []

const inventory = {
  schemaVersion: '1.0.0',
  measuredAt: new Date().toISOString(),
  baseline: {
    head: process.env.BASELINE_HEAD ?? null,
    branch: process.env.BASELINE_BRANCH ?? null,
    node: process.version,
    pnpm: process.env.BASELINE_PNPM ?? null,
    rustc: process.env.BASELINE_RUSTC ?? null,
    cargo: process.env.BASELINE_CARGO ?? null,
  } /**
   * `workspaceDirs` counts every directory under the three workspace globs;
   * `packageRoots` counts those that declare a package.json. The PRD's
   * "64 package directories" is the second number on an older tree, so both
   * are recorded with the explicit difference rather than one standing in
   * silently for the other.
   */,
  counts: {
    workspaceDirs: bare.length + withManifest.size,
    packageRoots: withManifest.size,
    packages: packages.length,
    apps: apps.length,
    tools: tools.length,
    bareDirectories: bare.length,
    ledgerOwners: ledgerOwners.length,
    sqlMigrationFiles: ledgerOwners.reduce((total, ledger) => total + ledger.migrations.length, 0),
    /** Recorded beside the true count because the suffix no longer implies ownership. */
    ledgerOwnersByPgSuffix: ledgerOwners.filter((ledger) => ledger.package.endsWith('-pg')).length,
    domainPackages: packages.filter((pkg) => !pkg.name.endsWith('-pg')).length,
  },
  bare,
  ledgers: ledgerOwners,
  browserSurface: {
    package: '@archava/crm',
    entrypoints: browserFiles,
    note: 'Panel modules under apps/crm/src; each Rust milestone must reproduce the same declared set.',
  },
  providerAdapters: {
    gemini: 'apps/crm/src/gemini.ts',
    note: 'First-party adapter to a documented HTTP/SSE protocol; no official Rust SDK exists.',
  },
  template: {
    path: 'Archava Template',
    language: 'python',
    backendEntrypoints: existsSync(join(root, 'Archava Template/backend/agent.py'))
      ? readdirSync(join(root, 'Archava Template/backend'))
          .filter((name) => name.endsWith('.py'))
          .sort()
      : [],
  },
  /**
   * The required-coverage pointer (RF01): where the completion denominator
   * lives and how it is validated. This file does NOT decide coverage — the
   * frozen manifest does, and `tools/rust/coverage.mjs` decides it by
   * re-opening producing files and re-hashing artifacts. The fields here are
   * measured (digest of the manifest file itself), so a regenerated inventory
   * on a different tree records which manifest it was measured against.
   */
  requiredCoverage: (() => {
    const manifestPath = join(root, 'docs/rust/required-coverage.json')
    const exists = existsSync(manifestPath)
    return {
      manifest: 'docs/rust/required-coverage.json',
      validator: 'tools/rust/coverage.mjs',
      evidenceDir: 'dist/rust-evidence/',
      manifestExists: exists,
      manifestSha256: exists
        ? createHash('sha256').update(readFileSync(manifestPath)).digest('hex')
        : null,
      note:
        'Scope items and their binding proofs are frozen in the manifest; the ' +
        'release gate validates them (never trusts this pointer as evidence).',
    }
  })(),
  /**
   * Candidate identity: what this tree is, sourced at generation time rather
   * than hand-typed. The release gate's coverage validator compares its own
   * live `candidateIdentity()` against the identity carried inside each proof
   * evidence file — this block is the inventory's own record of the same fact,
   * kept so a regenerated inventory on a different tree stays self-describing.
   */
  candidateIdentity: (() => {
    const consumers = existsSync(join(root, 'dist/rust-consumer-proof.json'))
      ? JSON.parse(readFileSync(join(root, 'dist/rust-consumer-proof.json'), 'utf8'))
      : null
    const profile = existsSync(join(root, 'dist/library/release-profile.json'))
      ? JSON.parse(readFileSync(join(root, 'dist/library/release-profile.json'), 'utf8'))
      : null
    const crateConsumer = consumers?.consumers.find((entry) => entry.consumer === 'crate')
    const addonConsumer = consumers?.consumers.find((entry) => entry.consumer === 'addon')

    // Baselined identity from git and toolchain — read at inventory generation
    // time, not hand-typed, so a future tree regenerates its own truth.
    const gitHead = execSync('git rev-parse HEAD', {
      cwd: root,
      stdio: ['pipe', 'pipe', 'ignore'],
      encoding: 'utf8',
    }).trim()
    const gitBranch = execSync('git rev-parse --abbrev-ref HEAD', {
      cwd: root,
      stdio: ['pipe', 'pipe', 'ignore'],
      encoding: 'utf8',
    }).trim()
    const pnpmVersion = execSync('pnpm --version', {
      stdio: ['pipe', 'pipe', 'ignore'],
      encoding: 'utf8',
    }).trim()
    const rustcVersion = execSync('rustc --version', {
      stdio: ['pipe', 'pipe', 'ignore'],
      encoding: 'utf8',
    }).trim()
    const cargoVersion = execSync('cargo --version', {
      stdio: ['pipe', 'pipe', 'ignore'],
      encoding: 'utf8',
    }).trim()

    return {
      baseline: {
        head: gitHead,
        branch: gitBranch,
        toolchain: {
          node: process.version,
          pnpm: pnpmVersion,
          rustc: rustcVersion,
          cargo: cargoVersion,
        },
      },
      releaseProfileVersion: profile?.catalog?.version ?? null,
      artifacts: {
        pricingTarballSha256:
          crateConsumer?.artifacts.find((a) => a.crate === 'archava-pricing')?.tarballSha256 ??
          null,
        ratesTarballSha256:
          crateConsumer?.artifacts.find((a) => a.crate === 'archava-rates')?.tarballSha256 ?? null,
        addonSha256: addonConsumer?.addonSha256 ?? null,
      },
    }
  })(),
}

writeFileSync(
  join(root, 'docs/rust/migration-inventory.json'),
  `${JSON.stringify(inventory, null, 2)}\n`,
)
console.log(
  `workspaceDirs=${inventory.counts.workspaceDirs} packageRoots=${inventory.counts.packageRoots} ` +
    `ledgers=${inventory.counts.ledgerOwners} (by -pg suffix: ${inventory.counts.ledgerOwnersByPgSuffix}) ` +
    `sql=${inventory.counts.sqlMigrationFiles} ` +
    `browser=${browserFiles.length} template=${inventory.template.backendEntrypoints.length}`,
)
