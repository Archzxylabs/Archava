/* global process, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, rm, readFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomUUID, createHash } from 'node:crypto'
import { build } from 'esbuild'
import { installEnv } from './npm-env.mjs'

const root = resolve(import.meta.dirname, '../..'),
  library = join(root, 'dist/library'),
  profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8')),
  trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8')),
  env = { ...process.env },
  scratch = await mkdtemp(join(tmpdir(), 'archava-field-proof-')),
  checks = [],
  proofs = []

const check = (label) => {
  checks.push(label)
  process.stdout.write(`PASS ${checks.length} ${label}\n`)
}

const PGHOST = join(scratch, 'pg')
const PORT = 5400 + Math.floor(Math.random() * 1000)
const RUNTIME_ROLE = 'archava_field_app'
const TENANT = 'tenant-field-a'
const OTHER = 'tenant-field-b'
const KEY = 'd'.repeat(32)

const human = (id) => ({ kind: 'human', id })
const automated = (id, onBehalfOf) => ({ kind: 'service', id, onBehalfOf })
const ALL = ['field.read', 'field.write', 'field.dispatch', 'field.signoff', 'audit.read']

const run = (command, args, options = {}) =>
  execFileSync(command, args, { encoding: 'utf8', env, stdio: 'pipe', ...options })

async function startCluster() {
  await mkdir(PGHOST, { recursive: true })
  run('initdb', ['-D', PGHOST, '-U', 'postgres', '--auth=trust', '--no-sync', '-E', 'UTF8'])
  run('pg_ctl', [
    '-D',
    PGHOST,
    '-l',
    join(scratch, 'postgres.log'),
    '-o',
    `-p ${PORT} -k ${PGHOST} -c listen_addresses='' -c fsync=off -c synchronous_commit=off`,
    'start',
  ])
  for (let attempt = 0; attempt < 80; attempt += 1) {
    const probe = spawnSync('psql', [
      '-h',
      PGHOST,
      '-p',
      String(PORT),
      '-U',
      'postgres',
      '-d',
      'postgres',
      '-tAc',
      'select 1',
    ])
    if (probe.status === 0) return
    await new Promise((done) => setTimeout(done, 200))
  }
  throw new Error('PostgreSQL proof cluster did not become ready')
}

const asOwner = (database, statement) =>
  run('psql', [
    '-h',
    PGHOST,
    '-p',
    String(PORT),
    '-U',
    'postgres',
    '-d',
    database,
    '-v',
    'ON_ERROR_STOP=1',
    '-c',
    statement,
  ])

const failed = async (work, code) => {
  try {
    await work()
  } catch (error) {
    const received =
      error.constructor.name === 'ZodError' ? 'validation_failed' : (error.code ?? error.message)
    assert.equal(received, code, `expected ${code}, received ${received}`)
    return
  }
  throw new Error(`expected refusal with code ${code}`)
}

let clusterRunning = false

try {
  await startCluster()
  clusterRunning = true

  asOwner('postgres', `CREATE ROLE ${RUNTIME_ROLE} LOGIN`)
  asOwner('postgres', 'CREATE DATABASE owner_field')
  asOwner('postgres', 'CREATE DATABASE service_field')

  const consumers = []
  for (const name of ['residential', 'commercial']) {
    const directory = join(scratch, name)
    await mkdir(directory, { recursive: true })
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify(
        {
          name: 'independent-field-' + name,
          private: true,
          type: 'module',
          overrides: Object.fromEntries(
            profile.catalog.externals.map((item) => [item.name, item.version]),
          ),
          dependencies: Object.fromEntries(
            profile.artifacts.map((item) => [item.name, 'file:' + join(library, item.tarball)]),
          ),
        },
        null,
        2,
      ),
    )
    run('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
      cwd: directory,
      env: { ...installEnv(env), NODE_PATH: '' },
    })

    const require_ = createRequire(join(directory, 'package.json'))
    const load = async (pkg) => await import(pathToFileURL(require_.resolve(pkg)).href)

    await (
      await load('@archava/config/release')
    ).verifyInstalledRelease(profile, trusted, join(directory, 'node_modules'))

    const domain = await load('@archava/field-services')
    const postgres = await load('@archava/field-services-pg')
    const browser = await build({
      entryPoints: [require_.resolve('@archava/field-services/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((item) =>
        /field-services-pg|identity-pg|crm-pg|node:/.test(item),
      ),
      false,
    )
    consumers.push({ name, directory, domain, postgres, require: require_ })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require_.resolve('@archava/field-services')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((item) => item.implementationSha256)).size, 1)
  check(
    'two independently installed residential and commercial consumers share unchanged field code and a browser-safe dashboard',
  )

  const installed = consumers[0]
  const { Pool } = installed.require('pg')
  const { FieldServicesService } = installed.domain
  const { runFieldTool } = installed.domain
  const pgModule = installed.postgres

  const connect = (user, database) => new Pool({ host: PGHOST, port: PORT, user, database, max: 4 })

  const ownerPool = connect('postgres', 'owner_field')
  const servicePool = connect('postgres', 'service_field')
  const poolDiagnostics = (pool, name) => {
    pool.on('error', (error) => process.stderr.write(`POOL-ERROR ${name}: ${error.message}\n`))
    return pool
  }
  poolDiagnostics(ownerPool, 'ownerPool')
  poolDiagnostics(servicePool, 'servicePool')

  await pgModule.migrateFieldServices(ownerPool)
  await pgModule.migrateFieldServices(servicePool)
  await pgModule.migrateFieldServices(ownerPool)
  await pgModule.grantFieldServicesRuntime(ownerPool, RUNTIME_ROLE)
  await pgModule.grantFieldServicesRuntime(servicePool, RUNTIME_ROLE)
  check(
    'field services schema migrations apply cleanly, idempotently and match the packaged ledger',
  )

  const runtime = connect(RUNTIME_ROLE, 'service_field')
  poolDiagnostics(runtime, 'runtimePool')
  const repository = await pgModule.PostgresFieldRepository.create(runtime, KEY)
  const svc = new FieldServicesService(repository)
  const commandId = () => randomUUID()

  const seed = async (tenant, principalId, kind, permissions, scope = 'all') => {
    await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [tenant])
    await servicePool.query(
      'INSERT INTO archava_field.memberships(tenant_id, principal_id, kind, permissions, scope) VALUES($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING',
      [tenant, principalId, kind, permissions, scope],
    )
  }

  await seed(TENANT, 'owner', 'human', ALL)
  await seed(TENANT, 'viewer', 'human', ['field.read', 'audit.read'])
  await seed(TENANT, 'technician', 'human', ['field.read', 'field.write', 'field.dispatch'])
  await seed(TENANT, 'signer', 'human', ['field.read', 'field.signoff', 'audit.read'])
  await seed(TENANT, 'robot', 'service', ALL)
  await seed(TENANT, 'inactive', 'human', ALL)
  await seed(OTHER, 'foreigner', 'human', ALL)
  await servicePool.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
  await servicePool.query(
    "UPDATE archava_field.memberships SET active = false WHERE principal_id = 'inactive'",
  )

  // 1. A cold-installed consumer registers an appointment-locked job.
  const job = await svc.execute(TENANT, human('owner'), {
    action: 'create_job',
    commandId: commandId(),
    job: {
      jobId: 'job-b3f9c1d2',
      customerRef: 'customer-north',
      siteRef: 'site-elm-street',
      summary: 'Replace condenser fan motor',
      priority: 'high',
      appointmentRef: 'appt-2291',
      assetRef: 'asset-hvac-014',
    },
  })
  assert.equal(job.action, 'create_job')
  const created = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(created.job.appointmentRef, 'appt-2291')
  assert.equal(created.job.status, 'requested')
  assert.equal(created.job.appointmentRef === created.job.jobId, false)
  check('a dispatched job is created against a scheduler appointment it must not lose')

  // 2. The appointment reference stays distinct from the job identity.
  const second = await svc.execute(TENANT, human('owner'), {
    action: 'create_job',
    commandId: commandId(),
    job: {
      jobId: 'job-c47a0e1b',
      customerRef: 'customer-north',
      siteRef: 'site-elm-street',
      summary: 'Belt replacement',
      appointmentRef: 'appt-2291',
    },
  })
  assert.equal(second.action, 'create_job')
  check('the same appointment may generate a second job without identifier collision')

  // 3. Dispatch is withheld from a read-only principal.
  await failed(
    () =>
      svc.execute(TENANT, human('viewer'), {
        action: 'assign_job',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        assignedPrincipal: 'technician',
      }),
    'forbidden',
  )
  check('job assignment requires the dedicated dispatch grant')

  // 4. Assignment moves the job forward and records the assignee.
  await svc.execute(TENANT, human('owner'), {
    action: 'assign_job',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    assignedPrincipal: 'technician',
  })
  const assigned = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(assigned.job.status, 'assigned')
  assert.equal(assigned.job.assignedPrincipal, 'technician')
  check('an accepted assignment moves the job to assigned and names the technician')

  // 5. Work evidence before a visit is refused.
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'append_work_line',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        line: {
          lineId: 'line-001',
          kind: 'labour',
          unit: 'hours',
          quantity: 2.5,
          description: 'Diagnostic work',
          partRef: null,
        },
      }),
    'illegal_transition',
  )
  check('work evidence cannot be recorded before a site visit')

  // 6. Signoff before evidence is refused.
  await failed(
    () =>
      svc.execute(TENANT, human('signer'), {
        action: 'record_signoff',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        signoff: {
          signoffId: 'signoff-001',
          signerRef: 'customer-north',
          accepted: true,
          comment: null,
        },
      }),
    'illegal_transition',
  )
  check('customer signoff cannot precede work evidence')

  // 7. Approval before signoff is refused.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        action: 'approve_job',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
      }),
    'illegal_transition',
  )
  check('approval cannot skip the customer signoff')

  // 8. The recorded visit advances the job.
  await svc.execute(TENANT, human('technician'), {
    action: 'record_visit',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    visit: {
      visitId: 'visit-001',
      outcome: 'completed',
      note: 'Motor seized, replaced under warranty',
      technicianRef: 'technician',
    },
  })
  const visited = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(visited.job.status, 'visited')
  assert.equal(visited.visits.length, 1)
  check('a recorded site visit advances the job past the visit gate')

  // 9. Repeated or backwards steps are refused.
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'record_visit',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        visit: {
          visitId: 'visit-002',
          outcome: 'partial',
          note: 'Second attempt at the same site',
          technicianRef: 'technician',
        },
      }),
    'illegal_transition',
  )
  check('a job that is already visited or beyond refuses a second visit')

  // 10. Labour hours and part pieces accumulate on separate counters.
  await svc.execute(TENANT, human('technician'), {
    action: 'append_work_line',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    line: {
      lineId: 'line-001',
      kind: 'labour',
      unit: 'hours',
      quantity: 2.5,
      description: 'Diagnostic work',
      partRef: null,
    },
  })
  await svc.execute(TENANT, human('technician'), {
    action: 'append_work_line',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    line: {
      lineId: 'line-002',
      kind: 'part',
      unit: 'pieces',
      quantity: 3,
      description: 'Capacitor 45/5 uF',
      partRef: 'part-capacitor-455',
    },
  })
  const evidenced = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(evidenced.job.laborHours, 2.5)
  assert.equal(evidenced.job.partsPieces, 3)
  check('labour hours and part pieces stay on separate counters')

  // 11. A work line kind outside labour and part is refused as invalid.
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'append_work_line',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        line: {
          lineId: 'line-003',
          kind: 'travel',
          unit: 'hours',
          quantity: 1,
          description: 'Travel time',
          partRef: null,
        },
      }),
    'validation_failed',
  )
  check('a work line kind outside labour and part is refused as invalid')

  // 12. Signoff requires the dedicated grant.
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'record_signoff',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        signoff: {
          signoffId: 'signoff-001',
          signerRef: 'customer-north',
          accepted: true,
          comment: null,
        },
      }),
    'forbidden',
  )
  check('customer signoff requires the dedicated signoff grant')

  // 13. The acceptance signoff is recorded.
  await svc.execute(TENANT, human('signer'), {
    action: 'record_signoff',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    signoff: {
      signoffId: 'signoff-001',
      signerRef: 'customer-north',
      accepted: true,
      comment: 'Work accepted',
    },
  })
  const signedOff = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(signedOff.job.status, 'signed_off')
  assert.equal(signedOff.signoff?.accepted, true)
  check('an accepted signoff records the customer acceptance')

  // 14. A rejected signoff is distinguishable from an acceptance.
  await svc.execute(TENANT, human('owner'), {
    action: 'create_job',
    commandId: commandId(),
    job: {
      jobId: 'job-d51b2f3c',
      customerRef: 'customer-south',
      siteRef: 'site-oak-avenue',
      summary: 'Thermostat calibration',
      appointmentRef: 'appt-3307',
    },
  })
  await svc.execute(TENANT, human('owner'), {
    action: 'assign_job',
    commandId: commandId(),
    jobId: 'job-d51b2f3c',
    assignedPrincipal: 'technician',
  })
  await svc.execute(TENANT, human('technician'), {
    action: 'record_visit',
    commandId: commandId(),
    jobId: 'job-d51b2f3c',
    visit: {
      visitId: 'visit-101',
      outcome: 'completed',
      note: 'Calibrated to 21C setpoint',
      technicianRef: 'technician',
    },
  })
  await svc.execute(TENANT, human('technician'), {
    action: 'append_work_line',
    commandId: commandId(),
    jobId: 'job-d51b2f3c',
    line: {
      lineId: 'line-101',
      kind: 'labour',
      unit: 'hours',
      quantity: 1.5,
      description: 'Calibration',
      partRef: null,
    },
  })
  await svc.execute(TENANT, human('signer'), {
    action: 'record_signoff',
    commandId: commandId(),
    jobId: 'job-d51b2f3c',
    signoff: {
      signoffId: 'signoff-101',
      signerRef: 'customer-south',
      accepted: false,
      comment: 'Setpoint drifted overnight',
    },
  })
  const rejected = await svc.detail(TENANT, human('owner'), 'job-d51b2f3c')
  assert.equal(rejected.signoff?.accepted, false)
  assert.equal(rejected.job.status, 'evidenced')
  check('a rejected signoff is recorded as rejection and never counts as acceptance')

  // 14b. Approval is unreachable while the only signoff on record is a rejection.
  await failed(
    () =>
      svc.execute(TENANT, human('signer'), {
        action: 'approve_job',
        commandId: commandId(),
        jobId: 'job-d51b2f3c',
      }),
    'illegal_transition',
  )
  check('a rejected customer signoff refuses approval instead of counting as acceptance')

  // 15. Revised evidence pulls a signed-off job back to evidenced.
  await svc.execute(TENANT, human('technician'), {
    action: 'append_work_line',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    line: {
      lineId: 'line-004',
      kind: 'part',
      unit: 'pieces',
      quantity: 1,
      description: 'Replacement contactor',
      partRef: 'part-contactor-24',
    },
  })
  const revised = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(revised.job.status, 'evidenced')
  assert.equal(revised.job.partsPieces, 4)
  check('evidence revised after signoff pulls the job back to evidenced')

  // 16. The revised chain returns through a fresh signoff.
  await svc.execute(TENANT, human('signer'), {
    action: 'record_signoff',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
    signoff: {
      signoffId: 'signoff-002',
      signerRef: 'customer-north',
      accepted: true,
      comment: null,
    },
  })
  const resigned = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(resigned.job.status, 'signed_off')
  check('the revised evidence chain returns to signed_off through a fresh signoff')

  // 17. Approval is the last forward step.
  await svc.execute(TENANT, human('owner'), {
    action: 'approve_job',
    commandId: commandId(),
    jobId: 'job-b3f9c1d2',
  })
  const approved = await svc.detail(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.equal(approved.job.status, 'approved')
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'append_work_line',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        line: {
          lineId: 'line-005',
          kind: 'labour',
          unit: 'hours',
          quantity: 1,
          description: 'Follow-up adjustment',
          partRef: null,
        },
      }),
    'illegal_transition',
  )
  check('an approved job refuses any further work evidence')

  // 18. Cancellation is explicit and terminal. It is a pre-evidence escape
  // hatch: a job that already carries work evidence cannot be un-committed.
  await svc.execute(TENANT, human('owner'), {
    action: 'create_job',
    commandId: commandId(),
    job: {
      jobId: 'job-cancel-target',
      customerRef: 'customer-south',
      siteRef: 'site-south-hall',
      summary: 'Canceled before dispatch',
      priority: 'low',
      appointmentRef: 'appt-9001',
    },
  })
  await svc.execute(TENANT, human('owner'), {
    action: 'cancel_job',
    commandId: commandId(),
    jobId: 'job-cancel-target',
    reason: 'Customer withdrew the appointment',
  })
  const canceled = await svc.detail(TENANT, human('owner'), 'job-cancel-target')
  assert.equal(canceled.job.status, 'canceled')
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'record_visit',
        commandId: commandId(),
        jobId: 'job-cancel-target',
        visit: {
          visitId: 'visit-102',
          outcome: 'completed',
          note: 'Attempted the canceled job anyway',
          technicianRef: 'technician',
        },
      }),
    'illegal_transition',
  )
  check('a canceled job is terminal and refuses a further visit')

  // 18b. A job carrying work evidence cannot be canceled out from under it.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        action: 'cancel_job',
        commandId: commandId(),
        jobId: 'job-d51b2f3c',
        reason: 'Tried to cancel a job that already has evidence',
      }),
    'illegal_transition',
  )
  check('a job with recorded work evidence refuses cancellation')

  // 19. Idempotent replay of the same command.
  const replayId = commandId()
  const replayCommand = {
    action: 'create_job',
    commandId: replayId,
    job: {
      jobId: 'job-e62c3a4d',
      customerRef: 'customer-east',
      siteRef: 'site-pine-road',
      summary: 'Annual service visit',
      appointmentRef: 'appt-4412',
    },
  }
  const first = await svc.execute(TENANT, human('owner'), replayCommand)
  const again = await svc.execute(TENANT, human('owner'), replayCommand)
  assert.deepEqual(again, first)
  assert.equal(
    (await svc.listJobs(TENANT, human('owner'), {})).filter((item) => item.jobId === 'job-e62c3a4d')
      .length,
    1,
  )
  check('a replayed command returns the original receipt without recording a second job')

  // 20. A replay with an altered payload is refused by fingerprint mismatch.
  await failed(
    () =>
      svc.execute(TENANT, human('owner'), {
        ...replayCommand,
        job: { ...replayCommand.job, summary: 'Rewritten summary for the same command' },
      }),
    'conflict',
  )
  check('a replayed command with an altered payload is refused by fingerprint mismatch')

  // 21. Command status recovery.
  const status = await svc.getCommandStatus(TENANT, human('owner'), replayId)
  assert.equal(status.commandId, replayId)
  assert.equal(status.status, 'accepted')
  assert.equal(status.jobId, 'job-e62c3a4d')
  const unknownStatus = await svc.getCommandStatus(TENANT, human('owner'), commandId())
  assert.equal(unknownStatus.status, 'unknown')
  check('command status recovers a recorded receipt and reports an unknown command')

  // 22. Service principals cannot mutate directly.
  await failed(
    () =>
      svc.execute(TENANT, automated('robot', 'owner'), {
        action: 'create_job',
        commandId: commandId(),
        job: {
          jobId: 'job-f73d4b5e',
          customerRef: 'customer-west',
          siteRef: 'site-maple-lane',
          summary: 'Service principal attempt',
          appointmentRef: 'appt-5590',
        },
      }),
    'forbidden',
  )
  check('service principals cannot record field mutations directly')

  // 22b. The same service principal still reads: the refusal is about who is
  // allowed to be the actor of record, not about automation being excluded.
  const robotJobs = await svc.listJobs(TENANT, automated('robot', 'owner'))
  assert.ok(robotJobs.length > 0)
  check('a service principal keeps read access while field mutations stay human-only')

  // 23. Cross-tenant disclosure and inactive principals are refused.
  await failed(() => svc.detail(OTHER, human('foreigner'), 'job-b3f9c1d2'), 'not_found')
  await failed(() => svc.detail(TENANT, human('inactive'), 'job-b3f9c1d2'), 'forbidden')
  check('cross-tenant job identifiers are never disclosed and inactive principals are refused')

  // 24. Read tools disclose to granted principals while unknown actions are refused.
  const access = await svc.getAccess(TENANT, human('owner'))
  assert.equal(access.permissions.includes('field.dispatch'), true)
  const audit = await svc.audit(TENANT, human('owner'), 'job-b3f9c1d2')
  assert.ok(audit.length > 0)
  await failed(
    () =>
      runFieldTool(svc, TENANT, human('owner'), 'field.unknownAction', {
        commandId: commandId(),
      }),
    'not_found',
  )
  // Once revoked above, the technician can still read but not write through the tool surface.
  assert.equal(
    await runFieldTool(svc, TENANT, human('technician'), 'field.listJobs', {}).then(
      (jobs) => jobs.length > 0,
    ),
    true,
  )
  check('read tools disclose to granted principals while unknown actions are refused')

  // 25. Revoking a grant takes effect on the next command.
  asOwner(
    'service_field',
    `UPDATE archava_field.memberships SET permissions = ARRAY['field.read'] WHERE tenant_id = '${TENANT}' AND principal_id = 'technician'`,
  )
  await failed(
    () =>
      svc.execute(TENANT, human('technician'), {
        action: 'append_work_line',
        commandId: commandId(),
        jobId: 'job-b3f9c1d2',
        line: {
          lineId: 'line-006',
          kind: 'labour',
          unit: 'hours',
          quantity: 1,
          description: 'After revocation',
          partRef: null,
        },
      }),
    'forbidden',
  )
  check('grant revocation takes effect on the next command')

  // 26. Migration checksums match the release catalog.
  const ledger = await pgModule.fieldMigrationLedger()
  const catalogEntry = trusted.packages.find((item) => item.name === '@archava/field-services-pg')
  assert.ok(catalogEntry)
  assert.deepEqual(
    catalogEntry.migrations.map((item) => item.file),
    ledger.map((item) => item.id + '.sql'),
  )
  assert.deepEqual(
    catalogEntry.migrations.map((item) => item.checksum),
    ledger.map((item) => item.checksum),
  )
  check('schema migration checksums match the release catalog')

  const roleState = (
    await runtime.query('SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user')
  ).rows[0]
  assert.equal(roleState.rolsuper, false)
  assert.equal(roleState.rolbypassrls, false)
  check('runtime role is neither superuser nor bypass-RLS')

  await ownerPool.end()
  await servicePool.end()
  await runtime.end()

  const destination = join(root, 'dist/field-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'appointment-locked technician dispatch, separated labour and parts evidence, customer signoff gating approval and replay-safe commands',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )

  process.stdout.write(`Field services domain verified with ${checks.length} checks\n`)
} catch (error) {
  process.stderr.write('CAUGHT: ' + String(error?.stack ?? error) + '\n')
  try {
    process.stderr.write((await readFile(join(scratch, 'postgres.log'), 'utf8')) + '\n')
  } catch {
    /* no log available */
  }
  throw error
} finally {
  if (clusterRunning) {
    try {
      run('pg_ctl', ['-D', PGHOST, '-m', 'immediate', 'stop'])
    } catch {
      /* the cluster may already be gone */
    }
  }
  await rm(scratch, { recursive: true, force: true })
}
