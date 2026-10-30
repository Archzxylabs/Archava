/** Cargo dependency inventory in CycloneDX 1.5 format. Includes dev/target dependencies. */
import { execFileSync } from 'node:child_process'
import { mkdir, readFile, writeFile } from 'node:fs/promises'
import { randomUUID } from 'node:crypto'
import { dirname, join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '../..')
const args = process.argv.slice(2)
if (args.includes('--help')) {
  process.stdout.write('node tools/release/sbom.mjs [--offline] [--output dist/sbom.cdx.json]\nInventory of the resolved Cargo graph, including development and target dependencies. Not a vulnerability scan.\n')
  process.exit(0)
}
let output = join(root, 'dist/sbom.cdx.json')
let offline = false
for (let index = 0; index < args.length; index += 1) {
  if (args[index] === '--offline') offline = true
  else if (args[index] === '--output' && args[index + 1] && !args[index + 1].startsWith('--')) output = resolve(root, args[++index])
  else throw new Error(`Unknown or incomplete option: ${args[index]}`)
}
const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--format-version', '1', ...(offline ? ['--offline'] : [])], { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }))
const profile = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const components = new Map()
for (const pkg of metadata.packages) {
  const purl = `pkg:cargo/${encodeURIComponent(pkg.name)}@${encodeURIComponent(pkg.version)}`
  components.set(purl, {
    type: 'library', name: pkg.name, version: pkg.version, purl,
    ...(pkg.license ? { licenses: [{ expression: pkg.license }] } : {}),
    ...(pkg.repository ? { externalReferences: [{ type: 'vcs', url: pkg.repository }] } : {}),
  })
}
const bom = {
  bomFormat: 'CycloneDX', specVersion: '1.5', serialNumber: `urn:uuid:${randomUUID()}`, version: 1,
  metadata: {
    timestamp: new Date().toISOString(),
    tools: [{ vendor: 'Archava', name: 'tools/release/sbom.mjs', version: '2' }],
    component: {
      type: 'application', name: 'archava', version: profile.version,
      properties: [{ name: 'archava:inventory-scope', value: 'resolved Cargo graph, including dev and target dependencies' }],
    },
  },
  components: [...components.values()].sort((a, b) => a.purl.localeCompare(b.purl)),
}
await mkdir(dirname(output), { recursive: true })
await writeFile(output, `${JSON.stringify(bom, null, 2)}\n`, { mode: 0o644 })
process.stdout.write(`${JSON.stringify({ ok: true, output, components: bom.components.length, profile: profile.version })}\n`)
