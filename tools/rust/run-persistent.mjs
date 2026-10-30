/**
 * Persistent Rust launcher for the root workspace.
 *
 * It deliberately requires DATABASE_URL and ARCHAVA_IDENTITY_KEY_HEX at the
 * Rust binary boundary. tools/rust/dev-slice.mjs runs the disposable preview.
 */
import { spawn } from 'node:child_process'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'


if (process.argv.includes('--help')) {
  process.stdout.write('node tools/rust/run-persistent.mjs\nBuild and run against an existing migrated database. Requires DATABASE_URL and ARCHAVA_IDENTITY_KEY_HEX.\n')
  process.exit(0)
}

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = root
const binary = join(workspace, 'target/release/archava-server')

execFileSync(process.execPath, [join(root, 'tools/rust/build-slice.mjs')], { stdio: 'inherit' })

process.stdout.write(
  'Rust persistent runtime: DATABASE_URL and ARCHAVA_IDENTITY_KEY_HEX are required; use node tools/rust/dev-slice.mjs run for the disposable fixture.\n',
)

const child = spawn(binary, process.argv.slice(2), {
  cwd: root,
  stdio: 'inherit',
  env: process.env,
})

child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal)
  else process.exit(code ?? 1)
})
