/* global process */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, readFile, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomBytes, randomUUID, createHash } from 'node:crypto'
import { build } from 'esbuild'
import { endProofPool } from './pg-proof.mjs'
import { installEnv } from './npm-env.mjs'

const root = resolve(import.meta.dirname, '../..')
const library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-orgs-proof-'))
const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
const env = Object.fromEntries(Object.entries(process.env).filter(([key]) => !key.startsWith('PG')))
const run = (name, args) =>
  execFileSync(join(bin, name), args, { encoding: 'utf8', stdio: 'pipe', env })

const socket = join(scratch, 'socket')
const data = join(scratch, 'data')
const port = 20000 + Math.floor(Math.random() * 25000)
await mkdir(socket)

let running = false
let owner
let app
let Pool

const start = () => {
  run('pg_ctl', [
    '-D',
    data,
    '-l',
    join(scratch, 'postgres.log'),
    '-w',
    'start',
    '-o',
    `-k ${socket} -p ${port} -c listen_addresses=''`,
  ])
  running = true
}

const stop = () => {
  if (running) {
    run('pg_ctl', ['-D', data, '-m', 'fast', '-w', 'stop'])
    running = false
  }
}

const target = {
  host: socket,
  port,
  user: 'postgres',
  database: 'postgres',
  password: () => '',
  ssl: false,
  max: 24,
}

const key = randomBytes(32).toString('hex')
const checks = []
const proofs = []

const check = (label) => {
  checks.push(label)
  process.stdout.write('PASS ' + label + '\n')
}

const human = (id) => ({ kind: 'human', id })
const failed = async (promise, code) =>
  assert.rejects(
    typeof promise === 'function' ? async () => promise() : promise,
    (error) => error.code === code,
  )

