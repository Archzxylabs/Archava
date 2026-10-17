/* global process */
/**
 * The ERP module consumers run (R06-TWO-CONSUMERS): two independently
 * installed consumers of the shipped ERP modules.
 *
 * ## The claim, stated before the code
 *
 * Two product verticals — hospitality and professional-services — each
 * `npm install` the COMPILED @archava library out of tarballs (never from
 * workspace source), typecheck/build/verify against the installed surface,
 * and then consume the two accepted R06 modules — `@archava/purchasing` and
 * `@archava/foodservice` with their `*-pg` ledgers — through real batteries:
 * the frozen migration ledgers, both command grammars' refusals, and the
 * service calls whose validation runs before any client query.
 *
 * It mirrors `tools/crm-reference/src/consumers.ts` in exactly one direction:
 * the packaging mechanics (`pnpm library:pack` → tarballs → scratch installs
 * → resolved-from-node_modules assertions → per-consumer implementation
 * hashes must MATCH across the pair). What it does NOT mirror is the browser
 * proof harness: these consumers ship a metafile with their own build, and
 * `verify.mjs` grades the isolation rather than a hosted browser run.
 *
 * ## What "independently installed" means here
 *
 * The same rule the R02 cold pair uses: no workspace/path dependency in any
 * consumer (`verify.mjs` refuses server `*-pg` module leaks from the browser
 * bundle; the driver refuses a `resolve()` that answers inside `/packages/`),
 * no `node:`-importing bundle, and the installed implementations hash the
 * same in both consumers — so the pair cannot pass while desynchronized.
 */

import { execFileSync, spawnSync } from 'node:child_process'
import { cp, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { resolve, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { createHash } from 'node:crypto'

// pnpm re-exports the invoking user's npm configuration into every script it
// runs; npm refuses project-scoped installs when the policy is in
// `npm_config_allow_scripts`. The installs below pass `--ignore-scripts`, so
// the inert policy is dropped for this process (same rule as
// tools/crm-reference/src/consumers.ts; see its header for the full story).
delete process.env['npm_config_allow_scripts']

const root = resolve(import.meta.dirname, '../../..')
const outputPath = join(root, 'dist', 'rust-erp-module-consumers-proof.json')
const consumers = ['hospitality', 'professional-services']

/** The modules whose installed implementations the proof hashes. Every name
 * must exist in the pack's `artifacts.json`; `@archava/config` is the release
 * profile carrier and the ERP modules are the accepted R06 pair. */
const hashedModules = [
  '@archava/purchasing',
  '@archava/purchasing-pg',
  '@archava/foodservice',
  '@archava/foodservice-pg',
  '@archava/config',
]

/** sha256 over the installed implementation files, recursively, in a stable
 * order — the value the two consumers must agree on. */
async function hash(directory) {
  const digest = createHash('sha256')
  const visit = async (current, prefix = '') => {
    for (const entry of (await readdir(current, { withFileTypes: true })).sort((a, b) =>
      a.name.localeCompare(b.name),
    )) {
      const path = join(current, entry.name)
      const name = `${prefix}${entry.name}`
      if (entry.isDirectory()) await visit(path, `${name}/`)
      else {
        digest.update(name)
        digest.update('\0')
        digest.update(await readFile(path))
        digest.update('\0')
      }
    }
  }
  await visit(directory)
  return digest.digest('hex')
}

export async function runModuleConsumers() {
  execFileSync('pnpm', ['library:pack'], { cwd: root, stdio: 'pipe' })
  const pack = JSON.parse(await readFile(join(root, 'dist/library/artifacts.json'), 'utf8')) as {
    artifacts: Array<{ name: string; version: string; tarball: string; sha256: string }>
  }
  const artifacts = hashedModules.map((name) => {
    const artifact = pack.artifacts.find((item) => item.name === name)
    if (!artifact) throw new Error(`ERP module artifact ${name} is not in the pack`)
    return artifact
  })

  const scratch = await mkdtemp(join(tmpdir(), 'archava-erp-consumers-'))
  const proofs: Array<{ consumer: string; modules: Array<Record<string, string>> }> = []
  try {
    for (const name of consumers) {
      const directory = join(scratch, name)
      await cp(join(root, 'examples/rust-consumers/erp', name), directory, { recursive: true })
      const manifestPath = join(directory, 'package.json')
      const manifest = JSON.parse(await readFile(manifestPath, 'utf8'))
      // The PIN: every dependency is the tarball from THIS run's pack. The
      // consumer's own package.json declarations exist so typecheck has names
      // before the driver rewrites them; the whole internal graph installs
      // because the pair's tarballs declare each other as versions.
      manifest.dependencies = Object.fromEntries(
        pack.artifacts.map((item) => [item.name, `file:${join(root, 'dist/library', item.tarball)}`]),
      )
      await writeFile(manifestPath, JSON.stringify(manifest, null, 2))
      await cp(
        join(root, 'dist/library/release-profile.json'),
        join(directory, 'release-profile.json'),
      )
      for (const args of [
        ['install', '--ignore-scripts', '--no-audit', '--no-fund'],
        ['run', 'typecheck'],
        ['run', 'build'],
        ['run', 'verify'],
      ]) {
        const result = spawnSync('npm', args, { cwd: directory, encoding: 'utf8' })
        if (result.status !== 0)
          throw new Error(
            `consumer ${name} failed at ${args[0]} (exit ${result.status}):\n` +
              [result.stderr, result.stdout].filter(Boolean).join('\n'),
          )
      }

      const modules: Array<Record<string, string>> = []
      for (const artifact of artifacts) {
        modules.push({
          name: artifact.name,
          version: artifact.version,
          implementationSha256: await hash(join(directory, 'node_modules', artifact.name)),
          tarballSha256: artifact.sha256,
        })
      }
      proofs.push({ consumer: name, modules })

      // The installed modules MUST have been resolved from node_modules: an
      // entry answer inside /packages/ means the workspace leaked into the
      // consumer's cold surface.
      const consumerRequire = (await import('node:module')).createRequire(
        join(directory, 'package.json'),
      )
      for (const artifact of artifacts) {
        const entry = consumerRequire.resolve(artifact.name)
        if (!entry.includes('/node_modules/'))
          throw new Error(`${artifact.name} resolved to workspace source in consumer ${name}`)
      }
    }
    if (JSON.stringify(proofs[0]?.modules) !== JSON.stringify(proofs[1]?.modules))
      throw new Error('ERP implementations differ across consumers')

    const proof = {
      claim: 'R06-TWO-CONSUMERS: two independently installed consumers of the accepted ERP modules',
      consumers: proofs,
      modulesChecked: hashedModules,
      packOutput: 'dist/library',
    }
    await mkdir(join(root, 'dist'), { recursive: true })
    await writeFile(outputPath, JSON.stringify(proof, null, 2) + '\n')
    return { ok: true, proof }
  } catch (error) {
    return { ok: false, error: String(error) }
  } finally {
    await rm(scratch, { recursive: true, force: true }).catch(() => undefined)
  }
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {  const outcome = await runModuleConsumers()
  if (outcome.ok) {
    process.stdout.write('PASS erp module consumers — every required check has its witnesses\n')
  } else {
    process.stdout.write(`FAIL erp module consumers — ${outcome.error}\n`)
    process.exitCode = 1
  }
}
