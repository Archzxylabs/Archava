/** Explicit live-Gemini launcher. Credentials stay in memory and child environment. */
import assert from 'node:assert/strict'
import { readFileSync, existsSync } from 'node:fs'
import { parseEnv } from 'node:util'
import { resolve, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import { spawn } from 'node:child_process'

export function liveEnvironment(variables, model, inherited = {}, thinkingLevel) {
  assert.match(model || '', /^[A-Za-z0-9_.-]{1,120}$/, 'Choose an explicit GenerateContent model')
  const key = variables.GEMINI_AGENT_API_KEY || variables.GEMINI_API_KEY
  assert.ok(/^AIza[A-Za-z0-9_-]{35}$/.test(key || ''), 'A literal Gemini API key is required in the credential file')
  assert.ok(!inherited.GEMINI_AGENT_ENDPOINT || inherited.GEMINI_AGENT_ENDPOINT === 'https://generativelanguage.googleapis.com', 'Live acceptance requires the official Google endpoint')
  for (const flag of ['ARCHAVA_E2E_AGENT_RUNTIME', 'ARCHAVA_E2E_AGENTIC', 'ARCHAVA_E2E_SIMULATIONS']) assert.notEqual(inherited[flag], '1', 'Run live-provider acceptance separately from deterministic planner probes')
  const level = thinkingLevel ?? inherited.GEMINI_AGENT_THINKING_LEVEL ?? ''
  assert.ok(!level || model.startsWith('gemini-3') && ['minimal', 'low', 'medium', 'high'].includes(level), 'Thinking level requires a supported Gemini 3 model/profile')
  return { ...inherited, ARCHAVA_E2E_AGENT_LIVE: '1', GEMINI_AGENT_API_KEY: key, GEMINI_AGENT_MODEL: model, GEMINI_AGENT_THINKING_LEVEL: level, GEMINI_AGENT_ENDPOINT: 'https://generativelanguage.googleapis.com' }
}
export function options(args) {
  assert.ok([6, 8].includes(args.length), 'Expected --credentials FILE --model NAME --output NEW_DIRECTORY [--thinking-level LEVEL]')
  const values = {}
  for (let i = 0; i < args.length; i += 2) {
    assert.ok(['--credentials', '--model', '--output', '--thinking-level'].includes(args[i]) && !Object.hasOwn(values, args[i]) && args[i + 1], 'Unknown, empty or duplicated argument')
    values[args[i]] = args[i + 1]
  }
  assert.ok(values['--credentials'] && values['--model'] && values['--output'], 'Missing live acceptance argument')
  return values
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv.includes('--help')) {
    process.stdout.write('node tools/e2e/run-agent-live.mjs --credentials PRIVATE_ENV_FILE --model GENERATE_CONTENT_MODEL --output NEW_DIRECTORY [--thinking-level minimal|low|medium|high]\nUses the existing fresh release, owned PostgreSQL/Chromium, synthetic data and at most six live planning turns, paced twenty seconds apart. Thinking level is only for supported Gemini 3 text models. Never prints or copies the key.\n')
  } else {
    const args = options(process.argv.slice(2)), root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
    assert.ok(!existsSync(resolve(root, args['--output'])), 'Use a new evidence directory; previous results are preserved')
    const env = liveEnvironment(parseEnv(readFileSync(resolve(args['--credentials']), 'utf8')), args['--model'], process.env, args['--thinking-level'])
    const child = spawn(process.execPath, ['tools/e2e/verify.mjs', '--output', args['--output']], { cwd: root, env, stdio: 'inherit' })
    child.once('error', () => { process.stderr.write('The live acceptance runner could not start.\n'); process.exitCode = 1 })
    child.once('exit', (code, signal) => { process.exitCode = signal ? 1 : code ?? 1 })
    for (const signal of ['SIGINT', 'SIGTERM']) process.once(signal, () => child.kill(signal))
  }
}
