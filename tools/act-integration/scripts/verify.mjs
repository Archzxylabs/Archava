/* global process */
import assert from 'node:assert/strict'
import { execFileSync, spawn } from 'node:child_process'
import { mkdtemp, mkdir, readFile, writeFile, readdir, rm } from 'node:fs/promises'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { pathToFileURL } from 'node:url'
import { createRequire } from 'node:module'
import { createHash, randomUUID } from 'node:crypto'
import { build } from 'esbuild'
import { verifyNative } from './native-proof.mjs'

// pnpm re-exports the invoking user npm config into every script it runs, so a user-level
// `allow-scripts` policy reaches npm as `npm_config_allow_scripts`. npm treats environment
// config as CLI-level policy and refuses it for project-scoped installs (EALLOWSCRIPTS).
// Installs below already pass `--ignore-scripts`; drop the inert policy for this process
// instead of rewriting the owner ~/.npmrc. Same rule as tools/library/npm-env.mjs.
delete process.env.npm_config_allow_scripts

const root = resolve(import.meta.dirname, '../../..')
const self = join(import.meta.dirname, 'verify.mjs')
const client = (pool) => ({
  storeId: 'act-isolated-verifier',
  capabilities: { transactions: true },
  async run(statement, options) {
    if (!options?.transaction) return (await pool.query(statement.sql, statement.params)).rows
    const c = await pool.connect()
    try {
      await c.query('BEGIN')
      const result = await c.query(statement.sql, statement.params)
      await c.query('COMMIT')
      return result.rows
    } catch (error) {
      await c.query('ROLLBACK')
      throw error
    } finally {
      c.release()
    }
  },
})
const moduleAt = async (directory, name) =>
  import(pathToFileURL(createRequire(join(directory, 'package.json')).resolve(name)).href)
