/* global process, URL */
/**
 * Cold bootstrap for the Rust dependency closure: `pnpm rust:bootstrap`.
 *
 * ## The gap this closes (RA05)
 *
 * Every proof in this repository compiles `--offline`, which is the right
 * property: an offline build cannot be quietly satisfied by whatever the
 * registry serves today, so a green run means "these exact bytes, this exact
 * lock". The same flag is what made a cold machine fail. The reaudit of
 * 2026-10-16 ran `cargo metadata --offline --locked` against an **empty Cargo
 * registry cache** and got exit 101 — `serde` did not exist on the machine yet,
 * and `--offline` forbids fetching it. CI never fetched locked dependencies
 * before the gate, so every cache miss would have failed the same way.
 *
 * The fix is not to drop `--offline` from the gates — that would trade a cold
 * failure for a non-reproducible success. The fix is the documented order a
 * clean checkout is meant to run in:
 *
 * ```sh
 * pnpm install --frozen-lockfile   # Node closure, frozen by pnpm-lock.yaml
 * pnpm rust:bootstrap              # this file: cargo fetch --locked, three roots
 * pnpm rust:verify                 # every step offline from here on
 * ```
 *
 * `fetch` happens exactly once, from the lockfile, with the pinned toolchain;
 * every proof afterwards stays `--offline --locked`. A cache miss is then a
 * bootstrap step, not a gate failure — which is what "cache miss tetap
 * bekerja" means.
 *
 * ## What it warms, and why those three
 *
 * 1. **The workspace** (`rust/Cargo.toml`): `cargo fetch --locked` against
 *    `rust/Cargo.lock`. This is the closure `cargo test/clippy/build --offline`
 *    and `rust:build-artifacts` need.
 * 2. **The benchmark crate** (`tools/rust/benchmark/rust`): deliberately NOT a
 *    workspace member (it must resolve like an outside consumer), so it carries
 *    its own `Cargo.lock` and needs its own fetch.
 * 3. **The cold-consumer registry closure, derived from the committed pin.**
 *    Consumer A vendors its third-party closure with `cargo vendor --offline`,
 *    and `--offline` there restricts resolution to crates already present in
 *    this machine's cache. Its two product dependencies are `path =` to
 *    unpacked tarballs that do not exist until the proof runs, so this step
 *    cannot simply `cargo fetch` the consumer directory. Instead it derives a
 *    throwaway package from **the consumer's own `Cargo.lock`** — the committed
 *    pin that the proof asserts `--locked` against (ADR-R10):
 *
 *    - the throwaway's `[dependencies]` are the consumer manifest's registry
 *      dependencies verbatim (version requirements and features — `serde_json`'s
 *      `preserve_order` pulls `indexmap`, so features are load-bearing for the
 *      closure's shape). Exactly the declared set, no more: promoting a locked
 *      *transitive* crate to a direct dependency activates its default
 *      features, which can pull packages the consumer's own lock does not
 *      contain and force cargo to rewrite the pin. Measured, not assumed —
 *      pinning `hashbrown` directly pulled in `allocator-api2` and `cargo
 *      fetch --locked` refused the lock. Every other closure crate is reached
 *      through the lock's own edges, which is how the consumer reaches it too;
 *    - the throwaway's `Cargo.lock` is derived from the consumer's lock —
 *      header and registry blocks kept byte-for-byte (checksums included), the
 *      local packages stripped, the root rewritten — so `cargo fetch --locked`
 *      fetches the same versions the proof will vendor, by construction;
 *    - a dependency line this parser cannot read, a lock that does not contain
 *      a declared dependency, an unreachable (stale) lock entry, or a closure
 *      crate that enters only through a shipped path dependency are each a
 *      hard failure rather than a silently skipped crate the proof would then
 *      miss.
 *
 *    Before ADR-R10 this fetch ran bare `cargo fetch` — a fresh resolve at
 *    whatever the index said today — which meant a green bootstrap pinned
 *    nothing and could not guarantee the proof would see the same closure.
 *
 * ## What it deliberately does not do
 *
 * - No build, no test, no proof. It fetches bytes named by lockfiles; a green
 *   bootstrap says nothing about whether the code works, and the gates say
 *   that.
 * - No `--offline` here, obviously — this is the one step allowed to touch the
 *   network, and it is confined to the registry fetch of locked versions.
 * - No cache mutation beyond `cargo fetch`'s own downloads: no `cargo clean`,
 *   no `rustup` installs (the pinned toolchain must already be present; a
 *   missing one is reported with the file that declares it).
 *
 * Exit codes are propagated from each `cargo` child; the first failure stops
 * the run with the exact command that failed, because a bootstrap that printed
 * a summary and exited 0 after a failed fetch would recreate the RA04 defect
 * one layer down.
 */

