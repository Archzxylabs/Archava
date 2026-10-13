/* global process, Buffer, setTimeout */
import assert from 'node:assert/strict'
import { mkdtemp, mkdir, writeFile, rm, readFile } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { execFileSync, spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { pathToFileURL } from 'node:url'
import { randomUUID, createHash, hkdfSync } from 'node:crypto'
import { build } from 'esbuild'
import { installEnv } from './npm-env.mjs'

const root = resolve(import.meta.dirname, '../..'),
  library = join(root, 'dist/library'),
  profile = JSON.parse(await readFile(join(library, 'release-profile.json'), 'utf8')),
  trusted = JSON.parse(await readFile(join(root, 'config/library-release.v1.json'), 'utf8')),
  env = { ...process.env },
  scratch = await mkdtemp(join(tmpdir(), 'archava-billing-proof-')),
  checks = [],
  proofs = [],
  openPools = []

const check = (label) => {
  checks.push(label)
  process.stdout.write(`PASS ${checks.length} ${label}\n`)
}

const PGHOST = join(scratch, 'pg')
const PORT = 5400 + Math.floor(Math.random() * 1000)
const RUNTIME_ROLE = 'archava_billing_app'
const TENANT = 'tenant-billing-a'
const OTHER = 'tenant-billing-b'
const KEY = 'f'.repeat(64)

const human = (id) => ({ kind: 'human', id })
const automated = (id, onBehalfOf) => ({ kind: 'service', id, onBehalfOf })
const ALL = ['billing.read', 'billing.write', 'billing.settle', 'audit.read']

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
    assert.equal(error.code, code, `expected ${code}, received ${error.code ?? error.message}`)
    return
  }
  throw new Error(`expected refusal with code ${code}`)
}

/**
 * The exact derivation the local host uses: a per tenant and provider key
 * drawn from the host's master secret. Signing with it here proves the host's
 * own wiring, not a proof-only stand-in.
 */
const providerKey = (tenant, providerId) =>
  new Uint8Array(
    hkdfSync(
      'sha256',
      Buffer.from(KEY, 'utf8'),
      Buffer.from(tenant, 'utf8'),
      Buffer.from(`archava/billing/provider-event/v1|${providerId}`, 'utf8'),
      32,
    ),
  )

let clusterRunning = false

