import test from 'node:test'
import assert from 'node:assert/strict'
import { gateCommands, runChecks } from '../rust/verify.mjs'

test('the default gate includes workspace integration tests and locked resolution', () => {
  const commands = gateCommands(); const tests = commands.find((c) => c[0] === 'cargo' && c[1] === 'test')
  assert.ok(tests.includes('--workspace')); assert.ok(tests.includes('--locked')); assert.ok(!tests.includes('--lib'))
  assert.ok(commands.every((c) => !c.join(' ').includes('pnpm')))
  assert.ok(commands.some((c) => c.includes('tools/check-docs.mjs')))
  assert.ok(commands.some((c) => c.includes('tools/test/docs.test.mjs')))
})
test('offline and library-only scope are explicit', () => {
  const commands = gateCommands({ offline: true, lib: true })
  for (const c of commands.filter((c) => c[0] === 'cargo' && ['test', 'clippy'].includes(c[1]))) assert.ok(c.includes('--offline'))
  assert.ok(commands.find((c) => c[1] === 'test').includes('--lib'))
})
test('wasm option includes wasm32 boundary checks', () => {
  const commands = gateCommands({ wasm: true })
  const wasmCommands = commands.filter((c) => c.includes('wasm32-unknown-unknown'))
  assert.equal(wasmCommands.length, 2)
  assert.ok(wasmCommands[0].includes('archava-core'))
  assert.ok(wasmCommands[1].includes('archava-slice'))
})
test('a failed or interrupted child stops later checks', () => {
  for (const failed of [{ status: 2 }, { status: null, signal: 'SIGTERM' }]) {
    let called = 0;const result = runChecks([['first'], ['second']], () => { called += 1; return failed })
    assert.equal(result.ok, false);assert.equal(called, 1)
  }
})