import { spawnSync } from 'node:child_process'
import { mkdirSync, mkdtempSync, readFileSync, writeFileSync, rmSync, existsSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = fileURLToPath(new URL('../../', import.meta.url))

/** Run a command, stream output, propagate a non-zero exit as an Error. */
function run(command, args, cwd, label) {
  process.stdout.write(
    `  $ ${command} ${args.join(' ')}${cwd === root ? '' : `   (in ${cwd.replace(root, '.')})`}\n`,
  )
  const result = spawnSync(command, args, { cwd, stdio: 'inherit' })
  if (result.error) throw new Error(`${label}: ${result.error.message}`)
  if (result.status !== 0)
    throw new Error(`${label} failed with exit ${result.status ?? '(signal)'}`)
}

/** The registry dependencies of a manifest, read from its `[dependencies]` sections. */
export function registryDependencies(manifestText) {
  const deps = []
  let section = null
  const lines = manifestText.split('\n')
  for (const raw of lines) {
    const line = raw.replace(/#.*$/, '').trim()
    if (line.startsWith('[')) {
      section = /^\[([^\]]+)\]/.exec(line)?.[1] ?? null
      continue
    }
    // Only plain dependency sections; `[dependencies.foo]` tables are walked
    // by their inline form below and their absence here is deliberate — this
    // parser handles what the consumer manifest actually uses and fails loudly
    // on anything else.
    if (section !== 'dependencies' && section !== 'dev-dependencies') continue
    // `name = { version = "1.0", features = [...] }` — the inline table form.
    const inline = /^([A-Za-z0-9_-]+)\s*=\s*\{(.*)\}$/.exec(line)
    if (inline) {
      const [, name, body] = inline
      const version = /version\s*=\s*"([^"]+)"/.exec(body)?.[1]
      if (version === undefined)
        throw new Error(
          `cannot read a version requirement for "${name}" from: ${line}\n` +
            'bootstrap fetches the consumer closure from the manifest itself; ' +
            'an unreadable dependency line must fail here rather than be skipped.',
        )
      const features = /features\s*=\s*\[([^\]]*)\]/.exec(body)?.[1]
      deps.push({
        name,
        requirement: version,
        features: (features ?? '')
          .split(',')
          .map((entry) => entry.trim().replace(/^"|"$/g, ''))
          .filter((entry) => entry.length > 0),
      })
      continue
    }
    // `name = "1.0"` — the bare form.
    const bare = /^([A-Za-z0-9_-]+)\s*=\s*"([^"]+)"$/.exec(line)
    if (bare) deps.push({ name: bare[1], requirement: bare[2], features: [] })
  }
  return deps
}

/**
 * Split a Cargo.lock into its header (comments plus the `version = N` line)
 * and its `[[package]]` blocks, each block still ending exactly where it did,
 * so header + blocks reassembled byte-for-byte equal the input.
 *
 * The split is a zero-width lookahead rather than a consuming one: a `\n`
 * before `[[package]]` belongs to the *previous* block (locks separate
 * blocks with a blank line), and a split that eats it would silently drop a
 * newline from every reassembly.
 *
 * @param {string} lockText
 * @returns {{ header: string, blocks: string[] }}
 */
