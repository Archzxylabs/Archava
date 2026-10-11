/* global process */
import assert from 'node:assert/strict'
import { mkdtemp, writeFile, readFile, rm, lstat, mkdir } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { createHash } from 'node:crypto'
import { build } from 'esbuild'

const root = resolve(import.meta.dirname, '../..')
const library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-kit-proof-'))
const proofs = []
const checks = []
const raw = (id, scope = 'operations') => ({
  schemaVersion: '1.0.0',
  catalogueVersion: '1.0.0',
  profile: trusted.version,
  selections: [{ id, scope }],
})
const command = (script, args) =>
  spawnSync(process.execPath, [join(root, 'tools/library', script), ...args], {
    cwd: root,
    encoding: 'utf8',
    env: { ...process.env, NODE_PATH: '' },
  })
try {
  for (const [id, name] of [
    ['K01', 'hotel-inquiry'],
    ['K05', 'project-services-inquiry'],
  ]) {
    const consumer = join(scratch, name)
    await mkdir(consumer)
    await writeFile(
      join(consumer, 'package.json'),
      JSON.stringify({
        name: 'independent-kit-' + name,
        private: true,
        type: 'module',
        overrides: Object.fromEntries(
          (profile.catalog.externals ?? []).map((p) => [p.name, p.version]),
        ),
        dependencies: Object.fromEntries(
          profile.artifacts.map((p) => [p.name, 'file:' + join(library, p.tarball)]),
        ),
      }),
    )
    execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: consumer,
      stdio: 'pipe',
      env: { ...process.env, NODE_PATH: '' },
    })
    const require = createRequire(join(consumer, 'package.json'))
    const entry = require.resolve('@archava/kits')
    assert.ok(entry.startsWith(join(consumer, 'node_modules')))
    const release = await import(pathToFileURL(require.resolve('@archava/config/release')).href)
    await release.verifyInstalledRelease(profile, trusted, join(consumer, 'node_modules'))
    const kits = await import(pathToFileURL(entry).href)
    const assembly = await import(pathToFileURL(require.resolve('@archava/config/assembly')).href)
    const planner = new kits.KitPlanner(trusted, assembly.COMPOSABLE_MODULES)
    assert.equal(kits.listKits().length, 40)
    const inquiry = planner.plan(raw(id, 'crm_inquiry'))
    assert.equal(inquiry.readyToAssemble, true)
    assert.deepEqual(inquiry.operationalKitsAccepted, [])
    for (const kitId of ['K05', 'K31']) {
      const tracking = planner.plan(raw(kitId, 'project_tracking'))
      assert.equal(tracking.readyToAssemble, true)
      assert.equal(tracking.assembly.projects, true)
      assert.deepEqual(tracking.operationalKitsAccepted, [])
    }
    for (const kitId of ['K07', 'K10']) {
      const reservations = planner.plan(raw(kitId, 'resource_reservations'))
      assert.equal(reservations.readyToAssemble, true)
      assert.equal(reservations.assembly.scheduling, true)
      assert.deepEqual(reservations.operationalKitsAccepted, [])
    }
    const stays = planner.plan(raw('K01', 'accommodation_stays'))
    assert.equal(stays.readyToAssemble, true)
    assert.equal(stays.assembly.accommodation, true)
    assert.equal(stays.assembly.scheduling, true)
    assert.deepEqual(stays.operationalKitsAccepted, [])
    assert.equal(planner.plan(raw('K02', 'accommodation_stays')).readyToAssemble, false)
    for (const kit of kits.listKits()) {
      const operation = planner.plan(raw(kit.id))
      assert.equal(operation.readyToAssemble, false)
      assert.equal(operation.assembly, null)
      assert.ok(operation.blockers.some((r) => r.id === kit.domainRequirement))
    }
    assert.throws(() => planner.plan({ ...raw(id), supported: true }))
    assert.throws(() => planner.plan({ ...raw(id), role: 'owner' }))
    const browser = await build({
      entryPoints: [entry],
      outfile: join(consumer, 'browser.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some(
        (path) =>
          path.includes('node:') ||
          /crm-pg|identity-pg|provider-runtime/.test(path) ||
          /config\/dist\/release\.js/.test(path),
      ),
      false,
    )
    const implementation = await readFile(entry)
    proofs.push({
      consumer: name,
      scope: inquiry.selections,
      candidateKits: 40,
      operationalKitsAccepted: 0,
      profile: trusted.version,
      implementationSha256: createHash('sha256').update(implementation).digest('hex'),
      installedProfileVerified: true,
      browserSafe: true,
    })
  }
  assert.equal(new Set(proofs.map((p) => p.implementationSha256)).size, 1)
  checks.push('two cold-installed kit consumers share exact implementations and verified profile')
  checks.push('all 40 full operation kits remain blocked on missing domain implementations')
  checks.push('two narrow inquiry scopes are accepted without claiming operational kit completion')
  checks.push(
    'two internal project tracking scopes compose the same project module without accepting full services or EPC kits',
  )
  checks.push('browser bundle excludes server/database/credential authority')

  const readyFile = join(scratch, 'inquiry.json'),
    blockedFile = join(scratch, 'hotel.json')
  await writeFile(readyFile, JSON.stringify(raw('K01', 'crm_inquiry')))
  await writeFile(blockedFile, JSON.stringify(raw('K01')))
  const ready = command('plan-kits.mjs', [readyFile]),
    blocked = command('plan-kits.mjs', [blockedFile])
  assert.equal(ready.status, 0, ready.stderr)
  assert.equal(blocked.status, 2, blocked.stderr)
  assert.equal(JSON.parse(blocked.stdout).readyToAssemble, false)
  const dryOut = join(scratch, 'dry-output')
  const dry = command('assemble.mjs', [readyFile, dryOut, '--kit', '--dry-run'])
  assert.equal(dry.status, 0, dry.stderr)
  assert.equal(JSON.parse(dry.stdout).kitPlan.readyToAssemble, true)
  await assert.rejects(
    () => lstat(dryOut),
    (error) => error.code === 'ENOENT',
  )
  const missingOut = join(scratch, 'blocked-output')
  const refused = command('assemble.mjs', [blockedFile, missingOut, '--kit'])
  assert.equal(refused.status, 2, refused.stderr)
  await assert.rejects(
    () => lstat(missingOut),
    (error) => error.code === 'ENOENT',
  )
  const realOut = join(scratch, 'assembled-inquiry')
  const assembled = command('assemble.mjs', [readyFile, realOut, '--kit'])
  assert.equal(assembled.status, 0, assembled.stderr)
  assert.equal(
    JSON.parse(await readFile(join(realOut, 'kit-plan.json'), 'utf8')).selections[0].scope,
    'crm_inquiry',
  )
  assert.notEqual(command('assemble.mjs', [readyFile, realOut, '--kit']).status, 0)
  const trackingFile = join(scratch, 'project-tracking.json'),
    trackingOut = join(scratch, 'assembled-projects')
  await writeFile(trackingFile, JSON.stringify(raw('K05', 'project_tracking')))
  const tracking = command('assemble.mjs', [trackingFile, trackingOut, '--kit'])
  assert.equal(tracking.status, 0, tracking.stderr)
  const projectAssembly = JSON.parse(await readFile(join(trackingOut, 'assembly.json'), 'utf8'))
  assert.equal(projectAssembly.version, '2.0.0')
  assert.ok(projectAssembly.entries.some((entry) => entry.entry === '@archava/projects'))
  assert.equal(
    JSON.parse(await readFile(join(trackingOut, 'kit-plan.json'), 'utf8')).selections[0].scope,
    'project_tracking',
  )
  const scheduleFile = join(scratch, 'resource-reservations.json'),
    scheduleOut = join(scratch, 'assembled-scheduling')
  await writeFile(scheduleFile, JSON.stringify(raw('K07', 'resource_reservations')))
  const schedule = command('assemble.mjs', [scheduleFile, scheduleOut, '--kit'])
  assert.equal(schedule.status, 0, schedule.stderr)
  const scheduleAssembly = JSON.parse(await readFile(join(scheduleOut, 'assembly.json'), 'utf8'))
  assert.equal(scheduleAssembly.version, '3.0.0')
  assert.ok(scheduleAssembly.entries.some((entry) => entry.entry === '@archava/scheduling'))
  assert.equal(
    JSON.parse(await readFile(join(scheduleOut, 'kit-plan.json'), 'utf8')).selections[0].scope,
    'resource_reservations',
  )
  checks.push(
    'narrow appointment and rental reservations assemble under v3 without accepting full operating kits',
  )
  const hotelFile = join(scratch, 'accommodation-stays.json'),
    hotelOut = join(scratch, 'assembled-hotel')
  await writeFile(hotelFile, JSON.stringify(raw('K01', 'accommodation_stays')))
  const hotel = command('assemble.mjs', [hotelFile, hotelOut, '--kit'])
  assert.equal(hotel.status, 0, hotel.stderr)
  const hotelAssembly = JSON.parse(await readFile(join(hotelOut, 'assembly.json'), 'utf8'))
  assert.equal(hotelAssembly.version, '4.0.0')
  assert.ok(hotelAssembly.entries.some((entry) => entry.entry === '@archava/accommodation'))
  assert.ok(hotelAssembly.entries.some((entry) => entry.entry === '@archava/scheduling'))
  assert.equal(
    JSON.parse(await readFile(join(hotelOut, 'kit-plan.json'), 'utf8')).selections[0].scope,
    'accommodation_stays',
  )
  checks.push(
    'bounded hotel stays assemble with shared occupancy under v4 while full hotel operations remain blocked',
  )
  await writeFile(
    join(scratch, 'forged.json'),
    JSON.stringify({ ...raw('K01'), apiKey: 'fixture-private' }),
  )
  const forged = command('plan-kits.mjs', [join(scratch, 'forged.json')])
  assert.equal(forged.status, 1)
  assert.equal(forged.stderr.includes('fixture-private'), false)
  checks.push('CLI distinguishes ready/refused/invalid plans without disclosing rejected fields')
  checks.push(
    'assembly records narrow scope, refuses missing requirements and overwrite, and dry-run writes nothing',
  )
  const destination = join(root, 'dist/kit-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        catalogueVersion: '1.0.0',
        candidates: 40,
        operationalKitsAccepted: 0,
        proofs,
        checks,
        identicalImplementations: true,
      },
      null,
      2,
    ) + '\n',
  )
  process.stdout.write('PASS ' + checks.join('\nPASS ') + '\n')
} finally {
  await rm(scratch, { recursive: true, force: true })
}
