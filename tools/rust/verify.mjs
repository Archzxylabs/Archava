/** Run the current Rust checks. Legacy parity/release producers are archived separately. */
import { spawnSync } from 'node:child_process'
import { resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { REPO_ROOT } from '../check-structure.mjs'

export function gateCommands({ offline = false, lib = false } = {}) {
  const cargoFlags = ['--locked', ...(offline ? ['--offline'] : [])]
  return [
    [process.execPath, 'tools/check-structure.mjs', ...(offline ? ['--offline'] : [])],
    [process.execPath, 'tools/check-docs.mjs'],
    [process.execPath, '--test', 'tools/test/structure.test.mjs', 'tools/test/docs.test.mjs', 'tools/test/verify.test.mjs'],
    ['cargo', 'fmt', '--all', '--check'],
    ['cargo', 'clippy', '--workspace', '--all-targets', ...cargoFlags, '--', '-D', 'warnings'],
    ['cargo', 'test', '--workspace', ...cargoFlags, ...(lib ? ['--lib'] : []), '--', '--test-threads=1'],
  ]
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
  const known = new Set(['--offline', '--lib', '--plan', '--help'])
  if (args.some((arg) => !known.has(arg))) {
    process.stderr.write('Unknown option. Use --help.\n'); process.exitCode = 2
  } else if (args.includes('--help')) {
    process.stdout.write('node tools/rust/verify.mjs [--offline] [--lib] [--plan]\nDefault: structure, documentation links, tool tests, formatting, Clippy, and all Rust tests.\n--lib runs library tests only; it does not certify integration or release readiness.\n')
  } else {
    const commands = gateCommands({ offline: args.includes('--offline'), lib: args.includes('--lib') })
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
