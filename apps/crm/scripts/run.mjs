/* global process */
import { execFileSync, spawn } from 'node:child_process'
import { existsSync } from 'node:fs'
import { resolve, join } from 'node:path'

const root = resolve(import.meta.dirname, '../../..')
const [mode, ...args] = process.argv.slice(2)
if (!['start', 'setup', 'user', 'backup', 'restore', 'verify'].includes(mode))
  throw new Error('Unknown CRM command')
const entry =
  mode === 'verify'
    ? join(import.meta.dirname, 'verify.mjs')
    : join(root, 'dist/crm-runtime', mode === 'start' ? 'server.mjs' : 'operator.mjs')
if (!existsSync(entry))
  execFileSync(process.execPath, [join(import.meta.dirname, 'build.mjs')], {
    cwd: root,
    stdio: 'inherit',
  })
const child = spawn(
  process.execPath,
  [entry, ...(['start', 'verify'].includes(mode) ? args : [mode, ...args])],
  {
    cwd: root,
    stdio: 'inherit',
    detached: process.platform !== 'win32',
  },
)
const interrupt = () => {
  if (!child.killed) child.kill('SIGINT')
}
const terminate = () => {
  if (!child.killed) child.kill('SIGTERM')
}
process.on('SIGINT', interrupt)
process.on('SIGTERM', terminate)
await new Promise((done) =>
  child.once('exit', (code) => {
    process.exitCode = code ?? 1
    done()
  }),
)
process.off('SIGINT', interrupt)
process.off('SIGTERM', terminate)
