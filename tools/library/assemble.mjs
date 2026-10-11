/* global process */
import { readFile, mkdir, writeFile, lstat, rm } from 'node:fs/promises'
import { resolve, join } from 'node:path'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'

const root = resolve(import.meta.dirname, '../..')
const args = process.argv.slice(2)
const dry = args.includes('--dry-run')
const kitMode = args.includes('--kit')
if (args.some((value) => value.startsWith('--') && value !== '--kit' && value !== '--dry-run'))
  throw new Error('Unknown assembly option')
const positional = args.filter((value) => value !== '--dry-run' && value !== '--kit')
if (positional.length !== 2)
  throw new Error('Usage: library:assemble selection.json NEW_DIRECTORY [--kit] [--dry-run]')
const library = join(root, 'dist/library')
const require = createRequire(join(root, 'dist/crm-runtime/package.json'))
const api = await import(pathToFileURL(require.resolve('@archava/config/assembly')).href)
const release = await import(pathToFileURL(require.resolve('@archava/config/release')).href)
const profile = release.resolvedReleaseSchema.parse(
  JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8')),
)
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
release.validateResolvedRelease(profile, trusted)
await release.verifyInstalledRelease(profile, trusted, join(root, 'dist/crm-runtime/node_modules'))
for (const artifact of profile.artifacts) {
  const bytes = await readFile(join(library, artifact.tarball))
  if (createHash('sha256').update(bytes).digest('hex') !== artifact.sha256)
    throw new Error('Artifact hash mismatch')
}
const raw = JSON.parse(await readFile(resolve(positional[0]), 'utf8'))
let kitPlan
if (kitMode) {
  const kits = await import(pathToFileURL(require.resolve('@archava/kits')).href)
  kitPlan = new kits.KitPlanner(trusted, api.COMPOSABLE_MODULES).plan(raw)
  if (!kitPlan.readyToAssemble || !kitPlan.assembly) {
    process.stdout.write(JSON.stringify({ kitPlan, dryRun: dry }, null, 2) + '\n')
    process.exit(2)
  }
}
const plan = api.planAssembly(
  kitPlan
    ? {
        version:
          kitPlan.assembly.accommodation !== undefined
            ? '4.0.0'
            : kitPlan.assembly.scheduling !== undefined
              ? '3.0.0'
              : kitPlan.assembly.projects !== undefined
                ? '2.0.0'
                : '1.0.0',
        profile: trusted.version,
        modules: kitPlan.assembly,
      }
    : raw,
  trusted,
)
const output = resolve(positional[1])
try {
  await lstat(output)
  throw new Error('Assembly output already exists')
} catch (error) {
  if (error.code !== 'ENOENT') throw error
}
if (!dry) {
  await mkdir(output, { mode: 0o700 })
  try {
    await writeFile(join(output, 'assembly.json'), JSON.stringify(plan, null, 2) + '\n', {
      flag: 'wx',
    })
    if (kitPlan)
      await writeFile(join(output, 'kit-plan.json'), JSON.stringify(kitPlan, null, 2) + '\n', {
        flag: 'wx',
      })
    await writeFile(join(output, 'release-profile.json'), JSON.stringify(profile, null, 2) + '\n', {
      flag: 'wx',
    })
    await writeFile(
      join(output, 'package.json'),
      JSON.stringify(
        {
          name: 'archava-local-assembly',
          private: true,
          type: 'module',
          overrides: Object.fromEntries(
            (profile.catalog.externals ?? []).map((item) => [item.name, item.version]),
          ),
          dependencies: Object.fromEntries(
            profile.artifacts.map((a) => [a.name, 'file:' + join(library, a.tarball)]),
          ),
        },
        null,
        2,
      ) + '\n',
      { flag: 'wx' },
    )
    await writeFile(
      join(output, 'entries.mjs'),
      plan.entries
        .map((item, i) => `export * as module${i} from ${JSON.stringify(item.entry)};`)
        .join('\n') + '\n',
      { flag: 'wx' },
    )
  } catch (error) {
    await rm(output, { recursive: true, force: true })
    throw error
  }
}
process.stdout.write(
  JSON.stringify({ ...plan, ...(kitPlan ? { kitPlan } : {}), dryRun: dry }, null, 2) + '\n',
)
