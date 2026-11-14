import test from 'node:test'
import assert from 'node:assert/strict'
import { inspect } from 'node:util'
import { liveEnvironment, options } from '../e2e/run-agent-live.mjs'
const syntheticKey = 'AIza' + 'a'.repeat(35)
test('live acceptance pins the Google endpoint and dedicated child key without changing the parent', () => {
  const inherited = { KEEP: 'parent-value' }, variables = { GEMINI_API_KEY: syntheticKey }
  const env = liveEnvironment(variables, 'gemini-2.5-flash-lite', inherited)
  assert.equal(env.GEMINI_AGENT_API_KEY, syntheticKey)
  assert.equal(env.GEMINI_AGENT_ENDPOINT, 'https://generativelanguage.googleapis.com')
  assert.equal(env.ARCHAVA_E2E_AGENT_LIVE, '1')
  assert.deepEqual(inherited, { KEEP: 'parent-value' })
  assert.equal(inherited.GEMINI_AGENT_API_KEY, undefined)
  assert.equal(env.GEMINI_AGENT_THINKING_LEVEL, '')
})
test('thinking profile is explicit and cannot alter older model protocols', () => {
  const variables = { GEMINI_API_KEY: syntheticKey }, inherited = { KEEP: 'retained' }
  assert.equal(liveEnvironment(variables, 'gemini-3-flash-preview', inherited, 'minimal').GEMINI_AGENT_THINKING_LEVEL, 'minimal')
  assert.deepEqual(inherited, { KEEP: 'retained' })
  for (const [model, level] of [['gemini-2.5-flash', 'minimal'], ['gemini-3-flash-preview', 'invented']]) assert.throws(() => liveEnvironment(variables, model, {}, level), /Thinking level/)
  const args = options(['--credentials', '/private/config.env', '--model', 'gemini-3-flash-preview', '--output', 'target/new', '--thinking-level', 'minimal'])
  assert.equal(args['--thinking-level'], 'minimal')
  assert.throws(() => options(['--credentials', '/private/config.env', '--model', 'gemini-3-flash-preview', '--output', 'target/new', '--thinking-level', '']), /empty/)
})
test('live acceptance refuses fixture endpoints and deterministic-planner suites', () => {
  assert.throws(() => liveEnvironment({ GEMINI_API_KEY: syntheticKey }, 'gemini-2.5-flash-lite', { GEMINI_AGENT_ENDPOINT: 'http://127.0.0.1:1234' }), /official Google/)
  assert.throws(() => liveEnvironment({ GEMINI_API_KEY: syntheticKey }, 'gemini-2.5-flash-lite', { ARCHAVA_E2E_AGENT_RUNTIME: '1' }), /separately/)
  assert.throws(() => liveEnvironment({ GEMINI_API_KEY: syntheticKey }, ''), /explicit/)
})
test('invalid credentials stay absent from errors and model/output selection is explicit', () => {
  const privateValue = 'synthetic-private-value-that-must-stay-redacted'
  try { liveEnvironment({ GEMINI_API_KEY: privateValue }, 'gemini-2.5-flash-lite'); assert.fail('invalid key accepted') } catch (error) {
    assert.match(error.message, /literal Gemini API key/)
    assert.ok(!inspect(error).includes(privateValue))
    assert.ok(!String(error.stack).includes(privateValue))
  }
  const args = options(['--credentials', '/private/config.env', '--model', 'gemini-2.5-flash-lite', '--output', 'target/e2e/new'])
  assert.equal(args['--output'], 'target/e2e/new')
  assert.throws(() => options(['--credentials', '/private/config.env']), /Expected/)
})
