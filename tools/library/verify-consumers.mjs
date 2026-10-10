/* global process */
import { mkdtemp, cp, readFile, writeFile, rm, mkdir, readdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve, dirname } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'
import { createRequire } from 'node:module'
import { createHash } from 'node:crypto'
import { spawnSync } from 'node:child_process'
import { packLibrary } from './pack.mjs'

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../..')
const { output, artifacts, profile } = await packLibrary()
const scratch = await mkdtemp(join(tmpdir(), 'archava-consumers-'))
const npm = process.platform === 'win32' ? 'npm.cmd' : 'npm'
const results = []

async function implementationHash(directory) {
  const digest = createHash('sha256')
  async function visit(current, prefix = '') {
    const entries = await readdir(current, { withFileTypes: true })
    for (const entry of entries.sort((a, b) => a.name.localeCompare(b.name))) {
      const file = join(current, entry.name)
      const name = `${prefix}${entry.name}`
      if (entry.isDirectory()) await visit(file, `${name}/`)
      else {
        digest.update(name)
        digest.update('\0')
        digest.update(await readFile(file))
        digest.update('\0')
      }
    }
  }
  await visit(directory)
  return digest.digest('hex')
}

function run(args, cwd) {
  const result = spawnSync(npm, args, {
    cwd,
    encoding: 'utf8',
    env: { ...process.env, NODE_PATH: '' },
  })
  if (result.status !== 0)
    throw new Error(`${args.join(' ')} failed:\n${result.stderr}\n${result.stdout}`)
  return result.stdout
}

try {
  for (const name of ['hospitality', 'retail']) {
    const consumer = join(scratch, name)
    await cp(join(root, 'examples/consumers', name), consumer, { recursive: true })
    const manifest = JSON.parse(await readFile(join(consumer, 'package.json'), 'utf8'))
    manifest.dependencies = Object.fromEntries(
      artifacts.map((item) => [item.name, `file:${join(output, item.tarball)}`]),
    )
    await writeFile(join(consumer, 'package.json'), JSON.stringify(manifest, null, 2))
    run(['install', '--ignore-scripts', '--no-audit', '--no-fund'], consumer)
    await cp(join(output, 'release-profile.json'), join(consumer, 'release-profile.json'))
    const require = createRequire(join(consumer, 'package.json'))
    const release = await import(pathToFileURL(require.resolve('@archava/config/release')).href)
    await release.verifyInstalledRelease(profile, profile.catalog, join(consumer, 'node_modules'))
    run(['run', 'typecheck'], consumer)
    run(['run', 'build'], consumer)
    const verified = run(['run', 'verify'], consumer).trim().split('\n').at(-1)
    const installed = []
    for (const artifact of artifacts) {
      const directory = join(consumer, 'node_modules', artifact.name)
      const packed = JSON.parse(await readFile(join(directory, 'package.json'), 'utf8'))
      if (packed.version !== artifact.version) throw new Error(`Version mismatch ${artifact.name}`)
      const hash = await implementationHash(join(directory, 'dist'))
      installed.push({
        name: artifact.name,
        version: packed.version,
        implementationSha256: hash,
        tarballSha256: artifact.sha256,
      })
    }
    results.push({
      consumer: name,
      checks: JSON.parse(verified),
      installed,
      releaseProfile: profile.catalog.version,
    })
    const demo = join(root, 'dist/reuse-demo', name)
    await mkdir(demo, { recursive: true })
    await cp(join(consumer, 'dist'), join(demo, 'dist'), { recursive: true })
    await cp(join(consumer, 'index.html'), join(demo, 'index.html'))
    process.stdout.write(
      `PASS ${name}: cold install, declarations, Node turn, browser bundle, tenant/policy denial.\n`,
    )
  }
  for (const artifact of artifacts) {
    const hashes = results.map(
      (item) => item.installed.find((entry) => entry.name === artifact.name).implementationSha256,
    )
    if (new Set(hashes).size !== 1) throw new Error(`Consumers forked ${artifact.name}`)
  }
  if (results[0].checks.text === results[1].checks.text)
    throw new Error('Consumer knowledge did not vary')
  if (results[0].checks.brand === results[1].checks.brand)
    throw new Error('Consumer branding did not vary')
  if (
    JSON.stringify(results[0].checks.allowedActions) ===
    JSON.stringify(results[1].checks.allowedActions)
  )
    throw new Error('Consumer actions did not vary')
  await writeFile(
    join(output, 'consumer-proof.json'),
    JSON.stringify(
      { node: process.version, consumers: results, identicalArtifacts: true },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(
    'PASS identical core artifact versions/hashes across two independent consumers.\n',
  )
} finally {
  await rm(scratch, { recursive: true, force: true })
}
