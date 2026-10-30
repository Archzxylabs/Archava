import test from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync, rmSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { validateStructure } from '../check-structure.mjs'

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), 'archava-structure-test-'))
  t.after(() => rmSync(root, { recursive: true, force: true }))
  for (const name of ['Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml']) writeFileSync(join(root, name), '')
  mkdirSync(join(root, 'crates/core/src'), { recursive: true })
  const pkg = { id: 'core-id', name: 'archava-core', manifest_path: join(root, 'crates/core/Cargo.toml'), dependencies: [] }
  return { root, pkg, metadata: { packages: [pkg], workspace_members: [pkg.id] } }
}

test('core rejects an HTTP-layer dependency', (t) => {
  const { root, pkg, metadata } = fixture(t)
  metadata.packages.push({ id: 'host-id', name: 'archava-host', manifest_path: join(root, 'crates/host/Cargo.toml'), dependencies: [] })
  metadata.workspace_members.push('host-id'); pkg.dependencies.push({ name: 'archava-host', kind: null })
  assert.match(validateStructure(root, metadata).problems.join('\n'), /forbidden dependency on archava-host/)
})
test('dev-only fixtures are allowed; production fixtures are refused', (t) => {
  const { root, pkg, metadata } = fixture(t)
  metadata.packages.push({ id: 'fixture-id', name: 'archava-pg-fixture', manifest_path: join(root, 'crates/pg-fixture/Cargo.toml'), dependencies: [] }); metadata.workspace_members.push('fixture-id')
  pkg.dependencies.push({ name: 'archava-pg-fixture', kind: 'dev' }); assert.deepEqual(validateStructure(root, metadata).problems, [])
  pkg.dependencies[0].kind = null; assert.match(validateStructure(root, metadata).problems.join('\n'), /fixture is a production dependency/)
})
test('missing embedded files and a second workspace are detected', (t) => {
  const { root, metadata } = fixture(t)
  writeFileSync(join(root, 'crates/core/src/lib.rs'), 'const SQL: &str = include_str!("missing.sql");')
  mkdirSync(join(root, 'rust')); writeFileSync(join(root, 'rust/Cargo.toml'), '')
  const errors = validateStructure(root, metadata).problems.join('\n')
  assert.match(errors, /missing embedded file/); assert.match(errors, /second rust\/Cargo.toml/)
})
test('historical tools are excluded, active broken imports are detected', (t) => {
  const { root, metadata } = fixture(t)
  mkdirSync(join(root, 'tools/legacy'), { recursive: true }); writeFileSync(join(root, 'tools/legacy/old.mjs'), "import x from '@archava/old';")
  assert.deepEqual(validateStructure(root, metadata).problems, [])
  writeFileSync(join(root, 'tools/current.mjs'), "import x from './missing.mjs';")
  assert.match(validateStructure(root, metadata).problems.join('\n'), /missing import/)
})
test('the UI cannot pull the HTTP host into its WASM build', (t) => {
  const { root, metadata } = fixture(t)
  const slice = { id: 'slice-id', name: 'archava-slice', manifest_path: join(root, 'crates/slice/Cargo.toml'), dependencies: [{ name: 'archava-host', kind: null, target: null }] }
  metadata.packages.push(slice, { id: 'host-id', name: 'archava-host', manifest_path: join(root, 'crates/host/Cargo.toml'), dependencies: [] });metadata.workspace_members.push('slice-id', 'host-id')
  assert.match(validateStructure(root, metadata).problems.join('\n'), /host must be restricted/)
  slice.dependencies[0].target = 'cfg(not(target_arch = "wasm32"))'
  assert.deepEqual(validateStructure(root, metadata).problems, [])
})
test('import examples inside ordinary strings are not treated as module imports', (t) => {
  const { root, metadata } = fixture(t)
  mkdirSync(join(root, 'tools'));writeFileSync(join(root, 'tools/current.mjs'), `const example = "import x from './missing.mjs';";`)
  assert.deepEqual(validateStructure(root, metadata).problems, [])
})