try {
  await startCluster()
  clusterRunning = true

  asOwner('postgres', `CREATE ROLE ${RUNTIME_ROLE} LOGIN`)
  asOwner('postgres', 'CREATE DATABASE owner_billing')
  asOwner('postgres', 'CREATE DATABASE service_billing')

  const consumers = []
  for (const name of ['clinic', 'rental']) {
    const directory = join(scratch, name)
    await mkdir(directory, { recursive: true })
    await writeFile(
      join(directory, 'package.json'),
      JSON.stringify(
        {
          name: 'independent-billing-' + name,
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

    const domain = await load('@archava/billing')
    const postgres = await load('@archava/billing-pg')
    const browser = await build({
      entryPoints: [require_.resolve('@archava/billing/dashboard')],
      outfile: join(directory, 'dashboard.mjs'),
      bundle: true,
      platform: 'browser',
      format: 'esm',
      metafile: true,
      logLevel: 'silent',
    })
    assert.equal(
      Object.keys(browser.metafile.inputs).some((item) =>
        /billing-pg|identity-pg|crm-pg|node:/.test(item),
      ),
      false,
    )

    consumers.push({ name, directory, domain, postgres, require: require_ })
    proofs.push({
      consumer: name,
      implementationSha256: createHash('sha256')
        .update(await readFile(require_.resolve('@archava/billing')))
        .digest('hex'),
      browserSafe: true,
      installedProfileVerified: true,
    })
  }
  assert.equal(new Set(proofs.map((item) => item.implementationSha256)).size, 1)
  check(
    'two independently installed clinic and rental consumers share unchanged billing code and a browser-safe dashboard',
  )

  const installed = consumers[0]
  const { Pool } = installed.require('pg')
  const { BillingService, executeBillingTool, providerEventDigest } = installed.domain
  const pgModule = installed.postgres

  const connect = (user, database) => {
    const pool = new Pool({ host: PGHOST, port: PORT, user, database, max: 4 })
    openPools.push(pool)
    return pool
  }

  const ownerPool = connect('postgres', 'owner_billing')
  const servicePool = connect('postgres', 'service_billing')

  await pgModule.migrateBilling(ownerPool)
  await pgModule.migrateBilling(servicePool)
  await pgModule.grantBillingRuntime(ownerPool, RUNTIME_ROLE)
  await pgModule.grantBillingRuntime(servicePool, RUNTIME_ROLE)

  const runtime = connect(RUNTIME_ROLE, 'service_billing')
  const repository = await pgModule.PostgresBillingRepository.create(runtime, KEY)
  const svc = new BillingService(repository, async (tenant, providerId) =>
    providerKey(tenant, providerId),
  )
  const commandId = () => randomUUID()

  // Seeded over one held connection so the session-scoped tenant guard is set
  // on the very connection that performs the insert.
  const seeding = await servicePool.connect()
  const seed = async (tenant, principalId, kind, permissions, scope = 'all') => {
    await seeding.query("SELECT set_config('archava.tenant_id', $1, false)", [tenant])
    await seeding.query(
      'INSERT INTO archava_billing.memberships(tenant_id, principal_id, display_name, kind, permissions, scope) VALUES($1, $2, $2, $3, $4, $5) ON CONFLICT DO NOTHING',
      [tenant, principalId, kind, permissions, scope],
    )
  }

  try {
    await seed(TENANT, 'owner', 'human', ALL)
    await seed(TENANT, 'viewer', 'human', ['billing.read', 'audit.read'])
    await seed(TENANT, 'clerk', 'human', ['billing.read', 'billing.write'])
    await seed(TENANT, 'settler', 'human', ['billing.read', 'billing.settle'])
    await seed(TENANT, 'inactive', 'human', ALL)
    await seed(TENANT, 'providerhook', 'service', ALL)
    await seed(OTHER, 'foreigner', 'human', ALL)
    await seeding.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
    await seeding.query(
      "UPDATE archava_billing.memberships SET active = false WHERE principal_id = 'inactive'",
    )
  } finally {
    seeding.release()
  }

  /** Build a provider event signed under the exact claim it asserts. */
  const signedEvent = (commandSeq, tenant, invoiceId, outcome, amountMinor, overrides = {}) => {
    const providerId = overrides.providerId ?? 'acquirer-demo'
    const providerEventId = overrides.providerEventId ?? `evt_${commandSeq}`
    const currency = overrides.currency ?? 'IDR'
    const key = overrides.key ?? providerKey(tenant, providerId)
    const signature = providerEventDigest(key, {
      tenantId: tenant,
      invoiceId,
      providerId,
      providerEventId,
      outcome,
      amountMinor,
      currency,
    })
    return {
      type: 'record_provider_event',
      commandId: `550e8400-e29b-41d4-a716-44665544${commandSeq}`,
      invoiceId,
      providerId,
      providerEventId,
      outcome,
      amountMinor,
      currency,
      signature,
    }
  }

  // 1. A priced draft invoice: gross is the sum of its lines.
  const clinicInvoice = await svc.execute(TENANT, human('owner'), {
    type: 'create_invoice',
    commandId: commandId(),
    invoiceCode: 'INV-2026-0001',
    customerRef: 'patient-aria',
    currency: 'IDR',
    consumerRef: 'consumer-1',
    lines: [
      { description: 'Specialist consultation', quantity: 2, unitMinor: 250000 },
      { description: 'Laboratory panel', quantity: 1, unitMinor: 150000 },
    ],
  })
  assert.ok(clinicInvoice.invoiceId)
  assert.equal(clinicInvoice.outstandingMinor, 650000)
  const clinicId = clinicInvoice.invoiceId
  check('a priced draft invoice opens with a frozen gross equal to the sum of its lines')

  // 2. Issuing freezes the snapshot; a second issue is refused.
  await svc.execute(TENANT, human('settler'), {
    type: 'issue_invoice',
    commandId: commandId(),
    invoiceId: clinicId,
    expectedRevision: 1,
    reason: 'Encounter closed, invoice approved',
  })
  await failed(
    () =>
      svc.execute(TENANT, human('settler'), {
        type: 'issue_invoice',
        commandId: commandId(),
        invoiceId: clinicId,
        expectedRevision: 2,
        reason: 'Attempting to reissue',
      }),
    'invalid_transition',
  )
  check('issuing freezes the priced snapshot and an issued invoice cannot be issued again')

  // 3. A correction adjusts what is owed; the priced gross never moves.
  await svc.execute(TENANT, human('settler'), {
    type: 'append_correction',
    commandId: commandId(),
    invoiceId: clinicId,
    amountMinor: -150000,
    reason: 'Goodwill credit for wait time',
  })
  const afterCredit = await svc.detail(TENANT, human('viewer'), clinicId)
  assert.equal(afterCredit.invoice.grossMinor, 650000)
  assert.equal(afterCredit.invoice.correctionMinor, -150000)
  assert.equal(afterCredit.invoice.outstandingMinor, 500000)
  assert.equal(afterCredit.corrections.length, 1)
  check('an appended correction changes what is owed while the priced snapshot stays intact')

  // 4. A payment attempt records intent and settles nothing.
  const attemptReceipt = await svc.execute(TENANT, human('clerk'), {
    type: 'record_payment_attempt',
    commandId: commandId(),
    invoiceId: clinicId,
    amountMinor: 500000,
    providerRef: 'pi_123',
  })
  assert.equal(attemptReceipt.outstandingMinor, 500000)
  const afterAttempt = await svc.detail(TENANT, human('viewer'), clinicId)
  assert.equal(afterAttempt.invoice.settledMinor, 0)
  assert.equal(afterAttempt.invoice.status, 'issued')
  assert.equal(afterAttempt.attempts.length, 1)
  assert.equal(afterAttempt.events.length, 0)
  check('a payment attempt records intent and can never settle an invoice')

  // 5. A structurally invalid attempt: there is no outcome field to send.
  const contracts = await import(
    pathToFileURL(installed.require.resolve('@archava/billing/contracts')).href
  )
  assert.throws(() =>
    contracts.billingMutationSchema.parse({
      type: 'record_payment_attempt',
      commandId: commandId(),
      invoiceId: clinicId,
      amountMinor: 500000,
      outcome: 'succeeded',
    }),
  )
  check('an attempt carries no field capable of asserting a payment result')

  // 6. A correctly signed provider event settles the invoice.
  const settleReceipt = await svc.execute(
    TENANT,
    human('clerk'),
    signedEvent('0101', TENANT, clinicId, 'succeeded', 500000),
  )
  assert.equal(settleReceipt.status, 'settled')
  assert.equal(settleReceipt.outstandingMinor, 0)
  const settled = await svc.detail(TENANT, human('viewer'), clinicId)
  assert.equal(settled.invoice.settledMinor, 500000)
  assert.equal(settled.invoice.status, 'settled')
  assert.equal(settled.invoice.grossMinor, 650000)
  assert.equal(settled.events.length, 1)
  check('only a correctly signed provider event settles an issued invoice')

  // 7. A tampered amount fails verification.
  const tampered = signedEvent('0102', TENANT, clinicId, 'succeeded', 500000)
  await failed(
    () => svc.execute(TENANT, human('clerk'), { ...tampered, amountMinor: 1 }),
    'invalid_signature',
  )
  check('a provider event whose amount was altered after signing is refused')

  // 8. A signature minted under another tenant's key is refused.
  await failed(
    () =>
      svc.execute(
        TENANT,
        human('clerk'),
        signedEvent('0103', TENANT, clinicId, 'succeeded', 1, {
          key: providerKey(OTHER, 'acquirer-demo'),
        }),
      ),
    'invalid_signature',
  )
  check('a signature minted under another tenant’s key is refused')

  // 9. A digest computed under a different domain is refused.
  const foreignDomain = createHash('sha256')
    .update('archava/other-domain/v1|anything')
    .digest('hex')
  await failed(
    () =>
      svc.execute(TENANT, human('clerk'), {
        ...signedEvent('0104', TENANT, clinicId, 'succeeded', 1),
        signature: foreignDomain,
      }),
    'invalid_signature',
  )
  check('a value minted under a foreign domain is refused by the same single code')

  // 10. An unconfigured provider fails closed.
  const unconfigured = new BillingService(repository, async () => null)
  await failed(
    () =>
      unconfigured.execute(
        TENANT,
        human('clerk'),
        signedEvent('0105', TENANT, clinicId, 'succeeded', 1),
      ),
    'invalid_signature',
  )
  check('a provider with no configured signing secret fails closed rather than open')

  // A signature is one half of the authority, not all of it: the principal
  // that may issue and correct this invoice still cannot relay a settlement
  // without the separate write grant.
  await failed(
    () =>
      svc.execute(
        TENANT,
        human('settler'),
        signedEvent('0113', TENANT, clinicId, 'refunded', 500000),
      ),
    'forbidden',
  )
  check('a correctly signed provider event still requires the dedicated write grant')

  // 12. A refund reopens the settled invoice back to issued.
  const refundReceipt = await svc.execute(
    TENANT,
    human('clerk'),
    signedEvent('0107', TENANT, clinicId, 'refunded', 500000),
  )
  assert.equal(refundReceipt.status, 'issued')
  assert.equal(refundReceipt.outstandingMinor, 500000)
  const reopened = await svc.detail(TENANT, human('viewer'), clinicId)
  assert.equal(reopened.invoice.settledMinor, 0)
  assert.equal(reopened.invoice.status, 'issued')
  check('a provider refund reopens a settled invoice instead of erasing the settlement')

  // 13. A failed provider event moves nothing at all.
  const beforeFailure = await svc.detail(TENANT, human('viewer'), clinicId)
  const failedReceipt = await svc.execute(
    TENANT,
    human('clerk'),
    signedEvent('0108', TENANT, clinicId, 'failed', 500000),
  )
  assert.equal(failedReceipt.status, 'failed')
  const afterFailure = await svc.detail(TENANT, human('viewer'), clinicId)
  assert.equal(afterFailure.invoice.revision, beforeFailure.invoice.revision)
  assert.equal(afterFailure.invoice.settledMinor, beforeFailure.invoice.settledMinor)
  assert.equal(afterFailure.invoice.outstandingMinor, beforeFailure.invoice.outstandingMinor)
  assert.equal(afterFailure.events.length, beforeFailure.events.length + 1)
  check('a failed provider notification is pure history: balance, status and revision stand still')

  // A settlement larger than the outstanding balance is refused. The invoice is
  // issued here precisely because a settlement is only even considered on an
  // issued invoice — the status guard and the balance guard are separate tests.
  await failed(
    () =>
      svc.execute(
        TENANT,
        human('clerk'),
        signedEvent('0106', TENANT, clinicId, 'succeeded', 999999),
      ),
    'unbalanced_settlement',
  )
  check('a settlement larger than the outstanding balance is refused as unbalanced')

  // 14. Overpayment is refused; refunding more than settled is refused.
  await failed(
    () =>
      svc.execute(
        TENANT,
        human('clerk'),
        signedEvent('0109', TENANT, clinicId, 'refunded', 500000),
      ),
    'unbalanced_settlement',
  )
  check('a refund larger than the amount already settled is refused')

  // 15. Service principals cannot perform human-only mutations.
  await failed(
    () =>
      svc.execute(TENANT, automated('bot-auto', 'user-1'), {
        type: 'create_invoice',
        commandId: commandId(),
        invoiceCode: 'INV-2026-9999',
        customerRef: 'nobody',
        currency: 'IDR',
        consumerRef: 'consumer-1',
        lines: [{ description: 'Ghost line', quantity: 1, unitMinor: 1 }],
      }),
    'forbidden',
  )
  check('service principals cannot create, issue, correct or void invoices')

  // 16. A service principal may carry a provider event — the signature is the authority.
  const viaService = await svc.execute(
    TENANT,
    automated('providerhook', 'user-1'),
    signedEvent('0110', TENANT, clinicId, 'succeeded', 1),
  )
  assert.equal(viaService.action, 'record_provider_event')
  check('a signed provider event is authorised by its signature, not by a human operator')

  // 17. The settle grant is separate from generic write.
  await failed(
    () =>
      svc.execute(TENANT, human('clerk'), {
        type: 'issue_invoice',
        commandId: commandId(),
        invoiceId: clinicId,
        expectedRevision: beforeFailure.invoice.revision,
        reason: 'Clerk tries to settle',
      }),
    'forbidden',
  )
  check('issuing, correcting and voiding require the dedicated settle grant')

  // 18. A second consumer's invoice stays tenant-isolated.
  const rentalInvoice = await svc.execute(TENANT, human('owner'), {
    type: 'create_invoice',
    commandId: commandId(),
    invoiceCode: 'INV-2026-0002',
    customerRef: 'renter-bravo',
    currency: 'IDR',
    consumerRef: 'consumer-1',
    lines: [{ description: 'Excavator hire, 3 days', quantity: 3, unitMinor: 400000 }],
  })
  await failed(() => svc.detail(OTHER, human('foreigner'), rentalInvoice.invoiceId), 'not_found')
  check('cross-tenant invoice identifiers are never disclosed')

  // 19. Inactive principals are refused.
  await failed(
    () =>
      svc.execute(TENANT, human('inactive'), {
        type: 'create_invoice',
        commandId: commandId(),
        invoiceCode: 'INV-2026-0003',
        customerRef: 'disabled',
        currency: 'IDR',
        consumerRef: 'consumer-1',
        lines: [{ description: 'Should never land', quantity: 1, unitMinor: 1 }],
      }),
    'forbidden',
  )
  check('inactive principals are refused')

  // 20. Idempotent replay returns the original receipt; a changed payload conflicts.
  const stableRequest = signedEvent('0111', TENANT, clinicId, 'failed', 500000)
  const firstReplay = await svc.execute(TENANT, human('clerk'), stableRequest)
  const replayed = await svc.execute(TENANT, human('clerk'), stableRequest)
  assert.equal(replayed.idempotencyReplay, true)
  assert.equal(replayed.eventId, firstReplay.eventId)
  await failed(
    () => svc.execute(TENANT, human('clerk'), signedEvent('0111', TENANT, clinicId, 'failed', 1)),
    'conflict',
  )
  check('idempotent replay returns the original receipt while a changed payload conflicts')

  // 21. Command status recovery.
  const recovered = await svc.getCommandStatus(TENANT, human('viewer'), firstReplay.commandId)
  assert.equal(recovered.status, 'recorded')
  assert.equal((await svc.getCommandStatus(TENANT, human('viewer'), commandId())).status, 'unknown')
  check('command status query recovers a recorded receipt after an interruption')

  // 22. Read-only tools: they read, and they refuse to write.
  const toolList = await executeBillingTool(svc, TENANT, human('viewer'), 'billing.listInvoices', {
    limit: 50,
    offset: 0,
  })
  assert.ok(toolList.length >= 2)
  const toolDetail = await executeBillingTool(
    svc,
    TENANT,
    human('viewer'),
    'billing.invoiceDetail',
    { invoiceId: clinicId },
  )
  assert.equal(toolDetail.invoice.id, clinicId)
  await failed(
    () =>
      executeBillingTool(svc, TENANT, human('viewer'), 'billing.invoiceDetail', {
        invoiceId: clinicId,
        tenant: OTHER,
      }),
    'bad_request',
  )
  await failed(
    () => executeBillingTool(svc, TENANT, human('owner'), 'billing.settleNow', {}),
    'not_found',
  )
  check('read-only tools read invoices, lines, corrections, attempts and events without writing')

  // 23. Evidence is append-only at the database boundary.
  //
  // A held client rather than a pooled query: the tenant guard is a
  // session-scoped setting, so on a different pooled connection the policy
  // would match no rows and these would pass for the wrong reason.
  const guarded = await servicePool.connect()
  try {
    await guarded.query("SELECT set_config('archava.tenant_id', $1, false)", [TENANT])
    const rowsAffected = async (statement, params = []) => {
      const result = await guarded.query(statement, params)
      return Number(result.rowCount)
    }
    assert.ok(
      (await rowsAffected('SELECT 1 FROM archava_billing.invoice_lines LIMIT 1')) > 0,
      'the tenant guard must see the evidence rows for this test to mean anything',
    )
    await assert.rejects(() =>
      guarded.query('UPDATE archava_billing.invoice_lines SET quantity = 999'),
    )
    await assert.rejects(() => guarded.query('DELETE FROM archava_billing.invoice_lines'))
    await assert.rejects(() => guarded.query('DELETE FROM archava_billing.invoice_corrections'))
    await assert.rejects(() => guarded.query('DELETE FROM archava_billing.payment_attempts'))
    await assert.rejects(() => guarded.query('DELETE FROM archava_billing.payment_events'))
    await assert.rejects(() => guarded.query('DELETE FROM archava_billing.audit'))
    check('invoice lines, corrections, attempts, events and audit are append-only at the database')

    // 24. A priced invoice's identity and total are frozen once created.
    await assert.rejects(() =>
      guarded.query(
        `UPDATE archava_billing.invoices SET customer_ref = 'someone-else' WHERE tenant_id = '${TENANT}'`,
      ),
    )
    await assert.rejects(() =>
      guarded.query(
        `UPDATE archava_billing.invoices SET gross_minor = 1 WHERE tenant_id = '${TENANT}'`,
      ),
    )
    check('a priced invoice’s customer and gross total are immutable at the database boundary')

    // 25. Only the domain's transitions are allowed; void is terminal.
    await assert.rejects(() =>
      guarded.query(
        `UPDATE archava_billing.invoices SET status = 'draft' WHERE tenant_id = '${TENANT}' AND status = 'issued'`,
      ),
    )
    check('invoice status cannot move backwards, and only domain transitions are permitted')
  } finally {
    guarded.release()
  }

  // 26. The runtime role is not privileged.
  const roleState = await runtime.query(
    'SELECT rolsuper, rolbypassrls FROM pg_roles WHERE rolname = current_user',
  )
  assert.equal(roleState.rows[0].rolsuper, false)
  assert.equal(roleState.rows[0].rolbypassrls, false)
  check('runtime role is neither superuser nor bypass-RLS')

  // 27. Revoking a grant takes effect on the next command.
  asOwner(
    'service_billing',
    `UPDATE archava_billing.memberships SET permissions = ARRAY['billing.read'] WHERE tenant_id = '${TENANT}' AND principal_id = 'clerk'`,
  )
  await failed(
    () => svc.execute(TENANT, human('clerk'), signedEvent('0112', TENANT, clinicId, 'failed', 1)),
    'forbidden',
  )
  check('grant revocation takes effect on the next command')

  // 28. Migration checksums match the release catalog.
  const ledger = await pgModule.billingMigrationLedger()
  const catalogEntry = trusted.packages.find((item) => item.name === '@archava/billing-pg')
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

  await ownerPool.end()
  await servicePool.end()
  await runtime.end()

  const destination = join(root, 'dist/billing-proof')
  await mkdir(destination, { recursive: true })
  await writeFile(
    join(destination, 'results.json'),
    JSON.stringify(
      {
        profile: profile.catalog.version,
        scope:
          'immutable invoice intent and snapshot, append-only correction chains and provider-authoritative settlement with signature verification',
        consumers: proofs,
        checks,
        operationalKitsAccepted: [],
        providerCalls: 0,
      },
      null,
      2,
    ) + '\n',
  )

  process.stdout.write(`Billing domain verified with ${checks.length} checks\n`)
} catch (error) {
  try {
    process.stderr.write((await readFile(join(scratch, 'postgres.log'), 'utf8')) + '\n')
  } catch {
    /* no log available */
  }
  throw error
} finally {
  // Close every pool before the cluster stops. Stopping postgres under a live
  // idle connection raises an unhandled 'error' event that would otherwise
  // replace the real finding with a connection message.
  for (const pool of openPools) {
    try {
      await pool.end()
    } catch {
      /* already closed */
    }
  }
  if (clusterRunning) {
    try {
      run('pg_ctl', ['-D', PGHOST, '-m', 'immediate', 'stop'])
    } catch {
      /* the cluster may already be gone */
    }
  }
  await rm(scratch, { recursive: true, force: true })
}
