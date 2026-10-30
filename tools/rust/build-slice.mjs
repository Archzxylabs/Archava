/**
 * Build the Rust product slice that the local Rust runtime serves.
 *
 * Builds the persistent HTTP server, disposable preview example, hydration
 * WASM, and browser bindgen payload from the root Cargo workspace.
 */
import { execFileSync } from 'node:child_process'
import { existsSync, mkdirSync, writeFileSync, statSync } from 'node:fs'
import { sourceManifest, sha256 } from './provenance.mjs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'

if (process.argv.includes('--help')) {
  process.stdout.write('node tools/rust/build-slice.mjs\nBuild the root Rust workspace server and site bundle in release mode. Requires pinned wasm-bindgen CLI.\n')
  process.exit(0)
}

const root = fileURLToPath(new URL('../../', import.meta.url))
const workspace = root
const wasmBindgen =
  process.env.WASM_BINDGEN ?? join(process.env.HOME ?? '', '.cargo/bin/wasm-bindgen')
const pkgDir = join(workspace, 'target/site/pkg')

function run(command, args, cwd = workspace) {
  process.stdout.write(`  $ ${command} ${args.join(' ')}\n`)
  execFileSync(command, args, { cwd, stdio: 'inherit' })
}

const sourceBefore = sourceManifest(root)
mkdirSync(pkgDir, { recursive: true })

run('cargo', ['build', '--release', '--locked', '-p', 'archava-slice', '--bin', 'archava-server'])
run('cargo', ['build', '--release', '--locked', '-p', 'archava-slice', '--example', 'slice_server'])
run('cargo', [
  'build',
  '--release',
  '--locked',
  '-p',
  'archava-slice',
  '--lib',
  '--target',
  'wasm32-unknown-unknown',
  '--features',
  'hydrate',
])

if (!existsSync(wasmBindgen)) {
  throw new Error(
    `wasm-bindgen is missing at ${wasmBindgen}; install the pinned CLI or set WASM_BINDGEN`,
  )
}

run(wasmBindgen, [
  '--out-dir',
  'target/site/pkg',
  '--out-name',
  'archava-slice',
  '--target',
  'web',
  '--no-typescript',
  'target/wasm32-unknown-unknown/release/archava_slice.wasm',
])

for (const path of [
  join(workspace, 'target/release/archava-server'),
  join(workspace, 'target/release/examples/slice_server'),
  join(pkgDir, 'archava-slice.js'),
  join(pkgDir, 'archava-slice_bg.wasm'),
]) {
  if (!existsSync(path)) throw new Error(`Rust slice build did not produce ${path}`)
}

const sourceAfter = sourceManifest(root)
if (sourceBefore.sha256 !== sourceAfter.sha256) throw new Error('Build inputs changed during compilation; rebuild before packaging')
const outputPaths = ['target/release/archava-server', 'target/release/examples/slice_server', 'target/site/pkg/archava-slice.js', 'target/site/pkg/archava-slice_bg.wasm']
const provenance = { schemaVersion:1, builtAt:new Date().toISOString(), source:sourceAfter,
  artifacts:outputPaths.map(path=>({path,sha256:sha256(join(root,path)),bytes:statSync(join(root,path)).size})) }
writeFileSync(join(root,'target/site/build-provenance.json'), JSON.stringify(provenance,null,2)+'\n')

process.stdout.write(
  'PASS Rust build: persistent server + fixture preview + SSR/WASM browser payload\n',
)
