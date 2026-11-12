/** Current-workspace boundaries. Historical tooling under tools/legacy is excluded. */
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import {domainAssets} from './rust/domain-assets.mjs'

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
const LAYERS = {
  'archava-core': [],
  'archava-domain': ['archava-core'],
  'archava-adapters': ['archava-core', 'archava-domain'],
  'archava-context': ['archava-core'],
  'archava-pricing': [],
  'archava-rates': [],
  'archava-assembly': [],
  'archava-capacity': [],
  'archava-assistant': [],
  'archava-own': [],
  'archava-ops': ['archava-assembly'],
}

function filesIn(directory, excluded = new Set(['target', 'node_modules', 'legacy', '.git', '.local'])) {
  if (!existsSync(directory)) return []
  return readdirSync(directory, { withFileTypes: true }).flatMap((entry) => {
    const path = join(directory, entry.name)
    if (entry.isDirectory()) return excluded.has(entry.name) ? [] : filesIn(path, excluded)
    return entry.isFile() ? [path] : []
  })
}

/**
 * SQL ledgers that exist in the tree but that no installer applies.
 *
 * They are named here rather than deleted (applied SQL is append-only, and a
 * file whose history is unknown is not safe to remove) and rather than added to
 * the manifest (that would change what the Ops installer verifies and applies).
 * The gate is that this list stays explicit: a new unmounted ledger, or one of
 * these files disappearing, fails the structure check instead of drifting.
 *
 * Exported because the helper test's fixture has to materialize exactly these
 * paths: a fixture that spelled them out itself would go stale the next time
 * the list changes, which is the drift this list exists to prevent.
 */
export const UNMOUNTED_MIGRATIONS = new Set([
  'migrations/act-confirmation-pg/001_confirmation_challenges.sql',
  'migrations/act-host-pg/001_host_intents.sql',
  'migrations/act-host-pg/002_host_scope_binding.sql',
  'migrations/act-storage/001_act_attempts.sql',
  'migrations/capacity-pg/001_delivery_capacity_pools.sql',
  'migrations/crm-act/001_native_confirmations.sql',
  'migrations/universal-pg/001_universal_enterprise_ledgers.sql',
])

/**
 * Every SQL file is either applied from the manifest or explicitly unmounted,
 * every manifest entry exists, and every recorded checksum still matches.
 * Without this, a ledger can look shipped while no installer verifies it (the
 * audit found seven such files).
 */
