/* global process */
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { openSync, closeSync } from 'node:fs'
import { readFile, writeFile, mkdir, readdir } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { createHash } from 'node:crypto'
const root = resolve(import.meta.dirname, '../..'),
  output = join(root, 'dist/security-proof')
await mkdir(output, { recursive: true })
const commandResults = []
function run(name, args, cwd = root, allowed = [0]) {
  const file = join(output, name + '.log'),
    fd = openSync(file, 'w', 0o600)
  let status = 0
  try {
    execFileSync(args[0], args.slice(1), { cwd, stdio: ['ignore', fd, fd] })
  } catch (error) {
    status = error.status ?? -1
  } finally {
    closeSync(fd)
  }
  commandResults.push({
    name,
    command: args.map((value) => value.replace(root + '/', '')).join(' '),
    exitCode: status,
  })
  if (!allowed.includes(status)) throw new Error('Security verification gate failed: ' + name)
}
if (process.argv.includes('--run')) {
  run('unit', [
    'pnpm',
    'exec',
    'vitest',
    'run',
    '--reporter=json',
    '--outputFile=' + join(output, 'unit.json'),
  ])
  run('crm', ['pnpm', 'crm:verify'])
  run('act', ['pnpm', 'act:verify'])
  run('runtime', ['pnpm', 'crm:verify-runtime'])
  run('reuse', ['pnpm', 'reuse:verify'])
  run('assembly', ['pnpm', 'assembly:verify'])
}
run('platform-audit', ['pnpm', 'audit', '--json'], root, [0, 1])
run('runtime-audit', ['npm', 'audit', '--json'], join(root, 'dist/crm-runtime'), [0, 1])
run('template-audit', ['npm', 'audit', '--json'], join(root, 'Archava Template'), [0, 1])
const json = async (file) => JSON.parse(await readFile(file, 'utf8'))
const audits = {}
for (const name of ['platform', 'runtime', 'template']) {
  const raw = await json(join(output, name + '-audit.log'))
  if (raw.error || !raw.metadata?.vulnerabilities)
    throw new Error('Incomplete advisory scan ' + name)
  const counts = raw.metadata.vulnerabilities
  if (name !== 'template')
    assert.equal(
      (counts.high ?? 0) + (counts.critical ?? 0),
      0,
      'Unresolved supported-profile high/critical advisory',
    )
  audits[name] = {
    counts,
    findings: Object.entries(raw.vulnerabilities ?? {}).map(([pkg, finding]) => ({
      package: pkg,
      severity: finding.severity,
      range: finding.range,
      advisories: finding.via
        .filter((v) => typeof v === 'object')
        .map((v) => ({ title: v.title, url: v.url, range: v.range })),
    })),
  }
}
const profile = await json(join(root, 'dist/library/release-profile.json'))
const unit = await json(join(output, 'unit.json'))
assert.equal(unit.numFailedTests, 0)
assert.ok(unit.numPassedTests > 0)
const proofs = {
  runtime: await json(join(root, 'dist/crm-runtime-proof/results.json')),
  crm: await json(join(root, 'dist/crm-proof/results.json')),
  act: await json(join(root, 'dist/act-proof/results.json')),
}
assert.equal(proofs.runtime.providerCalls, 0)
assert.equal(proofs.act.providerCalls, 0)
const controls = [
  [
    'identity-session-csrf',
    ['packages/identity-pg/src/index.ts', 'apps/crm/src/host.ts'],
    'runtime',
    'credentials.*cookie-only|session-bound CSRF',
  ],
  [
    'current-tenant-resource-grants',
    ['packages/crm-pg/src/repository.ts'],
    'runtime',
    'permissions.*tenant/resource isolation',
  ],
  [
    'non-owner-rls',
    ['packages/crm-pg/src/repository.ts', 'packages/crm-pg/migrations/001_crm_contacts.sql'],
    'crm',
    'non-owner runtime',
  ],
  ['service-human-intersection', ['packages/crm-pg/src/repository.ts'], 'act', 'native|scope'],
  [
    'strict-inputs-no-model-authority',
    ['apps/crm/src/host.ts', 'packages/crm-assistant/src/contracts.ts'],
    'runtime',
    'forged authority|forged flags',
  ],
  [
    'browser-server-separation',
    ['apps/crm/scripts/build.mjs', 'tools/library/verify-consumers.mjs'],
    'runtime',
    'browser/server isolation',
  ],
  [
    'one-time-exact-consent',
    ['packages/crm-act/src/postgres.ts', 'packages/act-host/src/index.ts'],
    'runtime',
    'payload drift.*repeat execution',
  ],
  [
    'revision-and-replay',
    ['packages/crm-pg/src/repository.ts', 'packages/crm-act/src/postgres.ts'],
    'runtime',
    'changed revision.*replay',
  ],
  [
    'unknown-outcome-recovery',
    ['packages/act-host/src/index.ts', 'packages/crm/src/service.ts'],
    'runtime',
    'read-only command recovery',
  ],
  [
    'private-state-and-secret-isolation',
    ['apps/crm/src/settings.ts', 'apps/crm/src/planner.ts'],
    'runtime',
    'private persistent installation|private connections',
  ],
  [
    'restore-identity-holds-epoch',
    ['apps/crm/src/operator.ts', 'apps/crm/src/native-actions.ts'],
    'runtime',
    'old credentials and sessions|holds native actions',
  ],
  [
    'backup-integrity-and-overwrite',
    ['apps/crm/src/backups.ts'],
    'runtime',
    'archives are never overwritten.*corrupt snapshots',
  ],
  [
    'module-lifecycle-no-abandonment',
    ['apps/crm/src/modules.ts', 'apps/crm/src/host.ts'],
    'runtime',
    'unresolved native effects|re-enable retains',
  ],
  [
    'presence-guest-binding-and-scope',
    ['packages/presence/src/index.ts', 'apps/crm/src/presence.ts'],
    'runtime',
    'standalone Python synthetic voice',
  ],
  [
    'profile-install-and-schema',
    ['packages/config/src/release.ts', 'apps/crm/src/server.ts'],
    'runtime',
    'startup refuses changed installed',
  ],
]
const matrix = []
for (const [id, sources, proof, pattern] of controls) {
  for (const source of sources) await readFile(join(root, source))
  const checks = proofs[proof].checks.filter((label) => new RegExp(pattern).test(label))
  assert.ok(checks.length, 'Missing runnable control evidence: ' + id)
  matrix.push({
    id,
    sources,
    command:
      proof === 'runtime'
        ? 'pnpm crm:verify-runtime'
        : proof === 'crm'
          ? 'pnpm crm:verify'
          : 'pnpm act:verify',
    positiveAndRefusalChecks: checks,
    result: 'passed',
  })
}
const installed = join(root, 'dist/crm-runtime/node_modules'),
  inventory = [],
  notices = []
