import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import {
  closureManifestBody,
  deriveClosureLock,
  parseLock,
  registryDependencies,
} from '../bootstrap.mjs'

/**
 * The cold-bootstrap derivation's own tests, run by `node --test`.
 *
 * ADR-R10 moved the consumer-closure fetch from a bare `cargo fetch` (a fresh
 * resolve of whatever the index said today) to a `--locked` fetch against a
 * throwaway manifest plus a lock *derived* from the consumer's committed
 * `Cargo.lock`. That makes the derivation the load-bearing half of the pin:
 * a derivation that keeps the wrong blocks, strips the wrong packages, or
 * accepts a stale lock produces a fetch that warms a different closure than
 * the proof will vendor — and does it with exit 0, which is exactly the
 * failure shape the old code had.
 *
 * So the checks here are the derivation's refusal contract and its byte
 * fidelity, tested against both a synthetic lock small enough to read and
 * the real committed consumer lock. Nothing here runs cargo: the fast gate
 * must stay cold-safe, and `pnpm rust:bootstrap` itself is where cargo's
 * acceptance of the derived lock is proven end to end.
 */

const repoRoot = join(dirname(fileURLToPath(import.meta.url)), '../../..')
const consumerManifestPath = join(repoRoot, 'examples/rust-consumers/crate/Cargo.toml')
const consumerLockPath = join(repoRoot, 'examples/rust-consumers/crate/Cargo.lock')

/** A lock small enough to reason about: two path packages, two registry ones. */
const syntheticLock = (orphan = '') => `# synthetic consumer lock
version = 4

[[package]]
name = "consumer-crate"
version = "0.0.0"
dependencies = [
 "shipped-crate",
 "serde",
 "serde_json",
]

[[package]]
name = "shipped-crate"
version = "0.1.0"
dependencies = [
 "serde",
 "serde_json",
]
${orphan}
[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://example/index"
checksum = "aaaaaaaa"

[[package]]
name = "serde_json"
version = "1.0.0"
source = "registry+https://example/index"
checksum = "bbbbbbbb"
`

const serdeDep = { name: 'serde', requirement: '1.0.0', features: [] }
const serdeJsonDep = { name: 'serde_json', requirement: '1.0.0', features: [] }

test('the manifest parser reads both dependency forms and skips path tables', () => {
  const manifest = `[package]
name = "fixture"
version = "0.0.0"

[dependencies]
serde = { version = "1.0.229", features = ["derive"] }
itoa = "1.0.18"

[dependencies.archava-pricing]
path = "deps/archava-pricing-0.1.0"

[dev-dependencies]
tempfile = { version = "3.0", default-features = false }
`
  assert.deepEqual(registryDependencies(manifest), [
    { name: 'serde', requirement: '1.0.229', features: ['derive'] },
    { name: 'itoa', requirement: '1.0.18', features: [] },
    // dev-dependencies are part of the closure: the lock records them.
    { name: 'tempfile', requirement: '3.0', features: [] },
  ])
})

test('a dependency line the parser cannot read fails closed', () => {
  const manifest = '[dependencies]\nserde = { features = ["derive"] }\n'
  assert.throws(() => registryDependencies(manifest), /cannot read a version requirement/)
})

test('parseLock reassembles the input byte for byte', () => {
  const lock = syntheticLock()
  const { header, blocks } = parseLock(lock)
  assert.equal(header + blocks.join(''), lock)
  assert.equal(blocks.length, 4)
})

test('a file with no package entries is refused as not a lock', () => {
  assert.throws(() => parseLock('# empty\nversion = 4\n'), /no \[\[package\]\] entries/)
})

