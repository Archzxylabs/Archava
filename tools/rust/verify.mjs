/** Run the current Rust checks. Legacy parity/release producers are archived separately. */
import { spawnSync } from 'node:child_process'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { REPO_ROOT } from '../check-structure.mjs'

export function gateCommands({ offline = false, lib = false, wasm = false } = {}) {
  const cargoFlags = ['--locked', ...(offline ? ['--offline'] : [])]
  const commands = [
    [process.execPath, 'tools/check-structure.mjs', ...(offline ? ['--offline'] : [])],
    [process.execPath, 'tools/check-modules.mjs'],
    [process.execPath, 'tools/check-docs.mjs'],
    [process.execPath, 'tools/product/check-delivery.mjs'],
    [process.execPath, '--test', 'tools/test/structure.test.mjs', 'tools/test/modules.test.mjs', 'tools/test/docs.test.mjs', 'tools/test/verify.test.mjs', 'tools/test/native-provenance.test.mjs', 'tools/test/domain-assets.test.mjs', 'tools/test/refusal-reasons.test.mjs', 'tools/test/load-benchmark.test.mjs', 'tools/test/agent-live.test.mjs', 'tools/test/product-delivery.test.mjs'],
    ['cargo', 'fmt', '--all', '--check'],
    ['cargo', 'clippy', '--workspace', '--all-targets', ...cargoFlags, '--', '-D', 'warnings'],
  ]
  if (wasm) {
    commands.push(['cargo', 'check', '--target', 'wasm32-unknown-unknown', '-p', 'archava-core', '-p', 'archava-domain', ...cargoFlags])
    commands.push(['cargo', 'check', '--target', 'wasm32-unknown-unknown', '-p', 'archava-slice', '--lib', '--features', 'hydrate', ...cargoFlags])
  }
  commands.push(['cargo', 'test', '--workspace', ...cargoFlags, ...(lib ? ['--lib'] : []), '--', '--test-threads=1'])
  return commands
}

export function runChecks(commands, execute = (command) => spawnSync(command[0], command.slice(1), { cwd: REPO_ROOT, stdio: 'inherit' })) {
  for (const command of commands) {
    process.stdout.write(`$ ${command.join(' ')}\n`)
    const result = execute(command)
    if (result.error || result.signal || result.status !== 0) return { ok: false, command, result }
  }
  return { ok: true }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2)
  const known = new Set(['--offline', '--lib', '--wasm', '--plan', '--help'])
  if (args.some((arg) => !known.has(arg))) {
    process.stderr.write('Unknown option. Use --help.\n'); process.exitCode = 2
  } else if (args.includes('--help')) {
    process.stdout.write('node tools/rust/verify.mjs [--offline] [--lib] [--wasm] [--plan]\nDefault: structure, documentation links, tool tests, formatting, Clippy, and all Rust tests.\n--lib runs library tests only; it does not certify integration or release readiness.\n--wasm includes wasm32-unknown-unknown boundary verification for core, domain, and slice.\n')
  } else {
    const commands = gateCommands({ offline: args.includes('--offline'), lib: args.includes('--lib'), wasm: args.includes('--wasm') })
    if (args.includes('--plan')) process.stdout.write(`${JSON.stringify(commands, null, 2)}\n`)
    else {
      const result = runChecks(commands)
      if (!result.ok) {
        process.stderr.write(`Verification stopped: ${result.command.join(' ')}${result.result.signal ? ` (${result.result.signal})` : ''}\n`)
        process.exitCode = Number.isInteger(result.result.status) && result.result.status > 0 ? result.result.status : 1
      }
    }
  }
}