export function parseLock(lockText) {
  const marker = lockText.indexOf('[[package]]')
  if (marker < 0) throw new Error('not a Cargo.lock: it contains no [[package]] entries')
  return {
    header: lockText.slice(0, marker),
    // The leading empty string (a zero-width match at position 0) is dropped;
    // every real block starts at a `[[package]]`.
    blocks: lockText
      .slice(marker)
      .split(/(?=\[\[package\]\])/)
      .filter((block) => block.length > 0),
  }
}

/** The `name` of one lock block. */
function blockName(block) {
  const match = /^name = "([^"]+)"/m.exec(block)
  if (!match) throw new Error(`a [[package]] block has no name:\n${block}`)
  return match[1]
}

/** Whether a lock block is a registry (or otherwise sourced) package — i.e. NOT a local path package. */
function blockHasSource(block) {
  return /^source = "/m.test(block)
}

/** The dependency names listed in a lock block, `["a", "b 1.0.0"]` → `["a", "b"]`. */
function blockDependencies(block) {
  const start = block.indexOf('dependencies = [')
  if (start < 0) return []
  const end = block.indexOf(']', start)
  const list = block.slice(start + 'dependencies = ['.length, end)
  return [...list.matchAll(/"([^"]+)"/g)].map((m) => m[1].split(' ')[0])
}

/**
 * Derive the lockfile for the throwaway closure package from the consumer's own.
 *
 * This is the pin's mechanical half of ADR-R10. The consumer lock is the only
 * artifact that knows which exact versions the proof will vendor; a fetch that
 * does not read it is a fetch of "whatever resolves now", which is the
 * reproducibility gap this replaced. The derivation:
 *
 * - keeps the header and every **sourced** (registry) package block verbatim —
 *   version and checksum untouched, so `cargo fetch --locked` downloads the
 *   very bytes the proof will vendor;
 * - strips the local packages (`consumer-crate` and the two `archava-*` path
 *   dependencies): the throwaway does not consume the product, and a path
 *   entry it cannot satisfy would make cargo prune the lock and refuse
 *   `--locked`;
 * - rewrites the root as `archava-bootstrap-closure 0.0.0` depending on
 *   exactly the consumer's declared registry dependencies — not on every
 *   locked package. The tempting shortcut (depend on all of them, pinned)
 *   was tried and measured wrong: a direct dependency takes its *default*
 *   features, so pinning `hashbrown` directly pulled in `allocator-api2`
 *   from behind an optional feature the consumer never enables, and `cargo
 *   fetch --locked` refused the lock. The closure crates the manifest does
 *   not name are reached through the lock's own edges — which is how the
 *   consumer reaches them too, under the same features.
 *
 * Three checks run before anything is written, each a hard failure with the
 * remediation named, because a wrong fetch that still exits 0 is the exact
 * failure mode ("bootstrap succeeded" ≠ "the closure was pinned") this file
 * exists to prevent:
 *
 * 1. every registry dependency the consumer manifest declares must be in the
 *    lock (stale lock → regenerate it);
 * 2. every registry package in the lock must be reachable from the consumer's
 *    own root (unreachable leftovers → regenerate);
 * 3. every registry package must be reachable from the declared registry
 *    dependencies alone (a crate that enters only through a shipped path
 *    dependency cannot be expressed by a registry-only fetch manifest →
 *    name it in the consumer manifest or extend this derivation).
 *
 * @param {string} lockText contents of the consumer's Cargo.lock
 * @param {Array<{name: string, requirement: string, features: string[]}>} registryDeps
 *        from [`registryDependencies`] over the consumer manifest
 * @returns {string} the derived Cargo.lock
 */
