/**
 * Rust-first local development entrypoint.
 *
 * This optional preview provisions a disposable PostgreSQL fixture. For an
 * existing migrated database use tools/rust/run-persistent.mjs.
 */
import { spawn } from 'node:child_process'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { execFileSync } from 'node:child_process'

if (process.argv.includes('--help')) {
  process.stdout.write('node tools/rust/dev-slice.mjs run\nBuild and run a disposable PostgreSQL preview; SLICE_PORT overrides the default port.\n')
  process.exit(0)
}

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = root
const binary = join(workspace, 'target/release/examples/slice_server')

// Always rebuild before serving: a stale release binary is worse than a slow
// local start because it makes the preview prove a different Rust tree.
execFileSync(process.execPath, [join(root, 'tools/rust/build-slice.mjs')], { stdio: 'inherit' })

process.stdout.write(
  'Rust preview: disposable PostgreSQL fixture; persistent databases use tools/rust/run-persistent.mjs.\n',
)

const child = spawn(binary, process.argv.slice(2), {
  cwd: workspace,
  stdio: 'inherit',
  env: process.env,
})

child.on('exit', (code, signal) => {
  if (signal) process.kill(process.pid, signal)
  else process.exit(code ?? 1)
})