test('the derived lock keeps registry blocks verbatim and strips the local ones', () => {
  const derived = deriveClosureLock(syntheticLock(), [serdeDep, serdeJsonDep])
  // Header preserved (comments and the version line both).
  assert.match(derived, /^# synthetic consumer lock\nversion = 4\n/)
  // The root is the throwaway package, depending on exactly the declared set.
  assert.match(
    derived,
    /\[\[package\]\]\nname = "archava-bootstrap-closure"\nversion = "0\.0\.0"\ndependencies = \[\n "serde",\n "serde_json",\n\]\n/,
  )
  // Local packages are gone: the throwaway cannot satisfy a path entry, and a
  // leftover one would make cargo prune the lock and refuse `--locked`.
  assert.ok(!derived.includes('consumer-crate'))
  assert.ok(!derived.includes('shipped-crate'))
  // Registry blocks — version and checksum — survive untouched.
  assert.match(
    derived,
    /name = "serde"\nversion = "1\.0\.0"\nsource = "registry\+https:\/\/example\/index"\nchecksum = "aaaaaaaa"/,
  )
  assert.match(derived, /name = "serde_json"\nversion = "1\.0\.0"\n/)
})

test('a declared dependency missing from the lock is refused as a stale pin', () => {
  const anyhowDep = { name: 'anyhow', requirement: '1.0', features: [] }
  assert.throws(
    () => deriveClosureLock(syntheticLock(), [serdeDep, serdeJsonDep, anyhowDep]),
    /declares a registry dependency "anyhow" that Cargo\.lock does not contain/,
  )
})

test('a lock entry unreachable from the consumer is refused as stale', () => {
  const orphan =
    '\n[[package]]\nname = "orphan-crate"\nversion = "1.0.0"\n' +
    'source = "registry+https://example/index"\nchecksum = "cccccccc"\n'
  assert.throws(
    () => deriveClosureLock(syntheticLock(orphan), [serdeDep, serdeJsonDep]),
    /registry package "orphan-crate" unreachable from the consumer's own dependencies/,
  )
})

test('a closure crate reachable only through a path dependency is refused by name', () => {
  // `serde_json` is in the lock only via `shipped-crate` here — the shape a
  // future shipped crate gains when it takes a third-party dependency the
  // consumer manifest does not declare. A registry-only fetch manifest cannot
  // reach it, and silently fetching a subset would starve the proof's vendor
  // step, so the refusal names the crate.
  assert.throws(
    () => deriveClosureLock(syntheticLock(), [serdeDep]),
    /"serde_json" is part of the consumer closure only through a shipped path dependency/,
  )
})

test('the throwaway manifest body carries requirements and features verbatim', () => {
  const manifest = readFileSync(consumerManifestPath, 'utf8')
  const body = closureManifestBody(registryDependencies(manifest))
  assert.equal(
    body,
    'serde = { version = "1.0.229", features = ["derive"] }\n' +
      'serde_json = { version = "1.0.154", features = ["preserve_order"] }',
  )
})

test('the real committed consumer lock derives into the pinned closure', () => {
  const manifest = readFileSync(consumerManifestPath, 'utf8')
  const lock = readFileSync(consumerLockPath, 'utf8')
  const registry = registryDependencies(manifest)
  const derived = deriveClosureLock(lock, registry)

  const { blocks } = parseLock(derived)
  const registryNames = blocks
    .filter((block) => /^source = "/m.test(block))
    .map((block) => /^name = "([^"]+)"/m.exec(block)[1])
  const originalRegistry = parseLock(lock).blocks.filter((block) => /^source = "/m.test(block))

  // Same registry set, byte-identical blocks: the fetch warms exactly what
  // the proof vendors, down to the checksums.
  assert.equal(registryNames.length, originalRegistry.length)
  for (const block of originalRegistry) assert.ok(derived.includes(block.trimEnd()))
  // The declared pair is the root; nothing else is.
  assert.match(derived, /name = "archava-bootstrap-closure"[\s\S]*?"serde",\n "serde_json",/)
  for (const local of ['consumer-crate', 'archava-pricing', 'archava-rates', 'archava-assembly'])
    assert.ok(!derived.includes(`name = "${local}"`), `${local} must be stripped`)
})