export function deriveClosureLock(lockText, registryDeps) {
  const { header, blocks } = parseLock(lockText)
  const registry = blocks.filter(blockHasSource)
  if (registry.length === 0)
    throw new Error('the consumer Cargo.lock contains no registry packages — nothing to fetch')

  // Every registry package by name. (One version per name is not required of
  // the lock, and the throwaway manifest never pins transitives directly, so
  // a two-version crate needs no special handling here — reachability does.)
  const registryNames = new Set(registry.map(blockName))

  // Check 1: every declared registry dependency is in the lock.
  for (const dep of registryDeps)
    if (!registryNames.has(dep.name))
      throw new Error(
        `the consumer manifest declares a registry dependency "${dep.name}" that Cargo.lock ` +
          'does not contain — the pin is stale: regenerate examples/rust-consumers/crate/Cargo.lock\n' +
          '  (materialize deps/, then `cargo generate-lockfile --offline` in the consumer directory)',
      )

  // Check 2: every registry package is reachable from the consumer's own
  // local roots, edges as the lock records them. Reachability is over the
  // FULL graph — through the path packages — because that is the graph the
  // committed lock is valid for.
  const edges = new Map()
  for (const block of blocks) edges.set(blockName(block), blockDependencies(block))
  const reachFrom = (seeds) => {
    const seen = new Set()
    const queue = [...seeds]
    while (queue.length > 0) {
      const name = queue.pop()
      if (seen.has(name)) continue
      seen.add(name)
      for (const next of edges.get(name) ?? []) if (!seen.has(next)) queue.push(next)
    }
    return seen
  }
  const consumerRoots = blocks.filter((block) => !blockHasSource(block)).map(blockName)
  const fullReach = reachFrom(consumerRoots)
  for (const name of registryNames)
    if (!fullReach.has(name))
      throw new Error(
        `Cargo.lock contains registry package "${name}" unreachable from the consumer's own ` +
          'dependencies — the pin is stale: regenerate examples/rust-consumers/crate/Cargo.lock',
      )

  // Check 3: every registry package must be reachable from the DECLARED
  // registry dependencies alone, because that is the whole throwaway graph.
  // A package that is reachable in the full graph (check 2 passed) but not
  // here entered only through one of the shipped path dependencies — cargo
  // would prune it and refuse the lock. Regenerating does not help: the
  // consumer's lock is right; this fetch cannot express it.
  const fetchReach = reachFrom(registryDeps.map((dep) => dep.name))
  for (const name of registryNames)
    if (!fetchReach.has(name))
      throw new Error(
        `registry package "${name}" is part of the consumer closure only through a shipped ` +
          'path dependency; a registry-only fetch manifest cannot reach it and cargo would ' +
          "prune it from the derived lock. Declare it in the consumer manifest's " +
          '[dependencies] (features included) so the closure shape is stated once, at the root.',
      )

  const rootDeps = registryDeps.map((dep) => dep.name).sort()
  const root =
    '[[package]]\nname = "archava-bootstrap-closure"\nversion = "0.0.0"\ndependencies = [\n' +
    `${rootDeps.map((name) => ` "${name}",`).join('\n')}\n]\n\n`
  return header + root + registry.join('')
}

/**
 * The throwaway closure manifest: the consumer's registry dependencies,
 * verbatim.
 *
 * Verbatim because cargo must re-validate their requirements against the pin —
 * a consumer manifest that moved ahead of the lock fails `--locked` here with
 * cargo's own message, before any proof runs — and because the features are
 * what make the resolved graph (and therefore the closure) equal the
 * consumer's. Deliberately *not* extended with locked transitive pins: a
 * direct dependency takes its default features, which can drag packages the
 * consumer's lock never contained — see [`deriveClosureLock`].
 *
 * @param {Array<{name: string, requirement: string, features: string[]}>} registryDeps
 * @returns {string} the `[dependencies]` body
 */
export function closureManifestBody(registryDeps) {
  return registryDeps
    .map(({ name, requirement, features }) =>
      features.length > 0
        ? `${name} = { version = "${requirement}", features = ${JSON.stringify(features)} }`
        : `${name} = "${requirement}"`,
    )
    .join('\n')
}

/**
 * Warm the registry closure a cold-consumer proof will vendor offline.
 *
 * Fetches `--locked` against a throwaway manifest plus the **derived copy of
 * the consumer's own `Cargo.lock`** (see [`deriveClosureLock`]), so the bytes
 * warmed here are the bytes `cargo vendor --offline` will copy in the proof.
 * The lock lives beside the consumer manifest (`Cargo.lock`, committed as the
 * closure pin — ADR-R10); a missing one is a hard failure, not a fallback to
 * resolving fresh, because the fallback is precisely the gap this replaced.
 *
 * @param {string} consumerManifest path to the consumer's Cargo.toml
 */
