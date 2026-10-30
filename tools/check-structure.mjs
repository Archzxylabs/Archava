/** Current-workspace boundaries. Historical tooling under tools/legacy is excluded. */
import { execFileSync } from 'node:child_process'
import { existsSync, readFileSync, readdirSync } from 'node:fs'
import { dirname, isAbsolute, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

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
