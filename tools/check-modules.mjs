/**
 * Module registry gate. `config/modules.v1.json` declares each installed
 * business module once; this check proves every hand-wired registration point
 * agrees with that declaration, and names the exact point that does not.
 *
 * It verifies wiring, not behaviour: a module that passes here is completely
 * registered, which is a different claim from its workflow being accepted.
 */
import { existsSync, readFileSync, statSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

export const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..')
export const REGISTRY = 'config/modules.v1.json'

const SERVER = 'crates/slice/src/bin/archava_server.rs'
const FIXTURE = 'crates/pg-fixture/src/lib.rs'
const ROUTES = 'crates/host/src/routes/erp.rs'
const DISPATCH = 'crates/host/src/business.rs'
const EXTRACTORS = 'crates/host/src/extractors.rs'
const FORMS = 'config/operator-actions.v1.json'
const MANIFEST = 'migrations/manifest.v1.json'
/** Files whose route literals may serve a module's `readPath`. */
const READ_SOURCES = ['crates/host/src/routes/erp.rs', 'crates/host/src/routes/crm.rs', 'crates/host/src/erp/projections.rs']
const WORKSPACES = 'crates/adapters/src/erp/projections.rs'
const LAYERS = ['domain', 'adapters', 'host']

const STRINGS = ['id', 'label', 'namespace', 'ledger', 'source', 'suite', 'spec', 'readPath', 'uiPath', 'control']
const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
const isFile = (root, path) => existsSync(join(root, path)) && statSync(join(root, path)).isFile()
const isDir = (root, path) => existsSync(join(root, path)) && statSync(join(root, path)).isDirectory()
const text = (root, path) => (isFile(root, path) ? readFileSync(join(root, path), 'utf8') : '')
function json(root, path) {
  if (!isFile(root, path)) return null
  try { return JSON.parse(readFileSync(join(root, path), 'utf8')) } catch { return null }
}

/** Problems in the registry file itself, before any wiring is compared. */
export function registryProblems(registry) {
  if (!registry || registry.schemaVersion !== 1 || !Array.isArray(registry.modules)) return [`${REGISTRY}: expected {"schemaVersion":1,"modules":[…]}`]
  const problems = []
  const seen = { id: new Map(), namespace: new Map(), rank: new Map() }
  registry.modules.forEach((module, index) => {
    const name = typeof module?.id === 'string' ? module.id : `modules[${index}]`
    for (const field of STRINGS) if (typeof module?.[field] !== 'string' || !module[field]) problems.push(`${REGISTRY}: ${name}.${field} must be a non-empty string`)
    if (typeof module?.id === 'string' && !/^[a-z][a-z0-9]*$/.test(module.id)) problems.push(`${REGISTRY}: ${name}.id must be lower-case alphanumeric (it is a route segment)`)
    if (typeof module?.source === 'string' && !/^[a-z][a-z0-9_]*$/.test(module.source)) problems.push(`${REGISTRY}: ${name}.source must be a Rust module name`)
    if (typeof module?.namespace === 'string' && !/^archava_[a-z0-9_]+$/.test(module.namespace)) problems.push(`${REGISTRY}: ${name}.namespace must start with archava_`)
    if (module?.tab !== undefined && (typeof module.tab !== 'string' || !module.tab)) problems.push(`${REGISTRY}: ${name}.tab must be a non-empty string when present`)
    if (module?.typedDetail !== undefined && typeof module.typedDetail !== 'boolean') problems.push(`${REGISTRY}: ${name}.typedDetail must be true or false when present`)
    const agent = module?.agent
    if (!Number.isInteger(agent?.rank) || agent.rank < 1) problems.push(`${REGISTRY}: ${name}.agent.rank must be a positive integer`)
    for (const list of ['keywords', 'collections']) {
      const values = agent?.[list]
      if (!Array.isArray(values) || values.length === 0 || values.some((value) => typeof value !== 'string' || !value)) problems.push(`${REGISTRY}: ${name}.agent.${list} must be a non-empty list of strings`)
      else if (list === 'keywords' && values.some((value) => value !== value.toLowerCase())) problems.push(`${REGISTRY}: ${name}.agent.keywords must be lower-case (utterances are lower-cased before matching)`)
    }
    for (const [key, value] of [['id', module?.id], ['namespace', module?.namespace], ['rank', agent?.rank]]) {
      if (value === undefined) continue
      if (seen[key].has(value)) problems.push(`${REGISTRY}: ${name} repeats ${key} ${JSON.stringify(value)} already used by ${seen[key].get(value)}`)
      else seen[key].set(value, name)
    }
  })
  return problems
}

/**
 * Every registration point for one module: `{ point, ok, fix }`. `fix` says
 * what to add and where, so a failing gate is also the instruction.
 */
export function moduleChecks(root, module, shared = loadShared(root)) {
  const { id, namespace, ledger, source, suite, spec, readPath } = module
  const checks = []
  const add = (point, ok, fix) => checks.push({ point, ok: Boolean(ok), fix })

  const forms = shared.forms?.modules?.find((entry) => entry.id === id)
  add('form catalogue', forms && forms.namespace === namespace,
    `${FORMS}: add a module with "id":"${id}" and "namespace":"${namespace}", then run node tools/rust/domain-assets.mjs --write`)

  add('migration directory', isDir(root, `migrations/${ledger}`), `create migrations/${ledger}/ with the module's numbered .sql files`)
  const mounted = shared.ledgers.find((entry) => entry.namespace === namespace)
  const paths = (mounted?.migrations ?? []).map((migration) => migration.path)
  add('migration manifest', paths.length > 0 && paths.every((path) => path.startsWith(`migrations/${ledger}/`)),
    `${MANIFEST}: add a ledger with "namespace":"${namespace}" whose migrations live under migrations/${ledger}/ (id, path, sha256)`)
  const missing = paths.filter((path) => !shared.server.includes(`${path}"`))
  add('server embed', paths.length > 0 && missing.length === 0,
    `${SERVER}: include_bytes! ${missing.length ? missing.join(', ') : `every migrations/${ledger}/*.sql`} and register "${namespace}" for readiness`)
  const mapped = new RegExp(`"${escape(namespace)}"\\s*=>\\s*"${escape(ledger)}"`).test(shared.fixture)
    || new RegExp(`_\\s*=>\\s*"${escape(ledger)}"`).test(shared.fixture)
  add('fixture ledger', mapped, `${FIXTURE}: map "${namespace}" => "${ledger}" and apply its migrations and runtime grants`)

  for (const layer of LAYERS) {
    const file = `crates/${layer}/src/erp/${source}.rs`
    add(`${layer} source`, isFile(root, file), `create ${file}`)
    add(`${layer} declaration`, new RegExp(`^\\s*(?:pub(?:\\([a-z]+\\))?\\s+)?mod\\s+${escape(source)}\\s*;`, 'm').test(shared.mods[layer]),
      `crates/${layer}/src/erp/mod.rs: declare \`pub mod ${source};\``)
  }

  add('routes', shared.routes.includes(`"/erp/${id}/`), `${ROUTES}: mount the module's handlers under "/erp/${id}/…"`)
  add('extractor coverage', shared.extractors.includes(`include_str!("erp/${source}.rs")`),
    `${EXTRACTORS}: add ("erp/${source}.rs", include_str!("erp/${source}.rs")) so the extractor rules cover it`)
  add('agent dispatch', new RegExp(`"${escape(id)}"\\s*=>`).test(shared.dispatch),
    `${DISPATCH}: add \`"${id}" => call!(crate::erp::${source}::<command handler>)\` so approved agent plans can execute`)
  // Workspace read routes are mounted from `WorkspaceDomain::ALL`, so their
  // evidence is the domain's slug arm rather than a route literal.
  const workspace = readPath === `/erp/${id}/workspace` && new RegExp(`Self::\\w+\\s*=>\\s*"${escape(id)}"`).test(shared.workspaces)
  add('read route', shared.reads.includes(`"${readPath}"`) || workspace,
    `declare the route "${readPath}" (${READ_SOURCES.join(', ')}), add the module to WorkspaceDomain in ${WORKSPACES}, or correct readPath`)

  add('integration suite', isFile(root, `crates/host/tests/${suite}.rs`), `create crates/host/tests/${suite}.rs with positive and negative coverage`)
  add('specification', isFile(root, spec), `write ${spec}`)
  return checks
}

function loadShared(root) {
  const manifest = json(root, MANIFEST)
  return {
    forms: json(root, FORMS),
    ledgers: Array.isArray(manifest?.ledgers) ? manifest.ledgers : [],
    server: text(root, SERVER),
    fixture: text(root, FIXTURE),
    routes: text(root, ROUTES),
    extractors: text(root, EXTRACTORS),
    dispatch: text(root, DISPATCH),
    reads: READ_SOURCES.map((path) => text(root, path)).join('\n'),
    workspaces: text(root, WORKSPACES),
    mods: Object.fromEntries(LAYERS.map((layer) => [layer, text(root, `crates/${layer}/src/erp/mod.rs`)])),
  }
}

/** The whole gate: registry shape, each module's wiring, and unregistered modules. */
export function checkModules(root = REPO_ROOT) {
  const registry = json(root, REGISTRY)
  if (!registry) return { modules: [], problems: [`Missing or invalid ${REGISTRY}`] }
  const problems = registryProblems(registry)
  if (problems.length) return { modules: [], problems }
  const shared = loadShared(root)
  const modules = registry.modules.map((module) => ({ id: module.id, checks: moduleChecks(root, module, shared) }))
  for (const { id, checks } of modules) for (const check of checks) if (!check.ok) problems.push(`${id}: ${check.point} — ${check.fix}`)
  const known = new Set(registry.modules.map((module) => module.id))
  for (const entry of shared.forms?.modules ?? []) if (!known.has(entry.id)) problems.push(`${entry.id}: has forms in ${FORMS} but no descriptor in ${REGISTRY}`)
  const arms = shared.dispatch.slice(shared.dispatch.indexOf('match module {'))
  for (const match of arms.matchAll(/"([a-z][a-z0-9]*)"\s*=>\s*call!/g)) if (!known.has(match[1])) problems.push(`${match[1]}: dispatched in ${DISPATCH} but has no descriptor in ${REGISTRY}`)
  return { modules, problems }
}

function explain(root, id) {
  const registry = json(root, REGISTRY)
  const shape = registryProblems(registry)
  if (shape.length) return { ok: false, lines: shape }
  const module = registry.modules.find((entry) => entry.id === id)
  if (!module) return { ok: false, lines: [`${id}: no descriptor in ${REGISTRY}. Add one first; known modules: ${registry.modules.map((entry) => entry.id).join(', ')}`] }
  const checks = moduleChecks(root, module)
  const lines = [`${module.label} (${id}) — schema ${module.namespace}, ledger migrations/${module.ledger}/`]
  for (const check of checks) lines.push(`  ${check.ok ? 'ok     ' : 'MISSING'} ${check.point}${check.ok ? '' : `\n          ${check.fix}`}`)
  const open = checks.filter((check) => !check.ok).length
  lines.push(open ? `${open} registration point(s) missing.` : 'Completely registered. Behaviour still needs its own integration evidence.')
  return { ok: open === 0, lines }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  if (args[0] === '--help') {
    process.stdout.write('node tools/check-modules.mjs [--explain <module-id>]\nDefault: verify every module in config/modules.v1.json is completely registered.\n--explain lists each registration point for one module and what to add where it is missing.\n')
  } else if (args[0] === '--explain' && args.length === 2) {
    const result = explain(REPO_ROOT, args[1])
    process.stdout.write(`${result.lines.join('\n')}\n`)
    process.exitCode = result.ok ? 0 : 1
  } else if (args.length) {
    process.stderr.write('Unknown option. Use --help.\n'); process.exitCode = 2
  } else {
    const result = checkModules(REPO_ROOT)
    for (const problem of result.problems) process.stderr.write(`${problem}\n`)
    process.stdout.write(`Modules: ${result.modules.length} registered; ${result.problems.length} problem(s)\n`)
    process.exitCode = result.problems.length ? 1 : 0
  }
}