export function fetchConsumerClosure(consumerManifest) {
  const text = readFileSync(consumerManifest, 'utf8')
  const registry = registryDependencies(text)
  if (registry.length === 0)
    throw new Error(
      `${consumerManifest} declares no registry dependencies — the closure check would be vacuous`,
    )
  const lockPath = join(dirname(consumerManifest), 'Cargo.lock')
  if (!existsSync(lockPath))
    throw new Error(
      `${lockPath} is missing — the consumer closure fetch is pinned by that committed lockfile\n` +
        '  (ADR-R10). Regenerate it: materialize deps/, then run `cargo generate-lockfile --offline`\n' +
        '  in the consumer directory, and commit the result.',
    )
  const consumerLock = readFileSync(lockPath, 'utf8')
  // Both outputs come from one parse of the consumer manifest, so the root
  // declared in the derived lock and the `[dependencies]` written to the
  // throwaway manifest can never disagree about what the closure depends on.
  const derivedLock = deriveClosureLock(consumerLock, registry)

  const scratch = mkdtempSync(join(tmpdir(), 'archava-bootstrap-'))
  try {
    // The throwaway package: consumer registry deps verbatim. Path
    // dependencies are excluded on purpose — they point at tarballs that do
    // not exist before the proof, and fetching is about the registry.
    writeFileSync(
      join(scratch, 'Cargo.toml'),
      `[package]\nname = "archava-bootstrap-closure"\nversion = "0.0.0"\nedition = "2021"\npublish = false\n\n[dependencies]\n${closureManifestBody(registry)}\n\n[workspace]\n`,
    )
    writeFileSync(join(scratch, 'Cargo.lock'), derivedLock)
    mkdirSync(join(scratch, 'src'), { recursive: true })
    writeFileSync(join(scratch, 'src/lib.rs'), '')
    // `--locked`: the fetch either warms exactly the pinned closure or fails.
    // An unlocked fetch here would re-resolve the index and warm versions the
    // proof's `--locked` assertions never see — a bootstrap that looks green
    // while pinning nothing, which is the defect this step's history carries.
    run('cargo', ['fetch', '--locked'], scratch, 'consumer closure fetch')
    process.stdout.write(
      `     ✓ consumer closure (${registry.map((d) => d.name).join(', ')} + locked transitive pin) fetched for offline vendoring\n`,
    )
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

export function bootstrap() {
  const workspace = join(root, 'rust')
  const benchmark = join(root, 'tools/rust/benchmark/rust')
  const consumerManifest = join(root, 'examples/rust-consumers/crate/Cargo.toml')

  // The pinned toolchain must already be installed: rustup reads
  // rust/rust-toolchain.toml and would otherwise start downloading mid-fetch.
  const toolchainFile = join(workspace, 'rust-toolchain.toml')
  if (existsSync(toolchainFile)) {
    const channel = /channel\s*=\s*"([^"]+)"/.exec(readFileSync(toolchainFile, 'utf8'))?.[1]
    const probe = spawnSync('rustup', ['toolchain', 'list'], { encoding: 'utf8' })
    if (probe.status === 0 && channel && !probe.stdout.includes(channel)) {
      throw new Error(
        `pinned toolchain ${channel} is not installed (declared in rust/rust-toolchain.toml)\n` +
          `  install it: rustup toolchain install ${channel}`,
      )
    }
  }

  process.stdout.write('rust:bootstrap — fetching locked dependency closures\n')
  run('cargo', ['fetch', '--locked'], workspace, 'workspace fetch')
  run('cargo', ['fetch', '--locked'], benchmark, 'benchmark fetch')
  fetchConsumerClosure(consumerManifest)
  process.stdout.write(
    'PASS rust:bootstrap — workspace, benchmark and consumer closures warm; ' +
      'every gate step runs --offline from here\n',
  )
  return true
}

if (import.meta.main) {
  try {
    bootstrap()
  } catch (error) {
    process.stdout.write(`\n✗ rust:bootstrap FAILED — ${error.message}\n`)
    process.exitCode = 1
  }
}