for (const external of profile.catalog.externals ?? []) {
  const dir = join(installed, external.name),
    pkg = await json(join(dir, 'package.json'))
  assert.equal(pkg.version, external.version)
  const files = (await readdir(dir)).filter((name) => /^licen[cs]e|^copying|^notice/i.test(name))
  const license = typeof pkg.license === 'string' ? pkg.license : 'UNDECLARED'
  inventory.push({
    name: pkg.name,
    version: pkg.version,
    license,
    noticeFiles: files,
    installScripts: Object.keys(pkg.scripts ?? {}).filter((key) =>
      ['preinstall', 'install', 'postinstall'].includes(key),
    ),
  })
  for (const file of files)
    notices.push(
      `${pkg.name}@${pkg.version} (${license})\n${await readFile(join(dir, file), 'utf8')}`,
    )
}
assert.ok(inventory.length > 0)
const hash = async (file) =>
  createHash('sha256')
    .update(await readFile(file))
    .digest('hex')
const report = {
  schemaVersion: '1.0.0',
  profile: profile.catalog.version,
  checkedAt: new Date().toISOString(),
  node: process.version,
  commands: commandResults,
  controls: matrix,
  unit: { passed: unit.numPassedTests, failed: unit.numFailedTests },
  audits,
  dependencies: inventory,
  locks: {
    pnpm: await hash(join(root, 'pnpm-lock.yaml')),
    runtimeNpm: await hash(join(root, 'dist/crm-runtime/package-lock.json')),
  },
  installPolicy:
    'Exact validated runtime external pins use already-tested resolved versions; artifact consumer installs disable lifecycle scripts. Root pnpm defaults refuse unapproved dependency build scripts; no new script approval.',
  distributionScope:
    'Private internal artifacts only. External SPDX metadata and included notices inventoried; no legal/IP-transfer opinion or public-distribution approval. Internal packages have no declared public license.',
  limits: {
    certification: false,
    penetrationTest: false,
    realtimeTemplate:
      '13 moderate OpenTelemetry-chain findings require compatibility/reachability review; separate SDK integration evidence missing. Public realtime release is not accepted.',
  },
}
await writeFile(join(output, 'results.json'), JSON.stringify(report, null, 2) + '\n')
await writeFile(join(output, 'third-party-notices.txt'), notices.join('\n\n') + '\n')
process.stdout.write(
  'PASS supported local control/evidence matrix, exact dependency inventory and advisory gate; realtime template remains separate.\n',
)
