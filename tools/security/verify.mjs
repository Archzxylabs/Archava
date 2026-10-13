/* global process */
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { openSync, closeSync } from 'node:fs'
import { readFile, writeFile, mkdir, readdir } from 'node:fs/promises'
import { join, resolve } from 'node:path'
import { createHash } from 'node:crypto'
import { installEnv } from '../library/npm-env.mjs'
const root = resolve(import.meta.dirname, '../..'),
  output = join(root, 'dist/security-proof')
await mkdir(output, { recursive: true })
const commandResults = []
function run(name, args, cwd = root, allowed = [0], env = process.env) {
  const file = join(output, name + '.log'),
    fd = openSync(file, 'w', 0o600)
  let status = 0
  try {
    execFileSync(args[0], args.slice(1), { cwd, stdio: ['ignore', fd, fd], env })
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
  run('kits', ['pnpm', 'kits:verify'])
  run('projects', ['pnpm', 'projects:verify'])
  run('project-host', ['pnpm', 'projects:verify-host'])
  run('scheduling', ['pnpm', 'scheduling:verify'])
  run('scheduling-host', ['pnpm', 'scheduling:verify-host'])
  run('accommodation', ['pnpm', 'accommodation:verify'])
  run('accommodation-host', ['pnpm', 'accommodation:verify-host'])
  run('commerce', ['pnpm', 'commerce:verify'])
  run('commerce-host', ['pnpm', 'commerce:verify-host'])
  run('foodservice', ['pnpm', 'foodservice:verify'])
  run('foodservice-host', ['pnpm', 'foodservice:verify-host'])
  run('support', ['pnpm', 'support:verify'])
  run('support-host', ['pnpm', 'support:verify-host'])
  run('purchasing', ['pnpm', 'purchasing:verify'])
  run('purchasing-host', ['pnpm', 'purchasing:verify-host'])
  run('organizations', ['pnpm', 'organizations:verify'])
  run('organizations-host', ['pnpm', 'organizations:verify-host'])
  run('proposals', ['pnpm', 'proposals:verify'])
  run('proposals-host', ['pnpm', 'proposals:verify-host'])
  run('capacity', ['pnpm', 'capacity:verify'])
  run('capacity-host', ['pnpm', 'capacity:verify-host'])
}
run('platform-audit', ['pnpm', 'audit', '--json'], root, [0, 1])
run(
  'runtime-audit',
  ['npm', 'audit', '--json'],
  join(root, 'dist/crm-runtime'),
  [0, 1],
  installEnv(),
)
run(
  'template-audit',
  ['npm', 'audit', '--json'],
  join(root, 'Archava Template'),
  [0, 1],
  installEnv(),
)
const json = async (file) => {
  const raw = await readFile(file, 'utf8')
  // npm prints config warnings on stderr, and run() captures both streams into
  // the same file. Parse the JSON payload itself; completeness is asserted below.
  const start = raw.indexOf('{'),
    end = raw.lastIndexOf('}')
  if (start < 0 || end <= start) throw new Error('Advisory scan produced no JSON payload')
  return JSON.parse(raw.slice(start, end + 1))
}
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
  kits: await json(join(root, 'dist/kit-proof/results.json')),
  projects: await json(join(root, 'dist/project-proof/results.json')),
  projectHost: await json(join(root, 'dist/project-host-proof/results.json')),
  scheduling: await json(join(root, 'dist/scheduling-proof/results.json')),
  schedulingHost: await json(join(root, 'dist/scheduling-host-proof/results.json')),
  accommodation: await json(join(root, 'dist/accommodation-proof/results.json')),
  accommodationHost: await json(join(root, 'dist/accommodation-host-proof/results.json')),
  commerce: await json(join(root, 'dist/commerce-proof/results.json')),
  commerceHost: await json(join(root, 'dist/commerce-host-proof/results.json')),
  foodservice: await json(join(root, 'dist/foodservice-proof/results.json')),
  foodserviceHost: await json(join(root, 'dist/foodservice-host-proof/results.json')),
  support: await json(join(root, 'dist/support-proof/results.json')),
  supportHost: await json(join(root, 'dist/support-host-proof/results.json')),
  purchasing: await json(join(root, 'dist/purchasing-proof/results.json')),
  purchasingHost: await json(join(root, 'dist/purchasing-host-proof/results.json')),
  organizations: await json(join(root, 'dist/organizations-proof/results.json')),
  organizationsHost: await json(join(root, 'dist/organizations-host-proof/results.json')),
  proposals: await json(join(root, 'dist/proposals-proof/results.json')),
  proposalsHost: await json(join(root, 'dist/proposals-host-proof/results.json')),
  capacity: await json(join(root, 'dist/delivery-capacity-proof/results.json')),
  capacityHost: await json(join(root, 'dist/delivery-capacity-host-proof/results.json')),
}
for (const name of [
  'runtime',
  'kits',
  'projects',
  'projectHost',
  'scheduling',
  'schedulingHost',
  'accommodation',
  'accommodationHost',
  'commerce',
  'commerceHost',
  'foodservice',
  'foodserviceHost',
  'support',
  'supportHost',
  'purchasing',
  'purchasingHost',
  'organizations',
  'organizationsHost',
  'proposals',
  'proposalsHost',
  'capacity',
  'capacityHost',
]) {
  assert.equal(proofs[name].profile, profile.catalog.version, 'Stale release proof: ' + name)
}
const expectedCatalogSha256 = createHash('sha256')
  .update(JSON.stringify(profile.catalog))
  .digest('hex')