if (process.argv[2] === 'worker') {
  const input = JSON.parse(await readFile(process.argv[3], 'utf8'))
  if (!input.config.host.startsWith('/tmp/archava-act-proof-'))
    throw new Error('Worker accepts only the private fixture')
  const { Pool } = createRequire(join(input.consumer, 'package.json'))('pg')
  const pool = new Pool({
    ...input.config,
    password: () => '',
    ssl: false,
    sslnegotiation: 'postgres',
    options: '-c row_security=on',
    connectionTimeoutMillis: 5000,
    max: 1,
  })
  try {
    const pg = await moduleAt(input.consumer, '@archava/act-host-pg')
    const ledger = await pg.PostgresActHostAttemptLedger.create(client(pool), input.scope)
    const result =
      input.operation === 'claim'
        ? await ledger.claim(input.intentId, input.claimId)
        : await ledger.open(input.record, input.previousIntentId)
    process.stdout.write(JSON.stringify(result) + '\n')
  } finally {
    await pool.end()
  }
} else {
  execFileSync('pnpm', ['library:pack'], { cwd: root, stdio: 'pipe' })
  const profile = JSON.parse(
    await readFile(join(root, 'dist/library/release-profile.json'), 'utf8'),
  )
  const scratch = await mkdtemp(join(tmpdir(), 'archava-act-proof-'))
  const socket = join(scratch, 'socket'),
    data = join(scratch, 'data')
  const bin = execFileSync('pg_config', ['--bindir'], { encoding: 'utf8' }).trim()
  const port = 20000 + Math.floor(Math.random() * 20000)
  const adminRole = 'act_verify_operator',
    appRole = 'act_verify_runtime'
  const config = { host: socket, port, database: 'postgres', user: appRole }
  const tool = (name, args) => execFileSync(join(bin, name), args, { stdio: 'pipe' })
  let running = false,
    admin,
    app
  const stop = () => {
    if (running) {
      tool('pg_ctl', ['-D', data, '-m', 'fast', '-w', 'stop'])
      running = false
    }
  }
  const start = () => {
    tool('pg_ctl', [
      '-D',
      data,
      '-l',
      join(scratch, 'postgres.log'),
      '-w',
      'start',
      '-o',
      `-k '${socket}' -p ${port} -c listen_addresses=''`,
    ])
    running = true
  }
  const checks = [],
    proofs = [],
    consumers = []
  const check = (label) => {
    checks.push(label)
    process.stdout.write('PASS ' + label + '\n')
  }
  const hash = async (directory) => {
    const digest = createHash('sha256')
    const visit = async (current, prefix = '') => {
      for (const entry of (await readdir(current, { withFileTypes: true })).sort((a, b) =>
        a.name.localeCompare(b.name),
      )) {
        const file = join(current, entry.name),
          name = prefix + entry.name
        if (entry.isDirectory()) await visit(file, name + '/')
        else {
          digest.update(name + '\0')
          digest.update(await readFile(file))
          digest.update('\0')
        }
      }
    }
    await visit(directory)
    return digest.digest('hex')
  }
  const artifacts = JSON.parse(
    await readFile(join(root, 'dist/library/artifacts.json'), 'utf8'),
  ).artifacts
  try {
    for (const name of ['hospitality', 'professional-services']) {
      const directory = join(scratch, name)
      await mkdir(directory)
      await writeFile(
        join(directory, 'package.json'),
        JSON.stringify({
          name: 'independent-act-' + name,
          type: 'module',
          private: true,
          overrides: Object.fromEntries(
            (profile.catalog.externals ?? []).map((item) => [item.name, item.version]),
          ),
          dependencies: Object.fromEntries(
            artifacts.map((a) => [a.name, 'file:' + join(root, 'dist/library', a.tarball)]),
          ),
        }),
      )
      execFileSync('npm', ['install', '--ignore-scripts', '--no-audit', '--no-fund'], {
        cwd: directory,
        stdio: 'pipe',
      })
      await build({
        entryPoints: [join(root, 'tools/act-integration/src/fixture.ts')],
        outfile: join(directory, 'fixture.mjs'),
        bundle: true,
        platform: 'node',
        target: 'node22',
        format: 'esm',
        external: ['@archava/*'],
      })
      const modules = []
      for (const name of [
        '@archava/act-host',
        '@archava/act-host-pg',
        '@archava/act-confirmation',
        '@archava/act-confirmation-pg',
        '@archava/act-storage',
        '@archava/act',
        '@archava/act-postmark',
        '@archava/crm-act',
        '@archava/crm',
        '@archava/crm-pg',
      ]) {
        const artifact = artifacts.find((a) => a.name === name)
        assert.ok(artifact)
        modules.push({
          name,
          version: artifact.version,
          implementationSha256: await hash(join(directory, 'node_modules', name)),
          tarballSha256: artifact.sha256,
        })
      }
      proofs.push({ consumer: name, modules })
      consumers.push(directory)
    }
    assert.deepEqual(proofs[0].modules, proofs[1].modules)
    check(
      'two independent cold installs use identical compiled Act implementations and included owned migrations',
    )
    const { Pool } = createRequire(join(consumers[0], 'package.json'))('pg')
    const pools = () => {
      admin = new Pool({
        ...config,
        user: adminRole,
        password: () => '',
        ssl: false,
        options: '-c row_security=on',
      })
      app = new Pool({
        ...config,
        password: () => '',
        ssl: false,
        options: '-c row_security=on',
        max: 8,
      })
    }
    await mkdir(socket, { mode: 0o700 })
    tool('initdb', ['-D', data, '-U', adminRole, '--auth=trust', '--no-sync'])
    start()
    pools()
    const pg = await moduleAt(consumers[0], '@archava/act-host-pg')
    await Promise.all([pg.migrateActHost(client(admin)), pg.migrateActHost(client(admin))])
    await admin.query(`CREATE ROLE ${appRole} LOGIN NOSUPERUSER NOBYPASSRLS`)
    await pg.grantActHostRuntime(client(admin), appRole)
    // The legacy ports own these migrations. Applied only to this disposable fixture.
    for (const [name, migration] of [
      ['@archava/act-storage', '001_act_attempts.sql'],
      ['@archava/act-confirmation-pg', '001_confirmation_challenges.sql'],
    ])
      await admin.query(
        await readFile(join(consumers[0], 'node_modules', name, 'migrations', migration), 'utf8'),
      )
    await admin.query(
      `GRANT SELECT,INSERT,UPDATE ON act_attempts,confirmation_challenges TO ${appRole}`,
    )
    const scope = { tenantId: 'tenant-example', sessionId: 'session-example' }
    await assert.rejects(() => pg.PostgresActHostAttemptLedger.create(client(admin), scope))
    let ledger = await pg.PostgresActHostAttemptLedger.create(client(app), scope)
    const base = {
      ...scope,
      actionId: 'booking.create',
      inputDigest: 'a'.repeat(64),
      occurredAt: '2026-10-09T10:00:00.000Z',
      expiresAt: 1791536700000,
    }
    check(
      'repeat/concurrent migration, checksummed ledger, forced RLS and non-owner runtime requirement',
    )
    const child = async (input) => {
      const path = join(scratch, 'worker-' + randomUUID() + '.json')
      await writeFile(path, JSON.stringify({ ...input, config, scope }), { mode: 0o600 })
      return await new Promise((done, fail) => {
        const processChild = spawn(process.execPath, [self, 'worker', path], {
          cwd: root,
          stdio: ['ignore', 'pipe', 'pipe'],
        })
        let stdout = '',
          stderr = ''
        processChild.stdout.on('data', (chunk) => {
          stdout += chunk
        })
        processChild.stderr.on('data', (chunk) => {
          stderr += chunk
        })
        processChild.once('exit', (code) =>
          code === 0
            ? done(JSON.parse(stdout))
            : fail(new Error('Isolated worker failed: ' + stderr)),
        )
        processChild.once('error', fail)
      })
    }
    const records = Array.from({ length: 4 }, (_, index) => ({
      ...base,
      challengeId: randomUUID(),
      occurredAt: new Date(Date.parse(base.occurredAt) + index * 1000).toISOString(),
    }))
    const opens = await Promise.all(
      records.map((record, index) =>
        child({ consumer: consumers[index % 2], operation: 'open', record }),
      ),
    )
    assert.equal(new Set(opens.map((row) => row.intentId)).size, 1)
    assert.equal(new Set(opens.map((row) => row.occurredAt)).size, 1)
    const original = opens[0]
    for (const record of records)
      assert.equal((await ledger.findByChallenge(record.challengeId)).intentId, original.intentId)
    check(
      'four separate Node processes atomically open one canonical intent/time and retain every challenge mapping',
    )
    const claims = await Promise.all(
      records.map((record, index) =>
        child({
          consumer: consumers[index % 2],
          operation: 'claim',
          intentId: original.intentId,
          claimId: record.challengeId,
        }),
      ),
    )
    assert.equal(claims.filter(Boolean).length, 1)
    const claimId = records[claims.findIndex(Boolean)].challengeId
    assert.equal(await ledger.cancel(original.intentId), false)
    assert.equal(await ledger.settle(original.intentId, claimId, 'succeeded'), true)
    assert.equal(await ledger.settle(original.intentId, claimId, 'unknown'), false)
    check(
      'four processes admit one executor claimant; cancellation/late settlement cannot replace terminal truth',
    )
    const newRecords = Array.from({ length: 4 }, () => ({ ...base, challengeId: randomUUID() }))
    const next = await Promise.all(
      newRecords.map((record, index) =>
        child({
          consumer: consumers[index % 2],
          operation: 'open',
          record,
          previousIntentId: original.intentId,
        }),
      ),
    )
    assert.equal(new Set(next.map((row) => row.intentId)).size, 1)
    assert.notEqual(next[0].intentId, original.intentId)
    assert.equal(next[0].generation, 2)
    assert.ok(Date.parse(next[0].occurredAt) > Date.parse(original.occurredAt))
    const successor = next[0]
    assert.equal(await ledger.claim(successor.intentId, newRecords[0].challengeId), true)
    assert.equal(
      await ledger.settle(successor.intentId, newRecords[0].challengeId, 'unknown'),
      true,
    )
    await assert.rejects(() =>
      ledger.open({ ...base, challengeId: randomUUID() }, successor.intentId),
    )
    const retriedTransition = await ledger.open(
      { ...base, challengeId: randomUUID() },
      original.intentId,
    )
    assert.equal(retriedTransition.intentId, successor.intentId)
    assert.equal(retriedTransition.phase, 'unknown')
    check(
      'explicit same-payload transition has one successor under contention; unknown never opens a new intent',
    )
    const foreign = await pg.PostgresActHostAttemptLedger.create(client(app), {
      tenantId: 'other-tenant',
      sessionId: scope.sessionId,
    })
    assert.equal(await foreign.findByIntent(original.intentId), null)
    assert.equal(await foreign.findByChallenge(records[0].challengeId), null)
    assert.equal(await foreign.claim(original.intentId, records[0].challengeId), false)
    const otherSession = await pg.PostgresActHostAttemptLedger.create(client(app), {
      tenantId: scope.tenantId,
      sessionId: 'other-session',
    })
    assert.equal(await otherSession.findByIntent(original.intentId), null)
    assert.equal((await app.query('SELECT * FROM archava_act_host.intents')).rowCount, 0)
    await assert.rejects(() => app.query('DELETE FROM archava_act_host.intents'))
    check('tenant/session isolation, transaction scope reset and no runtime deletion')
    const c = await app.connect()
    try {
      await c.query('BEGIN')
      await c.query(
        "SELECT set_config('archava.tenant_id',$1,true),set_config('archava.act_session',$2,true)",
        [scope.tenantId, scope.sessionId],
      )
      await assert.rejects(() =>
        c.query(
          'UPDATE archava_act_host.intents SET occurred_at_ms=occurred_at_ms+1 WHERE intent_id=$1',
          [original.intentId],
        ),
      )
      await c.query('ROLLBACK')
    } finally {
      c.release()
    }
    await admin.query("UPDATE archava_act_host.schema_migrations SET checksum=repeat('0',64)")
    await assert.rejects(() => pg.PostgresActHostAttemptLedger.create(client(app), scope))
    await assert.rejects(() => pg.migrateActHost(client(admin)))
    await admin.query('UPDATE archava_act_host.schema_migrations SET checksum=$1', [
      (await pg.actHostMigration()).checksum,
    ])
    check('immutable canonical identity and rejection of modified migration ledgers')
    await admin.end()
    await app.end()
    stop()
    start()
    pools()
    const otherPg = await moduleAt(consumers[1], '@archava/act-host-pg')
    ledger = await otherPg.PostgresActHostAttemptLedger.create(client(app), scope)
    assert.equal((await ledger.findByIntent(successor.intentId)).phase, 'unknown')
    assert.equal(
      (await ledger.findByChallenge(records[0].challengeId)).occurredAt,
      original.occurredAt,
    )
    assert.equal(await ledger.claim(successor.intentId, newRecords[1].challengeId), false)
    assert.equal(await ledger.reconcileSuccess(successor.intentId, 'b'.repeat(64)), true)
    assert.equal(
      await ledger.settle(successor.intentId, newRecords[0].challengeId, 'unknown'),
      false,
    )
    check(
      'real PostgreSQL restart retains canonical identity/unknown; positive reconciliation fences late claimants',
    )
    // Exercise the actual installed host, policy, consent and provider boundaries together.
    const storage = await moduleAt(consumers[0], '@archava/act-storage')
    const confirmationPg = await moduleAt(consumers[0], '@archava/act-confirmation-pg')
    const { DurableActHost } = await moduleAt(consumers[0], '@archava/act-host')
    const { actFixture } = await import(pathToFileURL(join(consumers[0], 'fixture.mjs')).href)
    const ports = async (tenant) => ({
      attempts: await pg.PostgresActHostAttemptLedger.create(client(app), {
        tenantId: tenant,
        sessionId: 'trusted-session-' + tenant,
      }),
      challengeStore: new confirmationPg.PostgresChallengeStore(client(app)),
      bookingStore: new storage.PostgresBookingAttemptStore({
        client: client(app),
        idempotencyKeyHmacKey: new Uint8Array(32).fill(23),
      }),
      emailStore: new storage.PostgresEmailAttemptStore({
        client: client(app),
        idempotencyKeyHmacKey: new Uint8Array(32).fill(23),
      }),
    })
    const prepare = async (f, actionId, inputs) => {
      const envelope = { utterance: 'Synthetic consent fixture', action: { actionId, inputs } }
      const review = await f.host.review({ envelope, graph: f.graph })
      assert.equal(review.status, 'challenge_issued')
      const presentation = {
        graph: f.graph,
        envelope: {
          ...envelope,
          confirmation: {
            challengeId: review.challenge.challengeId,
            token: review.challenge.token,
          },
        },
      }
      return { review, presentation }
    }
    let syntheticSends = 0,
      syntheticReservations = 0
    for (const tenant of ['hospitality-proof', 'professional-services-proof']) {
      const f = actFixture(tenant, 'act', await ports(tenant))
      const reviewed = await Promise.all(
        Array.from({ length: 4 }, () => prepare(f, 'booking.create', f.bookingInput)),
      )
      assert.equal(new Set(reviewed.map((row) => row.review.intentId)).size, 1)
      const results = await Promise.all(reviewed.map((row) => f.host.present(row.presentation)))
      assert.ok(results.some((row) => row.execution === 'succeeded'))
      assert.equal(f.bookingCalls.length, 1)
      const replay = await prepare(f, 'booking.create', f.bookingInput)
      assert.equal((await f.host.present(replay.presentation)).execution, 'succeeded')
      assert.equal(f.bookingCalls.length, 1)
      await assert.rejects(() => f.host.present(reviewed[0].presentation), {
        code: 'visitor_challenge_already_spent',
      })
      syntheticReservations += f.bookingCalls.length
      const mail = await prepare(f, 'email.send', f.emailInput)
      assert.equal((await f.host.present(mail.presentation)).execution, 'succeeded')
      const mailRetry = await prepare(f, 'email.send', f.emailInput)
      assert.equal((await f.host.present(mailRetry.presentation)).execution, 'succeeded')
      assert.equal(f.mailCalls.length, 1)
      syntheticSends += f.mailCalls.length
    }
    check(
      'real installed policy/validation/turn, PostgreSQL one-time consent/intent/provider records and bounded mocked gateways compose for two tenants; concurrent presentations perform one reservation',
    )
    const lostTenant = 'lost-response-proof'
    let lost = actFixture(lostTenant, 'act', await ports(lostTenant))
    lost.unknownMail()
    const mail = await prepare(lost, 'email.send', lost.emailInput)
    assert.equal((await lost.host.present(mail.presentation)).errorCode, 'provider_outcome_unknown')
    assert.equal(lost.mailCalls.length, 1)
    assert.equal(
      (await lost.host.reconcile(mail.review.intentId, lost.graph, lost.emailInput)).phase,
      'unknown',
    )
    assert.equal(lost.statusCalls.length, 1)
    const storedRows = JSON.stringify((await admin.query('SELECT * FROM act_attempts')).rows)
    assert.ok(!storedRows.includes(lost.emailInput.to))
    assert.ok(!storedRows.includes(lost.executionCalls[0].idempotencyKey))
    await admin.end()
    await app.end()
    stop()
    start()
    pools()
    const { actFixture: otherFixture } = await import(
      pathToFileURL(join(consumers[1], 'fixture.mjs')).href
    )
    lost = otherFixture(lostTenant, 'act', await ports(lostTenant))
    assert.equal((await lost.host.status(mail.review.intentId, lost.graph)).phase, 'unknown')
    const retry = await prepare(lost, 'email.send', lost.emailInput)
    assert.equal(
      (await lost.host.present(retry.presentation)).errorCode,
      'provider_outcome_unknown',
    )
    assert.equal(lost.mailCalls.length, 0)
    lost.confirmMailAcceptance()
    const restartedHost = new DurableActHost(lost.options)
    assert.equal(
      (await restartedHost.reconcile(mail.review.intentId, lost.graph, lost.emailInput)).phase,
      'succeeded',
    )
    assert.equal(lost.mailCalls.length, 0)
    assert.equal(lost.statusCalls.length, 1)
    const success = await prepare(lost, 'email.send', lost.emailInput)
    assert.equal((await restartedHost.present(success.presentation)).execution, 'succeeded')
    assert.equal(lost.mailCalls.length, 0)
    syntheticSends += 1
    check(
      'real database restart and second independent installed consumer retain unknown mail; exact positive mocked Postmark read recovers it without claim/send, while missing results do not',
    )
    await verifyNative({
      pools: () => ({ admin, app }),
      reboot: async () => {
        await admin.end()
        await app.end()
        stop()
        start()
        pools()
      },
      consumers,
      moduleAt,
      check,
    })
    await mkdir(join(root, 'dist/act-proof'), { recursive: true })
    await writeFile(
      join(root, 'dist/act-proof/results.json'),
      JSON.stringify(
        {
          node: process.version,
          checks,
          consumers: proofs,
          providerCalls: 0,
          mockedProviderCalls: { reservations: syntheticReservations, sends: syntheticSends },
          scope: 'isolated real PostgreSQL and four actual Node processes; no real booking/email',
        },
        null,
        2,
      ) + '\n',
    )
    process.stdout.write(`${checks.length}/${checks.length} durable Act database checks passed.\n`)
  } finally {
    await app?.end().catch(() => {})
    await admin?.end().catch(() => {})
    stop()
    await rm(scratch, { recursive: true, force: true })
  }
}
