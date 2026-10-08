/* global process */
import { build } from 'esbuild'
import { spawn } from 'node:child_process'
import { resolve } from 'node:path'

const mode = process.argv[2]
if (!['verify', 'dev', 'live-gemini'].includes(mode))
  throw new Error('Expected verify, dev or live-gemini')
const outfile = resolve('dist', `${mode}.mjs`)
await build({
  entryPoints: [`src/${mode}.ts`],
  outfile,
  bundle: true,
  packages: 'external',
  platform: 'node',
  format: 'esm',
  target: 'es2022',
})
// Plain Node ensures installed artifact resolution cannot inherit tsx workspace aliases.
const child = spawn(process.execPath, [outfile, ...process.argv.slice(3)], {
  stdio: 'inherit',
  detached: process.platform !== 'win32',
})
// Keep the wrapper alive until the reference's database cleanup finishes.
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
