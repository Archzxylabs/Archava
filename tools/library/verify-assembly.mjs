/* global process */
import assert from 'node:assert/strict'
import { mkdtemp, writeFile, readFile, rm, lstat, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
const root = resolve(import.meta.dirname, '../..')
const scratch = await mkdtemp(join(tmpdir(), 'archava-assembly-proof-'))
const profile = JSON.parse(await readFile(join(root, 'dist/library/release-profile.json'), 'utf8'))
const checks = []
const assemble = (file, out, extra = []) =>
  JSON.parse(
    execFileSync(
      process.execPath,
      [join(import.meta.dirname, 'assemble.mjs'), file, out, ...extra],
      { cwd: root, stdio: 'pipe', encoding: 'utf8' },
    ),
  )
try {
  const selections = [
    { contacts: true, pipeline: false, assistant: false, nativeActions: false },
    { contacts: true, pipeline: true, assistant: true, nativeActions: true },
  ]
  for (let i = 0; i < selections.length; i++) {
    const file = join(scratch, 'selection-' + i + '.json'),
      out = join(scratch, 'consumer-' + i)
    await writeFile(
      file,
      JSON.stringify({
        version: '1.0.0',
        profile: profile.catalog.version,
        modules: selections[i],
      }),
    )
    assert.equal(assemble(file, out, ['--dry-run']).dryRun, true)
    await assert.rejects(
      () => lstat(out),
      (error) => error.code === 'ENOENT',
    )
    const plan = assemble(file, out)
    assert.equal(plan.authority, 'configuration-only')
    assert.equal(plan.entries.length, i === 0 ? 1 : 4)
    execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: out,
      stdio: 'pipe',
    })
    const require = createRequire(join(out, 'package.json'))
    const release = await import(pathToFileURL(require.resolve('@archava/config/release')).href)
    await release.verifyInstalledRelease(profile, profile.catalog, join(out, 'node_modules'))
    await import(pathToFileURL(join(out, 'entries.mjs')).href)
    assert.throws(() => assemble(file, out))
    checks.push({
      consumer: i === 0 ? 'minimal-contacts' : 'full-owner',
      entries: plan.entries.map((x) => x.entry),
      profile: plan.profile,
      verified: true,
    })
  }
  const bad = join(scratch, 'bad.json')
  await writeFile(
    bad,
    JSON.stringify({
      version: '1.0.0',
      profile: profile.catalog.version,
      modules: { ...selections[1], assistant: false },
    }),
  )
  assert.throws(() => assemble(bad, join(scratch, 'bad-output')))
  await assert.rejects(
    () => lstat(join(scratch, 'bad-output')),
    (error) => error.code === 'ENOENT',
  )
  const destination = join(root, 'dist/assembly-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        checks,
        identicalProfileHashes: true,
        refusedOverwrite: true,
        refusedDependencies: true,
        generatedDomainCode: false,
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write(
    'PASS independently installed minimal/full compositions, installed hashes, dry-run, dependency refusal and no overwrite.\n',
  )
} finally {
  await rm(scratch, { recursive: true, force: true })
}