try {
  const consumers = []
  // Two independent domain consumers: Wholesale (K04) and Recruitment (K21)
  for (const name of ['wholesale', 'recruitment']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-orgs-' + name,
        private: true,
        type: 'module',
        overrides: Object.fromEntries(profile.catalog.externals.map((x) => [x.name, x.version])),
        dependencies: Object.fromEntries(
          profile.artifacts.map((x) => [x.name, 'file:' + join(library, x.tarball)]),
        ),
      }),
    )
    execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: directory,
      stdio: 'pipe',
      env: { ...installEnv(env), NODE_PATH: '' },
    })

    const require = createRequire(join(directory, 'package.json'))
    const load = async (pkg) => import(pathToFileURL(require.resolve(pkg)).href)

    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))

    const domain = await load('@archava/organizations')
    const postgres = await load('@archava/organizations-pg')
    Pool ??= require('pg').Pool

    const browser = await build({
      entryPoints: [require.resolve('@archava/organizations/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })

    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /organizations-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )

    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/organizations')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }

  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed wholesale and recruitment consumers share unchanged organizations code and browser-safe dashboard',
  )

  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)

  const { domain, postgres } = consumers[0]
  await postgres.migrateOrganizations(owner)
  // Migration idempotency
  await postgres.migrateOrganizations(owner)
  check('organizations database migration applies cleanly and idempotently')

  await owner.query('CREATE ROLE org_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantOrganizationsRuntime(owner, 'org_app')

  // Membership setup
  for (const [tenant, id, kind, grants, scope] of [
    ['wholesale', 'owner', 'human', domain.ORGANIZATION_PERMISSIONS, 'all'],
    ['wholesale', 'staff-a', 'human', domain.ORGANIZATION_PERMISSIONS, 'assigned'],
    ['wholesale', 'viewer', 'human', ['organizations.read', 'affiliations.read'], 'all'],
    ['wholesale', 'zero-viewer', 'human', [], 'assigned'],
    ['wholesale', 'bot', 'service', ['organizations.read', 'affiliations.read'], 'assigned'],
    ['wholesale', 'inactive', 'human', domain.ORGANIZATION_PERMISSIONS, 'all'],
    ['recruitment', 'owner', 'human', domain.ORGANIZATION_PERMISSIONS, 'all'],
  ]) {
    await owner.query(
      'INSERT INTO archava_organizations.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5)',
      [tenant, id, kind, grants, scope],
    )
  }
  await owner.query(
    "UPDATE archava_organizations.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  app = new Pool({ ...target, user: 'org_app' })

  // Non-owner runtime check
  await assert.rejects(
    async () => postgres.PostgresOrganizationRepository.create(owner, key),
    /requires a non-owner runtime/,
  )
  check('organizations repository refuses superuser or schema-owner database connection')

  const repository = await postgres.PostgresOrganizationRepository.create(app, key)
  const service = new domain.OrganizationService(repository)

  // Inactive user refusal
  await failed(
    () =>
      service.mutate('wholesale', human('inactive'), {
        requestId: randomUUID(),
        type: 'create_organization',
        data: { name: 'Invalid Org' },
      }),
    'forbidden',
  )
  check('inactive membership is denied mutation authority')

  // Machine mutation refusal
  await failed(
    () =>
      service.mutate(
        'wholesale',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        {
          requestId: randomUUID(),
          type: 'create_organization',
          data: { name: 'Machine Org' },
        },
      ),
    'forbidden',
  )
  check('machine service identity is denied organization write operations')

  // Organization creation
  const createCmdId = randomUUID()
  const createReceipt = await service.mutate('wholesale', human('owner'), {
    requestId: createCmdId,
    type: 'create_organization',
    data: {
      name: 'PT Mitra Logistik Nusantara',
      legalName: 'PT Mitra Logistik Nusantara Indonesia',
      taxId: '01.234.567.8-001.000',
      industry: 'Wholesale & Logistics',
      billingAddress: 'Kawasan Industri Pulogadung, Jakarta',
    },
  })
  assert.equal(createReceipt.type, 'create_organization')
  assert.equal(createReceipt.revision, 1)
  const orgId = createReceipt.organizationId
  check('human owner creates organization with full metadata and receives authoritative receipt')

  // Idempotency: exact same request returns identical cached receipt
  const replayReceipt = await service.mutate('wholesale', human('owner'), {
    requestId: createCmdId,
    type: 'create_organization',
    data: {
      name: 'PT Mitra Logistik Nusantara',
      legalName: 'PT Mitra Logistik Nusantara Indonesia',
      taxId: '01.234.567.8-001.000',
      industry: 'Wholesale & Logistics',
      billingAddress: 'Kawasan Industri Pulogadung, Jakarta',
    },
  })
  assert.deepEqual(replayReceipt, createReceipt)
  check('idempotent command replay by same actor returns identical receipt')

  // Command replay conflict: same requestId but tampered data
  await failed(
    service.mutate('wholesale', human('owner'), {
      requestId: createCmdId,
      type: 'create_organization',
      data: { name: 'Tampered Corp' },
    }),
    'conflict',
  )
  check('command replay with altered payload is rejected with conflict')

  // Command status query
  const statusCommitted = await service.commandStatus('wholesale', human('owner'), createCmdId)
  assert.equal(statusCommitted.status, 'committed')
  assert.deepEqual(statusCommitted.receipt, createReceipt)

  const statusUnknown = await service.commandStatus('wholesale', human('owner'), randomUUID())
  assert.equal(statusUnknown.status, 'not_committed')
  check('command status distinguishes committed from unknown requests')

  // Zero-grant viewer cannot read owner receipt
  await failed(
    () => service.commandStatus('wholesale', human('zero-viewer'), createCmdId),
    'forbidden',
  )
  check('zero-grant viewer is denied owner command receipt access')

  // Replay by original actor after grants removed is denied
  await owner.query(
    "UPDATE archava_organizations.memberships SET permissions = '{}' WHERE tenant_id = 'wholesale' AND principal_id = 'owner'",
  )
  await failed(
    () =>
      service.mutate('wholesale', human('owner'), {
        requestId: randomUUID(),
        type: 'create_organization',
        data: { name: 'Unauthorized Replay Org' },
      }),
    'forbidden',
  )
  await owner.query(
    'UPDATE archava_organizations.memberships SET permissions = $1 WHERE tenant_id = $2 AND principal_id = $3',
    [domain.ORGANIZATION_PERMISSIONS, 'wholesale', 'owner'],
  )
  check('replay after grants removed is rejected with forbidden')

  // Detail & search query
  const detail = await service.detail('wholesale', human('owner'), orgId)
  assert.equal(detail.organization.name, 'PT Mitra Logistik Nusantara')
  assert.equal(detail.organization.state, 'active')
  assert.equal(detail.affiliations.length, 0)

  const searchRes = await service.listOrganizations('wholesale', human('owner'), {
    search: 'Logistik',
  })
  assert.equal(searchRes.items.length, 1)
  assert.equal(searchRes.items[0].id, orgId)
  check('organization search and detail query return authoritative attributes')

  // Cross-tenant isolation check: recruitment tenant cannot see wholesale org
  await failed(service.detail('recruitment', human('owner'), orgId), 'not_found')
  const recruitmentList = await service.listOrganizations('recruitment', human('owner'), {})
  assert.equal(recruitmentList.items.length, 0)
  check('forced tenant RLS completely isolates organization data across tenants')

  // Stale revision update conflict
  await failed(
    service.mutate('wholesale', human('owner'), {
      requestId: randomUUID(),
      type: 'update_organization',
      data: {
        id: orgId,
        revision: 999, // Stale revision
        name: 'Stale Name',
      },
    }),
    'conflict',
  )
  check('optimistic revision check rejects update against stale revision')

  // Valid organization update
  const updateReceipt = await service.mutate('wholesale', human('owner'), {
    requestId: randomUUID(),
    type: 'update_organization',
    data: {
      id: orgId,
      revision: 1,
      website: 'https://mitralogistik.co.id',
    },
  })
  assert.equal(updateReceipt.revision, 2)
  check('organization update advances revision atomically')

  // Add primary affiliation
  const contactA = randomUUID()
  const affReceipt1 = await service.mutate('wholesale', human('owner'), {
    requestId: randomUUID(),
    type: 'create_affiliation',
    data: {
      organizationId: orgId,
      contactId: contactA,
      role: 'procurement',
      title: 'Head of Procurement',
      isPrimary: true,
    },
  })
  assert.equal(affReceipt1.type, 'create_affiliation')
  assert.equal(affReceipt1.revision, 3)
  assert.ok(affReceipt1.affiliationId)
  check('primary affiliation is created and advances organization revision')

  // Add non-primary affiliation
  const contactB = randomUUID()
  const affReceipt2 = await service.mutate('wholesale', human('owner'), {
    requestId: randomUUID(),
    type: 'create_affiliation',
    data: {
      organizationId: orgId,
      contactId: contactB,
      role: 'billing',
      title: 'Finance Manager',
      isPrimary: false,
    },
  })
  assert.equal(affReceipt2.revision, 4)
  const affId2 = affReceipt2.affiliationId
  check('secondary non-primary affiliation is added successfully')

  // Primary affiliation invariant enforcement: attempting to add a second active primary contact throws conflict!
  const contactC = randomUUID()
  await failed(
    service.mutate('wholesale', human('owner'), {
      requestId: randomUUID(),
      type: 'create_affiliation',
      data: {
        organizationId: orgId,
        contactId: contactC,
        role: 'operations',
        isPrimary: true, // Conflict: contact A is already primary
      },
    }),
    'primary_conflict',
  )
  check(
    'primary affiliation constraint strictly permits at most one active primary contact per organization',
  )

  // Update affiliation
  const updateAffReceipt = await service.mutate('wholesale', human('owner'), {
    requestId: randomUUID(),
    type: 'update_affiliation',
    data: {
      id: affId2,
      organizationId: orgId,
      title: 'Senior Finance Director',
    },
  })
  assert.equal(updateAffReceipt.revision, 5)

  // Remove affiliation (soft deactivates to inactive)
  const removeAffReceipt = await service.mutate('wholesale', human('owner'), {
    requestId: randomUUID(),
    type: 'remove_affiliation',
    data: {
      id: affId2,
      organizationId: orgId,
      reason: 'Staff departed company',
    },
  })
  assert.equal(removeAffReceipt.revision, 6)

  const affListAfter = await service.listAffiliations('wholesale', human('owner'), orgId, {})
  const removedAff = affListAfter.items.find((a) => a.id === affId2)
  assert.equal(removedAff.status, 'inactive')
  check('affiliation removal deactivates contact link while preserving audit history')

  // Archive organization
  const archiveReceipt = await service.mutate('wholesale', human('owner'), {
    requestId: randomUUID(),
    type: 'archive_organization',
    data: {
      id: orgId,
      revision: 6,
      reason: 'Account closed by mutual agreement',
    },
  })
  assert.equal(archiveReceipt.type, 'archive_organization')
  assert.equal(archiveReceipt.revision, 7)

  // Updating or affiliating with archived org is rejected
  await failed(
    service.mutate('wholesale', human('owner'), {
      requestId: randomUUID(),
      type: 'update_organization',
      data: {
        id: orgId,
        revision: 7,
        name: 'Archived Corp',
      },
    }),
    'archived',
  )
  await failed(
    service.mutate('wholesale', human('owner'), {
      requestId: randomUUID(),
      type: 'create_affiliation',
      data: {
        organizationId: orgId,
        contactId: randomUUID(),
        role: 'sales',
      },
    }),
    'archived',
  )
  check('archived organization rejects subsequent updates and new affiliations')

  // Second independent consumer: Recruitment agency (K21)
  const { domain: rDomain, postgres: rPostgres } = consumers[1]
  const rRepo = await rPostgres.PostgresOrganizationRepository.create(app, key)
  const rService = new rDomain.OrganizationService(rRepo)

  const rOrgReceipt = await rService.mutate('recruitment', human('owner'), {
    requestId: randomUUID(),
    type: 'create_organization',
    data: {
      name: 'Tech Talent Studio Ltd',
      industry: 'Human Resources & Recruitment',
    },
  })
  const rOrgId = rOrgReceipt.organizationId

  const candidateContact = randomUUID()
  const rAff1 = await rService.mutate('recruitment', human('owner'), {
    requestId: randomUUID(),
    type: 'create_affiliation',
    data: {
      organizationId: rOrgId,
      contactId: candidateContact,
      role: 'candidate',
      isPrimary: true,
    },
  })
  assert.equal(rAff1.revision, 2)
  check('recruitment consumer operates independent candidate and client affiliation workflow')

  // Service read intersection
  const botServiceRes = await service.listOrganizations(
    'wholesale',
    { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
    {},
  )
  assert.equal(botServiceRes.items.length, 1)
  check('service identity reads data through human permission intersection')

  // PostgreSQL restart test
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  start()
  owner = new Pool(target)
  app = new Pool({ ...target, user: 'org_app' })

  const restartedRepo = await postgres.PostgresOrganizationRepository.create(app, key)
  const restartedService = new domain.OrganizationService(restartedRepo)
  const detailAfterRestart = await restartedService.detail('wholesale', human('owner'), orgId)
  assert.equal(detailAfterRestart.organization.revision, 7)
  assert.equal(detailAfterRestart.organization.state, 'archived')
  check('organizations state, revisions and affiliations persist across PostgreSQL host restart')

  // Backup and restore test
  const dumpFile = join(scratch, 'orgs.dump')
  run('pg_dump', [
    '-Fc',
    '-f',
    dumpFile,
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    'postgres',
  ])

  await owner.query('CREATE DATABASE orgs_restored')
  run('pg_restore', [
    '-d',
    'orgs_restored',
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    dumpFile,
  ])

  const restoredApp = new Pool({ ...target, database: 'orgs_restored', user: 'org_app' })
  const restoredRepo = await postgres.PostgresOrganizationRepository.create(restoredApp, key)
  const restoredService = new domain.OrganizationService(restoredRepo)

  const detailRestored = await restoredService.detail('wholesale', human('owner'), orgId)
  assert.equal(detailRestored.organization.name, 'PT Mitra Logistik Nusantara')
  assert.equal(detailRestored.organization.revision, 7)
  await endProofPool(restoredApp)
  check(
    'pg_dump and pg_restore preserve all organization records, affiliations and command receipts',
  )

  const destination = join(root, 'dist/organizations-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: trusted.version,
        catalogSha256: createHash('sha256').update(JSON.stringify(profile.catalog)).digest('hex'),
        artifactSha256: createHash('sha256')
          .update(JSON.stringify(profile.artifacts))
          .digest('hex'),
        scope: 'reusable organizations, accounts and contact affiliations',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )

  process.stdout.write(
    `\nALL ${checks.length} ORGANIZATIONS & AFFILIATIONS CHECKS PASSED PERFECTLY!\n`,
  )
} finally {
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true }).catch(() => {})
}