const expectedArtifactSha256 = createHash('sha256')
  .update(JSON.stringify(profile.artifacts))
  .digest('hex')
for (const name of [
  'organizations',
  'organizationsHost',
  'proposals',
  'proposalsHost',
  'capacity',
  'capacityHost',
]) {
  if (proofs[name].catalogSha256) {
    assert.equal(
      proofs[name].catalogSha256,
      expectedCatalogSha256,
      'Mismatched catalog hash: ' + name,
    )
  }
  if (proofs[name].artifactSha256) {
    assert.equal(
      proofs[name].artifactSha256,
      expectedArtifactSha256,
      'Mismatched artifact hash: ' + name,
    )
  }
}
assert.equal(proofs.runtime.providerCalls, 0)
assert.equal(proofs.act.providerCalls, 0)
assert.equal(proofs.projectHost.providerCalls, 0)
assert.equal(proofs.scheduling.providerCalls, 0)
assert.equal(proofs.schedulingHost.providerCalls, 0)
assert.equal(proofs.accommodation.providerCalls, 0)
assert.equal(proofs.accommodationHost.providerCalls, 0)
assert.equal(proofs.commerce.providerCalls, 0)
assert.equal(proofs.commerceHost.providerCalls, 0)
assert.equal(proofs.foodservice.providerCalls, 0)
assert.equal(proofs.foodserviceHost.providerCalls, 0)
assert.equal(proofs.support.providerCalls, 0)
assert.equal(proofs.supportHost.providerCalls, 0)
assert.equal(proofs.purchasing.providerCalls, 0)
assert.equal(proofs.purchasingHost.providerCalls, 0)
assert.equal(proofs.organizations.providerCalls, 0)
assert.equal(proofs.organizationsHost.providerCalls, 0)
assert.equal(proofs.proposals.providerCalls, 0)
assert.equal(proofs.proposalsHost.providerCalls, 0)
assert.equal(proofs.capacity.providerCalls, 0)
assert.equal(proofs.capacityHost.providerCalls, 0)
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
  [
    'kit-readiness-no-implicit-support',
    ['packages/kits/src/index.ts', 'tools/library/assemble.mjs'],
    'kits',
    'all 40 full operation|refuses missing requirements',
  ],
  [
    'project-current-grants-and-scope',
    ['packages/projects-pg/src/repository.ts'],
    'projects',
    'tenant separation|revocation blocks stale receipts',
  ],
  [
    'project-human-writes-only',
    ['packages/projects/src/service.ts', 'packages/projects/src/tools.ts'],
    'projects',
    'viewer cannot write|read operations only',
  ],
  [
    'project-parent-revision-and-idempotency',
    ['packages/projects-pg/src/repository.ts'],
    'projects',
    '20 concurrent submissions|competing child mutations',
  ],
  [
    'project-delivery-and-scope-boundary',
    [
      'packages/projects-pg/src/repository.ts',
      'packages/projects-pg/migrations/001_project_delivery.sql',
    ],
    'projects',
    'unfinished required work|cancelled required tasks',
  ],
  [
    'project-host-auth-and-lifecycle',
    ['apps/crm/src/host.ts', 'apps/crm/src/operator.ts'],
    'projectHost',
    'CSRF|never grants domain access|disable and re-enable',
  ],
  [
    'project-recovery-and-restore',
    ['packages/projects/src/dashboard.ts', 'apps/crm/src/backups.ts'],
    'projectHost',
    'response loss|backup restore',
  ],
]
controls.push(
  [
    'scheduling-current-grants-and-tools',
    ['packages/scheduling-pg/src/repository.ts', 'packages/scheduling/src/tools.ts'],
    'scheduling',
    'current grants|read-only tools',
  ],
  [
    'scheduling-atomic-overlap-and-revisions',
    [
      'packages/scheduling-pg/src/repository.ts',
      'packages/scheduling-pg/migrations/001_resource_reservations.sql',
    ],
    'scheduling',
    '20 distinct competing|20 duplicate|competing revisions',
  ],
  [
    'scheduling-database-clock-and-expiry',
    [
      'packages/scheduling-pg/src/repository.ts',
      'packages/scheduling-pg/migrations/001_resource_reservations.sql',
    ],
    'scheduling',
    'expiry uses database time',
  ],
  [
    'scheduling-host-auth-and-lifecycle',
    ['apps/crm/src/host.ts', 'apps/crm/src/operator.ts'],
    'schedulingHost',
    'CSRF|never grants domain|disable and re-enable',
  ],
  [
    'scheduling-recovery-and-restore',
    ['packages/scheduling/src/dashboard.ts', 'apps/crm/src/backups.ts'],
    'schedulingHost',
    'response loss|backup restore',
  ],
)
controls.push(
  [
    'accommodation-current-grants-and-tools',
    ['packages/accommodation-pg/src/repository.ts', 'packages/accommodation/src/tools.ts'],
    'accommodation',
    'tenant and parent ownership|retries recheck write grants|strictly read-only tool',
  ],
  [
    'accommodation-shared-atomic-capacity',
    [
      'packages/scheduling-pg/migrations/002_shared_occupancy.sql',
      'packages/accommodation-pg/src/repository.ts',
    ],
    'accommodation',
    '20 independent room-night|20 duplicate requests|generic reservation API',
  ],
  [
    'accommodation-calendar-and-quote',
    [
      'packages/scheduling/src/calendar.ts',
      'packages/accommodation-pg/migrations/001_room_night_stays.sql',
    ],
    'accommodation',
    'rate changes|calendar nights remain|expiry is evaluated',
  ],
  [
    'accommodation-room-readiness',
    ['packages/accommodation-pg/migrations/001_room_night_stays.sql'],
    'accommodation',
    'walk-in requires confirmed',
  ],
  [
    'accommodation-host-and-recovery',
    ['packages/accommodation/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'accommodationHost',
    'CSRF|pending UUID|backup restore|enabling it never',
  ],
)
const matrix = []
controls.push(
  [
    'commerce-current-grants-and-tools',
    ['packages/commerce-pg/src/repository.ts', 'packages/commerce/src/tools.ts'],
    'commerce',
    'grant intersection|actor-bound|read-only tools',
  ],
  [
    'commerce-atomic-stock-and-quote',
    [
      'packages/commerce-pg/src/kernel.ts',
      'packages/commerce-pg/migrations/001_catalog_orders_stock.sql',
    ],
    'commerce',
    '20 distinct orders|20 duplicate fulfillment|multi-product shortage|future quotes',
  ],
  [
    'commerce-expiry-and-returns',
    [
      'packages/commerce-pg/src/repository.ts',
      'packages/commerce-pg/migrations/001_catalog_orders_stock.sql',
    ],
    'commerce',
    'hold expiry uses database clock|partial returns|derived balances',
  ],
  [
    'commerce-host-and-recovery',
    ['packages/commerce/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'commerceHost',
    'CSRF|pending UUID|backup restore|never enrolls',
  ],
)
controls.push(
  [
    'foodservice-current-grants-and-tools',
    ['packages/foodservice-pg/src/repository.ts', 'packages/foodservice/src/tools.ts'],
    'foodservice',
    'current food and commerce grants|read-only tools',
  ],
  [
    'foodservice-atomic-ingredients-and-tables',
    [
      'packages/foodservice-pg/src/repository.ts',
      'packages/foodservice-pg/migrations/001_tables_recipes_tickets.sql',
    ],
    'foodservice',
    'last physical table|last ingredient|failure after stock consumption|20 preparation retries',
  ],
  [
    'foodservice-frozen-recipes-and-waste',
    [
      'packages/foodservice-pg/migrations/001_tables_recipes_tickets.sql',
      'packages/commerce-pg/migrations/002_managed_menu_products.sql',
    ],
    'foodservice',
    'sealed recipe|generic catalog/order|records waste|preparation expiry',
  ],
  [
    'foodservice-upgrade-and-restore',
    ['packages/foodservice-pg/src/migrate.ts', 'packages/commerce-pg/src/migrate.ts'],
    'foodservice',
    'non-empty archived|database restart|fresh backup restore',
  ],
  [
    'foodservice-host-and-recovery',
    ['packages/foodservice/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'foodserviceHost',
    'CSRF|pending UUID|backup restore|never enrolls',
  ],
)
controls.push(
  [
    'support-current-grants-and-assignment',
    [
      'packages/support-pg/src/repository.ts',
      'packages/support-pg/migrations/001_cases_notes_resolution.sql',
    ],
    'support',
    'current human/service|globally authorized assignment|reassignment immediately',
  ],
  [
    'support-atomic-commands-and-history',
    ['packages/support-pg/src/repository.ts'],
    'support',
    '20 duplicate|20 competing revisions|failure after header',
  ],
  [
    'support-resolution-cycles-and-targets',
    ['packages/support-pg/migrations/001_cases_notes_resolution.sql'],
    'support',
    'reopening starts a new cycle|database-clock due|immutable notes/history',
  ],
  [
    'support-private-tools',
    ['packages/support/src/api.ts', 'packages/support/src/tools.ts'],
    'support',
    'strict bounded API and private read-only tools',
  ],
  [
    'support-host-and-recovery',
    ['packages/support/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'supportHost',
    'CSRF|pending UUID|backup restore|never enrolls',
  ],
  [
    'owned-postgres-shutdown',
    ['apps/crm/src/postgres-pool.ts', 'apps/crm/src/server.ts', 'apps/crm/src/backups.ts'],
    'runtime',
    'application/database restart|shared manual and scheduled publisher',
  ],
)
controls.push(
  [
    'purchasing-current-grants-and-scope',
    ['packages/purchasing-pg/src/repository.ts'],
    'purchasing',
    'current independent purchasing/commerce grants',
  ],
  [
    'purchasing-frozen-backend-quotes',
    ['packages/purchasing-pg/src/repository.ts', 'packages/purchasing/src/contracts.ts'],
    'purchasing',
    'stale drafts refuse confirmation',
  ],
  [
    'purchasing-atomic-stock-receipts',
    [
      'packages/purchasing-pg/src/repository.ts',
      'packages/commerce-pg/src/kernel.ts',
      'packages/commerce-pg/migrations/003_purchase_receipts.sql',
    ],
    'purchasing',
    'failure after purchase findings and shared stock writes',
  ],
  [
    'purchasing-immutable-physical-truth',
    ['packages/purchasing-pg/migrations/001_suppliers_orders_receipts.sql'],
    'purchasing',
    'exact inbound quantity binding resist direct runtime SQL bypass',
  ],
  [
    'purchasing-private-tools',
    ['packages/purchasing/src/api.ts', 'packages/purchasing/src/tools.ts'],
    'purchasing',
    'strict bounded API and read-only tools',
  ],
  [
    'purchasing-host-recovery',
    ['packages/purchasing/src/dashboard.ts', 'apps/crm/src/host.ts', 'apps/crm/src/operator.ts'],
    'purchasingHost',
    'original UUID.*pending outcome|backup restore retains supplier quote',
  ],
)
controls.push(
  [
    'organizations-current-grants-and-tools',
    ['packages/organizations-pg/src/repository.ts', 'packages/organizations/src/tools.ts'],
    'organizations',
    'zero-grant viewer is denied owner command receipt access|forced tenant RLS completely isolates organization data',
  ],
  [
    'organizations-atomic-mutations-and-revisions',
    [
      'packages/organizations-pg/src/repository.ts',
      'packages/organizations-pg/migrations/001_organizations_affiliations.sql',
    ],
    'organizations',
    'command replay with altered payload is rejected with conflict|optimistic revision check rejects update against stale revision|primary affiliation constraint strictly permits at most one active primary contact',
  ],
  [
    'organizations-host-and-recovery',
    ['packages/organizations/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'organizationsHost',
    'compiled host enforces CSRF, origin check|isolated browser completes organization and affiliation lifecycle|backup and restore preserves organization entities',
  ],
  [
    'proposals-current-grants-and-tools',
    ['packages/proposals-pg/src/repository.ts', 'packages/proposals/src/tools.ts'],
    'proposals',
    'zero-grant viewer is denied owner command receipt access|unrelated assigned staff is denied proposal modification',
  ],
  [
    'proposals-atomic-scope-and-amendments',
    [
      'packages/proposals-pg/src/repository.ts',
      'packages/proposals-pg/migrations/001_proposals_scope_amendments.sql',
    ],
    'proposals',
    'submitted proposal deliverables are frozen against direct modification|client acceptance seals baseline Scope Version 1|accepted proposal rejects direct edits and requires versioned change requests',
  ],
  [
    'proposals-host-and-recovery',
    ['packages/proposals/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'proposalsHost',
    'compiled host enforces CSRF, origin check|isolated browser completes proposal submit, accept|backup and restore preserves proposal entities',
  ],
  [
    'capacity-current-grants-and-tools',
    ['packages/capacity-pg/src/repository.ts', 'packages/capacity/src/tools.ts'],
    'capacity',
    'zero-grant viewer is denied owner command receipt access|forced tenant RLS completely isolates capacity pools',
  ],
  [
    'capacity-deterministic-burn-and-concurrency',
    ['packages/capacity-pg/src/repository.ts', 'packages/capacity/src/contracts.ts'],
    'capacity',
    'future allocation saturation prevents multi-day overload|Friday estimate duration distinguishes elapsed calendar days from working days',
  ],
  [
    'capacity-host-and-recovery',
    ['packages/capacity/src/dashboard.ts', 'apps/crm/src/operator.ts', 'apps/crm/src/host.ts'],
    'capacityHost',
    'compiled host enforces CSRF, origin check|isolated browser completes pool creation|backup and restore preserves capacity pool entities',
  ],
)
for (const [id, sources, proof, pattern] of controls) {
  for (const source of sources) await readFile(join(root, source))
  const checks = proofs[proof].checks.filter((label) => new RegExp(pattern).test(label))
  assert.ok(checks.length, 'Missing runnable control evidence: ' + id)
  matrix.push({
    id,
    sources,
    command: {
      runtime: 'pnpm crm:verify-runtime',
      crm: 'pnpm crm:verify',
      act: 'pnpm act:verify',
      kits: 'pnpm kits:verify',
      projects: 'pnpm projects:verify',
      projectHost: 'pnpm projects:verify-host',
      scheduling: 'pnpm scheduling:verify',
      schedulingHost: 'pnpm scheduling:verify-host',
      accommodation: 'pnpm accommodation:verify',
      accommodationHost: 'pnpm accommodation:verify-host',
      commerce: 'pnpm commerce:verify',
      commerceHost: 'pnpm commerce:verify-host',
      foodservice: 'pnpm foodservice:verify',
      foodserviceHost: 'pnpm foodservice:verify-host',
      support: 'pnpm support:verify',
      supportHost: 'pnpm support:verify-host',
      purchasing: 'pnpm purchasing:verify',
      purchasingHost: 'pnpm purchasing:verify-host',
      organizations: 'pnpm organizations:verify',
      organizationsHost: 'pnpm organizations:verify-host',
      proposals: 'pnpm proposals:verify',
      proposalsHost: 'pnpm proposals:verify-host',
      capacity: 'pnpm capacity:verify',
      capacityHost: 'pnpm capacity:verify-host',
    }[proof],
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
  catalogSha256: expectedCatalogSha256,
  artifactSha256: expectedArtifactSha256,
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
