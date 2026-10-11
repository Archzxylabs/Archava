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

const root = resolve(import.meta.dirname, '../..')
const library = join(root, 'dist/library')
const profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8'))
const trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8'))
const scratch = await mkdtemp(join(tmpdir(), 'archava-proposals-proof-'))
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
const failed = async (fn, code) => {
  try {
    await (typeof fn === 'function' ? fn() : fn)
    assert.fail(`Expected rejection with code ${code}, but call succeeded`)
  } catch (error) {
    if (error?.code !== code) {
      throw error
    }
    assert.equal(error.code, code)
  }
}

try {
  const consumers = []
  // Two independent consumers: Agency (K05) and Construction/EPC (K31)
  for (const name of ['agency', 'epc']) {
    const directory = join(scratch, name)
    await mkdir(directory)
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify({
        name: 'independent-proposals-' + name,
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
      env: { ...env, NODE_PATH: '' },
    })

    const require = createRequire(join(directory, 'package.json'))
    const load = async (pkg) => import(pathToFileURL(require.resolve(pkg)).href)

    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))

    const domain = await load('@archava/proposals')
    const postgres = await load('@archava/proposals-pg')
    Pool ??= require('pg').Pool

    const browser = await build({
      entryPoints: [require.resolve('@archava/proposals/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })

    assert.equal(
      Object.keys(browser.metafile.inputs).some((x) =>
        /proposals-pg|identity-pg|crm-pg|node:/.test(x),
      ),
      false,
    )

    consumers.push({ domain, postgres, name, directory })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require.resolve('@archava/proposals')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }

  assert.equal(new Set(proofs.map((x) => x.implementationSha256)).size, 1)
  check(
    'two independently installed agency and EPC consumers share unchanged proposals code and browser-safe dashboard',
  )

  run('initdb', ['-D', data, '-U', 'postgres', '--auth=trust', '--no-sync'])
  start()
  owner = new Pool(target)

  const { domain, postgres } = consumers[0]
  await postgres.migrateProposals(owner)
  // Idempotent migration
  await postgres.migrateProposals(owner)
  check('proposals database migration applies cleanly and idempotently')

  await owner.query('CREATE ROLE prop_app LOGIN NOSUPERUSER NOBYPASSRLS')
  await postgres.grantProposalsRuntime(owner, 'prop_app')

  // Membership setup
  for (const [tenant, id, kind, grants, scope] of [
    ['agency', 'owner', 'human', domain.PROPOSAL_PERMISSIONS, 'all'],
    ['agency', 'lead-pm', 'human', domain.PROPOSAL_PERMISSIONS, 'assigned'],
    ['agency', 'client-reviewer', 'human', ['proposals.read', 'proposals.decide', 'changerequests.read', 'changerequests.decide'], 'all'],
    ['agency', 'bot', 'service', ['proposals.read', 'changerequests.read'], 'assigned'],
    ['agency', 'inactive', 'human', domain.PROPOSAL_PERMISSIONS, 'all'],
    ['epc', 'owner', 'human', domain.PROPOSAL_PERMISSIONS, 'all'],
  ]) {
    await owner.query(
      'INSERT INTO archava_proposals.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5)',
      [tenant, id, kind, grants, scope],
    )
  }
  await owner.query(
    "UPDATE archava_proposals.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  app = new Pool({ ...target, user: 'prop_app' })

  // Non-owner runtime check
  await assert.rejects(
    async () => postgres.PostgresProposalRepository.create(owner, key),
    /requires a non-owner runtime/,
  )
  check('proposals repository refuses superuser or schema-owner database connection')

  const repository = await postgres.PostgresProposalRepository.create(app, key)
  const service = new domain.ProposalService(repository)

  // Inactive user refusal
  await failed(
    () =>
      service.mutate('agency', human('inactive'), {
        type: 'create_proposal',
        commandId: randomUUID(),
        title: 'Forbidden Proposal',
        summary: 'Cannot proceed',
        scopeItems: [{ title: 'D1', description: 'Desc' }],
      }),
    'forbidden',
  )
  check('inactive membership is denied mutation authority')

  // Machine mutation refusal
  await failed(
    () =>
      service.mutate(
        'agency',
        { kind: 'service', id: 'bot', onBehalfOf: 'owner' },
        {
          type: 'create_proposal',
          commandId: randomUUID(),
          title: 'Machine Proposal',
          summary: 'Cannot proceed',
          scopeItems: [{ title: 'D1', description: 'Desc' }],
        },
      ),
    'forbidden',
  )
  check('machine service identity is denied proposal write operations')

  // Human owner creates draft proposal with deliverables
  const createCmdId = randomUUID()
  const createReceipt = await service.mutate('agency', human('owner'), {
    type: 'create_proposal',
    commandId: createCmdId,
    title: 'Brand Transformation & Design System',
    summary: 'Comprehensive brand identity and enterprise design kit.',
    currency: 'USD',
    scopeItems: [
      {
        title: 'Brand Guidelines',
        description: 'Typography, palette, logos',
        estimatedDays: 14,
        estimatedValue: 15000,
        deliverableType: 'milestone',
      },
      {
        title: 'Design System Component Library',
        description: 'Figma and code tokens for 40+ components',
        estimatedDays: 30,
        estimatedValue: 35000,
        deliverableType: 'service',
      },
    ],
  })

  assert.equal(createReceipt.status, 'committed')
  assert.equal(createReceipt.revision, 1)
  assert.equal(createReceipt.scopeVersion, 1)
  const proposalId = createReceipt.proposalId
  assert.ok(proposalId)
  check('human owner creates proposal with deliverables and receives authoritative receipt')

  // Idempotent command replay by same actor
  const replayReceipt = await service.mutate('agency', human('owner'), {
    type: 'create_proposal',
    commandId: createCmdId,
    title: 'Brand Transformation & Design System',
    summary: 'Comprehensive brand identity and enterprise design kit.',
    currency: 'USD',
    scopeItems: [
      {
        title: 'Brand Guidelines',
        description: 'Typography, palette, logos',
        estimatedDays: 14,
        estimatedValue: 15000,
        deliverableType: 'milestone',
      },
      {
        title: 'Design System Component Library',
        description: 'Figma and code tokens for 40+ components',
        estimatedDays: 30,
        estimatedValue: 35000,
        deliverableType: 'service',
      },
    ],
  })
  assert.deepEqual(replayReceipt, createReceipt)
  check('idempotent command replay by same actor returns identical receipt')

  // Replay with altered payload rejected with conflict
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'create_proposal',
        commandId: createCmdId,
        title: 'Tampered Title',
        summary: 'Altered',
        currency: 'USD',
        scopeItems: [{ title: 'D1', description: 'Desc' }],
      }),
    'conflict',
  )
  check('command replay with altered payload is rejected with conflict')

  // Command status check
  const committedStatus = await service.commandStatus('agency', human('owner'), createCmdId)
  assert.equal(committedStatus.status, 'committed')
  const unknownStatus = await service.commandStatus('agency', human('owner'), randomUUID())
  assert.equal(unknownStatus.status, 'unknown')
  check('command status distinguishes committed from unknown requests')

  // Proposal detail check
  const detail = await service.detail('agency', human('owner'), proposalId)
  assert.equal(detail.proposal.title, 'Brand Transformation & Design System')
  assert.equal(detail.proposal.totalEstimatedValue, 50000)
  assert.equal(detail.proposal.state, 'draft')
  assert.equal(detail.proposal.currentScopeVersion, 1)
  assert.equal(detail.currentItems.length, 2)
  check('proposal search and detail query return authoritative attributes and deliverables')

  // Stale revision rejection
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'update_proposal',
        commandId: randomUUID(),
        proposalId,
        expectedRevision: 999,
        title: 'Updated Title',
        summary: 'Updated summary',
      }),
    'conflict',
  )
  check('optimistic revision check rejects update against stale revision')

  // Draft update advances revision atomically
  const updateReceipt = await service.mutate('agency', human('owner'), {
    type: 'update_proposal',
    commandId: randomUUID(),
    proposalId,
    expectedRevision: 1,
    title: 'Brand Transformation & Design System v1.1',
    summary: 'Updated summary text',
  })
  assert.equal(updateReceipt.revision, 2)
  check('proposal update advances revision atomically')

  // Replace scope items in draft
  const setScopeReceipt = await service.mutate('agency', human('owner'), {
    type: 'set_scope_items',
    commandId: randomUUID(),
    proposalId,
    expectedRevision: 2,
    scopeItems: [
      {
        title: 'Brand Guidelines',
        description: 'Typography, palette, logos',
        estimatedDays: 14,
        estimatedValue: 20000,
        deliverableType: 'milestone',
      },
      {
        title: 'Design System Component Library',
        description: 'Figma and code tokens for 50+ components',
        estimatedDays: 30,
        estimatedValue: 40000,
        deliverableType: 'service',
      },
    ],
  })
  assert.equal(setScopeReceipt.revision, 3)
  const detailAfterItems = await service.detail('agency', human('owner'), proposalId)
  assert.equal(detailAfterItems.proposal.totalEstimatedValue, 60000)
  check('draft scope deliverables can be revised with total value updated')

  // Submit proposal for client review
  const submitReceipt = await service.mutate('agency', human('owner'), {
    type: 'submit_proposal',
    commandId: randomUUID(),
    proposalId,
    expectedRevision: 3,
  })
  assert.equal(submitReceipt.revision, 4)
  const detailAfterSubmit = await service.detail('agency', human('owner'), proposalId)
  assert.equal(detailAfterSubmit.proposal.state, 'submitted')
  check('proposal submits to client review and enters submitted state')

  // Direct edit on submitted proposal rejected
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'set_scope_items',
        commandId: randomUUID(),
        proposalId,
        expectedRevision: 4,
        scopeItems: [{ title: 'Illegal', description: 'Desc' }],
      }),
    'scope_frozen',
  )
  check('submitted proposal deliverables are frozen against direct modification')

  // Client reviewer accepts proposal -> Seals Baseline Scope Version 1
  const projectId = randomUUID()
  const acceptReceipt = await service.mutate('agency', human('client-reviewer'), {
    type: 'accept_proposal',
    commandId: randomUUID(),
    proposalId,
    expectedRevision: 4,
    linkedProjectId: projectId,
  })
  assert.equal(acceptReceipt.revision, 5)
  assert.equal(acceptReceipt.scopeVersion, 1)

  const detailAfterAccept = await service.detail('agency', human('owner'), proposalId)
  assert.equal(detailAfterAccept.proposal.state, 'accepted')
  assert.equal(detailAfterAccept.proposal.currentScopeVersion, 1)
  assert.equal(detailAfterAccept.proposal.linkedProjectId, projectId)
  assert.equal(detailAfterAccept.versions.length, 1)
  assert.equal(detailAfterAccept.versions[0]?.scopeVersion, 1)
  assert.equal(detailAfterAccept.versions[0]?.totalValue, 60000)
  assert.equal(detailAfterAccept.versions[0]?.items.length, 2)
  check('client acceptance seals baseline Scope Version 1 and records immutable snapshot')

  // Direct modification on accepted proposal rejected
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'update_proposal',
        commandId: randomUUID(),
        proposalId,
        expectedRevision: 5,
        title: 'Illegal post-accept edit',
        summary: 'Cannot edit',
      }),
    'scope_frozen',
  )
  check('accepted proposal rejects direct edits and requires versioned change requests')

  // Forced tenant RLS isolation
  await failed(
    () => service.detail('epc', human('owner'), proposalId),
    'not_found',
  )
  check('forced tenant RLS completely isolates proposals across tenants')

  // Change request on non-accepted proposal rejected
  const draftPropReceipt = await service.mutate('agency', human('owner'), {
    type: 'create_proposal',
    commandId: randomUUID(),
    title: 'Draft Unaccepted Proposal',
    summary: 'Will stay draft',
    currency: 'USD',
    scopeItems: [{ title: 'D1', description: 'Desc' }],
  })
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'create_change_request',
        commandId: randomUUID(),
        proposalId: draftPropReceipt.proposalId,
        title: 'CR on Draft',
        reason: 'Illegal',
        deltaSummary: 'Cannot create CR on draft',
        newScopeItems: [{ title: 'Item', description: 'Desc' }],
      }),
    'invalid_state',
  )
  check('change request creation on unaccepted proposal is rejected')

  // Create Change Request CR-1 on accepted proposal
  const cr1Receipt = await service.mutate('agency', human('owner'), {
    type: 'create_change_request',
    commandId: randomUUID(),
    proposalId,
    title: 'Add Mobile App UI Components',
    reason: 'Client requested extension to native mobile screen design.',
    deltaSummary: '15 mobile screen components and navigation token set.',
    timeDeltaDays: 14,
    costDeltaValue: 12000,
    newScopeItems: [
      {
        title: 'Mobile Screens & Navigation System',
        description: 'iOS and Android tokens and screens',
        estimatedDays: 14,
        estimatedValue: 12000,
        deliverableType: 'service',
      },
    ],
  })
  assert.equal(cr1Receipt.revision, 1)
  const changeRequestId = cr1Receipt.changeRequestId
  assert.ok(changeRequestId)
  check('change request CR-1 is created against accepted proposal')

  // Attempt to create second concurrent open CR rejected
  await failed(
    () =>
      service.mutate('agency', human('owner'), {
        type: 'create_change_request',
        commandId: randomUUID(),
        proposalId,
        title: 'Concurrent CR-2',
        reason: 'Should be rejected',
        deltaSummary: 'Only one open CR permitted',
        newScopeItems: [{ title: 'Item', description: 'Desc' }],
      }),
    'active_change_request_exists',
  )
  check('concurrent open change request creation is strictly rejected')

  // Submit CR-1
  const submitCrReceipt = await service.mutate('agency', human('owner'), {
    type: 'submit_change_request',
    commandId: randomUUID(),
    changeRequestId,
    expectedRevision: 1,
  })
  assert.equal(submitCrReceipt.revision, 2)
  check('change request transitions from draft to submitted')

  // Client approves CR-1 -> Advances Scope Version to Version 2!
  const approveCrReceipt = await service.mutate('agency', human('client-reviewer'), {
    type: 'approve_change_request',
    commandId: randomUUID(),
    changeRequestId,
    expectedRevision: 2,
  })
  assert.equal(approveCrReceipt.revision, 3)
  assert.equal(approveCrReceipt.scopeVersion, 2)

  const detailAfterCr = await service.detail('agency', human('owner'), proposalId)
  assert.equal(detailAfterCr.proposal.currentScopeVersion, 2)
  assert.equal(detailAfterCr.proposal.totalEstimatedValue, 72000) // 60,000 + 12,000
  assert.equal(detailAfterCr.versions.length, 2)
  assert.equal(detailAfterCr.versions[1]?.scopeVersion, 2)
  assert.equal(detailAfterCr.versions[1]?.totalValue, 72000)
  assert.equal(detailAfterCr.changeRequests[0]?.state, 'approved')
  assert.equal(detailAfterCr.changeRequests[0]?.resultingScopeVersion, 2)
  check('change request approval advances scope version to v2 and appends amendment deliverables')

  // Second consumer: Construction/EPC (K31) site variation claim workflow
  const { domain: epcDomain, postgres: epcPostgres } = consumers[1]
  const epcRepo = await epcPostgres.PostgresProposalRepository.create(app, key)
  const epcService = new epcDomain.ProposalService(epcRepo)

  const epcCreateReceipt = await epcService.mutate('epc', human('owner'), {
    type: 'create_proposal',
    commandId: randomUUID(),
    title: 'Industrial Warehouse Structural Steel Contract',
    summary: 'Fabrication and erection of 2,400 m2 structural frame.',
    currency: 'IDR',
    scopeItems: [
      {
        title: 'Structural Steel Erection',
        description: 'Foundation anchor bolts and main portal frames',
        estimatedDays: 45,
        estimatedValue: 750000000,
        deliverableType: 'milestone',
      },
      {
        title: 'Roof Trusses & Purlins',
        description: 'Cold formed z-purlins and safety nets',
        estimatedDays: 20,
        estimatedValue: 250000000,
        deliverableType: 'milestone',
      },
    ],
  })
  const epcPropId = epcCreateReceipt.proposalId

  // Submit and accept EPC contract
  await epcService.mutate('epc', human('owner'), {
    type: 'submit_proposal',
    commandId: randomUUID(),
    proposalId: epcPropId,
    expectedRevision: 1,
  })
  await epcService.mutate('epc', human('owner'), {
    type: 'accept_proposal',
    commandId: randomUUID(),
    proposalId: epcPropId,
    expectedRevision: 2,
    linkedProjectId: randomUUID(),
  })

  // EPC variation claim CR (unforeseen subgrade soil condition requiring footing enlargement)
  const epcCrReceipt = await epcService.mutate('epc', human('owner'), {
    type: 'create_change_request',
    commandId: randomUUID(),
    proposalId: epcPropId,
    title: 'Site Variation Claim #01 - Foundation Enlargement',
    reason: 'Geotechnical borehole inspection revealed soft clay layer at Grid 3-B.',
    deltaSummary: 'Additional 18 m3 reinforced concrete foundation pad and rebar.',
    timeDeltaDays: 7,
    costDeltaValue: 85000000,
    newScopeItems: [
      {
        title: 'Pad Footing Enlargement & Excavation',
        description: 'Mass concrete blinding and Grade 35 concrete',
        estimatedDays: 7,
        estimatedValue: 85000000,
        deliverableType: 'milestone',
      },
    ],
  })

  await epcService.mutate('epc', human('owner'), {
    type: 'submit_change_request',
    commandId: randomUUID(),
    changeRequestId: epcCrReceipt.changeRequestId,
    expectedRevision: 1,
  })

  const epcApprovedCr = await epcService.mutate('epc', human('owner'), {
    type: 'approve_change_request',
    commandId: randomUUID(),
    changeRequestId: epcCrReceipt.changeRequestId,
    expectedRevision: 2,
  })
  assert.equal(epcApprovedCr.scopeVersion, 2)

  const epcDetail = await epcService.detail('epc', human('owner'), epcPropId)
  assert.equal(epcDetail.proposal.currentScopeVersion, 2)
  assert.equal(epcDetail.proposal.totalEstimatedValue, 1085000000) // 1,000,000,000 + 85,000,000
  check('EPC consumer operates independent contract variation claim workflow to v2')

  // Service identity read-only via human permission intersection
  const botPrincipal = { kind: 'service', id: 'bot', onBehalfOf: 'owner' }
  const botDetail = await service.detail('agency', botPrincipal, proposalId)
  assert.equal(botDetail.proposal.id, proposalId)
  assert.equal(botDetail.versions.length, 2)
  check('service identity reads proposal data through human permission intersection')

  // Restart PostgreSQL and verify persistence
  await app.end()
  await owner.end()
  stop()
  start()
  owner = new Pool(target)
  app = new Pool({ ...target, user: 'prop_app' })

  const restartedRepo = await postgres.PostgresProposalRepository.create(app, key)
  const restartedService = new domain.ProposalService(restartedRepo)
  const persistedDetail = await restartedService.detail('agency', human('owner'), proposalId)
  assert.equal(persistedDetail.proposal.title, 'Brand Transformation & Design System v1.1')
  assert.equal(persistedDetail.proposal.currentScopeVersion, 2)
  assert.equal(persistedDetail.versions.length, 2)
  assert.equal(persistedDetail.changeRequests.length, 1)
  assert.equal(persistedDetail.changeRequests[0]?.state, 'approved')
  check('proposals state, versions and change requests persist across PostgreSQL host restart')

  // pg_dump and pg_restore test
  const dumpFile = join(scratch, 'proposals.dump')
  run('pg_dump', ['-Fc', '-f', dumpFile, '-h', socket, '-p', String(port), '-U', 'postgres', 'postgres'])

  await owner.query('CREATE DATABASE prop_restored')
  run('pg_restore', [
    '-d',
    'prop_restored',
    '-h',
    socket,
    '-p',
    String(port),
    '-U',
    'postgres',
    dumpFile,
  ])

  const restoredApp = new Pool({ ...target, database: 'prop_restored', user: 'prop_app' })
  const restoredRepo = await postgres.PostgresProposalRepository.create(restoredApp, key)
  const restoredService = new domain.ProposalService(restoredRepo)
  const restoredDetail = await restoredService.detail('agency', human('owner'), proposalId)
  assert.equal(restoredDetail.proposal.title, 'Brand Transformation & Design System v1.1')
  assert.equal(restoredDetail.proposal.currentScopeVersion, 2)
  assert.equal(restoredDetail.proposal.totalEstimatedValue, 72000)
  assert.equal(restoredDetail.versions.length, 2)
  assert.equal(restoredDetail.changeRequests[0]?.state, 'approved')
  await endProofPool(restoredApp)
  check('pg_dump and pg_restore preserve all proposal records, versions and change requests')

  process.stdout.write(`\nALL ${checks.length} PROPOSALS, AGREED SCOPE & AMENDMENT CHECKS PASSED PERFECTLY!\n`)
} finally {
  await endProofPool(app)
  await endProofPool(owner)
  stop()
  await rm(scratch, { recursive: true, force: true }).catch(() => {})
}
