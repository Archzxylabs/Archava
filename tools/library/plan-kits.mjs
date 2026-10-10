/* global process */
import { readFile } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'

const root = resolve(import.meta.dirname, '../..')
const args = process.argv.slice(2)
if (args.length !== 1 || args[0].startsWith('--'))
  throw new Error('Usage: kits:plan REQUEST.json; build the local runtime first')
try {
  const runtime = join(root, 'dist/crm-runtime')
  const require = createRequire(join(runtime, 'package.json'))
  const release = await import(pathToFileURL(require.resolve('@archava/config/release')).href)
  const assembly = await import(pathToFileURL(require.resolve('@archava/config/assembly')).href)
  const kits = await import(pathToFileURL(require.resolve('@archava/kits')).href)
  const profile = JSON.parse(await readFile(join(runtime, 'release-profile.json'), 'utf8'))
  const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
  await release.verifyInstalledRelease(profile, trusted, join(runtime, 'node_modules'))
  const raw = JSON.parse(await readFile(resolve(args[0]), 'utf8'))
  // Prospective full module configuration, not a claim about private host settings.
  const plan = new kits.KitPlanner(trusted, assembly.COMPOSABLE_MODULES).plan(raw)
  process.stdout.write(JSON.stringify(plan, null, 2) + '\n')
  process.exitCode = plan.readyToAssemble ? 0 : 2
} catch (error) {
  process.stderr.write(
    `Kit plan refused: ${typeof error?.code === 'string' ? error.code : 'invalid_request_or_installation'}\n`,
  )
  process.exitCode = 1
}