function migrationProblems(root) {
  const problems = []
  const manifestPath = join(root, 'migrations', 'manifest.v1.json')
  if (!existsSync(manifestPath)) return ['Missing migrations/manifest.v1.json']
  const manifest = JSON.parse(readFileSync(manifestPath, 'utf8'))
  const listed = new Map()
  for (const ledger of Object.values(manifest.ledgers ?? {})) {
    for (const entry of ledger.migrations ?? []) {
      if (entry.path) listed.set(entry.path, entry.sha256)
    }
  }

  const files = []
  const migrationsRoot = join(root, 'migrations')
  for (const entry of readdirSync(migrationsRoot, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    for (const file of readdirSync(join(migrationsRoot, entry.name))) {
      if (file.endsWith('.sql')) files.push(`migrations/${entry.name}/${file}`)
    }
  }

  for (const file of files) {
    if (listed.has(file)) {
      const expected = listed.get(file)
      const actual = createHash('sha256')
        .update(readFileSync(join(root, file)))
        .digest('hex')
      if (expected && expected !== actual) {
        problems.push(`${file}: manifest checksum does not match the file`)
      }
      continue
    }
    if (!UNMOUNTED_MIGRATIONS.has(file)) {
      problems.push(
        `${file}: not in migrations/manifest.v1.json and not on the unmounted list`,
      )
    }
  }

  for (const file of listed.keys()) {
    if (!existsSync(join(root, file))) problems.push(`${file}: manifest entry has no file`)
  }
  for (const file of UNMOUNTED_MIGRATIONS) {
    if (!existsSync(join(root, file))) problems.push(`${file}: stale unmounted-list entry`)
    if (listed.has(file)) problems.push(`${file}: unmounted list and manifest disagree`)
  }
  return problems
}

/**
 * Every host source file must be declared in `extractors.rs`, because that is
 * where the crate's extractor rules live. A module that is not declared is a
 * module the rules do not cover — which is exactly how `feed.rs` kept a bare
 * `Query<` while the tripwire only read `router.rs`.
 */
function extractorCoverageProblems(root) {
  const problems = []
  const extractors = join(root, 'crates', 'host', 'src', 'extractors.rs')
  if (!existsSync(extractors)) return ['Missing crates/host/src/extractors.rs']
  const source = readFileSync(extractors, 'utf8')
  const declared = new Set(
    [...source.matchAll(/include_str!\("([^"]+)"\)/g)].map((match) => match[1]),
  )
  for (const path of filesIn(join(root, 'crates', 'host', 'src'))) {
    if (!path.endsWith('.rs')) continue
    const name = relative(join(root, 'crates', 'host', 'src'), path).split('\\').join('/')
    if (name === 'extractors.rs') continue
    if (!declared.has(name)) {
      problems.push(
        `crates/host/src/${name}: not declared in extractors.rs, so the extractor rules do not cover it`,
      )
    }
  }
  return problems
}

export function validateStructure(root, metadata) {
  const problems = []
  for (const name of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']) {
    if (!existsSync(join(root, name))) problems.push(`Missing workspace file: ${name}`)
  }
  if (existsSync(join(root, 'rust', 'Cargo.toml'))) problems.push('A second rust/Cargo.toml shadows the root workspace')
  const members = new Set(metadata.workspace_members)
  const packages = metadata.packages.filter((pkg) => members.has(pkg.id))
  const internal = new Set(packages.map((pkg) => pkg.name))
  if (packages.length === 0) problems.push('No Cargo workspace members found')
  for (const pkg of packages) {
    const manifest = relative(join(root, 'crates'), pkg.manifest_path)
    if (manifest.startsWith('..') || isAbsolute(manifest)) problems.push(`${pkg.name}: member is outside crates/`)
    for (const dep of pkg.dependencies) {
      if (!internal.has(dep.name) || dep.kind === 'dev') continue
      if (dep.name === 'archava-pg-fixture') problems.push(`${pkg.name}: PostgreSQL fixture is a production dependency`)
      const allowed = LAYERS[pkg.name]
      if (allowed && !allowed.includes(dep.name)) problems.push(`${pkg.name}: forbidden dependency on ${dep.name}`)
      if (pkg.name === 'archava-slice' && dep.name === 'archava-host' && dep.target !== 'cfg(not(target_arch = "wasm32"))') {
        problems.push('archava-slice: host must be restricted to the native server target')
      }
    }
  }
  for (const path of filesIn(join(root, 'crates'), new Set(['target', 'node_modules', '.git', '.local']))) {
    if (path.endsWith('.sql')) problems.push(`${relative(root, path)}: SQL must live under migrations/`)
    if (!path.endsWith('.rs')) continue
    const source = readFileSync(path, 'utf8')
    for (const match of source.matchAll(/(?:include_bytes|include_str)!\(\s*"([^"\n]+)"/g)) {
      if (!existsSync(resolve(dirname(path), match[1]))) problems.push(`${relative(root, path)}: missing embedded file ${match[1]}`)
    }
  }
  for (const path of filesIn(join(root, 'tools'))) {
    if (!path.endsWith('.mjs')) continue
    const source = readFileSync(path, 'utf8')
    for (const match of source.matchAll(/^\s*(?:import|export)\s+(?:[^'";]*?\s+from\s*)?['"]([^'"]+)['"]/gm)) {
      const specifier = match[1]
      if (specifier.startsWith('@archava/')) problems.push(`${relative(root, path)}: imports the removed TypeScript workspace`)
      if (specifier.startsWith('.') && !existsSync(resolve(dirname(path), specifier))) problems.push(`${relative(root, path)}: missing import ${specifier}`)
    }
  }
  problems.push(...migrationProblems(root))
  problems.push(...extractorCoverageProblems(root))
  if (existsSync(join(root,'crates/domain/Cargo.toml'))) {
    try { domainAssets(root) } catch(error) { problems.push(error.message) }
  }
  return { packages: packages.length, problems }
}

export function checkStructure(root = REPO_ROOT, offline = false) {
  const args = ['metadata', '--locked', '--no-deps', '--format-version', '1']
  if (offline) args.push('--offline')
  const metadata = JSON.parse(execFileSync('cargo', args, { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }))
  return validateStructure(root, metadata)
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const result = checkStructure(REPO_ROOT, process.argv.includes('--offline'))
    for (const problem of result.problems) process.stderr.write(`${problem}\n`)
    process.stdout.write(`Structure: ${result.packages} crates; ${result.problems.length} problem(s)\n`)
    process.exitCode = result.problems.length ? 1 : 0
  } catch (error) {
    process.stderr.write(`Structure check failed: ${error.message}\n`)
    process.exitCode = 1
  }
}
